//! Blocking, bounded Pi RPC child driver shared by the managed worker and
//! `ovrcr operator models`.
//!
//! Framing follows Pi's JSONL contract: records split only on LF, an optional
//! trailing CR is ignored, and every complete or incomplete record is capped at
//! 16 MiB. Reads are nonblocking and bounded so deadlines and cancellation are
//! never starved by a silent child. stderr is drained by a helper thread into a
//! bounded ring and is never persisted.
use crate::operator::pi::PiError;
use serde_json::Value;
use std::{
    io::{self, Read},
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Largest accepted JSON record, excluding its LF terminator.
pub const FRAME_LIMIT: usize = 16 * 1024 * 1024;
/// Bounded stderr retention; only an excerpt is exposed.
pub const STDERR_LIMIT: usize = 64 * 1024;
const STDERR_EXCERPT: usize = 4 * 1024;
const READ_BATCHES: usize = 16;
const READ_SIZE: usize = 16 * 1024;
const POLL: Duration = Duration::from_millis(10);
const SEND_DEADLINE: Duration = Duration::from_secs(20);
const KILL_DEADLINE: Duration = Duration::from_secs(2);

/// One decoded protocol record. Responses are matched by `id`; every other
/// record, including extension UI requests, is an unsolicited event.
#[derive(Clone, Debug, PartialEq)]
pub enum RpcMessage {
    Response(RpcResponse),
    Event(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RpcResponse {
    pub id: String,
    pub command: String,
    pub success: bool,
    pub error: Option<String>,
    /// `Value::Null` when the response carried no data.
    pub data: Value,
}

/// A spawned Pi RPC child with piped stdin/stdout and continuously drained
/// stderr.
pub struct RpcChild {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: ChildStdout,
    owned_group: bool,
    reaped: bool,
    pending: Vec<u8>,
    stderr: Arc<Mutex<Vec<u8>>>,
    #[cfg(test)]
    stderr_done: Arc<AtomicBool>,
    stderr_tail: String,
}

impl RpcChild {
    /// Spawn `cmd` with piped stdio. `owned_group` puts Pi in a fresh process
    /// group this driver may signal; the managed worker passes `false` so the
    /// lifetime Session keeps ownership of Pi and its tool children.
    pub fn spawn(cmd: &mut Command, owned_group: bool) -> Result<Self, PiError> {
        if owned_group {
            cmd.process_group(0);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                PiError::unavailable("Pi executable was not found")
            } else {
                PiError::integration(format!("could not start Pi: {error}"))
            }
        })?;
        let stdin = child.stdin.take();
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let _ = child.kill();
                let _ = child.try_wait();
                return Err(PiError::integration("Pi stdout is unavailable"));
            }
        };
        let stderr_pipe = child.stderr.take();
        if let Some(stdin) = stdin.as_ref()
            && let Err(error) = nonblocking(stdin.as_raw_fd())
        {
            let _ = child.kill();
            let _ = child.try_wait();
            return Err(PiError::integration(format!(
                "could not configure Pi stdin: {error}"
            )));
        }
        if let Err(error) = nonblocking(stdout.as_raw_fd()) {
            let _ = child.kill();
            let _ = child.try_wait();
            return Err(PiError::integration(format!(
                "could not configure Pi stdout: {error}"
            )));
        }
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stderr_done = Arc::new(AtomicBool::new(false));
        if let Some(mut pipe) = stderr_pipe {
            let ring = Arc::clone(&stderr);
            let done = Arc::clone(&stderr_done);
            if thread::Builder::new()
                .name("operator-pi-stderr".into())
                .spawn(move || drain_stderr(&mut pipe, &ring, &done))
                .is_err()
            {
                // Without a drain thread a chatty child can block on stderr.
                let _ = child.kill();
                let _ = child.try_wait();
                return Err(PiError::integration("could not drain Pi diagnostics"));
            }
        } else {
            stderr_done.store(true, Ordering::Release);
        }
        Ok(Self {
            child,
            stdin,
            stdout,
            owned_group,
            reaped: false,
            pending: Vec::new(),
            stderr,
            #[cfg(test)]
            stderr_done,
            stderr_tail: String::new(),
        })
    }

    /// Write one JSON command followed by LF. Bounded so a stalled child cannot
    /// wedge the worker.
    pub fn send(&mut self, msg: &Value) -> Result<(), PiError> {
        let stdin = self
            .stdin
            .as_ref()
            .ok_or_else(|| PiError::request("Pi stdin is closed"))?;
        let mut bytes =
            serde_json::to_vec(msg).map_err(|_| PiError::request("invalid Pi command"))?;
        if bytes.len() > FRAME_LIMIT {
            return Err(PiError::request("Pi command exceeds 16 MiB"));
        }
        bytes.push(b'\n');
        let fd = stdin.as_raw_fd();
        let expires = Instant::now() + SEND_DEADLINE;
        let mut written = 0;
        while written < bytes.len() {
            match write_fd(fd, &bytes[written..]) {
                Ok(0) => return Err(PiError::request("Pi stdin closed")),
                Ok(n) => written += n,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if Instant::now() >= expires {
                        return Err(PiError::request("Pi stdin stalled"));
                    }
                    thread::sleep(POLL);
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    return Err(PiError::request(format!("writing Pi stdin: {error}")));
                }
            }
        }
        Ok(())
    }

    /// Return the next complete record, or a bounded failure. `deadline` is
    /// measured from this call; a deadline overrun is `PiError::timed_out`.
    pub fn next_message(&mut self, deadline: Option<Duration>) -> Result<RpcMessage, PiError> {
        let expires = deadline.map(|limit| Instant::now() + limit);
        let mut buffer = [0u8; READ_SIZE];
        loop {
            if let Some(message) = self.take_frame()? {
                self.refresh_stderr();
                return Ok(message);
            }
            if self.pending.len() > FRAME_LIMIT {
                self.refresh_stderr();
                return Err(PiError::request("Pi record exceeds 16 MiB"));
            }
            if expires.is_some_and(|expires| Instant::now() >= expires) {
                self.refresh_stderr();
                return Err(PiError::timeout("Pi did not answer in time"));
            }
            let mut eof = false;
            let mut read_any = false;
            // Bounded batch: a chatty child must not starve the deadline check.
            for _ in 0..READ_BATCHES {
                match read_fd(self.stdout.as_raw_fd(), &mut buffer) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(n) => {
                        self.pending.extend_from_slice(&buffer[..n]);
                        read_any = true;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        self.refresh_stderr();
                        return Err(PiError::request(format!("reading Pi output: {error}")));
                    }
                }
            }
            if read_any {
                continue;
            }
            if eof {
                self.refresh_stderr();
                return Err(PiError::request("Pi closed its output before a reply"));
            }
            thread::sleep(POLL);
        }
    }

    /// Bounded, redacted diagnostics excerpt. Never contains the full log.
    pub fn stderr_excerpt(&self) -> &str {
        &self.stderr_tail
    }

    pub fn try_exit(&mut self) -> Option<ExitStatus> {
        let status = self.child.try_wait().ok().flatten();
        if status.is_some() {
            self.reaped = true;
            self.refresh_stderr();
        }
        status
    }

    /// Close stdin and, for an owned group, return a reaped leader only after
    /// proving group absence. `None` leaves cleanup unproven. Non-owned children
    /// are only disconnected; their exit status never proves Session cleanup.
    pub fn terminate(&mut self, grace: Duration) -> Option<ExitStatus> {
        self.terminate_with(grace, KILL_DEADLINE, signal_group)
    }

    fn terminate_with(
        &mut self,
        grace: Duration,
        kill_deadline: Duration,
        mut signal: impl FnMut(i32, i32),
    ) -> Option<ExitStatus> {
        self.close_stdin();
        if !self.owned_group {
            return self.try_exit();
        }
        let group = self.child.id() as i32;
        if !self.reaped {
            // WNOWAIT keeps the leader's identity reserved through the final
            // group signal, even when it exits before a resistant descendant.
            let mut exited = unreaped_exit(group).ok()?;
            signal(group, libc::SIGTERM);
            let expires = Instant::now() + grace;
            while !exited {
                let remaining = expires.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                thread::sleep(POLL.min(remaining));
                exited = unreaped_exit(group).ok()?;
            }
            signal(group, libc::SIGKILL);
        }

        // No more signals after the first successful try_wait: the PGID may
        // subsequently be reused. Only read-only absence probes are safe then.
        let expires = Instant::now() + kill_deadline;
        let mut status = None;
        loop {
            if status.is_none() {
                status = self.try_exit();
            }
            if status.is_some() && group_absent(group) {
                return status;
            }
            let remaining = expires.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.refresh_stderr();
                return None;
            }
            thread::sleep(POLL.min(remaining));
        }
    }

    pub fn close_stdin(&mut self) {
        self.stdin = None;
    }

    fn take_frame(&mut self) -> Result<Option<RpcMessage>, PiError> {
        loop {
            let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') else {
                return Ok(None);
            };
            if end > FRAME_LIMIT {
                return Err(PiError::request("Pi record exceeds 16 MiB"));
            }
            let mut line: Vec<u8> = self.pending.drain(..=end).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let value: Value = serde_json::from_slice(&line)
                .map_err(|error| PiError::request(format!("invalid Pi JSON record: {error}")))?;
            let object = value
                .as_object()
                .ok_or_else(|| PiError::request("Pi record is not a JSON object"))?;
            let kind = object
                .get("type")
                .and_then(Value::as_str)
                .ok_or_else(|| PiError::request("Pi record has no type"))?;
            if kind == "response" {
                return Ok(Some(RpcMessage::Response(RpcResponse {
                    id: object
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    command: object
                        .get("command")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    success: object
                        .get("success")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    error: object
                        .get("error")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    data: object.get("data").cloned().unwrap_or(Value::Null),
                })));
            }
            return Ok(Some(RpcMessage::Event(value)));
        }
    }

    fn refresh_stderr(&mut self) {
        let bytes = lock(&self.stderr);
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        if text.len() > STDERR_EXCERPT {
            // The most recent diagnostics are the useful ones.
            let mut start = text.len() - STDERR_EXCERPT;
            while start < text.len() && !text.is_char_boundary(start) {
                start += 1;
            }
            text = text[start..].to_owned();
        }
        self.stderr_tail = text;
    }

    #[cfg(test)]
    fn stderr_retained(&self) -> usize {
        lock(&self.stderr).len()
    }

    #[cfg(test)]
    fn stderr_finished(&self) -> bool {
        self.stderr_done.load(Ordering::Acquire)
    }
}

fn drain_stderr(pipe: &mut impl Read, ring: &Mutex<Vec<u8>>, done: &AtomicBool) {
    let mut chunk = [0u8; 8 * 1024];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let mut bytes = lock(ring);
                bytes.extend_from_slice(&chunk[..n]);
                if bytes.len() > STDERR_LIMIT {
                    let excess = bytes.len() - STDERR_LIMIT;
                    bytes.drain(..excess);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    done.store(true, Ordering::Release);
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn nonblocking(fd: i32) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn read_fd(fd: i32, buffer: &mut [u8]) -> io::Result<usize> {
    let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
    if count < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(count as usize)
    }
}

fn write_fd(fd: i32, bytes: &[u8]) -> io::Result<usize> {
    let count = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
    if count < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(count as usize)
    }
}

fn signal_group(group: i32, signal: i32) {
    if group > 0 {
        unsafe {
            libc::kill(-group, signal);
        }
    }
}

fn unreaped_exit(pid: i32) -> io::Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as u32,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { info.si_pid() } == pid)
}

fn group_absent(group: i32) -> bool {
    (unsafe { libc::kill(-group, 0) }) == -1
        && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script);
        command
    }

    fn event(message: RpcMessage) -> Value {
        match message {
            RpcMessage::Event(value) => value,
            RpcMessage::Response(response) => panic!("expected event, got {response:?}"),
        }
    }

    fn response(message: RpcMessage) -> RpcResponse {
        match message {
            RpcMessage::Response(response) => response,
            RpcMessage::Event(value) => panic!("expected response, got {value}"),
        }
    }

    #[test]
    fn splits_records_on_lf_only() {
        let mut child = RpcChild::spawn(
            &mut shell("printf '{\"type\":\"a\"}\n{\"type\":\"b\"}\n'"),
            true,
        )
        .unwrap();
        assert_eq!(
            event(child.next_message(Some(Duration::from_secs(5))).unwrap())["type"],
            "a"
        );
        assert_eq!(
            event(child.next_message(Some(Duration::from_secs(5))).unwrap())["type"],
            "b"
        );
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn tolerates_trailing_carriage_return() {
        let mut child = RpcChild::spawn(&mut shell("printf '{\"type\":\"a\"}\r\n'"), true).unwrap();
        assert_eq!(
            event(child.next_message(Some(Duration::from_secs(5))).unwrap())["type"],
            "a"
        );
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn unicode_line_separators_are_not_frame_boundaries() {
        let mut child = RpcChild::spawn(
            &mut shell("printf '{\"type\":\"a\",\"text\":\"x\\342\\200\\250y\"}\n'"),
            true,
        )
        .unwrap();
        let value = event(child.next_message(Some(Duration::from_secs(5))).unwrap());
        assert_eq!(value["text"], "x\u{2028}y");
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn incomplete_trailing_frame_is_an_eof_error() {
        let mut child = RpcChild::spawn(&mut shell("printf '{\"type\":\"a\"}'"), true).unwrap();
        let error = child
            .next_message(Some(Duration::from_secs(5)))
            .unwrap_err();
        assert_eq!(error.kind(), crate::operator::pi::PiErrorKind::Request);
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn premature_eof_is_an_error() {
        let mut child = RpcChild::spawn(&mut shell("exit 0"), true).unwrap();
        let error = child
            .next_message(Some(Duration::from_secs(5)))
            .unwrap_err();
        assert_eq!(error.kind(), crate::operator::pi::PiErrorKind::Request);
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn malformed_json_is_an_error() {
        let mut child = RpcChild::spawn(&mut shell("printf 'not json\n'"), true).unwrap();
        let error = child
            .next_message(Some(Duration::from_secs(5)))
            .unwrap_err();
        assert_eq!(error.kind(), crate::operator::pi::PiErrorKind::Request);
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn exact_frame_limit_is_accepted_and_one_more_byte_is_refused() {
        let prefix = "{\"type\":\"e\",\"pad\":\"";
        let padding = FRAME_LIMIT - prefix.len() - 2;
        let script = format!(
            "printf '{prefix}'; head -c {padding} /dev/zero | tr '\\0' a; printf '\"}}\\n'"
        );
        let mut child = RpcChild::spawn(&mut shell(&script), true).unwrap();
        let message = child.next_message(Some(Duration::from_secs(30))).unwrap();
        let value = event(message);
        assert_eq!(value["type"], "e");
        child.terminate(Duration::from_millis(100));

        let oversize = format!("head -c {} /dev/zero | tr '\\0' a", FRAME_LIMIT + 1);
        let mut child = RpcChild::spawn(&mut shell(&oversize), true).unwrap();
        let error = child
            .next_message(Some(Duration::from_secs(30)))
            .unwrap_err();
        assert_eq!(error.kind(), crate::operator::pi::PiErrorKind::Request);
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn split_read_batches_reassemble_one_record() {
        // 200 KiB exceeds one read batch (16 x 16 KiB).
        let script = "printf '{\"type\":\"big\",\"pad\":\"'; \
            head -c 200000 /dev/zero | tr '\\0' b; printf '\"}\\n'";
        let mut child = RpcChild::spawn(&mut shell(script), true).unwrap();
        let value = event(child.next_message(Some(Duration::from_secs(10))).unwrap());
        assert_eq!(value["type"], "big");
        assert_eq!(value["pad"].as_str().unwrap().len(), 200000);
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn interleaved_event_precedes_the_awaited_response() {
        let script = "printf '{\"type\":\"message_update\",\"delta\":\"x\"}\n\
            {\"id\":\"r1\",\"type\":\"response\",\"command\":\"get_state\",\"success\":true,\"data\":{\"n\":1}}\n'";
        let mut child = RpcChild::spawn(&mut shell(script), true).unwrap();
        assert_eq!(
            event(child.next_message(Some(Duration::from_secs(5))).unwrap())["type"],
            "message_update"
        );
        let reply = response(child.next_message(Some(Duration::from_secs(5))).unwrap());
        assert_eq!(reply.id, "r1");
        assert_eq!(reply.command, "get_state");
        assert!(reply.success);
        assert!(reply.error.is_none());
        assert_eq!(reply.data["n"], 1);
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn deadline_is_reported_as_timed_out() {
        let mut child = RpcChild::spawn(&mut shell("sleep 30"), true).unwrap();
        let started = Instant::now();
        let error = child
            .next_message(Some(Duration::from_millis(120)))
            .unwrap_err();
        assert!(error.timed_out_error());
        assert!(started.elapsed() < Duration::from_secs(5));
        let status = child.terminate(Duration::from_millis(200));
        assert!(status.is_some(), "owned group must be reaped");
    }

    #[test]
    fn stderr_ring_stays_bounded_and_keeps_the_tail() {
        let script = "head -c 131072 /dev/zero | tr '\\0' e >&2; \
            printf 'TAIL-MARK\\n' >&2; printf '{\"type\":\"done\"}\\n'";
        let mut child = RpcChild::spawn(&mut shell(script), true).unwrap();
        event(child.next_message(Some(Duration::from_secs(10))).unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !child.stderr_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(child.stderr_finished(), "stderr must be drained to EOF");
        assert!(child.stderr_retained() <= STDERR_LIMIT);
        assert_eq!(child.stderr_retained(), STDERR_LIMIT);
        child.refresh_stderr();
        assert!(child.stderr_excerpt().contains("TAIL-MARK"));
        assert!(child.stderr_excerpt().len() <= STDERR_EXCERPT);
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn terminate_owned_group_reaps_and_releases_the_group() {
        let mut child = RpcChild::spawn(&mut shell("sleep 30"), true).unwrap();
        let group = child.child.id() as i32;
        let started = Instant::now();
        let status = child.terminate(Duration::from_millis(300));
        assert!(status.is_some());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(child.try_exit().is_some());
        let remaining = unsafe { libc::kill(-group, 0) };
        assert_eq!(remaining, -1, "owned group must be gone");
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }

    #[test]
    fn leader_exit_is_not_proof_a_term_resistant_group_member_stopped() {
        let root = tempfile::tempdir().unwrap();
        let ready = root.path().join("member-ready");
        let release = root.path().join("member-release");
        let mut command = shell(
            "/bin/sh -c 'trap \"\" TERM; printf ready > \"$READY\"; \
                while [ ! -e \"$RELEASE\" ]; do /bin/sleep 0.01; done' & \
             while [ ! -e \"$READY\" ]; do /bin/sleep 0.01; done; \
             printf '{\"type\":\"ready\"}\\n'; exit 0",
        );
        command.env("READY", &ready).env("RELEASE", &release);
        let mut child = RpcChild::spawn(&mut command, true).unwrap();
        event(child.next_message(Some(Duration::from_secs(5))).unwrap());
        let group = child.child.id() as i32;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !unreaped_exit(group).unwrap() && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        let leader_exited = unreaped_exit(group).unwrap();
        let member_ready = ready.exists();
        let group_was_present = !group_absent(group);

        let status = child.terminate(Duration::from_millis(50));
        let absent_at_return = group_absent(group);
        // Release the member even on the pre-fix failure without signalling
        // a group whose leader terminate may already have reaped.
        std::fs::write(&release, b"release").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !group_absent(group) && Instant::now() < deadline {
            thread::sleep(POLL);
        }
        assert!(group_absent(group), "fixture group must be gone");
        assert!(leader_exited && member_ready && group_was_present);
        assert!(
            status.is_none() || absent_at_return,
            "leader death was incorrectly returned as group cleanup proof"
        );
    }

    #[test]
    fn termination_deadline_does_not_fall_through_to_a_blocking_wait() {
        let mut child = RpcChild::spawn(
            &mut shell("printf '{\"type\":\"ready\"}\\n'; exec /bin/sleep 30"),
            true,
        )
        .unwrap();
        event(child.next_message(Some(Duration::from_secs(5))).unwrap());
        let pid = child.child.id() as i32;
        let (release, stopped) = std::sync::mpsc::channel();
        let watchdog = thread::spawn(move || {
            if matches!(
                stopped.recv_timeout(Duration::from_millis(750)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                // The exact child remains owned and unreaped while the
                // termination call is stuck. Bound the pre-fix failure too.
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        });
        let started = Instant::now();
        // Model a kernel which has not delivered either signal by the
        // deadline, using the same real-child polling path.
        let status = child.terminate_with(Duration::ZERO, Duration::from_millis(50), |_, _| {});
        let elapsed = started.elapsed();
        let _ = release.send(());
        watchdog.join().unwrap();
        let cleanup = child.terminate(Duration::ZERO);
        assert!(cleanup.is_some(), "fixture child must be reaped");
        assert!(elapsed < Duration::from_millis(300), "{elapsed:?}");
        assert!(status.is_none(), "a live child is not cleanup proof");
    }

    #[test]
    fn send_round_trips_a_command() {
        let script = "IFS= read -r line; printf '%s\\n' \"$line\"";
        let mut child = RpcChild::spawn(&mut shell(script), true).unwrap();
        child.send(&json!({"id":"1","type":"get_state"})).unwrap();
        let value = event(child.next_message(Some(Duration::from_secs(5))).unwrap());
        assert_eq!(value["id"], "1");
        assert_eq!(value["type"], "get_state");
        child.close_stdin();
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn non_owned_child_is_not_signalled() {
        let mut child = RpcChild::spawn(&mut shell("sleep 30"), false).unwrap();
        assert!(child.terminate(Duration::from_millis(50)).is_none());
        // The test still owns the shell; clean it up directly.
        let _ = child.child.kill();
        let _ = child.child.wait();
    }
}

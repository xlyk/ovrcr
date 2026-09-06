use anyhow::{Context, Result, bail};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, mpsc::SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

#[derive(Clone, Debug)]
pub struct SessionSpec {
    pub project: String,
    pub workspace: String,
    pub name: String,
    pub label: String,
    pub cwd: PathBuf,
    pub argv: Vec<OsString>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionPhase {
    Running,
    Paused,
    Exited {
        code: Option<u32>,
        signal: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub project: String,
    pub workspace: String,
    pub name: String,
    pub label: String,
    pub pid: Option<u32>,
    pub started_unix_ms: u64,
    pub phase: SessionPhase,
}

#[derive(Clone, Debug)]
pub enum SessionEvent {
    Output { id: SessionId, bytes: Vec<u8> },
    Exited { id: SessionId, phase: SessionPhase },
}

struct SessionState {
    phase: SessionPhase,
    pid: Option<u32>,
}

struct JoinHandles {
    reader: Option<JoinHandle<()>>,
    waiter: Option<JoinHandle<()>>,
}

pub struct Session {
    summary: SessionSummary,
    state: Mutex<SessionState>,
    state_changed: Condvar,
    parser: Mutex<vt100::Parser>,
    parser_revision: Mutex<u64>,
    parser_changed: Condvar,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    pgid: libc::pid_t,
    terminate_lock: Mutex<()>,
    reader_done: Mutex<bool>,
    reader_changed: Condvar,
    handles: Mutex<JoinHandles>,
}

impl Session {
    pub fn spawn(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: SyncSender<SessionEvent>,
    ) -> Result<Arc<Self>> {
        Self::spawn_internal(id, spec, size, events, None)
    }

    pub(crate) fn spawn_with_ready(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: SyncSender<SessionEvent>,
        ready: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Arc<Self>> {
        Self::spawn_internal(id, spec, size, events, Some(ready))
    }

    fn spawn_internal(
        id: SessionId,
        spec: SessionSpec,
        size: TerminalSize,
        events: SyncSender<SessionEvent>,
        ready: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Arc<Self>> {
        let argv0 = spec
            .argv
            .first()
            .context("session command cannot be empty")?
            .clone();
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(argv0);
        command.args(spec.argv.iter().skip(1));
        command.cwd(spec.cwd);
        let mut child = pair.slave.spawn_command(command)?;
        let pid = child.process_id().context("PTY child has no process ID")?;

        #[cfg(unix)]
        let pgid = pair
            .master
            .process_group_leader()
            .context("PTY did not provide a process-group leader")?;
        #[cfg(not(unix))]
        let pgid = {
            let _ = pid;
            bail!("OVRCR sessions require Unix process groups")
        };

        if pgid != pid as libc::pid_t {
            let _ = child.kill();
            bail!("PTY process-group leader does not own the child process");
        }
        verify_group_identity(pgid, false)?;

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let started_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let session = Arc::new(Self {
            summary: SessionSummary {
                id,
                project: spec.project,
                workspace: spec.workspace,
                name: spec.name,
                label: spec.label,
                pid: Some(pid),
                started_unix_ms,
                phase: SessionPhase::Running,
            },
            state: Mutex::new(SessionState {
                phase: SessionPhase::Running,
                pid: Some(pid),
            }),
            state_changed: Condvar::new(),
            parser: Mutex::new(vt100::Parser::new(size.rows, size.cols, 0)),
            parser_revision: Mutex::new(0),
            parser_changed: Condvar::new(),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            pgid,
            terminate_lock: Mutex::new(()),
            reader_done: Mutex::new(false),
            reader_changed: Condvar::new(),
            handles: Mutex::new(JoinHandles {
                reader: None,
                waiter: None,
            }),
        });

        let reader_session = Arc::clone(&session);
        let reader_events = events.clone();
        let reader_handle = thread::Builder::new()
            .name(format!("ovrcr-session-reader-{id:?}"))
            .spawn(move || {
                read_pty(reader, reader_session, reader_events);
            })?;

        let waiter_session = Arc::clone(&session);
        let waiter_events = events;
        let waiter_handle = thread::Builder::new()
            .name(format!("ovrcr-session-waiter-{id:?}"))
            .spawn(move || {
                wait_for_child(&mut *child, waiter_session, waiter_events);
            })?;
        {
            let mut handles = session.handles.lock().unwrap();
            handles.reader = Some(reader_handle);
            handles.waiter = Some(waiter_handle);
        }
        if let Some(ready) = ready {
            ready();
        }
        Ok(session)
    }

    pub fn summary(&self) -> SessionSummary {
        let state = self.state.lock().unwrap();
        let mut summary = self.summary.clone();
        summary.phase = state.phase.clone();
        summary.pid = state.pid;
        summary
    }

    pub fn set_paused(&self, paused: bool) -> Result<bool> {
        let _control = self.terminate_lock.lock().unwrap();
        let mut state = self.state.lock().unwrap();
        if matches!(state.phase, SessionPhase::Exited { .. }) {
            bail!("session has exited");
        }
        verify_owned_group(self)?;
        let signal = if paused { libc::SIGSTOP } else { libc::SIGCONT };
        if !signal_group(self.pgid, signal)? {
            bail!("PTY process group no longer exists");
        }
        let next = if paused {
            SessionPhase::Paused
        } else {
            SessionPhase::Running
        };
        let changed = state.phase != next;
        state.phase = next;
        self.state_changed.notify_all();
        Ok(changed)
    }

    fn admit_input(&self) -> Result<()> {
        let phase = self.state.lock().unwrap().phase.clone();
        match phase {
            SessionPhase::Running => Ok(()),
            SessionPhase::Paused => {
                bail!("session is paused; resume it before sending input")
            }
            SessionPhase::Exited { .. } => bail!("session has exited"),
        }
    }

    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        self.admit_input()?;
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes).context("write to PTY")?;
        writer.flush().context("flush PTY")?;
        Ok(())
    }

    pub fn resize(&self, size: TerminalSize) -> Result<()> {
        self.master
            .lock()
            .unwrap()
            .resize(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("resize PTY")?;
        self.parser
            .lock()
            .unwrap()
            .screen_mut()
            .set_size(size.rows, size.cols);
        Ok(())
    }

    pub fn apply_event(&self, event: SessionEvent) {
        match event {
            SessionEvent::Output { id, bytes } if id == self.summary.id => {
                self.parser.lock().unwrap().process(&bytes);
                let mut revision = self.parser_revision.lock().unwrap();
                *revision = revision.saturating_add(1);
                self.parser_changed.notify_all();
            }
            SessionEvent::Exited { id, phase } if id == self.summary.id => {
                let mut state = self.state.lock().unwrap();
                state.phase = phase;
                state.pid = None;
                self.state_changed.notify_all();
            }
            _ => {}
        }
    }

    pub fn current_screen(&self) -> Vec<u8> {
        self.parser.lock().unwrap().screen().state_formatted()
    }

    pub fn terminal_text(&self) -> (TerminalSize, String) {
        let parser = self.parser.lock().unwrap();
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        (TerminalSize { rows, cols }, screen.contents())
    }

    pub fn send_text(&self, text: &str, submit: bool) -> Result<()> {
        self.admit_input()?;
        let bytes = {
            let parser = self.parser.lock().unwrap();
            crate::tui::encode_paste(text, parser.screen().bracketed_paste())
        };
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(&bytes).context("write text to PTY")?;
        if submit {
            writer.write_all(b"\r").context("submit text to PTY")?;
        }
        writer.flush().context("flush PTY")?;
        Ok(())
    }

    pub fn wait_for_output(&self, timeout: Duration) {
        let revision = self.parser_revision.lock().unwrap();
        if *revision == 0 {
            let _ = self.parser_changed.wait_timeout(revision, timeout);
        }
    }

    pub fn terminate(&self, grace: Duration) -> Result<()> {
        let _termination = self.terminate_lock.lock().unwrap();
        if matches!(
            self.state.lock().unwrap().phase,
            SessionPhase::Exited { .. }
        ) {
            self.join_threads()?;
            return Ok(());
        }

        if should_signal_group(self)? {
            signal_group(self.pgid, libc::SIGTERM)?;
        }
        let deadline = Instant::now() + grace;
        if !wait_for_group_exit(self.pgid, deadline)? && group_exists(self.pgid)? {
            if should_signal_group(self)? {
                signal_group(self.pgid, libc::SIGKILL)?;
            }
            let kill_deadline = Instant::now() + grace.max(Duration::from_secs(2));
            if !wait_for_group_exit(self.pgid, kill_deadline)? {
                bail!("PTY process group did not exit after SIGKILL")
            }
        }

        let phase = self.wait_until_exited(grace.max(Duration::from_secs(2)))?;
        if !matches!(phase, SessionPhase::Exited { .. }) {
            bail!("session did not publish its exit event")
        }
        self.join_threads()?;
        if group_exists(self.pgid)? {
            bail!("PTY process group still exists after termination")
        }
        Ok(())
    }

    pub fn wait_until_exited(&self, timeout: Duration) -> Result<SessionPhase> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap();
        loop {
            if let SessionPhase::Exited { .. } = state.phase {
                return Ok(state.phase.clone());
            }
            let now = Instant::now();
            if now >= deadline {
                bail!("timed out waiting for session exit")
            }
            let (next, result) = self
                .state_changed
                .wait_timeout(state, deadline.saturating_duration_since(now))
                .unwrap();
            state = next;
            if result.timed_out() {
                bail!("timed out waiting for session exit")
            }
        }
    }

    fn join_threads(&self) -> Result<()> {
        let (reader, waiter) = {
            let mut handles = self.handles.lock().unwrap();
            (handles.reader.take(), handles.waiter.take())
        };
        if let Some(handle) = reader {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("PTY reader panicked"))?;
        }
        if let Some(handle) = waiter {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("child waiter panicked"))?;
        }
        Ok(())
    }
}

fn read_pty(
    mut reader: Box<dyn Read + Send>,
    session: Arc<Session>,
    events: SyncSender<SessionEvent>,
) {
    let mut buffer = [0_u8; 8192];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        if read == 0 {
            break;
        }
        if events
            .send(SessionEvent::Output {
                id: session.summary.id,
                bytes: buffer[..read].to_vec(),
            })
            .is_err()
        {
            break;
        }
    }
    let mut done = session.reader_done.lock().unwrap();
    *done = true;
    session.reader_changed.notify_all();
}

fn wait_for_child(
    child: &mut (dyn portable_pty::Child + Send + Sync),
    session: Arc<Session>,
    events: SyncSender<SessionEvent>,
) {
    let phase = match child.wait() {
        Ok(status) => SessionPhase::Exited {
            code: Some(status.exit_code()),
            signal: status.signal().map(str::to_owned),
        },
        Err(error) => SessionPhase::Exited {
            code: None,
            signal: Some(error.to_string()),
        },
    };
    let mut reader_done = session.reader_done.lock().unwrap();
    while !*reader_done {
        reader_done = session.reader_changed.wait(reader_done).unwrap();
    }
    drop(reader_done);
    while group_exists(session.pgid).unwrap_or(true) {
        thread::park_timeout(Duration::from_millis(5));
    }
    let _ = events.send(SessionEvent::Exited {
        id: session.summary.id,
        phase,
    });
}

#[cfg(unix)]
fn verify_group_identity(pgid: libc::pid_t, allow_reaped_leader: bool) -> Result<()> {
    if pgid <= 1 || pgid == unsafe { libc::getpgrp() } {
        bail!("refusing unsafe process group")
    }
    let actual = unsafe { libc::getpgid(pgid) };
    if actual == pgid {
        return Ok(());
    }
    if allow_reaped_leader
        && actual == -1
        && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        && group_exists(pgid)?
    {
        return Ok(());
    }
    bail!("PTY process group is not owned")
}

#[cfg(not(unix))]
fn verify_group_identity(_: libc::pid_t, _: bool) -> Result<()> {
    bail!("OVRCR sessions require Unix process groups")
}

fn verify_owned_group(session: &Session) -> Result<()> {
    #[cfg(unix)]
    {
        if !group_exists(session.pgid)? {
            bail!("PTY process group no longer exists")
        }
        verify_group_identity(session.pgid, true)
    }
    #[cfg(not(unix))]
    {
        let _ = session;
        bail!("OVRCR sessions require Unix process groups")
    }
}

fn should_signal_group(session: &Session) -> Result<bool> {
    if !group_exists(session.pgid)? {
        return Ok(false);
    }
    match verify_owned_group(session) {
        Ok(()) => Ok(true),
        Err(_error) if !group_exists(session.pgid)? => Ok(false),
        Err(error) => Err(error),
    }
}

fn signal_group(pgid: libc::pid_t, signal: libc::c_int) -> Result<bool> {
    if pgid <= 1 || signal <= 0 {
        bail!("refusing unsafe process-group signal")
    }
    let result = unsafe { libc::kill(-pgid, signal) };
    if result == -1 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(false);
        }
        return Err(error).context("signal PTY process group");
    }
    Ok(true)
}

fn group_exists(pgid: libc::pid_t) -> Result<bool> {
    if pgid <= 1 {
        bail!("invalid process group")
    }
    let result = unsafe { libc::kill(-pgid, 0) };
    if result == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Ok(false),
        Some(libc::EPERM) => Ok(true),
        _ => Err(error).context("check PTY process group"),
    }
}

fn wait_for_group_exit(pgid: libc::pid_t, deadline: Instant) -> Result<bool> {
    loop {
        if !group_exists(pgid)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        thread::park_timeout(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::sync::mpsc::{self, Receiver};

    struct TerminationGuard(Arc<Session>);

    impl Drop for TerminationGuard {
        fn drop(&mut self) {
            let _ = self.0.terminate(Duration::from_millis(200));
        }
    }

    fn spawn_test_shell() -> Arc<Session> {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(1),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "local".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![OsString::from("sh")],
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let _ = Box::leak(Box::new(dir));
        let _dispatcher = dispatch_test_events(session.clone(), rx);
        session
    }

    fn dispatch_test_events(session: Arc<Session>, rx: Receiver<SessionEvent>) -> JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                let exited = matches!(event, SessionEvent::Exited { .. });
                session.apply_event(event);
                if exited {
                    break;
                }
            }
        })
    }

    fn wait_for_screen(session: &Session, marker: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if String::from_utf8_lossy(&session.current_screen()).contains(marker) {
                return true;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn pause_resume_phase_and_input_admission() {
        let session = spawn_test_shell();
        let _cleanup = TerminationGuard(Arc::clone(&session));
        session.write(b"printf 'READY\\n'\r").unwrap();
        assert!(wait_for_screen(&session, "READY", Duration::from_secs(2)));

        assert!(session.set_paused(true).unwrap());
        assert_eq!(session.summary().phase, SessionPhase::Paused);
        assert!(!session.set_paused(true).unwrap());
        assert!(session.write(b"SHOULD_NOT_BE_ACCEPTED\r").is_err());
        assert!(session.send_text("SHOULD_NOT_BE_ACCEPTED", true).is_err());
        assert!(session.set_paused(false).unwrap());
        assert!(!session.set_paused(false).unwrap());
        session.write(b"printf 'RESUME_%s\\n' ACK\r").unwrap();
        assert!(wait_for_screen(
            &session,
            "RESUME_ACK",
            Duration::from_secs(2)
        ));
        session.terminate(Duration::from_millis(200)).unwrap();
        assert!(session.set_paused(true).is_err());
        assert!(session.set_paused(false).is_err());
    }

    #[test]
    fn pause_resume_exit_event_cannot_be_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(4),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "exit-race".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec!["sh".into(), "-c".into(), "printf EXIT_READY".into()],
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let _cleanup = TerminationGuard(Arc::clone(&session));
        let gate = Arc::new(Barrier::new(2));
        let dispatcher =
            dispatch_test_events_with_exit_gate(Arc::clone(&session), rx, Arc::clone(&gate));
        let control_gate = Arc::clone(&gate);
        let control_session = Arc::clone(&session);
        let control = thread::spawn(move || {
            control_gate.wait();
            control_session.set_paused(true)
        });

        assert!(wait_for_screen(
            &session,
            "EXIT_READY",
            Duration::from_secs(2)
        ));
        assert!(matches!(control.join().unwrap(), Err(_)));
        dispatcher.join().unwrap();
        assert!(matches!(
            session.summary().phase,
            SessionPhase::Exited { .. }
        ));
    }

    fn dispatch_test_events_with_exit_gate(
        session: Arc<Session>,
        rx: Receiver<SessionEvent>,
        gate: Arc<Barrier>,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                let exited = matches!(event, SessionEvent::Exited { .. });
                if exited {
                    gate.wait();
                }
                session.apply_event(event);
                if exited {
                    break;
                }
            }
        })
    }

    #[test]
    fn pause_resume_rejects_unsafe_group() {
        assert!(verify_group_identity(0, false).is_err());
        assert!(verify_group_identity(1, false).is_err());
        assert!(verify_group_identity(unsafe { libc::getpgrp() }, false).is_err());
    }

    #[test]
    fn shell_round_trip_updates_the_current_screen() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(1),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "local".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![OsString::from("sh")],
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let dispatcher = dispatch_test_events(session.clone(), rx);
        session.write(b"printf 'OVRCR_%s' READY\r").unwrap();
        assert!(wait_for_screen(
            &session,
            "OVRCR_READY",
            Duration::from_secs(2)
        ));
        session.terminate(Duration::from_millis(200)).unwrap();
        dispatcher.join().unwrap();
    }

    #[test]
    fn final_output_is_parsed_before_session_becomes_removable() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(2),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "final".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec!["sh".into(), "-c".into(), "printf FINAL_MARKER".into()],
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let first_event = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(session.summary().phase, SessionPhase::Running));
        let mut next_event = Some(first_event);
        let mut saw_output = false;
        loop {
            let event = match next_event.take() {
                Some(event) => event,
                None => rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            };
            let exited = matches!(event, SessionEvent::Exited { .. });
            if !exited {
                saw_output = saw_output || matches!(event, SessionEvent::Output { .. });
            }
            session.apply_event(event);
            if exited {
                break;
            }
        }
        assert!(saw_output);
        assert!(String::from_utf8_lossy(&session.current_screen()).contains("FINAL_MARKER"));
        assert!(matches!(
            session.summary().phase,
            SessionPhase::Exited { .. }
        ));
    }

    #[test]
    fn resize_reaches_the_child_tty() {
        let session = spawn_test_shell();
        session
            .resize(TerminalSize {
                rows: 37,
                cols: 111,
            })
            .unwrap();
        session.write(b"stty size\r").unwrap();
        assert!(wait_for_screen(&session, "37 111", Duration::from_secs(2)));
        session.terminate(Duration::from_millis(200)).unwrap();
    }

    #[test]
    fn terminate_removes_the_whole_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::sync_channel(64);
        let session = Session::spawn(
            SessionId(3),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "group".into(),
                label: "sh".into(),
                cwd: dir.path().to_path_buf(),
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "trap '' HUP; sleep 30 & printf 'OVRCR_DESC:%s\\n' \"$!\"; exit".into(),
                ],
            },
            TerminalSize { rows: 24, cols: 80 },
            tx,
        )
        .unwrap();
        let dispatcher = dispatch_test_events(session.clone(), rx);
        let leader = session.summary().pid.unwrap();
        let screen_deadline = Instant::now() + Duration::from_secs(2);
        let descendant = loop {
            if let Some(pid) = extract_tagged_pid(&session.current_screen(), b"OVRCR_DESC:")
                && pid_exists(pid)
            {
                break pid;
            }
            assert!(
                Instant::now() < screen_deadline,
                "descendant PID was not observed: {}",
                String::from_utf8_lossy(&session.current_screen())
            );
            thread::park_timeout(Duration::from_millis(5));
        };
        assert!(pid_exists(descendant));
        let termination = session.terminate(Duration::from_millis(200));
        if termination.is_err() {
            let owned_group = unsafe { libc::getpgid(descendant as libc::pid_t) } == session.pgid;
            assert!(owned_group, "negative-control cleanup lost PTY ownership");
            signal_group(session.pgid, libc::SIGKILL).unwrap();
            assert!(
                wait_for_group_exit(session.pgid, Instant::now() + Duration::from_secs(2)).unwrap()
            );
            let _ = session.wait_until_exited(Duration::from_secs(2));
        }
        termination.unwrap();
        dispatcher.join().unwrap();
        assert_pid_is_gone(leader);
        assert_pid_is_gone(descendant);
        assert!(!group_exists(session.pgid).unwrap());
    }

    fn extract_tagged_pid(screen: &[u8], tag: &[u8]) -> Option<u32> {
        let start = screen.windows(tag.len()).position(|window| window == tag)? + tag.len();
        let digits = screen[start..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .copied()
            .collect::<Vec<_>>();
        if digits.is_empty() {
            return None;
        }
        std::str::from_utf8(&digits).ok()?.parse().ok()
    }

    fn pid_exists(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    fn assert_pid_is_gone(pid: u32) {
        assert_eq!(unsafe { libc::kill(pid as libc::pid_t, 0) }, -1);
        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    }
}

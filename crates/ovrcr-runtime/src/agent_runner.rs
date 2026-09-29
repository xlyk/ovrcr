//! Native invocation mechanics and bounded opaque reporting lifecycle.
use anyhow::{Context, Result};
use signal_hook::iterator::Signals;
use std::{
    ffi::OsString,
    io,
    os::unix::process::CommandExt,
    process::{Command, ExitStatus, Stdio},
    time::Duration,
};

pub enum HookEvent<'a> {
    Request {
        input: &'a [u8],
        native_root: bool,
        deadline: std::time::Instant,
    },
    Poll {
        deadline: std::time::Instant,
    },
    NativeCompleted {
        deadline: std::time::Instant,
    },
}
pub type HookHandler = Box<dyn FnMut(HookEvent<'_>) -> Vec<u8> + Send>;

// All group signals happen while the unreaped direct child anchors this PGID.
pub fn run_native(
    argv: &[OsString],
    channel_ready: impl FnOnce(bool, &mut Vec<OsString>) -> Option<HookHandler>,
) -> Result<ExitStatus> {
    argv.first().context("native command is required")?;
    let mut signals = Signals::new([
        libc::SIGINT,
        libc::SIGTERM,
        libc::SIGHUP,
        libc::SIGQUIT,
        libc::SIGCONT,
        libc::SIGTSTP,
    ])?;
    let terminal = Terminal::capture();
    let mut channel = InvocationChannel::new().ok();
    let mut argv = argv.to_vec();
    // The caller releases reporting ownership before an untracked native spawn.
    let handler = channel_ready(channel.is_some(), &mut argv);
    let executable = argv.first().context("native command is required")?;
    let mut command = Command::new(executable);
    command
        .args(&argv[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env_remove("OVRCR_HOOK_SOCKET")
        .env_remove("OVRCR_SESSION_ID")
        .env_remove("OVRCR_HOOK_TOKEN")
        .env_remove("OVRCR_AGENT_SOCKET")
        .env_remove("OVRCR_AGENT_TOKEN")
        .process_group(0);
    if let Some(channel) = &channel {
        command
            .env("OVRCR_AGENT_SOCKET", &channel.path)
            .env("OVRCR_AGENT_TOKEN", &channel.token);
    }
    let foreground = terminal
        .as_ref()
        .is_some_and(|tty| tty.group == unsafe { libc::getpgrp() });
    // Establish foreground ownership before exec can read the terminal.
    unsafe {
        command.pre_exec(move || {
            if foreground {
                set_foreground(libc::getpgrp())?;
            }
            Ok(())
        });
    }
    let mut child = command.spawn().context("start native agent")?;
    let group = child.id() as libc::pid_t;
    if let Some(channel) = &mut channel {
        *channel.native.lock().unwrap() = Some(group);
        channel.start(handler);
    }
    let mut completion_deadline = None;
    let result = (|| -> Result<ExitStatus> {
        loop {
            let mut continued = false;
            for signal in signals.pending() {
                if signal == libc::SIGCONT {
                    continued = true;
                    if unsafe { libc::tcgetpgrp(0) } == unsafe { libc::getpgrp() } {
                        set_foreground(group)?;
                    }
                }
                unsafe {
                    libc::kill(-group, signal);
                }
            }
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id(),
                    &mut info,
                    libc::WEXITED
                        | libc::WSTOPPED
                        | libc::WCONTINUED
                        | libc::WNOHANG
                        | libc::WNOWAIT,
                )
            };
            if rc != 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error.into());
            }
            if unsafe { info.si_pid() } != 0 {
                match info.si_code {
                    libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED => {
                        completion_deadline =
                            Some(std::time::Instant::now() + Duration::from_secs(2));
                        break;
                    }
                    libc::CLD_STOPPED => {
                        // Consume only the stop notification, leaving exits unreaped.
                        unsafe {
                            libc::waitid(
                                libc::P_PID,
                                child.id(),
                                &mut info,
                                libc::WSTOPPED | libc::WNOHANG,
                            );
                        }
                        if !continued {
                            let native_modes = terminal_modes();
                            if let Some(tty) = &terminal {
                                tty.restore();
                            }
                            unsafe {
                                libc::raise(libc::SIGSTOP);
                            }
                            if let Some(modes) = native_modes {
                                without_ttou(|| {
                                    if unsafe { libc::tcsetattr(0, libc::TCSANOW, &modes) } == 0 {
                                        Ok(())
                                    } else {
                                        Err(io::Error::last_os_error())
                                    }
                                })?;
                            }
                            if unsafe { libc::tcgetpgrp(0) } == unsafe { libc::getpgrp() } {
                                set_foreground(group)?;
                            }
                            unsafe {
                                libc::kill(-group, libc::SIGCONT);
                            }
                        }
                    }
                    libc::CLD_CONTINUED => unsafe {
                        libc::waitid(
                            libc::P_PID,
                            child.id(),
                            &mut info,
                            libc::WCONTINUED | libc::WNOHANG,
                        );
                    },
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Some(channel) = &channel {
            *channel.native.lock().unwrap() = None;
        }
        // The zombie leader prevents PGID reuse while remaining group members are killed.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
        child.wait().context("wait for native agent")
    })();
    if result.is_err() {
        if let Some(channel) = &channel {
            *channel.native.lock().unwrap() = None;
        }
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
        let _ = child.wait();
    }
    drop(terminal);
    if let Some(channel) = &mut channel {
        channel.complete(
            completion_deadline
                .unwrap_or_else(|| std::time::Instant::now() + Duration::from_secs(2)),
        );
    }
    result
}

struct Terminal {
    group: libc::pid_t,
    modes: libc::termios,
}
impl Terminal {
    fn capture() -> Option<Self> {
        let group = unsafe { libc::tcgetpgrp(0) };
        if group <= 0 {
            return None;
        }
        terminal_modes().map(|modes| Self { group, modes })
    }
    fn restore(&self) {
        let _ = without_ttou(|| {
            if unsafe { libc::tcsetpgrp(0, self.group) } != 0 {
                return Err(io::Error::last_os_error());
            }
            if unsafe { libc::tcsetattr(0, libc::TCSANOW, &self.modes) } != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        self.restore();
    }
}
fn terminal_modes() -> Option<libc::termios> {
    let mut modes = unsafe { std::mem::zeroed() };
    (unsafe { libc::tcgetattr(0, &mut modes) } == 0).then_some(modes)
}
fn set_foreground(group: libc::pid_t) -> io::Result<()> {
    without_ttou(|| {
        if unsafe { libc::tcsetpgrp(0, group) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    })
}
fn without_ttou(action: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    unsafe {
        let mut mask = std::mem::zeroed();
        let mut old = std::mem::zeroed();
        libc::sigemptyset(&mut mask);
        libc::sigaddset(&mut mask, libc::SIGTTOU);
        if libc::sigprocmask(libc::SIG_BLOCK, &mask, &mut old) != 0 {
            return Err(io::Error::last_os_error());
        }
        let result = action();
        libc::sigprocmask(libc::SIG_SETMASK, &old, std::ptr::null_mut());
        result
    }
}

pub fn private_identifier() -> io::Result<String> {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

struct InvocationChannel {
    _directory: tempfile::TempDir,
    path: std::path::PathBuf,
    token: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    initialize: Option<std::sync::mpsc::SyncSender<Option<HookHandler>>>,
    native: std::sync::Arc<std::sync::Mutex<Option<libc::pid_t>>>,
    completion: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
}
impl InvocationChannel {
    fn new() -> io::Result<Self> {
        use std::os::unix::{fs::PermissionsExt, net::UnixListener};
        let directory = tempfile::Builder::new().prefix("ovrcr-a-").tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        let path = directory.path().join("hook");
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let token = private_identifier()?;
        let expected = format!("{token}\n").into_bytes();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = stop.clone();
        let completion = std::sync::Arc::new(std::sync::Mutex::new(None));
        let thread_completion = completion.clone();
        let native = std::sync::Arc::new(std::sync::Mutex::new(None));
        let thread_native = native.clone();
        let (initialize, ready) = std::sync::mpsc::sync_channel::<Option<HookHandler>>(1);
        // Start the receiver before reporting setup can authorize argv changes.
        let thread = std::thread::Builder::new().spawn(move || {
            use std::{io::Write, sync::atomic::Ordering, time::Instant};
            let Ok(mut handler) = ready.recv() else {
                return;
            };
            let stop = thread_stop;
            let mut next_poll = Instant::now();
            while !stop.load(Ordering::SeqCst) {
                let completion_deadline = *thread_completion.lock().unwrap();
                if let Some(deadline) = completion_deadline {
                    if let Some(handler) = &mut handler {
                        handler(HookEvent::NativeCompleted { deadline });
                    }
                    break;
                }
                if Instant::now() >= next_poll {
                    next_poll = Instant::now() + Duration::from_millis(100);
                    if let Some(handler) = &mut handler {
                        handler(HookEvent::Poll {
                            deadline: next_poll,
                        });
                    }
                }
                if thread_completion.lock().unwrap().is_some() {
                    continue;
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        let started = Instant::now();
                        let auth_deadline = started + Duration::from_millis(200);
                        let mut received = [0u8; 65];
                        if read_private_bytes(&mut stream, &mut received, auth_deadline).is_err()
                            || received.as_slice() != expected
                        {
                            continue;
                        }
                        let mut length = [0u8; 4];
                        let response = if read_private_bytes(
                            &mut stream,
                            &mut length,
                            auth_deadline,
                        )
                        .is_ok()
                        {
                            let length = u32::from_be_bytes(length) as usize;
                            if length > 65_536 {
                                continue;
                            }
                            let mut request = vec![0u8; length];
                            if read_private_bytes(&mut stream, &mut request, auth_deadline).is_err()
                            {
                                continue;
                            }
                            if let Some(handler) = &mut handler {
                                handler(HookEvent::Request {
                                    input: &request,
                                    native_root: trusted_native_root(&stream, &thread_native),
                                    deadline: started + Duration::from_millis(800),
                                })
                            } else {
                                b"admission-unavailable\n".to_vec()
                            }
                        } else {
                            b"admission-unavailable\n".to_vec()
                        };
                        if response.len() > 65_536 {
                            continue;
                        }
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                        let _ = stream.write_all(&response);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        })?;
        Ok(Self {
            _directory: directory,
            path,
            token,
            stop,
            thread: Some(thread),
            initialize: Some(initialize),
            completion,
            native,
        })
    }
    fn complete(&mut self, deadline: std::time::Instant) {
        *self.completion.lock().unwrap() = Some(deadline);
        // The handler contract uses this same absolute deadline. Never acquire a new
        // grace period or block indefinitely joining a failed reporting callback.
        while self
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        if let Some(thread) = self.thread.take() {
            if thread.is_finished() {
                let _ = thread.join();
            } else {
                self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }
    fn start(&mut self, handler: Option<HookHandler>) {
        if let Some(initialize) = self.initialize.take() {
            let _ = initialize.send(handler);
        }
    }
}
// The caller holds the native anchor only while checking OS evidence, never over
// provider callbacks or transport. The supervisor clears it before reaping.
//
// Codex CLI >=0.158 runs synchronous hooks from a detached
// `app-server --managed-daemon` that is a descendant of the native TUI (often
// under a node wrapper), not a direct child. Trust any peer in the native
// process tree; reject callers outside it even with a stolen token.
fn trusted_native_root(
    stream: &std::os::unix::net::UnixStream,
    native: &std::sync::Mutex<Option<libc::pid_t>>,
) -> bool {
    let anchor = native.lock().unwrap();
    let Some(root) = *anchor else {
        return false;
    };
    let Some(peer) = peer_pid(stream) else {
        return false;
    };
    if !native_tree_contains(peer, root) {
        return false;
    }
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // A waitable exit is already dead even when the supervisor has not observed it.
    unsafe {
        libc::waitid(
            libc::P_PID,
            root as _,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        ) == 0
            && info.si_pid() == 0
    }
}

fn native_tree_contains(peer: libc::pid_t, root: libc::pid_t) -> bool {
    let mut current = peer;
    for _ in 0..32 {
        if current == root {
            return true;
        }
        if current <= 1 {
            return false;
        }
        match process_parent(current) {
            Some(parent) if parent > 0 && parent != current => current = parent,
            _ => return false,
        }
    }
    false
}

#[cfg(target_os = "macos")]
fn peer_pid(stream: &std::os::unix::net::UnixStream) -> Option<libc::pid_t> {
    use std::os::fd::AsRawFd;
    let mut uid = 0;
    let mut gid = 0;
    let mut pid = 0;
    let mut len = std::mem::size_of_val(&pid) as libc::socklen_t;
    unsafe {
        if libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) != 0
            || uid != libc::geteuid()
            || libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut len,
            ) != 0
            || len as usize != std::mem::size_of_val(&pid)
        {
            return None;
        }
    }
    (pid > 0).then_some(pid)
}
#[cfg(target_os = "linux")]
fn peer_pid(stream: &std::os::unix::net::UnixStream) -> Option<libc::pid_t> {
    use std::os::fd::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of_val(&cred) as libc::socklen_t;
    unsafe {
        if libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        ) != 0
            || len as usize != std::mem::size_of_val(&cred)
            || cred.uid != libc::geteuid()
        {
            return None;
        }
    }
    (cred.pid > 0).then_some(cred.pid)
}
#[cfg(target_os = "macos")]
fn process_parent(pid: libc::pid_t) -> Option<libc::pid_t> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as libc::c_int;
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (read == size && info.pbi_pid == pid as u32).then_some(info.pbi_ppid as libc::pid_t)
}
#[cfg(target_os = "linux")]
fn process_parent(pid: libc::pid_t) -> Option<libc::pid_t> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, fields) = stat.rsplit_once(") ")?;
    fields.split_whitespace().nth(1)?.parse().ok()
}

fn read_private_bytes(
    stream: &mut std::os::unix::net::UnixStream,
    bytes: &mut [u8],
    deadline: std::time::Instant,
) -> io::Result<()> {
    use std::io::Read;
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(Some(remaining))?;
        match stream.read(&mut bytes[offset..]) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => offset += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
impl Drop for InvocationChannel {
    fn drop(&mut self) {
        drop(self.initialize.take());
        self.complete(std::time::Instant::now() + Duration::from_secs(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn invocation_channel_accepts_exact_payload_limit() {
        let mut channel = InvocationChannel::new().unwrap();
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let handler_calls = calls.clone();
        channel.start(Some(Box::new(move |event| match event {
            HookEvent::Request { input, .. } => {
                assert_eq!(input.len(), 65_536);
                handler_calls.fetch_add(1, Ordering::SeqCst);
                b"accepted\n".to_vec()
            }
            _ => Vec::new(),
        })));
        let mut stream = UnixStream::connect(&channel.path).unwrap();
        stream
            .write_all(format!("{}\n", channel.token).as_bytes())
            .unwrap();
        stream.write_all(&65_536_u32.to_be_bytes()).unwrap();
        stream.write_all(&vec![b'x'; 65_536]).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert_eq!(response, "accepted\n");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn invocation_channel_rejects_payload_above_limit_without_allocating_or_dispatching() {
        let mut channel = InvocationChannel::new().unwrap();
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let handler_calls = calls.clone();
        channel.start(Some(Box::new(move |event| {
            if matches!(event, HookEvent::Request { .. }) {
                handler_calls.fetch_add(1, Ordering::SeqCst);
            }
            Vec::new()
        })));
        let mut stream = UnixStream::connect(&channel.path).unwrap();
        stream
            .write_all(format!("{}\n", channel.token).as_bytes())
            .unwrap();
        stream.write_all(&65_537_u32.to_be_bytes()).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        assert!(response.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn native_tree_contains_direct_child_and_deeper_descendants() {
        let self_pid = unsafe { libc::getpid() };
        let parent = process_parent(self_pid).expect("parent");
        assert!(native_tree_contains(self_pid, self_pid));
        assert!(native_tree_contains(self_pid, parent));
        // Init is not under this process; a non-ancestor root never matches.
        assert!(!native_tree_contains(1, self_pid));
        assert!(!native_tree_contains(self_pid, 2_000_000_000));
    }
}

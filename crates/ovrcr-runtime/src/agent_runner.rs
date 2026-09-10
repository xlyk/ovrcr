//! Native invocation mechanics; provider admission is deliberately unavailable.
use anyhow::{Context, Result};
use signal_hook::iterator::Signals;
use std::{
    ffi::OsString,
    io,
    os::unix::process::CommandExt,
    process::{Command, ExitStatus, Stdio},
    time::Duration,
};

// All group signals happen while the unreaped direct child anchors this PGID.
pub fn run_native(argv: &[OsString]) -> Result<ExitStatus> {
    let executable = argv.first().context("native command is required")?;
    let mut signals = Signals::new([
        libc::SIGINT,
        libc::SIGTERM,
        libc::SIGHUP,
        libc::SIGQUIT,
        libc::SIGCONT,
        libc::SIGTSTP,
    ])?;
    let terminal = Terminal::capture();
    let channel = InvocationChannel::new()?;
    let mut command = Command::new(executable);
    command
        .args(&argv[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env_remove("OVRCR_HOOK_SOCKET")
        .env_remove("OVRCR_SESSION_ID")
        .env_remove("OVRCR_HOOK_TOKEN")
        .env("OVRCR_AGENT_SOCKET", &channel.path)
        .env("OVRCR_AGENT_TOKEN", &channel.token)
        .process_group(0);
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
                    libc::CLD_EXITED | libc::CLD_KILLED | libc::CLD_DUMPED => break,
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
        // The zombie leader prevents PGID reuse while remaining group members are killed.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
        child.wait().context("wait for native agent")
    })();
    if result.is_err() {
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
        let _ = child.wait();
    }
    drop(terminal);
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
}
impl InvocationChannel {
    fn new() -> io::Result<Self> {
        use std::{
            io::{Read, Write},
            os::unix::{fs::PermissionsExt, net::UnixListener},
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
        };
        let directory = tempfile::Builder::new()
            .prefix("ovrcr-a-")
            .tempdir_in("/tmp")?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        let path = directory.path().join("hook");
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let token = private_identifier()?;
        let expected = format!("{token}\n").into_bytes();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                        let mut received = [0u8; 65];
                        let deadline = std::time::Instant::now() + Duration::from_millis(200);
                        let mut offset = 0;
                        while offset < received.len() {
                            let remaining =
                                deadline.saturating_duration_since(std::time::Instant::now());
                            if remaining.is_zero() {
                                break;
                            }
                            if stream.set_read_timeout(Some(remaining)).is_err() {
                                break;
                            }
                            match stream.read(&mut received[offset..]) {
                                Ok(0) | Err(_) => break,
                                Ok(count) => offset += count,
                            }
                        }
                        if offset == received.len() && received.as_slice() == expected {
                            // This mechanics-only endpoint deliberately never accepts provider data.
                            let _ = stream.write_all(b"admission-unavailable\n");
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            _directory: directory,
            path,
            token,
            stop,
            thread: Some(thread),
        })
    }
}
impl Drop for InvocationChannel {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

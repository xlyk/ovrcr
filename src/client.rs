use anyhow::{Context, Result, bail};
use ovrcr_protocol::exchange_preamble;
use ovrcr_runtime::server::{ServerPaths, prepare_socket_directory};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long a peer may take to answer the protocol handshake. A live server
/// answers immediately; this bounds the wait on a listener that is not one.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Connect to the server socket without performing the protocol handshake.
///
/// Use this only when something must be checked on the raw connection first,
/// such as the peer's credentials; call [`handshake`] before sending frames.
pub fn connect_raw_if_running(paths: &ServerPaths) -> Result<Option<UnixStream>> {
    match UnixStream::connect(&paths.socket) {
        Ok(stream) => Ok(Some(stream)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("connect server {}", paths.socket.display()))
        }
    }
}

/// Complete the protocol handshake on a freshly connected stream, bounded by
/// [`HANDSHAKE_TIMEOUT`], and leave the stream blocking afterwards.
pub fn handshake(stream: &mut UnixStream, socket: &Path) -> Result<()> {
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .context("bound protocol handshake")?;
    let result = exchange_preamble(stream).map_err(|error| {
        let timed_out = error.chain().any(|cause| {
            cause
                .downcast_ref::<io::Error>()
                .is_some_and(|io| matches!(io.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut))
        });
        if timed_out {
            anyhow::anyhow!(
                "no protocol handshake from {} within {:?}; the listener is not an OVRCR server or is not responding",
                socket.display(),
                HANDSHAKE_TIMEOUT
            )
        } else {
            error
        }
    });
    let _ = stream.set_read_timeout(None);
    result.with_context(|| format!("handshake with server {}", socket.display()))
}

/// Connect to the server socket and complete the protocol handshake.
pub fn connect_if_running(paths: &ServerPaths) -> Result<Option<UnixStream>> {
    let Some(mut stream) = connect_raw_if_running(paths)? else {
        return Ok(None);
    };
    handshake(&mut stream, &paths.socket)?;
    Ok(Some(stream))
}

pub fn connect_or_start(paths: &ServerPaths) -> Result<UnixStream> {
    if let Some(stream) = connect_if_running(paths)? {
        return Ok(stream);
    }
    if crate::service::start_if_installed()? {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(stream) = connect_if_running(paths)? {
                return Ok(stream);
            }
            if Instant::now() >= deadline {
                bail!("timed out waiting for installed service startup");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    let executable = std::env::var_os("OVRCR_SERVER_EXECUTABLE")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| std::env::current_exe().context("resolve OVRCR executable"))?;
    let log_path = prepare_socket_directory(&paths.socket)?;
    let log = open_server_log(&log_path)?;
    let mut command = Command::new(executable);
    command
        .arg("server")
        .stdin(Stdio::null())
        .stdout(log.try_clone().context("share server log")?)
        .stderr(log);
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().context("start detached OVRCR server")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(stream) = connect_if_running(paths)? {
            return Ok(stream);
        }
        if let Some(status) = child.try_wait().context("poll detached OVRCR server")? {
            bail!(
                "OVRCR server exited during startup ({status}); see {}\n{}",
                log_path.display(),
                log_tail(&log_path, 20)
            );
        }
        if Instant::now() >= deadline {
            bail!(
                "timed out waiting for server startup; see {}\n{}",
                log_path.display(),
                log_tail(&log_path, 20)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Open the detached server's log for append, creating it privately.
fn open_server_log(path: &Path) -> Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("open server log {}", path.display()))
}

/// The last `lines` lines of the server log, or a note when it is empty or
/// unreadable, for inclusion in startup errors.
fn log_tail(path: &Path, lines: usize) -> String {
    match std::fs::read_to_string(path) {
        Ok(contents) if contents.trim().is_empty() => "(server log is empty)".to_owned(),
        Ok(contents) => {
            let all = contents.lines().collect::<Vec<_>>();
            let start = all.len().saturating_sub(lines);
            all[start..].join("\n")
        }
        Err(error) => format!("(server log unavailable: {error})"),
    }
}

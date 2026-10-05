use anyhow::{Context, Result, bail};
use ovrcr_protocol::exchange_preamble;
use ovrcr_runtime::server::build_identity::{executable_build, read_server_build, socket_identity};
use ovrcr_runtime::server::{ServerPaths, prepare_socket_directory};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
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
    stream
        .set_write_timeout(Some(HANDSHAKE_TIMEOUT))
        .context("bound protocol handshake write")?;
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
    let _ = stream.set_write_timeout(None);
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
    let executable = intended_executable()?;
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

fn intended_executable() -> Result<PathBuf> {
    let path = std::env::var_os("OVRCR_SERVER_EXECUTABLE")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| std::env::current_exe().context("resolve OVRCR executable"))?;
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

/// Check the captured server build before Dashboard attachment. Restart
/// approval applies only to this connection and its observed socket identity.
pub fn connect_dashboard(
    paths: &ServerPaths,
    confirm: &mut impl FnMut(&str) -> Result<bool>,
) -> Result<UnixStream> {
    let executable = intended_executable()?;
    let intended = executable_build(&executable)?;
    let before = socket_identity(&paths.socket).ok();
    let Some(mut stream) = connect_raw_if_running(paths)? else {
        crate::service::dashboard_service(paths, &executable, None)?;
        let stream = connect_or_start(paths)?;
        return check_started_build(paths, stream, &intended);
    };
    let socket = socket_identity(&paths.socket)?;
    if before != Some(socket) {
        bail!("server socket changed while connecting; retry Dashboard startup");
    }
    let peer = peer_pid(&stream)?;
    let service = crate::service::dashboard_service(paths, &executable, Some(&stream))?;
    if let Err(error) = handshake(&mut stream, &paths.socket) {
        let detail = error
            .chain()
            .find_map(|cause| {
                let text = cause.to_string();
                text.starts_with("protocol version mismatch")
                    .then(|| text.split(';').next().unwrap().to_owned())
            })
            .unwrap_or_else(|| format!("{error:#}"));
        bail!(
            "{detail}; automatic restart refused for {}. Use the matching old CLI with OVRCR_SOCKET set to this socket and `shutdown --kill`, or its matching CLI's `service stop`, then retry",
            paths.socket.display()
        );
    }
    let captured = read_server_build(&paths.socket, peer).ok();
    if captured
        .as_ref()
        .is_some_and(|record| record.build == intended)
    {
        return Ok(stream);
    }
    let reason = if captured.is_some() {
        "a different build"
    } else {
        "an unknown build (legacy or invalid identity)"
    };
    if !confirm(&format!(
        "Restart OVRCR server\nSocket: {}\nRunning build: {reason}\nReplacement: {}\n\nAll running sessions will stop.",
        paths.socket.display(),
        executable.display()
    ))? {
        bail!(
            "server restart declined for {}; running sessions were left untouched. Retry and approve the separate restart offer, or use the matching CLI to stop this server",
            paths.socket.display()
        );
    }
    if socket_identity(&paths.socket)? != socket
        || peer_pid(&stream)? != peer
        || read_server_build(&paths.socket, peer).ok() != captured
        || executable_build(&executable)? != intended
        || crate::service::dashboard_service(paths, &executable, Some(&stream))? != service
    {
        bail!(
            "server, executable or service changed during restart approval; no shutdown was sent; retry Dashboard startup"
        );
    }
    const STOP_TIMEOUT: Duration = Duration::from_secs(10);
    stream.set_read_timeout(Some(STOP_TIMEOUT))?;
    stream.set_write_timeout(Some(STOP_TIMEOUT))?;
    ovrcr_protocol::client::shutdown(&mut stream, 1, true)
        .context("controlled server restart refused or failed; no process signal was sent")?;
    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        match std::fs::symlink_metadata(&paths.socket) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error).context("wait for checked server socket removal"),
            Ok(_) if socket_identity(&paths.socket)? != socket => bail!(
                "a replacement server appeared during controlled shutdown; retry Dashboard startup"
            ),
            Ok(_) => {}
        }
        if Instant::now() >= deadline {
            bail!("timed out waiting for controlled server shutdown; no process signal was sent");
        }
        thread::sleep(Duration::from_millis(10));
    }
    drop(stream);
    if crate::service::dashboard_service(paths, &executable, None)? != service
        || executable_build(&executable)? != intended
    {
        bail!("executable or service changed during controlled shutdown; retry Dashboard startup");
    }
    let stream = connect_or_start(paths)?;
    check_started_build(paths, stream, &intended)
}

fn check_started_build(
    paths: &ServerPaths,
    stream: UnixStream,
    intended: &ovrcr_runtime::server::build_identity::ExecutableBuild,
) -> Result<UnixStream> {
    let captured = read_server_build(&paths.socket, peer_pid(&stream)?).context(
        "started server has unknown build identity; use a current OVRCR server executable",
    )?;
    if &captured.build != intended {
        bail!(
            "started server has a different build; check OVRCR_SERVER_EXECUTABLE and the installed service, then retry Dashboard startup"
        );
    }
    Ok(stream)
}

pub(crate) fn peer_pid(stream: &UnixStream) -> Result<u32> {
    #[cfg(target_os = "macos")]
    let (level, option, mut credentials) = (libc::SOL_LOCAL, libc::LOCAL_PEERPID, 0 as libc::pid_t);
    #[cfg(target_os = "linux")]
    let (level, option, mut credentials) = (
        libc::SOL_SOCKET,
        libc::SO_PEERCRED,
        libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        },
    );
    let mut length = std::mem::size_of_val(&credentials) as libc::socklen_t;
    // SAFETY: credentials is a correctly sized, writable value for the platform's socket option.
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            level,
            option,
            std::ptr::from_mut(&mut credentials).cast(),
            &mut length,
        )
    } == -1
    {
        return Err(std::io::Error::last_os_error()).context("identify service socket peer");
    }
    #[cfg(target_os = "macos")]
    let pid = credentials;
    #[cfg(target_os = "linux")]
    let pid = credentials.pid;
    if pid <= 0 {
        bail!("service socket peer has no live process identity");
    }
    Ok(pid as u32)
}

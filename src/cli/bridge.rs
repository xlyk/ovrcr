//! Fixed-argument callback transport. It deliberately bypasses connect_or_start.
use super::RuntimeError;
use ovrcr::protocol::{self, BridgeNavigationResult, BridgeNavigationTicket, Request, Response};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const CALLBACK_BUDGET: Duration = Duration::from_millis(1500);

pub(super) fn navigate() -> super::AppResult<()> {
    let deadline = Instant::now() + CALLBACK_BUDGET;
    let bytes = read_ticket(deadline).map_err(RuntimeError::internal)?;
    let ticket: BridgeNavigationTicket =
        serde_json::from_slice(&bytes).map_err(RuntimeError::internal)?;
    if !ticket.validate()
        || !std::env::current_exe().ok().is_some_and(|path| {
            compatible_executable(
                &path,
                std::path::Path::new(&ticket.callback_executable),
                &ticket.callback_executable_sha256,
                deadline,
            )
            .unwrap_or(false)
        })
    {
        return Err(RuntimeError::new(
            protocol::ErrorCode::InvalidRequest,
            "invalid notification navigation ticket",
        ));
    }
    let result = connect_and_navigate(ticket, deadline)
        .unwrap_or_else(|_| BridgeNavigationResult::ignored());
    super::output::print_json(&serde_json::to_value(result).map_err(RuntimeError::internal)?)
}

pub(super) fn owner() -> super::AppResult<()> {
    let deadline = Instant::now() + CALLBACK_BUDGET;
    let bytes = read_ticket(deadline).map_err(RuntimeError::internal)?;
    let call: protocol::BridgeOwnerCall =
        serde_json::from_slice(&bytes).map_err(RuntimeError::internal)?;
    let context = &call.owner.context;
    if !call.owner.validate()
        || !std::env::current_exe().ok().is_some_and(|path| {
            compatible_executable(
                &path,
                std::path::Path::new(&context.callback_executable),
                &context.callback_executable_sha256,
                deadline,
            )
            .unwrap_or(false)
        })
    {
        return Err(RuntimeError::new(
            protocol::ErrorCode::InvalidRequest,
            "invalid Bridge owner context",
        ));
    }
    let result = connect_and_owner(call, deadline)
        .unwrap_or_else(|_| protocol::BridgeOwnerResult::unavailable());
    super::output::print_json(&serde_json::to_value(result).map_err(RuntimeError::internal)?)
}

fn connect_and_owner(
    call: protocol::BridgeOwnerCall,
    deadline: Instant,
) -> anyhow::Result<protocol::BridgeOwnerResult> {
    let socket = &call.owner.context.server_socket;
    let metadata = std::fs::symlink_metadata(socket)?;
    anyhow::ensure!(
        metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() },
        "invalid Bridge owner socket"
    );
    let stream = bounded_connect(std::path::Path::new(socket), deadline)?;
    let mut stream = CallbackStream::new(stream, deadline)?;
    protocol::exchange_preamble(&mut stream)?;
    match protocol::client::request(&mut stream, 1, Request::BridgeOwner(call))? {
        Response::BridgeOwner(result)
            if result.schema == protocol::BRIDGE_SCHEMA_VERSION
                && result.server_wire == protocol::PROTOCOL_VERSION =>
        {
            Ok(result)
        }
        _ => Ok(protocol::BridgeOwnerResult::unavailable()),
    }
}

fn compatible_executable(
    current: &std::path::Path,
    original: &std::path::Path,
    expected: &str,
    deadline: Instant,
) -> io::Result<bool> {
    Ok(
        ovrcr::server::bridge_executable_sha256(current, deadline)? == expected
            && ovrcr::server::bridge_executable_sha256(original, deadline)? == expected,
    )
}

fn read_ticket(deadline: Instant) -> io::Result<Vec<u8>> {
    let mut input = io::stdin().lock();
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut poll = libc::pollfd {
            fd: input.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe {
            libc::poll(
                &mut poll,
                1,
                remaining.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if ready == 0 {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut chunk = [0; 4096];
        let count = input.read(&mut chunk)?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len() + count > protocol::MAX_FRAME_BYTES {
            return Err(io::ErrorKind::InvalidData.into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

struct CallbackStream {
    stream: UnixStream,
    deadline: Instant,
}
impl CallbackStream {
    fn new(stream: UnixStream, deadline: Instant) -> io::Result<Self> {
        // Repeated SO_RCVTIMEO updates failed with EINVAL on Darwin during
        // a framed callback. Readiness polling preserves one absolute budget.
        #[cfg(target_os = "macos")]
        stream.set_nonblocking(true)?;
        Ok(Self { stream, deadline })
    }

    fn remaining(&self) -> io::Result<Duration> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(io::ErrorKind::TimedOut.into())
        } else {
            Ok(remaining)
        }
    }

    #[cfg(target_os = "macos")]
    fn wait_ready(&self, events: libc::c_short) -> io::Result<()> {
        loop {
            let remaining = self.remaining()?;
            let mut descriptor = libc::pollfd {
                fd: self.stream.as_raw_fd(),
                events,
                revents: 0,
            };
            let ready = unsafe {
                libc::poll(
                    &mut descriptor,
                    1,
                    remaining
                        .as_millis()
                        .saturating_add(1)
                        .min(i32::MAX as u128) as i32,
                )
            };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            self.remaining()?;
            if descriptor.revents & libc::POLLNVAL != 0 {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            if ready > 0 {
                return Ok(());
            }
        }
    }
}
impl Read for CallbackStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        #[cfg(target_os = "macos")]
        {
            if bytes.is_empty() {
                return Ok(0);
            }
            loop {
                self.wait_ready(libc::POLLIN)?;
                match self.stream.read(bytes) {
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    result => return result,
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.stream.set_read_timeout(Some(self.remaining()?))?;
            self.stream.read(bytes)
        }
    }
}
impl Write for CallbackStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        #[cfg(target_os = "macos")]
        {
            if bytes.is_empty() {
                return Ok(0);
            }
            loop {
                self.wait_ready(libc::POLLOUT)?;
                match self.stream.write(bytes) {
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    result => return result,
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.stream.set_write_timeout(Some(self.remaining()?))?;
            self.stream.write(bytes)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn connect_and_navigate(
    ticket: BridgeNavigationTicket,
    deadline: Instant,
) -> anyhow::Result<BridgeNavigationResult> {
    let metadata = std::fs::symlink_metadata(&ticket.server_socket)?;
    anyhow::ensure!(
        metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() },
        "invalid callback socket"
    );
    let stream = bounded_connect(std::path::Path::new(&ticket.server_socket), deadline)?;
    let mut stream = CallbackStream::new(stream, deadline)?;
    protocol::exchange_preamble(&mut stream)?;
    match protocol::client::request(&mut stream, 1, Request::NavigateNotification { ticket })? {
        Response::NotificationNavigation(result)
            if result.schema == protocol::BRIDGE_SCHEMA_VERSION
                && result.server_wire == protocol::PROTOCOL_VERSION
                && (result.applied || result.activation.is_none()) =>
        {
            Ok(result)
        }
        _ => Ok(BridgeNavigationResult::ignored()),
    }
}

fn bounded_connect(path: &std::path::Path, deadline: Instant) -> io::Result<UnixStream> {
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let path = path.as_os_str().as_bytes();
    if path.len() >= address.sun_path.len() || path.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = std::mem::size_of_val(&address) as u8;
    }
    for (output, byte) in address.sun_path.iter_mut().zip(path) {
        *output = *byte as libc::c_char;
    }
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    let connected = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&address) as libc::socklen_t,
        )
    };
    if connected < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(error);
        }
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let mut poll = libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            let ready = unsafe {
                libc::poll(
                    &mut poll,
                    1,
                    remaining
                        .as_millis()
                        .saturating_add(1)
                        .min(i32::MAX as u128) as i32,
                )
            };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if ready == 0 {
                return Err(io::ErrorKind::TimedOut.into());
            }
            let mut error: libc::c_int = 0;
            let mut length = std::mem::size_of_val(&error) as libc::socklen_t;
            if unsafe {
                libc::getsockopt(
                    fd.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_ERROR,
                    (&mut error as *mut libc::c_int).cast(),
                    &mut length,
                )
            } < 0
            {
                return Err(io::Error::last_os_error());
            }
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
            break;
        }
    }
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(false)?;
    if Instant::now() >= deadline {
        return Err(io::ErrorKind::TimedOut.into());
    }
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn callback_stream_preserves_fragmented_owner_response_under_one_deadline() {
        let (client, server) = UnixStream::pair().unwrap();
        let deadline = Instant::now() + CALLBACK_BUDGET;
        let expected = Response::BridgeOwner(protocol::BridgeOwnerResult {
            schema: protocol::BRIDGE_SCHEMA_VERSION,
            server_wire: protocol::PROTOCOL_VERSION,
            target: Some(protocol::BridgeActivationTarget {
                dashboard_pid: 7,
                dashboard_start_seconds: 8,
                dashboard_start_microseconds: 9,
                iterm_session_id: Some("w0t0p0:12345678-1234-4234-8234-123456789abc".into()),
                iterm_focus: false,
                owner: None,
            }),
        });
        let response = expected.clone();
        let peer = std::thread::spawn(move || {
            let mut server = CallbackStream::new(server, deadline).unwrap();
            protocol::exchange_preamble(&mut server).unwrap();
            let request: protocol::ClientMessage = protocol::read_frame(&mut server).unwrap();
            assert_eq!(request.request_id, 9);
            assert_eq!(request.request, Request::Inspect);
            let mut frame = Vec::new();
            protocol::write_frame(
                &mut frame,
                &protocol::ServerMessage::Response {
                    request_id: 9,
                    response,
                },
            )
            .unwrap();
            for fragment in frame.chunks(3) {
                server.write_all(fragment).unwrap();
                std::thread::park_timeout(Duration::from_millis(1));
            }
        });
        let mut client = CallbackStream::new(client, deadline).unwrap();
        protocol::exchange_preamble(&mut client).unwrap();
        assert_eq!(
            protocol::client::request(&mut client, 9, Request::Inspect).unwrap(),
            expected
        );
        peer.join().unwrap();
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn callback_stream_partial_writes_preserve_bytes_and_deadline() {
        let (client, peer) = UnixStream::pair().unwrap();
        let deadline = Instant::now() + CALLBACK_BUDGET;
        let payload: Vec<u8> = (0..128 * 1024).map(|index| (index % 251) as u8).collect();
        let length = payload.len();
        let received = std::thread::spawn(move || {
            let mut peer = CallbackStream::new(peer, deadline).unwrap();
            std::thread::park_timeout(Duration::from_millis(5));
            let mut bytes = vec![0; length];
            peer.read_exact(&mut bytes).unwrap();
            bytes
        });
        let mut client = CallbackStream::new(client, deadline).unwrap();
        client.write_all(&payload).unwrap();
        client.flush().unwrap();
        assert_eq!(received.join().unwrap(), payload);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn callback_stream_silent_expired_and_closed_peers_are_bounded() {
        let (client, _silent) = UnixStream::pair().unwrap();
        let deadline = Instant::now() + Duration::from_millis(25);
        let mut client = CallbackStream::new(client, deadline).unwrap();
        assert_eq!(
            client.read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(Instant::now() >= deadline);

        let (client, mut peer) = UnixStream::pair().unwrap();
        peer.write_all(&[2]).unwrap();
        let mut client = CallbackStream::new(client, Instant::now()).unwrap();
        assert_eq!(
            client.read(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(
            client.write(&[1]).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );

        let (client, peer) = UnixStream::pair().unwrap();
        drop(peer);
        let mut client = CallbackStream::new(client, Instant::now() + CALLBACK_BUDGET).unwrap();
        assert_eq!(client.read(&mut [0]).unwrap(), 0);
    }

    #[test]
    fn bridge_callback_accepts_only_matching_cached_executable_bytes_after_relocation() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let packaged = root.path().join("packaged");
        std::fs::write(&original, b"reviewed executable").unwrap();
        std::fs::set_permissions(&original, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::copy(&original, &packaged).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let cached = ovrcr::server::bridge_executable_sha256(&original, deadline).unwrap();
        assert!(compatible_executable(&packaged, &original, &cached, deadline).unwrap());
        std::fs::write(&original, b"new on-disk executable").unwrap();
        assert!(!compatible_executable(&packaged, &original, &cached, deadline).unwrap());
        std::fs::copy(&original, &packaged).unwrap();
        assert!(
            !compatible_executable(&packaged, &original, &cached, deadline).unwrap(),
            "replacing both paths cannot rewrite the running Server's cached digest"
        );
    }

    #[test]
    fn bridge_callback_connect_and_handshake_are_bounded_for_absent_or_wrong_listener() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("server.sock");
        assert!(bounded_connect(&socket, Instant::now() + Duration::from_millis(25)).is_err());
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let ticket = BridgeNavigationTicket {
            schema: protocol::BRIDGE_SCHEMA_VERSION,
            server_wire: protocol::PROTOCOL_VERSION,
            server_socket: socket.to_str().unwrap().into(),
            callback_executable: "/tmp/ovrcr".into(),
            callback_executable_sha256: "0".repeat(64),
            server_lifetime: "12345678-1234-4234-8234-123456789abc".into(),
            session: protocol::SessionId(1),
            run: protocol::SessionRunId(1),
        };
        let start = Instant::now();
        assert!(connect_and_navigate(ticket, start + Duration::from_millis(25)).is_err());
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "a same-user listener must not extend the CLI budget"
        );
    }
}

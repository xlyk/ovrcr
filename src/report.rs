use crate::protocol::{
    AgentReport, AgentUpdate, ClientMessage, ErrorCode, Request, Response, ServerMessage,
    read_frame, write_frame,
};
use crate::session::{AgentActivity, SessionId};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Instant;

pub const HOOK_INPUT_LIMIT: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportFailure {
    Invalid,
    Unavailable,
    Timeout,
}

#[derive(Debug)]
pub struct ReportError {
    code: ErrorCode,
    failure: ReportFailure,
}

impl ReportError {
    pub fn code(&self) -> ErrorCode {
        self.code.clone()
    }
}

impl std::fmt::Display for ReportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self.failure {
            ReportFailure::Invalid => "hook identity or input is invalid",
            ReportFailure::Unavailable => "hook server is unavailable",
            ReportFailure::Timeout => "hook report timed out",
        })
    }
}

impl std::error::Error for ReportError {}

fn report_error(code: ErrorCode, failure: ReportFailure) -> anyhow::Error {
    anyhow::Error::new(ReportError { code, failure })
}

#[derive(Debug)]
struct HookIdentity {
    socket: std::path::PathBuf,
    session: SessionId,
    capability: [u8; 32],
}

impl HookIdentity {
    fn from_environment() -> Result<Self> {
        let socket = std::env::var_os("OVRCR_HOOK_SOCKET")
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
            .ok_or_else(|| report_error(ErrorCode::InvalidRequest, ReportFailure::Invalid))?;
        let session = std::env::var("OVRCR_SESSION_ID")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .map(SessionId)
            .ok_or_else(|| report_error(ErrorCode::InvalidRequest, ReportFailure::Invalid))?;
        let token = std::env::var("OVRCR_HOOK_TOKEN")
            .ok()
            .filter(|value| value.len() == 64)
            .ok_or_else(|| report_error(ErrorCode::InvalidRequest, ReportFailure::Invalid))?;
        let mut capability = [0_u8; 32];
        for (index, pair) in token.as_bytes().chunks_exact(2).enumerate() {
            capability[index] = hex_pair(pair)
                .ok_or_else(|| report_error(ErrorCode::InvalidRequest, ReportFailure::Invalid))?;
        }
        Ok(Self {
            socket,
            session,
            capability,
        })
    }
}

fn hex_pair(pair: &[u8]) -> Option<u8> {
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    Some(nibble(pair[0])? << 4 | nibble(pair[1])?)
}

pub fn send_report(update: AgentUpdate, sequence: Option<u64>, deadline: Instant) -> Result<()> {
    let identity = HookIdentity::from_environment()?;
    let mut stream = connect_deadline(&identity.socket, deadline)
        .map_err(|error| map_transport_error(&error))?;
    let mut io = DeadlineIo::new(&mut stream, deadline);
    let message = ClientMessage {
        request_id: 1,
        request: Request::AgentReport(AgentReport {
            session: identity.session,
            capability: identity.capability,
            sequence,
            update,
        }),
    };
    write_frame(&mut io, &message).map_err(|error| map_transport_error(&error))?;
    let response =
        read_frame::<ServerMessage>(&mut io).map_err(|error| map_transport_error(&error))?;
    match response {
        ServerMessage::Response {
            request_id: 1,
            response: Response::Ok,
        } => Ok(()),
        ServerMessage::Response {
            request_id: 1,
            response: Response::Error { code, .. },
        } => Err(report_error(code, ReportFailure::Unavailable)),
        _ => Err(report_error(
            ErrorCode::Internal,
            ReportFailure::Unavailable,
        )),
    }
}

fn map_transport_error(error: &anyhow::Error) -> anyhow::Error {
    if error.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|error| error.kind() == io::ErrorKind::TimedOut)
    }) {
        report_error(ErrorCode::Conflict, ReportFailure::Timeout)
    } else {
        report_error(ErrorCode::NotFound, ReportFailure::Unavailable)
    }
}

#[derive(Deserialize)]
struct ClaudeHookInput {
    hook_event_name: String,
    session_id: String,
    #[serde(default, deserialize_with = "deserialize_agent_id")]
    agent_id: Option<String>,
}

fn deserialize_agent_id<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}

pub fn claude_activity(input: &[u8]) -> Result<Option<AgentActivity>> {
    if input.len() > HOOK_INPUT_LIMIT {
        bail!("hook input exceeds limit");
    }
    let parsed: ClaudeHookInput =
        serde_json::from_slice(input).context("parse Claude hook input")?;
    if parsed.session_id.is_empty() {
        bail!("hook session id is empty");
    }
    if parsed.agent_id.is_some() {
        return Ok(None);
    }
    Ok(match parsed.hook_event_name.as_str() {
        "SessionStart" | "Stop" => Some(AgentActivity::Idle),
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => {
            Some(AgentActivity::Busy)
        }
        "PermissionRequest" => Some(AgentActivity::WaitingInput),
        "StopFailure" => Some(AgentActivity::Error),
        "SessionEnd" => Some(AgentActivity::Unknown),
        _ => None,
    })
}

pub fn read_hook_stdin(deadline: Instant) -> Result<Vec<u8>> {
    let stdin = io::stdin();
    let stdin = stdin.lock();
    let fd = stdin.as_raw_fd();
    let mut input = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        poll_fd(fd, libc::POLLIN | libc::POLLHUP | libc::POLLERR, deadline)
            .context("wait for hook stdin")?;
        let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), buffer.len()) };
        if count == 0 {
            return Ok(input);
        }
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                if Instant::now() >= deadline {
                    return Err(
                        io::Error::new(io::ErrorKind::TimedOut, "hook deadline expired").into(),
                    );
                }
                continue;
            }
            return Err(error).context("read hook stdin");
        }
        input.extend_from_slice(&buffer[..count as usize]);
        if input.len() > HOOK_INPUT_LIMIT {
            bail!("hook input exceeds limit");
        }
    }
}

fn connect_deadline(path: &Path, deadline: Instant) -> Result<UnixStream> {
    let address = unix_address(path)?;
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error()).context("create hook socket");
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    set_nonblocking(&owned)?;
    let result = unsafe {
        libc::connect(
            owned.as_raw_fd(),
            (&address.0 as *const libc::sockaddr_un).cast(),
            address.1,
        )
    };
    if result == 0 {
        return Ok(UnixStream::from(owned));
    }
    let error = io::Error::last_os_error();
    if !matches!(
        error.raw_os_error(),
        Some(libc::EINPROGRESS | libc::EALREADY | libc::EWOULDBLOCK)
    ) {
        return Err(error).context("connect hook socket");
    }
    poll_fd(
        owned.as_raw_fd(),
        libc::POLLOUT | libc::POLLERR | libc::POLLHUP,
        deadline,
    )
    .context("wait for hook socket")?;
    let mut socket_error: libc::c_int = 0;
    let mut length = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            owned.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&mut socket_error as *mut libc::c_int).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error()).context("inspect hook socket connection");
    }
    if socket_error != 0 {
        return Err(io::Error::from_raw_os_error(socket_error)).context("connect hook socket");
    }
    Ok(UnixStream::from(owned))
}

fn set_nonblocking(fd: &OwnedFd) -> Result<()> {
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error()).context("inspect hook socket flags");
    }
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error()).context("set hook socket nonblocking");
    }
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error()).context("set hook socket close-on-exec");
    }
    Ok(())
}

fn unix_address(path: &Path) -> Result<(libc::sockaddr_un, libc::socklen_t)> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.is_empty() || bytes.contains(&0) {
        bail!("hook socket path is invalid");
    }
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= address.sun_path.len() {
        bail!("hook socket path is too long");
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    let offset = (&address.sun_path as *const _ as usize) - (&address as *const _ as usize);
    let length = offset + bytes.len() + 1;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = u8::try_from(length).context("hook socket path length overflow")?;
    }
    Ok((address, length as libc::socklen_t))
}

fn poll_fd(fd: libc::c_int, events: libc::c_short, deadline: Instant) -> io::Result<()> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "hook deadline expired"))?;
        let timeout = remaining.as_millis().min(i32::MAX as u128) as libc::c_int;
        let mut poll = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut poll, 1, timeout) };
        if result > 0 {
            if poll.revents & libc::POLLNVAL != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "hook descriptor invalid",
                ));
            }
            return Ok(());
        }
        if result == 0 {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "hook deadline expired",
            ));
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "hook deadline expired",
                ));
            }
            continue;
        }
        return Err(error);
    }
}

struct DeadlineIo<'a> {
    stream: &'a mut UnixStream,
    deadline: Instant,
}

impl<'a> DeadlineIo<'a> {
    fn new(stream: &'a mut UnixStream, deadline: Instant) -> Self {
        Self { stream, deadline }
    }
}

impl Read for DeadlineIo<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            poll_fd(
                self.stream.as_raw_fd(),
                libc::POLLIN | libc::POLLHUP | libc::POLLERR,
                self.deadline,
            )?;
            match self.stream.read(buffer) {
                Ok(count) => return Ok(count),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    if Instant::now() >= self.deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "hook deadline expired",
                        ));
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Write for DeadlineIo<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            poll_fd(
                self.stream.as_raw_fd(),
                libc::POLLOUT | libc::POLLERR | libc::POLLHUP,
                self.deadline,
            )?;
            match self.stream.write(buffer) {
                Ok(count) => return Ok(count),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    if Instant::now() >= self.deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "hook deadline expired",
                        ));
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_hook_claude_maps_supported_events() {
        for (event, expected) in [
            ("SessionStart", AgentActivity::Idle),
            ("UserPromptSubmit", AgentActivity::Busy),
            ("PreToolUse", AgentActivity::Busy),
            ("PostToolUse", AgentActivity::Busy),
            ("PostToolUseFailure", AgentActivity::Busy),
            ("PermissionRequest", AgentActivity::WaitingInput),
            ("Stop", AgentActivity::Idle),
            ("StopFailure", AgentActivity::Error),
            ("SessionEnd", AgentActivity::Unknown),
        ] {
            let input = format!(r#"{{"hook_event_name":"{event}","session_id":"root-1"}}"#);
            assert_eq!(
                claude_activity(input.as_bytes()).unwrap(),
                Some(expected),
                "{event}"
            );
        }
    }

    #[test]
    fn agent_hook_claude_ignores_nested_and_unknown_events() {
        let nested = br#"{"hook_event_name":"Stop","session_id":"root-1","agent_id":"child-1"}"#;
        assert_eq!(claude_activity(nested).unwrap(), None);
        let unknown = br#"{"hook_event_name":"Notification","session_id":"root-1"}"#;
        assert_eq!(claude_activity(unknown).unwrap(), None);
        let unknown_event = br#"{"hook_event_name":"FutureEvent","session_id":"root-1"}"#;
        assert_eq!(claude_activity(unknown_event).unwrap(), None);
    }

    #[test]
    fn agent_hook_claude_rejects_malformed_or_oversized_input() {
        assert!(claude_activity(b"not json").is_err());
        assert!(claude_activity(br#"{"hook_event_name":"Stop"}"#).is_err());
        assert!(claude_activity(br#"{"hook_event_name":"Stop","session_id":""}"#).is_err());
        assert!(
            claude_activity(br#"{"hook_event_name":"Stop","session_id":"root-1","agent_id":1}"#)
                .is_err()
        );
        let oversized = vec![b' '; HOOK_INPUT_LIMIT + 1];
        assert!(claude_activity(&oversized).is_err());
    }
}

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io::{Read, Write};

pub const MAX_FRAME_BYTES: usize = 1_048_576;

pub fn write_frame<T: Serialize, W: Write>(writer: &mut W, value: &T) -> Result<()> {
    let bytes = bincode::serde::encode_to_vec(value, bincode::config::standard())
        .context("serialize protocol frame")?;
    if bytes.len() > MAX_FRAME_BYTES {
        bail!("frame too large: {} bytes", bytes.len());
    }
    let length = u32::try_from(bytes.len()).context("protocol frame length overflow")?;
    writer
        .write_all(&length.to_be_bytes())
        .context("write protocol frame length")?;
    writer.write_all(&bytes).context("write protocol frame")?;
    writer.flush().context("flush protocol frame")?;
    Ok(())
}

pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<T> {
    let mut header = [0_u8; 4];
    reader
        .read_exact(&mut header)
        .context("read protocol frame length")?;
    let length = u32::from_be_bytes(header) as usize;
    if length > MAX_FRAME_BYTES {
        bail!("frame too large: {length} bytes");
    }
    let mut bytes = vec![0_u8; length];
    reader
        .read_exact(&mut bytes)
        .context("read protocol frame")?;
    let (value, consumed) = bincode::serde::decode_from_slice(&bytes, bincode::config::standard())
        .context("decode protocol frame")?;
    if consumed != bytes.len() {
        bail!("malformed protocol frame: trailing bytes");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentActivity, AgentReport, AgentUpdate, ClientMessage, Request, ServerEvent,
        ServerMessage, SessionId, SessionPhase, SessionSummary,
    };
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    #[test]
    fn request_round_trip_preserves_raw_bytes() {
        let (mut left, mut right) = UnixStream::pair().unwrap();
        let message = ClientMessage {
            request_id: 7,
            request: Request::Input {
                session: SessionId(9),
                bytes: vec![0, 27, 255],
            },
        };
        write_frame(&mut left, &message).unwrap();
        assert_eq!(read_frame::<ClientMessage>(&mut right).unwrap(), message);
    }

    #[test]
    fn oversized_frame_is_rejected_before_allocation() {
        let (mut left, mut right) = UnixStream::pair().unwrap();
        left.write_all(&((MAX_FRAME_BYTES as u32) + 1).to_be_bytes())
            .unwrap();
        assert!(
            read_frame::<Request>(&mut right)
                .unwrap_err()
                .to_string()
                .contains("frame too large")
        );
    }

    #[test]
    fn appended_resource_requests_round_trip() {
        for request in [
            Request::Inspect,
            Request::ReadTerminal {
                session: SessionId(3),
                max_lines: Some(7),
            },
            Request::SendTerminal {
                session: SessionId(3),
                text: "hello\nworld".into(),
                submit: false,
            },
            Request::CloseTerminal {
                session: SessionId(3),
            },
        ] {
            let (mut left, mut right) = UnixStream::pair().unwrap();
            let message = ClientMessage {
                request_id: 11,
                request,
            };
            write_frame(&mut left, &message).unwrap();
            assert_eq!(read_frame::<ClientMessage>(&mut right).unwrap(), message);
        }
    }

    #[test]
    fn pause_resume_requests_round_trip() {
        let requests = [
            Request::PauseSession {
                session: SessionId(3),
            },
            Request::ResumeSession {
                session: SessionId(3),
            },
        ];
        for request in requests {
            let (mut left, mut right) = UnixStream::pair().unwrap();
            let message = ClientMessage {
                request_id: 12,
                request,
            };
            write_frame(&mut left, &message).unwrap();
            assert_eq!(read_frame::<ClientMessage>(&mut right).unwrap(), message);
        }

        let paused = SessionSummary {
            id: SessionId(3),
            project: "project".into(),
            workspace: "workspace".into(),
            name: "session".into(),
            label: "sh".into(),
            pid: Some(42),
            started_unix_ms: 7,
            phase: SessionPhase::Paused,
            activity: AgentActivity::Unknown,
            context_usage: None,
        };
        let (mut left, mut right) = UnixStream::pair().unwrap();
        let message = ServerMessage::Event(ServerEvent::SessionChanged(paused));
        write_frame(&mut left, &message).unwrap();
        assert_eq!(read_frame::<ServerMessage>(&mut right).unwrap(), message);
    }

    #[test]
    fn agent_report_round_trip_and_redaction() {
        let token = [0xA5; 32];
        let report = AgentReport {
            session: SessionId(3),
            capability: token,
            sequence: Some(7),
            update: AgentUpdate::Activity(AgentActivity::Busy),
        };
        let debug = format!("{report:?}");
        assert!(debug.contains("capability: [redacted]"));
        assert!(!debug.contains("165"));
        assert!(debug.contains("Busy"));

        let (mut left, mut right) = UnixStream::pair().unwrap();
        write_frame(&mut left, &report).unwrap();
        assert_eq!(read_frame::<AgentReport>(&mut right).unwrap(), report);
    }
}

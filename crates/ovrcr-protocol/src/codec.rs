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
        AgentActivity, AgentReport, AgentUpdate, ClientMessage, DashboardView, PaneTarget, Request,
        Response, ServerEvent, ServerMessage, SessionId, SessionPhase, SessionSummary,
        TerminalSize,
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
    fn split_view_validation_rejects_ambiguous_targets() {
        let target = PaneTarget {
            session: SessionId(1),
            size: TerminalSize { rows: 36, cols: 39 },
        };
        let mut view = DashboardView {
            revision: 1,
            panes: vec![target.clone()],
            focused: Some(SessionId(1)),
        };
        assert!(view.validate().is_ok());
        view.panes.push(target);
        assert!(view.validate().is_err());
        view.panes.pop();
        view.focused = Some(SessionId(2));
        assert!(view.validate().is_err());

        view.panes = vec![
            PaneTarget {
                session: SessionId(1),
                size: TerminalSize { rows: 36, cols: 39 },
            },
            PaneTarget {
                session: SessionId(2),
                size: TerminalSize { rows: 18, cols: 39 },
            },
            PaneTarget {
                session: SessionId(3),
                size: TerminalSize { rows: 18, cols: 39 },
            },
        ];
        view.focused = Some(SessionId(1));
        assert!(view.validate().is_err());

        view.panes.truncate(1);
        view.revision = 0;
        assert!(view.validate().is_err());
        view.revision = 1;
        view.panes[0].size.rows = 0;
        assert!(view.validate().is_err());
        view.panes[0].size.rows = 36;
        view.panes[0].size.cols = 0;
        assert!(view.validate().is_err());
        view.panes[0].size.cols = 39;
        view.focused = None;
        assert!(view.validate().is_err());
        view.panes.clear();
        assert!(view.validate().is_ok());
        view.focused = Some(SessionId(1));
        assert!(view.validate().is_err());
    }

    #[test]
    fn split_view_frames_round_trip() {
        let view = DashboardView {
            revision: 3,
            panes: vec![PaneTarget {
                session: SessionId(1),
                size: TerminalSize { rows: 36, cols: 39 },
            }],
            focused: Some(SessionId(1)),
        };
        let message = ClientMessage {
            request_id: 2,
            request: Request::SetView { view },
        };
        let mut wire = Vec::new();
        write_frame(&mut wire, &message).unwrap();
        assert_eq!(
            read_frame::<ClientMessage>(&mut wire.as_slice()).unwrap(),
            message
        );
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

    #[test]
    fn history_page_round_trip_and_size_bound() {
        let cell = crate::HistoryCell {
            text: "x".repeat(22),
            width: 2,
            fg: crate::HistoryColor::Rgb(1, 2, 3),
            bg: crate::HistoryColor::Rgb(4, 5, 6),
            attributes: u8::MAX,
        };
        let page = crate::HistoryRows {
            session: SessionId(9),
            snapshot: crate::HistorySnapshotId(4),
            start_row: 12,
            start_col: 7,
            rows: (0..crate::PAGE_ROWS)
                .map(|_| crate::HistoryRow {
                    width: crate::PAGE_COLS,
                    cells: (0..crate::PAGE_COLS).map(|_| cell.clone()).collect(),
                    wrapped: true,
                })
                .collect(),
        };
        let message = ServerMessage::Response {
            request_id: 17,
            response: Response::HistoryRows(page.clone()),
        };
        let request = ClientMessage {
            request_id: 18,
            request: Request::HistoryPage {
                session: SessionId(9),
                snapshot: crate::HistorySnapshotId(4),
                start_row: 12,
                rows: crate::PAGE_ROWS,
                start_col: 7,
                cols: crate::PAGE_COLS,
            },
        };
        let mut frame = Vec::new();
        write_frame(&mut frame, &message).unwrap();
        let encoded = read_frame::<ServerMessage>(&mut frame.as_slice()).unwrap();
        assert_eq!(encoded, message);

        let payload_len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
        assert!(payload_len <= crate::PAGE_BYTES);

        let (mut left, mut right) = UnixStream::pair().unwrap();
        write_frame(&mut left, &request).unwrap();
        assert_eq!(read_frame::<ClientMessage>(&mut right).unwrap(), request);
    }
}

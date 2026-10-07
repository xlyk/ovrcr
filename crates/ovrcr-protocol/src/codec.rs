use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io::{Read, Write};

pub const MAX_FRAME_BYTES: usize = 1_048_576;

/// Version of the framed protocol exchanged over the server socket.
///
/// bincode encodes enum variants by declaration index, so any change to a
/// serialized type in this crate (adding, removing, or reordering a variant
/// or field of `Request`, `Response`, `ServerEvent`, `TaskRequest`,
/// `TaskResponse`, `SessionSummary`, and the types they contain) must bump
/// this number and regenerate the wire snapshot test in `wire.rs`. Peers
/// exchange it in an 8-byte preamble before the first frame so a client and
/// a long-running server built from different sources fail with a clear
/// message instead of decoding one request as another.
pub const PROTOCOL_VERSION: u32 = 42;

const PREAMBLE_MAGIC: [u8; 4] = *b"OVRC";

/// Write the 8-byte preamble: the magic `OVRC` followed by the big-endian
/// protocol version.
pub fn write_preamble<W: Write>(writer: &mut W) -> Result<()> {
    let mut preamble = [0_u8; 8];
    preamble[..4].copy_from_slice(&PREAMBLE_MAGIC);
    preamble[4..].copy_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    writer
        .write_all(&preamble)
        .context("write protocol preamble")?;
    writer.flush().context("flush protocol preamble")?;
    Ok(())
}

/// Read the peer's preamble and return the protocol version it speaks.
///
/// A peer that closes the connection or sends something other than the
/// magic is reported as such; an older OVRCR server rejects the preamble as
/// an oversized frame and closes. That close surfaces as an unexpected EOF,
/// or on Linux as a connection reset when our preamble was still unread in
/// the peer's socket buffer.
pub fn read_preamble<R: Read>(reader: &mut R) -> Result<u32> {
    let mut preamble = [0_u8; 8];
    reader.read_exact(&mut preamble).map_err(|error| {
        if matches!(
            error.kind(),
            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
        ) {
            anyhow::anyhow!(
                "peer closed the connection before completing the protocol handshake; it may be an older OVRCR build"
            )
        } else {
            anyhow::Error::new(error).context("read protocol preamble")
        }
    })?;
    if preamble[..4] != PREAMBLE_MAGIC {
        bail!("peer is not an OVRCR protocol endpoint (bad preamble magic)");
    }
    Ok(u32::from_be_bytes([
        preamble[4],
        preamble[5],
        preamble[6],
        preamble[7],
    ]))
}

/// Send our preamble, read the peer's, and fail unless the versions match.
///
/// Both sides write before they read, so the exchange cannot deadlock.
pub fn exchange_preamble<S: Read + Write>(stream: &mut S) -> Result<()> {
    write_preamble(stream)?;
    let peer = read_preamble(stream)?;
    if peer != PROTOCOL_VERSION {
        bail!(
            "protocol version mismatch: the server speaks version {peer} but this client speaks version {PROTOCOL_VERSION}; stop the old server with `ovrcr shutdown --kill` (or restart the installed service) and retry"
        );
    }
    Ok(())
}

/// Connect to a server socket and complete the protocol handshake.
pub fn connect_server(
    path: impl AsRef<std::path::Path>,
) -> std::io::Result<std::os::unix::net::UnixStream> {
    let mut stream = std::os::unix::net::UnixStream::connect(path)?;
    exchange_preamble(&mut stream).map_err(|error| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{error:#}"))
    })?;
    Ok(stream)
}

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
        Response, ServerEvent, ServerMessage, SessionId, SessionKind, SessionPhase, SessionRunId,
        SessionSummary, TerminalSize,
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
                run: SessionRunId(1),
                bytes: vec![0, 27, 255],
            },
        };
        write_frame(&mut left, &message).unwrap();
        assert_eq!(read_frame::<ClientMessage>(&mut right).unwrap(), message);
    }

    #[test]
    fn preamble_round_trip() {
        let (mut left, mut right) = UnixStream::pair().unwrap();
        write_preamble(&mut left).unwrap();
        assert_eq!(read_preamble(&mut right).unwrap(), PROTOCOL_VERSION);
        let server = std::thread::spawn(move || exchange_preamble(&mut right));
        exchange_preamble(&mut left).unwrap();
        server.join().unwrap().unwrap();
    }

    #[test]
    fn preamble_rejects_wrong_magic_and_version() {
        let (mut left, mut right) = UnixStream::pair().unwrap();
        left.write_all(b"NOPE\0\0\0\x01").unwrap();
        let error = read_preamble(&mut right).unwrap_err().to_string();
        assert!(error.contains("bad preamble magic"), "{error}");

        for version in [PROTOCOL_VERSION - 1, PROTOCOL_VERSION + 1] {
            let (mut left, mut right) = UnixStream::pair().unwrap();
            let mut preamble = Vec::from(*b"OVRC");
            preamble.extend_from_slice(&version.to_be_bytes());
            left.write_all(&preamble).unwrap();
            let error = exchange_preamble(&mut right).unwrap_err().to_string();
            assert!(error.contains("protocol version mismatch"), "{error}");
            assert!(error.contains(&format!("version {version}")), "{error}");
        }

        let (left, mut right) = UnixStream::pair().unwrap();
        drop(left);
        let error = read_preamble(&mut right).unwrap_err().to_string();
        assert!(error.contains("older OVRCR build"), "{error}");

        // A peer that closes without reading our preamble: Linux reports the
        // unread bytes as a connection reset rather than an EOF.
        let (left, mut right) = UnixStream::pair().unwrap();
        write_preamble(&mut right).unwrap();
        drop(left);
        let error = read_preamble(&mut right).unwrap_err().to_string();
        assert!(error.contains("older OVRCR build"), "{error}");
    }

    #[test]
    fn split_view_validation_rejects_ambiguous_targets() {
        let target = PaneTarget {
            session: SessionId(1),
            run: SessionRunId(1),
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
                run: SessionRunId(1),
                size: TerminalSize { rows: 36, cols: 39 },
            },
            PaneTarget {
                session: SessionId(2),
                run: SessionRunId(1),
                size: TerminalSize { rows: 18, cols: 39 },
            },
            PaneTarget {
                session: SessionId(3),
                run: SessionRunId(1),
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
                run: SessionRunId(1),
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
                expected_run: crate::SessionRunId(3),
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
            archived: false,
            cwd: "/work".into(),
            id: SessionId(3),
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            recovery: None,
            project: "project".into(),
            workspace: "workspace".into(),
            name: "session".into(),
            label: "sh".into(),
            pid: Some(42),
            started_unix_ms: Some(7),
            phase: SessionPhase::Paused,
            activity: AgentActivity::Unknown,
            context_usage: None,
            agent: None,
            agent_epoch: 0,
            unread: None,
            title: None,
            manual_title: None,
        };
        let (mut left, mut right) = UnixStream::pair().unwrap();
        let message = ServerMessage::Event(ServerEvent::SessionChanged(Box::new(paused)));
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

#[cfg(test)]
mod agent_foundation_red {
    #[test]
    fn agent_report_reservation_wire_contract() {
        let request = serde_json::json!({"ReserveAgent": {
            "session": 1, "capability": ([7; 32].to_vec()), "operation": "reserve-one",
            "expected_epoch": 0, "invocation": "invocation-one", "provider": "Claude"
        }});
        let decoded = serde_json::from_value::<crate::Request>(request);
        assert!(
            decoded.is_ok(),
            "shared reservation request must decode: {decoded:?}"
        );
    }
}

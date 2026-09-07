use super::copy::{CopyMotion, CopyPoint, CopySelection};
use super::event_loop::{
    dashboard_hello_result, dashboard_message_channel, next_dashboard_message,
};
use crate::protocol::{ErrorCode, Response, ServerMessage, SessionId};
use ovrcr_terminal::vt100;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn dashboard_surfaces_hello_refusal_and_reader_disconnect() {
    let refusal = ServerMessage::Response {
        request_id: 1,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "another dashboard is already connected".into(),
        },
    };
    let error = dashboard_hello_result(&refusal).unwrap_err().to_string();
    assert!(error.contains("dashboard hello failed"));
    assert!(error.contains("another dashboard is already connected"));

    let (sender, receiver) = dashboard_message_channel();
    drop(sender);
    let error = next_dashboard_message(&receiver).unwrap_err().to_string();
    assert_eq!(error, "dashboard connection lost");
}

#[test]
fn dashboard_output_wake_interrupts_idle_wait() {
    let (wake_receiver, mut wake_sender) = UnixStream::pair().unwrap();
    wake_receiver.set_nonblocking(true).unwrap();
    wake_sender.set_nonblocking(true).unwrap();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(20));
        wake_sender.write_all(&[1]).unwrap();
    });

    let started = Instant::now();
    let activity = super::event_loop::wait_for_dashboard_activity(
        -1,
        Some(&wake_receiver),
        Duration::from_millis(500),
    )
    .unwrap();
    assert!(activity.server_ready);
    assert!(!activity.input_ready);
    assert!(started.elapsed() < Duration::from_millis(250));
}

#[test]
fn dashboard_wait_reports_input_and_output_together() {
    let (input_receiver, mut input_sender) = UnixStream::pair().unwrap();
    let (wake_receiver, mut wake_sender) = UnixStream::pair().unwrap();
    input_receiver.set_nonblocking(true).unwrap();
    input_sender.set_nonblocking(true).unwrap();
    wake_receiver.set_nonblocking(true).unwrap();
    wake_sender.set_nonblocking(true).unwrap();
    input_sender.write_all(&[1]).unwrap();
    wake_sender.write_all(&[1]).unwrap();

    let activity = super::event_loop::wait_for_dashboard_activity(
        input_receiver.as_raw_fd(),
        Some(&wake_receiver),
        Duration::from_millis(500),
    )
    .unwrap();
    assert!(activity.input_ready);
    assert!(activity.server_ready);
}

#[test]
fn copy_selection_joins_soft_wraps_and_preserves_unicode() {
    let mut parser = vt100::Parser::new(3, 4, 0);
    parser.process("A界e\u{301}Z\r\nQ".as_bytes());
    let mut selection = CopySelection::capture(SessionId(9), parser.screen());
    assert_eq!(selection.selected_text(), None);
    selection.cursor = CopyPoint { row: 0, col: 0 };
    selection.set_anchor();
    selection.cursor = CopyPoint { row: 2, col: 0 };
    assert_eq!(
        selection.selected_text().as_deref(),
        Some("A界e\u{301}Z\nQ")
    );

    parser.process(b"\x1b[2J\x1b[Hchanged");
    assert_eq!(
        selection.selected_text().as_deref(),
        Some("A界e\u{301}Z\nQ")
    );
    std::mem::swap(&mut selection.cursor, selection.anchor.as_mut().unwrap());
    assert_eq!(
        selection.selected_text().as_deref(),
        Some("A界e\u{301}Z\nQ")
    );

    let mut explicit_spaces = vt100::Parser::new(1, 3, 0);
    explicit_spaces.process(b"a  ");
    let mut explicit_selection = CopySelection::capture(SessionId(9), explicit_spaces.screen());
    explicit_selection.cursor = CopyPoint { row: 0, col: 0 };
    explicit_selection.set_anchor();
    explicit_selection.cursor = CopyPoint { row: 0, col: 2 };
    assert_eq!(explicit_selection.selected_text().as_deref(), Some("a  "));

    let empty_cells = vt100::Parser::new(1, 3, 0);
    let mut empty_selection = CopySelection::capture(SessionId(9), empty_cells.screen());
    empty_selection.cursor = CopyPoint { row: 0, col: 0 };
    empty_selection.set_anchor();
    assert_eq!(empty_selection.selected_text().as_deref(), Some(""));

    let mut restored = vt100::Parser::new(3, 4, 0);
    restored.process(&selection.screen.state_formatted());
    let mut restored_selection = CopySelection::capture(SessionId(9), restored.screen());
    restored_selection.cursor = CopyPoint { row: 0, col: 0 };
    restored_selection.set_anchor();
    restored_selection.cursor = CopyPoint { row: 2, col: 0 };
    assert_eq!(
        restored_selection.selected_text().as_deref(),
        Some("A界e\u{301}Z\nQ")
    );
}

#[test]
fn copy_selection_moves_over_wide_cells_and_clamps() {
    let mut parser = vt100::Parser::new(2, 4, 0);
    parser.process("A界B".as_bytes());
    let mut selection = CopySelection::capture(SessionId(10), parser.screen());
    selection.cursor = CopyPoint { row: 0, col: 0 };

    selection.move_cursor(CopyMotion::Right);
    assert_eq!(selection.cursor, CopyPoint { row: 0, col: 1 });
    selection.move_cursor(CopyMotion::Right);
    assert_eq!(selection.cursor, CopyPoint { row: 0, col: 3 });
    selection.move_cursor(CopyMotion::Left);
    assert_eq!(selection.cursor, CopyPoint { row: 0, col: 1 });
    selection.set_anchor();
    assert_eq!(selection.selected_text().as_deref(), Some("界"));
    assert!(selection.contains(CopyPoint { row: 0, col: 1 }));
    assert!(selection.contains(CopyPoint { row: 0, col: 2 }));
    assert!(!selection.contains(CopyPoint { row: 0, col: 0 }));
    assert!(!selection.contains(CopyPoint { row: 99, col: 99 }));

    selection.move_cursor(CopyMotion::First);
    assert_eq!(selection.cursor, CopyPoint { row: 0, col: 0 });
    selection.move_cursor(CopyMotion::Last);
    assert_eq!(selection.cursor, CopyPoint { row: 1, col: 3 });

    let one_by_one = vt100::Parser::new(1, 1, 0);
    let mut clamped = CopySelection::capture(SessionId(11), one_by_one.screen());
    clamped.cursor = CopyPoint { row: 99, col: 99 };
    clamped.move_cursor(CopyMotion::Up);
    assert_eq!(clamped.cursor, CopyPoint { row: 0, col: 0 });
    clamped.move_cursor(CopyMotion::Down);
    assert_eq!(clamped.cursor, CopyPoint { row: 0, col: 0 });
    clamped.move_cursor(CopyMotion::Left);
    assert_eq!(clamped.cursor, CopyPoint { row: 0, col: 0 });
    clamped.move_cursor(CopyMotion::Right);
    assert_eq!(clamped.cursor, CopyPoint { row: 0, col: 0 });
    clamped.move_cursor(CopyMotion::RowStart);
    assert_eq!(clamped.cursor, CopyPoint { row: 0, col: 0 });
    clamped.move_cursor(CopyMotion::RowEnd);
    assert_eq!(clamped.cursor, CopyPoint { row: 0, col: 0 });
    clamped.move_cursor(CopyMotion::First);
    assert_eq!(clamped.cursor, CopyPoint { row: 0, col: 0 });
    clamped.move_cursor(CopyMotion::Last);
    assert_eq!(clamped.cursor, CopyPoint { row: 0, col: 0 });
}

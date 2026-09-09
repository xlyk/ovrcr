use super::copy::{CopyMotion, CopyPoint, CopySelection};
use super::event_loop::{
    dashboard_hello_result, dashboard_message_channel, drain_dashboard_input_then_emit_with,
    drain_ready_dashboard_input, emit_pending_history_copy, next_dashboard_message,
};
use super::{
    Dashboard, HistoryCopyCompletion, HistoryCopyPoint, HistoryCopyRange, HistoryCursor,
    HistoryView, InputMode,
};
use crate::protocol::{
    ErrorCode, HistoryCell, HistoryColor, HistoryOpened, HistoryRow, HistoryRows,
    HistorySnapshotId, Request, Response, ServerEvent, ServerMessage, SessionId, TerminalSize,
    write_frame,
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ovrcr_terminal::vt100;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn hints_name_targets_and_explain_disabled_session_actions() {
    use crate::protocol::{
        AgentActivity, ProjectSummary, SessionPhase, SessionSummary, WorkspaceSummary,
    };
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.hierarchy.projects.push(ProjectSummary {
        name: "consigint".into(),
        workspaces: vec![WorkspaceSummary {
            project: "consigint".into(),
            name: "auth-handoff".into(),
            path: "/tmp/auth-handoff".into(),
            sessions: vec![SessionSummary {
                id: SessionId(12),
                project: "consigint".into(),
                workspace: "auth-handoff".into(),
                name: "agent".into(),
                label: "shell".into(),
                pid: None,
                started_unix_ms: 0,
                phase: SessionPhase::Running,
                activity: AgentActivity::Unknown,
                context_usage: None,
            }],
        }],
    });
    dashboard.select_session(SessionId(12));
    let groups = super::hints::key_hints(&dashboard);
    let workspace = groups
        .iter()
        .flat_map(|g| &g.hints)
        .find(|h| h.key == "w")
        .unwrap();
    assert!(workspace.description.contains("consigint"));
    let session = groups.iter().find(|g| g.title == "Session").unwrap();
    let close = session.hints.iter().find(|h| h.key == "X").unwrap();
    assert!(close.description.contains("agent (#12)"));
    assert!(close.description.contains("auth-handoff"));
    assert!(close.description.contains("confirmation"));
    let resume = session.hints.iter().find(|h| h.key == "r").unwrap();
    assert!(!resume.enabled);
    assert!(resume.description.contains("not paused"));
    assert!(
        session
            .hints
            .iter()
            .filter(|h| h.enabled)
            .all(|h| h.description.contains("agent (#12)"))
    );
    dashboard.panes[0].session = None;
    let groups = super::hints::key_hints(&dashboard);
    assert!(
        groups
            .iter()
            .find(|g| g.title == "Session")
            .unwrap()
            .hints
            .iter()
            .all(|h| !h.enabled && h.description.contains("no session selected"))
    );
}

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
    let dashboard = staged_history_copy_dashboard();
    let mut bytes = Vec::new();
    let backend = CrosstermBackend::new(&mut bytes);
    let terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 6, 2)),
        },
    )
    .unwrap();
    drop(sender);
    let error = next_dashboard_message(&receiver).unwrap_err().to_string();
    assert_eq!(error, "dashboard connection lost");
    assert!(
        dashboard
            .history
            .as_ref()
            .unwrap()
            .copy_completion
            .is_some()
    );
    drop(dashboard);
    drop(terminal);
    assert!(bytes.is_empty());
}

#[test]
fn initial_selection_completes_zero_target_view_on_ok() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 4, cols: 20 });
    dashboard.panes[0].session = Some(SessionId(1));
    let request = dashboard
        .view_request(Rect::new(0, 0, 1, 1), 3)
        .unwrap()
        .expect("zero-target SetView should still be emitted");
    let (mut receiver, mut sender) = UnixStream::pair().unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    thread::spawn(move || {
        write_frame(
            &mut sender,
            &ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Ok,
            },
        )
        .unwrap();
    });
    super::event_loop::read_initial_selection(&mut receiver, &mut dashboard, 3, 0).unwrap();
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
}

#[test]
fn dashboard_reader_reconciles_two_pane_burst_at_ack_boundary() {
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.panes[0].session = Some(SessionId(1));
    let mut second = super::PaneState::new(TerminalSize { rows: 36, cols: 40 });
    second.session = Some(SessionId(2));
    dashboard.panes.push(second);
    dashboard.focused_pane = 1;
    let request = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 9)
        .unwrap()
        .expect("two pane view should request both snapshots");
    let Request::SetView { view } = request.request else {
        panic!("expected SetView request");
    };
    let revision = view.revision;
    let targets = view.panes.clone();
    let (sender, receiver) = mpsc::channel();
    for _ in 0..62 {
        sender
            .send(ServerMessage::Event(ServerEvent::Output {
                session: SessionId(1),
                revision,
                bytes: b"ignored while unready".to_vec(),
            }))
            .unwrap();
    }
    for (index, target) in targets.iter().enumerate() {
        sender
            .send(ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Screen {
                    session: target.session,
                    revision,
                    size: target.size,
                    bytes: format!("SCREEN_{index}").into_bytes(),
                },
            })
            .unwrap();
    }
    sender
        .send(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Ok,
        })
        .unwrap();
    sender
        .send(ServerMessage::Event(ServerEvent::Output {
            session: SessionId(1),
            revision,
            bytes: b"TAIL_A".to_vec(),
        }))
        .unwrap();
    sender
        .send(ServerMessage::Event(ServerEvent::Output {
            session: SessionId(2),
            revision,
            bytes: b"TAIL_B".to_vec(),
        }))
        .unwrap();
    let (_peer, mut stream) = UnixStream::pair().unwrap();

    assert!(
        super::event_loop::next_dashboard_messages(&receiver, &mut dashboard, &mut stream,)
            .unwrap()
    );
    assert!(dashboard.pending_view.is_some());
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    assert!(
        dashboard.panes[0]
            .parser
            .screen()
            .contents()
            .contains("SCREEN_0")
    );
    assert!(
        dashboard.panes[1]
            .parser
            .screen()
            .contents()
            .contains("SCREEN_1")
    );
    assert!(
        !dashboard.panes[0]
            .parser
            .screen()
            .contents()
            .contains("TAIL_A")
    );
    assert!(
        !dashboard.panes[1]
            .parser
            .screen()
            .contents()
            .contains("TAIL_B")
    );

    assert!(
        super::event_loop::next_dashboard_messages(&receiver, &mut dashboard, &mut stream,)
            .unwrap()
    );
    assert!(dashboard.pending_view.is_none());
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
    assert!(
        dashboard.panes[0]
            .parser
            .screen()
            .contents()
            .contains("TAIL_A")
    );
    assert!(
        dashboard.panes[1]
            .parser
            .screen()
            .contents()
            .contains("TAIL_B")
    );
}

#[test]
fn initial_selection_does_not_complete_after_wrong_screen_and_ok() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 8, cols: 40 });
    dashboard.panes[0].session = Some(SessionId(1));
    let request = dashboard
        .view_request(Rect::new(0, 0, 40, 8), 4)
        .unwrap()
        .expect("view should request a snapshot");
    let revision = dashboard.view_revision;
    let size = dashboard.panes[0].desired_size;
    let (mut receiver, mut sender) = UnixStream::pair().unwrap();
    thread::spawn(move || {
        for message in [
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Screen {
                    session: SessionId(2),
                    revision,
                    size,
                    bytes: b"wrong".to_vec(),
                },
            },
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Ok,
            },
        ] {
            write_frame(&mut sender, &message).unwrap();
        }
    });
    let result = super::event_loop::read_initial_selection(&mut receiver, &mut dashboard, 4, 1);
    assert!(
        result.is_err(),
        "wrong Screen followed by Ok must not complete startup"
    );
    assert!(!dashboard.panes[0].ready);
}

#[test]
fn initial_selection_ignores_wrong_screen_before_matching_ok() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 8, cols: 40 });
    dashboard.panes[0].session = Some(SessionId(1));
    let request = dashboard
        .view_request(Rect::new(0, 0, 40, 8), 3)
        .unwrap()
        .expect("view should request a snapshot");
    let revision = dashboard.view_revision;
    let size = dashboard.panes[0].desired_size;
    let (mut receiver, mut sender) = UnixStream::pair().unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    thread::spawn(move || {
        for message in [
            ServerMessage::Response {
                request_id: request.request_id.saturating_add(1),
                response: Response::Screen {
                    session: SessionId(1),
                    revision,
                    size,
                    bytes: b"wrong request".to_vec(),
                },
            },
            ServerMessage::Event(ServerEvent::Output {
                session: SessionId(1),
                revision,
                bytes: b"intervening".to_vec(),
            }),
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Screen {
                    session: SessionId(2),
                    revision,
                    size,
                    bytes: b"wrong".to_vec(),
                },
            },
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Ok,
            },
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Screen {
                    session: SessionId(1),
                    revision: revision.saturating_sub(1),
                    size,
                    bytes: b"wrong revision".to_vec(),
                },
            },
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Screen {
                    session: SessionId(1),
                    revision,
                    size,
                    bytes: b"right".to_vec(),
                },
            },
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Ok,
            },
        ] {
            write_frame(&mut sender, &message).unwrap();
        }
    });
    super::event_loop::read_initial_selection(&mut receiver, &mut dashboard, 3, 1).unwrap();
    assert!(dashboard.requested_view.is_some());
    assert!(dashboard.panes[0].ready);
    assert!(
        !dashboard.panes[0]
            .parser
            .screen()
            .contents()
            .contains("wrong")
    );
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
fn dashboard_input_boundary_defers_after_a_bounded_ready_batch() {
    let mut polls = 0;
    let mut processed = 0;
    let boundary = drain_ready_dashboard_input(
        || {
            polls += 1;
            Ok(true)
        },
        || {
            processed += 1;
            Ok(false)
        },
    )
    .unwrap();
    assert_eq!(boundary, super::event_loop::DashboardBoundary::InputPending);
    assert_eq!(processed, 32);
    assert_eq!(polls, 33);
}

#[test]
fn dashboard_input_boundary_reports_detach_from_a_processed_event() {
    let boundary = drain_ready_dashboard_input(|| Ok(true), || Ok(true)).unwrap();
    assert_eq!(boundary, super::event_loop::DashboardBoundary::Detached);
}

#[test]
fn dashboard_input_boundary_separates_deferred_input_from_idle_emission() {
    let (peer, mut sender) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    sender.set_nonblocking(true).unwrap();
    let mut dashboard = staged_history_copy_dashboard();
    let mut ready_polls = 0;
    let mut bytes = Vec::new();
    let backend = CrosstermBackend::new(&mut bytes);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 6, 2)),
        },
    )
    .unwrap();
    let mut mouse_enabled = false;
    let mut processed = 0;
    let boundary = drain_dashboard_input_then_emit_with(
        &mut terminal,
        &mut sender,
        &mut dashboard,
        &mut mouse_enabled,
        || {
            ready_polls += 1;
            Ok(true)
        },
        |_, _, dashboard, _| {
            processed += 1;
            assert!(matches!(
                dashboard.key_action(KeyEvent::new_with_kind(
                    KeyCode::Enter,
                    KeyModifiers::NONE,
                    KeyEventKind::Repeat,
                )),
                super::DashboardAction::None
            ));
            Ok(false)
        },
    )
    .unwrap();
    assert_eq!(boundary, super::event_loop::DashboardBoundary::InputPending);
    assert_eq!(ready_polls, 33);
    assert_eq!(processed, 32);
    assert!(
        dashboard
            .history
            .as_ref()
            .unwrap()
            .copy_completion
            .is_some()
    );
    drop(terminal);
    assert!(bytes.is_empty());

    let mut queued_escape = true;
    let mut bytes = Vec::new();
    let backend = CrosstermBackend::new(&mut bytes);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 6, 2)),
        },
    )
    .unwrap();
    let mut mouse_enabled = false;
    let boundary = drain_dashboard_input_then_emit_with(
        &mut terminal,
        &mut sender,
        &mut dashboard,
        &mut mouse_enabled,
        || {
            let ready = queued_escape;
            queued_escape = false;
            Ok(ready)
        },
        |_, _, dashboard, _| {
            assert!(matches!(
                dashboard.key_action(KeyEvent::new_with_kind(
                    KeyCode::Esc,
                    KeyModifiers::NONE,
                    KeyEventKind::Press,
                )),
                super::DashboardAction::Redraw
            ));
            Ok(false)
        },
    )
    .unwrap();
    assert_eq!(boundary, super::event_loop::DashboardBoundary::Work);
    drop(terminal);
    assert!(bytes.is_empty());
    assert!(
        dashboard
            .history
            .as_ref()
            .unwrap()
            .copy_completion
            .is_none()
    );
    assert_eq!(dashboard.copy_notice.as_deref(), Some("Copy cancelled"));

    let mut dashboard = staged_history_copy_dashboard();
    let mut bytes = Vec::new();
    let backend = CrosstermBackend::new(&mut bytes);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 6, 2)),
        },
    )
    .unwrap();
    let mut mouse_enabled = false;
    let boundary = drain_dashboard_input_then_emit_with(
        &mut terminal,
        &mut sender,
        &mut dashboard,
        &mut mouse_enabled,
        || Ok(false),
        |_, _, _, _| Ok(false),
    )
    .unwrap();
    assert_eq!(boundary, super::event_loop::DashboardBoundary::Work);
    drop(terminal);
    assert!(
        bytes
            .windows(b"\x1b]52;c;YQ==\x1b\\".len())
            .any(|window| { window == b"\x1b]52;c;YQ==\x1b\\" })
    );
}

#[test]
fn completed_history_copy_emits_once_through_the_writer_adapter() {
    let mut dashboard = staged_history_copy_dashboard();

    let mut bytes = Vec::new();
    let backend = CrosstermBackend::new(&mut bytes);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 6, 2)),
        },
    )
    .unwrap();
    assert!(emit_pending_history_copy(&mut terminal, &mut dashboard));
    assert!(!emit_pending_history_copy(&mut terminal, &mut dashboard));
    drop(terminal);
    assert!(
        bytes
            .windows(b"\x1b]52;c;YQ==\x1b\\".len())
            .any(|window| { window == b"\x1b]52;c;YQ==\x1b\\" })
    );
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Clipboard request sent; paste to verify")
    );
}

struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("clipboard writer failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn staged_history_copy_dashboard() -> Dashboard {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 4, cols: 20 });
    dashboard.select_session(SessionId(1));
    dashboard.mode = InputMode::History;
    dashboard.panes[dashboard.focused_pane].snapshot_installed = true;
    dashboard.panes[dashboard.focused_pane].ready = true;
    let opened = HistoryOpened {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        revision: 1,
        size: TerminalSize { rows: 4, cols: 20 },
        history_rows: 0,
        total_rows: 1,
    };
    let range = HistoryCopyRange {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        anchor: HistoryCopyPoint { row: 0, col: 0 },
        cursor: HistoryCopyPoint { row: 0, col: 0 },
    };
    let mut view = HistoryView::new(opened, 0);
    view.cursor = Some(HistoryCursor {
        point: range.cursor,
        row_width: 1,
        cell_width: 1,
    });
    view.cursor_target = None;
    view.anchor = Some(range.anchor);
    view.copy_completion = Some(HistoryCopyCompletion {
        id: 3,
        range,
        text: "a".into(),
    });
    dashboard.history = Some(view);
    dashboard
}

#[test]
fn failed_history_copy_writer_preserves_selection_for_retry() {
    let mut dashboard = staged_history_copy_dashboard();
    let mut writer = FailingWriter;
    let backend = CrosstermBackend::new(&mut writer);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 6, 2)),
        },
    )
    .unwrap();
    assert!(emit_pending_history_copy(&mut terminal, &mut dashboard));
    drop(terminal);
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("clipboard writer failed")
    );
    {
        let view = dashboard.history.as_ref().unwrap();
        assert_eq!(dashboard.mode, InputMode::History);
        assert_eq!(view.anchor, Some(HistoryCopyPoint { row: 0, col: 0 }));
        assert!(view.copy_completion.is_none());
    }
    let retry = match dashboard.key(KeyCode::Char('y')) {
        super::DashboardAction::Request(request) => request,
        action => panic!("expected writer retry request, got {action:?}"),
    };
    let follow_up = dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 0,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 1,
                cells: vec![HistoryCell {
                    text: "a".into(),
                    width: 1,
                    fg: HistoryColor::Default,
                    bg: HistoryColor::Default,
                    attributes: 0,
                }],
                wrapped: false,
            }],
        }),
    });
    assert!(follow_up.is_empty());
    let mut bytes = Vec::new();
    let backend = CrosstermBackend::new(&mut bytes);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 6, 2)),
        },
    )
    .unwrap();
    assert!(emit_pending_history_copy(&mut terminal, &mut dashboard));
    drop(terminal);
    assert!(
        bytes
            .windows(b"\x1b]52;c;YQ==\x1b\\".len())
            .any(|window| { window == b"\x1b]52;c;YQ==\x1b\\" })
    );
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

#[test]
fn mouse_release_precedes_the_replacement_view_request() {
    use crate::protocol::{
        AgentActivity, ClientMessage, HierarchySnapshot, ProjectSummary, SessionPhase,
        SessionSummary, WorkspaceSummary,
    };
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let summary = |id: u64| SessionSummary {
        id: SessionId(id),
        project: "consigint".into(),
        workspace: "auth".into(),
        name: "session".into(),
        label: "zsh".into(),
        pid: Some(100 + u32::try_from(id).unwrap()),
        started_unix_ms: 0,
        phase: SessionPhase::Running,
        activity: AgentActivity::Unknown,
        context_usage: None,
    };
    let hierarchy = |ids: &[u64]| HierarchySnapshot {
        projects: vec![ProjectSummary {
            name: "consigint".into(),
            workspaces: vec![WorkspaceSummary {
                project: "consigint".into(),
                name: "auth".into(),
                path: "/tmp/auth".into(),
                sessions: ids.iter().copied().map(summary).collect(),
            }],
        }],
    };
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.outer_area = area;
    dashboard.hierarchy = hierarchy(&[1, 2]);
    dashboard.panes[0].session = Some(SessionId(1));
    let mut second = super::PaneState::new(TerminalSize { rows: 36, cols: 40 });
    second.session = Some(SessionId(2));
    dashboard.panes.push(second);
    dashboard.focused_pane = 0;

    let request = dashboard
        .view_request(area, 9)
        .unwrap()
        .expect("two pane view should request both snapshots");
    let request_id = request.request_id;
    let Request::SetView { view } = request.request else {
        panic!("expected SetView request");
    };
    for pane in &view.panes {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Screen {
                session: pane.session,
                revision: view.revision,
                size: pane.size,
                bytes: Vec::new(),
            },
        });
    }
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| pane.ready));

    dashboard.mode = InputMode::Terminal;
    dashboard.panes[0].parser.process(b"\x1b[?1002h\x1b[?1006h");
    let inner = super::pane_rects(area, dashboard.panes.len(), dashboard.focused_pane)
        .into_iter()
        .find(|pane| pane.pane_index == 0)
        .expect("focused pane rect")
        .terminal;
    let down = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: inner.x + 2,
        row: inner.y + 3,
        modifiers: KeyModifiers::NONE,
    };
    assert!(
        matches!(
            dashboard.mouse_action(down, area),
            super::DashboardAction::PtyBytes(_)
        ),
        "a tracked press must forward bytes so a release is owed"
    );

    let (mut peer, mut stream) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let (sender, receiver) = mpsc::channel();
    sender
        .send(ServerMessage::Response {
            request_id: 77,
            response: Response::Hierarchy(hierarchy(&[2])),
        })
        .unwrap();
    assert!(
        super::event_loop::next_dashboard_messages(&receiver, &mut dashboard, &mut stream).unwrap()
    );
    drop(sender);

    let first = crate::protocol::read_frame::<ClientMessage>(&mut peer).unwrap();
    assert!(
        matches!(
            first.request,
            Request::Input { session, .. } if session == SessionId(1)
        ),
        "the synthetic release must be written before the replacement view, got {:?}",
        first.request
    );
    let second = crate::protocol::read_frame::<ClientMessage>(&mut peer).unwrap();
    assert!(
        matches!(second.request, Request::SetView { .. }),
        "the replacement view must follow the release, got {:?}",
        second.request
    );
}

#[test]
fn hierarchy_removal_clears_a_parked_wheel_deferral() {
    use crate::protocol::{
        AgentActivity, HierarchySnapshot, ProjectSummary, SessionPhase, SessionSummary,
        WorkspaceSummary,
    };
    use crossterm::event::MouseEventKind;
    let summary = |id: u64| SessionSummary {
        id: SessionId(id),
        project: "consigint".into(),
        workspace: "auth".into(),
        name: "session".into(),
        label: "zsh".into(),
        pid: Some(100 + u32::try_from(id).unwrap()),
        started_unix_ms: 0,
        phase: SessionPhase::Running,
        activity: AgentActivity::Unknown,
        context_usage: None,
    };
    let hierarchy = |ids: &[u64]| HierarchySnapshot {
        projects: vec![ProjectSummary {
            name: "consigint".into(),
            workspaces: vec![WorkspaceSummary {
                project: "consigint".into(),
                name: "auth".into(),
                path: "/tmp/auth".into(),
                sessions: ids.iter().copied().map(summary).collect(),
            }],
        }],
    };
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.outer_area = area;
    dashboard.hierarchy = hierarchy(&[1, 2]);
    dashboard.panes[0].session = Some(SessionId(1));
    let mut second = super::PaneState::new(TerminalSize { rows: 36, cols: 40 });
    second.session = Some(SessionId(2));
    dashboard.panes.push(second);
    dashboard.focused_pane = 0;
    let request = dashboard
        .view_request(area, 11)
        .unwrap()
        .expect("two pane view should request both snapshots");
    let request_id = request.request_id;
    let Request::SetView { view } = request.request else {
        panic!("expected SetView request");
    };
    for pane in &view.panes {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Screen {
                session: pane.session,
                revision: view.revision,
                size: pane.size,
                bytes: Vec::new(),
            },
        });
    }
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| pane.ready));

    let unfocused = super::pane_rects(area, dashboard.panes.len(), dashboard.focused_pane)
        .into_iter()
        .find(|pane| pane.pane_index == 1)
        .expect("unfocused pane rect")
        .terminal;
    let wheel = crossterm::event::MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: unfocused.x + 2,
        row: unfocused.y + 3,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(
        dashboard.mouse_action(wheel, area),
        super::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_pane, 1);
    assert_eq!(
        dashboard.deferred_history_at_tail,
        Some(1),
        "the wheel tick should park against the pane it focused"
    );

    // Session A leaves. The survivor keeps focus, so `invalidate_view_readiness` never runs,
    // but `retain` shifts B from pane 1 to pane 0 and `focused_pane` follows it.
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 12,
        response: Response::Hierarchy(hierarchy(&[2])),
    });
    assert_eq!(dashboard.panes.len(), 1);
    assert_eq!(dashboard.focused_session(), Some(SessionId(2)));
    assert_eq!(dashboard.focused_pane, 0);
    assert_eq!(
        dashboard.deferred_history_at_tail, None,
        "removing a pane reshuffles indices, so the removal must clear the parked deferral itself"
    );

    let replacement = outgoing
        .iter()
        .find_map(|message| match &message.request {
            Request::SetView { .. } => Some(message.clone()),
            _ => None,
        })
        .expect("removal should emit a replacement view");
    let Request::SetView { ref view } = replacement.request else {
        panic!("expected SetView request");
    };
    for pane in &view.panes {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: replacement.request_id,
            response: Response::Screen {
                session: pane.session,
                revision: view.revision,
                size: pane.size,
                bytes: Vec::new(),
            },
        });
    }
    let completed = dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
    assert!(
        !completed
            .iter()
            .any(|message| matches!(message.request, Request::HistoryBegin { .. })),
        "a deferral dropped by the removal must not open history later, got {completed:?}"
    );
    assert!(dashboard.history.is_none());
}

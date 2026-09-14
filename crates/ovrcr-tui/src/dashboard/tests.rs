use super::copy::{
    CopyMotion, CopyPoint, CopySelection, HistoryCopyCompletion, HistoryCopyPoint, HistoryCopyRange,
};
use super::event_loop::{
    dashboard_hello_result, dashboard_message_channel, drain_dashboard_input_then_emit_with,
    drain_ready_dashboard_input, emit_pending_history_copy, install_panic_terminal_restore_hook,
    next_dashboard_message,
};
use super::state::{HistoryCursor, HistoryView};
use super::{Dashboard, InputMode, PANIC_TERMINAL_RESTORED};
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
use std::panic;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[test]
fn sidebar_maps_each_line_to_its_visible_tree_row() {
    use super::TreeRow;
    use super::render::{tree_line_at, tree_line_count};

    let rows = vec![
        TreeRow::Project { name: "one".into() },
        TreeRow::Workspace {
            project: "one".into(),
            name: "first".into(),
        },
        TreeRow::Session { id: SessionId(1) },
        TreeRow::Workspace {
            project: "one".into(),
            name: "empty".into(),
        },
        TreeRow::Workspace {
            project: "one".into(),
            name: "last".into(),
        },
        TreeRow::Session { id: SessionId(2) },
        TreeRow::Project { name: "two".into() },
    ];
    // One line per row; a blank gap line precedes every project after the first.
    let expected = [
        Some(0),
        Some(1),
        Some(2),
        Some(3),
        Some(4),
        Some(5),
        None,
        Some(6),
    ];
    assert_eq!(tree_line_count(&rows), 8);
    for (line, row) in expected.into_iter().enumerate() {
        assert_eq!(
            tree_line_at(&rows, line),
            row.map(|row| &rows[row]),
            "line {line}"
        );
    }
    assert_eq!(tree_line_at(&rows, 8), None);
    assert_eq!(tree_line_count(&[]), 0);
    assert_eq!(tree_line_at(&[], 0), None);
}

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
                agent: None,
                agent_epoch: 0,
                unread: None,
            }],
        }],
    });
    dashboard.select_session(SessionId(12));
    let groups = super::keymap::keymap(&dashboard);
    let workspace = groups
        .iter()
        .flat_map(|g| &g.keys)
        .find(|h| h.key == "w")
        .unwrap();
    assert!(workspace.description.contains("consigint"));
    let session = groups.iter().find(|g| g.title == "Session").unwrap();
    let close = session.keys.iter().find(|h| h.key == "X").unwrap();
    assert!(close.description.contains("agent (#12)"));
    assert!(close.description.contains("auth-handoff"));
    assert!(close.description.contains("confirmation"));
    let resume = session.keys.iter().find(|h| h.key == "r").unwrap();
    assert!(!resume.enabled());
    assert!(resume.description.contains("not paused"));
    assert!(
        session
            .keys
            .iter()
            .filter(|h| h.enabled())
            .all(|h| h.description.contains("agent (#12)"))
    );
    dashboard.panes[0].session = None;
    let groups = super::keymap::keymap(&dashboard);
    assert!(
        groups
            .iter()
            .find(|g| g.title == "Session")
            .unwrap()
            .keys
            .iter()
            .all(|h| !h.enabled() && h.description.contains("no session selected"))
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
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| !dashboard.pane_ready(pane))
    );
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
    assert!(dashboard.pending_targets() > 0);
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| !dashboard.pane_ready(pane))
    );
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
    assert_eq!(dashboard.pending_targets(), 0);
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| dashboard.pane_ready(pane))
    );
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
    let revision = dashboard.view_revision();
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
    assert!(!dashboard.pane_ready(&dashboard.panes[0]));
}

#[test]
fn initial_selection_ignores_wrong_screen_before_matching_ok() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 8, cols: 40 });
    dashboard.panes[0].session = Some(SessionId(1));
    let request = dashboard
        .view_request(Rect::new(0, 0, 40, 8), 3)
        .unwrap()
        .expect("view should request a snapshot");
    let revision = dashboard.view_revision();
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
            // No `Ok` precedes the expected Screen here: a final acknowledgement without it
            // fails the view, which `initial_selection_does_not_complete_after_wrong_screen_and_ok`
            // covers.
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
    assert!(dashboard.handshake.acknowledged().is_some());
    assert!(dashboard.pane_ready(&dashboard.panes[0]));
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
    // The waker passes the barrier only as this thread goes into the wait, and
    // the idle timeout is far longer than the wake needs: returning with output
    // ready can then only be the wake, never the timeout, so the test reads no
    // clock at all.
    let gate = Arc::new(std::sync::Barrier::new(2));
    let waker_gate = Arc::clone(&gate);
    let waker = thread::spawn(move || {
        waker_gate.wait();
        wake_sender.write_all(&[1]).unwrap();
    });

    gate.wait();
    let activity = super::event_loop::wait_for_dashboard_activity(
        -1,
        Some(&wake_receiver),
        Duration::from_secs(5),
    )
    .unwrap();
    waker.join().unwrap();
    assert!(activity.server_ready);
    assert!(!activity.input_ready);
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
    dashboard.install_screen(SessionId(1), &[]);
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

fn emit_staged_history_copy(dashboard: &mut Dashboard) -> bool {
    let mut bytes = Vec::new();
    let backend = CrosstermBackend::new(&mut bytes);
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 6, 2)),
        },
    )
    .unwrap();
    emit_pending_history_copy(&mut terminal, dashboard)
}

#[test]
fn pending_history_copy_is_dropped_when_focus_leaves_the_session() {
    let mut dashboard = staged_history_copy_dashboard();
    dashboard.select_session(SessionId(2));
    assert!(!emit_staged_history_copy(&mut dashboard));
}

#[test]
fn pending_history_copy_survives_unfocused_hierarchy_removal() {
    use crate::protocol::{HierarchySnapshot, ProjectSummary, WorkspaceSummary};
    use crate::session::{SessionPhase, SessionSummary};
    let mut dashboard = staged_history_copy_dashboard();
    let summary = |id| SessionSummary {
        id: SessionId(id),
        project: "p".into(),
        workspace: "w".into(),
        name: format!("s{id}"),
        label: "zsh".into(),
        pid: Some(1),
        started_unix_ms: 0,
        phase: SessionPhase::Running,
        activity: crate::session::AgentActivity::Unknown,
        agent: None,
        agent_epoch: 0,
        unread: None,
        context_usage: None,
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 99,
        response: Response::Hierarchy(HierarchySnapshot {
            projects: vec![ProjectSummary {
                name: "p".into(),
                workspaces: vec![WorkspaceSummary {
                    project: "p".into(),
                    name: "w".into(),
                    path: std::path::PathBuf::from("/tmp"),
                    sessions: vec![summary(1)],
                }],
            }],
        }),
    });
    assert!(emit_staged_history_copy(&mut dashboard));
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
        agent: None,
        agent_epoch: 0,
        unread: None,
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
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| dashboard.pane_ready(pane))
    );

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
        agent: None,
        agent_epoch: 0,
        unread: None,
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
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| dashboard.pane_ready(pane))
    );

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

#[derive(Clone)]
struct RecordingWriter(Arc<Mutex<Vec<u8>>>);

impl Write for RecordingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// The panic hook is process-global, so tests that install one must serialize
// with any other test in this binary that also touches it.
static PANIC_HOOK_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn panic_hook_ignores_non_main_threads() {
    let _serialize = PANIC_HOOK_TEST_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());

    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writer_bytes = Arc::clone(&bytes);
    let prior_hook =
        install_panic_terminal_restore_hook(move || RecordingWriter(Arc::clone(&writer_bytes)));

    // A panic on a background thread (the dashboard reader or a task
    // worker, in production) must not touch the terminal: the main loop is
    // still drawing to it.
    let background = thread::spawn(|| {
        let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| {
            panic!("background thread panic must not restore the terminal");
        }));
    });
    background
        .join()
        .expect("the background thread must not itself panic");
    assert!(
        bytes.lock().unwrap().is_empty(),
        "a non-main-thread panic wrote to the terminal writer"
    );

    // A panic on the thread that installed the hook (the main loop) must
    // restore the terminal.
    let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        panic!("installing thread panic must restore the terminal");
    }));
    let restored = bytes.lock().unwrap().clone();
    for sequence in [b"\x1b[?1049l".as_slice(), b"\x1b[?25h".as_slice()] {
        assert!(
            restored
                .windows(sequence.len())
                .any(|window| window == sequence),
            "the installing thread's panic did not emit {sequence:?}, got {restored:?}"
        );
    }

    // Leave no trace for other tests in this binary: reinstall whatever
    // hook was active before this test, and clear the thread-local flag the
    // hook set on this thread.
    let _ = panic::take_hook();
    if let Some(prior) = prior_hook.lock().unwrap().take() {
        panic::set_hook(prior);
    }
    PANIC_TERMINAL_RESTORED.with(|restored| restored.set(false));
}

#[test]
fn provider_dashboard_preserves_quality_unknowns_and_component_age() {
    use crate::context::{ContextSource, ContextUsageReport, ContextUsageSnapshot};
    use crate::protocol::*;
    use ratatui::backend::TestBackend;
    fn measurement<T>(value: T) -> Measurement<T> {
        Measurement {
            value,
            source: "fixture".into(),
            source_revision: Some("1".into()),
            source_sequence: Some(1),
            freshness: MeasurementFreshness::SourceIdentified,
        }
    }
    let now = 400_000;
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 24,
        cols: 180,
    });
    dashboard.hierarchy.projects.push(ProjectSummary {
        name: "p".into(),
        workspaces: vec![WorkspaceSummary {
            project: "p".into(),
            name: "w".into(),
            path: "/tmp/w".into(),
            sessions: vec![SessionSummary {
                id: SessionId(1),
                project: "p".into(),
                workspace: "w".into(),
                name: "claude".into(),
                label: "claude".into(),
                pid: Some(1),
                started_unix_ms: now,
                phase: SessionPhase::Running,
                activity: AgentActivity::Busy,
                context_usage: Some(ContextUsageSnapshot {
                    report: ContextUsageReport {
                        source: ContextSource::Generic,
                        model: None,
                        conversation: None,
                        used_tokens: Some(99),
                        capacity_tokens: Some(100),
                    },
                    received_unix_ms: now,
                }),
                agent: Some(AgentSnapshot {
                    binding: AgentBinding {
                        provider: AgentProvider::Claude,
                        invocation: "private-invocation".into(),
                        conversation: "private-conversation".into(),
                        generation: 1,
                    },
                    activity: Some(ActivitySample {
                        state: AgentActivity::Idle,
                        quality: SampleQuality::Observed,
                        turn: None,
                    }),
                    metrics: Some(MetricsSnapshot {
                        sample: MetricsSample {
                            model: None,
                            context: measurement(ContextSample {
                                used_tokens: None,
                                capacity_tokens: Some(100),
                                quality: SampleQuality::Observed,
                            }),
                            usage: measurement(UsageTotals {
                                scope: UsageScope::Conversation,
                                coverage: UsageCoverage::Partial,
                                input_tokens: Some(20),
                                output_tokens: Some(5),
                                cache_read_tokens: Some(10),
                                cache_write_tokens: None,
                                reasoning_output_tokens: None,
                            }),
                            cost: measurement(None),
                        },
                        received_unix_ms: now,
                        context_received_unix_ms: now,
                        usage_received_unix_ms: now,
                        cost_received_unix_ms: now,
                    }),
                    health: HealthSample {
                        state: ReporterHealth::Connected,
                        reason: None,
                    },
                    activity_revision: 1,
                    metrics_revision: 1,
                    health_revision: 1,
                    input_requests: Vec::new(),
                    input_revision: 0,
                }),
                agent_epoch: 1,
                unread: None,
            }],
        }],
    });
    dashboard.select_session(SessionId(1));
    fn drawn(dashboard: &Dashboard, now: u64, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|frame| super::draw_dashboard_at(frame, dashboard, now))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }
    let text = drawn(&dashboard, now, 180);
    for expected in ["agent idle observed", "tokens conv partial 25", "cost —"] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(!text.contains("ctx "));
    assert!(!text.contains("99%"));
    assert!(!text.contains("private-"));
    // Ready keeps its observed quality and unknown metrics independently of reporter health.
    let original = dashboard.hierarchy.projects[0].workspaces[0].sessions[0].clone();
    {
        let session = &mut dashboard.hierarchy.projects[0].workspaces[0].sessions[0];
        session.activity = serde_json::from_str("\"ResponseReady\"").unwrap();
        let agent = session.agent.as_mut().unwrap();
        agent.activity.as_mut().unwrap().state = session.activity;
        agent.metrics = None;
    }
    let ready_text = drawn(&dashboard, now, 180);
    assert!(
        ready_text.contains("response ready · observed"),
        "{ready_text}"
    );
    assert!(ready_text.contains("tokens —  cost —"));
    dashboard.hierarchy.projects[0].workspaces[0].sessions[0]
        .agent
        .as_mut()
        .unwrap()
        .health
        .state = ReporterHealth::Unavailable;
    // Background Codex health must survive sidebar clipping, independently of the focused session.
    let mut foreground = original.clone();
    foreground.id = SessionId(2);
    dashboard.hierarchy.projects[0].workspaces[0]
        .sessions
        .push(foreground);
    dashboard.hierarchy.projects[0].workspaces[0].sessions[0]
        .agent
        .as_mut()
        .unwrap()
        .binding
        .provider = AgentProvider::Codex;
    dashboard.hierarchy.projects[0].workspaces[0].sessions[0].label =
        "long Codex label that cannot fit".into();
    dashboard.select_session(SessionId(2));
    for width in [80, 100, 120] {
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|frame| super::draw_dashboard_at(frame, &dashboard, now))
            .unwrap();
        // The sidebar shows Ready as a glyph, muted while the reporter is unavailable;
        // the literal state and health text live on the metadata line.
        let cell = &terminal.backend().buffer()[(5, 3)];
        assert_eq!(
            cell.symbol(),
            "✓",
            "background Codex glyph at width {width}"
        );
        assert_eq!(cell.fg, ratatui::style::Color::Rgb(108, 112, 134));
        let next_row: String = (0..39)
            .map(|x| terminal.backend().buffer()[(x, 4)].symbol())
            .collect();
        assert!(
            next_row.contains(&original.name),
            "the next session follows the background row: {next_row}"
        );
    }
    dashboard.hierarchy.projects[0].workspaces[0].sessions.pop();
    dashboard.select_session(SessionId(1));
    for width in [80, 100, 120] {
        for exited in [false, true] {
            dashboard.hierarchy.projects[0].workspaces[0].sessions[0].phase = if exited {
                SessionPhase::Exited {
                    code: Some(0),
                    signal: None,
                }
            } else {
                SessionPhase::Running
            };
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            terminal
                .draw(|frame| super::draw_dashboard_at(frame, &dashboard, now))
                .unwrap();
            let rect = super::pane_rects(Rect::new(0, 0, width, 24), 1, 0)[0].metadata;
            let row: String = (rect.x..rect.right())
                .map(|x| terminal.backend().buffer()[(x, rect.y)].symbol())
                .collect();
            assert!(
                row.contains(if exited { "pid: closed" } else { "pid: 1" }),
                "{row}"
            );
            assert!(
                row.contains("unavailable") && row.contains("response ready"),
                "single width={width}: {row}"
            );
        }
    }
    dashboard.hierarchy.projects[0].workspaces[0].sessions[0].phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    let exited_text = drawn(&dashboard, now, 180);
    assert!(exited_text.contains("closed"), "{exited_text}");
    assert!(
        exited_text.contains("response ready · observed"),
        "{exited_text}"
    );
    for width in [1, 2, 10, 40, 80] {
        let text = drawn(&dashboard, now, width);
        assert_eq!(text.chars().count(), usize::from(width) * 24);
    }
    dashboard.hierarchy.projects[0].workspaces[0].sessions[0].phase = SessionPhase::Running;
    let mut ready_second = dashboard.hierarchy.projects[0].workspaces[0].sessions[0].clone();
    ready_second.id = SessionId(2);
    dashboard.hierarchy.projects[0].workspaces[0]
        .sessions
        .push(ready_second);
    assert!(dashboard.split_pane());
    // Read the actual pane metadata cells: sidebar labels cannot satisfy these assertions.
    for width in [122, 142, 166, 202, 240] {
        for unavailable in [false, true] {
            for exited in [false, true] {
                for session in &mut dashboard.hierarchy.projects[0].workspaces[0].sessions {
                    session.name = "a".into();
                    session.phase = if exited {
                        SessionPhase::Exited {
                            code: Some(0),
                            signal: None,
                        }
                    } else {
                        SessionPhase::Running
                    };
                    session.agent.as_mut().unwrap().health.state = if unavailable {
                        ReporterHealth::Unavailable
                    } else {
                        ReporterHealth::Connected
                    };
                }
                for session in [SessionId(1), SessionId(2)] {
                    dashboard.install_screen(session, &[]);
                }
                let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
                terminal
                    .draw(|frame| super::draw_dashboard_at(frame, &dashboard, now))
                    .unwrap();
                let rects =
                    super::pane_rects(Rect::new(0, 0, width, 24), 2, dashboard.focused_pane);
                assert_eq!(rects.len(), 2);
                for rect in rects {
                    let row: String = (rect.metadata.x..rect.metadata.right())
                        .map(|x| terminal.backend().buffer()[(x, rect.metadata.y)].symbol())
                        .collect();
                    let context = format!(
                        "width={width}, pane_width={}, exited={exited}, unavailable={unavailable}: {row}",
                        rect.metadata.width
                    );
                    assert!(
                        row.contains(if exited { "pid: closed" } else { "pid: 1" }),
                        "{context}"
                    );
                    assert!(row.contains("response ready"), "{context}");
                    assert_eq!(row.contains("unavailable"), unavailable, "{context}");
                    if rect.metadata.width >= 62 {
                        assert!(row.contains("response ready · observed"), "{context}");
                    }
                    if unavailable && rect.metadata.width == 40 {
                        assert!(
                            row.ends_with('…'),
                            "readiness quality is visibly clipped: {context}"
                        );
                    }
                }
            }
        }
    }
    dashboard.panes.pop();
    dashboard.focused_pane = 0;
    dashboard.hierarchy.projects[0].workspaces[0].sessions.pop();
    dashboard.hierarchy.projects[0].workspaces[0].sessions[0] = original;
    let agent = dashboard.hierarchy.projects[0].workspaces[0].sessions[0]
        .agent
        .as_mut()
        .unwrap();
    agent.activity.as_mut().unwrap().quality = SampleQuality::Confirmed;
    let metrics = agent.metrics.as_mut().unwrap();
    metrics.sample.cost.value = Some(UsageCost {
        usd_ticks: 0,
        kind: CostKind::Reported,
        scope: UsageScope::Invocation,
    });
    metrics.usage_received_unix_ms = 1;
    metrics.sample.context.value.used_tokens = Some(50);
    let text = drawn(&dashboard, now, 180);
    for expected in [
        "agent idle confirmed",
        "tokens conv partial 25 stale",
        "cost inv $0.00",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    let agent = dashboard.hierarchy.projects[0].workspaces[0].sessions[0]
        .agent
        .as_mut()
        .unwrap();
    agent.health.state = ReporterHealth::Unavailable;
    let metrics = agent.metrics.as_mut().unwrap();
    metrics.sample.cost.value.as_mut().unwrap().kind = CostKind::Estimated;
    metrics.sample.context.freshness = MeasurementFreshness::Uncertain;
    let text = drawn(&dashboard, now, 180);
    for expected in ["agent unavailable", "estimate $0.00"] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    for width in [1, 2, 10, 40, 80] {
        let _ = drawn(&dashboard, now, width);
    }
    let mut second = dashboard.hierarchy.projects[0].workspaces[0].sessions[0].clone();
    second.id = SessionId(2);
    dashboard.hierarchy.projects[0].workspaces[0]
        .sessions
        .push(second);
    assert!(dashboard.split_pane());
    let text = drawn(&dashboard, now, 240);
    assert_eq!(text.matches("tokens conv partial 25 stale").count(), 2);
    assert_eq!(text.matches("cost inv estimate $0.00").count(), 2);
    for session in &mut dashboard.hierarchy.projects[0].workspaces[0].sessions {
        let metrics = session.agent.as_mut().unwrap().metrics.as_mut().unwrap();
        metrics.sample.usage.value.input_tokens = Some(711_653);
        metrics.sample.usage.value.output_tokens = Some(1);
        metrics.sample.cost.value = Some(UsageCost {
            usd_ticks: 4_700_000_000,
            kind: CostKind::Estimated,
            scope: UsageScope::Conversation,
        });
        metrics.cost_received_unix_ms = 1;
    }
    let pane_widths: Vec<_> = super::pane_rects(
        Rect::new(0, 0, 166, 24),
        dashboard.panes.len(),
        dashboard.focused_pane,
    )
    .into_iter()
    .map(|pane| pane.metadata.width)
    .collect();
    assert_eq!(pane_widths, [62, 63]);
    let text = drawn(&dashboard, now, 166);
    assert_eq!(
        text.matches("tokens conv partial 711654 stale  cost conv est $0.47 stale")
            .count(),
        2,
        "split metrics must preserve both freshness labels: {text}"
    );
    for session in &mut dashboard.hierarchy.projects[0].workspaces[0].sessions {
        let metrics = session.agent.as_mut().unwrap().metrics.as_mut().unwrap();
        metrics.sample.usage.value.input_tokens = Some(82_589);
        metrics.sample.usage.value.output_tokens = Some(1);
        metrics.sample.usage.freshness = MeasurementFreshness::Uncertain;
        metrics.sample.cost.value.as_mut().unwrap().usd_ticks = 600_000_000;
        metrics.sample.cost.freshness = MeasurementFreshness::Uncertain;
        metrics.usage_received_unix_ms = now;
        metrics.cost_received_unix_ms = now;
    }
    let text = drawn(&dashboard, now, 166);
    assert_eq!(
        text.matches("tok conv partial 82590 uncertain cost conv est $0.06 uncertain")
            .count(),
        2,
        "62-column split metrics must preserve scope, values, and uncertainty: {text}"
    );
    dashboard.hierarchy.projects[0].workspaces[0].sessions[0]
        .agent
        .as_mut()
        .unwrap()
        .metrics = None;
    assert!(!drawn(&dashboard, now, 240).contains("99%"));
}

#[test]
fn ready_sound_action_is_opt_in_and_discoverable() {
    use super::DashboardAction;
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 24,
        cols: 120,
    });
    let names = |dashboard: &Dashboard| -> Vec<(String, String)> {
        super::keymap::keymap(dashboard)
            .iter()
            .flat_map(|group| &group.keys)
            .map(|hint| (hint.key.to_string(), hint.name.to_string()))
            .collect()
    };
    assert!(names(&dashboard).contains(&("S".into(), "Enable ready sound".into())));
    assert_eq!(dashboard.key(KeyCode::Char('S')), DashboardAction::Redraw);
    assert!(names(&dashboard).contains(&("S".into(), "Disable ready sound".into())));
    assert!(
        names(&dashboard).contains(&("N".into(), "Enable desktop notifications".into())),
        "sound leaves desktop notifications untouched"
    );
}

#[test]
fn desktop_notification_action_is_opt_in_and_discoverable() {
    use super::DashboardAction;
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 24,
        cols: 120,
    });
    assert_eq!(dashboard.key(KeyCode::Char('N')), DashboardAction::Redraw);
    let hints = super::keymap::keymap(&dashboard);
    assert!(
        hints
            .iter()
            .flat_map(|group| &group.keys)
            .any(|hint| { hint.name == "Disable desktop notifications" && hint.key == "N" })
    );
    assert_eq!(dashboard.key(KeyCode::Char('N')), DashboardAction::Redraw);
    let hints = super::keymap::keymap(&dashboard);
    assert!(
        hints
            .iter()
            .flat_map(|group| &group.keys)
            .any(|hint| { hint.name == "Enable desktop notifications" && hint.key == "N" })
    );
}

#[test]
fn render_terminal_copies_text_style_wide_cells_and_cursor() {
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};
    let mut parser = vt100::Parser::new(3, 12, 0);
    parser.process(b"plain \x1b[31mred\x1b[0m\r\nwide: \xE7\x95\x8C");
    let backend = TestBackend::new(12, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            super::render::render_terminal(frame, Rect::new(0, 0, 12, 3), parser.screen(), true);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].symbol(), "p");
    assert_eq!(buffer[(0, 0)].fg, Color::Rgb(205, 214, 244));
    assert_eq!(buffer[(0, 0)].bg, Color::Rgb(30, 30, 46));
    assert_eq!(buffer[(6, 0)].fg, Color::Indexed(1));
    assert_eq!(buffer[(6, 1)].symbol(), "界");
    assert_eq!(buffer[(7, 1)].symbol(), " ");
    assert_eq!(buffer[(7, 0)].modifier, Modifier::empty());
    assert_eq!(terminal.backend().cursor_position(), (8, 1).into());
}

#[test]
fn render_terminal_crossterm_roundtrip_clears_replaced_text() {
    let mut output = Vec::new();
    {
        let backend = CrosstermBackend::new(&mut output);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 20, 2)),
            },
        )
        .unwrap();

        let mut initial = vt100::Parser::new(2, 20, 0);
        initial.process(b"CODEX BANNER\r\nprompt$ ");
        terminal
            .draw(|frame| {
                super::render::render_terminal(
                    frame,
                    Rect::new(0, 0, 20, 2),
                    initial.screen(),
                    false,
                );
            })
            .unwrap();

        let mut replacement = vt100::Parser::new(2, 20, 0);
        replacement.process(b"ok");
        terminal
            .draw(|frame| {
                super::render::render_terminal(
                    frame,
                    Rect::new(0, 0, 20, 2),
                    replacement.screen(),
                    false,
                );
            })
            .unwrap();
    }

    let mut outer = vt100::Parser::new(2, 20, 0);
    outer.process(&output);
    let rows = outer.screen().rows(0, 20).collect::<Vec<_>>();
    assert_eq!(rows[0].trim_end(), "ok");
    assert_eq!(rows[1].trim_end(), "");
}

#[test]
fn inverse_colors_keep_explicit_colors_and_set_reverse_modifier() {
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};
    let mut parser = vt100::Parser::new(1, 1, 0);
    parser.process(b"\x1b[31;42;7mX");
    let backend = TestBackend::new(1, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            super::render::render_terminal(frame, Rect::new(0, 0, 1, 1), parser.screen(), true);
        })
        .unwrap();
    let cell = &terminal.backend().buffer()[(0, 0)];
    assert_eq!(cell.fg, Color::Indexed(1));
    assert_eq!(cell.bg, Color::Indexed(2));
    assert!(cell.modifier.contains(Modifier::REVERSED));
}

fn history_cell(text: &str, width: u8) -> HistoryCell {
    HistoryCell {
        text: text.into(),
        width,
        fg: HistoryColor::Default,
        bg: HistoryColor::Default,
        attributes: 0,
    }
}

#[test]
fn history_cursor_async_continuation_resolution_reveals_predecessor_tile() {
    use super::copy::HistoryCopyPoint;
    use super::state::HistoryCursorTarget;

    let mut dashboard = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    dashboard.mode = InputMode::History;
    let mut view = HistoryView::new(
        HistoryOpened {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            revision: 1,
            size: TerminalSize { rows: 10, cols: 20 },
            history_rows: 0,
            total_rows: 1,
        },
        0,
    );
    view.left = 128;
    view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 0 },
        row_width: 130,
        cell_width: 1,
    });
    view.cursor_target = Some(HistoryCursorTarget::At(HistoryCopyPoint {
        row: 0,
        col: 128,
    }));
    dashboard.history = Some(view);
    let first = dashboard
        .history_request_if_needed()
        .expect("continuation tile request");
    assert!(matches!(
        first.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 128,
            ..
        }
    ));
    let predecessor = dashboard
        .handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: SessionId(1),
                snapshot: HistorySnapshotId(7),
                start_row: 0,
                start_col: 128,
                rows: vec![HistoryRow {
                    width: 130,
                    cells: vec![history_cell("", 0), history_cell("tail", 1)],
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("predecessor tile request");
    assert_eq!(
        dashboard.history.as_ref().unwrap().cursor.unwrap().point,
        HistoryCopyPoint { row: 0, col: 0 }
    );
    assert!(matches!(
        predecessor.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 0,
            ..
        }
    ));
    let mut leader_cells = vec![history_cell("", 1); 128];
    leader_cells[127] = history_cell("界", 2);
    let follow_up = dashboard.handle_server_message(ServerMessage::Response {
        request_id: predecessor.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 0,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 130,
                cells: leader_cells,
                wrapped: false,
            }],
        }),
    });
    assert!(follow_up.is_empty());
    let view = dashboard.history.as_ref().unwrap();
    assert_eq!(view.left, 127);
    assert_eq!(
        view.cursor.unwrap().point,
        HistoryCopyPoint { row: 0, col: 127 }
    );
    assert_eq!(view.cursor.unwrap().cell_width, 2);
}

#[test]
fn history_footer_names_escape_back_when_copy_notice_is_clear() {
    use super::copy::HistoryCopyPoint;
    use ratatui::backend::TestBackend;

    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 24,
        cols: 120,
    });
    dashboard.mode = InputMode::History;
    let mut view = HistoryView::new(
        HistoryOpened {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            revision: 1,
            size: TerminalSize { rows: 10, cols: 20 },
            history_rows: 0,
            total_rows: 2,
        },
        0,
    );
    view.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 1 },
        row_width: 2,
        cell_width: 1,
    });
    dashboard.history = Some(view);
    dashboard.copy_notice = None;
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    terminal
        .draw(|frame| super::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let footer: String = (0..120)
        .map(|x| terminal.backend().buffer()[(x, 23)].symbol().to_string())
        .collect();
    assert!(footer.contains("Esc Back"), "{footer}");
    assert!(footer.contains("y Copy"), "{footer}");
}

/// One project, one workspace, a running session per id.
fn session_hierarchy(ids: &[u64]) -> crate::protocol::HierarchySnapshot {
    use crate::protocol::{
        AgentActivity, HierarchySnapshot, ProjectSummary, SessionPhase, SessionSummary,
        WorkspaceSummary,
    };
    HierarchySnapshot {
        projects: vec![ProjectSummary {
            name: "consigint".into(),
            workspaces: vec![WorkspaceSummary {
                project: "consigint".into(),
                name: "auth".into(),
                path: "/tmp/auth".into(),
                sessions: ids
                    .iter()
                    .map(|id| SessionSummary {
                        id: SessionId(*id),
                        project: "consigint".into(),
                        workspace: "auth".into(),
                        name: "session".into(),
                        label: "zsh".into(),
                        pid: Some(100 + u32::try_from(*id).unwrap()),
                        started_unix_ms: 0,
                        phase: SessionPhase::Running,
                        activity: AgentActivity::Unknown,
                        context_usage: None,
                        agent: None,
                        agent_epoch: 0,
                        unread: None,
                    })
                    .collect(),
            }],
        }],
    }
}

#[test]
fn history_end_survives_split_and_leave_history() {
    // Regression: release_for_selection_change parked HistoryEnd in a slot that split/focus/close
    // never drained and leave_history overwrote.
    let mut dashboard = staged_history_copy_dashboard();
    dashboard.hierarchy = session_hierarchy(&[1, 2]);
    assert!(dashboard.split_pane());
    // Deliberately undrained: re-open history on the new pane and leave it, so the split's
    // release and `leave_history`'s end compete for the one slot the old code parked in.
    dashboard.mode = InputMode::History;
    dashboard.history = Some(HistoryView::new(
        HistoryOpened {
            session: SessionId(2),
            snapshot: HistorySnapshotId(9),
            revision: 1,
            size: TerminalSize { rows: 4, cols: 20 },
            history_rows: 0,
            total_rows: 1,
        },
        0,
    ));
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Esc,
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        super::DashboardAction::EnterBrowse
    ));
    let batch = dashboard.drain_outbox();
    let ends: Vec<SessionId> = batch
        .iter()
        .filter_map(|message| match message.request {
            Request::HistoryEnd { session, .. } => Some(session),
            _ => None,
        })
        .collect();
    assert_eq!(
        ends,
        vec![SessionId(1), SessionId(2)],
        "the split's release and the later leave must both survive: {batch:?}"
    );
    assert!(dashboard.drain_outbox().is_empty(), "drained once");
    assert_eq!(dashboard.mode, InputMode::Browse);
}

#[test]
fn emptying_the_tree_releases_the_captured_history() {
    // Regression: move_selection's empty-tree branch cleared the pane without releasing the
    // History/Copy capture bound to the vanished session. staged_history_copy_dashboard leaves
    // the hierarchy empty (it never assigns one), so `visible_rows()` is already empty here and
    // move_selection takes the empty-ids branch directly.
    let mut dashboard = staged_history_copy_dashboard(); // history open on session 1
    dashboard.move_selection(1);
    let batch = dashboard.drain_outbox();
    assert!(
        batch.iter().any(|m| matches!(
            m.request,
            Request::HistoryEnd {
                session: SessionId(1),
                ..
            }
        )),
        "{batch:?}"
    );
    assert_eq!(dashboard.mode, InputMode::Browse);
}

#[test]
fn mouse_cleanup_precedes_the_replacement_set_view() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.outer_area = area;
    dashboard.hierarchy = session_hierarchy(&[1, 2]);
    dashboard.panes[0].session = Some(SessionId(1));
    dashboard.mouse.held[0] = Some(super::HeldMouse {
        session: SessionId(1),
        event: MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        mode: vt100::MouseProtocolMode::PressRelease,
        encoding: vt100::MouseProtocolEncoding::Sgr,
    });
    dashboard.select_session(SessionId(2));
    let _ = dashboard.request_view_at(area);
    let batch = dashboard.drain_outbox();
    let cleanup = batch
        .iter()
        .position(|message| matches!(message.request, Request::Input { .. }));
    let set_view = batch
        .iter()
        .position(|message| matches!(message.request, Request::SetView { .. }));
    assert!(
        cleanup.is_some(),
        "the held button owes a release: {batch:?}"
    );
    assert!(set_view.is_some(), "the retarget owes a view: {batch:?}");
    assert!(cleanup < set_view, "{batch:?}");
}

fn keymap_session(
    id: u64,
    phase: crate::protocol::SessionPhase,
) -> crate::protocol::SessionSummary {
    use crate::protocol::{AgentActivity, SessionSummary};
    SessionSummary {
        id: SessionId(id),
        project: "consigint".into(),
        workspace: "auth".into(),
        name: "agent".into(),
        label: "shell".into(),
        pid: None,
        started_unix_ms: 0,
        phase,
        activity: AgentActivity::Unknown,
        context_usage: None,
        agent: None,
        agent_epoch: 0,
        unread: None,
    }
}

/// One running session in one workspace, with an acknowledged screen: the state
/// every Browse binding is available in.
fn keymap_dashboard(phase: crate::protocol::SessionPhase) -> Dashboard {
    use crate::protocol::{ProjectSummary, WorkspaceSummary};
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.hierarchy.projects.push(ProjectSummary {
        name: "consigint".into(),
        workspaces: vec![WorkspaceSummary {
            project: "consigint".into(),
            name: "auth".into(),
            path: "/tmp/auth".into(),
            sessions: vec![keymap_session(12, phase)],
        }],
    });
    dashboard.select_session(SessionId(12));
    dashboard.install_screen(SessionId(12), &[]);
    dashboard
}

fn binding_named(dashboard: &Dashboard, key: &str) -> super::keymap::KeyBinding {
    super::keymap::keymap(dashboard)
        .into_iter()
        .flat_map(|group| group.keys)
        .find(|binding| binding.key == key)
        .unwrap_or_else(|| panic!("no binding for {key}"))
}

#[test]
fn keymap_gives_each_key_in_a_mode_exactly_one_binding() {
    use super::keymap::{Action, KeyBinding, keymap};
    // Ctrl-g is the one binding whose key carries a modifier.
    let event = |binding: &KeyBinding| match binding.action {
        Action::Browse => KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
        _ => KeyEvent::new(binding.code, KeyModifiers::NONE),
    };
    let mut dashboard = keymap_dashboard(crate::protocol::SessionPhase::Running);
    for mode in [InputMode::Browse, InputMode::Copy, InputMode::History] {
        dashboard.mode = mode;
        let groups = keymap(&dashboard);
        let bindings: Vec<&KeyBinding> = groups.iter().flat_map(|group| &group.keys).collect();
        assert!(!bindings.is_empty(), "{mode:?} lists no keys");
        for binding in &bindings {
            let matched: Vec<_> = bindings
                .iter()
                .filter(|other| other.matches(event(binding)))
                .collect();
            assert_eq!(
                matched.len(),
                1,
                "{mode:?} {} resolves to {} bindings",
                binding.key,
                matched.len()
            );
            assert_eq!(
                matched[0].action, binding.action,
                "{mode:?} {}",
                binding.key
            );
        }
    }
    dashboard.mode = InputMode::Browse;
    for (event, action) in [
        (
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            Action::NextSession,
        ),
        (
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            Action::PreviousSession,
        ),
        (
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE),
            Action::OtherPane,
        ),
        (
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE),
            Action::Tasks,
        ),
        (
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
            Action::Tasks,
        ),
    ] {
        assert_eq!(
            dashboard
                .key_binding_for(event)
                .map(|binding| binding.action),
            Some(action),
            "{event:?}"
        );
    }
    // Other Ctrl-modified keys stay unbound in Browse.
    for code in [KeyCode::Char('n'), KeyCode::Char('q'), KeyCode::Char('g')] {
        assert!(
            dashboard
                .key_binding_for(KeyEvent::new(code, KeyModifiers::CONTROL))
                .is_none(),
            "{code:?}"
        );
    }
    // Toggles and one-shot captures wait for a press.
    for code in [
        KeyCode::Char('R'),
        KeyCode::Char('N'),
        KeyCode::Char('S'),
        KeyCode::Char('['),
    ] {
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            assert!(
                dashboard
                    .key_binding_for(KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind))
                    .is_none(),
                "{code:?} {kind:?}"
            );
        }
        assert!(
            dashboard
                .key_binding_for(KeyEvent::new(code, KeyModifiers::NONE))
                .is_some(),
            "{code:?}"
        );
    }
}

#[test]
fn keymap_reasons_name_the_condition_that_disables_a_key() {
    use crate::protocol::SessionPhase;
    let reason = |dashboard: &Dashboard, key: &str| binding_named(dashboard, key).reason;
    let mut dashboard = keymap_dashboard(SessionPhase::Running);
    for key in ["Enter", "p", "X", "[", "PageUp", "n", "w", "a", "q", ":"] {
        assert_eq!(reason(&dashboard, key), None, "{key} on a ready session");
    }
    assert_eq!(reason(&dashboard, "r"), Some("not paused"));
    assert_eq!(reason(&dashboard, "R"), Some("no unread response"));
    assert_eq!(reason(&dashboard, "x"), Some("only one pane"));
    assert_eq!(reason(&dashboard, "Tab/Shift-Tab"), Some("only one pane"));
    assert_eq!(reason(&dashboard, "v"), Some("no other visible session"));

    dashboard.install_unready(SessionId(12));
    for key in ["Enter", "[", "PageUp"] {
        assert_eq!(
            reason(&dashboard, key),
            Some("waiting for acknowledged screen"),
            "{key} before an ack"
        );
    }

    let mut paused = keymap_dashboard(SessionPhase::Paused);
    assert_eq!(reason(&paused, "Enter"), Some("session is not running"));
    assert_eq!(reason(&paused, "p"), Some("not running"));
    assert_eq!(reason(&paused, "r"), None);

    paused.panes[0].session = None;
    for key in ["Enter", "p", "r", "R", "X", "[", "PageUp"] {
        assert_eq!(reason(&paused, key), Some("no session selected"), "{key}");
    }

    let empty = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    assert_eq!(
        reason(&empty, "n"),
        Some("no workspace available; register a project first")
    );
    assert_eq!(reason(&empty, "w"), Some("no project registered"));
    assert_eq!(reason(&empty, "a"), None);
}

#[test]
fn key_popup_groups_take_their_keys_from_the_table() {
    use super::keymap::{Action, keymap};
    let dashboard = keymap_dashboard(crate::protocol::SessionPhase::Running);
    let bindings: Vec<_> = keymap(&dashboard)
        .into_iter()
        .flat_map(|group| group.keys)
        .collect();
    let slots: Vec<_> = bindings
        .iter()
        .filter_map(|binding| {
            binding
                .group
                .map(|slot| (slot.group, slot.key, binding.action))
        })
        .collect();
    for expected in [
        ('t', "Enter", Action::Focus),
        ('t', "p", Action::Pause),
        ('t', "r", Action::Resume),
        ('t', "R", Action::MarkReviewed),
        ('t', "x", Action::CloseTerminal),
        ('t', "c", Action::CopyScreen),
        ('t', "h", Action::History),
        ('w', "n", Action::CreateTerminal),
        ('p', "n", Action::CreateWorkspace),
        ('p', "a", Action::RegisterProject),
        ('v', "t/Ctrl-t", Action::Tasks),
        ('v', "x", Action::ClosePane),
    ] {
        assert!(
            slots.contains(&expected),
            "missing {expected:?} in {slots:?}"
        );
    }
    // A group row answers to the group's key, and the bare key keeps its own action.
    let close = bindings
        .into_iter()
        .find(|binding| binding.action == Action::CloseTerminal)
        .unwrap();
    let bare = KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE);
    let grouped = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    assert!(close.matches(bare) && !close.matches(grouped));
    let close = close.with_group_key("x");
    assert!(close.matches(grouped) && !close.matches(bare));
    assert_eq!(close.action, Action::CloseTerminal);
    assert_eq!(
        dashboard
            .key_binding_for(grouped)
            .map(|binding| binding.action),
        Some(Action::ClosePane)
    );
}

#[test]
fn key_popup_group_keeps_a_waiting_row_and_drops_an_inapplicable_one() {
    use crate::protocol::SessionPhase;
    let shown = |dashboard: &Dashboard, key: &str| binding_named(dashboard, key).shown_in_group();
    let mut dashboard = keymap_dashboard(SessionPhase::Running);
    assert!(shown(&dashboard, "Enter"));
    dashboard.install_unready(SessionId(12));
    assert!(
        shown(&dashboard, "Enter"),
        "a pane still waiting for its screen keeps the dimmed row"
    );
    let paused = keymap_dashboard(SessionPhase::Paused);
    assert!(
        !shown(&paused, "Enter"),
        "a paused session drops Focus from the group"
    );
    assert!(shown(&paused, "r"));
    assert!(!shown(&paused, "p"));
}

#[test]
fn browse_dispatch_keeps_its_own_modifier_rule() {
    use super::DashboardAction;
    use crate::protocol::SessionPhase;
    let mut dashboard = keymap_dashboard(SessionPhase::Running);
    dashboard.hierarchy.projects[0].workspaces[0]
        .sessions
        .push(keymap_session(13, SessionPhase::Running));

    // Alt and Super do not select a different action: Alt-j selects the next
    // session and Alt-x runs the pane close x runs.
    assert_eq!(dashboard.focused_session(), Some(SessionId(12)));
    dashboard.key_action(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT));
    assert_eq!(dashboard.focused_session(), Some(SessionId(13)));
    dashboard.key_action(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::SUPER));
    assert_eq!(dashboard.focused_session(), Some(SessionId(12)));
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT)),
        DashboardAction::Redraw,
        "Alt-x must reach the same action as x"
    );

    // Ctrl reaches Browse only as Ctrl-t, whatever else is held with it.
    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ] {
        let mut dashboard = keymap_dashboard(SessionPhase::Running);
        assert_eq!(
            dashboard.key_action(KeyEvent::new(KeyCode::Char('t'), modifiers)),
            DashboardAction::Redraw,
            "{modifiers:?}"
        );
        assert!(dashboard.tasks.is_some(), "{modifiers:?} must open tasks");
    }
    let mut dashboard = keymap_dashboard(SessionPhase::Running);
    for code in [KeyCode::Char('n'), KeyCode::Char('x'), KeyCode::Char('q')] {
        assert_eq!(
            dashboard.key_action(KeyEvent::new(code, KeyModifiers::CONTROL)),
            DashboardAction::None,
            "{code:?}"
        );
    }
    assert!(dashboard.tasks.is_none());

    // The popup claims Space and ? only unmodified, so a modified one runs
    // nothing rather than opening the popup.
    for code in [KeyCode::Char(' '), KeyCode::Char('?')] {
        for modifiers in [
            KeyModifiers::ALT,
            KeyModifiers::SUPER,
            KeyModifiers::CONTROL,
        ] {
            assert_eq!(
                dashboard.key_action(KeyEvent::new(code, modifiers)),
                DashboardAction::None,
                "{code:?} {modifiers:?}"
            );
            assert!(dashboard.whichkey.is_none(), "{code:?} {modifiers:?}");
        }
    }
    assert_eq!(dashboard.key(KeyCode::Char(' ')), DashboardAction::Redraw);
    assert!(
        dashboard.whichkey.is_some(),
        "the unmodified key still opens the popup"
    );

    // Ctrl blocks the Esc that cancels a history request, as it blocks every
    // Browse key but Ctrl-t.
    let mut dashboard = keymap_dashboard(SessionPhase::Running);
    dashboard.key(KeyCode::PageUp);
    assert!(dashboard.history_begin_request.is_some());
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Esc, KeyModifiers::CONTROL)),
        DashboardAction::None
    );
    assert!(
        !dashboard.history_begin_request.as_ref().unwrap().cancelled,
        "Ctrl-Esc must leave the pending request alone"
    );
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::EnterBrowse);
    assert!(dashboard.history_begin_request.as_ref().unwrap().cancelled);
}

#[test]
fn a_pending_leader_group_ignores_an_arrow_that_names_no_row() {
    use super::DashboardAction;
    use crate::protocol::SessionPhase;
    let mut dashboard = keymap_dashboard(SessionPhase::Running);
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('t'));
    // The group lists History under h. An arrow is not that key.
    for code in [KeyCode::Left, KeyCode::Down, KeyCode::Up, KeyCode::Right] {
        assert_eq!(dashboard.key(code), DashboardAction::Redraw, "{code:?}");
        assert!(
            dashboard.whichkey.is_some(),
            "{code:?} must leave the popup open"
        );
        assert!(
            dashboard.history_begin_request.is_none(),
            "{code:?} must run nothing"
        );
    }
    dashboard.key(KeyCode::Char('h'));
    assert!(dashboard.whichkey.is_none(), "h runs the History row");
    assert!(dashboard.history_begin_request.is_some());
}

#[test]
fn a_clicked_popup_row_clears_the_desktop_notice() {
    use super::DashboardAction;
    use crate::protocol::SessionPhase;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::backend::TestBackend;
    let mut dashboard = keymap_dashboard(SessionPhase::Running);
    dashboard.key(KeyCode::Char(' '));
    dashboard.desktop.notice = Some("Desktop notifications unavailable".into());
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| super::render::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let (column, row) = (0..40)
        .find_map(|y| {
            let line: String = (0..120).map(|x| buffer[(x, y)].symbol()).collect();
            line.find("q  Detach").map(|x| (x as u16 + 1, y))
        })
        .expect("the popup lists Detach");
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(
        dashboard.event_action(crossterm::event::Event::Mouse(click)),
        DashboardAction::Detach
    );
    assert!(
        dashboard.desktop.notice.is_none(),
        "a clicked row clears the notice a key press would have cleared"
    );
}

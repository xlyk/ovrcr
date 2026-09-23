//! Copy mode, frozen history, history copy, and pause/resume.

use crate::*;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr::protocol::{
    ClientMessage, ErrorCode, HistoryCell, HistoryColor, HistoryOpened, HistoryRow, HistoryRows,
    HistorySnapshotId, Request, Response, ServerEvent, ServerMessage,
};
use ovrcr::session::{AgentActivity, SessionId, SessionPhase, TerminalSize};
use ovrcr::tui::{Dashboard, DashboardAction, draw_dashboard_at};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;

fn screen_ready(bytes: &[u8]) -> Dashboard {
    let mut dashboard = dashboard_fixture();
    let id = dashboard.focused_session().unwrap();
    dashboard.install_screen(id, bytes);
    dashboard
}

fn copy_ready() -> Dashboard {
    screen_ready(b"abc")
}

fn unready_dashboard() -> Dashboard {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    dashboard.install_area(Rect::new(0, 0, 88, 38));
    dashboard.install_hierarchy(fixture_hierarchy());
    dashboard.install_focus(SessionId(1));
    dashboard
}

fn draw_text(dashboard: &Dashboard, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn footer(dashboard: &Dashboard, width: u16, height: u16) -> String {
    draw_text(dashboard, width, height)
        .lines()
        .last()
        .unwrap_or("")
        .trim_end()
        .to_owned()
}

fn focused_terminal(dashboard: &Dashboard, area: Rect) -> Rect {
    let rects = dashboard.pane_rects(area);
    assert!(!rects.is_empty(), "expected a terminal pane");
    rects[0].terminal
}

fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn history_opened(total_rows: u32) -> HistoryOpened {
    HistoryOpened {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        revision: 1,
        size: TerminalSize { rows: 10, cols: 20 },
        history_rows: total_rows.saturating_sub(10),
        total_rows,
    }
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

fn history_page_from_request(request: &ClientMessage, marker: &str) -> HistoryRows {
    let Request::HistoryPage {
        session,
        snapshot,
        start_row,
        rows,
        start_col,
        cols,
        ..
    } = request.request
    else {
        panic!("expected HistoryPage, got {:?}", request.request);
    };
    let mut first = vec![history_cell("", 1); usize::from(cols)];
    for (index, ch) in marker.chars().enumerate() {
        if let Some(cell) = first.get_mut(index) {
            *cell = history_cell(&ch.to_string(), 1);
        }
    }
    HistoryRows {
        session,
        snapshot,
        start_row,
        start_col,
        rows: (0..rows)
            .map(|row| HistoryRow {
                width: start_col.saturating_add(cols),
                cells: if row == 0 {
                    first.clone()
                } else {
                    vec![history_cell("", 1); usize::from(cols)]
                },
                wrapped: false,
            })
            .collect(),
    }
}

fn take_request(action: DashboardAction) -> ClientMessage {
    match action {
        DashboardAction::Request(request) => request,
        DashboardAction::RequestBatch(mut requests) if !requests.is_empty() => requests.remove(0),
        other => panic!("expected request, got {other:?}"),
    }
}

fn history_begin(dashboard: &mut Dashboard) -> ClientMessage {
    let request = take_request(dashboard.key(KeyCode::PageUp));
    assert!(matches!(
        request.request,
        Request::HistoryBegin {
            session: SessionId(1)
        }
    ));
    request
}

fn open_history(dashboard: &mut Dashboard, opened: HistoryOpened) -> Vec<ClientMessage> {
    let begin = history_begin(dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(opened),
    })
}

fn answer_pages(dashboard: &mut Dashboard, mut requests: Vec<ClientMessage>, marker: &str) {
    for _ in 0..64 {
        let Some(request) = requests.pop() else {
            return;
        };
        if !matches!(request.request, Request::HistoryPage { .. }) {
            continue;
        }
        let page = history_page_from_request(&request, marker);
        requests.extend(dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::HistoryRows(page),
        }));
    }
    panic!("history pages did not settle");
}

fn pump_frozen(
    dashboard: &mut Dashboard,
    frozen: &mut ovrcr_terminal::history::FrozenHistory,
    mut requests: Vec<ClientMessage>,
) {
    for _ in 0..512 {
        let Some(request) = requests.pop() else {
            return;
        };
        let Request::HistoryPage {
            start_row,
            rows,
            start_col,
            cols,
            ..
        } = request.request
        else {
            panic!("unexpected history request: {:?}", request.request);
        };
        let page = frozen
            .page(start_row, rows, start_col, cols)
            .expect("frozen history page");
        requests.extend(dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::HistoryRows(page),
        }));
    }
    panic!("frozen history pump did not settle");
}

fn pump_frozen_action(
    dashboard: &mut Dashboard,
    frozen: &mut ovrcr_terminal::history::FrozenHistory,
    action: DashboardAction,
) {
    let requests = match action {
        DashboardAction::Request(request) => vec![request],
        DashboardAction::RequestBatch(requests) => requests,
        _ => return,
    };
    pump_frozen(dashboard, frozen, requests);
}

fn numbered_frozen() -> ovrcr_terminal::history::FrozenHistory {
    let mut parser = ovrcr_terminal::vt100::Parser::new(4, 132, 512);
    for row in 0..400 {
        let suffix = if row + 1 == 400 { "" } else { "\r\n" };
        parser.process(format!("OLD_{row:03}{}{suffix}", "x".repeat(123)).as_bytes());
    }
    ovrcr_terminal::history::FrozenHistory::capture(
        SessionId(1),
        HistorySnapshotId(7),
        1,
        parser.screen().clone(),
    )
    .unwrap()
}

fn set_view_from(messages: &[ClientMessage]) -> ClientMessage {
    messages
        .iter()
        .find(|message| matches!(message.request, Request::SetView { .. }))
        .cloned()
        .expect("expected SetView")
}

fn screen_dirty(session: SessionId, revision: u64) -> ServerMessage {
    ServerMessage::Event(ServerEvent::ScreenDirty {
        session,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
    })
}

#[test]
fn resize_matching_screen_restores_snapshot_and_ignores_stale_screen() {
    let mut dashboard = dashboard_fixture();
    let session = dashboard.focused_session().unwrap();
    dashboard.install_area(Rect::new(0, 0, 80, 24));
    let resize = set_view_from(&dashboard.handle_server_message(screen_dirty(session, 0)));
    let Request::SetView { ref view } = resize.request else {
        panic!("expected SetView");
    };
    assert_eq!(view.focused, Some(session));
    assert!(
        view.panes
            .iter()
            .any(|pane| pane.session == session && pane.size == TerminalSize { rows: 20, cols: 40 })
    );

    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id.wrapping_sub(1),
        response: Response::Screen {
            session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: TerminalSize { rows: 20, cols: 40 },
            bytes: b"STALE_SCREEN".to_vec(),
        },
    });
    assert!(!draw_text(&dashboard, 80, 24).contains("STALE_SCREEN"));

    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Screen {
            session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: TerminalSize { rows: 20, cols: 40 },
            bytes: b"RESTORED_SCREEN".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Ok,
    });
    let rendered = draw_text(&dashboard, 80, 24);
    assert!(rendered.contains("RESTORED_SCREEN"));
    assert!(!rendered.contains("STALE_SCREEN"));
}

#[test]
fn browse_and_terminal_modes_keep_input_ownership_clear() {
    let mut dashboard = dashboard_fixture();
    assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Detach);
    let mut dashboard = dashboard_fixture();
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('q')),
        DashboardAction::PtyBytes(vec![b'q'])
    );
    assert_eq!(dashboard.ctrl('g'), DashboardAction::EnterBrowse);
    assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Detach);
}

#[test]
fn copy_mode_routes_keys_and_freezes_output() {
    let mut dashboard = copy_ready();
    let id = dashboard.focused_session().unwrap();
    assert_eq!(dashboard.key(KeyCode::Char('[')), DashboardAction::Redraw);
    assert!(footer(&dashboard, 120, 40).starts_with("COPY"));
    assert_eq!(dashboard.key(KeyCode::Home), DashboardAction::Redraw);
    assert_eq!(dashboard.key(KeyCode::Char('v')), DashboardAction::Redraw);
    assert_eq!(dashboard.key(KeyCode::Right), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('y')),
        DashboardAction::CopyText("ab".into())
    );
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        run: ovrcr_protocol::SessionRunId(1),
        revision: 0,
        bytes: b"\rNEW".to_vec(),
    }));
    assert!(!draw_text(&dashboard, 120, 40).contains("NEW"));
    assert_eq!(
        dashboard.key(KeyCode::Char('y')),
        DashboardAction::CopyText("ab".into())
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("secret".into())),
        DashboardAction::None
    );

    let mut pending_screen = unready_dashboard();
    assert_eq!(
        pending_screen.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    assert!(footer(&pending_screen, 120, 40).contains("Waiting for terminal screen"));
    assert_eq!(
        pending_screen.key(KeyCode::Char('q')),
        DashboardAction::Detach
    );

    let mut failed_selection = copy_ready();
    failed_selection.install_focus(SessionId(5));
    assert_eq!(
        failed_selection.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    assert!(footer(&failed_selection, 120, 40).contains("Waiting for terminal screen"));

    let mut release = copy_ready();
    assert_eq!(
        release.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )),
        DashboardAction::None
    );
    assert_eq!(release.key(KeyCode::Char('q')), DashboardAction::Detach);

    let mut repeat_motion = copy_ready();
    assert_eq!(
        repeat_motion.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    repeat_motion.key(KeyCode::Home);
    repeat_motion.key(KeyCode::Char('v'));
    assert_eq!(
        repeat_motion.key_action(KeyEvent::new_with_kind(
            KeyCode::Right,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        DashboardAction::Redraw
    );
    assert_eq!(
        repeat_motion.key(KeyCode::Char('y')),
        DashboardAction::CopyText("ab".into())
    );

    let mut press_only = copy_ready();
    press_only.key(KeyCode::Char('['));
    press_only.key(KeyCode::Home);
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        DashboardAction::None
    );
    assert_eq!(press_only.key(KeyCode::Char('y')), DashboardAction::Redraw);
    assert!(footer(&press_only, 120, 40).contains("Set an anchor with v"));
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Right,
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        DashboardAction::None
    );
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        DashboardAction::CopyText("ab".into())
    );

    let mut press_exit = copy_ready();
    press_exit.key(KeyCode::Char('['));
    assert_eq!(
        press_exit.key_action(KeyEvent::new_with_kind(
            KeyCode::Esc,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        DashboardAction::None
    );
    assert!(footer(&press_exit, 120, 40).starts_with("COPY"));
    assert_eq!(
        press_exit.key_action(KeyEvent::new_with_kind(
            KeyCode::Esc,
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    assert_eq!(press_exit.key(KeyCode::Char('q')), DashboardAction::Detach);
}

#[test]
fn matching_history_open_clears_resize_notice_but_stale_response_does_not() {
    let mut dashboard = copy_ready();
    let begin = take_request(dashboard.key(KeyCode::PageUp));
    let stale = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id + 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert!(stale.is_empty());
    assert!(!draw_text(&dashboard, 180, 40).contains("HISTORY · frozen"));

    let opened = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert!(!opened.is_empty());
    assert!(draw_text(&dashboard, 180, 40).contains("HISTORY"));
}

#[test]
fn copy_render_highlights_unicode_and_preserves_layout() {
    let mut dashboard = screen_ready("\x1b[?25lA界B".as_bytes());
    let area = Rect::new(0, 0, 88, 38);
    dashboard.install_area(area);
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Home);
    dashboard.key(KeyCode::Char('v'));
    dashboard.key(KeyCode::Right);
    let pane = focused_terminal(&dashboard, area);
    let mut terminal = Terminal::new(TestBackend::new(88, 38)).unwrap();
    terminal
        .draw(|frame| {
            draw_dashboard_at(frame, &dashboard, 0);
            let buffer = frame.buffer_mut();
            let base = Color::Rgb(30, 30, 46);
            let teal = Color::Rgb(148, 226, 213);
            assert_eq!(buffer[(pane.x + 1, pane.y)].fg, base);
            assert_eq!(buffer[(pane.x + 1, pane.y)].bg, teal);
            assert_eq!(buffer[(pane.x + 2, pane.y)].fg, base);
            assert_eq!(buffer[(pane.x + 2, pane.y)].bg, teal);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(pane.x, pane.y)].symbol(), "A");
    assert_eq!(buffer[(pane.x + 1, pane.y)].symbol(), "界");
    assert_eq!(buffer[(pane.x + 3, pane.y)].symbol(), "B");
}

#[test]
fn copy_render_tiny_pane_and_footer() {
    let mut tiny = copy_ready();
    tiny.key(KeyCode::Char('['));
    let mut tiny_terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
    tiny_terminal
        .draw(|frame| draw_dashboard_at(frame, &tiny, 0))
        .unwrap();

    let mut dashboard = copy_ready();
    let initial = draw_text(&dashboard, 120, 40);
    dashboard.key(KeyCode::Char('['));
    let id = dashboard.focused_session().unwrap();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        run: ovrcr_protocol::SessionRunId(1),
        revision: 0,
        bytes: b"\rLIVE".to_vec(),
    }));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let copy_footer = (0..120)
        .map(|col| buffer[(col, 39)].symbol())
        .collect::<String>();
    assert!(copy_footer.starts_with("COPY  ? Help  Esc Back"));
    assert!(
        ["h Move left", "j Move down", "k Move up", "l Move right"]
            .iter()
            .all(|hint| copy_footer.contains(hint))
    );
    assert!(copy_footer.contains("v Select  y Copy"));
    let rendered = draw_text(&dashboard, 120, 40);
    assert!(initial.contains("pid: 111  elapsed: 0m"));
    assert_eq!(buffer[(40, 3)].symbol(), "a");
    assert_eq!(buffer[(41, 3)].symbol(), "b");
    assert_eq!(buffer[(42, 3)].symbol(), "c");
    assert!(!rendered.contains("LIVE"));

    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 99,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "server failed".into(),
        },
    });
    assert_eq!(
        footer(&dashboard, 120, 40),
        "ERROR: Internal: server failed"
    );

    let mut entry_notice = unready_dashboard();
    assert_eq!(
        entry_notice.key(KeyCode::Char('[')),
        DashboardAction::Redraw
    );
    assert_eq!(
        footer(&entry_notice, 120, 40),
        "ERROR: Waiting for terminal screen"
    );
}

#[test]
fn copy_mode_cancels_at_identity_boundaries() {
    let mut dashboard = copy_ready();
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Esc);
    assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Detach);

    let mut dashboard = copy_ready();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        DashboardAction::Request(_)
    ));
    dashboard.key(KeyCode::Char('['));
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert!(matches!(
        release.as_slice(),
        [ClientMessage {
            request: Request::HistoryEnd { .. },
            ..
        }]
    ));
    assert!(footer(&dashboard, 120, 40).starts_with("COPY"));

    let mut dashboard = copy_ready();
    dashboard.key(KeyCode::PageUp);
    dashboard.key(KeyCode::Char('['));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "cancelled".into(),
        },
    });
    assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Redraw);
    assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Detach);

    let mut dashboard = copy_ready();
    dashboard.key(KeyCode::Char('['));
    dashboard.install_focus(SessionId(5));
    assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Detach);

    for key in [KeyCode::Char('q'), KeyCode::Char('\u{7}')] {
        let mut dashboard = copy_ready();
        dashboard.key(KeyCode::Char('['));
        assert_eq!(dashboard.key(key), DashboardAction::Redraw);
        assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Detach);
    }
    let mut dashboard = copy_ready();
    dashboard.key(KeyCode::Char('['));
    assert_eq!(dashboard.ctrl('g'), DashboardAction::Redraw);
    assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Detach);

    let mut dashboard = copy_ready();
    dashboard.key(KeyCode::Char('['));
    let mut removed = fixture_hierarchy();
    removed.projects[1].workspaces[1]
        .sessions
        .retain(|session| session.id != SessionId(1));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(removed)));
    assert!(!footer(&dashboard, 120, 40).starts_with("COPY"));
}

#[test]
fn copy_mode_waits_for_matching_screen() {
    let mut dashboard = dashboard_fixture();
    let first = dashboard.focused_session().unwrap();
    dashboard.install_focus(SessionId(5));
    let pending = dashboard
        .request_view_at(Rect::new(0, 0, 88, 38))
        .expect("expected focus SetView");
    dashboard.drain_outbox();
    let Request::SetView { ref view } = pending.request else {
        panic!("expected SetView");
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::Screen {
            session: first,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: view.panes[0].size,
            bytes: b"late-first".to_vec(),
        },
    });
    assert!(!draw_text(&dashboard, 120, 40).contains("late-first"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id.wrapping_sub(1),
        response: Response::Screen {
            session: SessionId(5),
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: view.panes[0].size,
            bytes: b"old-request".to_vec(),
        },
    });
    assert!(!draw_text(&dashboard, 120, 40).contains("old-request"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::Screen {
            session: SessionId(5),
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: view.panes[0].size,
            bytes: b"matching".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::Ok,
    });
    assert!(draw_text(&dashboard, 120, 40).contains("matching"));
    assert_eq!(dashboard.key(KeyCode::Char('[')), DashboardAction::Redraw);
    assert!(footer(&dashboard, 120, 40).starts_with("COPY"));
}

#[test]
fn copy_mode_keeps_alternate_and_resync_snapshots() {
    let mut dashboard = dashboard_fixture();
    let id = dashboard.focused_session().unwrap();
    dashboard.install_screen(id, b"\x1b[?1049hALT");
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Home);
    dashboard.key(KeyCode::Char('v'));
    dashboard.key(KeyCode::Right);
    let requests = dashboard.handle_server_message(screen_dirty(id, 0));
    let view_request = set_view_from(&requests);
    let Request::SetView { ref view } = view_request.request else {
        panic!("expected SetView");
    };
    assert_eq!(view.focused, Some(id));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: view_request.request_id,
        response: Response::Screen {
            session: id,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: view.panes[0].size,
            bytes: b"\x1b[?1049lPRIMARY".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: view_request.request_id,
        response: Response::Ok,
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        run: ovrcr_protocol::SessionRunId(1),
        revision: view.revision,
        bytes: b"-LIVE".to_vec(),
    }));
    assert_eq!(
        dashboard.key(KeyCode::Char('y')),
        DashboardAction::CopyText("AL".into())
    );
    assert!(footer(&dashboard, 120, 40).starts_with("COPY"));
    dashboard.key(KeyCode::Esc);
    assert!(draw_text(&dashboard, 120, 40).contains("PRIMARY-LIVE"));
}

#[test]
fn pause_resume_browse_keys_send_explicit_requests() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(5));
    dashboard.install_screen(SessionId(5), &[]);

    assert_eq!(
        dashboard.key(KeyCode::Char('p')),
        DashboardAction::Request(ClientMessage {
            request_id: 1,
            request: Request::PauseSession {
                session: SessionId(5),
            },
        })
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('p')),
        DashboardAction::Request(ClientMessage {
            request_id: 2,
            request: Request::PauseSession {
                session: SessionId(5),
            },
        })
    );

    let mut paused = session_summary(5, "consigint", "auth", "local", "zsh", Some(555), 0);
    paused.phase = SessionPhase::Paused;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        paused,
    ))));
    assert_eq!(
        dashboard.key(KeyCode::Char('r')),
        DashboardAction::Request(ClientMessage {
            request_id: 3,
            request: Request::ResumeSession {
                session: SessionId(5),
            },
        })
    );

    dashboard.install_focus(SessionId(2));
    assert_eq!(dashboard.key(KeyCode::Char('p')), DashboardAction::Redraw);
    assert!(
        dashboard
            .drain_outbox()
            .iter()
            .all(|message| { !matches!(message.request, Request::PauseSession { .. }) }),
        "an exited terminal must not receive a pause request"
    );

    dashboard.install_focus(SessionId(5));
    dashboard.install_screen(SessionId(5), &[]);
    let mut running = session_summary(5, "consigint", "auth", "local", "zsh", Some(555), 0);
    running.phase = SessionPhase::Running;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        running,
    ))));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('p')),
        DashboardAction::PtyBytes(vec![b'p'])
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('r')),
        DashboardAction::PtyBytes(vec![b'r'])
    );
}

#[test]
fn pause_resume_input_and_paste_stay_guarded() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(5));
    dashboard.install_screen(SessionId(5), &[]);
    let mut paused = session_summary(5, "consigint", "auth", "local", "zsh", Some(555), 0);
    paused.phase = SessionPhase::Paused;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        paused.clone(),
    ))));

    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        footer(&dashboard, 120, 40),
        "ERROR: Session paused; press r to resume"
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("blocked".into())),
        DashboardAction::None
    );

    let mut running = paused.clone();
    running.phase = SessionPhase::Running;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        running,
    ))));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        paused,
    ))));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(footer(&dashboard, 120, 40).contains("Session paused"));
    assert!(!matches!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(_)
    ));
}

#[test]
fn history_navigation_never_writes_to_pty() {
    let mut dashboard = dashboard_fixture();
    let begin = dashboard.key(KeyCode::PageUp);
    assert_eq!(
        begin,
        DashboardAction::Request(ClientMessage {
            request_id: 1,
            request: Request::HistoryBegin {
                session: SessionId(1),
            },
        })
    );
    assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Detach);

    let mut terminal = dashboard_fixture();
    assert_eq!(terminal.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        terminal.key(KeyCode::PageUp),
        DashboardAction::PtyBytes(b"\x1b[5~".to_vec())
    );

    let mut dashboard = dashboard_fixture();
    let begin = history_begin(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert!(draw_text(&dashboard, 180, 40).contains("HISTORY"));
    for key in [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Char('k'),
        KeyCode::Char('j'),
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Char('h'),
        KeyCode::Char('l'),
        KeyCode::Enter,
    ] {
        assert!(!matches!(dashboard.key(key), DashboardAction::PtyBytes(_)));
    }
    assert!(!matches!(
        dashboard.event_action(Event::Paste("blocked".into())),
        DashboardAction::PtyBytes(_)
    ));
    assert_eq!(
        dashboard.key(KeyCode::Char('q')),
        DashboardAction::EnterBrowse
    );
    assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Detach);
}

#[test]
fn history_begin_is_cancelled_and_captured_by_active_overlays() {
    for overlay in ["palette", "tasks"] {
        let mut dashboard = dashboard_fixture();
        assert!(matches!(
            dashboard.key(KeyCode::PageUp),
            DashboardAction::Request(ClientMessage {
                request: Request::HistoryBegin {
                    session: SessionId(1)
                },
                ..
            })
        ));
        if overlay == "palette" {
            assert!(matches!(
                dashboard.key(KeyCode::Char(':')),
                DashboardAction::Request(ClientMessage {
                    request: Request::Inspect,
                    ..
                })
            ));
        } else {
            assert_eq!(dashboard.ctrl('t'), DashboardAction::Redraw);
        }
        assert_eq!(dashboard.key(KeyCode::PageUp), DashboardAction::Redraw);
        assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
        let requests = dashboard.handle_server_message(ServerMessage::Response {
            request_id: 1,
            response: Response::HistoryOpened(history_opened(100)),
        });
        assert!(!draw_text(&dashboard, 180, 40).contains("HISTORY · frozen"));
        assert!(matches!(
            requests.as_slice(),
            [ClientMessage {
                request: Request::HistoryEnd {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(7),
                },
                ..
            }]
        ));
    }
}

#[test]
fn history_release_precedes_hierarchy_fallback_selection() {
    let mut dashboard = dashboard_fixture();
    open_history(&mut dashboard, history_opened(100));
    let mut hierarchy = fixture_hierarchy();
    for workspace in hierarchy
        .projects
        .iter_mut()
        .flat_map(|project| &mut project.workspaces)
    {
        workspace
            .sessions
            .retain(|session| session.id != SessionId(1));
    }
    let requests = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::HierarchyChanged(hierarchy),
    ));
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    assert!(!draw_text(&dashboard, 180, 40).contains("HISTORY · frozen"));
    assert!(matches!(
        requests.as_slice(),
        [
            ClientMessage {
                request: Request::HistoryEnd {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(7),
                },
                ..
            },
            ClientMessage {
                request: Request::SetView { view },
                ..
            }
        ] if view.focused == Some(SessionId(5))
            && view.panes.iter().any(|pane| pane.session == SessionId(5))
    ));
}

#[test]
fn history_live_output_preserves_anchor() {
    let mut dashboard = screen_ready(b"");
    let requests = open_history(&mut dashboard, history_opened(100));
    let page_request = requests.first().cloned().expect("initial history page");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(history_page_from_request(&page_request, "frozen")),
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        run: ovrcr_protocol::SessionRunId(1),
        revision: 0,
        bytes: b"LIVE_MARKER".to_vec(),
    }));
    let rendered = draw_text(&dashboard, 180, 40);
    assert!(rendered.contains("HISTORY · frozen"));
    assert!(rendered.contains("frozen"));
    assert!(!rendered.contains("LIVE_MARKER"));

    let select = dashboard.handle_server_message(screen_dirty(SessionId(1), 0));
    let view_request = set_view_from(&select);
    let Request::SetView { ref view } = view_request.request else {
        panic!("expected SetView");
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: view_request.request_id,
        response: Response::Screen {
            session: SessionId(1),
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: view.panes[0].size,
            bytes: b"SCREEN_MARKER".to_vec(),
        },
    });
    let after = draw_text(&dashboard, 180, 40);
    assert!(after.contains("HISTORY · frozen"));
    assert!(after.contains("frozen"));
    assert!(!after.contains("SCREEN_MARKER"));
}

#[test]
fn history_resize_preserves_capture() {
    let mut dashboard = dashboard_fixture();
    let requests = open_history(&mut dashboard, history_opened(100));
    answer_pages(&mut dashboard, requests, "old");
    assert!(draw_text(&dashboard, 180, 40).contains("HISTORY · frozen"));
    dashboard.install_area(Rect::new(0, 0, 70, 54));
    let resize = set_view_from(&dashboard.handle_server_message(screen_dirty(SessionId(1), 0)));
    acknowledge_all_view_targets(&mut dashboard, resize);
    assert!(draw_text(&dashboard, 180, 54).contains("HISTORY · frozen"));
    assert!(draw_text(&dashboard, 180, 54).contains("old"));
}

#[test]
fn history_stale_response_is_ignored() {
    let mut dashboard = dashboard_fixture();
    let requests = open_history(&mut dashboard, history_opened(100));
    let page_request = requests.first().cloned().expect("history page pending");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id.wrapping_add(1),
        response: Response::HistoryRows(history_page_from_request(&page_request, "stale")),
    });
    assert!(!draw_text(&dashboard, 180, 40).contains("stale"));

    let mut wrong_session = history_page_from_request(&page_request, "wrong");
    wrong_session.session = SessionId(5);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(wrong_session),
    });
    assert!(!draw_text(&dashboard, 180, 40).contains("wrong"));

    dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(history_page_from_request(&page_request, "tile")),
    });
    assert!(draw_text(&dashboard, 180, 40).contains("tile"));

    dashboard.install_focus(SessionId(5));
    assert!(!draw_text(&dashboard, 180, 40).contains("HISTORY · frozen"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(history_page_from_request(&page_request, "old")),
    });
    assert!(!draw_text(&dashboard, 180, 40).contains("old"));
}

#[test]
fn history_empty_rows_only_anchor_after_explicit_anchor() {
    let mut dashboard = dashboard_fixture();
    let requests = open_history(&mut dashboard, history_opened(1));
    let page_request = requests.first().cloned().expect("empty history page");
    let Request::HistoryPage {
        session,
        snapshot,
        start_row,
        start_col,
        rows,
        ..
    } = page_request.request
    else {
        panic!("expected HistoryPage");
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(HistoryRows {
            session,
            snapshot,
            start_row,
            start_col,
            rows: (0..rows)
                .map(|_| HistoryRow {
                    width: 0,
                    cells: Vec::new(),
                    wrapped: false,
                })
                .collect(),
        }),
    });
    assert_eq!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    assert_eq!(footer(&dashboard, 120, 40), "Nothing to select on this row");
}

#[test]
fn history_key_kinds_gate_actions() {
    let mut dashboard = dashboard_fixture();
    open_history(&mut dashboard, history_opened(100));
    let before = draw_text(&dashboard, 180, 40);
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Enter,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        DashboardAction::None
    ));
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Down,
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )),
        DashboardAction::None
    ));
    assert_eq!(draw_text(&dashboard, 180, 40), before);
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Esc,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        DashboardAction::None
    ));
    assert!(draw_text(&dashboard, 180, 40).contains("HISTORY"));
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Esc,
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        DashboardAction::EnterBrowse
    ));
}

#[test]
fn history_pending_keys_coalesce() {
    let mut dashboard = dashboard_fixture();
    let requests = open_history(&mut dashboard, history_opened(400));
    let page_request = requests.first().cloned().expect("initial history page");
    assert!(matches!(
        dashboard.event_action(Event::Paste("paste while browsing history".into())),
        DashboardAction::None
    ));
    for _ in 0..100 {
        assert!(!matches!(
            dashboard.key(KeyCode::PageUp),
            DashboardAction::Request(_)
        ));
    }
    let follow_up = dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(history_page_from_request(&page_request, "tile")),
    });
    assert_eq!(follow_up.len(), 1);
    let retry_id = follow_up[0].request_id;
    let blocked = dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "history tile missing".into(),
        },
    });
    assert!(blocked.is_empty());
    assert!(footer(&dashboard, 120, 40).contains("missing"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry_id,
        response: Response::Ok,
    });
    assert!(footer(&dashboard, 120, 40).contains("missing"));
    assert!(matches!(
        dashboard.key(KeyCode::Down),
        DashboardAction::Request(_)
    ));
    assert!(!footer(&dashboard, 120, 40).contains("missing"));
}

#[test]
fn history_unanchored_retry_keys_reissue_failed_cursor_request() {
    for malformed in [false, true] {
        for retry_key in [KeyCode::Char('v'), KeyCode::Char('y')] {
            let mut dashboard = dashboard_fixture();
            let requests = open_history(&mut dashboard, history_opened(40));
            let failed = requests.first().cloned().expect("cursor page request");
            let failure_response = if malformed {
                let Request::HistoryPage {
                    session,
                    snapshot,
                    start_row,
                    start_col,
                    ..
                } = failed.request
                else {
                    panic!("unexpected cursor request");
                };
                Response::HistoryRows(HistoryRows {
                    session,
                    snapshot,
                    start_row,
                    start_col,
                    rows: vec![HistoryRow {
                        width: 20,
                        cells: Vec::new(),
                        wrapped: false,
                    }],
                })
            } else {
                Response::Error {
                    code: ErrorCode::Internal,
                    message: "cursor page unavailable".into(),
                }
            };
            assert!(
                dashboard
                    .handle_server_message(ServerMessage::Response {
                        request_id: failed.request_id,
                        response: failure_response,
                    })
                    .is_empty()
            );
            let retry = match dashboard.key_action(KeyEvent::new_with_kind(
                retry_key,
                KeyModifiers::NONE,
                KeyEventKind::Press,
            )) {
                DashboardAction::Request(request) => request,
                action => {
                    panic!("expected explicit cursor retry for {retry_key:?}, got {action:?}")
                }
            };
            assert!(matches!(retry.request, Request::HistoryPage { .. }));
            let follow_up = dashboard.handle_server_message(ServerMessage::Response {
                request_id: retry.request_id,
                response: Response::HistoryRows(history_page_from_request(&retry, "resolved")),
            });
            assert!(
                follow_up
                    .iter()
                    .all(|request| matches!(request.request, Request::HistoryPage { .. }))
            );
            assert!(draw_text(&dashboard, 180, 40).contains("HISTORY"));
        }
    }
}

#[test]
fn copy_history_range_survives_live_eviction() {
    let mut frozen = numbered_frozen();
    let opened = frozen.opened().clone();
    let mut dashboard = screen_ready(b"");
    let requests = open_history(&mut dashboard, opened.clone());
    pump_frozen(&mut dashboard, &mut frozen, requests);
    let home = dashboard.key(KeyCode::Home);
    pump_frozen_action(&mut dashboard, &mut frozen, home);
    let anchor = dashboard.key(KeyCode::Char('v'));
    pump_frozen_action(&mut dashboard, &mut frozen, anchor);
    let down = dashboard.key(KeyCode::Down);
    pump_frozen_action(&mut dashboard, &mut frozen, down);
    let right = dashboard.key(KeyCode::Right);
    pump_frozen_action(&mut dashboard, &mut frozen, right);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        run: ovrcr_protocol::SessionRunId(1),
        revision: 0,
        bytes: b"LIVE_MARKER\r\n".to_vec(),
    }));
    let rendered = draw_text(&dashboard, 180, 40);
    assert!(rendered.contains("HISTORY · frozen"));
    assert!(!rendered.contains("LIVE_MARKER"));
    let yank = dashboard.key(KeyCode::Char('y'));
    assert!(matches!(
        yank,
        DashboardAction::Request(ClientMessage {
            request: Request::HistoryPage { .. },
            ..
        })
    ));
    assert!(draw_text(&dashboard, 180, 40).contains("copying"));
}

#[test]
fn copy_history_invalid_snapshot_never_emits_partial_text() {
    for code in [ErrorCode::Conflict, ErrorCode::NotFound] {
        let mut dashboard = dashboard_fixture();
        let requests = open_history(&mut dashboard, history_opened(2));
        answer_pages(&mut dashboard, requests, "A");
        dashboard.key(KeyCode::Char('v'));
        let yank = take_request(dashboard.key(KeyCode::Char('y')));
        let release = dashboard.handle_server_message(ServerMessage::Response {
            request_id: yank.request_id,
            response: Response::Error {
                code,
                message: "snapshot unavailable".into(),
            },
        });
        assert!(!draw_text(&dashboard, 180, 40).contains("HISTORY · frozen"));
        assert!(matches!(
            release.as_slice(),
            [ClientMessage {
                request: Request::HistoryEnd {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(7),
                },
                ..
            }]
        ));
        assert_eq!(dashboard.key(KeyCode::Char('q')), DashboardAction::Detach);
    }

    let mut internal = dashboard_fixture();
    let requests = open_history(&mut internal, history_opened(2));
    answer_pages(&mut internal, requests, "A");
    internal.key(KeyCode::Char('v'));
    let yank = take_request(internal.key(KeyCode::Char('y')));
    let internal_release = internal.handle_server_message(ServerMessage::Response {
        request_id: yank.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "history backend unavailable".into(),
        },
    });
    assert!(internal_release.is_empty());
    assert!(draw_text(&internal, 180, 40).contains("HISTORY"));
    let retry = take_request(internal.key(KeyCode::Char('y')));
    assert!(matches!(retry.request, Request::HistoryPage { .. }));
}

#[test]
fn history_render_preserves_cells_and_clips() {
    let mut dashboard = dashboard_fixture();
    let requests = open_history(&mut dashboard, history_opened(1));
    let page_request = requests.first().cloned().expect("history page");
    let Request::HistoryPage {
        session,
        snapshot,
        start_row,
        start_col,
        rows,
        cols,
        ..
    } = page_request.request
    else {
        panic!("expected HistoryPage");
    };
    let mut first = vec![history_cell("", 1); usize::from(cols).max(4)];
    first[0] = HistoryCell {
        text: "界".into(),
        width: 2,
        fg: HistoryColor::Rgb(1, 2, 3),
        bg: HistoryColor::Default,
        attributes: 0,
    };
    first[1] = history_cell("", 0);
    first[2] = history_cell("e\u{301}", 1);
    first[3] = history_cell("X", 1);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(HistoryRows {
            session,
            snapshot,
            start_row,
            start_col,
            rows: (0..rows)
                .map(|row| HistoryRow {
                    width: start_col.saturating_add(cols),
                    cells: if row == 0 {
                        first.clone()
                    } else {
                        vec![history_cell("", 1); usize::from(cols)]
                    },
                    wrapped: false,
                })
                .collect(),
        }),
    });
    let area = Rect::new(0, 0, 88, 38);
    let pane = focused_terminal(&dashboard, area);
    let mut terminal = Terminal::new(TestBackend::new(88, 38)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(pane.x, pane.y)].symbol(), "界");
    assert_eq!(buffer[(pane.x, pane.y)].fg, Color::Rgb(1, 2, 3));
    assert_eq!(buffer[(pane.x + 2, pane.y)].symbol(), "e\u{301}");
    assert_eq!(buffer[(pane.x + 3, pane.y)].symbol(), "X");
    let text = draw_text(&dashboard, 180, 40);
    assert!(text.contains("HISTORY · frozen · loaded"));

    let mut loading = dashboard_fixture();
    open_history(&mut loading, history_opened(100));
    assert!(draw_text(&loading, 180, 40).contains("loading"));
}

#[test]
fn history_cancelled_begin_releases_late_snapshot() {
    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        DashboardAction::Request(_)
    ));
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::EnterBrowse);
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert!(!draw_text(&dashboard, 180, 40).contains("HISTORY · frozen"));
    assert_eq!(release.len(), 1);
    assert!(matches!(
        release[0].request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    ));
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        DashboardAction::Request(ClientMessage {
            request: Request::HistoryBegin { .. },
            ..
        })
    ));

    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        DashboardAction::Request(_)
    ));
    assert_eq!(dashboard.ctrl('g'), DashboardAction::EnterBrowse);
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert!(matches!(
        release[0].request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    ));

    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        DashboardAction::Request(_)
    ));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
    assert_eq!(dashboard.ctrl('g'), DashboardAction::EnterBrowse);
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert!(matches!(
        release[0].request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    ));
}

#[test]
fn pause_resume_dense_status_has_priority() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(5));
    dashboard.install_screen(SessionId(5), &[]);
    let mut paused = session_summary(5, "consigint", "auth", "local", "zsh", Some(555), 0);
    paused.phase = SessionPhase::Paused;
    paused.activity = AgentActivity::Busy;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        paused,
    ))));

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y| (0..39).map(|x| buffer[(x, y)].symbol()).collect::<String>();
    assert_eq!(row(3).trim_end(), "▌    P local");
    let rendered = draw_text(&dashboard, 120, 40);
    assert!(rendered.contains("pid: 555  elapsed: 0m  agent busy  paused"));
    assert!(rendered.lines().last().unwrap().contains(" r "));
    assert!(
        !rendered
            .lines()
            .last()
            .unwrap()
            .split_whitespace()
            .any(|key| key == "p")
    );
    assert!(
        rendered
            .lines()
            .last()
            .unwrap()
            .split_whitespace()
            .any(|key| key == "q")
    );
    assert!(
        rendered
            .lines()
            .last()
            .unwrap()
            .split_whitespace()
            .any(|key| key == "t")
    );

    let mut narrow = Terminal::new(TestBackend::new(40, 20)).unwrap();
    narrow
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let footer = (0..40)
        .map(|x| narrow.backend().buffer()[(x, 19)].symbol())
        .collect::<String>();
    assert!(
        footer.split_whitespace().any(|key| key == "r"),
        "narrow footer was {footer:?}"
    );
}

#[test]
fn history_waits_for_coalesced_focus_view_before_capture() {
    let mut dashboard = dashboard_fixture();
    let split = dashboard.key(KeyCode::Char('v'));
    pump_view(&mut dashboard, split);
    assert_eq!(dashboard.key(KeyCode::PageUp), DashboardAction::Redraw);
    assert!(footer(&dashboard, 120, 40).contains("Pane is loading; retry history"));
    let captured_session = dashboard.focused_session().unwrap();
    let view = dashboard
        .request_view_at(Rect::new(0, 0, 88, 38))
        .expect("expected split SetView");
    dashboard.drain_outbox();
    acknowledge_all_view_targets(&mut dashboard, view);
    let DashboardAction::Request(begin) = dashboard.key(KeyCode::PageUp) else {
        panic!("history must open after the committed focus is acknowledged");
    };
    assert_eq!(
        begin.request,
        Request::HistoryBegin {
            session: captured_session
        }
    );
}

#[test]
fn ux_history_footer_names_the_actual_escape_action() {
    let mut dashboard = dashboard_fixture();
    let requests = open_history(&mut dashboard, history_opened(2));
    answer_pages(&mut dashboard, requests, "AB");
    dashboard.key(KeyCode::Char('v'));
    let yank = dashboard.key(KeyCode::Char('y'));
    assert!(matches!(
        yank,
        DashboardAction::Request(ClientMessage {
            request: Request::HistoryPage { .. },
            ..
        })
    ));
    for width in [40, 80, 120] {
        let text = footer(&dashboard, width, 24);
        assert!(text.contains("Esc Cancel copy"), "{text}");
        assert!(!text.contains("y Copy"), "{text}");
    }
    dashboard.key(KeyCode::Esc);
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::EnterBrowse);

    let mut copy = copy_ready();
    copy.key(KeyCode::Char('['));
    let text = footer(&copy, 120, 24);
    for label in ["COPY", "Esc Back", "v Select", "y Copy"] {
        assert!(text.contains(label), "{text}");
    }
}

#[test]
fn copy_mouse_drag_normalizes_wide_cells_and_visible_copy_close_capture_input() {
    let mut d = screen_ready("a界z".as_bytes());
    d.key(KeyCode::Char('['));
    let area = Rect::new(0, 0, 88, 38);
    d.install_area(area);
    let pane = focused_terminal(&d, area);
    d.mouse_action(
        mouse_event(MouseEventKind::Down(MouseButton::Left), pane.x + 2, pane.y),
        area,
    );
    d.mouse_action(
        mouse_event(MouseEventKind::Drag(MouseButton::Left), pane.x + 3, pane.y),
        area,
    );
    d.mouse_action(
        mouse_event(MouseEventKind::Up(MouseButton::Left), pane.x + 3, pane.y),
        area,
    );
    assert_eq!(
        d.key(KeyCode::Char('y')),
        DashboardAction::CopyText("界z".into())
    );
    let action = d.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            area.right() - 16,
            area.y,
        ),
        area,
    );
    assert_eq!(action, DashboardAction::CopyText("界z".into()));
    assert_eq!(d.focused_session(), Some(SessionId(1)));
    d.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            area.right() - 6,
            area.y,
        ),
        area,
    );
    assert_eq!(d.key(KeyCode::Char('q')), DashboardAction::Detach);
}

#[test]
fn history_mouse_drag_uses_frozen_offsets_and_cancels_stale_completion() {
    let mut frozen = numbered_frozen();
    let mut d = screen_ready(b"");
    let requests = open_history(&mut d, frozen.opened().clone());
    pump_frozen(&mut d, &mut frozen, requests);
    let area = Rect::new(0, 0, 88, 38);
    d.install_area(area);
    let pane = focused_terminal(&d, area);
    d.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            pane.x + 1,
            pane.y + 1,
        ),
        area,
    );
    d.mouse_action(
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            pane.x + 4,
            pane.y + 2,
        ),
        area,
    );
    d.mouse_action(
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            pane.x + 4,
            pane.y + 2,
        ),
        area,
    );
    let action = d.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            area.right() - 16,
            area.y,
        ),
        area,
    );
    if let DashboardAction::Request(request) = action {
        pump_frozen(&mut d, &mut frozen, vec![request]);
    }
    assert!(draw_text(&d, 180, 40).contains("copying"));
    d.mouse_action(
        mouse_event(MouseEventKind::Down(MouseButton::Left), pane.x + 3, pane.y),
        area,
    );
    assert!(!draw_text(&d, 180, 40).contains("copying"));
    d.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            area.right() - 6,
            area.y,
        ),
        area,
    );
    assert_eq!(d.key(KeyCode::Char('q')), DashboardAction::Detach);
}

#[test]
fn copy_mouse_release_position_and_focus_loss_bound_the_gesture() {
    let mut d = screen_ready(b"abcdef");
    d.key(KeyCode::Char('['));
    let area = Rect::new(0, 0, 88, 38);
    d.install_area(area);
    let pane = focused_terminal(&d, area);
    d.mouse_action(
        mouse_event(MouseEventKind::Down(MouseButton::Left), pane.x, pane.y),
        area,
    );
    d.mouse_action(
        mouse_event(MouseEventKind::Up(MouseButton::Left), pane.x + 2, pane.y),
        area,
    );
    assert_eq!(
        d.key(KeyCode::Char('y')),
        DashboardAction::CopyText("abc".into())
    );
    d.mouse_action(
        mouse_event(MouseEventKind::Drag(MouseButton::Left), pane.x + 5, pane.y),
        area,
    );
    assert_eq!(
        d.key(KeyCode::Char('y')),
        DashboardAction::CopyText("abc".into())
    );
    d.mouse_action(
        mouse_event(MouseEventKind::Down(MouseButton::Left), pane.x + 1, pane.y),
        area,
    );
    d.event_action(Event::FocusLost);
    d.event_action(Event::FocusGained);
    d.mouse_action(
        mouse_event(MouseEventKind::Drag(MouseButton::Left), pane.x + 5, pane.y),
        area,
    );
    assert_eq!(
        d.key(KeyCode::Char('y')),
        DashboardAction::CopyText("b".into())
    );
}

#[test]
fn whichkey_acquisition_cancels_copy_and_history_drags() {
    let mut copy = screen_ready(b"abcdef");
    copy.key(KeyCode::Char('['));
    let copy_area = Rect::new(0, 0, 88, 38);
    copy.install_area(copy_area);
    let copy_pane = focused_terminal(&copy, copy_area);
    copy.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            copy_pane.x,
            copy_pane.y,
        ),
        copy_area,
    );
    copy.event_action(Event::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE,
    )));
    copy.event_action(Event::Mouse(mouse_event(
        MouseEventKind::Up(MouseButton::Left),
        copy_pane.x + 5,
        copy_pane.y,
    )));
    copy.key(KeyCode::Esc);
    copy.mouse_action(
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            copy_pane.x + 5,
            copy_pane.y,
        ),
        copy_area,
    );
    assert_eq!(
        copy.key(KeyCode::Char('y')),
        DashboardAction::CopyText("a".into())
    );

    let mut frozen = numbered_frozen();
    let mut history = screen_ready(b"");
    let requests = open_history(&mut history, frozen.opened().clone());
    pump_frozen(&mut history, &mut frozen, requests);
    let history_area = Rect::new(0, 0, 88, 38);
    history.install_area(history_area);
    let history_pane = focused_terminal(&history, history_area);
    history.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            history_pane.x + 1,
            history_pane.y + 1,
        ),
        history_area,
    );
    history.event_action(Event::Key(KeyEvent::new(
        KeyCode::Char('?'),
        KeyModifiers::NONE,
    )));
    history.event_action(Event::Mouse(mouse_event(
        MouseEventKind::Up(MouseButton::Left),
        history_pane.x + 4,
        history_pane.y + 2,
    )));
    history.key(KeyCode::Esc);
    history.mouse_action(
        mouse_event(
            MouseEventKind::Drag(MouseButton::Left),
            history_pane.x + 4,
            history_pane.y + 2,
        ),
        history_area,
    );
    assert!(
        footer(&history, 88, 38).contains("HISTORY"),
        "cancelled drag must leave history mode"
    );
    let yank = history.key(KeyCode::Char('y'));
    assert!(
        !matches!(yank, DashboardAction::PtyBytes(_)),
        "history yank must not type into the PTY, got {yank:?}"
    );
}

#[test]
fn history_mouse_bounds_match_offset_painted_cells_and_resize_keeps_capture() {
    let mut d = screen_ready(b"");
    let area = Rect::new(0, 0, 88, 38);
    d.install_area(area);
    let requests = open_history(&mut d, history_opened(100));
    answer_pages(&mut d, requests, "xxxxx");
    let pane = focused_terminal(&d, area);
    d.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            pane.x + 2,
            pane.y + 1,
        ),
        area,
    );
    d.mouse_action(
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            pane.x + 3,
            pane.y + 2,
        ),
        area,
    );
    let yank = d.key(KeyCode::Char('y'));
    assert!(matches!(
        yank,
        DashboardAction::Request(ClientMessage {
            request: Request::HistoryPage { .. },
            ..
        }) | DashboardAction::Redraw
    ));
    for (x, y) in [
        (pane.x, pane.y.saturating_sub(1)),
        (pane.x, pane.y.saturating_add(pane.height)),
    ] {
        assert_eq!(
            d.mouse_action(
                mouse_event(MouseEventKind::Down(MouseButton::Left), x, y),
                area
            ),
            DashboardAction::None
        );
    }
    d.install_area(Rect::new(0, 0, 100, 20));
    assert!(draw_text(&d, 180, 40).contains("HISTORY · frozen"));
    d.install_focus(SessionId(5));
    assert!(!draw_text(&d, 180, 40).contains("HISTORY · frozen"));
}

#[test]
fn history_copy_joins_soft_wrap_without_hard_newline() {
    let mut dashboard = screen_ready(b"");
    let opened = HistoryOpened {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        revision: 1,
        size: TerminalSize { rows: 4, cols: 8 },
        history_rows: 0,
        total_rows: 2,
    };
    let requests = open_history(&mut dashboard, opened);
    for request in requests {
        let Request::HistoryPage {
            session,
            snapshot,
            start_row,
            start_col,
            rows,
            cols,
            ..
        } = request.request
        else {
            continue;
        };
        let wrapped = HistoryRow {
            width: start_col.saturating_add(cols),
            cells: {
                let mut cells = vec![history_cell("x", 1); usize::from(cols)];
                if start_col == 0 && cols > 1 {
                    cells[0] = history_cell("界", 2);
                    if cells.len() > 1 {
                        cells[1] = history_cell("", 0);
                    }
                }
                cells
            },
            wrapped: start_row == 0,
        };
        let next = HistoryRow {
            width: start_col.saturating_add(cols),
            cells: vec![history_cell("y", 1); usize::from(cols)],
            wrapped: false,
        };
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::HistoryRows(HistoryRows {
                session,
                snapshot,
                start_row,
                start_col,
                rows: vec![wrapped, next]
                    .into_iter()
                    .take(usize::from(rows))
                    .collect(),
            }),
        });
    }
    dashboard.key(KeyCode::Home);
    dashboard.key(KeyCode::Char('v'));
    dashboard.key(KeyCode::Down);
    match dashboard.key(KeyCode::Char('y')) {
        DashboardAction::CopyText(text) => {
            assert!(text.contains('界'), "{text:?}");
            assert!(
                !text.contains("界\n"),
                "soft wrap must not insert a hard newline: {text:?}"
            );
        }
        DashboardAction::Request(request) => {
            assert!(matches!(request.request, Request::HistoryPage { .. }));
        }
        other => panic!("expected CopyText or HistoryPage, got {other:?}"),
    }
}

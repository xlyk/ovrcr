use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr::context::{ContextSource, ContextUsageReport, ContextUsageSnapshot};
use ovrcr::protocol::{
    ClientMessage, ErrorCode, HierarchySnapshot, HistoryCell, HistoryColor, HistoryOpened,
    HistoryRow, HistoryRows, HistorySnapshotId, ProjectSummary, Request, Response, ServerEvent,
    ServerMessage, WorkspaceSummary,
};
use ovrcr::session::{AgentActivity, SessionId, SessionPhase, SessionSummary, TerminalSize};
use ovrcr::tui::{
    CopyPoint, CopySelection, DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, HistoryCopyCompletion,
    HistoryCopyJob, HistoryCopyPoint, HistoryCopyRange, HistoryCursor, HistoryView, KeyEncoding,
    append_history_selection, dashboard_message_channel, draw_dashboard_at, encode_key,
    encode_mouse, encode_paste, event_to_request, pane_rects, render_copy, write_clipboard,
};
use ovrcr_terminal::{history::FrozenHistory, vt100};
use ratatui::backend::{Backend, CrosstermBackend, TestBackend};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::io::{self, Write};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::mpsc::TrySendError;
use std::sync::{Arc, Mutex};

fn dashboard_fixture() -> Dashboard {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    let session = |id: u64,
                   project: &str,
                   workspace: &str,
                   name: &str,
                   label: &str,
                   pid: Option<u32>,
                   started_unix_ms: u64| SessionSummary {
        id: SessionId(id),
        project: project.into(),
        workspace: workspace.into(),
        name: name.into(),
        label: label.into(),
        pid,
        started_unix_ms,
        phase: SessionPhase::Running,
        activity: AgentActivity::Unknown,
        context_usage: None,
    };
    dashboard.hierarchy = HierarchySnapshot {
        projects: vec![
            ProjectSummary {
                name: "spacelift-agent".into(),
                workspaces: vec![WorkspaceSummary {
                    project: "spacelift-agent".into(),
                    name: "progress".into(),
                    path: PathBuf::from("/tmp/progress"),
                    sessions: vec![session(
                        3,
                        "spacelift-agent",
                        "progress",
                        "local",
                        "zsh",
                        Some(333),
                        0,
                    )],
                }],
            },
            ProjectSummary {
                name: "consigint".into(),
                workspaces: vec![
                    WorkspaceSummary {
                        project: "consigint".into(),
                        name: "lifecycle".into(),
                        path: PathBuf::from("/tmp/lifecycle"),
                        sessions: vec![
                            session(
                                2,
                                "consigint",
                                "lifecycle",
                                "implement",
                                "claude",
                                Some(222),
                                0,
                            ),
                            session(4, "consigint", "lifecycle", "local", "zsh", Some(444), 0),
                        ],
                    },
                    WorkspaceSummary {
                        project: "consigint".into(),
                        name: "auth".into(),
                        path: PathBuf::from("/tmp/auth"),
                        sessions: vec![
                            session(
                                1,
                                "consigint",
                                "auth",
                                "review",
                                "claude",
                                Some(111),
                                u64::MAX,
                            ),
                            session(5, "consigint", "auth", "local", "zsh", Some(555), 0),
                        ],
                    },
                ],
            },
        ],
    };
    dashboard.hierarchy.projects[1].workspaces[0].sessions[0].phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    dashboard.hierarchy.projects[1].workspaces[0].sessions[0].pid = None;
    dashboard.outer_area = Rect::new(0, 0, 88, 38);
    dashboard.select_session(SessionId(1));
    let request = dashboard
        .view_request(dashboard.outer_area, 999)
        .unwrap()
        .expect("fixture view should request a snapshot");
    let revision = dashboard.view_revision;
    let session = dashboard.focused_session().unwrap();
    let size = dashboard.panes[dashboard.focused_pane].desired_size;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session,
            revision,
            size,
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes[dashboard.focused_pane].ready);
    dashboard
}

fn acknowledge_focused_view(dashboard: &mut Dashboard, request_id: u64) {
    let request = dashboard
        .view_request(dashboard.outer_area, request_id)
        .unwrap()
        .expect("focused view should request a snapshot");
    acknowledge_view_request(dashboard, request);
}

fn acknowledge_view_request(dashboard: &mut Dashboard, request: ovrcr::protocol::ClientMessage) {
    let revision = dashboard.view_revision;
    let session = dashboard.focused_session().expect("focused session");
    let size = dashboard.panes[dashboard.focused_pane].desired_size;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session,
            revision,
            size,
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes[dashboard.focused_pane].ready);
}

fn acknowledge_all_view_targets(
    dashboard: &mut Dashboard,
    request: ovrcr::protocol::ClientMessage,
) {
    let request_id = request.request_id;
    let Request::SetView { view } = request.request else {
        panic!("expected SetView request");
    };
    deliver_all_view_screens(dashboard, request_id, &view);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Ok,
    });
}

fn assert_view_input_blocked(dashboard: &mut Dashboard, request_id: u64) {
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("blocked".into())),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.input_request(vec![b'x'], request_id).is_none());
    dashboard.mode = ovrcr::tui::InputMode::Browse;
}

fn assert_view_input_allowed(dashboard: &mut Dashboard, session: SessionId, request_id: u64) {
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Terminal);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        ovrcr::tui::DashboardAction::PtyBytes(b"z".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("paste".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"paste".to_vec())
    );
    assert_eq!(
        dashboard
            .input_request(vec![b'x'], request_id)
            .unwrap()
            .request,
        Request::Input {
            session,
            bytes: vec![b'x'],
        }
    );
    dashboard.mode = ovrcr::tui::InputMode::Browse;
}

fn deliver_all_view_screens(
    dashboard: &mut Dashboard,
    request_id: u64,
    view: &ovrcr::protocol::DashboardView,
) {
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
}

fn palette_text(dashboard: &Dashboard) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    (0..40)
        .map(|y| {
            (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn leader_workspace_uses_inspection_and_name_first_picker_defaults() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(' '));
    let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Char('w')) else {
        panic!("leader workspace must request repository inspection");
    };
    assert_eq!(message.request, Request::Inspect);
    dashboard.event_action(Event::Paste("leader-workspace".into()));
    let text = palette_text(&dashboard);
    assert!(!text.contains("Which key"));
    assert!(text.contains("feature/leader-workspace"));
    assert!(text.contains("consigint"));
    assert!(text.contains("Branch mode"));
}

#[test]
fn palette_hint_action_ignores_late_inspection_error() {
    let mut dashboard = dashboard_fixture();
    let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Char(':')) else {
        panic!("palette must request inspection");
    };
    assert_eq!(message.request, Request::Inspect);
    dashboard.event_action(Event::Paste("focus".into()));
    dashboard.key(KeyCode::Enter);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Terminal);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "late inspection".into(),
        },
    });
    assert!(
        dashboard.error.is_none(),
        "closed palette must discard its inspection response"
    );
}

#[test]
fn space_then_n_opens_the_terminal_form() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(' '));
    let text = palette_text(&dashboard);
    assert!(text.contains("Which key"));
    assert!(text.contains("Create"));
    dashboard.key(KeyCode::Char('n'));
    let text = palette_text(&dashboard);
    assert!(!text.contains("Which key"));
    assert!(text.contains("Create terminal"));
    assert!(text.contains("Agent"));
}

#[test]
fn question_mark_popup_is_browsable() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('?'));
    assert!(palette_text(&dashboard).contains("Which key"));
    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Enter);
    let text = palette_text(&dashboard);
    assert!(!text.contains("Which key"));
    assert!(text.contains("Register project"));
    assert!(text.contains("Repository"));
    assert!(text.contains("Workspace root"));
}

#[test]
fn narrow_popup_moves_description_to_detail_line() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('?'));
    dashboard.key(KeyCode::Down); // workspace creation
    let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let lines: Vec<String> = (0..40)
        .map(|y| {
            (0..80)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect()
        })
        .collect();
    let row = lines
        .iter()
        .position(|line| line.contains("w  Create workspace"))
        .unwrap();
    assert!(!lines[row].contains("consigint"));
    let detail = lines
        .iter()
        .position(|line| line.contains("Create a worktree"))
        .unwrap();
    assert!(detail > row);
    assert!(lines[detail..].join("\n").contains("consigint"));
    assert!(
        lines[37].contains("opens a form"),
        "full description must reach the last inner popup line"
    );
    for _ in 0..4 {
        dashboard.key(KeyCode::Down);
    }
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let resume_row = (2..37)
        .find(|y| {
            (3..77)
                .map(|x| terminal.backend().buffer()[(x, *y)].symbol())
                .collect::<String>()
                .contains("r  Resume")
        })
        .unwrap();
    assert!(
        terminal.backend().buffer()[(3, resume_row)]
            .modifier
            .contains(Modifier::DIM)
    );
    let last: String = (3..77)
        .map(|x| terminal.backend().buffer()[(x, 37)].symbol())
        .collect();
    assert!(last.contains("Resume: not paused"));
}

#[test]
fn empty_hierarchy_shows_start_screen() {
    let dashboard = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    let text = palette_text(&dashboard);
    assert!(text.contains("Welcome to OVRCR"));
    assert!(text.contains("a  Register project"));
    assert!(text.contains(":  Command palette"));
    assert!(text.contains("?  Help"));
    assert!(text.contains("Config:"));
    assert!(text.contains("Socket:"));
}

#[test]
fn palette_entries_show_keys_and_descriptions() {
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "create workspace");
    let text = palette_text(&dashboard);
    assert!(text.contains("w"));
    assert!(text.contains("Create a worktree"));
    assert!(text.contains("consigint"));
}

#[test]
fn whichkey_mouse_uses_rendered_rows_and_blocks_background_clicks() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('?'));
    for (width, height) in [(80, 40), (320, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        let mut target = None;
        for y in 0..height {
            let row: String = (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect();
            if let Some(x) = row.find("a  Register project") {
                target = Some((x as u16, y));
            }
        }
        let (column, row) = target.expect("register row must be visible");
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, width, height),
        );
        assert!(palette_text(&dashboard).contains("Repository"));
        assert!(palette_text(&dashboard).contains("Workspace root"));
        assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
        dashboard.key(KeyCode::Esc);
        dashboard.key(KeyCode::Char('?'));
    }
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(palette_text(&dashboard).contains("Which key"));
}

#[test]
fn tasks_hotkey_cancels_pending_history_like_control_t() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::PageUp);
    assert!(!dashboard.history_begin_request.as_ref().unwrap().cancelled);
    dashboard.key(KeyCode::Char('t'));
    assert!(dashboard.tasks.is_some());
    assert!(dashboard.history_begin_request.as_ref().unwrap().cancelled);
}

#[test]
fn whichkey_preserves_readiness_and_never_forwards_overlay_input() {
    use ovrcr::tui::{DashboardAction, InputMode};
    let mut dashboard = dashboard_fixture();
    let revision = dashboard.view_revision;
    dashboard.key(KeyCode::Char(' '));
    assert_eq!(
        dashboard.event_action(Event::Paste("never forward".into())),
        DashboardAction::None
    );
    assert_eq!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('n'),
            KeyModifiers::NONE,
            KeyEventKind::Release
        )),
        DashboardAction::None
    );
    assert!(palette_text(&dashboard).contains("Which key"));
    dashboard.key(KeyCode::Esc);
    assert!(!palette_text(&dashboard).contains("Which key"));
    assert_eq!(dashboard.view_revision, revision);
    assert!(dashboard.panes[0].ready);
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('r')); // running session: resume is disabled
    assert!(!palette_text(&dashboard).contains("Which key"));
    assert_eq!(dashboard.mode, InputMode::Browse);
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Enter);
    assert_eq!(dashboard.mode, InputMode::Terminal);
    assert_eq!(
        dashboard.key(KeyCode::Char('?')),
        DashboardAction::PtyBytes(b"?".to_vec())
    );
    assert_eq!(
        dashboard.key(KeyCode::Char(' ')),
        DashboardAction::PtyBytes(b" ".to_vec())
    );
}

#[test]
fn whichkey_copy_leader_preserves_capture_and_v_anchors() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('['));
    let session = dashboard.copy.as_ref().unwrap().session;
    dashboard.key(KeyCode::Char(' '));
    assert!(dashboard.mouse_capture_required());
    assert!(palette_text(&dashboard).contains("Selection"));
    assert!(dashboard.copy.as_ref().unwrap().anchor.is_none());
    dashboard.key(KeyCode::Char('v'));
    assert!(!palette_text(&dashboard).contains("Which key"));
    assert_eq!(dashboard.copy.as_ref().unwrap().session, session);
    assert!(dashboard.copy.as_ref().unwrap().anchor.is_some());
    dashboard.key(KeyCode::Char('?'));
    dashboard.cancel_copy(Some("Copy cancelled: terminal resized"));
    assert!(!palette_text(&dashboard).contains("Which key"));
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Char(' '));
    dashboard.ctrl('g');
    assert!(
        dashboard.copy.is_none(),
        "leader Ctrl-g must run the listed Browse action"
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
}

#[test]
fn whichkey_copy_cursor_moves_into_popup_instead_of_underlying_capture() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Char('?'));
    let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let cursor = terminal.backend().cursor_position();
    // Popup first motion row begins after its border and Move heading.
    assert_eq!(cursor, Position::new(3, 3));
}

#[test]
fn whichkey_empty_workspace_click_sets_terminal_form_target() {
    let mut dashboard = dashboard_fixture();
    dashboard.hierarchy.projects[0]
        .workspaces
        .push(WorkspaceSummary {
            project: "spacelift-agent".into(),
            name: "empty".into(),
            path: "/tmp/empty".into(),
            sessions: vec![],
        });
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let row = (0..39)
        .find(|y| {
            (0..39)
                .map(|x| terminal.backend().buffer()[(x, *y)].symbol())
                .collect::<String>()
                .contains("empty")
        })
        .unwrap();
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    assert_eq!(dashboard.focused_session(), None);
    assert!(!dashboard.panes[0].ready);
    assert!(palette_text(&dashboard).contains("press n to start a terminal here"));
    dashboard.key(KeyCode::Char('n'));
    assert!(palette_text(&dashboard).contains("spacelift-agent / empty"));
    assert_eq!(
        dashboard.hierarchy.projects[1].workspaces[1].sessions.len(),
        2
    );
}

#[test]
fn whichkey_tiny_layouts_and_last_disabled_detail_remain_browsable() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    dashboard.key(KeyCode::Char('?'));
    for _ in 0..6 {
        dashboard.key(KeyCode::Down);
    }
    for (width, height) in [(1, 1), (3, 2), (20, 8), (80, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
    }
    let text = palette_text(&dashboard);
    assert!(text.contains("no session selected"));
    dashboard.key(KeyCode::Enter);
    assert!(palette_text(&dashboard).contains("Which key"));
    dashboard.key(KeyCode::Esc);
    assert!(!palette_text(&dashboard).contains("Which key"));
}

#[test]
fn uppercase_x_confirms_session_close_without_closing_pane() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('X'));
    let text = palette_text(&dashboard);
    assert!(text.contains("Confirm action"));
    assert!(text.contains("review (#1)"));
    assert_eq!(dashboard.panes.len(), 1);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("close confirmation did not submit");
    };
    assert_eq!(
        message.request,
        Request::CloseTerminal {
            session: SessionId(1)
        }
    );
}

fn palette_search(dashboard: &mut Dashboard, query: &str) {
    let action = dashboard.key(KeyCode::Char(':'));
    answer_palette_inspect(dashboard, action);
    dashboard.event_action(Event::Paste(query.into()));
}

fn answer_palette_inspect(dashboard: &mut Dashboard, action: ovrcr::tui::DashboardAction) {
    if let ovrcr::tui::DashboardAction::Request(ClientMessage {
        request_id,
        request: Request::Inspect,
    }) = action
    {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Inventory {
                registry: Default::default(),
                sessions: Vec::new(),
            },
        });
    }
}

#[test]
fn palette_filters_and_captures_input_without_sending_it_to_terminal() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "create terminal");
    let mut helper_dashboard = dashboard_fixture();
    palette_search(&mut helper_dashboard, "create terminal");
    helper_dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(
        event_to_request(
            &mut helper_dashboard,
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            100,
        )
        .is_none()
    );
    assert!(
        event_to_request(
            &mut helper_dashboard,
            Event::Paste("blocked by palette".into()),
            101,
        )
        .is_none()
    );
    assert!(palette_text(&dashboard).contains("Create terminal"));
    assert!(!palette_text(&dashboard).contains("Register project"));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(palette_text(&dashboard).contains("consigint"));
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Tab);
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("palette-shell".into()));
    dashboard.key(KeyCode::Tab);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("form did not submit");
    };
    let ovrcr::protocol::Request::CreateSession(request) = message.request else {
        panic!("wrong request");
    };
    assert_eq!(
        (
            request.project.as_str(),
            request.workspace.as_str(),
            request.name.as_str()
        ),
        ("consigint", "auth", "palette-shell")
    );
    assert_eq!(request.argv.len(), 1);
    assert!(!request.argv[0].is_empty());
    assert_eq!(
        dashboard.event_action(Event::Paste("never execute".into())),
        DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        DashboardAction::Redraw,
        "pending submit must not repeat"
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "Name already exists".into(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id + 1,
        response: Response::Ok,
    });
    assert!(palette_text(&dashboard).contains("Name already exists"));
    assert!(palette_text(&dashboard).contains("palette-shell"));
    dashboard.key(KeyCode::Esc);
    assert!(!palette_text(&dashboard).contains("Name already exists"));
}

#[test]
fn palette_switches_by_search_and_confirms_exact_close_target() {
    use ovrcr::protocol::Request;
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "spacelift-agent progress local");
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("switch missing");
    };
    assert!(matches!(
        message.request,
        Request::SetView { view }
            if view.focused == Some(SessionId(3))
                && view.panes.iter().any(|pane| pane.session == SessionId(3))
    ));
    dashboard.ctrl('g');
    palette_search(&mut dashboard, "close terminal");
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(palette_text(&dashboard).contains("spacelift-agent / progress / local"));
    dashboard.key(KeyCode::Esc);
    palette_search(&mut dashboard, "close terminal");
    dashboard.key(KeyCode::Enter);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("close missing");
    };
    assert_eq!(
        message.request,
        Request::CloseTerminal {
            session: SessionId(3)
        }
    );
}

#[test]
fn palette_forms_build_workspace_and_project_requests_and_draw_at_small_sizes() {
    use ovrcr::protocol::{BranchRequest, Request};
    use ovrcr::tui::DashboardAction;
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let cases = [
        (
            "create workspace",
            vec!["consigint", "new-space", "new", "feature/palette", "main"],
            Request::CreateWorkspace {
                project: "consigint".into(),
                name: "new-space".into(),
                branch: BranchRequest::New {
                    branch: "feature/palette".into(),
                    base: "main".into(),
                },
            },
        ),
        (
            "create workspace",
            vec!["consigint", "existing", "existing", "topic"],
            Request::CreateWorkspace {
                project: "consigint".into(),
                name: "existing".into(),
                branch: BranchRequest::Existing {
                    branch: "topic".into(),
                },
            },
        ),
        (
            "register project",
            vec!["/tmp/repo with spaces", "repo", "/tmp/worktrees"],
            Request::AddProject {
                name: "repo".into(),
                repo: "/tmp/repo with spaces".into(),
                workspace_root: "/tmp/worktrees".into(),
            },
        ),
        (
            "register project",
            vec!["~/Code/repo/", "repo", "~/worktrees"],
            Request::AddProject {
                name: "repo".into(),
                repo: home.join("Code/repo"),
                workspace_root: home.join("worktrees"),
            },
        ),
        (
            "remove workspace",
            vec!["consigint", "auth"],
            Request::RemoveWorkspace {
                project: "consigint".into(),
                name: "auth".into(),
            },
        ),
        (
            "remove project",
            vec!["consigint"],
            Request::RemoveProject {
                name: "consigint".into(),
            },
        ),
    ];
    for (query, values, expected) in cases {
        let mut dashboard = dashboard_fixture();
        palette_search(&mut dashboard, query);
        dashboard.key(KeyCode::Enter);
        if query == "create workspace" {
            dashboard.key(KeyCode::BackTab);
        }
        for (index, value) in values.iter().enumerate() {
            if query == "create workspace" && index == 2 {
                if *value == "existing" {
                    dashboard.key(KeyCode::Char(' '));
                }
            } else {
                dashboard.ctrl('u');
                dashboard.event_action(Event::Paste((*value).into()));
            }
            for (width, height) in [(120, 40), (40, 12), (12, 5), (1, 1)] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
                    .unwrap();
            }
            if index + 1 < values.len() {
                let next = if query == "register project" {
                    KeyCode::Enter
                } else {
                    KeyCode::Tab
                };
                dashboard.key(next);
            }
        }
        let mut action = dashboard.key(KeyCode::Enter);
        if query.starts_with("remove") {
            assert_eq!(action, DashboardAction::Redraw);
            action = dashboard.key(KeyCode::Enter);
        }
        let DashboardAction::Request(message) = action else {
            panic!("{query}: did not submit");
        };
        assert_eq!(message.request, expected);
    }
}

#[test]
fn w_opens_workspace_form_with_project_and_derived_branch() {
    use ovrcr::protocol::BranchRequest;
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    let action = dashboard.key(KeyCode::Char('w'));
    answer_palette_inspect(&mut dashboard, action);
    dashboard.event_action(Event::Paste("pick-demo".into()));
    let text = palette_text(&dashboard);
    assert!(text.contains("Create workspace"), "{text}");
    assert!(text.contains("consigint"), "{text}");
    assert!(text.contains("feature/pick-demo"), "{text}");
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("Enter on Name must submit with defaults");
    };
    assert_eq!(
        message.request,
        Request::CreateWorkspace {
            project: "consigint".into(),
            name: "pick-demo".into(),
            branch: BranchRequest::New {
                branch: "feature/pick-demo".into(),
                base: "main".into()
            },
        }
    );
}

#[test]
fn workspace_branch_mode_uses_local_branches_and_preserves_edits() {
    use ovrcr::config::{ProjectRecord, Registry};
    use ovrcr::tui::DashboardAction;
    use std::process::Command;
    let repo = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
        vec!["branch", "topic"],
        vec!["branch", "z1"],
        vec!["branch", "z2"],
        vec!["branch", "z3"],
        vec!["branch", "z4"],
        vec!["branch", "z5"],
        vec!["branch", "z6"],
        vec!["update-ref", "refs/remotes/origin/topic", "HEAD"],
        vec![
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/topic",
        ],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut dashboard = dashboard_fixture();
    let DashboardAction::Request(inspect) = dashboard.key(KeyCode::Char('w')) else {
        panic!("Inspect missing");
    };
    assert_eq!(inspect.request, Request::Inspect);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: inspect.request_id,
        response: Response::Inventory {
            registry: Registry {
                projects: vec![ProjectRecord {
                    name: "consigint".into(),
                    repo: repo.path().into(),
                    workspace_root: repo.path().join("worktrees"),
                    workspaces: vec![],
                }],
            },
            sessions: vec![],
        },
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        dashboard.event_action(Event::Paste(String::new()));
        if palette_text(&dashboard).contains("topic") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Git hints never reached the form"
        );
        std::thread::yield_now();
    }
    dashboard.event_action(Event::Paste("first".into()));
    dashboard.key(KeyCode::Tab); // mode
    dashboard.key(KeyCode::Tab); // branch
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("custom/kept".into()));
    dashboard.key(KeyCode::BackTab);
    dashboard.key(KeyCode::BackTab); // name
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("renamed".into()));
    assert!(palette_text(&dashboard).contains("custom/kept"));
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Right); // existing
    dashboard.key(KeyCode::Tab); // branch pick
    let text = palette_text(&dashboard);
    assert!(text.contains("main") && text.contains("topic"), "{text}");
    assert!(!text.contains("Base"), "{text}");
    for _ in 0..7 {
        dashboard.key(KeyCode::Down);
    }
    let text = palette_text(&dashboard);
    assert!(
        text.contains("› z6"),
        "selected branch must remain visible: {text}"
    );
    dashboard.event_action(Event::Paste("topic".into()));
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("existing branch did not submit");
    };
    assert_eq!(
        message.request,
        Request::CreateWorkspace {
            project: "consigint".into(),
            name: "renamed".into(),
            branch: ovrcr::protocol::BranchRequest::Existing {
                branch: "topic".into()
            },
        }
    );
}

fn hint_repository(extra_branches: &[&str]) -> tempfile::TempDir {
    use std::process::Command;
    let repo = tempfile::tempdir().unwrap();
    let mut commands: Vec<Vec<&str>> = vec![
        vec!["init", "-b", "main"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    ];
    for branch in extra_branches {
        commands.push(vec!["branch", branch]);
    }
    for args in commands {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    repo
}

/// Answers the create-workspace Inspect so Git hints start for a real repository.
fn answer_workspace_inspect(dashboard: &mut Dashboard, request_id: u64, repo: &std::path::Path) {
    use ovrcr::config::{ProjectRecord, Registry};
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Inventory {
            registry: Registry {
                projects: vec![ProjectRecord {
                    name: "consigint".into(),
                    repo: repo.into(),
                    workspace_root: repo.join("worktrees"),
                    workspaces: vec![],
                }],
            },
            sessions: vec![],
        },
    });
}

/// Drives the create-workspace form to a deferred submit on a typed branch.
fn type_existing_branch_and_submit(dashboard: &mut Dashboard, name: &str, branch: &str) -> u64 {
    use ovrcr::tui::DashboardAction;
    let DashboardAction::Request(inspect) = dashboard.key(KeyCode::Char('w')) else {
        panic!("leader workspace must request repository inspection");
    };
    assert_eq!(inspect.request, Request::Inspect);
    dashboard.event_action(Event::Paste(name.into()));
    dashboard.key(KeyCode::Tab); // branch mode
    dashboard.key(KeyCode::Right); // existing
    dashboard.key(KeyCode::Tab); // branch, still a text field while hints are absent
    dashboard.event_action(Event::Paste(branch.into()));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let text = palette_text(dashboard);
    assert!(text.contains("Waiting for Git suggestions"), "{text}");
    inspect.request_id
}

#[test]
fn typed_existing_branch_survives_hint_arrival() {
    use ovrcr::protocol::BranchRequest;
    let repo = hint_repository(&["release-2"]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "typed-branch", "release-2");
    answer_workspace_inspect(&mut dashboard, inspect_id, repo.path());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let message = loop {
        let (_, request) = dashboard.poll_palette();
        let text = palette_text(&dashboard);
        // Without real branches the field would stay text and submit vacuously.
        assert!(!text.contains("enter branch/base manually"), "{text}");
        if let Some(message) = request {
            break message;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Git hints never reached the deferred submit: {text}"
        );
        std::thread::yield_now();
    };
    assert_eq!(
        message.request,
        Request::CreateWorkspace {
            project: "consigint".into(),
            name: "typed-branch".into(),
            branch: BranchRequest::Existing {
                branch: "release-2".into()
            },
        }
    );
}

/// Polls the deferred submit until the palette stops waiting on Git, refusing
/// any request along the way, and returns the rendered palette.
fn poll_until_deferred_submit_refused(dashboard: &mut Dashboard, why: &str) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let (_, request) = dashboard.poll_palette();
        assert!(request.is_none(), "{why}");
        let text = palette_text(dashboard);
        // Without real branches the field would stay text and submit anyway.
        assert!(!text.contains("enter branch/base manually"), "{text}");
        if !text.contains("Waiting for Git suggestions") {
            return text;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Git hints never reached the deferred submit: {text}"
        );
        std::thread::yield_now();
    }
}

#[test]
fn fuzzy_matched_branch_is_not_submitted_without_confirmation() {
    use ovrcr::protocol::BranchRequest;
    use ovrcr::tui::DashboardAction;
    let repo = hint_repository(&["release-2-old"]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "typed-branch", "release-2");
    answer_workspace_inspect(&mut dashboard, inspect_id, repo.path());
    let text = poll_until_deferred_submit_refused(
        &mut dashboard,
        "a branch the user never named must not be submitted",
    );
    assert!(text.contains("Create workspace"), "{text}");
    assert!(text.contains("Branch not found in repository"), "{text}");
    // The refusal is not a dead end: the filtered list is now on screen, so a
    // second Enter accepts the near match the user can finally see.
    assert!(text.contains("release-2-old"), "{text}");
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("confirming the filtered branch must submit");
    };
    assert_eq!(
        message.request,
        Request::CreateWorkspace {
            project: "consigint".into(),
            name: "typed-branch".into(),
            branch: BranchRequest::Existing {
                branch: "release-2-old".into()
            },
        }
    );
}

#[test]
fn typed_branch_missing_from_hints_is_refused_not_replaced() {
    let repo = hint_repository(&[]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "typed-branch", "release-2");
    answer_workspace_inspect(&mut dashboard, inspect_id, repo.path());
    let text = poll_until_deferred_submit_refused(
        &mut dashboard,
        "a branch missing from the repository must not be submitted",
    );
    assert!(text.contains("Create workspace"), "{text}");
    assert!(text.contains("Branch not found in repository"), "{text}");
    assert!(!text.contains("Working"), "{text}");
}

#[test]
fn workspace_created_with_exited_local_shell_closes_palette() {
    use ovrcr::tui::{DashboardAction, InputMode};
    let mut dashboard = dashboard_fixture();
    let action = dashboard.key(KeyCode::Char('w'));
    answer_palette_inspect(&mut dashboard, action);
    dashboard.event_action(Event::Paste("pick-demo".into()));
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("workspace form did not submit");
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Ok,
    });
    assert!(palette_text(&dashboard).contains("Working"));
    let mut hierarchy = workspace_creation_hierarchy(&dashboard);
    let workspace = hierarchy
        .projects
        .iter_mut()
        .find(|project| project.name == "consigint")
        .unwrap()
        .workspaces
        .last_mut()
        .unwrap();
    workspace.sessions[0].phase = SessionPhase::Exited {
        code: Some(1),
        signal: None,
    };
    workspace.sessions[0].pid = None;
    let outgoing = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::HierarchyChanged(hierarchy),
    ));
    let text = palette_text(&dashboard);
    assert!(!text.contains("Create workspace"), "{text}");
    assert!(!text.contains("Working"), "{text}");
    assert!(!outgoing.iter().any(
        |message| matches!(&message.request, Request::SetView { view } if view.focused == Some(SessionId(99)))
    ));
    assert_ne!(dashboard.focused_session(), Some(SessionId(99)));
    assert_eq!(dashboard.mode, InputMode::Browse);
}

fn workspace_creation_hierarchy(dashboard: &Dashboard) -> HierarchySnapshot {
    let mut hierarchy = dashboard.hierarchy.clone();
    let project = hierarchy
        .projects
        .iter_mut()
        .find(|project| project.name == "consigint")
        .unwrap();
    let mut workspace = project.workspaces[1].clone();
    workspace.name = "pick-demo".into();
    workspace.sessions.truncate(1);
    workspace.sessions[0].id = SessionId(99);
    workspace.sessions[0].workspace = "pick-demo".into();
    workspace.sessions[0].name = "local".into();
    project.workspaces.push(workspace);
    hierarchy
}

#[test]
fn palette_switch_drops_late_inventory_without_clearing_current_error() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    let DashboardAction::Request(inspect) = dashboard.key(KeyCode::Char(':')) else {
        panic!("no Inspect");
    };
    dashboard.event_action(Event::Paste("spacelift-agent progress local".into()));
    dashboard.key(KeyCode::Enter);
    dashboard.error = Some("current error".into());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: inspect.request_id,
        response: Response::Inventory {
            registry: Default::default(),
            sessions: vec![],
        },
    });
    assert_eq!(dashboard.error.as_deref(), Some("current error"));
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
}

#[test]
fn workspace_creation_cancel_and_failure_do_not_attach_late() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    let action = dashboard.key(KeyCode::Char('w'));
    answer_palette_inspect(&mut dashboard, action);
    dashboard.event_action(Event::Paste("pick-demo".into()));
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("no create request");
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id + 1,
        response: Response::Ok,
    });
    assert!(palette_text(&dashboard).contains("Working"));
    let hierarchy = workspace_creation_hierarchy(&dashboard);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy.clone(),
    )));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "creation refused".into(),
        },
    });
    assert!(palette_text(&dashboard).contains("creation refused"));
    let DashboardAction::Request(retry) = dashboard.key(KeyCode::Enter) else {
        panic!("no retry");
    };
    dashboard.key(KeyCode::Esc);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry.request_id,
        response: Response::Ok,
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(!palette_text(&dashboard).contains("Create workspace"));
}

#[test]
fn workspace_creation_attaches_to_its_local_shell() {
    use ovrcr::tui::{DashboardAction, InputMode};
    for hierarchy_first in [true, false] {
        let mut dashboard = dashboard_fixture();
        let action = dashboard.key(KeyCode::Char('w'));
        answer_palette_inspect(&mut dashboard, action);
        dashboard.event_action(Event::Paste("pick-demo".into()));
        let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
            panic!("workspace form did not submit");
        };
        let event = ServerMessage::Event(ServerEvent::HierarchyChanged(
            workspace_creation_hierarchy(&dashboard),
        ));
        let ack = ServerMessage::Response {
            request_id: message.request_id,
            response: Response::Ok,
        };
        let outgoing = if hierarchy_first {
            dashboard.handle_server_message(event);
            assert_ne!(dashboard.focused_session(), Some(SessionId(99)));
            dashboard.handle_server_message(ack)
        } else {
            dashboard.handle_server_message(ack);
            assert_ne!(dashboard.focused_session(), Some(SessionId(99)));
            dashboard.handle_server_message(event)
        };
        assert_eq!(dashboard.focused_session(), Some(SessionId(99)));
        assert_eq!(dashboard.mode, InputMode::Terminal);
        assert!(
            dashboard
                .input_request(b"not ready".to_vec(), 999)
                .is_none()
        );
        assert_eq!(outgoing.len(), 1);
        assert!(
            matches!(&outgoing[0].request, Request::SetView { view } if view.focused == Some(SessionId(99)))
        );
        acknowledge_view_request(&mut dashboard, outgoing[0].clone());
        assert!(matches!(
            dashboard.input_request(b"ready".to_vec(), 1000),
            Some(ClientMessage {
                request: Request::Input {
                    session: SessionId(99),
                    ..
                },
                ..
            })
        ));
    }
}

#[test]
fn n_opens_terminal_form_prefilled_for_selected_workspace() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    assert_eq!(dashboard.key(KeyCode::Char('n')), DashboardAction::Redraw);
    let text = palette_text(&dashboard);
    assert!(text.contains("Create terminal"));
    assert!(text.contains("consigint / auth"));
    let path = std::env::var_os("PATH").unwrap_or_default();
    let shell = std::env::var_os("SHELL");
    let first = ovrcr::tui::detect_agents(&path, shell.as_deref())
        .into_iter()
        .next()
        .expect("shell is always detected")
        .name;
    assert!(text.contains(&first), "{text}");
}

#[test]
fn created_session_enters_terminal_mode() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('n'));
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("form did not submit");
    };
    let Request::CreateSession(request) = &message.request else {
        panic!("wrong request");
    };
    let session = SessionSummary {
        id: SessionId(99),
        project: request.project.clone(),
        workspace: request.workspace.clone(),
        name: request.name.clone(),
        label: request.label.clone().unwrap_or_default(),
        pid: Some(1),
        started_unix_ms: 0,
        phase: SessionPhase::Running,
        activity: AgentActivity::Unknown,
        context_usage: None,
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::CreatedSession(Box::new(session)),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Terminal);
    assert_eq!(dashboard.focused_session(), Some(SessionId(99)));
}

#[test]
fn n_is_listed_in_the_browse_footer() {
    let dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let text: String = (0..40)
        .map(|y| {
            (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    assert!(text.contains("? help  n "), "{text}");
}

#[test]
fn a_opens_project_form_with_roots_and_derives_name_and_root() {
    use ovrcr::tui::DashboardAction;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Code");
    let repo = root.join("demo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir(root.join("plain")).unwrap();
    let mut dashboard = dashboard_fixture();
    dashboard.settings.picker_roots = vec![root];
    dashboard.config_dir = dir.path().join("config");
    assert_eq!(dashboard.key(KeyCode::Char('a')), DashboardAction::Redraw);
    let text = palette_text(&dashboard);
    assert!(text.contains("Register project"), "{text}");
    assert!(text.contains("Code"), "{text}");
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Tab);
    let text = palette_text(&dashboard);
    assert!(text.contains("demo"), "{text}");
    assert!(text.contains("workspaces/demo"), "{text}");
}

#[test]
fn tab_on_a_leaf_path_advances_to_the_next_field() {
    use ovrcr::tui::DashboardAction;
    let dir = tempfile::tempdir().unwrap();
    let leaf = dir.path().join("leaf");
    std::fs::create_dir(&leaf).unwrap();
    let mut dashboard = dashboard_fixture();
    dashboard.settings.picker_roots = vec![dir.path().to_path_buf()];
    assert_eq!(dashboard.key(KeyCode::Char('a')), DashboardAction::Redraw);
    dashboard.event_action(Event::Paste(format!("{}/", leaf.display())));
    let before = palette_text(&dashboard);
    assert!(before.contains("Repository"), "{before}");
    dashboard.key(KeyCode::Tab);
    let after = palette_text(&dashboard);
    assert!(after.contains("› leaf"), "{after}");
}

#[test]
fn paste_into_repository_field_refreshes_the_listing() {
    use ovrcr::tui::DashboardAction;
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join("child")).unwrap();
    let mut dashboard = dashboard_fixture();
    dashboard.settings.picker_roots = vec![dir.path().to_path_buf()];
    assert_eq!(dashboard.key(KeyCode::Char('a')), DashboardAction::Redraw);
    dashboard.event_action(Event::Paste(format!("{}/", repo.display())));
    let text = palette_text(&dashboard);
    assert!(text.contains("  › child"), "{text}");
}

#[test]
fn double_enter_refreshes_the_derived_workspace_root_listing() {
    use ovrcr::tui::DashboardAction;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Code");
    let repo = root.join("demo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut dashboard = dashboard_fixture();
    dashboard.settings.picker_roots = vec![root];
    dashboard.config_dir = dir.path().join("config");
    std::fs::create_dir_all(dashboard.config_dir.join("workspaces").join("demo")).unwrap();
    assert_eq!(dashboard.key(KeyCode::Char('a')), DashboardAction::Redraw);
    dashboard.event_action(Event::Paste(format!("{}/", repo.display())));
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let text = palette_text(&dashboard);
    assert!(text.contains("  › demo"), "{text}");
}

#[test]
fn search_selection_clamps_when_entries_shrink() {
    // The fixture starts focused on session 1, so the two "lifecycle"
    // switch entries (sessions 2 and 4) are both a real focus change,
    // letting the assertions below distinguish "Enter did nothing" (the
    // bug) from "Enter acted on the clamped last entry" (the fix).
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "lifecycle /");
    dashboard.key(KeyCode::Down);
    let mut hierarchy = dashboard.hierarchy.clone();
    for workspace in hierarchy
        .projects
        .iter_mut()
        .flat_map(|p| &mut p.workspaces)
    {
        workspace.sessions.retain(|s| s.id != SessionId(4));
    }
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    let action = dashboard.key(KeyCode::Enter);
    assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
    assert_eq!(dashboard.focused_session(), Some(SessionId(2)));
}

#[test]
fn palette_requires_fields_and_keeps_terminal_keys_outside_palette() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key(KeyCode::Char(':')),
        DashboardAction::PtyBytes(b":".to_vec())
    );
    dashboard.ctrl('g');
    palette_search(&mut dashboard, "register project");
    dashboard.settings.picker_roots.clear();
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(palette_text(&dashboard).contains("Repository is required"));
    dashboard.event_action(Event::Paste("é\nname\u{1b}".into()));
    dashboard.key(KeyCode::Backspace);
    assert!(palette_text(&dashboard).contains("énam"));
}

#[test]
fn palette_close_updates_selection_and_clears_removed_screen() {
    let mut dashboard = dashboard_fixture();
    dashboard.panes[dashboard.focused_pane]
        .parser
        .process(b"removed terminal output");
    palette_search(&mut dashboard, "close terminal");
    dashboard.key(KeyCode::Enter);
    let ovrcr::tui::DashboardAction::Request(close) = dashboard.key(KeyCode::Enter) else {
        panic!("close missing");
    };
    let mut hierarchy = dashboard.hierarchy.clone();
    for workspace in hierarchy
        .projects
        .iter_mut()
        .flat_map(|p| &mut p.workspaces)
    {
        workspace.sessions.retain(|s| s.id != SessionId(1));
    }
    let requests = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::HierarchyChanged(hierarchy),
    ));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: close.request_id,
        response: Response::Ok,
    });
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    assert!(matches!(
        requests.as_slice(),
        [ovrcr::protocol::ClientMessage {
            request: ovrcr::protocol::Request::SetView { view },
            ..
        }] if view.focused == Some(SessionId(5))
            && view.panes.iter().any(|pane| pane.session == SessionId(5))
    ));
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("removed terminal output")
    );
}

#[test]
fn palette_active_field_remains_visible_in_a_small_window() {
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "create terminal");
    dashboard.key(KeyCode::Enter);
    for _ in 0..3 {
        dashboard.key(KeyCode::Tab);
    }
    dashboard.event_action(Event::Paste("visible-command".into()));
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let text = (0..12)
        .map(|y| {
            (0..40)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<String>();
    assert!(text.contains("visible-command"), "{text}");
}

/// The fixture after the selected session's screen has arrived, so live
/// output is applied to the pane.
fn screen_ready_dashboard(bytes: &[u8]) -> Dashboard {
    let mut dashboard = dashboard_fixture();
    let id = dashboard.focused_session().unwrap();
    dashboard
        .view_request(Rect::new(0, 0, 89, 38), 900)
        .unwrap()
        .expect("copy fixture should request a refreshed view");
    let size = dashboard.panes[dashboard.focused_pane].desired_size;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Screen {
            session: id,
            revision: dashboard.view_revision,
            size,
            bytes: bytes.to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Ok,
    });
    dashboard
}

fn copy_ready_dashboard() -> Dashboard {
    screen_ready_dashboard(b"abc")
}

/// The outer area whose sole pane terminal is exactly `size`. The sidebar keeps its full 40
/// columns only while the pane is at least as wide, so narrower panes are not expressible.
fn outer_area_for_pane(size: TerminalSize) -> Rect {
    assert!(size.cols >= 40, "a narrower pane splits the sidebar width");
    Rect::new(
        0,
        0,
        size.cols.saturating_add(40),
        size.rows.saturating_add(4),
    )
}

#[test]
fn resize_matching_screen_restores_snapshot_and_ignores_stale_screen() {
    let mut dashboard = dashboard_fixture();
    let session = dashboard.focused_session().unwrap();
    let size = TerminalSize { rows: 20, cols: 40 };
    let resize = dashboard
        .view_request(outer_area_for_pane(size), 901)
        .unwrap()
        .expect("selected dashboard should request resize");
    assert!(matches!(
        resize.request,
        Request::SetView { view }
            if view.focused == Some(session)
                && view.panes.iter().any(|pane| pane.session == session && pane.size == size)
    ));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Screen {
            session,
            revision: dashboard.view_revision,
            size,
            bytes: b"STALE_SCREEN".to_vec(),
        },
    });
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("STALE_SCREEN")
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Screen {
            session,
            revision: dashboard.view_revision,
            size,
            bytes: b"RESTORED_SCREEN".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Ok,
    });
    assert_eq!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .size(),
        (size.rows, size.cols)
    );
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("RESTORED_SCREEN")
    );
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

fn history_page(
    start_row: u32,
    start_col: u16,
    width: u16,
    cells: Vec<HistoryCell>,
) -> HistoryRows {
    HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row,
        start_col,
        rows: vec![HistoryRow {
            width,
            cells,
            wrapped: false,
        }],
    }
}

fn history_complete_page(
    start_row: u32,
    start_col: u16,
    rows: u16,
    width: u16,
    first_cells: Vec<HistoryCell>,
) -> HistoryRows {
    let expected = usize::from(width.saturating_sub(start_col));
    let mut first_cells = first_cells;
    first_cells.resize(expected, history_cell("", 1));
    HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row,
        start_col,
        rows: (0..rows)
            .map(|row| HistoryRow {
                width,
                cells: if row == 0 {
                    first_cells.clone()
                } else {
                    vec![history_cell("", 1); expected]
                },
                wrapped: false,
            })
            .collect(),
    }
}

fn history_tile_page(start_row: u32, start_col: u16, rows: u16, cols: u16) -> HistoryRows {
    HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row,
        start_col,
        rows: (0..rows)
            .map(|_| HistoryRow {
                width: start_col.saturating_add(cols),
                cells: (0..cols).map(|_| history_cell("", 1)).collect(),
                wrapped: false,
            })
            .collect(),
    }
}

fn numbered_history() -> (vt100::Parser, FrozenHistory, Vec<String>) {
    let mut parser = vt100::Parser::new(4, 132, 512);
    let lines = (0..400)
        .map(|row| format!("OLD_{row:03}{}", "x".repeat(123)))
        .collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        let suffix = if index + 1 == lines.len() { "" } else { "\r\n" };
        parser.process(format!("{line}{suffix}").as_bytes());
    }
    let frozen = FrozenHistory::capture(
        SessionId(1),
        HistorySnapshotId(7),
        1,
        parser.screen().clone(),
    )
    .unwrap();
    (parser, frozen, lines)
}

fn pump_frozen_history(
    dashboard: &mut Dashboard,
    frozen: &mut FrozenHistory,
    mut requests: Vec<ClientMessage>,
) {
    for _ in 0..512 {
        let Some(request) = requests.pop() else {
            return;
        };
        let (request_id, start_row, rows, start_col, cols) = match request.request {
            Request::HistoryPage {
                session,
                snapshot,
                start_row,
                rows,
                start_col,
                cols,
            } => {
                assert_eq!(session, frozen.opened().session);
                assert_eq!(snapshot, frozen.opened().snapshot);
                assert!((1..=16).contains(&rows));
                assert!((1..=128).contains(&cols));
                (request.request_id, start_row, rows, start_col, cols)
            }
            request => panic!("unexpected history request: {request:?}"),
        };
        let page = frozen
            .page(start_row, rows, start_col, cols)
            .expect("frozen history page");
        assert_eq!(page.session, frozen.opened().session);
        assert_eq!(page.snapshot, frozen.opened().snapshot);
        assert_eq!(page.start_row, start_row);
        assert_eq!(page.start_col, start_col);
        let follow_up = dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::HistoryRows(page),
        });
        assert!(
            dashboard
                .history
                .as_ref()
                .and_then(|view| view.pending.as_ref())
                .is_none_or(|pending| pending.request_id != request_id)
        );
        assert!(
            dashboard
                .history
                .as_ref()
                .is_none_or(|view| view.pages.len() <= 16)
        );
        requests.extend(follow_up);
    }
    panic!("history request pump did not settle");
}

fn pump_frozen_action(
    dashboard: &mut Dashboard,
    frozen: &mut FrozenHistory,
    action: ovrcr::tui::DashboardAction,
) {
    let requests = match action {
        ovrcr::tui::DashboardAction::Request(request) => vec![request],
        ovrcr::tui::DashboardAction::RequestBatch(requests) => requests,
        _ => return,
    };
    pump_frozen_history(dashboard, frozen, requests);
}

fn pump_frozen_key(dashboard: &mut Dashboard, frozen: &mut FrozenHistory, key: KeyCode) {
    let action = dashboard.key(key);
    pump_frozen_action(dashboard, frozen, action);
}

fn special_history_row(row: u32) -> HistoryRow {
    match row {
        15 => {
            let mut cells = vec![history_cell("", 1); 132];
            cells[126] = history_cell("e\u{301}", 1);
            cells[127] = history_cell("界", 2);
            cells[128] = history_cell("", 0);
            cells[129] = history_cell(" ", 1);
            cells[131] = history_cell("R", 1);
            HistoryRow {
                width: 132,
                cells,
                wrapped: true,
            }
        }
        16 => HistoryRow {
            width: 5,
            cells: vec![
                history_cell("C", 1),
                history_cell("", 1),
                history_cell("", 1),
                history_cell("", 1),
                history_cell("D", 1),
            ],
            wrapped: false,
        },
        17 => HistoryRow {
            width: 4,
            cells: vec![
                history_cell("R", 1),
                history_cell("", 1),
                history_cell("S", 1),
                history_cell("T", 1),
            ],
            wrapped: false,
        },
        _ => HistoryRow {
            width: 0,
            cells: Vec::new(),
            wrapped: false,
        },
    }
}

fn special_history_page(start_row: u32, rows: u16, start_col: u16, cols: u16) -> HistoryRows {
    let rows = (0..rows)
        .map(|offset| {
            let row = special_history_row(start_row + u32::from(offset));
            let cells = if start_col >= row.width {
                Vec::new()
            } else {
                let end = row.width.min(start_col.saturating_add(cols));
                row.cells[usize::from(start_col)..usize::from(end)].to_vec()
            };
            HistoryRow { cells, ..row }
        })
        .collect();
    HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row,
        start_col,
        rows,
    }
}

fn pump_special_history(dashboard: &mut Dashboard, mut requests: Vec<ClientMessage>) {
    for _ in 0..32 {
        let Some(request) = requests.pop() else {
            return;
        };
        let (request_id, start_row, rows, start_col, cols) = match request.request {
            Request::HistoryPage {
                start_row,
                rows,
                start_col,
                cols,
                ..
            } => (request.request_id, start_row, rows, start_col, cols),
            request => panic!("unexpected special history request: {request:?}"),
        };
        requests.extend(dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::HistoryRows(special_history_page(start_row, rows, start_col, cols)),
        }));
    }
    panic!("special history request pump did not settle");
}

#[test]
fn dashboard_layout() {
    let mut dashboard = dashboard_fixture();
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rendered = (0..40)
        .map(|row| {
            (0..120)
                .map(|col| buffer[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        dashboard.visible_rows(),
        vec![
            ovrcr::tui::TreeRow::Project {
                name: "consigint".into()
            },
            ovrcr::tui::TreeRow::Workspace {
                project: "consigint".into(),
                name: "auth".into()
            },
            ovrcr::tui::TreeRow::Session { id: SessionId(5) },
            ovrcr::tui::TreeRow::Session { id: SessionId(1) },
            ovrcr::tui::TreeRow::Workspace {
                project: "consigint".into(),
                name: "lifecycle".into()
            },
            ovrcr::tui::TreeRow::Session { id: SessionId(4) },
            ovrcr::tui::TreeRow::Session { id: SessionId(2) },
            ovrcr::tui::TreeRow::Project {
                name: "spacelift-agent".into()
            },
            ovrcr::tui::TreeRow::Workspace {
                project: "spacelift-agent".into(),
                name: "progress".into()
            },
            ovrcr::tui::TreeRow::Session { id: SessionId(3) },
        ]
    );
    assert!(rendered[0].contains("OVRCR  agent runtime"));
    assert!(
        rendered
            .iter()
            .any(|row| row.contains("pid: 111  elapsed: 0m"))
    );
    assert!(rendered.iter().any(|row| row.contains("ctx —")));
    assert!(rendered.iter().any(|row| row.contains(" j k q")));
    assert!(
        rendered
            .iter()
            .any(|row| row.contains("Space leader  ? help"))
    );
    assert_eq!(buffer[(1, 0)].bg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(39, 10)].symbol(), "│");
    assert_eq!(buffer[(39, 1)].symbol(), "│");
    assert!(rendered[1].contains("pid: 111  elapsed: 0m"));
    assert!(rendered[2].contains("─"));
    assert!(buffer[(8, 14)].modifier.contains(Modifier::DIM));
    assert_eq!(buffer[(1, 6)].bg, Color::Rgb(203, 166, 247));

    dashboard.select_session(SessionId(2));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let selected_exited = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 1)].symbol())
        .collect::<String>();
    assert!(selected_exited.contains("pid: closed"));
}

#[test]
fn sidebar_glyphs_and_columns_match_the_reference_tree() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y| (0..39).map(|x| buffer[(x, y)].symbol()).collect::<String>();
    assert_eq!(row(1).trim_end(), "▼ 󰉋 consigint");
    assert_eq!(row(2).trim_end(), "  ▼ auth");
    assert_eq!(row(3).trim_end(), "  - local");
    assert_eq!(row(4).trim_end(), "     ├ terminal");
    assert_eq!(row(5).trim_end(), "     └ run 0m  ctx —");
    assert_eq!(row(6).trim_end(), "  - review");
    assert_eq!(row(7).trim_end(), "     ├ claude");
    assert_eq!(buffer[(2, 1)].fg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(2, 2)].fg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(4, 2)].fg, Color::Rgb(205, 214, 244));
    assert_eq!(buffer[(2, 6)].fg, Color::Rgb(108, 112, 134));
    assert_eq!(row(9).trim_end(), "");
    assert_eq!(row(10).trim_end(), "  ▼ lifecycle");
    assert_eq!(row(11).trim_end(), "  - local");
    assert_eq!(row(12).trim_end(), "     ├ terminal");
    assert_eq!(row(13).trim_end(), "     └ run 0m  ctx —");
    assert_eq!(row(14).trim_end(), "    implement");
    assert_eq!(row(17).trim_end(), "");
    assert_eq!(row(18).trim_end(), "▼ 󰉋 spacelift-agent");
    assert_eq!(row(19).trim_end(), "  ▼ progress");
    assert_eq!(buffer[(2, 18)].fg, Color::Rgb(137, 220, 235));
    assert_eq!(buffer[(7, 12)].fg, Color::Rgb(118, 123, 146));
    assert_eq!(buffer[(7, 13)].fg, Color::Rgb(92, 96, 116));
    for gap in [9, 17] {
        assert_eq!(
            dashboard.mouse_action(
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: 5,
                    row: gap,
                    modifiers: KeyModifiers::NONE,
                },
                Rect::new(0, 0, 120, 40)
            ),
            ovrcr::tui::DashboardAction::None
        );
        assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    }
    dashboard.select_session(SessionId(1));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(
        terminal.backend().buffer()[(2, 6)].fg,
        Color::Rgb(17, 17, 27)
    );
}

#[test]
fn sidebar_animates_only_explicitly_busy_sessions() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].activity = AgentActivity::Busy;
    for (time, marker) in [(0, "⠋"), (100, "⠙"), (900, "⠏"), (1000, "⠋")] {
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, time))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(2, 6)].symbol(), marker);
        assert_eq!(
            terminal.backend().buffer()[(2, 6)].fg,
            Color::Rgb(166, 227, 161)
        );
        assert_eq!(terminal.backend().buffer()[(2, 3)].symbol(), "-");
    }
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].activity = AgentActivity::Idle;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1100))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(2, 6)].symbol(), " ");
    // An exited process cannot remain busy, even if an old signal is retained.
    dashboard.hierarchy.projects[1].workspaces[0].sessions[0].activity = AgentActivity::Busy;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1200))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(2, 14)].symbol(), " ");
}

#[test]
fn agent_hook_selected_metadata_reports_activity_and_lifecycle() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(1));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let metadata = |terminal: &Terminal<TestBackend>| {
        (40..120)
            .map(|column| terminal.backend().buffer()[(column, 1)].symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    };

    for (activity, expected) in [
        (
            AgentActivity::Unknown,
            "pid: 111  elapsed: 0m  agent unknown",
        ),
        (AgentActivity::Idle, "pid: 111  elapsed: 0m  agent idle"),
        (AgentActivity::Busy, "pid: 111  elapsed: 0m  agent busy"),
        (
            AgentActivity::WaitingInput,
            "pid: 111  elapsed: 0m  agent waiting input",
        ),
        (AgentActivity::Error, "pid: 111  elapsed: 0m  agent error"),
    ] {
        dashboard.hierarchy.projects[1].workspaces[1].sessions[0].activity = activity;
        dashboard.hierarchy.projects[1].workspaces[1].sessions[0].phase = SessionPhase::Running;
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        assert_eq!(metadata(&terminal), expected);
    }

    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].phase = SessionPhase::Paused;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(
        metadata(&terminal),
        "pid: 111  elapsed: 0m  agent error  paused"
    );

    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].pid = None;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(metadata(&terminal), "pid: closed  elapsed: 0m");
}

#[test]
fn agent_hook_sidebar_states_are_literal() {
    let mut dashboard = dashboard_fixture();
    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].activity = AgentActivity::Unknown;
    dashboard.hierarchy.projects[1].workspaces[0].sessions[1].activity = AgentActivity::Idle;
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].activity =
        AgentActivity::WaitingInput;
    dashboard.hierarchy.projects[0].workspaces[0].sessions[0].activity = AgentActivity::Error;
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(2, 3)].symbol(), "-");
    assert_eq!(buffer[(2, 11)].symbol(), " ");
    assert_eq!(buffer[(2, 6)].symbol(), "?");
    assert_eq!(buffer[(2, 20)].symbol(), "!");
    assert_eq!(buffer[(2, 14)].symbol(), " ");
}

#[test]
fn agent_hook_summary_updates_drive_animation() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    acknowledge_focused_view(&mut dashboard, 900);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(2, 3)].symbol(), "-");

    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(5),
        revision: dashboard.view_revision,
        bytes: b"PTY output\x1b]52;c;V0FJVElORw==\x1b\\".to_vec(),
    }));
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("PTY output")
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
    );
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("V0FJVElORw==")
    );
    assert_eq!(
        dashboard.hierarchy.projects[1].workspaces[1].sessions[1].activity,
        AgentActivity::Unknown
    );
    dashboard.mode = ovrcr::tui::InputMode::Browse;

    let mut summary = dashboard.hierarchy.projects[1].workspaces[1].sessions[1].clone();
    summary.activity = AgentActivity::Busy;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        summary,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 100))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(2, 3)].symbol(), "⠙");

    summary = dashboard.hierarchy.projects[1].workspaces[1].sessions[1].clone();
    summary.activity = AgentActivity::Idle;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        summary,
    ))));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
    );
    assert_eq!(
        dashboard.hierarchy.projects[1].workspaces[1].sessions[1].activity,
        AgentActivity::Idle
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 200))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(2, 3)].symbol(), " ");
}

#[test]
fn selected_session_uses_three_full_width_sidebar_lines() {
    let dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();

    let selected_lines = (6..=8)
        .map(|row| (0..39).map(|col| buffer[(col, row)].bg).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    assert!(
        selected_lines
            .iter()
            .flatten()
            .all(|background| *background == Color::Rgb(203, 166, 247))
    );
    let rendered = (6..=8)
        .map(|row| {
            (0..39)
                .map(|col| buffer[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert!(rendered[0].contains("review"));
    assert!(rendered[1].contains("├ claude"));
    assert!(rendered[2].contains("└ run 0m  ctx —"));
}

#[test]
fn context_sidebar_updates_and_expires() {
    let mut dashboard = dashboard_fixture();
    let mut summary = dashboard.hierarchy.projects[1].workspaces[1].sessions[0].clone();
    summary.context_usage = Some(ContextUsageSnapshot {
        report: ContextUsageReport {
            source: ContextSource::Generic,
            model: Some("sample-model".into()),
            conversation: Some("sample-conversation".into()),
            used_tokens: Some(25),
            capacity_tokens: Some(100),
        },
        received_unix_ms: 1_000,
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        summary,
    ))));

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1_000))
        .unwrap();
    let receipt = terminal.backend().buffer();
    let metrics = (0..39)
        .map(|column| receipt[(column, 8)].symbol())
        .collect::<String>();
    assert_eq!(metrics.trim_end(), "     └ run 0m  ctx 25%");
    let selected_rows = (6..=8)
        .map(|row| {
            (0..39)
                .map(|column| receipt[(column, row)].bg)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let selected_prefix = (6..=7)
        .map(|row| {
            (0..39)
                .map(|column| receipt[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 301_000))
        .unwrap();
    let expired = terminal.backend().buffer();
    let expired_metrics = (0..39)
        .map(|column| expired[(column, 8)].symbol())
        .collect::<String>();
    assert_eq!(expired_metrics.trim_end(), "     └ run 0m  ctx 25%~");
    assert_eq!(
        selected_prefix,
        (6..=8)
            .take(2)
            .map(|row| {
                (0..39)
                    .map(|column| expired[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(
        selected_rows,
        (6..=8)
            .map(|row| {
                (0..39)
                    .map(|column| expired[(column, row)].bg)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    );
}

#[test]
fn context_sidebar_unknown_and_over_capacity() {
    let mut dashboard = dashboard_fixture();
    let mut summary = dashboard.hierarchy.projects[1].workspaces[1].sessions[0].clone();
    summary.context_usage = Some(ContextUsageSnapshot {
        report: ContextUsageReport {
            source: ContextSource::Generic,
            model: None,
            conversation: None,
            used_tokens: None,
            capacity_tokens: None,
        },
        received_unix_ms: 10_000,
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        summary,
    ))));

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 10_000))
        .unwrap();
    let unknown = (0..39)
        .map(|column| terminal.backend().buffer()[(column, 8)].symbol())
        .collect::<String>();
    assert_eq!(unknown.trim_end(), "     └ run 0m  ctx —");

    let mut over_capacity = dashboard.hierarchy.projects[1].workspaces[1].sessions[0].clone();
    over_capacity.context_usage = Some(ContextUsageSnapshot {
        report: ContextUsageReport {
            source: ContextSource::Generic,
            model: None,
            conversation: None,
            used_tokens: Some(101),
            capacity_tokens: Some(100),
        },
        received_unix_ms: 10_000,
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        over_capacity,
    ))));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 10_000))
        .unwrap();
    let over = (0..39)
        .map(|column| terminal.backend().buffer()[(column, 8)].symbol())
        .collect::<String>();
    assert_eq!(over.trim_end(), "     └ run 0m  ctx >100%");

    let mut narrow = Terminal::new(TestBackend::new(40, 20)).unwrap();
    narrow
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 10_000))
        .unwrap();
    assert_eq!(narrow.backend().buffer()[(19, 8)].symbol(), "│");
    assert!(
        (0..19)
            .map(|column| narrow.backend().buffer()[(column, 8)].symbol())
            .collect::<String>()
            .chars()
            .count()
            <= 19
    );
}

#[test]
fn sidebar_uses_agent_label_prefixes_and_emphasizes_tree_names() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].label = "claude / sonnet-4".into();
    dashboard.hierarchy.projects[1].workspaces[0].sessions[0].label = "unknown / model".into();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();

    assert!(buffer[(4, 2)].modifier.contains(Modifier::BOLD));
    assert!(buffer[(4, 6)].modifier.contains(Modifier::BOLD));
    assert_eq!(buffer[(7, 7)].fg, Color::Rgb(173, 127, 104));
    assert_eq!(buffer[(7, 15)].fg, Color::Rgb(144, 150, 175));
}

#[test]
fn long_sidebar_names_clip_to_one_screen_line() {
    let mut dashboard = dashboard_fixture();
    dashboard.hierarchy.projects[1].name = "consigint-界界界界界界界界界界界界界界".into();
    dashboard.hierarchy.projects[1].workspaces[1].sessions[0].name =
        "review-a-very-long-session-name-that-must-not-wrap".into();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let project = (0..39)
        .map(|column| buffer[(column, 1)].symbol())
        .collect::<String>();
    assert!(project.trim_end().ends_with('…'));
    assert_eq!(buffer[(39, 1)].symbol(), "│");
}

#[test]
fn dashboard_geometry_remains_drawable_at_target_and_tiny_sizes() {
    for (width, height, terminal_width, terminal_height) in
        [(180, 72, 140, 68), (120, 40, 80, 36), (80, 24, 40, 20)]
    {
        assert_eq!(
            ovrcr::tui::actual_drawn_inner_rect(Rect::new(0, 0, width, height)),
            Rect::new(40, 3, terminal_width, terminal_height)
        );
        let dashboard = dashboard_fixture();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
    }

    let dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
}

#[test]
fn browse_and_terminal_modes_keep_input_ownership_clear() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    acknowledge_focused_view(&mut dashboard, 900);
    dashboard.hierarchy.projects[1].workspaces[0].sessions[0].phase = SessionPhase::Running;
    let action = dashboard.key_action(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let ovrcr::tui::DashboardAction::Request(request) = action else {
        panic!("selection did not request a view")
    };
    acknowledge_view_request(&mut dashboard, request);
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Terminal);
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'q'])
    );
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL)),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Detach
    );
}

#[test]
fn copy_mode_routes_keys_and_freezes_output() {
    let mut dashboard = dashboard_fixture();
    let id = dashboard.focused_session().unwrap();
    dashboard
        .view_request(Rect::new(0, 0, 89, 38), 900)
        .unwrap()
        .expect("copy test should request a refreshed view");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Screen {
            session: id,
            revision: dashboard.view_revision,
            size: dashboard.panes[dashboard.focused_pane].desired_size,
            bytes: b"abc".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Ok,
    });
    dashboard.key(KeyCode::Char('['));
    dashboard.copy_notice = Some("stale movement notice".into());
    assert_eq!(
        dashboard.key(KeyCode::Home),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.copy_notice.is_none());
    dashboard.copy_notice = Some("stale anchor notice".into());
    assert_eq!(
        dashboard.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.copy_notice.is_none());
    dashboard.copy_notice = Some("stale movement notice".into());
    assert_eq!(
        dashboard.key(KeyCode::Right),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.copy_notice.is_none());
    assert_eq!(
        dashboard.key(KeyCode::Char('y')),
        ovrcr::tui::DashboardAction::CopyText("ab".into())
    );
    let anchor = dashboard.copy.as_ref().and_then(|copy| copy.anchor);
    dashboard.finish_copy(Err(io::Error::other("denied")));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
    assert_eq!(dashboard.copy.as_ref().and_then(|copy| copy.anchor), anchor);
    assert!(
        dashboard
            .copy_notice
            .as_deref()
            .is_some_and(|notice| notice.contains("denied"))
    );
    dashboard.finish_copy(Ok(()));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Clipboard request sent; paste to verify")
    );
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        revision: dashboard.view_revision,
        bytes: b"\rNEW".to_vec(),
    }));
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("NEW")
    );
    assert_eq!(
        dashboard.copy.as_ref().unwrap().selected_text().as_deref(),
        Some("ab")
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("secret".into())),
        ovrcr::tui::DashboardAction::None
    );

    let mut pending_screen = dashboard_fixture();
    pending_screen
        .view_request(Rect::new(0, 0, 89, 38), 901)
        .unwrap()
        .expect("pending copy fixture should have an in-flight view");
    assert_eq!(
        pending_screen.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(pending_screen.mode, ovrcr::tui::InputMode::Browse);
    assert_eq!(
        pending_screen.error.as_deref(),
        Some("Waiting for terminal screen")
    );

    let mut failed_selection = copy_ready_dashboard();
    failed_selection.select_request(SessionId(5), 902);
    failed_selection.handle_server_message(ServerMessage::Response {
        request_id: 902,
        response: Response::Error {
            code: ErrorCode::NotFound,
            message: "selection failed".into(),
        },
    });
    assert_eq!(
        failed_selection.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(failed_selection.mode, ovrcr::tui::InputMode::Browse);
    assert_eq!(
        failed_selection.error.as_deref(),
        Some("Waiting for terminal screen")
    );

    let mut release = copy_ready_dashboard();
    assert_eq!(
        release.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(release.mode, ovrcr::tui::InputMode::Browse);
    assert!(release.copy.is_none());

    let mut repeat_motion = copy_ready_dashboard();
    assert_eq!(
        repeat_motion.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('['),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    let before = repeat_motion.copy.as_ref().unwrap().cursor;
    assert_eq!(
        repeat_motion.key_action(KeyEvent::new_with_kind(
            KeyCode::Right,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_ne!(repeat_motion.copy.as_ref().unwrap().cursor, before);

    let mut press_only = copy_ready_dashboard();
    press_only.key(KeyCode::Char('['));
    press_only.key(KeyCode::Home);
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::None
    );
    assert!(press_only.copy.as_ref().unwrap().anchor.is_none());
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(press_only.copy.as_ref().unwrap().anchor.is_some());
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Right,
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(
        press_only.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::CopyText("ab".into())
    );

    let mut press_exit = copy_ready_dashboard();
    press_exit.key(KeyCode::Char('['));
    assert_eq!(
        press_exit.key_action(KeyEvent::new_with_kind(
            KeyCode::Esc,
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(press_exit.mode, ovrcr::tui::InputMode::Copy);
    assert_eq!(
        press_exit.key_action(KeyEvent::new_with_kind(
            KeyCode::Esc,
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(press_exit.mode, ovrcr::tui::InputMode::Browse);
}

#[test]
fn matching_history_open_clears_resize_notice_but_stale_response_does_not() {
    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    let resize = dashboard
        .view_request(
            outer_area_for_pane(TerminalSize { rows: 20, cols: 40 }),
            901,
        )
        .unwrap()
        .expect("the new geometry should request a view");
    acknowledge_all_view_targets(&mut dashboard, resize);
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Copy cancelled: terminal resized")
    );

    let begin = match dashboard.key(KeyCode::PageUp) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected history begin request, got {action:?}"),
    };
    let stale = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id + 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert!(stale.is_empty());
    assert!(dashboard.history.is_none());
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Copy cancelled: terminal resized")
    );

    let opened = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);
    assert!(dashboard.history.is_some());
    assert!(dashboard.copy_notice.is_none());
    assert!(!opened.is_empty());

    dashboard.copy_notice = Some("stale replacement notice".into());
    dashboard.history_begin_request = Some(ovrcr::tui::PendingHistoryBegin {
        request_id: 77,
        session: SessionId(1),
        cancelled: false,
        at_tail: false,
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 77,
        response: Response::HistoryOpened(HistoryOpened {
            snapshot: HistorySnapshotId(8),
            ..history_opened(100)
        }),
    });
    assert!(dashboard.copy_notice.is_none());
    assert_eq!(
        dashboard.history.as_ref().unwrap().opened.snapshot,
        HistorySnapshotId(8)
    );
}

#[test]
fn copy_render_highlights_unicode_and_preserves_layout() {
    let mut parser = vt100::Parser::new(2, 6, 0);
    parser.process("\x1b[?25lA界B".as_bytes());
    let mut selection = CopySelection::capture(SessionId(1), parser.screen());
    selection.cursor = CopyPoint { row: 0, col: 1 };
    selection.anchor = Some(selection.cursor);
    assert!(selection.screen.hide_cursor());
    assert!(selection.contains(CopyPoint { row: 0, col: 1 }));
    assert!(selection.contains(CopyPoint { row: 0, col: 2 }));

    let base = Color::Rgb(30, 30, 46);
    let teal = Color::Rgb(148, 226, 213);
    let text = Color::Rgb(205, 214, 244);
    let mut terminal = Terminal::new(TestBackend::new(6, 2)).unwrap();
    terminal
        .draw(|frame| {
            render_copy(frame, Rect::new(0, 0, 6, 2), &selection);
            let buffer = frame.buffer_mut();
            assert_eq!(buffer[(1, 0)].fg, base);
            assert_eq!(buffer[(1, 0)].bg, teal);
            assert_eq!(buffer[(2, 0)].fg, base);
            assert_eq!(buffer[(2, 0)].bg, teal);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].symbol(), "A");
    assert_eq!(buffer[(0, 0)].fg, text);
    assert_eq!(buffer[(0, 0)].bg, base);
    assert_eq!(buffer[(1, 0)].symbol(), "界");
    assert_eq!(buffer[(3, 0)].symbol(), "B");
    assert_eq!(buffer[(3, 0)].fg, text);
    assert_eq!(buffer[(3, 0)].bg, base);
    assert_eq!(
        terminal.backend_mut().get_cursor_position().unwrap(),
        Position::new(1, 0)
    );

    let mut output = Vec::new();
    {
        let backend = CrosstermBackend::new(&mut output);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 6, 2)),
            },
        )
        .unwrap();
        terminal
            .draw(|frame| render_copy(frame, Rect::new(0, 0, 6, 2), &selection))
            .unwrap();
    }
    let mut rendered = vt100::Parser::new(2, 6, 0);
    rendered.process(&output);
    assert_eq!(rendered.screen().cell(0, 1).unwrap().contents(), "界");
    assert_eq!(rendered.screen().cell(0, 3).unwrap().contents(), "B");
    assert_eq!(rendered.screen().cell(0, 4).unwrap().contents(), " ");
}

#[test]
fn copy_render_tiny_pane_and_footer() {
    let mut tiny = copy_ready_dashboard();
    tiny.key(KeyCode::Char('['));
    let mut tiny_terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
    tiny_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &tiny))
        .unwrap();

    let mut dashboard = copy_ready_dashboard();
    let sidebar_and_metadata = |buffer: &ratatui::buffer::Buffer| {
        (1..3)
            .map(|row| {
                (0..120)
                    .map(|col| buffer[(col, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let initial_layout = sidebar_and_metadata(terminal.backend().buffer());
    dashboard.key(KeyCode::Char('['));
    let id = dashboard.focused_session().unwrap();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        revision: dashboard.view_revision,
        bytes: b"\rLIVE".to_vec(),
    }));
    assert_eq!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .cell(0, 0)
            .unwrap()
            .contents(),
        "L"
    );
    assert_eq!(
        dashboard
            .copy
            .as_ref()
            .unwrap()
            .screen
            .cell(0, 0)
            .unwrap()
            .contents(),
        "a"
    );
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let footer = (0..120)
        .map(|col| buffer[(col, 39)].symbol())
        .collect::<String>();
    assert!(footer.starts_with("COPY  Space leader  ? help"));
    assert!(footer.contains(" h j k l "));
    assert!(footer.contains(" v y Enter Esc q Ctrl-g"));
    let rendered_layout = sidebar_and_metadata(buffer);
    assert_eq!(rendered_layout, initial_layout);
    assert!(initial_layout[0].contains("pid: 111  elapsed: 0m"));
    assert!(initial_layout[1].contains("─"));
    assert_eq!(buffer[(39, 1)].symbol(), "│");
    assert_eq!(buffer[(39, 10)].symbol(), "│");
    assert_eq!(buffer[(40, 1)].symbol(), "p");
    assert_eq!(buffer[(40, 2)].symbol(), "─");
    assert_eq!(buffer[(40, 3)].symbol(), "a");
    assert_eq!(buffer[(41, 3)].symbol(), "b");
    assert_eq!(buffer[(42, 3)].symbol(), "c");

    dashboard.copy_notice = Some("Copy cancelled: terminal resized".into());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let notice_footer = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 39)].symbol())
        .collect::<String>();
    assert_eq!(notice_footer.trim_end(), "Copy cancelled: terminal resized");

    dashboard.error = Some("server failed".into());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let error_footer = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 39)].symbol())
        .collect::<String>();
    assert_eq!(error_footer.trim_end(), "ERROR: server failed");

    dashboard.error = None;
    dashboard.cancel_copy(Some("Copy cancelled: terminal resized"));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let browse_notice_footer = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 39)].symbol())
        .collect::<String>();
    assert_eq!(
        browse_notice_footer.trim_end(),
        "Copy cancelled: terminal resized"
    );

    let mut entry_notice = dashboard_fixture();
    entry_notice.panes[entry_notice.focused_pane].ready = false;
    assert_eq!(
        entry_notice.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        entry_notice.error.as_deref(),
        Some("Waiting for terminal screen")
    );
    entry_notice.error = None;
    entry_notice.copy_notice = Some("Waiting for terminal screen".into());
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &entry_notice, 0))
        .unwrap();
    let entry_notice_footer = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 39)].symbol())
        .collect::<String>();
    assert_eq!(
        entry_notice_footer.trim_end(),
        "Waiting for terminal screen"
    );
}

#[test]
fn copy_mode_cancels_at_identity_boundaries() {
    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Esc);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.copy.is_none());

    let mut dashboard = copy_ready_dashboard();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    assert!(
        dashboard
            .history_begin_request
            .as_ref()
            .is_some_and(|pending| !pending.cancelled)
    );
    dashboard.key(KeyCode::Char('['));
    assert!(
        dashboard
            .history_begin_request
            .as_ref()
            .is_some_and(|pending| pending.cancelled)
    );
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
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
    assert!(dashboard.copy.is_some());

    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::PageUp);
    dashboard.key(KeyCode::Char('['));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "cancelled".into(),
        },
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
    assert!(dashboard.copy.is_some());

    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    dashboard.select_session(SessionId(5));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.copy.is_none());

    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    dashboard
        .view_request(
            outer_area_for_pane(TerminalSize { rows: 20, cols: 40 }),
            901,
        )
        .unwrap()
        .expect("the new geometry should request a view");
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.copy.is_none());
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Copy cancelled: terminal resized")
    );

    for key in [KeyCode::Char('q'), KeyCode::Char('\u{7}')] {
        let mut dashboard = copy_ready_dashboard();
        dashboard.key(KeyCode::Char('['));
        assert_eq!(dashboard.key(key), ovrcr::tui::DashboardAction::Redraw);
        assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
        assert!(dashboard.copy.is_none());
    }
    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    assert_eq!(dashboard.ctrl('g'), ovrcr::tui::DashboardAction::Redraw);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.copy.is_none());

    let mut dashboard = copy_ready_dashboard();
    dashboard.key(KeyCode::Char('['));
    let hierarchy = dashboard.hierarchy.clone();
    let mut removed = hierarchy;
    removed.projects[1].workspaces[1]
        .sessions
        .retain(|session| session.id != SessionId(1));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(removed)));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.copy.is_none());
}

#[test]
fn copy_mode_waits_for_matching_screen() {
    let mut dashboard = dashboard_fixture();
    let first = dashboard.focused_session().unwrap();
    dashboard
        .view_request(Rect::new(0, 0, 89, 38), 901)
        .unwrap()
        .expect("copy readiness test should request a refreshed view");
    dashboard.select_session(SessionId(5));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Screen {
            session: first,
            revision: dashboard.view_revision,
            size: dashboard.panes[dashboard.focused_pane].desired_size,
            bytes: b"late-first".to_vec(),
        },
    });
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("late-first")
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Screen {
            session: SessionId(5),
            revision: dashboard.view_revision,
            size: dashboard.panes[dashboard.focused_pane].desired_size,
            bytes: b"old-request".to_vec(),
        },
    });
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("old-request")
    );
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Ok,
    });
    assert_eq!(requests.len(), 1);
    let request_id = requests[0].request_id;
    let revision = dashboard.view_revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Screen {
            session: SessionId(5),
            revision,
            size: dashboard.panes[dashboard.focused_pane].desired_size,
            bytes: b"matching".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Ok,
    });
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("matching")
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
}

#[test]
fn copy_mode_keeps_alternate_and_resync_snapshots() {
    let mut dashboard = dashboard_fixture();
    let id = dashboard.focused_session().unwrap();
    dashboard
        .view_request(Rect::new(0, 0, 89, 38), 903)
        .unwrap()
        .expect("alternate screen test should request a refreshed view");
    let size = dashboard.panes[dashboard.focused_pane].desired_size;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 903,
        response: Response::Screen {
            session: id,
            revision: dashboard.view_revision,
            size,
            bytes: b"\x1b[?1049hALT".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 903,
        response: Response::Ok,
    });
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Home);
    dashboard.key(KeyCode::Char('v'));
    dashboard.key(KeyCode::Right);
    let requests =
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session: id,
            revision: dashboard.view_revision,
        }));
    let view_revision = dashboard.view_revision;
    assert!(matches!(
        requests.as_slice(),
        [ClientMessage {
            request: Request::SetView { view },
            ..
        }] if view.revision == view_revision
            && view.focused == Some(id)
    ));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: requests[0].request_id,
        response: Response::Screen {
            session: id,
            revision: view_revision,
            size,
            bytes: b"\x1b[?1049lPRIMARY".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: requests[0].request_id,
        response: Response::Ok,
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        revision: view_revision,
        bytes: b"-LIVE".to_vec(),
    }));
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("PRIMARY-LIVE")
    );
    assert_eq!(
        dashboard.copy.as_ref().unwrap().selected_text().as_deref(),
        Some("AL")
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
}

#[test]
fn pause_resume_browse_keys_send_explicit_requests() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    acknowledge_focused_view(&mut dashboard, 900);

    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request_id: 1,
            request: Request::PauseSession {
                session: SessionId(5),
            },
        })
    );
    assert_eq!(
        dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase,
        SessionPhase::Running
    );

    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request_id: 2,
            request: Request::PauseSession {
                session: SessionId(5),
            },
        })
    );

    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase = SessionPhase::Paused;
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request_id: 3,
            request: Request::ResumeSession {
                session: SessionId(5),
            },
        })
    );

    dashboard.panes[dashboard.focused_pane].session = None;
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        dashboard
            .error
            .as_deref()
            .is_some_and(|error| error.contains("selected"))
    );

    dashboard.select_session(SessionId(2));
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        dashboard
            .error
            .as_deref()
            .is_some_and(|error| error.contains("exited"))
    );

    dashboard.select_session(SessionId(5));
    acknowledge_focused_view(&mut dashboard, 901);
    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase = SessionPhase::Running;
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'p'])
    );
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'r'])
    );
}

#[test]
fn pause_resume_input_and_paste_stay_guarded() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase = SessionPhase::Paused;

    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert_eq!(
        dashboard.error.as_deref(),
        Some("Session paused; press r to resume")
    );

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.event_action(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("blocked".into())),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        event_to_request(
            &mut dashboard,
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            100,
        )
        .is_none()
    );
    assert!(event_to_request(&mut dashboard, Event::Paste("blocked".into()), 101).is_none());
    assert!(dashboard.input_request(b"blocked".to_vec(), 99).is_none());
    assert!(
        dashboard.event_action(Event::Key(KeyEvent::new(
            KeyCode::Char('g'),
            KeyModifiers::CONTROL,
        ))) == ovrcr::tui::DashboardAction::EnterBrowse
    );

    let paused = dashboard.hierarchy.projects[1].workspaces[1].sessions[1].clone();
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        paused,
    ))));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        dashboard.hierarchy.clone(),
    )));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
}

#[test]
fn history_navigation_never_writes_to_pty() {
    let mut dashboard = dashboard_fixture();
    let begin = dashboard.key(KeyCode::PageUp);
    assert_eq!(
        begin,
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request_id: 1,
            request: Request::HistoryBegin {
                session: SessionId(1),
            },
        })
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[5~".to_vec())
    );

    dashboard.mode = ovrcr::tui::InputMode::Browse;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(HistoryOpened {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            revision: 1,
            size: TerminalSize { rows: 10, cols: 20 },
            history_rows: 90,
            total_rows: 100,
        }),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);

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
        assert!(!matches!(
            dashboard.key(key),
            ovrcr::tui::DashboardAction::PtyBytes(_)
        ));
    }
    assert!(!matches!(
        dashboard.event_action(Event::Paste("blocked".into())),
        ovrcr::tui::DashboardAction::PtyBytes(_)
    ));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);
    assert!(!matches!(
        dashboard.key(KeyCode::End),
        ovrcr::tui::DashboardAction::PtyBytes(_)
    ));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);
    assert_eq!(
        dashboard.key(KeyCode::Char('q')),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);

    dashboard.key(KeyCode::Char(':'));
    dashboard.event_action(Event::Paste("spacelift-agent progress local".into()));
    let ovrcr::tui::DashboardAction::RequestBatch(requests) = dashboard.key(KeyCode::Enter) else {
        panic!("palette switch did not preserve pending history release");
    };
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
        ] if view.focused == Some(SessionId(3))
            && view.panes.iter().any(|pane| pane.session == SessionId(3))
    ));
}

#[test]
fn history_begin_is_cancelled_and_captured_by_active_overlays() {
    for overlay in ["palette", "tasks"] {
        let mut dashboard = dashboard_fixture();
        assert!(matches!(
            dashboard.key(KeyCode::PageUp),
            ovrcr::tui::DashboardAction::Request(ClientMessage {
                request: Request::HistoryBegin {
                    session: SessionId(1)
                },
                ..
            })
        ));
        if overlay == "palette" {
            assert!(matches!(
                dashboard.key(KeyCode::Char(':')),
                ovrcr::tui::DashboardAction::Request(ClientMessage {
                    request: Request::Inspect,
                    ..
                })
            ));
        } else {
            assert_eq!(dashboard.ctrl('t'), ovrcr::tui::DashboardAction::Redraw);
        }
        assert!(
            dashboard
                .history_begin_request
                .as_ref()
                .is_some_and(|pending| pending.cancelled)
        );
        assert_eq!(
            dashboard.key(KeyCode::PageUp),
            ovrcr::tui::DashboardAction::Redraw
        );
        assert_eq!(
            dashboard.key(KeyCode::Esc),
            ovrcr::tui::DashboardAction::Redraw
        );
        let requests = dashboard.handle_server_message(ServerMessage::Response {
            request_id: 1,
            response: Response::HistoryOpened(history_opened(100)),
        });
        assert!(dashboard.history.is_none());
        assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
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
    dashboard.key(KeyCode::PageUp);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    let mut hierarchy = dashboard.hierarchy.clone();
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
    assert!(dashboard.history.is_none());
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
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
    let mut dashboard = screen_ready_dashboard(b"");
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    let page_request = requests.first().cloned().expect("initial history page");
    let (start_row, start_col, rows, request_id) = match page_request.request {
        Request::HistoryPage {
            start_row,
            start_col,
            rows,
            ..
        } => (start_row, start_col, rows, page_request.request_id),
        request => panic!("unexpected history request: {request:?}"),
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::HistoryRows(history_complete_page(
            start_row,
            start_col,
            rows,
            20,
            vec![history_cell("frozen", 1)],
        )),
    });
    let before = dashboard.history.clone().expect("history opened");
    let current_revision = dashboard.view_revision;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        revision: current_revision,
        bytes: b"LIVE_MARKER".to_vec(),
    }));
    let after = dashboard.history.as_ref().expect("history remains open");
    assert_eq!(after.opened, before.opened);
    assert_eq!(after.top, before.top);
    assert_eq!(after.left, before.left);
    assert_eq!(after.pages, before.pages);
    assert!(after.new_output);
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("LIVE_MARKER")
    );
    let select = dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
        session: SessionId(1),
        revision: current_revision,
    }));
    assert!(matches!(
        select.as_slice(),
        [ClientMessage {
            request: Request::SetView { view },
            ..
        }] if view.focused == Some(SessionId(1))
            && view.panes.iter().any(|pane| pane.session == SessionId(1))
    ));
    let before_screen = dashboard.history.clone().expect("history remains open");
    let view_revision = dashboard.view_revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: select[0].request_id,
        response: Response::Screen {
            session: SessionId(1),
            revision: view_revision,
            size: TerminalSize { rows: 12, cols: 30 },
            bytes: b"SCREEN_MARKER".to_vec(),
        },
    });
    let after_screen = dashboard.history.as_ref().expect("history remains open");
    assert_eq!(after_screen.opened, before_screen.opened);
    assert_eq!(after_screen.top, before_screen.top);
    assert_eq!(after_screen.left, before_screen.left);
    assert_eq!(after_screen.pages, before_screen.pages);
    assert_eq!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .size(),
        (12, 30)
    );
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("SCREEN_MARKER")
    );
}

#[test]
fn history_resize_preserves_capture() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::PageUp);
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    let page_request = requests.first().cloned().expect("initial history page");
    let (start_row, start_col, rows) = match page_request.request {
        Request::HistoryPage {
            start_row,
            start_col,
            rows,
            ..
        } => (start_row, start_col, rows),
        request => panic!("unexpected history request: {request:?}"),
    };
    let mut follow_up = dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(history_complete_page(
            start_row,
            start_col,
            rows,
            20,
            vec![history_cell("old", 1)],
        )),
    });
    let mut captured_anchor = false;
    while let Some(request) = follow_up.pop() {
        let (start_row, start_col, rows, cols) = match request.request {
            Request::HistoryPage {
                start_row,
                start_col,
                rows,
                cols,
                ..
            } => (start_row, start_col, rows, cols),
            request => panic!("unexpected history request: {request:?}"),
        };
        captured_anchor |= start_row == 48;
        follow_up = dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::HistoryRows(history_tile_page(start_row, start_col, rows, cols)),
        });
    }
    assert!(captured_anchor);
    dashboard.history.as_mut().unwrap().top = 60;
    dashboard.history.as_mut().unwrap().left = 4;
    let before = dashboard.history.clone().expect("history opened");
    assert!(before.pages.iter().any(|page| page.start_row == 48));
    let resize = dashboard
        .view_request(Rect::new(0, 0, 70, 54), 99)
        .unwrap()
        .expect("live resize request");
    assert!(matches!(resize.request, Request::SetView { .. }));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Screen {
            session: SessionId(1),
            revision: dashboard.view_revision,
            size: TerminalSize { rows: 50, cols: 30 },
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Ok,
    });
    let after = dashboard.history.as_ref().expect("history remains open");
    assert_eq!(after.opened, before.opened);
    assert_eq!(after.pages, before.pages);
    assert_eq!(after.top, before.top);
    assert_eq!(after.left, before.left);
    assert_eq!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .size(),
        (50, 30)
    );
}

#[test]
fn history_stale_response_is_ignored() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::PageUp);
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    let page_request = requests.first().cloned().expect("initial history page");
    let pending = dashboard
        .history
        .as_ref()
        .and_then(|view| view.pending.clone())
        .expect("history page pending");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id.wrapping_add(1),
        response: Response::HistoryRows(history_complete_page(
            pending.start_row,
            pending.start_col,
            pending.rows,
            20,
            vec![],
        )),
    });
    assert_eq!(
        dashboard.history.as_ref().unwrap().pending.as_ref(),
        Some(&pending)
    );
    assert!(dashboard.history.as_ref().unwrap().pages.is_empty());
    for page in [
        {
            let mut page = history_page(pending.start_row, pending.start_col, 20, vec![]);
            page.session = SessionId(5);
            page
        },
        {
            let mut page = history_page(pending.start_row, pending.start_col, 20, vec![]);
            page.snapshot = HistorySnapshotId(8);
            page
        },
        history_page(
            pending.start_row.saturating_add(16),
            pending.start_col,
            20,
            vec![],
        ),
    ] {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: pending.request_id,
            response: Response::HistoryRows(page),
        });
        let view = dashboard.history.as_ref().expect("history remains pending");
        assert_eq!(view.pending.as_ref(), Some(&pending));
        assert!(view.pages.is_empty());
    }
    let accepted = dashboard.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::HistoryRows(history_complete_page(
            pending.start_row,
            pending.start_col,
            pending.rows,
            pending.start_col,
            vec![],
        )),
    });
    assert_eq!(dashboard.history.as_ref().unwrap().pages.len(), 1);
    assert!(accepted.len() <= 1);
    dashboard.select_session(SessionId(5));
    assert!(dashboard.history.is_none());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(history_page(0, 0, 20, vec![history_cell("old", 1)])),
    });
    assert!(dashboard.history.is_none());
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
}

#[test]
fn history_empty_rows_only_anchor_after_explicit_anchor() {
    let mut dashboard = dashboard_fixture();
    dashboard.mode = ovrcr::tui::InputMode::History;
    let mut fresh = HistoryView::new(history_opened(1), 0);
    fresh.pages.push_back(history_page(0, 0, 0, vec![]));
    dashboard.history = Some(fresh);
    assert!(dashboard.history_request_if_needed().is_none());
    assert!(dashboard.history.as_ref().unwrap().cursor.is_none());
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        None,
        "fresh empty row remains unanchorable"
    );
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('v'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        )),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert_eq!(
        dashboard.copy_notice.as_deref(),
        Some("Nothing to select on this row")
    );

    let mut anchored = HistoryView::new(history_opened(1), 0);
    anchored.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    anchored.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 0,
        col: 0,
    }));
    anchored.pages.push_back(history_page(0, 0, 0, vec![]));
    dashboard.history = Some(anchored);
    assert!(dashboard.history_request_if_needed().is_none());
    assert_eq!(
        dashboard.history.as_ref().unwrap().cursor.unwrap().point,
        HistoryCopyPoint { row: 0, col: 0 }
    );
    assert!(dashboard.history.as_ref().unwrap().cursor_target.is_none());

    let mut traversing = HistoryView::new(history_opened(3), 0);
    traversing.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    traversing.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 0 },
        row_width: 4,
        cell_width: 1,
    });
    traversing.cursor_target = None;
    traversing.pages.push_back(HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        start_col: 0,
        rows: vec![
            HistoryRow {
                width: 4,
                cells: vec![
                    history_cell("A", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                ],
                wrapped: false,
            },
            HistoryRow {
                width: 0,
                cells: Vec::new(),
                wrapped: false,
            },
            HistoryRow {
                width: 2,
                cells: vec![history_cell("B", 1), history_cell(" ", 1)],
                wrapped: false,
            },
        ],
    });
    dashboard.history = Some(traversing);
    assert!(matches!(
        dashboard.key(KeyCode::Down),
        ovrcr::tui::DashboardAction::Redraw
    ));
    let empty_cursor = dashboard.history.as_ref().unwrap().cursor.unwrap();
    assert_eq!(empty_cursor.point, HistoryCopyPoint { row: 1, col: 0 });
    assert_eq!(empty_cursor.row_width, 0);
    dashboard.history.as_mut().unwrap().top = 1;
    let mut empty_terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
    empty_terminal
        .draw(|frame| {
            ovrcr::tui::render_history(
                frame,
                Rect::new(0, 0, 1, 1),
                dashboard.history.as_ref().unwrap(),
            )
        })
        .unwrap();
    assert_eq!(empty_terminal.backend().buffer()[(0, 0)].symbol(), " ");
    assert_eq!(
        empty_terminal.backend().buffer()[(0, 0)].bg,
        Color::Rgb(30, 30, 46)
    );
    assert!(!empty_terminal.backend().cursor_visible());
    assert!(matches!(
        dashboard.key(KeyCode::Down),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert_eq!(
        dashboard.history.as_ref().unwrap().cursor.unwrap().point,
        HistoryCopyPoint { row: 2, col: 0 }
    );
    assert!(matches!(
        dashboard.key(KeyCode::Up),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert_eq!(
        dashboard.history.as_ref().unwrap().copy_range(),
        Some(HistoryCopyRange {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 1, col: 0 },
        })
    );
}

#[test]
fn history_cursor_stays_visible_while_predecessor_tile_loads() {
    let mut view = HistoryView::new(history_opened(40), 0);
    view.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 0 },
        row_width: 4,
        cell_width: 1,
    });
    view.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 17,
        col: 2,
    }));
    view.pages.push_back(history_page(
        0,
        0,
        4,
        vec![
            history_cell("old", 1),
            history_cell("", 1),
            history_cell("", 1),
            history_cell("", 1),
        ],
    ));
    assert!(
        !view
            .resolve_cursor(TerminalSize { rows: 4, cols: 8 })
            .unwrap()
    );
    assert_eq!(
        view.cursor.unwrap().point,
        HistoryCopyPoint { row: 0, col: 0 }
    );
    assert!(view.cursor_target.is_some());
}

#[test]
fn history_cursor_resolution_reveals_anchored_viewport() {
    let mut view = HistoryView::new(history_opened(40), 0);
    view.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    view.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 17,
        col: 3,
    }));
    view.pages.push_back(history_complete_page(
        16,
        0,
        16,
        8,
        vec![history_cell("", 1); 8],
    ));
    assert!(
        view.resolve_cursor(TerminalSize { rows: 4, cols: 4 })
            .unwrap()
    );
    assert_eq!(view.top, 14);
    assert_eq!(view.left, 0);
    assert_eq!(
        view.cursor.unwrap().point,
        HistoryCopyPoint { row: 17, col: 3 }
    );

    let mut unanchored = HistoryView::new(history_opened(2), 0);
    unanchored.left = 128;
    unanchored.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 0,
        col: 128,
    }));
    unanchored.pages.push_back(history_page(
        0,
        127,
        130,
        vec![
            history_cell("界", 2),
            history_cell("", 0),
            history_cell("x", 1),
        ],
    ));
    assert!(
        unanchored
            .resolve_cursor(TerminalSize { rows: 2, cols: 2 })
            .unwrap()
    );
    assert_eq!(unanchored.left, 127);
    assert_eq!(
        unanchored.cursor.unwrap().point,
        HistoryCopyPoint { row: 0, col: 127 }
    );
}

#[test]
fn history_cursor_async_continuation_resolution_reveals_predecessor_tile() {
    let mut dashboard = dashboard_fixture();
    dashboard.mode = ovrcr::tui::InputMode::History;
    let mut view = HistoryView::new(history_opened(1), 0);
    view.left = 128;
    view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 0 },
        row_width: 130,
        cell_width: 1,
    });
    view.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
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
    assert!(view.cursor_target.is_none());
}

#[test]
fn history_key_kinds_gate_actions() {
    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    let top_before = dashboard.history.as_ref().unwrap().top;

    let repeat_enter =
        KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Repeat);
    assert!(matches!(
        dashboard.key_action(repeat_enter),
        ovrcr::tui::DashboardAction::None
    ));
    assert_eq!(dashboard.history.as_ref().unwrap().top, top_before);

    let release_down =
        KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Release);
    assert!(matches!(
        dashboard.key_action(release_down),
        ovrcr::tui::DashboardAction::None
    ));
    assert_eq!(dashboard.history.as_ref().unwrap().top, top_before);

    let repeat_escape =
        KeyEvent::new_with_kind(KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Repeat);
    assert!(matches!(
        dashboard.key_action(repeat_escape),
        ovrcr::tui::DashboardAction::None
    ));
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);

    let press_escape =
        KeyEvent::new_with_kind(KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Press);
    assert!(matches!(
        dashboard.key_action(press_escape),
        ovrcr::tui::DashboardAction::EnterBrowse
    ));
}

#[test]
fn history_pending_keys_coalesce() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::PageUp);
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(400)),
    });
    let page_request = requests.first().cloned().expect("initial history page");
    assert!(matches!(
        dashboard.event_action(Event::Paste("paste while browsing history".into())),
        ovrcr::tui::DashboardAction::None
    ));
    let pending_before = dashboard
        .history
        .as_ref()
        .and_then(|view| view.pending.clone())
        .expect("one page pending");
    for _ in 0..100 {
        assert!(!matches!(
            dashboard.key(KeyCode::PageUp),
            ovrcr::tui::DashboardAction::Request(_)
        ));
    }
    let pending_after = dashboard
        .history
        .as_ref()
        .and_then(|view| view.pending.clone())
        .expect("one page remains pending");
    assert_eq!(pending_after.request_id, pending_before.request_id);
    assert_eq!(dashboard.history.as_ref().unwrap().pages.len(), 0);
    let (start_row, start_col) = match page_request.request {
        Request::HistoryPage {
            start_row,
            start_col,
            ..
        } => (start_row, start_col),
        request => panic!("unexpected history request: {request:?}"),
    };
    let follow_up = dashboard.handle_server_message(ServerMessage::Response {
        request_id: page_request.request_id,
        response: Response::HistoryRows(history_complete_page(
            start_row,
            start_col,
            pending_before.rows,
            20,
            vec![history_cell("tile", 1)],
        )),
    });
    assert_eq!(dashboard.history.as_ref().unwrap().pages.len(), 1);
    assert_eq!(follow_up.len(), 1);
    assert!(dashboard.history.as_ref().unwrap().pending.is_some());
    assert!(dashboard.history.as_ref().unwrap().pages.len() <= 16);

    let retry_id = dashboard
        .history
        .as_ref()
        .unwrap()
        .pending
        .as_ref()
        .unwrap()
        .request_id;
    let blocked = dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "history tile missing".into(),
        },
    });
    assert!(blocked.is_empty());
    assert!(dashboard.history.as_ref().unwrap().pending.is_none());
    assert!(
        dashboard
            .error
            .as_deref()
            .is_some_and(|error| error.contains("missing"))
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry_id,
        response: Response::Ok,
    });
    assert!(
        dashboard
            .error
            .as_deref()
            .is_some_and(|error| error.contains("missing"))
    );
    let retry = match dashboard.key(KeyCode::Down) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected explicit retry request, got {action:?}"),
    };
    assert!(dashboard.error.is_none());

    let latest = dashboard.history.as_ref().unwrap().pending.clone().unwrap();
    let latest_top = dashboard.history.as_ref().unwrap().top;
    assert_eq!(latest.start_row, (latest_top / 16) * 16);
    assert_eq!(latest.start_col, 0);

    let initial_page = dashboard
        .history
        .as_ref()
        .unwrap()
        .pages
        .front()
        .unwrap()
        .clone();
    dashboard.panes[dashboard.focused_pane].size = TerminalSize {
        rows: 64,
        cols: 256,
    };
    dashboard.history.as_mut().unwrap().top = 1;
    dashboard.history.as_mut().unwrap().left = 1;
    let mut next = ovrcr::tui::DashboardAction::Request(retry);
    let mut starts = Vec::new();
    while let ovrcr::tui::DashboardAction::Request(request) = next {
        let (start_row, start_col, rows, cols) = match request.request {
            Request::HistoryPage {
                start_row,
                start_col,
                rows,
                cols,
                ..
            } => (start_row, start_col, rows, cols),
            request => panic!("unexpected history request: {request:?}"),
        };
        starts.push((start_row, start_col));
        next = dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: request.request_id,
                response: Response::HistoryRows(history_tile_page(
                    start_row, start_col, rows, cols,
                )),
            })
            .into_iter()
            .next()
            .map(ovrcr::tui::DashboardAction::Request)
            .unwrap_or(ovrcr::tui::DashboardAction::Redraw);
    }
    let expected_tiles = (0..5)
        .flat_map(|row| (0..3).map(move |col| (row * 16, col * 128)))
        .collect::<std::collections::BTreeSet<_>>();
    let cached_tiles = dashboard
        .history
        .as_ref()
        .unwrap()
        .pages
        .iter()
        .map(|page| (page.start_row, page.start_col))
        .collect::<std::collections::BTreeSet<_>>();
    assert!(expected_tiles.is_subset(&cached_tiles));
    assert!(starts.iter().any(|(row, col)| *row == 0 && *col == 0));
    assert!(starts.iter().any(|(row, col)| *row == 64 && *col == 256));
    dashboard.history.as_mut().unwrap().top = 200;
    dashboard.history.as_mut().unwrap().left = 0;
    let eviction = match dashboard.key(KeyCode::Down) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected eviction request, got {action:?}"),
    };
    let follow_eviction = dashboard.handle_server_message(ServerMessage::Response {
        request_id: eviction.request_id,
        response: Response::HistoryRows(history_tile_page(192, 0, 16, 128)),
    });
    let eviction_two = follow_eviction
        .first()
        .cloned()
        .expect("next eviction tile");
    let (start_row, start_col, rows, cols) = match eviction_two.request {
        Request::HistoryPage {
            start_row,
            start_col,
            rows,
            cols,
            ..
        } => (start_row, start_col, rows, cols),
        request => panic!("unexpected eviction request: {request:?}"),
    };
    let second_page = history_tile_page(start_row, start_col, rows, cols);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: eviction_two.request_id,
        response: Response::HistoryRows(second_page.clone()),
    });
    assert_eq!(dashboard.history.as_ref().unwrap().pages.len(), 16);
    assert!(
        dashboard
            .history
            .as_ref()
            .unwrap()
            .pages
            .contains(&second_page)
    );
    assert!(
        !dashboard
            .history
            .as_ref()
            .unwrap()
            .pages
            .contains(&initial_page)
    );
}

#[test]
fn history_unanchored_retry_keys_reissue_failed_cursor_request() {
    for malformed in [false, true] {
        for retry_key in [KeyCode::Char('v'), KeyCode::Char('y')] {
            let mut dashboard = dashboard_fixture();
            assert!(matches!(
                dashboard.key(KeyCode::PageUp),
                ovrcr::tui::DashboardAction::Request(_)
            ));
            let requests = dashboard.handle_server_message(ServerMessage::Response {
                request_id: 1,
                response: Response::HistoryOpened(history_opened(40)),
            });
            let failed = requests.first().cloned().expect("cursor page request");
            let failed_bounds = match failed.request {
                Request::HistoryPage {
                    start_row,
                    start_col,
                    rows,
                    cols,
                    ..
                } => (start_row, start_col, rows, cols),
                request => panic!("unexpected cursor request: {request:?}"),
            };
            let failure_response = if malformed {
                Response::HistoryRows(HistoryRows {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(7),
                    start_row: failed_bounds.0,
                    start_col: failed_bounds.1,
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
                ovrcr::tui::DashboardAction::Request(request) => request,
                action => {
                    panic!("expected explicit cursor retry for {retry_key:?}, got {action:?}")
                }
            };
            assert!(matches!(retry.request, Request::HistoryPage { .. }));
            let follow_up = dashboard.handle_server_message(ServerMessage::Response {
                request_id: retry.request_id,
                response: Response::HistoryRows(history_complete_page(
                    failed_bounds.0,
                    failed_bounds.1,
                    failed_bounds.2,
                    20,
                    vec![history_cell("resolved", 1)],
                )),
            });
            assert!(
                follow_up
                    .iter()
                    .all(|request| matches!(request.request, Request::HistoryPage { .. }))
            );
            let view = dashboard.history.as_ref().unwrap();
            assert!(view.cursor_target.is_none());
            assert_eq!(view.cursor.unwrap().point.row, failed_bounds.0);
        }
    }
}

#[test]
fn copy_history_range_survives_live_eviction() {
    let (mut live_parser, mut frozen, old_lines) = numbered_history();
    let opened = frozen.opened().clone();
    let mut dashboard = screen_ready_dashboard(b"");
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(opened.clone()),
    });
    pump_frozen_history(&mut dashboard, &mut frozen, requests);

    pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Home);
    for _ in 0..15 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Down);
    }
    for _ in 0..126 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Right);
    }
    let anchor_action = dashboard.key(KeyCode::Char('v'));
    assert_eq!(anchor_action, ovrcr::tui::DashboardAction::Redraw);
    let anchored = dashboard.history.as_ref().unwrap().anchor;
    assert_eq!(anchored, Some(HistoryCopyPoint { row: 15, col: 126 }));

    for _ in 0..18 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Down);
    }
    for _ in 0..3 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Right);
    }
    let range = dashboard.history.as_ref().unwrap().copy_range().unwrap();
    assert_eq!(range.anchor, HistoryCopyPoint { row: 15, col: 126 });
    assert_eq!(range.cursor, HistoryCopyPoint { row: 33, col: 129 });
    let view = dashboard.history.as_ref().unwrap();
    let cursor = view.cursor.unwrap();
    assert!(cursor.point.row >= view.top);
    assert!(
        cursor.point.row
            < view
                .top
                .saturating_add(dashboard.panes[dashboard.focused_pane].size.rows as u32)
    );
    assert!(cursor.point.col >= view.left);
    assert!(
        u32::from(cursor.point.col) + u32::from(cursor.cell_width)
            <= u32::from(view.left) + u32::from(dashboard.panes[dashboard.focused_pane].size.cols)
    );

    let before_resize = dashboard.history.as_ref().unwrap().copy_range();
    let resize = dashboard
        .view_request(
            outer_area_for_pane(TerminalSize { rows: 30, cols: 70 }),
            900,
        )
        .unwrap()
        .expect("resize should request a view");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Screen {
            session: SessionId(1),
            revision: dashboard.view_revision,
            size: TerminalSize { rows: 30, cols: 70 },
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        dashboard.history.as_ref().unwrap().copy_range(),
        before_resize
    );
    assert_eq!(
        dashboard.history.as_ref().unwrap().opened.snapshot,
        opened.snapshot
    );

    let live_bytes = (0..600)
        .map(|row| format!("LIVE_{row:03}\r\n"))
        .collect::<String>();
    live_parser.process(live_bytes.as_bytes());
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        revision: 0,
        bytes: live_bytes.into_bytes(),
    }));
    assert!(dashboard.history.as_ref().unwrap().new_output);
    let refreshed = FrozenHistory::capture(
        SessionId(1),
        HistorySnapshotId(8),
        2,
        live_parser.screen().clone(),
    )
    .unwrap();
    let refreshed_page = {
        let mut refreshed = refreshed;
        refreshed.page(15, 1, 0, 128).unwrap()
    };
    let refreshed_row = refreshed_page.rows[0]
        .cells
        .iter()
        .filter(|cell| cell.width != 0)
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    assert!(!refreshed_row.contains("OLD_015"));

    for start_row in (0..opened.total_rows).step_by(16) {
        if let Some(view) = dashboard.history.as_mut() {
            view.top = start_row;
            view.cursor_target = None;
        }
        if let Some(request) = dashboard.history_request_if_needed() {
            pump_frozen_history(&mut dashboard, &mut frozen, vec![request]);
        }
    }
    assert_eq!(dashboard.history.as_ref().unwrap().pages.len(), 16);
    assert!(
        !dashboard
            .history
            .as_ref()
            .unwrap()
            .pages
            .iter()
            .any(|page| page.start_row == 0)
    );
    assert_eq!(
        dashboard.history.as_ref().unwrap().copy_range(),
        Some(range)
    );

    pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Char('y'));
    let expected = (range.anchor.row..=range.cursor.row)
        .map(|row| {
            let line = &old_lines[row as usize];
            let start = if row == range.anchor.row {
                usize::from(range.anchor.col)
            } else {
                0
            };
            let end = if row == range.cursor.row {
                usize::from(range.cursor.col) + 1
            } else {
                line.len()
            };
            line[start..end].to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        dashboard.take_pending_history_copy().as_deref(),
        Some(expected.as_str())
    );

    let reverse_anchor = dashboard.key(KeyCode::Char('v'));
    assert_eq!(reverse_anchor, ovrcr::tui::DashboardAction::Redraw);
    pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Char('g'));
    for _ in 0..15 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Down);
    }
    for _ in 0..126 {
        pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Right);
    }
    let reverse_range = dashboard.history.as_ref().unwrap().copy_range().unwrap();
    assert_eq!(reverse_range.anchor, HistoryCopyPoint { row: 33, col: 129 });
    assert_eq!(reverse_range.cursor, HistoryCopyPoint { row: 15, col: 126 });
    let view = dashboard.history.as_ref().unwrap();
    let cursor = view.cursor.unwrap();
    assert!(cursor.point.row >= view.top);
    assert!(
        cursor.point.row
            < view
                .top
                .saturating_add(dashboard.panes[dashboard.focused_pane].size.rows as u32)
    );
    assert!(cursor.point.col >= view.left);
    assert!(
        u32::from(cursor.point.col) + u32::from(cursor.cell_width)
            <= u32::from(view.left) + u32::from(dashboard.panes[dashboard.focused_pane].size.cols)
    );
    pump_frozen_key(&mut dashboard, &mut frozen, KeyCode::Char('y'));
    assert_eq!(
        dashboard.take_pending_history_copy().as_deref(),
        Some(expected.as_str())
    );
}

#[test]
fn copy_history_extraction_joins_wrap_and_skips_continuations() {
    let row15 = special_history_row(15);
    let mut direct = String::new();
    let mut pending_blanks = 0;
    let row15_slice = HistoryRow {
        width: row15.width,
        cells: row15.cells[125..].to_vec(),
        wrapped: row15.wrapped,
    };
    append_history_selection(
        &mut direct,
        &row15_slice,
        125,
        126,
        132,
        &mut pending_blanks,
    )
    .unwrap();
    assert_eq!(direct, "e\u{301}界  R");
    let mut omitted = String::new();
    let mut omitted_pending = 0;
    append_history_selection(
        &mut omitted,
        &HistoryRow {
            width: 4,
            cells: vec![
                history_cell("x", 1),
                history_cell("", 1),
                history_cell("", 1),
                history_cell("", 1),
            ],
            wrapped: false,
        },
        0,
        0,
        4,
        &mut omitted_pending,
    )
    .unwrap();
    assert_eq!(omitted, "x");
    assert_eq!(omitted_pending, 3);
    append_history_selection(
        &mut omitted,
        &HistoryRow {
            width: 1,
            cells: vec![history_cell("R", 1)],
            wrapped: false,
        },
        0,
        0,
        1,
        &mut omitted_pending,
    )
    .unwrap();
    assert_eq!(omitted, "x   R");
    assert_eq!(omitted_pending, 0);

    let opened = history_opened(18);
    let range = HistoryCopyRange {
        session: opened.session,
        snapshot: opened.snapshot,
        anchor: HistoryCopyPoint { row: 15, col: 126 },
        cursor: HistoryCopyPoint { row: 17, col: 3 },
    };
    let mut dashboard = dashboard_fixture();
    dashboard.mode = ovrcr::tui::InputMode::History;
    let mut view = HistoryView::new(opened.clone(), 0);
    view.anchor = Some(range.anchor);
    view.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 15,
        col: 128,
    }));
    dashboard.history = Some(view);
    let request = dashboard.history_request_if_needed().expect("cursor page");
    pump_special_history(&mut dashboard, vec![request]);
    assert_eq!(
        dashboard.history.as_ref().unwrap().cursor.unwrap().point,
        HistoryCopyPoint { row: 15, col: 127 }
    );
    dashboard.history.as_mut().unwrap().cursor_target =
        Some(ovrcr::tui::HistoryCursorTarget::At(range.cursor));
    if let Some(request) = dashboard.history_request_if_needed() {
        pump_special_history(&mut dashboard, vec![request]);
    }
    assert_eq!(
        dashboard.history.as_ref().unwrap().cursor.unwrap().point,
        range.cursor
    );

    dashboard.history.as_mut().unwrap().copy_job = Some(HistoryCopyJob::new(77, range));
    let request = dashboard.history_request_if_needed().expect("copy page");
    pump_special_history(&mut dashboard, vec![request]);
    let copied = dashboard
        .take_pending_history_copy()
        .expect("copy completion");
    assert_eq!(copied, "e\u{301}界  RC   D\nR ST");
    assert_eq!(copied.matches("界").count(), 1);
    assert_eq!(copied.matches('\n').count(), 1);
    assert!(copied.contains("e\u{301}"));

    let edge_range = HistoryCopyRange {
        session: opened.session,
        snapshot: opened.snapshot,
        anchor: HistoryCopyPoint { row: 0, col: 127 },
        cursor: HistoryCopyPoint { row: 0, col: 255 },
    };
    let mut edge_dashboard = dashboard_with_history_copy(edge_range);
    let edge_first = edge_dashboard
        .history_request_if_needed()
        .expect("edge copy first tile");
    assert!(matches!(
        edge_first.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 126,
            cols: 128,
            ..
        }
    ));
    let mut edge_first_cells = vec![history_cell("", 1); 128];
    edge_first_cells[1] = history_cell("界", 2);
    edge_first_cells[2] = history_cell("", 0);
    let edge_second = edge_dashboard
        .handle_server_message(ServerMessage::Response {
            request_id: edge_first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: opened.session,
                snapshot: opened.snapshot,
                start_row: 0,
                start_col: 126,
                rows: vec![HistoryRow {
                    width: 256,
                    cells: edge_first_cells,
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("edge copy second tile");
    assert!(matches!(
        edge_second.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 253,
            cols: 128,
            ..
        }
    ));
    let mut edge_second_cells = vec![history_cell("", 1); 3];
    edge_second_cells[2] = history_cell("R", 1);
    edge_dashboard.handle_server_message(ServerMessage::Response {
        request_id: edge_second.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: opened.session,
            snapshot: opened.snapshot,
            start_row: 0,
            start_col: 253,
            rows: vec![HistoryRow {
                width: 256,
                cells: edge_second_cells,
                wrapped: false,
            }],
        }),
    });
    let expected_edge_text = format!("界{}R", " ".repeat(126));
    assert_eq!(
        edge_dashboard
            .history
            .as_ref()
            .and_then(|view| view.copy_completion.as_ref())
            .map(|completion| completion.text.as_str()),
        Some(expected_edge_text.as_str())
    );
    let mut edge_output = Vec::new();
    let mut expected_edge_output = Vec::new();
    write_clipboard(&mut expected_edge_output, &expected_edge_text).unwrap();
    assert!(write_completed_history_copy(
        &mut edge_dashboard,
        &mut edge_output
    ));
    assert_eq!(edge_output, expected_edge_output);

    let deferred_edge_range = HistoryCopyRange {
        session: opened.session,
        snapshot: opened.snapshot,
        anchor: HistoryCopyPoint { row: 0, col: 127 },
        cursor: HistoryCopyPoint { row: 0, col: 255 },
    };
    let mut deferred_edge = dashboard_with_history_copy(deferred_edge_range);
    let deferred_first = deferred_edge
        .history_request_if_needed()
        .expect("requested-edge first tile");
    assert!(matches!(
        deferred_first.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 126,
            cols: 128,
            ..
        }
    ));
    let mut deferred_first_cells = vec![history_cell("", 1); 128];
    deferred_first_cells[127] = history_cell("界", 2);
    let deferred_second = deferred_edge
        .handle_server_message(ServerMessage::Response {
            request_id: deferred_first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: opened.session,
                snapshot: opened.snapshot,
                start_row: 0,
                start_col: 126,
                rows: vec![HistoryRow {
                    width: 256,
                    cells: deferred_first_cells,
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("requested-edge overlapping tile");
    assert!(matches!(
        deferred_second.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 252,
            cols: 128,
            ..
        }
    ));
    assert!(
        deferred_edge
            .history
            .as_ref()
            .unwrap()
            .copy_completion
            .is_none()
    );
    let mut deferred_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut deferred_edge,
        &mut deferred_output
    ));
    assert!(deferred_output.is_empty());
    let mut deferred_second_cells = vec![history_cell("", 1); 4];
    deferred_second_cells[1] = history_cell("界", 2);
    deferred_second_cells[2] = history_cell("", 0);
    deferred_second_cells[3] = history_cell("R", 1);
    deferred_edge.handle_server_message(ServerMessage::Response {
        request_id: deferred_second.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: opened.session,
            snapshot: opened.snapshot,
            start_row: 0,
            start_col: 252,
            rows: vec![HistoryRow {
                width: 256,
                cells: deferred_second_cells,
                wrapped: false,
            }],
        }),
    });
    let expected_deferred_edge = format!("{}界R", " ".repeat(126));
    assert_eq!(
        deferred_edge
            .history
            .as_ref()
            .and_then(|view| view.copy_completion.as_ref())
            .map(|completion| completion.text.as_str()),
        Some(expected_deferred_edge.as_str())
    );
    let mut expected_deferred_output = Vec::new();
    write_clipboard(&mut expected_deferred_output, &expected_deferred_edge).unwrap();
    assert!(write_completed_history_copy(
        &mut deferred_edge,
        &mut deferred_output
    ));
    assert_eq!(deferred_output, expected_deferred_output);

    let boundary_range = HistoryCopyRange {
        session: opened.session,
        snapshot: opened.snapshot,
        anchor: HistoryCopyPoint { row: 0, col: 0 },
        cursor: HistoryCopyPoint { row: 2, col: 1 },
    };
    let mut boundary_dashboard = dashboard_with_history_copy(boundary_range);
    let boundary_first = boundary_dashboard
        .history_request_if_needed()
        .expect("boundary first row");
    let boundary_second = boundary_dashboard
        .handle_server_message(ServerMessage::Response {
            request_id: boundary_first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: opened.session,
                snapshot: opened.snapshot,
                start_row: 0,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 1,
                    cells: vec![history_cell("A", 1)],
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("boundary empty row");
    let boundary_third = boundary_dashboard
        .handle_server_message(ServerMessage::Response {
            request_id: boundary_second.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: opened.session,
                snapshot: opened.snapshot,
                start_row: 1,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 0,
                    cells: Vec::new(),
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("boundary final row");
    boundary_dashboard.handle_server_message(ServerMessage::Response {
        request_id: boundary_third.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: opened.session,
            snapshot: opened.snapshot,
            start_row: 2,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 2,
                cells: vec![history_cell("B", 1), history_cell(" ", 1)],
                wrapped: false,
            }],
        }),
    });
    assert_eq!(
        boundary_dashboard.take_pending_history_copy().as_deref(),
        Some("A\n\nB ")
    );
    let mut boundary_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut boundary_dashboard,
        &mut boundary_output
    ));
    assert!(boundary_output.is_empty());
    let mut boundary_dashboard = dashboard_with_history_copy(boundary_range);
    let mut boundary_requests = vec![
        boundary_dashboard
            .history_request_if_needed()
            .expect("boundary writer retry first row"),
    ];
    while let Some(request) = boundary_requests.pop() {
        let page = match request.request {
            Request::HistoryPage { start_row, .. } => match start_row {
                0 => HistoryRows {
                    session: opened.session,
                    snapshot: opened.snapshot,
                    start_row: 0,
                    start_col: 0,
                    rows: vec![HistoryRow {
                        width: 1,
                        cells: vec![history_cell("A", 1)],
                        wrapped: false,
                    }],
                },
                1 => HistoryRows {
                    session: opened.session,
                    snapshot: opened.snapshot,
                    start_row: 1,
                    start_col: 0,
                    rows: vec![HistoryRow {
                        width: 0,
                        cells: Vec::new(),
                        wrapped: false,
                    }],
                },
                2 => HistoryRows {
                    session: opened.session,
                    snapshot: opened.snapshot,
                    start_row: 2,
                    start_col: 0,
                    rows: vec![HistoryRow {
                        width: 2,
                        cells: vec![history_cell("B", 1), history_cell(" ", 1)],
                        wrapped: false,
                    }],
                },
                row => panic!("unexpected boundary row {row}"),
            },
            request => panic!("unexpected boundary request {request:?}"),
        };
        boundary_requests.extend(boundary_dashboard.handle_server_message(
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::HistoryRows(page),
            },
        ));
    }
    let mut expected_boundary_output = Vec::new();
    write_clipboard(&mut expected_boundary_output, "A\n\nB ").unwrap();
    assert!(write_completed_history_copy(
        &mut boundary_dashboard,
        &mut boundary_output
    ));
    assert_eq!(boundary_output, expected_boundary_output);

    let mut endpoint_dashboard = dashboard_fixture();
    endpoint_dashboard.mode = ovrcr::tui::InputMode::History;
    let mut endpoint_view = HistoryView::new(history_opened(3), 0);
    endpoint_view.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    endpoint_view.cursor_target = Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
        row: 2,
        col: 0,
    }));
    endpoint_view.pages.push_back(HistoryRows {
        session: opened.session,
        snapshot: opened.snapshot,
        start_row: 0,
        start_col: 0,
        rows: vec![
            HistoryRow {
                width: 1,
                cells: vec![history_cell("A", 1)],
                wrapped: false,
            },
            HistoryRow {
                width: 1,
                cells: vec![history_cell("B", 1)],
                wrapped: false,
            },
            HistoryRow {
                width: 0,
                cells: Vec::new(),
                wrapped: false,
            },
        ],
    });
    endpoint_dashboard.history = Some(endpoint_view);
    assert!(endpoint_dashboard.history_request_if_needed().is_none());
    assert_eq!(
        endpoint_dashboard.history.as_ref().unwrap().cursor,
        Some(HistoryCursor {
            point: HistoryCopyPoint { row: 2, col: 0 },
            row_width: 0,
            cell_width: 1,
        })
    );
    assert!(
        endpoint_dashboard
            .history
            .as_ref()
            .unwrap()
            .cursor_target
            .is_none()
    );
    let endpoint_request = match endpoint_dashboard.key(KeyCode::Char('y')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected empty endpoint copy request, got {action:?}"),
    };
    let mut endpoint_requests = vec![endpoint_request];
    while let Some(request) = endpoint_requests.pop() {
        let page = match request.request {
            Request::HistoryPage { start_row, .. } => match start_row {
                0 => history_page(0, 0, 1, vec![history_cell("A", 1)]),
                1 => history_page(1, 0, 1, vec![history_cell("B", 1)]),
                2 => history_page(2, 0, 0, vec![]),
                row => panic!("unexpected empty-endpoint row {row}"),
            },
            request => panic!("unexpected empty-endpoint request {request:?}"),
        };
        endpoint_requests.extend(endpoint_dashboard.handle_server_message(
            ServerMessage::Response {
                request_id: request.request_id,
                response: Response::HistoryRows(page),
            },
        ));
    }
    assert_eq!(
        endpoint_dashboard
            .history
            .as_ref()
            .and_then(|view| view.copy_completion.as_ref())
            .map(|completion| completion.text.as_str()),
        Some("A\nB\n")
    );
    let mut endpoint_output = Vec::new();
    let mut expected_endpoint_output = Vec::new();
    write_clipboard(&mut expected_endpoint_output, "A\nB\n").unwrap();
    assert!(write_completed_history_copy(
        &mut endpoint_dashboard,
        &mut endpoint_output
    ));
    assert_eq!(endpoint_output, expected_endpoint_output);

    let mut exact = HistoryCopyJob::new(
        88,
        HistoryCopyRange {
            session: opened.session,
            snapshot: opened.snapshot,
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 511, col: 127 },
        },
    );
    for row in 0..512 {
        let done = exact
            .consume_page(&HistoryRows {
                session: opened.session,
                snapshot: opened.snapshot,
                start_row: row,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 128,
                    cells: vec![history_cell("x", 1); 128],
                    wrapped: true,
                }],
            })
            .unwrap();
        assert_eq!(done, row == 511);
    }
    assert_eq!(exact.into_completion().text.len(), 65_536);

    let mut utf8_overflow = HistoryCopyJob::new(
        89,
        HistoryCopyRange {
            session: opened.session,
            snapshot: opened.snapshot,
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 511, col: 127 },
        },
    );
    for row in 0..512 {
        let mut cells = vec![history_cell("x", 1); 128];
        if row == 511 {
            cells[127] = history_cell("é", 1);
        }
        let result = utf8_overflow.consume_page(&HistoryRows {
            session: opened.session,
            snapshot: opened.snapshot,
            start_row: row,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 128,
                cells,
                wrapped: true,
            }],
        });
        if row < 511 {
            assert!(!result.unwrap());
        } else {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        }
    }
}

fn dashboard_with_history_copy(range: HistoryCopyRange) -> Dashboard {
    let mut dashboard = dashboard_fixture();
    dashboard.mode = ovrcr::tui::InputMode::History;
    let mut view = HistoryView::new(
        HistoryOpened {
            session: range.session,
            snapshot: range.snapshot,
            revision: 1,
            size: TerminalSize { rows: 4, cols: 20 },
            history_rows: 0,
            total_rows: u32::max(range.anchor.row, range.cursor.row).saturating_add(1),
        },
        0,
    );
    view.anchor = Some(range.anchor);
    view.cursor = Some(HistoryCursor {
        point: range.cursor,
        row_width: 4,
        cell_width: 1,
    });
    view.cursor_target = None;
    view.copy_job = Some(HistoryCopyJob::new(1, range));
    dashboard.history = Some(view);
    dashboard
}

fn simple_copy_page(start_row: u32, text: &str) -> HistoryRows {
    HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row,
        start_col: 0,
        rows: vec![HistoryRow {
            width: 4,
            cells: (0..4)
                .map(|col| {
                    if col == 0 {
                        history_cell(text, 1)
                    } else {
                        history_cell("", 1)
                    }
                })
                .collect(),
            wrapped: false,
        }],
    }
}

fn write_completed_history_copy(dashboard: &mut Dashboard, output: &mut Vec<u8>) -> bool {
    let Some(text) = dashboard.take_pending_history_copy() else {
        return false;
    };
    write_clipboard(output, &text).expect("history copy writer");
    true
}

fn retry_simple_history_copy(
    dashboard: &mut Dashboard,
    text: &str,
    output: &mut Vec<u8>,
) -> Vec<u8> {
    let first = match dashboard.key(KeyCode::Char('y')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected retry copy request, got {action:?}"),
    };
    let next = dashboard.handle_server_message(ServerMessage::Response {
        request_id: first.request_id,
        response: Response::HistoryRows(simple_copy_page(0, text)),
    });
    let second = next.first().cloned().expect("retry second copy page");
    let follow_up = dashboard.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::HistoryRows(simple_copy_page(1, text)),
    });
    assert!(follow_up.is_empty());
    assert!(write_completed_history_copy(dashboard, output));
    output.clone()
}

fn retry_wide_history_copy(dashboard: &mut Dashboard, output: &mut Vec<u8>) -> Vec<u8> {
    let mut request = match dashboard.key(KeyCode::Char('y')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected wide retry copy request, got {action:?}"),
    };
    loop {
        let (start_row, start_col, cols) = match request.request {
            Request::HistoryPage {
                start_row,
                start_col,
                cols,
                ..
            } => (start_row, start_col, cols),
            request => panic!("unexpected wide retry request: {request:?}"),
        };
        let cell_count = usize::from(cols.min(260u16.saturating_sub(start_col)));
        let next = dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: SessionId(1),
                snapshot: HistorySnapshotId(7),
                start_row,
                start_col,
                rows: vec![HistoryRow {
                    width: 260,
                    cells: vec![history_cell("x", 1); cell_count],
                    wrapped: false,
                }],
            }),
        });
        let Some(next_request) = next.into_iter().next() else {
            break;
        };
        request = next_request;
    }
    assert!(write_completed_history_copy(dashboard, output));
    output.clone()
}

fn dashboard_copy_pending_after_first_tile(range: HistoryCopyRange) -> (Dashboard, ClientMessage) {
    let mut dashboard = dashboard_with_history_copy(range);
    let first = dashboard
        .history_request_if_needed()
        .expect("first copy page");
    let next = dashboard.handle_server_message(ServerMessage::Response {
        request_id: first.request_id,
        response: Response::HistoryRows(simple_copy_page(0, "partial")),
    });
    let pending = next
        .first()
        .cloned()
        .expect("next copy page remains pending");
    assert!(dashboard.history.as_ref().unwrap().copy_job.is_some());
    assert!(dashboard.take_pending_history_copy().is_none());
    (dashboard, pending)
}

#[test]
fn copy_history_invalid_snapshot_never_emits_partial_text() {
    let range = HistoryCopyRange {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        anchor: HistoryCopyPoint { row: 0, col: 0 },
        cursor: HistoryCopyPoint { row: 1, col: 0 },
    };
    for code in [ErrorCode::Conflict, ErrorCode::NotFound] {
        let (mut dashboard, pending) = dashboard_copy_pending_after_first_tile(range);
        let mut output = Vec::new();
        assert!(!write_completed_history_copy(&mut dashboard, &mut output));
        assert!(output.is_empty());
        let release = dashboard.handle_server_message(ServerMessage::Response {
            request_id: pending.request_id,
            response: Response::Error {
                code,
                message: "snapshot unavailable".into(),
            },
        });
        assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
        assert!(dashboard.history.is_none());
        assert!(dashboard.take_pending_history_copy().is_none());
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
        assert!(!write_completed_history_copy(&mut dashboard, &mut output));
        assert!(output.is_empty());
    }

    let (mut internal, pending) = dashboard_copy_pending_after_first_tile(range);
    let mut internal_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut internal,
        &mut internal_output
    ));
    assert!(internal_output.is_empty());
    let internal_release = internal.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "history backend unavailable".into(),
        },
    });
    assert!(internal_release.is_empty());
    assert_eq!(internal.mode, ovrcr::tui::InputMode::History);
    assert!(internal.history.as_ref().unwrap().copy_job.is_none());
    assert!(internal.history.as_ref().unwrap().pending.is_none());
    assert!(internal.take_pending_history_copy().is_none());
    assert!(internal.history_request_if_needed().is_none());
    let expected_retry = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, "retry\nretry").unwrap();
        expected
    };
    assert_eq!(
        retry_simple_history_copy(&mut internal, "retry", &mut internal_output),
        expected_retry
    );
    assert!(!write_completed_history_copy(
        &mut internal,
        &mut internal_output
    ));
    assert_eq!(internal_output, expected_retry);

    let malformed_pages = vec![
        HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 1,
            start_col: 0,
            rows: Vec::new(),
        },
        HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 1,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 4,
                cells: vec![history_cell("short", 1)],
                wrapped: false,
            }],
        },
        HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 1,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 4,
                cells: vec![
                    history_cell("", 3),
                    history_cell("", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                ],
                wrapped: false,
            }],
        },
        HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 1,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 4,
                cells: vec![
                    history_cell("", 0),
                    history_cell("", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                ],
                wrapped: false,
            }],
        },
    ];
    for malformed_page in malformed_pages {
        let (mut malformed, pending) = dashboard_copy_pending_after_first_tile(range);
        let mut output = Vec::new();
        assert!(!write_completed_history_copy(&mut malformed, &mut output));
        assert!(output.is_empty());
        let _ = malformed.handle_server_message(ServerMessage::Response {
            request_id: pending.request_id,
            response: Response::HistoryRows(malformed_page),
        });
        assert!(malformed.take_pending_history_copy().is_none());
        assert!(malformed.history.as_ref().unwrap().copy_job.is_none());
        assert!(malformed.history_request_if_needed().is_none());
        assert_eq!(
            retry_simple_history_copy(&mut malformed, "retry", &mut output),
            expected_retry
        );
        assert!(!write_completed_history_copy(&mut malformed, &mut output));
        assert_eq!(output, expected_retry);
    }

    for wrong in 0..5 {
        let (mut stale, pending) = dashboard_copy_pending_after_first_tile(range);
        let mut output = Vec::new();
        assert!(!write_completed_history_copy(&mut stale, &mut output));
        assert!(output.is_empty());
        let mut page = simple_copy_page(1, "stale");
        let request_id = match wrong {
            0 => pending.request_id.wrapping_add(1),
            _ => pending.request_id,
        };
        match wrong {
            1 => page.session = SessionId(99),
            2 => page.snapshot = HistorySnapshotId(99),
            3 => page.start_row = 2,
            4 => page.start_col = 1,
            _ => {}
        }
        let _ = stale.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::HistoryRows(page),
        });
        assert_eq!(
            stale
                .history
                .as_ref()
                .unwrap()
                .pending
                .as_ref()
                .unwrap()
                .request_id,
            pending.request_id
        );
        assert!(stale.history.as_ref().unwrap().copy_job.is_some());
        assert!(stale.take_pending_history_copy().is_none());
        assert!(!write_completed_history_copy(&mut stale, &mut output));
        assert!(output.is_empty());
        stale.handle_server_message(ServerMessage::Response {
            request_id: pending.request_id,
            response: Response::HistoryRows(simple_copy_page(1, "ok")),
        });
        let expected = {
            let mut expected = Vec::new();
            write_clipboard(&mut expected, "partial\nok").unwrap();
            expected
        };
        assert!(write_completed_history_copy(&mut stale, &mut output));
        assert_eq!(output, expected);
        assert!(!write_completed_history_copy(&mut stale, &mut output));
    }

    let conflicting_range = HistoryCopyRange {
        anchor: HistoryCopyPoint { row: 0, col: 0 },
        cursor: HistoryCopyPoint { row: 0, col: 259 },
        ..range
    };
    let mut conflicting = dashboard_with_history_copy(conflicting_range);
    let mut conflicting_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut conflicting,
        &mut conflicting_output
    ));
    assert!(conflicting_output.is_empty());
    let first = conflicting
        .history_request_if_needed()
        .expect("first wide copy page");
    let second = conflicting
        .handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: SessionId(1),
                snapshot: HistorySnapshotId(7),
                start_row: 0,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 260,
                    cells: vec![history_cell("x", 1); 128],
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("second wide copy page");
    let _ = conflicting.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 0,
            start_col: 127,
            rows: vec![HistoryRow {
                width: 260,
                cells: vec![history_cell("x", 1); 128],
                wrapped: true,
            }],
        }),
    });
    assert!(conflicting.history.as_ref().unwrap().copy_job.is_none());
    assert!(conflicting.take_pending_history_copy().is_none());
    assert!(conflicting.history_request_if_needed().is_none());
    assert!(conflicting_output.is_empty());
    assert!(!write_completed_history_copy(
        &mut conflicting,
        &mut conflicting_output
    ));
    assert!(conflicting_output.is_empty());
    let expected_wrapped_retry = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, &"x".repeat(260)).unwrap();
        expected
    };
    assert_eq!(
        retry_wide_history_copy(&mut conflicting, &mut conflicting_output),
        expected_wrapped_retry
    );
    assert!(!write_completed_history_copy(
        &mut conflicting,
        &mut conflicting_output
    ));

    let mut changed_width = dashboard_with_history_copy(conflicting_range);
    let mut changed_width_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut changed_width,
        &mut changed_width_output
    ));
    let first = changed_width
        .history_request_if_needed()
        .expect("changed-width first copy page");
    let second = changed_width
        .handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: SessionId(1),
                snapshot: HistorySnapshotId(7),
                start_row: 0,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 260,
                    cells: vec![history_cell("x", 1); 128],
                    wrapped: false,
                }],
            }),
        })
        .into_iter()
        .next()
        .expect("changed-width second copy page");
    let _ = changed_width.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 0,
            start_col: 127,
            rows: vec![HistoryRow {
                width: 261,
                cells: vec![history_cell("x", 1); 128],
                wrapped: false,
            }],
        }),
    });
    assert!(changed_width.history.as_ref().unwrap().copy_job.is_none());
    assert!(changed_width.take_pending_history_copy().is_none());
    assert!(changed_width_output.is_empty());
    let expected_changed_width = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, &"x".repeat(260)).unwrap();
        expected
    };
    assert_eq!(
        retry_wide_history_copy(&mut changed_width, &mut changed_width_output),
        expected_changed_width
    );
    assert!(!write_completed_history_copy(
        &mut changed_width,
        &mut changed_width_output
    ));

    let mut collision = dashboard_fixture();
    collision.mode = ovrcr::tui::InputMode::History;
    let opened = history_opened(2);
    let mut cached = HistoryView::new(opened.clone(), 0);
    cached.cursor_target = None;
    cached.anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    cached.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 0 },
        row_width: 4,
        cell_width: 1,
    });
    cached.pages.push_back(history_complete_page(
        0,
        0,
        2,
        4,
        vec![history_cell("canonical", 1)],
    ));
    cached.copy_job = Some(HistoryCopyJob::new(
        91,
        HistoryCopyRange {
            session: opened.session,
            snapshot: opened.snapshot,
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 0, col: 0 },
        },
    ));
    collision.history = Some(cached);
    let collision_request = collision
        .history_request_if_needed()
        .expect("copy collision request");
    let _ = collision.handle_server_message(ServerMessage::Response {
        request_id: collision_request.request_id,
        response: Response::HistoryRows(simple_copy_page(0, "copy")),
    });
    assert_eq!(
        collision.history.as_ref().unwrap().pages[0].rows[0].cells[0].text,
        "canonical"
    );
    assert_eq!(
        collision.take_pending_history_copy().as_deref(),
        Some("copy")
    );

    let mut incomplete = dashboard_fixture();
    incomplete.mode = ovrcr::tui::InputMode::History;
    let mut incomplete_view = HistoryView::new(history_opened(2), 0);
    incomplete_view.cursor_target = None;
    incomplete_view.pages.push_back(HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        start_col: 0,
        rows: vec![
            HistoryRow {
                width: 4,
                cells: vec![history_cell("a", 1); 4],
                wrapped: false,
            },
            HistoryRow {
                width: 4,
                cells: vec![history_cell("short", 1)],
                wrapped: false,
            },
        ],
    });
    incomplete.history = Some(incomplete_view);
    let mut incomplete_terminal = Terminal::new(TestBackend::new(180, 40)).unwrap();
    incomplete_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &incomplete, 0))
        .unwrap();
    let incomplete_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| incomplete_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(incomplete_text.contains("HISTORY · frozen · loading"));
    let incomplete_request = incomplete
        .history_request_if_needed()
        .expect("canonical viewport request");
    let incomplete_start = match incomplete_request.request {
        Request::HistoryPage { start_row, .. } => start_row,
        request => panic!("unexpected incomplete-cache request: {request:?}"),
    };
    let _ = incomplete.handle_server_message(ServerMessage::Response {
        request_id: incomplete_request.request_id,
        response: Response::HistoryRows(history_page(
            incomplete_start,
            0,
            4,
            vec![history_cell("short", 1)],
        )),
    });
    assert_eq!(incomplete.history.as_ref().unwrap().pages.len(), 1);
    assert!(incomplete.history_request_if_needed().is_none());

    let (mut switched, _) = dashboard_copy_pending_after_first_tile(range);
    let mut switched_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut switched,
        &mut switched_output
    ));
    switched.select_session(SessionId(2));
    assert_eq!(switched.mode, ovrcr::tui::InputMode::Browse);
    assert!(switched.history.is_none());
    assert!(switched_output.is_empty());

    let (mut removed, _) = dashboard_copy_pending_after_first_tile(range);
    let mut removed_output = Vec::new();
    let mut removed_hierarchy = removed.hierarchy.clone();
    removed_hierarchy.projects[1].workspaces[1]
        .sessions
        .retain(|session| session.id != SessionId(1));
    let _ = removed.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        removed_hierarchy,
    )));
    assert_eq!(removed.mode, ovrcr::tui::InputMode::Browse);
    assert!(removed.history.is_none());
    assert!(!write_completed_history_copy(
        &mut removed,
        &mut removed_output
    ));
    assert!(removed_output.is_empty());

    let (mut staged, pending) = dashboard_copy_pending_after_first_tile(range);
    let _ = staged.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "done")),
    });
    assert!(
        staged
            .history
            .as_ref()
            .and_then(|view| view.copy_completion.as_ref())
            .is_some()
    );
    let mut staged_output = Vec::new();
    staged.select_session(SessionId(2));
    assert!(staged.history.is_none());
    assert!(staged.take_pending_history_copy().is_none());
    assert!(!write_completed_history_copy(
        &mut staged,
        &mut staged_output
    ));
    assert!(staged_output.is_empty());

    let mut dashboard = dashboard_with_history_copy(range);
    let request = dashboard.history_request_if_needed().expect("copy page");
    let next = dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::HistoryRows(simple_copy_page(0, "partial")),
    });
    let old_pending = next.first().cloned().expect("next row remains pending");
    let old_pending_page = dashboard.history.as_ref().unwrap().pending.clone().unwrap();
    let old_job_id = dashboard
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap()
        .id;
    let original_range = dashboard
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap()
        .range;
    let mut cancellation_output = Vec::new();
    assert!(dashboard.history.as_ref().unwrap().copy_job.is_some());
    assert!(dashboard.take_pending_history_copy().is_none());
    assert!(!write_completed_history_copy(
        &mut dashboard,
        &mut cancellation_output
    ));
    assert!(cancellation_output.is_empty());
    let cancelled = dashboard.key(KeyCode::Esc);
    assert_eq!(cancelled, ovrcr::tui::DashboardAction::Redraw);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);
    assert_eq!(dashboard.copy_notice.as_deref(), Some("Copy cancelled"));
    assert_eq!(
        dashboard.history.as_ref().unwrap().pending.as_ref(),
        Some(&old_pending_page)
    );
    assert!(matches!(
        dashboard.key(KeyCode::Char('y')),
        ovrcr::tui::DashboardAction::Redraw
    ));
    let new_job = dashboard
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap();
    assert_ne!(new_job.id, old_job_id);
    assert_eq!(new_job.range, original_range);
    let new_job_id = new_job.id;
    assert_eq!(
        dashboard.history.as_ref().unwrap().pending.as_ref(),
        Some(&old_pending_page)
    );
    let late_requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: old_pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "late")),
    });
    assert!(dashboard.take_pending_history_copy().is_none());
    assert_eq!(
        dashboard
            .history
            .as_ref()
            .unwrap()
            .copy_job
            .as_ref()
            .unwrap()
            .id,
        new_job_id
    );
    let new_request = late_requests
        .first()
        .cloned()
        .expect("late response retires old slot and starts replacement");
    assert!(matches!(
        new_request.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 0,
            ..
        }
    ));
    let replacement_pending = dashboard.history.as_ref().unwrap().pending.clone().unwrap();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: old_pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "duplicate-late")),
    });
    assert_eq!(
        dashboard.history.as_ref().unwrap().pending.as_ref(),
        Some(&replacement_pending)
    );
    let next = dashboard.handle_server_message(ServerMessage::Response {
        request_id: new_request.request_id,
        response: Response::HistoryRows(simple_copy_page(0, "new")),
    });
    let final_request = next.first().cloned().expect("replacement second page");
    let _ = dashboard.handle_server_message(ServerMessage::Response {
        request_id: final_request.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "new")),
    });
    assert_eq!(
        dashboard
            .history
            .as_ref()
            .and_then(|view| view.copy_completion.as_ref())
            .map(|completion| completion.text.as_str()),
        Some("new\nnew")
    );
    let expected_cancellation = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, "new\nnew").unwrap();
        expected
    };
    assert!(write_completed_history_copy(
        &mut dashboard,
        &mut cancellation_output
    ));
    assert_eq!(cancellation_output, expected_cancellation);
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::None
    ));
    assert!(dashboard.history.as_ref().unwrap().copy_job.is_none());
    assert!(!write_completed_history_copy(
        &mut dashboard,
        &mut cancellation_output
    ));
    assert!(matches!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('y'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat,
        )),
        ovrcr::tui::DashboardAction::None
    ));
    assert_eq!(
        dashboard.key(KeyCode::Esc),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    assert!(dashboard.take_pending_history_copy().is_none());

    let (mut changed_range, old_changed_pending) = dashboard_copy_pending_after_first_tile(range);
    let changed_old_pending_page = changed_range
        .history
        .as_ref()
        .unwrap()
        .pending
        .clone()
        .unwrap();
    let changed_old_job_id = changed_range
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap()
        .id;
    let changed_old_range = changed_range
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap()
        .range;
    let mut changed_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut changed_range,
        &mut changed_output
    ));
    assert_eq!(
        changed_range.key(KeyCode::Esc),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(!write_completed_history_copy(
        &mut changed_range,
        &mut changed_output
    ));
    assert!(changed_output.is_empty());
    assert!(matches!(
        changed_range.key(KeyCode::Down),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert!(matches!(
        changed_range.key(KeyCode::Char('y')),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert!(changed_range.history.as_ref().unwrap().copy_job.is_none());
    assert_eq!(
        changed_range.history.as_ref().unwrap().pending.as_ref(),
        Some(&changed_old_pending_page)
    );
    let changed_requests = changed_range.handle_server_message(ServerMessage::Response {
        request_id: old_changed_pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "late")),
    });
    let changed_first = changed_requests
        .first()
        .cloned()
        .expect("changed-range cursor request");
    assert!(matches!(
        changed_first.request,
        Request::HistoryPage {
            start_row: 0,
            start_col: 0,
            ..
        }
    ));
    let changed_next = changed_range.handle_server_message(ServerMessage::Response {
        request_id: changed_first.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 0,
            start_col: 0,
            rows: vec![
                HistoryRow {
                    width: 4,
                    cells: vec![history_cell("old", 1); 4],
                    wrapped: false,
                },
                HistoryRow {
                    width: 4,
                    cells: vec![history_cell("late", 1); 4],
                    wrapped: false,
                },
            ],
        }),
    });
    assert!(changed_next.is_empty());
    assert!(
        changed_range
            .history
            .as_ref()
            .unwrap()
            .cursor_target
            .is_none()
    );
    assert_eq!(
        changed_range.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let changed_copy_request = match changed_range.key(KeyCode::Char('y')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected changed-range copy request, got {action:?}"),
    };
    let changed_job = changed_range
        .history
        .as_ref()
        .unwrap()
        .copy_job
        .as_ref()
        .unwrap();
    assert_ne!(changed_job.id, changed_old_job_id);
    assert_ne!(changed_job.range, changed_old_range);
    assert!(matches!(
        changed_copy_request.request,
        Request::HistoryPage {
            start_row: 1,
            start_col: 0,
            ..
        }
    ));
    let changed_copy_pending = changed_range.history.as_ref().unwrap().pending.clone();
    assert_eq!(
        changed_copy_pending.as_ref().unwrap().request_id,
        changed_copy_request.request_id
    );
    changed_range.handle_server_message(ServerMessage::Response {
        request_id: old_changed_pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "duplicate-old")),
    });
    assert_eq!(
        changed_range.history.as_ref().unwrap().pending.as_ref(),
        changed_copy_pending.as_ref()
    );
    assert!(!write_completed_history_copy(
        &mut changed_range,
        &mut changed_output
    ));
    assert!(changed_output.is_empty());
    changed_range.handle_server_message(ServerMessage::Response {
        request_id: changed_copy_request.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "changed")),
    });
    let expected_changed = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, "changed").unwrap();
        expected
    };
    assert!(write_completed_history_copy(
        &mut changed_range,
        &mut changed_output
    ));
    assert_eq!(changed_output, expected_changed);

    let (mut replacement, _) = dashboard_copy_pending_after_first_tile(range);
    let mut replacement_output = Vec::new();
    assert!(replacement.history.as_ref().unwrap().copy_job.is_some());
    assert!(replacement.history.as_ref().unwrap().pending.is_some());
    replacement.history_begin_request = Some(ovrcr::tui::PendingHistoryBegin {
        request_id: 90,
        session: SessionId(1),
        cancelled: false,
        at_tail: false,
    });
    let replacement_opened = HistoryOpened {
        snapshot: HistorySnapshotId(8),
        ..history_opened(2)
    };
    let replacement_requests = replacement.handle_server_message(ServerMessage::Response {
        request_id: 90,
        response: Response::HistoryOpened(replacement_opened.clone()),
    });
    assert_eq!(
        replacement.history.as_ref().unwrap().opened.snapshot,
        HistorySnapshotId(8)
    );
    assert!(replacement.history.as_ref().unwrap().copy_job.is_none());
    assert!(replacement.take_pending_history_copy().is_none());
    assert!(!write_completed_history_copy(
        &mut replacement,
        &mut replacement_output
    ));
    assert!(replacement_output.is_empty());
    assert!(replacement_requests.iter().any(|request| matches!(
        request.request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    )));

    let oversize_range = HistoryCopyRange {
        cursor: HistoryCopyPoint { row: 511, col: 127 },
        ..range
    };
    let (mut dashboard, mut request) = {
        let mut dashboard = dashboard_with_history_copy(oversize_range);
        let request = dashboard.history_request_if_needed().expect("copy page");
        (dashboard, request)
    };
    for row in 0..512 {
        let mut cells = vec![history_cell("x", 1); 128];
        if row == 511 {
            cells[127] = history_cell("é", 1);
        }
        let follow_up = dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::HistoryRows(HistoryRows {
                session: SessionId(1),
                snapshot: HistorySnapshotId(7),
                start_row: row,
                start_col: 0,
                rows: vec![HistoryRow {
                    width: 128,
                    cells,
                    wrapped: true,
                }],
            }),
        });
        if row < 511 {
            request = follow_up.into_iter().next().expect("next valid copy page");
        } else {
            assert!(follow_up.iter().any(|request| matches!(
                request.request,
                Request::HistoryPage {
                    start_row: 0,
                    start_col: 0,
                    ..
                }
            )));
        }
    }
    assert!(dashboard.take_pending_history_copy().is_none());
    assert!(dashboard.history.as_ref().unwrap().copy_job.is_none());
    assert!(
        dashboard
            .copy_notice
            .as_deref()
            .is_some_and(|notice| notice.contains("65536"))
    );
    assert_eq!(
        dashboard.history.as_ref().unwrap().copy_range(),
        Some(oversize_range)
    );
    let oversize_pending = dashboard.history.as_ref().unwrap().pending.clone().unwrap();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: oversize_pending.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "oversize retry slot retired".into(),
        },
    });
    assert!(dashboard.history.as_ref().unwrap().pending.is_none());
    let mut oversize_output = Vec::new();
    assert!(!write_completed_history_copy(
        &mut dashboard,
        &mut oversize_output
    ));
    assert!(oversize_output.is_empty());
    dashboard.history.as_mut().unwrap().cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 3 },
        row_width: 4,
        cell_width: 1,
    });
    let retry_request = match dashboard.key(KeyCode::Char('y')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected smaller valid retry, got {action:?}"),
    };
    let _ = dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry_request.request_id,
        response: Response::HistoryRows(simple_copy_page(0, "ok")),
    });
    let expected_oversize_retry = {
        let mut expected = Vec::new();
        write_clipboard(&mut expected, "ok").unwrap();
        expected
    };
    assert!(write_completed_history_copy(
        &mut dashboard,
        &mut oversize_output
    ));
    assert_eq!(oversize_output, expected_oversize_retry);
    assert!(!write_completed_history_copy(
        &mut dashboard,
        &mut oversize_output
    ));
}

#[test]
fn history_render_preserves_cells_and_clips() {
    let mut view = ovrcr::tui::HistoryView::new(history_opened(1), 0);
    let mut wide = history_cell("界", 2);
    wide.fg = HistoryColor::Rgb(1, 2, 3);
    view.pages.push_back(history_page(
        0,
        0,
        4,
        vec![
            wide,
            history_cell("", 0),
            history_cell("e\u{301}", 1),
            history_cell("X", 1),
        ],
    ));
    let mut terminal = Terminal::new(TestBackend::new(3, 1)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 3, 1), &view))
        .unwrap();
    terminal.show_cursor().unwrap();
    terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 3, 1), &view))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "界");
    assert_eq!(terminal.backend().buffer()[(1, 0)].symbol(), " ");
    assert_eq!(terminal.backend().buffer()[(2, 0)].symbol(), "e\u{301}");
    assert_eq!(terminal.backend().buffer()[(0, 0)].fg, Color::Rgb(1, 2, 3));
    assert!(!terminal.backend().cursor_visible());

    view.left = 1;
    terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 3, 1), &view))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), " ");
    assert_eq!(terminal.backend().buffer()[(1, 0)].symbol(), "e\u{301}");
    assert_eq!(terminal.backend().buffer()[(2, 0)].symbol(), "X");

    view.left = 0;
    terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 1, 1), &view))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), " ");

    let mut tiled_view = ovrcr::tui::HistoryView::new(history_opened(1), 0);
    let mut left_cells = (0..128).map(|_| history_cell("", 1)).collect::<Vec<_>>();
    left_cells[127] = history_cell("界", 2);
    tiled_view
        .pages
        .push_back(history_page(0, 0, 256, left_cells));
    let mut right_cells = vec![history_cell("", 1); 128];
    right_cells[0] = history_cell("", 0);
    right_cells[1] = history_cell("R", 1);
    tiled_view
        .pages
        .push_back(history_page(0, 128, 256, right_cells));
    let mut tiled_terminal = Terminal::new(TestBackend::new(130, 1)).unwrap();
    tiled_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 130, 1), &tiled_view))
        .unwrap();
    assert_eq!(tiled_terminal.backend().buffer()[(127, 0)].symbol(), "界");
    assert_eq!(tiled_terminal.backend().buffer()[(128, 0)].symbol(), " ");
    assert_eq!(tiled_terminal.backend().buffer()[(129, 0)].symbol(), "R");
    tiled_view.left = 128;
    tiled_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 2, 1), &tiled_view))
        .unwrap();
    assert_eq!(tiled_terminal.backend().buffer()[(0, 0)].symbol(), " ");
    assert_eq!(tiled_terminal.backend().buffer()[(1, 0)].symbol(), "R");

    let mut selected_view = tiled_view.clone();
    selected_view.left = 0;
    selected_view.anchor = Some(HistoryCopyPoint { row: 0, col: 127 });
    selected_view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 127 },
        row_width: 256,
        cell_width: 2,
    });
    selected_view.cursor_target = None;
    tiled_terminal
        .draw(|frame| {
            ovrcr::tui::render_history(frame, Rect::new(0, 0, 130, 1), &selected_view);
            let buffer = frame.buffer_mut();
            assert_eq!(buffer[(127, 0)].fg, Color::Rgb(30, 30, 46));
            assert_eq!(buffer[(128, 0)].fg, Color::Rgb(30, 30, 46));
            assert_eq!(buffer[(127, 0)].bg, Color::Rgb(148, 226, 213));
            assert_eq!(buffer[(128, 0)].bg, Color::Rgb(148, 226, 213));
            assert_eq!(buffer[(129, 0)].symbol(), "R");
            assert_eq!(buffer[(129, 0)].bg, Color::Rgb(30, 30, 46));
        })
        .unwrap();
    assert_eq!(tiled_terminal.backend().buffer()[(127, 0)].symbol(), "界");
    assert_eq!(tiled_terminal.backend().buffer()[(128, 0)].symbol(), " ");

    let mut selected_output = Vec::new();
    {
        let backend = CrosstermBackend::new(&mut selected_output);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 130, 1)),
            },
        )
        .unwrap();
        terminal
            .draw(|frame| {
                ovrcr::tui::render_history(frame, Rect::new(0, 0, 130, 1), &selected_view)
            })
            .unwrap();
    }
    let mut selected_rendered = vt100::Parser::new(1, 130, 0);
    selected_rendered.process(&selected_output);
    assert_eq!(
        selected_rendered.screen().cell(0, 127).unwrap().contents(),
        "界"
    );
    assert_eq!(
        selected_rendered.screen().cell(0, 128).unwrap().contents(),
        ""
    );
    assert_eq!(
        selected_rendered.screen().cell(0, 129).unwrap().contents(),
        "R"
    );

    selected_view.left = 128;
    let mut left_edge_terminal = Terminal::new(TestBackend::new(1, 1)).unwrap();
    left_edge_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 1, 1), &selected_view))
        .unwrap();
    assert_eq!(left_edge_terminal.backend().buffer()[(0, 0)].symbol(), " ");
    assert_eq!(
        left_edge_terminal.backend().buffer()[(0, 0)].bg,
        Color::Rgb(30, 30, 46)
    );
    assert!(!left_edge_terminal.backend().cursor_visible());
    selected_view.left = 129;
    left_edge_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 1, 1), &selected_view))
        .unwrap();
    assert_eq!(left_edge_terminal.backend().buffer()[(0, 0)].symbol(), "R");
    assert_eq!(
        left_edge_terminal.backend().buffer()[(0, 0)].bg,
        Color::Rgb(30, 30, 46)
    );

    let mut dashboard = dashboard_fixture();
    dashboard.mode = ovrcr::tui::InputMode::History;
    let mut loading = ovrcr::tui::HistoryView::new(history_opened(100), 0);
    loading.pending = Some(ovrcr::tui::PendingHistoryPage {
        request_id: 2,
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        rows: 16,
        start_col: 0,
        cols: 128,
        purpose: ovrcr::tui::HistoryPagePurpose::Viewport,
    });
    dashboard.history = Some(loading);
    let mut dashboard_terminal = Terminal::new(TestBackend::new(180, 40)).unwrap();
    dashboard_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let dashboard_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| dashboard_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dashboard_text.contains("loading"));
    assert!(dashboard_text.contains("row 1"));
    assert!(dashboard_text.contains("col 1"));
    let mut loaded_history = ovrcr::tui::HistoryView::new(history_opened(1), 0);
    loaded_history.cursor_target = None;
    dashboard.history = Some(loaded_history);
    dashboard
        .history
        .as_mut()
        .unwrap()
        .pages
        .push_back(history_complete_page(
            0,
            0,
            1,
            4,
            vec![history_cell("payload", 1)],
        ));
    dashboard_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let dashboard_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| dashboard_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dashboard_text.contains("HISTORY · frozen · loaded"));
    dashboard.history.as_mut().unwrap().copy_job = Some(HistoryCopyJob::new(
        4,
        HistoryCopyRange {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 0, col: 0 },
        },
    ));
    dashboard.history.as_mut().unwrap().pending = Some(ovrcr::tui::PendingHistoryPage {
        request_id: 5,
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        rows: 1,
        start_col: 0,
        cols: 128,
        purpose: ovrcr::tui::HistoryPagePurpose::Copy(4),
    });
    dashboard_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let dashboard_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| dashboard_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dashboard_text.contains("HISTORY · frozen · loaded · copying"));
    dashboard.history.as_mut().unwrap().copy_job = None;
    dashboard.history.as_mut().unwrap().cursor_target =
        Some(ovrcr::tui::HistoryCursorTarget::At(HistoryCopyPoint {
            row: 0,
            col: 0,
        }));
    dashboard_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let dashboard_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| dashboard_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dashboard_text.contains("HISTORY · frozen · loaded · cursor loading"));

    selected_view.left = 0;
    let mut roundtrip_view = tiled_view.clone();
    roundtrip_view.left = 0;
    roundtrip_view.anchor = Some(HistoryCopyPoint { row: 0, col: 127 });
    roundtrip_view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 127 },
        row_width: 256,
        cell_width: 2,
    });
    roundtrip_view.cursor_target = None;
    let mut crossterm_bytes = Vec::new();
    {
        crossterm::style::force_color_output(true);
        let backend = CrosstermBackend::new(&mut crossterm_bytes);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 132, 1)),
            },
        )
        .unwrap();
        terminal
            .draw(|frame| {
                ovrcr::tui::render_history(frame, Rect::new(0, 0, 130, 1), &roundtrip_view)
            })
            .unwrap();
    }
    let mut round_trip = vt100::Parser::new(1, 130, 0);
    round_trip.process(&crossterm_bytes);
    let leader = round_trip.screen().cell(0, 127).unwrap();
    assert_eq!(leader.contents(), "界");
    assert_eq!(leader.fgcolor(), vt100::Color::Rgb(30, 30, 46));
    assert_eq!(leader.bgcolor(), vt100::Color::Rgb(148, 226, 213));
    assert!(
        round_trip
            .screen()
            .cell(0, 128)
            .unwrap()
            .is_wide_continuation()
    );
    assert_eq!(round_trip.screen().cell(0, 129).unwrap().contents(), "R");
    crossterm::style::force_color_output(false);

    let mut clipped_terminal = Terminal::new(TestBackend::new(128, 1)).unwrap();
    clipped_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 128, 1), &selected_view))
        .unwrap();
    assert_eq!(clipped_terminal.backend().buffer()[(127, 0)].symbol(), " ");
    assert_eq!(
        clipped_terminal.backend().buffer()[(127, 0)].bg,
        Color::Rgb(30, 30, 46)
    );
    assert!(!clipped_terminal.backend().cursor_visible());
    let mut empty_history = ovrcr::tui::HistoryView::new(history_opened(1), 0);
    empty_history.cursor_target = None;
    dashboard.history = Some(empty_history);
    dashboard
        .history
        .as_mut()
        .unwrap()
        .pages
        .push_back(history_page(0, 0, 0, vec![]));
    dashboard_terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let dashboard_text = (0..40)
        .map(|row| {
            (0..180)
                .map(|column| dashboard_terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dashboard_text.contains("empty"));
}

#[test]
fn history_cancelled_begin_releases_late_snapshot() {
    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    assert_eq!(
        dashboard.key(KeyCode::Esc),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    assert!(
        dashboard
            .history_begin_request
            .as_ref()
            .is_some_and(|pending| pending.cancelled)
    );
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.history.is_none());
    assert_eq!(release.len(), 1);
    assert_eq!(release[0].request_id, 2);
    assert!(matches!(
        release[0].request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    ));
    assert!(dashboard.history_begin_request.is_none());
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request: Request::HistoryBegin { .. },
            ..
        })
    ));

    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    assert_eq!(
        dashboard.ctrl('g'),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.history.is_none());
    assert_eq!(release.len(), 1);
    assert!(matches!(
        release[0].request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    ));
    assert!(dashboard.history_begin_request.is_none());
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request: Request::HistoryBegin { .. },
            ..
        })
    ));

    let mut dashboard = dashboard_fixture();
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Terminal);
    assert_eq!(
        dashboard.ctrl('g'),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    let release = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(100)),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.history.is_none());
    assert_eq!(release.len(), 1);
    assert!(matches!(
        release[0].request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    ));
    assert!(dashboard.history_begin_request.is_none());
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request: Request::HistoryBegin { .. },
            ..
        })
    ));
}

#[test]
fn pause_resume_dense_status_has_priority() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase = SessionPhase::Paused;
    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].activity = AgentActivity::Busy;

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = |y| (0..39).map(|x| buffer[(x, y)].symbol()).collect::<String>();
    assert_eq!(row(3).trim_end(), "  P local");
    let rendered = (0..40)
        .map(|y| {
            (0..120)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
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
            .is_some_and(|footer| footer.split_whitespace().any(|key| key == "q"))
    );
    assert!(
        rendered
            .lines()
            .last()
            .unwrap()
            .split_whitespace()
            .any(|key| key == "t")
    );

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let terminal_footer = (0..120)
        .map(|x| terminal.backend().buffer()[(x, 39)].symbol())
        .collect::<String>();
    assert!(terminal_footer.contains("Terminal mode"));
    assert!(terminal_footer.contains("Ctrl-g"));
    assert!(!terminal_footer.contains("q detach"));

    dashboard.mode = ovrcr::tui::InputMode::Browse;
    let mut narrow = Terminal::new(TestBackend::new(40, 20)).unwrap();
    narrow
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
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
fn collapse_and_mouse_hits_use_current_visible_tree() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_session(SessionId(5));
    dashboard.toggle_selected_group();
    assert!(
        !dashboard
            .visible_rows()
            .contains(&ovrcr::tui::TreeRow::Session { id: SessionId(5) })
    );
    dashboard.toggle_selected_group();
    let mouse = |row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 4,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let action = dashboard.mouse_action(mouse(2), Rect::new(0, 0, 120, 40));
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert!(
        !dashboard
            .visible_rows()
            .contains(&ovrcr::tui::TreeRow::Session { id: SessionId(5) })
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let collapsed_workspace = (0..39)
        .map(|column| terminal.backend().buffer()[(column, 2)].symbol())
        .collect::<String>();
    assert!(collapsed_workspace.contains('▶'));
    let action = dashboard.mouse_action(mouse(2), Rect::new(0, 0, 120, 40));
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    let action = dashboard.mouse_action(mouse(6), Rect::new(0, 0, 120, 40));
    assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let action = dashboard.mouse_action(mouse(1), Rect::new(0, 0, 120, 40));
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert!(dashboard.collapsed_projects.contains("consigint"));
    let action = dashboard.mouse_action(
        MouseEvent {
            column: 60,
            ..mouse(6)
        },
        Rect::new(0, 0, 120, 40),
    );
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.mouse_action(mouse(6), Rect::new(0, 0, 120, 40)),
        ovrcr::tui::DashboardAction::None
    );
}

#[test]
fn fifty_session_selection_scrolls_tree_and_mouse_hits_viewport() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 34, cols: 88 });
    dashboard.outer_area = Rect::new(0, 0, 120, 40);
    dashboard.hierarchy = HierarchySnapshot {
        projects: vec![ProjectSummary {
            name: "project".into(),
            workspaces: vec![WorkspaceSummary {
                project: "project".into(),
                name: "workspace".into(),
                path: PathBuf::from("/tmp/workspace"),
                sessions: (1..=50)
                    .map(|id| SessionSummary {
                        id: SessionId(id),
                        project: "project".into(),
                        workspace: "workspace".into(),
                        name: format!("session-{id}"),
                        label: "sh".into(),
                        pid: Some(id as u32),
                        started_unix_ms: 0,
                        phase: SessionPhase::Running,
                        activity: AgentActivity::Unknown,
                        context_usage: None,
                    })
                    .collect(),
            }],
        }],
    };
    for _ in 0..50 {
        dashboard.move_selection(1);
    }
    assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let rendered = (0..40)
        .map(|row| {
            (0..39)
                .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("session-50"));
    for (index, row) in (35..=37).enumerate() {
        let action = dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 4,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        );
        if index == 0 {
            assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
        } else {
            assert!(matches!(
                action,
                ovrcr::tui::DashboardAction::Redraw | ovrcr::tui::DashboardAction::None
            ));
        }
        assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    }
}

#[test]
fn dashboard_inner_rect_keeps_last_pty_row_and_cursor_visible() {
    let inner = ovrcr::tui::actual_drawn_inner_rect(Rect::new(0, 0, 120, 40));
    assert_eq!(inner.height, 36);
    assert_eq!(inner.width, 80);
    let mut parser = vt100::Parser::new(inner.height, inner.width, 0);
    parser.process(b"\x1b[36;1HBOTTOM_MARKER");
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| {
            ovrcr::tui::render_terminal(frame, inner, parser.screen(), true);
        })
        .unwrap();
    let bottom = (inner.x..inner.right())
        .map(|column| terminal.backend().buffer()[(column, inner.bottom() - 1)].symbol())
        .collect::<String>();
    assert!(bottom.contains("BOTTOM_MARKER"));
    assert_eq!(terminal.backend().cursor_position().y, inner.bottom() - 1);
}

#[test]
fn ordinary_response_errors_remain_visible_to_dashboard() {
    let mut dashboard = dashboard_fixture();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 9,
        response: Response::Error {
            code: ErrorCode::NotFound,
            message: "session 99 not found".into(),
        },
    });
    assert_eq!(
        dashboard.error.as_deref(),
        Some("NotFound: session 99 not found")
    );
}

#[test]
fn shrinking_dashboard_keeps_selected_tree_row_visible() {
    let mut dashboard = dashboard_fixture();
    let workspace = &mut dashboard.hierarchy.projects[0].workspaces[0];
    for id in 6..=50 {
        workspace.sessions.push(SessionSummary {
            id: SessionId(id),
            project: "consigint".into(),
            workspace: "auth".into(),
            name: format!("session-{id}"),
            label: "sh".into(),
            pid: Some(id as u32),
            started_unix_ms: 0,
            phase: SessionPhase::Running,
            activity: AgentActivity::Unknown,
            context_usage: None,
        });
    }
    dashboard.select_session(SessionId(50));
    assert_eq!(dashboard.focused_session(), Some(SessionId(50)));
    let resize = dashboard
        .view_request(outer_area_for_pane(TerminalSize { rows: 20, cols: 40 }), 99)
        .unwrap()
        .expect("resize should request a view");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Screen {
            session: SessionId(50),
            revision: dashboard.view_revision,
            size: TerminalSize { rows: 20, cols: 40 },
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: resize.request_id,
        response: Response::Ok,
    });
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let rendered = (0..24)
        .map(|row| {
            (0..39)
                .map(|col| terminal.backend().buffer()[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("session-50"));
}

#[derive(Clone)]
struct InjectedWriter {
    output: Arc<Mutex<Vec<u8>>>,
    writes: Arc<Mutex<usize>>,
    fail_after: usize,
}

impl InjectedWriter {
    fn new(fail_after: usize) -> (Self, Arc<Mutex<Vec<u8>>>) {
        let output = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                output: Arc::clone(&output),
                writes: Arc::new(Mutex::new(0)),
                fail_after,
            },
            output,
        )
    }
}

impl Write for InjectedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut writes = self.writes.lock().unwrap();
        if *writes == self.fail_after {
            *writes += 1;
            return Err(io::Error::other("injected writer failure"));
        }
        *writes += 1;
        self.output.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn occurrences(bytes: &[u8], needle: &[u8]) -> usize {
    bytes
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

#[test]
fn terminal_guard_restores_each_enabled_mode_once_on_normal_return_and_unwind() {
    let (writer, output) = InjectedWriter::new(usize::MAX);
    let guard = ovrcr::tui::TerminalGuard::enter_with_writer(writer).unwrap();
    drop(guard);
    let bytes = output.lock().unwrap().clone();
    assert_eq!(occurrences(&bytes, b"\x1b[?1049l"), 1);
    assert_eq!(occurrences(&bytes, b"\x1b[?2004l"), 1);
    assert_eq!(occurrences(&bytes, b"\x1b[?1000l"), 1);
    assert_eq!(occurrences(&bytes, b"\x1b[?25h"), 1);

    let (writer, output) = InjectedWriter::new(usize::MAX);
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _guard = ovrcr::tui::TerminalGuard::enter_with_writer(writer).unwrap();
        panic!("exercise unwind restoration");
    }));
    assert!(result.is_err());
    let bytes = output.lock().unwrap().clone();
    assert_eq!(occurrences(&bytes, b"\x1b[?1049l"), 1);
    assert_eq!(occurrences(&bytes, b"\x1b[?2004l"), 1);
    assert_eq!(occurrences(&bytes, b"\x1b[?1000l"), 1);
    assert_eq!(occurrences(&bytes, b"\x1b[?25h"), 1);
}

#[test]
fn terminal_guard_restores_before_prior_hook_callback() {
    let (writer, output) = InjectedWriter::new(usize::MAX);
    let mut guard = ovrcr::tui::TerminalGuard::enter_with_writer(writer).unwrap();
    let observed = Arc::clone(&output);
    guard.restore_before(|| {
        let bytes = observed.lock().unwrap().clone();
        assert_eq!(occurrences(&bytes, b"\x1b[?1049l"), 1);
        assert_eq!(occurrences(&bytes, b"\x1b[?2004l"), 1);
        assert_eq!(occurrences(&bytes, b"\x1b[?1000l"), 1);
        assert_eq!(occurrences(&bytes, b"\x1b[?25h"), 1);
    });
    drop(guard);
    let bytes = output.lock().unwrap().clone();
    assert_eq!(occurrences(&bytes, b"\x1b[?1049l"), 1);
}

#[test]
fn terminal_guard_rolls_back_modes_when_a_later_enter_write_fails() {
    let (writer, output) = InjectedWriter::new(2);
    assert!(ovrcr::tui::TerminalGuard::enter_with_writer(writer).is_err());
    let bytes = output.lock().unwrap().clone();
    assert_eq!(occurrences(&bytes, b"\x1b[?1049l"), 1);
}

#[test]
fn ctrl_g_is_reserved_and_other_control_keys_reach_the_pty() {
    assert_eq!(
        encode_key(
            KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
            false
        ),
        KeyEncoding::Browse
    );
    assert_eq!(
        encode_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            false
        ),
        KeyEncoding::Bytes(vec![3])
    );
    assert_eq!(
        encode_key(
            KeyEvent::new(KeyCode::Char('\u{7}'), KeyModifiers::NONE),
            false
        ),
        KeyEncoding::Browse
    );
}

#[test]
fn paste_honors_bracketed_paste_mode() {
    assert_eq!(encode_paste("hello\n", false), b"hello\n".to_vec());
    assert_eq!(
        encode_paste("hello\n", true),
        b"\x1b[200~hello\n\x1b[201~".to_vec()
    );
}

#[test]
fn render_terminal_copies_text_style_wide_cells_and_cursor() {
    let mut parser = vt100::Parser::new(3, 12, 0);
    parser.process(b"plain \x1b[31mred\x1b[0m\r\nwide: \xE7\x95\x8C");
    let backend = TestBackend::new(12, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            ovrcr::tui::render_terminal(frame, Rect::new(0, 0, 12, 3), parser.screen(), true);
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
                ovrcr::tui::render_terminal(frame, Rect::new(0, 0, 20, 2), initial.screen(), false);
            })
            .unwrap();

        let mut replacement = vt100::Parser::new(2, 20, 0);
        replacement.process(b"ok");
        terminal
            .draw(|frame| {
                ovrcr::tui::render_terminal(
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
    let mut parser = vt100::Parser::new(1, 1, 0);
    parser.process(b"\x1b[31;42;7mX");
    let backend = TestBackend::new(1, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            ovrcr::tui::render_terminal(frame, Rect::new(0, 0, 1, 1), parser.screen(), true);
        })
        .unwrap();
    let cell = &terminal.backend().buffer()[(0, 0)];
    assert_eq!(cell.fg, Color::Indexed(1));
    assert_eq!(cell.bg, Color::Indexed(2));
    assert!(cell.modifier.contains(Modifier::REVERSED));
}

#[test]
fn runtime_paste_event_becomes_selected_session_input() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_request(SessionId(5), 1);
    let size = dashboard.panes[dashboard.focused_pane].desired_size;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::Screen {
            session: SessionId(5),
            revision: dashboard.view_revision,
            size,
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::Ok,
    });
    dashboard.panes[dashboard.focused_pane]
        .parser
        .process(b"\x1b[?2004h");
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    let request = event_to_request(&mut dashboard, Event::Paste("hello\n".into()), 2).unwrap();
    assert_eq!(
        request.request,
        ovrcr::protocol::Request::Input {
            session: SessionId(5),
            bytes: b"\x1b[200~hello\n\x1b[201~".to_vec(),
        }
    );
}

#[test]
fn dashboard_reader_channel_applies_bounded_backpressure() {
    let (sender, receiver) = dashboard_message_channel();
    for _ in 0..DASHBOARD_READER_QUEUE_CAPACITY {
        sender
            .send(ServerMessage::Event(ServerEvent::ScreenDirty {
                session: SessionId(1),
                revision: 0,
            }))
            .unwrap();
    }
    assert!(matches!(
        sender.try_send(ServerMessage::Response {
            request_id: 1,
            response: Response::Ok,
        }),
        Err(TrySendError::Full(_))
    ));
    drop(receiver);
}

#[test]
fn history_waits_for_coalesced_focus_view_before_capture() {
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    let split = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 901)
        .unwrap()
        .unwrap();
    acknowledge_all_view_targets(&mut dashboard, split);
    let captured_session = dashboard.focused_session().unwrap();
    assert!(dashboard.focus_pane(0));
    let pending = dashboard
        .view_request(dashboard.outer_area, 902)
        .unwrap()
        .unwrap();
    assert!(dashboard.focus_pane(1));
    assert!(
        dashboard
            .view_request(dashboard.outer_area, 903)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.history_begin_request.is_none());
    assert_eq!(
        dashboard.error.as_deref(),
        Some("Pane is loading; retry history")
    );

    let Request::SetView { view } = pending.request else {
        panic!("expected view")
    };
    deliver_all_view_screens(&mut dashboard, pending.request_id, &view);
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: pending.request_id,
        response: Response::Ok,
    });
    let [replacement] = outgoing.as_slice() else {
        panic!("expected coalesced view: {outgoing:?}")
    };
    assert!(matches!(replacement.request, Request::SetView { .. }));
    assert_eq!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.history_begin_request.is_none());
    acknowledge_all_view_targets(&mut dashboard, replacement.clone());
    let ovrcr::tui::DashboardAction::Request(begin) = dashboard.key(KeyCode::PageUp) else {
        panic!("history must open after the committed focus is acknowledged");
    };
    assert_eq!(
        begin.request,
        Request::HistoryBegin {
            session: captured_session
        }
    );
    assert_eq!(
        dashboard.history_begin_request.as_ref().unwrap().session,
        captured_session
    );
}

#[test]
fn output_for_newly_selected_session_waits_for_screen() {
    let mut dashboard = copy_ready_dashboard();
    assert!(
        dashboard.panes[0]
            .parser
            .screen()
            .contents()
            .contains("abc")
    );
    dashboard.select_request(SessionId(5), 901).unwrap();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(5),
        revision: dashboard.view_revision,
        bytes: b"EARLY OUTPUT".to_vec(),
    }));
    let contents = dashboard.panes[0].parser.screen().contents();
    assert!(
        !contents.contains("EARLY OUTPUT"),
        "output applied before the screen arrived: {contents}"
    );
    assert!(!contents.contains("abc"), "stale pane kept: {contents}");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Screen {
            session: SessionId(5),
            revision: dashboard.view_revision,
            size: dashboard.panes[0].desired_size,
            bytes: b"FIVE SCREEN".to_vec(),
        },
    });
    assert!(
        dashboard.panes[0]
            .parser
            .screen()
            .contents()
            .contains("FIVE SCREEN")
    );
    assert!(!dashboard.panes[0].ready);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Ok,
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(5),
        revision: dashboard.view_revision,
        bytes: b" LIVE".to_vec(),
    }));
    assert!(
        dashboard.panes[0]
            .parser
            .screen()
            .contents()
            .contains("FIVE SCREEN LIVE")
    );
}

#[test]
fn split_state_focus_and_close_preserve_sessions() {
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    assert_eq!(dashboard.panes.len(), 2);
    assert_eq!(dashboard.focused_pane, 1);
    let right = dashboard.focused_session().unwrap();
    assert_ne!(right, SessionId(1));
    assert!(!dashboard.split_pane());
    assert!(dashboard.close_focused_pane());
    assert_eq!(dashboard.panes.len(), 1);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(!dashboard.close_focused_pane());

    let mut single = dashboard_fixture();
    for project in &mut single.hierarchy.projects {
        for workspace in &mut project.workspaces {
            workspace
                .sessions
                .retain(|session| session.id == SessionId(1));
        }
    }
    assert!(!single.split_pane());
    assert_eq!(
        single.error.as_deref(),
        Some("No other visible session to split")
    );
}

#[test]
fn pane_rects_keep_split_geometry_and_hide_unfocused_tiny_pane() {
    let split = pane_rects(Rect::new(0, 0, 120, 40), 2, 0);
    assert_eq!(split[0].terminal, Rect::new(40, 3, 39, 36));
    assert_eq!(split[1].terminal, Rect::new(80, 3, 40, 36));

    let focused_only = pane_rects(Rect::new(0, 0, 80, 24), 2, 1);
    assert_eq!(focused_only.len(), 1);
    assert_eq!(focused_only[0].pane_index, 1);
    assert_eq!(focused_only[0].terminal, Rect::new(40, 3, 40, 20));
    assert!(pane_rects(Rect::new(0, 0, 1, 1), 2, 0).is_empty());
}

#[test]
fn split_layout_geometry_and_narrow_fallback() {
    let rects = pane_rects(Rect::new(0, 0, 120, 40), 2, 1);
    assert_eq!(rects[0].terminal, Rect::new(40, 3, 39, 36));
    assert_eq!(rects[1].terminal, Rect::new(80, 3, 40, 36));

    let narrow = pane_rects(Rect::new(0, 0, 80, 24), 2, 1);
    assert_eq!(narrow.len(), 1);
    assert_eq!(narrow[0].pane_index, 1);
    assert_eq!(narrow[0].terminal, Rect::new(40, 3, 40, 20));
    assert!(pane_rects(Rect::new(0, 0, 1, 1), 2, 1).is_empty());
}

#[test]
fn split_layout_renders_independent_cells_and_cursor() {
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    let left_session = dashboard.panes[0].session.expect("left split session");
    let right_session = dashboard.panes[1].session.expect("right split session");
    let split = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 100)
        .unwrap()
        .expect("split view should request both panes");
    let initial_view = match &split.request {
        Request::SetView { view } => view,
        other => panic!("expected SetView, got {other:?}"),
    };
    assert_eq!(initial_view.focused, Some(right_session));
    assert_eq!(initial_view.panes.len(), 2);
    assert_eq!(initial_view.panes[0].session, left_session);
    assert_eq!(
        initial_view.panes[0].size,
        TerminalSize { rows: 36, cols: 39 }
    );
    assert_eq!(initial_view.panes[1].session, right_session);
    assert_eq!(
        initial_view.panes[1].size,
        TerminalSize { rows: 36, cols: 40 }
    );
    let mut loading_terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    loading_terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let loading_buffer = loading_terminal.backend().buffer();
    let loading_left = (40..79)
        .map(|x| loading_buffer[(x, 1)].symbol())
        .collect::<String>();
    let loading_right = (80..120)
        .map(|x| loading_buffer[(x, 1)].symbol())
        .collect::<String>();
    assert!(loading_left.starts_with("  loading review"));
    assert!(loading_right.starts_with("> loading local"));
    acknowledge_all_view_targets(&mut dashboard, split);

    dashboard.panes[0]
        .parser
        .process(b"\x1b[31mLEFT\x1b[0m\x1b[36;38H\xE7\x95\x8C\x1b[31m\x1b[36;1H<\x1b[2;4H");
    dashboard.panes[1]
        .parser
        .process(b"\x1b[32mRIGHT\x1b[0m\x1b[36;39H\xE7\x95\x8C\x1b[32m\x1b[36;1H>\x1b[2;5H");
    dashboard.mode = ovrcr::tui::InputMode::Terminal;

    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let left_metadata = (40..79)
        .map(|x| buffer[(x, 1)].symbol())
        .collect::<String>();
    let right_metadata = (80..120)
        .map(|x| buffer[(x, 1)].symbol())
        .collect::<String>();
    assert!(left_metadata.starts_with("  review 39x36"));
    assert!(right_metadata.starts_with("> local 40x36"));
    assert_eq!(buffer[(40, 3)].symbol(), "L");
    assert_eq!(buffer[(80, 3)].symbol(), "R");
    assert_eq!(buffer[(40, 3 + 35)].symbol(), "<");
    assert_eq!(buffer[(80, 3 + 35)].symbol(), ">");
    assert_eq!(buffer[(40, 3 + 35)].fg, Color::Indexed(1));
    assert_eq!(buffer[(80, 3 + 35)].fg, Color::Indexed(2));
    assert_eq!(buffer[(40 + 37, 3 + 35)].symbol(), "界");
    assert_eq!(buffer[(40 + 38, 3 + 35)].symbol(), " ");
    assert_eq!(buffer[(80 + 38, 3 + 35)].symbol(), "界");
    assert_eq!(buffer[(80 + 39, 3 + 35)].symbol(), " ");
    for row in 1..39 {
        assert_eq!(buffer[(79, row)].symbol(), "│", "separator row {row}");
    }
    assert_eq!(terminal.backend().cursor_position(), (84, 4).into());

    dashboard.mode = ovrcr::tui::InputMode::Browse;
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let browse_footer = (0..120)
        .map(|x| terminal.backend().buffer()[(x, 39)].symbol())
        .collect::<String>();
    assert!(browse_footer.contains("BROWSE"));
    assert!(
        !browse_footer.split_whitespace().any(|key| key == "v"),
        "split is disabled when already split"
    );
    assert!(browse_footer.contains("Tab x"));
    assert!(browse_footer.contains("Space leader  ? help"));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;

    let mut output = Vec::new();
    {
        let backend = CrosstermBackend::new(&mut output);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 120, 40)),
            },
        )
        .unwrap();
        terminal
            .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
    }
    let mut emitted = vt100::Parser::new(40, 120, 0);
    emitted.process(&output);
    let emitted_left = (40..44)
        .map(|column| emitted.screen().cell(3, column).unwrap().contents())
        .collect::<String>();
    let emitted_right = (80..85)
        .map(|column| emitted.screen().cell(3, column).unwrap().contents())
        .collect::<String>();
    assert_eq!(emitted_left, "LEFT");
    assert_eq!(emitted_right, "RIGHT");
    assert_eq!(emitted.screen().cell(38, 77).unwrap().contents(), "界");
    assert!(
        emitted
            .screen()
            .cell(38, 78)
            .unwrap()
            .is_wide_continuation()
    );
    assert_eq!(emitted.screen().cell(38, 79).unwrap().contents(), "│");
    assert_eq!(emitted.screen().cell(38, 118).unwrap().contents(), "界");
    assert!(
        emitted
            .screen()
            .cell(38, 119)
            .unwrap()
            .is_wide_continuation()
    );
    assert_eq!(emitted.screen().cell(38, 40).unwrap().contents(), "<");
    assert_eq!(emitted.screen().cell(38, 80).unwrap().contents(), ">");

    let mut edge_parser = vt100::Parser::new(36, 41, 0);
    edge_parser.process(b"\x1b[36;40H\xE7\x95\x8C");
    dashboard.panes[1].parser = edge_parser;
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(119, 38)].symbol(), " ");
    assert_eq!(terminal.backend().buffer()[(79, 38)].symbol(), "│");
    let mut edge_output = Vec::new();
    {
        let backend = CrosstermBackend::new(&mut edge_output);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 120, 40)),
            },
        )
        .unwrap();
        terminal
            .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
    }
    let mut emitted = vt100::Parser::new(40, 120, 0);
    emitted.process(&edge_output);
    assert_eq!(emitted.screen().cell(38, 119).unwrap().contents(), " ");
    assert_eq!(emitted.screen().cell(38, 79).unwrap().contents(), "│");

    let mut left_edge_parser = vt100::Parser::new(36, 40, 0);
    left_edge_parser.process(b"\x1b[36;39H\xE7\x95\x8C");
    dashboard.panes[0].parser = left_edge_parser;
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(78, 38)].symbol(), " ");
    assert_eq!(terminal.backend().buffer()[(79, 38)].symbol(), "│");
    let mut left_edge_output = Vec::new();
    {
        let backend = CrosstermBackend::new(&mut left_edge_output);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 120, 40)),
            },
        )
        .unwrap();
        terminal
            .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
    }
    let mut emitted = vt100::Parser::new(40, 120, 0);
    emitted.process(&left_edge_output);
    assert_eq!(emitted.screen().cell(38, 78).unwrap().contents(), " ");
    assert_eq!(emitted.screen().cell(38, 79).unwrap().contents(), "│");

    assert!(dashboard.focus_pane(0));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    dashboard.panes[0].parser.process(b"\x1b[?25h\x1b[2;4H");
    assert!(
        dashboard
            .hierarchy
            .projects
            .iter()
            .flat_map(|project| project.workspaces.iter())
            .flat_map(|workspace| workspace.sessions.iter())
            .find(|session| Some(session.id) == dashboard.panes[0].session)
            .is_some_and(|session| matches!(session.phase, SessionPhase::Running))
    );
    assert!(!dashboard.panes[0].parser.screen().hide_cursor());
    let focused_left = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 101)
        .unwrap()
        .expect("focus change should request replacement view");
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());
    acknowledge_all_view_targets(&mut dashboard, focused_left);
    dashboard.panes[0].parser.process(b"\x1b[2;4H");
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), (43, 4).into());

    dashboard.panes[0].parser.process(b"\x1b[?25l");
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());

    dashboard.panes[0].parser.process(b"\x1b[?25h\x1b[2;4H");
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());

    let backend = TestBackend::new(1, 1);
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    dashboard.panes[0].parser.process(b"\x1b[?25h\x1b[2;4H");
    assert!(
        dashboard
            .hierarchy
            .projects
            .iter()
            .flat_map(|project| project.workspaces.iter())
            .flat_map(|workspace| workspace.sessions.iter())
            .find(|session| Some(session.id) == dashboard.panes[0].session)
            .is_some_and(|session| matches!(session.phase, SessionPhase::Running))
    );
    assert!(!dashboard.panes[0].parser.screen().hide_cursor());
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    let narrow = dashboard
        .view_request(Rect::new(0, 0, 80, 24), 102)
        .unwrap()
        .expect("narrow view should request the focused pane");
    let narrow_view = match &narrow.request {
        Request::SetView { view } => view,
        other => panic!("expected SetView, got {other:?}"),
    };
    assert_eq!(narrow_view.focused, Some(left_session));
    assert_eq!(narrow_view.panes.len(), 1);
    assert_eq!(narrow_view.panes[0].session, left_session);
    assert_eq!(
        narrow_view.panes[0].size,
        TerminalSize { rows: 20, cols: 40 }
    );
    assert_eq!(
        dashboard
            .panes
            .iter()
            .map(|pane| pane.session)
            .collect::<Vec<_>>(),
        vec![Some(left_session), Some(right_session)]
    );
    assert_eq!(pane_rects(Rect::new(0, 0, 80, 24), 2, 0).len(), 1);
    acknowledge_all_view_targets(&mut dashboard, narrow);
    dashboard.panes[0].parser.process(b"L\x1b[2;4H");
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(40, 3)].symbol(), "L");
    assert_eq!(terminal.backend().cursor_position(), (43, 4).into());
    let footer = (0..80)
        .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
        .collect::<String>();
    assert!(footer.contains("split hidden: terminal too small"));

    let mut ordinary = dashboard_fixture();
    ordinary.mode = ovrcr::tui::InputMode::Browse;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &ordinary, 0))
        .unwrap();
    let ordinary_footer = (0..80)
        .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
        .collect::<String>();
    assert!(ordinary_footer.contains("BROWSE"));
    assert!(ordinary_footer.split_whitespace().any(|key| key == "v"));
    assert!(
        !ordinary_footer
            .split_whitespace()
            .any(|key| matches!(key, "Tab" | "x")),
        "pane switch and close need a split"
    );
    assert!(ordinary_footer.contains("Space leader  ? help"));

    dashboard.mode = ovrcr::tui::InputMode::Browse;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let browse_footer = (0..80)
        .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
        .collect::<String>();
    assert!(browse_footer.contains("BROWSE"));
    assert!(browse_footer.contains("split hidden: terminal too small"));

    dashboard.key(KeyCode::Char('['));
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let copy_footer = (0..80)
        .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
        .collect::<String>();
    assert!(copy_footer.contains("COPY"));
    assert!(copy_footer.contains("split hidden: terminal too small"));
    assert!(copy_footer.contains("Space leader  ? help"));
    // At this width, the popup is the discoverable source for the full table.
    dashboard.key(KeyCode::Char('?'));
    let copy_help = palette_text(&dashboard);
    assert!(copy_help.contains("Move"));
    assert!(copy_help.contains("Selection"));
    dashboard.key(KeyCode::Esc);
    dashboard.mode = ovrcr::tui::InputMode::History;
    dashboard.history = Some(HistoryView::new(history_opened(1), 0));
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let history_footer = (0..80)
        .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
        .collect::<String>();
    assert!(history_footer.contains("HISTORY"));
    assert!(history_footer.contains("Waiting"));
    assert!(history_footer.contains("split hidden: terminal too small"));
    dashboard.key(KeyCode::Char('?'));
    for _ in 0..30 {
        dashboard.key(KeyCode::Down);
    }
    assert!(palette_text(&dashboard).contains("Esc"));
    dashboard.key(KeyCode::Esc);
    dashboard.history = None;
    dashboard.mode = ovrcr::tui::InputMode::Terminal;

    let wide = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 103)
        .unwrap()
        .expect("wide view should restore both panes");
    let wide_view = match &wide.request {
        Request::SetView { view } => view,
        other => panic!("expected SetView, got {other:?}"),
    };
    assert_eq!(wide_view.focused, Some(left_session));
    assert_eq!(wide_view.panes.len(), 2);
    assert_eq!(wide_view.panes[0].session, left_session);
    assert_eq!(wide_view.panes[0].size, TerminalSize { rows: 36, cols: 39 });
    assert_eq!(wide_view.panes[1].session, right_session);
    assert_eq!(wide_view.panes[1].size, TerminalSize { rows: 36, cols: 40 });
    assert_eq!(
        dashboard
            .panes
            .iter()
            .map(|pane| pane.session)
            .collect::<Vec<_>>(),
        vec![Some(left_session), Some(right_session)]
    );
    acknowledge_all_view_targets(&mut dashboard, wide);
    dashboard.panes[0].parser.process(b"L");
    dashboard.panes[1].parser.process(b"R");
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(40, 3)].symbol(), "L");
    assert_eq!(terminal.backend().buffer()[(80, 3)].symbol(), "R");

    dashboard.mode = ovrcr::tui::InputMode::Browse;
    assert!(matches!(
        dashboard.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    ));
    assert!(dashboard.copy.is_some());
    assert_eq!(
        dashboard
            .copy
            .as_ref()
            .unwrap()
            .screen
            .cell(0, 0)
            .unwrap()
            .contents(),
        "L"
    );
    dashboard.panes[0].parser.process(b"\x1b[1;1HNEW");
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: right_session,
        revision: dashboard.view_revision,
        bytes: b"\x1b[1;1HOTHER".to_vec(),
    }));
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(40, 3)].symbol(), "L");
    assert_eq!(terminal.backend().buffer()[(80, 3)].symbol(), "O");

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    let mut exited_split = dashboard_fixture();
    assert!(exited_split.split_pane());
    let split_request = exited_split
        .view_request(Rect::new(0, 0, 120, 40), 200)
        .unwrap()
        .expect("split view should request both panes");
    acknowledge_all_view_targets(&mut exited_split, split_request);
    assert!(exited_split.focus_pane(0));
    let focused_request = exited_split
        .view_request(Rect::new(0, 0, 120, 40), 201)
        .unwrap()
        .expect("focus change should request the left pane");
    acknowledge_all_view_targets(&mut exited_split, focused_request);
    exited_split.mode = ovrcr::tui::InputMode::Terminal;
    exited_split.panes[0].parser.process(b"FINAL\x1b[2;4H");
    let mut exited_summary = exited_split
        .hierarchy
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .find(|session| session.id == SessionId(1))
        .cloned()
        .expect("fixture review session");
    exited_summary.phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    exited_summary.pid = None;
    let lifecycle_outgoing = exited_split.handle_server_message(ServerMessage::Event(
        ServerEvent::SessionChanged(Box::new(exited_summary)),
    ));
    assert!(lifecycle_outgoing.is_empty());
    assert_eq!(exited_split.focused_pane, 0);
    assert_eq!(exited_split.panes[0].session, Some(SessionId(1)));
    // Running sessions precede exited records in the current sidebar order.
    assert_eq!(exited_split.panes[1].session, Some(SessionId(4)));
    exited_split.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(4),
        revision: exited_split.view_revision,
        bytes: b"SURVIVOR".to_vec(),
    }));
    assert!(
        exited_split.panes[1]
            .parser
            .screen()
            .contents()
            .contains("SURVIVOR")
    );
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &exited_split, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(40, 3)].symbol(), "F");
    assert_eq!(terminal.backend().buffer()[(80, 3)].symbol(), "S");
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());

    let mut exited_single = dashboard_fixture();
    exited_single.mode = ovrcr::tui::InputMode::Terminal;
    exited_single.panes[0].parser.process(b"FINAL\x1b[2;4H");
    let mut exited_summary = exited_single
        .hierarchy
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .find(|session| session.id == SessionId(1))
        .cloned()
        .expect("fixture review session");
    exited_summary.phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    exited_summary.pid = None;
    exited_single.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
        Box::new(exited_summary),
    )));
    let backend = TestBackend::new(89, 38);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &exited_single, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(40, 3)].symbol(), "F");
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());

    let click = |column, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    let mouse_sessions = dashboard
        .panes
        .iter()
        .map(|pane| pane.session)
        .collect::<Vec<_>>();
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    assert_eq!(
        dashboard.mouse_action(click(40, 3), Rect::new(0, 0, 120, 40)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_pane, 0);
    assert_eq!(
        dashboard
            .panes
            .iter()
            .map(|pane| pane.session)
            .collect::<Vec<_>>(),
        mouse_sessions
    );
    assert_eq!(
        dashboard.mouse_action(click(79, 20), Rect::new(0, 0, 120, 40)),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(dashboard.focused_pane, 0);
    assert_eq!(
        dashboard
            .panes
            .iter()
            .map(|pane| pane.session)
            .collect::<Vec<_>>(),
        mouse_sessions
    );
    assert_eq!(
        dashboard.mouse_action(click(80, 1), Rect::new(0, 0, 120, 40)),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(dashboard.focused_pane, 0);
    assert_eq!(
        dashboard
            .panes
            .iter()
            .map(|pane| pane.session)
            .collect::<Vec<_>>(),
        mouse_sessions
    );
    assert_eq!(
        dashboard.mouse_action(click(80, 3), Rect::new(0, 0, 120, 40)),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_pane, 1);
    assert_eq!(
        dashboard
            .panes
            .iter()
            .map(|pane| pane.session)
            .collect::<Vec<_>>(),
        mouse_sessions
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.mouse_action(click(40, 3), Rect::new(0, 0, 120, 40)),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(dashboard.focused_pane, 1);
    assert_eq!(
        dashboard
            .panes
            .iter()
            .map(|pane| pane.session)
            .collect::<Vec<_>>(),
        mouse_sessions
    );
}

#[test]
fn control_punctuation_and_modified_keys_encode() {
    let bytes = |code: KeyCode, modifiers: KeyModifiers| match encode_key(
        KeyEvent::new(code, modifiers),
        false,
    ) {
        KeyEncoding::Bytes(bytes) => bytes,
        other => panic!("{code:?} with {modifiers:?} encoded as {other:?}"),
    };
    for (code, expected) in [
        (KeyCode::Char(' '), 0x00),
        (KeyCode::Char('@'), 0x00),
        (KeyCode::Char('\\'), 0x1c),
        (KeyCode::Char('4'), 0x1c),
        (KeyCode::Char(']'), 0x1d),
        (KeyCode::Char('5'), 0x1d),
        (KeyCode::Char('^'), 0x1e),
        (KeyCode::Char('6'), 0x1e),
        (KeyCode::Char('_'), 0x1f),
        (KeyCode::Char('7'), 0x1f),
    ] {
        assert_eq!(
            bytes(code, KeyModifiers::CONTROL),
            vec![expected],
            "{code:?}"
        );
    }
    assert_eq!(bytes(KeyCode::Up, KeyModifiers::SHIFT), b"\x1b[1;2A");
    assert_eq!(bytes(KeyCode::Right, KeyModifiers::CONTROL), b"\x1b[1;5C");
    assert_eq!(bytes(KeyCode::Backspace, KeyModifiers::ALT), b"\x1b\x7f");
    assert_eq!(bytes(KeyCode::Enter, KeyModifiers::SHIFT), b"\x1b[13;2u");
    assert_eq!(bytes(KeyCode::PageUp, KeyModifiers::SHIFT), b"\x1b[5;2~");
    assert_eq!(bytes(KeyCode::Enter, KeyModifiers::NONE), b"\r");
    assert_eq!(bytes(KeyCode::Backspace, KeyModifiers::NONE), b"\x7f");
}

#[test]
fn palette_escape_cancels_pending_request() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "register project");
    dashboard.key(KeyCode::Enter);
    let mut submitted = None;
    for value in ["late-project", "/tmp/late-repo", "/tmp/late-root"] {
        dashboard.event_action(Event::Paste(value.into()));
        if let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) {
            submitted = Some(message);
        }
    }
    let message = submitted.expect("form did not submit");
    assert!(palette_text(&dashboard).contains("Working…"));
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
    assert!(!palette_text(&dashboard).contains("Command palette"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "late failure".into(),
        },
    });
    assert!(!palette_text(&dashboard).contains("Command palette"));
    assert_eq!(dashboard.error, None);
}

#[test]
fn split_state_modes_and_input_are_local() {
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    let right = dashboard.focused_session().unwrap();
    let request = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 20)
        .unwrap()
        .unwrap();
    let revision = dashboard.view_revision;
    dashboard.apply_screen(
        revision,
        right,
        TerminalSize { rows: 36, cols: 40 },
        b"\x1b[?1h\x1b[?2004hRIGHT",
    );
    let left = dashboard.panes[0].session.unwrap();
    dashboard.apply_screen(revision, left, TerminalSize { rows: 36, cols: 39 }, b"LEFT");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
    assert!(
        dashboard
            .view_request(Rect::new(0, 0, 120, 40), 21)
            .unwrap()
            .is_none()
    );
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.event_action(Event::Paste("x".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[200~x\x1b[201~".to_vec())
    );
    assert_eq!(
        dashboard.key(KeyCode::Up),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1bOA".to_vec())
    );
    assert_eq!(
        dashboard.input_request(vec![b'x'], 21).unwrap().request,
        Request::Input {
            session: right,
            bytes: vec![b'x']
        }
    );
}

#[test]
fn split_review_readiness_requires_matching_targets_and_cancels_resize_copy() {
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    let request = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 20)
        .unwrap()
        .unwrap();
    acknowledge_all_view_targets(&mut dashboard, request);
    assert!(dashboard.panes.iter().all(|pane| pane.ready));

    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    let focused = dashboard.focused_session().unwrap();
    assert!(dashboard.input_request(vec![b'x'], 21).is_some());
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    assert!(dashboard.key(KeyCode::Char('[')) == ovrcr::tui::DashboardAction::Redraw);
    assert!(dashboard.copy.is_some());
    let resized = dashboard
        .view_request(Rect::new(0, 0, 120, 44), 22)
        .unwrap()
        .expect("resize should issue SetView through the shared path");
    assert!(dashboard.copy.is_none());
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(!matches!(
        dashboard.key(KeyCode::Char('y')),
        ovrcr::tui::DashboardAction::CopyText(_)
    ));

    acknowledge_all_view_targets(&mut dashboard, resized);
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
    for pane in &mut dashboard.panes {
        pane.parser.process(b"HIDDEN_STATE");
    }
    let hidden_state = dashboard
        .panes
        .iter()
        .map(|pane| {
            (
                pane.session,
                pane.size,
                pane.parser.screen().contents().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let shrink = dashboard
        .view_request(Rect::new(0, 0, 50, 10), 23)
        .unwrap()
        .expect("narrow view should hide one pane and request the focused target");
    let Request::SetView { ref view } = shrink.request else {
        panic!("expected SetView");
    };
    assert_eq!(view.panes.len(), 1);
    assert!(
        view.panes
            .iter()
            .all(|pane| pane.size.rows > 0 && pane.size.cols > 0)
    );
    assert!(dashboard.panes.iter().enumerate().all(|(index, pane)| {
        let (session, size, contents) = &hidden_state[index];
        pane.session == *session
            && pane.size == *size
            && pane.parser.screen().contents() == *contents
    }));
    let shrink_target = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: shrink.request_id,
        response: Response::Screen {
            session: shrink_target.session,
            revision: view.revision,
            size: shrink_target.size,
            bytes: b"SHRUNK".to_vec(),
        },
    });
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: shrink.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes[dashboard.focused_pane].ready);
    assert!(
        dashboard
            .panes
            .iter()
            .enumerate()
            .all(|(index, pane)| index == dashboard.focused_pane || !pane.ready)
    );
    let hidden_index = (0..dashboard.panes.len())
        .find(|index| *index != dashboard.focused_pane)
        .expect("narrow view should retain a hidden pane");
    let (hidden_session, hidden_size, hidden_contents) = &hidden_state[hidden_index];
    let hidden_pane = &dashboard.panes[hidden_index];
    assert_eq!(hidden_pane.session, *hidden_session);
    assert_eq!(hidden_pane.size, *hidden_size);
    assert_eq!(hidden_pane.parser.screen().contents(), *hidden_contents);

    let normal = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 25)
        .unwrap()
        .unwrap();
    let Request::SetView { ref view } = normal.request else {
        panic!("expected SetView");
    };
    deliver_all_view_screens(&mut dashboard, normal.request_id, view);
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 24).is_none());
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: normal.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
    assert_eq!(dashboard.focused_session(), Some(focused));

    let empty = dashboard
        .view_request(Rect::new(0, 0, 1, 1), 26)
        .unwrap()
        .expect("empty view should still emit SetView");
    let Request::SetView { ref view } = empty.request else {
        panic!("expected SetView");
    };
    assert!(view.panes.is_empty());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: empty.request_id,
        response: Response::Ok,
    });
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 27).is_none());
    dashboard.mode = ovrcr::tui::InputMode::Browse;

    let mut dirty_copy = copy_ready_dashboard();
    assert_eq!(
        dirty_copy.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dirty_copy.copy.is_some());
    let dirty_outgoing =
        dirty_copy.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session: dirty_copy.focused_session().unwrap(),
            revision: dirty_copy.view_revision,
        }));
    assert!(matches!(
        dirty_outgoing.as_slice(),
        [ClientMessage {
            request: Request::SetView { .. },
            ..
        }]
    ));
    assert!(dirty_copy.copy.is_some());

    assert!(dashboard.focus_pane(0));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 28).is_none());
    let focused_view = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 27)
        .unwrap()
        .unwrap();
    acknowledge_all_view_targets(&mut dashboard, focused_view);
    assert!(dashboard.select_request(SessionId(1), 28).is_none());
    assert!(dashboard.panes[dashboard.focused_pane].ready);
}

#[test]
fn split_review_preserves_nonfirst_survivor_on_hierarchy_removal() {
    let mut dashboard = dashboard_fixture();
    let select = dashboard
        .select_request(SessionId(2), 30)
        .expect("selecting a running session should request a view");
    acknowledge_view_request(&mut dashboard, select);
    assert!(dashboard.split_pane());
    let split = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 31)
        .unwrap()
        .unwrap();
    let survivor = dashboard.panes[0].session.unwrap();
    let removed = dashboard.panes[1].session.unwrap();
    let split_request_id = split.request_id;
    acknowledge_all_view_targets(&mut dashboard, split);
    let split_revision = dashboard.view_revision;
    let mut hierarchy = dashboard.hierarchy.clone();
    for project in &mut hierarchy.projects {
        for workspace in &mut project.workspaces {
            workspace.sessions.retain(|session| session.id != removed);
        }
    }
    let outgoing = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::HierarchyChanged(hierarchy),
    ));
    assert_eq!(dashboard.panes.len(), 1);
    assert_eq!(dashboard.panes[0].session, Some(survivor));
    assert!(
        outgoing
            .iter()
            .any(|request| matches!(request.request, Request::SetView { .. }))
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: split_request_id,
        response: Response::Screen {
            session: removed,
            revision: split_revision,
            size: TerminalSize { rows: 36, cols: 40 },
            bytes: b"removed old".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: removed,
        revision: split_revision,
        bytes: b"removed output".to_vec(),
    }));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
        session: removed,
        revision: split_revision,
    }));
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| pane.session != Some(removed))
    );
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| !pane.parser.screen().contents().contains("removed"))
    );
}

#[test]
fn split_review_requires_all_screens_before_ok_and_ignores_stale_view_completion() {
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    let request = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 40)
        .unwrap()
        .unwrap();
    let Request::SetView { view } = request.request.clone() else {
        panic!("expected SetView");
    };
    let first = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id.saturating_add(1),
        response: Response::Screen {
            session: first.session,
            revision: view.revision,
            size: first.size,
            bytes: b"wrong request".to_vec(),
        },
    });
    assert!(
        dashboard
            .view_request(Rect::new(0, 0, 120, 40), 401)
            .unwrap()
            .is_none()
    );
    assert_eq!(dashboard.view_revision, view.revision);
    assert!(
        dashboard.panes.iter().all(|pane| !pane
            .parser
            .screen()
            .contents()
            .contains("wrong request"))
    );
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    assert_eq!(dashboard.view_revision, view.revision);
    assert_view_input_blocked(&mut dashboard, 40);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: first.session,
            revision: view.revision.saturating_sub(1),
            size: first.size,
            bytes: b"wrong revision".to_vec(),
        },
    });
    assert!(
        dashboard
            .view_request(Rect::new(0, 0, 120, 40), 402)
            .unwrap()
            .is_none()
    );
    assert_eq!(dashboard.view_revision, view.revision);
    assert!(
        dashboard.panes.iter().all(|pane| !pane
            .parser
            .screen()
            .contents()
            .contains("wrong revision"))
    );
    assert_eq!(dashboard.view_revision, view.revision);
    assert_view_input_blocked(&mut dashboard, 41);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: SessionId(5),
            revision: view.revision,
            size: first.size,
            bytes: b"wrong session".to_vec(),
        },
    });
    assert!(
        dashboard
            .view_request(Rect::new(0, 0, 120, 40), 403)
            .unwrap()
            .is_none()
    );
    assert_eq!(dashboard.view_revision, view.revision);
    assert!(
        dashboard.panes.iter().all(|pane| !pane
            .parser
            .screen()
            .contents()
            .contains("wrong session"))
    );
    assert_eq!(dashboard.view_revision, view.revision);
    assert_view_input_blocked(&mut dashboard, 42);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: first.session,
            revision: view.revision,
            size: first.size,
            bytes: b"first".to_vec(),
        },
    });
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 41).is_none());
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("blocked".into())),
        ovrcr::tui::DashboardAction::Redraw
    );
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    let second = view.panes[1].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: second.session,
            revision: view.revision,
            size: second.size,
            bytes: b"second".to_vec(),
        },
    });
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("still-blocked".into())),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.input_request(vec![b'x'], 42).is_none());
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    for text in ["wrong request", "wrong revision", "wrong session"] {
        assert!(
            dashboard
                .panes
                .iter()
                .all(|pane| !pane.parser.screen().contents().contains(text)),
            "stale payload installed: {text}"
        );
    }
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id.saturating_add(1),
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    assert_eq!(dashboard.view_revision, view.revision);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("wrong-ok".into())),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.input_request(vec![b'x'], 42).is_none());
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 43).is_some());
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    let focused_before_narrow = dashboard.focused_session();

    let narrow = dashboard
        .view_request(Rect::new(0, 0, 80, 40), 44)
        .unwrap()
        .expect("narrow geometry should request a replacement view");
    let Request::SetView { ref view } = narrow.request else {
        panic!("expected SetView");
    };
    let narrow_revision = view.revision;
    assert_eq!(narrow_revision, dashboard.view_revision);
    assert_eq!(view.panes.len(), 1);
    assert_eq!(view.focused, focused_before_narrow);
    assert!(
        dashboard
            .view_request(Rect::new(0, 0, 120, 40), 45)
            .unwrap()
            .is_none()
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 46).is_none());
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    // `Ok` is final and every snapshot precedes it, so this acknowledgement without the narrow
    // view's Screen is a failure: the dashboard refreshes instead of waiting for a receipt that
    // can no longer arrive.
    let mut outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: narrow.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    assert_eq!(outgoing.len(), 1);
    let latest = outgoing.pop().expect("latest desired view request");
    let Request::SetView { ref view } = latest.request else {
        panic!("expected replacement SetView");
    };
    assert_eq!(view.revision, narrow_revision + 1);
    assert_eq!(view.revision, dashboard.view_revision);
    assert_eq!(view.panes.len(), 2);
    assert_eq!(view.focused, focused_before_narrow);
    let expected_targets = pane_rects(Rect::new(0, 0, 120, 40), 2, dashboard.focused_pane)
        .into_iter()
        .map(|rect| {
            (
                dashboard.panes[rect.pane_index].session.unwrap(),
                TerminalSize {
                    rows: rect.terminal.height,
                    cols: rect.terminal.width,
                },
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        expected_targets,
        vec![
            (SessionId(1), TerminalSize { rows: 36, cols: 39 }),
            (SessionId(4), TerminalSize { rows: 36, cols: 40 }),
        ]
    );
    assert_eq!(
        view.panes
            .iter()
            .map(|pane| (pane.session, pane.size))
            .collect::<Vec<_>>(),
        expected_targets
    );
    assert_view_input_blocked(&mut dashboard, 47);
    // The abandoned narrow request can neither install a snapshot nor complete a second time.
    deliver_all_view_screens(
        &mut dashboard,
        narrow.request_id,
        match &narrow.request {
            Request::SetView { view } => view,
            _ => panic!("expected SetView"),
        },
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: narrow.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    assert_eq!(dashboard.view_revision, narrow_revision + 1);
    deliver_all_view_screens(&mut dashboard, latest.request_id, view);
    assert_view_input_blocked(&mut dashboard, 48);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: latest.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
}

#[test]
fn split_review_pending_view_completes_after_focus_and_geometry_return() {
    let mut focus_dashboard = dashboard_fixture();
    assert!(focus_dashboard.split_pane());
    let focus_view = focus_dashboard
        .view_request(Rect::new(0, 0, 120, 40), 43)
        .unwrap()
        .unwrap();
    let Request::SetView { ref view } = focus_view.request else {
        panic!("expected SetView");
    };
    let focus_revision = view.revision;
    deliver_all_view_screens(&mut focus_dashboard, focus_view.request_id, view);
    assert!(focus_dashboard.focus_pane(0));
    assert!(focus_dashboard.focus_pane(1));
    focus_dashboard.handle_server_message(ServerMessage::Response {
        request_id: focus_view.request_id,
        response: Response::Ok,
    });
    assert_eq!(focus_dashboard.view_revision, focus_revision);
    assert!(focus_dashboard.panes.iter().all(|pane| pane.ready));

    let mut changed_focus_dashboard = dashboard_fixture();
    assert!(changed_focus_dashboard.split_pane());
    let changed_focus_view = changed_focus_dashboard
        .view_request(Rect::new(0, 0, 120, 40), 44)
        .unwrap()
        .unwrap();
    let Request::SetView { ref view } = changed_focus_view.request else {
        panic!("expected SetView");
    };
    let changed_focus_revision = view.revision;
    deliver_all_view_screens(
        &mut changed_focus_dashboard,
        changed_focus_view.request_id,
        view,
    );
    assert!(changed_focus_dashboard.focus_pane(0));
    assert!(
        changed_focus_dashboard
            .view_request(Rect::new(0, 0, 120, 40), 45)
            .unwrap()
            .is_none()
    );
    let mut replacement = changed_focus_dashboard.handle_server_message(ServerMessage::Response {
        request_id: changed_focus_view.request_id,
        response: Response::Ok,
    });
    assert_eq!(replacement.len(), 1);
    assert!(changed_focus_dashboard.panes.iter().all(|pane| !pane.ready));
    changed_focus_dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(
        changed_focus_dashboard
            .input_request(vec![b'x'], 46)
            .is_none()
    );
    let replacement_request = replacement.pop().expect("focus replacement SetView");
    let Request::SetView { ref view } = replacement_request.request else {
        panic!("expected focus replacement SetView");
    };
    assert_eq!(view.revision, changed_focus_revision + 1);
    assert_eq!(view.focused, Some(SessionId(1)));
    acknowledge_all_view_targets(&mut changed_focus_dashboard, replacement_request);
    assert!(changed_focus_dashboard.panes.iter().all(|pane| pane.ready));

    let mut geometry_dashboard = dashboard_fixture();
    assert!(geometry_dashboard.split_pane());
    let geometry_view = geometry_dashboard
        .view_request(Rect::new(0, 0, 120, 40), 44)
        .unwrap()
        .unwrap();
    let Request::SetView { ref view } = geometry_view.request else {
        panic!("expected SetView");
    };
    let geometry_revision = view.revision;
    deliver_all_view_screens(&mut geometry_dashboard, geometry_view.request_id, view);
    assert!(
        geometry_dashboard
            .view_request(Rect::new(0, 0, 80, 40), 45)
            .unwrap()
            .is_none()
    );
    assert!(
        geometry_dashboard
            .view_request(Rect::new(0, 0, 120, 40), 46)
            .unwrap()
            .is_none()
    );
    geometry_dashboard.handle_server_message(ServerMessage::Response {
        request_id: geometry_view.request_id,
        response: Response::Ok,
    });
    assert_eq!(geometry_dashboard.view_revision, geometry_revision);
    assert!(geometry_dashboard.panes.iter().all(|pane| pane.ready));
}

#[test]
fn split_review_pending_a_b_a_preserves_received_parser_before_ok() {
    let mut dashboard = dashboard_fixture();
    let initial = dashboard
        .select_request(SessionId(3), 46)
        .expect("A selection should request a view");
    let Request::SetView { ref view } = initial.request else {
        panic!("expected SetView");
    };
    let target = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Screen {
            session: target.session,
            revision: view.revision,
            size: target.size,
            bytes: b"PENDING_A\x1b[?1h\x1b[?2004h".to_vec(),
        },
    });
    assert!(dashboard.select_request(SessionId(4), 47).is_none());
    assert!(dashboard.select_request(SessionId(3), 48).is_none());
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
    let mut outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        outgoing.len(),
        1,
        "stale parser must request exactly one replacement"
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(!dashboard.panes[dashboard.focused_pane].ready);
    assert!(dashboard.input_request(vec![b'x'], 52).is_none());
    let replacement = outgoing.pop().expect("replacement SetView");
    let Request::SetView { ref view } = replacement.request else {
        panic!("expected replacement SetView");
    };
    assert_eq!(view.panes.len(), 1);
    let replacement_target = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Screen {
            session: replacement_target.session,
            revision: view.revision,
            size: replacement_target.size,
            bytes: b"FRESH_A\x1b[?1h\x1b[?2004h".to_vec(),
        },
    });
    assert!(!dashboard.panes[dashboard.focused_pane].ready);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
    let pane = &dashboard.panes[dashboard.focused_pane];
    assert!(pane.ready);
    assert_eq!(pane.size, replacement_target.size);
    assert!(pane.parser.screen().contents().contains("FRESH_A"));
    assert_eq!(
        dashboard.key(KeyCode::Up),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1bOA".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("fresh".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[200~fresh\x1b[201~".to_vec())
    );
}

#[test]
fn split_review_discarded_pending_view_refreshes_before_late_receipts() {
    let mut dashboard = dashboard_fixture();
    let initial = dashboard
        .select_request(SessionId(3), 106)
        .expect("initial A selection should request a view");
    let Request::SetView { ref view } = initial.request else {
        panic!("expected SetView");
    };
    let initial_revision = view.revision;
    let initial_target = view.panes[0].clone();

    assert!(dashboard.select_request(SessionId(4), 107).is_none());
    assert!(dashboard.select_request(SessionId(3), 108).is_none());
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
    assert_eq!(dashboard.view_revision, initial_revision);

    // `Ok` is final, so an acknowledgement that arrives without the expected Screen has failed
    // the discarded A→B→A request: it is abandoned and replaced rather than left waiting.
    let mut outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        outgoing.len(),
        1,
        "an incomplete discarded request must emit exactly one replacement"
    );
    let replacement = outgoing.pop().expect("replacement SetView");
    let Request::SetView { ref view } = replacement.request else {
        panic!("expected replacement SetView");
    };
    assert_eq!(view.panes.len(), 1);
    assert_eq!(view.panes[0].session, SessionId(3));
    assert_eq!(dashboard.view_revision, initial_revision + 1);
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 109).is_none());
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    assert!(
        dashboard
            .view_request(Rect::new(0, 0, 88, 38), 110)
            .unwrap()
            .is_none()
    );
    assert_eq!(dashboard.view_revision, initial_revision + 1);

    // The abandoned request's late snapshot and duplicate ack install nothing.
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Screen {
            session: initial_target.session,
            revision: initial_revision,
            size: initial_target.size,
            bytes: b"ORIGINAL_A".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| !pane.parser.screen().contents().contains("ORIGINAL_A"))
    );
    assert_eq!(dashboard.view_revision, initial_revision + 1);
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 111).is_none());
    dashboard.mode = ovrcr::tui::InputMode::Browse;

    let replacement_target = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Screen {
            session: replacement_target.session,
            revision: view.revision,
            size: replacement_target.size,
            bytes: b"FRESH_A\x1b[?1h\x1b[?2004h".to_vec(),
        },
    });
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
    assert_eq!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents(),
        "FRESH_A"
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.event_action(Event::Paste("fresh".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[200~fresh\x1b[201~".to_vec())
    );
    assert!(dashboard.input_request(vec![b'x'], 112).is_some());
}

#[test]
fn split_review_absent_a_screen_cannot_authorize_reassigned_parser() {
    let mut dashboard = dashboard_fixture();
    let initial = dashboard
        .select_request(SessionId(3), 49)
        .expect("A selection should request a view");
    let Request::SetView { ref view } = initial.request else {
        panic!("expected SetView");
    };
    let target = view.panes[0].clone();
    assert!(dashboard.select_request(SessionId(4), 50).is_none());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Screen {
            session: target.session,
            revision: view.revision,
            size: target.size,
            bytes: b"ABSENT_A".to_vec(),
        },
    });
    assert!(dashboard.select_request(SessionId(3), 51).is_none());
    let mut outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        outgoing.len(),
        1,
        "absent A must request exactly one replacement"
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 52).is_none());
    let replacement = outgoing.pop().expect("replacement SetView");
    let Request::SetView { ref view } = replacement.request else {
        panic!("expected replacement SetView");
    };
    let replacement_target = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Screen {
            session: replacement_target.session,
            revision: view.revision,
            size: replacement_target.size,
            bytes: b"FRESH_ABSENT_A".to_vec(),
        },
    });
    assert!(!dashboard.panes[dashboard.focused_pane].ready);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes[dashboard.focused_pane].ready);
    assert!(
        dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("FRESH_ABSENT_A")
    );
}

#[test]
fn split_review_close_reopen_does_not_authorize_recreated_parser() {
    let mut dashboard = dashboard_fixture();
    let initial = dashboard
        .select_request(SessionId(4), 99)
        .expect("initial A selection should request a view");
    acknowledge_view_request(&mut dashboard, initial);
    dashboard.outer_area = Rect::new(0, 0, 120, 40);
    assert!(dashboard.split_pane());
    let initial = dashboard
        .select_request(SessionId(3), 100)
        .expect("split replacement should request snapshots");
    let Request::SetView { ref view } = initial.request else {
        panic!("expected SetView");
    };
    assert_eq!(view.panes.len(), 2);
    let left = view.panes[0].clone();
    let right = view.panes[1].clone();
    assert_eq!(right.session, SessionId(3));
    assert_eq!(left.size, TerminalSize { rows: 36, cols: 39 });
    assert_eq!(right.size, TerminalSize { rows: 36, cols: 40 });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Screen {
            session: left.session,
            revision: view.revision,
            size: left.size,
            bytes: b"A_CONTENT".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Screen {
            session: right.session,
            revision: view.revision,
            size: right.size,
            bytes: b"B_CONTENT\x1b[?1h\x1b[?2004h".to_vec(),
        },
    });

    assert!(dashboard.close_focused_pane());
    assert!(dashboard.split_pane());
    assert!(dashboard.select_request(SessionId(5), 102).is_none());
    assert!(dashboard.select_request(right.session, 103).is_none());
    assert_eq!(dashboard.focused_session(), Some(right.session));
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));

    let mut outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        outgoing.len(),
        1,
        "recreated pane must request one fresh SetView"
    );
    let replacement = outgoing.pop().expect("replacement SetView");
    let Request::SetView { ref view } = replacement.request else {
        panic!("expected replacement SetView");
    };
    assert_eq!(
        view.panes
            .iter()
            .map(|pane| (pane.session, pane.size))
            .collect::<Vec<_>>(),
        vec![
            (left.session, TerminalSize { rows: 36, cols: 39 }),
            (right.session, TerminalSize { rows: 36, cols: 40 }),
        ]
    );
    assert_eq!(view.focused, Some(right.session));
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("blocked".into())),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.input_request(vec![b'x'], 104).is_none());

    for pane in &view.panes {
        let bytes = if pane.session == right.session {
            b"FRESH_B\x1b[?1h\x1b[?2004h".to_vec()
        } else {
            b"FRESH_A".to_vec()
        };
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: replacement.request_id,
            response: Response::Screen {
                session: pane.session,
                revision: view.revision,
                size: pane.size,
                bytes,
            },
        });
    }
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    let right_pane = dashboard
        .panes
        .iter()
        .find(|pane| pane.session == Some(right.session))
        .expect("recreated B pane");
    assert_eq!(right_pane.size, TerminalSize { rows: 36, cols: 40 });
    assert!(right_pane.parser.screen().application_cursor());
    assert!(right_pane.parser.screen().bracketed_paste());
    assert!(right_pane.parser.screen().contents().contains("FRESH_B"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
    assert_eq!(
        dashboard.key(KeyCode::Up),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1bOA".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("fresh".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[200~fresh\x1b[201~".to_vec())
    );
    assert_eq!(
        dashboard.input_request(vec![b'x'], 105).unwrap().request,
        Request::Input {
            session: right.session,
            bytes: vec![b'x'],
        }
    );
}

#[test]
fn split_review_focus_select_and_close_revoke_public_input_until_setview_ok() {
    let mut dashboard = dashboard_fixture();
    let first = dashboard
        .select_request(SessionId(3), 47)
        .expect("first running session should request a view");
    acknowledge_view_request(&mut dashboard, first);
    assert!(dashboard.split_pane());
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    let second = dashboard
        .select_request(SessionId(4), 48)
        .expect("next distinct visible running session should replace the new pane");
    let Request::SetView { ref view } = second.request else {
        panic!("expected SetView");
    };
    assert_eq!(view.panes.len(), 2);
    assert!(view.panes.iter().any(|pane| pane.session == SessionId(3)));
    assert!(view.panes.iter().any(|pane| pane.session == SessionId(4)));
    let first_target = view.panes[0].clone();
    let second_target = view.panes[1].clone();
    assert_view_input_blocked(&mut dashboard, 49);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::Screen {
            session: first_target.session,
            revision: view.revision,
            size: first_target.size,
            bytes: Vec::new(),
        },
    });
    assert_view_input_blocked(&mut dashboard, 50);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::Screen {
            session: second_target.session,
            revision: view.revision,
            size: second_target.size,
            bytes: Vec::new(),
        },
    });
    assert_view_input_blocked(&mut dashboard, 51);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::Ok,
    });
    assert_eq!(dashboard.panes.len(), 2);
    assert_eq!(dashboard.panes[0].session, Some(SessionId(3)));
    assert_eq!(dashboard.panes[1].session, Some(SessionId(4)));
    assert_view_input_allowed(&mut dashboard, SessionId(4), 52);

    assert!(dashboard.focus_pane(0));
    assert_view_input_blocked(&mut dashboard, 53);
    let refocus = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 51)
        .unwrap()
        .expect("focus change should emit one SetView");
    let Request::SetView { ref view } = refocus.request else {
        panic!("expected SetView");
    };
    let refocus_targets = view.panes.clone();
    assert_view_input_blocked(&mut dashboard, 54);
    for pane in refocus_targets {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: refocus.request_id,
            response: Response::Screen {
                session: pane.session,
                revision: view.revision,
                size: pane.size,
                bytes: Vec::new(),
            },
        });
        assert_view_input_blocked(&mut dashboard, 55);
    }
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: refocus.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes[0].ready);
    assert_view_input_allowed(&mut dashboard, SessionId(3), 56);

    let select_other = dashboard
        .select_request(SessionId(4), 57)
        .expect("selecting the other displayed pane should focus it");
    let Request::SetView { ref view } = select_other.request else {
        panic!("expected SetView");
    };
    assert_eq!(view.focused, Some(SessionId(4)));
    assert_eq!(
        view.panes
            .iter()
            .map(|pane| pane.session)
            .collect::<Vec<_>>(),
        vec![SessionId(3), SessionId(4)]
    );
    assert_eq!(dashboard.panes.len(), 2);
    assert_eq!(dashboard.panes[0].session, Some(SessionId(3)));
    assert_eq!(dashboard.panes[1].session, Some(SessionId(4)));
    let select_targets = view.panes.clone();
    assert_view_input_blocked(&mut dashboard, 58);
    for pane in select_targets {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: select_other.request_id,
            response: Response::Screen {
                session: pane.session,
                revision: view.revision,
                size: pane.size,
                bytes: Vec::new(),
            },
        });
        assert_view_input_blocked(&mut dashboard, 59);
    }
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: select_other.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes[1].ready);
    assert_view_input_allowed(&mut dashboard, SessionId(4), 60);

    assert!(dashboard.close_focused_pane());
    assert_eq!(dashboard.panes.len(), 1);
    assert_eq!(dashboard.panes[0].session, Some(SessionId(3)));
    assert!(
        dashboard
            .hierarchy
            .projects
            .iter()
            .flat_map(|project| project.workspaces.iter())
            .flat_map(|workspace| workspace.sessions.iter())
            .any(|session| session.id == SessionId(4))
    );
    assert_view_input_blocked(&mut dashboard, 61);
    let close_view = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 55)
        .unwrap()
        .expect("close should emit only the replacement SetView");
    assert!(matches!(close_view.request, Request::SetView { .. }));
    let Request::SetView { ref view } = close_view.request else {
        panic!("expected SetView");
    };
    assert_view_input_blocked(&mut dashboard, 62);
    deliver_all_view_screens(&mut dashboard, close_view.request_id, view);
    assert_view_input_blocked(&mut dashboard, 63);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: close_view.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard, SessionId(3), 64);
}

#[test]
fn split_review_replaces_a_b_a_and_rejects_old_completions() {
    let mut dashboard = dashboard_fixture();
    let first_a = dashboard
        .select_request(SessionId(4), 50)
        .expect("A selection should request a view");
    let first_a_revision = dashboard.view_revision;
    acknowledge_view_request(&mut dashboard, first_a.clone());

    let b = dashboard
        .select_request(SessionId(3), 51)
        .expect("B selection should request a view");
    let b_revision = dashboard.view_revision;
    acknowledge_view_request(&mut dashboard, b.clone());

    let second_a = dashboard
        .select_request(SessionId(4), 52)
        .expect("returning to A should request a distinct view");
    assert_ne!(first_a.request_id, second_a.request_id);
    assert_ne!(first_a_revision, dashboard.view_revision);
    let current_revision = dashboard.view_revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first_a.request_id,
        response: Response::Screen {
            session: SessionId(4),
            revision: first_a_revision,
            size: TerminalSize { rows: 34, cols: 44 },
            bytes: b"old A".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: b.request_id,
        response: Response::Screen {
            session: SessionId(3),
            revision: b_revision,
            size: TerminalSize { rows: 34, cols: 44 },
            bytes: b"old B".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(4),
        revision: first_a_revision,
        bytes: b"stale output".to_vec(),
    }));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
        session: SessionId(4),
        revision: first_a_revision,
    }));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first_a.request_id,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "old A error".into(),
        },
    });
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| !pane.parser.screen().contents().contains("old A"))
    );
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| !pane.parser.screen().contents().contains("old B"))
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 53).is_none());
    dashboard.mode = ovrcr::tui::InputMode::Browse;
    let dirty_outgoing =
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session: SessionId(4),
            revision: first_a_revision,
        }));
    assert!(dirty_outgoing.is_empty());
    assert_eq!(dashboard.view_revision, current_revision);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first_a.request_id,
        response: Response::Ok,
    });
    assert_eq!(dashboard.view_revision, current_revision);
    // A completed request id is retired, so this second response for it is indistinguishable
    // from any other server error and shows the server's message; it still must not clear it.
    assert_eq!(dashboard.error.as_deref(), Some("Conflict: old A error"));
    assert!(
        dashboard
            .panes
            .iter()
            .all(|pane| !pane.parser.screen().contents().contains("stale"))
    );

    let size = dashboard.panes[dashboard.focused_pane].desired_size;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second_a.request_id,
        response: Response::Screen {
            session: SessionId(4),
            revision: current_revision,
            size,
            bytes: b"new A".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second_a.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes[dashboard.focused_pane].ready);
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 54).is_some());
}

#[test]
fn split_review_matching_view_error_disables_input_until_a_new_view_is_ready() {
    let mut dashboard = dashboard_fixture();
    let initial = dashboard
        .select_request(SessionId(2), 54)
        .expect("initial selection should request a view");
    acknowledge_view_request(&mut dashboard, initial);
    assert!(dashboard.split_pane());
    let next = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 55)
        .unwrap()
        .expect("replacement should request a two-target view");
    let Request::SetView { ref view } = next.request else {
        panic!("expected SetView");
    };
    assert_eq!(view.panes.len(), 2);
    let target = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: next.request_id,
        response: Response::Screen {
            session: target.session,
            revision: view.revision,
            size: target.size,
            bytes: b"partial".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: next.request_id,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "view refused".into(),
        },
    });
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    assert_eq!(dashboard.error.as_deref(), Some("Conflict: view refused"));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(dashboard.input_request(vec![b'x'], 56).is_none());
    // The refused view is retried, but only once its backoff has elapsed.
    assert!(
        dashboard
            .view_request(Rect::new(0, 0, 120, 40), 57)
            .unwrap()
            .is_none()
    );
    std::thread::sleep(PAST_VIEW_RETRY_BACKOFF);
    let retry = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 57)
        .unwrap()
        .expect("new view should retry after partial failure");
    acknowledge_all_view_targets(&mut dashboard, retry);
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
    assert!(dashboard.input_request(vec![b'x'], 58).is_some());
}

/// Slightly longer than `VIEW_RETRY_BACKOFF` in `crates/ovrcr-tui/src/dashboard/state.rs`.
const PAST_VIEW_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_millis(260);

#[test]
fn view_error_on_unchanged_view_retries_once_after_backoff() {
    let mut dashboard = dashboard_fixture();
    let session = dashboard.focused_session().unwrap();
    let mut dirty =
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session,
            revision: dashboard.view_revision,
        }));
    assert_eq!(dirty.len(), 1);
    let refresh = dirty
        .pop()
        .expect("a dirty screen should re-request the same view");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: refresh.request_id,
        response: Response::Error {
            code: ErrorCode::NotFound,
            message: "session vanished".into(),
        },
    });
    assert!(!dashboard.panes[dashboard.focused_pane].ready);
    assert_eq!(
        dashboard.error.as_deref(),
        Some("NotFound: session vanished")
    );
    assert_view_input_blocked(&mut dashboard, 601);
    for request_id in 602..605 {
        assert!(
            dashboard
                .view_request(dashboard.outer_area, request_id)
                .unwrap()
                .is_none(),
            "a refused view must wait out the backoff before it is re-sent"
        );
    }
    std::thread::sleep(PAST_VIEW_RETRY_BACKOFF);
    let retry = dashboard
        .view_request(dashboard.outer_area, 605)
        .unwrap()
        .expect("the refused view must be retried once the backoff elapses");
    assert!(matches!(retry.request, Request::SetView { .. }));
    acknowledge_view_request(&mut dashboard, retry);
    assert!(dashboard.panes[dashboard.focused_pane].ready);
    assert_view_input_allowed(&mut dashboard, session, 606);
}

#[test]
fn view_error_on_changed_view_does_not_spin() {
    let mut dashboard = dashboard_fixture();
    let wider = Rect::new(0, 0, 120, 40);
    let changed = dashboard
        .view_request(wider, 610)
        .unwrap()
        .expect("new geometry should request a view");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: changed.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "pty refused".into(),
        },
    });
    for request_id in 611..615 {
        assert!(
            dashboard.view_request(wider, request_id).unwrap().is_none(),
            "a changed view must not re-send SetView on every pass"
        );
    }
    std::thread::sleep(PAST_VIEW_RETRY_BACKOFF);
    let retry = dashboard
        .view_request(wider, 615)
        .unwrap()
        .expect("the refused view must be retried once the backoff elapses");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "pty refused".into(),
        },
    });
    assert!(
        dashboard.view_request(wider, 616).unwrap().is_none(),
        "a second refusal must arm a new backoff window"
    );
}

#[test]
fn incomplete_final_ok_marks_panes_failed_and_refreshes() {
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    let request = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 620)
        .unwrap()
        .expect("split should request a two-target view");
    let Request::SetView { view } = request.request.clone() else {
        panic!("expected SetView");
    };
    assert_eq!(view.panes.len(), 2);
    let first = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: first.session,
            revision: view.revision,
            size: first.size,
            bytes: b"only one snapshot".to_vec(),
        },
    });
    let mut outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes.iter().all(|pane| !pane.ready));
    assert!(dashboard.panes.iter().all(|pane| !pane.snapshot_installed));
    assert_view_input_blocked(&mut dashboard, 621);
    assert_eq!(
        outgoing.len(),
        1,
        "a final Ok without every snapshot must refresh the view"
    );
    let refresh = outgoing.pop().expect("refreshed view request");
    let Request::SetView { view: refreshed } = refresh.request.clone() else {
        panic!("expected a refreshed SetView");
    };
    assert_eq!(refreshed.revision, view.revision + 1);
    assert_eq!(refreshed.panes.len(), 2);
    deliver_all_view_screens(&mut dashboard, request.request_id, &view);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert!(
        dashboard.panes.iter().all(|pane| !pane.ready),
        "the abandoned request must not complete a second time"
    );
    acknowledge_all_view_targets(&mut dashboard, refresh);
    assert!(dashboard.panes.iter().all(|pane| pane.ready));
}

#[test]
fn view_request_ids_are_released_on_final_response() {
    let mut dashboard = dashboard_fixture();
    for round in 0..10 {
        let session = if round % 2 == 0 {
            SessionId(5)
        } else {
            SessionId(1)
        };
        let request = dashboard
            .select_request(session, 700 + round)
            .expect("alternating selection should request a view");
        acknowledge_view_request(&mut dashboard, request);
    }
    assert!(
        dashboard.view_request_ids.len() <= 1,
        "completed view requests must release their ids, kept {}",
        dashboard.view_request_ids.len()
    );
}

#[test]
fn settings_error_survives_the_geometry_ack() {
    let mut dashboard = dashboard_fixture();
    dashboard.error = Some("settings: invalid TOML at line 3".into());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 2,
        response: Response::Ok,
    });
    assert_eq!(
        dashboard.error.as_deref(),
        Some("settings: invalid TOML at line 3")
    );
}

#[test]
fn split_error_survives_mouse_cleanup_ack() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_terminal_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = focused_terminal_rect(&dashboard, area);
    dashboard.mouse_action(
        click_in(inner, MouseEventKind::Down(MouseButton::Left), 2, 3),
        area,
    );
    assert_eq!(
        dashboard.ctrl('g'),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    let cleanup = dashboard
        .take_mouse_cleanup()
        .expect("the held button should queue a release");
    for project in &mut dashboard.hierarchy.projects {
        for workspace in &mut project.workspaces {
            workspace
                .sessions
                .retain(|session| session.id == SessionId(1));
        }
    }
    assert!(!dashboard.split_pane());
    assert_eq!(
        dashboard.error.as_deref(),
        Some("No other visible session to split")
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: cleanup.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        dashboard.error.as_deref(),
        Some("No other visible session to split"),
        "a synthetic mouse release must not clear a fresh error"
    );
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    let input = dashboard
        .input_request(b"x".to_vec(), 630)
        .expect("a ready pane should accept input");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: input.request_id,
        response: Response::Ok,
    });
    assert!(
        dashboard.error.is_none(),
        "an acknowledged user input owns the banner and clears it"
    );
}

#[test]
fn split_review_terminal_modes_use_each_panes_parser() {
    let mut dashboard = dashboard_fixture();
    let initial = dashboard
        .select_request(SessionId(3), 57)
        .expect("initial selection should request a view");
    acknowledge_view_request(&mut dashboard, initial);
    assert!(dashboard.split_pane());
    let split = dashboard.select_request(SessionId(4), 58).unwrap();
    acknowledge_all_view_targets(&mut dashboard, split);
    let right = dashboard.focused_pane;
    dashboard.panes[right]
        .parser
        .process(b"\x1b[?1l\x1b[?2004h");
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Up),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[A".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("right".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[200~right\x1b[201~".to_vec())
    );
    let focused_before_literals = dashboard.focused_pane;
    assert_eq!(
        dashboard.key(KeyCode::Tab),
        ovrcr::tui::DashboardAction::PtyBytes(b"\t".to_vec())
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::PtyBytes(b"v".to_vec())
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(b"x".to_vec())
    );
    assert_eq!(dashboard.panes.len(), 2);
    assert_eq!(dashboard.focused_pane, focused_before_literals);

    assert!(dashboard.focus_pane(0));
    let left_view = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 59)
        .unwrap()
        .unwrap();
    acknowledge_all_view_targets(&mut dashboard, left_view);
    dashboard.panes[0].parser.process(b"\x1b[?1h");
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Up),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1bOA".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("left".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"left".to_vec())
    );
}

#[test]
fn split_review_keeps_history_a_while_b_changes_and_cancels_late_work() {
    let mut dashboard = dashboard_fixture();
    let select = dashboard
        .select_request(SessionId(2), 60)
        .expect("A selection should request a view");
    acknowledge_view_request(&mut dashboard, select);
    assert!(dashboard.split_pane());
    let split = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 61)
        .unwrap()
        .unwrap();
    acknowledge_all_view_targets(&mut dashboard, split);
    assert!(dashboard.focus_pane(0));
    let focus_a = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 62)
        .unwrap()
        .unwrap();
    acknowledge_all_view_targets(&mut dashboard, focus_a);
    let a = dashboard.focused_session().unwrap();
    let b = dashboard.panes[1].session.unwrap();
    let revision = dashboard.view_revision;
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request: Request::HistoryBegin { session }, ..
        }) if session == a
    ));
    let opened = HistoryOpened {
        session: a,
        snapshot: HistorySnapshotId(77),
        revision: 1,
        size: TerminalSize { rows: 34, cols: 39 },
        history_rows: 0,
        total_rows: 0,
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(opened.clone()),
    });
    let frozen_page = HistoryRows {
        session: a,
        snapshot: HistorySnapshotId(77),
        start_row: 0,
        start_col: 0,
        rows: vec![HistoryRow {
            width: 1,
            cells: vec![history_cell("A frozen", 1)],
            wrapped: false,
        }],
    };
    dashboard
        .history
        .as_mut()
        .unwrap()
        .pages
        .push_back(frozen_page);
    dashboard.history.as_mut().unwrap().anchor = Some(HistoryCopyPoint { row: 0, col: 0 });
    let frozen_pages = dashboard.history.as_ref().unwrap().pages.clone();
    let frozen_anchor = dashboard.history.as_ref().unwrap().anchor;
    assert_eq!(dashboard.history.as_ref().unwrap().opened.session, a);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: b,
        revision,
        bytes: b"B output".to_vec(),
    }));
    assert!(
        dashboard
            .panes
            .iter()
            .find(|pane| pane.session == Some(b))
            .unwrap()
            .parser
            .screen()
            .contents()
            .contains("B output")
    );
    assert_eq!(dashboard.history.as_ref().unwrap().opened, opened);
    assert_eq!(dashboard.history.as_ref().unwrap().pages, frozen_pages);
    assert_eq!(dashboard.history.as_ref().unwrap().anchor, frozen_anchor);

    assert_eq!(
        dashboard.key(KeyCode::Esc),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    let begin = dashboard.key(KeyCode::PageUp);
    let begin_request_id = match begin {
        ovrcr::tui::DashboardAction::Request(request) => request.request_id,
        action => panic!("expected second history begin, got {action:?}"),
    };
    assert!(dashboard.focus_pane(1));
    assert!(dashboard.history.is_none());
    let late = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin_request_id,
        response: Response::HistoryOpened(opened),
    });
    assert!(dashboard.history.is_none());
    assert!(late.iter().any(|request| matches!(
        request.request,
        Request::HistoryEnd {
            session: SessionId(2),
            ..
        }
    )));

    let focused_view = dashboard
        .view_request(dashboard.outer_area, 3000)
        .unwrap()
        .unwrap();
    acknowledge_all_view_targets(&mut dashboard, focused_view);
    let begin_b = match dashboard.key(KeyCode::PageUp) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected B history begin, got {action:?}"),
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin_b.request_id,
        response: Response::HistoryOpened(HistoryOpened {
            session: b,
            snapshot: HistorySnapshotId(78),
            revision: 1,
            size: TerminalSize { rows: 34, cols: 39 },
            history_rows: 0,
            total_rows: 0,
        }),
    });
    dashboard.history.as_mut().unwrap().copy_completion = Some(HistoryCopyCompletion {
        id: 1,
        range: HistoryCopyRange {
            session: b,
            snapshot: HistorySnapshotId(78),
            anchor: HistoryCopyPoint { row: 0, col: 0 },
            cursor: HistoryCopyPoint { row: 0, col: 0 },
        },
        text: "ready clipboard".into(),
    });
    assert!(dashboard.focus_pane(0));
    assert!(dashboard.history.is_none());
}

#[test]
fn split_review_cancels_inflight_history_copy_before_focus_change() {
    let mut dashboard = dashboard_fixture();
    let first = dashboard
        .select_request(SessionId(3), 70)
        .expect("first running session should request a view");
    acknowledge_view_request(&mut dashboard, first);
    assert!(dashboard.split_pane());
    let split = dashboard
        .select_request(SessionId(4), 71)
        .expect("second running session should request a view");
    acknowledge_all_view_targets(&mut dashboard, split);
    let session = dashboard.focused_session().unwrap();
    let begin = match dashboard.key(KeyCode::PageUp) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected history begin request, got {action:?}"),
    };
    let open_follow_up = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(HistoryOpened {
            session,
            snapshot: HistorySnapshotId(79),
            revision: 1,
            size: TerminalSize { rows: 34, cols: 40 },
            history_rows: 0,
            total_rows: 1,
        }),
    });
    let viewport = open_follow_up
        .into_iter()
        .find(|request| matches!(request.request, Request::HistoryPage { .. }))
        .expect("opened history should request its first page");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: viewport.request_id,
        response: Response::HistoryRows(HistoryRows {
            session,
            snapshot: HistorySnapshotId(79),
            start_row: 0,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 4,
                cells: vec![
                    history_cell("B", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                ],
                wrapped: false,
            }],
        }),
    });
    assert_eq!(
        dashboard.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let copy_request = match dashboard.key(KeyCode::Char('y')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected history copy page request, got {action:?}"),
    };
    assert!(
        dashboard
            .history
            .as_ref()
            .is_some_and(|view| view.copy_job.is_some())
    );
    assert!(dashboard.focus_pane(0));
    assert!(dashboard.history.is_none());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: copy_request.request_id,
        response: Response::HistoryRows(HistoryRows {
            session,
            snapshot: HistorySnapshotId(79),
            start_row: 0,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 4,
                cells: vec![
                    history_cell("late clipboard", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                    history_cell("", 1),
                ],
                wrapped: false,
            }],
        }),
    });
    let mut clipboard = Vec::new();
    assert!(!write_completed_history_copy(
        &mut dashboard,
        &mut clipboard
    ));
    assert!(clipboard.is_empty());
}

#[test]
fn split_review_unfocused_removal_preserves_focused_history_capture() {
    let mut dashboard = dashboard_fixture();
    let first = dashboard
        .select_request(SessionId(3), 72)
        .expect("first running session should request a view");
    acknowledge_view_request(&mut dashboard, first);
    assert!(dashboard.split_pane());
    let split = dashboard
        .select_request(SessionId(4), 73)
        .expect("second running session should request a view");
    acknowledge_all_view_targets(&mut dashboard, split);
    assert!(dashboard.focus_pane(0));
    let focused = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 74)
        .unwrap()
        .expect("focus change should request a view");
    acknowledge_all_view_targets(&mut dashboard, focused);
    let session = dashboard.focused_session().unwrap();
    let begin = match dashboard.key(KeyCode::PageUp) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected history begin request, got {action:?}"),
    };
    let viewport_requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(HistoryOpened {
            session,
            snapshot: HistorySnapshotId(80),
            revision: 1,
            size: TerminalSize { rows: 34, cols: 39 },
            history_rows: 0,
            total_rows: 1,
        }),
    });
    let viewport = viewport_requests
        .into_iter()
        .find(|request| matches!(request.request, Request::HistoryPage { .. }))
        .expect("HistoryOpened should request its first page");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: viewport.request_id,
        response: Response::HistoryRows(HistoryRows {
            session,
            snapshot: HistorySnapshotId(80),
            start_row: 0,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 39,
                cells: (0..39)
                    .map(|index| {
                        if index == 0 {
                            history_cell("frozen", 1)
                        } else {
                            history_cell("", 1)
                        }
                    })
                    .collect(),
                wrapped: false,
            }],
        }),
    });
    assert_eq!(
        dashboard.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let copy_request = match dashboard.key(KeyCode::Char('y')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("expected in-flight History copy request, got {action:?}"),
    };
    assert!(
        dashboard
            .history
            .as_ref()
            .is_some_and(|view| view.copy_job.is_some())
    );
    let before_pages = dashboard.history.as_ref().unwrap().pages.clone();
    let before_anchor = dashboard.history.as_ref().unwrap().anchor;
    let opened_identity = dashboard.history.as_ref().unwrap().opened.clone();
    let removed = dashboard.panes[1].session.unwrap();
    let mut hierarchy = dashboard.hierarchy.clone();
    for project in &mut hierarchy.projects {
        for workspace in &mut project.workspaces {
            workspace
                .sessions
                .retain(|candidate| candidate.id != removed);
        }
    }
    let outgoing = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::HierarchyChanged(hierarchy),
    ));
    assert!(
        outgoing
            .iter()
            .any(|request| matches!(request.request, Request::SetView { .. }))
    );
    assert_eq!(dashboard.focused_session(), Some(session));
    assert_eq!(dashboard.history.as_ref().unwrap().opened, opened_identity);
    assert_eq!(dashboard.history.as_ref().unwrap().pages, before_pages);
    assert_eq!(dashboard.history.as_ref().unwrap().anchor, before_anchor);
    let copy_page = HistoryRows {
        session,
        snapshot: HistorySnapshotId(80),
        start_row: 0,
        start_col: 0,
        rows: vec![HistoryRow {
            width: 128,
            cells: (0..128)
                .map(|index| {
                    if index == 0 {
                        history_cell("copied", 1)
                    } else {
                        history_cell("", 1)
                    }
                })
                .collect(),
            wrapped: false,
        }],
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: copy_request.request_id,
        response: Response::HistoryRows(copy_page),
    });
    let mut clipboard = Vec::new();
    assert!(write_completed_history_copy(&mut dashboard, &mut clipboard));
    let mut expected_clipboard = Vec::new();
    write_clipboard(&mut expected_clipboard, "copied").unwrap();
    assert_eq!(clipboard, expected_clipboard);
}

#[test]
fn split_state_ignores_stale_revisions_and_removed_sessions() {
    let mut dashboard = dashboard_fixture();
    assert!(dashboard.split_pane());
    let right = dashboard.focused_session().unwrap();
    let first = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 20)
        .unwrap()
        .unwrap();
    let revision = dashboard.view_revision;
    dashboard.apply_screen(
        revision,
        right,
        TerminalSize { rows: 36, cols: 40 },
        b"first",
    );
    dashboard.select_session(SessionId(1));
    dashboard.select_session(right);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: right,
        revision,
        bytes: b"stale".to_vec(),
    }));
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("stale")
    );
    assert!(first.request_id > 0);
}

#[test]
fn split_preserves_history_copy_and_paused_input() {
    let mut dashboard = dashboard_fixture();
    let first = dashboard.focused_session().unwrap();
    dashboard.select_request(first, 900);
    let size = dashboard.panes[dashboard.focused_pane].desired_size;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Screen {
            session: first,
            revision: dashboard.view_revision,
            size,
            bytes: b"A".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Ok,
    });
    assert_eq!(
        dashboard.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.copy.is_some());
    assert!(dashboard.split_pane());
    assert!(dashboard.copy.is_none());
    let right = dashboard.focused_session().unwrap();
    let request = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 901)
        .unwrap()
        .unwrap();
    let revision = dashboard.view_revision;
    dashboard.apply_screen(revision, first, TerminalSize { rows: 36, cols: 39 }, b"A");
    dashboard.apply_screen(revision, right, TerminalSize { rows: 36, cols: 40 }, b"B");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: right,
        revision,
        bytes: b"B output".to_vec(),
    }));
    let mut paused = dashboard
        .hierarchy
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .find(|session| session.id == right)
        .unwrap()
        .clone();
    paused.phase = SessionPhase::Paused;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        paused,
    ))));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.event_action(Event::Paste("blocked".into())),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.error.as_deref(),
        Some("Session paused; press r to resume")
    );
}

fn mouse_event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers,
    }
}

#[test]
fn mouse_encode_sgr_events() {
    let event = mouse_event(
        MouseEventKind::Down(MouseButton::Left),
        2,
        3,
        KeyModifiers::SHIFT | KeyModifiers::CONTROL,
    );
    assert_eq!(
        encode_mouse(
            event,
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<20;3;4M".to_vec())
    );
    let up = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        ..event
    };
    assert_eq!(
        encode_mouse(
            up,
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<20;3;4m".to_vec())
    );
    assert_eq!(
        encode_mouse(
            up,
            vt100::MouseProtocolMode::Press,
            vt100::MouseProtocolEncoding::Default,
        ),
        None
    );
    let at = |kind| mouse_event(kind, 2, 3, KeyModifiers::NONE);
    assert_eq!(
        encode_mouse(
            at(MouseEventKind::Down(MouseButton::Middle)),
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<1;3;4M".to_vec())
    );
    assert_eq!(
        encode_mouse(
            at(MouseEventKind::Down(MouseButton::Right)),
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<2;3;4M".to_vec())
    );
    assert_eq!(
        encode_mouse(
            at(MouseEventKind::ScrollUp),
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<64;3;4M".to_vec())
    );
    assert_eq!(
        encode_mouse(
            at(MouseEventKind::ScrollDown),
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<65;3;4M".to_vec())
    );
    assert_eq!(
        encode_mouse(
            at(MouseEventKind::ScrollLeft),
            vt100::MouseProtocolMode::ButtonMotion,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<66;3;4M".to_vec())
    );
    assert_eq!(
        encode_mouse(
            at(MouseEventKind::ScrollRight),
            vt100::MouseProtocolMode::AnyMotion,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<67;3;4M".to_vec())
    );
    assert_eq!(
        encode_mouse(
            at(MouseEventKind::Drag(MouseButton::Left)),
            vt100::MouseProtocolMode::ButtonMotion,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<32;3;4M".to_vec())
    );
    assert_eq!(
        encode_mouse(
            at(MouseEventKind::Moved),
            vt100::MouseProtocolMode::AnyMotion,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<35;3;4M".to_vec())
    );
    let far = mouse_event(
        MouseEventKind::Down(MouseButton::Left),
        300,
        400,
        KeyModifiers::NONE,
    );
    assert_eq!(
        encode_mouse(
            far,
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<0;301;401M".to_vec())
    );
    let shifted_wheel = mouse_event(
        MouseEventKind::ScrollUp,
        2,
        3,
        KeyModifiers::SHIFT | KeyModifiers::ALT | KeyModifiers::CONTROL,
    );
    assert_eq!(
        encode_mouse(
            shifted_wheel,
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<92;3;4M".to_vec())
    );
}

#[test]
fn mouse_encode_tracking_filters() {
    let down = mouse_event(
        MouseEventKind::Down(MouseButton::Left),
        2,
        3,
        KeyModifiers::SHIFT,
    );
    assert_eq!(
        encode_mouse(
            down,
            vt100::MouseProtocolMode::None,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        None
    );
    assert_eq!(
        encode_mouse(
            down,
            vt100::MouseProtocolMode::Press,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<0;3;4M".to_vec())
    );
    for kind in [
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Moved,
        MouseEventKind::ScrollUp,
        MouseEventKind::ScrollDown,
        MouseEventKind::ScrollLeft,
        MouseEventKind::ScrollRight,
    ] {
        assert_eq!(
            encode_mouse(
                mouse_event(kind, 2, 3, KeyModifiers::NONE),
                vt100::MouseProtocolMode::Press,
                vt100::MouseProtocolEncoding::Sgr,
            ),
            None,
            "{kind:?}"
        );
    }
    let drag = mouse_event(
        MouseEventKind::Drag(MouseButton::Left),
        2,
        3,
        KeyModifiers::NONE,
    );
    let moved = mouse_event(MouseEventKind::Moved, 2, 3, KeyModifiers::NONE);
    assert_eq!(
        encode_mouse(
            drag,
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        None
    );
    assert_eq!(
        encode_mouse(
            moved,
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        None
    );
    assert_eq!(
        encode_mouse(
            moved,
            vt100::MouseProtocolMode::ButtonMotion,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        None
    );
    assert_eq!(
        encode_mouse(
            drag,
            vt100::MouseProtocolMode::ButtonMotion,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(b"\x1b[<32;3;4M".to_vec())
    );
}

#[test]
fn mouse_encode_legacy_boundaries() {
    let at = |column, row| {
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            KeyModifiers::NONE,
        )
    };
    let origin = encode_mouse(
        at(0, 0),
        vt100::MouseProtocolMode::Press,
        vt100::MouseProtocolEncoding::Default,
    )
    .unwrap();
    assert_eq!(origin, b"\x1b[M !!");
    assert_eq!(
        encode_mouse(
            at(222, 222),
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Default,
        ),
        Some(b"\x1b[M \xff\xff".to_vec())
    );
    assert_eq!(
        encode_mouse(
            at(223, 0),
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Default,
        ),
        None
    );
    assert_eq!(
        encode_mouse(
            at(0, 223),
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Default,
        ),
        None
    );
    let utf8_ok = encode_mouse(
        at(2014, 0),
        vt100::MouseProtocolMode::PressRelease,
        vt100::MouseProtocolEncoding::Utf8,
    )
    .unwrap();
    let mut expected = b"\x1b[M ".to_vec();
    expected.extend("\u{07ff}".as_bytes());
    expected.extend("!".as_bytes());
    assert_eq!(utf8_ok, expected);
    assert_eq!(
        encode_mouse(
            at(2015, 0),
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Utf8,
        ),
        None
    );
    assert_eq!(
        encode_mouse(
            at(u16::MAX, 0),
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Sgr,
        ),
        Some(format!("\x1b[<0;{};1M", u32::from(u16::MAX) + 1).into_bytes())
    );
    let release = mouse_event(
        MouseEventKind::Up(MouseButton::Left),
        0,
        0,
        KeyModifiers::CONTROL,
    );
    assert_eq!(
        encode_mouse(
            release,
            vt100::MouseProtocolMode::PressRelease,
            vt100::MouseProtocolEncoding::Default,
        ),
        Some(b"\x1b[M3!!".to_vec())
    );
}

fn focused_terminal_rect(dashboard: &Dashboard, area: Rect) -> Rect {
    pane_rects(area, dashboard.panes.len(), dashboard.focused_pane)
        .into_iter()
        .find(|pane| pane.pane_index == dashboard.focused_pane)
        .expect("focused pane rect")
        .terminal
}

fn enable_terminal_mouse(dashboard: &mut Dashboard, modes: &[u8]) {
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    dashboard.panes[dashboard.focused_pane]
        .parser
        .process(modes);
}

fn click_in(inner: Rect, kind: MouseEventKind, dx: u16, dy: u16) -> MouseEvent {
    mouse_event(kind, inner.x + dx, inner.y + dy, KeyModifiers::NONE)
}

#[test]
fn browse_click_in_pane_focuses_without_bytes() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let inner = focused_terminal_rect(&dashboard, area);
    let action = dashboard.mouse_action(
        click_in(inner, MouseEventKind::Down(MouseButton::Left), 2, 3),
        area,
    );
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert!(dashboard.take_mouse_cleanup().is_none());
}

#[test]
fn press_outside_rectangle_is_not_clamped() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_terminal_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = focused_terminal_rect(&dashboard, area);
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                inner.x.saturating_sub(1),
                inner.y,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                inner.x,
                inner.y.saturating_sub(1),
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::None
    );
}

#[test]
fn leaving_the_rectangle_releases_at_last_valid_cell() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_terminal_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = focused_terminal_rect(&dashboard, area);
    assert_eq!(
        dashboard.mouse_action(
            click_in(inner, MouseEventKind::Down(MouseButton::Left), 2, 3),
            area,
        ),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<0;3;4M".to_vec())
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                inner.x.saturating_sub(1),
                inner.y + 3,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<0;3;4m".to_vec())
    );
}

#[test]
fn ctrl_g_releases_held_buttons_before_switching() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_terminal_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = focused_terminal_rect(&dashboard, area);
    dashboard.mouse_action(
        click_in(inner, MouseEventKind::Down(MouseButton::Left), 2, 3),
        area,
    );
    assert_eq!(
        dashboard.ctrl('g'),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    let cleanup = dashboard.take_mouse_cleanup().expect("held release");
    assert!(matches!(
        cleanup.request,
        Request::Input { session, bytes }
            if session == SessionId(1) && bytes == b"\x1b[<0;3;4m"
    ));
    assert!(dashboard.take_mouse_cleanup().is_none());
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
}

#[test]
fn protocol_change_clears_held_state() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_terminal_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = focused_terminal_rect(&dashboard, area);
    dashboard.mouse_action(
        click_in(inner, MouseEventKind::Down(MouseButton::Left), 2, 3),
        area,
    );
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        revision: dashboard.view_revision,
        bytes: b"\x1b[?1002l".to_vec(),
    }));
    assert!(dashboard.take_mouse_cleanup().is_none());
    assert_eq!(
        dashboard.mouse_action(
            click_in(inner, MouseEventKind::Up(MouseButton::Left), 2, 3),
            area,
        ),
        ovrcr::tui::DashboardAction::None
    );
}

#[test]
fn wheel_over_a_pane_without_tracking_opens_history_at_the_tail() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let inner = focused_terminal_rect(&dashboard, area);
    let action = dashboard.mouse_action(click_in(inner, MouseEventKind::ScrollUp, 2, 3), area);
    let ovrcr::tui::DashboardAction::Request(begin) = action else {
        panic!("expected history begin, got {action:?}");
    };
    assert_eq!(
        begin.request,
        Request::HistoryBegin {
            session: SessionId(1)
        }
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(history_opened(80)),
    });
    let view = dashboard.history.as_ref().expect("history open");
    let size = ovrcr::tui::history_view_size(dashboard.panes[dashboard.focused_pane].size);
    let max_top = view.opened.total_rows.saturating_sub(u32::from(size.rows));
    assert_eq!(view.top, max_top);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);
}

#[test]
fn wheel_down_at_newest_row_returns_to_live() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let inner = focused_terminal_rect(&dashboard, area);
    let ovrcr::tui::DashboardAction::Request(begin) =
        dashboard.mouse_action(click_in(inner, MouseEventKind::ScrollUp, 2, 3), area)
    else {
        panic!("expected history begin");
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(history_opened(80)),
    });
    let action = dashboard.mouse_action(click_in(inner, MouseEventKind::ScrollDown, 2, 3), area);
    assert!(
        matches!(
            action,
            ovrcr::tui::DashboardAction::EnterBrowse
                | ovrcr::tui::DashboardAction::Request(_)
                | ovrcr::tui::DashboardAction::RequestBatch(_)
        ),
        "{action:?}"
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(dashboard.history.is_none());
}

#[test]
fn wheel_is_forwarded_when_tracking_is_enabled() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_terminal_mouse(&mut dashboard, b"\x1b[?1000h\x1b[?1006h");
    let inner = focused_terminal_rect(&dashboard, area);
    assert_eq!(
        dashboard.mouse_action(click_in(inner, MouseEventKind::ScrollUp, 2, 3), area),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<64;3;4M".to_vec())
    );
}

#[test]
fn wheel_up_over_the_unfocused_pane_opens_its_history() {
    let mut dashboard = dashboard_fixture();
    let area = dashboard.outer_area;
    assert!(dashboard.split_pane());
    let split_view = dashboard
        .view_request(area, 70)
        .unwrap()
        .expect("split should request both snapshots");
    acknowledge_all_view_targets(&mut dashboard, split_view);
    assert!(dashboard.focus_pane(0));
    let refocus = dashboard
        .view_request(area, 71)
        .unwrap()
        .expect("focus change should request a view");
    acknowledge_all_view_targets(&mut dashboard, refocus);
    assert_eq!(dashboard.focused_pane, 0);
    assert!(dashboard.panes.iter().all(|pane| pane.ready));

    let unfocused = pane_rects(area, dashboard.panes.len(), dashboard.focused_pane)
        .into_iter()
        .find(|pane| pane.pane_index == 1)
        .expect("unfocused pane rect")
        .terminal;
    let session_b = dashboard.panes[1].session.expect("unfocused pane session");
    let action = dashboard.mouse_action(click_in(unfocused, MouseEventKind::ScrollUp, 2, 3), area);
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert_eq!(dashboard.focused_pane, 1);
    assert_eq!(
        dashboard.error.as_deref(),
        None,
        "a wheel over the other pane must not report a loading refusal"
    );
    assert!(dashboard.history.is_none());

    let replacement = dashboard
        .view_request(area, 72)
        .unwrap()
        .expect("the wheel focus change should emit one SetView");
    let Request::SetView { ref view } = replacement.request else {
        panic!("expected SetView");
    };
    deliver_all_view_screens(&mut dashboard, replacement.request_id, view);
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
    assert!(dashboard.panes[1].ready);
    assert!(
        outgoing.iter().any(|message| matches!(
            message.request,
            Request::HistoryBegin { session } if session == session_b
        )),
        "the deferred history open should follow the acknowledged view, got {outgoing:?}"
    );
}

#[test]
fn pause_cancels_a_held_mouse_gesture() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_terminal_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = focused_terminal_rect(&dashboard, area);
    assert_eq!(
        dashboard.mouse_action(
            click_in(inner, MouseEventKind::Down(MouseButton::Left), 2, 3),
            area,
        ),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<0;3;4M".to_vec())
    );
    let mut paused = dashboard
        .hierarchy
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .find(|session| session.id == SessionId(1))
        .expect("focused session")
        .clone();
    paused.phase = SessionPhase::Paused;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        paused,
    ))));
    let cleanup = dashboard
        .take_mouse_cleanup()
        .expect("pausing the focused session should release the held button");
    assert!(
        matches!(
            cleanup.request,
            Request::Input { session, ref bytes }
                if session == SessionId(1) && bytes == b"\x1b[<0;3;4m"
        ),
        "{:?}",
        cleanup.request
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert_eq!(
        dashboard.mouse_action(
            click_in(inner, MouseEventKind::Up(MouseButton::Left), 2, 3),
            area,
        ),
        ovrcr::tui::DashboardAction::None
    );
    assert!(dashboard.take_mouse_cleanup().is_none());
}

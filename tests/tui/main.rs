#[path = "../support/deadline.rs"]
mod deadline;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use deadline::wait_deadline;
use ovrcr::context::{ContextSource, ContextUsageReport, ContextUsageSnapshot};
use ovrcr::protocol::{
    ClientMessage, ErrorCode, HierarchySnapshot, HistoryCell, HistoryColor, HistoryOpened,
    HistoryRow, HistoryRows, HistorySnapshotId, ProjectSummary, Request, Response, ServerEvent,
    ServerMessage, WorkspaceSummary,
};
use ovrcr::session::{AgentActivity, SessionId, SessionPhase, SessionSummary, TerminalSize};
use ovrcr::tui::{
    DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, DashboardAction, KeyEncoding,
    dashboard_message_channel, draw_dashboard_at, encode_key, encode_mouse, encode_paste,
    pane_rects,
};
use ovrcr_terminal::vt100;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::path::PathBuf;
use std::sync::mpsc::TrySendError;

mod copy_history;
mod mouse;
mod palette;
mod sidebar;
mod split;
mod unread;

fn session_summary(
    id: u64,
    project: &str,
    workspace: &str,
    name: &str,
    label: &str,
    pid: Option<u32>,
    started_unix_ms: u64,
) -> SessionSummary {
    SessionSummary {
        archived: false,
        cwd: "/work".into(),
        id: SessionId(id),
        run: ovrcr::protocol::SessionRunId(1),
        kind: ovrcr::protocol::SessionKind::Terminal,
        recovery: None,
        project: project.into(),
        workspace: workspace.into(),
        name: name.into(),
        title: None,
        label: label.into(),
        pid,
        started_unix_ms: Some(started_unix_ms),
        phase: SessionPhase::Running,
        activity: AgentActivity::Unknown,
        context_usage: None,
        agent: None,
        agent_epoch: 0,
        unread: None,
    }
}

fn fixture_hierarchy() -> HierarchySnapshot {
    let mut hierarchy = HierarchySnapshot {
        projects: vec![
            ProjectSummary {
                name: "spacelift-agent".into(),
                workspaces: vec![WorkspaceSummary {
                    project: "spacelift-agent".into(),
                    name: "progress".into(),
                    path: PathBuf::from("/tmp/progress"),
                    sessions: vec![session_summary(
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
                            session_summary(
                                2,
                                "consigint",
                                "lifecycle",
                                "implement",
                                "claude",
                                Some(222),
                                0,
                            ),
                            session_summary(
                                4,
                                "consigint",
                                "lifecycle",
                                "local",
                                "zsh",
                                Some(444),
                                0,
                            ),
                        ],
                    },
                    WorkspaceSummary {
                        project: "consigint".into(),
                        name: "auth".into(),
                        path: PathBuf::from("/tmp/auth"),
                        sessions: vec![
                            session_summary(
                                1,
                                "consigint",
                                "auth",
                                "review",
                                "claude",
                                Some(111),
                                u64::MAX,
                            ),
                            session_summary(5, "consigint", "auth", "local", "zsh", Some(555), 0),
                        ],
                    },
                ],
            },
        ],
    };
    hierarchy.projects[1].workspaces[0].sessions[0].phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    hierarchy.projects[1].workspaces[0].sessions[0].pid = None;
    hierarchy
}

fn dashboard_fixture() -> Dashboard {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    dashboard.install_area(Rect::new(0, 0, 88, 38));
    dashboard.install_hierarchy(fixture_hierarchy());
    dashboard.install_focus(SessionId(1));
    dashboard.install_screen(SessionId(1), &[]);
    dashboard
}

fn pump_view(dashboard: &mut Dashboard, action: DashboardAction) {
    match action {
        DashboardAction::Request(request) => {
            if matches!(request.request, Request::SetView { .. }) {
                acknowledge_all_view_targets(dashboard, request);
            }
        }
        DashboardAction::RequestBatch(requests) => {
            for request in requests {
                if matches!(request.request, Request::SetView { .. }) {
                    acknowledge_all_view_targets(dashboard, request);
                }
            }
        }
        _ => {}
    }
}

fn acknowledge_view_request(dashboard: &mut Dashboard, request: ovrcr::protocol::ClientMessage) {
    acknowledge_all_view_targets(dashboard, request);
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

fn assert_view_input_blocked(dashboard: &mut Dashboard) {
    let footer = rendered_footer(dashboard, 120);
    if !footer.contains("ERROR:") {
        let _ = dashboard.ctrl('g');
        assert_eq!(
            dashboard.key(KeyCode::Enter),
            DashboardAction::Redraw,
            "blocked view must refuse terminal entry"
        );
        let footer = rendered_footer(dashboard, 120);
        assert!(
            footer.contains("ERROR:"),
            "refusal must show a banner, got {footer:?}"
        );
    }
    dashboard.install_terminal_input();
    assert!(
        dashboard.input_request(b"x".to_vec(), 9000).is_none(),
        "blocked view must not mint an Input request"
    );
}

fn assert_view_input_allowed(dashboard: &mut Dashboard) {
    let _ = dashboard.ctrl('g');
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        DashboardAction::Redraw,
        "ready view must enter the terminal"
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("paste".into())),
        DashboardAction::PtyBytes(ovrcr::tui::encode_paste(
            "paste",
            dashboard.focused_bracketed_paste()
        ))
    );
    assert_eq!(dashboard.ctrl('g'), DashboardAction::EnterBrowse);
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
                run: ovrcr_protocol::SessionRunId(1),
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

/// Polls the deferred submit until the palette stops waiting on Git, refusing
/// any request along the way, and returns the rendered palette.
fn poll_until_deferred_submit_refused(dashboard: &mut Dashboard, why: &str) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        dashboard.poll_palette();
        assert!(dashboard.drain_outbox().is_empty(), "{why}");
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

/// The fixture after the selected session's screen has arrived, so live
/// output is applied to the pane.
fn screen_ready_dashboard(bytes: &[u8]) -> Dashboard {
    let mut dashboard = dashboard_fixture();
    let id = dashboard.focused_session().unwrap();
    dashboard.install_screen(id, bytes);
    dashboard
}

fn copy_ready_dashboard() -> Dashboard {
    screen_ready_dashboard(b"abc")
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
    let footer = rendered_footer(&dashboard, 88);
    assert!(
        footer.contains("NotFound: session 99 not found"),
        "{footer}"
    );
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
fn runtime_paste_event_becomes_selected_session_input() {
    let mut dashboard = dashboard_fixture();
    let session = dashboard.focused_session().unwrap();
    dashboard.install_screen(session, b"\x1b[?2004h");
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.event_action(Event::Paste("hello\n".into())),
        DashboardAction::PtyBytes(b"\x1b[200~hello\n\x1b[201~".to_vec())
    );
}

#[test]
fn dashboard_reader_channel_applies_bounded_backpressure() {
    let (sender, receiver) = dashboard_message_channel();
    for _ in 0..DASHBOARD_READER_QUEUE_CAPACITY {
        sender
            .send(ServerMessage::Event(ServerEvent::ScreenDirty {
                session: SessionId(1),
                run: ovrcr_protocol::SessionRunId(1),
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
fn output_for_newly_selected_session_waits_for_screen() {
    let mut dashboard = copy_ready_dashboard();
    let text = rendered_rows(&dashboard, 88, 38).join("\n");
    assert!(text.contains("abc"), "{text}");
    dashboard.install_focus(SessionId(5));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(5),
        run: ovrcr_protocol::SessionRunId(1),
        revision: 0,
        bytes: b"EARLY OUTPUT".to_vec(),
    }));
    let text = rendered_rows(&dashboard, 88, 38).join("\n");
    assert!(
        !text.contains("EARLY OUTPUT"),
        "output applied before the screen arrived: {text}"
    );
    assert!(!text.contains("abc"), "stale pane kept: {text}");
    dashboard.install_screen(SessionId(5), b"FIVE SCREEN");
    let text = rendered_rows(&dashboard, 88, 38).join("\n");
    assert!(text.contains("FIVE SCREEN"), "{text}");
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(5),
        run: ovrcr_protocol::SessionRunId(1),
        revision: 0,
        bytes: b" LIVE".to_vec(),
    }));
    let text = rendered_rows(&dashboard, 88, 38).join("\n");
    assert!(
        text.contains("FIVE SCREEN LIVE"),
        "live output missing after screen: {text}"
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
fn inactive_to_live_requests_a_current_run_view_without_old_output() {
    let mut dashboard = screen_ready_dashboard(b"OLD_OUTPUT");
    let mut stopped = fixture_hierarchy();
    let session = stopped
        .projects
        .iter_mut()
        .flat_map(|project| project.workspaces.iter_mut())
        .flat_map(|workspace| workspace.sessions.iter_mut())
        .find(|session| session.id == SessionId(1))
        .unwrap();
    session.phase = SessionPhase::Stopped;
    session.pid = None;
    session.title = Some("Pinned".into());
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(stopped)));
    let mut live = fixture_hierarchy();
    let session = live
        .projects
        .iter_mut()
        .flat_map(|project| project.workspaces.iter_mut())
        .flat_map(|workspace| workspace.sessions.iter_mut())
        .find(|session| session.id == SessionId(1))
        .unwrap();
    session.phase = SessionPhase::Running;
    session.title = Some("Pinned".into());
    let outgoing =
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(live)));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let text = rendered_rows(&dashboard, 88, 38).join("\n");
    assert!(
        !text.contains("OLD_OUTPUT"),
        "inactive-to-live must not keep the previous screen: {text}"
    );
    assert!(
        text.contains("Pinned"),
        "same-row reopen must keep the title: {text}"
    );
    let set_view = outgoing
        .iter()
        .find(|message| matches!(message.request, Request::SetView { .. }))
        .expect("inactive-to-live must request a view");
    let Request::SetView { view } = &set_view.request else {
        panic!("expected SetView");
    };
    assert!(
        view.panes.iter().any(
            |pane| pane.session == SessionId(1) && pane.run == ovrcr::protocol::SessionRunId(1)
        ),
        "view must bind the live run: {view:?}"
    );
}

/// Slightly longer than `VIEW_RETRY_BACKOFF` in `crates/ovrcr-tui/src/dashboard/state.rs`.
const PAST_VIEW_RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_millis(260);

fn mouse_event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers,
    }
}

fn focused_terminal_rect(dashboard: &Dashboard, area: Rect) -> Rect {
    let rects = dashboard.pane_rects(area);
    match rects.as_slice() {
        [only] => only.terminal,
        [_, right] => right.terminal,
        _ => panic!("expected one or two pane rects, got {}", rects.len()),
    }
}

fn enable_terminal_mouse(dashboard: &mut Dashboard, modes: &[u8]) {
    let id = dashboard.focused_session().expect("focused session");
    dashboard.install_screen(id, modes);
    dashboard.key(KeyCode::Enter);
}

fn click_in(inner: Rect, kind: MouseEventKind, dx: u16, dy: u16) -> MouseEvent {
    mouse_event(kind, inner.x + dx, inner.y + dy, KeyModifiers::NONE)
}

fn rendered_footer(dashboard: &Dashboard, width: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    (0..width)
        .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn rendered_rows(dashboard: &Dashboard, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect()
        })
        .collect()
}

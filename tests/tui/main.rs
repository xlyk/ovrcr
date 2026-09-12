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

mod copy_history;
mod mouse;
mod palette;
mod sidebar;
mod split;
mod unread;

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
        agent: None,
        agent_epoch: 0,
        unread: None,
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

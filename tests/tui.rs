use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr::context::{ContextSource, ContextUsageReport, ContextUsageSnapshot};
use ovrcr::protocol::{
    ClientMessage, ErrorCode, HierarchySnapshot, HistoryCell, HistoryColor, HistoryOpened,
    HistoryRow, HistoryRows, HistorySnapshotId, ProjectSummary, Request, Response, ServerEvent,
    ServerMessage, WorkspaceSummary,
};
use ovrcr::session::{AgentActivity, SessionId, SessionPhase, SessionSummary, TerminalSize};
use ovrcr::tui::{
    CopyPoint, CopySelection, DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, HistoryCopyJob,
    HistoryCopyPoint, HistoryCopyRange, HistoryCursor, HistoryView, KeyEncoding,
    append_history_selection, dashboard_message_channel, encode_key, encode_paste,
    event_to_request, render_copy,
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
    dashboard.selected = Some(SessionId(1));
    dashboard
}

fn copy_ready_dashboard() -> Dashboard {
    let mut dashboard = dashboard_fixture();
    let id = dashboard.selected.unwrap();
    let size = dashboard.pane_size;
    dashboard.select_request(id, 900);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Screen {
            session: id,
            size,
            bytes: b"abc".to_vec(),
        },
    });
    dashboard
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
        let follow_up = dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::HistoryRows(
                frozen
                    .page(start_row, rows, start_col, cols)
                    .expect("frozen history page"),
            ),
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
    assert!(rendered.iter().any(|row| row.contains("j/k/↑/↓")));
    assert!(rendered.iter().any(|row| row.contains("Ctrl-g")));
    assert_eq!(buffer[(1, 0)].bg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(39, 10)].symbol(), "│");
    assert_eq!(buffer[(39, 1)].symbol(), "│");
    assert!(rendered[1].contains("pid: 111  elapsed: 0m"));
    assert!(rendered[2].contains("─"));
    assert!(buffer[(8, 14)].modifier.contains(Modifier::DIM));
    assert_eq!(buffer[(1, 6)].bg, Color::Rgb(203, 166, 247));

    dashboard.selected = Some(SessionId(2));
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
    dashboard.selected = Some(SessionId(5));
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
        assert_eq!(dashboard.selected, Some(SessionId(5)));
    }
    dashboard.selected = Some(SessionId(1));
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
    dashboard.selected = Some(SessionId(5));
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
    dashboard.selected = Some(SessionId(1));
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
    dashboard.selected = Some(SessionId(5));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();

    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(2, 3)].symbol(), "-");

    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(5),
        bytes: b"PTY output\x1b]52;c;V0FJVElORw==\x1b\\".to_vec(),
    }));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
    );
    assert!(
        !dashboard
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
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(summary)));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 100))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(2, 3)].symbol(), "⠙");

    summary = dashboard.hierarchy.projects[1].workspaces[1].sessions[1].clone();
    summary.activity = AgentActivity::Idle;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(summary)));
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
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(summary)));

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
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(summary)));

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
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
        over_capacity,
    )));
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
    dashboard.selected = Some(SessionId(5));
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
    dashboard.selected = Some(SessionId(5));
    let action = dashboard.key_action(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
    assert_eq!(dashboard.selected, Some(SessionId(1)));
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
    let id = dashboard.selected.unwrap();
    dashboard.select_request(id, 900);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 900,
        response: Response::Screen {
            session: id,
            size: dashboard.pane_size,
            bytes: b"abc".to_vec(),
        },
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
        bytes: b"\rNEW".to_vec(),
    }));
    assert!(dashboard.parser.screen().contents().contains("NEW"));
    assert_eq!(
        dashboard.copy.as_ref().unwrap().selected_text().as_deref(),
        Some("ab")
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("secret".into())),
        ovrcr::tui::DashboardAction::None
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
    let id = dashboard.selected.unwrap();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        bytes: b"\rLIVE".to_vec(),
    }));
    assert_eq!(
        dashboard.parser.screen().cell(0, 0).unwrap().contents(),
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
    assert_eq!(
        footer.trim_end(),
        "COPY  h/j/k/l move  Space anchor  y copy  Esc cancel"
    );
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
    dashboard.resize_request(TerminalSize { rows: 20, cols: 40 }, 901);
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
    let first = dashboard.selected.unwrap();
    dashboard.select_request(first, 901);
    dashboard.select_request(SessionId(5), 902);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Screen {
            session: first,
            size: dashboard.pane_size,
            bytes: b"late-first".to_vec(),
        },
    });
    assert!(!dashboard.parser.screen().contents().contains("late-first"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 901,
        response: Response::Screen {
            session: SessionId(5),
            size: dashboard.pane_size,
            bytes: b"old-request".to_vec(),
        },
    });
    assert!(!dashboard.parser.screen().contents().contains("old-request"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 902,
        response: Response::Screen {
            session: SessionId(5),
            size: dashboard.pane_size,
            bytes: b"matching".to_vec(),
        },
    });
    assert!(dashboard.parser.screen().contents().contains("matching"));
    assert_eq!(
        dashboard.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Copy);
}

#[test]
fn copy_mode_keeps_alternate_and_resync_snapshots() {
    let mut dashboard = dashboard_fixture();
    let id = dashboard.selected.unwrap();
    let size = dashboard.pane_size;
    dashboard.select_request(id, 903);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 903,
        response: Response::Screen {
            session: id,
            size,
            bytes: b"\x1b[?1049hALT".to_vec(),
        },
    });
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Home);
    dashboard.key(KeyCode::Char('v'));
    dashboard.key(KeyCode::Right);
    let requests =
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session: id,
        }));
    assert!(matches!(
        requests.as_slice(),
        [ClientMessage {
            request: Request::Select { session, .. },
            ..
        }] if *session == id
    ));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: requests[0].request_id,
        response: Response::Screen {
            session: id,
            size,
            bytes: b"\x1b[?1049lPRIMARY".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: id,
        bytes: b"-LIVE".to_vec(),
    }));
    assert!(
        dashboard
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
    dashboard.selected = Some(SessionId(5));

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

    dashboard.selected = None;
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

    dashboard.selected = Some(SessionId(2));
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

    dashboard.selected = Some(SessionId(5));
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
    dashboard.selected = Some(SessionId(5));
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
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(paused)));
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
}

#[test]
fn history_live_output_preserves_anchor() {
    let mut dashboard = dashboard_fixture();
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
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        bytes: b"LIVE_MARKER".to_vec(),
    }));
    let after = dashboard.history.as_ref().expect("history remains open");
    assert_eq!(after.opened, before.opened);
    assert_eq!(after.top, before.top);
    assert_eq!(after.left, before.left);
    assert_eq!(after.pages, before.pages);
    assert!(after.new_output);
    assert!(dashboard.parser.screen().contents().contains("LIVE_MARKER"));
    let select = dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
        session: SessionId(1),
    }));
    assert!(matches!(
        select.as_slice(),
        [ClientMessage {
            request: Request::Select {
                session: SessionId(1),
                ..
            },
            ..
        }]
    ));
    let before_screen = dashboard.history.clone().expect("history remains open");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: select[0].request_id,
        response: Response::Screen {
            session: SessionId(1),
            size: TerminalSize { rows: 12, cols: 30 },
            bytes: b"SCREEN_MARKER".to_vec(),
        },
    });
    let after_screen = dashboard.history.as_ref().expect("history remains open");
    assert_eq!(after_screen.opened, before_screen.opened);
    assert_eq!(after_screen.top, before_screen.top);
    assert_eq!(after_screen.left, before_screen.left);
    assert_eq!(after_screen.pages, before_screen.pages);
    assert_eq!(dashboard.parser.screen().size(), (12, 30));
    assert!(
        dashboard
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
        .resize_request(TerminalSize { rows: 50, cols: 30 }, 99)
        .expect("live resize request");
    assert!(matches!(resize.request, Request::Resize { .. }));
    let after = dashboard.history.as_ref().expect("history remains open");
    assert_eq!(after.opened, before.opened);
    assert_eq!(after.pages, before.pages);
    assert_eq!(after.top, before.top);
    assert_eq!(after.left, before.left);
    assert_eq!(dashboard.parser.screen().size(), (50, 30));
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
fn history_pending_keys_coalesce() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::PageUp);
    let requests = dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::HistoryOpened(history_opened(400)),
    });
    let page_request = requests.first().cloned().expect("initial history page");
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
    assert!(latest.start_row >= (latest_top / 16) * 16);
    assert_eq!(latest.start_row % 16, 0);
    assert_eq!(latest.start_col, 0);

    let initial_page = dashboard
        .history
        .as_ref()
        .unwrap()
        .pages
        .front()
        .unwrap()
        .clone();
    dashboard.pane_size = TerminalSize {
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
fn copy_history_range_survives_live_eviction() {
    let (mut live_parser, mut frozen, old_lines) = numbered_history();
    let opened = frozen.opened().clone();
    let mut dashboard = dashboard_fixture();
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
    let anchor_action = dashboard.key(KeyCode::Char(' '));
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

    let before_resize = dashboard.history.as_ref().unwrap().copy_range();
    dashboard.resize_request(TerminalSize { rows: 30, cols: 70 }, 900);
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
    assert!(
        !refreshed_page.rows[0]
            .cells
            .iter()
            .any(|cell| cell.text == "OLD_015")
    );

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

    let reverse_anchor = dashboard.key(KeyCode::Char(' '));
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
            total_rows: 2,
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

#[test]
fn copy_history_invalid_snapshot_never_emits_partial_text() {
    let range = HistoryCopyRange {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        anchor: HistoryCopyPoint { row: 0, col: 0 },
        cursor: HistoryCopyPoint { row: 1, col: 0 },
    };
    for code in [ErrorCode::Conflict, ErrorCode::NotFound] {
        let mut dashboard = dashboard_with_history_copy(range);
        let request = dashboard.history_request_if_needed().expect("copy page");
        let release = dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
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
    }

    let mut dashboard = dashboard_with_history_copy(range);
    let request = dashboard.history_request_if_needed().expect("copy page");
    let next = dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::HistoryRows(simple_copy_page(0, "partial")),
    });
    let old_pending = next.first().cloned().expect("next row remains pending");
    assert!(dashboard.history.as_ref().unwrap().copy_job.is_some());
    assert!(dashboard.take_pending_history_copy().is_none());
    let cancelled = dashboard.key(KeyCode::Esc);
    assert_eq!(cancelled, ovrcr::tui::DashboardAction::Redraw);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::History);
    assert_eq!(dashboard.copy_notice.as_deref(), Some("Copy cancelled"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: old_pending.request_id,
        response: Response::HistoryRows(simple_copy_page(1, "late")),
    });
    assert!(dashboard.take_pending_history_copy().is_none());

    let mut malformed = dashboard_with_history_copy(range);
    let request = malformed.history_request_if_needed().expect("copy page");
    let _ = malformed.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::HistoryRows(HistoryRows {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
            start_row: 0,
            start_col: 0,
            rows: vec![HistoryRow {
                width: 4,
                cells: vec![history_cell("short", 1)],
                wrapped: false,
            }],
        }),
    });
    assert!(malformed.history.as_ref().unwrap().copy_job.is_none());
    assert!(malformed.take_pending_history_copy().is_none());

    let mut replacement = dashboard_with_history_copy(range);
    replacement.history_begin_request = Some(ovrcr::tui::PendingHistoryBegin {
        request_id: 90,
        session: SessionId(1),
        cancelled: false,
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
    assert!(replacement_requests.iter().any(|request| matches!(
        request.request,
        Request::HistoryEnd {
            session: SessionId(1),
            snapshot: HistorySnapshotId(7),
        }
    )));

    let mut oversize = dashboard_with_history_copy(HistoryCopyRange {
        cursor: HistoryCopyPoint { row: 0, col: 127 },
        ..range
    });
    let request = oversize.history_request_if_needed().expect("copy page");
    let oversize_page = HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        start_col: 0,
        rows: vec![HistoryRow {
            width: 128,
            cells: (0..128)
                .map(|_| history_cell(&"x".repeat(600), 1))
                .collect(),
            wrapped: false,
        }],
    };
    dashboard = oversize;
    let _ = dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::HistoryRows(oversize_page),
    });
    assert!(dashboard.take_pending_history_copy().is_none());
    assert!(dashboard.history.as_ref().unwrap().copy_job.is_none());
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
        .push_back(history_page(0, 0, 130, left_cells));
    tiled_view.pages.push_back(history_page(
        0,
        128,
        130,
        vec![history_cell("", 0), history_cell("R", 1)],
    ));
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

    let mut selected_row = special_history_row(15);
    selected_row.cells[129] = history_cell("N", 1);
    let mut selected_view = ovrcr::tui::HistoryView::new(history_opened(1), 0);
    selected_view.anchor = Some(HistoryCopyPoint { row: 0, col: 126 });
    selected_view.cursor = Some(HistoryCursor {
        point: HistoryCopyPoint { row: 0, col: 129 },
        row_width: 132,
        cell_width: 1,
    });
    selected_view.pages.push_back(HistoryRows {
        session: SessionId(1),
        snapshot: HistorySnapshotId(7),
        start_row: 0,
        start_col: 0,
        rows: vec![selected_row.clone()],
    });
    let mut selected_terminal = Terminal::new(TestBackend::new(132, 1)).unwrap();
    selected_terminal
        .draw(|frame| {
            ovrcr::tui::render_history(frame, Rect::new(0, 0, 132, 1), &selected_view);
            for col in 126..=129 {
                assert_eq!(frame.buffer_mut()[(col, 0)].fg, Color::Rgb(30, 30, 46));
                assert_eq!(frame.buffer_mut()[(col, 0)].bg, Color::Rgb(148, 226, 213));
            }
        })
        .unwrap();
    for col in [126, 127, 129] {
        assert_eq!(
            selected_terminal.backend().buffer()[(col, 0)].fg,
            Color::Rgb(30, 30, 46)
        );
        assert_eq!(
            selected_terminal.backend().buffer()[(col, 0)].bg,
            Color::Rgb(148, 226, 213)
        );
    }
    assert_eq!(
        selected_terminal.backend().buffer()[(127, 0)].symbol(),
        "界"
    );
    assert_eq!(selected_terminal.backend().buffer()[(128, 0)].symbol(), " ");
    assert_eq!(selected_terminal.backend().buffer()[(129, 0)].symbol(), "N");

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
                ovrcr::tui::render_history(frame, Rect::new(0, 0, 132, 1), &selected_view)
            })
            .unwrap();
    }
    let mut round_trip = vt100::Parser::new(1, 132, 0);
    round_trip.process(&crossterm_bytes);
    for col in [126, 127, 129] {
        let cell = round_trip.screen().cell(0, col).unwrap();
        assert_eq!(cell.fgcolor(), vt100::Color::Rgb(30, 30, 46));
        assert_eq!(cell.bgcolor(), vt100::Color::Rgb(148, 226, 213));
    }
    assert_eq!(round_trip.screen().cell(0, 127).unwrap().contents(), "界");
    assert_eq!(round_trip.screen().cell(0, 129).unwrap().contents(), "N");
    crossterm::style::force_color_output(false);

    let mut clipped_terminal = Terminal::new(TestBackend::new(128, 1)).unwrap();
    clipped_terminal
        .draw(|frame| ovrcr::tui::render_history(frame, Rect::new(0, 0, 128, 1), &selected_view))
        .unwrap();
    assert_eq!(clipped_terminal.backend().buffer()[(127, 0)].symbol(), " ");
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
    dashboard.selected = Some(SessionId(5));
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
    assert!(rendered.contains("p pause  r resume"));
    assert!(
        rendered
            .lines()
            .last()
            .is_some_and(|footer| footer.contains("q detach"))
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
        footer.starts_with("r resume"),
        "narrow footer was {footer:?}"
    );
}

#[test]
fn collapse_and_mouse_hits_use_current_visible_tree() {
    let mut dashboard = dashboard_fixture();
    dashboard.selected = Some(SessionId(5));
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
    assert_eq!(dashboard.selected, Some(SessionId(1)));
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
    assert_eq!(action, ovrcr::tui::DashboardAction::None);
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.mouse_action(mouse(6), Rect::new(0, 0, 120, 40)),
        ovrcr::tui::DashboardAction::None
    );
}

#[test]
fn fifty_session_selection_scrolls_tree_and_mouse_hits_viewport() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 34, cols: 88 });
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
    assert_eq!(dashboard.selected, Some(SessionId(50)));
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
    for row in 34..=36 {
        let action = dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 4,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        );
        assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
        assert_eq!(dashboard.selected, Some(SessionId(50)));
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
    assert_eq!(dashboard.selected, Some(SessionId(50)));
    dashboard.resize_request(TerminalSize { rows: 20, cols: 40 }, 99);
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
    dashboard.parser.process(b"\x1b[?2004h");
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

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr::protocol::{
    ClientMessage, ErrorCode, HierarchySnapshot, ProjectSummary, Request, Response, ServerEvent,
    ServerMessage, WorkspaceSummary,
};
use ovrcr::session::{SessionId, SessionPhase, SessionSummary, TerminalSize};
use ovrcr::tui::{
    DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, KeyEncoding, dashboard_message_channel, encode_key,
    encode_paste, event_to_request,
};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::Rect;
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
    assert!(!buffer[(8, 14)].modifier.contains(Modifier::DIM));
    assert_eq!(buffer[(1, 6)].bg, Color::Rgb(203, 166, 247));

    dashboard.selected = Some(SessionId(2));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let selected_exited = (0..120)
        .map(|col| terminal.backend().buffer()[(col, 1)].symbol())
        .collect::<String>();
    assert!(selected_exited.contains("pid: —"));
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
    assert_eq!(row(3).trim_end(), "    local");
    assert_eq!(row(4).trim_end(), "     ├ terminal");
    assert_eq!(row(5).trim_end(), "     └ run 0m  ctx —");
    assert_eq!(row(6).trim_end(), "    review");
    assert_eq!(row(7).trim_end(), "     ├ claude");
    assert_eq!(buffer[(2, 1)].fg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(2, 2)].fg, Color::Rgb(203, 166, 247));
    assert_eq!(buffer[(4, 2)].fg, Color::Rgb(205, 214, 244));
    assert_eq!(buffer[(2, 6)].fg, Color::Rgb(108, 112, 134));
    assert_eq!(row(9).trim_end(), "");
    assert_eq!(row(10).trim_end(), "  ▼ lifecycle");
    assert_eq!(row(11).trim_end(), "    local");
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
    dashboard.busy_sessions.insert(SessionId(1));
    for (time, marker) in [(0, "⠋"), (100, "⠙"), (900, "⠏"), (1000, "⠋")] {
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, time))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(2, 6)].symbol(), marker);
        assert_eq!(
            terminal.backend().buffer()[(2, 6)].fg,
            Color::Rgb(166, 227, 161)
        );
        assert_eq!(terminal.backend().buffer()[(2, 3)].symbol(), " ");
    }
    dashboard.busy_sessions.clear();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1100))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(2, 6)].symbol(), " ");
    // An exited process cannot remain busy, even if an old signal is retained.
    dashboard.busy_sessions.insert(SessionId(2));
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 1200))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(2, 14)].symbol(), " ");
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
fn pause_resume_dense_status_has_priority() {
    let mut dashboard = dashboard_fixture();
    dashboard.selected = Some(SessionId(5));
    dashboard.hierarchy.projects[1].workspaces[1].sessions[1].phase = SessionPhase::Paused;
    dashboard.busy_sessions.insert(SessionId(5));

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
    assert!(rendered.contains("pid: 555  elapsed: 0m  paused"));
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

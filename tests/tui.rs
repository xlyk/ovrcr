use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr::protocol::{
    HierarchySnapshot, ProjectSummary, Response, ServerEvent, ServerMessage, WorkspaceSummary,
};
use ovrcr::session::{SessionId, SessionPhase, SessionSummary, TerminalSize};
use ovrcr::tui::{
    DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, KeyEncoding, dashboard_message_channel, encode_key,
    encode_paste, event_to_request,
};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use std::path::PathBuf;
use std::sync::mpsc::TrySendError;

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
                            session(1, "consigint", "auth", "review", "claude", Some(111), 0),
                            session(5, "consigint", "auth", "local", "zsh", Some(555), 0),
                        ],
                    },
                ],
            },
        ],
    };
    dashboard.selected = Some(SessionId(1));
    dashboard
}

#[test]
fn dashboard_layout() {
    let dashboard = dashboard_fixture();
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
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
    assert_eq!(rendered[0].trim_end(), "OVRCR");
    assert!(rendered.iter().any(|row| row.contains("PID 111")));
    assert!(rendered.iter().any(|row| row.contains("ctx -")));
    assert!(rendered.iter().any(|row| row.contains("BROWSE")));
    assert!(rendered.iter().any(|row| row.contains("Ctrl-g")));
    assert_eq!(buffer[(1, 0)].bg, Color::Rgb(203, 166, 247));
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
    let action = dashboard.mouse_action(mouse(3), Rect::new(0, 0, 120, 40));
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert!(
        !dashboard
            .visible_rows()
            .contains(&ovrcr::tui::TreeRow::Session { id: SessionId(5) })
    );
    let action = dashboard.mouse_action(mouse(3), Rect::new(0, 0, 120, 40));
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    let action = dashboard.mouse_action(mouse(5), Rect::new(0, 0, 120, 40));
    assert!(matches!(action, ovrcr::tui::DashboardAction::Request(_)));
    assert_eq!(dashboard.selected, Some(SessionId(1)));
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert_eq!(
        dashboard.mouse_action(mouse(5), Rect::new(0, 0, 120, 40)),
        ovrcr::tui::DashboardAction::None
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
    assert_eq!(buffer[(6, 0)].fg, Color::Indexed(1));
    assert_eq!(buffer[(6, 1)].symbol(), "界");
    assert_eq!(buffer[(7, 1)].symbol(), " ");
    assert_eq!(buffer[(7, 0)].modifier, Modifier::empty());
    assert_eq!(terminal.backend().cursor_position(), (8, 1).into());
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
    let mut dashboard = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
    dashboard.select_request(SessionId(7), 1);
    dashboard.parser.process(b"\x1b[?2004h");
    dashboard.mode = ovrcr::tui::InputMode::Terminal;
    let request = event_to_request(&mut dashboard, Event::Paste("hello\n".into()), 2).unwrap();
    assert_eq!(
        request.request,
        ovrcr::protocol::Request::Input {
            session: SessionId(7),
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

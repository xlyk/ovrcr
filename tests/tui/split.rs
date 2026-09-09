//! Split panes: view readiness, per-pane state, and view retries.

use crate::*;

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
    assert!(browse_footer.contains("Tab Next pane  x Close pane"));
    assert!(browse_footer.contains("? Help"));
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
    assert!(footer.contains("split hidden"));

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
    assert!(ordinary_footer.contains("? Help"));

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
    assert!(browse_footer.contains("split hidden"));

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
    assert!(copy_footer.contains("split hidden"));
    assert!(copy_footer.contains("? Help"));
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
    assert!(history_footer.contains("split hidden"));
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
fn split_error_survives_a_screen_dirty_refresh() {
    let mut dashboard = dashboard_fixture();
    let session = dashboard.focused_session().unwrap();
    for project in &mut dashboard.hierarchy.projects {
        for workspace in &mut project.workspaces {
            workspace
                .sessions
                .retain(|session| session.id == SessionId(1));
        }
    }
    assert_eq!(
        dashboard.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.error.as_deref(),
        Some("No other visible session to split")
    );
    // The server, not the user, asks for this view; its success owns no banner.
    let mut dirty =
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session,
            revision: dashboard.view_revision,
        }));
    assert_eq!(dirty.len(), 1);
    let refresh = dirty
        .pop()
        .expect("a dirty screen should re-request the view");
    acknowledge_view_request(&mut dashboard, refresh);
    assert_eq!(
        dashboard.error.as_deref(),
        Some("No other visible session to split"),
        "a server-driven refresh must not clear a fresh error"
    );

    // A refused view does own its banner, so the view that succeeds clears it.
    let refused = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 810)
        .unwrap()
        .expect("new geometry should request a view");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: refused.request_id,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "view refused".into(),
        },
    });
    assert_eq!(dashboard.error.as_deref(), Some("Conflict: view refused"));
    std::thread::sleep(PAST_VIEW_RETRY_BACKOFF);
    let retry = dashboard
        .view_request(Rect::new(0, 0, 120, 40), 811)
        .unwrap()
        .expect("the refused view must be retried once the backoff elapses");
    acknowledge_all_view_targets(&mut dashboard, retry);
    assert!(
        dashboard.error.is_none(),
        "a view failure banner is cleared by the view that succeeds"
    );
}

#[test]
fn split_error_is_cleared_by_the_view_the_next_selection_requests() {
    let mut dashboard = dashboard_fixture();
    // Hiding the focused session's workspace leaves `v` with nothing to split from while `j`
    // still has another session to select, so the banner and the selection are independent.
    dashboard
        .collapsed_workspaces
        .insert(("consigint".into(), "auth".into()));
    assert_eq!(
        dashboard.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.error.as_deref(),
        Some("No other visible session to split")
    );
    let request = match dashboard.key(KeyCode::Char('j')) {
        ovrcr::tui::DashboardAction::Request(request) => request,
        action => panic!("a selection change should request a view: {action:?}"),
    };
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    acknowledge_view_request(&mut dashboard, request);
    assert!(
        dashboard.error.is_none(),
        "a banner must not outlive the view change the user asked for: {:?}",
        dashboard.error
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

#[test]
fn ux_narrow_layout_keeps_help_focus_and_nonzero_view_sizes() {
    for (width, height) in [(80, 24), (60, 20), (40, 12)] {
        for split in [false, true] {
            let mut dashboard = dashboard_fixture();
            if split {
                assert!(dashboard.split_pane());
            }
            let area = Rect::new(0, 0, width, height);
            let request = dashboard.view_request(area, 2000).unwrap().unwrap();
            let Request::SetView { view } = &request.request else {
                panic!("view request")
            };
            assert_eq!(
                view.panes.len(),
                1,
                "narrow split must show its focused pane"
            );
            for pane in &view.panes {
                assert!(pane.size.rows > 0 && pane.size.cols > 0);
            }
            acknowledge_all_view_targets(&mut dashboard, request);
            let rows = rendered_rows(&dashboard, width, height);
            assert!(
                rows.last().unwrap().contains("? Help"),
                "{width} split={split}: {rows:?}"
            );
            assert!(dashboard.panes[dashboard.focused_pane].ready);
            let before = dashboard.focused_session();
            dashboard.key(KeyCode::Char('j'));
            assert_ne!(dashboard.focused_session(), before);
        }
    }
}

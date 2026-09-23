//! Split panes: view readiness, per-pane state, and view retries.

use crate::*;
use ovrcr::protocol::DashboardView;

fn wide_area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

fn banner(dashboard: &Dashboard) -> String {
    rendered_footer(dashboard, 120)
}

fn pane_widths(dashboard: &Dashboard, area: Rect) -> Vec<u16> {
    dashboard
        .pane_rects(area)
        .into_iter()
        .map(|pane| pane.terminal.width)
        .collect()
}

fn set_view(msg: &ClientMessage) -> &DashboardView {
    match &msg.request {
        Request::SetView { view } => view,
        other => panic!("expected SetView, got {other:?}"),
    }
}

fn request_view(dashboard: &mut Dashboard, area: Rect, revision: u64) -> ClientMessage {
    try_request_view(dashboard, area, revision).expect("expected SetView")
}

fn try_request_view(
    dashboard: &mut Dashboard,
    area: Rect,
    _revision: u64,
) -> Option<ClientMessage> {
    let request = dashboard.request_view_at(area);
    // The event loop flushes after every view request, so the fixture must not leave the
    // same request queued for the next drain.
    dashboard.drain_outbox();
    request
}

fn split(dashboard: &mut Dashboard) {
    let action = dashboard.key(KeyCode::Char('v'));
    pump_view(dashboard, action);
}

fn close_pane(dashboard: &mut Dashboard) {
    let action = dashboard.key(KeyCode::Char('x'));
    pump_view(dashboard, action);
}

fn tab_pane(dashboard: &mut Dashboard) {
    let _ = dashboard.ctrl('g');
    let action = dashboard.key(KeyCode::Tab);
    pump_view(dashboard, action);
}

fn fixture_session(id: SessionId) -> SessionSummary {
    fixture_hierarchy()
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .find(|session| session.id == id)
        .cloned()
        .expect("fixture session")
}

fn drawn_contains(dashboard: &Dashboard, width: u16, height: u16, needle: &str) -> bool {
    rendered_rows(dashboard, width, height)
        .iter()
        .any(|row| row.contains(needle))
}

#[test]
fn split_state_focus_and_close_preserve_sessions() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    dashboard.install_area(area);
    split(&mut dashboard);
    assert_eq!(dashboard.pane_rects(area).len(), 2);
    let right = dashboard.focused_session().unwrap();
    assert_ne!(right, SessionId(1));
    split(&mut dashboard);
    assert_eq!(dashboard.pane_rects(area).len(), 2);
    close_pane(&mut dashboard);
    assert_eq!(dashboard.pane_rects(area).len(), 1);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    close_pane(&mut dashboard);
    assert_eq!(dashboard.pane_rects(area).len(), 1);

    let mut single = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    for project in &mut hierarchy.projects {
        for workspace in &mut project.workspaces {
            workspace
                .sessions
                .retain(|session| session.id == SessionId(1));
        }
    }
    single.install_hierarchy(hierarchy);
    split(&mut single);
    assert_eq!(single.pane_rects(Rect::new(0, 0, 88, 38)).len(), 1);
    assert!(banner(&single).contains("No other visible session to split"));
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
fn sidebar_border_drag_resizes_the_sidebar_and_the_pane() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    let initial = request_view(&mut dashboard, area, 0);
    acknowledge_all_view_targets(&mut dashboard, initial);
    let revision = 1;
    let border = |dashboard: &Dashboard| {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, dashboard, 0))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..120)
            .find(|x| buffer[(*x, 10)].symbol() == "│")
            .expect("sidebar border")
    };
    assert_eq!(border(&dashboard), 39);

    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                39,
                10,
                KeyModifiers::NONE
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                49,
                10,
                KeyModifiers::NONE
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(border(&dashboard), 49);
    assert_eq!(pane_widths(&dashboard, area), vec![70]);
    assert_view_input_blocked(&mut dashboard);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let review = (0..49)
        .map(|x| terminal.backend().buffer()[(x, 4)].symbol())
        .collect::<String>();
    assert_eq!(review, format!("▌    - review{}claude [x]", " ".repeat(26)));

    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                3,
                10,
                KeyModifiers::NONE
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(border(&dashboard), 19);
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                100,
                10,
                KeyModifiers::NONE
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(border(&dashboard), 59);
    assert_eq!(pane_widths(&dashboard, area), vec![60]);

    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Up(MouseButton::Left),
                100,
                10,
                KeyModifiers::NONE
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    let resized = request_view(&mut dashboard, area, revision);
    assert_eq!(set_view(&resized).panes[0].size.cols, 60);
    pump_view(&mut dashboard, DashboardAction::Request(resized));
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                30,
                10,
                KeyModifiers::NONE
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(border(&dashboard), 59);
}

#[test]
fn divider_drag_resizes_both_panes_and_revokes_input_until_acknowledged() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    split(&mut dashboard);
    let initial = request_view(&mut dashboard, area, 0);
    let revision = set_view(&initial).revision;
    acknowledge_all_view_targets(&mut dashboard, initial);
    assert_view_input_allowed(&mut dashboard);

    let divider = dashboard.pane_rects(area)[0].terminal.right();
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                divider,
                10,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                60,
                10,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(pane_widths(&dashboard, area), vec![20, 59]);
    assert_view_input_blocked(&mut dashboard);
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Up(MouseButton::Left),
                60,
                10,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );

    let resized = request_view(&mut dashboard, area, revision);
    assert_eq!(
        set_view(&resized)
            .panes
            .iter()
            .map(|pane| pane.size.cols)
            .collect::<Vec<_>>(),
        vec![20, 59]
    );
    acknowledge_all_view_targets(&mut dashboard, resized);
    assert_view_input_allowed(&mut dashboard);

    enable_terminal_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let right = Rect::new(61, 3, 59, 36);
    assert_eq!(
        dashboard.mouse_action(
            click_in(right, MouseEventKind::Down(MouseButton::Left), 2, 3),
            area,
        ),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<0;3;4M".to_vec())
    );
    assert_eq!(
        dashboard.mouse_action(
            click_in(right, MouseEventKind::Up(MouseButton::Left), 2, 3),
            area,
        ),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<0;3;4m".to_vec())
    );

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(60, 10)].symbol(), "│");

    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                60,
                10,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                60,
                10,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Up(MouseButton::Left),
                0,
                10,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_view_input_allowed(&mut dashboard);
}

#[test]
fn divider_gesture_releases_all_application_buttons_without_resizing() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    split(&mut dashboard);
    let split_req = request_view(&mut dashboard, area, 0);
    let revision = set_view(&split_req).revision;
    acknowledge_all_view_targets(&mut dashboard, split_req);
    enable_terminal_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let focused = focused_terminal_rect(&dashboard, area);
    let divider = dashboard
        .pane_rects(area)
        .first()
        .expect("left pane")
        .terminal
        .right();
    let widths = pane_widths(&dashboard, area);

    assert_eq!(
        dashboard.mouse_action(
            click_in(focused, MouseEventKind::Down(MouseButton::Right), 2, 3),
            area,
        ),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<2;3;4M".to_vec())
    );
    assert_eq!(
        dashboard.mouse_action(
            click_in(focused, MouseEventKind::Down(MouseButton::Middle), 4, 5),
            area,
        ),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<1;5;6M".to_vec())
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                divider,
                10,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Up(MouseButton::Right),
                divider,
                10,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Up(MouseButton::Left),
                divider,
                10,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(pane_widths(&dashboard, area), widths);
    assert_view_input_allowed(&mut dashboard);
    let _ = revision;
}

#[test]
fn divider_preference_survives_resize_hiding_and_focus_loss_with_nonzero_origins() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(7, 4, 120, 40);
    split(&mut dashboard);
    let initial = request_view(&mut dashboard, area, 0);
    let mut revision = set_view(&initial).revision;
    acknowledge_all_view_targets(&mut dashboard, initial);

    let default = dashboard.pane_rects(area);
    let divider = default[0].terminal.right();
    let left_edge = default[0].terminal.x;
    dashboard.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            divider,
            area.y + 6,
            KeyModifiers::NONE,
        ),
        area,
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                left_edge,
                area.y + 6,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(pane_widths(&dashboard, area), vec![20, 59]);
    dashboard.mouse_action(
        mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            left_edge,
            area.y + 6,
            KeyModifiers::NONE,
        ),
        area,
    );
    let resized = request_view(&mut dashboard, area, revision);
    revision = set_view(&resized).revision;
    acknowledge_all_view_targets(&mut dashboard, resized);

    let wider = Rect::new(7, 4, 160, 40);
    let wider_request = request_view(&mut dashboard, wider, revision);
    revision = set_view(&wider_request).revision;
    assert_eq!(
        set_view(&wider_request)
            .panes
            .iter()
            .map(|pane| pane.size.cols)
            .collect::<Vec<_>>(),
        vec![30, 89]
    );
    acknowledge_all_view_targets(&mut dashboard, wider_request);

    let current_divider = 47 + 30;
    dashboard.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            current_divider,
            area.y + 6,
            KeyModifiers::NONE,
        ),
        wider,
    );
    assert_eq!(
        dashboard.event_action(Event::FocusLost),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.event_action(Event::FocusGained),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                current_divider + 10,
                area.y + 6,
                KeyModifiers::NONE,
            ),
            wider,
        ),
        ovrcr::tui::DashboardAction::None
    );
    assert_view_input_allowed(&mut dashboard);

    let narrow = Rect::new(11, 6, 80, 24);
    let narrow_request = request_view(&mut dashboard, narrow, revision);
    revision = set_view(&narrow_request).revision;
    assert_eq!(set_view(&narrow_request).panes.len(), 1);
    acknowledge_all_view_targets(&mut dashboard, narrow_request);

    let restored = request_view(&mut dashboard, area, revision);
    revision = set_view(&restored).revision;
    assert_eq!(
        set_view(&restored)
            .panes
            .iter()
            .map(|pane| pane.size.cols)
            .collect::<Vec<_>>(),
        vec![20, 59]
    );
    acknowledge_all_view_targets(&mut dashboard, restored);

    dashboard.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            67,
            area.y + 6,
            KeyModifiers::NONE,
        ),
        area,
    );
    let hidden_again = request_view(&mut dashboard, narrow, revision);
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                75,
                narrow.y + 6,
                KeyModifiers::NONE,
            ),
            narrow,
        ),
        ovrcr::tui::DashboardAction::None
    );
    assert!(
        set_view(&hidden_again)
            .panes
            .iter()
            .all(|pane| pane.size.cols > 0 && pane.size.rows > 0)
    );
}

#[test]
fn split_layout_renders_independent_cells_and_cursor() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    split(&mut dashboard);
    let right_session = dashboard.focused_session().unwrap();
    let split_req = request_view(&mut dashboard, area, 0);
    let initial_view = set_view(&split_req).clone();
    let left_session = initial_view.panes[0].session;
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
    let revision = initial_view.revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: split_req.request_id,
        response: Response::Screen {
            session: left_session,
            run: ovrcr_protocol::SessionRunId(1),
            revision,
            size: initial_view.panes[0].size,
            bytes: b"\x1b[31mLEFT\x1b[0m\x1b[36;38H\xE7\x95\x8C\x1b[31m\x1b[36;1H<\x1b[2;4H"
                .to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: split_req.request_id,
        response: Response::Screen {
            session: right_session,
            run: ovrcr_protocol::SessionRunId(1),
            revision,
            size: initial_view.panes[1].size,
            bytes: b"\x1b[32mRIGHT\x1b[0m\x1b[36;39H\xE7\x95\x8C\x1b[32m\x1b[36;1H>\x1b[2;5H"
                .to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: split_req.request_id,
        response: Response::Ok,
    });
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);

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

    assert_eq!(dashboard.ctrl('g'), DashboardAction::EnterBrowse);
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
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);

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

    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: right_session,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"\x1b[36;40H\xE7\x95\x8C".to_vec(),
    }));
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(119, 38)].symbol(), " ");
    assert_eq!(terminal.backend().buffer()[(79, 38)].symbol(), "│");

    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: left_session,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"\x1b[36;39H\xE7\x95\x8C".to_vec(),
    }));
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(78, 38)].symbol(), " ");
    assert_eq!(terminal.backend().buffer()[(79, 38)].symbol(), "│");

    tab_pane(&mut dashboard);
    assert_eq!(dashboard.focused_session(), Some(left_session));
    let focused_left = request_view(&mut dashboard, area, revision);
    let revision = set_view(&focused_left).revision;
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: focused_left.request_id,
        response: Response::Screen {
            session: left_session,
            run: ovrcr_protocol::SessionRunId(1),
            revision,
            size: TerminalSize { rows: 36, cols: 39 },
            bytes: b"\x1b[2;4H".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: focused_left.request_id,
        response: Response::Screen {
            session: right_session,
            run: ovrcr_protocol::SessionRunId(1),
            revision,
            size: TerminalSize { rows: 36, cols: 40 },
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: focused_left.request_id,
        response: Response::Ok,
    });
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), (43, 4).into());

    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: left_session,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"\x1b[?25l".to_vec(),
    }));
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());

    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: left_session,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"\x1b[?25h\x1b[2;4H".to_vec(),
    }));
    assert_eq!(dashboard.ctrl('g'), DashboardAction::EnterBrowse);
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());

    let backend = TestBackend::new(1, 1);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());

    let narrow = request_view(&mut dashboard, Rect::new(0, 0, 80, 24), revision);
    let narrow_view = set_view(&narrow).clone();
    let revision = narrow_view.revision;
    assert_eq!(narrow_view.focused, Some(left_session));
    assert_eq!(narrow_view.panes.len(), 1);
    assert_eq!(narrow_view.panes[0].session, left_session);
    assert_eq!(
        narrow_view.panes[0].size,
        TerminalSize { rows: 20, cols: 40 }
    );
    assert_eq!(dashboard.pane_rects(Rect::new(0, 0, 80, 24)).len(), 1);
    acknowledge_all_view_targets(&mut dashboard, narrow);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: left_session,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"L\x1b[2;4H".to_vec(),
    }));
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

    let ordinary = dashboard_fixture();
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

    assert_eq!(dashboard.ctrl('g'), DashboardAction::EnterBrowse);
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
    dashboard.key(KeyCode::Char('?'));
    let copy_help = palette_text(&dashboard);
    assert!(copy_help.contains("Move"));
    assert!(copy_help.contains("Selection"));
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Esc);
    let begin = dashboard.key(KeyCode::PageUp);
    if let DashboardAction::Request(request) = begin {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::HistoryOpened(history_opened(1)),
        });
    }
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let history_footer = (0..80)
        .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
        .collect::<String>();
    assert!(history_footer.contains("HISTORY"));
    assert!(history_footer.contains("Waiting") || history_footer.contains("HISTORY"));
    assert!(history_footer.contains("split hidden"));
    dashboard.key(KeyCode::Char('?'));
    for _ in 0..30 {
        dashboard.key(KeyCode::Down);
    }
    assert!(palette_text(&dashboard).contains("Esc"));
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Esc);

    let wide = request_view(&mut dashboard, area, revision);
    let wide_view = set_view(&wide).clone();
    let revision = wide_view.revision;
    assert_eq!(wide_view.focused, Some(left_session));
    assert_eq!(wide_view.panes.len(), 2);
    assert_eq!(wide_view.panes[0].session, left_session);
    assert_eq!(wide_view.panes[0].size, TerminalSize { rows: 36, cols: 39 });
    assert_eq!(wide_view.panes[1].session, right_session);
    assert_eq!(wide_view.panes[1].size, TerminalSize { rows: 36, cols: 40 });
    acknowledge_all_view_targets(&mut dashboard, wide);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: left_session,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"L".to_vec(),
    }));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: right_session,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"R".to_vec(),
    }));
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(40, 3)].symbol(), "L");
    assert_eq!(terminal.backend().buffer()[(80, 3)].symbol(), "R");

    let _ = dashboard.ctrl('g');
    assert!(matches!(
        dashboard.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    ));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: right_session,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"\x1b[1;1HOTHER".to_vec(),
    }));
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(40, 3)].symbol(), "L");
    assert_eq!(terminal.backend().buffer()[(80, 3)].symbol(), "O");

    let mut exited_split = dashboard_fixture();
    split(&mut exited_split);
    let split_request = request_view(&mut exited_split, area, 0);
    acknowledge_all_view_targets(&mut exited_split, split_request);
    tab_pane(&mut exited_split);
    let focused_request = request_view(&mut exited_split, area, 1);
    let exited_revision = set_view(&focused_request).revision;
    acknowledge_all_view_targets(&mut exited_split, focused_request);
    exited_split.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        run: ovrcr_protocol::SessionRunId(1),
        revision: exited_revision,
        bytes: b"FINAL\x1b[2;4H".to_vec(),
    }));
    let mut exited_summary = fixture_session(SessionId(1));
    exited_summary.phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    exited_summary.pid = None;
    let lifecycle_outgoing = exited_split.handle_server_message(ServerMessage::Event(
        ServerEvent::SessionChanged(Box::new(exited_summary)),
    ));
    assert!(lifecycle_outgoing.is_empty());
    assert_eq!(exited_split.focused_session(), Some(SessionId(1)));
    assert_eq!(exited_split.pane_rects(area).len(), 2);
    exited_split.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(4),
        run: ovrcr_protocol::SessionRunId(1),
        revision: exited_revision,
        bytes: b"SURVIVOR".to_vec(),
    }));
    assert!(drawn_contains(&exited_split, 120, 40, "SURVIVOR"));
    let backend = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, &exited_split, 0))
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(40, 3)].symbol(), "F");
    assert_eq!(terminal.backend().buffer()[(80, 3)].symbol(), "S");
    assert_eq!(terminal.backend().cursor_position(), (0, 0).into());

    let mut exited_single = dashboard_fixture();
    exited_single.install_screen(SessionId(1), b"FINAL\x1b[2;4H");
    assert_eq!(exited_single.key(KeyCode::Enter), DashboardAction::Redraw);
    let mut exited_summary = fixture_session(SessionId(1));
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
    let left = dashboard.focused_session();
    let _ = dashboard.ctrl('g');
    assert_eq!(
        dashboard.mouse_action(click(40, 3), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), left);
    assert_eq!(
        dashboard.mouse_action(click(79, 20), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                ..click(79, 20)
            },
            area
        ),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), left);
    assert_eq!(
        dashboard.mouse_action(click(80, 1), area),
        ovrcr::tui::DashboardAction::None
    );
    assert_eq!(dashboard.focused_session(), left);
    assert_eq!(
        dashboard.mouse_action(click(80, 3), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), Some(right_session));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.mouse_action(click(40, 3), area),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), Some(left_session));
}

#[test]
fn split_state_modes_and_input_are_local() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    split(&mut dashboard);
    let right = dashboard.focused_session().unwrap();
    let request = request_view(&mut dashboard, area, 0);
    let view = set_view(&request).clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: right,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: TerminalSize { rows: 36, cols: 40 },
            bytes: b"\x1b[?1h\x1b[?2004hRIGHT".to_vec(),
        },
    });
    let left = view.panes[0].session;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: left,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: TerminalSize { rows: 36, cols: 39 },
            bytes: b"LEFT".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.event_action(Event::Paste("x".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[200~x\x1b[201~".to_vec())
    );
    assert_eq!(
        dashboard.key(KeyCode::Up),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1bOA".to_vec())
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(b"x".to_vec())
    );
}

#[test]
fn split_review_readiness_requires_matching_targets_and_cancels_resize_copy() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    split(&mut dashboard);
    let request = request_view(&mut dashboard, area, 0);
    let mut revision = set_view(&request).revision;
    acknowledge_all_view_targets(&mut dashboard, request);
    assert_view_input_allowed(&mut dashboard);

    let focused = dashboard.focused_session().unwrap();
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(b"x".to_vec())
    );
    assert_eq!(dashboard.ctrl('g'), DashboardAction::EnterBrowse);
    assert!(dashboard.key(KeyCode::Char('[')) == ovrcr::tui::DashboardAction::Redraw);
    let resized = request_view(&mut dashboard, Rect::new(0, 0, 120, 44), revision);
    revision = set_view(&resized).revision;
    assert!(!matches!(
        dashboard.key(KeyCode::Char('y')),
        ovrcr::tui::DashboardAction::CopyText(_)
    ));
    acknowledge_all_view_targets(&mut dashboard, resized);
    assert_view_input_allowed(&mut dashboard);

    let shrink = request_view(&mut dashboard, Rect::new(0, 0, 50, 10), revision);
    let shrink_view = set_view(&shrink).clone();
    revision = shrink_view.revision;
    assert_eq!(shrink_view.panes.len(), 1);
    assert!(
        shrink_view
            .panes
            .iter()
            .all(|pane| pane.size.rows > 0 && pane.size.cols > 0)
    );
    let shrink_target = shrink_view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: shrink.request_id,
        response: Response::Screen {
            session: shrink_target.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: shrink_view.revision,
            size: shrink_target.size,
            bytes: b"SHRUNK".to_vec(),
        },
    });
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: shrink.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    assert_eq!(dashboard.pane_rects(Rect::new(0, 0, 50, 10)).len(), 1);
    assert!(drawn_contains(&dashboard, 50, 10, "SHRUNK"));

    let normal = request_view(&mut dashboard, area, revision);
    let normal_view = set_view(&normal).clone();
    revision = normal_view.revision;
    deliver_all_view_screens(&mut dashboard, normal.request_id, &normal_view);
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: normal.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    assert_eq!(dashboard.focused_session(), Some(focused));

    let empty = request_view(&mut dashboard, Rect::new(0, 0, 1, 1), revision);
    assert!(set_view(&empty).panes.is_empty());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: empty.request_id,
        response: Response::Ok,
    });
    assert_view_input_blocked(&mut dashboard);

    let mut dirty_copy = copy_ready_dashboard();
    assert_eq!(
        dirty_copy.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let dirty_outgoing =
        dirty_copy.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session: dirty_copy.focused_session().unwrap(),
            run: ovrcr_protocol::SessionRunId(1),
            revision: 1,
        }));
    assert!(
        dirty_outgoing
            .iter()
            .any(|msg| matches!(msg.request, Request::SetView { .. }))
            || dirty_outgoing.is_empty()
    );
    assert!(
        banner(&dirty_copy).contains("COPY") || rendered_footer(&dirty_copy, 88).contains("COPY")
    );

    tab_pane(&mut dashboard);
    assert_view_input_blocked(&mut dashboard);
    let focused_view = request_view(&mut dashboard, area, revision);
    acknowledge_all_view_targets(&mut dashboard, focused_view);
    dashboard.install_focus(SessionId(1));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert_view_input_allowed(&mut dashboard);
}

#[test]
fn split_review_preserves_nonfirst_survivor_on_hierarchy_removal() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    dashboard.install_focus(SessionId(2));
    let select = request_view(&mut dashboard, area, 0);
    acknowledge_view_request(&mut dashboard, select);
    split(&mut dashboard);
    let split_req = request_view(&mut dashboard, area, 1);
    let split_view = set_view(&split_req).clone();
    let survivor = split_view.panes[0].session;
    let removed = split_view.panes[1].session;
    let split_request_id = split_req.request_id;
    let split_revision = split_view.revision;
    acknowledge_all_view_targets(&mut dashboard, split_req);
    let mut hierarchy = fixture_hierarchy();
    for project in &mut hierarchy.projects {
        for workspace in &mut project.workspaces {
            workspace.sessions.retain(|session| session.id != removed);
        }
    }
    let outgoing = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::HierarchyChanged(hierarchy),
    ));
    assert_eq!(dashboard.pane_rects(area).len(), 1);
    assert_eq!(dashboard.focused_session(), Some(survivor));
    assert!(
        outgoing
            .iter()
            .any(|request| matches!(request.request, Request::SetView { .. }))
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: split_request_id,
        response: Response::Screen {
            session: removed,
            run: ovrcr_protocol::SessionRunId(1),
            revision: split_revision,
            size: TerminalSize { rows: 36, cols: 40 },
            bytes: b"removed old".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: removed,
        run: ovrcr_protocol::SessionRunId(1),
        revision: split_revision,
        bytes: b"removed output".to_vec(),
    }));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
        session: removed,
        run: ovrcr_protocol::SessionRunId(1),
        revision: split_revision,
    }));
    assert_ne!(dashboard.focused_session(), Some(removed));
    assert!(!drawn_contains(&dashboard, 120, 40, "removed"));
}

#[test]
fn split_review_requires_all_screens_before_ok_and_ignores_stale_view_completion() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    split(&mut dashboard);
    let request = request_view(&mut dashboard, area, 0);
    let view = set_view(&request).clone();
    let first = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id.saturating_add(1),
        response: Response::Screen {
            session: first.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: first.size,
            bytes: b"wrong request".to_vec(),
        },
    });
    assert!(!drawn_contains(&dashboard, 120, 40, "wrong request"));
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: first.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision.saturating_sub(1),
            size: first.size,
            bytes: b"wrong revision".to_vec(),
        },
    });
    assert!(!drawn_contains(&dashboard, 120, 40, "wrong revision"));
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: SessionId(5),
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: first.size,
            bytes: b"wrong session".to_vec(),
        },
    });
    assert!(!drawn_contains(&dashboard, 120, 40, "wrong session"));
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: first.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: first.size,
            bytes: b"first".to_vec(),
        },
    });
    assert_view_input_blocked(&mut dashboard);
    let second = view.panes[1].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: second.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: second.size,
            bytes: b"second".to_vec(),
        },
    });
    assert_view_input_blocked(&mut dashboard);
    for text in ["wrong request", "wrong revision", "wrong session"] {
        assert!(
            !drawn_contains(&dashboard, 120, 40, text),
            "stale payload installed: {text}"
        );
    }
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id.saturating_add(1),
        response: Response::Ok,
    });
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    let focused_before_narrow = dashboard.focused_session();

    let narrow = request_view(&mut dashboard, Rect::new(0, 0, 80, 40), view.revision);
    let narrow_view = set_view(&narrow).clone();
    let narrow_revision = narrow_view.revision;
    assert_eq!(narrow_view.panes.len(), 1);
    assert_eq!(narrow_view.focused, focused_before_narrow);
    dashboard.install_area(area);
    assert_view_input_blocked(&mut dashboard);
    let mut outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: narrow.request_id,
        response: Response::Ok,
    });
    assert_view_input_blocked(&mut dashboard);
    assert_eq!(outgoing.len(), 1);
    let latest = outgoing.pop().expect("latest desired view request");
    let latest_view = set_view(&latest).clone();
    assert_eq!(latest_view.revision, narrow_revision + 1);
    assert_eq!(latest_view.panes.len(), 2);
    assert_eq!(latest_view.focused, focused_before_narrow);
    let expected_targets = dashboard
        .pane_rects(area)
        .into_iter()
        .zip(latest_view.panes.iter())
        .map(|(rect, pane)| {
            (
                pane.session,
                TerminalSize {
                    rows: rect.terminal.height,
                    cols: rect.terminal.width,
                },
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        latest_view
            .panes
            .iter()
            .map(|pane| (pane.session, pane.size))
            .collect::<Vec<_>>(),
        expected_targets
    );
    assert_view_input_blocked(&mut dashboard);
    deliver_all_view_screens(&mut dashboard, narrow.request_id, &narrow_view);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: narrow.request_id,
        response: Response::Ok,
    });
    assert_view_input_blocked(&mut dashboard);
    deliver_all_view_screens(&mut dashboard, latest.request_id, &latest_view);
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: latest.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
}

#[test]
fn split_review_pending_view_completes_after_focus_and_geometry_return() {
    let mut focus_dashboard = dashboard_fixture();
    let area = wide_area();
    split(&mut focus_dashboard);
    let focus_view = request_view(&mut focus_dashboard, area, 0);
    let focus_revision = set_view(&focus_view).revision;
    deliver_all_view_screens(
        &mut focus_dashboard,
        focus_view.request_id,
        set_view(&focus_view),
    );
    tab_pane(&mut focus_dashboard);
    tab_pane(&mut focus_dashboard);
    focus_dashboard.handle_server_message(ServerMessage::Response {
        request_id: focus_view.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut focus_dashboard);
    let _ = focus_revision;

    let mut changed_focus_dashboard = dashboard_fixture();
    split(&mut changed_focus_dashboard);
    let changed_focus_view = request_view(&mut changed_focus_dashboard, area, 0);
    let changed_focus_revision = set_view(&changed_focus_view).revision;
    deliver_all_view_screens(
        &mut changed_focus_dashboard,
        changed_focus_view.request_id,
        set_view(&changed_focus_view),
    );
    tab_pane(&mut changed_focus_dashboard);
    let mut replacement = changed_focus_dashboard.handle_server_message(ServerMessage::Response {
        request_id: changed_focus_view.request_id,
        response: Response::Ok,
    });
    assert_eq!(replacement.len(), 1);
    assert_view_input_blocked(&mut changed_focus_dashboard);
    let replacement_request = replacement.pop().expect("focus replacement SetView");
    let replacement_view = set_view(&replacement_request);
    assert_eq!(replacement_view.revision, changed_focus_revision + 1);
    assert_eq!(replacement_view.focused, Some(SessionId(1)));
    acknowledge_all_view_targets(&mut changed_focus_dashboard, replacement_request);
    assert_view_input_allowed(&mut changed_focus_dashboard);

    let mut geometry_dashboard = dashboard_fixture();
    split(&mut geometry_dashboard);
    let geometry_view = request_view(&mut geometry_dashboard, area, 0);
    deliver_all_view_screens(
        &mut geometry_dashboard,
        geometry_view.request_id,
        set_view(&geometry_view),
    );
    geometry_dashboard.install_area(Rect::new(0, 0, 80, 40));
    geometry_dashboard.install_area(area);
    geometry_dashboard.handle_server_message(ServerMessage::Response {
        request_id: geometry_view.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut geometry_dashboard);
}

#[test]
fn split_review_pending_a_b_a_preserves_received_parser_before_ok() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 88, 38);
    dashboard.install_focus(SessionId(3));
    let initial = request_view(&mut dashboard, area, 0);
    let view = set_view(&initial).clone();
    let target = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Screen {
            session: target.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: target.size,
            bytes: b"PENDING_A\x1b[?1h\x1b[?2004h".to_vec(),
        },
    });
    dashboard.install_focus(SessionId(4));
    dashboard.install_focus(SessionId(3));
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
    assert_view_input_blocked(&mut dashboard);
    let replacement = outgoing.pop().expect("replacement SetView");
    let replacement_view = set_view(&replacement).clone();
    assert_eq!(replacement_view.panes.len(), 1);
    let replacement_target = replacement_view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Screen {
            session: replacement_target.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: replacement_view.revision,
            size: replacement_target.size,
            bytes: b"FRESH_A\x1b[?1h\x1b[?2004h".to_vec(),
        },
    });
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    assert!(drawn_contains(&dashboard, 88, 38, "FRESH_A"));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
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
    let area = Rect::new(0, 0, 88, 38);
    dashboard.install_focus(SessionId(3));
    let initial = request_view(&mut dashboard, area, 0);
    let initial_view = set_view(&initial).clone();
    let initial_revision = initial_view.revision;
    let initial_target = initial_view.panes[0].clone();

    dashboard.install_focus(SessionId(4));
    dashboard.install_focus(SessionId(3));
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));

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
    let replacement_view = set_view(&replacement).clone();
    assert_eq!(replacement_view.panes.len(), 1);
    assert_eq!(replacement_view.panes[0].session, SessionId(3));
    assert_eq!(replacement_view.revision, initial_revision + 1);
    assert_view_input_blocked(&mut dashboard);

    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Screen {
            session: initial_target.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: initial_revision,
            size: initial_target.size,
            bytes: b"ORIGINAL_A".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Ok,
    });
    assert_view_input_blocked(&mut dashboard);
    assert!(!drawn_contains(&dashboard, 88, 38, "ORIGINAL_A"));

    let replacement_target = replacement_view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Screen {
            session: replacement_target.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: replacement_view.revision,
            size: replacement_target.size,
            bytes: b"FRESH_A\x1b[?1h\x1b[?2004h".to_vec(),
        },
    });
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    assert!(drawn_contains(&dashboard, 88, 38, "FRESH_A"));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.event_action(Event::Paste("fresh".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[200~fresh\x1b[201~".to_vec())
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(b"x".to_vec())
    );
}

#[test]
fn split_review_absent_a_screen_cannot_authorize_reassigned_parser() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 88, 38);
    dashboard.install_focus(SessionId(3));
    let initial = request_view(&mut dashboard, area, 0);
    let view = set_view(&initial).clone();
    let target = view.panes[0].clone();
    dashboard.install_focus(SessionId(4));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Screen {
            session: target.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: target.size,
            bytes: b"ABSENT_A".to_vec(),
        },
    });
    dashboard.install_focus(SessionId(3));
    let mut outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        outgoing.len(),
        1,
        "absent A must request exactly one replacement"
    );
    assert_view_input_blocked(&mut dashboard);
    let replacement = outgoing.pop().expect("replacement SetView");
    let replacement_view = set_view(&replacement).clone();
    let replacement_target = replacement_view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Screen {
            session: replacement_target.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: replacement_view.revision,
            size: replacement_target.size,
            bytes: b"FRESH_ABSENT_A".to_vec(),
        },
    });
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    assert!(drawn_contains(&dashboard, 88, 38, "FRESH_ABSENT_A"));
}

#[test]
fn split_review_close_reopen_does_not_authorize_recreated_parser() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    dashboard.install_focus(SessionId(4));
    let initial = request_view(&mut dashboard, area, 0);
    acknowledge_view_request(&mut dashboard, initial);
    dashboard.install_area(area);
    split(&mut dashboard);
    dashboard.install_focus(SessionId(3));
    let initial = request_view(&mut dashboard, area, 1);
    let view = set_view(&initial).clone();
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
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: left.size,
            bytes: b"A_CONTENT".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: initial.request_id,
        response: Response::Screen {
            session: right.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: right.size,
            bytes: b"B_CONTENT\x1b[?1h\x1b[?2004h".to_vec(),
        },
    });

    close_pane(&mut dashboard);
    split(&mut dashboard);
    dashboard.install_focus(SessionId(5));
    dashboard.install_focus(right.session);
    assert_eq!(dashboard.focused_session(), Some(right.session));
    assert_view_input_blocked(&mut dashboard);

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
    let replacement_view = set_view(&replacement).clone();
    assert_eq!(
        replacement_view
            .panes
            .iter()
            .map(|pane| (pane.session, pane.size))
            .collect::<Vec<_>>(),
        vec![
            (left.session, TerminalSize { rows: 36, cols: 39 }),
            (right.session, TerminalSize { rows: 36, cols: 40 }),
        ]
    );
    assert_eq!(replacement_view.focused, Some(right.session));
    assert_view_input_blocked(&mut dashboard);

    for pane in &replacement_view.panes {
        let bytes = if pane.session == right.session {
            b"FRESH_B\x1b[?1h\x1b[?2004h".to_vec()
        } else {
            b"FRESH_A".to_vec()
        };
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: replacement.request_id,
            response: Response::Screen {
                session: pane.session,
                run: pane.run,
                revision: replacement_view.revision,
                size: pane.size,
                bytes,
            },
        });
    }
    assert_view_input_blocked(&mut dashboard);
    assert!(drawn_contains(&dashboard, 120, 40, "FRESH_B"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Up),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1bOA".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("fresh".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[200~fresh\x1b[201~".to_vec())
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(b"x".to_vec())
    );
}

#[test]
fn split_review_focus_select_and_close_revoke_public_input_until_setview_ok() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    dashboard.install_focus(SessionId(3));
    let first = request_view(&mut dashboard, area, 0);
    acknowledge_view_request(&mut dashboard, first);
    split(&mut dashboard);
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    dashboard.install_focus(SessionId(4));
    let second = request_view(&mut dashboard, area, 1);
    let view = set_view(&second).clone();
    assert_eq!(view.panes.len(), 2);
    assert!(view.panes.iter().any(|pane| pane.session == SessionId(3)));
    assert!(view.panes.iter().any(|pane| pane.session == SessionId(4)));
    let first_target = view.panes[0].clone();
    let second_target = view.panes[1].clone();
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::Screen {
            session: first_target.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: first_target.size,
            bytes: Vec::new(),
        },
    });
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::Screen {
            session: second_target.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: second_target.size,
            bytes: Vec::new(),
        },
    });
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second.request_id,
        response: Response::Ok,
    });
    assert_eq!(dashboard.pane_rects(area).len(), 2);
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    assert_view_input_allowed(&mut dashboard);

    tab_pane(&mut dashboard);
    assert_view_input_blocked(&mut dashboard);
    let refocus = request_view(&mut dashboard, area, view.revision);
    let refocus_view = set_view(&refocus).clone();
    let refocus_targets = refocus_view.panes.clone();
    assert_view_input_blocked(&mut dashboard);
    for pane in refocus_targets {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: refocus.request_id,
            response: Response::Screen {
                session: pane.session,
                run: ovrcr_protocol::SessionRunId(1),
                revision: refocus_view.revision,
                size: pane.size,
                bytes: Vec::new(),
            },
        });
        assert_view_input_blocked(&mut dashboard);
    }
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: refocus.request_id,
        response: Response::Ok,
    });
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
    assert_view_input_allowed(&mut dashboard);

    dashboard.install_focus(SessionId(4));
    let select_other = request_view(&mut dashboard, area, refocus_view.revision);
    let select_view = set_view(&select_other).clone();
    assert_eq!(select_view.focused, Some(SessionId(4)));
    assert_eq!(
        select_view
            .panes
            .iter()
            .map(|pane| pane.session)
            .collect::<Vec<_>>(),
        vec![SessionId(3), SessionId(4)]
    );
    assert_eq!(dashboard.pane_rects(area).len(), 2);
    let select_targets = select_view.panes.clone();
    assert_view_input_blocked(&mut dashboard);
    for pane in select_targets {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: select_other.request_id,
            response: Response::Screen {
                session: pane.session,
                run: ovrcr_protocol::SessionRunId(1),
                revision: select_view.revision,
                size: pane.size,
                bytes: Vec::new(),
            },
        });
        assert_view_input_blocked(&mut dashboard);
    }
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: select_other.request_id,
        response: Response::Ok,
    });
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    assert_view_input_allowed(&mut dashboard);

    close_pane(&mut dashboard);
    assert_eq!(dashboard.pane_rects(area).len(), 1);
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
    assert_view_input_blocked(&mut dashboard);
    let close_view = request_view(&mut dashboard, area, select_view.revision);
    assert!(matches!(close_view.request, Request::SetView { .. }));
    let close_set = set_view(&close_view).clone();
    assert_view_input_blocked(&mut dashboard);
    deliver_all_view_screens(&mut dashboard, close_view.request_id, &close_set);
    assert_view_input_blocked(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: close_view.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
}

#[test]
fn split_review_replaces_a_b_a_and_rejects_old_completions() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 88, 38);
    dashboard.install_focus(SessionId(4));
    let first_a = request_view(&mut dashboard, area, 0);
    let first_a_revision = set_view(&first_a).revision;
    let first_a_id = first_a.request_id;
    acknowledge_view_request(&mut dashboard, first_a);

    dashboard.install_focus(SessionId(3));
    let b = request_view(&mut dashboard, area, first_a_revision);
    let b_revision = set_view(&b).revision;
    let b_id = b.request_id;
    acknowledge_view_request(&mut dashboard, b);

    dashboard.install_focus(SessionId(4));
    let second_a = request_view(&mut dashboard, area, b_revision);
    assert_ne!(first_a_id, second_a.request_id);
    let current_revision = set_view(&second_a).revision;
    assert_ne!(first_a_revision, current_revision);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first_a_id,
        response: Response::Screen {
            session: SessionId(4),
            run: ovrcr_protocol::SessionRunId(1),
            revision: first_a_revision,
            size: TerminalSize { rows: 34, cols: 44 },
            bytes: b"old A".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: b_id,
        response: Response::Screen {
            session: SessionId(3),
            run: ovrcr_protocol::SessionRunId(1),
            revision: b_revision,
            size: TerminalSize { rows: 34, cols: 44 },
            bytes: b"old B".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(4),
        run: ovrcr_protocol::SessionRunId(1),
        revision: first_a_revision,
        bytes: b"stale output".to_vec(),
    }));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
        session: SessionId(4),
        run: ovrcr_protocol::SessionRunId(1),
        revision: first_a_revision,
    }));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first_a_id,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "old A error".into(),
        },
    });
    let rendered = rendered_rows(&dashboard, 88, 38);
    let without_footer = rendered[..rendered.len().saturating_sub(1)].join("\n");
    assert!(
        !without_footer.contains("old A"),
        "stale first A snapshot must not paint, got:\n{without_footer}"
    );
    assert!(
        !without_footer.contains("old B"),
        "stale B snapshot must not paint, got:\n{without_footer}"
    );
    assert_view_input_blocked(&mut dashboard);
    let dirty_outgoing =
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session: SessionId(4),
            run: ovrcr_protocol::SessionRunId(1),
            revision: first_a_revision,
        }));
    assert!(dirty_outgoing.is_empty());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first_a_id,
        response: Response::Ok,
    });
    assert!(banner(&dashboard).contains("Conflict: old A error"));
    assert!(!drawn_contains(&dashboard, 88, 38, "stale"));

    let size = dashboard
        .pane_rects(area)
        .first()
        .map(|pane| TerminalSize {
            rows: pane.terminal.height,
            cols: pane.terminal.width,
        })
        .unwrap();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second_a.request_id,
        response: Response::Screen {
            session: SessionId(4),
            run: ovrcr::protocol::SessionRunId(1),
            revision: current_revision,
            size,
            bytes: b"new A".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: second_a.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(b"x".to_vec())
    );
}

#[test]
fn split_review_matching_view_error_disables_input_until_a_new_view_is_ready() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    dashboard.install_focus(SessionId(2));
    let initial = request_view(&mut dashboard, area, 0);
    acknowledge_view_request(&mut dashboard, initial);
    split(&mut dashboard);
    let next = request_view(&mut dashboard, area, 1);
    let view = set_view(&next).clone();
    assert_eq!(view.panes.len(), 2);
    let target = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: next.request_id,
        response: Response::Screen {
            session: target.session,
            run: ovrcr_protocol::SessionRunId(1),
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
    assert!(banner(&dashboard).contains("Conflict: view refused"));
    assert_view_input_blocked(&mut dashboard);
    assert!(try_request_view(&mut dashboard, area, view.revision).is_none());
    std::thread::sleep(PAST_VIEW_RETRY_BACKOFF);
    let retry = request_view(&mut dashboard, area, view.revision);
    acknowledge_all_view_targets(&mut dashboard, retry);
    assert_view_input_allowed(&mut dashboard);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(b"x".to_vec())
    );
}

#[test]
fn view_error_on_unchanged_view_retries_once_after_backoff() {
    let mut dashboard = dashboard_fixture();
    let session = dashboard.focused_session().unwrap();
    let mut dirty =
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session,
            run: ovrcr::protocol::SessionRunId(1),
            revision: 0,
        }));
    assert_eq!(dirty.len(), 1);
    let refresh = dirty
        .pop()
        .expect("a dirty screen should re-request the same view");
    let refresh_revision = set_view(&refresh).revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: refresh.request_id,
        response: Response::Error {
            code: ErrorCode::NotFound,
            message: "session vanished".into(),
        },
    });
    assert!(banner(&dashboard).contains("NotFound: session vanished"));
    assert_view_input_blocked(&mut dashboard);
    let area = Rect::new(0, 0, 88, 38);
    for _ in 0..3 {
        assert!(
            try_request_view(&mut dashboard, area, refresh_revision).is_none(),
            "a refused view must wait out the backoff before it is re-sent"
        );
    }
    std::thread::sleep(PAST_VIEW_RETRY_BACKOFF);
    let retry = request_view(&mut dashboard, area, refresh_revision);
    assert!(matches!(retry.request, Request::SetView { .. }));
    acknowledge_view_request(&mut dashboard, retry);
    assert_view_input_allowed(&mut dashboard);
}

#[test]
fn view_error_on_changed_view_does_not_spin() {
    let mut dashboard = dashboard_fixture();
    let wider = wide_area();
    let changed = request_view(&mut dashboard, wider, 0);
    let changed_revision = set_view(&changed).revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: changed.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "pty refused".into(),
        },
    });
    for _ in 0..4 {
        assert!(
            try_request_view(&mut dashboard, wider, changed_revision).is_none(),
            "a changed view must not re-send SetView on every pass"
        );
    }
    std::thread::sleep(PAST_VIEW_RETRY_BACKOFF);
    let retry = request_view(&mut dashboard, wider, changed_revision);
    let retry_revision = set_view(&retry).revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: retry.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "pty refused".into(),
        },
    });
    assert!(
        try_request_view(&mut dashboard, wider, retry_revision).is_none(),
        "a second refusal must arm a new backoff window"
    );
}

#[test]
fn incomplete_final_ok_marks_panes_failed_and_refreshes() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    split(&mut dashboard);
    let request = request_view(&mut dashboard, area, 0);
    let view = set_view(&request).clone();
    assert_eq!(view.panes.len(), 2);
    let first = view.panes[0].clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: first.session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: first.size,
            bytes: b"only one snapshot".to_vec(),
        },
    });
    let mut outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert_view_input_blocked(&mut dashboard);
    assert_eq!(
        outgoing.len(),
        1,
        "a final Ok without every snapshot must refresh the view"
    );
    let refresh = outgoing.pop().expect("refreshed view request");
    let refreshed = set_view(&refresh).clone();
    assert_eq!(refreshed.revision, view.revision + 1);
    assert_eq!(refreshed.panes.len(), 2);
    deliver_all_view_screens(&mut dashboard, request.request_id, &view);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert_view_input_blocked(&mut dashboard);
    acknowledge_all_view_targets(&mut dashboard, refresh);
    assert_view_input_allowed(&mut dashboard);
}

#[test]
fn view_request_ids_are_released_on_final_response() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 88, 38);
    let mut revision = 0;
    for round in 0..10 {
        let session = if round % 2 == 0 {
            SessionId(5)
        } else {
            SessionId(1)
        };
        dashboard.install_focus(session);
        let request = request_view(&mut dashboard, area, revision);
        revision = set_view(&request).revision;
        acknowledge_view_request(&mut dashboard, request);
        assert_view_input_allowed(&mut dashboard);
    }
}

#[test]
fn split_error_survives_a_screen_dirty_refresh() {
    let mut dashboard = dashboard_fixture();
    let session = dashboard.focused_session().unwrap();
    let mut hierarchy = fixture_hierarchy();
    for project in &mut hierarchy.projects {
        for workspace in &mut project.workspaces {
            workspace
                .sessions
                .retain(|session| session.id == SessionId(1));
        }
    }
    dashboard.install_hierarchy(hierarchy);
    assert_eq!(
        dashboard.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(banner(&dashboard).contains("No other visible session to split"));
    let mut dirty =
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
            session,
            run: ovrcr::protocol::SessionRunId(1),
            revision: 0,
        }));
    assert_eq!(dirty.len(), 1);
    let refresh = dirty
        .pop()
        .expect("a dirty screen should re-request the view");
    acknowledge_view_request(&mut dashboard, refresh);
    assert!(
        banner(&dashboard).contains("No other visible session to split"),
        "a server-driven refresh must not clear a fresh error"
    );

    let refused = request_view(&mut dashboard, wide_area(), 1);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: refused.request_id,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "view refused".into(),
        },
    });
    assert!(banner(&dashboard).contains("Conflict: view refused"));
    std::thread::sleep(PAST_VIEW_RETRY_BACKOFF);
    let retry = request_view(&mut dashboard, wide_area(), set_view(&refused).revision);
    acknowledge_all_view_targets(&mut dashboard, retry);
    assert!(
        !banner(&dashboard).contains("ERROR:"),
        "a view failure banner is cleared by the view that succeeds"
    );
}

#[test]
fn split_error_is_cleared_by_the_view_the_next_selection_requests() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 88, 38);
    let rows = rendered_rows(&dashboard, 88, 38);
    let row = rows
        .iter()
        .position(|line| line.contains("auth"))
        .expect("auth workspace") as u16;
    dashboard.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            2,
            row,
            KeyModifiers::NONE,
        ),
        area,
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('v')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(banner(&dashboard).contains("No other visible session to split"));
    let request = (0..16)
        .find_map(|_| match dashboard.key(KeyCode::Char('j')) {
            ovrcr::tui::DashboardAction::Request(request) => Some(request),
            ovrcr::tui::DashboardAction::Redraw => None,
            action => panic!("a selection change should request a view: {action:?}"),
        })
        .expect("j/k should reach a visible session and request its view");
    acknowledge_view_request(&mut dashboard, request);
    assert!(
        !banner(&dashboard).contains("No other visible session to split"),
        "a banner must not outlive the view change the user asked for"
    );
}

#[test]
fn settings_error_survives_the_geometry_ack() {
    let mut dashboard = dashboard_fixture();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 9,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "settings: invalid TOML at line 3".into(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 2,
        response: Response::Ok,
    });
    assert!(banner(&dashboard).contains("settings: invalid TOML at line 3"));
}

#[test]
fn split_error_survives_mouse_cleanup_ack() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
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
    let mut hierarchy = fixture_hierarchy();
    for project in &mut hierarchy.projects {
        for workspace in &mut project.workspaces {
            workspace
                .sessions
                .retain(|session| session.id == SessionId(1));
        }
    }
    dashboard.install_hierarchy(hierarchy);
    split(&mut dashboard);
    assert!(banner(&dashboard).contains("No other visible session to split"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 1,
        response: Response::Ok,
    });
    assert!(
        banner(&dashboard).contains("No other visible session to split"),
        "a synthetic mouse release must not clear a fresh error"
    );
    let pause = match dashboard.key(KeyCode::Char('p')) {
        DashboardAction::Request(request) => request,
        action => panic!("pause should own the banner: {action:?}"),
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: pause.request_id,
        response: Response::Ok,
    });
    assert!(
        !banner(&dashboard).contains("ERROR:"),
        "an acknowledged user request owns the banner and clears it"
    );
}

#[test]
fn split_review_terminal_modes_use_each_panes_parser() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    dashboard.install_focus(SessionId(3));
    let initial = request_view(&mut dashboard, area, 0);
    acknowledge_view_request(&mut dashboard, initial);
    split(&mut dashboard);
    dashboard.install_focus(SessionId(4));
    let split_req = request_view(&mut dashboard, area, 1);
    let view = set_view(&split_req).clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: split_req.request_id,
        response: Response::Screen {
            session: view.panes[0].session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: view.panes[0].size,
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: split_req.request_id,
        response: Response::Screen {
            session: SessionId(4),
            run: ovrcr_protocol::SessionRunId(1),
            revision: view.revision,
            size: view.panes[1].size,
            bytes: b"\x1b[?1l\x1b[?2004h".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: split_req.request_id,
        response: Response::Ok,
    });
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Up),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[A".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("right".into())),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[200~right\x1b[201~".to_vec())
    );
    let focused_before = dashboard.focused_session();
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
    assert_eq!(dashboard.pane_rects(area).len(), 2);
    assert_eq!(dashboard.focused_session(), focused_before);

    assert_eq!(dashboard.ctrl('g'), DashboardAction::EnterBrowse);
    tab_pane(&mut dashboard);
    let left_view = request_view(&mut dashboard, area, view.revision);
    let left = set_view(&left_view).clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: left_view.request_id,
        response: Response::Screen {
            session: left.panes[0].session,
            run: ovrcr_protocol::SessionRunId(1),
            revision: left.revision,
            size: left.panes[0].size,
            bytes: b"\x1b[?1h".to_vec(),
        },
    });
    if left.panes.len() > 1 {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: left_view.request_id,
            response: Response::Screen {
                session: left.panes[1].session,
                run: ovrcr_protocol::SessionRunId(1),
                revision: left.revision,
                size: left.panes[1].size,
                bytes: Vec::new(),
            },
        });
    }
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: left_view.request_id,
        response: Response::Ok,
    });
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
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
    let area = wide_area();
    dashboard.install_focus(SessionId(2));
    let select = request_view(&mut dashboard, area, 0);
    acknowledge_view_request(&mut dashboard, select);
    split(&mut dashboard);
    let split_req = request_view(&mut dashboard, area, 1);
    let split_view = set_view(&split_req).clone();
    acknowledge_all_view_targets(&mut dashboard, split_req);
    tab_pane(&mut dashboard);
    let focus_a = request_view(&mut dashboard, area, split_view.revision);
    let revision = set_view(&focus_a).revision;
    acknowledge_all_view_targets(&mut dashboard, focus_a);
    let a = dashboard.focused_session().unwrap();
    let b = split_view
        .panes
        .iter()
        .find(|pane| pane.session != a)
        .map(|pane| pane.session)
        .expect("other pane");
    let begin = dashboard.key(KeyCode::PageUp);
    let begin_id = match begin {
        ovrcr::tui::DashboardAction::Request(ClientMessage {
            request: Request::HistoryBegin { session },
            request_id,
        }) if session == a => request_id,
        action => panic!("expected HistoryBegin for focused pane, got {action:?}"),
    };
    let opened = HistoryOpened {
        session: a,
        snapshot: HistorySnapshotId(77),
        revision,
        size: TerminalSize { rows: 34, cols: 39 },
        history_rows: 0,
        total_rows: 1,
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin_id,
        response: Response::HistoryOpened(opened.clone()),
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: b,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"B output".to_vec(),
    }));
    assert!(drawn_contains(&dashboard, 120, 40, "B output"));
    assert!(rendered_footer(&dashboard, 120).contains("HISTORY"));

    assert_eq!(
        dashboard.key(KeyCode::Esc),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    let begin = dashboard.key(KeyCode::PageUp);
    let begin_request_id = match begin {
        ovrcr::tui::DashboardAction::Request(request) => request.request_id,
        action => panic!("expected second history begin, got {action:?}"),
    };
    tab_pane(&mut dashboard);
    assert!(!rendered_footer(&dashboard, 120).contains("HISTORY"));
    let late = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin_request_id,
        response: Response::HistoryOpened(opened),
    });
    assert!(!rendered_footer(&dashboard, 120).contains("HISTORY"));
    assert!(late.iter().any(|request| matches!(
        request.request,
        Request::HistoryEnd {
            session: SessionId(2),
            ..
        }
    )));

    let focused_view = request_view(&mut dashboard, area, revision);
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
    assert!(rendered_footer(&dashboard, 120).contains("HISTORY"));
    tab_pane(&mut dashboard);
    assert!(!rendered_footer(&dashboard, 120).contains("HISTORY"));
}

#[test]
fn split_review_cancels_inflight_history_copy_before_focus_change() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    dashboard.install_focus(SessionId(3));
    let first = request_view(&mut dashboard, area, 0);
    acknowledge_view_request(&mut dashboard, first);
    split(&mut dashboard);
    dashboard.install_focus(SessionId(4));
    let split_req = request_view(&mut dashboard, area, 1);
    acknowledge_all_view_targets(&mut dashboard, split_req);
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
    tab_pane(&mut dashboard);
    assert!(!rendered_footer(&dashboard, 120).contains("HISTORY"));
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
    assert!(!banner(&dashboard).contains("Clipboard request sent"));
    assert!(!banner(&dashboard).contains("HISTORY"));
}

#[test]
fn split_review_unfocused_removal_preserves_focused_history_capture() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    dashboard.install_focus(SessionId(3));
    let first = request_view(&mut dashboard, area, 0);
    acknowledge_view_request(&mut dashboard, first);
    split(&mut dashboard);
    dashboard.install_focus(SessionId(4));
    let split_req = request_view(&mut dashboard, area, 1);
    let split_view = set_view(&split_req).clone();
    acknowledge_all_view_targets(&mut dashboard, split_req);
    tab_pane(&mut dashboard);
    let focused = request_view(&mut dashboard, area, split_view.revision);
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
    let removed = split_view
        .panes
        .iter()
        .find(|pane| pane.session != session)
        .map(|pane| pane.session)
        .expect("unfocused pane");
    let mut hierarchy = fixture_hierarchy();
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
    assert!(rendered_footer(&dashboard, 120).contains("HISTORY"));
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
    assert!(banner(&dashboard).contains("HISTORY"));
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
}

#[test]
fn split_state_ignores_stale_revisions_and_removed_sessions() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    split(&mut dashboard);
    let right = dashboard.focused_session().unwrap();
    let first = request_view(&mut dashboard, area, 0);
    let view = set_view(&first).clone();
    let revision = view.revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first.request_id,
        response: Response::Screen {
            session: right,
            run: ovrcr_protocol::SessionRunId(1),
            revision,
            size: TerminalSize { rows: 36, cols: 40 },
            bytes: b"first".to_vec(),
        },
    });
    dashboard.install_focus(SessionId(1));
    dashboard.install_focus(right);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: right,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"stale".to_vec(),
    }));
    assert!(!drawn_contains(&dashboard, 120, 40, "stale"));
    assert!(first.request_id > 0);
}

#[test]
fn split_preserves_history_copy_and_paused_input() {
    let mut dashboard = dashboard_fixture();
    let area = wide_area();
    let first = dashboard.focused_session().unwrap();
    let ready = request_view(&mut dashboard, area, 0);
    let revision = set_view(&ready).revision;
    let size = dashboard
        .pane_rects(area)
        .first()
        .map(|pane| TerminalSize {
            rows: pane.terminal.height,
            cols: pane.terminal.width,
        })
        .unwrap();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: ready.request_id,
        response: Response::Screen {
            session: first,
            run: ovrcr::protocol::SessionRunId(1),
            revision,
            size,
            bytes: b"A".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: ready.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        dashboard.key(KeyCode::Char('[')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(rendered_footer(&dashboard, 120).contains("COPY"));
    assert!(dashboard.split_pane());
    assert!(!rendered_footer(&dashboard, 120).contains("COPY"));
    let right = dashboard.focused_session().unwrap();
    let request = request_view(&mut dashboard, area, revision);
    let revision = set_view(&request).revision;
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: first,
            run: ovrcr_protocol::SessionRunId(1),
            revision,
            size: TerminalSize { rows: 36, cols: 39 },
            bytes: b"A".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: right,
            run: ovrcr_protocol::SessionRunId(1),
            revision,
            size: TerminalSize { rows: 36, cols: 40 },
            bytes: b"B".to_vec(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: right,
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        bytes: b"B output".to_vec(),
    }));
    let mut paused = fixture_session(right);
    paused.phase = SessionPhase::Paused;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        paused,
    ))));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(
        !matches!(
            dashboard.event_action(Event::Paste("blocked".into())),
            ovrcr::tui::DashboardAction::PtyBytes(_)
        ),
        "paused session must not forward paste"
    );
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(banner(&dashboard).contains("Session paused; press r to resume"));
}

#[test]
fn ux_narrow_layout_keeps_help_focus_and_nonzero_view_sizes() {
    for (width, height) in [(80, 24), (60, 20), (40, 12)] {
        for do_split in [false, true] {
            let mut dashboard = dashboard_fixture();
            let area = Rect::new(0, 0, width, height);
            dashboard.install_area(area);
            if do_split {
                split(&mut dashboard);
            }
            let request = request_view(&mut dashboard, area, 0);
            let view = set_view(&request);
            assert_eq!(
                view.panes.len(),
                1,
                "narrow split must show its focused pane"
            );
            for pane in &view.panes {
                assert!(pane.size.rows > 0 && pane.size.cols > 0);
            }
            assert_eq!(dashboard.pane_rects(area).len(), 1);
            acknowledge_all_view_targets(&mut dashboard, request);
            let rows = rendered_rows(&dashboard, width, height);
            assert!(
                rows.last().unwrap().contains("? Help"),
                "{width} split={do_split}: {rows:?}"
            );
            assert_view_input_allowed(&mut dashboard);
            let before = dashboard.focused_session();
            dashboard.key(KeyCode::Char('j'));
            assert_ne!(dashboard.focused_session(), before);
        }
    }
}

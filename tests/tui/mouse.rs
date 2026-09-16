//! Mouse encoding, forwarding, gestures, and wheel history.

use crate::*;

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

fn enable_mouse(dashboard: &mut Dashboard, modes: &[u8]) {
    let id = dashboard.focused_session().expect("focused session");
    dashboard.install_screen(id, modes);
    dashboard.key(KeyCode::Enter);
}

fn split_ready(area: Rect) -> Dashboard {
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    let action = dashboard.key(KeyCode::Char('v'));
    pump_view(&mut dashboard, action);
    let left = SessionId(1);
    let right = dashboard.focused_session().expect("split focus");
    dashboard.install_screen(left, &[]);
    dashboard.install_screen(right, &[]);
    dashboard
}

fn terminal_rect(dashboard: &Dashboard, area: Rect) -> Rect {
    let mut rects = dashboard.pane_rects(area);
    if rects.len() == 1 {
        return rects.remove(0).terminal;
    }
    rects
        .into_iter()
        .find(|pane| pane.pane_index == 1)
        .expect("focused split pane")
        .terminal
}

fn other_terminal(dashboard: &Dashboard, area: Rect) -> Rect {
    dashboard
        .pane_rects(area)
        .into_iter()
        .find(|pane| pane.pane_index == 0)
        .expect("unfocused pane")
        .terminal
}

#[test]
fn browse_click_in_pane_focuses_without_bytes() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let inner = terminal_rect(&dashboard, area);
    let action = dashboard.mouse_action(
        click_in(inner, MouseEventKind::Down(MouseButton::Left), 2, 3),
        area,
    );
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
    );
}

#[test]
fn browse_click_focuses_other_running_pane_and_types_after_view_ack() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = split_ready(area);
    let original = SessionId(1);
    let split = dashboard.focused_session().expect("split session");
    assert_ne!(split, original);

    let other = other_terminal(&dashboard, area);
    let action = dashboard.mouse_action(
        click_in(other, MouseEventKind::Down(MouseButton::Left), 2, 3),
        area,
    );

    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert_eq!(dashboard.focused_session(), Some(original));
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::Redraw
    );
    dashboard.install_screen(original, &[]);
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::PtyBytes(vec![b'x'])
    );
}

#[test]
fn terminal_click_focuses_other_pane_without_forwarding() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = split_ready(area);
    enable_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let original = SessionId(1);

    let other = other_terminal(&dashboard, area);
    let action = dashboard.mouse_action(
        click_in(other, MouseEventKind::Down(MouseButton::Left), 2, 3),
        area,
    );

    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert_eq!(dashboard.focused_session(), Some(original));
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::Redraw
    );
}

#[test]
fn terminal_mode_keeps_dashboard_mouse_capture_without_a_ready_session() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 20, cols: 80 });
    let area = Rect::new(0, 0, 80, 20);
    dashboard.install_area(area);
    assert!(dashboard.focused_session().is_none());
    assert_ne!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                50,
                10,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::PtyBytes(Vec::new())
    );
}

#[test]
fn press_outside_rectangle_is_not_clamped() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = terminal_rect(&dashboard, area);
    // The column left of the pane is the sidebar border: the dashboard takes the
    // press as a resize gesture instead of clamping it into the application.
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
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Up(MouseButton::Left),
                inner.x.saturating_sub(1),
                inner.y,
                KeyModifiers::NONE,
            ),
            area,
        ),
        ovrcr::tui::DashboardAction::Redraw
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
    enable_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = terminal_rect(&dashboard, area);
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
fn dragging_tracked_mouse_into_sidebar_releases_application_button() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = terminal_rect(&dashboard, area);
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
                10,
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
    enable_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = terminal_rect(&dashboard, area);
    dashboard.mouse_action(
        click_in(inner, MouseEventKind::Down(MouseButton::Left), 2, 3),
        area,
    );
    assert_eq!(
        dashboard.ctrl('g'),
        ovrcr::tui::DashboardAction::EnterBrowse
    );
    assert_eq!(
        dashboard.mouse_action(
            click_in(inner, MouseEventKind::Up(MouseButton::Left), 2, 3),
            area,
        ),
        ovrcr::tui::DashboardAction::None
    );
    assert!(
        !matches!(
            dashboard.key(KeyCode::Char('z')),
            ovrcr::tui::DashboardAction::PtyBytes(_)
        ),
        "browse after ctrl-g must not type into the PTY"
    );
}

#[test]
fn protocol_change_clears_held_state() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = terminal_rect(&dashboard, area);
    dashboard.mouse_action(
        click_in(inner, MouseEventKind::Down(MouseButton::Left), 2, 3),
        area,
    );
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: SessionId(1),
        run: ovrcr_protocol::SessionRunId(1),
        revision: 0,
        bytes: b"\x1b[?1002l".to_vec(),
    }));
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
    let inner = terminal_rect(&dashboard, area);
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
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(history_opened(80)),
    });
    assert!(
        outgoing.iter().any(|message| matches!(
            message.request,
            Request::HistoryPage { start_row, .. } if start_row > 0
        )),
        "history opened at the tail should page from a non-zero row, got {outgoing:?}"
    );
}

#[test]
fn wheel_down_at_newest_row_returns_to_live() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    let inner = terminal_rect(&dashboard, area);
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
}

#[test]
fn wheel_is_forwarded_when_tracking_is_enabled() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    enable_mouse(&mut dashboard, b"\x1b[?1000h\x1b[?1006h");
    let inner = terminal_rect(&dashboard, area);
    assert_eq!(
        dashboard.mouse_action(click_in(inner, MouseEventKind::ScrollUp, 2, 3), area),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<64;3;4M".to_vec())
    );
}

#[test]
fn wheel_up_over_the_unfocused_pane_opens_its_history() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = split_ready(area);
    let session_b = dashboard.focused_session().expect("unfocused pane session");
    let original = SessionId(1);
    dashboard.install_focus(original);
    dashboard.install_screen(original, &[]);
    assert_eq!(dashboard.focused_session(), Some(original));

    let unfocused = dashboard
        .pane_rects(area)
        .into_iter()
        .find(|pane| pane.pane_index == 1)
        .expect("unfocused split pane")
        .terminal;
    let action = dashboard.mouse_action(click_in(unfocused, MouseEventKind::ScrollUp, 2, 3), area);
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    assert_eq!(dashboard.focused_session(), Some(session_b));

    let replacement = dashboard
        .request_view_at(area)
        .expect("the wheel focus change should emit one SetView");
    dashboard.drain_outbox();
    let Request::SetView { ref view } = replacement.request else {
        panic!("expected SetView");
    };
    deliver_all_view_screens(&mut dashboard, replacement.request_id, view);
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: replacement.request_id,
        response: Response::Ok,
    });
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
    enable_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = terminal_rect(&dashboard, area);
    assert_eq!(
        dashboard.mouse_action(
            click_in(inner, MouseEventKind::Down(MouseButton::Left), 2, 3),
            area,
        ),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<0;3;4M".to_vec())
    );
    let mut paused = fixture_hierarchy()
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
    assert_eq!(
        dashboard.mouse_action(
            click_in(inner, MouseEventKind::Up(MouseButton::Left), 2, 3),
            area,
        ),
        ovrcr::tui::DashboardAction::None
    );
}

#[test]
fn focus_lost_finishes_gestures_and_focus_gained_resumes() {
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 88, 38);
    dashboard.install_area(area);
    enable_mouse(&mut dashboard, b"\x1b[?1002h\x1b[?1006h");
    let inner = terminal_rect(&dashboard, area);
    assert_eq!(
        dashboard.event_action(Event::Mouse(click_in(
            inner,
            MouseEventKind::Down(MouseButton::Left),
            2,
            3,
        ))),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<0;3;4M".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::FocusLost),
        ovrcr::tui::DashboardAction::Redraw
    );
    for kind in [
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::ScrollUp,
    ] {
        assert_eq!(
            dashboard.event_action(Event::Mouse(click_in(inner, kind, 4, 5))),
            ovrcr::tui::DashboardAction::None,
            "{kind:?} must be dropped while the terminal has no focus"
        );
    }
    assert_eq!(
        dashboard.event_action(Event::FocusGained),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.event_action(Event::Mouse(click_in(
            inner,
            MouseEventKind::Down(MouseButton::Left),
            4,
            5,
        ))),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<0;5;6M".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::Mouse(click_in(
            inner,
            MouseEventKind::Up(MouseButton::Left),
            4,
            5,
        ))),
        ovrcr::tui::DashboardAction::PtyBytes(b"\x1b[<0;5;6m".to_vec())
    );
}

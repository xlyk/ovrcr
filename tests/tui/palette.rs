//! Command palette, which-key popup, and the creation forms.

use crate::*;

fn extra_workspace_hierarchy(id: impl Into<String>) -> HierarchySnapshot {
    let id = id.into();
    let mut hierarchy = fixture_hierarchy();
    let project = hierarchy
        .projects
        .iter_mut()
        .find(|project| project.name == "consigint")
        .unwrap();
    let mut workspace = project.workspaces[1].clone();
    workspace.id = id.clone();
    workspace.name = "pick-demo".into();
    workspace.sessions.retain(|session| session.name == "local");
    workspace.sessions[0].id = SessionId(99);
    workspace.sessions[0].workspace = id;
    project.workspaces.push(workspace);
    hierarchy
}

fn install_picker_roots(dashboard: &mut Dashboard, roots: Vec<PathBuf>) {
    dashboard.install_settings(ovrcr::tui::Settings {
        picker_roots: roots,
        ..Default::default()
    });
}

fn footer_text(dashboard: &Dashboard) -> String {
    rendered_footer(dashboard, 120)
}

#[test]
fn leader_workspace_uses_inspection_and_name_first_picker_defaults() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('p'));
    let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Char('n')) else {
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
    // macOS also offers "Set up iTerm focus" before the existing Focus hint.
    // Select the typing action; the fixture's screen already acknowledges it.
    #[cfg(target_os = "macos")]
    dashboard.key(KeyCode::Down);
    assert!(palette_text(&dashboard).contains("› Focus"));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        ovrcr::tui::DashboardAction::PtyBytes(b"z".to_vec())
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "late inspection".into(),
        },
    });
    assert!(
        !footer_text(&dashboard).contains("late inspection"),
        "closed palette must discard its inspection response"
    );
}

#[test]
fn space_w_n_opens_the_terminal_form() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(' '));
    let text = palette_text(&dashboard);
    assert!(text.contains("Which key"));
    assert!(text.contains("w  Workspace"));
    dashboard.key(KeyCode::Char('w'));
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
    for _ in 0..4 {
        dashboard.key(KeyCode::Down);
    }
    dashboard.key(KeyCode::Enter);
    let text = palette_text(&dashboard);
    assert!(!text.contains("Which key"));
    assert!(text.contains("Register project"));
    assert!(text.contains("Repository"));
    assert!(text.contains("Workspace root"));
}

#[test]
fn question_mark_popup_lists_ready_sound_beside_desktop_notifications() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('?'));
    let text = palette_text(&dashboard);
    assert!(text.contains("Enable desktop notifications"), "{text}");
    assert!(text.contains("Enable ready sound"), "{text}");
    dashboard.key(KeyCode::Char('S'));
    assert!(!palette_text(&dashboard).contains("Which key"));
    // The toggle applies when the Server's reading of the saved value arrives.
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
        Box::new(ovrcr::protocol::SettingsReport {
            path: "/srv/dashboard.toml".into(),
            read_unix_ms: 0,
            settings: ovrcr::protocol::Settings {
                ready_sound: true,
                ..Default::default()
            },
            rows: Vec::new(),
            findings: Vec::new(),
            unparseable: false,
        }),
    )));
    dashboard.key(KeyCode::Char('?'));
    let text = palette_text(&dashboard);
    assert!(text.contains("Disable ready sound"), "{text}");
    assert!(text.contains("Enable desktop notifications"), "{text}");
}

#[test]
fn whichkey_compact_corner_preserves_the_surrounding_dashboard() {
    for (width, height) in [(80, 40), (160, 60)] {
        let mut dashboard = dashboard_fixture();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| draw_dashboard_at(f, &dashboard, 0))
            .unwrap();
        let before = terminal.backend().buffer().clone();
        dashboard.key(KeyCode::Char(' '));
        terminal
            .draw(|f| draw_dashboard_at(f, &dashboard, 0))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let area = whichkey_test_bounds(buffer);
        assert!(
            area.width <= 48,
            "menu must leave room for the dashboard: {area:?}"
        );
        assert!(area.height <= 30, "menu height must be bounded: {area:?}");
        assert_eq!(area.right(), width - 2);
        assert_eq!(area.bottom(), height - 2);
        for y in 0..height {
            for x in 0..width {
                if !area.contains(Position::new(x, y)) {
                    assert_eq!(
                        buffer[(x, y)],
                        before[(x, y)],
                        "background changed at {x},{y}"
                    );
                }
            }
        }
    }
}

fn whichkey_test_bounds(buffer: &ratatui::buffer::Buffer) -> Rect {
    let corner = |symbol: &str| {
        (0..buffer.area.height)
            .find_map(|y| {
                (0..buffer.area.width).find_map(|x| {
                    let cell = &buffer[(x, y)];
                    (cell.symbol() == symbol && cell.fg == Color::Rgb(203, 166, 247))
                        .then_some((x, y))
                })
            })
            .expect("popup border must be visible")
    };
    let (x, y) = corner("┌");
    let (right, bottom) = corner("┘");
    Rect::new(x, y, right - x + 1, bottom - y + 1)
}

#[test]
fn whichkey_scrolled_last_row_remains_clickable_after_resize() {
    for (width, height) in [(80, 40), (40, 16), (20, 12), (20, 8)] {
        let mut dashboard = dashboard_fixture();
        dashboard.key(KeyCode::Char(' '));
        for _ in 0..40 {
            dashboard.mouse_action(
                MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: width - 4,
                    row: height - 4,
                    modifiers: KeyModifiers::NONE,
                },
                Rect::new(0, 0, width, height),
            );
        }
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| draw_dashboard_at(f, &dashboard, 0))
            .unwrap();
        let cursor = terminal.backend().cursor_position();
        assert_eq!(
            terminal.backend().buffer()[(cursor.x, cursor.y)].symbol(),
            "q"
        );
        let action = dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: cursor.x,
                row: cursor.y,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, width, height),
        );
        assert_eq!(action, ovrcr::tui::DashboardAction::Detach);
    }
}

#[test]
fn narrow_popup_moves_description_to_detail_line() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('?'));
    dashboard.key(KeyCode::Char('p')); // project submenu: workspace creation
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
        .position(|line| line.contains("n  Create workspace"))
        .unwrap();
    assert!(!lines[row].contains("consigint"));
    let detail = lines
        .iter()
        .position(|line| line.contains("Create a worktree"))
        .unwrap();
    assert!(detail > row);
    assert!(lines[detail..].join("\n").contains("consigint"));
    let area = whichkey_test_bounds(terminal.backend().buffer());
    assert!(
        lines[usize::from(area.bottom() - 2)].contains("opens a form"),
        "full description must reach the last inner popup line"
    );
    dashboard.key(KeyCode::Backspace);
    dashboard.install_unready(dashboard.focused_session().unwrap());
    dashboard.key(KeyCode::Char('t'));
    terminal
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let area = whichkey_test_bounds(terminal.backend().buffer());
    let focus_row = (area.y + 1..area.bottom() - 1)
        .find(|y| {
            (area.x + 1..area.right() - 1)
                .map(|x| terminal.backend().buffer()[(x, *y)].symbol())
                .collect::<String>()
                .contains("Enter  Focus")
        })
        .unwrap();
    assert!(
        terminal.backend().buffer()[(area.x + 1, focus_row)]
            .modifier
            .contains(Modifier::DIM)
    );
    let detail = (area.y + 1..area.bottom() - 1)
        .map(|y| {
            (area.x + 1..area.right() - 1)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(detail.contains("waiting for acknowledged screen"));
}

#[test]
fn empty_hierarchy_shows_start_screen() {
    let dashboard = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    let text = palette_text(&dashboard);
    assert!(text.contains("Welcome to OVRCR"));
    assert!(text.contains("a  Register project"));
    assert!(text.contains(":  Search"));
    assert!(text.contains("?  Help"));
    assert!(text.contains("Home:"));
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
fn keyboard_overlays_cancel_divider_gesture_before_consuming_release() {
    use ovrcr::tui::DashboardAction;

    let cases = [
        (
            "palette",
            KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE),
        ),
        (
            "which-key",
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        ),
        (
            "tasks",
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
        ),
    ];
    for (name, key) in cases {
        let area = Rect::new(0, 0, 120, 40);
        let mut dashboard = dashboard_fixture();
        dashboard.key(KeyCode::Char('v'));
        dashboard.install_area(area);
        let rects = dashboard.pane_rects(area);
        assert_eq!(rects.len(), 2, "{name}");
        let focused = if rects.len() == 1 {
            rects[0].terminal
        } else {
            rects
                .iter()
                .find(|pane| pane.pane_index != 0)
                .unwrap_or(&rects[1])
                .terminal
        };
        let divider = dashboard
            .pane_rects(area)
            .first()
            .expect("left pane")
            .terminal
            .right();

        assert_eq!(
            dashboard.event_action(Event::Mouse(mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                divider,
                10,
                KeyModifiers::NONE,
            ))),
            DashboardAction::Redraw,
            "{name} setup"
        );
        assert_ne!(
            dashboard.event_action(Event::Key(key)),
            DashboardAction::None,
            "{name} keyboard intent"
        );
        dashboard.event_action(Event::Mouse(mouse_event(
            MouseEventKind::Up(MouseButton::Left),
            60,
            20,
            KeyModifiers::NONE,
        )));
        dashboard.event_action(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));

        assert_eq!(
            dashboard.event_action(Event::Mouse(click_in(
                focused,
                MouseEventKind::Down(MouseButton::Left),
                2,
                3,
            ))),
            DashboardAction::Redraw,
            "{name} must release divider ownership"
        );
        assert_eq!(
            dashboard.event_action(Event::Mouse(click_in(
                focused,
                MouseEventKind::Drag(MouseButton::Left),
                3,
                3,
            ))),
            DashboardAction::None,
            "{name} post-dismissal drag"
        );
        assert_eq!(dashboard.pane_rects(area).len(), 2, "{name}");
    }

    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    let session = dashboard.focused_session().unwrap();
    dashboard.install_screen(session, b"\x1b[?1002h\x1b[?1006h");
    dashboard.key(KeyCode::Enter);
    let focused = dashboard
        .pane_rects(area)
        .into_iter()
        .next()
        .expect("focused pane rect")
        .terminal;
    assert_eq!(
        dashboard.mouse_action(
            click_in(focused, MouseEventKind::Down(MouseButton::Right), 2, 3,),
            area,
        ),
        DashboardAction::PtyBytes(b"\x1b[<2;3;4M".to_vec())
    );
    assert!(matches!(
        dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                105,
                0,
                KeyModifiers::NONE,
            ),
            area,
        ),
        DashboardAction::Request(_)
    ));
    assert!(
        palette_text(&dashboard).contains("Search"),
        "opening Actions should capture the overlay"
    );
}

#[test]
fn tasks_hotkey_cancels_pending_history_like_control_t() {
    let mut dashboard = dashboard_fixture();
    let begin = dashboard.key(KeyCode::PageUp);
    assert!(matches!(begin, ovrcr::tui::DashboardAction::Request(_)));
    dashboard.key(KeyCode::Char('t'));
    assert!(
        palette_text(&dashboard).contains("OVRCR  Tasks"),
        "tasks hotkey must open the tasks overlay"
    );
    if let ovrcr::tui::DashboardAction::Request(message) = begin {
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: message.request_id,
            response: Response::HistoryOpened(history_opened(20)),
        });
        assert!(
            palette_text(&dashboard).contains("OVRCR  Tasks"),
            "cancelled history must not replace the tasks overlay"
        );
    }
}

#[test]
fn whichkey_preserves_readiness_and_never_forwards_overlay_input() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
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
    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
    dashboard.ctrl('g');
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('t'));
    dashboard.key(KeyCode::Char('r')); // running session: resume is absent
    assert!(palette_text(&dashboard).contains("Which key"));
    dashboard.key(KeyCode::Esc);
    assert!(footer_text(&dashboard).contains("BROWSE"));
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('t'));
    dashboard.key(KeyCode::Enter);
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
    dashboard.key(KeyCode::Char(' '));
    assert!(palette_text(&dashboard).contains("Selection"));
    dashboard.key(KeyCode::Char('v'));
    assert!(!palette_text(&dashboard).contains("Which key"));
    assert_eq!(dashboard.pane_rects(Rect::new(0, 0, 88, 38)).len(), 1);
    dashboard.key(KeyCode::Char('?'));
    dashboard.key(KeyCode::Esc);
    assert!(!palette_text(&dashboard).contains("Which key"));
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Char(' '));
    dashboard.ctrl('g');
    assert!(
        !palette_text(&dashboard).contains("Selection"),
        "leader Ctrl-g must run the listed Browse action"
    );
    assert!(footer_text(&dashboard).contains("BROWSE"));
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
    let area = whichkey_test_bounds(terminal.backend().buffer());
    assert_eq!(cursor, Position::new(area.x + 1, area.y + 2));
    assert_eq!(
        terminal.backend().buffer()[(cursor.x, cursor.y)].symbol(),
        "h"
    );
}

#[test]
fn whichkey_empty_workspace_click_sets_terminal_form_target() {
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[0].workspaces.push(WorkspaceSummary {
        project: "spacelift-agent".into(),
        name: "empty".into(),
        id: "empty".into(),
        root: false,
        warning: None,
        path: "/tmp/empty".into(),
        sessions: vec![],
    });
    dashboard.install_hierarchy(hierarchy);
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(palette_text(&dashboard).contains("press n to start a terminal here"));
    dashboard.key(KeyCode::Char('n'));
    assert!(palette_text(&dashboard).contains("spacelift-agent / empty"));
}

#[test]
fn whichkey_tiny_layouts_and_last_disabled_detail_remain_browsable() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(3));
    dashboard.key(KeyCode::Char('?'));
    dashboard.key(KeyCode::Char('t'));
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
    assert!(
        whichkey_menu_text(&dashboard).contains("waiting for acknowledged screen"),
        "{text}"
    );
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
    assert!(text.contains("Close terminal?"));
    assert!(text.contains("review (#1)"));
    assert_eq!(dashboard.pane_rects(Rect::new(0, 0, 88, 38)).len(), 1);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("close confirmation did not submit");
    };
    assert_eq!(
        message.request,
        Request::CloseTerminal {
            session: SessionId(1),
            expected_run: ovrcr::protocol::SessionRunId(1),
        }
    );
}

#[test]
fn palette_filters_and_captures_input_without_sending_it_to_terminal() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "create terminal");
    let mut helper_dashboard = dashboard_fixture();
    palette_search(&mut helper_dashboard, "create terminal");
    assert_ne!(
        helper_dashboard.event_action(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE
        ))),
        DashboardAction::PtyBytes(b"x".to_vec())
    );
    assert_ne!(
        helper_dashboard.event_action(Event::Paste("blocked by palette".into())),
        DashboardAction::PtyBytes(b"blocked by palette".to_vec())
    );
    assert!(palette_text(&dashboard).contains("Create terminal"));
    assert!(
        !palette_text(&dashboard)
            .lines()
            .any(|line| line.contains("│") && line.contains("Register project"))
    );
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
    assert!(palette_text(&dashboard).contains("Close terminal?"));
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
            session: SessionId(3),
            expected_run: ovrcr::protocol::SessionRunId(1),
        }
    );
}

#[test]
fn palette_forms_build_workspace_and_project_requests_and_draw_at_small_sizes() {
    use ovrcr::protocol::Request;
    use ovrcr::tui::DashboardAction;
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let cases = [
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
            vec!["consigint / auth"],
            Request::RemoveWorkspace {
                project: "consigint".into(),
                name: "auth".into(),
                force: false,
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
        for (index, value) in values.iter().enumerate() {
            dashboard.ctrl('u');
            dashboard.event_action(Event::Paste((*value).into()));
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
fn create_workspace_form_submits_branch_without_name() {
    use ovrcr::protocol::{BranchRequest, Request};
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "create workspace");
    dashboard.key(KeyCode::Enter);
    let text = palette_text(&dashboard);
    assert!(text.contains("Branch"), "{text}");
    assert!(!text.contains("Name"), "{text}");
    dashboard.key(KeyCode::BackTab);
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("consigint".into()));
    dashboard.key(KeyCode::Tab);
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("feature/palette".into()));
    dashboard.key(KeyCode::Tab); // mode
    dashboard.key(KeyCode::Tab); // base
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("main".into()));
    dashboard.key(KeyCode::BackTab); // mode
    dashboard.key(KeyCode::BackTab); // branch
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("create workspace did not submit");
    };
    let Request::CreateWorkspaceWithLaunch {
        project,
        id,
        branch,
        launch,
    } = message.request
    else {
        panic!("expected create workspace: {:?}", message.request);
    };
    assert_eq!(project, "consigint");
    assert_workspace_id(&id);
    assert_eq!(
        branch,
        BranchRequest::New {
            branch: "feature/palette".into(),
            base: "main".into(),
        }
    );
    assert!(launch.is_some());
}

#[test]
fn create_workspace_existing_branch_keeps_generated_id() {
    use ovrcr::protocol::{BranchRequest, Request};
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "create workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Tab); // mode
    dashboard.key(KeyCode::Char(' ')); // existing
    dashboard.key(KeyCode::BackTab); // branch
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("topic".into()));
    let DashboardAction::Request(first) = dashboard.key(KeyCode::Enter) else {
        panic!("existing branch did not submit");
    };
    let Request::CreateWorkspaceWithLaunch {
        id: first_id,
        branch,
        ..
    } = &first.request
    else {
        panic!("{:?}", first.request);
    };
    assert_workspace_id(first_id);
    assert_eq!(
        branch,
        &BranchRequest::Existing {
            branch: "topic".into()
        }
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "retry me".into(),
        },
    });
    let DashboardAction::Request(retry) = dashboard.key(KeyCode::Enter) else {
        panic!("retry did not submit");
    };
    let Request::CreateWorkspaceWithLaunch { id: retry_id, .. } = retry.request else {
        panic!("{:?}", retry.request);
    };
    assert_eq!(retry_id, *first_id);
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
    assert!(!text.contains("Name"), "{text}");
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("Enter on Branch must submit with defaults");
    };
    let Request::CreateWorkspaceWithLaunch {
        project,
        id,
        branch,
        launch,
    } = message.request
    else {
        panic!("{:?}", message.request);
    };
    assert_eq!(project, "consigint");
    assert_workspace_id(&id);
    assert_eq!(
        branch,
        BranchRequest::New {
            branch: "feature/pick-demo".into(),
            base: "main".into()
        }
    );
    assert!(launch.is_some());
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
    let deadline = std::time::Instant::now() + wait_deadline();
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
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("custom/kept".into()));
    dashboard.key(KeyCode::Tab); // mode
    dashboard.key(KeyCode::BackTab); // branch
    assert!(palette_text(&dashboard).contains("custom/kept"));
    dashboard.key(KeyCode::Tab); // mode
    dashboard.key(KeyCode::Right); // existing
    dashboard.key(KeyCode::BackTab); // branch pick
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
    let Request::CreateWorkspaceWithLaunch {
        project,
        id,
        branch,
        launch,
    } = message.request
    else {
        panic!("{:?}", message.request);
    };
    assert_eq!(project, "consigint");
    assert_workspace_id(&id);
    assert_eq!(
        branch,
        ovrcr::protocol::BranchRequest::Existing {
            branch: "topic".into()
        }
    );
    assert!(launch.is_some());
}

#[test]
fn typed_existing_branch_survives_hint_arrival() {
    use ovrcr::protocol::BranchRequest;
    let repo = hint_repository(&["release-2"]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "release-2");
    answer_workspace_inspect(&mut dashboard, inspect_id, repo.path());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let message = loop {
        dashboard.poll_palette();
        let mut drained = dashboard.drain_outbox();
        let text = palette_text(&dashboard);
        // Without real branches the field would stay text and submit vacuously.
        assert!(!text.contains("enter branch/base manually"), "{text}");
        if !drained.is_empty() {
            break drained.remove(0);
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Git hints never reached the deferred submit: {text}"
        );
        std::thread::yield_now();
    };
    let Request::CreateWorkspaceWithLaunch {
        project,
        id,
        branch,
        launch,
    } = message.request
    else {
        panic!("expected create workspace: {:?}", message.request);
    };
    assert_eq!(project, "consigint");
    assert_workspace_id(&id);
    assert_eq!(
        branch,
        BranchRequest::Existing {
            branch: "release-2".into()
        }
    );
    assert!(launch.is_some());
}

#[test]
fn fuzzy_matched_branch_is_not_submitted_without_confirmation() {
    use ovrcr::protocol::BranchRequest;
    use ovrcr::tui::DashboardAction;
    let repo = hint_repository(&["release-2-old"]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "release-2");
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
    let Request::CreateWorkspaceWithLaunch {
        project,
        id,
        branch,
        launch,
    } = message.request
    else {
        panic!("expected create workspace: {:?}", message.request);
    };
    assert_eq!(project, "consigint");
    assert_workspace_id(&id);
    assert_eq!(
        branch,
        BranchRequest::Existing {
            branch: "release-2-old".into()
        }
    );
    assert!(launch.is_some());
}

#[test]
fn case_only_branch_mismatch_is_refused_not_submitted() {
    // Git refs are case-sensitive, so `Release-2` is a different branch from the `release-2`
    // the user typed: creating the workspace on it would silently use the wrong branch.
    let repo = hint_repository(&["Release-2"]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "release-2");
    answer_workspace_inspect(&mut dashboard, inspect_id, repo.path());
    let text = poll_until_deferred_submit_refused(
        &mut dashboard,
        "a branch that differs only in case must not be submitted",
    );
    assert!(text.contains("Create workspace"), "{text}");
    assert!(text.contains("Branch not found in repository"), "{text}");
    assert!(text.contains("Release-2"), "{text}");
}

#[test]
fn typed_branch_missing_from_hints_is_refused_not_replaced() {
    let repo = hint_repository(&[]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "release-2");
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
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    let action = dashboard.key(KeyCode::Char('w'));
    answer_palette_inspect(&mut dashboard, action);
    dashboard.event_action(Event::Paste("pick-demo".into()));
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("workspace form did not submit");
    };
    let Request::CreateWorkspaceWithLaunch { id, .. } = &message.request else {
        panic!("{:?}", message.request);
    };
    let created_id = id.clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Ok,
    });
    assert!(palette_text(&dashboard).contains("Working"));
    assert!(palette_text(&dashboard).contains("┌ Create workspace"));
    let mut hierarchy = extra_workspace_hierarchy(created_id);
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
    assert!(
        !text.lines().any(|line| line.contains("┌ Create workspace")),
        "{text}"
    );
    assert!(!text.contains("Working"), "{text}");
    assert!(!outgoing.iter().any(
        |message| matches!(&message.request, Request::SetView { view } if view.focused == Some(SessionId(99)))
    ));
    assert_ne!(dashboard.focused_session(), Some(SessionId(99)));
    assert!(footer_text(&dashboard).contains("BROWSE"));
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
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: 9,
        response: Response::Error {
            code: ErrorCode::NotFound,
            message: "session 99 not found".into(),
        },
    });
    assert!(footer_text(&dashboard).contains("session 99 not found"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: inspect.request_id,
        response: Response::Inventory {
            registry: Default::default(),
            sessions: vec![],
        },
    });
    assert!(footer_text(&dashboard).contains("session 99 not found"));
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
    assert!(palette_text(&dashboard).contains("┌ Create workspace"));
    let hierarchy = extra_workspace_hierarchy("pick-demo");
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
    assert!(footer_text(&dashboard).contains("BROWSE"));
    assert!(
        !palette_text(&dashboard)
            .lines()
            .any(|line| line.contains("┌ Create workspace"))
    );
}

#[test]
fn workspace_creation_attaches_to_its_local_shell() {
    use ovrcr::tui::DashboardAction;
    for hierarchy_first in [true, false] {
        let mut dashboard = dashboard_fixture();
        let action = dashboard.key(KeyCode::Char('w'));
        answer_palette_inspect(&mut dashboard, action);
        dashboard.event_action(Event::Paste("pick-demo".into()));
        let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
            panic!("workspace form did not submit");
        };
        let created_id = match &message.request {
            Request::CreateWorkspaceWithLaunch { id, .. } => id.clone(),
            other => panic!("expected create workspace: {other:?}"),
        };
        let event = ServerMessage::Event(ServerEvent::HierarchyChanged(extra_workspace_hierarchy(
            created_id,
        )));
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
        assert_eq!(dashboard.key(KeyCode::Char('z')), DashboardAction::Redraw);
        assert_eq!(outgoing.len(), 1);
        assert!(
            matches!(&outgoing[0].request, Request::SetView { view } if view.focused == Some(SessionId(99)))
        );
        acknowledge_view_request(&mut dashboard, outgoing[0].clone());
        assert_eq!(
            dashboard.key(KeyCode::Char('z')),
            DashboardAction::PtyBytes(b"z".to_vec())
        );
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
    assert!(text.contains("Start"), "{text}");
    assert!(text.contains("Terminal"), "{text}");
}

#[test]
fn created_session_enters_terminal_mode() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('n'));
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("form did not submit");
    };
    let Request::CreateSession(request) = &message.request else {
        panic!("wrong request");
    };
    let session = SessionSummary {
        cwd: "/work".into(),
        archived: false,
        id: SessionId(99),
        run: ovrcr::protocol::SessionRunId(1),
        kind: ovrcr::protocol::SessionKind::Terminal,
        recovery: None,
        project: request.project.clone(),
        workspace: request.workspace.clone(),
        name: request.name.clone(),
        title: None,
        manual_title: None,
        label: request.label.clone().unwrap_or_default(),
        pid: Some(1),
        started_unix_ms: Some(0),
        phase: SessionPhase::Running,
        activity: AgentActivity::Unknown,
        context_usage: None,
        agent: None,
        agent_epoch: 0,
        unread: None,
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::CreatedSession(Box::new(session)),
    });
    assert_eq!(dashboard.focused_session(), Some(SessionId(99)));
    assert!(!palette_text(&dashboard).contains("┌ Create terminal"));
}

#[test]
fn browse_footer_lists_only_menu_search_and_help() {
    let dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let footer: String = (0..120)
        .map(|x| terminal.backend().buffer()[(x, 39)].symbol())
        .collect();
    assert_eq!(
        footer.trim_end(),
        "BROWSE  Space Menu  : Search  ? Help",
        "{footer}"
    );
}

#[test]
fn n_is_listed_in_the_hint_table() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('?'));
    dashboard.key(KeyCode::Char('w'));
    let text = palette_text(&dashboard);
    assert!(text.contains("Which key"), "{text}");
    let create = text
        .lines()
        .skip_while(|line| !line.contains("Workspace:"))
        .take_while(|line| !line.contains("Remove workspace"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        create.contains("n  Create terminal"),
        "the Workspace group must list n: {create}"
    );
    let detail = text
        .lines()
        .skip_while(|line| !line.contains("Choose an agent or shell"))
        .take_while(|line| !line.contains("└ Esc close"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        detail.contains("consigint / auth"),
        "the selected n hint must name its workspace below the list: {detail}"
    );
    dashboard.key(KeyCode::Enter);
    let form = palette_text(&dashboard);
    assert!(!form.contains("Which key"), "{form}");
    assert!(form.contains("Agent"), "{form}");
    assert!(form.contains("Workspace"), "{form}");
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
    install_picker_roots(&mut dashboard, vec![root]);
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
    install_picker_roots(&mut dashboard, vec![dir.path().to_path_buf()]);
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
    install_picker_roots(&mut dashboard, vec![dir.path().to_path_buf()]);
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
    install_picker_roots(&mut dashboard, vec![root.clone()]);
    dashboard.install_config_dir(dir.path().join("config"));
    std::fs::create_dir_all(dir.path().join("config").join("workspaces").join("demo")).unwrap();
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
    let mut hierarchy = fixture_hierarchy();
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
    install_picker_roots(&mut dashboard, Vec::new());
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
    dashboard.install_screen(SessionId(1), b"removed terminal output");
    palette_search(&mut dashboard, "close terminal");
    dashboard.key(KeyCode::Enter);
    let ovrcr::tui::DashboardAction::Request(close) = dashboard.key(KeyCode::Enter) else {
        panic!("close missing");
    };
    let mut hierarchy = fixture_hierarchy();
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
    let drawn = palette_text(&dashboard);
    assert!(!drawn.contains("removed terminal output"), "{drawn}");
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
    assert!(palette_text(&dashboard).contains("┌ Register project"));
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
    assert!(!palette_text(&dashboard).contains("┌ Register project"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "late failure".into(),
        },
    });
    assert!(!palette_text(&dashboard).contains("┌ Register project"));
    assert!(!footer_text(&dashboard).contains("late failure"));
}

#[test]
fn nested_whichkey_terminal_group_waits_for_action_and_backspace_returns() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(' '));
    assert_eq!(dashboard.key(KeyCode::Char('t')), DashboardAction::Redraw);
    assert!(
        !palette_text(&dashboard).contains("OVRCR  Tasks"),
        "group key must not open tasks"
    );
    let text = palette_text(&dashboard);
    assert!(text.contains("Space t"), "{text}");
    assert!(text.contains("Terminal: review"), "{text}");
    assert!(text.contains("p  Pause"));
    assert!(!text.contains("r  Resume"));
    assert!(text.contains("x  Close terminal"));
    dashboard.key(KeyCode::Backspace);
    let text = palette_text(&dashboard);
    assert!(text.contains("t  Terminal"));
    assert!(!text.contains("x  Close terminal"));
    dashboard.key(KeyCode::Char('t'));
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Char('p')) else {
        panic!("pause must emit a request");
    };
    assert_eq!(
        message.request,
        Request::PauseSession {
            session: SessionId(1)
        }
    );
}

#[test]
fn nested_whichkey_removal_confirms_the_selected_target_and_can_cancel() {
    use ovrcr::tui::DashboardAction;
    for (group, expected) in [
        (
            't',
            Request::CloseTerminal {
                session: SessionId(1),
                expected_run: ovrcr::protocol::SessionRunId(1),
            },
        ),
        (
            'w',
            Request::RemoveWorkspace {
                project: "consigint".into(),
                name: "auth".into(),
                force: false,
            },
        ),
        (
            'p',
            Request::RemoveProject {
                name: "consigint".into(),
            },
        ),
    ] {
        let mut dashboard = dashboard_fixture();
        for confirm in [false, true] {
            dashboard.key(KeyCode::Char(' '));
            dashboard.key(KeyCode::Char(group));
            assert_eq!(dashboard.key(KeyCode::Char('x')), DashboardAction::Redraw);
            if group != 't' {
                assert!(!palette_text(&dashboard).contains("Confirm action"));
                assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
            }
            let text = palette_text(&dashboard);
            let title = if group == 't' {
                "Close terminal?"
            } else {
                "Confirm action"
            };
            assert!(text.contains(title), "{group}: {text}");
            assert!(text.contains("consigint"));
            if group == 'w' {
                assert!(text.contains("Archive stopped sessions"), "{text}");
                assert!(text.contains("original paths"), "{text}");
            }
            if confirm {
                let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
                    panic!("confirmation must emit removal request");
                };
                assert_eq!(message.request, expected);
            } else {
                assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
                let dismissed = palette_text(&dashboard);
                assert!(!dismissed.contains("Confirm action"));
                assert!(!dismissed.contains("Close terminal?"));
                assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
            }
        }
    }
}

fn whichkey_menu_text(dashboard: &Dashboard) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|f| draw_dashboard_at(f, dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let area = whichkey_test_bounds(buffer);
    (area.y + 1..area.bottom() - 1)
        .map(|y| {
            (area.x + 1..area.right() - 1)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn click_whichkey_text(
    dashboard: &mut Dashboard,
    text: &str,
    sidebar: bool,
) -> ovrcr::tui::DashboardAction {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|f| draw_dashboard_at(f, dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let area = if sidebar {
        Rect::new(0, 1, 35, 37)
    } else {
        whichkey_test_bounds(buffer)
    };
    let row = (area.y..area.bottom())
        .find(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, *y)].symbol())
                .collect::<String>()
                .contains(text)
        })
        .expect("target row must be rendered");
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + if sidebar { 4 } else { 2 },
            row,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    )
}

#[test]
fn nested_whichkey_container_selection_limits_groups_and_removal_target() {
    use ovrcr::tui::DashboardAction;
    for (row, group, expected) in [
        (
            "SPACELIFT-AGENT",
            'p',
            Request::RemoveProject {
                name: "spacelift-agent".into(),
            },
        ),
        (
            "progress",
            'w',
            Request::RemoveWorkspace {
                project: "spacelift-agent".into(),
                name: "progress".into(),
                force: false,
            },
        ),
    ] {
        let mut dashboard = dashboard_fixture();
        click_whichkey_text(&mut dashboard, row, true);
        assert_eq!(dashboard.focused_session(), None);
        dashboard.key(KeyCode::Char(' '));
        let menu = whichkey_menu_text(&dashboard);
        assert!(!menu.contains("t  Terminal"), "{menu}");
        assert!(menu.contains("p  Project"));
        assert_eq!(menu.contains("w  Workspace"), group == 'w');
        dashboard.key(KeyCode::Char(group));
        dashboard.key(KeyCode::Char('x'));
        assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
        assert!(palette_text(&dashboard).contains("Confirm action"));
        let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
            panic!("removal must require confirmation");
        };
        assert_eq!(message.request, expected);
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: message.request_id,
            response: Response::Error {
                code: ErrorCode::Conflict,
                message: "still contains terminals".into(),
            },
        });
        assert!(palette_text(&dashboard).contains("still contains terminals"));
        assert!(palette_text(&dashboard).contains("spacelift-agent"));
    }
}

#[test]
fn nested_whichkey_mouse_and_enter_open_groups_before_running_actions() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('?'));
    dashboard.key(KeyCode::Enter);
    assert!(whichkey_menu_text(&dashboard).contains("Terminal: review"));
    dashboard.key(KeyCode::Backspace);
    click_whichkey_text(&mut dashboard, "w  Workspace", false);
    assert!(whichkey_menu_text(&dashboard).contains("Workspace: consigint / auth"));
    click_whichkey_text(&mut dashboard, "n  Create terminal", false);
    assert!(palette_text(&dashboard).contains("Agent"));
    assert!(palette_text(&dashboard).contains("consigint / auth"));
}

#[test]
fn nested_whichkey_live_phase_updates_pause_resume_and_ignores_invalid_keys() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('t'));
    let mut session = session_summary(
        1,
        "consigint",
        "auth",
        "review",
        "claude",
        Some(111),
        u64::MAX,
    );
    session.phase = SessionPhase::Paused;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        session,
    ))));
    let menu = whichkey_menu_text(&dashboard);
    assert!(menu.contains("r  Resume"), "{menu}");
    assert!(!menu.contains("p  Pause"));
    assert!(!menu.contains("Enter  Focus"));
    assert_eq!(dashboard.key(KeyCode::Char('p')), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key_action(KeyEvent::new_with_kind(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
            KeyEventKind::Repeat
        )),
        DashboardAction::None
    );
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT)),
        DashboardAction::Redraw
    );
    assert!(whichkey_menu_text(&dashboard).contains("r  Resume"));
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Char('r')) else {
        panic!("resume must emit a request");
    };
    assert_eq!(
        message.request,
        Request::ResumeSession {
            session: SessionId(1)
        }
    );
}

#[test]
fn nested_whichkey_empty_hierarchy_can_register_and_open_global_view() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    dashboard.key(KeyCode::Char(' '));
    let menu = whichkey_menu_text(&dashboard);
    for absent in ["t  Terminal", "w  Workspace", "p  Project"] {
        assert!(!menu.contains(absent), "{menu}");
    }
    dashboard.key(KeyCode::Char('v'));
    assert!(whichkey_menu_text(&dashboard).contains("Tasks"));
    dashboard.key(KeyCode::Char('t'));
    assert!(palette_text(&dashboard).contains("OVRCR  Tasks"));
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('a'));
    assert!(palette_text(&dashboard).contains("Repository"));
}

#[test]
fn ux_browse_footer_is_stable_across_widths_and_state() {
    let mut dashboard = dashboard_fixture();
    let full = "BROWSE  Space Menu  : Search  ? Help";
    assert_eq!(rendered_footer(&dashboard, 120).trim_end(), full);
    for width in [20, 40, 80, 120] {
        let text = rendered_footer(&dashboard, width);
        assert!(text.starts_with("BROWSE  Space Menu"), "{text}");
        assert!(full.starts_with(text.trim_end()), "partial hint: {text}");
    }
    // Selection and an empty hierarchy change the menu, not the footer.
    dashboard.install_focus(SessionId(3));
    assert_eq!(rendered_footer(&dashboard, 120).trim_end(), full);
    dashboard.install_hierarchy(HierarchySnapshot { projects: vec![] });
    assert_eq!(rendered_footer(&dashboard, 120).trim_end(), full);
}

#[test]
fn ux_search_keeps_session_identity_and_filters_complete_context() {
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "local");
    for width in [40, 80, 120] {
        let rows = rendered_rows(&dashboard, width, 24);
        for id in [3, 4, 5] {
            assert!(
                rows.iter()
                    .any(|row| row.contains(&format!("local (#{id})"))),
                "{rows:?}"
            );
        }
        assert!(
            rows.iter()
                .any(|row| row.contains("Project: spacelift-agent")),
            "{rows:?}"
        );
    }
    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Enter);
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    for query in ["spacelift-agent", "progress", "#3"] {
        palette_search(&mut dashboard, query);
        dashboard.key(KeyCode::Enter);
        assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
    }
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[0].workspaces[0].sessions[0].name = "界🙂".repeat(50);
    dashboard.install_hierarchy(hierarchy);
    palette_search(&mut dashboard, "#3");
    for width in [40, 80, 120] {
        let rows = rendered_rows(&dashboard, width, 24);
        let row = rows
            .iter()
            .find(|row| row.contains("(#3)"))
            .expect("visible ID");
        assert!(row.contains("… (#3)"), "{row}");
        assert!(row.contains('界'), "{row}");
        assert!(row.trim_end().ends_with('│'), "border clipped: {row}");
    }
    dashboard.event_action(Event::Paste("zzzz".into()));
    let rows = rendered_rows(&dashboard, 80, 24);
    assert!(
        rows.iter()
            .any(|row| row.contains("No matching actions or terminals")),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("Backspace delete")),
        "{rows:?}"
    );
}

#[test]
fn ux_forms_put_task_name_in_border_and_keep_active_field_visible() {
    for (key, title) in [
        ('n', "Create terminal"),
        ('w', "Create workspace"),
        ('a', "Register project"),
    ] {
        let mut dashboard = dashboard_fixture();
        dashboard.key(KeyCode::Char(key));
        for (width, height) in [(40, 12), (80, 24)] {
            let rows = rendered_rows(&dashboard, width, height);
            assert!(
                rows.iter()
                    .any(|row| row.contains('┌') && row.contains(title)),
                "{rows:?}"
            );
            assert!(
                rows.iter().any(|row| row.contains('›')),
                "active field missing: {rows:?}"
            );
        }
    }
}

#[test]
fn removal_pickers_filter_and_confirm_a_different_target() {
    for (query, filter, expected) in [
        (
            "remove workspace",
            "spa / pro",
            Request::RemoveWorkspace {
                project: "spacelift-agent".into(),
                name: "progress".into(),
                force: false,
            },
        ),
        (
            "remove project",
            "spa",
            Request::RemoveProject {
                name: "spacelift-agent".into(),
            },
        ),
    ] {
        let mut dashboard = dashboard_fixture();
        palette_search(&mut dashboard, query);
        dashboard.key(KeyCode::Enter);
        assert!(palette_text(&dashboard).contains("spacelift-agent"));
        dashboard.event_action(Event::Paste(filter.into()));
        assert_eq!(
            dashboard.key(KeyCode::Enter),
            ovrcr::tui::DashboardAction::Redraw
        );
        let text = palette_text(&dashboard);
        assert!(text.contains("Confirm action"), "{text}");
        assert!(text.contains("spacelift-agent"));
        let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
            panic!("confirmation must submit the picked target");
        };
        assert_eq!(message.request, expected);
    }
}

#[test]
fn removal_picker_rejects_no_match_and_excludes_root() {
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces.push(WorkspaceSummary {
        project: "consigint".into(),
        name: "main".into(),
        id: "root-id".into(),
        root: true,
        warning: None,
        path: "/tmp/root".into(),
        sessions: vec![],
    });
    hierarchy.projects[1].workspaces.push(WorkspaceSummary {
        project: "consigint".into(),
        name: "root".into(),
        id: "feature-root".into(),
        root: false,
        warning: None,
        path: "/tmp/feature-root".into(),
        sessions: vec![],
    });
    dashboard.install_hierarchy(hierarchy);
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    let text = palette_text(&dashboard);
    assert!(!text.contains("consigint / main"), "{text}");
    assert!(text.contains("consigint / root"), "{text}");
    dashboard.event_action(Event::Paste("main".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(!palette_text(&dashboard).contains("Confirm action"));
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("no-such-workspace".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(!palette_text(&dashboard).contains("Confirm action"));
    dashboard.key(KeyCode::Esc);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
}

#[test]
fn removal_pickers_with_empty_hierarchy_never_confirm() {
    for query in ["remove project", "remove workspace"] {
        let mut dashboard = dashboard_fixture();
        dashboard.install_hierarchy(HierarchySnapshot { projects: vec![] });
        palette_search(&mut dashboard, query);
        dashboard.key(KeyCode::Enter);
        for key in [KeyCode::Down, KeyCode::Tab, KeyCode::Enter] {
            assert!(!matches!(
                dashboard.key(key),
                ovrcr::tui::DashboardAction::Request(_)
            ));
        }
        let text = palette_text(&dashboard);
        assert!(text.contains("No matches"), "{text}");
        assert!(!text.contains("Confirm action"));
    }
}

#[test]
fn picker_validation_keeps_recovery_hints_and_cancels_without_removal() {
    for query in ["remove workspace", "remove project"] {
        for (width, height) in [(120, 40), (40, 12)] {
            let mut dashboard = dashboard_fixture();
            palette_search(&mut dashboard, query);
            dashboard.key(KeyCode::Enter);
            dashboard.ctrl('u');
            dashboard.event_action(Event::Paste("missing".into()));
            assert_eq!(
                dashboard.key(KeyCode::Enter),
                ovrcr::tui::DashboardAction::Redraw
            );
            let text = rendered_rows(&dashboard, width, height).join("\n");
            assert!(text.contains("not found"), "{text}");
            assert!(text.contains("Esc cancel"), "{text}");
            assert!(text.contains("Ctrl-u clear"), "{text}");
            assert!(!text.contains("Confirm action"), "{text}");

            assert_eq!(dashboard.ctrl('u'), ovrcr::tui::DashboardAction::Redraw);
            dashboard.event_action(Event::Paste("spacelift".into()));
            let text = palette_text(&dashboard);
            assert!(text.contains("spacelift-agent"), "{text}");
            assert!(!text.contains("not found"), "{text}");
            assert_eq!(
                dashboard.key(KeyCode::Esc),
                ovrcr::tui::DashboardAction::Redraw
            );
            assert!(!palette_text(&dashboard).contains("Remove workspace"));
            assert!(!palette_text(&dashboard).contains("Remove project"));
            assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
        }
    }
}

#[test]
fn manual_title_events_update_sidebar_pane_and_palette_without_identity_change() {
    let mut dashboard = dashboard_fixture();
    let hierarchy = fixture_hierarchy();
    let mut sessions = hierarchy
        .projects
        .iter()
        .flat_map(|p| &p.workspaces)
        .flat_map(|w| &w.sessions)
        .filter(|s| s.id == SessionId(1) || s.id == SessionId(3))
        .cloned()
        .collect::<Vec<_>>();
    for session in &mut sessions {
        session.title = Some("Same title".into());
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
            Box::new(session.clone()),
        )));
    }
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
    terminal
        .draw(|f| draw_dashboard_at(f, &dashboard, 0))
        .unwrap();
    let rendered = (0..40)
        .map(|y| {
            (0..160)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("Same title (#1)"), "{rendered}");
    assert!(rendered.contains("Same title (#3)"), "{rendered}");
    assert!(
        rendered.matches("Same title (#1)").count() >= 2,
        "title appears in sidebar and pane: {rendered}"
    );
    let mut narrow = Terminal::new(TestBackend::new(80, 30)).unwrap();
    narrow
        .draw(|f| draw_dashboard_at(f, &dashboard, 0))
        .unwrap();
    let narrow_title = (40..80)
        .map(|x| narrow.backend().buffer()[(x, 2)].symbol())
        .collect::<String>();
    assert!(
        narrow_title.contains("Same title (#1)"),
        "title must remain visible at 80 columns: {narrow_title}"
    );
    palette_search(&mut dashboard, "Same title");
    let text = palette_text(&dashboard);
    assert!(text.contains("(#1)") && text.contains("(#3)"), "{text}");
    dashboard.key(KeyCode::Esc);
    palette_search(&mut dashboard, "Rename terminal");
    dashboard.key(KeyCode::Enter);
    dashboard.ctrl('u');
    let rename_text = palette_text(&dashboard);
    assert!(rename_text.contains("original name"), "{rename_text}");
    assert!(!rename_text.contains("Automatic"), "{rename_text}");
    dashboard.event_action(Event::Paste("Pinned by user".into()));
    let ovrcr::tui::DashboardAction::Request(pin) = dashboard.key(KeyCode::Enter) else {
        panic!()
    };
    assert_eq!(
        pin.request,
        Request::SetSessionTitle {
            session: SessionId(1),
            title: Some("Pinned by user".into())
        }
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: pin.request_id,
        response: Response::Ok,
    });
    palette_search(&mut dashboard, "Rename terminal");
    dashboard.key(KeyCode::Enter);
    dashboard.ctrl('u');
    let ovrcr::tui::DashboardAction::Request(reset) = dashboard.key(KeyCode::Enter) else {
        panic!()
    };
    assert_eq!(
        reset.request,
        Request::SetSessionTitle {
            session: SessionId(1),
            title: None
        }
    );
}

#[test]
fn empty_workspace_creation_selects_its_empty_state_in_either_response_order() {
    use ovrcr::tui::DashboardAction;
    for hierarchy_first in [false, true] {
        let mut dashboard = dashboard_fixture();
        let action = dashboard.key(KeyCode::Char('w'));
        answer_palette_inspect(&mut dashboard, action);
        dashboard.event_action(Event::Paste("pick-demo".into()));
        for _ in 0..3 {
            dashboard.key(KeyCode::Tab);
        }
        dashboard.ctrl('u');
        dashboard.event_action(Event::Paste("Nothing yet".into()));
        let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
            panic!("empty workspace form did not submit")
        };
        let Request::CreateWorkspaceWithLaunch { id, launch, .. } = &message.request else {
            panic!("{:?}", message.request);
        };
        assert!(launch.is_none());
        let created_id = id.clone();
        let mut hierarchy = extra_workspace_hierarchy(created_id.clone());
        let workspace = hierarchy
            .projects
            .iter_mut()
            .find(|p| p.name == "consigint")
            .unwrap()
            .workspaces
            .last_mut()
            .unwrap();
        workspace.name = "feature/pick-demo".into();
        workspace.sessions.clear();
        let event = ServerMessage::Event(ServerEvent::HierarchyChanged(hierarchy));
        let ack = ServerMessage::Response {
            request_id: message.request_id,
            response: Response::Ok,
        };
        let outgoing = if hierarchy_first {
            dashboard.handle_server_message(event);
            assert_eq!(
                dashboard.focused_session(),
                Some(SessionId(1)),
                "unacknowledged creation must not change selection"
            );
            dashboard.handle_server_message(ack)
        } else {
            dashboard.handle_server_message(ack);
            assert_eq!(
                dashboard.focused_session(),
                Some(SessionId(1)),
                "wait until the created workspace exists in hierarchy"
            );
            dashboard.handle_server_message(event)
        };
        assert_eq!(
            dashboard.focused_session(),
            None,
            "created empty workspace must replace the previous pane selection"
        );
        assert!(outgoing.iter().any(|m| matches!(&m.request, Request::SetView { view } if view.focused.is_none() && view.panes.is_empty())));
        let text = palette_text(&dashboard);
        assert!(
            text.contains("consigint / feature/pick-demo: press n to start a terminal here"),
            "{text}"
        );
        assert!(!text.contains("┌ Create workspace"), "{text}");
        dashboard.key(KeyCode::Char('n'));
        assert!(palette_text(&dashboard).contains("consigint / feature/pick-demo"));
    }
}

#[test]
fn reopen_confirm_names_stopped_processes_and_does_not_treat_retry_as_ack() {
    use ovrcr::tui::DashboardAction;
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[0].sessions[0].recovery =
        Some(ovrcr::protocol::SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: None,
        });
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(2));
    palette_search(&mut dashboard, "Reopen in a fresh shell");
    dashboard.key(KeyCode::Enter);
    let text = palette_text(&dashboard);
    assert!(
        text.contains("Previous agent or background processes"),
        "{text}"
    );
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("confirming reopen must send a request");
    };
    assert_eq!(
        message.request,
        Request::ReopenSession {
            session: SessionId(2),
            expected_run: ovrcr::protocol::SessionRunId(1),
            acknowledge_stopped: true,
        }
    );
}

#[test]
fn failed_reopen_retries_the_current_run_from_the_confirm() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(2));
    palette_search(&mut dashboard, "Reopen in a fresh shell");
    dashboard.key(KeyCode::Enter);
    let DashboardAction::Request(first) = dashboard.key(KeyCode::Enter) else {
        panic!("reopen confirm must send");
    };
    assert_eq!(
        first.request,
        Request::ReopenSession {
            session: SessionId(2),
            expected_run: ovrcr::protocol::SessionRunId(1),
            acknowledge_stopped: false,
        }
    );
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[0].sessions[0].run = ovrcr::protocol::SessionRunId(4);
    hierarchy.projects[1].workspaces[0].sessions[0].recovery =
        Some(ovrcr::protocol::SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: false,
            unavailable: None,
            failure: Some("cwd missing".into()),
        });
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "reopen failed".into(),
        },
    });
    let DashboardAction::Request(retry) = dashboard.key(KeyCode::Enter) else {
        panic!("failed reopen must offer an explicit retry");
    };
    assert_eq!(
        retry.request,
        Request::ReopenSession {
            session: SessionId(2),
            expected_run: ovrcr::protocol::SessionRunId(4),
            acknowledge_stopped: false,
        }
    );
}

#[test]
fn ownership_uncertain_create_does_not_create_another_row_on_enter() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('n'));
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("form did not submit");
    };
    let Request::CreateSession(request) = &message.request else {
        panic!("wrong request");
    };
    let retained = SessionSummary {
        cwd: "/work".into(),
        archived: false,
        id: SessionId(99),
        run: ovrcr::protocol::SessionRunId(3),
        kind: ovrcr::protocol::SessionKind::Terminal,
        recovery: Some(ovrcr::protocol::SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: Some("spawn uncertain".into()),
        }),
        project: request.project.clone(),
        workspace: request.workspace.clone(),
        name: request.name.clone(),
        title: None,
        manual_title: None,
        label: request.label.clone().unwrap_or_default(),
        pid: None,
        started_unix_ms: None,
        phase: SessionPhase::Stopped,
        activity: AgentActivity::Unknown,
        context_usage: None,
        agent: None,
        agent_epoch: 0,
        unread: None,
    };
    let mut hierarchy = fixture_hierarchy();
    if let Some(workspace) = hierarchy
        .projects
        .iter_mut()
        .find(|project| project.name == retained.project)
        .and_then(|project| {
            project
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.name == retained.workspace)
        })
    {
        workspace.sessions.push(retained.clone());
    } else {
        panic!("create form must target a fixture workspace");
    }
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Error {
            code: ErrorCode::OwnershipUncertain,
            message: "process ownership is uncertain".into(),
        },
    });
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    let text = palette_text(&dashboard);
    assert!(
        text.contains("previous agent or background processes") || text.contains("Acknowledge"),
        "{text}"
    );
    if let DashboardAction::Request(next) = dashboard.key(KeyCode::Enter) {
        match next.request {
            Request::CreateSession(_) | Request::AcknowledgeSessionStopped { .. } => {
                panic!("uncertain create must not replay creation or acknowledge a guessed row")
            }
            other => panic!("unexpected request {other:?}"),
        }
    }
    dashboard.install_focus(SessionId(99));
    palette_search(&mut dashboard, "Acknowledge stopped without reopening");
    dashboard.key(KeyCode::Enter);
    let DashboardAction::Request(next) = dashboard.key(KeyCode::Enter) else {
        panic!("explicit ack after selecting the retained row");
    };
    assert_eq!(
        next.request,
        Request::AcknowledgeSessionStopped {
            session: SessionId(99),
            expected_run: ovrcr::protocol::SessionRunId(3),
        }
    );
    let _ = retained;
}

#[test]
fn ownership_uncertain_create_does_not_guess_among_two_rows() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    let focused = dashboard.focused_session();
    dashboard.key(KeyCode::Char('n'));
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("form did not submit");
    };
    let Request::CreateSession(request) = &message.request else {
        panic!("wrong request");
    };
    let mut hierarchy = fixture_hierarchy();
    let workspace = hierarchy
        .projects
        .iter_mut()
        .find(|project| project.name == request.project)
        .and_then(|project| {
            project
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.name == request.workspace)
        })
        .expect("create form must target a fixture workspace");
    for id in [98, 99] {
        workspace.sessions.push(SessionSummary {
            cwd: "/work".into(),
            archived: false,
            id: SessionId(id),
            run: ovrcr::protocol::SessionRunId(3),
            kind: ovrcr::protocol::SessionKind::Terminal,
            recovery: Some(ovrcr::protocol::SessionRecovery {
                conversation: None,
                attached: false,
                requires_ack: true,
                unavailable: None,
                failure: Some("spawn uncertain".into()),
            }),
            project: request.project.clone(),
            workspace: request.workspace.clone(),
            name: format!("uncertain-{id}"),
            title: None,
            manual_title: None,
            label: request.label.clone().unwrap_or_default(),
            pid: None,
            started_unix_ms: None,
            phase: SessionPhase::Stopped,
            activity: AgentActivity::Unknown,
            context_usage: None,
            agent: None,
            agent_epoch: 0,
            unread: None,
        });
    }
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Error {
            code: ErrorCode::OwnershipUncertain,
            message: "process ownership is uncertain".into(),
        },
    });
    assert_eq!(dashboard.focused_session(), focused);
    if let DashboardAction::Request(next) = dashboard.key(KeyCode::Enter) {
        match next.request {
            Request::CreateSession(_) | Request::AcknowledgeSessionStopped { .. } => {
                panic!("uncertain create must not replay creation or acknowledge a guessed row")
            }
            other => panic!("unexpected request {other:?}"),
        }
    }
}

#[test]
fn reopen_confirm_keeps_captured_run_when_hierarchy_advances() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(2));
    palette_search(&mut dashboard, "Reopen in a fresh shell");
    dashboard.key(KeyCode::Enter);
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[0].sessions[0].run = ovrcr::protocol::SessionRunId(4);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    let DashboardAction::Request(first) = dashboard.key(KeyCode::Enter) else {
        panic!("confirming reopen must send a request");
    };
    assert_eq!(
        first.request,
        Request::ReopenSession {
            session: SessionId(2),
            expected_run: ovrcr::protocol::SessionRunId(1),
            acknowledge_stopped: false,
        }
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first.request_id,
        response: Response::Error {
            code: ErrorCode::Conflict,
            message: "run moved".into(),
        },
    });
    let DashboardAction::Request(second) = dashboard.key(KeyCode::Enter) else {
        panic!("Conflict must wait for another Enter");
    };
    assert_eq!(
        second.request,
        Request::ReopenSession {
            session: SessionId(2),
            expected_run: ovrcr::protocol::SessionRunId(4),
            acknowledge_stopped: false,
        }
    );
}

#[test]
fn refused_workspace_removal_offers_force_and_sends_it_only_on_explicit_confirm() {
    {
        let code = ovrcr::protocol::ErrorCode::SessionsRemain;
        let mut dashboard = dashboard_fixture();
        palette_search(&mut dashboard, "remove workspace");
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Enter);
        let ovrcr::tui::DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
            panic!("expected confirmed removal")
        };
        let Request::RemoveWorkspace {
            project,
            name,
            force: false,
        } = request.request.clone()
        else {
            panic!("first removal must not force: {:?}", request.request)
        };
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Error {
                code: code.clone(),
                message: "refused".into(),
            },
        });
        let text = palette_text(&dashboard);
        assert!(text.contains("Force remove workspace"), "{code:?}: {text}");
        assert!(text.contains("refused"), "{code:?}: {text}");
        let ovrcr::tui::DashboardAction::Request(forced) = dashboard.key(KeyCode::Enter) else {
            panic!("{code:?}: forced removal needs another explicit Confirm")
        };
        assert_eq!(
            forced.request,
            Request::RemoveWorkspace {
                project: project.clone(),
                name: name.clone(),
                force: true,
            }
        );
        // A forced refusal (live sessions) must not loop into anything stronger.
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: forced.request_id,
            response: Response::Error {
                code: ovrcr::protocol::ErrorCode::SessionsRemain,
                message: "live sessions remain".into(),
            },
        });
        assert!(palette_text(&dashboard).contains("live sessions remain"));
        assert!(!matches!(
            dashboard.key(KeyCode::Esc),
            ovrcr::tui::DashboardAction::Request(_)
        ));
    }
}

#[test]
fn dirty_workspace_removal_asks_to_save_then_removes_only_after_yes() {
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let ovrcr::tui::DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
        panic!("expected confirmed removal")
    };
    let Request::RemoveWorkspace {
        project,
        name,
        force: false,
    } = request.request.clone()
    else {
        panic!("first removal must not force: {:?}", request.request)
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Error {
            code: ovrcr::protocol::ErrorCode::DirtyWorktree,
            message: "worktree has changes".into(),
        },
    });
    let text = palette_text(&dashboard);
    assert!(
        text.contains("Save uncommitted work to origin/wip/"),
        "{text}"
    );
    assert!(text.contains("worktree has changes"), "{text}");
    assert!(!text.contains("Force remove workspace"), "{text}");
    let ovrcr::tui::DashboardAction::Request(save) = dashboard.key(KeyCode::Enter) else {
        panic!("yes must send the save")
    };
    assert_eq!(
        save.request,
        Request::SaveWorkspaceWip {
            project: project.clone(),
            name: name.clone(),
        }
    );
    let followed = dashboard.handle_server_message(ServerMessage::Response {
        request_id: save.request_id,
        response: Response::Ok,
    });
    assert!(
        followed.iter().any(|message| {
            message.request
                == Request::RemoveWorkspace {
                    project: project.clone(),
                    name: name.clone(),
                    force: true,
                }
        }),
        "yes removes the workspace after the save: {followed:?}"
    );
}

#[test]
fn declining_the_save_does_not_remove_the_workspace() {
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let ovrcr::tui::DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
        panic!("expected confirmed removal")
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Error {
            code: ovrcr::protocol::ErrorCode::DirtyWorktree,
            message: "worktree has changes".into(),
        },
    });
    assert!(!matches!(
        dashboard.key(KeyCode::Esc),
        ovrcr::tui::DashboardAction::Request(_)
    ));
    assert!(
        !palette_text(&dashboard).contains("Save uncommitted work"),
        "no leaves the save question"
    );
}

#[test]
fn shutdown_asks_once_per_dirty_worktree() {
    let mut dashboard = dashboard_fixture();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::WipSavePrompt {
        project: "consigint".into(),
        workspace: "one".into(),
        branch: "feature/one".into(),
    }));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::WipSavePrompt {
        project: "consigint".into(),
        workspace: "two".into(),
        branch: "feature/two".into(),
    }));
    let text = palette_text(&dashboard);
    assert!(
        text.contains("Save uncommitted work to origin/wip/feature/one?"),
        "{text}"
    );
    assert!(!text.contains("feature/two"), "{text}");
    let ovrcr::tui::DashboardAction::Request(first) = dashboard.key(KeyCode::Enter) else {
        panic!("first yes")
    };
    assert_eq!(
        first.request,
        Request::AnswerWipSave {
            project: "consigint".into(),
            workspace: "one".into(),
            save: true,
        }
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first.request_id,
        response: Response::Ok,
    });
    let text = palette_text(&dashboard);
    assert!(
        text.contains("Save uncommitted work to origin/wip/feature/two?"),
        "{text}"
    );
    let ovrcr::tui::DashboardAction::Request(second) = dashboard.key(KeyCode::Esc) else {
        panic!("second no")
    };
    assert_eq!(
        second.request,
        Request::AnswerWipSave {
            project: "consigint".into(),
            workspace: "two".into(),
            save: false,
        }
    );
}

fn shutdown_navigation_fixture() -> (
    Dashboard,
    ovrcr::protocol::BridgeNavigationOffer,
    tempfile::TempDir,
) {
    use ovrcr::protocol::{
        BRIDGE_SCHEMA_VERSION, BridgeContext, BridgeNavigationOffer, BridgeNavigationTicket,
        PROTOCOL_VERSION, SessionRunId,
    };

    let root = tempfile::tempdir().unwrap();
    // This pure UI fixture checks ticket correlation, not executable-byte admission.
    // No callback executable, socket, provider or native Bridge is started.
    let context = BridgeContext {
        server_socket: root.path().join("server.sock").to_str().unwrap().into(),
        callback_executable: root.path().join("ovrcr").to_str().unwrap().into(),
        callback_executable_sha256: "0".repeat(64),
        server_lifetime: "10000000-0000-4000-8000-000000000001".into(),
    };
    assert!(context.validate());
    let offer = BridgeNavigationOffer {
        navigation: "20000000-0000-4000-8000-000000000002".into(),
        ticket: BridgeNavigationTicket {
            schema: BRIDGE_SCHEMA_VERSION,
            server_wire: PROTOCOL_VERSION,
            server_socket: context.server_socket.clone(),
            callback_executable: context.callback_executable.clone(),
            callback_executable_sha256: context.callback_executable_sha256.clone(),
            server_lifetime: context.server_lifetime.clone(),
            session: SessionId(3),
            run: SessionRunId(1),
        },
    };
    assert!(offer.ticket.validate());
    let mut dashboard = dashboard_fixture();
    if let Some(view) = dashboard.request_view_at(Rect::new(0, 0, 88, 38)) {
        dashboard.drain_outbox();
        acknowledge_all_view_targets(&mut dashboard, view);
    }
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::BridgeContext(context)));
    assert!(dashboard.drain_outbox().is_empty());
    (dashboard, offer, root)
}

fn shutdown_navigation_confirmation(
    dashboard: &mut Dashboard,
    offer: &ovrcr::protocol::BridgeNavigationOffer,
) -> ClientMessage {
    let mut outgoing = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::NotificationNavigation(offer.clone()),
    ));
    assert_eq!(
        outgoing.len(),
        1,
        "a fresh valid offer must only request confirmation"
    );
    let confirmation = outgoing.pop().unwrap();
    assert_eq!(
        confirmation.request,
        Request::ConfirmNotificationNavigation {
            navigation: offer.navigation.clone(),
        }
    );
    confirmation
}

fn shutdown_question(dashboard: &mut Dashboard, name: &str) {
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Event(ServerEvent::WipSavePrompt {
                project: "consigint".into(),
                workspace: name.into(),
                branch: format!("feature/{name}"),
            }))
            .is_empty()
    );
}

fn assert_shutdown_question(dashboard: &Dashboard, name: &str, hidden: &str) {
    let text = palette_text(dashboard);
    assert!(
        text.contains(&format!(
            "Save uncommitted work to origin/wip/feature/{name}?"
        )),
        "{text}"
    );
    assert!(!text.contains(&format!("feature/{hidden}")), "{text}");
}

fn answer_shutdown_question(dashboard: &mut Dashboard, name: &str, save: bool) -> ClientMessage {
    let DashboardAction::Request(answer) =
        dashboard.key(if save { KeyCode::Enter } else { KeyCode::Esc })
    else {
        panic!("shutdown question must require its explicit answer");
    };
    assert_eq!(
        answer.request,
        Request::AnswerWipSave {
            project: "consigint".into(),
            workspace: name.into(),
            save,
        }
    );
    answer
}

#[test]
fn shutdown_question_rejects_notification_offer_without_inferring_an_answer() {
    for save in [false, true] {
        let (mut dashboard, offer, _root) = shutdown_navigation_fixture();
        let focused = dashboard.focused_session();
        let revision = dashboard.view_revision();
        shutdown_question(&mut dashboard, "one");
        let outgoing = dashboard.handle_server_message(ServerMessage::Event(
            ServerEvent::NotificationNavigation(offer),
        ));
        assert!(
            outgoing.is_empty(),
            "a shutdown question forbids confirmation, application, input or an inferred answer: {outgoing:?}"
        );
        assert_eq!(dashboard.focused_session(), focused);
        assert_eq!(dashboard.view_revision(), revision);
        assert!(dashboard.request_view_at(Rect::new(0, 0, 88, 38)).is_none());
        assert_shutdown_question(&dashboard, "one", "two");
        assert!(
            dashboard
                .input_request(b"hidden-input".to_vec(), 9000)
                .is_none()
        );
        let answer = answer_shutdown_question(&mut dashboard, "one", save);
        assert!(
            dashboard
                .handle_server_message(ServerMessage::Response {
                    request_id: answer.request_id,
                    response: Response::Ok,
                })
                .is_empty()
        );
        assert!(!palette_text(&dashboard).contains("Save uncommitted work"));
        assert_view_input_allowed(&mut dashboard);
    }
}

#[test]
fn shutdown_question_rejects_already_pending_navigation_confirmation() {
    let (mut dashboard, offer, _root) = shutdown_navigation_fixture();
    let focused = dashboard.focused_session();
    let revision = dashboard.view_revision();
    let confirmation = shutdown_navigation_confirmation(&mut dashboard, &offer);
    shutdown_question(&mut dashboard, "one");
    shutdown_question(&mut dashboard, "two");
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: confirmation.request_id,
        response: Response::NotificationNavigationConfirmed(Some(Box::new(offer.clone()))),
    });
    assert!(
        outgoing.is_empty(),
        "a late confirmation must not apply, input or infer an answer: {outgoing:?}"
    );
    assert_eq!(dashboard.focused_session(), focused);
    assert_eq!(dashboard.view_revision(), revision);
    assert!(dashboard.request_view_at(Rect::new(0, 0, 88, 38)).is_none());
    assert_shutdown_question(&dashboard, "one", "two");
    let first = answer_shutdown_question(&mut dashboard, "one", false);
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: first.request_id,
                response: Response::Ok,
            })
            .is_empty()
    );
    assert_shutdown_question(&dashboard, "two", "one");
    let second = answer_shutdown_question(&mut dashboard, "two", true);
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: second.request_id,
                response: Response::Ok,
            })
            .is_empty()
    );
    // The refused matching confirmation was consumed; clearing the queue cannot replay it.
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: confirmation.request_id,
                response: Response::NotificationNavigationConfirmed(Some(Box::new(offer))),
            })
            .is_empty()
    );
    assert_eq!(dashboard.focused_session(), focused);
    assert_eq!(dashboard.view_revision(), revision);
    assert_view_input_allowed(&mut dashboard);
}

#[test]
fn shutdown_pending_answer_survives_navigation_and_advances_only_on_its_exact_ok() {
    let (mut dashboard, offer, _root) = shutdown_navigation_fixture();
    let focused = dashboard.focused_session();
    let revision = dashboard.view_revision();
    let confirmation = shutdown_navigation_confirmation(&mut dashboard, &offer);
    shutdown_question(&mut dashboard, "one");
    shutdown_question(&mut dashboard, "two");
    let first = answer_shutdown_question(&mut dashboard, "one", true);
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: confirmation.request_id,
        response: Response::NotificationNavigationConfirmed(Some(Box::new(offer.clone()))),
    });
    assert!(
        outgoing.is_empty(),
        "navigation must preserve the submitted answer and emit no application/input/answer: {outgoing:?}"
    );
    assert_eq!(dashboard.focused_session(), focused);
    assert_eq!(dashboard.view_revision(), revision);
    assert!(dashboard.request_view_at(Rect::new(0, 0, 88, 38)).is_none());
    assert_shutdown_question(&dashboard, "one", "two");
    for wrong in [confirmation.request_id, first.request_id + 1000] {
        assert!(
            dashboard
                .handle_server_message(ServerMessage::Response {
                    request_id: wrong,
                    response: Response::Ok,
                })
                .is_empty()
        );
        assert_shutdown_question(&dashboard, "one", "two");
        assert_eq!(
            dashboard.key(KeyCode::Enter),
            DashboardAction::Redraw,
            "an unanswered request must not resubmit"
        );
        assert!(dashboard.drain_outbox().is_empty());
    }
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: first.request_id,
                response: Response::Ok,
            })
            .is_empty()
    );
    assert_shutdown_question(&dashboard, "two", "one");
    // A repeated old answer acknowledgement cannot consume the second question.
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: first.request_id,
                response: Response::Ok,
            })
            .is_empty()
    );
    assert_shutdown_question(&dashboard, "two", "one");
    let second = answer_shutdown_question(&mut dashboard, "two", false);
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: second.request_id,
                response: Response::Ok,
            })
            .is_empty()
    );
    assert!(!palette_text(&dashboard).contains("Save uncommitted work"));
    assert_view_input_allowed(&mut dashboard);

    let mut fresh = offer;
    fresh.navigation = "30000000-0000-4000-8000-000000000003".into();
    let confirmation = shutdown_navigation_confirmation(&mut dashboard, &fresh);
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: confirmation.request_id,
        response: Response::NotificationNavigationConfirmed(Some(Box::new(fresh.clone()))),
    });
    assert_eq!(dashboard.focused_session(), Some(fresh.ticket.session));
    // Selection coalesces until the next ordinary render requests its view.
    let view = outgoing
        .iter()
        .find(|message| matches!(message.request, Request::SetView { .. }))
        .cloned()
        .unwrap_or_else(|| {
            dashboard
                .request_view_at(Rect::new(0, 0, 88, 38))
                .expect("fresh navigation must request its acknowledged view")
        });
    assert!(dashboard.drain_outbox().is_empty());
    let Request::SetView { view: selected } = &view.request else {
        unreachable!()
    };
    assert_eq!(selected.focused, Some(fresh.ticket.session));
    assert!(dashboard.view_revision() > revision);
    assert!(dashboard.input_request(b"before fresh view ack".to_vec(), 9001).is_none());
    acknowledge_all_view_targets(&mut dashboard, view);
    assert_view_input_allowed(&mut dashboard);
    assert_eq!(
        outgoing
            .iter()
            .filter(|message| matches!(
                &message.request,
                Request::NotificationNavigationApplied { navigation }
                    if navigation == &fresh.navigation
            ))
            .count(),
        1
    );
    assert!(
        outgoing.iter().all(|message| matches!(
            message.request,
            Request::NotificationNavigationApplied { .. } | Request::SetView { .. }
        )),
        "fresh navigation must not infer an answer or send input: {outgoing:?}"
    );
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: confirmation.request_id,
                response: Response::NotificationNavigationConfirmed(Some(Box::new(fresh))),
            })
            .is_empty()
    );
}

#[test]
fn force_removal_prompt_survives_unchanged_hierarchy_then_needs_consent_after_relabel() {
    let mut dashboard = dashboard_fixture();
    let mut original = fixture_hierarchy();
    original.projects[1].workspaces[1].name = "feature/auth.previous".into();
    dashboard.install_hierarchy(original.clone());
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let ovrcr::tui::DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
        panic!("expected confirmed removal")
    };
    let Request::RemoveWorkspace {
        project,
        name,
        force: false,
    } = request.request.clone()
    else {
        panic!("first removal must not force: {:?}", request.request)
    };
    assert_eq!((project.as_str(), name.as_str()), ("consigint", "auth"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Error {
            code: ovrcr::protocol::ErrorCode::SessionsRemain,
            message: "refused".into(),
        },
    });
    let force_text = palette_text(&dashboard);
    assert!(
        force_text.contains("Force remove workspace"),
        "{force_text}"
    );
    assert!(
        force_text.contains("consigint / feature/auth.previous"),
        "{force_text}"
    );
    assert!(
        !force_text.contains("Force remove workspace consigint / auth."),
        "force caption must use the branch heading, not the internal id: {force_text}"
    );

    let mut refreshed = original.clone();
    refreshed.projects[0].workspaces[0].sessions[0].pid = Some(999);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        refreshed,
    )));
    let still = palette_text(&dashboard);
    assert!(
        still.contains("Confirm action") && still.contains("Force remove workspace"),
        "ordinary hierarchy refresh must keep the force prompt: {still}"
    );
    assert!(
        still.contains("consigint / feature/auth.previous"),
        "{still}"
    );

    let mut relabelled = fixture_hierarchy();
    relabelled.projects[1].workspaces[1].name = "feature/auth".into();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        relabelled,
    )));
    let text = palette_text(&dashboard);
    assert!(
        !text.contains("Confirm action"),
        "relabelled force prompt must require fresh consent: {text}"
    );
    assert!(
        text.contains("consigint / feature/auth"),
        "picker must show the current branch: {text}"
    );
    if let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) {
        panic!(
            "relabelled force confirmation must not submit: {:?}",
            message.request
        );
    }
}

#[test]
fn workspace_removal_errors_keep_confirmation_and_session_context_visible() {
    for (code, message) in [
        (
            ovrcr::protocol::ErrorCode::SessionsRemain,
            "live or ownership-uncertain sessions remain",
        ),
        (
            ovrcr::protocol::ErrorCode::DirtyWorktree,
            "worktree has changes",
        ),
        (
            ovrcr::protocol::ErrorCode::PartialFailure,
            "worktree is unavailable; session records retained for recovery",
        ),
    ] {
        let mut dashboard = dashboard_fixture();
        palette_search(&mut dashboard, "remove workspace");
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Enter);
        let ovrcr::tui::DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
            panic!("expected confirmed removal")
        };
        assert!(matches!(request.request, Request::RemoveWorkspace { .. }));
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Error {
                code,
                message: message.into(),
            },
        });
        let text = palette_text(&dashboard);
        assert!(text.contains(message), "{text}");
        assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
        assert!(!matches!(
            dashboard.key(KeyCode::Esc),
            ovrcr::tui::DashboardAction::Request(_)
        ));
    }
}

#[test]
fn archive_search_unarchives_without_launching() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(':'));
    dashboard.event_action(Event::Paste("Archived sessions".into()));
    let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("archive must load inventory");
    };
    assert_eq!(message.request, Request::Inspect);
    let mut row = fixture_hierarchy().projects[0].workspaces[0].sessions[0].clone();
    row.archived = true;
    row.phase = SessionPhase::Stopped;
    row.title = Some("Archive needle".into());
    row.workspace = "archive-workspace".into();
    row.cwd = "/original/removed-worktree".into();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Inventory {
            registry: Default::default(),
            sessions: vec![row.clone()],
        },
    });
    assert!(palette_text(&dashboard).contains("Archived sessions"));
    dashboard.event_action(Event::Paste("archive-workspace removed-worktree".into()));
    assert!(palette_text(&dashboard).contains("Archive needle"));
    assert!(palette_text(&dashboard).contains("/original/removed-worktree"));
    let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("unarchive must issue a record operation");
    };
    assert_eq!(
        message.request,
        Request::UnarchiveSession {
            session: row.id,
            expected_run: row.run
        }
    );
}

#[test]
fn close_confirms_live_work_but_archives_exited_work_immediately() {
    let mut dashboard = dashboard_fixture();
    let action = dashboard.key(KeyCode::Char('X'));
    assert!(!matches!(action, ovrcr::tui::DashboardAction::Request(_)));
    assert!(palette_text(&dashboard).contains("archives its record"));
    let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("confirm must close");
    };
    let Request::CloseTerminal {
        session,
        expected_run,
    } = message.request
    else {
        panic!("wrong request");
    };
    let mut hierarchy = fixture_hierarchy();
    for row in hierarchy
        .projects
        .iter_mut()
        .flat_map(|p| &mut p.workspaces)
        .flat_map(|w| &mut w.sessions)
    {
        if row.id == session {
            row.phase = SessionPhase::Exited {
                code: Some(0),
                signal: None,
            };
        }
    }
    let mut dashboard = dashboard_fixture();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    // Select the same initial row; the fixture begins on this session.
    let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Char('X')) else {
        panic!("exited close must be immediate");
    };
    assert_eq!(
        message.request,
        Request::CloseTerminal {
            session,
            expected_run
        }
    );
}

#[test]
fn exited_agent_keeps_actionable_recovery_reason_in_the_header() {
    let mut hierarchy = fixture_hierarchy();
    let row = &mut hierarchy.projects[1].workspaces[0].sessions[0];
    row.kind = ovrcr::protocol::SessionKind::Agent {
        name: "claude".into(),
    };
    row.phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    row.recovery = Some(ovrcr::protocol::SessionRecovery {
        conversation: None,
        attached: false,
        requires_ack: true,
        unavailable: Some("No certified conversation; use managed launch".into()),
        failure: None,
    });
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(2));
    let text = rendered_rows(&dashboard, 180, 24).join("\n");
    assert!(
        text.contains("No certified conversation; use managed launch"),
        "{text}"
    );
}

#[test]
fn exited_agent_with_empty_recovery_keeps_stale_activity_out_of_header() {
    let mut hierarchy = fixture_hierarchy();
    let row = &mut hierarchy.projects[1].workspaces[0].sessions[0];
    row.kind = ovrcr::protocol::SessionKind::Agent {
        name: "claude".into(),
    };
    row.phase = SessionPhase::Exited {
        code: Some(0),
        signal: None,
    };
    row.activity = ovrcr::protocol::AgentActivity::Busy;
    row.agent = None;
    row.recovery = Some(ovrcr::protocol::SessionRecovery {
        conversation: None,
        attached: false,
        requires_ack: false,
        unavailable: None,
        failure: None,
    });
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(2));
    let text = rendered_rows(&dashboard, 180, 24).join("\n");
    assert!(text.contains("pid: closed"), "{text}");
    assert!(!text.contains("busy"), "{text}");
    assert!(!text.contains("busy"), "{text}");
}

#[test]
fn workspace_selection_and_fold_survive_branch_relabel() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('k'));
    dashboard.key(KeyCode::Char('k'));
    dashboard.key(KeyCode::Enter);
    let collapsed = rendered_rows(&dashboard, 88, 38).join("\n");
    assert!(collapsed.contains("▸"), "{collapsed}");
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[1].name = "feature/auth-renamed".into();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    let text = rendered_rows(&dashboard, 88, 38).join("\n");
    assert!(text.contains("feature/auth-renamed"), "{text}");
    assert!(text.contains("▸"), "{text}");
    assert!(
        !text.contains("review"),
        "folded workspace must keep sessions hidden: {text}"
    );
}

#[test]
fn duplicate_branch_labels_include_distinguishing_paths() {
    let mut hierarchy = fixture_hierarchy();
    let prefix = "/private/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/ovrcr";
    hierarchy.projects[1].workspaces.extend([
        WorkspaceSummary {
            project: "consigint".into(),
            id: "renamed-one".into(),
            name: "feature/workspace-renamed".into(),
            path: format!("{prefix}/worktree-one").into(),
            root: false,
            warning: None,
            sessions: vec![],
        },
        WorkspaceSummary {
            project: "consigint".into(),
            id: "renamed-two".into(),
            name: "feature/workspace-renamed".into(),
            path: format!("{prefix}/worktree-two").into(),
            root: false,
            warning: None,
            sessions: vec![],
        },
    ]);
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    // Default sidebar is 40 cells: prefix-clipping the full path made both rows
    // `feature/workspace-renamed (/privat…`. The distinguishing basename must
    // remain visible at that width.
    let rows = rendered_rows(&dashboard, 88, 38);
    let labels: Vec<_> = rows
        .iter()
        .filter(|row| {
            row.contains("feature/workspace-renamed")
                || row.contains("worktree-one")
                || row.contains("worktree-two")
        })
        .cloned()
        .collect();
    let one = labels
        .iter()
        .find(|row| row.contains("worktree-one"))
        .unwrap_or_else(|| panic!("missing worktree-one in sidebar: {labels:?}"));
    let two = labels
        .iter()
        .find(|row| row.contains("worktree-two"))
        .unwrap_or_else(|| panic!("missing worktree-two in sidebar: {labels:?}"));
    assert_ne!(
        one, two,
        "narrow sidebar must distinguish duplicate branches: {one}"
    );
    assert!(!one.contains("worktree-two"), "{one}");
    assert!(!two.contains("worktree-one"), "{two}");
    dashboard.key(KeyCode::Char('n'));
    dashboard.key(KeyCode::Tab); // Workspace pick list
    let form = palette_text(&dashboard);
    assert!(
        form.contains("/private/var/folders") && form.contains("worktree-one"),
        "picker keeps the full path when there is room: {form}"
    );
}

#[test]
fn warning_disables_new_terminal_and_shows_reason() {
    let mut hierarchy = fixture_hierarchy();
    let workspace = &mut hierarchy.projects[1].workspaces[1];
    workspace.root = true;
    workspace.warning = Some("root is on feature/x; check out main".into());
    workspace.name = "feature/x".into();
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    dashboard.key(KeyCode::Char('k'));
    dashboard.key(KeyCode::Char('k'));
    let text = rendered_rows(&dashboard, 120, 38).join("\n");
    assert!(text.contains("feature/x"), "{text}");
    assert!(
        text.contains("root is on feature/x") || text.contains('!'),
        "{text}"
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('n')),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        rendered_rows(&dashboard, 120, 38)
            .join("\n")
            .contains("root is on feature/x")
            || palette_text(&dashboard).is_empty(),
        "warning must surface instead of opening create terminal"
    );
}

#[test]
fn slash_branch_picker_does_not_split_on_slashes() {
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[1].name = "feature/auth".into();
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    dashboard.key(KeyCode::Char('n'));
    let text = palette_text(&dashboard);
    assert!(text.contains("consigint / feature/auth"), "{text}");
    let mut submitted = None;
    for _ in 0..6 {
        if let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) {
            submitted = Some(message);
            break;
        }
    }
    let message = submitted.expect("create terminal must submit");
    let Request::CreateSession(request) = message.request else {
        panic!("{:?}", message.request);
    };
    assert_eq!(request.project, "consigint");
    assert_eq!(request.workspace, "auth");
    assert_eq!(request.workspace, "auth");
}

#[test]
fn shared_internal_ids_are_resolved_with_project() {
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[0].workspaces.push(WorkspaceSummary {
        project: "spacelift-agent".into(),
        id: "auth".into(),
        name: "feature/other".into(),
        path: "/tmp/other".into(),
        root: false,
        warning: None,
        sessions: vec![session_summary(
            99,
            "spacelift-agent",
            "auth",
            "local",
            "zsh",
            Some(99),
            0,
        )],
    });
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(99));
    dashboard.key(KeyCode::Char('n'));
    let text = palette_text(&dashboard);
    assert!(text.contains("spacelift-agent / feature/other"), "{text}");
    let mut submitted = None;
    for _ in 0..6 {
        if let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) {
            submitted = Some(message);
            break;
        }
    }
    let message = submitted.expect("create terminal must submit");
    let Request::CreateSession(request) = message.request else {
        panic!("{:?}", message.request);
    };
    assert_eq!(request.project, "spacelift-agent");
    assert_eq!(request.workspace, "auth");
}

#[test]
fn terminal_workspace_picker_shows_current_branch_and_keeps_stable_target() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('n'));
    dashboard.key(KeyCode::Tab);
    let open = palette_text(&dashboard);
    assert!(open.contains("Create terminal"), "{open}");
    assert!(open.contains("consigint / auth"), "{open}");

    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[1].name = "feature/auth-renamed".into();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));

    let text = palette_text(&dashboard);
    assert!(text.contains("Create terminal"), "{text}");
    assert!(
        text.contains("consigint / feature/auth-renamed"),
        "picker must show the current branch: {text}"
    );
    assert!(
        !text.contains("consigint / auth"),
        "picker must not keep the old branch label: {text}"
    );

    let mut submitted = None;
    for _ in 0..6 {
        if let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) {
            submitted = Some(message);
            break;
        }
    }
    let message = submitted.expect("create terminal must submit the same workspace");
    let Request::CreateSession(request) = message.request else {
        panic!("{:?}", message.request);
    };
    assert_eq!(request.project, "consigint");
    assert_eq!(request.workspace, "auth");
}

#[test]
fn workspace_removal_confirmation_cannot_submit_after_branch_relabel() {
    let mut dashboard = dashboard_fixture();
    let mut original = fixture_hierarchy();
    original.projects[1].workspaces[1].name = "feature/auth.previous".into();
    dashboard.install_hierarchy(original);
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let confirm = palette_text(&dashboard);
    assert!(confirm.contains("Confirm action"), "{confirm}");
    assert!(
        confirm.contains("consigint / feature/auth.previous"),
        "{confirm}"
    );

    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[1].name = "feature/auth".into();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));

    let text = palette_text(&dashboard);
    assert!(
        !text.contains("Confirm action"),
        "stale confirmation must be invalidated: {text}"
    );
    assert!(
        text.contains("consigint / feature/auth"),
        "picker must show the current branch: {text}"
    );
    if let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) {
        panic!("stale confirmation must not submit: {:?}", message.request);
    }
}

#[test]
fn relabeled_workspace_filter_requires_reselection_before_retargeting() {
    let mut dashboard = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[1].name = "feature/foo".into();
    hierarchy.projects[1].workspaces[0].name = "feature/bar".into();
    dashboard.install_hierarchy(hierarchy.clone());
    dashboard.key(KeyCode::Char('n'));
    dashboard.key(KeyCode::Tab);
    for character in "feature/foo".chars() {
        dashboard.key(KeyCode::Char(character));
    }

    hierarchy.projects[1].workspaces[1].name = "feature/bar".into();
    hierarchy.projects[1].workspaces[0].name = "feature/foo".into();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    for _ in 0..6 {
        assert!(
            !matches!(
                dashboard.key(KeyCode::Enter),
                ovrcr::tui::DashboardAction::Request(_)
            ),
            "a stale filtered selection must not target the workspace that acquired its branch"
        );
    }
    dashboard.key(KeyCode::Down);
    let mut submitted = None;
    for _ in 0..6 {
        if let ovrcr::tui::DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) {
            submitted = Some(message.request);
            break;
        }
    }
    let Some(Request::CreateSession(request)) = submitted else {
        panic!("explicit reselection must permit submission");
    };
    assert_eq!(request.project, "consigint");
    assert_eq!(request.workspace, "lifecycle");
}

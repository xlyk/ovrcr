//! Command palette, which-key popup, and the creation forms.

#[cfg(not(target_os = "macos"))]
use crate::deadline::wait_deadline;
#[cfg(target_os = "macos")]
use crate::live::wait_deadline;
use crate::*;
use ratatui::layout::Position;

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

/// The fixture with every session exited. Removing a workspace here sends the
/// removal itself instead of first stopping its running sessions.
fn stopped_hierarchy() -> HierarchySnapshot {
    let mut hierarchy = fixture_hierarchy();
    for session in hierarchy
        .projects
        .iter_mut()
        .flat_map(|project| &mut project.workspaces)
        .flat_map(|workspace| &mut workspace.sessions)
    {
        session.phase = SessionPhase::Exited {
            code: Some(0),
            signal: None,
        };
        session.pid = None;
    }
    hierarchy
}

fn stopped_fixture() -> Dashboard {
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(stopped_hierarchy());
    dashboard
}

fn sidebar_row_for(dashboard: &Dashboard, needle: &str) -> u16 {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    (0..40)
        .find(|y| {
            (0..39)
                .map(|x| terminal.backend().buffer()[(x, *y)].symbol())
                .collect::<String>()
                .contains(needle)
        })
        .unwrap_or_else(|| panic!("sidebar row {needle:?} was not rendered"))
}

fn rendered_sequence(dashboard: &Dashboard, needle: &str) -> Position {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    let symbols: Vec<_> = needle.chars().map(|ch| ch.to_string()).collect();
    for y in 0..40 {
        for x in 0..=120_u16.saturating_sub(symbols.len() as u16) {
            if symbols.iter().enumerate().all(|(offset, symbol)| {
                terminal.backend().buffer()[(x + offset as u16, y)].symbol() == symbol
            }) {
                return Position::new(x, y);
            }
        }
    }
    panic!("rendered text {needle:?} was not visible");
}

fn open_sidebar_create_menu(dashboard: &mut Dashboard, row_needle: &str) {
    let area = Rect::new(0, 0, 120, 40);
    let row = sidebar_row_for(dashboard, row_needle);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 5,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    let button = rendered_sequence(dashboard, "[+]");
    assert_eq!(button.y, row, "create button belongs to {row_needle}");
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: button.x + 1,
            row: button.y,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
}

fn click_rendered_menu_item(dashboard: &mut Dashboard, label: &str) {
    let area = Rect::new(0, 0, 120, 40);
    let position = rendered_sequence(dashboard, label);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: position.x,
            row: position.y,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
}

fn open_new_terminal_from_menu(dashboard: &mut Dashboard) {
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('w'));
    dashboard.key(KeyCode::Char('t'));
}

fn open_generic_create_terminal_from_palette(dashboard: &mut Dashboard) {
    dashboard.key(KeyCode::Char(':'));
    // The command palette's initial inspection is unrelated to the form under test.
    dashboard.drain_outbox();
    dashboard.event_action(Event::Paste("Create terminal".into()));
    dashboard.key(KeyCode::Enter);
}

/// Everything inside the palette's border as one line of words, so a message
/// the palette wrapped still reads whole.
fn palette_words(dashboard: &Dashboard) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let cell = |x: u16, y: u16| buffer[(x, y)].symbol();
    let Some((left, top)) = (0..40)
        .flat_map(|y| (0..120).map(move |x| (x, y)))
        .find(|&(x, y)| cell(x, y) == "┌")
    else {
        return String::new();
    };
    let right = (left..120).find(|&x| cell(x, top) == "┐").unwrap_or(120);
    let bottom = (top..40).find(|&y| cell(left, y) == "└").unwrap_or(40);
    (top + 1..bottom)
        .map(|y| (left + 1..right).map(|x| cell(x, y)).collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn space_p_n_opens_branch_form_with_project_and_explicit_name() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('p'));
    assert_eq!(
        dashboard.key(KeyCode::Char('n')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
    assert_eq!(message.request, Request::Inspect);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Inventory {
            registry: fixture_registry(),
            sessions: Vec::new(),
        },
    });
    dashboard.event_action(Event::Paste("feature/leader-workspace".into()));
    let text = palette_text(&dashboard);
    assert!(!text.contains("Which key"));
    assert!(text.contains("feature/leader-workspace"));
    assert!(text.contains("consigint"));
    assert!(text.contains("Branch name"));
    assert!(!text.contains("Task"));
    assert!(!text.contains("Start"));
}

#[test]
fn palette_hint_action_ignores_late_inspection_error() {
    let mut dashboard = dashboard_fixture();
    assert_eq!(
        dashboard.key(KeyCode::Char(':')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
fn space_w_n_opens_new_agent_setup() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(' '));
    let text = palette_text(&dashboard);
    assert!(text.contains("Which key"));
    assert!(text.contains("w  Workspace"));
    dashboard.key(KeyCode::Char('w'));
    dashboard.key(KeyCode::Char('n'));
    let text = palette_text(&dashboard);
    assert!(!text.contains("Which key"));
    assert!(text.contains("New agent session"));
    assert!(text.contains("Agent harness"));
}

#[test]
fn space_w_t_opens_terminal_with_terminal_start_selected() {
    let mut dashboard = dashboard_fixture();
    open_new_terminal_from_menu(&mut dashboard);
    let words = palette_words(&dashboard);
    let screen = palette_text(&dashboard);
    assert!(screen.contains("Create terminal"), "{screen}");
    assert!(screen.contains("Start: Terminal"), "{screen}");
    assert!(words.contains("Start Terminal"), "{words}");
    for control in ["Workspace", "Name", "Command"] {
        assert!(words.contains(control), "{control}: {words}");
    }
    for control in ["Initial prompt", "Agent harness", "Model", "Agent", "Task"] {
        assert!(!words.contains(control), "{control}: {words}");
    }
    assert!(
        dashboard.drain_outbox().is_empty(),
        "the named Terminal action must not start agent model discovery"
    );

    assert_eq!(
        dashboard.event_action(Event::Key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT,)
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    let create = only_request(dashboard.drain_outbox());
    let Request::CreateSession(create) = create.request else {
        panic!("New terminal must preserve the terminal creation route");
    };
    assert_eq!(create.kind, ovrcr_protocol::SessionKind::Terminal);
    assert_eq!(
        (create.project.as_str(), create.workspace.as_str()),
        ("consigint", "auth")
    );
}

#[test]
fn question_mark_popup_is_browsable() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('?'));
    assert!(palette_text(&dashboard).contains("Which key"));
    for _ in 0..5 {
        dashboard.key(KeyCode::Down);
    }
    dashboard.key(KeyCode::Enter);
    let text = palette_text(&dashboard);
    assert!(!text.contains("Which key"), "{text}");
    assert!(text.contains("Register project"));
    assert!(text.contains("Repository"));
    assert!(text.contains("Workspace root"));
}

#[test]
fn space_top_level_lists_groups_registration_and_detach_but_no_toggles() {
    let mut dashboard = dashboard_fixture();
    for opener in [' ', '?'] {
        dashboard.key(KeyCode::Char(opener));
        let menu = whichkey_menu_text(&dashboard);
        for row in [
            "t  Terminal",
            "w  Workspace",
            "p  Project",
            "v  View",
            "s  Panes",
            "a  Register project",
            "q  Detach",
        ] {
            assert!(menu.contains(row), "{opener:?} {row}: {menu}");
        }
        for toggle in ["notifications", "sound", "N  ", "S  "] {
            assert!(!menu.contains(toggle), "{opener:?} {toggle}: {menu}");
        }
        dashboard.key(KeyCode::Esc);
    }
    // The toggles keep their Browse keys and stay in the palette and Settings.
    dashboard.key(KeyCode::Char('S'));
    assert!(matches!(
        only_request(dashboard.drain_outbox()).request,
        Request::SetSetting { ref path, .. } if path == "ready_sound"
    ));
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
    palette_search(&mut dashboard, "sound");
    let text = palette_text(&dashboard);
    assert!(text.contains("Disable ready sound"), "{text}");
    assert!(text.contains("Settings: Ready sound"), "{text}");
    dashboard.key(KeyCode::Esc);
    palette_search(&mut dashboard, "notifications");
    let text = palette_text(&dashboard);
    assert!(text.contains("Enable desktop notifications"), "{text}");
    assert!(text.contains("Settings: Desktop notifications"), "{text}");
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
        .position(|line| line.contains("n  Create branch"))
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
    assert!(text.contains("1  a  Register a repository"), "{text}");
    assert!(text.contains("2  w  Create a branch"), "{text}");
    assert!(
        text.contains("Space actions · : commands · ? keys"),
        "{text}"
    );
    // The instance paths moved to the Events list.
    assert!(!text.contains("Home:"), "{text}");
    assert!(!text.contains("Socket:"), "{text}");
}

#[test]
fn palette_search_finds_the_create_branch_entry() {
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "create branch");
    let text = palette_text(&dashboard);
    assert!(text.contains("Create branch (w)"), "{text}");
    assert!(!text.contains("Task"), "{text}");
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
            // `find` answers in bytes; the click needs the cell column.
            if let Some(x) = row.find("a  Register project") {
                target = Some((row[..x].chars().count() as u16, y));
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
        // `v` offers a session for the new pane; Esc keeps the suggestion.
        dashboard.key(KeyCode::Esc);
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
    assert!({
        let _ = dashboard.mouse_action(
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                105,
                0,
                KeyModifiers::NONE,
            ),
            area,
        );
        matches!(dashboard.drain_outbox().as_slice(), [
            ClientMessage { request: Request::Input { session: SessionId(1), bytes, .. }, .. },
            ClientMessage { request: Request::Inspect, .. }
        ] if bytes == b"\x1b[<2;3;4m")
    });
    assert!(
        palette_text(&dashboard).contains("Search"),
        "opening Commands should capture the overlay"
    );
}

#[test]
fn tasks_hotkey_cancels_pending_history_like_control_t() {
    let mut dashboard = dashboard_fixture();
    let begin = dashboard.key(KeyCode::PageUp);
    assert_eq!(begin, DashboardAction::Redraw);
    let message = only_request(dashboard.drain_outbox());
    assert!(matches!(message.request, Request::HistoryBegin { .. }));
    dashboard.key(KeyCode::Char('t'));
    assert!(
        palette_text(&dashboard).contains("OVRCR  Tasks"),
        "tasks hotkey must open the tasks overlay"
    );
    {
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
fn whichkey_empty_workspace_click_sets_new_agent_destination() {
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
    let registry = registry_from_hierarchy(&hierarchy);
    dashboard.install_hierarchy(hierarchy.clone());
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
    let card = palette_text(&dashboard);
    assert!(card.contains("spacelift-agent / empty"), "{card}");
    assert!(card.contains("No sessions yet"), "{card}");
    assert!(card.contains("n        new agent session"), "{card}");
    let action = dashboard.key(KeyCode::Char('n'));
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    let requests = dashboard.drain_outbox();
    assert!(
        requests.len() == 2
            && requests
                .iter()
                .any(|message| matches!(message.request, Request::Inspect))
            && requests
                .iter()
                .any(|message| matches!(message.request, Request::List)),
        "direct n must request read-only Inspect and List after selecting an empty workspace: {requests:?}"
    );
    for message in requests {
        let response = match message.request {
            Request::Inspect => Response::Inventory {
                registry: registry.clone(),
                sessions: vec![],
            },
            Request::List => Response::Hierarchy(hierarchy.clone()),
            other => panic!("new-agent setup must start with read-only requests: {other:?}"),
        };
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: message.request_id,
            response,
        });
    }
    let form = palette_text(&dashboard);
    assert!(form.contains("New agent session"), "{form}");
    assert!(form.contains("spacelift-agent / empty"), "{form}");
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
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('X'));
    let text = palette_text(&dashboard);
    assert!(text.contains("Close terminal?"));
    assert!(text.contains("review (#1)"));
    assert_eq!(dashboard.pane_rects(Rect::new(0, 0, 88, 38)).len(), 1);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
            // Tab moves on from a path with nothing to complete; this folder never exists.
            "register project",
            vec!["~/.ovrcr-missing-fixture/repo/", "repo", "~/worktrees"],
            Request::AddProject {
                name: "repo".into(),
                repo: home.join(".ovrcr-missing-fixture/repo"),
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
        // Removing a workspace with running sessions stops them first; that
        // sequence has its own tests.
        let mut dashboard = stopped_fixture();
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
                // Enter submits from any field; Tab moves to the next one.
                dashboard.key(KeyCode::Tab);
            }
        }
        let mut action = dashboard.key(KeyCode::Enter);
        if query.starts_with("remove") {
            assert_eq!(action, DashboardAction::Redraw);
            action = dashboard.key(KeyCode::Enter);
        }
        assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
        let message = only_request(dashboard.drain_outbox());
        assert_eq!(message.request, expected);
    }
}

#[test]
fn create_branch_form_submits_explicit_name_without_launch() {
    use ovrcr::protocol::{BranchRequest, Request};
    let mut dashboard = dashboard_fixture();
    let repo = hint_repository(&[]);
    open_palette_branch_command_with_repo(&mut dashboard, repo.path());
    wait_for_branch_hints(&mut dashboard);
    let text = palette_text(&dashboard);
    assert!(text.contains("Create branch"), "{text}");
    assert!(text.contains("Branch name"), "{text}");
    assert!(text.contains("Workspace options"), "{text}");
    assert!(!text.contains("Task"), "{text}");
    assert!(!text.contains("Agent"), "{text}");
    assert!(!text.contains("Start"), "{text}");
    assert!(!text.contains("Name"), "{text}");
    dashboard.event_action(Event::Paste("feature/palette".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
    let Request::CreateWorkspace {
        project,
        id,
        branch,
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
}

/// Settings with three named agents, remembered for `consigint`.
fn fixture_agents(dashboard: &mut Dashboard) {
    dashboard.install_settings(ovrcr::tui::Settings {
        agents: ["fixture-a", "fixture-b", "fixture-c"]
            .into_iter()
            .map(|name| ovrcr_protocol::AgentOverride {
                name: name.into(),
                argv: vec!["/bin/sh".into(), "-c".into(), "exit 0".into()],
            })
            .collect(),
        launch_choices: [
            (
                "consigint".to_owned(),
                ovrcr_protocol::LaunchChoice::Agent {
                    preset: "fixture-a".into(),
                    model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
                },
            ),
            (
                "spacelift-agent".to_owned(),
                ovrcr_protocol::LaunchChoice::Agent {
                    preset: "fixture-a".into(),
                    model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
                },
            ),
        ]
        .into(),
        ..Default::default()
    });
}

/// Panics unless each part appears in `text`, in this order.
fn assert_in_order(text: &str, parts: &[&str]) {
    let mut rest = text;
    for part in parts {
        let Some(at) = rest.find(part) else {
            panic!("{part:?} missing or out of order in: {text}");
        };
        rest = &rest[at + part.len()..];
    }
}

/// Opens the standalone branch form with Branch name focused.
fn open_workspace_form(dashboard: &mut Dashboard) {
    let action = dashboard.key(KeyCode::Char('w'));
    answer_palette_inspect_with_fixture_registry(dashboard, action);
    wait_for_branch_hints(dashboard);
    to_branch(dashboard);
}

fn open_branch_form_with_repo(dashboard: &mut Dashboard, repo: &std::path::Path) {
    assert_eq!(
        dashboard.key(KeyCode::Char('w')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let inspect = only_request(dashboard.drain_outbox());
    assert_eq!(inspect.request, Request::Inspect);
    answer_workspace_inspect(dashboard, inspect.request_id, repo);
}

fn open_workspace_form_with_repo(dashboard: &mut Dashboard, repo: &std::path::Path) {
    open_branch_form_with_repo(dashboard, repo);
    wait_for_branch_hints(dashboard);
    to_branch(dashboard);
}

fn trunk_hint_repository() -> tempfile::TempDir {
    let repo = hint_repository(&[]);
    let renamed = std::process::Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["branch", "-m", "trunk"])
        .output()
        .unwrap();
    assert!(
        renamed.status.success(),
        "{}",
        String::from_utf8_lossy(&renamed.stderr)
    );
    repo
}

fn open_palette_branch_command_with_repo(dashboard: &mut Dashboard, repo: &std::path::Path) {
    assert_eq!(
        dashboard.key(KeyCode::Char(':')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let inspect = only_request(dashboard.drain_outbox());
    assert_eq!(inspect.request, Request::Inspect);
    answer_workspace_inspect(dashboard, inspect.request_id, repo);
    dashboard.event_action(Event::Paste("create branch".into()));
    dashboard.key(KeyCode::Enter);
}

fn wait_for_branch_hints(dashboard: &mut Dashboard) {
    let deadline = std::time::Instant::now() + wait_deadline();
    loop {
        dashboard.poll_palette();
        let text = palette_text(dashboard);
        if !text.contains("Loading the repository default branch") {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Git hints never reached the branch form: {text}"
        );
        std::thread::yield_now();
    }
}

/// Types a branch before Git's default-branch lookup returns, then defers the
/// submit until the registry provides the disposable repository path.
fn type_branch_and_submit(dashboard: &mut Dashboard, branch: &str) -> u64 {
    assert_eq!(
        dashboard.key(KeyCode::Char('w')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let inspect = only_request(dashboard.drain_outbox());
    assert_eq!(inspect.request, Request::Inspect);
    to_branch(dashboard);
    dashboard.event_action(Event::Paste(branch.into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    inspect.request_id
}

#[test]
fn standalone_branch_form_requires_an_explicit_valid_name() {
    let mut dashboard = dashboard_fixture();
    open_workspace_form(&mut dashboard);
    let words = palette_words(&dashboard);
    assert!(words.contains("Branch name"), "{words}");
    assert!(words.contains("Workspace options"), "{words}");
    for removed in ["Task", "Start", "Agent"] {
        assert!(!words.contains(removed), "{removed}: {words}");
    }
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        dashboard.drain_outbox().is_empty(),
        "an empty name cannot submit"
    );
    assert!(palette_words(&dashboard).contains("Branch name is required"));
    dashboard.event_action(Event::Paste("feature/contains space".into()));
    dashboard.key(KeyCode::Enter);
    assert!(
        dashboard.drain_outbox().is_empty(),
        "Git rejects spaces in refs"
    );
    assert!(palette_words(&dashboard).contains("Branch names can't contain spaces"));
    dashboard.ctrl('u');
    assert!(!palette_words(&dashboard).contains("Branch names can't contain spaces"));
}

#[test]
fn branch_names_git_would_refuse_are_named_inline() {
    let mut dashboard = dashboard_fixture();
    open_workspace_form(&mut dashboard);
    dashboard.event_action(Event::Paste("feature/status check".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.drain_outbox().is_empty());
    assert_in_order(
        &palette_words(&dashboard),
        &[
            "Branch name › feature/status check",
            "Branch names can't contain spaces",
        ],
    );
    dashboard.ctrl('u');
    assert!(!palette_words(&dashboard).contains("can't contain spaces"));
}

#[test]
fn register_project_submits_from_its_first_field() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("Code").join("demo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let mut dashboard = dashboard_fixture();
    install_picker_roots(&mut dashboard, vec![dir.path().join("Code")]);
    dashboard.install_config_dir(dir.path().join("config"));
    dashboard.key(KeyCode::Char('a'));
    dashboard.event_action(Event::Paste(format!("{}/", repo.display())));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
    assert_eq!(
        message.request,
        Request::AddProject {
            name: "demo".into(),
            repo: repo.clone(),
            workspace_root: dir.path().join("config").join("workspaces").join("demo"),
        }
    );
}

#[test]
fn tab_on_a_repository_checkout_moves_to_name() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("demo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    let mut dashboard = dashboard_fixture();
    install_picker_roots(&mut dashboard, vec![dir.path().to_path_buf()]);
    dashboard.key(KeyCode::Char('a'));
    dashboard.event_action(Event::Paste(format!("{}/", repo.display())));
    // The footer says what Tab will do here.
    let words = palette_words(&dashboard);
    assert!(words.contains("↑/↓ highlight · Tab next"), "{words}");
    assert!(!words.contains("Tab complete"), "{words}");
    dashboard.key(KeyCode::Tab);
    dashboard.event_action(Event::Paste("-renamed".into()));
    let text = palette_words(&dashboard);
    assert!(text.contains("Name › demo-renamed"), "{text}");
    assert!(
        text.contains(&format!("Repository {}/", repo.display())),
        "the checkout is complete, not descended into: {text}"
    );
}

#[test]
fn pick_arrows_change_the_value_at_once_and_left_right_wrap() {
    let mut dashboard = dashboard_fixture();
    fixture_agents(&mut dashboard);
    open_generic_create_terminal_from_palette(&mut dashboard);
    let title = |dashboard: &Dashboard| {
        palette_text(dashboard)
            .lines()
            .find(|line| line.contains("┌ Create terminal"))
            .unwrap_or_default()
            .to_owned()
    };
    assert!(title(&dashboard).contains("Start: Agent (fixture-a)"));
    dashboard.key(KeyCode::Tab); // Start follows the optional Initial prompt
    // No Tab or Enter: the arrow is the choice, and the form follows it.
    dashboard.key(KeyCode::Down);
    assert!(
        title(&dashboard).contains("Start: Terminal"),
        "{}",
        title(&dashboard)
    );
    let text = palette_words(&dashboard);
    assert!(text.contains("Command"), "{text}");
    assert!(!text.contains("fixture-a"), "{text}");
    dashboard.key(KeyCode::Right);
    assert!(
        title(&dashboard).contains("Start: Agent"),
        "wraps past the end"
    );
    dashboard.key(KeyCode::Left);
    assert!(
        title(&dashboard).contains("Start: Terminal"),
        "wraps past the start"
    );
}

#[test]
fn new_agent_form_exposes_the_harness_and_model_controls() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('n'));
    let text = palette_text(&dashboard);
    assert!(text.contains("New agent session"), "{text}");
    for control in ["Branch", "Agent harness", "Model", "Prompt (optional)"] {
        assert!(text.contains(control), "{control}: {text}");
    }
    for removed in ["Task", "Start:"] {
        assert!(!text.contains(removed), "{removed}: {text}");
    }
}

#[test]
fn footer_lists_only_keys_that_work_for_the_focused_field() {
    let mut dashboard = dashboard_fixture();
    install_picker_roots(&mut dashboard, Vec::new());
    dashboard.key(KeyCode::Char('a'));
    let text = palette_words(&dashboard);
    assert!(
        text.contains("↑/↓ highlight · Tab complete · Enter register · Esc cancel"),
        "{text}"
    );
    for wrong in ["Command blank", "Name blank", "toggle", "Space"] {
        assert!(!text.contains(wrong), "{wrong}: {text}");
    }
    dashboard.key(KeyCode::Esc);

    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    let text = palette_words(&dashboard);
    assert!(
        text.contains("↑/↓/←/→ choose · type to filter · Enter continue · Esc cancel"),
        "{text}"
    );
    assert!(
        !text.contains("Tab"),
        "a single field has nowhere to Tab: {text}"
    );
    dashboard.key(KeyCode::Esc);

    open_workspace_form(&mut dashboard);
    assert!(
        palette_words(&dashboard).contains("Ctrl-u clear · Tab/Shift-Tab fields · Enter create"),
        "{}",
        palette_words(&dashboard)
    );
    dashboard.key(KeyCode::Tab);
    assert!(
        palette_words(&dashboard).contains("Space/←/→ switch · Tab/Shift-Tab fields"),
        "{}",
        palette_words(&dashboard)
    );
}

#[test]
fn create_branch_retry_keeps_generated_id_and_branch() {
    use ovrcr::protocol::{BranchRequest, Request};
    let mut dashboard = dashboard_fixture();
    let repo = hint_repository(&[]);
    open_palette_branch_command_with_repo(&mut dashboard, repo.path());
    wait_for_branch_hints(&mut dashboard);
    to_branch(&mut dashboard);
    dashboard.event_action(Event::Paste("feature/topic".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let first = only_request(dashboard.drain_outbox());
    let Request::CreateWorkspace {
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
        &BranchRequest::New {
            branch: "feature/topic".into(),
            base: "main".into(),
        }
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: first.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "retry me".into(),
        },
    });
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let retry = only_request(dashboard.drain_outbox());
    let Request::CreateWorkspace {
        id: retry_id,
        branch: retry_branch,
        ..
    } = retry.request
    else {
        panic!("{:?}", retry.request);
    };
    assert_eq!(retry_id, *first_id);
    assert_eq!(retry_branch, *branch);
}

#[test]
fn w_opens_branch_form_with_project_and_explicit_branch() {
    use ovrcr::protocol::BranchRequest;
    let mut dashboard = dashboard_fixture();
    let repo = hint_repository(&[]);
    open_branch_form_with_repo(&mut dashboard, repo.path());
    wait_for_branch_hints(&mut dashboard);
    to_branch(&mut dashboard);
    dashboard.event_action(Event::Paste("feature/pick-demo".into()));
    let text = palette_text(&dashboard);
    assert!(text.contains("Create branch"), "{text}");
    assert!(text.contains("consigint"), "{text}");
    assert!(text.contains("feature/pick-demo"), "{text}");
    assert!(!text.contains("Task"), "{text}");
    assert!(!text.contains("Start"), "{text}");
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
    let Request::CreateWorkspace {
        project,
        id,
        branch,
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
}

#[test]
fn branch_form_uses_detected_default_and_accepts_an_explicit_local_base() {
    use ovrcr::protocol::{BranchRequest, Request};
    let repo = hint_repository(&["topic"]);
    let mut dashboard = dashboard_fixture();
    let action = dashboard.key(KeyCode::Char('w'));
    let inspect = only_request(dashboard.drain_outbox());
    assert_eq!(inspect.request, Request::Inspect);
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    answer_workspace_inspect(&mut dashboard, inspect.request_id, repo.path());
    let deadline = std::time::Instant::now() + wait_deadline();
    loop {
        dashboard.poll_palette();
        let text = palette_text(&dashboard);
        if !text.contains("Loading the repository default branch") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Git hints never reached the branch form: {text}"
        );
        std::thread::yield_now();
    }
    dashboard.event_action(Event::Paste("feature/from-topic".into()));
    dashboard.key(KeyCode::Tab); // Workspace options
    dashboard.key(KeyCode::Char(' ')); // Expand Base
    dashboard.key(KeyCode::Tab); // Base
    let text = palette_words(&dashboard);
    assert!(text.contains("Base"), "{text}");
    assert!(
        text.contains("main"),
        "the detected default is shown: {text}"
    );
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("topic".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
    let Request::CreateWorkspace {
        project,
        id,
        branch,
    } = message.request
    else {
        panic!("{:?}", message.request);
    };
    assert_eq!(project, "consigint");
    assert_workspace_id(&id);
    assert_eq!(
        branch,
        BranchRequest::New {
            branch: "feature/from-topic".into(),
            base: "topic".into(),
        }
    );
}

#[test]
fn deferred_branch_submit_refuses_a_name_that_already_exists() {
    let repo = hint_repository(&["feature/release-2"]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_branch_and_submit(&mut dashboard, "feature/release-2");
    answer_workspace_inspect(&mut dashboard, inspect_id, repo.path());
    let text = poll_until_deferred_submit_refused(
        &mut dashboard,
        "an existing branch name must not create a duplicate worktree",
    );
    assert!(
        text.contains("That branch already exists in this repository"),
        "{text}"
    );
}

#[test]
fn base_matching_is_case_sensitive() {
    let repo = hint_repository(&["Release-2"]);
    let mut dashboard = dashboard_fixture();
    let action = dashboard.key(KeyCode::Char('w'));
    let inspect = only_request(dashboard.drain_outbox());
    assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
    answer_workspace_inspect(&mut dashboard, inspect.request_id, repo.path());
    let deadline = std::time::Instant::now() + wait_deadline();
    loop {
        dashboard.poll_palette();
        let text = palette_text(&dashboard);
        if !text.contains("Loading the repository default branch") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{text}");
        std::thread::yield_now();
    }
    dashboard.event_action(Event::Paste("feature/child".into()));
    dashboard.key(KeyCode::Tab); // Workspace options
    dashboard.key(KeyCode::Char(' ')); // Expand Base
    dashboard.key(KeyCode::Tab); // Base
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("release-2".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.drain_outbox().is_empty());
    assert!(palette_words(&dashboard).contains("Base must resolve to a local branch"));
}

#[test]
fn workspace_created_with_exited_local_shell_closes_palette() {
    let repo = trunk_hint_repository();
    let mut dashboard = dashboard_fixture();
    open_workspace_form_with_repo(&mut dashboard, repo.path());
    dashboard.event_action(Event::Paste("pick-demo".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
    let Request::CreateWorkspace { id, .. } = &message.request else {
        panic!("{:?}", message.request);
    };
    let created_id = id.clone();
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Ok,
    });
    // Keep the form visible until the lifecycle result and fresh readbacks
    // confirm the new branch.
    let mid = palette_text(&dashboard);
    assert!(
        mid.lines().any(|line| line.contains("┌ Create branch")) && mid.contains("Working…"),
        "{mid}"
    );
    let mut hierarchy = extra_workspace_hierarchy(created_id.clone());
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
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy.clone(),
    )));
    // A branch's hierarchy row alone is not completion evidence.
    assert_ne!(dashboard.focused_session(), Some(SessionId(99)));
    let readbacks = lifecycle_completed(
        &mut dashboard,
        &message,
        ovrcr::protocol::LifecycleOutcome::Succeeded,
    );
    let outgoing = answer_branch_readbacks(&mut dashboard, readbacks, &hierarchy);
    let text = palette_text(&dashboard);
    assert!(
        !text.lines().any(|line| line.contains("┌ Create branch")),
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
    let mut dashboard = dashboard_fixture();
    assert_eq!(
        dashboard.key(KeyCode::Char(':')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let inspect = only_request(dashboard.drain_outbox());
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
    let repo = trunk_hint_repository();
    let mut dashboard = dashboard_fixture();
    open_workspace_form_with_repo(&mut dashboard, repo.path());
    dashboard.event_action(Event::Paste("pick-demo".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id + 1,
        response: Response::Ok,
    });
    assert!(palette_text(&dashboard).contains("Working"));
    assert!(palette_text(&dashboard).contains("┌ Create branch"));
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
    dashboard.key(KeyCode::Esc);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy.clone(),
    )));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(footer_text(&dashboard).contains("BROWSE"));
    assert!(!palette_text(&dashboard).contains("┌ Create branch"));

    let mut cancelled = dashboard_fixture();
    open_workspace_form_with_repo(&mut cancelled, repo.path());
    cancelled.key(KeyCode::Esc);
    assert!(cancelled.drain_outbox().is_empty());
    cancelled.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    assert_eq!(cancelled.focused_session(), Some(SessionId(1)));
    assert!(!palette_text(&cancelled).contains("┌ Create branch"));
}

#[test]
fn branch_creation_selects_workspace_without_launching_a_session() {
    let repo = trunk_hint_repository();
    for hierarchy_first in [true, false] {
        let mut dashboard = dashboard_fixture();
        open_workspace_form_with_repo(&mut dashboard, repo.path());
        dashboard.event_action(Event::Paste("pick-demo".into()));
        assert_eq!(
            dashboard.key(KeyCode::Enter),
            ovrcr::tui::DashboardAction::Redraw
        );
        let message = only_request(dashboard.drain_outbox());
        let created_id = match &message.request {
            Request::CreateWorkspace { id, branch, .. } => {
                assert_eq!(
                    branch,
                    &ovrcr::protocol::BranchRequest::New {
                        branch: "pick-demo".into(),
                        base: "trunk".into(),
                    }
                );
                id.clone()
            }
            other => panic!("expected create workspace: {other:?}"),
        };
        let hierarchy = extra_workspace_hierarchy(created_id);
        accept(&mut dashboard, &message);
        let readbacks = if hierarchy_first {
            dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
                hierarchy.clone(),
            )));
            assert_ne!(dashboard.focused_session(), Some(SessionId(99)));
            lifecycle_completed(
                &mut dashboard,
                &message,
                ovrcr::protocol::LifecycleOutcome::Succeeded,
            )
        } else {
            let readbacks = lifecycle_completed(
                &mut dashboard,
                &message,
                ovrcr::protocol::LifecycleOutcome::Succeeded,
            );
            assert_ne!(dashboard.focused_session(), Some(SessionId(99)));
            dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
                hierarchy.clone(),
            )));
            readbacks
        };
        let outgoing = answer_branch_readbacks(&mut dashboard, readbacks, &hierarchy);
        assert_eq!(dashboard.focused_session(), None);
        assert!(
            !outgoing.iter().any(|message| matches!(
                &message.request,
                Request::SetView { view } if view.focused == Some(SessionId(99))
            )),
            "branch creation must not attach a local shell: {outgoing:?}"
        );
        assert!(
            !outgoing.iter().any(|message| matches!(
                &message.request,
                Request::CreateSession(_) | Request::CreateAgentSession(_)
            )),
            "branch creation must not launch a session: {outgoing:?}"
        );
        let text = palette_text(&dashboard);
        assert!(text.contains("consigint / pick-demo"), "{text}");
        assert!(
            text.contains("$ local"),
            "the readback includes its existing local shell: {text}"
        );
        assert!(!text.contains("┌ Create branch"), "{text}");
    }
}

#[test]
fn n_opens_new_agent_form_prefilled_for_selected_workspace() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    let action = dashboard.key(KeyCode::Char('n'));
    assert_eq!(action, DashboardAction::Redraw);
    answer_palette_inspect_with_fixture_registry(&mut dashboard, action);
    let text = palette_text(&dashboard);
    assert!(text.contains("New agent session"));
    assert!(text.contains("consigint / auth"));
    assert!(text.contains("Agent harness"), "{text}");
    assert!(text.contains("Model"), "{text}");
}

#[test]
fn new_agent_project_picker_defaults_from_selected_session_and_scopes_branches() {
    let mut dashboard = dashboard_fixture();
    let action = dashboard.key(KeyCode::Char('n'));
    answer_palette_inspect_with_fixture_registry(&mut dashboard, action);

    let form = palette_text(&dashboard);
    assert_in_order(&form, &["Project", "consigint", "Branch", "auth"]);

    assert_eq!(
        dashboard.key(KeyCode::Tab),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let branches = palette_text(&dashboard);
    let picker = branches
        .lines()
        .filter(|line| line.contains("││"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(picker.contains("lifecycle"), "{picker}");
    assert!(picker.contains("auth"), "{picker}");
    assert!(
        !picker.contains("progress"),
        "branches crossed project boundary: {picker}"
    );
}

#[test]
fn new_agent_project_defaults_follow_project_workspace_and_session_rows() {
    for (row_label, expected_project) in [
        ("SPACELIFT-AGENT", "spacelift-agent"),
        ("lifecycle", "consigint"),
        ("implement", "consigint"),
    ] {
        let mut dashboard = dashboard_fixture();
        let area = Rect::new(0, 0, 120, 40);
        let row = sidebar_row_for(&dashboard, row_label);
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        let action = dashboard.key(KeyCode::Char('n'));
        answer_palette_inspect_with_fixture_registry(&mut dashboard, action);
        let form = palette_text(&dashboard);
        assert_in_order(&form, &["Project", expected_project, "Branch"]);
    }
}

#[test]
fn new_agent_project_uses_first_project_without_selection_and_switch_clears_branch() {
    let mut empty = Dashboard::new(ovrcr::session::TerminalSize { rows: 38, cols: 88 });
    empty.install_area(Rect::new(0, 0, 88, 38));
    empty.install_hierarchy(fixture_hierarchy());
    let action = empty.key(KeyCode::Char('n'));
    answer_palette_inspect_with_fixture_registry(&mut empty, action);
    let first_project = palette_text(&empty);
    assert_in_order(&first_project, &["Project", "spacelift-agent", "Branch"]);

    let mut dashboard = dashboard_fixture();
    fixture_agents(&mut dashboard);
    let action = dashboard.key(KeyCode::Char('n'));
    answer_palette_inspect_with_fixture_registry(&mut dashboard, action);
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Enter);
    dashboard.event_action(Event::Paste("au".into()));
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::BackTab);
    dashboard.key(KeyCode::Enter);
    dashboard.event_action(Event::Paste("spacelift-agent".into()));
    dashboard.key(KeyCode::Enter);

    let switched = palette_text(&dashboard);
    assert!(switched.contains("spacelift-agent"), "{switched}");
    assert!(
        !switched.contains("consigint / auth"),
        "stale branch survived: {switched}"
    );
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Enter);
    let branches = palette_text(&dashboard)
        .lines()
        .filter(|line| line.contains("││"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(branches.contains("progress"), "{branches}");
    assert!(
        !branches.contains("auth"),
        "old project branch survived: {branches}"
    );

    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)),
        ovrcr::tui::DashboardAction::Redraw
    );
    let requests = dashboard.drain_outbox();
    assert_eq!(requests.len(), 1, "{}", palette_text(&dashboard));
    let allocation = requests.into_iter().next().unwrap();
    assert_eq!(allocation.request, Request::AllocateInitialPromptIntent);
    let followup = dashboard.handle_server_message(ServerMessage::Response {
        request_id: allocation.request_id,
        response: Response::InitialPromptIntent(ovrcr::protocol::InitialPromptIntentId {
            server_lifetime: "123e4567-e89b-12d3-a456-426614174000".into(),
            sequence: 1,
        }),
    });
    let [message] = followup.as_slice() else {
        panic!("initial-prompt intent should resume the selected launch: {followup:?}");
    };
    let Request::CreateAgentSessionWithPrompt(launch) = &message.request else {
        panic!(
            "expected Agent launch for stable project workspace: {:?}",
            message.request
        );
    };
    assert_eq!(launch.launch.session.project, "spacelift-agent");
    assert_eq!(launch.launch.session.workspace, "progress");
}

#[test]
fn new_agent_branch_picker_offers_typed_creation_by_match_position() {
    fn picker(query: &str) -> String {
        let mut dashboard = dashboard_fixture();
        let action = dashboard.key(KeyCode::Char('n'));
        answer_palette_inspect_with_fixture_registry(&mut dashboard, action);
        dashboard.key(KeyCode::Tab);
        dashboard.key(KeyCode::Enter);
        if !query.is_empty() {
            dashboard.event_action(Event::Paste(query.into()));
        }
        palette_text(&dashboard)
            .lines()
            .filter(|line| line.contains("││"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    let empty = picker("");
    assert!(!empty.contains("Create branch"), "{empty}");

    let partial = picker("au");
    assert_in_order(&partial, &["auth", "Create branch \"au\""]);
    assert!(!partial.contains("Create new branch"), "{partial}");

    let exact = picker("auth");
    assert!(exact.contains("auth"), "{exact}");
    assert!(!exact.contains("Create branch"), "{exact}");

    let unmatched = picker("spike/pane-tree");
    assert!(
        unmatched.contains("Create branch \"spike/pane-tree\""),
        "{unmatched}"
    );
    assert!(!unmatched.contains("auth"), "{unmatched}");
    assert!(!unmatched.contains("lifecycle"), "{unmatched}");
}

#[test]
fn new_agent_destination_uses_default_and_advanced_base_at_supported_sizes() {
    fn render_at(dashboard: &Dashboard, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
            .unwrap();
        (0..height)
            .map(|row| {
                (0..width)
                    .map(|column| terminal.backend().buffer()[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    let repo = hint_repository(&["develop"]);
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('n'));
    let requests = dashboard.drain_outbox();
    let inspect = requests
        .iter()
        .find(|message| matches!(message.request, Request::Inspect))
        .unwrap();
    answer_workspace_inspect(&mut dashboard, inspect.request_id, repo.path());
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Enter);
    dashboard.event_action(Event::Paste("feature/destination-preview".into()));
    dashboard.key(KeyCode::Enter);
    wait_for_branch_hints(&mut dashboard);
    let destination = palette_text(&dashboard);
    assert!(
        destination.contains("worktrees/feature-destination-preview")
            && destination.contains("From main"),
        "default branch destination missing: {destination}"
    );
    for (width, height) in [(80, 24), (60, 18)] {
        let text = render_at(&dashboard, width, height);
        assert!(
            text.contains("worktrees/feature-destination-preview") && text.contains("From main"),
            "{width}x{height}: {text}"
        );
    }

    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    for _ in 0..4 {
        dashboard.key(KeyCode::Tab);
    }
    assert_eq!(
        dashboard.key(KeyCode::Char(' ')),
        ovrcr::tui::DashboardAction::Redraw
    );
    dashboard.key(KeyCode::Tab);
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("develop".into()));
    let overridden = palette_text(&dashboard);
    assert!(overridden.contains("From develop"), "{overridden}");
    for (width, height) in [(80, 24), (60, 18)] {
        let text = render_at(&dashboard, width, height);
        assert!(text.contains("From develop"), "{width}x{height}: {text}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn live_dashboard_creates_typed_branch_before_starting_agent() {
    use crate::live::{Live, PROJECT};
    use ovrcr::protocol as p;
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    fn exchange_dashboard_requests(
        dashboard: &mut Dashboard,
        stream: &mut std::os::unix::net::UnixStream,
        initial: Vec<p::ClientMessage>,
    ) {
        let mut pending = HashSet::new();
        let mut outgoing = initial;
        let deadline = Instant::now() + Duration::from_secs(8);
        while !outgoing.is_empty() || !pending.is_empty() {
            for message in outgoing.drain(..) {
                pending.insert(message.request_id);
                p::write_frame(stream, &message).expect("send Dashboard request");
            }
            if pending.is_empty() {
                break;
            }
            assert!(Instant::now() < deadline, "Dashboard requests timed out");
            let server =
                p::read_frame::<p::ServerMessage>(stream).expect("read Dashboard response");
            if let p::ServerMessage::Response { request_id, .. } = &server {
                pending.remove(request_id);
            }
            outgoing.extend(dashboard.handle_server_message(server));
        }
    }

    let live = Live::idle().bounded();
    std::fs::write(
        live.config.join("dashboard.toml"),
        "agents = [{ name = \"fixture-agent\", argv = [\"/bin/sh\", \"-c\", \"exit 0\"] }]\n\
         [launch_choices.fixture]\nkind = \"Agent\"\npreset = \"fixture-agent\"\nmodel_policy = \"HarnessDefault\"\n",
    )
    .unwrap();
    live.start_binary();
    live.ready("feature/live-seed");
    let mut stream = p::connect_server(&live.socket).expect("connect to private Server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let p::Response::Hierarchy(hierarchy) =
        p::client::request(&mut stream, 910_000, p::Request::DashboardHello).unwrap()
    else {
        panic!("DashboardHello should return the live hierarchy");
    };

    let mut dashboard = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
    dashboard.install_area(Rect::new(0, 0, 80, 24));
    dashboard.install_hierarchy(hierarchy);
    let settings_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(
            Instant::now() < settings_deadline,
            "DashboardHello did not publish the initial Server settings"
        );
        let server =
            p::read_frame::<p::ServerMessage>(&mut stream).expect("read initial Dashboard events");
        let is_settings = matches!(
            &server,
            p::ServerMessage::Event(p::ServerEvent::SettingsChanged(_))
        );
        if let p::ServerMessage::Event(p::ServerEvent::SettingsChanged(report)) = &server {
            assert!(
                report
                    .settings
                    .agents
                    .iter()
                    .any(|agent| agent.name == "fixture-agent"),
                "live fixture must publish its configured test launcher: {:?}",
                report.settings.agents
            );
            assert_eq!(
                report.settings.launch_choices.get(PROJECT),
                Some(&p::LaunchChoice::Agent {
                    preset: "fixture-agent".into(),
                    model_policy: p::AgentModelPolicy::HarnessDefault,
                })
            );
        }
        dashboard.handle_server_message(server);
        if is_settings {
            break;
        }
    }
    dashboard.key(KeyCode::Char('n'));
    let initial = dashboard.drain_outbox();
    exchange_dashboard_requests(&mut dashboard, &mut stream, initial);
    assert!(palette_text(&dashboard).contains("Project"));
    assert!(palette_text(&dashboard).contains(PROJECT));
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Enter);
    dashboard.event_action(Event::Paste("feature/live-agent".into()));
    dashboard.key(KeyCode::Enter);
    wait_for_branch_hints(&mut dashboard);
    let form = palette_text(&dashboard);
    assert!(form.contains("feature-live-agent"), "{form}");
    assert!(form.contains("From main"), "{form}");

    let form_before_submit = palette_text(&dashboard);
    let action = dashboard.key_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
    let form_after_submit = palette_text(&dashboard);
    let requests = dashboard.drain_outbox();
    let [create] = requests.as_slice() else {
        panic!(
            "typed branch should issue one live workspace request: {requests:?}; action={action:?}; form before submit:\n{form_before_submit}\nform after submit:\n{form_after_submit}"
        );
    };
    let (workspace_id, token) = match &create.request {
        Request::CreateWorkspaceWithLaunch {
            project,
            id,
            launch,
            branch: p::BranchRequest::New { branch, base },
        } => {
            assert_eq!(project, PROJECT);
            assert_eq!(branch, "feature/live-agent");
            assert_eq!(base, "main");
            assert!(launch.is_none());
            (id.clone(), create.request_id)
        }
        other => panic!("expected new branch workspace request: {other:?}"),
    };
    p::write_frame(&mut stream, create).expect("send branch creation to live Server");

    let mut accepted = false;
    let mut workspace_succeeded = false;
    let mut allocation = None;
    let workspace_deadline = Instant::now() + Duration::from_secs(20);
    while !workspace_succeeded {
        assert!(
            Instant::now() < workspace_deadline,
            "workspace lifecycle timed out"
        );
        let server =
            p::read_frame::<p::ServerMessage>(&mut stream).expect("read live workspace lifecycle");
        match &server {
            p::ServerMessage::Response {
                request_id,
                response,
            } if *request_id == token => {
                assert_eq!(
                    response,
                    &p::Response::Ok,
                    "Server accepted branch creation"
                );
                accepted = true;
            }
            p::ServerMessage::Event(p::ServerEvent::LifecycleCompleted {
                client_token,
                op: p::LifecycleOp::CreateWorkspaceWithLaunch,
                outcome: p::LifecycleOutcome::Succeeded,
                target_workspace: Some(identity),
            }) if *client_token == token => {
                assert_eq!(identity.project, PROJECT);
                assert_eq!(identity.id, workspace_id);
                workspace_succeeded = true;
            }
            p::ServerMessage::Event(p::ServerEvent::LifecycleCompleted {
                client_token,
                outcome: p::LifecycleOutcome::Failed { code, message },
                ..
            }) if *client_token == token => {
                panic!("live branch creation failed: {code:?}: {message}");
            }
            _ => {}
        }
        for message in dashboard.handle_server_message(server) {
            if matches!(message.request, Request::AllocateInitialPromptIntent) {
                allocation = Some(message.clone());
            }
            p::write_frame(&mut stream, &message).expect("send chained Dashboard request");
        }
    }
    assert!(
        accepted,
        "the branch mutation received its acceptance response"
    );
    let allocation = allocation.expect("workspace success should allocate a prompt intent");

    let mut agent_launch = None;
    let mut prompt_intent = None;
    let prompt_deadline = Instant::now() + Duration::from_secs(8);
    while agent_launch.is_none() {
        assert!(
            Instant::now() < prompt_deadline,
            "prompt allocation timed out"
        );
        let server =
            p::read_frame::<p::ServerMessage>(&mut stream).expect("read prompt intent response");
        if let p::ServerMessage::Response {
            request_id,
            response,
        } = &server
            && *request_id == allocation.request_id
        {
            let p::Response::InitialPromptIntent(intent) = response else {
                panic!("prompt allocation returned an unexpected response: {response:?}");
            };
            prompt_intent = Some(intent.clone());
        }
        let followup = dashboard.handle_server_message(server);
        for message in followup {
            if matches!(message.request, Request::CreateAgentSessionWithPrompt(_)) {
                agent_launch = Some(message.clone());
            } else {
                p::write_frame(&mut stream, &message).expect("send chained Dashboard request");
            }
        }
    }
    let agent_launch = agent_launch.unwrap();
    let Request::CreateAgentSessionWithPrompt(launch) = &agent_launch.request else {
        unreachable!();
    };
    assert_eq!(launch.launch.session.project, PROJECT);
    assert_eq!(launch.launch.session.workspace, workspace_id);
    assert!(launch.prompt.is_empty());
    p::write_frame(&mut stream, &agent_launch).expect("start Agent in created workspace");

    let mut agent_accepted = false;
    let mut launched_session = None;
    let mut visible_session = None;
    let mut hierarchy_session_ids = HashSet::new();
    let agent_deadline = Instant::now() + Duration::from_secs(20);
    while !(agent_accepted && launched_session.is_some() && visible_session.is_some()) {
        assert!(
            Instant::now() < agent_deadline,
            "Agent launch was not accepted and published to the Dashboard"
        );
        let server =
            p::read_frame::<p::ServerMessage>(&mut stream).expect("read live Agent launch events");
        match &server {
            p::ServerMessage::Response {
                request_id,
                response,
            } if *request_id == agent_launch.request_id => {
                assert_eq!(
                    response,
                    &p::Response::Ok,
                    "Server accepted the Agent start"
                );
                agent_accepted = true;
            }
            p::ServerMessage::Event(p::ServerEvent::InitialPromptIntentChanged(snapshot))
                if prompt_intent
                    .as_ref()
                    .is_some_and(|intent| intent == &snapshot.id) =>
            {
                match snapshot.state {
                    p::InitialPromptIntentState::LaunchAccepted => {
                        let session = snapshot
                            .session
                            .expect("accepted prompt intent is bound to an Agent session");
                        assert_eq!(snapshot.project, PROJECT);
                        assert_eq!(snapshot.workspace, workspace_id);
                        launched_session = Some(session);
                    }
                    p::InitialPromptIntentState::Refused => {
                        panic!("live Agent launch was refused: {:?}", snapshot.message);
                    }
                    _ => {}
                }
            }
            p::ServerMessage::Event(p::ServerEvent::HierarchyChanged(hierarchy)) => {
                hierarchy_session_ids = hierarchy
                    .projects
                    .iter()
                    .find(|project| project.name == PROJECT)
                    .and_then(|project| {
                        project
                            .workspaces
                            .iter()
                            .find(|workspace| workspace.id == workspace_id)
                    })
                    .map(|workspace| {
                        workspace
                            .sessions
                            .iter()
                            .map(|session| session.id)
                            .collect()
                    })
                    .unwrap_or_default();
            }
            _ => {}
        }
        visible_session =
            launched_session.filter(|session| hierarchy_session_ids.contains(session));
        let _ = dashboard.handle_server_message(server);
    }
    assert!(
        agent_accepted,
        "Agent start received its acceptance response"
    );
    let created_session = launched_session.expect("accepted prompt intent names the new session");
    assert_eq!(visible_session, Some(created_session));

    let p::Response::Inventory { registry, .. } = live.request(p::Request::Inspect) else {
        panic!("live registry readback failed");
    };
    let project = registry
        .projects
        .iter()
        .find(|project| project.name == PROJECT)
        .unwrap();
    let workspace = project
        .workspaces
        .iter()
        .find(|workspace| workspace.id == workspace_id)
        .expect("typed branch workspace is registered");
    assert_eq!(workspace.branch, "feature/live-agent");
    let branch_commit = std::process::Command::new("git")
        .arg("-C")
        .arg(&project.repo)
        .args(["rev-parse", "feature/live-agent"])
        .output()
        .unwrap();
    let main_commit = std::process::Command::new("git")
        .arg("-C")
        .arg(&project.repo)
        .args(["rev-parse", "main"])
        .output()
        .unwrap();
    assert!(branch_commit.status.success());
    assert!(main_commit.status.success());
    assert_eq!(
        branch_commit.stdout, main_commit.stdout,
        "new branch starts at main"
    );
    assert!(workspace.path.exists(), "created worktree exists");
}

#[test]
fn row_create_menu_entries_open_existing_forms_with_the_row_context() {
    let mut create_workspace = dashboard_fixture();
    open_sidebar_create_menu(&mut create_workspace, "CONSIGINT");
    click_rendered_menu_item(&mut create_workspace, "New workspace");
    let form = palette_text(&create_workspace);
    assert!(form.contains("Create branch"), "{form}");
    assert!(form.contains("Project"), "{form}");
    assert!(
        form.contains("consigint"),
        "project context missing: {form}"
    );

    let mut project_agent = dashboard_fixture();
    open_sidebar_create_menu(&mut project_agent, "CONSIGINT");
    click_rendered_menu_item(&mut project_agent, "New agent session");
    let form = palette_text(&project_agent);
    assert!(form.contains("New agent session"), "{form}");
    assert!(
        form.contains("consigint"),
        "project context missing: {form}"
    );

    let mut workspace_agent = dashboard_fixture();
    open_sidebar_create_menu(&mut workspace_agent, "auth");
    click_rendered_menu_item(&mut workspace_agent, "New agent session");
    let form = palette_text(&workspace_agent);
    assert!(form.contains("New agent session"), "{form}");
    assert!(
        form.contains("consigint / auth"),
        "workspace context missing: {form}"
    );

    let mut workspace_terminal = dashboard_fixture();
    open_sidebar_create_menu(&mut workspace_terminal, "auth");
    click_rendered_menu_item(&mut workspace_terminal, "New terminal");
    let form = palette_text(&workspace_terminal);
    assert!(form.contains("Create terminal"), "{form}");
    assert!(
        form.contains("consigint / auth"),
        "workspace context missing: {form}"
    );
}

#[test]
fn root_workspace_menu_reuses_launch_reason_without_blocking_agent_setup() {
    let mut hierarchy = fixture_hierarchy();
    let root = hierarchy
        .projects
        .iter_mut()
        .find(|project| project.name == "consigint")
        .unwrap()
        .workspaces
        .iter_mut()
        .find(|workspace| workspace.id == "auth")
        .unwrap();
    root.root = true;
    root.name = "feature/x".into();
    root.warning = Some("root is on feature/x; check out main".into());
    let registry = registry_from_hierarchy(&hierarchy);

    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    open_sidebar_create_menu(&mut dashboard, "feature/x");
    let menu_rows = rendered_rows(&dashboard, 120, 40);
    let agent = menu_rows
        .iter()
        .find(|line| line.contains("New agent session"))
        .expect("agent setup entry");
    assert!(!agent.contains("root workspace is not on the default branch"));
    let terminal = menu_rows
        .iter()
        .find(|line| line.contains("New terminal"))
        .expect("terminal entry");
    assert!(
        terminal.contains("New terminal · root workspace is not on the default branch"),
        "{terminal}"
    );
    let disabled = rendered_sequence(&dashboard, "New terminal");
    let mut screen = Terminal::new(TestBackend::new(120, 40)).unwrap();
    screen
        .draw(|frame| draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    assert!(
        screen.backend().buffer()[(disabled.x, disabled.y)]
            .modifier
            .contains(Modifier::DIM),
        "unavailable entry is visibly dimmed"
    );

    assert_eq!(
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: disabled.x,
                row: disabled.y,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        ),
        DashboardAction::None
    );
    assert!(
        rendered_rows(&dashboard, 120, 40)
            .join("\n")
            .contains("New terminal")
    );

    dashboard.drain_outbox();
    assert_eq!(dashboard.key(KeyCode::Down), DashboardAction::Redraw);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::None);
    assert!(dashboard.drain_outbox().is_empty());
    assert!(
        rendered_rows(&dashboard, 120, 40)
            .join("\n")
            .contains("New terminal")
    );

    assert_eq!(dashboard.key(KeyCode::Up), DashboardAction::Redraw);
    let action = dashboard.key(KeyCode::Enter);
    assert_eq!(action, DashboardAction::Redraw);
    answer_palette_inspect_with_registry(&mut dashboard, action, registry);
    let form = palette_text(&dashboard);
    assert!(form.contains("New agent session"), "{form}");
    assert!(
        form.contains("launch blocked"),
        "root target should retain its form guard: {form}"
    );
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
    assert_eq!(dashboard.key(KeyCode::Char('n')), DashboardAction::Redraw);
    assert!(palette_text(&dashboard).contains("New agent session"));
}

#[test]
fn created_session_enters_terminal_mode() {
    let mut dashboard = dashboard_fixture();
    open_new_terminal_from_menu(&mut dashboard);
    assert_eq!(
        dashboard.event_action(Event::Key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT,)
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
    let Request::CreateSession(request) = &message.request else {
        panic!("wrong request");
    };
    let session = SessionSummary {
        scope: ovrcr_protocol::SessionScope::Workspace,
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
        assignment_control: None,
        label: request.label.clone().unwrap_or_default(),
        pid: Some(1),
        started_unix_ms: Some(0),
        last_activity_unix_ms: None,
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
fn browse_footer_lists_only_menu_commands_and_help() {
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
        "BROWSE  Space Menu  : Commands  ? Help  m Recent activity",
        "{footer}"
    );
}

#[test]
fn workspace_menu_lists_new_agent_and_new_terminal_actions() {
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
        create.contains("n  New agent session") && create.contains("t  New terminal"),
        "the Workspace group must list both named creation actions: {create}"
    );
    dashboard.key(KeyCode::Down);
    let text = palette_text(&dashboard);
    let detail = text
        .lines()
        .skip_while(|line| !line.contains("Create a terminal in"))
        .take_while(|line| !line.contains("└ Esc close"))
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        detail.contains("consigint /") && detail.contains("auth"),
        "the selected t hint must name its workspace below the list: {detail}"
    );
    dashboard.key(KeyCode::Enter);
    let form = palette_text(&dashboard);
    assert!(!form.contains("Which key"), "{form}");
    assert!(form.contains("Start: Terminal"), "{form}");
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
fn tabbing_to_workspace_root_refreshes_its_derived_listing() {
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
    // `demo` has no folders to complete, so Tab moves on: Name, then Workspace root.
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Tab);
    assert!(dashboard.drain_outbox().is_empty());
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
    assert!({
        let _ = action;
        matches!(dashboard.drain_outbox().as_slice(), [_])
    });
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let close = only_request(dashboard.drain_outbox());
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
        let _ = dashboard.key(KeyCode::Enter);
        if let Some(message) = optional_request(dashboard.drain_outbox()) {
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
    assert!(text.contains("X  Close terminal"), "{text}");
    assert!(!text.contains("x  Close terminal"), "{text}");
    // Lowercase x never closes a session: the menu stays open and nothing is asked.
    assert_eq!(
        dashboard.key(KeyCode::Char('x')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let text = palette_text(&dashboard);
    assert!(
        text.contains("Space t") && !text.contains("Close terminal?"),
        "{text}"
    );
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Backspace);
    let text = palette_text(&dashboard);
    assert!(text.contains("t  Terminal"));
    assert!(!text.contains("X  Close terminal"));
    dashboard.key(KeyCode::Char('t'));
    assert_eq!(
        dashboard.key(KeyCode::Char('p')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
            // `auth` has two running sessions: removal stops the first one first.
            'w',
            Request::CloseTerminal {
                session: SessionId(1),
                expected_run: ovrcr::protocol::SessionRunId(1),
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
            // Every destructive group action is uppercase; lowercase x does nothing.
            assert_eq!(dashboard.key(KeyCode::Char('x')), DashboardAction::Redraw);
            assert!(dashboard.drain_outbox().is_empty());
            let text = palette_text(&dashboard);
            assert!(
                text.contains("Which key") && !text.contains("Confirm action"),
                "{group}: {text}"
            );
            assert!(!text.contains("Close terminal?"), "{group}: {text}");
            assert_eq!(dashboard.key(KeyCode::Char('X')), DashboardAction::Redraw);
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
                assert!(
                    text.contains("still has 2 running sessions: review, local"),
                    "{text}"
                );
                assert!(text.contains("Stop them"), "{text}");
            }
            if confirm {
                assert_eq!(
                    dashboard.key(KeyCode::Enter),
                    ovrcr::tui::DashboardAction::Redraw
                );
                let message = only_request(dashboard.drain_outbox());
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
            // Its running `local` shell is stopped before the removal.
            "progress",
            'w',
            Request::CloseTerminal {
                session: SessionId(3),
                expected_run: ovrcr::protocol::SessionRunId(1),
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
        dashboard.key(KeyCode::Char('X'));
        assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
        assert!(palette_text(&dashboard).contains("Confirm action"));
        assert_eq!(
            dashboard.key(KeyCode::Enter),
            ovrcr::tui::DashboardAction::Redraw
        );
        let message = only_request(dashboard.drain_outbox());
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
    click_whichkey_text(&mut dashboard, "t  New terminal", false);
    assert!(palette_text(&dashboard).contains("Start: Terminal"));
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
    assert_eq!(
        dashboard.key(KeyCode::Char('r')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    let full = "BROWSE  Space Menu  : Commands  ? Help  m Recent activity";
    assert_eq!(rendered_footer(&dashboard, 120).trim_end(), full);
    for width in [20, 40, 80, 120] {
        let text = rendered_footer(&dashboard, width);
        assert!(text.starts_with("BROWSE  Space Menu"), "{text}");
        assert!(full.starts_with(text.trim_end()), "partial hint: {text}");
    }
    // Selection and an empty hierarchy change the menu, not the footer.
    dashboard.install_focus(SessionId(3));
    assert_eq!(rendered_footer(&dashboard, 120).trim_end(), full);
    dashboard.install_hierarchy(HierarchySnapshot {
        usage: ovrcr_protocol::UsageSnapshot::default(),
        projects: vec![],
    });
    assert_eq!(rendered_footer(&dashboard, 120).trim_end(), full);
}

#[test]
fn recent_sidebar_toggle_is_in_browse_footer_view_popup_and_palette() {
    let mut dashboard = dashboard_fixture();
    assert!(rendered_footer(&dashboard, 120).contains("m Recent activity"));

    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('v'));
    let menu = whichkey_menu_text(&dashboard);
    assert!(menu.contains("Recent activity"), "View popup: {menu}");
    dashboard.key(KeyCode::Esc);

    palette_search(&mut dashboard, "Recent activity");
    assert!(palette_text(&dashboard).contains("Recent activity"));
}

#[test]
fn ux_search_keeps_session_identity_and_filters_complete_context() {
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "local");
    // Sessions read `project / branch / title`; unique rows carry no ID.
    for width in [40, 80, 120] {
        let rows = rendered_rows(&dashboard, width, 24);
        for place in [
            "spacelift-agent / progress / local",
            "consigint / lifecycle / local",
            "consigint / auth / local",
        ] {
            assert!(rows.iter().any(|row| row.contains(place)), "{rows:?}");
        }
        assert!(!rows.iter().any(|row| row.contains("(#")), "{rows:?}");
    }
    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Enter);
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    // The ID stays searchable although it is not drawn.
    for query in ["spacelift-agent", "progress", "#3"] {
        palette_search(&mut dashboard, query);
        dashboard.key(KeyCode::Enter);
        assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
    }
    // Two sessions that read the same keep their IDs, and clipping keeps them visible.
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[0].sessions[0].name = "local".into();
    hierarchy.projects[0].workspaces[0].sessions[0].name = "界🙂".repeat(50);
    dashboard.install_hierarchy(hierarchy);
    palette_search(&mut dashboard, "lifecycle local");
    let text = palette_text(&dashboard);
    assert!(
        text.contains("consigint / lifecycle / local (#2)")
            && text.contains("consigint / lifecycle / local (#4)"),
        "{text}"
    );
    dashboard.key(KeyCode::Esc);
    palette_search(&mut dashboard, "#3");
    for width in [40, 80, 120] {
        let rows = rendered_rows(&dashboard, width, 24);
        let row = rows
            .iter()
            .find(|row| row.contains("spacelift-agent / progress / 界"))
            .expect("visible place");
        assert!(row.contains('…'), "{row}");
        assert!(!row.contains("(#3)"), "a unique row needs no ID: {row}");
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
fn palette_lists_only_the_actions_the_target_can_run() {
    let mut dashboard = dashboard_fixture();
    // Session #1 runs, is not paused, has no unread reply and no assignment.
    for hidden in ["take control", "return to pi", "resume", "mark reviewed"] {
        palette_search(&mut dashboard, hidden);
        let text = palette_text(&dashboard);
        assert!(
            text.contains("No matching actions or terminals"),
            "{hidden}: {text}"
        );
        dashboard.key(KeyCode::Esc);
    }
    palette_search(&mut dashboard, "pause");
    assert!(palette_text(&dashboard).contains("p — Pause the processes"));
    dashboard.key(KeyCode::Esc);
    // A pane still waiting for its screen keeps the action listed with its
    // reason, as the key popup does; Enter on it does nothing yet.
    dashboard.install_unready(SessionId(1));
    palette_search(&mut dashboard, "copy screen");
    let text = palette_text(&dashboard);
    assert!(
        text.contains("Copy screen: waiting for acknowledged screen"),
        "{text}"
    );
    dashboard.key(KeyCode::Enter);
    assert!(dashboard.drain_outbox().is_empty());
    assert!(palette_text(&dashboard).contains("Command palette"));
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('t'));
    let menu = whichkey_menu_text(&dashboard);
    assert!(menu.contains("c  Copy screen"), "{menu}");
    dashboard.key(KeyCode::Esc);
    // A root workspace off its default branch keeps the named Terminal action
    // discoverable with the reason it cannot run.
    let mut blocked = dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[1].warning = Some("main is not checked out".into());
    blocked.install_hierarchy(hierarchy);
    blocked.key(KeyCode::Char(' '));
    blocked.key(KeyCode::Char('w'));
    blocked.key(KeyCode::Down);
    let text = whichkey_menu_text(&blocked);
    assert!(
        text.contains("t  New terminal")
            && text.contains("root workspace is not on the default branch"),
        "{text}"
    );
    // With no project there is nothing to create a terminal or workspace in.
    let mut empty = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    palette_search(&mut empty, "create");
    let text = palette_text(&empty);
    assert!(!text.contains("Create branch (w)"), "{text}");
    empty.key(KeyCode::Esc);
    palette_search(&mut empty, "register");
    assert!(palette_text(&empty).contains("Register project (a)"));
}

#[test]
fn palette_row_under_the_cursor_stays_put_when_the_pane_becomes_ready() {
    let selected = |dashboard: &Dashboard| {
        palette_text(dashboard)
            .lines()
            .find_map(|line| {
                line.split_once("│› ")
                    .map(|(_, rest)| rest.split('│').next().unwrap().trim().to_owned())
            })
            .expect("a selected palette row")
    };
    let mut dashboard = dashboard_fixture();
    dashboard.install_unready(SessionId(1));
    palette_search(&mut dashboard, "");
    for _ in 0..200 {
        dashboard.key(KeyCode::Down);
    }
    let before = selected(&dashboard);
    // The screen acknowledgement lands while the palette is open.
    dashboard.install_screen(SessionId(1), &[]);
    assert_eq!(selected(&dashboard), before);
}

#[test]
fn unfiltered_palette_starts_with_creation_and_keeps_settings_rows_for_search() {
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "");
    let text = palette_text(&dashboard);
    assert!(text.contains("› New agent session (n)"), "{text}");
    assert!(!text.contains("› Archived sessions"), "{text}");
    // The last entries are key actions; settings rows are not listed unasked.
    for _ in 0..200 {
        dashboard.key(KeyCode::Down);
    }
    let text = palette_text(&dashboard);
    assert!(!text.contains("Settings: "), "{text}");
    dashboard.key(KeyCode::Esc);
    palette_search(&mut dashboard, "archived");
    assert!(palette_text(&dashboard).contains("› Archived sessions"));
}

#[test]
fn palette_finds_settings_by_label_or_key_and_opens_that_row() {
    let mut dashboard = dashboard_fixture();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
        Box::new(ovrcr::settings::parse(
            std::path::Path::new("/srv/dashboard.toml"),
            "",
        )),
    )));
    for (query, label, detail) in [
        ("theme", "Settings: Theme", "Appearance · theme"),
        (
            "notifications",
            "Settings: Desktop notifications",
            "Alerts · desktop_notifications",
        ),
        ("sound", "Settings: Ready sound", "Alerts · ready_sound"),
        (
            "branch_prefix",
            "Settings: Branch prefix",
            "Workspaces · branch_prefix",
        ),
    ] {
        palette_search(&mut dashboard, query);
        let text = palette_text(&dashboard);
        assert!(
            text.contains(label) && text.contains(detail),
            "{query}: {text}"
        );
        dashboard.key(KeyCode::Esc);
    }
    palette_search(&mut dashboard, "theme");
    dashboard.key(KeyCode::Enter);
    let rows = rendered_rows(&dashboard, 120, 60);
    let theme = rows
        .iter()
        .find(|row| row.contains("Theme") && row.contains("Catppuccin Mocha"))
        .unwrap_or_else(|| panic!("Settings shows the Theme row: {rows:?}"));
    assert!(
        theme.contains('›'),
        "Settings opens on the Theme row: {theme}"
    );
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Esc);
    // A key finds its row too; the Operator model is one optional Pi identity.
    palette_search(&mut dashboard, "operator.model");
    dashboard.key(KeyCode::Enter);
    let rows = rendered_rows(&dashboard, 120, 60);
    let model = rows
        .iter()
        .find(|row| row.contains("Model") && row.contains('›'))
        .unwrap_or_else(|| panic!("Settings opens on the Operator Model row: {rows:?}"));
    assert!(
        model.contains("unset"),
        "an unset override reads unset: {model}"
    );
    assert!(!rows.join("\n").contains("chat_gpt"), "{rows:?}");
}

#[test]
fn creation_forms_keep_their_title_and_active_field_visible_at_supported_sizes() {
    let mut agent = dashboard_fixture();
    agent.key(KeyCode::Char('n'));
    for (width, height) in [(60, 18), (80, 24)] {
        let rows = rendered_rows(&agent, width, height);
        assert!(
            rows.iter()
                .any(|row| row.contains('┌') && row.contains("New agent session")),
            "{rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains('›')),
            "active field missing: {rows:?}"
        );
    }

    for (key, title) in [('w', "Create branch"), ('a', "Register project")] {
        let mut dashboard = dashboard_fixture();
        dashboard.key(KeyCode::Char(key));
        for (width, height) in [(60, 18), (80, 24)] {
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

    let mut terminal = dashboard_fixture();
    open_new_terminal_from_menu(&mut terminal);
    for (width, height) in [(60, 18), (80, 24)] {
        let rows = rendered_rows(&terminal, width, height);
        assert!(
            rows.iter()
                .any(|row| row.contains('┌') && row.contains("Create terminal")),
            "{rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains('›')),
            "active field missing: {rows:?}"
        );
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
        // No running sessions, so the removal itself is what confirming sends.
        let mut dashboard = stopped_fixture();
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
        assert_eq!(
            dashboard.key(KeyCode::Enter),
            ovrcr::tui::DashboardAction::Redraw
        );
        let message = only_request(dashboard.drain_outbox());
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
        dashboard.install_hierarchy(HierarchySnapshot {
            usage: ovrcr_protocol::UsageSnapshot::default(),
            projects: vec![],
        });
        palette_search(&mut dashboard, query);
        dashboard.key(KeyCode::Enter);
        for key in [KeyCode::Down, KeyCode::Tab, KeyCode::Enter] {
            assert!({
                let _ = dashboard.key(key);
                dashboard.drain_outbox().is_empty()
            });
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
    // The title leads the header's first line; the second no longer repeats it.
    let narrow_title = (40..80)
        .map(|x| narrow.backend().buffer()[(x, 1)].symbol())
        .collect::<String>();
    assert!(
        narrow_title.starts_with("Same title (#1)  "),
        "title must remain visible at 80 columns: {narrow_title}"
    );
    let second_line = (40..80)
        .map(|x| narrow.backend().buffer()[(x, 2)].symbol())
        .collect::<String>();
    assert!(!second_line.contains("Same title"), "{second_line}");
    palette_search(&mut dashboard, "Same title");
    let text = palette_text(&dashboard);
    assert!(
        text.contains("consigint / auth / Same title")
            && text.contains("spacelift-agent / progress / Same title"),
        "{text}"
    );
    dashboard.key(KeyCode::Esc);
    palette_search(&mut dashboard, "Rename terminal");
    dashboard.key(KeyCode::Enter);
    dashboard.ctrl('u');
    let rename_text = palette_text(&dashboard);
    assert!(rename_text.contains("original name"), "{rename_text}");
    assert!(!rename_text.contains("Automatic"), "{rename_text}");
    dashboard.event_action(Event::Paste("Pinned by user".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let pin = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let reset = only_request(dashboard.drain_outbox());
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
    let repo = trunk_hint_repository();
    for hierarchy_first in [false, true] {
        let mut dashboard = dashboard_fixture();
        open_workspace_form_with_repo(&mut dashboard, repo.path());
        dashboard.event_action(Event::Paste("pick-demo".into()));
        assert_eq!(
            dashboard.key(KeyCode::Enter),
            ovrcr::tui::DashboardAction::Redraw
        );
        let message = only_request(dashboard.drain_outbox());
        let Request::CreateWorkspace { id, .. } = &message.request else {
            panic!("{:?}", message.request);
        };
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
        workspace.name = "pick-demo".into();
        workspace.sessions.clear();
        accept(&mut dashboard, &message);
        let readbacks = if hierarchy_first {
            dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
                hierarchy.clone(),
            )));
            assert_eq!(
                dashboard.focused_session(),
                Some(SessionId(1)),
                "unacknowledged creation must not change selection"
            );
            lifecycle_completed(
                &mut dashboard,
                &message,
                ovrcr::protocol::LifecycleOutcome::Succeeded,
            )
        } else {
            let readbacks = lifecycle_completed(
                &mut dashboard,
                &message,
                ovrcr::protocol::LifecycleOutcome::Succeeded,
            );
            assert_eq!(
                dashboard.focused_session(),
                Some(SessionId(1)),
                "wait until the created workspace exists in hierarchy"
            );
            dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
                hierarchy.clone(),
            )));
            readbacks
        };
        let outgoing = answer_branch_readbacks(&mut dashboard, readbacks, &hierarchy);
        assert_eq!(
            dashboard.focused_session(),
            None,
            "created empty workspace must replace the previous pane selection"
        );
        assert!(
            !outgoing.iter().any(|message| matches!(
                &message.request,
                Request::SetView { view } if view.focused.is_some()
            )),
            "branch-only creation must not attach a session: {outgoing:?}"
        );
        assert!(
            !outgoing.iter().any(|message| matches!(
                &message.request,
                Request::CreateSession(_) | Request::CreateAgentSession(_)
            )),
            "branch-only creation must not launch a session: {outgoing:?}"
        );
        let text = palette_text(&dashboard);
        assert!(text.contains("consigint / pick-demo"), "{text}");
        assert!(text.contains("n        new agent session"), "{text}");
        assert!(text.contains("No sessions yet"), "{text}");
        assert!(!text.contains("┌ Create branch"), "{text}");
        dashboard.key(KeyCode::Char('n'));
        assert!(palette_text(&dashboard).contains("consigint / pick-demo"));
    }
}

#[test]
fn reopen_confirm_names_stopped_processes_and_does_not_treat_retry_as_ack() {
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(2));
    palette_search(&mut dashboard, "Reopen in a fresh shell");
    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let first = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let retry = only_request(dashboard.drain_outbox());
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
    let mut dashboard = dashboard_fixture();
    open_new_terminal_from_menu(&mut dashboard);
    assert_eq!(
        dashboard.event_action(Event::Key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT,)
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
    let Request::CreateSession(request) = &message.request else {
        panic!("wrong request");
    };
    let retained = SessionSummary {
        scope: ovrcr_protocol::SessionScope::Workspace,
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
        assignment_control: None,
        label: request.label.clone().unwrap_or_default(),
        pid: None,
        started_unix_ms: None,
        last_activity_unix_ms: None,
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
    let _ = dashboard.key(KeyCode::Enter);
    if let Some(next) = optional_request(dashboard.drain_outbox()) {
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let next = only_request(dashboard.drain_outbox());
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
    let mut dashboard = dashboard_fixture();
    let focused = dashboard.focused_session();
    open_new_terminal_from_menu(&mut dashboard);
    assert_eq!(
        dashboard.event_action(Event::Key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT,)
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
            scope: ovrcr_protocol::SessionScope::Workspace,
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
            assignment_control: None,
            label: request.label.clone().unwrap_or_default(),
            pid: None,
            started_unix_ms: None,
            last_activity_unix_ms: None,
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
    let _ = dashboard.key(KeyCode::Enter);
    if let Some(next) = optional_request(dashboard.drain_outbox()) {
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
    let mut dashboard = dashboard_fixture();
    dashboard.install_focus(SessionId(2));
    palette_search(&mut dashboard, "Reopen in a fresh shell");
    dashboard.key(KeyCode::Enter);
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[0].sessions[0].run = ovrcr::protocol::SessionRunId(4);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let first = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let second = only_request(dashboard.drain_outbox());
    assert_eq!(
        second.request,
        Request::ReopenSession {
            session: SessionId(2),
            expected_run: ovrcr::protocol::SessionRunId(4),
            acknowledge_stopped: false,
        }
    );
}

fn lifecycle_completed(
    dashboard: &mut Dashboard,
    request: &ClientMessage,
    outcome: ovrcr::protocol::LifecycleOutcome,
) -> Vec<ClientMessage> {
    let op = match request.request {
        Request::CloseTerminal { .. } => ovrcr::protocol::LifecycleOp::CloseTerminal,
        Request::RemoveWorkspace { .. } => ovrcr::protocol::LifecycleOp::RemoveWorkspace,
        Request::CreateSession(_) | Request::CreateAgentSession(_) => {
            ovrcr::protocol::LifecycleOp::CreateSession
        }
        Request::CreateWorkspace { .. } => ovrcr::protocol::LifecycleOp::CreateWorkspace,
        _ => ovrcr::protocol::LifecycleOp::CreateWorkspaceWithLaunch,
    };
    let target_workspace = match &request.request {
        Request::CreateWorkspace { project, id, .. }
        | Request::CreateWorkspaceWithLaunch { project, id, .. } => {
            Some(ovrcr::protocol::WorkspaceLifecycleIdentity {
                project: project.clone(),
                id: id.clone(),
            })
        }
        _ => None,
    };
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::LifecycleCompleted {
        client_token: request.request_id,
        op,
        outcome,
        target_workspace,
    }))
}

fn answer_branch_readbacks(
    dashboard: &mut Dashboard,
    requests: Vec<ClientMessage>,
    hierarchy: &HierarchySnapshot,
) -> Vec<ClientMessage> {
    assert_eq!(
        requests.len(),
        2,
        "branch creation requires Inspect and List"
    );
    let registry = registry_from_hierarchy(hierarchy);
    let mut outgoing = Vec::new();
    for message in requests {
        let response = match message.request {
            Request::Inspect => Response::Inventory {
                registry: registry.clone(),
                sessions: vec![],
            },
            Request::List => Response::Hierarchy(hierarchy.clone()),
            other => panic!("unexpected branch readback: {other:?}"),
        };
        outgoing.extend(dashboard.handle_server_message(ServerMessage::Response {
            request_id: message.request_id,
            response,
        }));
    }
    outgoing
}

/// Answers a Lifecycle job the way the Server does: accepted at once, done later.
fn accept(dashboard: &mut Dashboard, request: &ClientMessage) {
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Ok,
            })
            .is_empty()
    );
}

#[test]
fn removing_a_workspace_with_running_sessions_stops_them_then_removes_it() {
    use ovrcr::protocol::{LifecycleOutcome, SessionRunId};
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let text = palette_text(&dashboard);
    assert!(
        text.contains("consigint / auth still has 2 running sessions: review, local"),
        "{text}"
    );
    assert!(!text.contains("Force"), "{text}");
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    // One Lifecycle job at a time: each step goes out when the previous one succeeds.
    let first = only_request(dashboard.drain_outbox());
    assert_eq!(
        first.request,
        Request::CloseTerminal {
            session: SessionId(1),
            expected_run: SessionRunId(1),
        }
    );
    accept(&mut dashboard, &first);
    assert!(
        !palette_text(&dashboard).contains("Confirm action"),
        "the confirmation steps aside while the sessions stop"
    );
    let second = only_request(lifecycle_completed(
        &mut dashboard,
        &first,
        LifecycleOutcome::Succeeded,
    ));
    assert_eq!(
        second.request,
        Request::CloseTerminal {
            session: SessionId(5),
            expected_run: SessionRunId(1),
        }
    );
    accept(&mut dashboard, &second);
    let removal = only_request(lifecycle_completed(
        &mut dashboard,
        &second,
        LifecycleOutcome::Succeeded,
    ));
    assert_eq!(
        removal.request,
        Request::RemoveWorkspace {
            project: "consigint".into(),
            name: "auth".into(),
            force: false,
        }
    );
    accept(&mut dashboard, &removal);
    assert!(
        lifecycle_completed(&mut dashboard, &removal, LifecycleOutcome::Succeeded).is_empty(),
        "nothing follows the removal"
    );
    assert!(!palette_text(&dashboard).contains("Confirm action"));
    assert!(!footer_text(&dashboard).contains("Esc dismiss"));
}

#[test]
fn removal_blocked_by_sessions_never_offers_force() {
    use ovrcr::protocol::LifecycleOutcome;
    // Sessions that started after the confirmation: named, then stopped first.
    let mut dashboard = stopped_fixture();
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let removal = only_request(dashboard.drain_outbox());
    assert!(matches!(
        removal.request,
        Request::RemoveWorkspace { force: false, .. }
    ));
    accept(&mut dashboard, &removal);
    let mut started = stopped_hierarchy();
    started.projects[1].workspaces[1].sessions[0].phase = SessionPhase::Running;
    started.projects[1].workspaces[1].sessions[0].title = Some("verify spinner".into());
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(started)));
    lifecycle_completed(
        &mut dashboard,
        &removal,
        LifecycleOutcome::Failed {
            code: ErrorCode::SessionsRemain,
            message:
                "sessions block removal of workspace consigint/auth: live: #1; kill or close them"
                    .into(),
        },
    );
    let text = palette_text(&dashboard);
    assert!(
        text.contains("consigint / auth still has 1 running session: verify spinner"),
        "{text}"
    );
    assert!(!text.contains("Force"), "{text}");
    assert!(!text.contains("SessionsRemain"), "{text}");
    assert!(!footer_text(&dashboard).contains("Failed"), "no failed row");
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        only_request(dashboard.drain_outbox()).request,
        Request::CloseTerminal {
            session: SessionId(1),
            expected_run: ovrcr::protocol::SessionRunId(1),
        }
    );

    // Blockers the hierarchy cannot show (a task run, an unacknowledged
    // archived session) are explained in plain words; still no Force.
    let mut dashboard = stopped_fixture();
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let removal = only_request(dashboard.drain_outbox());
    accept(&mut dashboard, &removal);
    lifecycle_completed(
        &mut dashboard,
        &removal,
        LifecycleOutcome::Failed {
            code: ErrorCode::SessionsRemain,
            message: "task run remains for workspace consigint/auth".into(),
        },
    );
    let text = palette_text(&dashboard);
    assert!(
        text.contains("Couldn't remove consigint / auth: task run remains for workspace"),
        "{text}"
    );
    assert!(
        !text.contains("Force") && !text.contains("Discard"),
        "{text}"
    );
    assert!({
        let _ = dashboard.key(KeyCode::Esc);
        dashboard.drain_outbox().is_empty()
    });
}

/// Review S1: a stopped session awaiting acknowledgement would refuse the
/// removal after the running ones were already stopped. It is named first and
/// nothing is sent.
#[test]
fn removal_waits_for_acknowledgement_before_stopping_anything() {
    let mut hierarchy = fixture_hierarchy();
    // auth: `review` still runs, `local` stopped with ownership uncertain.
    let local = &mut hierarchy.projects[1].workspaces[1].sessions[1];
    local.phase = SessionPhase::Stopped;
    local.pid = None;
    local.recovery = Some(ovrcr::protocol::SessionRecovery {
        conversation: None,
        attached: false,
        requires_ack: true,
        unavailable: None,
        failure: None,
    });
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let text = palette_words(&dashboard);
    assert!(
        text.contains(
            "consigint / auth has 1 stopped session that may still have processes running: local (#5)."
        ),
        "{text}"
    );
    assert!(text.contains("\"Acknowledge stopped\""), "{text}");
    assert!(!text.contains("Stop it"), "{text}");
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        dashboard.drain_outbox().is_empty(),
        "no session is stopped for a removal that would be refused"
    );
    assert!(!palette_text(&dashboard).contains("Confirm action"));
}

/// Review S2: the Server's `--force` advice never reaches the Dashboard.
#[test]
fn archived_blockers_point_at_acknowledge_stopped_not_force() {
    use ovrcr::protocol::LifecycleOutcome;
    let mut dashboard = stopped_fixture();
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let removal = only_request(dashboard.drain_outbox());
    accept(&mut dashboard, &removal);
    lifecycle_completed(
        &mut dashboard,
        &removal,
        LifecycleOutcome::Failed {
            code: ErrorCode::SessionsRemain,
            message: "sessions block removal of workspace consigint/auth: unacknowledged stopped: #77 (archived); run `ovrcr terminal acknowledge-stopped <id>` or `ovrcr terminal remove <id>`, or remove with --force".into(),
        },
    );
    let text = palette_words(&dashboard);
    assert!(
        text.contains("stopped sessions may still have processes running: #77 (archived); once they have stopped, choose \"Acknowledge stopped\" for each"),
        "{text}"
    );
    for wrong in ["--force", "ovrcr terminal", "SessionsRemain", "Force"] {
        assert!(!text.contains(wrong), "{wrong}: {text}");
    }
}

#[test]
fn uncommitted_changes_alone_offer_discard_in_plain_words() {
    use ovrcr::protocol::LifecycleOutcome;
    // A detached checkout cannot be saved to origin/wip/<branch>.
    let mut hierarchy = stopped_hierarchy();
    hierarchy.projects[1].workspaces[1].name = "detached @ 1a2b3c4".into();
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let removal = only_request(dashboard.drain_outbox());
    let Request::RemoveWorkspace {
        project,
        name,
        force: false,
    } = removal.request.clone()
    else {
        panic!("first removal must not force: {:?}", removal.request)
    };
    accept(&mut dashboard, &removal);
    lifecycle_completed(
        &mut dashboard,
        &removal,
        LifecycleOutcome::Failed {
            code: ErrorCode::DirtyWorktree,
            message: "worktree has changes".into(),
        },
    );
    let text = palette_words(&dashboard);
    assert!(
        text.contains(
            "consigint / detached @ 1a2b3c4 has uncommitted changes. \
             Discard them and remove the workspace? The branch is kept."
        ),
        "{text}"
    );
    assert!(
        text.contains("Couldn't remove consigint / detached @ 1a2b3c4: worktree has changes"),
        "{text}"
    );
    assert!(!text.contains("DirtyWorktree"), "{text}");
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        only_request(dashboard.drain_outbox()).request,
        Request::RemoveWorkspace {
            project,
            name,
            force: true,
        }
    );
}

/// Submits Create workspace for `feature/status-check` and lets the Server
/// accept it; returns the request and its workspace id.
fn create_status_check_workspace(
    dashboard: &mut Dashboard,
    repo: &std::path::Path,
) -> (ClientMessage, String) {
    open_workspace_form_with_repo(dashboard, repo);
    dashboard.event_action(Event::Paste("feature/status-check".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let create = only_request(dashboard.drain_outbox());
    let Request::CreateWorkspace { id, .. } = &create.request else {
        panic!("{:?}", create.request);
    };
    let id = id.clone();
    accept(dashboard, &create);
    let text = palette_text(dashboard);
    assert!(
        text.contains("┌ Create branch") && text.contains("Working…"),
        "the accepted branch remains visible while Git works: {text}"
    );
    (create, id)
}

#[test]
fn failed_workspace_creation_returns_to_the_form_with_its_values() {
    use ovrcr::protocol::LifecycleOutcome;
    let repo = trunk_hint_repository();
    let mut dashboard = dashboard_fixture();
    open_workspace_form_with_repo(&mut dashboard, repo.path());
    dashboard.event_action(Event::Paste("feature/status-check".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let create = only_request(dashboard.drain_outbox());
    let Request::CreateWorkspace { id, branch, .. } = &create.request else {
        panic!("{:?}", create.request);
    };
    assert_eq!(
        branch,
        &ovrcr::protocol::BranchRequest::New {
            branch: "feature/status-check".into(),
            base: "trunk".into(),
        }
    );
    let id = id.clone();
    accept(&mut dashboard, &create);
    let text = palette_text(&dashboard);
    assert!(
        text.contains("┌ Create branch") && text.contains("Working…"),
        "the accepted branch remains visible while Git works: {text}"
    );
    lifecycle_completed(
        &mut dashboard,
        &create,
        LifecycleOutcome::Failed {
            code: ErrorCode::Conflict,
            message: format!(
                "create workspace consigint/{id}: git command failed (exit status: 255): fatal: a branch named 'feature/status-check' already exists"
            ),
        },
    );
    let screen = palette_text(&dashboard);
    assert!(screen.contains("┌ Create branch"), "{screen}");
    assert!(!screen.contains("Not created"), "no failed row: {screen}");
    assert!(!screen.contains(&id), "no internal id: {screen}");
    let words = palette_words(&dashboard);
    assert!(
        words.contains(
            "Couldn't create the branch: a branch named 'feature/status-check' already exists"
        ),
        "{words}"
    );
    assert!(!words.contains("Conflict"), "{words}");
    assert!(
        words.contains("Branch name › feature/status-check"),
        "{words}"
    );
    // Fixing the name retries the same creation.
    dashboard.event_action(Event::Paste("-2".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let retry = only_request(dashboard.drain_outbox());
    let Request::CreateWorkspace {
        id: retry_id,
        branch,
        ..
    } = retry.request
    else {
        panic!("{:?}", retry.request);
    };
    assert_eq!(retry_id, id);
    assert_eq!(
        branch,
        ovrcr::protocol::BranchRequest::New {
            branch: "feature/status-check-2".into(),
            base: "trunk".into(),
        }
    );
}

#[test]
fn partial_branch_creation_requires_readback_before_retrying() {
    use ovrcr::protocol::LifecycleOutcome;
    let repo = trunk_hint_repository();
    let mut dashboard = dashboard_fixture();
    let (create, id) = create_status_check_workspace(&mut dashboard, repo.path());
    let readbacks = lifecycle_completed(
        &mut dashboard,
        &create,
        LifecycleOutcome::Failed {
            code: ErrorCode::PartialFailure,
            message: format!("worktree for workspace {id} remains after a partial create"),
        },
    );
    let mut hierarchy = extra_workspace_hierarchy(id);
    let created = hierarchy.projects[1].workspaces.last_mut().unwrap();
    created.name = "feature/status-check".into();
    created.sessions.clear();
    answer_branch_readbacks(&mut dashboard, readbacks, &hierarchy);

    let words = palette_words(&dashboard);
    assert!(
        words.contains("feature/status-check exists, but the lifecycle result was not successful; retry is blocked"),
        "{words}"
    );
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn named_terminal_failure_keeps_the_form_and_can_be_dismissed() {
    use ovrcr::protocol::LifecycleOutcome;
    let mut dashboard = dashboard_fixture();
    open_new_terminal_from_menu(&mut dashboard);
    assert_eq!(
        dashboard.event_action(Event::Key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT,)
        )),
        ovrcr::tui::DashboardAction::Redraw
    );
    let create = only_request(dashboard.drain_outbox());
    let Request::CreateSession(session) = &create.request else {
        panic!("New terminal must use CreateSession: {:?}", create.request);
    };
    assert_eq!(session.kind, ovrcr_protocol::SessionKind::Terminal);
    accept(&mut dashboard, &create);
    lifecycle_completed(
        &mut dashboard,
        &create,
        LifecycleOutcome::Failed {
            code: ErrorCode::Internal,
            message: "Launch failed before a process started; check the executable and retry"
                .into(),
        },
    );
    let screen = palette_text(&dashboard);
    assert!(screen.contains("┌ Create terminal"), "{screen}");
    assert!(screen.contains("Command"), "{screen}");
    assert!(screen.contains("Couldn't start the session"), "{screen}");
    assert!(!screen.contains("Create branch"), "{screen}");
    dashboard.key(KeyCode::Esc);
    assert!(!palette_text(&dashboard).contains("┌ Create terminal"));
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn a_session_that_refuses_to_stop_returns_to_the_removal_confirmation() {
    use ovrcr::protocol::LifecycleOutcome;
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let first = only_request(dashboard.drain_outbox());
    accept(&mut dashboard, &first);
    let followed = lifecycle_completed(
        &mut dashboard,
        &first,
        LifecycleOutcome::Failed {
            code: ErrorCode::Conflict,
            message: "session was reopened; the replacement run was not controlled".into(),
        },
    );
    assert!(followed.is_empty(), "the removal never runs: {followed:?}");
    let text = palette_text(&dashboard);
    assert!(text.contains("Confirm action"), "{text}");
    assert!(
        text.contains("Couldn't stop review, so the workspace was not removed"),
        "{text}"
    );
    assert!(
        !footer_text(&dashboard).contains("Esc dismiss"),
        "no failed row"
    );
}

#[test]
fn dirty_workspace_removal_asks_to_save_then_removes_only_after_yes() {
    let mut dashboard = stopped_fixture();
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let request = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let save = only_request(dashboard.drain_outbox());
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
    let mut dashboard = stopped_fixture();
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let request = only_request(dashboard.drain_outbox());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Error {
            code: ovrcr::protocol::ErrorCode::DirtyWorktree,
            message: "worktree has changes".into(),
        },
    });
    assert!({
        let _ = dashboard.key(KeyCode::Esc);
        dashboard.drain_outbox().is_empty()
    });
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let first = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(KeyCode::Esc),
        ovrcr::tui::DashboardAction::Redraw
    );
    let second = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(if save { KeyCode::Enter } else { KeyCode::Esc }),
        ovrcr::tui::DashboardAction::Redraw
    );
    let answer = only_request(dashboard.drain_outbox());
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
fn operator_quick_palette_fallback_restores_same_search_after_dismissal() {
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "Quick message");
    dashboard.drain_outbox();
    let before = palette_text(&dashboard);
    dashboard.key(KeyCode::Enter);
    assert!(palette_text(&dashboard).contains("Quick message · Ctrl-o"));
    dashboard.key(KeyCode::Esc);
    assert_eq!(palette_text(&dashboard), before);
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn operator_quick_pending_shutdown_ack_advances_under_visible_modal() {
    let (mut dashboard, _, _root) = shutdown_navigation_fixture();
    shutdown_question(&mut dashboard, "one");
    let answer = answer_shutdown_question(&mut dashboard, "one", true);
    dashboard.ctrl('o');
    dashboard.event_action(Event::Paste("still editing".into()));
    shutdown_question(&mut dashboard, "two");
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: answer.request_id,
                response: Response::Ok
            })
            .is_empty()
    );
    assert!(palette_text(&dashboard).contains("still editing"));
    dashboard.key(KeyCode::Esc);
    assert!(dashboard.drain_outbox().is_empty());
    assert_shutdown_question(&dashboard, "two", "one");
}

#[test]
fn operator_quick_notification_retarget_cancels_unsent_and_retains_submitted_continuation() {
    use ovrcr_protocol::{OperatorRequest, OperatorResponse};
    for submitted in [false, true] {
        let (mut dashboard, offer, _root) = shutdown_navigation_fixture();
        let confirmation = shutdown_navigation_confirmation(&mut dashboard, &offer);
        dashboard.ctrl('o');
        dashboard.event_action(Event::Paste("captured before navigation".into()));
        let open = if submitted {
            dashboard.key(KeyCode::Enter);
            Some(
                dashboard
                    .drain_outbox()
                    .into_iter()
                    .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Open)))
                    .unwrap(),
            )
        } else {
            None
        };
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: confirmation.request_id,
            response: Response::NotificationNavigationConfirmed(Some(Box::new(offer.clone()))),
        });
        assert!(!palette_text(&dashboard).contains("Quick message"));
        assert_eq!(dashboard.focused_session(), Some(offer.ticket.session));
        dashboard.drain_outbox();
        if let Some(open) = open {
            let outgoing = dashboard.handle_server_message(ServerMessage::Response {
                request_id: open.request_id,
                response: Response::Operator(OperatorResponse::Opened(
                    ovrcr_protocol::OperatorConversationSummary {
                        reasoning: None,
                        id: ovrcr_protocol::OperatorConversationId(7),
                        session: SessionId(99),
                        run: ovrcr_protocol::SessionRunId(0),
                        worker_stopped: true,
                        model: None,
                        title: "Operator".into(),
                        current_turn: None,
                        current_turn_started_unix_ms: None,
                        last_reply_unix_ms: None,
                    },
                )),
            });
            assert_eq!(
                outgoing
                    .iter()
                    .filter(|m| matches!(
                        m.request,
                        Request::Operator(OperatorRequest::Submit { .. })
                    ))
                    .count(),
                1
            );
            assert!(!palette_text(&dashboard).contains("Quick message"));
        } else {
            dashboard.ctrl('o');
            assert!(!palette_text(&dashboard).contains("captured before navigation"));
            assert!(dashboard.drain_outbox().is_empty());
        }
    }
}

#[test]
fn operator_quick_preserves_workspace_save_chain_and_exact_pending_response() {
    let mut dashboard = dashboard_fixture();
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let remove = only_request(dashboard.drain_outbox());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: remove.request_id,
        response: Response::Error {
            code: ErrorCode::DirtyWorktree,
            message: "worktree has changes".into(),
        },
    });
    dashboard.ctrl('o');
    dashboard.event_action(Event::Paste("private quick draft".into()));
    dashboard.key(KeyCode::Esc);
    assert!(dashboard.drain_outbox().is_empty());
    assert!(palette_text(&dashboard).contains("Save uncommitted work"));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let save = only_request(dashboard.drain_outbox());
    assert!(matches!(save.request, Request::SaveWorkspaceWip { .. }));
    dashboard.ctrl('o');
    dashboard.key(KeyCode::Esc);
    assert!(dashboard.drain_outbox().is_empty());
    assert!(
        dashboard
            .handle_server_message(ServerMessage::Response {
                request_id: save.request_id + 99,
                response: Response::Ok
            })
            .is_empty()
    );
    assert!(palette_text(&dashboard).contains("Save uncommitted work"));
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: save.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        outgoing
            .iter()
            .filter(|m| matches!(m.request, Request::RemoveWorkspace { force: true, .. }))
            .count(),
        1
    );
}

#[test]
fn operator_quick_escape_never_answers_or_replaces_pending_shutdown_question() {
    for pending in [false, true] {
        let (mut dashboard, _, _root) = shutdown_navigation_fixture();
        shutdown_question(&mut dashboard, "one");
        let answer = pending.then(|| answer_shutdown_question(&mut dashboard, "one", true));
        dashboard.ctrl('o');
        dashboard.event_action(Event::Paste("quick draft".into()));
        shutdown_question(&mut dashboard, "two");
        dashboard.key(KeyCode::Esc);
        assert!(
            dashboard.drain_outbox().is_empty(),
            "modal Escape cannot answer WIP save"
        );
        assert_shutdown_question(&dashboard, "one", "two");
        if let Some(answer) = answer {
            dashboard.handle_server_message(ServerMessage::Response {
                request_id: answer.request_id + 999,
                response: Response::Ok,
            });
            assert_shutdown_question(&dashboard, "one", "two");
            dashboard.handle_server_message(ServerMessage::Response {
                request_id: answer.request_id,
                response: Response::Ok,
            });
            assert_shutdown_question(&dashboard, "two", "one");
        }
    }
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
    let view = if let Some(view) = outgoing
        .iter()
        .find(|message| matches!(message.request, Request::SetView { .. }))
    {
        assert!(dashboard.drain_outbox().is_empty());
        view.clone()
    } else {
        let view = dashboard
            .request_view_at(Rect::new(0, 0, 88, 38))
            .expect("fresh navigation must request its acknowledged view");
        assert_eq!(dashboard.drain_outbox(), vec![view.clone()]);
        view
    };
    let Request::SetView { view: selected } = &view.request else {
        unreachable!()
    };
    assert_eq!(selected.focused, Some(fresh.ticket.session));
    assert!(dashboard.view_revision() > revision);
    assert!(
        dashboard
            .input_request(b"before fresh view ack".to_vec(), 9001)
            .is_none()
    );
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
fn discard_prompt_survives_unchanged_hierarchy_then_needs_consent_after_relabel() {
    let mut dashboard = dashboard_fixture();
    // A detached checkout cannot be saved to origin/wip, so dirt offers discard.
    let mut original = stopped_hierarchy();
    original.projects[1].workspaces[1].name = "detached @ 1a2b3c4".into();
    dashboard.install_hierarchy(original.clone());
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let request = only_request(dashboard.drain_outbox());
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
            code: ovrcr::protocol::ErrorCode::DirtyWorktree,
            message: "worktree has changes".into(),
        },
    });
    let discard_text = palette_text(&dashboard);
    assert!(
        discard_text.contains("consigint / detached @ 1a2b3c4 has uncommitted changes"),
        "{discard_text}"
    );
    assert!(
        !discard_text.contains("consigint / auth has"),
        "the prompt must use the branch heading, not the internal id: {discard_text}"
    );

    let mut refreshed = original.clone();
    refreshed.projects[0].workspaces[0].sessions[0].pid = Some(999);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        refreshed,
    )));
    let still = palette_text(&dashboard);
    assert!(
        still.contains("Confirm action") && still.contains("has uncommitted changes"),
        "ordinary hierarchy refresh must keep the discard prompt: {still}"
    );

    let mut relabelled = stopped_hierarchy();
    relabelled.projects[1].workspaces[1].name = "feature/auth".into();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        relabelled,
    )));
    let text = palette_text(&dashboard);
    assert!(
        !text.contains("Confirm action"),
        "relabelled discard prompt must require fresh consent: {text}"
    );
    assert!(
        text.contains("consigint / feature/auth"),
        "picker must show the current branch: {text}"
    );
    let _ = dashboard.key(KeyCode::Enter);
    if let Some(message) = optional_request(dashboard.drain_outbox()) {
        panic!(
            "relabelled discard confirmation must not submit: {:?}",
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
        let mut dashboard = stopped_fixture();
        palette_search(&mut dashboard, "remove workspace");
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Enter);
        assert_eq!(
            dashboard.key(KeyCode::Enter),
            ovrcr::tui::DashboardAction::Redraw
        );
        let request = only_request(dashboard.drain_outbox());
        assert!(matches!(request.request, Request::RemoveWorkspace { .. }));
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Error {
                code: code.clone(),
                message: message.into(),
            },
        });
        let text = palette_words(&dashboard);
        assert!(
            text.contains(&format!("Couldn't remove consigint / auth: {message}")),
            "{text}"
        );
        assert!(!text.contains(&format!("{code:?}")), "{text}");
        assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
        assert!({
            let _ = dashboard.key(KeyCode::Esc);
            dashboard.drain_outbox().is_empty()
        });
    }
}

#[test]
fn archive_search_unarchives_without_launching() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(':'));
    let inspect = only_request(dashboard.drain_outbox());
    assert_eq!(inspect.request, Request::Inspect);
    dashboard.event_action(Event::Paste("Archived sessions".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    assert!({
        let _ = action;
        dashboard.drain_outbox().is_empty()
    });
    assert!(palette_text(&dashboard).contains("archives its record"));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    assert_eq!(
        dashboard.key(KeyCode::Char('X')),
        ovrcr::tui::DashboardAction::Redraw
    );
    let message = only_request(dashboard.drain_outbox());
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
    assert!(text.contains("implement  claude  Exited · 0m"), "{text}");
    assert!(!text.contains("busy"), "{text}");
    assert!(!text.contains("Working"), "{text}");
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
    let rows = rendered_rows(&dashboard, 88, 38);
    let text = rows.join("\n");
    assert!(text.contains("feature/auth-renamed"), "{text}");
    assert!(text.contains("▸"), "{text}");
    // The fold belongs to the sidebar; the selected workspace's card in the pane
    // still lists what is inside it.
    assert!(text.contains("consigint / feature/auth-renamed"), "{text}");
    let pane_x = dashboard.pane_rects(Rect::new(0, 0, 88, 38))[0].terminal.x;
    let sidebar = rows
        .iter()
        .map(|row| row.chars().take(usize::from(pane_x)).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !sidebar.contains("review"),
        "folded workspace must keep sessions hidden: {sidebar}"
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
    open_new_terminal_from_menu(&mut dashboard);
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
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('w'));
    let menu = palette_text(&dashboard);
    assert!(menu.contains("New terminal"), "{menu}");
    dashboard.key(KeyCode::Char('t'));
    let menu = palette_text(&dashboard);
    assert!(
        menu.contains("Which key"),
        "blocked action keeps the menu open: {menu}"
    );
    assert!(!menu.contains("┌ Create terminal"), "{menu}");
    dashboard.key(KeyCode::Down);
    let menu = palette_text(&dashboard)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        menu.contains("New terminal: root workspace is not")
            && menu.contains("on the default branch"),
        "{menu}"
    );
}

#[test]
fn slash_branch_picker_does_not_split_on_slashes() {
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[1].name = "feature/auth".into();
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    open_new_terminal_from_menu(&mut dashboard);
    let text = palette_text(&dashboard);
    assert!(text.contains("consigint / feature/auth"), "{text}");
    let _ = dashboard.event_action(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)));
    let message = optional_request(dashboard.drain_outbox()).expect("create terminal must submit");
    let Request::CreateSession(request) = message.request else {
        panic!("{:?}", message.request);
    };
    assert_eq!(request.project, "consigint");
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
    open_new_terminal_from_menu(&mut dashboard);
    let text = palette_text(&dashboard);
    assert!(text.contains("spacelift-agent / feature/other"), "{text}");
    let _ = dashboard.event_action(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)));
    let message = optional_request(dashboard.drain_outbox()).expect("create terminal must submit");
    let Request::CreateSession(request) = message.request else {
        panic!("{:?}", message.request);
    };
    assert_eq!(request.project, "spacelift-agent");
    assert_eq!(request.workspace, "auth");
}

#[test]
fn terminal_workspace_picker_shows_current_branch_and_keeps_stable_target() {
    let mut dashboard = dashboard_fixture();
    open_new_terminal_from_menu(&mut dashboard);
    dashboard.key(KeyCode::Enter);
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

    let submitted = {
        let _ =
            dashboard.event_action(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)));
        optional_request(dashboard.drain_outbox())
    };
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
    let _ = dashboard.key(KeyCode::Enter);
    if let Some(message) = optional_request(dashboard.drain_outbox()) {
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
    open_new_terminal_from_menu(&mut dashboard);
    for character in "feature/foo".chars() {
        dashboard.key(KeyCode::Char(character));
    }

    hierarchy.projects[1].workspaces[1].name = "feature/bar".into();
    hierarchy.projects[1].workspaces[0].name = "feature/foo".into();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    for _ in 0..6 {
        let _ = dashboard.key(KeyCode::Enter);
        assert!(
            dashboard.drain_outbox().is_empty(),
            "a stale filtered selection must not target the workspace that acquired its branch"
        );
    }
    dashboard.key(KeyCode::Down);
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    assert!(
        dashboard.drain_outbox().is_empty(),
        "accepting a workspace must not submit"
    );
    let _ = dashboard.event_action(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)));
    let submitted = optional_request(dashboard.drain_outbox()).map(|message| message.request);
    let Some(Request::CreateSession(request)) = submitted else {
        panic!("explicit reselection must permit submission");
    };
    assert_eq!(request.project, "consigint");
    assert_eq!(request.workspace, "lifecycle");
}

#[test]
fn cloud_close_warns_before_archiving_live_and_exited_rows_from_key_and_palette() {
    for live in [true, false] {
        for palette in [false, true] {
            let mut hierarchy = fixture_hierarchy();
            let row = hierarchy
                .projects
                .iter_mut()
                .flat_map(|p| &mut p.workspaces)
                .flat_map(|w| &mut w.sessions)
                .find(|s| s.id == SessionId(1))
                .unwrap();
            row.kind = ovrcr::protocol::SessionKind::Agent {
                name: "cursor-cloud:work".into(),
            };
            if !live {
                row.phase = SessionPhase::Exited {
                    code: Some(0),
                    signal: None,
                };
            }
            let id = row.id;
            let run = row.run;
            let mut dashboard = dashboard_fixture();
            dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
                hierarchy,
            )));
            let action = if palette {
                dashboard.key(KeyCode::Char(':'));
                let inspect = only_request(dashboard.drain_outbox());
                assert_eq!(inspect.request, Request::Inspect);
                dashboard.event_action(Event::Paste("Close terminal".into()));
                dashboard.key(KeyCode::Enter)
            } else {
                dashboard.key(KeyCode::Char('X'))
            };
            assert!(
                {
                    let _ = action;
                    dashboard.drain_outbox().is_empty()
                },
                "cloud close requires visible warning before dispatch"
            );
            let text = palette_text(&dashboard);
            assert!(
                text.contains("Remote work and charges may continue"),
                "{text}"
            );
            assert_eq!(
                dashboard.key(KeyCode::Enter),
                ovrcr::tui::DashboardAction::Redraw
            );
            let message = only_request(dashboard.drain_outbox());
            assert_eq!(
                message.request,
                Request::CloseTerminal {
                    session: id,
                    expected_run: run
                }
            );
        }
    }
}

/// Configure named launchers for form choice tests; these executables are not run.
fn fixture_agent_overrides(dashboard: &mut Dashboard) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let mut agents = Vec::new();
    for name in ["claude", "codex", "hermes"] {
        let path = dir.path().join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        agents.push(ovrcr_protocol::AgentOverride {
            name: name.into(),
            argv: vec![path.to_str().unwrap().into()],
        });
    }
    dashboard.install_settings(ovrcr::tui::Settings {
        agents,
        launch_choices: [(
            "consigint".to_owned(),
            ovrcr_protocol::LaunchChoice::Agent {
                preset: "claude".into(),
                model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
            },
        )]
        .into(),
        ..Default::default()
    });
    dir
}

#[test]
fn create_terminal_uses_agent_prompt_and_terminal_command_fields() {
    let mut dashboard = dashboard_fixture();
    fixture_agent_overrides(&mut dashboard);
    open_generic_create_terminal_from_palette(&mut dashboard);
    let words = palette_words(&dashboard);
    assert_in_order(&words, &["Initial prompt", "Start", "Workspace", "Name"]);
    assert!(words.contains("Start Agent"), "{words}");
    assert!(words.contains("Agent claude"), "{words}");
    assert!(!words.contains("Task"), "{words}");

    dashboard.key(KeyCode::Tab); // Start
    dashboard.key(KeyCode::Down); // Terminal
    let words = palette_words(&dashboard);
    assert!(
        words.contains("Start") && words.contains("Terminal"),
        "{words}"
    );
    assert!(words.contains("Command"), "{words}");
    assert!(words.contains("Workspace"), "{words}");
    assert!(words.contains("Name"), "{words}");
    assert!(!words.contains("Initial prompt"), "{words}");
    assert!(!words.contains("Agent claude"), "{words}");
    assert!(!words.contains("Model"), "{words}");
    assert!(!words.contains("Task"), "{words}");
}

//! Command palette, which-key popup, and the creation forms.

use crate::*;

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
    dashboard.key(KeyCode::Enter);
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Terminal);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Error {
            code: ErrorCode::Internal,
            message: "late inspection".into(),
        },
    });
    assert!(
        dashboard.error.is_none(),
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
    dashboard.panes[0].ready = false;
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
    assert!(text.contains("Config:"));
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
fn tasks_hotkey_cancels_pending_history_like_control_t() {
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::PageUp);
    assert!(!dashboard.history_begin_request.as_ref().unwrap().cancelled);
    dashboard.key(KeyCode::Char('t'));
    assert!(dashboard.tasks.is_some());
    assert!(dashboard.history_begin_request.as_ref().unwrap().cancelled);
}

#[test]
fn whichkey_preserves_readiness_and_never_forwards_overlay_input() {
    use ovrcr::tui::{DashboardAction, InputMode};
    let mut dashboard = dashboard_fixture();
    let revision = dashboard.view_revision;
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
    assert_eq!(dashboard.view_revision, revision);
    assert!(dashboard.panes[0].ready);
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('t'));
    dashboard.key(KeyCode::Char('r')); // running session: resume is absent
    assert!(palette_text(&dashboard).contains("Which key"));
    dashboard.key(KeyCode::Esc);
    assert_eq!(dashboard.mode, InputMode::Browse);
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('t'));
    dashboard.key(KeyCode::Enter);
    assert_eq!(dashboard.mode, InputMode::Terminal);
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
    let session = dashboard.copy.as_ref().unwrap().session;
    dashboard.key(KeyCode::Char(' '));
    assert!(dashboard.mouse_capture_required());
    assert!(palette_text(&dashboard).contains("Selection"));
    assert!(dashboard.copy.as_ref().unwrap().anchor.is_none());
    dashboard.key(KeyCode::Char('v'));
    assert!(!palette_text(&dashboard).contains("Which key"));
    assert_eq!(dashboard.copy.as_ref().unwrap().session, session);
    assert!(dashboard.copy.as_ref().unwrap().anchor.is_some());
    dashboard.key(KeyCode::Char('?'));
    dashboard.cancel_copy(Some("Copy cancelled: terminal resized"));
    assert!(!palette_text(&dashboard).contains("Which key"));
    dashboard.key(KeyCode::Char('['));
    dashboard.key(KeyCode::Char(' '));
    dashboard.ctrl('g');
    assert!(
        dashboard.copy.is_none(),
        "leader Ctrl-g must run the listed Browse action"
    );
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
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
    dashboard.hierarchy.projects[0]
        .workspaces
        .push(WorkspaceSummary {
            project: "spacelift-agent".into(),
            name: "empty".into(),
            path: "/tmp/empty".into(),
            sessions: vec![],
        });
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
    assert!(!dashboard.panes[0].ready);
    assert!(palette_text(&dashboard).contains("press n to start a terminal here"));
    dashboard.key(KeyCode::Char('n'));
    assert!(palette_text(&dashboard).contains("spacelift-agent / empty"));
    assert_eq!(
        dashboard.hierarchy.projects[1].workspaces[1].sessions.len(),
        2
    );
}

#[test]
fn whichkey_tiny_layouts_and_last_disabled_detail_remain_browsable() {
    let mut dashboard = dashboard_fixture();
    dashboard.panes[0].ready = false;
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
    assert!(text.contains("Confirm action"));
    assert!(text.contains("review (#1)"));
    assert_eq!(dashboard.panes.len(), 1);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("close confirmation did not submit");
    };
    assert_eq!(
        message.request,
        Request::CloseTerminal {
            session: SessionId(1)
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
    helper_dashboard.mode = ovrcr::tui::InputMode::Terminal;
    assert!(
        event_to_request(
            &mut helper_dashboard,
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            100,
        )
        .is_none()
    );
    assert!(
        event_to_request(
            &mut helper_dashboard,
            Event::Paste("blocked by palette".into()),
            101,
        )
        .is_none()
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
            session: SessionId(3)
        }
    );
}

#[test]
fn palette_forms_build_workspace_and_project_requests_and_draw_at_small_sizes() {
    use ovrcr::protocol::{BranchRequest, Request};
    use ovrcr::tui::DashboardAction;
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let cases = [
        (
            "create workspace",
            vec!["consigint", "new-space", "new", "feature/palette", "main"],
            Request::CreateWorkspace {
                project: "consigint".into(),
                name: "new-space".into(),
                branch: BranchRequest::New {
                    branch: "feature/palette".into(),
                    base: "main".into(),
                },
            },
        ),
        (
            "create workspace",
            vec!["consigint", "existing", "existing", "topic"],
            Request::CreateWorkspace {
                project: "consigint".into(),
                name: "existing".into(),
                branch: BranchRequest::Existing {
                    branch: "topic".into(),
                },
            },
        ),
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
        if query == "create workspace" {
            dashboard.key(KeyCode::BackTab);
        }
        for (index, value) in values.iter().enumerate() {
            if query == "create workspace" && index == 2 {
                if *value == "existing" {
                    dashboard.key(KeyCode::Char(' '));
                }
            } else {
                dashboard.ctrl('u');
                dashboard.event_action(Event::Paste((*value).into()));
            }
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
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("Enter on Name must submit with defaults");
    };
    assert_eq!(
        message.request,
        Request::CreateWorkspace {
            project: "consigint".into(),
            name: "pick-demo".into(),
            branch: BranchRequest::New {
                branch: "feature/pick-demo".into(),
                base: "main".into()
            },
        }
    );
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
    dashboard.event_action(Event::Paste("first".into()));
    dashboard.key(KeyCode::Tab); // mode
    dashboard.key(KeyCode::Tab); // branch
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("custom/kept".into()));
    dashboard.key(KeyCode::BackTab);
    dashboard.key(KeyCode::BackTab); // name
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("renamed".into()));
    assert!(palette_text(&dashboard).contains("custom/kept"));
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Right); // existing
    dashboard.key(KeyCode::Tab); // branch pick
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
    assert_eq!(
        message.request,
        Request::CreateWorkspace {
            project: "consigint".into(),
            name: "renamed".into(),
            branch: ovrcr::protocol::BranchRequest::Existing {
                branch: "topic".into()
            },
        }
    );
}

#[test]
fn typed_existing_branch_survives_hint_arrival() {
    use ovrcr::protocol::BranchRequest;
    let repo = hint_repository(&["release-2"]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "typed-branch", "release-2");
    answer_workspace_inspect(&mut dashboard, inspect_id, repo.path());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let message = loop {
        let (_, request) = dashboard.poll_palette();
        let text = palette_text(&dashboard);
        // Without real branches the field would stay text and submit vacuously.
        assert!(!text.contains("enter branch/base manually"), "{text}");
        if let Some(message) = request {
            break message;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Git hints never reached the deferred submit: {text}"
        );
        std::thread::yield_now();
    };
    assert_eq!(
        message.request,
        Request::CreateWorkspace {
            project: "consigint".into(),
            name: "typed-branch".into(),
            branch: BranchRequest::Existing {
                branch: "release-2".into()
            },
        }
    );
}

#[test]
fn fuzzy_matched_branch_is_not_submitted_without_confirmation() {
    use ovrcr::protocol::BranchRequest;
    use ovrcr::tui::DashboardAction;
    let repo = hint_repository(&["release-2-old"]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "typed-branch", "release-2");
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
    assert_eq!(
        message.request,
        Request::CreateWorkspace {
            project: "consigint".into(),
            name: "typed-branch".into(),
            branch: BranchRequest::Existing {
                branch: "release-2-old".into()
            },
        }
    );
}

#[test]
fn case_only_branch_mismatch_is_refused_not_submitted() {
    // Git refs are case-sensitive, so `Release-2` is a different branch from the `release-2`
    // the user typed: creating the workspace on it would silently use the wrong branch.
    let repo = hint_repository(&["Release-2"]);
    let mut dashboard = dashboard_fixture();
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "typed-branch", "release-2");
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
    let inspect_id = type_existing_branch_and_submit(&mut dashboard, "typed-branch", "release-2");
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
    use ovrcr::tui::{DashboardAction, InputMode};
    let mut dashboard = dashboard_fixture();
    let action = dashboard.key(KeyCode::Char('w'));
    answer_palette_inspect(&mut dashboard, action);
    dashboard.event_action(Event::Paste("pick-demo".into()));
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("workspace form did not submit");
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::Ok,
    });
    assert!(palette_text(&dashboard).contains("Working"));
    assert!(palette_text(&dashboard).contains("┌ Create workspace"));
    let mut hierarchy = workspace_creation_hierarchy(&dashboard);
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
    assert_eq!(dashboard.mode, InputMode::Browse);
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
    dashboard.error = Some("current error".into());
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: inspect.request_id,
        response: Response::Inventory {
            registry: Default::default(),
            sessions: vec![],
        },
    });
    assert_eq!(dashboard.error.as_deref(), Some("current error"));
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
    let hierarchy = workspace_creation_hierarchy(&dashboard);
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
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Browse);
    assert!(
        !palette_text(&dashboard)
            .lines()
            .any(|line| line.contains("┌ Create workspace"))
    );
}

#[test]
fn workspace_creation_attaches_to_its_local_shell() {
    use ovrcr::tui::{DashboardAction, InputMode};
    for hierarchy_first in [true, false] {
        let mut dashboard = dashboard_fixture();
        let action = dashboard.key(KeyCode::Char('w'));
        answer_palette_inspect(&mut dashboard, action);
        dashboard.event_action(Event::Paste("pick-demo".into()));
        let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
            panic!("workspace form did not submit");
        };
        let event = ServerMessage::Event(ServerEvent::HierarchyChanged(
            workspace_creation_hierarchy(&dashboard),
        ));
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
        assert_eq!(dashboard.mode, InputMode::Terminal);
        assert!(
            dashboard
                .input_request(b"not ready".to_vec(), 999)
                .is_none()
        );
        assert_eq!(outgoing.len(), 1);
        assert!(
            matches!(&outgoing[0].request, Request::SetView { view } if view.focused == Some(SessionId(99)))
        );
        acknowledge_view_request(&mut dashboard, outgoing[0].clone());
        assert!(matches!(
            dashboard.input_request(b"ready".to_vec(), 1000),
            Some(ClientMessage {
                request: Request::Input {
                    session: SessionId(99),
                    ..
                },
                ..
            })
        ));
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
    let path = std::env::var_os("PATH").unwrap_or_default();
    let shell = std::env::var_os("SHELL");
    let first = ovrcr::tui::detect_agents(&path, shell.as_deref())
        .into_iter()
        .next()
        .expect("shell is always detected")
        .name;
    assert!(text.contains(&first), "{text}");
}

#[test]
fn created_session_enters_terminal_mode() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char('n'));
    dashboard.key(KeyCode::Enter);
    dashboard.key(KeyCode::Enter);
    let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
        panic!("form did not submit");
    };
    let Request::CreateSession(request) = &message.request else {
        panic!("wrong request");
    };
    let session = SessionSummary {
        id: SessionId(99),
        project: request.project.clone(),
        workspace: request.workspace.clone(),
        name: request.name.clone(),
        label: request.label.clone().unwrap_or_default(),
        pid: Some(1),
        started_unix_ms: 0,
        phase: SessionPhase::Running,
        activity: AgentActivity::Unknown,
        context_usage: None,
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::CreatedSession(Box::new(session)),
    });
    assert_eq!(dashboard.mode, ovrcr::tui::InputMode::Terminal);
    assert_eq!(dashboard.focused_session(), Some(SessionId(99)));
}

#[test]
fn n_is_listed_in_the_browse_footer() {
    let dashboard = dashboard_fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard(frame, &dashboard))
        .unwrap();
    let text: String = (0..40)
        .map(|y| {
            (0..120)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    assert!(text.contains("n Create terminal"), "{text}");
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
    dashboard.settings.picker_roots = vec![root];
    dashboard.config_dir = dir.path().join("config");
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
    dashboard.settings.picker_roots = vec![dir.path().to_path_buf()];
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
    dashboard.settings.picker_roots = vec![dir.path().to_path_buf()];
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
    dashboard.settings.picker_roots = vec![root];
    dashboard.config_dir = dir.path().join("config");
    std::fs::create_dir_all(dashboard.config_dir.join("workspaces").join("demo")).unwrap();
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
    let mut hierarchy = dashboard.hierarchy.clone();
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
    dashboard.settings.picker_roots.clear();
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
    dashboard.panes[dashboard.focused_pane]
        .parser
        .process(b"removed terminal output");
    palette_search(&mut dashboard, "close terminal");
    dashboard.key(KeyCode::Enter);
    let ovrcr::tui::DashboardAction::Request(close) = dashboard.key(KeyCode::Enter) else {
        panic!("close missing");
    };
    let mut hierarchy = dashboard.hierarchy.clone();
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
    assert!(
        !dashboard.panes[dashboard.focused_pane]
            .parser
            .screen()
            .contents()
            .contains("removed terminal output")
    );
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
    assert_eq!(dashboard.error, None);
}

#[test]
fn nested_whichkey_terminal_group_waits_for_action_and_backspace_returns() {
    use ovrcr::tui::DashboardAction;
    let mut dashboard = dashboard_fixture();
    dashboard.key(KeyCode::Char(' '));
    assert_eq!(dashboard.key(KeyCode::Char('t')), DashboardAction::Redraw);
    assert!(dashboard.tasks.is_none(), "group key must not open tasks");
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
            },
        ),
        (
            'w',
            Request::RemoveWorkspace {
                project: "consigint".into(),
                name: "auth".into(),
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
            assert!(text.contains("Confirm action"), "{group}: {text}");
            assert!(text.contains("consigint"));
            if confirm {
                let DashboardAction::Request(message) = dashboard.key(KeyCode::Enter) else {
                    panic!("confirmation must emit removal request");
                };
                assert_eq!(message.request, expected);
            } else {
                assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
                assert!(!palette_text(&dashboard).contains("Confirm action"));
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
            column: area.x + 2,
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
            "spacelift-agent",
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
    let mut session = dashboard
        .hierarchy
        .projects
        .iter()
        .flat_map(|p| &p.workspaces)
        .flat_map(|w| &w.sessions)
        .find(|s| s.id == SessionId(1))
        .unwrap()
        .clone();
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
    assert!(dashboard.tasks.is_some());
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('a'));
    assert!(palette_text(&dashboard).contains("Repository"));
}

#[test]
fn ux_browse_footer_names_actions_and_respects_availability() {
    let mut dashboard = dashboard_fixture();
    let text = rendered_footer(&dashboard, 120);
    for label in ["? Help", "Enter Focus", "n Create terminal", ": Search"] {
        assert!(text.contains(label), "missing {label}: {text}");
    }
    for width in [20, 40, 80, 120] {
        let text = rendered_footer(&dashboard, width);
        assert!(text.starts_with("BROWSE  ? Help"), "{text}");
        assert!(!text.ends_with("Enter"), "partial action: {text}");
    }
    dashboard.panes[dashboard.focused_pane].ready = false;
    assert!(!rendered_footer(&dashboard, 120).contains("Enter Focus"));
    dashboard.hierarchy.projects.clear();
    let text = rendered_footer(&dashboard, 80);
    assert!(text.contains("a Register project"), "{text}");
    assert!(!text.contains("n Create terminal"), "{text}");
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
    dashboard.hierarchy.projects[0].workspaces[0].sessions[0].name = "界🙂".repeat(50);
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
    dashboard.hierarchy.projects[1]
        .workspaces
        .push(WorkspaceSummary {
            project: "consigint".into(),
            name: "root".into(),
            path: "/tmp/root".into(),
            sessions: vec![],
        });
    palette_search(&mut dashboard, "remove workspace");
    dashboard.key(KeyCode::Enter);
    assert!(!palette_text(&dashboard).contains("consigint / root"));
    dashboard.event_action(Event::Paste("root".into()));
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
        dashboard.hierarchy.projects.clear();
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

//! Cards: what a pane shows when it has no session, through the real key, mouse and
//! hierarchy paths.

use crate::*;

fn area() -> Rect {
    Rect::new(0, 0, 120, 40)
}

/// The rows of `pane`'s terminal area, read cell by cell and trimmed.
fn pane_text(dashboard: &Dashboard, pane: usize) -> Vec<String> {
    pane_rows(dashboard, pane, area(), false)
}

/// The rows of `pane` in a window of `area`: its terminal area, or its header line.
fn pane_rows(dashboard: &Dashboard, pane: usize, area: Rect, header: bool) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal
        .draw(|frame| draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    let rects = dashboard
        .pane_rects(area)
        .into_iter()
        .find(|rect| rect.pane_index == pane)
        .expect("pane is drawn");
    let rect = if header {
        Rect::new(rects.metadata.x, rects.metadata.y, rects.metadata.width, 1)
    } else {
        rects.terminal
    };
    (rect.y..rect.bottom())
        .map(|y| {
            (rect.x..rect.right())
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn has(lines: &[String], line: &str) -> bool {
    lines.iter().any(|candidate| candidate == line)
}

#[test]
fn moving_through_the_tree_shows_a_card_instead_of_a_blank_pane() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area());
    // From consigint / auth / review: up to its local shell, then its workspace.
    dashboard.key(KeyCode::Char('k'));
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    dashboard.key(KeyCode::Char('k'));
    assert_eq!(dashboard.focused_session(), None);
    // The pane header names the workspace; the card does not repeat it.
    assert_eq!(pane_rows(&dashboard, 0, area(), true), ["consigint / auth"]);
    let card = pane_text(&dashboard, 0);
    assert_eq!(card[..3], ["/tmp/auth", "", "Sessions"]);
    assert!(card[3].starts_with("  $ local"), "{card:#?}");
    assert!(card[4].starts_with("  - review"), "{card:#?}");
    assert!(card[4].contains("claude"), "{card:#?}");
    for hint in [
        "  j        open local",
        "  n        new agent session",
        "  w        create branch",
        "  Space w  more",
    ] {
        assert!(has(&card, hint), "{hint:?} in {card:#?}");
    }

    // The card promised `j` opens the local shell.
    let action = dashboard.key(KeyCode::Char('j'));
    pump_view(&mut dashboard, action);
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    assert!(!has(&pane_text(&dashboard, 0), "  w        create branch"));

    // On up to the project row.
    dashboard.key(KeyCode::Char('k'));
    dashboard.key(KeyCode::Char('k'));
    assert_eq!(pane_rows(&dashboard, 0, area(), true), ["consigint"]);
    let card = pane_text(&dashboard, 0);
    assert_eq!(
        card[..10],
        [
            // The fixture's sessions are all terminals: no agent yet.
            "Next: press n to start your first agent",
            "Workspace root  /tmp",
            "",
            "Workspaces",
            "  auth       2 sessions",
            "  lifecycle  2 sessions",
            "",
            "  n  new agent session",
            "  w  create branch",
            "  a  register another project",
        ]
    );
}

#[test]
fn cards_are_display_only_and_enter_still_folds_the_row() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area());
    let action = dashboard.key(KeyCode::Char('j'));
    pump_view(&mut dashboard, action);
    assert_eq!(dashboard.focused_session(), None);
    assert_eq!(pane_text(&dashboard, 0)[0], "/tmp/lifecycle");
    assert!(
        dashboard.drain_outbox().is_empty(),
        "a card asks for nothing"
    );

    // Enter folds the workspace as before, and the card says how to reach its
    // sessions again instead of offering a row that is no longer below it.
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let card = pane_text(&dashboard, 0);
    assert!(
        has(&card, "  Enter    expand to reach its sessions"),
        "{card:#?}"
    );
    assert!(!card.iter().any(|line| line.contains("open")), "{card:#?}");
    assert!(
        dashboard.drain_outbox().is_empty(),
        "a card asks for nothing"
    );

    // Nothing about the card makes the empty pane accept input.
    dashboard.install_terminal_input();
    assert!(dashboard.input_request(b"x".to_vec(), 9000).is_none());
}

#[test]
fn clicking_a_project_row_shows_its_card() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area());
    let row = rendered_rows(&dashboard, 120, 40)
        .iter()
        .position(|row| row.contains("SPACELIFT-AGENT"))
        .expect("project row is drawn") as u16;
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area(),
    );
    assert_eq!(pane_rows(&dashboard, 0, area(), true), ["spacelift-agent"]);
    let card = pane_text(&dashboard, 0);
    assert_eq!(card[0], "Next: press n to start your first agent");
    assert!(has(&card, "  progress  1 session"), "{card:#?}");
    assert!(has(&card, "  w  create branch"), "{card:#?}");
}

#[test]
fn registering_a_first_project_suggests_the_first_agent() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 38, cols: 88 });
    dashboard.install_area(area());
    assert_eq!(pane_text(&dashboard, 0)[0], "Welcome to OVRCR");
    let shell = session_summary(1, "myapp", "root-id", "local", "zsh", Some(10), 0);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        HierarchySnapshot {
            projects: vec![ProjectSummary {
                name: "myapp".into(),
                workspaces: vec![WorkspaceSummary {
                    project: "myapp".into(),
                    id: "root-id".into(),
                    name: "main".into(),
                    path: "/srv/myapp".into(),
                    root: true,
                    warning: None,
                    sessions: vec![shell],
                }],
            }],
        },
    )));
    // Registration selects nothing; the pane says what to do next.
    assert_eq!(dashboard.focused_session(), None);
    let card = pane_text(&dashboard, 0);
    assert_eq!(
        card[..7],
        [
            "myapp",
            "Next: press n to start your first agent",
            "Repository      /srv/myapp",
            "",
            "Workspaces",
            "  main · root  1 session",
            "",
        ]
    );
    let action = dashboard.key(KeyCode::Char('w'));
    answer_palette_inspect(&mut dashboard, action);
    let form = palette_text(&dashboard);
    assert!(form.contains("Create branch"), "{form}");
    assert!(form.contains("myapp"), "{form}");
}

#[test]
fn workspace_paths_under_home_read_with_a_tilde() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[0].path = home.join("Code/consigint-lifecycle");
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_area(area());
    dashboard.key(KeyCode::Char('j'));
    let card = pane_text(&dashboard, 0);
    assert_eq!(card[0], "~/Code/consigint-lifecycle");
}

#[test]
fn a_split_shows_the_card_in_the_pane_the_selection_emptied() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area());
    // `v` opens the split chooser on the suggested session; Esc keeps the suggestion.
    dashboard.key(KeyCode::Char('v'));
    let action = dashboard.key(KeyCode::Esc);
    pump_view(&mut dashboard, action);
    assert_eq!(dashboard.pane_rects(area()).len(), 2);
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    // Up from lifecycle's local shell to its workspace: the focused pane empties and
    // focus moves to the pane that still shows review.
    let action = dashboard.key(KeyCode::Char('k'));
    pump_view(&mut dashboard, action);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    // A split header says only `no session`, so this card names the workspace itself.
    let card = pane_text(&dashboard, 1);
    assert_eq!(card[..2], ["consigint / lifecycle", "/tmp/lifecycle"]);
    assert!(has(&card, "  j        open local"), "{card:#?}");
    assert!(!has(&pane_text(&dashboard, 0), "  w        create branch"));

    // `j` opens the session in the focused pane; the emptied pane keeps a card (the
    // projects, since nothing is selected now) instead of going blank.
    let action = dashboard.key(KeyCode::Char('j'));
    pump_view(&mut dashboard, action);
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    let card = pane_text(&dashboard, 1);
    assert_eq!(card[0], "consigint", "{card:#?}");
    assert!(has(&card, "  w  create branch"), "{card:#?}");
}

#[test]
fn an_80_column_window_keeps_each_row_state_and_age_in_its_40_column_pane() {
    let mut hierarchy = fixture_hierarchy();
    let review = &mut hierarchy.projects[1].workspaces[1].sessions[0];
    review.name = "implement lifecycle hooks and retries".into();
    review.activity = AgentActivity::Busy;
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    let small = Rect::new(0, 0, 80, 24);
    dashboard.install_area(small);
    dashboard.key(KeyCode::Char('k'));
    dashboard.key(KeyCode::Char('k'));
    assert_eq!(dashboard.focused_session(), None);
    let card = pane_rows(&dashboard, 0, small, false);
    let row = card
        .iter()
        .find(|line| line.contains("implement"))
        .expect("the long title is listed");
    assert!(row.ends_with("Working · 0m"), "{card:#?}");
    assert!(row.contains('…'), "{card:#?}");

    let mut hierarchy = fixture_hierarchy();
    let review = &mut hierarchy.projects[1].workspaces[1].sessions[0];
    review.name = "implement lifecycle hooks and retries".into();
    review.activity = AgentActivity::ResponseReady;
    review.unread = Some(ovrcr::protocol::ReadyObservation {
        binding: ovrcr::protocol::AgentBinding {
            provider: ovrcr::protocol::AgentProvider::Claude,
            invocation: "card-fixture".into(),
            conversation: "card-conversation".into(),
            generation: 1,
        },
        turn: Some("card-turn".into()),
        activity_revision: 1,
    });
    hierarchy.projects[1].workspaces[1].sessions[1].activity = AgentActivity::WaitingInput;
    dashboard.install_hierarchy(hierarchy);
    let card = pane_rows(&dashboard, 0, small, false);
    let row = card
        .iter()
        .find(|line| line.contains("unread reply"))
        .unwrap_or_else(|| panic!("{card:#?}"));
    assert!(row.ends_with("Ready · unread reply · 0m"), "{card:#?}");
    dashboard.key(KeyCode::Char('k'));
    let card = pane_rows(&dashboard, 0, small, false);
    let row = card.iter().find(|line| line.contains("auth")).unwrap();
    assert!(row.ends_with("2 sessions · ? 1 · ✓ 1"), "{card:#?}");
}

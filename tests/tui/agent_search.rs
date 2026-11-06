use super::*;
use crossterm::event::ModifierKeyCode;
use ovrcr::protocol::{SessionKind, SessionRunId};

fn agents_hierarchy() -> HierarchySnapshot {
    let mut hierarchy = fixture_hierarchy();
    for session in hierarchy
        .projects
        .iter_mut()
        .flat_map(|p| &mut p.workspaces)
        .flat_map(|w| &mut w.sessions)
    {
        if matches!(session.id.0, 1..=3) {
            session.kind = SessionKind::Agent {
                name: "fixture-agent".into(),
            };
            session.label = "fixture-agent".into();
            session.phase = SessionPhase::Running;
            session.title = Some(format!("agent-title-{}", session.id.0));
        }
    }
    hierarchy
}

fn agents_with_separate_destination() -> HierarchySnapshot {
    let mut hierarchy = agents_hierarchy();
    for (index, project) in hierarchy.projects.iter_mut().enumerate() {
        project.name = format!("demo-{index}");
        for workspace in &mut project.workspaces {
            workspace.project = project.name.clone();
            for session in &mut workspace.sessions {
                session.project = project.name.clone();
            }
        }
    }
    let original = &mut hierarchy.projects[0].workspaces[0];
    let index = original
        .sessions
        .iter()
        .position(|s| s.id == SessionId(3))
        .unwrap();
    let mut session = original.sessions.remove(index);
    let mut workspace = original.clone();
    workspace.id = "destination-b".into();
    workspace.name = "destination-b".into();
    workspace.root = false;
    session.workspace = workspace.id.clone();
    workspace.sessions = vec![session];
    hierarchy.projects[0].workspaces.push(workspace);
    hierarchy
}

fn agents_dashboard() -> Dashboard {
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(agents_hierarchy());
    dashboard
}

fn open_agents(dashboard: &mut Dashboard) {
    dashboard.key(KeyCode::Char('s'));
    assert!(
        palette_text(dashboard).contains("┌ Agents"),
        "s must open agent search"
    );
}

fn agent_results(dashboard: &Dashboard) -> String {
    palette_text(dashboard)
        .lines()
        .filter(|line| line.contains("│") && line.contains("(#"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn command_event(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(code, modifiers, kind))
}

#[test]
fn command_hold_labels_visible_running_agents_without_moving_the_title_column() {
    let mut dashboard = agents_dashboard();
    let before = rendered_rows(&dashboard, 120, 40);
    let original = before
        .iter()
        .find(|row| {
            row.split('│')
                .next()
                .is_some_and(|left| left.contains("agent-title-1"))
        })
        .expect("the current Agent title is drawn")
        .chars()
        .skip(5)
        .collect::<String>();

    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));

    let held = rendered_rows(&dashboard, 120, 40);
    let labeled = held
        .iter()
        .find(|row| {
            row.split('│')
                .next()
                .is_some_and(|left| left.contains("agent-title-1"))
        })
        .expect("the current Agent title remains drawn");
    assert!(
        labeled.contains("[1]"),
        "current Agent receives label 1: {labeled}; footer: {}",
        rendered_footer(&dashboard, 120)
    );
    assert_eq!(
        labeled.chars().skip(5).collect::<String>(),
        original,
        "the status glyph and title retain their columns"
    );
    for (title, label) in [("agent-title-2", "[2]"), ("agent-title-3", "[3]")] {
        let row = held
            .iter()
            .find(|row| {
                row.split('│')
                    .next()
                    .is_some_and(|left| left.contains(title))
            })
            .unwrap_or_else(|| panic!("visible Agent title {title} remains drawn"));
        assert!(row.contains(label), "{title} receives {label}: {row}");
    }
    assert_eq!(held.len(), before.len(), "labels do not add sidebar rows");

    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    let after = rendered_rows(&dashboard, 120, 40);
    assert!(!after.iter().any(|row| row.contains("[1]")));
    assert!(
        after
            .iter()
            .find(|row| row
                .split('│')
                .next()
                .is_some_and(|left| left.contains("agent-title-1")))
            .is_some_and(|row| row.chars().skip(5).collect::<String>() == original),
        "cancelling an unselected hold restores the original row"
    );
}

#[test]
fn command_label_selects_its_frozen_agent_and_waits_for_release_and_readiness() {
    let mut dashboard = agents_dashboard();
    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    let before = rendered_rows(&dashboard, 120, 40);
    let target_row = before
        .iter()
        .find(|row| {
            row.split('│')
                .next()
                .is_some_and(|left| left.contains("agent-title-2"))
        })
        .expect("the second running Agent title is drawn");
    assert!(
        target_row.contains("[2]"),
        "the second visible Agent has label 2: {target_row}"
    );

    assert_eq!(
        dashboard.event_action(command_event(
            KeyCode::Char('2'),
            KeyModifiers::SUPER,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(2)));
    assert!(
        !rendered_rows(&dashboard, 120, 40).iter().any(|row| row
            .split('│')
            .next()
            .is_some_and(|left| left.contains("[1]"))),
        "selection hides every temporary label"
    );
    assert!(rendered_footer(&dashboard, 120).contains("Session selected"));
    let request = only_request(dashboard.drain_outbox());
    assert!(matches!(
        &request.request,
        Request::SetView { view } if view.focused == Some(SessionId(2))
    ));

    assert_eq!(dashboard.key(KeyCode::Char('z')), DashboardAction::None);
    assert_eq!(
        dashboard.drain_outbox().len(),
        0,
        "held input cannot reach either Agent"
    );
    acknowledge_all_view_targets(&mut dashboard, request);
    assert!(
        rendered_footer(&dashboard, 120).contains("Session selected"),
        "an acknowledged view still waits for Command release"
    );
    assert_eq!(
        dashboard.event_action(command_event(
            KeyCode::Char('3'),
            KeyModifiers::SUPER,
            KeyEventKind::Repeat,
        )),
        DashboardAction::None,
        "repeat and later digits cannot select a second destination"
    );

    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    assert_eq!(dashboard.key(KeyCode::Char('z')), DashboardAction::Redraw);
    assert!(
        dashboard.drain_outbox().is_empty(),
        "input queued before the event-loop boundary is refused rather than replayed"
    );
    assert!(rendered_footer(&dashboard, 120).contains("input was not sent"));
}

#[test]
fn command_labels_preserve_geometry_at_wide_and_narrow_sizes_with_unicode_titles() {
    for (cols, rows) in [(120, 40), (80, 24), (60, 24)] {
        let mut hierarchy = agents_hierarchy();
        let unicode_title = "界e\u{301}🙂";
        hierarchy
            .projects
            .iter_mut()
            .flat_map(|project| &mut project.workspaces)
            .flat_map(|workspace| &mut workspace.sessions)
            .find(|session| session.id == SessionId(1))
            .unwrap()
            .title = Some(unicode_title.into());
        let mut dashboard = Dashboard::new(TerminalSize { rows, cols });
        dashboard.install_hierarchy(hierarchy);
        dashboard.install_area(Rect::new(0, 0, cols, rows));
        dashboard.install_focus(SessionId(1));
        let before = rendered_rows(&dashboard, cols, rows);
        let before_row = before
            .iter()
            .find(|line| {
                line.split('│')
                    .next()
                    .is_some_and(|left| left.contains("界"))
            })
            .unwrap_or_else(|| {
                panic!(
                    "wide title prefix is visible at {cols}x{rows}: {}",
                    before.join("\n")
                )
            });
        let original_suffix = before_row.chars().skip(5).collect::<String>();
        let panes = dashboard.pane_rects(Rect::new(0, 0, cols, rows));

        dashboard.event_action(command_event(
            KeyCode::Modifier(ModifierKeyCode::LeftSuper),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        ));

        let held = rendered_rows(&dashboard, cols, rows);
        let labeled_row = held
            .iter()
            .find(|line| {
                line.split('│')
                    .next()
                    .is_some_and(|left| left.contains("界"))
            })
            .expect("labeled title remains visible");
        assert!(
            labeled_row.contains("[1]"),
            "visible Agent is labeled at {cols}x{rows}: {labeled_row}"
        );
        assert_eq!(
            labeled_row.chars().skip(5).collect::<String>(),
            original_suffix,
            "the status and Unicode title keep their columns at {cols}x{rows}"
        );
        assert_eq!(held.len(), before.len(), "no row is added at {cols}x{rows}");
        assert_eq!(
            dashboard.pane_rects(Rect::new(0, 0, cols, rows)),
            panes,
            "pane geometry and advertised PTY dimensions stay fixed at {cols}x{rows}"
        );
    }
}

#[test]
fn command_keys_share_one_hold_and_hidden_sidebar_never_intercepts() {
    let mut dashboard = agents_dashboard();
    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::RightSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    assert!(
        rendered_rows(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("agent-title-1") && row.contains("[1]")),
        "releasing one Command key leaves the other held"
    );
    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::RightSuper),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    assert!(
        !rendered_rows(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[1]"))
    );

    dashboard.key(KeyCode::Char('b'));
    let focused = dashboard.focused_session();
    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    assert!(
        !rendered_rows(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[1]"))
    );
    assert_eq!(
        dashboard.event_action(command_event(
            KeyCode::Char('2'),
            KeyModifiers::SUPER,
            KeyEventKind::Press,
        )),
        DashboardAction::None,
        "a hidden sidebar does not intercept the modified digit"
    );
    assert_eq!(dashboard.focused_session(), focused);
    assert!(dashboard.drain_outbox().is_empty());

    let mut terminal = agents_dashboard();
    assert_eq!(terminal.key(KeyCode::Enter), DashboardAction::Redraw);
    terminal.ctrl('g');
    terminal.key(KeyCode::Char('b'));
    assert_eq!(terminal.key(KeyCode::Enter), DashboardAction::Redraw);
    terminal.drain_outbox();
    terminal.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    assert_eq!(
        terminal.event_action(command_event(
            KeyCode::Char('2'),
            KeyModifiers::SUPER,
            KeyEventKind::Press,
        )),
        DashboardAction::None,
        "a Command digit cannot become a child byte when the sidebar is hidden"
    );
    assert!(terminal.drain_outbox().is_empty());
}

#[test]
fn a_nonlabel_command_key_cancels_without_navigating() {
    let mut dashboard = agents_dashboard();
    let focused = dashboard.focused_session();
    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    assert!(
        rendered_rows(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[1]"))
    );

    assert_eq!(
        dashboard.event_action(command_event(
            KeyCode::Backspace,
            KeyModifiers::SUPER,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), focused);
    assert!(
        !rendered_rows(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("[1]"))
    );
    assert!(rendered_footer(&dashboard, 120).contains("Session jump cancelled"));
}

#[test]
fn command_labels_keep_frozen_run_identity_and_allow_retry_after_unavailable_label() {
    let mut dashboard = agents_dashboard();
    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));

    let mut changed = agents_hierarchy();
    let mut inserted = changed.projects[0].workspaces[0].sessions[0].clone();
    inserted.id = SessionId(6);
    inserted.run = SessionRunId(6);
    inserted.name = "aaa-new-agent".into();
    inserted.title = Some("aaa-new-agent".into());
    let replacement = changed.projects[1].workspaces[0]
        .sessions
        .iter_mut()
        .find(|session| session.id == SessionId(2))
        .unwrap();
    replacement.run = SessionRunId(9);
    changed.projects[0].workspaces[0]
        .sessions
        .insert(0, inserted);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(changed)));

    let rows = rendered_rows(&dashboard, 120, 40);
    let replacement_row = rows
        .iter()
        .find(|row| row.contains("agent-title-2"))
        .expect("replacement row remains visible");
    assert!(!replacement_row.contains("[2]"), "new run is unavailable");
    let third_row = rows
        .iter()
        .find(|row| row.contains("agent-title-3"))
        .expect("third row remains visible");
    assert!(
        third_row.contains("[3]"),
        "new arrival and replacement do not renumber the frozen third target"
    );
    let inserted_row = rows
        .iter()
        .find(|row| row.contains("aaa-new-agent"))
        .expect("new Agent row is visible");
    assert!(!inserted_row.contains("[1]") && !inserted_row.contains("[2]"));
    let focused = dashboard.focused_session();
    assert_eq!(
        dashboard.event_action(command_event(
            KeyCode::Char('2'),
            KeyModifiers::SUPER,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), focused);
    assert!(rendered_footer(&dashboard, 120).contains("Agent unavailable"));
    assert_eq!(
        dashboard.event_action(command_event(
            KeyCode::Char('3'),
            KeyModifiers::SUPER,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw,
        "an unavailable frozen target does not prevent retrying a valid label"
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
}

#[test]
fn ten_visible_agents_use_two_digit_labels_and_search_stays_available() {
    let mut hierarchy = agents_hierarchy();
    let template = hierarchy
        .projects
        .iter()
        .flat_map(|project| &project.workspaces)
        .flat_map(|workspace| &workspace.sessions)
        .find(|session| session.id == SessionId(2))
        .unwrap()
        .clone();
    let sessions = &mut hierarchy.projects[0].workspaces[0].sessions;
    for id in 10..=16 {
        let mut agent = template.clone();
        agent.id = SessionId(id);
        agent.run = SessionRunId(id);
        agent.name = format!("agent-{id}");
        agent.title = Some(format!("agent-title-{id}"));
        sessions.push(agent);
    }
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    let rows = rendered_rows(&dashboard, 120, 40);
    assert!(rows.iter().any(|row| row.contains("[01]")));
    assert!(rows.iter().any(|row| row.contains("[10]")));
    assert!(rendered_footer(&dashboard, 120).contains("two digits"));
    dashboard.event_action(command_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    open_agents(&mut dashboard);
    assert!(palette_text(&dashboard).contains("Agents"));
}

#[test]
fn agent_search_excludes_current_shells_and_inactive_agents_across_projects() {
    let mut dashboard = agents_dashboard();
    open_agents(&mut dashboard);
    let rows = agent_results(&dashboard);
    assert!(rows.contains("agent-title-2 (#2)"));
    assert!(rows.contains("agent-title-3 (#3)"));
    assert!(!rows.contains("(#1)"));
    assert!(!rows.contains("(#4)"));
    assert!(!rows.contains("(#5)"));
    dashboard.event_action(Event::Paste("SPACELIFT progress AGENT-title-3".into()));
    assert!(!agent_results(&dashboard).contains("(#2)"));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let request = only_request(dashboard.drain_outbox());
    assert!(
        matches!(&request.request, Request::SetView { view } if view.focused == Some(SessionId(3)))
    );
    acknowledge_all_view_targets(&mut dashboard, request);
    assert!(!palette_text(&dashboard).contains("┌ Agents"));
    assert!(rendered_footer(&dashboard, 120).contains("Terminal mode"));
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
}

#[test]
fn agent_search_snapshot_disables_paused_and_replaced_runs_without_substitution() {
    for replacement in [false, true] {
        let mut dashboard = agents_dashboard();
        open_agents(&mut dashboard);
        let mut hierarchy = agents_hierarchy();
        let session = &mut hierarchy.projects[1].workspaces[0].sessions[0];
        if replacement {
            session.run = SessionRunId(2);
        } else {
            session.phase = SessionPhase::Paused;
        }
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
            hierarchy,
        )));
        assert!(
            agent_results(&dashboard).contains("(#2)"),
            "disabled row must retain its place"
        );
        let action = dashboard.key(KeyCode::Enter);
        assert_eq!(
            action,
            DashboardAction::Redraw,
            "invalid candidate cannot switch"
        );
        assert!(palette_text(&dashboard).contains("unavailable"));
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
            agents_hierarchy(),
        )));
        assert_eq!(
            dashboard.key(KeyCode::Enter),
            DashboardAction::Redraw,
            "a disabled candidate cannot silently revive"
        );
        dashboard.key(KeyCode::Down);
        assert_eq!(
            dashboard.key(KeyCode::Enter),
            ovrcr::tui::DashboardAction::Redraw
        );
        let request = only_request(dashboard.drain_outbox());
        assert!(
            matches!(request.request, Request::SetView { view } if view.focused == Some(SessionId(3)))
        );
    }
}

#[test]
fn agent_search_snapshot_defers_new_candidates_until_reopening() {
    let mut dashboard = agents_dashboard();
    open_agents(&mut dashboard);
    let mut hierarchy = agents_hierarchy();
    let session = &mut hierarchy.projects[1].workspaces[0].sessions[1];
    session.kind = SessionKind::Agent {
        name: "new-agent".into(),
    };
    session.title = Some("new-arrival".into());
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    assert!(!agent_results(&dashboard).contains("(#4)"));
    dashboard.key(KeyCode::Esc);
    open_agents(&mut dashboard);
    assert!(agent_results(&dashboard).contains("new-arrival (#4)"));
}

#[test]
fn agent_search_empty_filter_and_cancellation_never_reopen_or_write() {
    let mut dashboard = agents_dashboard();
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('s')),
        DashboardAction::PtyBytes(b"s".to_vec())
    );
    dashboard.ctrl('g');
    open_agents(&mut dashboard);
    assert_eq!(
        dashboard.event_action(Event::Paste("agent-title-1".into())),
        DashboardAction::Redraw
    );
    assert!(palette_text(&dashboard).contains("No matching agents"));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    dashboard.key(KeyCode::Esc);
    assert!(rendered_footer(&dashboard, 120).contains("BROWSE"));
    dashboard.key(KeyCode::Enter);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
    dashboard.ctrl('g');
    let mut hierarchy = agents_hierarchy();
    for session in hierarchy
        .projects
        .iter_mut()
        .flat_map(|p| &mut p.workspaces)
        .flat_map(|w| &mut w.sessions)
    {
        if session.id != SessionId(1) {
            session.phase = SessionPhase::Stopped;
        }
    }
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    open_agents(&mut dashboard);
    assert!(palette_text(&dashboard).contains("No other running agents"));
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
}

fn confirm_agent(dashboard: &mut Dashboard, id: u64) -> DashboardAction {
    dashboard.ctrl('g');
    open_agents(dashboard);
    dashboard.event_action(Event::Paste(format!("agent-title-{id}")));
    dashboard.key(KeyCode::Enter)
}

#[test]
fn agent_search_one_enter_waits_for_matching_screens_and_final_ack() {
    let mut dashboard = agents_dashboard();
    assert_eq!(
        confirm_agent(&mut dashboard, 3),
        ovrcr::tui::DashboardAction::Redraw
    );
    let request = only_request(dashboard.drain_outbox());
    let Request::SetView { view } = &request.request else {
        panic!("SetView");
    };
    assert_eq!(dashboard.key(KeyCode::Char('a')), DashboardAction::Redraw);
    assert!(
        !palette_text(&dashboard).contains("Register project"),
        "loading text cannot become a Browse command"
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("EARLY_PASTE".into())),
        DashboardAction::Redraw
    );
    assert!(rendered_footer(&dashboard, 120).contains("paste was not sent"));
    deliver_all_view_screens(&mut dashboard, request.request_id + 99, view);
    let mut wrong = view.clone();
    wrong.revision += 1;
    deliver_all_view_screens(&mut dashboard, request.request_id, &wrong);
    let pane = &view.panes[0];
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: pane.session,
            run: SessionRunId(99),
            revision: view.revision,
            size: pane.size,
            bytes: Vec::new(),
        },
    });
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Screen {
            session: SessionId(99),
            run: pane.run,
            revision: view.revision,
            size: pane.size,
            bytes: Vec::new(),
        },
    });
    assert!(rendered_footer(&dashboard, 120).contains("Loading agent"));
    deliver_all_view_screens(&mut dashboard, request.request_id, view);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id + 99,
        response: Response::Ok,
    });
    assert!(rendered_footer(&dashboard, 120).contains("Loading agent"));
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Ok,
    });
    assert!(rendered_footer(&dashboard, 120).contains("Terminal mode"));
    assert!(
        dashboard
            .drain_outbox()
            .iter()
            .all(|r| !matches!(r.request, Request::Input { .. })),
        "loading payload cannot replay after readiness"
    );
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::Output {
        session: pane.session,
        run: pane.run,
        revision: view.revision,
        bytes: b"\x1b[?2004h\x1b[?1h".to_vec(),
    }));
    assert_eq!(
        dashboard.event_action(Event::Paste("READY_PASTE".into())),
        DashboardAction::PtyBytes(b"\x1b[200~READY_PASTE\x1b[201~".to_vec())
    );
    assert_eq!(
        dashboard.key(KeyCode::Up),
        DashboardAction::PtyBytes(b"\x1bOA".to_vec())
    );
}

#[test]
fn agent_search_cancelled_history_begin_does_not_block_new_typing() {
    let mut dashboard = agents_dashboard();
    assert!({
        let _ = dashboard.key(KeyCode::PageUp);
        matches!(
            dashboard.drain_outbox().as_slice(),
            [ClientMessage {
                request: Request::HistoryBegin { .. },
                ..
            }]
        )
    });
    let action = confirm_agent(&mut dashboard, 3);
    pump_view(&mut dashboard, action);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec()),
        "cancelled history ownership cannot veto a fresh Agent choice"
    );
}

#[test]
fn agent_search_browse_cancels_auto_entry_but_a_fresh_choice_can_succeed() {
    let mut dashboard = agents_dashboard();
    assert_eq!(
        confirm_agent(&mut dashboard, 2),
        ovrcr::tui::DashboardAction::Redraw
    );
    let request = only_request(dashboard.drain_outbox());
    dashboard.ctrl('g');
    acknowledge_all_view_targets(&mut dashboard, request);
    assert!(rendered_footer(&dashboard, 120).contains("BROWSE"));
    assert_eq!(dashboard.key(KeyCode::Char('z')), DashboardAction::None);
    let action = confirm_agent(&mut dashboard, 3);
    pump_view(&mut dashboard, action);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
}

#[test]
fn agent_search_late_lifecycle_completion_cannot_reenable_typing() {
    for change in 0..3 {
        let mut dashboard = agents_dashboard();
        assert_eq!(
            confirm_agent(&mut dashboard, 2),
            ovrcr::tui::DashboardAction::Redraw
        );
        let request = only_request(dashboard.drain_outbox());
        let mut hierarchy = agents_hierarchy();
        let sessions = &mut hierarchy.projects[1].workspaces[0].sessions;
        match change {
            0 => sessions[0].phase = SessionPhase::Paused,
            1 => sessions[0].run = SessionRunId(2),
            _ => sessions.retain(|s| s.id != SessionId(2)),
        }
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
            hierarchy,
        )));
        acknowledge_all_view_targets(&mut dashboard, request);
        open_agents(&mut dashboard);
        assert!(
            palette_text(&dashboard).contains("Agents"),
            "late lifecycle completion must remain Browse"
        );
    }
}

#[test]
fn agent_search_rapid_choices_coalesce_and_require_a_fresh_a_to_b_to_a_view() {
    let mut dashboard = agents_dashboard();
    assert_eq!(
        confirm_agent(&mut dashboard, 2),
        ovrcr::tui::DashboardAction::Redraw
    );
    let first = only_request(dashboard.drain_outbox());
    assert_eq!(confirm_agent(&mut dashboard, 3), DashboardAction::Redraw);
    assert_eq!(confirm_agent(&mut dashboard, 2), DashboardAction::Redraw);
    let Request::SetView { view } = &first.request else {
        panic!("SetView");
    };
    deliver_all_view_screens(&mut dashboard, first.request_id, view);
    let follow = dashboard.handle_server_message(ServerMessage::Response {
        request_id: first.request_id,
        response: Response::Ok,
    });
    assert!(rendered_footer(&dashboard, 120).contains("Loading agent"));
    assert_eq!(follow.len(), 1, "one coalesced replacement view");
    let Request::SetView { view: fresh } = &follow[0].request else {
        panic!("fresh SetView");
    };
    assert!(fresh.revision > view.revision && fresh.focused == Some(SessionId(2)));
    acknowledge_all_view_targets(&mut dashboard, follow.into_iter().next().unwrap());
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
}

fn wide_agent_pair() -> Dashboard {
    let mut dashboard = agents_dashboard();
    dashboard.key(KeyCode::Char('b'));
    let action = dashboard.key(KeyCode::Char('v'));
    // `v` offers a session for the new pane; Esc keeps the suggestion.
    dashboard.key(KeyCode::Esc);
    pump_view(&mut dashboard, action);
    let action = confirm_agent(&mut dashboard, 3);
    pump_view(&mut dashboard, action);
    let action = confirm_agent(&mut dashboard, 1);
    pump_view(&mut dashboard, action);
    dashboard.ctrl('g');
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    dashboard
}

#[test]
fn agent_search_focus_only_round_trip_requires_a_fresh_view() {
    let mut dashboard = wide_agent_pair();
    assert_eq!(
        confirm_agent(&mut dashboard, 3),
        ovrcr::tui::DashboardAction::Redraw
    );
    let first = only_request(dashboard.drain_outbox());
    let Request::SetView { view } = &first.request else {
        panic!("SetView");
    };
    assert_eq!(view.panes.len(), 2);
    assert_eq!(confirm_agent(&mut dashboard, 1), DashboardAction::Redraw);
    assert_eq!(confirm_agent(&mut dashboard, 3), DashboardAction::Redraw);
    deliver_all_view_screens(&mut dashboard, first.request_id, view);
    let follow = dashboard.handle_server_message(ServerMessage::Response {
        request_id: first.request_id,
        response: Response::Ok,
    });
    assert_eq!(
        follow.len(),
        1,
        "focus-only coalescing must not grant the older view"
    );
    let Request::SetView { view: fresh } = &follow[0].request else {
        panic!("fresh SetView");
    };
    assert!(
        fresh.revision > view.revision
            && fresh.focused == Some(SessionId(3))
            && fresh.panes.len() == 2
    );
    assert!(rendered_footer(&dashboard, 120).contains("Loading agent"));
    acknowledge_all_view_targets(&mut dashboard, follow.into_iter().next().unwrap());
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
}

#[test]
fn agent_search_sibling_updates_superseding_the_view_cancel_pending_typing() {
    for replace_run in [true, false] {
        let mut dashboard = wide_agent_pair();
        assert_eq!(
            confirm_agent(&mut dashboard, 3),
            ovrcr::tui::DashboardAction::Redraw
        );
        let first = only_request(dashboard.drain_outbox());
        let Request::SetView { view } = &first.request else {
            panic!("SetView");
        };
        // Both screens arrive before a sibling update and the held final acknowledgement.
        deliver_all_view_screens(&mut dashboard, first.request_id, view);
        if replace_run {
            let mut sibling = agents_hierarchy()
                .projects
                .into_iter()
                .flat_map(|p| p.workspaces)
                .flat_map(|w| w.sessions)
                .find(|s| s.id == SessionId(1))
                .expect("assigned sibling");
            assert_eq!(sibling.id, SessionId(1));
            sibling.run = SessionRunId(2);
            dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(
                Box::new(sibling),
            )));
        } else {
            dashboard.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
                session: SessionId(1),
                run: SessionRunId(1),
                revision: view.revision,
            }));
        }
        let follow = dashboard.handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::Ok,
        });
        assert_eq!(
            follow.len(),
            1,
            "updated sibling requires a replacement view"
        );
        let Request::SetView { view: fresh } = &follow[0].request else {
            panic!("replacement SetView");
        };
        assert!(fresh.revision > view.revision && fresh.focused == Some(SessionId(3)));
        acknowledge_all_view_targets(&mut dashboard, follow.into_iter().next().unwrap());
        assert!(
            !rendered_footer(&dashboard, 120).contains("Loading agent"),
            "a superseded intent must cancel, not wait forever"
        );
        assert!(rendered_footer(&dashboard, 120).contains("cancelled"));
        assert_eq!(dashboard.key(KeyCode::Char('z')), DashboardAction::None);
        let action = confirm_agent(&mut dashboard, 1);
        assert_eq!(action, ovrcr::tui::DashboardAction::Redraw);
        let retry = only_request(dashboard.drain_outbox());
        acknowledge_all_view_targets(&mut dashboard, retry);
        assert_eq!(
            dashboard.key(KeyCode::Char('z')),
            DashboardAction::PtyBytes(b"z".to_vec())
        );
    }
}

#[test]
fn agent_search_failed_or_incomplete_view_cancels_auto_entry() {
    for incomplete in [false, true] {
        let mut dashboard = agents_dashboard();
        assert_eq!(
            confirm_agent(&mut dashboard, 2),
            ovrcr::tui::DashboardAction::Redraw
        );
        let request = only_request(dashboard.drain_outbox());
        let response = if incomplete {
            Response::Ok
        } else {
            Response::Error {
                code: ErrorCode::Conflict,
                message: "owned view refusal".into(),
            }
        };
        let follow = dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response,
        });
        pump_view_requests(&mut dashboard, follow);
        open_agents(&mut dashboard);
        assert!(
            palette_text(&dashboard).contains("Agents"),
            "view failure cannot enable Terminal through retry"
        );
        dashboard.key(KeyCode::Esc);
        let action = confirm_agent(&mut dashboard, 3);
        pump_view(&mut dashboard, action);
        assert_eq!(
            dashboard.key(KeyCode::Char('z')),
            DashboardAction::PtyBytes(b"z".to_vec()),
            "fresh choice after failure can succeed"
        );
    }
}

#[test]
fn agent_search_filtered_workspace_removal_never_substitutes_neighbor() {
    let mut hierarchy = agents_with_separate_destination();
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy.clone());
    open_agents(&mut dashboard);
    dashboard.event_action(Event::Paste("demo".into()));
    assert!(palette_text(&dashboard).contains("› agent-title-3"));
    hierarchy.projects[0]
        .workspaces
        .retain(|w| w.id != "destination-b");
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    let text = palette_text(&dashboard);
    assert!(
        text.contains("› agent-title-3") && text.contains("session removed"),
        "removed destination must keep its filtered identity: {text}"
    );
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(palette_text(&dashboard).contains("Agents"));
}

#[test]
fn agent_search_filtered_title_change_keeps_selected_identity() {
    let mut hierarchy = agents_with_separate_destination();
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy.clone());
    open_agents(&mut dashboard);
    dashboard.event_action(Event::Paste("agent-title".into()));
    hierarchy.projects[0]
        .workspaces
        .iter_mut()
        .flat_map(|w| &mut w.sessions)
        .find(|s| s.id == SessionId(3))
        .unwrap()
        .title = Some("renamed destination".into());
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy,
    )));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let request = only_request(dashboard.drain_outbox());
    assert!(
        matches!(request.request, Request::SetView { view } if view.focused == Some(SessionId(3))),
        "title update must not choose its neighbor"
    );
}

#[test]
fn agent_search_removal_fallback_disables_newly_focused_candidate() {
    let mut hierarchy = agents_hierarchy();
    let mut dashboard = agents_dashboard();
    open_agents(&mut dashboard);
    for workspace in hierarchy
        .projects
        .iter_mut()
        .flat_map(|p| &mut p.workspaces)
    {
        workspace.sessions.retain(|s| matches!(s.id.0, 2 | 3));
    }
    let messages = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::HierarchyChanged(hierarchy),
    ));
    pump_view_requests(&mut dashboard, messages);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    let text = palette_text(&dashboard);
    assert!(
        text.contains("Agents") && text.contains("already focused"),
        "new current Agent cannot be confirmed from its old candidate: {text}"
    );
}

#[test]
fn agent_search_is_reachable_from_view_menu_and_full_palette() {
    let mut dashboard = agents_dashboard();
    dashboard.key(KeyCode::Char(' '));
    dashboard.key(KeyCode::Char('v'));
    dashboard.key(KeyCode::Char('s'));
    assert!(palette_text(&dashboard).contains("Agents"));
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Char(':'));
    let inspect = only_request(dashboard.drain_outbox());
    assert_eq!(inspect.request, Request::Inspect);
    dashboard.event_action(Event::Paste("Agents".into()));
    dashboard.key(KeyCode::Enter);
    let text = palette_text(&dashboard);
    assert!(text.contains("Agents") && text.contains("agent-title-2"));
}

#[test]
fn agent_search_reaches_collapsed_workspace() {
    let mut dashboard = agents_dashboard();
    for _ in 0..2 {
        let action = dashboard.key(KeyCode::Up);
        pump_view(&mut dashboard, action);
    }
    dashboard.key(KeyCode::Enter);
    open_agents(&mut dashboard);
    dashboard.event_action(Event::Paste("agent-title-1".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let request = only_request(dashboard.drain_outbox());
    assert!(
        matches!(request.request, Request::SetView { view } if view.focused == Some(SessionId(1)))
    );
}

#[test]
fn agent_search_excludes_each_inactive_phase_and_archived_records() {
    for phase in [
        SessionPhase::Paused,
        SessionPhase::Stopped,
        SessionPhase::Interrupted,
        SessionPhase::Exited {
            code: Some(0),
            signal: None,
        },
    ] {
        let mut dashboard = agents_dashboard();
        let mut hierarchy = agents_hierarchy();
        hierarchy.projects[1].workspaces[0].sessions[0].phase = phase;
        hierarchy.projects[0].workspaces[0].sessions[0].archived = true;
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
            hierarchy,
        )));
        open_agents(&mut dashboard);
        assert!(palette_text(&dashboard).contains("No other running agents"));
        assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    }
}

#[test]
fn agent_search_reaches_hidden_sidebar_and_reuses_other_pane() {
    let mut dashboard = agents_dashboard();
    let action = dashboard.key(KeyCode::Char('v'));
    // `v` offers a session for the new pane; Esc keeps the suggestion.
    dashboard.key(KeyCode::Esc);
    pump_view(&mut dashboard, action);
    // Split selects a shell, so agent #1 in the other pane is now a destination.
    dashboard.key(KeyCode::Char('b'));
    open_agents(&mut dashboard);
    dashboard.event_action(Event::Paste("agent-title-1".into()));
    assert_eq!(
        dashboard.key(KeyCode::Enter),
        ovrcr::tui::DashboardAction::Redraw
    );
    let request = only_request(dashboard.drain_outbox());
    assert!(
        matches!(&request.request, Request::SetView { view } if view.focused == Some(SessionId(1)) && view.panes.len() == 2)
    );
    acknowledge_all_view_targets(&mut dashboard, request);
    assert!(rendered_footer(&dashboard, 120).contains("Terminal mode"));
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
    dashboard.ctrl('g');
    assert_eq!(dashboard.key(KeyCode::Tab), DashboardAction::Redraw);
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    let back = dashboard
        .request_view_at(Rect::new(0, 0, 88, 38))
        .expect("event-loop view for retained other pane");
    assert!(
        matches!(back.request, Request::SetView { view } if view.focused == Some(SessionId(4)) && view.panes.len() == 2 && view.panes[0].session == SessionId(1) && view.panes[1].session == SessionId(4))
    );
}

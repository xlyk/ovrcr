use super::*;
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
    let DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
        panic!("selection must request the existing view");
    };
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
        let DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
            panic!("another deliberate selection should succeed");
        };
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
    let DashboardAction::Request(request) = confirm_agent(&mut dashboard, 3) else {
        panic!("agent view request");
    };
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
    assert!(matches!(
        dashboard.key(KeyCode::PageUp),
        DashboardAction::Request(ClientMessage {
            request: Request::HistoryBegin { .. },
            ..
        })
    ));
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
    let DashboardAction::Request(request) = confirm_agent(&mut dashboard, 2) else {
        panic!("agent view request");
    };
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
        let DashboardAction::Request(request) = confirm_agent(&mut dashboard, 2) else {
            panic!("agent view request");
        };
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
    let DashboardAction::Request(first) = confirm_agent(&mut dashboard, 2) else {
        panic!("first view request");
    };
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
    let DashboardAction::Request(first) = confirm_agent(&mut dashboard, 3) else {
        panic!("first focus-only view");
    };
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
        let DashboardAction::Request(first) = confirm_agent(&mut dashboard, 3) else {
            panic!("two-agent view");
        };
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
        let DashboardAction::Request(retry) = action else {
            panic!(
                "fresh choice after cancellation: {action:?}\n{}",
                palette_text(&dashboard)
            );
        };
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
        let DashboardAction::Request(request) = confirm_agent(&mut dashboard, 2) else {
            panic!("view request");
        };
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
        for request in follow {
            pump_view(&mut dashboard, DashboardAction::Request(request));
        }
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
    let DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
        panic!("original destination must remain selectable");
    };
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
    for message in messages {
        pump_view(&mut dashboard, DashboardAction::Request(message));
    }
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
    let DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
        panic!("collapsed agent remains searchable");
    };
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
    pump_view(&mut dashboard, action);
    // Split selects a shell, so agent #1 in the other pane is now a destination.
    dashboard.key(KeyCode::Char('b'));
    open_agents(&mut dashboard);
    dashboard.event_action(Event::Paste("agent-title-1".into()));
    let DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
        panic!("other-pane choice must request a focus change");
    };
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

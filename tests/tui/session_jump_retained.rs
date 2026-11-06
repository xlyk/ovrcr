use super::*;
use crossterm::event::ModifierKeyCode;
use ovrcr::protocol::{
    ActivitySample, AgentBinding, AgentProvider, AgentSnapshot, HealthSample, InputKind,
    InputRequest, ReadyObservation, ReporterHealth, SampleQuality, SessionKind, SessionRecovery,
    SessionRunId,
};

fn retained_jump_hierarchy() -> HierarchySnapshot {
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects.truncate(1);
    hierarchy.projects[0].workspaces.truncate(1);
    let workspace = &mut hierarchy.projects[0].workspaces[0];
    let template = workspace.sessions[0].clone();
    workspace.sessions = (1..=5)
        .map(|id| {
            let mut session = template.clone();
            session.id = SessionId(id);
            session.run = SessionRunId(id);
            session.kind = SessionKind::Agent {
                name: "fixture-agent".into(),
            };
            session.label = "fixture-agent".into();
            session.name = format!("agent-{id}");
            session.title = Some(format!("agent-title-{id}"));
            session.phase = match id {
                1 => SessionPhase::Exited {
                    code: Some(0),
                    signal: None,
                },
                2 => SessionPhase::Stopped,
                3 => SessionPhase::Interrupted,
                4 => SessionPhase::Running,
                _ => SessionPhase::Paused,
            };
            session.pid = (id == 4 || id == 5).then_some(333);
            session.recovery = None;
            session.agent = None;
            session.unread = None;
            session
        })
        .collect();

    let retained = &mut workspace.sessions[0];
    let binding = AgentBinding {
        provider: AgentProvider::Codex,
        invocation: "fixture-retained".into(),
        conversation: "reporting-only".into(),
        generation: 1,
    };
    retained.agent = Some(AgentSnapshot {
        binding: binding.clone(),
        activity: Some(ActivitySample {
            state: AgentActivity::WaitingInput,
            quality: SampleQuality::Observed,
            turn: Some("turn-retained".into()),
        }),
        metrics: None,
        health: HealthSample {
            state: ReporterHealth::Connected,
            reason: None,
        },
        activity_revision: 2,
        metrics_revision: 0,
        health_revision: 1,
        input_revision: 1,
        input_requests: vec![InputRequest {
            id: "question:retained".into(),
            kind: InputKind::Confirm,
        }],
        native_control: None,
        native_control_revision: 0,
    });
    retained.unread = Some(ReadyObservation {
        binding,
        turn: Some("turn-retained".into()),
        activity_revision: 1,
    });
    hierarchy
}

fn jump_event(code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(code, modifiers, kind))
}

#[test]
fn retained_and_running_agents_share_visible_labels_and_retained_selection_stays_in_browse() {
    let mut dashboard = Dashboard::with_input_profile(
        TerminalSize {
            rows: 40,
            cols: 120,
        },
        super::DashboardInputProfile::VerifiedOvrcrGui,
    );
    dashboard.install_hierarchy(retained_jump_hierarchy());
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_focus(SessionId(2));
    let before = rendered_rows(&dashboard, 120, 40);
    let retained_title = before
        .iter()
        .find(|row| row.contains("agent-title-1"))
        .expect("retained Agent title is drawn")
        .chars()
        .skip(5)
        .collect::<String>();

    dashboard.event_action(jump_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));

    let held = rendered_rows(&dashboard, 120, 40);
    for (title, label) in [
        ("agent-title-1", "[1]"),
        ("agent-title-2", "[2]"),
        ("agent-title-3", "[3]"),
        ("agent-title-4", "[4]"),
    ] {
        let row = held
            .iter()
            .find(|row| {
                row.split('│')
                    .next()
                    .is_some_and(|sidebar| sidebar.contains(title))
            })
            .unwrap_or_else(|| panic!("visible Agent title {title} remains drawn"));
        assert!(row.contains(label), "{title} receives {label}: {row}");
    }
    let paused_row = held
        .iter()
        .find(|row| {
            row.split('│')
                .next()
                .is_some_and(|sidebar| sidebar.contains("agent-title-5"))
        })
        .expect("paused Agent title remains drawn");
    assert!(
        !paused_row.contains("[5]"),
        "paused Agents are not retained"
    );
    assert_eq!(held.len(), before.len(), "retained labels add no rows");

    assert_eq!(
        dashboard.event_action(jump_event(
            KeyCode::Char('1'),
            KeyModifiers::SUPER,
            KeyEventKind::Press,
        )),
        DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(rendered_footer(&dashboard, 120).contains("Session selected"));
    let request = only_request(dashboard.drain_outbox());
    assert!(matches!(
        &request.request,
        Request::SetView { view } if view.focused == Some(SessionId(1))
    ));
    assert!(
        rendered_rows(&dashboard, 120, 40)
            .iter()
            .find(|row| {
                row.split('│')
                    .next()
                    .is_some_and(|sidebar| sidebar.contains("agent-title-1"))
            })
            .is_some_and(|row| row.chars().skip(5).collect::<String>() == retained_title),
        "selection preserves retained status and title columns"
    );

    dashboard.event_action(jump_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    assert!(
        rendered_footer(&dashboard, 120).starts_with("BROWSE"),
        "releasing Command cannot resume a retained destination"
    );
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn running_selection_from_retained_browse_waits_for_release_and_the_view_ack() {
    let mut dashboard = Dashboard::with_input_profile(
        TerminalSize {
            rows: 40,
            cols: 120,
        },
        super::DashboardInputProfile::VerifiedOvrcrGui,
    );
    dashboard.install_hierarchy(retained_jump_hierarchy());
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_focus(SessionId(2));

    dashboard.event_action(jump_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    dashboard.event_action(jump_event(
        KeyCode::Char('1'),
        KeyModifiers::SUPER,
        KeyEventKind::Press,
    ));
    let retained_view = only_request(dashboard.drain_outbox());
    acknowledge_all_view_targets(&mut dashboard, retained_view);
    dashboard.event_action(jump_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    assert!(rendered_footer(&dashboard, 120).starts_with("BROWSE"));

    dashboard.event_action(jump_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    dashboard.event_action(jump_event(
        KeyCode::Char('4'),
        KeyModifiers::SUPER,
        KeyEventKind::Press,
    ));
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    let running_view = only_request(dashboard.drain_outbox());
    assert!(matches!(
        &running_view.request,
        Request::SetView { view } if view.focused == Some(SessionId(4))
    ));
    acknowledge_all_view_targets(&mut dashboard, running_view);
    assert!(
        rendered_footer(&dashboard, 120).contains("Session selected"),
        "view readiness alone cannot type while Command remains held"
    );
    assert!(rendered_footer(&dashboard, 120).starts_with("Session selected"));

    dashboard.event_action(jump_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ));
    let footer = rendered_footer(&dashboard, 120);
    assert!(
        footer.starts_with("Loading agent") && !footer.starts_with("Terminal mode"),
        "the public event seam remains gated until the production input-drain boundary: {footer}"
    );
}

#[test]
fn inspecting_an_interrupted_retained_agent_does_not_auto_recover_it() {
    let mut hierarchy = retained_jump_hierarchy();
    let interrupted = hierarchy.projects[0].workspaces[0]
        .sessions
        .iter_mut()
        .find(|session| session.id == SessionId(3))
        .unwrap();
    interrupted.recovery = Some(SessionRecovery {
        conversation: Some("recoverable-conversation".into()),
        attached: false,
        requires_ack: false,
        unavailable: None,
        failure: None,
    });

    let mut dashboard = Dashboard::with_input_profile(
        TerminalSize {
            rows: 40,
            cols: 120,
        },
        super::DashboardInputProfile::VerifiedOvrcrGui,
    );
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_focus(SessionId(2));
    dashboard.event_action(jump_event(
        KeyCode::Modifier(ModifierKeyCode::LeftSuper),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ));
    dashboard.event_action(jump_event(
        KeyCode::Char('3'),
        KeyModifiers::SUPER,
        KeyEventKind::Press,
    ));

    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
    let requests = dashboard.drain_outbox();
    assert_eq!(
        requests.len(),
        1,
        "inspection must not trigger automatic recovery: {requests:#?}"
    );
    assert!(matches!(
        &requests[0].request,
        Request::SetView { view } if view.focused == Some(SessionId(3))
    ));
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn a_captured_target_that_changes_phase_run_or_archive_cannot_be_reassigned() {
    for changed_target in ["retained-run", "running-phase", "archived"] {
        let mut dashboard = Dashboard::with_input_profile(
            TerminalSize {
                rows: 40,
                cols: 120,
            },
            super::DashboardInputProfile::VerifiedOvrcrGui,
        );
        dashboard.install_hierarchy(retained_jump_hierarchy());
        dashboard.install_area(Rect::new(0, 0, 120, 40));
        dashboard.install_focus(SessionId(2));
        dashboard.event_action(jump_event(
            KeyCode::Modifier(ModifierKeyCode::LeftSuper),
            KeyModifiers::NONE,
            KeyEventKind::Press,
        ));

        let mut changed = retained_jump_hierarchy();
        let target_id = if changed_target == "running-phase" {
            SessionId(4)
        } else {
            SessionId(1)
        };
        let target = changed.projects[0].workspaces[0]
            .sessions
            .iter_mut()
            .find(|session| session.id == target_id)
            .unwrap();
        match changed_target {
            "retained-run" => {
                target.run = SessionRunId(91);
                target.phase = SessionPhase::Running;
            }
            "running-phase" => {
                target.phase = SessionPhase::Exited {
                    code: Some(0),
                    signal: None,
                }
            }
            "archived" => target.archived = true,
            _ => unreachable!(),
        }
        dashboard
            .handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(changed)));
        let _ = dashboard.drain_outbox();

        let (digit, expected_session) = if target_id == SessionId(4) {
            ('4', SessionId(4))
        } else {
            ('1', SessionId(1))
        };
        dashboard.event_action(jump_event(
            KeyCode::Char(digit),
            KeyModifiers::SUPER,
            KeyEventKind::Press,
        ));
        assert_eq!(
            dashboard.focused_session(),
            Some(SessionId(2)),
            "{changed_target} target must not select a neighboring session {expected_session:?}"
        );
        assert!(
            dashboard.drain_outbox().is_empty(),
            "{changed_target} target must not request a view or lifecycle action"
        );
    }
}

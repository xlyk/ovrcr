use super::*;
use ovrcr::protocol::{
    ActivitySample, AgentBinding, AgentProvider, AgentSnapshot, HealthSample, InputKind,
    InputRequest, ReadyObservation, ReporterHealth, SampleQuality, SessionKind,
};

fn population(count: u64) -> HierarchySnapshot {
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects.truncate(1);
    hierarchy.projects[0].workspaces.truncate(1);
    let project = hierarchy.projects[0].name.clone();
    let workspace = &mut hierarchy.projects[0].workspaces[0];
    let template = workspace.sessions[0].clone();
    workspace.sessions = (1..=count)
        .map(|id| {
            let mut row = template.clone();
            row.id = SessionId(id);
            row.project = project.clone();
            row.workspace = workspace.id.clone();
            row.name = format!("agent-{id:02}");
            row.title = Some(format!("agent-title-{id:02}"));
            row.kind = SessionKind::Agent {
                name: "codex".into(),
            };
            row.phase = SessionPhase::Running;
            row.activity = AgentActivity::Idle;
            row.agent = None;
            row.unread = None;
            row
        })
        .collect();
    hierarchy
}

fn reporting(
    row: &mut SessionSummary,
    state: AgentActivity,
    unread: bool,
    input: bool,
    health: ReporterHealth,
) {
    let binding = AgentBinding {
        provider: AgentProvider::Codex,
        invocation: format!("fixture-{}", row.id.0),
        conversation: format!("conversation-{}", row.id.0),
        generation: 1,
    };
    row.agent = Some(AgentSnapshot {
        binding: binding.clone(),
        activity: Some(ActivitySample {
            state,
            quality: SampleQuality::Observed,
            turn: Some("turn-one".into()),
        }),
        metrics: None,
        health: HealthSample {
            state: health,
            reason: (health == ReporterHealth::Unavailable)
                .then(|| "fixture reporting lost".into()),
        },
        activity_revision: 1,
        metrics_revision: 0,
        health_revision: 1,
        input_revision: u64::from(input),
        input_requests: if input {
            vec![InputRequest {
                id: "question:fixture".into(),
                kind: InputKind::Confirm,
            }]
        } else {
            Vec::new()
        },
    });
    // Accepted effective activity, not this deliberately conflicting legacy rollup, wins.
    row.activity = AgentActivity::Error;
    row.unread = unread.then(|| ReadyObservation {
        binding,
        turn: Some("turn-one".into()),
        activity_revision: 1,
    });
}

fn ranked_hierarchy() -> HierarchySnapshot {
    let mut hierarchy = population(9);
    let rows = &mut hierarchy.projects[0].workspaces[0].sessions;
    reporting(
        &mut rows[0],
        AgentActivity::Busy,
        true,
        true,
        ReporterHealth::Connected,
    );
    reporting(
        &mut rows[1],
        AgentActivity::Busy,
        false,
        true,
        ReporterHealth::Connected,
    );
    reporting(
        &mut rows[2],
        AgentActivity::Busy,
        true,
        false,
        ReporterHealth::Connected,
    );
    reporting(
        &mut rows[3],
        AgentActivity::ResponseReady,
        false,
        false,
        ReporterHealth::Connected,
    );
    reporting(
        &mut rows[4],
        AgentActivity::Error,
        false,
        false,
        ReporterHealth::Connected,
    );
    reporting(
        &mut rows[5],
        AgentActivity::Unknown,
        false,
        false,
        ReporterHealth::Connected,
    );
    reporting(
        &mut rows[6],
        AgentActivity::Busy,
        false,
        false,
        ReporterHealth::Unavailable,
    );
    reporting(
        &mut rows[7],
        AgentActivity::ResponseReady,
        true,
        true,
        ReporterHealth::Connected,
    );
    reporting(
        &mut rows[8],
        AgentActivity::Idle,
        true,
        false,
        ReporterHealth::Unavailable,
    );
    hierarchy
}

fn attention_dashboard(hierarchy: HierarchySnapshot) -> Dashboard {
    let mut dashboard = dashboard_fixture();
    dashboard.install_hierarchy(hierarchy);
    dashboard.key(KeyCode::Char('s'));
    assert!(palette_text(&dashboard).contains("┌ Agents"));
    dashboard
}

fn selected(dashboard: &Dashboard, id: u64) {
    assert!(
        palette_text(dashboard).contains(&format!("› agent-title-{id:02} (#{id})")),
        "wrong highlighted identity: {}",
        palette_text(dashboard)
    );
}

#[test]
fn attention_exact_priority_ties_effective_input_and_current_exclusion() {
    let mut dashboard = attention_dashboard(ranked_hierarchy());
    for id in [2, 8, 3, 9, 4, 5, 6, 7] {
        selected(&dashboard, id);
        let text = palette_text(&dashboard);
        assert!(
            !text.contains("agent-title-01 (#1)"),
            "current attention must be excluded"
        );
        match id {
            2 | 8 => assert!(text.contains("input needed")),
            3 | 9 => assert!(text.contains("Unread")),
            4 => assert!(text.contains("response ready")),
            5 => assert!(text.contains("error observed")),
            6 => assert!(text.contains("unknown observed")),
            7 => assert!(text.contains("fixture reporting lost")),
            _ => unreachable!(),
        }
        dashboard.key(KeyCode::Down);
    }
}

#[test]
fn attention_live_status_and_titles_refresh_without_reordering_or_reviewing() {
    let mut hierarchy = ranked_hierarchy();
    let mut dashboard = attention_dashboard(hierarchy.clone());
    dashboard.key(KeyCode::Down);
    selected(&dashboard, 8);
    let unread = hierarchy.projects[0].workspaces[0].sessions[7]
        .unread
        .clone()
        .unwrap();
    reporting(
        &mut hierarchy.projects[0].workspaces[0].sessions[1],
        AgentActivity::Idle,
        false,
        false,
        ReporterHealth::Connected,
    );
    reporting(
        &mut hierarchy.projects[0].workspaces[0].sessions[6],
        AgentActivity::Busy,
        false,
        true,
        ReporterHealth::Connected,
    );
    let chosen = &mut hierarchy.projects[0].workspaces[0].sessions[7];
    chosen.title = Some("new highlighted title".into());
    chosen.agent.as_mut().unwrap().input_requests.clear();
    chosen.agent.as_mut().unwrap().health.state = ReporterHealth::Unavailable;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
        hierarchy.clone(),
    )));
    assert!(palette_text(&dashboard).contains("› new highlighted title (#8)"));
    assert!(palette_text(&dashboard).contains("Unread Unavailable"));
    let DashboardAction::Request(chosen) = dashboard.key(KeyCode::Enter) else {
        panic!("live-updated highlighted destination");
    };
    assert!(
        matches!(&chosen.request, Request::SetView { view } if view.focused == Some(SessionId(8)))
    );
    acknowledge_all_view_targets(&mut dashboard, chosen);
    dashboard
        .draw(&mut Terminal::new(TestBackend::new(88, 38)).unwrap())
        .unwrap();
    dashboard.ctrl('g');
    let DashboardAction::Request(review) = dashboard.key(KeyCode::Char('R')) else {
        panic!("selection cannot mark Unread reviewed");
    };
    assert_eq!(
        review.request,
        Request::MarkReviewed {
            session: SessionId(8),
            expected: unread.clone()
        }
    );
    // Rebuild independently to inspect the actual request and accepted facts without a helper seam.
    let mut dashboard = attention_dashboard(hierarchy.clone());
    selected(&dashboard, 7);
    dashboard.event_action(Event::Paste("agent-title-08".into()));
    assert!(
        palette_text(&dashboard).contains("No matching agents"),
        "reopening refreshes the searchable identity"
    );
    dashboard.ctrl('u');
    dashboard.event_action(Event::Paste("new highlighted title".into()));
    let DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
        panic!("ranked destination request");
    };
    assert!(
        matches!(&request.request, Request::SetView { view } if view.focused == Some(SessionId(8)))
    );
    acknowledge_all_view_targets(&mut dashboard, request);
    assert_eq!(dashboard.focused_session(), Some(SessionId(8)));
    dashboard
        .draw(&mut Terminal::new(TestBackend::new(88, 38)).unwrap())
        .unwrap();
    dashboard.ctrl('g');
    let DashboardAction::Request(review) = dashboard.key(KeyCode::Char('R')) else {
        panic!("Unread must remain reviewable after opening and selection");
    };
    assert_eq!(
        review.request,
        Request::MarkReviewed {
            session: SessionId(8),
            expected: unread
        }
    );
}

#[test]
fn attention_compact_unicode_name_preserves_title_identity_and_session_id() {
    for name in ["é".repeat(10), "界".repeat(5), "🙂".repeat(5)] {
        let mut hierarchy = population(3);
        hierarchy.projects[0].workspaces[0].sessions[1].kind =
            SessionKind::Agent { name: name.clone() };
        let dashboard = attention_dashboard(hierarchy);
        let mut terminal = Terminal::new(TestBackend::new(24, 9)).unwrap();
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        let row = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .skip(2 * 24)
            .take(24)
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(
            row.contains("age")
                && row.contains("#2")
                && row
                    .matches(&name.chars().next().unwrap().to_string())
                    .count()
                    == name.chars().count(),
            "display-column budget must preserve Unicode Agent identity and title: {row}"
        );
    }
}

#[test]
fn attention_compact_retained_unread_without_reporter_is_unavailable() {
    let mut hierarchy = population(3);
    let row = &mut hierarchy.projects[0].workspaces[0].sessions[1];
    reporting(
        row,
        AgentActivity::ResponseReady,
        true,
        false,
        ReporterHealth::Connected,
    );
    row.agent = None;
    row.activity = AgentActivity::Unknown;
    hierarchy.projects[0].workspaces[0].sessions[2].activity = AgentActivity::Unknown;
    let mut dashboard = attention_dashboard(hierarchy);
    let mut terminal = Terminal::new(TestBackend::new(24, 9)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let detail = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .skip(3 * 24)
        .take(24)
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        detail.contains("Unread Unavailable"),
        "retained Unread needs truthful no-reporter health: {detail}"
    );
    dashboard.key(KeyCode::Down);
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let detail = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .skip(3 * 24)
        .take(24)
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        detail.contains("Unknown") && !detail.contains("Unavailable"),
        "ordinary unsupported agents remain Unknown, not Unavailable: {detail}"
    );
}

#[test]
fn attention_unavailable_waiting_and_compact_location_remain_textual() {
    let mut hierarchy = population(3);
    reporting(
        &mut hierarchy.projects[0].workspaces[0].sessions[1],
        AgentActivity::Busy,
        false,
        true,
        ReporterHealth::Unavailable,
    );
    let dashboard = attention_dashboard(hierarchy);
    let text = palette_text(&dashboard);
    assert!(
        text.contains("input needed") && text.contains("fixture reporting lost"),
        "reporting loss cannot be represented only by color: {text}"
    );
    let mut terminal = Terminal::new(TestBackend::new(24, 9)).unwrap();
    terminal
        .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        text.contains("codex") && text.contains("Input") && text.contains("Unavailable"),
        "compact identity, Input and reporting loss must remain visible together: {text}"
    );
    assert!(
        text.contains("progress") && text.contains("spaceli"),
        "compact context must retain project and workspace: {text}"
    );
}

#[test]
fn attention_maximum_population_offscreen_filter_and_compact_duplicate_identity() {
    let mut hierarchy = population(50);
    for row in &mut hierarchy.projects[0].workspaces[0].sessions {
        row.title = Some("a duplicate title with a long searchable tail NEEDLE".into());
    }
    let mut dashboard = attention_dashboard(hierarchy);
    for _ in 0..48 {
        dashboard.key(KeyCode::Down);
    }
    assert!(palette_text(&dashboard).contains("(#50)"));
    for (width, height) in [(42, 12), (24, 9)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 0))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(
            text.contains("#50") && text.contains("Idle") && text.contains("codex"),
            "identity/status clipped away at {width}x{height}: {text}"
        );
        assert!(text.contains("OK") || text.contains("Open"));
    }
    dashboard.event_action(Event::Paste("nEeDlE #50".into()));
    let DashboardAction::Request(request) = dashboard.key(KeyCode::Enter) else {
        panic!("last of49 eligible agents remains reachable");
    };
    assert!(
        matches!(&request.request, Request::SetView { view } if view.focused == Some(SessionId(50)))
    );
    acknowledge_all_view_targets(&mut dashboard, request);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
}

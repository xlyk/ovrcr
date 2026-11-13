//! The Operator sidebar section leads the session tree, after attention when
//! present; Quota remains below the tree. Every case drives the production
//! Dashboard input/render path with real protocol replies.

use crate::*;
use ovrcr::protocol::{
    ErrorCode, OperatorConversationId, OperatorConversationSummary, OperatorHistory,
    OperatorMessage, OperatorMessageContent, OperatorRequest, OperatorResponse, OperatorTurn,
    OperatorTurnId, OperatorTurnIdentity, OperatorTurnState, OperatorWorkerEvent, QuotaSnapshot,
    QuotaState, QuotaWindow, SessionKind, SessionRunId,
};

fn operator_dashboard_fixture() -> Dashboard {
    let mut dashboard = crate::dashboard_fixture();
    let mut settings = ovrcr::tui::Settings::default();
    settings.operator.sidebar = true;
    dashboard.install_settings(settings);
    dashboard
}

fn summary() -> OperatorConversationSummary {
    OperatorConversationSummary {
        run: SessionRunId(1),
        worker_stopped: true,
        worker_live: false,
        id: OperatorConversationId(7),
        session: SessionId(99),
        model: None,
        title: "Operator".into(),
        current_turn: None,
        reasoning: None,
        current_turn_started_unix_ms: None,
        last_reply_unix_ms: None,
    }
}

fn summary_with_turn(
    state: OperatorTurnState,
    worker_stopped: bool,
) -> OperatorConversationSummary {
    let mut value = summary();
    let identity = OperatorTurnIdentity {
        conversation: value.id,
        turn: OperatorTurnId(1),
        session: value.session,
        run: value.run,
    };
    value.worker_stopped = worker_stopped;
    value.current_turn = Some(OperatorTurn {
        identity,
        state,
        sequence: 2,
    });
    value.current_turn_started_unix_ms = Some(0);
    value.last_reply_unix_ms = Some(0);
    value
}

fn operator_requests(messages: &[ClientMessage]) -> Vec<&OperatorRequest> {
    messages
        .iter()
        .filter_map(|message| match &message.request {
            Request::Operator(request) => Some(request),
            _ => None,
        })
        .collect()
}

fn respond(dashboard: &mut Dashboard, request_id: u64, value: OperatorConversationSummary) {
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Operator(OperatorResponse::Opened(value)),
    });
}

fn open_with_history(
    dashboard: &mut Dashboard,
    area: Rect,
    summary: OperatorConversationSummary,
    messages: Vec<OperatorMessage>,
) {
    let open = open_once(dashboard, area, 2, 2);
    let followups = dashboard.handle_server_message(ServerMessage::Response {
        request_id: open,
        response: Response::Operator(OperatorResponse::Opened(summary.clone())),
    });
    let read = followups
        .into_iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Read { .. })
            )
        })
        .expect("opening the conversation reads its retained history");
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: read.request_id,
        response: Response::Operator(OperatorResponse::History(OperatorHistory {
            conversation: summary,
            messages,
            next_after: None,
        })),
    });
}

fn assistant_reply(text: &str) -> Vec<OperatorMessage> {
    vec![
        OperatorMessage {
            ordinal: 1,
            turn: OperatorTurnId(1),
            content: OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                text.into(),
            )),
        },
        OperatorMessage {
            ordinal: 2,
            turn: OperatorTurnId(1),
            content: OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                leased: false,
            }),
        },
    ]
}

fn click(dashboard: &mut Dashboard, area: Rect, column: u16, row: u16) -> DashboardAction {
    dashboard.mouse_action(
        mouse_event(
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            KeyModifiers::NONE,
        ),
        area,
    )
}

/// Clicks the Operator section once and returns the single Open it sends.
fn open_once(dashboard: &mut Dashboard, area: Rect, column: u16, row: u16) -> u64 {
    assert_eq!(
        click(dashboard, area, column, row),
        DashboardAction::Redraw,
        "the Operator section is navigation"
    );
    let mut out = dashboard.drain_outbox();
    assert_eq!(
        operator_requests(&out),
        [&OperatorRequest::Open],
        "one Open from the section: {out:?}"
    );
    out.drain(..)
        .find(|message| matches!(message.request, Request::Operator(OperatorRequest::Open)))
        .expect("an Open request")
        .request_id
}

fn session_id_in(row: &str) -> Option<u64> {
    row.split("session-")
        .nth(1)?
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

fn sidebar_line(row: &str) -> &str {
    row.split_once('│').map_or(row, |(sidebar, _)| sidebar)
}

fn quota_with_windows() -> QuotaSnapshot {
    let mut snapshot = QuotaSnapshot::default();
    for provider in [
        &mut snapshot.claude,
        &mut snapshot.codex,
        &mut snapshot.grok,
        &mut snapshot.cursor,
    ] {
        provider.state = QuotaState::Current;
        provider.reason = None;
        provider.observed_unix_ms = Some(0);
        provider.checked_unix_ms = Some(0);
        provider.next_check_unix_ms = None;
        provider.windows = ["5h", "7d"]
            .into_iter()
            .map(|label| QuotaWindow {
                id: label.into(),
                label: label.into(),
                general: true,
                used_basis_points: Some(4_200),
                over_limit: false,
                resets_unix_ms: None,
            })
            .collect();
    }
    snapshot
}

#[test]
fn attention_and_operator_sections_lead_the_tree_and_leave_blank_rows() {
    if crate::pi_available_test::run_with_pi_available(
        "operator_dock::attention_and_operator_sections_lead_the_tree_and_leave_blank_rows",
    ) {
        return;
    }
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = operator_dashboard_fixture();
    dashboard.install_area(area);
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[1].sessions[0].activity = AgentActivity::WaitingInput;
    dashboard.install_hierarchy(hierarchy);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(Box::new(
        quota_with_windows(),
    ))));

    let rows = rendered_rows(&dashboard, 120, 40);
    assert!(rows[1].contains("? 1 needs input"), "{rows:?}");
    assert!(
        sidebar_line(&rows[2]).trim().is_empty(),
        "attention has one blank row: {rows:?}"
    );
    assert!(rows[3].starts_with("┃ 󰚩 Operator"), "{rows:?}");
    assert!(
        rows[4].contains("Off") || rows[4].contains("Not set up"),
        "{rows:?}"
    );
    assert!(
        rows[5].contains("Ctrl-o"),
        "the detail gives the Off entry points: {rows:?}"
    );
    assert!(
        sidebar_line(&rows[6]).trim().is_empty(),
        "Operator has one blank row: {rows:?}"
    );
    assert!(
        rows[7].contains("CONSIGINT"),
        "the tree follows Operator: {rows:?}"
    );
    let quota = rows
        .iter()
        .position(|row| row.contains("QUOTA LEFT"))
        .expect("Quota remains below the tree");
    assert!(quota > 7, "{rows:?}");
}

#[test]
fn shrinking_drops_gaps_then_quota_and_operator_while_attention_stays_visible() {
    let mut dashboard = operator_dashboard_fixture();
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects[1].workspaces[1].sessions[0].activity = AgentActivity::WaitingInput;
    dashboard.install_hierarchy(hierarchy);
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(Box::new(
        quota_with_windows(),
    ))));

    let at = |dashboard: &mut Dashboard, height: u16| {
        dashboard.install_area(Rect::new(0, 0, 120, height));
        rendered_rows(dashboard, 120, height)
            .into_iter()
            .map(|row| sidebar_line(&row).trim_end().to_owned())
            .collect::<Vec<_>>()
    };
    let full = at(&mut dashboard, 35);
    assert!(
        sidebar_line(&full[2]).trim().is_empty(),
        "the attention gap exists while roomy: {full:?}"
    );
    assert!(
        full[3].starts_with("┃ 󰚩 Operator"),
        "Operator follows attention: {full:?}"
    );
    assert!(
        sidebar_line(&full[6]).trim().is_empty(),
        "the Operator gap exists while roomy: {full:?}"
    );
    assert!(
        full[7].contains("CONSIGINT"),
        "tree follows Operator: {full:?}"
    );
    assert!(
        full.iter().any(|row| row.contains("USAGE ·"))
            && full.iter().any(|row| row.contains("1 input")),
        "Usage shows its heading and non-zero counters: {full:?}"
    );
    assert!(
        full.iter().any(|row| row.contains("RESOURCES"))
            && full.iter().any(|row| row.contains("QUOTA LEFT")),
        "Resources and full Quota follow: {full:?}"
    );

    // Each section's first disappearing form records its rung; the rungs must
    // follow SIDEBAR_SHRINK_ORDER.
    let mut rung = |key: &str| {
        let predicate = |rows: &[String]| match key {
            "attention gap gone" => rows
                .get(2)
                .is_some_and(|row| row.starts_with("┃ 󰚩 Operator")),
            "operator gap gone" => rows
                .iter()
                .position(|row| row.contains("CONSIGINT"))
                .is_some_and(|tree| {
                    rows.get(tree.saturating_sub(1))
                        .is_some_and(|row| !row.trim().is_empty())
                }),
            "usage compact" => rows.iter().any(|row| row.contains("Usage ·")),
            "resources compact" => rows.iter().any(|row| row.contains("Resources ·")),
            "quota compact" => {
                !rows.iter().any(|row| row.contains("QUOTA LEFT"))
                    && rows.iter().any(|row| row.contains("Claude"))
            }
            "usage hidden" => !rows
                .iter()
                .any(|row| row.contains("USAGE") || row.contains("Usage ·")),
            "operator compact" => rows
                .iter()
                .any(|row| row.contains("󰚩 Operator") && !row.trim_end().ends_with("Operator")),
            "quota pointer" => rows.iter().any(|row| row.contains("Quota: u")),
            "operator hidden" => !rows.iter().any(|row| row.contains("Operator")),
            _ => unreachable!("unknown rung"),
        };
        (6..=35)
            .rev()
            .find(|height| predicate(&at(&mut dashboard, *height)))
            .unwrap_or_else(|| panic!("{key} never happens"))
    };
    let attention_gap = rung("attention gap gone");
    let operator_gap = rung("operator gap gone");
    let usage_compact = rung("usage compact");
    let resources_compact = rung("resources compact");
    let quota_compact = rung("quota compact");
    let usage_hidden = rung("usage hidden");
    let operator_compact = rung("operator compact");
    let quota_pointer = rung("quota pointer");
    let operator_hidden = rung("operator hidden");
    for (earlier, later) in [
        (attention_gap, operator_gap),
        (operator_gap, usage_compact),
        (usage_compact, resources_compact),
        (resources_compact, quota_compact),
        (quota_compact, usage_hidden),
        (usage_hidden, operator_compact),
        (operator_compact, quota_pointer),
        (quota_pointer, operator_hidden),
    ] {
        assert!(
            earlier > later,
            "sections shrink out of order: {attention_gap} {operator_gap} {usage_compact} {resources_compact} {quota_compact} {usage_hidden} {operator_compact} {quota_pointer} {operator_hidden}"
        );
    }

    let usage_hidden_rows = at(&mut dashboard, usage_hidden);
    assert!(
        usage_hidden_rows[0].is_empty() || usage_hidden_rows[0].starts_with("󰚩 OVRCR"),
        "{usage_hidden_rows:?}"
    );
    assert!(
        !usage_hidden_rows
            .iter()
            .any(|row| row.contains("USAGE") || row.contains("Usage ·")),
        "Usage hides once the tree needs room: {usage_hidden_rows:?}"
    );
    assert!(
        usage_hidden_rows.iter().any(|row| row.contains("Operator")),
        "Operator remains expanded when Usage is hidden: {usage_hidden_rows:?}"
    );

    let operator_compact_rows = at(&mut dashboard, operator_compact);
    assert!(
        operator_compact_rows
            .iter()
            .any(|row| row.contains("󰚩 Operator") && !row.trim_end().ends_with("Operator")),
        "the card keeps its identity and state on one line: {operator_compact_rows:?}"
    );

    let minimum = at(&mut dashboard, 6);
    assert!(
        minimum[1].contains("needs input"),
        "attention persists at the minimum height: {minimum:?}"
    );
    assert!(
        !minimum
            .iter()
            .any(|row| row.contains("OPERATOR") || row.contains("Operator")),
        "Operator hides last: {minimum:?}"
    );
    assert!(
        minimum[2..5].iter().all(|row| !row.trim().is_empty()),
        "three tree rows remain when feasible: {minimum:?}"
    );
}

#[test]
fn clicking_the_operator_status_row_opens_the_single_conversation() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = operator_dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();

    let open = open_once(&mut dashboard, area, 2, 2);
    respond(&mut dashboard, open, summary());
    assert!(rendered_rows(&dashboard, 120, 40)[1].contains("Operator"));
}

#[test]
fn the_dashboard_setting_hides_the_operator_section_without_disabling_its_key() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(area);
    dashboard.install_hierarchy(fixture_hierarchy());
    assert!(
        rendered_rows(&dashboard, 120, 40)
            .iter()
            .any(|row| row.contains("Operator"))
    );

    dashboard.install_settings(ovrcr::tui::Settings {
        operator: ovrcr_protocol::OperatorSettings {
            sidebar: false,
            ..Default::default()
        },
        ..Default::default()
    });
    let rows = rendered_rows(&dashboard, 120, 40);
    assert!(!rows.iter().any(|row| row.contains("Operator")), "{rows:?}");
    assert!(
        rows[1].contains("CONSIGINT"),
        "the tree takes its rows: {rows:?}"
    );

    dashboard.key(KeyCode::Char('o'));
    assert_eq!(
        operator_requests(&dashboard.drain_outbox()),
        [&OperatorRequest::Open]
    );
}

#[test]
fn the_five_sidebar_states_show_timing_and_activity_without_a_reply_title() {
    if crate::pi_available_test::run_with_pi_available(
        "operator_dock::the_five_sidebar_states_show_timing_and_activity_without_a_reply_title",
    ) {
        return;
    }
    let area = Rect::new(0, 0, 120, 40);

    let mut off = operator_dashboard_fixture();
    off.install_area(area);
    let rows = rendered_rows(&off, 120, 40);
    if rows[2].contains("Off") {
        assert!(
            rows[3].contains("o open"),
            "Off explains how to start: {rows:?}"
        );
    } else {
        assert!(
            rows[2].contains("Not set up"),
            "missing Pi is reported honestly: {rows:?}"
        );
        assert!(
            rows[3].contains("run ovrcr operator setup"),
            "setup action is clear: {rows:?}"
        );
    }

    for failure in [
        "Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE.",
        "No Pi model selected; run ovrcr operator setup.",
        "Configured Operator model is unavailable in the dedicated Pi profile.",
    ] {
        let mut not_setup = operator_dashboard_fixture();
        not_setup.install_area(area);
        not_setup.key(KeyCode::Char('o'));
        let open = not_setup.drain_outbox().into_iter().next().expect("Open");
        not_setup.handle_server_message(ServerMessage::Response {
            request_id: open.request_id,
            response: Response::Error {
                code: ErrorCode::Internal,
                message: failure.into(),
            },
        });
        let rows = rendered_rows(&not_setup, 120, 40);
        assert!(rows[2].contains("Not set up"), "{failure}: {rows:?}");
        assert!(
            rows[3].contains("run ovrcr operator setup"),
            "{failure}: {rows:?}"
        );
    }

    let mut working = operator_dashboard_fixture();
    working.install_area(area);
    let summary = summary_with_turn(OperatorTurnState::Running, false);
    let tool = OperatorMessage {
        ordinal: 1,
        turn: OperatorTurnId(1),
        content: OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
            id: "tool-1".into(),
            name: "bash".into(),
            arguments: r#"{"command":"ovrcr terminal list"}"#.into(),
        }),
    };
    open_with_history(&mut working, area, summary, vec![tool]);
    let rows = rendered_rows(&working, 120, 40);
    assert!(
        rows[2].contains("Responding"),
        "working state is explicit: {rows:?}"
    );
    assert!(
        rows[2].contains("0s"),
        "the retained turn start provides elapsed time: {rows:?}"
    );
    assert!(
        rows[3].contains("running ovrcr terminal list"),
        "current command is shown: {rows:?}"
    );
    assert!(
        !rows[2].contains("pi") && !rows[2].contains("model"),
        "no harness or model: {rows:?}"
    );

    let mut reply_ready = operator_dashboard_fixture();
    reply_ready.install_area(area);
    let summary = summary_with_turn(OperatorTurnState::Complete, true);
    open_with_history(
        &mut reply_ready,
        area,
        summary,
        assistant_reply("The deployment finished cleanly."),
    );
    let rows = rendered_rows(&reply_ready, 120, 40);
    assert!(
        rows[2].contains("Reply ready · 1 unread"),
        "unread reply is counted: {rows:?}"
    );
    assert!(
        rows[3].contains("o open to review") && !rows[3].contains('“'),
        "the section offers review without echoing the reply as a title: {rows:?}"
    );

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    reply_ready.draw(&mut terminal).unwrap();
    reply_ready.key(KeyCode::Esc);
    let rows = rendered_rows(&reply_ready, 120, 40);
    assert!(
        rows[2].contains("Idle"),
        "a reviewed conversation becomes idle: {rows:?}"
    );
    assert!(
        rows[2].contains("last reply 0s"),
        "completed reply time is retained: {rows:?}"
    );
    assert!(
        rows[3].contains("o open · Ctrl-o quick message") && !rows[3].contains('“'),
        "idle keeps its action hint without a reply title: {rows:?}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn live_server_dashboard_draws_the_sidebar_and_opens_operator_from_its_section() {
    if crate::pi_available_test::run_with_pi_available(
        "operator_dock::live_server_dashboard_draws_the_sidebar_and_opens_operator_from_its_section",
    ) {
        return;
    }
    use ovrcr::protocol::{ClientMessage, exchange_preamble, read_frame, write_frame};
    use std::collections::BTreeSet;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    fn drive(
        connection: &mut UnixStream,
        dashboard: &mut Dashboard,
        initial: Vec<ClientMessage>,
    ) -> Vec<ServerMessage> {
        let mut awaiting = BTreeSet::new();
        for message in initial {
            assert!(awaiting.insert(message.request_id));
            write_frame(connection, &message).expect("send Dashboard request");
        }
        let mut observed = Vec::new();
        while !awaiting.is_empty() {
            let message = read_frame::<ServerMessage>(connection).expect("read Dashboard frame");
            let response_id = match &message {
                ServerMessage::Response { request_id, .. } => Some(*request_id),
                ServerMessage::Event(_) => None,
            };
            for followup in dashboard.handle_server_message(message.clone()) {
                assert!(awaiting.insert(followup.request_id));
                write_frame(connection, &followup).expect("send Dashboard follow-up");
            }
            if let Some(request_id) = response_id {
                awaiting.remove(&request_id);
            }
            observed.push(message);
        }
        observed
    }

    let live = crate::live::Live::thread().bounded();
    live.ready("feature/sidebar-operator");
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_config_dir(live.config.clone());
    dashboard.install_area(area);
    let mut connection = UnixStream::connect(&live.socket).expect("connect fixture Server");
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    connection
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    exchange_preamble(&mut connection).expect("Dashboard protocol handshake");
    let startup = drive(
        &mut connection,
        &mut dashboard,
        vec![
            ClientMessage {
                request_id: 900_001,
                request: Request::DashboardHello,
            },
            ClientMessage {
                request_id: 900_002,
                request: Request::DashboardGeometry {
                    size: ovrcr::protocol::TerminalSize {
                        rows: 40,
                        cols: 120,
                    },
                },
            },
        ],
    );
    assert!(
        startup.iter().any(|message| {
            matches!(
                message,
                ServerMessage::Event(ServerEvent::SettingsChanged(_))
            )
        }),
        "the Dashboard receives the Server's settings"
    );
    let rows = rendered_rows(&dashboard, 120, 40);
    assert!(
        rows[1].starts_with("┃ 󰚩 Operator"),
        "live Dashboard draws the section first: {rows:?}"
    );
    assert!(
        rows[2].contains("Off") || rows[2].contains("Not set up"),
        "live state follows server setup: {rows:?}"
    );
    assert!(
        rows[5].contains("FIXTURE"),
        "the real Server hierarchy follows Operator: {rows:?}"
    );

    assert_eq!(click(&mut dashboard, area, 2, 2), DashboardAction::Redraw);
    let requests = dashboard.drain_outbox();
    let opened = drive(&mut connection, &mut dashboard, requests);
    assert!(
        opened.iter().any(|message| matches!(
            message,
            ServerMessage::Response {
                response: Response::Operator(OperatorResponse::Opened(_)),
                ..
            }
        )),
        "clicking the live section opens the Server's singleton"
    );
    let rows = rendered_rows(&dashboard, 120, 40);
    assert!(
        rows[2].contains("Idle"),
        "live Open returns the idle state: {rows:?}"
    );
}

#[test]
fn the_operator_section_precedes_the_tree_and_does_not_claim_title_space() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = operator_dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();
    let rows = rendered_rows(&dashboard, 120, 40);
    assert!(
        rows[0].starts_with("󰚩 OVRCR"),
        "the title keeps its brand: {:?}",
        rows[0]
    );
    assert!(
        !rows[0].contains("Operator") && !rows[0].contains("Ctrl-o"),
        "the title bar carries no Operator shortcut: {:?}",
        rows[0]
    );
    assert!(
        rows[1].starts_with("┃ 󰚩 Operator"),
        "Operator follows the title: {:?}",
        &rows[..6]
    );
    assert!(
        rows[2].contains("Off") || rows[2].contains("Not set up"),
        "state follows heading: {:?}",
        rows[2]
    );
    assert!(
        rows[5].contains("CONSIGINT"),
        "the first project follows Operator: {:?}",
        &rows[3..7]
    );
    assert!(rows[39].starts_with("BROWSE"), "{:?}", rows[39]);
}

#[test]
fn retired_title_controls_open_neither_surface() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = operator_dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();
    let before = rendered_rows(&dashboard, 120, 40);

    // The old `Operator [o]` label and `[Ctrl-o]` control are inert.
    assert_eq!(click(&mut dashboard, area, 10, 0), DashboardAction::None);
    assert_eq!(click(&mut dashboard, area, 34, 0), DashboardAction::None);
    let out = dashboard.drain_outbox();
    assert!(
        operator_requests(&out).is_empty(),
        "a retired title control must not open Operator: {out:?}"
    );
    let after = rendered_rows(&dashboard, 120, 40);
    assert!(
        !after.iter().any(|row| row.contains("Quick message")),
        "the retired Quick control opens nothing: {after:?}"
    );
    assert!(
        !after.iter().any(|row| row.contains("Operator · pi")),
        "the retired label opens nothing: {after:?}"
    );
    assert_eq!(
        after.iter().map(|row| row.trim_end()).collect::<Vec<_>>(),
        before.iter().map(|row| row.trim_end()).collect::<Vec<_>>(),
        "the frame is unchanged"
    );

    // Browse `o` still opens the shared conversation.
    dashboard.key(KeyCode::Char('o'));
    let out = dashboard.drain_outbox();
    assert_eq!(operator_requests(&out), [&OperatorRequest::Open]);
    let request = out
        .iter()
        .find(|message| matches!(message.request, Request::Operator(OperatorRequest::Open)))
        .expect("the keyboard route sends Open");
    respond(&mut dashboard, request.request_id, summary());
    let open = rendered_rows(&dashboard, 120, 40);
    assert!(
        open.iter().any(|row| row.contains("Operator · pi")),
        "{open:?}"
    );

    // Commands still reaches Quick message after the title control retired.
    let mut commands = operator_dashboard_fixture();
    commands.install_area(area);
    palette_search(&mut commands, "Quick message");
    commands.key(KeyCode::Enter);
    let quick = rendered_rows(&commands, 120, 40);
    assert!(
        quick.iter().any(|row| row.contains("Quick message")),
        "the Commands entry still opens Quick message: {quick:?}"
    );
}

#[test]
fn both_ends_of_the_operator_section_open_the_same_conversation() {
    if crate::pi_available_test::run_with_pi_available(
        "operator_dock::both_ends_of_the_operator_section_open_the_same_conversation",
    ) {
        return;
    }
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = operator_dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();

    // Left end of the row.
    let open = open_once(&mut dashboard, area, 0, 2);
    respond(&mut dashboard, open, summary());
    dashboard.drain_outbox();
    let open_rows = rendered_rows(&dashboard, 120, 40);
    assert!(
        open_rows[2].starts_with('┃'),
        "the open conversation draws its selection marker: {:?}",
        open_rows[2]
    );
    assert!(open_rows[2].contains("Idle"), "{:?}", open_rows[2]);

    // Returning to Browse restores the terminal panes and the quiet row.
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::EnterBrowse);
    let browse_rows = rendered_rows(&dashboard, 120, 40);
    assert!(browse_rows[2].starts_with('┃'), "{:?}", browse_rows[2]);
    assert!(browse_rows[2].contains("Idle"), "{:?}", browse_rows[2]);
    assert!(
        browse_rows[5].contains("CONSIGINT"),
        "the tree is back: {:?}",
        &browse_rows[..3]
    );

    // Right end of the row opens the same singleton.
    let open = open_once(&mut dashboard, area, 38, 2);
    respond(&mut dashboard, open, summary());
    let reopened = rendered_rows(&dashboard, 120, 40);
    assert!(reopened[2].starts_with('┃'), "{:?}", reopened[2]);
    assert!(
        reopened.iter().any(|row| row.contains("Operator · pi")),
        "{reopened:?}"
    );
}

#[test]
fn representative_frames_keep_the_operator_section_before_the_tree() {
    for (cols, rows) in [(140, 40), (100, 24), (80, 24), (60, 24), (60, 16)] {
        let area = Rect::new(0, 0, cols, rows);
        let mut dashboard = operator_dashboard_fixture();
        dashboard.install_area(area);
        dashboard.drain_outbox();
        let frame = rendered_rows(&dashboard, cols, rows);
        assert!(
            frame[0].starts_with("󰚩 OVRCR") && !frame[0].contains("Operator"),
            "{cols}x{rows}: {:?}",
            frame[0]
        );
        assert!(
            frame[1].starts_with("┃ 󰚩 Operator"),
            "{cols}x{rows}: Operator follows the title: {:?}",
            &frame[..6]
        );
        assert!(
            frame[2].contains("Off") || frame[2].contains("Not set up"),
            "{cols}x{rows}: state follows title: {:?}",
            &frame[1..4]
        );
        assert!(
            frame
                .iter()
                .skip(4)
                .any(|row| sidebar_line(row).contains("CONSIGINT")),
            "{cols}x{rows}: tree follows Operator: {:?}",
            &frame[1..]
        );

        let open = open_once(&mut dashboard, area, 0, 2);
        respond(&mut dashboard, open, summary());
        let open_frame = rendered_rows(&dashboard, cols, rows);
        let header: String = if cols < 100 {
            // Narrow frames open the conversation full-width by default.
            assert!(
                open_frame[1].starts_with("Operator · pi"),
                "{cols}x{rows}: full-width conversation: {:?}",
                &open_frame[..3]
            );
            open_frame[1].chars().take(14).collect()
        } else {
            assert!(
                open_frame[1].contains("󰚩 Operator"),
                "{cols}x{rows}: the sidebar stays: {:?}",
                &open_frame[..3]
            );
            open_frame[1].chars().skip(40).take(14).collect()
        };
        assert_eq!(header, "Operator · pi ", "{cols}x{rows}");

        if cols < 100 {
            // The ordinary sidebar stays available on demand, section included.
            dashboard.ctrl('l');
            let shown = rendered_rows(&dashboard, cols, rows);
            assert!(
                shown[1].starts_with("┃ 󰚩 Operator")
                    && shown
                        .iter()
                        .skip(4)
                        .any(|row| sidebar_line(row).contains("CONSIGINT")),
                "{cols}x{rows}: Ctrl-l reveals the sidebar: {:?}",
                &shown[..7]
            );
            assert!(
                shown[1].contains("󰚩 Operator") && shown[2].starts_with('┃'),
                "{cols}x{rows}: the selected Operator returns with the sidebar: {:?}",
                &shown[1..4]
            );
        }
    }
}

#[test]
fn the_floor_keeps_the_composer_and_drafts_survive_below_it() {
    let area = Rect::new(0, 0, 60, 16);
    let mut dashboard = operator_dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();
    let open = open_once(&mut dashboard, area, 0, 2);
    respond(&mut dashboard, open, summary());
    dashboard.drain_outbox();
    for character in "unsent draft".chars() {
        dashboard.key(KeyCode::Char(character));
    }
    let floor = rendered_rows(&dashboard, 60, 16);
    assert!(
        floor.iter().any(|row| row.contains("unsent draft")),
        "the composer works at 60×16: {floor:?}"
    );

    dashboard.install_area(Rect::new(0, 0, 59, 16));
    let below = rendered_rows(&dashboard, 59, 16);
    assert!(
        below.iter().any(|row| row.contains("60×16")),
        "below the floor the resize guidance stays: {below:?}"
    );

    dashboard.install_area(area);
    let returned = rendered_rows(&dashboard, 60, 16);
    assert!(
        returned.iter().any(|row| row.contains("unsent draft")),
        "the draft survives the floor: {returned:?}"
    );
}

#[test]
fn a_hidden_or_short_sidebar_yields_the_operator_section_but_keeps_the_key() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = operator_dashboard_fixture();
    dashboard.install_area(area);
    dashboard.drain_outbox();
    assert!(rendered_rows(&dashboard, 120, 40)[1].contains("Operator"));

    // A hidden sidebar hides the Operator section too.
    dashboard.key(KeyCode::Char('b'));
    let hidden = rendered_rows(&dashboard, 120, 40);
    assert!(
        !hidden.iter().any(|row| row.contains("Operator")),
        "{hidden:?}"
    );
    assert!(!hidden[1].contains("CONSIGINT"), "{hidden:?}");
    dashboard.drain_outbox();
    dashboard.key(KeyCode::Char('o'));
    let out = dashboard.drain_outbox();
    assert_eq!(
        operator_requests(&out),
        [&OperatorRequest::Open],
        "the keyboard route works without the section: {out:?}"
    );
    let request = out
        .iter()
        .find(|message| matches!(message.request, Request::Operator(OperatorRequest::Open)))
        .expect("Open");
    respond(&mut dashboard, request.request_id, summary());
    let open = rendered_rows(&dashboard, 120, 40);
    assert!(open[1].starts_with("Operator · pi"), "{:?}", &open[..3]);
    dashboard.key(KeyCode::Esc);
    dashboard.key(KeyCode::Char('b'));

    // Six rows leave four sidebar lines: three tree lines plus compact Operator.
    dashboard.install_area(Rect::new(0, 0, 120, 6));
    let roomy = rendered_rows(&dashboard, 120, 6);
    assert!(roomy[1].contains("Operator"), "{roomy:?}");
    for row in &roomy[2..5] {
        assert!(
            !row.contains("QUOTA"),
            "the tree keeps three lines: {roomy:?}"
        );
    }

    // Five rows leave three: Operator yields them to the tree, whose three
    // drawn lines move up by one row.
    dashboard.install_area(Rect::new(0, 0, 120, 5));
    let tight = rendered_rows(&dashboard, 120, 5);
    assert!(
        !tight.iter().any(|row| row.contains("Operator")),
        "{tight:?}"
    );
    assert_eq!(
        roomy[2..5]
            .iter()
            .map(|row| sidebar_line(row))
            .collect::<Vec<_>>(),
        tight[1..4]
            .iter()
            .map(|row| sidebar_line(row))
            .collect::<Vec<_>>(),
        "the short sidebar gives the compact Operator row to the tree"
    );
    assert!(roomy[5].starts_with("BROWSE"), "{roomy:?}");
    assert!(tight[4].starts_with("BROWSE"), "{tight:?}");
    dashboard.drain_outbox();
    dashboard.key(KeyCode::Char('o'));
    let out = dashboard.drain_outbox();
    assert_eq!(
        operator_requests(&out),
        [&OperatorRequest::Open],
        "the keyboard route works in a short sidebar: {out:?}"
    );
}

#[test]
fn a_fifty_session_tree_scrolls_between_the_fixed_sidebar_sections() {
    let area = Rect::new(0, 0, 120, 40);
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects.truncate(1);
    let template = hierarchy.projects[0].workspaces[0].sessions[0].clone();
    hierarchy.projects[0].workspaces[0].sessions = (1..=50)
        .map(|id| {
            let mut session = template.clone();
            session.id = SessionId(id);
            session.run = SessionRunId(id);
            session.name = format!("session-{id}");
            session.label = "sh".into();
            session.agent = None;
            session.unread = None;
            session.activity = AgentActivity::Unknown;
            session
        })
        .collect();
    let mut waiting = hierarchy.projects[0].workspaces[0].sessions[0].clone();
    waiting.activity = AgentActivity::WaitingInput;
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(area);
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(50));
    dashboard.drain_outbox();

    let initial = rendered_rows(&dashboard, 120, 40);
    assert!(
        initial[5].contains("session-"),
        "the scrolled tree follows Operator: {:?}",
        &initial[3..8]
    );
    assert!(initial[1].contains("Operator"), "{:?}", &initial[..5]);
    let quota_row = initial
        .iter()
        .position(|row| row.contains("QUOTA LEFT"))
        .expect("quota section");

    // Scrolling the tree never moves the preceding Operator section.
    for _ in 0..3 {
        assert_eq!(
            click_wheel(&mut dashboard, area, MouseEventKind::ScrollUp, 10, 5),
            DashboardAction::Redraw
        );
    }
    let scrolled = rendered_rows(&dashboard, 120, 40);
    assert_eq!(
        scrolled[1..4]
            .iter()
            .map(|row| sidebar_line(row))
            .collect::<Vec<_>>(),
        initial[1..4]
            .iter()
            .map(|row| sidebar_line(row))
            .collect::<Vec<_>>(),
        "Operator stays fixed while the tree scrolls"
    );
    assert_ne!(
        &scrolled[5..quota_row],
        &initial[5..quota_row],
        "the tree scrolled"
    );

    // The last visible session row selects exactly the session it draws.
    let last = (1..quota_row)
        .rev()
        .find(|&row| scrolled[row].contains("session-"))
        .expect("a visible session row");
    let identity = session_id_in(&scrolled[last]).expect("the drawn session id");
    assert_eq!(
        click(&mut dashboard, area, 4, last as u16),
        DashboardAction::Redraw
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(identity)));

    // A quota update changes the section but not Operator's rows.
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::QuotaChanged(Box::new(
        quota_with_windows(),
    ))));
    dashboard.drain_outbox();
    let requoted = rendered_rows(&dashboard, 120, 40);
    assert_eq!(
        requoted[1..4]
            .iter()
            .map(|row| sidebar_line(row))
            .collect::<Vec<_>>(),
        initial[1..4]
            .iter()
            .map(|row| sidebar_line(row))
            .collect::<Vec<_>>(),
        "a quota update leaves Operator in place"
    );
    assert_ne!(
        &requoted[5..],
        &initial[5..],
        "the quota section changed shape"
    );

    // Attention moves Operator down by its line and blank row.
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
        waiting,
    ))));
    dashboard.drain_outbox();
    let attended = rendered_rows(&dashboard, 120, 40);
    assert!(
        attended[1].contains("needs input"),
        "attention remains first: {:?}",
        &attended[..5]
    );
    assert!(
        attended[3].starts_with("┃ 󰚩 Operator"),
        "attention shifts Operator: {:?}",
        &attended[..7]
    );
    assert!(
        attended[7].contains("session-"),
        "tree follows attention and Operator: {:?}",
        &attended[5..9]
    );
    let strip = attended
        .iter()
        .position(|row| row.contains("needs input") || row.contains("need input"))
        .expect("attention strip");
    assert_eq!(
        click(&mut dashboard, area, 8, strip as u16),
        DashboardAction::Redraw
    );
    assert_eq!(
        dashboard.focused_session(),
        Some(SessionId(1)),
        "the strip jumps to the session it counts"
    );

    // The quota section opens its details at its drawn row.
    let quota = attended
        .iter()
        .position(|row| row.contains("QUOTA LEFT"))
        .expect("quota section");
    assert_eq!(
        click(&mut dashboard, area, 2, quota as u16),
        DashboardAction::Redraw
    );
    let details = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(details.contains("Quota details"), "{details}");
    dashboard.key(KeyCode::Esc);

    // The sidebar divider remains draggable, and Operator keeps its top rows.
    for (kind, column) in [
        (MouseEventKind::Down(MouseButton::Left), 39),
        (MouseEventKind::Drag(MouseButton::Left), 19),
        (MouseEventKind::Up(MouseButton::Left), 19),
    ] {
        assert_eq!(
            dashboard.mouse_action(mouse_event(kind, column, 38, KeyModifiers::NONE), area),
            DashboardAction::Redraw
        );
    }
    assert_eq!(dashboard.pane_rects(area)[0].terminal.x, 20);
    let resized = rendered_rows(&dashboard, 120, 40);
    assert!(
        resized[2].contains("Operator"),
        "Operator stays at the top at the minimum sidebar width: {:?}",
        &resized[1..7]
    );
    let open = open_once(&mut dashboard, area, 18, 3);
    respond(&mut dashboard, open, summary());
}

fn click_wheel(
    dashboard: &mut Dashboard,
    area: Rect,
    kind: MouseEventKind,
    column: u16,
    row: u16,
) -> DashboardAction {
    dashboard.mouse_action(mouse_event(kind, column, row, KeyModifiers::NONE), area)
}

fn jump_hierarchy(count: u64) -> HierarchySnapshot {
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects.truncate(1);
    hierarchy.projects[0].workspaces.truncate(1);
    let project = hierarchy.projects[0].name.clone();
    let workspace = hierarchy.projects[0].workspaces[0].name.clone();
    hierarchy.projects[0].workspaces[0].sessions = (1..=count)
        .map(|id| {
            let mut session = session_summary(
                id,
                &project,
                &workspace,
                &format!("agent-{id}"),
                "fixture-agent",
                Some(u32::try_from(id).unwrap()),
                0,
            );
            session.kind = SessionKind::Agent {
                name: "fixture-agent".into(),
            };
            session.phase = SessionPhase::Running;
            session.run = SessionRunId(id);
            session
        })
        .collect();
    hierarchy
}

#[test]
fn jump_labels_stay_in_the_tree_and_never_mark_operator() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(area);
    dashboard.install_hierarchy(jump_hierarchy(2));
    dashboard.install_focus(SessionId(2));
    dashboard.install_screen(SessionId(2), &[]);
    dashboard.drain_outbox();
    dashboard.ctrl('g');
    let rows = rendered_rows(&dashboard, 120, 40);
    assert!(
        rows.iter().any(|row| row.contains("[1]")),
        "Agent sessions keep their labels: {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("[2]")),
        "Agent sessions keep their labels: {rows:?}"
    );
    assert!(
        !rows[1..4].iter().any(|row| row.contains('[')),
        "Operator is outside the numbered targets: {:?}",
        &rows[1..4]
    );
    assert!(rows[1].contains("Operator"), "{:?}", rows[1]);

    // A number still addresses the Agent session it marks.
    dashboard.key(KeyCode::Char('1'));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
}

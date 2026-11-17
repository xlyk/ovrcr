//! The Operator is one pi conversation. These cases drive the production
//! Dashboard entry points: the fixed navigation row, the full view, Quick
//! message, Stop and history paging. There is no create form, chat list,
//! switching or archive action to exercise.
use super::*;
use ovrcr_protocol::{
    OperatorContext, OperatorConversationId, OperatorConversationSummary, OperatorHistory,
    OperatorMessage, OperatorMessageContent, OperatorModel, OperatorRequest, OperatorResponse,
    OperatorToolStatus, OperatorTurn, OperatorTurnId, OperatorTurnIdentity, OperatorTurnState,
    OperatorWorkerEvent, SessionKind, SessionRunId,
};

const BUSY_REASON: &str = "The Operator is busy or its worker stop is unconfirmed; draft retained.";

fn summary() -> OperatorConversationSummary {
    OperatorConversationSummary {
        reasoning: None,
        run: SessionRunId(1),
        worker_stopped: true,
        worker_live: false,
        id: OperatorConversationId(7),
        session: SessionId(99),
        session_epoch: 0,
        model: None,
        title: "Operator".into(),
        current_turn: None,
        current_turn_started_unix_ms: None,
        last_reply_unix_ms: None,
    }
}

#[test]
fn explicit_skill_send_keeps_the_captured_target_in_both_entry_points() {
    for quick in [false, true] {
        let mut dashboard = dashboard_fixture();
        let hierarchy = fixture_hierarchy();
        let target = captured_context(&hierarchy, SessionId(1));
        dashboard.install_hierarchy(hierarchy);
        dashboard.install_focus(SessionId(1));
        open_and_settle(&mut dashboard, summary());
        if quick {
            dashboard.ctrl('o');
        }
        dashboard.event_action(Event::Paste(
            "/skill:ovrcr-status inspect captured target".into(),
        ));
        dashboard.key(KeyCode::Enter);
        // A later pane selection cannot rewrite the already accepted Send's outbox payload.
        dashboard.install_focus(SessionId(3));
        let out = dashboard.drain_outbox();
        assert_eq!(
            operator_requests(&out),
            [&OperatorRequest::Submit {
                conversation: OperatorConversationId(7),
                expected_run: SessionRunId(1),
                text: "inspect captured target".into(),
                context: target,
                skill: Some(ovrcr_protocol::OperatorSkillSelection::named("ovrcr-status").unwrap()),
            }]
        );
        assert!(!out.iter().any(|message| matches!(
            message.request,
            Request::Input { .. } | Request::AssignedHumanInput { .. }
        )));
    }
}

#[test]
fn skills_picker_preserves_shared_request_and_target_until_send() {
    for quick in [false, true] {
        let mut d = dashboard_fixture();
        let hierarchy = fixture_hierarchy();
        let target = captured_context(&hierarchy, SessionId(1));
        d.install_hierarchy(hierarchy);
        d.install_focus(SessionId(1));
        open_and_settle(&mut d, summary());
        if quick {
            d.key(KeyCode::Esc);
            d.ctrl('o');
        }
        d.event_action(Event::Paste("inspect captured target".into()));
        d.key(KeyCode::Home);
        d.event_action(Event::Paste("/skills ".into()));
        d.key(KeyCode::End);
        if quick {
            // An underlying selection change must not recapture this draft.
            d.install_focus(SessionId(3));
        }
        d.key(KeyCode::Enter);
        // Quick may open the full conversation, but never starts a worker.
        let out = d.drain_outbox();
        for message in &out {
            if matches!(message.request, Request::Operator(OperatorRequest::Open)) {
                let follow = respond(
                    &mut d,
                    message.request_id,
                    OperatorResponse::Opened(summary()),
                );
                for read in follow {
                    if matches!(
                        read.request,
                        Request::Operator(OperatorRequest::Read { .. })
                    ) {
                        respond(
                            &mut d,
                            read.request_id,
                            history_of(summary(), Vec::new(), None),
                        );
                    }
                }
            }
        }
        assert!(out.iter().all(|m| !matches!(
            m.request,
            Request::Operator(OperatorRequest::Submit { .. })
                | Request::Input { .. }
                | Request::AssignedHumanInput { .. }
        )));
        let screen = draw_text(&mut d, 140, 35);
        assert!(screen.contains("Operator skills"), "{screen}");
        assert!(
            screen.contains("ovrcr-status") && screen.contains("ovrcr-coordinator"),
            "{screen}"
        );
        d.event_action(Event::Paste("INVENTORY".into()));
        let filtered = draw_text(&mut d, 140, 35);
        assert!(
            filtered.contains("ovrcr-status") && !filtered.contains("ovrcr-coordinator"),
            "{filtered}"
        );
        d.key(KeyCode::Esc);
        let cancelled = draw_text(&mut d, 140, 35);
        assert!(
            cancelled.contains("/skills inspect captured target"),
            "{cancelled}"
        );
        d.key(KeyCode::Enter);
        d.event_action(Event::Paste("ovrcr-status".into()));
        d.key(KeyCode::Enter);
        assert!(d.drain_outbox().iter().all(|m| !matches!(
            m.request,
            Request::Operator(OperatorRequest::Submit { .. }) | Request::Input { .. }
        )));
        assert!(draw_text(&mut d, 140, 35).contains("/skill:ovrcr-status inspect captured target"));
        d.key(KeyCode::Enter);
        assert_eq!(
            operator_requests(&d.drain_outbox()),
            [&OperatorRequest::Submit {
                conversation: OperatorConversationId(7),
                expected_run: SessionRunId(1),
                text: "inspect captured target".into(),
                context: target,
                skill: Some(ovrcr_protocol::OperatorSkillSelection::named("ovrcr-status").unwrap()),
            }]
        );
    }
}

#[test]
fn skills_picker_slash_menu_prepares_a_bare_invocation() {
    let mut d = dashboard_fixture();
    open_and_settle(&mut d, summary());
    d.event_action(Event::Paste("/sk".into()));
    assert!(draw_text(&mut d, 140, 35).contains("Browse shipped skills"));
    d.key(KeyCode::Enter);
    d.event_action(Event::Paste("ovrcr-status".into()));
    d.key(KeyCode::Enter);
    assert!(d.drain_outbox().is_empty());
    d.key(KeyCode::Enter);
    assert!(matches!(operator_requests(&d.drain_outbox()).as_slice(),
        [OperatorRequest::Submit { text, skill: Some(skill), .. }] if text.is_empty() && skill.name == "ovrcr-status"));
}

#[test]
fn skills_picker_cold_quick_mouse_cancel_and_busy_layouts() {
    for value in [summary(), busy(), stop_unconfirmed()] {
        let mut d = dashboard_fixture();
        d.install_hierarchy(fixture_hierarchy());
        d.install_focus(SessionId(1));
        d.ctrl('o');
        d.event_action(Event::Paste("/skills keep this request".into()));
        d.key(KeyCode::Enter);
        let out = d.drain_outbox();
        assert_eq!(operator_requests(&out), [&OperatorRequest::Open]);
        let follow = respond(
            &mut d,
            out.iter()
                .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Open)))
                .unwrap()
                .request_id,
            OperatorResponse::Opened(value.clone()),
        );
        let read = read_request(&follow);
        respond(
            &mut d,
            read.request_id,
            history_of(value.clone(), Vec::new(), None),
        );
        for (cols, rows) in [(80, 24), (60, 24), (60, 16), (25, 8), (1, 1)] {
            let screen = draw_text(&mut d, cols, rows);
            if cols >= 60 {
                assert!(
                    screen.contains("Operator skills") && screen.contains("ovrcr-status"),
                    "{cols}x{rows}: {screen}"
                );
                assert!(
                    screen.contains("prepare") && screen.contains("close"),
                    "{screen}"
                );
            }
            assert!(d.drain_outbox().is_empty());
        }
        draw_text(&mut d, 80, 24);
        // Outside dismissal is consumed, leaving the draft, not terminal input.
        assert!(!matches!(
            d.event_action(Event::Mouse(mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                0,
                0,
                KeyModifiers::NONE
            ))),
            DashboardAction::PtyBytes(_)
        ));
        assert!(draw_text(&mut d, 140, 35).contains("/skills keep this request"));
        d.key(KeyCode::Enter);
        d.event_action(Event::Paste("NOT-SHIPPED".into()));
        assert!(draw_text(&mut d, 80, 24).contains("No shipped skills match"));
        d.key(KeyCode::Enter);
        assert!(d.drain_outbox().is_empty());
        d.ctrl('c');
        d.event_action(Event::Paste("ovrcr-status".into()));
        let lines = draw_text(&mut d, 80, 24);
        let row = lines
            .lines()
            .position(|line| line.contains("ovrcr-status") && !line.contains("Filter:"))
            .unwrap() as u16;
        assert!(!matches!(
            d.event_action(Event::Mouse(mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                5,
                row,
                KeyModifiers::NONE
            ))),
            DashboardAction::PtyBytes(_)
        ));
        assert!(d.drain_outbox().is_empty());
        assert!(draw_text(&mut d, 140, 35).contains("/skill:ovrcr-status keep this request"));
        d.key(KeyCode::Enter);
        let out = d.drain_outbox();
        if !value.worker_stopped {
            assert!(out.is_empty());
            let screen = draw_text(&mut d, 140, 35);
            assert!(
                screen.contains(BUSY_REASON)
                    && screen.contains("/skill:ovrcr-status keep this request"),
                "{screen}"
            );
        } else {
            let submit = out
                .iter()
                .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Submit { .. })))
                .unwrap();
            d.handle_server_message(ServerMessage::Response {
                request_id: submit.request_id,
                response: Response::Error {
                    code: ovrcr_protocol::ErrorCode::InvalidRequest,
                    message: "Operator skill is unavailable or incompatible; select it again."
                        .into(),
                },
            });
            let screen = draw_text(&mut d, 140, 35);
            assert!(
                screen.contains("unavailable or incompatible")
                    && screen.contains("/skill:ovrcr-status keep this request"),
                "{screen}"
            );
        }
    }
}

fn with_model(mut value: OperatorConversationSummary) -> OperatorConversationSummary {
    value.model = Some(OperatorModel {
        provider: "openai".into(),
        id: "gpt-6.1-sol".into(),
    });
    value
}

fn turn(run: u64, state: OperatorTurnState) -> OperatorTurn {
    OperatorTurn {
        identity: OperatorTurnIdentity {
            conversation: OperatorConversationId(7),
            turn: OperatorTurnId(run),
            session: SessionId(99),
            run: SessionRunId(run),
        },
        state,
        sequence: 1,
    }
}

fn busy() -> OperatorConversationSummary {
    let mut value = summary();
    value.run = SessionRunId(2);
    value.worker_stopped = false;
    value.current_turn = Some(turn(2, OperatorTurnState::Running));
    value
}

fn stop_unconfirmed() -> OperatorConversationSummary {
    let mut value = summary();
    value.run = SessionRunId(2);
    value.worker_stopped = false;
    value.current_turn = Some(turn(2, OperatorTurnState::Complete));
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

/// Browse `o`: the full view opens the singleton directly.
fn open_request(dashboard: &mut Dashboard) -> ClientMessage {
    dashboard.key(KeyCode::Char('o'));
    dashboard
        .drain_outbox()
        .into_iter()
        .find(|message| matches!(message.request, Request::Operator(OperatorRequest::Open)))
        .expect("the full view sends Open, never a create or list request")
}

fn respond(
    dashboard: &mut Dashboard,
    request_id: u64,
    response: OperatorResponse,
) -> Vec<ClientMessage> {
    dashboard.handle_server_message(ServerMessage::Response {
        request_id,
        response: Response::Operator(response),
    })
}

fn opened(dashboard: &mut Dashboard, value: OperatorConversationSummary) -> Vec<ClientMessage> {
    let request = open_request(dashboard);
    respond(
        dashboard,
        request.request_id,
        OperatorResponse::Opened(value),
    )
}

fn read_request(messages: &[ClientMessage]) -> ClientMessage {
    messages
        .iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Read { .. })
            )
        })
        .cloned()
        .expect("a dirty journal is read")
}

fn history_of(
    conversation: OperatorConversationSummary,
    messages: Vec<OperatorMessage>,
    next_after: Option<u64>,
) -> OperatorResponse {
    OperatorResponse::History(OperatorHistory {
        conversation,
        messages,
        next_after,
    })
}

fn message(ordinal: u64, content: OperatorMessageContent) -> OperatorMessage {
    OperatorMessage {
        ordinal,
        turn: OperatorTurnId(1),
        content,
    }
}

fn user(ordinal: u64, text: &str, context: OperatorContext) -> OperatorMessage {
    message(
        ordinal,
        OperatorMessageContent::User {
            skill: None,
            text: text.into(),
            context,
        },
    )
}

fn delta(ordinal: u64, text: &str) -> OperatorMessage {
    message(
        ordinal,
        OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text.into())),
    )
}

fn completed(ordinal: u64) -> OperatorMessage {
    message(
        ordinal,
        OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
            leased: false,
            pi_first_entry_id: None,
            pi_leaf_entry_id: None,
        }),
    )
}

/// Opens the singleton and answers its first read with an empty journal.
fn open_and_settle(dashboard: &mut Dashboard, value: OperatorConversationSummary) {
    let out = opened(dashboard, value.clone());
    let read = read_request(&out);
    let out = respond(
        dashboard,
        read.request_id,
        history_of(value, Vec::new(), None),
    );
    assert!(out.is_empty(), "an empty tail requests nothing: {out:?}");
}

fn draw_text(dashboard: &mut Dashboard, cols: u16, rows: u16) -> String {
    dashboard.install_area(Rect::new(0, 0, cols, rows));
    rendered_rows(dashboard, cols, rows).join("\n")
}

/// The production draw path, which commits a presented durable tail as reviewed.
fn reviewed_screen(dashboard: &mut Dashboard, cols: u16, rows: u16) -> String {
    dashboard.install_area(Rect::new(0, 0, cols, rows));
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
    dashboard.draw(&mut terminal).unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| buffer[(col, row)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn mouse(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn reasoning_summary(running: bool) -> OperatorConversationSummary {
    use ovrcr_protocol::{OperatorReasoning, OperatorThinkingLevel as Level};
    let mut value = with_model(if running { busy() } else { summary() });
    value.reasoning = Some(OperatorReasoning {
        requested: Level::High,
        level: Level::High,
        available: vec![Level::Minimal, Level::Low, Level::Medium, Level::High],
    });
    value
}

#[test]
fn operator_reasoning_header_and_picker_use_pi_capabilities_and_server_setting_writes() {
    use ovrcr_protocol::OperatorThinkingLevel as Level;
    let mut dashboard = dashboard_fixture();
    dashboard.install_settings(ovrcr::tui::Settings {
        operator: ovrcr_protocol::OperatorSettings {
            thinking_level: Level::High,
            ..Default::default()
        },
        ..Default::default()
    });
    open_and_settle(&mut dashboard, reasoning_summary(false));
    let screen = draw_text(&mut dashboard, 160, 40);
    assert!(screen.contains("[Reasoning high ▾ Ctrl-t]"), "{screen}");
    dashboard.ctrl('t');
    let screen = draw_text(&mut dashboard, 160, 40);
    assert!(screen.contains("default (Pi's choice)"), "{screen}");
    assert!(screen.contains("✓ high"), "{screen}");
    assert!(screen.contains("not offered: off, xhigh, max"), "{screen}");
    dashboard.key(KeyCode::Up);
    dashboard.key(KeyCode::Enter);
    let out = dashboard.drain_outbox();
    assert!(
        out.iter().any(|message| matches!(&message.request,
        Request::SetSetting { path, value: Some(value) }
        if path == "operator.thinking_level" && value == "\"medium\"")),
        "{out:?}"
    );
    assert!(!draw_text(&mut dashboard, 160, 40).contains("Enter pick"));

    // Both entry points use the drawn control's geometry.
    let rows = rendered_rows(&dashboard, 160, 40);
    let row = rows
        .iter()
        .position(|row| row.contains("[Reasoning"))
        .unwrap();
    let column = rows[row].chars().position(|ch| ch == '[').unwrap();
    dashboard.mouse_action(
        mouse(column as u16 + 2, row as u16),
        Rect::new(0, 0, 160, 40),
    );
    assert!(draw_text(&mut dashboard, 160, 40).contains("Enter pick"));
    dashboard.key(KeyCode::Esc);
    assert!(!draw_text(&mut dashboard, 160, 40).contains("Enter pick"));
    assert!(draw_text(&mut dashboard, 160, 40).contains("Reasoning high"));
}

#[test]
fn operator_reasoning_substitution_and_next_turn_are_distinct_at_narrow_sizes() {
    use ovrcr_protocol::OperatorThinkingLevel as Level;
    for (cols, rows) in [(160, 40), (80, 24), (60, 24)] {
        let mut dashboard = dashboard_fixture();
        dashboard.install_settings(ovrcr::tui::Settings {
            operator: ovrcr_protocol::OperatorSettings {
                thinking_level: Level::Xhigh,
                ..Default::default()
            },
            ..Default::default()
        });
        let mut value = reasoning_summary(false);
        value.reasoning.as_mut().unwrap().requested = Level::Xhigh;
        open_and_settle(&mut dashboard, value);
        let screen = draw_text(&mut dashboard, cols, rows);
        assert!(
            screen.contains("Reasoning high (set: xhigh)"),
            "{cols}: {screen}"
        );
        assert!(screen.contains("Ctrl-t]"), "{cols}: {screen}");
        dashboard.ctrl('t');
        let screen = draw_text(&mut dashboard, cols, rows);
        assert!(
            screen.contains("not offered: off, xhigh, max"),
            "{cols}: {screen}"
        );
    }
    let mut dashboard = dashboard_fixture();
    dashboard.install_settings(ovrcr::tui::Settings {
        operator: ovrcr_protocol::OperatorSettings {
            thinking_level: Level::High,
            ..Default::default()
        },
        ..Default::default()
    });
    open_and_settle(&mut dashboard, reasoning_summary(true));
    dashboard.ctrl('t');
    dashboard.key(KeyCode::Up);
    dashboard.key(KeyCode::Enter);
    let request = dashboard
        .drain_outbox()
        .into_iter()
        .find(|message| matches!(message.request, Request::SetSetting { .. }))
        .unwrap();
    // A refused write must not change the displayed saved value.
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::Error {
            code: ovrcr_protocol::ErrorCode::Conflict,
            message: "write refused".into(),
        },
    });
    let screen = draw_text(&mut dashboard, 160, 40);
    assert!(screen.contains("write refused"), "{screen}");
    assert!(!screen.contains("next turn"), "{screen}");
    let mut settings = ovrcr::tui::Settings::default();
    settings.operator.thinking_level = Level::Medium;
    dashboard.install_settings(settings);
    let screen = draw_text(&mut dashboard, 160, 40);
    assert!(
        screen.contains("Reasoning high → medium next turn"),
        "{screen}"
    );
}

#[test]
fn operator_reasoning_pending_default_is_distinct_from_applied_default() {
    let mut dashboard = dashboard_fixture();
    open_and_settle(&mut dashboard, reasoning_summary(false));
    let screen = draw_text(&mut dashboard, 160, 40);
    assert!(screen.contains("Reasoning high (set: default)"), "{screen}");
    let mut value = reasoning_summary(false);
    value.reasoning.as_mut().unwrap().requested = ovrcr_protocol::OperatorThinkingLevel::Default;
    let mut dashboard = dashboard_fixture();
    open_and_settle(&mut dashboard, value);
    let screen = draw_text(&mut dashboard, 160, 40);
    assert!(screen.contains("[Reasoning high ▾ Ctrl-t]"), "{screen}");
}

/// Sidebar rows belonging to the Operator section, excluding the shared pane.
fn dock_rows(rows: &[String]) -> Vec<String> {
    let left: Vec<_> = rows
        .iter()
        .map(|row| row.split('│').next().unwrap_or(row).to_string())
        .collect();
    let start = left
        .iter()
        .position(|row| row.contains("Operator"))
        .expect("the Operator section is drawn");
    let has_status = left.get(start + 1).is_some_and(|row| {
        [
            "Off",
            "Idle",
            "Responding",
            "Stopping",
            "Sending",
            "Reply ready",
            "Not set up",
        ]
        .iter()
        .any(|word| row.contains(word))
    });
    let height = if has_status { 2 } else { 1 };
    left.into_iter().skip(start).take(height).collect()
}

fn dock_row(rows: &[String]) -> String {
    dock_rows(rows).join("\n")
}

fn dock_status_row(rows: &[String]) -> String {
    let rows = dock_rows(rows);
    rows.get(1).cloned().unwrap_or_else(|| rows[0].clone())
}

#[test]
fn closed_idle_dock_offers_open_chat_and_open_idle_omits_it() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::closed_idle_dock_offers_open_chat_and_open_idle_omits_it",
    ) {
        return;
    }
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let rows = rendered_rows(&dashboard, 120, 40);
    let dock = dock_row(&rows);
    assert!(
        dock.contains("Operator") && dock.contains("Off"),
        "an unopened conversation has a section and Off state: {dock:?}"
    );
    assert!(
        !dock.contains("Ctrl-o") && !dock.contains("o open"),
        "Off carries no keyboard shortcut copy: {dock:?}"
    );
    assert!(
        dashboard.drain_outbox().is_empty(),
        "drawing an unopened dock starts no worker or request"
    );

    // Opening focuses the full conversation and selects its status/detail rows.
    open_and_settle(&mut dashboard, summary());
    let rows = rendered_rows(&dashboard, 120, 40);
    let dock = dock_status_row(&rows);
    assert!(
        dock.starts_with('┃'),
        "an open conversation keeps the Operator card: {dock:?}"
    );
    assert!(
        dock.contains("Idle"),
        "an open conversation is idle: {dock:?}"
    );
    assert!(!dock.contains("ready"), "{dock:?}");
    assert!(
        dashboard.drain_outbox().is_empty(),
        "drawing an idle conversation starts no worker or request"
    );
}

/// Deliver a durable summary change and answer its follow-up read in one step.
fn changed(
    dashboard: &mut Dashboard,
    value: OperatorConversationSummary,
    messages: Vec<OperatorMessage>,
) -> Vec<ClientMessage> {
    let dirty = dashboard
        .handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(value.id)));
    let read = read_request(&dirty);
    respond(
        dashboard,
        read.request_id,
        history_of(value, messages, None),
    )
}

fn turn_message(turn: u64, ordinal: u64, content: OperatorMessageContent) -> OperatorMessage {
    OperatorMessage {
        ordinal,
        turn: OperatorTurnId(turn),
        content,
    }
}

/// Drag the drawn sidebar divider to `column` through the production mouse path.
fn drag_sidebar_to(dashboard: &mut Dashboard, area: Rect, column: u16) {
    dashboard.mouse_action(mouse(39, 10), area);
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row: 10,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    dashboard.drain_outbox();
}

#[test]
fn sidebar_stays_off_while_opening_and_recognizes_known_setup_failures() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::sidebar_stays_off_while_opening_and_recognizes_known_setup_failures",
    ) {
        return;
    }
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let open = open_request(&mut dashboard);
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(
        dock.contains("Idle"),
        "a queued Open is visible as Idle before there is a worker: {dock:?}"
    );
    assert!(!dock.contains("Responding"), "{dock:?}");
    assert!(
        dashboard.drain_outbox().is_empty(),
        "drawing an opening dock starts no retry"
    );

    dashboard.handle_server_message(ServerMessage::Response {
        request_id: open.request_id,
        response: Response::Error {
            code: ErrorCode::InvalidRequest,
            message: "Pi is unavailable; install pi or set OVRCR_PI_EXECUTABLE.".into(),
        },
    });
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Not set up"), "{dock:?}");
    assert!(dock.contains("run ovrcr operator setup"), "{dock:?}");

    // Reopening leaves the failed view first, then retries and returns to the
    // actionable idle row.
    dashboard.key(KeyCode::Esc);
    let open = open_request(&mut dashboard);
    let out = respond(
        &mut dashboard,
        open.request_id,
        OperatorResponse::Opened(summary()),
    );
    respond(
        &mut dashboard,
        read_request(&out).request_id,
        history_of(summary(), Vec::new(), None),
    );
    let dock = dock_status_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.starts_with('┃'), "{dock:?}");
    assert!(
        dock.contains("Idle"),
        "successful open returns to Idle: {dock:?}"
    );
}

#[test]
fn dock_distinguishes_sending_tools_stopping_and_turn_outcomes() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::dock_distinguishes_sending_tools_stopping_and_turn_outcomes",
    ) {
        return;
    }
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    open_and_settle(&mut dashboard, summary());
    let context = captured_context(&fixture_hierarchy(), SessionId(1));

    // Sending: the draft is submitted but the Server has not accepted it.
    dashboard.event_action(Event::Paste("inspect the tree".into()));
    dashboard.key(KeyCode::Enter);
    let out = dashboard.drain_outbox();
    let submit = out
        .iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Submit { .. })
            )
        })
        .cloned()
        .expect("Enter sends the draft");
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Sending"), "{dock:?}");

    // Accepted and loaded: the turn is responding rather than idle.
    let out = respond(
        &mut dashboard,
        submit.request_id,
        OperatorResponse::Submitted(turn(2, OperatorTurnState::Running)),
    );
    let read = read_request(&out);
    let mut running = summary();
    running.run = SessionRunId(2);
    running.worker_stopped = false;
    running.current_turn = Some(turn(2, OperatorTurnState::Running));
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(
            running.clone(),
            vec![turn_message(
                2,
                1,
                OperatorMessageContent::User {
                    skill: None,
                    text: "inspect the tree".into(),
                    context: context.clone(),
                },
            )],
            None,
        ),
    );
    assert!(out.is_empty(), "{out:?}");
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Responding"), "{dock:?}");
    assert!(!dock.contains("1 unread"), "{dock:?}");
    assert!(
        dashboard.drain_outbox().is_empty(),
        "drawing an active conversation starts no request"
    );

    // The two-row section keeps the state visible without a shortcut line.
    let out = changed(
        &mut dashboard,
        running.clone(),
        vec![turn_message(
            2,
            2,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                id: "call-1".into(),
                name: "bash".into(),
                arguments: "{}".into(),
            }),
        )],
    );
    assert!(out.is_empty(), "{out:?}");
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Responding"), "{dock:?}");
    let out = changed(
        &mut dashboard,
        running.clone(),
        vec![turn_message(
            2,
            3,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                id: "call-1".into(),
                output: "ok".into(),
                truncated: false,
                status: OperatorToolStatus::Succeeded,
            }),
        )],
    );
    assert!(out.is_empty(), "{out:?}");
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Responding"), "{dock:?}");

    // Stopping is its own word while the Stop request is in flight.
    dashboard.ctrl('x');
    let out = dashboard.drain_outbox();
    let stop = out
        .iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Stop { .. })
            )
        })
        .cloned()
        .expect("Ctrl-x sends Stop");
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Stopping"), "{dock:?}");
    assert!(!dock.contains("Sending"), "{dock:?}");

    // The section returns to its compact Idle state after the turn ends; the
    // full conversation retains exceptional turn details.
    let mut unconfirmed = summary();
    unconfirmed.run = SessionRunId(2);
    unconfirmed.worker_stopped = false;
    unconfirmed.current_turn = Some(turn(2, OperatorTurnState::Complete));
    let out = respond(
        &mut dashboard,
        stop.request_id,
        OperatorResponse::Stopped(turn(2, OperatorTurnState::Interrupted)),
    );
    let read = read_request(&out);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(unconfirmed, Vec::new(), None),
    );
    let _ = reviewed_screen(&mut dashboard, 120, 40);
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(
        dock.contains("Responding"),
        "an unconfirmed worker remains Working: {dock:?}"
    );

    // A failure stays identifiable while the worker's stop is unconfirmed.
    let mut failed = summary();
    failed.run = SessionRunId(2);
    failed.worker_stopped = false;
    failed.current_turn = Some(turn(2, OperatorTurnState::Error));
    changed(
        &mut dashboard,
        failed,
        vec![turn_message(
            2,
            4,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Failed("provider refused".into())),
        )],
    );
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(
        dock.contains("Responding"),
        "an unconfirmed worker remains Working: {dock:?}"
    );
    assert!(draw_text(&mut dashboard, 120, 40).contains("provider refused"));

    // An interruption remains identifiable as well.
    let mut interrupted = summary();
    interrupted.run = SessionRunId(2);
    interrupted.worker_stopped = true;
    interrupted.current_turn = Some(turn(2, OperatorTurnState::Interrupted));
    changed(
        &mut dashboard,
        interrupted,
        vec![turn_message(2, 5, OperatorMessageContent::Interrupted)],
    );
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Idle"), "{dock:?}");

    // With the worker confirmed stopped, a failure names the failed turn.
    let mut stopped_failure = summary();
    stopped_failure.run = SessionRunId(2);
    stopped_failure.worker_stopped = true;
    stopped_failure.current_turn = Some(turn(2, OperatorTurnState::Error));
    changed(&mut dashboard, stopped_failure, Vec::new());
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Idle"), "{dock:?}");

    // A reviewed, successful turn returns to the idle action.
    let mut complete = summary();
    complete.run = SessionRunId(2);
    complete.worker_stopped = true;
    complete.current_turn = Some(turn(2, OperatorTurnState::Complete));
    changed(
        &mut dashboard,
        complete,
        vec![turn_message(
            2,
            6,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                leased: false,
                pi_first_entry_id: None,
                pi_leaf_entry_id: None,
            }),
        )],
    );
    let _ = reviewed_screen(&mut dashboard, 120, 40);
    dashboard.key(KeyCode::Esc);
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Idle"), "{dock:?}");
    assert!(!dock.contains("complete"), "{dock:?}");
}

#[test]
fn a_retained_draft_reopens_with_the_new_target_and_keeps_the_journal() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::a_retained_draft_reopens_with_the_new_target_and_keeps_the_journal",
    ) {
        return;
    }
    let target = captured_context(&fixture_hierarchy(), SessionId(1));
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let out = opened(&mut dashboard, summary());
    let read = read_request(&out);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(summary(), vec![user(1, "earlier request", target)], None),
    );
    dashboard.event_action(Event::Paste("unsent draft".into()));

    // Leaving keeps the draft in the conversation; the section stays focused
    // on the latest reply/status instead of exposing composer contents.
    dashboard.key(KeyCode::Esc);
    let rows = rendered_rows(&dashboard, 120, 40);
    let dock = dock_row(&rows);
    assert!(dock.contains("Idle"), "{dock:?}");
    assert!(!dock.contains("unsent draft"), "{dock:?}");
    assert!(!dock.contains("ready"), "{dock:?}");

    // Selecting another session by clicking its drawn row updates the target.
    let row = rows
        .iter()
        .position(|row| row.contains("implement"))
        .expect("the second session row is drawn") as u16;
    dashboard.mouse_action(mouse(5, row), Rect::new(0, 0, 120, 40));
    assert_eq!(dashboard.focused_session(), Some(SessionId(2)));
    dashboard.drain_outbox();

    // Reopening is idempotent, focuses the composer, and captures the new target.
    assert_eq!(dashboard.key(KeyCode::Char('o')), DashboardAction::Redraw);
    dashboard.drain_outbox();
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("Managing consigint / lifecycle / #2 run 1"),
        "{screen}"
    );
    assert!(screen.contains("unsent draft"), "{screen}");
    assert!(screen.contains("▏"), "the composer is focused: {screen}");
    // The journal keeps the earlier message and its original captured context.
    assert!(screen.contains("earlier request"), "{screen}");
    assert!(screen.contains("→ consigint / auth / #1 run 1"), "{screen}");
}

#[test]
fn dock_draws_unread_until_the_loaded_tail_is_reviewed() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::dock_draws_unread_until_the_loaded_tail_is_reviewed",
    ) {
        return;
    }
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    open_and_settle(&mut dashboard, summary());
    dashboard.key(KeyCode::Esc);

    // The completed turn arrives while Browse is showing.
    let mut complete = summary();
    complete.run = SessionRunId(2);
    complete.worker_stopped = true;
    complete.current_turn = Some(turn(2, OperatorTurnState::Complete));
    let out = changed(
        &mut dashboard,
        complete,
        vec![
            turn_message(
                2,
                1,
                OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                    "answer".into(),
                )),
            ),
            turn_message(
                2,
                2,
                OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                    leased: false,
                    pi_first_entry_id: None,
                    pi_leaf_entry_id: None,
                }),
            ),
        ],
    );
    assert!(out.is_empty(), "{out:?}");

    // Drawing Browse, including the dock, never acknowledges the tail.
    for _ in 0..2 {
        let _ = reviewed_screen(&mut dashboard, 120, 40);
        let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
        assert!(dock.contains("1 unread"), "{dock:?}");
    }

    // Opening draws the loaded tail; only that review clears unread.
    dashboard.key(KeyCode::Char('o'));
    let screen = reviewed_screen(&mut dashboard, 120, 40);
    assert!(screen.contains("answer"), "{screen}");
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(!dock.contains("1 unread"), "{dock:?}");

    // Closing leaves the reviewed, successful turn idle.
    dashboard.key(KeyCode::Esc);
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Idle"), "{dock:?}");
}

#[test]
fn unread_outranks_a_retained_draft_in_the_operator_section() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::unread_outranks_a_retained_draft_in_the_operator_section",
    ) {
        return;
    }
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    open_and_settle(&mut dashboard, summary());
    dashboard.event_action(Event::Paste("pending draft".into()));
    dashboard.key(KeyCode::Esc);

    // A reply arriving behind a draft is still the first thing the section says.
    let mut complete = summary();
    complete.run = SessionRunId(2);
    complete.worker_stopped = true;
    complete.current_turn = Some(turn(2, OperatorTurnState::Complete));
    changed(
        &mut dashboard,
        complete,
        vec![turn_message(
            2,
            1,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                leased: false,
                pi_first_entry_id: None,
                pi_leaf_entry_id: None,
            }),
        )],
    );
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Reply ready · 1 unread"), "{dock:?}");
    assert!(!dock.contains("Draft"), "{dock:?}");

    // The loaded-tail review clears unread; the section continues to summarize
    // the turn and does not expose composer text.
    dashboard.key(KeyCode::Char('o'));
    let _ = reviewed_screen(&mut dashboard, 120, 40);
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Idle"), "{dock:?}");
    assert!(!dock.contains("1 unread"), "{dock:?}");
    assert!(!dock.contains("pending draft"), "{dock:?}");
    dashboard.key(KeyCode::Esc);
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Idle"), "{dock:?}");
    assert!(!dock.contains("pending draft"), "{dock:?}");
}

#[test]
fn narrow_sidebar_keeps_the_operator_heading_and_compact_state_rows() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::narrow_sidebar_keeps_the_operator_heading_and_compact_state_rows",
    ) {
        return;
    }
    let mut dashboard = dashboard_fixture();
    let area = Rect::new(0, 0, 120, 40);
    dashboard.install_area(area);
    // The real minimum draggable width leaves 19 content cells.
    drag_sidebar_to(&mut dashboard, area, 19);
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(
        dock.contains("Operator"),
        "the section heading survives: {dock:?}"
    );
    assert!(dock_status_row(&rendered_rows(&dashboard, 120, 40)).contains("Off"));
    assert!(
        dock.lines().all(|line| line.chars().count() == 19),
        "{dock:?}"
    );

    open_and_settle(&mut dashboard, busy());
    let out = changed(
        &mut dashboard,
        busy(),
        vec![turn_message(
            2,
            1,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                id: "call-1".into(),
                name: "mcp__tool_with_a_very_long_name".into(),
                arguments: "{}".into(),
            }),
        )],
    );
    assert!(out.is_empty(), "{out:?}");
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(
        dock.contains("Operator") && dock.contains("Responding"),
        "{dock:?}"
    );
    assert!(
        dock.lines().all(|line| line.chars().count() == 19),
        "{dock:?}"
    );

    // A long unconfirmed-stop clause keeps a distinct cell-readable prefix.
    dashboard.ctrl('x');
    let out = dashboard.drain_outbox();
    let stop = out
        .iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Stop { .. })
            )
        })
        .cloned()
        .expect("Ctrl-x sends Stop");
    let mut unconfirmed = busy();
    unconfirmed.current_turn = Some(turn(2, OperatorTurnState::Complete));
    let out = respond(
        &mut dashboard,
        stop.request_id,
        OperatorResponse::Stopped(turn(2, OperatorTurnState::Complete)),
    );
    let read = read_request(&out);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(unconfirmed, Vec::new(), None),
    );
    let _ = reviewed_screen(&mut dashboard, 120, 40);
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(
        dock.contains("Responding"),
        "an unconfirmed worker remains Working: {dock:?}"
    );
    assert!(
        dock.lines().all(|line| line.chars().count() == 19),
        "{dock:?}"
    );

    // An unconfirmed error still reads as Working; a confirmed interruption
    // returns the sidebar to the compact Idle state.
    let mut failed = busy();
    failed.current_turn = Some(turn(2, OperatorTurnState::Error));
    changed(&mut dashboard, failed, Vec::new());
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Responding"), "{dock:?}");
    let mut interrupted = summary();
    interrupted.run = SessionRunId(2);
    interrupted.worker_stopped = true;
    interrupted.current_turn = Some(turn(2, OperatorTurnState::Interrupted));
    changed(&mut dashboard, interrupted, Vec::new());
    let dock = dock_row(&rendered_rows(&dashboard, 120, 40));
    assert!(dock.contains("Idle"), "{dock:?}");
}

#[test]
fn operator_section_click_opens_the_singleton_from_its_right_edge() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::operator_section_click_opens_the_singleton_from_its_right_edge",
    ) {
        return;
    }
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(area);
    dashboard.install_hierarchy(fixture_hierarchy());
    dashboard.install_focus(SessionId(1));

    let rows = rendered_rows(&dashboard, 120, 40);
    // Attention is empty, so Operator leads the sidebar before the tree.
    assert!(rows[2].contains("Operator"), "{rows:?}");
    assert!(rows[3].contains("Off"), "{rows:?}");
    assert!(rows[6].contains("CONSIGINT"), "{rows:?}");
    let screen = rows.join("\n");
    for removed in [
        "New Conversation",
        "Chats",
        "conversation list",
        "[+]",
        "[x]",
    ] {
        assert!(
            !screen.contains(removed),
            "removed control: {removed}\n{screen}"
        );
    }

    // Any section row is navigation, including its right edge; there is no close target.
    dashboard.mouse_action(mouse(38, 2), area);
    let requests = dashboard.drain_outbox();
    assert_eq!(operator_requests(&requests), [&OperatorRequest::Open]);
    // A second Manual Open is coalesced while the first is pending.
    dashboard.mouse_action(mouse(38, 2), area);
    assert!(dashboard.drain_outbox().is_empty());

    let open = requests.into_iter().next().unwrap();
    let out = respond(
        &mut dashboard,
        open.request_id,
        OperatorResponse::Opened(summary()),
    );
    assert!(matches!(
        read_request(&out).request,
        Request::Operator(OperatorRequest::Read { .. })
    ));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));

    // Ctrl-n is not a shortcut: while the conversation is open it neither
    // creates nor resets the composer draft.
    dashboard.event_action(Event::Paste("kept".into()));
    dashboard.ctrl('n');
    assert!(dashboard.drain_outbox().is_empty());
    assert!(
        draw_text(&mut dashboard, 120, 40).contains("kept"),
        "the composer draft survives Ctrl-n"
    );

    // The retired title-bar control opens neither Operator surface.
    dashboard.mouse_action(mouse(9, 0), area);
    assert!(
        operator_requests(&dashboard.drain_outbox()).is_empty(),
        "the clean title bar sends no Operator request"
    );
    dashboard.key(KeyCode::Esc);
    dashboard.mouse_action(mouse(9, 0), area);
    assert!(operator_requests(&dashboard.drain_outbox()).is_empty());
    assert!(
        !draw_text(&mut dashboard, 120, 40).contains("Managing"),
        "no Operator surface opens from the title bar"
    );

    dashboard.ctrl('g');
    dashboard.ctrl('n');
    assert!(dashboard.drain_outbox().is_empty());

    // Browse help/menu describes the singleton and never a chat list.
    dashboard.key(KeyCode::Char(' '));
    let menu = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(
        menu.contains("Panes"),
        "the Space menu lists the Panes group: {menu}"
    );
    for removed in [
        "concurrent",
        "Ctrl-n",
        "Close conversation",
        "New Conversation",
        "Chats",
    ] {
        assert!(
            !menu.contains(removed),
            "removed menu action: {removed}\n{menu}"
        );
    }
}

#[test]
fn opening_again_keeps_state_and_foreign_receipts_never_replace_it() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let out = opened(&mut dashboard, summary());
    let read = read_request(&out);
    let transcript: String = (0..40).map(|row| format!("KEEP_ROW_{row:02}\n")).collect();
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(summary(), vec![delta(1, &transcript)], None),
    );
    assert!(out.is_empty());
    dashboard.event_action(Event::Paste("keep me".into()));

    // Leaving and reopening sends Open again, never a list, and resets nothing.
    dashboard.ctrl('g');
    let open = open_request(&mut dashboard);
    let out = respond(
        &mut dashboard,
        open.request_id,
        OperatorResponse::Opened(summary()),
    );
    assert!(
        out.is_empty(),
        "an unchanged journal is not re-read: {out:?}"
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("keep me"), "{screen}");
    assert!(screen.contains("KEEP_ROW_"), "{screen}");

    // A receipt for another conversation cannot replace the singleton.
    dashboard.ctrl('g');
    let foreign = open_request(&mut dashboard);
    let mut other = summary();
    other.id = OperatorConversationId(8);
    other.session = SessionId(98);
    let out = respond(
        &mut dashboard,
        foreign.request_id,
        OperatorResponse::Opened(other),
    );
    assert!(out.is_empty());
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("did not match the singleton conversation"),
        "{screen}"
    );
    assert!(screen.contains("keep me"), "the draft survives: {screen}");

    // OperatorChanged for the singleton loads it; legacy ids stay on disk and hidden.
    let legacy = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::OperatorChanged(OperatorConversationId(9)),
    ));
    assert!(
        legacy.is_empty(),
        "legacy rows never enter this view: {legacy:?}"
    );
    let dirty = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::OperatorChanged(OperatorConversationId(7)),
    ));
    let read = read_request(&dirty);
    assert!(matches!(
        read.request,
        Request::Operator(OperatorRequest::Read {
            conversation: OperatorConversationId(7),
            after: 1,
            ..
        })
    ));
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(summary(), vec![delta(2, "appended")], None),
    );
    assert!(out.is_empty());
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("appended"), "{screen}");
}

#[test]
fn header_names_pi_and_only_a_really_selected_model() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_settings(ovrcr::tui::Settings {
        operator: ovrcr_protocol::OperatorSettings {
            model: Some("openai/gpt-6.1-sol".into()),
            ..Default::default()
        },
        ..Default::default()
    });
    let out = opened(&mut dashboard, summary());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(
            summary(),
            vec![
                user(1, "Review this.", OperatorContext::default()),
                message(
                    2,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::model_selected(
                        OperatorModel {
                            provider: "openai".into(),
                            id: "gpt-6.1-sol".into(),
                        },
                        None,
                    )),
                ),
                delta(3, "Inspecting."),
            ],
            None,
        ),
    );
    assert!(out.is_empty());
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("Operator · pi · openai/gpt-6.1-sol · ready"),
        "{screen}"
    );
    assert!(
        !screen.contains("ModelSelected"),
        "the selected model is metadata, not a journal row: {screen}"
    );

    // Without a ModelSelected the settings value is never guessed into the header.
    let mut dashboard = dashboard_fixture();
    dashboard.install_settings(ovrcr::tui::Settings {
        operator: ovrcr_protocol::OperatorSettings {
            model: Some("openai/gpt-6.1-sol".into()),
            ..Default::default()
        },
        ..Default::default()
    });
    open_and_settle(&mut dashboard, summary());
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("Operator · pi · ready"), "{screen}");
    assert!(!screen.contains("gpt-6.1-sol"), "{screen}");

    // pi's reserved placeholder is treated as no selection.
    let mut placeholder = summary();
    placeholder.model = Some(OperatorModel {
        provider: "unknown".into(),
        id: "unknown".into(),
    });
    let mut dashboard = dashboard_fixture();
    open_and_settle(&mut dashboard, placeholder);
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("Operator · pi · ready"), "{screen}");
    assert!(
        !screen.contains("Operator · pi · unknown"),
        "an unknown model placeholder must stay out of the Operator header: {screen}"
    );

    // The fixed setup failure is shown verbatim, routing to `ovrcr operator setup`.
    let mut dashboard = dashboard_fixture();
    let out = opened(&mut dashboard, summary());
    let read = read_request(&out);
    let out = dashboard.handle_server_message(ServerMessage::Response {
        request_id: read.request_id,
        response: Response::Error {
            code: ErrorCode::InvalidRequest,
            message: "No Pi model selected; run ovrcr operator setup.".into(),
        },
    });
    assert!(out.is_empty(), "a failed read is not retried: {out:?}");
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("No Pi model selected; run ovrcr operator setup."),
        "{screen}"
    );
    // Reopening clears the stale banner; the next dirty signal loads the failed turn.
    dashboard.ctrl('g');
    let open = open_request(&mut dashboard);
    let out = respond(
        &mut dashboard,
        open.request_id,
        OperatorResponse::Opened(summary()),
    );
    assert!(
        out.is_empty(),
        "an unchanged journal is not re-read: {out:?}"
    );
    let dirty = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::OperatorChanged(OperatorConversationId(7)),
    ));
    let read = read_request(&dirty);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(
            summary(),
            vec![message(
                1,
                OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(
                    "Pi is unavailable; install pi or set OVRCR_PI_EXECUTABLE.".into(),
                )),
            )],
            None,
        ),
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("Pi is unavailable; install pi or set OVRCR_PI_EXECUTABLE."),
        "{screen}"
    );
}

#[test]
fn submission_freezes_context_and_only_acceptance_clears_that_draft() {
    let mut dashboard = dashboard_fixture();
    let hierarchy = fixture_hierarchy();
    let (project, workspace, session) = hierarchy
        .projects
        .iter()
        .flat_map(|project| {
            project.workspaces.iter().flat_map(move |workspace| {
                workspace
                    .sessions
                    .iter()
                    .map(move |session| (project, workspace, session))
            })
        })
        .find(|(_, _, session)| session.id == SessionId(1))
        .unwrap();
    let expected = OperatorContext {
        project: Some(project.name.clone()),
        workspace: Some(workspace.name.clone()),
        workspace_id: Some(workspace.id.clone()),
        cwd: Some(session.cwd.clone()),
        session: Some(session.id),
        run: Some(session.run),
    };
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    open_and_settle(&mut dashboard, summary());

    assert!(!matches!(
        dashboard.event_action(Event::Paste("hello\n界".into())),
        DashboardAction::PtyBytes(_)
    ));
    for event in [
        Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
        Event::Key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)),
        Event::Key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL)),
        Event::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
        Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT)),
        Event::Paste("界".into()),
        Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
    ] {
        assert!(!matches!(
            dashboard.event_action(event),
            DashboardAction::PtyBytes(_) | DashboardAction::Detach
        ));
    }
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        dashboard.event_action(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            kind,
        )));
    }
    assert!(
        dashboard.drain_outbox().is_empty(),
        "draft clear and held Ctrl+C cannot send"
    );
    dashboard.ctrl('c');
    assert!(
        dashboard.drain_outbox().is_empty(),
        "an empty idle Ctrl+C cannot send"
    );
    dashboard.event_action(Event::Paste("hello\n界".into()));
    let requests = dashboard.drain_outbox();
    assert!(requests.is_empty(), "paste only edits: {requests:?}");
    dashboard.key(KeyCode::Enter);
    let requests = dashboard.drain_outbox();
    let submit = requests
        .iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Submit { .. })
            )
        })
        .expect("composer Enter submits the frozen draft and context");
    assert_eq!(
        submit.request,
        Request::Operator(OperatorRequest::Submit {
            skill: None,
            conversation: OperatorConversationId(7),
            expected_run: SessionRunId(1),
            text: "hello\n界".into(),
            context: expected.clone(),
        })
    );
    // Selecting another terminal never recaptures the pending conversation's context.
    let frozen = submit.request.clone();
    dashboard.install_focus(SessionId(3));
    assert_eq!(submit.request, frozen, "the pending payload is frozen");
    // Reopening shows the same conversation; the pending submission is unchanged.
    let open = open_request(&mut dashboard);
    let out = respond(
        &mut dashboard,
        open.request_id,
        OperatorResponse::Opened(summary()),
    );
    assert!(out.is_empty(), "the journal is unchanged: {out:?}");
    assert!(!requests.iter().any(|message| matches!(
        message.request,
        Request::Input { .. } | Request::AssignedHumanInput { .. }
    )));

    let submitted = respond(
        &mut dashboard,
        submit.request_id,
        OperatorResponse::Submitted(turn(2, OperatorTurnState::Running)),
    );
    let read = read_request(&submitted);
    let mut running = summary();
    running.run = SessionRunId(2);
    running.worker_stopped = false;
    running.current_turn = Some(turn(2, OperatorTurnState::Running));
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(running.clone(), vec![], None),
    );
    assert!(out.is_empty());
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("running"), "{screen}");
    assert!(screen.contains("[Stop Ctrl-x]"), "{screen}");
    assert!(
        !screen.contains("hello"),
        "acceptance cleared exactly the submitted draft: {screen}"
    );

    // Busy: the draft stays with the fixed reason; nothing is queued or steered.
    dashboard.event_action(Event::Paste("keep this".into()));
    dashboard.key(KeyCode::Enter);
    assert!(dashboard.drain_outbox().is_empty());
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("keep this"), "{screen}");
    assert!(screen.contains(BUSY_REASON), "{screen}");

    // Complete without controlled-stop proof is still not submittable.
    let mut finishing = running.clone();
    finishing.worker_stopped = false;
    finishing.current_turn = Some(turn(2, OperatorTurnState::Complete));
    let dirty = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::OperatorChanged(OperatorConversationId(7)),
    ));
    let read = read_request(&dirty);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(finishing.clone(), vec![], None),
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("finishing · stop unconfirmed"), "{screen}");
    dashboard.key(KeyCode::Enter);
    assert!(dashboard.drain_outbox().is_empty(), "stop proof gates Send");
    assert!(draw_text(&mut dashboard, 120, 40).contains(BUSY_REASON));

    // The refreshed summary carries the exact-run proof and unlocks Send.
    let mut certified = finishing;
    certified.worker_stopped = true;
    let dirty = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::OperatorChanged(OperatorConversationId(7)),
    ));
    let read = read_request(&dirty);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(certified, vec![], None),
    );
    assert!(draw_text(&mut dashboard, 120, 40).contains("complete"));
    dashboard.key(KeyCode::Enter);
    let requests = dashboard.drain_outbox();
    assert!(matches!(
        requests.first().map(|message| &message.request),
        Some(Request::Operator(OperatorRequest::Submit {
            expected_run: SessionRunId(2),
            ..
        }))
    ));
}

/// The target `capture_operator_context` derives for a fixture session.
fn captured_context(hierarchy: &HierarchySnapshot, id: SessionId) -> OperatorContext {
    let (project, workspace, session) = hierarchy
        .projects
        .iter()
        .flat_map(|project| {
            project.workspaces.iter().flat_map(move |workspace| {
                workspace
                    .sessions
                    .iter()
                    .map(move |session| (project, workspace, session))
            })
        })
        .find(|(_, _, session)| session.id == id)
        .expect("the fixture session exists");
    OperatorContext {
        project: Some(project.name.clone()),
        workspace: Some(workspace.name.clone()),
        workspace_id: Some(workspace.id.clone()),
        cwd: Some(session.cwd.clone()),
        session: Some(session.id),
        run: Some(session.run),
    }
}

#[test]
fn reopening_either_composer_captures_the_selected_target_for_the_next_send() {
    let hierarchy = fixture_hierarchy();
    let target_a = captured_context(&hierarchy, SessionId(1));
    let target_b = captured_context(&hierarchy, SessionId(3));
    assert_ne!(target_a, target_b, "the fixture targets are distinct");

    // Quick: open on target A, leave, select B, then send the retained draft.
    let mut dashboard = dashboard_fixture();
    let out = opened(&mut dashboard, summary());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(
            summary(),
            vec![user(1, "earlier request", target_a.clone()), completed(2)],
            None,
        ),
    );
    assert!(out.is_empty(), "an empty tail requests nothing: {out:?}");
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("Managing consigint / auth / #1 run 1"),
        "the composer starts at the captured target: {screen}"
    );
    dashboard.key(KeyCode::Esc);
    dashboard.install_focus(SessionId(3));
    dashboard.drain_outbox();
    dashboard.ctrl('o');
    dashboard.event_action(Event::Paste("to the selected target".into()));
    let overlay = draw_text(&mut dashboard, 120, 40);
    assert!(
        overlay.contains("Operator · pi · spacelift-agent / progress / #3 run 1"),
        "Quick names the target captured for this message: {overlay}"
    );
    dashboard.key(KeyCode::Enter);
    let requests = dashboard.drain_outbox();
    let submit = requests
        .iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Submit { .. })
            )
        })
        .expect("Quick sends the retained draft to the selected target");
    assert_eq!(
        submit.request,
        Request::Operator(OperatorRequest::Submit {
            skill: None,
            conversation: OperatorConversationId(7),
            expected_run: SessionRunId(1),
            text: "to the selected target".into(),
            context: target_b.clone(),
        })
    );

    // The full composer follows the same rule after a reselect.
    let mut dashboard = dashboard_fixture();
    let out = opened(&mut dashboard, summary());
    let read = read_request(&out);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(
            summary(),
            vec![user(1, "earlier request", target_a.clone()), completed(2)],
            None,
        ),
    );
    dashboard.key(KeyCode::Esc);
    dashboard.install_focus(SessionId(3));
    dashboard.drain_outbox();
    dashboard.key(KeyCode::Char('o'));
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("Managing spacelift-agent / progress / #3 run 1"),
        "the row above the composer names the next message's target: {screen}"
    );
    dashboard.event_action(Event::Paste("full composer request".into()));
    dashboard.key(KeyCode::Enter);
    let requests = dashboard.drain_outbox();
    let submit = requests
        .iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Submit { .. })
            )
        })
        .expect("the full composer sends to the reselected target");
    assert_eq!(
        submit.request,
        Request::Operator(OperatorRequest::Submit {
            skill: None,
            conversation: OperatorConversationId(7),
            expected_run: SessionRunId(1),
            text: "full composer request".into(),
            context: target_b.clone(),
        })
    );

    // An accepted Quick Send opens the full view. Quick opened over it must use
    // that view's next-message target, not an older navigation capture.
    let mut dashboard = dashboard_fixture();
    let out = opened(&mut dashboard, summary());
    let read = read_request(&out);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(
            summary(),
            vec![user(1, "earlier request", target_a), completed(2)],
            None,
        ),
    );
    dashboard.key(KeyCode::Esc);
    dashboard.install_focus(SessionId(3));
    dashboard.drain_outbox();
    dashboard.ctrl('o');
    dashboard.event_action(Event::Paste("to the selected target".into()));
    dashboard.key(KeyCode::Enter);
    let requests = dashboard.drain_outbox();
    let submit = requests
        .iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Submit { .. })
            )
        })
        .expect("Quick sends to the reselected target");
    assert_eq!(
        submit.request,
        Request::Operator(OperatorRequest::Submit {
            skill: None,
            conversation: OperatorConversationId(7),
            expected_run: SessionRunId(1),
            text: "to the selected target".into(),
            context: target_b.clone(),
        })
    );
    let submitted = respond(
        &mut dashboard,
        submit.request_id,
        OperatorResponse::Submitted(turn(2, OperatorTurnState::Running)),
    );
    let read = read_request(&submitted);
    let mut certified = summary();
    certified.run = SessionRunId(2);
    certified.worker_stopped = true;
    certified.current_turn = Some(turn(2, OperatorTurnState::Complete));
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(
            certified,
            vec![
                user(1, "to the selected target", target_b.clone()),
                completed(2),
            ],
            None,
        ),
    );
    assert!(out.is_empty(), "the accepted turn finishes: {out:?}");
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("Operator · pi · complete"),
        "acceptance shows the sole conversation: {screen}"
    );
    dashboard.ctrl('o');
    dashboard.event_action(Event::Paste("quick over the full view".into()));
    dashboard.key(KeyCode::Enter);
    let requests = dashboard.drain_outbox();
    let submit = requests
        .iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Submit { .. })
            )
        })
        .expect("Quick over the visible full view sends");
    assert_eq!(
        submit.request,
        Request::Operator(OperatorRequest::Submit {
            skill: None,
            conversation: OperatorConversationId(7),
            expected_run: SessionRunId(2),
            text: "quick over the full view".into(),
            context: target_b,
        })
    );
}

#[test]
fn stop_uses_the_exact_identity_and_only_a_refresh_certifies_it() {
    let area = Rect::new(0, 0, 120, 40);

    // Ctrl-x on a running turn sends the exact identity, once.
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    open_and_settle(&mut dashboard, busy());
    dashboard.ctrl('x');
    let requests = dashboard.drain_outbox();
    assert_eq!(
        operator_requests(&requests),
        [&OperatorRequest::Stop {
            identity: turn(2, OperatorTurnState::Running).identity
        }]
    );
    dashboard.ctrl('x');
    assert!(dashboard.drain_outbox().is_empty(), "Stop is not repeated");
    // While the Stop is in flight neither Ctrl-x nor an empty Ctrl-c repeats it.
    dashboard.event_action(Event::Paste("kept".into()));
    dashboard.ctrl('x');
    dashboard.ctrl('c');
    assert!(
        dashboard.drain_outbox().is_empty(),
        "an in-flight Stop is never re-sent"
    );

    // The acknowledgement retires authority; only the refreshed stop proof certifies.
    let stop = requests.into_iter().next().unwrap();
    let out = respond(
        &mut dashboard,
        stop.request_id,
        OperatorResponse::Stopped(turn(2, OperatorTurnState::Interrupted)),
    );
    let read = read_request(&out);
    let mut interrupted = busy();
    interrupted.current_turn = Some(turn(2, OperatorTurnState::Interrupted));
    respond(
        &mut dashboard,
        read.request_id,
        history_of(interrupted.clone(), vec![], None),
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("interrupted · stop unconfirmed"),
        "{screen}"
    );

    interrupted.worker_stopped = true;
    let dirty = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::OperatorChanged(OperatorConversationId(7)),
    ));
    let read = read_request(&dirty);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(interrupted, vec![], None),
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("interrupted"), "{screen}");
    assert!(!screen.contains("stop unconfirmed"), "{screen}");

    // A nonempty empty-state Ctrl-c clears the draft; an empty one stops.
    dashboard.event_action(Event::Paste("clear me".into()));
    dashboard.ctrl('c');
    assert!(
        dashboard.drain_outbox().is_empty(),
        "clearing is not a Stop"
    );
    assert!(!draw_text(&mut dashboard, 120, 40).contains("clear me"));
    dashboard.event_action(Event::Paste("second".into()));
    dashboard.key(KeyCode::Enter);
    let submit = dashboard.drain_outbox().remove(0);
    let out = respond(
        &mut dashboard,
        submit.request_id,
        OperatorResponse::Submitted(turn(3, OperatorTurnState::Running)),
    );
    let read = read_request(&out);
    let mut running = summary();
    running.run = SessionRunId(3);
    running.worker_stopped = false;
    running.current_turn = Some(turn(3, OperatorTurnState::Running));
    respond(
        &mut dashboard,
        read.request_id,
        history_of(running, vec![], None),
    );
    dashboard.ctrl('c');
    let requests = dashboard.drain_outbox();
    assert_eq!(
        operator_requests(&requests),
        [&OperatorRequest::Stop {
            identity: turn(3, OperatorTurnState::Running).identity
        }],
        "an empty full-composer Ctrl-c stops the exact turn"
    );
}

#[test]
fn paged_history_escapes_receipts_and_unread_waits_for_a_drawn_tail() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::paged_history_escapes_receipts_and_unread_waits_for_a_drawn_tail",
    ) {
        return;
    }
    let mut dashboard = dashboard_fixture();
    dashboard.install_config_dir(std::path::PathBuf::from("/instance"));
    let out = opened(&mut dashboard, with_model(summary()));
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(
            with_model(summary()),
            vec![
                user(
                    1,
                    "run it",
                    OperatorContext {
                        project: Some("proj".into()),
                        workspace: Some("work".into()),
                        cwd: Some("/captured/cwd".into()),
                        session: Some(SessionId(7)),
                        run: Some(SessionRunId(3)),
                        ..Default::default()
                    },
                ),
                message(
                    2,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                        id: "call-1".into(),
                        name: "bash".into(),
                        arguments: "{\"command\":\"ls /captured\"}".into(),
                    }),
                ),
            ],
            Some(2),
        ),
    );
    // A paged response continues from its last ordinal instead of restarting.
    let next = read_request(&out);
    assert!(matches!(
        next.request,
        Request::Operator(OperatorRequest::Read { after: 2, .. })
    ));
    let out = respond(
        &mut dashboard,
        next.request_id,
        history_of(
            with_model(summary()),
            vec![
                delta(
                    3,
                    &(0..60)
                        .map(|row| format!("BULK_ROW_{row:02}  \n"))
                        .collect::<String>(),
                ),
                message(
                    4,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                        id: "call-1".into(),
                        output: "a\u{1b}]52;c;INJECTED\u{7}".into(),
                        truncated: false,
                        status: OperatorToolStatus::Succeeded,
                    }),
                ),
                completed(5),
            ],
            None,
        ),
    );
    assert!(out.is_empty(), "the durable tail ends the paging: {out:?}");

    // A too-small draw cannot acknowledge the tail; the completion Unread stays.
    let _small = reviewed_screen(&mut dashboard, 40, 12);
    let screen = reviewed_screen(&mut dashboard, 140, 40);
    assert!(screen.contains("1 unread"), "{screen}");
    assert!(screen.contains("BULK_ROW_59"), "{screen}");
    assert!(
        !screen.contains("\u{1b}]52;c;INJECTED"),
        "a command row never leaks active controls: {screen}"
    );
    assert!(!screen.contains("INJECTED"), "{screen}");
    // Rendering the loaded tail acknowledges it once, and only then.
    let reviewed = reviewed_screen(&mut dashboard, 140, 40);
    assert!(!reviewed.contains("1 unread"), "{reviewed}");

    // End reaches the command above the viewport and scrolls it into view;
    // expanding it shows the command and its output escaped.
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::End);
    dashboard.key(KeyCode::Enter);
    let expanded = draw_text(&mut dashboard, 120, 40);
    assert!(expanded.contains("▌       $ ls /captured"), "{expanded}");
    assert!(
        expanded.contains("\\u{1b}]52;c;INJECTED\\u{7}"),
        "{expanded}"
    );
    // The rendered receipt's actual hit row collapses it through mouse geometry.
    let mut rendered = Terminal::new(TestBackend::new(120, 40)).unwrap();
    dashboard.draw(&mut rendered).unwrap();
    let hit = (1..39)
        .find(|y| {
            (0..120)
                .map(|x| rendered.backend().buffer()[(x, *y)].symbol())
                .collect::<String>()
                .contains("$ ls /captured")
        })
        .expect("the expanded command is drawn");
    dashboard.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 42,
            row: hit,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 120, 40),
    );
    let collapsed = draw_text(&mut dashboard, 120, 40);
    assert!(
        !collapsed.contains("INJECTED"),
        "a mouse collapse hides the expanded output: {collapsed}"
    );
    assert!(
        collapsed.contains("▌     ls /captured"),
        "the command row stays: {collapsed}"
    );

    // The captured target is its own stop below the request; expanding it
    // names the captured cwd and the fixed agent workdir separately.
    dashboard.key(KeyCode::Home);
    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Enter);
    let context = draw_text(&mut dashboard, 120, 40);
    assert!(context.contains("captured cwd: /captured/cwd"), "{context}");
    assert!(
        context.contains("agent workdir: /instance/operator-workdir"),
        "{context}"
    );

    // Scrolling away keeps a durable anchor; returning to the tail follows again.
    for _ in 0..20 {
        dashboard.key(KeyCode::PageDown);
    }
    assert!(
        rendered_rows(&dashboard, 120, 40)
            .join("\n")
            .contains("BULK_ROW_59")
    );
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::PageUp);
    let scrolled = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(!scrolled.contains("BULK_ROW_59"), "{scrolled}");
    for _ in 0..20 {
        dashboard.key(KeyCode::PageDown);
    }
    assert!(
        rendered_rows(&dashboard, 120, 40)
            .join("\n")
            .contains("BULK_ROW_59")
    );
}

#[test]
fn quick_creates_nothing_when_cancelled_and_shares_the_one_draft() {
    // A cancelled unsent Quick sends nothing and leaves no overlay.
    let mut dashboard = dashboard_fixture();
    dashboard.ctrl('o');
    dashboard.event_action(Event::Paste("abandoned".into()));
    dashboard.key(KeyCode::Esc);
    assert!(dashboard.drain_outbox().is_empty());
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(!screen.contains("abandoned"), "{screen}");
    assert!(!screen.contains("Quick message"), "{screen}");
    // No conversation was attached locally, so the first Send must still Open.
    dashboard.ctrl('o');
    dashboard.event_action(Event::Paste("first message".into()));
    dashboard.key(KeyCode::Enter);
    let requests = dashboard.drain_outbox();
    assert_eq!(
        operator_requests(&requests),
        [&OperatorRequest::Open],
        "the first explicit Send opens the one conversation; nothing is created by the draft"
    );
    let open = requests.into_iter().next().unwrap();
    let out = respond(
        &mut dashboard,
        open.request_id,
        OperatorResponse::Opened(summary()),
    );
    let submits: Vec<_> = out
        .iter()
        .filter(|message| {
            matches!(
                &message.request,
                Request::Operator(OperatorRequest::Submit {
                    skill: None,
                    conversation: OperatorConversationId(7),
                    expected_run: SessionRunId(1),
                    text,
                    ..
                }) if text == "first message"
            )
        })
        .collect();
    assert_eq!(submits.len(), 1, "one frozen Submit: {out:?}");
    let out = respond(
        &mut dashboard,
        submits[0].request_id,
        OperatorResponse::Submitted(turn(2, OperatorTurnState::Running)),
    );
    assert!(
        out.is_empty(),
        "the in-flight read suppresses a second one: {out:?}"
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("running"),
        "acceptance shows the turn: {screen}"
    );
    assert!(!screen.contains("Quick message"), "{screen}");
    assert!(
        !screen.contains("first message"),
        "only the accepted draft was cleared: {screen}"
    );

    // Both entry points edit the singleton draft, not competing copies.
    dashboard.event_action(Event::Paste("shared".into()));
    dashboard.ctrl('o');
    let overlay = draw_text(&mut dashboard, 120, 40);
    assert!(overlay.contains("shared"), "{overlay}");
    dashboard.key(KeyCode::Char('!'));
    dashboard.key(KeyCode::Esc);
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("shared!"), "{screen}");

    // A late receipt after dismissal never steals the view.
    let mut dashboard = dashboard_fixture();
    dashboard.ctrl('o');
    dashboard.event_action(Event::Paste("later".into()));
    dashboard.key(KeyCode::Enter);
    let open = dashboard.drain_outbox().remove(0);
    dashboard.key(KeyCode::Esc);
    let out = respond(
        &mut dashboard,
        open.request_id,
        OperatorResponse::Opened(summary()),
    );
    let submit = out
        .iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Submit { .. })
            )
        })
        .unwrap();
    respond(
        &mut dashboard,
        submit.request_id,
        OperatorResponse::Submitted(turn(2, OperatorTurnState::Running)),
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        !screen.contains("Managing"),
        "the view stays dismissed: {screen}"
    );
    assert!(!screen.contains("Quick message"), "{screen}");
    assert!(rendered_footer(&dashboard, 120).starts_with("BROWSE"));
}

#[test]
fn quick_send_while_busy_keeps_the_draft_and_reports_the_reason() {
    for value in [busy(), stop_unconfirmed()] {
        let mut dashboard = dashboard_fixture();
        open_and_settle(&mut dashboard, value);
        dashboard.event_action(Event::Paste("keep this".into()));
        dashboard.ctrl('o');
        dashboard.key(KeyCode::Enter);
        assert!(
            dashboard.drain_outbox().is_empty(),
            "never queues, steers or creates another conversation"
        );
        let screen = draw_text(&mut dashboard, 120, 40);
        assert!(screen.contains("keep this"), "{screen}");
        assert!(screen.contains(BUSY_REASON), "{screen}");
        assert!(screen.contains("Quick message"), "{screen}");
    }
}

#[test]
fn unicode_editing_keeps_the_full_composer_cursor_and_draft() {
    let mut dashboard = dashboard_fixture();
    open_and_settle(&mut dashboard, summary());
    dashboard.event_action(Event::Paste("draft 界".into()));
    dashboard.key(KeyCode::Left);
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("draft ▏界"), "{screen}");

    // Command+Backspace is not a composer edit and never leaves Operator.
    dashboard.event_action(Event::Key(KeyEvent::new(
        KeyCode::Backspace,
        KeyModifiers::SUPER,
    )));
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("draft ▏界"), "{screen}");
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.key(KeyCode::Char('!'));
    assert!(draw_text(&mut dashboard, 120, 40).contains("draft !▏界"));

    // Shift-Enter inserts a newline; unsupported Control chords insert no letters.
    for (letter, alt, shift) in [
        ('a', true, false),
        ('f', false, true),
        ('h', true, true),
        ('u', false, false),
        ('k', true, false),
        ('w', false, true),
    ] {
        let before = draw_text(&mut dashboard, 120, 40);
        dashboard.event_action(Event::Key(KeyEvent::new(
            KeyCode::Char(letter),
            KeyModifiers::CONTROL
                | if alt {
                    KeyModifiers::ALT
                } else {
                    KeyModifiers::NONE
                }
                | if shift {
                    KeyModifiers::SHIFT
                } else {
                    KeyModifiers::NONE
                },
        )));
        let after = draw_text(&mut dashboard, 120, 40);
        assert!(
            !after.contains(&format!("{letter}▏")) && !after.contains(&format!("▏{letter}")),
            "{letter}: an unsupported chord inserted text\n{after}"
        );
        let _ = before;
        assert!(!dashboard.drain_outbox().iter().any(|message| matches!(
            message.request,
            Request::Input { .. } | Request::AssignedHumanInput { .. }
        )));
    }
}

#[test]
fn operator_ctrl_g_labels_visible_agents_and_keeps_the_composer_draft() {
    let mut hierarchy = fixture_hierarchy();
    let agent = hierarchy.projects[1].workspaces[0]
        .sessions
        .iter_mut()
        .find(|session| session.id == SessionId(4))
        .unwrap();
    agent.kind = SessionKind::Agent {
        name: "fixture-agent".into(),
    };
    agent.name = "operator-agent".into();
    agent.label = "fixture-agent".into();
    agent.title = Some("operator-agent".into());

    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_area(area);
    dashboard.install_focus(SessionId(1));
    open_and_settle(&mut dashboard, summary());

    let open = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(open.contains("Operator · pi"), "{open}");
    assert!(
        !open.contains("[1]"),
        "an open conversation does not label Agents: {open}"
    );

    // Ctrl-g keeps Operator's ordinary leave behavior and opens the picker in Browse.
    dashboard.ctrl('g');
    let browse = rendered_rows(&dashboard, 120, 40);
    let agent_row = browse
        .iter()
        .find(|row| row.contains("operator-agent"))
        .expect("the visible Agent title is in the shared sidebar");
    assert!(agent_row.contains("[1]"), "{agent_row}");
    assert!(rendered_footer(&dashboard, 120).contains("1–9 jump"));

    dashboard.key(KeyCode::Esc);
    let out = opened(&mut dashboard, summary());
    assert!(
        out.is_empty(),
        "an unchanged journal is not re-read: {out:?}"
    );

    dashboard.event_action(Event::Paste("draft 界".into()));
    dashboard.key(KeyCode::Left);
    let before = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(
        before.contains("draft") && before.contains("界"),
        "{before}"
    );

    dashboard.ctrl('g');
    let labeled = rendered_rows(&dashboard, 120, 40);
    let agent_row = labeled
        .iter()
        .find(|row| row.contains("operator-agent"))
        .expect("the Agent remains in the shared sidebar");
    assert!(agent_row.contains("[1]"), "{agent_row}");

    dashboard.key(KeyCode::Esc);
    let out = opened(&mut dashboard, summary());
    assert!(
        out.is_empty(),
        "an unchanged journal is not re-read: {out:?}"
    );
    let returned = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(
        returned.contains("draft") && returned.contains("界"),
        "navigation preserves the composer draft: {returned}"
    );
}

#[test]
fn operator_quick_opens_over_every_input_mode_without_pty_bytes() {
    for mode in [
        "Browse",
        "Terminal then Ctrl-g",
        "History",
        "Copy",
        "Tasks",
        "Palette",
        "Which-key",
        "Operator",
        "Form",
        "Details",
    ] {
        let mut d = dashboard_fixture();
        match mode {
            "Terminal then Ctrl-g" => {
                d.key(KeyCode::Enter);
                d.ctrl('g');
                // Ctrl-g also opens the picker; close it before Quick message.
                d.ctrl('g');
            }
            "History" => {
                d.key(KeyCode::PageUp);
                if let Some(request) = optional_request(d.drain_outbox()) {
                    d.handle_server_message(ServerMessage::Response {
                        request_id: request.request_id,
                        response: Response::HistoryOpened(history_opened(20)),
                    });
                }
            }
            "Copy" => {
                d.key(KeyCode::Char('['));
            }
            "Tasks" => {
                d.ctrl('t');
            }
            "Palette" => {
                d.key(KeyCode::Char(':'));
                let inspect = only_request(d.drain_outbox());
                assert_eq!(inspect.request, Request::Inspect);
            }
            "Which-key" => {
                d.key(KeyCode::Char(' '));
            }
            "Operator" => {
                d.key(KeyCode::Char('o'));
            }
            "Form" => {
                d.key(KeyCode::Char('a'));
            }
            "Details" => {
                d.key(KeyCode::Char('u'));
            }
            _ => {}
        }
        d.drain_outbox();
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        d.draw(&mut terminal).unwrap();
        let before = terminal.backend().buffer().clone();
        for event in [
            Event::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)),
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            Event::Paste("popup\n界".into()),
        ] {
            assert!(
                !matches!(d.event_action(event), DashboardAction::PtyBytes(_)),
                "mode {mode:?}"
            );
        }
        for event in [
            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Event::Paste("xpopup\n界".into()),
            Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
            Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)),
            Event::Paste("界".into()),
            Event::Key(KeyEvent::new(
                KeyCode::Char('b'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            )),
        ] {
            assert!(!matches!(
                d.event_action(event),
                DashboardAction::PtyBytes(_) | DashboardAction::Detach
            ));
        }
        d.draw(&mut terminal).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(
            text.contains("Quick message") && text.contains("popup"),
            "mode {mode:?}: {text}"
        );
        assert_eq!(d.focused_session(), Some(SessionId(1)));
        assert!(!d.drain_outbox().iter().any(|m| matches!(
            m.request,
            Request::Input { .. } | Request::AssignedHumanInput { .. }
        )));
        d.key(KeyCode::Esc);
        d.draw(&mut terminal).unwrap();
        assert_eq!(
            terminal.backend().buffer(),
            &before,
            "underlying {mode} changed"
        );
    }
}

/// Oh My Pi expands tool output and Claude Code opens its transcript with Ctrl-o, so in
/// Terminal mode the key reaches the program; Quick message is Ctrl-g then Ctrl-o.
#[test]
fn ctrl_o_in_terminal_mode_goes_to_the_program() {
    let ctrl_o = || Event::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    let mut d = dashboard_fixture();
    d.key(KeyCode::Enter);
    d.drain_outbox();
    assert_eq!(
        d.event_action(ctrl_o()),
        DashboardAction::PtyBytes(b"\x0f".to_vec())
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    d.draw(&mut terminal).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(!text.contains("Quick message ·"), "{text}");
    assert!(
        text.contains("Ctrl-o Quick message"),
        "footer names the route: {text}"
    );
    d.ctrl('g');
    assert!(!matches!(
        d.event_action(ctrl_o()),
        DashboardAction::PtyBytes(_)
    ));
    d.draw(&mut terminal).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("Quick message ·"), "{text}");
    assert!(!d.drain_outbox().iter().any(|m| matches!(
        m.request,
        Request::Input { .. } | Request::AssignedHumanInput { .. }
    )));
}

#[test]
fn leaving_operator_restores_terminal_input_and_keeps_the_draft() {
    let area = Rect::new(0, 0, 120, 40);
    for leave in [
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    ] {
        let mut dashboard = dashboard_fixture();
        dashboard.install_area(area);
        open_and_settle(&mut dashboard, summary());
        dashboard.event_action(Event::Paste("-draft".into()));
        assert!(draw_text(&mut dashboard, 120, 40).contains("-draft"));

        // Operator empties the terminal view while it is shown.
        let hidden = dashboard.request_view_at(area).expect("Operator view");
        let Request::SetView { view } = &hidden.request else {
            panic!("expected SetView, got {:?}", hidden.request);
        };
        assert!(view.panes.is_empty());
        acknowledge_view_request(&mut dashboard, hidden);

        dashboard.event_action(Event::Key(leave));
        if leave.modifiers == KeyModifiers::CONTROL {
            // Ctrl-g is the navigation key and also opens the label picker.
            // Close it before checking the plain Browse footer.
            dashboard.event_action(Event::Key(leave));
        }
        let screen = draw_text(&mut dashboard, 120, 40);
        assert!(!screen.contains("Managing"), "{leave:?}: {screen}");
        assert!(!screen.contains("New Conversation"), "{leave:?}: {screen}");
        assert!(
            rendered_footer(&dashboard, 120).starts_with("BROWSE"),
            "{leave:?}"
        );
        let shown = dashboard.request_view_at(area).expect("terminal view");
        let Request::SetView { view } = &shown.request else {
            panic!("expected SetView, got {:?}", shown.request);
        };
        assert_eq!(
            view.panes
                .iter()
                .map(|pane| pane.session)
                .collect::<Vec<_>>(),
            [SessionId(1)]
        );
        acknowledge_view_request(&mut dashboard, shown);
        // Ordinary terminal input is live again.
        assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
        assert_eq!(
            dashboard.key(KeyCode::Char('x')),
            DashboardAction::PtyBytes(b"x".to_vec())
        );
        // Ctrl-g returns to Browse and opens the picker; `o` then returns to
        // the same conversation and draft.
        dashboard.ctrl('g');
        dashboard.key(KeyCode::Char('o'));
        assert!(draw_text(&mut dashboard, 120, 40).contains("-draft"));
    }
}

#[test]
fn terminal_input_stays_responsive_while_a_stop_is_unconfirmed() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(area);
    open_and_settle(&mut dashboard, busy());
    dashboard.ctrl('x');
    let stop = dashboard.drain_outbox().remove(0);
    assert!(matches!(
        stop.request,
        Request::Operator(OperatorRequest::Stop { .. })
    ));
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("stopping · accepted operations may finish"),
        "{screen}"
    );

    // The pending Stop never blocks the ordinary terminal path.
    dashboard.ctrl('g');
    // Ctrl-g also opens the label picker; close it before ordinary input.
    dashboard.ctrl('g');
    let shown = dashboard.request_view_at(area).expect("terminal view");
    acknowledge_view_request(&mut dashboard, shown);
    assert_eq!(dashboard.key(KeyCode::Enter), DashboardAction::Redraw);
    assert_eq!(
        dashboard.key(KeyCode::Char('z')),
        DashboardAction::PtyBytes(b"z".to_vec())
    );
    assert_eq!(
        dashboard.event_action(Event::Paste("paste".into())),
        DashboardAction::PtyBytes(ovrcr::tui::encode_paste(
            "paste",
            dashboard.focused_bracketed_paste()
        ))
    );
    assert!(dashboard.drain_outbox().is_empty());
}

#[test]
fn operator_navigation_is_never_a_jump_target() {
    let mut hierarchy = fixture_hierarchy();
    let agent = hierarchy.projects[1].workspaces[0]
        .sessions
        .iter_mut()
        .find(|session| session.id == SessionId(4))
        .unwrap();
    agent.kind = SessionKind::Agent {
        name: "fixture-agent".into(),
    };
    agent.name = "operator-agent".into();
    agent.label = "fixture-agent".into();
    agent.title = Some("operator-agent".into());

    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_area(area);
    dashboard.install_focus(SessionId(1));
    let out = opened(&mut dashboard, summary());
    let read = read_request(&out);
    let transcript: String = (0..40).map(|row| format!("KEEP_ROW_{row:02}\n")).collect();
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(summary(), vec![delta(1, &transcript)], None),
    );
    assert!(out.is_empty());
    dashboard.event_action(Event::Paste("draft 界".into()));
    dashboard.key(KeyCode::Left);
    dashboard.key(KeyCode::PageUp);
    let transcript_before = rendered_rows(&dashboard, 120, 40)
        .into_iter()
        .filter(|row| row.contains("KEEP_ROW_"))
        .collect::<Vec<_>>();
    assert!(
        !transcript_before.is_empty(),
        "the fixture transcript is visible"
    );

    // Ctrl-g leaves Operator and opens the picker in Browse: only Agent rows
    // receive labels and the singleton row stays a navigation target.
    dashboard.ctrl('g');
    let held = rendered_rows(&dashboard, 120, 40);
    let agent_row = held
        .iter()
        .find(|row| row.contains("operator-agent"))
        .expect("the visible Agent title remains in the shared sidebar");
    assert!(
        agent_row.contains("[1]"),
        "Agent rows are the only targets: {agent_row}"
    );
    let operator_row = held
        .iter()
        .find(|row| row.contains("Operator") && !row.contains("operator-agent"))
        .unwrap_or_else(|| panic!("the fixed Operator navigation row: {held:?}"));
    assert!(
        !operator_row.contains("[1]"),
        "the singleton row is not a session target: {operator_row}"
    );
    assert!(
        rendered_footer(&dashboard, 120).contains("1–9 jump"),
        "the footer explains the picker"
    );

    // Esc closes the picker without touching the retained draft.
    dashboard.key(KeyCode::Esc);
    assert!(!rendered_footer(&dashboard, 120).contains("1–9 jump"));
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(dashboard.drain_outbox().is_empty());

    // The jump selects the Agent, never the Operator conversation.
    dashboard.ctrl('g');
    dashboard.key(KeyCode::Char('1'));
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    assert!(rendered_footer(&dashboard, 120).starts_with("Loading agent"));
    let view = only_request(dashboard.drain_outbox());
    assert!(matches!(
        &view.request,
        Request::SetView { view } if view.focused == Some(SessionId(4))
    ));
    acknowledge_view_request(&mut dashboard, view);
    assert!(rendered_footer(&dashboard, 120).starts_with("Terminal mode"));

    // Returning to the shell keeps the conversation, draft and scroll position.
    dashboard.ctrl('g');
    let out = opened(&mut dashboard, summary());
    assert!(
        out.is_empty(),
        "an unchanged journal is not re-read: {out:?}"
    );
    dashboard.event_action(Event::Paste("!X".into()));
    let returned = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(returned.contains("!X"), "{returned}");
    assert!(returned.contains("Operator · pi"), "{returned}");
    assert_eq!(
        rendered_rows(&dashboard, 120, 40)
            .into_iter()
            .filter(|row| row.contains("KEEP_ROW_"))
            .collect::<Vec<_>>(),
        transcript_before,
        "navigation preserves the transcript scroll position"
    );
}

#[test]
fn operator_ctrl_g_leaves_operator_and_labels_the_browse_sidebar() {
    let mut hierarchy = fixture_hierarchy();
    hierarchy.projects.truncate(1);
    hierarchy.projects[0].workspaces.truncate(1);
    let workspace = &mut hierarchy.projects[0].workspaces[0];
    let mut agent = workspace.sessions[0].clone();
    agent.kind = SessionKind::Agent {
        name: "fixture-agent".into(),
    };
    agent.name = "operator-agent".into();
    agent.label = "fixture-agent".into();
    agent.title = Some("operator-agent".into());
    workspace.sessions = vec![agent];

    let mut dashboard = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_area(Rect::new(0, 0, 80, 24));
    dashboard.install_focus(SessionId(3));
    open_and_settle(&mut dashboard, summary());
    let before = rendered_rows(&dashboard, 80, 24).join("\n");
    assert!(before.contains("Operator · pi"), "{before}");

    dashboard.ctrl('g');
    let browse = rendered_rows(&dashboard, 80, 24);
    let agent_row = browse
        .iter()
        .find(|row| {
            row.split('│')
                .next()
                .is_some_and(|sidebar| sidebar.contains("operator-agent"))
        })
        .expect("leaving Operator shows the Browse sidebar");
    assert!(
        agent_row.contains("[1]"),
        "the visible Agent receives a label: {agent_row}"
    );
    assert!(rendered_footer(&dashboard, 80).contains("1–9 jump"));
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));

    dashboard.event_action(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('1'),
        KeyModifiers::NONE,
        KeyEventKind::Press,
    )));
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
}

#[test]
fn operator_jump_keeps_agent_columns_and_rows_at_sidebar_boundaries() {
    for (cols, rows) in [(120, 40), (100, 24), (80, 24), (60, 24), (60, 16), (59, 16)] {
        let below_operator_floor = cols < 60 || rows < 16;
        let mut hierarchy = fixture_hierarchy();
        hierarchy.projects.truncate(1);
        hierarchy.projects[0].workspaces.truncate(1);
        let workspace = &mut hierarchy.projects[0].workspaces[0];
        let local = workspace.sessions[0].clone();
        let mut agent = local.clone();
        agent.id = SessionId(4);
        agent.kind = SessionKind::Agent {
            name: "fixture-agent".into(),
        };
        agent.name = "jump-target界".into();
        agent.label = "fixture-agent".into();
        agent.title = Some("jump-target界".into());
        workspace.sessions = vec![local, agent];

        let area = Rect::new(0, 0, cols, rows);
        let mut dashboard = Dashboard::new(TerminalSize { rows, cols });
        dashboard.install_hierarchy(hierarchy);
        dashboard.install_area(area);
        dashboard.install_focus(SessionId(3));
        open_and_settle(&mut dashboard, summary());
        if cols < 100 {
            // Operator's on-demand sidebar must be opened explicitly at narrow widths.
            dashboard.ctrl('l');
        }

        let before = if below_operator_floor {
            let floor = rendered_rows(&dashboard, cols, rows)
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                floor.contains("60×16") && floor.contains("Resize to continue"),
                "{cols}×{rows}: existing Operator resize guidance remains: {floor}"
            );
            // Below the floor Esc leaves the guidance for the ordinary Browse pane.
            dashboard.key(KeyCode::Esc);
            rendered_rows(&dashboard, cols, rows)
        } else {
            rendered_rows(&dashboard, cols, rows)
        };
        let before_agent = before
            .iter()
            .find(|row| row.contains("jump-target"))
            .unwrap_or_else(|| panic!("{cols}×{rows}: visible Agent row missing: {before:?}"));
        let status_and_title = before_agent.chars().skip(5).collect::<String>();

        // Ctrl-g leaves Operator and opens the picker in Browse.
        dashboard.ctrl('g');
        let held = rendered_rows(&dashboard, cols, rows);
        let after_agent = held
            .iter()
            .find(|row| row.contains("jump-target"))
            .unwrap_or_else(|| panic!("{cols}×{rows}: Agent row disappeared: {held:?}"));
        assert!(
            after_agent.contains("[1]"),
            "{cols}×{rows}: visible Agent gets a label: {after_agent}"
        );
        assert_eq!(
            after_agent.chars().skip(5).collect::<String>(),
            status_and_title,
            "{cols}×{rows}: title/status columns stay fixed"
        );
        assert_eq!(held.len(), before.len(), "{cols}×{rows}: rows do not move");
        assert!(
            rendered_footer(&dashboard, cols).contains("1–9 jump"),
            "{cols}×{rows}: the picker hint is visible"
        );

        // A second Ctrl-g closes the picker; mouse hit-testing still selects.
        dashboard.ctrl('g');
        let target_row = rendered_rows(&dashboard, cols, rows)
            .iter()
            .position(|row| row.contains("jump-target"))
            .unwrap_or_else(|| panic!("{cols}×{rows}: target row remains visible"));
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: u16::try_from(target_row).unwrap(),
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert_eq!(
            dashboard.focused_session(),
            Some(SessionId(4)),
            "{cols}×{rows}: mouse hit-testing still selects the drawn Agent row"
        );
    }
}

#[test]
fn operator_activity_draw_tracks_real_responses_and_clock() {
    for theme in [
        ovrcr_protocol::ThemeId::default(),
        ovrcr_protocol::ThemeId::ALL[1],
    ] {
        for (cols, rows) in [(140, 40), (80, 24), (60, 24), (60, 16)] {
            let mut d = Dashboard::new(TerminalSize { rows, cols });
            d.install_settings(ovrcr::tui::Settings {
                theme,
                ..Default::default()
            });
            open_and_settle(&mut d, summary());
            d.event_action(Event::Paste("submitted".into()));
            d.key(KeyCode::Enter);
            let submit = d
                .drain_outbox()
                .into_iter()
                .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Submit { .. })))
                .unwrap();
            let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
            let render = |d: &Dashboard, terminal: &mut Terminal<TestBackend>, time| {
                terminal.draw(|f| draw_dashboard_at(f, d, time)).unwrap();
                terminal.backend().buffer().clone()
            };
            let text = |b: &ratatui::buffer::Buffer| {
                b.content().iter().map(|c| c.symbol()).collect::<String>()
            };
            assert!(text(&render(&d, &mut terminal, 0)).contains("Sending"));
            let turn = OperatorTurn {
                identity: OperatorTurnIdentity {
                    conversation: OperatorConversationId(7),
                    turn: OperatorTurnId(2),
                    session: SessionId(99),
                    run: SessionRunId(2),
                },
                state: OperatorTurnState::Running,
                sequence: 1,
            };
            let out = d.handle_server_message(ServerMessage::Response {
                request_id: submit.request_id,
                response: Response::Operator(OperatorResponse::Submitted(turn.clone())),
            });
            // The accepted turn dirties the journal; answer its read with the durable tail.
            let read = out
                .into_iter()
                .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Read { .. })))
                .unwrap();
            let mut running = summary();
            running.run = SessionRunId(2);
            running.worker_stopped = false;
            running.current_turn = Some(turn.clone());
            d.handle_server_message(ServerMessage::Response {
                request_id: read.request_id,
                response: Response::Operator(OperatorResponse::History(OperatorHistory {
                    conversation: running.clone(),
                    messages: vec![message(
                        1,
                        OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                            format!("{}LATEST_DURABLE", "older history 界  \n".repeat(50)),
                        )),
                    )],
                    next_after: None,
                })),
            });
            d.event_action(Event::Paste("retained 界🙂 draft".into()));
            d.drain_outbox();
            let first = render(&d, &mut terminal, 0);
            let second = render(&d, &mut terminal, 100);
            let content_x = if cols >= 100 { 40 } else { 0 };
            let content = |buffer: &ratatui::buffer::Buffer| {
                (0..rows)
                    .map(|y| {
                        (content_x..cols)
                            .map(|x| buffer[(x, y)].symbol())
                            .collect::<String>()
                    })
                    .collect::<String>()
            };
            assert!(
                content(&first).contains("Responding"),
                "{cols}×{rows}: {}",
                text(&first)
            );
            let activity_y = (1..rows - 6)
                .find(|&y| {
                    (content_x..cols)
                        .map(|x| first[(x, y)].symbol())
                        .collect::<String>()
                        .contains("Responding")
                })
                .expect("activity must be inside the transcript");
            let preceding: String = (content_x..cols)
                .map(|x| first[(x, activity_y - 1)].symbol())
                .collect();
            assert!(preceding.contains("LATEST_DURABLE"), "{preceding}");
            assert_eq!(
                (0..rows - 6)
                    .filter(|&y| (content_x..cols)
                        .map(|x| first[(x, y)].symbol())
                        .collect::<String>()
                        .contains("Responding"))
                    .count(),
                1
            );
            let changes: Vec<_> = first
                .content()
                .iter()
                .zip(second.content())
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(i, _)| (i % cols as usize, i / cols as usize))
                .collect();
            assert!(
                changes.iter().any(|&(x, y)| y == activity_y as usize
                    && matches!(
                        first[(x as u16, y as u16)].symbol(),
                        "⠋" | "⠙" | "⠹" | "⠸" | "⠼" | "⠴" | "⠦" | "⠧" | "⠇" | "⠏"
                    )),
                "clock did not move the journal spinner: {changes:?}"
            );
            assert!(text(&second).contains("retained") && text(&second).contains("draft"));
            assert!(
                changes
                    .iter()
                    .all(|&(x, y)| y == activity_y as usize || (cols >= 100 && x < 39)),
                "clock changed content: {changes:?}"
            );
            assert!(d.drain_outbox().is_empty(), "draw cannot send requests");
            let area = Rect::new(0, 0, cols, rows);
            d.install_area(area);
            let journal_x = if cols >= 100 { 42 } else { 2 };
            for kind in [
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::ScrollUp,
            ] {
                assert!(!matches!(
                    d.mouse_action(
                        MouseEvent {
                            kind,
                            column: journal_x,
                            row: activity_y,
                            modifiers: KeyModifiers::NONE,
                        },
                        area
                    ),
                    DashboardAction::PtyBytes(_)
                ));
            }
            d.key(KeyCode::PageUp);
            assert!(!content(&render(&d, &mut terminal, 100)).contains("Responding"));
            for _ in 0..10 {
                d.key(KeyCode::PageDown);
            }
            assert!(content(&render(&d, &mut terminal, 100)).contains("Responding"));

            // A refreshed singleton summary, not a conversation list, updates authority.
            let mut complete = running;
            complete.worker_stopped = true;
            complete.current_turn = Some(OperatorTurn {
                state: OperatorTurnState::Complete,
                sequence: 2,
                ..turn
            });
            let dirty = d.handle_server_message(ServerMessage::Event(
                ServerEvent::OperatorChanged(OperatorConversationId(7)),
            ));
            let read = dirty
                .into_iter()
                .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Read { .. })))
                .expect("a dirty singleton journal is read");
            d.handle_server_message(ServerMessage::Response {
                request_id: read.request_id,
                response: Response::Operator(OperatorResponse::History(OperatorHistory {
                    conversation: complete,
                    messages: vec![],
                    next_after: None,
                })),
            });
            let complete = text(&render(&d, &mut terminal, 200));
            assert!(
                !complete.contains("Responding") && !complete.contains("Sending"),
                "{complete}"
            );
            assert!(complete.contains("retained") && complete.contains("draft"));
        }
    }
}

#[test]
fn narrow_and_wide_layouts_and_the_floor_guidance_keep_the_draft() {
    let mut dashboard = dashboard_fixture();
    open_and_settle(&mut dashboard, with_model(summary()));
    dashboard.event_action(Event::Paste("preserve draft".into()));

    // Wide: the saved sidebar stays; the conversation header starts after it.
    dashboard.install_area(Rect::new(0, 0, 140, 40));
    let wide = rendered_rows(&dashboard, 140, 40);
    let wide_text = wide.join("\n");
    assert!(
        wide[1].contains("Operator · pi · openai/gpt-6.1-sol · ready"),
        "{wide_text}"
    );
    assert!(
        wide[0].contains("OVRCR") && !wide[0].contains("Operator"),
        "the title bar carries no Operator control: {wide_text}"
    );
    assert!(wide_text.contains("preserve draft"), "{wide_text}");

    // Narrow: the conversation wins and Ctrl-l reveals only the ordinary sidebar.
    dashboard.install_area(Rect::new(0, 0, 80, 24));
    let narrow = rendered_rows(&dashboard, 80, 24).join("\n");
    assert!(
        narrow.contains("Operator · pi · openai/gpt-6.1-sol · ready"),
        "{narrow}"
    );
    assert!(
        !narrow.contains("CONSIGINT"),
        "sidebar stays on demand: {narrow}"
    );
    assert!(narrow.contains("preserve draft"), "{narrow}");
    dashboard.ctrl('l');
    let open = rendered_rows(&dashboard, 80, 24).join("\n");
    assert!(open.contains("CONSIGINT"), "{open}");
    assert!(
        !open.contains("Chats") && !open.contains("Conversation"),
        "{open}"
    );
    dashboard.ctrl('l');

    // Below the 60×16 floor the view asks for a resize and keeps the draft.
    for (cols, rows) in [(59, 16), (60, 15)] {
        let screen = draw_text(&mut dashboard, cols, rows);
        assert!(
            screen.contains("60×16") && screen.contains("Resize to continue"),
            "{cols}×{rows}: {screen}"
        );
    }
    // At the floor the conversation and its draft are still drawn.
    let floor = draw_text(&mut dashboard, 60, 16);
    assert!(floor.contains("Operator · pi"), "{floor}");
    assert!(floor.contains("preserve draft"), "{floor}");
    // Returning to a supported size keeps the same draft.
    assert!(draw_text(&mut dashboard, 120, 40).contains("preserve draft"));
}

#[test]
fn short_sidebar_compacts_then_hides_the_operator_section_to_preserve_tree() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::short_sidebar_compacts_then_hides_the_operator_section_to_preserve_tree",
    ) {
        return;
    }
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(fixture_hierarchy());
    dashboard.install_focus(SessionId(1));

    // 6 rows leave 4 body lines: the section compacts to one row above the tree.
    let roomy = rendered_rows(&dashboard, 120, 6);
    assert!(
        roomy[1].contains("Operator") && roomy[1].contains("Off"),
        "{roomy:?}"
    );
    assert!(roomy[2].contains("CONSIGINT"), "{roomy:?}");
    // 5 rows leave 3: the section is hidden and the tree takes the space.
    let tight = rendered_rows(&dashboard, 120, 5);
    assert!(
        !tight
            .iter()
            .skip(1)
            .any(|row| row.contains("Operator") || row.contains("OPERATOR")),
        "{tight:?}"
    );
    assert!(tight[1].contains("CONSIGINT"), "{tight:?}");

    // The row is one line whatever the activity and unread state.
    let area = Rect::new(0, 0, 120, 40);
    let before = rendered_rows(&dashboard, 120, 40);
    open_and_settle(&mut dashboard, busy());
    let busy_rows = rendered_rows(&dashboard, 120, 40);
    assert_eq!(busy_rows.len(), before.len());
    let busy_dock = dock_row(&busy_rows);
    assert!(busy_dock.contains("Responding"), "{busy_rows:?}");
    assert!(
        !busy_dock.contains('▾') && !busy_dock.contains('▸'),
        "no collapse arrow: {busy_rows:?}"
    );
    // The right edge of the status row is navigation, never a close target.
    dashboard.mouse_action(mouse(38, 2), area);
    assert!(
        dashboard
            .drain_outbox()
            .iter()
            .any(|message| matches!(message.request, Request::Operator(OperatorRequest::Open)))
    );
    assert!(dashboard.key(KeyCode::Esc) != DashboardAction::Detach);
}

#[test]
fn operator_projects_viewport_keeps_the_last_selection_at_the_bottom() {
    let mut hierarchy = fixture_hierarchy();
    let mut project = hierarchy.projects.pop().unwrap();
    let mut workspace = project.workspaces.pop().unwrap();
    let template = workspace.sessions[0].clone();
    workspace.sessions = (1..=30)
        .map(|id| {
            let mut session = template.clone();
            session.id = SessionId(id);
            session.name = format!("viewport-session-{id}");
            session
        })
        .collect();
    project.workspaces = vec![workspace];
    hierarchy.projects = vec![project];
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 24,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 24));
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(30));

    let assert_bottom = |dashboard: &Dashboard| {
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal
            .draw(|f| draw_dashboard_at(f, dashboard, 0))
            .unwrap();
        let selected: Vec<_> = (0..24)
            .filter(|y| terminal.backend().buffer()[(0, *y)].symbol() == "▌")
            .collect();
        let rows = rendered_rows(dashboard, 120, 24);
        let last_session_row = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.contains("viewport-session-"))
            .map(|(row, _)| row)
            .max()
            .expect("the filled tree viewport shows a session row");
        assert!(
            rows[last_session_row].contains("viewport-session-30"),
            "the last selected session remains visible at the bottom of the tree: {rows:?}"
        );
        assert_eq!(
            selected,
            vec![u16::try_from(last_session_row).unwrap()],
            "the selected terminal must remain at the filled tree viewport's bottom"
        );
        assert_eq!(dashboard.focused_session(), Some(SessionId(30)));
    };
    assert_bottom(&dashboard);

    // Opening the conversation, changing activity and hiding it never move the tree.
    open_and_settle(&mut dashboard, busy());
    dashboard.install_focus(SessionId(30));
    assert_bottom(&dashboard);
    dashboard.ctrl('g');
    assert_bottom(&dashboard);
    // A legacy row identifier never inserts a conversation section row.
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(
        OperatorConversationId(99),
    )));
    assert_bottom(&dashboard);
}

fn ended(state: OperatorTurnState) -> OperatorConversationSummary {
    let mut value = summary();
    value.run = SessionRunId(2);
    value.current_turn = Some(turn(2, state));
    value
}

fn feed_call(turn: u64, ordinal: u64, id: &str, command: &str) -> OperatorMessage {
    turn_message(
        turn,
        ordinal,
        OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
            id: id.into(),
            name: "bash".into(),
            arguments: format!("{{\"command\":\"{command}\",\"timeout\":30}}"),
        }),
    )
}

fn feed_output(turn: u64, ordinal: u64, id: &str, output: &str) -> OperatorMessage {
    turn_message(
        turn,
        ordinal,
        OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
            id: id.into(),
            output: output.into(),
            truncated: false,
            status: OperatorToolStatus::Succeeded,
        }),
    )
}

/// A request on turn 2 that has thought, looked something up and started a change.
fn request_journal() -> Vec<OperatorMessage> {
    vec![
        turn_message(
            2,
            1,
            OperatorMessageContent::User {
                skill: None,
                text: "set it up".into(),
                context: OperatorContext::default(),
            },
        ),
        turn_message(
            2,
            2,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(
                "PRIVATE_PLAN first\nPRIVATE_PLAN latest".into(),
            )),
        ),
        feed_call(2, 3, "look", "ovrcr list"),
        feed_output(2, 4, "look", "LOOKUP_OUTPUT"),
        feed_call(2, 5, "make", "ovrcr workspace create ovrcr issue-9"),
        turn_message(
            2,
            6,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(
                "waiting on the create\n".into(),
            )),
        ),
    ]
}

fn request_finish() -> Vec<OperatorMessage> {
    vec![
        feed_output(2, 7, "make", "CREATED_OUTPUT"),
        turn_message(
            2,
            8,
            OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta("Done.".into())),
        ),
        turn_message(
            2,
            9,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                leased: false,
                pi_first_entry_id: None,
                pi_leaf_entry_id: None,
            }),
        ),
    ]
}

fn running_request() -> OperatorConversationSummary {
    let mut running = summary();
    running.run = SessionRunId(2);
    running.worker_stopped = false;
    running.current_turn = Some(turn(2, OperatorTurnState::Running));
    running
}

fn row_of(screen: &str, needle: &str) -> usize {
    screen
        .lines()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle:?}: {screen}"))
}

#[test]
fn a_running_request_lists_its_commands_and_one_live_thought_then_hides_thinking() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let out = opened(&mut dashboard, running_request());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(running_request(), request_journal(), None),
    );
    assert!(out.is_empty(), "{out:?}");
    let screen = draw_text(&mut dashboard, 120, 40);
    let request = row_of(&screen, "▌ ● set it up");
    let lookup = row_of(&screen, "▌     ovrcr list");
    let change = row_of(&screen, "▌   + ovrcr workspace create ovrcr issue-9");
    let thought = row_of(&screen, "▌     waiting on the create");
    assert!(
        request < lookup && lookup < change && change < thought,
        "visible order: {screen}"
    );
    assert!(
        screen.lines().nth(change).unwrap().contains("running"),
        "the unanswered command is running: {screen}"
    );
    assert!(
        !screen.contains("PRIVATE_PLAN"),
        "only the trailing thought is shown: {screen}"
    );
    assert!(
        !screen.contains("LOOKUP_OUTPUT"),
        "a command row does not show its output: {screen}"
    );

    // The finished request drops the thought and keeps commands above the reply.
    let out = changed(
        &mut dashboard,
        ended(OperatorTurnState::Complete),
        request_finish(),
    );
    assert!(out.is_empty(), "{out:?}");
    let screen = draw_text(&mut dashboard, 120, 40);
    let request = row_of(&screen, "▌ ✓ set it up");
    let change = row_of(&screen, "▌   + ovrcr workspace create ovrcr issue-9");
    let reply = row_of(&screen, "▌   Done.");
    assert!(request < change && change < reply, "{screen}");
    assert!(!screen.contains("waiting on the create"), "{screen}");
    assert!(!screen.contains("running"), "{screen}");
}

#[test]
fn entry_and_command_choices_toggle_by_keyboard_and_mouse_and_survive_reopen() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let out = opened(&mut dashboard, running_request());
    let read = read_request(&out);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(running_request(), request_journal(), None),
    );

    // Tab focuses the transcript, End selects the newest command, Enter opens it.
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::End);
    dashboard.key(KeyCode::Enter);
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("▌       $ ovrcr workspace create ovrcr issue-9"),
        "{screen}"
    );
    // The choice outlives the command's result and the turn ending.
    let out = changed(
        &mut dashboard,
        ended(OperatorTurnState::Complete),
        request_finish(),
    );
    assert!(out.is_empty(), "{out:?}");
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("▌       CREATED_OUTPUT"), "{screen}");

    // A click on the lookup's row opens that command and no other.
    let row = row_of(&screen, "▌     ovrcr list") as u16;
    dashboard.mouse_action(mouse(48, row), Rect::new(0, 0, 120, 40));
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("▌       LOOKUP_OUTPUT"), "{screen}");
    assert!(screen.contains("▌       CREATED_OUTPUT"), "{screen}");
    // A click on its output closes it again.
    let row = row_of(&screen, "LOOKUP_OUTPUT") as u16;
    dashboard.mouse_action(mouse(48, row), Rect::new(0, 0, 120, 40));
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(!screen.contains("LOOKUP_OUTPUT"), "{screen}");

    // Enter on the request collapses it to one row that names its outcome.
    dashboard.key(KeyCode::Home);
    dashboard.key(KeyCode::Enter);
    let screen = draw_text(&mut dashboard, 120, 40);
    let header = screen
        .lines()
        .nth(row_of(&screen, "▌ ✓ set it up"))
        .unwrap();
    assert!(header.contains("1 change · 2 commands"), "{screen}");
    assert!(!screen.contains("▌   Done."), "{screen}");
    assert!(!screen.contains("CREATED_OUTPUT"), "{screen}");

    // Reopening goes through Browse, which is the only route that sends Open.
    dashboard.key(KeyCode::Esc);
    let out = opened(&mut dashboard, ended(OperatorTurnState::Complete));
    assert!(out.is_empty(), "reopening reads no new history: {out:?}");
    let reopened = draw_text(&mut dashboard, 120, 40);
    assert!(
        !reopened.contains("▌   Done."),
        "reopening keeps the collapsed request: {reopened}"
    );
    // Opening it again still shows the command that was left expanded.
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::Home);
    dashboard.key(KeyCode::Enter);
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("▌   Done."), "{screen}");
    assert!(screen.contains("▌       CREATED_OUTPUT"), "{screen}");
}

#[test]
fn older_requests_collapse_when_a_new_one_starts_and_narrow_rows_stay_clipped() {
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let mut journal = request_journal();
    journal.extend(request_finish());
    journal.push(turn_message(
        3,
        10,
        OperatorMessageContent::User {
            skill: None,
            text: "and a second request with a long sentence that cannot fit a narrow pane".into(),
            context: OperatorContext::default(),
        },
    ));
    journal.push(turn_message(
        3,
        11,
        OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
            "A reply whose words wrap at spaces instead of splitting in the middle.".into(),
        )),
    ));
    let mut value = summary();
    value.run = SessionRunId(2);
    let out = opened(&mut dashboard, value.clone());
    let read = read_request(&out);
    respond(
        &mut dashboard,
        read.request_id,
        history_of(value, journal, None),
    );
    // The reply to the first request was never drawn at the tail, so it stays
    // open until this draw reviews it; the next draw collapses it.
    let _ = reviewed_screen(&mut dashboard, 120, 40);
    let screen = draw_text(&mut dashboard, 120, 40);
    let first = screen
        .lines()
        .nth(row_of(&screen, "▌ ✓ set it up"))
        .unwrap();
    assert!(first.contains("1 change · 2 commands"), "{screen}");
    assert!(!screen.contains("▌   Done."), "{screen}");
    assert!(screen.contains("▌   A reply whose words wrap"), "{screen}");

    // At the 60-column floor the pane is 60 cells: rows clip or wrap inside it.
    let narrow = draw_text(&mut dashboard, 60, 24);
    for row in narrow.lines().filter(|row| row.contains('▌')) {
        assert!(row.chars().count() <= 60, "{row:?}");
    }
    assert!(
        narrow.contains("▌   A reply whose words wrap at spaces instead of splitting"),
        "{narrow}"
    );
    assert!(narrow.contains("▌   in the middle."), "{narrow}");
}

#[test]
fn compaction_activity_is_transient_and_scoped_to_the_running_turn() {
    if crate::pi_available_test::run_with_pi_available(
        "operator::compaction_activity_is_transient_and_scoped_to_the_running_turn",
    ) {
        return;
    }
    let context = OperatorContext::default();
    let mut running = summary();
    running.run = SessionRunId(2);
    running.worker_stopped = false;
    running.current_turn = Some(turn(2, OperatorTurnState::Running));
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));

    // Reopening during compaction derives the phase from durable history alone.
    let out = opened(&mut dashboard, running.clone());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(
            running.clone(),
            vec![
                turn_message(
                    2,
                    1,
                    OperatorMessageContent::User {
                        skill: None,
                        text: "long task".into(),
                        context: context.clone(),
                    },
                ),
                turn_message(
                    2,
                    2,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted),
                ),
            ],
            None,
        ),
    );
    assert!(out.is_empty(), "{out:?}");
    let rows = rendered_rows(&dashboard, 120, 40);
    let screen = rows.join("\n");
    let transcript = rows
        .iter()
        .map(|row| row.split_once('│').map_or("", |(_, right)| right))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(dock_row(&rows).contains("Responding"), "{screen}");
    assert!(transcript.contains("Compacting context…"), "{transcript}");
    assert!(!transcript.contains("Responding"), "{transcript}");
    assert!(!screen.contains("Compaction"), "no permanent row: {screen}");

    // Catch-up after the native finish returns the ordinary derived activity.
    let out = changed(
        &mut dashboard,
        running.clone(),
        vec![turn_message(
            2,
            3,
            OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionEnded),
        )],
    );
    assert!(out.is_empty(), "{out:?}");
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(!screen.contains("Compacting context…"), "{screen}");
    assert!(screen.contains("Responding"), "{screen}");

    // Completed work is terminal even when history retains the start event.
    let mut complete = running.clone();
    complete.worker_stopped = true;
    complete.current_turn = Some(turn(2, OperatorTurnState::Complete));
    changed(
        &mut dashboard,
        complete,
        vec![turn_message(
            2,
            4,
            OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted),
        )],
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(!screen.contains("Compacting context…"), "{screen}");

    // Failure is terminal even when history retains the start event.
    let mut failed = running.clone();
    failed.worker_stopped = true;
    failed.current_turn = Some(turn(2, OperatorTurnState::Error));
    changed(
        &mut dashboard,
        failed,
        vec![turn_message(
            2,
            5,
            OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted),
        )],
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(!screen.contains("Compacting context…"), "{screen}");
    assert!(screen.contains("error"), "{screen}");

    // Interrupted/Stop likewise cannot leave a stale phase.
    let mut interrupted = running.clone();
    interrupted.worker_stopped = true;
    interrupted.current_turn = Some(turn(2, OperatorTurnState::Interrupted));
    changed(
        &mut dashboard,
        interrupted,
        vec![turn_message(
            2,
            6,
            OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted),
        )],
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(!screen.contains("Compacting context…"), "{screen}");

    // A later running turn ignores an earlier turn's unfinished compaction.
    let mut next = running;
    next.run = SessionRunId(3);
    next.worker_stopped = false;
    next.current_turn = Some(turn(3, OperatorTurnState::Running));
    changed(
        &mut dashboard,
        next,
        vec![
            turn_message(
                2,
                7,
                OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted),
            ),
            turn_message(
                3,
                8,
                OperatorMessageContent::User {
                    skill: None,
                    text: "next task".into(),
                    context,
                },
            ),
        ],
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(!screen.contains("Compacting context…"), "{screen}");
    assert!(screen.contains("Responding"), "{screen}");
}

#[test]
fn unknown_single_segment_slash_commands_fail_locally_while_paths_and_leading_spaces_submit() {
    // 1. Cold Quick message rejects unknown single-segment /commands before even sending Open.
    let mut cold = dashboard_fixture();
    cold.ctrl('o');
    assert!(cold.drain_outbox().is_empty());
    cold.event_action(Event::Paste("/compcat".into()));
    cold.key(KeyCode::Enter);
    assert!(
        cold.drain_outbox().is_empty(),
        "mistyped /command must never open or send to the model"
    );
    let quick_screen = draw_text(&mut cold, 120, 30);
    assert!(
        quick_screen.contains("Quick message · Ctrl-o"),
        "{quick_screen}"
    );
    assert!(quick_screen.contains("/compcat"), "{quick_screen}");
    assert!(
        quick_screen.to_lowercase().contains("unknown") && quick_screen.contains("/copy"),
        "expected inline unknown-command error in Quick message: {quick_screen}"
    );

    // Bare `/` in Quick message also fails locally while preserving `/`.
    cold.ctrl('c');
    cold.event_action(Event::Paste("/".into()));
    cold.key(KeyCode::Enter);
    assert!(cold.drain_outbox().is_empty());
    let bare_screen = draw_text(&mut cold, 120, 30);
    assert!(
        bare_screen.contains("Quick message · Ctrl-o"),
        "{bare_screen}"
    );
    assert!(
        bare_screen.to_lowercase().contains("unknown"),
        "{bare_screen}"
    );

    // 2. Full Operator rejects unknown single-segment /commands locally with draft preserved.
    let mut dashboard = dashboard_fixture();
    open_and_settle(&mut dashboard, summary());
    dashboard.event_action(Event::Paste("/compcat focus".into()));
    dashboard.key(KeyCode::Enter);
    assert!(
        dashboard.drain_outbox().is_empty(),
        "unknown single-segment /command must not enqueue Submit"
    );
    let screen = draw_text(&mut dashboard, 120, 30);
    assert!(
        screen.contains("/compcat focus"),
        "composer draft must be preserved: {screen}"
    );
    assert!(
        screen.to_lowercase().contains("unknown") && screen.contains("/thinking"),
        "expected local error notice in Operator: {screen}"
    );

    // 3. Leading whitespace (` /copy`) passes through as an ordinary user prompt.
    dashboard.ctrl('c');
    dashboard.event_action(Event::Paste(" /copy".into()));
    dashboard.key(KeyCode::Enter);
    let out = dashboard.drain_outbox();
    assert!(
        out.iter().any(|msg| matches!(
            &msg.request,
            Request::Operator(OperatorRequest::Submit { text, .. }) if text == " /copy"
        )),
        "leading-space draft must submit as normal prompt: {out:?}"
    );

    // 4. Multi-segment Unix paths (`/etc/hosts ...` and `/Users/alice/repo`) submit as normal prompts.
    let mut path_dashboard = dashboard_fixture();
    open_and_settle(&mut path_dashboard, summary());
    path_dashboard.event_action(Event::Paste("/etc/hosts is missing an entry".into()));
    path_dashboard.key(KeyCode::Enter);
    let out = path_dashboard.drain_outbox();
    assert!(
        out.iter().any(|msg| matches!(
            &msg.request,
            Request::Operator(OperatorRequest::Submit { text, .. })
                if text == "/etc/hosts is missing an entry"
        )),
        "multi-segment path in composer must submit as normal prompt: {out:?}"
    );

    let mut quick_path = dashboard_fixture();
    open_and_settle(&mut quick_path, summary());
    quick_path.key(KeyCode::Esc);
    quick_path.drain_outbox();
    quick_path.ctrl('o');
    quick_path.event_action(Event::Paste("/Users/alice/repo".into()));
    quick_path.key(KeyCode::Enter);
    let out = quick_path.drain_outbox();
    assert!(
        out.iter().any(|msg| matches!(
            &msg.request,
            Request::Operator(OperatorRequest::Submit { text, .. })
                if text == "/Users/alice/repo"
        )),
        "multi-segment path in Quick message must submit as normal prompt: {out:?}"
    );
}

#[test]
fn composer_slash_menu_filters_completes_and_dismisses_while_quick_stays_compact() {
    let mut dashboard = dashboard_fixture();
    open_and_settle(&mut dashboard, with_model(summary()));

    // Typing `/` opens the slash menu above the composer rule showing `/copy` and `/thinking`.
    dashboard.key(KeyCode::Char('/'));
    let rows = rendered_rows(&dashboard, 120, 30);
    let screen = rows.join("\n");
    assert!(
        screen.contains("Copy latest assistant reply Markdown"),
        "{screen}"
    );
    assert!(
        screen.contains("Choose or set reasoning effort level"),
        "{screen}"
    );
    let menu_row = rows
        .iter()
        .position(|r| r.contains("Copy latest assistant reply Markdown"))
        .expect("slash menu row present");
    let rule_row = rows
        .iter()
        .rposition(|r| r.contains("────────"))
        .expect("composer rule present");
    // There are two rules (top_rule and bottom_rule); both are below the popup.
    let top_rule_row = rows[..rule_row]
        .iter()
        .rposition(|r| r.contains("────────"))
        .expect("top composer rule present");
    assert!(
        menu_row < top_rule_row,
        "slash popup ({menu_row}) must appear above composer top rule ({top_rule_row}):\n{screen}"
    );

    // Case-insensitive live filtering: `/CO` matches `/copy` and hides `/thinking`.
    dashboard.key(KeyCode::Char('C'));
    dashboard.key(KeyCode::Char('O'));
    let filtered = draw_text(&mut dashboard, 120, 30);
    assert!(
        filtered.contains("Copy latest assistant reply Markdown"),
        "{filtered}"
    );
    assert!(
        !filtered.contains("Choose or set reasoning effort level"),
        "{filtered}"
    );

    // Backspacing back to `/` restores both entries; Down + Tab completes `/thinking ` with space.
    dashboard.key(KeyCode::Backspace);
    dashboard.key(KeyCode::Backspace);
    let restored = draw_text(&mut dashboard, 120, 30);
    assert!(
        restored.contains("Choose or set reasoning effort level"),
        "{restored}"
    );
    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Tab);
    let after_thinking_tab = draw_text(&mut dashboard, 120, 30);
    assert!(
        !after_thinking_tab.contains("Choose or set reasoning effort level"),
        "menu closes after Tab completion with trailing space:\n{after_thinking_tab}"
    );
    assert!(
        after_thinking_tab.contains("> /thinking ▏"),
        "expected `/thinking ` with trailing space before caret:\n{after_thinking_tab}"
    );

    // Zero-arg `/copy` completes via Tab without trailing space and dismisses the menu.
    dashboard.ctrl('c');
    dashboard.key(KeyCode::Char('/'));
    dashboard.key(KeyCode::Up);
    dashboard.key(KeyCode::Tab);
    let after_copy_tab = draw_text(&mut dashboard, 120, 30);
    assert!(
        !after_copy_tab.contains("Copy latest assistant reply Markdown"),
        "menu closes after Tab completion:\n{after_copy_tab}"
    );
    assert!(
        after_copy_tab.contains("> /copy▏"),
        "expected `/copy` with no trailing space:\n{after_copy_tab}"
    );

    // Esc dismisses only the `/` menu while keeping draft and staying in Operator;
    // a second Esc leaves Operator for Browse.
    dashboard.ctrl('c');
    dashboard.event_action(Event::Paste("/t".into()));
    let open_menu = draw_text(&mut dashboard, 120, 30);
    assert!(
        open_menu.contains("Choose or set reasoning effort level"),
        "{open_menu}"
    );
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::Redraw);
    let dismissed = draw_text(&mut dashboard, 120, 30);
    assert!(
        !dismissed.contains("Choose or set reasoning effort level"),
        "Esc must dismiss the `/` menu:\n{dismissed}"
    );
    assert!(
        dismissed.contains("Operator · pi · openai/gpt-6.1-sol"),
        "first Esc must stay inside Operator:\n{dismissed}"
    );
    assert!(
        dismissed.contains("> /t▏"),
        "draft must be retained on menu dismissal:\n{dismissed}"
    );
    assert_eq!(dashboard.key(KeyCode::Esc), DashboardAction::EnterBrowse);

    // Quick message (`Ctrl-o`) never renders the `/` popup while typing.
    dashboard.drain_outbox();
    dashboard.ctrl('o');
    dashboard.ctrl('c');
    dashboard.key(KeyCode::Char('/'));
    let quick_slash = draw_text(&mut dashboard, 120, 30);
    assert!(
        quick_slash.contains("Quick message · Ctrl-o"),
        "{quick_slash}"
    );
    assert!(
        !quick_slash.contains("Copy latest assistant reply Markdown")
            && !quick_slash.contains("Choose or set reasoning effort level"),
        "Quick message must never render the `/` popup:\n{quick_slash}"
    );
    dashboard.key(KeyCode::Char('c'));
    dashboard.key(KeyCode::Char('o'));
    let quick_co = draw_text(&mut dashboard, 120, 30);
    assert!(
        !quick_co.contains("Copy latest assistant reply Markdown"),
        "Quick message must stay compact while typing `/co`:\n{quick_co}"
    );
}

#[test]
fn slash_copy_emits_raw_markdown_osc52_and_reports_empty_or_oversized_replies() {
    // 1. Empty conversation: `/copy` reports a clear notice in both composer and Quick message.
    let mut empty = dashboard_fixture();
    open_and_settle(&mut empty, summary());
    empty.event_action(Event::Paste("/copy".into()));
    assert_eq!(empty.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(empty.drain_outbox().is_empty());
    let empty_screen = draw_text(&mut empty, 120, 30);
    assert!(
        empty_screen
            .to_lowercase()
            .contains("no assistant reply to copy yet"),
        "expected empty-reply notice in Operator:\n{empty_screen}"
    );

    empty.key(KeyCode::Esc);
    empty.drain_outbox();
    empty.ctrl('o');
    empty.event_action(Event::Paste("/copy".into()));
    assert_eq!(empty.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(empty.drain_outbox().is_empty());
    let empty_quick = draw_text(&mut empty, 120, 30);
    assert!(
        empty_quick.contains("Quick message · Ctrl-o"),
        "{empty_quick}"
    );
    assert!(
        empty_quick
            .to_lowercase()
            .contains("no assistant reply to copy yet"),
        "expected empty-reply notice in Quick message:\n{empty_quick}"
    );

    // 2. Unexpected arguments (`/copy extra`) fail locally preserving draft.
    empty.ctrl('c');
    empty.event_action(Event::Paste("/copy extra".into()));
    assert_eq!(empty.key(KeyCode::Enter), DashboardAction::Redraw);
    let bad_args = draw_text(&mut empty, 120, 30);
    assert!(bad_args.contains("/copy extra"), "{bad_args}");
    assert!(
        bad_args.to_lowercase().contains("no arguments"),
        "{bad_args}"
    );

    // 3. Latest visible assistant reply raw Markdown (even when a later turn has no reply yet).
    let raw_md = "# Deployment Plan\n\n```sh\novrcr list\n```\n\n- **step 1**";
    let mut d = dashboard_fixture();
    let mut done = with_model(summary());
    done.run = SessionRunId(3);
    done.current_turn = Some(turn(3, OperatorTurnState::Interrupted));
    let out = opened(&mut d, done.clone());
    let read = read_request(&out);
    respond(
        &mut d,
        read.request_id,
        history_of(
            done,
            vec![
                OperatorMessage {
                    ordinal: 1,
                    turn: OperatorTurnId(1),
                    content: OperatorMessageContent::User {
                        skill: None,
                        text: "first".into(),
                        context: OperatorContext::default(),
                    },
                },
                OperatorMessage {
                    ordinal: 2,
                    turn: OperatorTurnId(1),
                    content: OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                        "# Old Reply\n\nShould not be copied.".into(),
                    )),
                },
                OperatorMessage {
                    ordinal: 3,
                    turn: OperatorTurnId(1),
                    content: OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                        leased: false,
                        pi_first_entry_id: None,
                        pi_leaf_entry_id: None,
                    }),
                },
                OperatorMessage {
                    ordinal: 4,
                    turn: OperatorTurnId(2),
                    content: OperatorMessageContent::User {
                        skill: None,
                        text: "second".into(),
                        context: OperatorContext::default(),
                    },
                },
                OperatorMessage {
                    ordinal: 5,
                    turn: OperatorTurnId(2),
                    content: OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                        raw_md.into(),
                    )),
                },
                OperatorMessage {
                    ordinal: 6,
                    turn: OperatorTurnId(2),
                    content: OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                        leased: false,
                        pi_first_entry_id: None,
                        pi_leaf_entry_id: None,
                    }),
                },
                OperatorMessage {
                    ordinal: 7,
                    turn: OperatorTurnId(3),
                    content: OperatorMessageContent::User {
                        skill: None,
                        text: "third interrupted".into(),
                        context: OperatorContext::default(),
                    },
                },
                OperatorMessage {
                    ordinal: 8,
                    turn: OperatorTurnId(3),
                    content: OperatorMessageContent::Interrupted,
                },
            ],
            None,
        ),
    );

    // Run `/copy` directly from the `/` menu via Enter (`/` highlights `/copy` first).
    d.key(KeyCode::Char('/'));
    let action = d.key(KeyCode::Enter);
    assert_eq!(action, DashboardAction::CopyText(raw_md.into()));
    assert!(d.drain_outbox().is_empty());
    let mut osc52 = Vec::new();
    ovrcr::tui::write_clipboard(&mut osc52, raw_md).unwrap();
    let osc52_str = String::from_utf8(osc52).unwrap();
    assert!(
        osc52_str.starts_with("\x1b]52;c;"),
        "expected OSC 52 clipboard sequence: {osc52_str:?}"
    );
    let copied_screen = draw_text(&mut d, 120, 30);
    assert!(
        copied_screen.to_lowercase().contains("copied"),
        "expected copy confirmation in Operator:\n{copied_screen}"
    );
    assert!(
        copied_screen.contains("> ▏"),
        "composer draft must be cleared after `/copy`:\n{copied_screen}"
    );

    // Quick message (`Ctrl-o`) also executes `/copy` in place, closes Quick, and returns CopyText.
    d.key(KeyCode::Esc);
    d.drain_outbox();
    d.ctrl('o');
    d.event_action(Event::Paste("/copy".into()));
    assert_eq!(
        d.key(KeyCode::Enter),
        DashboardAction::CopyText(raw_md.into())
    );
    assert!(d.drain_outbox().is_empty());
    let browse_after_quick_copy = draw_text(&mut d, 120, 30);
    assert!(
        !browse_after_quick_copy.contains("Quick message · Ctrl-o"),
        "Quick message closes on successful `/copy`:\n{browse_after_quick_copy}"
    );
    assert!(
        browse_after_quick_copy.to_lowercase().contains("copied"),
        "expected copy confirmation in Browse footer:\n{browse_after_quick_copy}"
    );

    // 4. Exact 64 KiB (65,536 bytes) succeeds; 65,537 bytes fails with a clear >64 KiB notice.
    let exact_max = "m".repeat(65_536);
    let mut max_ok = dashboard_fixture();
    let out = opened(&mut max_ok, summary());
    let read = read_request(&out);
    respond(
        &mut max_ok,
        read.request_id,
        history_of(
            summary(),
            vec![
                user(1, "big", OperatorContext::default()),
                delta(2, &exact_max),
                completed(3),
            ],
            None,
        ),
    );
    max_ok.event_action(Event::Paste("/copy".into()));
    assert_eq!(
        max_ok.key(KeyCode::Enter),
        DashboardAction::CopyText(exact_max)
    );

    let too_big = "m".repeat(65_537);
    let mut oversized = dashboard_fixture();
    let out = opened(&mut oversized, summary());
    let read = read_request(&out);
    respond(
        &mut oversized,
        read.request_id,
        history_of(
            summary(),
            vec![
                user(1, "too big", OperatorContext::default()),
                delta(2, &too_big),
                completed(3),
            ],
            None,
        ),
    );
    oversized.event_action(Event::Paste("/copy".into()));
    assert_eq!(oversized.key(KeyCode::Enter), DashboardAction::Redraw);
    let over_screen = draw_text(&mut oversized, 120, 30);
    assert!(
        over_screen.contains("64 KiB"),
        "expected >64 KiB notice in Operator:\n{over_screen}"
    );
    oversized.key(KeyCode::Esc);
    oversized.drain_outbox();
    oversized.ctrl('o');
    oversized.event_action(Event::Paste("/copy".into()));
    assert_eq!(oversized.key(KeyCode::Enter), DashboardAction::Redraw);
    let over_quick = draw_text(&mut oversized, 120, 30);
    assert!(over_quick.contains("64 KiB"), "{over_quick}");
}

#[test]
fn slash_thinking_opens_picker_validates_capabilities_and_writes_settings_across_entry_points() {
    // 1. Cold Quick message `/thinking` (no args) closes Quick, opens full Operator,
    //    and opens ReasoningPicker with full capabilities once Opened arrives.
    let mut cold = dashboard_fixture();
    cold.ctrl('o');
    cold.event_action(Event::Paste("/thinking".into()));
    cold.key(KeyCode::Enter);
    let open_msgs = cold.drain_outbox();
    let open_req = open_msgs
        .iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Open)))
        .expect("Quick `/thinking` opens Operator");
    assert!(
        !open_msgs
            .iter()
            .any(|m| matches!(m.request, Request::Operator(OperatorRequest::Submit { .. }))),
        "must never submit `/thinking` as a prompt: {open_msgs:?}"
    );
    let out = respond(
        &mut cold,
        open_req.request_id,
        OperatorResponse::Opened(reasoning_summary(false)),
    );
    let read = read_request(&out);
    respond(
        &mut cold,
        read.request_id,
        history_of(reasoning_summary(false), Vec::new(), None),
    );
    let cold_screen = draw_text(&mut cold, 140, 35);
    assert!(
        !cold_screen.contains("Quick message · Ctrl-o"),
        "Quick message must be closed:\n{cold_screen}"
    );
    assert!(
        cold_screen.contains("Operator · pi · openai/gpt-6.1-sol"),
        "full Operator must be open:\n{cold_screen}"
    );
    assert!(
        cold_screen.contains("default (Pi's choice)")
            && cold_screen.contains("not offered: off, xhigh, max")
            && cold_screen.contains("Enter pick"),
        "ReasoningPicker must be open with model capabilities:\n{cold_screen}"
    );
    cold.key(KeyCode::Esc);
    let after_picker_esc = draw_text(&mut cold, 140, 35);
    assert!(
        !after_picker_esc.contains("Enter pick") && after_picker_esc.contains("> ▏"),
        "Quick draft was cleared and composer is clean:\n{after_picker_esc}"
    );

    // 2. Before capabilities are known (`reasoning: None`), all 8 levels match case-insensitively,
    //    while invalid names fail locally listing all 8 valid choices with draft preserved.
    let mut unknown_caps = dashboard_fixture();
    open_and_settle(&mut unknown_caps, summary());
    for (input, expected) in [
        ("/thinking DEFAULT", "\"default\""),
        ("/thinking Off", "\"off\""),
        ("/thinking MINIMAL", "\"minimal\""),
        ("/thinking low", "\"low\""),
        ("/thinking Medium", "\"medium\""),
        ("/thinking HIGH", "\"high\""),
        ("/thinking xHigh", "\"xhigh\""),
        ("/thinking MAX", "\"max\""),
    ] {
        unknown_caps.event_action(Event::Paste(input.into()));
        unknown_caps.key(KeyCode::Enter);
        let out = unknown_caps.drain_outbox();
        assert!(
            out.iter().any(|m| matches!(
                &m.request,
                Request::SetSetting { path, value: Some(v) }
                    if path == "operator.thinking_level" && v == expected
            )),
            "expected SetSetting {expected} for `{input}`, got {out:?}"
        );
    }
    unknown_caps.event_action(Event::Paste("/thinking turbo".into()));
    unknown_caps.key(KeyCode::Enter);
    assert!(unknown_caps.drain_outbox().is_empty());
    let invalid_screen = draw_text(&mut unknown_caps, 140, 30);
    assert!(
        invalid_screen.contains("> /thinking turbo▏"),
        "{invalid_screen}"
    );
    for choice in [
        "default", "off", "minimal", "low", "medium", "high", "xhigh", "max",
    ] {
        assert!(
            invalid_screen.contains(choice),
            "expected `{choice}` listed in error:\n{invalid_screen}"
        );
    }

    // 3. When model capabilities are known (`minimal, low, medium, high`), even during a running turn:
    //    - `/th` + Enter in `/` menu opens ReasoningPicker
    //    - `/thinking medium` and `/thinking DEFAULT` write SetSetting (in composer and Quick)
    //    - `/thinking max` and `/thinking off` fail locally listing supported choices and keeping draft
    let mut active = dashboard_fixture();
    open_and_settle(&mut active, reasoning_summary(true));

    active.event_action(Event::Paste("/th".into()));
    active.key(KeyCode::Enter);
    let picker_screen = draw_text(&mut active, 140, 35);
    assert!(picker_screen.contains("Enter pick"), "{picker_screen}");
    active.key(KeyCode::Esc);

    active.event_action(Event::Paste("/thinking max".into()));
    active.key(KeyCode::Enter);
    assert!(active.drain_outbox().is_empty());
    let unsupported_screen = draw_text(&mut active, 140, 30);
    assert!(
        unsupported_screen.contains("> /thinking max▏"),
        "draft must be preserved on unsupported level:\n{unsupported_screen}"
    );
    assert!(
        unsupported_screen.contains("default, minimal, low, medium, high"),
        "expected supported choices listed:\n{unsupported_screen}"
    );

    active.ctrl('c');
    active.event_action(Event::Paste("/thinking MEDIUM".into()));
    active.key(KeyCode::Enter);
    let out = active.drain_outbox();
    assert!(
        out.iter().any(|m| matches!(
            &m.request,
            Request::SetSetting { path, value: Some(v) }
                if path == "operator.thinking_level" && v == "\"medium\""
        )),
        "supported level writes SetSetting even while turn is running: {out:?}"
    );

    active.event_action(Event::Paste("/thinking DEFAULT".into()));
    active.key(KeyCode::Enter);
    let out = active.drain_outbox();
    assert!(
        out.iter().any(|m| matches!(
            &m.request,
            Request::SetSetting { path, value: Some(v) }
                if path == "operator.thinking_level" && v == "\"default\""
        )),
        "`default` is always allowed even when capabilities omit Default: {out:?}"
    );

    // Quick message (`Ctrl-o`) capability check and direct SetSetting execution.
    active.key(KeyCode::Esc);
    active.drain_outbox();
    active.ctrl('o');
    active.event_action(Event::Paste("/thinking OFF".into()));
    active.key(KeyCode::Enter);
    assert!(active.drain_outbox().is_empty());
    let quick_unsupported = draw_text(&mut active, 140, 30);
    assert!(
        quick_unsupported.contains("Quick message · Ctrl-o")
            && quick_unsupported.contains("/thinking OFF")
            && quick_unsupported.contains("default, minimal, low, medium, high"),
        "Quick message must reject model-unsupported level inline with draft preserved:\n{quick_unsupported}"
    );

    active.ctrl('c');
    active.event_action(Event::Paste("/thinking low".into()));
    active.key(KeyCode::Enter);
    let out = active.drain_outbox();
    assert!(
        out.iter().any(|m| matches!(
            &m.request,
            Request::SetSetting { path, value: Some(v) }
                if path == "operator.thinking_level" && v == "\"low\""
        )),
        "Quick message valid `/thinking low` writes SetSetting directly: {out:?}"
    );
    let after_quick_set = draw_text(&mut active, 140, 30);
    assert!(
        !after_quick_set.contains("Quick message · Ctrl-o"),
        "Quick message closes after valid `/thinking low`:\n{after_quick_set}"
    );
}

#[test]
fn slash_menu_lists_and_completes_new_and_session_and_rejects_extra_args() {
    let mut d = dashboard_fixture();
    open_and_settle(&mut d, with_model(summary()));

    d.key(KeyCode::Char('/'));
    let menu = draw_text(&mut d, 120, 30);
    for entry in ["/copy", "/thinking", "/model", "/new", "/session"] {
        assert!(
            menu.contains(entry),
            "expected `{entry}` in slash menu:\n{menu}"
        );
    }

    // Filter `/n` -> Tab completes `/new` with no trailing space and dismisses menu.
    d.key(KeyCode::Char('n'));
    let filtered_new = draw_text(&mut d, 120, 30);
    assert!(filtered_new.contains("/new"), "{filtered_new}");
    assert!(!filtered_new.contains("/copy"), "{filtered_new}");
    d.key(KeyCode::Tab);
    let completed_new = draw_text(&mut d, 120, 30);
    assert!(completed_new.contains("> /new▏"), "{completed_new}");

    // `/new extra` fails locally with draft preserved.
    d.event_action(Event::Paste(" extra".into()));
    assert_eq!(d.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(d.drain_outbox().is_empty());
    let new_bad_args = draw_text(&mut d, 120, 30);
    assert!(new_bad_args.contains("> /new extra▏"), "{new_bad_args}");
    assert!(
        new_bad_args.to_lowercase().contains("no arguments"),
        "{new_bad_args}"
    );

    // Filter `/s` -> Tab completes `/session` with no trailing space and dismisses menu.
    d.ctrl('c');
    d.event_action(Event::Paste("/s".into()));
    let filtered_session = draw_text(&mut d, 120, 30);
    assert!(filtered_session.contains("/session"), "{filtered_session}");
    d.key(KeyCode::Tab);
    let completed_session = draw_text(&mut d, 120, 30);
    assert!(
        completed_session.contains("> /session▏"),
        "{completed_session}"
    );

    // `/session extra` fails locally with draft preserved.
    d.event_action(Event::Paste(" extra".into()));
    assert_eq!(d.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(d.drain_outbox().is_empty());
    let session_bad_args = draw_text(&mut d, 120, 30);
    assert!(
        session_bad_args.contains("> /session extra▏"),
        "{session_bad_args}"
    );
    assert!(
        session_bad_args.to_lowercase().contains("no arguments"),
        "{session_bad_args}"
    );
}

#[test]
fn slash_new_resets_feed_epoch_and_copy_state_while_refusing_busy_or_leased_workers() {
    // 1. Busy (`Running`) and leased (`!worker_stopped || worker_live`) conversations refuse `/new`.
    let mut busy_dash = dashboard_fixture();
    open_and_settle(&mut busy_dash, busy());
    busy_dash.event_action(Event::Paste("/new".into()));
    busy_dash.key(KeyCode::Enter);
    assert!(
        busy_dash.drain_outbox().is_empty(),
        "running turn must refuse `/new` locally without sending Submit"
    );
    let busy_screen = draw_text(&mut busy_dash, 120, 30);
    assert!(busy_screen.contains("> /new▏"), "{busy_screen}");
    assert!(
        busy_screen.to_lowercase().contains("running"),
        "expected running refusal reason:\n{busy_screen}"
    );

    let mut leased_state = with_model(summary());
    leased_state.run = SessionRunId(2);
    leased_state.worker_stopped = false;
    leased_state.worker_live = true;
    leased_state.current_turn = Some(turn(2, OperatorTurnState::Complete));
    let mut leased_dash = dashboard_fixture();
    open_and_settle(&mut leased_dash, leased_state);
    leased_dash.event_action(Event::Paste("/new".into()));
    leased_dash.key(KeyCode::Enter);
    assert!(leased_dash.drain_outbox().is_empty());
    let leased_screen = draw_text(&mut leased_dash, 120, 30);
    assert!(leased_screen.contains("> /new▏"), "{leased_screen}");
    assert!(
        leased_screen.to_lowercase().contains("lease")
            || leased_screen.to_lowercase().contains("active"),
        "expected lease refusal reason:\n{leased_screen}"
    );

    // 2. Idle conversation with prior messages: `/new` sends NewSession, handles Server errors cleanly,
    //    and on Conversation(epoch 1) clears visible feed, resets model/reasoning, and clears `/copy`.
    let mut d = dashboard_fixture();
    let mut initial = with_model(summary());
    initial.run = SessionRunId(2);
    initial.current_turn = Some(turn(2, OperatorTurnState::Complete));
    let out = opened(&mut d, initial.clone());
    let read = read_request(&out);
    respond(
        &mut d,
        read.request_id,
        history_of(
            initial.clone(),
            vec![
                user(1, "epoch zero prompt", OperatorContext::default()),
                delta(2, "Epoch zero reply markdown."),
                completed(3),
            ],
            None,
        ),
    );
    let before_screen = draw_text(&mut d, 120, 30);
    assert!(
        before_screen.contains("epoch zero prompt"),
        "{before_screen}"
    );

    d.event_action(Event::Paste("/new".into()));
    d.key(KeyCode::Enter);
    let out = d.drain_outbox();
    let new_req = out
        .iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::NewSession {
                    conversation: OperatorConversationId(7),
                    expected_run: SessionRunId(2),
                })
            )
        })
        .expect("expected OperatorRequest::NewSession");

    // Simulate Server conflict error first -> draft preserved, error rendered.
    d.handle_server_message(ServerMessage::Response {
        request_id: new_req.request_id,
        response: Response::Error {
            code: ovrcr_protocol::ErrorCode::Conflict,
            message:
                "cannot start a new Operator session while a watched-session work lease is active"
                    .into(),
        },
    });
    let conflict_screen = draw_text(&mut d, 120, 30);
    assert!(conflict_screen.contains("> /new▏"), "{conflict_screen}");
    assert!(
        conflict_screen.contains("watched-session work lease"),
        "{conflict_screen}"
    );

    // Retry `/new` and succeed with epoch 1 summary.
    d.key(KeyCode::Enter);
    let out = d.drain_outbox();
    let new_req2 = out
        .iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::NewSession { .. })
            )
        })
        .expect("expected retry NewSession");
    let mut epoch1 = summary();
    epoch1.run = SessionRunId(2);
    epoch1.session_epoch = 1;
    let follow = respond(
        &mut d,
        new_req2.request_id,
        OperatorResponse::Conversation(epoch1.clone()),
    );
    if let Some(read_msg) = follow
        .iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Read { .. })))
    {
        respond(
            &mut d,
            read_msg.request_id,
            history_of(epoch1, Vec::new(), None),
        );
    }

    let after_screen = draw_text(&mut d, 120, 30);
    assert!(
        !after_screen.contains("epoch zero prompt") && !after_screen.contains("Epoch zero reply"),
        "visible feed must be cleared after `/new`:\n{after_screen}"
    );
    assert!(
        after_screen.contains("> ▏"),
        "draft must be cleared:\n{after_screen}"
    );
    assert!(
        after_screen.to_lowercase().contains("epoch 1"),
        "expected epoch 1 confirmation notice:\n{after_screen}"
    );

    // `/copy` after `/new` no longer sees epoch-0 assistant replies!
    d.event_action(Event::Paste("/copy".into()));
    assert_eq!(d.key(KeyCode::Enter), DashboardAction::Redraw);
    let copy_after_new = draw_text(&mut d, 120, 30);
    assert!(
        copy_after_new
            .to_lowercase()
            .contains("no assistant reply to copy yet"),
        "{copy_after_new}"
    );

    // 3. Cold Quick message (`Ctrl-o`) `/new` opens singleton, sends NewSession, and closes Quick.
    let mut cold = dashboard_fixture();
    cold.ctrl('o');
    cold.event_action(Event::Paste("/new".into()));
    cold.key(KeyCode::Enter);
    let open_msgs = cold.drain_outbox();
    let open_req = open_msgs
        .iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Open)))
        .expect("cold Quick `/new` sends Open first");
    let next_msgs = respond(
        &mut cold,
        open_req.request_id,
        OperatorResponse::Opened(summary()),
    );
    let quick_new = next_msgs
        .iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::NewSession { .. })
            )
        })
        .expect("Opened triggers NewSession");
    let mut reset = summary();
    reset.session_epoch = 1;
    respond(
        &mut cold,
        quick_new.request_id,
        OperatorResponse::Conversation(reset),
    );
    let quick_done = draw_text(&mut cold, 120, 30);
    assert!(
        !quick_done.contains("Quick message · Ctrl-o"),
        "Quick message closes once `/new` succeeds:\n{quick_done}"
    );
    assert!(
        quick_done.to_lowercase().contains("epoch 1"),
        "Browse notice confirms new epoch:\n{quick_done}"
    );
}

#[test]
fn slash_session_opens_modal_across_composer_and_quick_and_dismisses_with_esc_enter_or_q() {
    let mut d = dashboard_fixture();
    open_and_settle(&mut d, with_model(summary()));

    // Invoke `/session` in full Operator composer.
    d.event_action(Event::Paste("/session".into()));
    d.key(KeyCode::Enter);
    let out = d.drain_outbox();
    let session_req = out
        .iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::SessionInfo {
                    conversation: OperatorConversationId(7),
                })
            )
        })
        .expect("expected OperatorRequest::SessionInfo");

    let populated = ovrcr_protocol::OperatorSessionInfo {
        conversation: OperatorConversationId(7),
        session: SessionId(99),
        run: SessionRunId(2),
        session_epoch: 0,
        pi_session_id: Some("operator-7".into()),
        checkpoint_turn: Some(OperatorTurnId(2)),
        active_tip_turn: Some(OperatorTurnId(2)),
        configured_model: Some("anthropic/claude-sonnet-4-5".into()),
        active_model: Some(OperatorModel {
            provider: "openai".into(),
            id: "gpt-6.1-sol".into(),
        }),
        configured_thinking_level: ovrcr_protocol::OperatorThinkingLevel::High,
        reasoning: Some(ovrcr_protocol::OperatorReasoning {
            requested: ovrcr_protocol::OperatorThinkingLevel::High,
            level: ovrcr_protocol::OperatorThinkingLevel::Medium,
            available: vec![
                ovrcr_protocol::OperatorThinkingLevel::Low,
                ovrcr_protocol::OperatorThinkingLevel::Medium,
                ovrcr_protocol::OperatorThinkingLevel::High,
            ],
        }),
        turns: 3,
        completed_turns: 2,
        branch_turns: 2,
        user_messages: 3,
        assistant_messages: 2,
        tool_calls: 5,
        tool_results: 4,
        epoch_tool_calls: 5,
        epoch_tool_results: 4,
        total_messages: 9,
        epoch_total_messages: 9,
        tokens: ovrcr_protocol::OperatorTokenStats {
            input: 1_200,
            output: 350,
            cache_read: 400,
            cache_write: 50,
            total: 2_000,
        },
        cost_usd_nanos: 12_500_000,
        context_window: ovrcr_protocol::OperatorContextWindowUsage {
            used_tokens: Some(2_000),
            capacity_tokens: Some(200_000),
        },
    };
    respond(
        &mut d,
        session_req.request_id,
        OperatorResponse::SessionInfo(Box::new(populated.clone())),
    );
    let modal_screen = draw_text(&mut d, 140, 35);
    for expected in [
        "operator-7",
        "openai/gpt-6.1-sol",
        "anthropic/claude-sonnet-4-5",
        "medium",
        "1,200",
        "350",
        "400",
        "50",
        "2,000",
        "$0.0125",
        "200,000",
        "1.0%",
    ] {
        assert!(
            modal_screen.contains(expected),
            "expected `{expected}` in SessionModal:\n{modal_screen}"
        );
    }

    // Dismiss with `Esc`, `Enter`, and `q` — each closes only SessionModal while staying in Operator!
    for dismiss in [KeyCode::Esc, KeyCode::Enter, KeyCode::Char('q')] {
        if !draw_text(&mut d, 140, 35).contains("operator-7") {
            d.event_action(Event::Paste("/session".into()));
            d.key(KeyCode::Enter);
            let req = d
                .drain_outbox()
                .into_iter()
                .find(|m| {
                    matches!(
                        m.request,
                        Request::Operator(OperatorRequest::SessionInfo { .. })
                    )
                })
                .unwrap();
            respond(
                &mut d,
                req.request_id,
                OperatorResponse::SessionInfo(Box::new(populated.clone())),
            );
        }
        assert_eq!(d.key(dismiss), DashboardAction::Redraw);
        let closed = draw_text(&mut d, 140, 35);
        assert!(
            !closed.contains("$0.0125"),
            "`{dismiss:?}` must dismiss SessionModal:\n{closed}"
        );
        assert!(
            closed.contains("Operator · pi · openai/gpt-6.1-sol"),
            "`{dismiss:?}` must remain inside full Operator:\n{closed}"
        );
        assert!(closed.contains("> ▏"), "{closed}");
    }

    // Cold Quick message (`Ctrl-o`) `/session` transitions into full Operator and opens SessionModal
    // cleanly on an empty / post-`/new` epoch!
    let mut cold = dashboard_fixture();
    cold.ctrl('o');
    cold.event_action(Event::Paste("/session".into()));
    cold.key(KeyCode::Enter);
    let open_msgs = cold.drain_outbox();
    let open_req = open_msgs
        .iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Open)))
        .expect("Quick `/session` sends Open");
    let mut empty_epoch = summary();
    empty_epoch.session_epoch = 1;
    let follow = respond(
        &mut cold,
        open_req.request_id,
        OperatorResponse::Opened(empty_epoch.clone()),
    );
    let info_req = follow
        .iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::SessionInfo { .. })
            )
        })
        .expect("Opened triggers SessionInfo");
    if let Some(read_req) = follow
        .iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Read { .. })))
    {
        respond(
            &mut cold,
            read_req.request_id,
            history_of(empty_epoch, Vec::new(), None),
        );
    }
    let empty_info = ovrcr_protocol::OperatorSessionInfo {
        conversation: OperatorConversationId(7),
        session: SessionId(99),
        run: SessionRunId(1),
        session_epoch: 1,
        pi_session_id: None,
        checkpoint_turn: None,
        active_tip_turn: None,
        configured_model: None,
        active_model: None,
        configured_thinking_level: ovrcr_protocol::OperatorThinkingLevel::Default,
        reasoning: None,
        turns: 0,
        completed_turns: 0,
        branch_turns: 0,
        user_messages: 0,
        assistant_messages: 0,
        tool_calls: 0,
        tool_results: 0,
        epoch_tool_calls: 0,
        epoch_tool_results: 0,
        total_messages: 0,
        epoch_total_messages: 0,
        tokens: ovrcr_protocol::OperatorTokenStats::default(),
        cost_usd_nanos: 0,
        context_window: ovrcr_protocol::OperatorContextWindowUsage::default(),
    };
    respond(
        &mut cold,
        info_req.request_id,
        OperatorResponse::SessionInfo(Box::new(empty_info)),
    );
    let empty_modal = draw_text(&mut cold, 140, 35);
    assert!(
        !empty_modal.contains("Quick message · Ctrl-o"),
        "Quick message must close when transitioning to SessionModal:\n{empty_modal}"
    );
    for expected in ["none", "harness default", "$0.0000"] {
        assert!(
            empty_modal.to_lowercase().contains(expected),
            "expected `{expected}` in empty SessionModal:\n{empty_modal}"
        );
    }
}

#[test]
fn slash_model_opens_searchable_picker_switches_default_and_exact_and_routes_quick_message() {
    use ovrcr_protocol::OperatorModelCatalogEntry;

    let catalog = vec![
        OperatorModelCatalogEntry {
            provider: "anthropic".into(),
            id: "claude-sonnet-4-6".into(),
            name: Some("Claude Sonnet 4.6".into()),
        },
        OperatorModelCatalogEntry {
            provider: "openai".into(),
            id: "gpt-5.4".into(),
            name: None,
        },
        OperatorModelCatalogEntry {
            provider: "openrouter".into(),
            id: "vendor/sub/id".into(),
            name: Some("Nested Router Model".into()),
        },
    ];

    // 1. `/` menu includes `/model [provider/id | default]`, filters on `/MO`, Tab-completes `/model `,
    //    and Enter in `/` menu triggers `OperatorRequest::Models`.
    let mut d = dashboard_fixture();
    open_and_settle(&mut d, reasoning_summary(true)); // running turn (`true`) proves mid-turn switching works
    d.key(KeyCode::Char('/'));
    let menu_screen = draw_text(&mut d, 140, 30);
    assert!(
        menu_screen.contains("/model [provider/id | default]")
            && menu_screen.contains("Choose or set Operator model"),
        "expected `/model` in `/` popup:\n{menu_screen}"
    );
    d.key(KeyCode::Char('M'));
    d.key(KeyCode::Char('O'));
    let mo_screen = draw_text(&mut d, 140, 30);
    assert!(
        mo_screen.contains("Choose or set Operator model")
            && !mo_screen.contains("Copy latest assistant reply Markdown"),
        "case-insensitive `/MO` filter should isolate `/model`:\n{mo_screen}"
    );
    d.key(KeyCode::Tab);
    let after_tab = draw_text(&mut d, 140, 30);
    assert!(
        after_tab.contains("> /model ▏") && !after_tab.contains("Choose or set Operator model"),
        "Tab completes `/model ` with trailing space and dismisses menu:\n{after_tab}"
    );

    // 2. `/model DEFAULT` in both full Operator and cold Quick message writes `operator.model = None`
    //    immediately without querying `OperatorRequest::Models` or opening a picker.
    d.event_action(Event::Paste("DEFAULT".into()));
    d.key(KeyCode::Enter);
    let out = d.drain_outbox();
    assert!(
        out.iter().any(|m| matches!(
            &m.request,
            Request::SetSetting { path, value: None } if path == "operator.model"
        )),
        "expected SetSetting operator.model = None, got {out:?}"
    );
    assert!(
        !out.iter()
            .any(|m| matches!(m.request, Request::Operator(OperatorRequest::Models))),
        "`/model default` must not query catalog: {out:?}"
    );
    let after_default = draw_text(&mut d, 140, 30);
    assert!(
        after_default.contains("> ▏") && after_default.contains("Harness default"),
        "composer cleared and confirmation shown:\n{after_default}"
    );

    let mut cold_default = dashboard_fixture();
    cold_default.ctrl('o');
    cold_default.event_action(Event::Paste("/model default".into()));
    cold_default.key(KeyCode::Enter);
    let out = cold_default.drain_outbox();
    assert_eq!(out.len(), 1, "only SetSetting should be sent: {out:?}");
    assert!(matches!(
        &out[0].request,
        Request::SetSetting { path, value: None } if path == "operator.model"
    ));
    let browse_screen = draw_text(&mut cold_default, 140, 30);
    assert!(
        !browse_screen.contains("Quick message · Ctrl-o")
            && !browse_screen.contains("Operator · pi"),
        "cold Quick `/model default` closes Quick without opening full Operator:\n{browse_screen}"
    );

    // 3. `/model` opens searchable `ModelPicker` with `Harness default` first, marks active setting,
    //    filters live by substring on id or display name, closes on Esc, and writes SetSetting on Enter
    //    even while a turn is running.
    d.event_action(Event::Paste("/mo".into()));
    d.key(KeyCode::Enter); // run `/model` directly from `/` menu
    let out = d.drain_outbox();
    let models_req = out
        .iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Models)))
        .expect("`/model` requests dedicated profile catalog");
    respond(
        &mut d,
        models_req.request_id,
        OperatorResponse::Models(catalog.clone()),
    );
    let picker_default_active = draw_text(&mut d, 140, 35);
    assert!(
        picker_default_active.contains("✓ Harness default")
            && picker_default_active.contains("anthropic/claude-sonnet-4-6 · Claude Sonnet 4.6")
            && picker_default_active.contains("openai/gpt-5.4")
            && picker_default_active.contains("openrouter/vendor/sub/id · Nested Router Model"),
        "picker lists Harness default first + catalog entries with display names:\n{picker_default_active}"
    );

    // Live substring filtering by display name ("Nested"), then Backspace + Enter selects `openrouter/vendor/sub/id`.
    d.event_action(Event::Paste("nestedx".into()));
    let no_match_live = draw_text(&mut d, 140, 35);
    assert!(
        no_match_live.contains("No Operator models match \"nestedx\""),
        "live filter reports zero matches:\n{no_match_live}"
    );
    d.key(KeyCode::Backspace);
    let filtered_nested = draw_text(&mut d, 140, 35);
    assert!(
        filtered_nested.contains("openrouter/vendor/sub/id · Nested Router Model")
            && !filtered_nested.contains("openai/gpt-5.4"),
        "backspacing live filter narrows to matching entry:\n{filtered_nested}"
    );
    d.key(KeyCode::Enter);
    let out = d.drain_outbox();
    assert!(
        out.iter().any(|m| matches!(
            &m.request,
            Request::SetSetting { path, value: Some(v) }
                if path == "operator.model" && v == "\"openrouter/vendor/sub/id\""
        )),
        "Enter on filtered catalog model writes SetSetting mid-turn: {out:?}"
    );

    // Install updated Settings snapshot and verify `/model` marks `openrouter/vendor/sub/id` active (`✓`),
    // and Esc closes only the picker while staying inside full Operator.
    d.install_settings(ovrcr::tui::Settings {
        operator: ovrcr_protocol::OperatorSettings {
            model: Some("openrouter/vendor/sub/id".into()),
            ..Default::default()
        },
        ..Default::default()
    });
    d.event_action(Event::Paste("/model".into()));
    d.key(KeyCode::Enter);
    let models_req = d
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Models)))
        .expect("`/model` requests catalog");
    respond(
        &mut d,
        models_req.request_id,
        OperatorResponse::Models(catalog.clone()),
    );
    let picker_explicit_active = draw_text(&mut d, 140, 35);
    assert!(
        picker_explicit_active.contains("✓ openrouter/vendor/sub/id · Nested Router Model")
            && !picker_explicit_active.contains("✓ Harness default"),
        "picker marks configured Settings snapshot model active:\n{picker_explicit_active}"
    );
    assert_eq!(d.key(KeyCode::Esc), DashboardAction::Redraw);
    let after_esc = draw_text(&mut d, 140, 35);
    assert!(
        !after_esc.contains("Filter:") && after_esc.contains("Operator · pi · openai/gpt-6.1-sol"),
        "Esc closes ModelPicker while keeping full Operator open:\n{after_esc}"
    );

    // 4. `/model <provider/id>` exact match (including slash-containing ID) from `Ctrl-o` Quick message
    //    writes SetSetting immediately on catalog response and closes Quick without opening full Operator.
    let mut quick_exact = dashboard_fixture();
    quick_exact.ctrl('o');
    quick_exact.event_action(Event::Paste("/model openrouter/vendor/sub/id".into()));
    quick_exact.key(KeyCode::Enter);
    let out = quick_exact.drain_outbox();
    let models_req = out
        .iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Models)))
        .expect("Quick `/model <arg>` queries catalog");
    assert!(
        !out.iter()
            .any(|m| matches!(m.request, Request::Operator(OperatorRequest::Open))),
        "Quick exact `/model <provider/id>` should not force Open before checking catalog: {out:?}"
    );
    let out = respond(
        &mut quick_exact,
        models_req.request_id,
        OperatorResponse::Models(catalog.clone()),
    );
    assert!(
        out.iter().any(|m| matches!(
            &m.request,
            Request::SetSetting { path, value: Some(v) }
                if path == "operator.model" && v == "\"openrouter/vendor/sub/id\""
        )),
        "exact match from Quick writes SetSetting immediately: {out:?}"
    );
    assert!(
        !out.iter()
            .any(|m| matches!(m.request, Request::Operator(OperatorRequest::Open))),
        "exact match from Quick does not force full Operator open: {out:?}"
    );
    let quick_exact_screen = draw_text(&mut quick_exact, 140, 30);
    assert!(
        !quick_exact_screen.contains("Quick message · Ctrl-o"),
        "Quick closes after exact `/model <provider/id>`:\n{quick_exact_screen}"
    );

    // 5. Cold `Ctrl-o` `/model sonnet` (non-exact query) transitions into full Operator with
    //    `ModelPicker` pre-filtered by `sonnet`, and empty catalog / zero-match queries surface clear notices.
    let mut quick_partial = dashboard_fixture();
    quick_partial.ctrl('o');
    quick_partial.event_action(Event::Paste("/model sonnet".into()));
    quick_partial.key(KeyCode::Enter);
    let models_req = quick_partial
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Models)))
        .expect("Quick `/model sonnet` queries catalog");
    let out = respond(
        &mut quick_partial,
        models_req.request_id,
        OperatorResponse::Models(catalog),
    );
    let open_req = out
        .iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Open)))
        .expect("non-exact Quick `/model <query>` transitions into full Operator");
    let out = respond(
        &mut quick_partial,
        open_req.request_id,
        OperatorResponse::Opened(summary()),
    );
    let read_req = read_request(&out);
    respond(
        &mut quick_partial,
        read_req.request_id,
        history_of(summary(), Vec::new(), None),
    );
    let prefiltered_screen = draw_text(&mut quick_partial, 140, 35);
    assert!(
        !prefiltered_screen.contains("Quick message · Ctrl-o")
            && prefiltered_screen.contains("Filter: sonnet▏")
            && prefiltered_screen.contains("anthropic/claude-sonnet-4-6 · Claude Sonnet 4.6")
            && !prefiltered_screen.contains("openai/gpt-5.4"),
        "Quick partial `/model sonnet` opens full Operator with pre-filtered ModelPicker:\n{prefiltered_screen}"
    );

    // Empty dedicated profile catalog surfaces a clear `ovrcr operator setup` notice.
    quick_partial.key(KeyCode::Esc);
    quick_partial.event_action(Event::Paste("/model anything".into()));
    quick_partial.key(KeyCode::Enter);
    let models_req = quick_partial
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Models)))
        .expect("requests catalog");
    respond(
        &mut quick_partial,
        models_req.request_id,
        OperatorResponse::Models(Vec::new()),
    );
    let empty_catalog_screen = draw_text(&mut quick_partial, 140, 35);
    assert!(
        empty_catalog_screen.contains("ovrcr operator setup"),
        "empty catalog must surface clear setup guidance:\n{empty_catalog_screen}"
    );
}

#[test]
fn slash_compact_menu_completion_and_empty_or_busy_refusal() {
    let mut d = dashboard_fixture();
    open_and_settle(&mut d, with_model(summary()));

    d.key(KeyCode::Char('/'));
    let menu = draw_text(&mut d, 120, 30);
    for entry in [
        "/copy",
        "/thinking",
        "/model",
        "/new",
        "/session",
        "/compact",
        "/tree",
    ] {
        assert!(
            menu.contains(entry),
            "expected `{entry}` in slash menu:\n{menu}"
        );
    }

    // `/co` matches `/copy` then `/compact`; Down + Tab completes `/compact ` with trailing space.
    d.event_action(Event::Paste("co".into()));
    let filtered = draw_text(&mut d, 120, 30);
    assert!(
        filtered.contains("/copy") && filtered.contains("/compact"),
        "{filtered}"
    );
    d.key(KeyCode::Down);
    d.key(KeyCode::Tab);
    let completed_draft = draw_text(&mut d, 120, 30);
    assert!(
        completed_draft.contains("> /compact ▏"),
        "expected trailing space on `/compact` Tab completion:\n{completed_draft}"
    );

    // Brand-new conversation (`current_turn: None`) refuses `/compact` and keeps draft.
    assert_eq!(d.key(KeyCode::Enter), DashboardAction::Redraw);
    assert!(
        d.drain_outbox().is_empty(),
        "empty-epoch `/compact` must refuse without sending Submit or Compact"
    );
    let empty_refusal = draw_text(&mut d, 120, 30);
    assert!(
        empty_refusal.contains("> /compact ▏"),
        "draft preserved on refusal:\n{empty_refusal}"
    );
    assert!(
        empty_refusal.contains("before the first completed turn"),
        "expected empty-epoch refusal:\n{empty_refusal}"
    );

    // Busy (`Running`) conversation also refuses `/compact` and keeps draft.
    let mut busy_dash = dashboard_fixture();
    open_and_settle(&mut busy_dash, busy());
    busy_dash.event_action(Event::Paste("/compact".into()));
    busy_dash.key(KeyCode::Enter);
    assert!(busy_dash.drain_outbox().is_empty());
    let busy_screen = draw_text(&mut busy_dash, 120, 30);
    assert!(busy_screen.contains("> /compact▏"), "{busy_screen}");
    assert!(
        busy_screen.to_lowercase().contains("busy"),
        "expected busy refusal:\n{busy_screen}"
    );
}

#[test]
fn slash_compact_dispatches_across_composer_and_quick_renders_activity_receipt_and_stop_without_clobbering_copy()
 {
    let mut d = dashboard_fixture();
    let mut seeded = with_model(summary());
    seeded.run = SessionRunId(2);
    seeded.current_turn = Some(turn(1, OperatorTurnState::Complete));
    let out = opened(&mut d, seeded.clone());
    let read = read_request(&out);
    respond(
        &mut d,
        read.request_id,
        history_of(
            seeded.clone(),
            vec![
                user(1, "inspect logs", OperatorContext::default()),
                delta(2, "Prior assistant reply markdown."),
                completed(3),
            ],
            None,
        ),
    );

    // 1. Composer `/compact keep test failures` sends OperatorRequest::Compact.
    d.event_action(Event::Paste("/compact keep test failures".into()));
    d.key(KeyCode::Enter);
    let out = d.drain_outbox();
    let compact_req = out
        .iter()
        .find(|m| {
            matches!(
                &m.request,
                Request::Operator(OperatorRequest::Compact {
                    conversation: OperatorConversationId(7),
                    expected_run: SessionRunId(2),
                    instructions: Some(instructions),
                }) if instructions == "keep test failures"
            )
        })
        .expect("expected OperatorRequest::Compact");

    // Server error keeps draft intact and renders error.
    d.handle_server_message(ServerMessage::Response {
        request_id: compact_req.request_id,
        response: Response::Error {
            code: ovrcr_protocol::ErrorCode::Conflict,
            message:
                "Cannot compact Operator context before the first completed turn in this session."
                    .into(),
        },
    });
    let err_screen = draw_text(&mut d, 120, 30);
    assert!(
        err_screen.contains("> /compact keep test failures▏")
            && err_screen.contains("before the first completed turn"),
        "{err_screen}"
    );

    // Retry and accept turn 2 (`Running`): draft clears, activity shows `Compacting context…`,
    // and `Ctrl-x` sends foreground `Stop`.
    d.key(KeyCode::Enter);
    let compact_req2 = d
        .drain_outbox()
        .into_iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::Compact { .. })
            )
        })
        .expect("retry Compact");
    let mut running_turn = turn(2, OperatorTurnState::Running);
    running_turn.identity.run = SessionRunId(3);
    let follow = respond(
        &mut d,
        compact_req2.request_id,
        OperatorResponse::Submitted(running_turn.clone()),
    );
    let mut running_summary = seeded.clone();
    running_summary.run = SessionRunId(3);
    running_summary.worker_stopped = false;
    running_summary.current_turn = Some(running_turn.clone());
    let read_running = read_request(&follow);
    respond(
        &mut d,
        read_running.request_id,
        history_of(
            running_summary.clone(),
            vec![
                turn_message(
                    2,
                    4,
                    OperatorMessageContent::Compaction {
                        instructions: Some("keep test failures".into()),
                    },
                ),
                turn_message(
                    2,
                    5,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted),
                ),
            ],
            None,
        ),
    );
    let compacting_screen = draw_text(&mut d, 120, 30);
    assert!(
        compacting_screen.contains("> ▏")
            && compacting_screen.contains("Compacting context…")
            && compacting_screen.contains("Compact context · keep test failures"),
        "expected live compaction activity & header:\n{compacting_screen}"
    );

    // Stop (`Ctrl-x`) during manual compaction sends `OperatorRequest::Stop`.
    d.ctrl('x');
    let stop_out = d.drain_outbox();
    let stop_req = stop_out
        .iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::Stop { identity })
                    if identity == running_turn.identity
            )
        })
        .expect("Ctrl-x during /compact sends Stop");
    let mut interrupted_turn = running_turn.clone();
    interrupted_turn.state = OperatorTurnState::Interrupted;
    let follow_stop = respond(
        &mut d,
        stop_req.request_id,
        OperatorResponse::Stopped(interrupted_turn.clone()),
    );
    let mut interrupted_summary = running_summary.clone();
    interrupted_summary.worker_stopped = true;
    interrupted_summary.current_turn = Some(interrupted_turn);
    let read_stop = read_request(&follow_stop);
    respond(
        &mut d,
        read_stop.request_id,
        history_of(
            interrupted_summary.clone(),
            vec![turn_message(2, 6, OperatorMessageContent::Interrupted)],
            None,
        ),
    );
    let stopped_screen = draw_text(&mut d, 120, 30);
    assert!(
        stopped_screen.contains("previous context preserved"),
        "expected truthful interrupted compaction receipt:\n{stopped_screen}"
    );

    // 2. Menu `Enter` on `/compact` executes zero-arg `/compact` and renders completed receipt
    //    with `tokens_before` + summary while preserving `/copy` of the prior assistant reply!
    d.event_action(Event::Paste("/compact".into()));
    let menu_compact = draw_text(&mut d, 120, 30);
    assert!(menu_compact.contains("/compact"), "{menu_compact}");
    d.key(KeyCode::Enter);
    let compact_req3 = d
        .drain_outbox()
        .into_iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::Compact {
                    instructions: None,
                    ..
                })
            )
        })
        .expect("menu Enter executes zero-arg Compact");
    let mut turn3 = turn(3, OperatorTurnState::Running);
    turn3.identity.run = SessionRunId(4);
    let follow3 = respond(
        &mut d,
        compact_req3.request_id,
        OperatorResponse::Submitted(turn3.clone()),
    );
    let mut complete_summary = interrupted_summary;
    complete_summary.run = SessionRunId(4);
    complete_summary.worker_stopped = true;
    turn3.state = OperatorTurnState::Complete;
    complete_summary.current_turn = Some(turn3);
    let read3 = read_request(&follow3);
    respond(
        &mut d,
        read3.request_id,
        history_of(
            complete_summary,
            vec![
                turn_message(
                    3,
                    7,
                    OperatorMessageContent::Compaction { instructions: None },
                ),
                turn_message(
                    3,
                    8,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted),
                ),
                turn_message(
                    3,
                    9,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionEnded),
                ),
                turn_message(
                    3,
                    10,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionSummary {
                        summary: "Compacted summary body.".into(),
                        tokens_before: 4_200,
                    }),
                ),
                turn_message(
                    3,
                    11,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                        leased: false,
                        pi_first_entry_id: None,
                        pi_leaf_entry_id: None,
                    }),
                ),
            ],
            None,
        ),
    );
    let receipt_screen = draw_text(&mut d, 120, 30);
    for expected in ["Compact context", "4,200 tokens", "Compacted summary body."] {
        assert!(
            receipt_screen.contains(expected),
            "expected `{expected}` in compaction receipt:\n{receipt_screen}"
        );
    }

    // `/copy` copies the prior assistant reply, NOT the compaction summary!
    d.event_action(Event::Paste("/copy".into()));
    assert_eq!(
        d.key(KeyCode::Enter),
        DashboardAction::CopyText("Prior assistant reply markdown.".into())
    );

    // 3. Cold Quick message (`Ctrl-o`) `/compact concise` refuses empty-epoch conversations
    //    keeping Quick open, and transitions into full Operator when a checkpoint exists.
    let mut cold_empty = dashboard_fixture();
    cold_empty.ctrl('o');
    cold_empty.event_action(Event::Paste("/compact concise".into()));
    cold_empty.key(KeyCode::Enter);
    let open_req = cold_empty
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Open)))
        .expect("cold Quick `/compact` opens singleton");
    respond(
        &mut cold_empty,
        open_req.request_id,
        OperatorResponse::Opened(summary()),
    );
    let cold_empty_screen = draw_text(&mut cold_empty, 120, 30);
    assert!(
        cold_empty_screen.contains("Quick message · Ctrl-o")
            && cold_empty_screen.contains("/compact concise")
            && cold_empty_screen.contains("before the first completed turn"),
        "cold Quick `/compact` on empty epoch must preserve Quick overlay and draft:\n{cold_empty_screen}"
    );

    let mut cold_ready = dashboard_fixture();
    cold_ready.ctrl('o');
    cold_ready.event_action(Event::Paste("/compact concise".into()));
    cold_ready.key(KeyCode::Enter);
    let open_req = cold_ready
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Open)))
        .expect("cold Quick `/compact` opens singleton");
    let next = respond(
        &mut cold_ready,
        open_req.request_id,
        OperatorResponse::Opened(seeded.clone()),
    );
    let quick_compact = next
        .iter()
        .find(|m| {
            matches!(
                &m.request,
                Request::Operator(OperatorRequest::Compact {
                    instructions: Some(instructions),
                    ..
                }) if instructions == "concise"
            )
        })
        .expect("Opened triggers Compact");
    let mut q_turn = turn(2, OperatorTurnState::Running);
    q_turn.identity.run = SessionRunId(3);
    respond(
        &mut cold_ready,
        quick_compact.request_id,
        OperatorResponse::Submitted(q_turn),
    );
    let transitioned = draw_text(&mut cold_ready, 120, 30);
    assert!(
        !transitioned.contains("Quick message · Ctrl-o")
            && transitioned.contains("Compacting context…"),
        "accepted Quick `/compact` transitions into full Operator:\n{transitioned}"
    );
}

#[test]
fn tree_slash_command_inspects_searches_filters_switches_rewinds_and_refuses_when_busy() {
    use ovrcr_protocol::{
        OperatorSessionTree, OperatorTreeEntry, OperatorTreeEntryKind, OperatorTreeNavigation,
    };

    let mut d = dashboard_fixture();
    let mut seeded = with_model(summary());
    seeded.run = SessionRunId(3);
    seeded.current_turn = Some(turn(3, OperatorTurnState::Complete));
    let out = opened(&mut d, seeded.clone());
    let read = read_request(&out);
    respond(
        &mut d,
        read.request_id,
        history_of(
            seeded.clone(),
            vec![
                user(1, "inspect logs", OperatorContext::default()),
                delta(2, "Found two sessions."),
                completed(3),
                turn_message(
                    3,
                    4,
                    OperatorMessageContent::User {
                        skill: None,
                        text: "check beta latency".into(),
                        context: OperatorContext::default(),
                    },
                ),
                turn_message(
                    3,
                    5,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                        "Beta p99 is 12ms.".into(),
                    )),
                ),
                turn_message(
                    3,
                    6,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                        leased: false,
                        pi_first_entry_id: Some("u2b".into()),
                        pi_leaf_entry_id: Some("a2b".into()),
                    }),
                ),
            ],
            None,
        ),
    );

    // 1. `/tr` filters slash menu to `/tree` and Tab completes without trailing space;
    //    `/tree extra` rejects locally with draft preserved.
    d.event_action(Event::Paste("/tr".into()));
    let menu = draw_text(&mut d, 120, 30);
    assert!(
        menu.contains("/tree") && menu.contains("Inspect or switch Operator conversation branch"),
        "{menu}"
    );
    d.key(KeyCode::Tab);
    assert!(draw_text(&mut d, 120, 30).contains("> /tree▏"));
    d.event_action(Event::Paste(" extra".into()));
    d.key(KeyCode::Enter);
    assert!(d.drain_outbox().is_empty());
    let bad_args = draw_text(&mut d, 120, 30);
    assert!(
        bad_args.contains("Usage: /tree (takes no arguments).")
            && bad_args.contains("> /tree extra▏"),
        "{bad_args}"
    );

    // 2. `/tree` sends `OperatorRequest::Tree` and renders connectors, `●` markers, filters, and search.
    d.ctrl('c');
    d.event_action(Event::Paste("/tree".into()));
    d.key(KeyCode::Enter);
    let tree_req = d
        .drain_outbox()
        .into_iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::Tree {
                    conversation: OperatorConversationId(7)
                })
            )
        })
        .expect("expected OperatorRequest::Tree");

    let sample_tree = OperatorSessionTree {
        pi_session_id: Some("pi-7".into()),
        current_leaf_id: Some("a2b".into()),
        entries: vec![
            OperatorTreeEntry {
                id: "m1".into(),
                parent_id: None,
                kind: OperatorTreeEntryKind::ModelChange,
                preview: "openai/gpt-6.1-sol".into(),
            },
            OperatorTreeEntry {
                id: "u1".into(),
                parent_id: Some("m1".into()),
                kind: OperatorTreeEntryKind::User,
                preview: "inspect logs".into(),
            },
            OperatorTreeEntry {
                id: "tr1".into(),
                parent_id: Some("u1".into()),
                kind: OperatorTreeEntryKind::ToolResult,
                preview: "tool result: list_sessions".into(),
            },
            OperatorTreeEntry {
                id: "a1".into(),
                parent_id: Some("tr1".into()),
                kind: OperatorTreeEntryKind::Assistant { tool_only: false },
                preview: "Found two sessions.".into(),
            },
            OperatorTreeEntry {
                id: "u2a".into(),
                parent_id: Some("a1".into()),
                kind: OperatorTreeEntryKind::User,
                preview: "restart alpha worker".into(),
            },
            OperatorTreeEntry {
                id: "a2a".into(),
                parent_id: Some("u2a".into()),
                kind: OperatorTreeEntryKind::Assistant { tool_only: false },
                preview: "Alpha restarted.".into(),
            },
            OperatorTreeEntry {
                id: "u2b".into(),
                parent_id: Some("a1".into()),
                kind: OperatorTreeEntryKind::User,
                preview: "check beta latency".into(),
            },
            OperatorTreeEntry {
                id: "a2b".into(),
                parent_id: Some("u2b".into()),
                kind: OperatorTreeEntryKind::Assistant { tool_only: false },
                preview: "Beta p99 is 12ms.".into(),
            },
        ],
    };
    respond(
        &mut d,
        tree_req.request_id,
        OperatorResponse::Tree(Box::new(sample_tree.clone())),
    );
    let tree_screen = draw_text(&mut d, 120, 32);
    for expected in [
        "filter: default",
        "● user: inspect logs",
        "├─   user: restart alpha worker",
        "│    assistant: Alpha restarted.",
        "└─ ● user: check beta latency",
        "● assistant: Beta p99 is 12ms. ← active",
    ] {
        assert!(
            tree_screen.contains(expected),
            "expected `{expected}` in `/tree` modal:\n{tree_screen}"
        );
    }
    assert!(
        !tree_screen.contains("[model]"),
        "`default` filter must hide model_change:\n{tree_screen}"
    );

    // Filter mode cycles `default` -> `no-tools` -> `user-only` -> `all` -> `default`.
    d.key(KeyCode::Tab);
    let no_tools = draw_text(&mut d, 120, 32);
    assert!(
        no_tools.contains("filter: no-tools") && !no_tools.contains("tool: tool result"),
        "{no_tools}"
    );
    d.ctrl('f');
    let user_only = draw_text(&mut d, 120, 32);
    assert!(
        user_only.contains("filter: user-only") && !user_only.contains("Alpha restarted."),
        "{user_only}"
    );
    d.key(KeyCode::Tab);
    let all_mode = draw_text(&mut d, 120, 32);
    assert!(
        all_mode.contains("filter: all") && all_mode.contains("[model]: openai/gpt-6.1-sol"),
        "{all_mode}"
    );
    d.key(KeyCode::Tab);
    assert!(draw_text(&mut d, 120, 32).contains("filter: default"));

    // Multi-token search + `Ctrl-c` clear:
    d.event_action(Event::Paste("alpha assistant".into()));
    let searched = draw_text(&mut d, 120, 32);
    assert!(
        searched.contains("Alpha restarted.") && !searched.contains("← active"),
        "{searched}"
    );
    d.ctrl('c');
    assert!(draw_text(&mut d, 120, 32).contains("Beta p99 is 12ms. ← active"));

    // 3. Pressing `Enter` on the already-active leaf (`a2b`) closes `/tree` as a zero-op!
    d.key(KeyCode::Enter);
    assert!(
        d.drain_outbox().is_empty(),
        "selecting active leaf must not send NavigateTree"
    );
    assert!(!draw_text(&mut d, 120, 32).contains("Operator tree · /tree"));

    // 4. Reopen `/tree`, search `"alpha assistant"` (`a2a`), press `Enter` -> switches branch,
    //    clears composer, reloads feed from `after: 0`, and `/session` shows branch vs epoch counts.
    d.event_action(Event::Paste("/tree".into()));
    d.key(KeyCode::Enter);
    let req2 = d
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Tree { .. })))
        .unwrap();
    respond(
        &mut d,
        req2.request_id,
        OperatorResponse::Tree(Box::new(sample_tree.clone())),
    );
    d.event_action(Event::Paste("alpha assistant".into()));
    d.key(KeyCode::Enter);
    let summarize_prompt = draw_text(&mut d, 120, 32);
    assert!(
        summarize_prompt.contains("Summarize branch?")
            && summarize_prompt.contains("No summary")
            && summarize_prompt.contains("Summarize with custom prompt"),
        "switching away from `a2b` must prompt `Summarize branch?`:\n{summarize_prompt}"
    );
    d.key(KeyCode::Enter); // confirm default `No summary`
    let nav_req = d
        .drain_outbox()
        .into_iter()
        .find(|m| {
            matches!(
                &m.request,
                Request::Operator(OperatorRequest::NavigateTree {
                    conversation: OperatorConversationId(7),
                    expected_run: SessionRunId(3),
                    target_entry_id,
                    summarize: None,
                }) if target_entry_id == "a2a"
            )
        })
        .expect("expected NavigateTree to `a2a`");
    let follow = respond(
        &mut d,
        nav_req.request_id,
        OperatorResponse::TreeNavigated(Box::new(OperatorTreeNavigation {
            conversation: seeded.clone(),
            current_leaf_id: Some("a2a".into()),
            noop: false,
            editor_text: None,
            restored_context: None,
            submitted: None,
        })),
    );
    let reload = follow
        .iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::Read {
                    conversation: OperatorConversationId(7),
                    after: 0,
                    ..
                })
            )
        })
        .expect("branch switch reloads history from after=0");
    respond(
        &mut d,
        reload.request_id,
        history_of(
            seeded.clone(),
            vec![
                user(1, "inspect logs", OperatorContext::default()),
                delta(2, "Found two sessions."),
                completed(3),
                turn_message(
                    2,
                    4,
                    OperatorMessageContent::User {
                        skill: None,
                        text: "restart alpha worker".into(),
                        context: OperatorContext::default(),
                    },
                ),
                turn_message(
                    2,
                    5,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                        "Alpha restarted.".into(),
                    )),
                ),
                turn_message(
                    2,
                    6,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                        leased: false,
                        pi_first_entry_id: Some("u2a".into()),
                        pi_leaf_entry_id: Some("a2a".into()),
                    }),
                ),
            ],
            None,
        ),
    );
    let switched_screen = draw_text(&mut d, 120, 32);
    assert!(
        switched_screen.contains("Alpha restarted.")
            && !switched_screen.contains("Beta p99 is 12ms.")
            && switched_screen.contains("> ▏"),
        "{switched_screen}"
    );

    // `/session` shows active-branch vs epoch counts when branched.
    d.event_action(Event::Paste("/session".into()));
    d.key(KeyCode::Enter);
    let sreq = d
        .drain_outbox()
        .into_iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::SessionInfo { .. })
            )
        })
        .unwrap();
    respond(
        &mut d,
        sreq.request_id,
        OperatorResponse::SessionInfo(Box::new(ovrcr_protocol::OperatorSessionInfo {
            conversation: OperatorConversationId(7),
            session: SessionId(99),
            run: SessionRunId(3),
            session_epoch: 0,
            pi_session_id: Some("pi-7".into()),
            checkpoint_turn: Some(OperatorTurnId(3)),
            active_tip_turn: Some(OperatorTurnId(2)),
            configured_model: None,
            active_model: None,
            configured_thinking_level: ovrcr_protocol::OperatorThinkingLevel::Default,
            reasoning: None,
            turns: 3,
            completed_turns: 3,
            branch_turns: 2,
            user_messages: 2,
            assistant_messages: 2,
            tool_calls: 1,
            tool_results: 1,
            epoch_tool_calls: 3,
            epoch_tool_results: 2,
            total_messages: 5,
            epoch_total_messages: 7,
            tokens: ovrcr_protocol::OperatorTokenStats::default(),
            cost_usd_nanos: 0,
            context_window: ovrcr_protocol::OperatorContextWindowUsage::default(),
        })),
    );
    let branched_session = draw_text(&mut d, 140, 35);
    assert!(
        branched_session.contains("2 on branch · 3 total")
            && branched_session.contains("5 on branch (2 user · 2 assistant) · 7 in epoch")
            && branched_session
                .contains("1 calls · 1 results on branch · 3 calls · 2 results in epoch"),
        "{branched_session}"
    );
    d.key(KeyCode::Esc);

    // 5. Rewind a user prompt (`u2a`): restores unwrapped text + captured `OperatorContext` into composer!
    let target_ctx = captured_context(&fixture_hierarchy(), SessionId(1));
    d.event_action(Event::Paste("/tree".into()));
    d.key(KeyCode::Enter);
    let req3 = d
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Tree { .. })))
        .unwrap();
    let mut branch_a_tree = sample_tree.clone();
    branch_a_tree.current_leaf_id = Some("a2a".into());
    respond(
        &mut d,
        req3.request_id,
        OperatorResponse::Tree(Box::new(branch_a_tree)),
    );
    d.event_action(Event::Paste("restart alpha".into()));
    d.key(KeyCode::Enter);
    assert!(
        draw_text(&mut d, 120, 32).contains("Summarize branch?"),
        "rewinding `u2a` while `a2a` is leaf must prompt `Summarize branch?`"
    );
    d.key(KeyCode::Enter); // confirm `No summary`
    let rewind_req = d
        .drain_outbox()
        .into_iter()
        .find(|m| {
            matches!(
                &m.request,
                Request::Operator(OperatorRequest::NavigateTree {
                    target_entry_id,
                    summarize: None,
                    ..
                }) if target_entry_id == "u2a"
            )
        })
        .expect("expected NavigateTree to user node `u2a`");
    let rewind_follow = respond(
        &mut d,
        rewind_req.request_id,
        OperatorResponse::TreeNavigated(Box::new(OperatorTreeNavigation {
            conversation: seeded.clone(),
            current_leaf_id: Some("a1".into()),
            noop: false,
            editor_text: Some("restart alpha worker".into()),
            restored_context: Some(target_ctx),
            submitted: None,
        })),
    );
    let rewind_read = read_request(&rewind_follow);
    respond(
        &mut d,
        rewind_read.request_id,
        history_of(
            seeded.clone(),
            vec![
                user(1, "inspect logs", OperatorContext::default()),
                delta(2, "Found two sessions."),
                completed(3),
            ],
            None,
        ),
    );
    d.ctrl('l'); // hide notice on next edit? First verify notice + draft, then check restored context!
    let rewound_screen = draw_text(&mut d, 120, 32);
    assert!(
        rewound_screen.contains("> restart alpha worker▏"),
        "expected unwrapped prompt restored in composer:\n{rewound_screen}"
    );
    // Editing a char clears the transient notice and reveals the restored `Managing consigint / auth / #1 run 1` caption!
    d.key(KeyCode::Char('!'));
    let ctx_screen = draw_text(&mut d, 120, 32);
    assert!(
        ctx_screen.contains("Managing consigint / auth / #1 run 1")
            && ctx_screen.contains("> restart alpha worker!▏"),
        "expected restored OperatorContext in composer caption:\n{ctx_screen}"
    );

    // 6. Read-only `/tree` while a turn is `Running`: inspection works, switching refuses cleanly!
    let mut busy = dashboard_fixture();
    open_and_settle(&mut busy, running_request());
    busy.event_action(Event::Paste("/tree".into()));
    busy.key(KeyCode::Enter);
    let busy_tree_req = busy
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Tree { .. })))
        .expect("read-only `/tree` allowed while Running");
    respond(
        &mut busy,
        busy_tree_req.request_id,
        OperatorResponse::Tree(Box::new(sample_tree)),
    );
    assert!(draw_text(&mut busy, 120, 32).contains("● user: inspect logs"));
    busy.key(KeyCode::Up); // move off active leaf `a2b` onto `u2b`
    busy.key(KeyCode::Enter);
    assert!(
        busy.drain_outbox().is_empty(),
        "busy `/tree` must refuse NavigateTree locally"
    );
    let busy_refused = draw_text(&mut busy, 120, 32);
    assert!(
        busy_refused.contains("Cannot switch Operator tree branch while a turn is running."),
        "{busy_refused}"
    );

    // 7. Cold Quick message (`Ctrl-o`) `/tree` transitions into full Operator and renders empty state.
    let mut cold = dashboard_fixture();
    cold.ctrl('o');
    cold.event_action(Event::Paste("/tree".into()));
    cold.key(KeyCode::Enter);
    let open_req = cold
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Open)))
        .expect("Quick `/tree` opens singleton");
    let cold_follow = respond(
        &mut cold,
        open_req.request_id,
        OperatorResponse::Opened(summary()),
    );
    let cold_tree_req = cold_follow
        .iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Tree { .. })))
        .expect("Opened triggers Tree");
    respond(
        &mut cold,
        cold_tree_req.request_id,
        OperatorResponse::Tree(Box::default()),
    );
    let cold_screen = draw_text(&mut cold, 120, 32);
    assert!(
        !cold_screen.contains("Quick message · Ctrl-o")
            && cold_screen.contains("No turns in this Operator session yet."),
        "{cold_screen}"
    );
}

#[test]
fn tree_summarize_branch_modal_supports_esc_back_navigation_custom_prompt_composer_restore_and_stop()
 {
    use ovrcr_protocol::{
        OperatorBranchSummaryRequest, OperatorSessionTree, OperatorTreeEntry,
        OperatorTreeEntryKind, OperatorTreeNavigation, OperatorTurn, OperatorTurnIdentity,
    };

    let mut d = dashboard_fixture();
    let mut seeded = with_model(summary());
    seeded.run = SessionRunId(3);
    seeded.current_turn = Some(turn(3, OperatorTurnState::Complete));
    let out = opened(&mut d, seeded.clone());
    let read_req = read_request(&out);
    respond(
        &mut d,
        read_req.request_id,
        history_of(
            seeded.clone(),
            vec![
                user(1, "inspect logs", OperatorContext::default()),
                delta(2, "Found two sessions."),
                completed(3),
                turn_message(
                    3,
                    4,
                    OperatorMessageContent::User {
                        skill: None,
                        text: "check beta latency".into(),
                        context: OperatorContext::default(),
                    },
                ),
                turn_message(
                    3,
                    5,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                        "Beta p99 is 12ms.".into(),
                    )),
                ),
                turn_message(
                    3,
                    6,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                        leased: false,
                        pi_first_entry_id: Some("u2b".into()),
                        pi_leaf_entry_id: Some("a2b".into()),
                    }),
                ),
            ],
            None,
        ),
    );

    let entries = vec![
        OperatorTreeEntry {
            id: "u1".into(),
            parent_id: None,
            kind: OperatorTreeEntryKind::User,
            preview: "inspect logs".into(),
        },
        OperatorTreeEntry {
            id: "a1".into(),
            parent_id: Some("u1".into()),
            kind: OperatorTreeEntryKind::Assistant { tool_only: false },
            preview: "Found two sessions.".into(),
        },
        OperatorTreeEntry {
            id: "u2a".into(),
            parent_id: Some("a1".into()),
            kind: OperatorTreeEntryKind::User,
            preview: "restart alpha worker".into(),
        },
        OperatorTreeEntry {
            id: "a2a".into(),
            parent_id: Some("u2a".into()),
            kind: OperatorTreeEntryKind::Assistant { tool_only: false },
            preview: "Alpha restarted.".into(),
        },
        OperatorTreeEntry {
            id: "u2b".into(),
            parent_id: Some("a1".into()),
            kind: OperatorTreeEntryKind::User,
            preview: "check beta latency".into(),
        },
        OperatorTreeEntry {
            id: "a2b".into(),
            parent_id: Some("u2b".into()),
            kind: OperatorTreeEntryKind::Assistant { tool_only: false },
            preview: "Beta p99 is 12ms.".into(),
        },
    ];

    // 1. Navigating forward to a descendant (`current_leaf_id = Some("a1")` -> `"a2a"`) has ZERO
    //    abandoned entries on `a1`, so it must NOT prompt `"Summarize branch?"`!
    d.event_action(Event::Paste("/tree".into()));
    d.key(KeyCode::Enter);
    let tree_req_1 = d
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Tree { .. })))
        .unwrap();
    respond(
        &mut d,
        tree_req_1.request_id,
        OperatorResponse::Tree(Box::new(OperatorSessionTree {
            pi_session_id: Some("pi-7".into()),
            current_leaf_id: Some("a1".into()),
            entries: entries.clone(),
        })),
    );
    d.event_action(Event::Paste("Alpha restarted".into()));
    d.key(KeyCode::Enter);
    let forward_req = d
        .drain_outbox()
        .into_iter()
        .find(|m| {
            matches!(
                &m.request,
                Request::Operator(OperatorRequest::NavigateTree {
                    target_entry_id,
                    summarize: None,
                    ..
                }) if target_entry_id == "a2a"
            )
        })
        .expect("descendant navigation with zero abandoned entries must not prompt");
    let forward_follow = respond(
        &mut d,
        forward_req.request_id,
        OperatorResponse::TreeNavigated(Box::new(OperatorTreeNavigation {
            conversation: seeded.clone(),
            current_leaf_id: Some("a2b".into()),
            noop: true,
            editor_text: None,
            restored_context: None,
            submitted: None,
        })),
    );
    assert!(forward_follow.is_empty());

    // 2. Navigating from `a2b` to `u2a` with filter `user-only` + search `"restart alpha"` opens
    //    `"Summarize branch?"`; `Esc` from custom-instructions -> `"Summarize branch?"` -> `/tree`
    //    preserves filter, search query, and selected node!
    d.event_action(Event::Paste("unsent draft before summary".into()));
    d.ctrl('c'); // clear draft so `/tree` can be entered
    d.event_action(Event::Paste("/tree".into()));
    d.key(KeyCode::Enter);
    let tree_req_2 = d
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Tree { .. })))
        .unwrap();
    respond(
        &mut d,
        tree_req_2.request_id,
        OperatorResponse::Tree(Box::new(OperatorSessionTree {
            pi_session_id: Some("pi-7".into()),
            current_leaf_id: Some("a2b".into()),
            entries: entries.clone(),
        })),
    );
    d.key(KeyCode::Tab); // no-tools
    d.key(KeyCode::Tab); // user-only
    d.event_action(Event::Paste("restart alpha".into()));
    assert!(draw_text(&mut d, 120, 32).contains("filter: user-only"));

    d.key(KeyCode::Enter); // -> "Summarize branch?"
    let confirm_screen = draw_text(&mut d, 120, 32);
    assert!(
        confirm_screen.contains("Summarize branch?")
            && confirm_screen.contains("▸ No summary")
            && confirm_screen.contains("  Summarize")
            && confirm_screen.contains("  Summarize with custom prompt"),
        "{confirm_screen}"
    );
    d.key(KeyCode::Down); // Summarize
    d.key(KeyCode::Down); // Summarize with custom prompt
    d.key(KeyCode::Enter); // -> Custom instructions input
    let custom_screen = draw_text(&mut d, 120, 32);
    assert!(
        custom_screen.contains("Instructions:")
            && custom_screen.contains("Summarize branch · Custom prompt"),
        "{custom_screen}"
    );

    // `Esc` from custom instructions -> back to `"Summarize branch?"` with `Summarize with custom prompt` still selected!
    d.key(KeyCode::Esc);
    let back_to_confirm = draw_text(&mut d, 120, 32);
    assert!(
        back_to_confirm.contains("Summarize branch?")
            && back_to_confirm.contains("▸ Summarize with custom prompt"),
        "{back_to_confirm}"
    );

    // `Esc` from `"Summarize branch?"` -> back to `/tree` preserving `filter: user-only`, `Search: restart alpha▏`, and `u2a`!
    d.key(KeyCode::Esc);
    let back_to_tree = draw_text(&mut d, 120, 32);
    assert!(
        back_to_tree.contains("filter: user-only")
            && back_to_tree.contains("Search: restart alpha▏")
            && back_to_tree.contains("user: restart alpha worker"),
        "{back_to_tree}"
    );

    // 3. Re-enter -> `Summarize with custom prompt` -> submit `"focus on auth"` -> running activity -> completion reloads
    //    feed with receipt AND restores unwrapped `"restart alpha worker"` + `OperatorContext` into composer!
    d.key(KeyCode::Enter); // -> "Summarize branch?"
    d.key(KeyCode::Down);
    d.key(KeyCode::Down);
    d.key(KeyCode::Enter); // -> Custom instructions
    d.event_action(Event::Paste("focus on auth".into()));
    d.key(KeyCode::Enter);

    let sum_req = d
        .drain_outbox()
        .into_iter()
        .find(|m| {
            matches!(
                &m.request,
                Request::Operator(OperatorRequest::NavigateTree {
                    target_entry_id,
                    summarize: Some(OperatorBranchSummaryRequest {
                        instructions: Some(inst),
                    }),
                    ..
                }) if target_entry_id == "u2a" && inst == "focus on auth"
            )
        })
        .expect("expected NavigateTree with custom instructions");

    let target_ctx = captured_context(&fixture_hierarchy(), SessionId(1));
    let running_turn = OperatorTurn {
        identity: OperatorTurnIdentity {
            conversation: seeded.id,
            session: seeded.session,
            run: SessionRunId(4),
            turn: OperatorTurnId(4),
        },
        state: OperatorTurnState::Running,
        sequence: 0,
    };
    let mut running_summary = seeded.clone();
    running_summary.run = SessionRunId(4);
    running_summary.current_turn = Some(running_turn.clone());
    running_summary.worker_live = true;
    running_summary.worker_stopped = false;

    let sum_follow = respond(
        &mut d,
        sum_req.request_id,
        OperatorResponse::TreeNavigated(Box::new(OperatorTreeNavigation {
            conversation: running_summary.clone(),
            current_leaf_id: Some("a2b".into()),
            noop: false,
            editor_text: Some("restart alpha worker".into()),
            restored_context: Some(target_ctx.clone()),
            submitted: Some(running_turn.clone()),
        })),
    );
    let inc_read = read_request(&sum_follow);
    respond(
        &mut d,
        inc_read.request_id,
        history_of(
            running_summary.clone(),
            vec![turn_message(
                4,
                7,
                OperatorMessageContent::BranchSummary {
                    target_entry_id: "u2a".into(),
                    instructions: Some("focus on auth".into()),
                },
            )],
            None,
        ),
    );
    let running_screen = draw_text(&mut d, 120, 32);
    assert!(
        running_screen.contains("Summarizing branch…")
            && running_screen.contains("Summarize branch · focus on auth")
            && !running_screen.contains("> restart alpha worker"),
        "while running, show `Summarizing branch…` without prematurely overwriting composer:\n{running_screen}"
    );

    // Complete Turn 3 -> triggers full `Read { after: 0 }` and restores composer + context!
    let mut completed_summary = running_summary.clone();
    completed_summary.current_turn = Some(OperatorTurn {
        state: OperatorTurnState::Complete,
        sequence: 2,
        ..running_turn
    });
    completed_summary.worker_live = false;
    completed_summary.worker_stopped = true;

    let changed_reqs = d.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(
        seeded.id,
    )));
    let tail_read = changed_reqs
        .into_iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::Read { after: 7, .. })
            )
        })
        .expect("incremental tail Read");
    let complete_follow = respond(
        &mut d,
        tail_read.request_id,
        history_of(
            completed_summary.clone(),
            vec![
                turn_message(
                    4,
                    8,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::BranchSummary {
                        summary: "Abandoned beta branch checked latency.".into(),
                    }),
                ),
                turn_message(
                    4,
                    9,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                        leased: false,
                        pi_first_entry_id: Some("bs3".into()),
                        pi_leaf_entry_id: Some("bs3".into()),
                    }),
                ),
            ],
            None,
        ),
    );
    let full_reload = complete_follow
        .iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::Read { after: 0, .. })
            )
        })
        .expect("completing branch summary must reload active-branch feed from after=0");
    respond(
        &mut d,
        full_reload.request_id,
        history_of(
            completed_summary.clone(),
            vec![
                user(1, "inspect logs", OperatorContext::default()),
                delta(2, "Found two sessions."),
                completed(3),
                turn_message(
                    4,
                    7,
                    OperatorMessageContent::BranchSummary {
                        target_entry_id: "u2a".into(),
                        instructions: Some("focus on auth".into()),
                    },
                ),
                turn_message(
                    4,
                    8,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::BranchSummary {
                        summary: "Abandoned beta branch checked latency.".into(),
                    }),
                ),
                turn_message(
                    4,
                    9,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                        leased: false,
                        pi_first_entry_id: Some("bs3".into()),
                        pi_leaf_entry_id: Some("bs3".into()),
                    }),
                ),
            ],
            None,
        ),
    );

    let completed_screen = draw_text(&mut d, 120, 36);
    assert!(
        completed_screen.contains("Branch summary recorded")
            && completed_screen.contains("Abandoned beta branch checked latency.")
            && !completed_screen.contains("Beta p99 is 12ms.")
            && completed_screen.contains("> restart alpha worker▏"),
        "expected branch summary receipt + active branch projection + composer restore:\n{completed_screen}"
    );
    d.key(KeyCode::Char('?'));
    let restored_ctx_screen = draw_text(&mut d, 120, 36);
    assert!(
        restored_ctx_screen.contains("Managing consigint / auth / #1 run 1")
            && restored_ctx_screen.contains("> restart alpha worker?▏"),
        "{restored_ctx_screen}"
    );

    // 4. Interrupted branch-summary turn renders interruption receipt without clobbering composer!
    let interrupted_turn = OperatorTurn {
        identity: OperatorTurnIdentity {
            conversation: seeded.id,
            session: seeded.session,
            run: SessionRunId(5),
            turn: OperatorTurnId(5),
        },
        state: OperatorTurnState::Interrupted,
        sequence: 1,
    };
    let mut interrupted_summary = completed_summary;
    interrupted_summary.run = SessionRunId(5);
    interrupted_summary.current_turn = Some(interrupted_turn);
    let int_reqs = d.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(
        seeded.id,
    )));
    let int_read = int_reqs
        .into_iter()
        .find(|m| {
            matches!(
                m.request,
                Request::Operator(OperatorRequest::Read { after: 9, .. })
            )
        })
        .unwrap();
    respond(
        &mut d,
        int_read.request_id,
        history_of(
            interrupted_summary,
            vec![
                turn_message(
                    5,
                    10,
                    OperatorMessageContent::BranchSummary {
                        target_entry_id: "u1".into(),
                        instructions: None,
                    },
                ),
                turn_message(5, 11, OperatorMessageContent::Interrupted),
            ],
            None,
        ),
    );
    let interrupted_screen = draw_text(&mut d, 120, 36);
    assert!(
        interrupted_screen
            .contains("Interrupted · branch summarization cancelled; current branch preserved.")
            && interrupted_screen.contains("> restart alpha worker?▏"),
        "{interrupted_screen}"
    );
}

#[test]
fn review_fixes_copy_completed_only_tree_ctrl_u_keymap_and_copy_notice_clear() {
    use ovrcr_protocol::{OperatorSessionTree, OperatorTreeEntry, OperatorTreeEntryKind};

    // 1. `/copy` ignores partial assistant text on Interrupted, Error, and Running turns!
    let mut d = dashboard_fixture();
    let mut state = with_model(summary());
    state.run = SessionRunId(3);
    state.current_turn = Some(OperatorTurn {
        identity: OperatorTurnIdentity {
            conversation: state.id,
            session: state.session,
            run: SessionRunId(3),
            turn: OperatorTurnId(3),
        },
        state: OperatorTurnState::Interrupted,
        sequence: 2,
    });
    let out = opened(&mut d, state.clone());
    let read = read_request(&out);
    respond(
        &mut d,
        read.request_id,
        history_of(
            state,
            vec![
                turn_message(
                    1,
                    1,
                    OperatorMessageContent::User {
                        skill: None,
                        text: "completed question".into(),
                        context: OperatorContext::default(),
                    },
                ),
                turn_message(
                    1,
                    2,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                        "Completed durable reply.".into(),
                    )),
                ),
                turn_message(
                    1,
                    3,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                        leased: false,
                        pi_first_entry_id: Some("u1".into()),
                        pi_leaf_entry_id: Some("a1".into()),
                    }),
                ),
                turn_message(
                    2,
                    4,
                    OperatorMessageContent::User {
                        skill: None,
                        text: "failed question".into(),
                        context: OperatorContext::default(),
                    },
                ),
                turn_message(
                    2,
                    5,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                        "Partial failed reply MUST NOT COPY.".into(),
                    )),
                ),
                turn_message(
                    2,
                    6,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::Failed("boom".into())),
                ),
                turn_message(
                    3,
                    7,
                    OperatorMessageContent::User {
                        skill: None,
                        text: "interrupted question".into(),
                        context: OperatorContext::default(),
                    },
                ),
                turn_message(
                    3,
                    8,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                        "Partial interrupted reply MUST NOT COPY.".into(),
                    )),
                ),
                turn_message(3, 9, OperatorMessageContent::Interrupted),
            ],
            None,
        ),
    );

    d.event_action(Event::Paste("/copy".into()));
    assert_eq!(
        d.key(KeyCode::Enter),
        DashboardAction::CopyText("Completed durable reply.".into()),
        "/copy must skip Failed and Interrupted partial replies and copy the latest Complete reply"
    );

    d.event_action(Event::Paste("/tree".into()));
    d.key(KeyCode::Enter);
    let tree_req = d
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Tree { .. })))
        .expect("expected Tree request");

    // Loading state advertises `Esc close`.
    let loading_tree = draw_text(&mut d, 140, 32);
    assert!(loading_tree.contains("Esc close"), "{loading_tree}");

    respond(
        &mut d,
        tree_req.request_id,
        OperatorResponse::Tree(Box::new(OperatorSessionTree {
            pi_session_id: Some("pi-7".into()),
            current_leaf_id: Some("a2".into()),
            entries: vec![
                OperatorTreeEntry {
                    id: "u1".into(),
                    parent_id: None,
                    kind: OperatorTreeEntryKind::User,
                    preview: "first prompt".into(),
                },
                OperatorTreeEntry {
                    id: "a1".into(),
                    parent_id: Some("u1".into()),
                    kind: OperatorTreeEntryKind::Assistant { tool_only: false },
                    preview: "first answer".into(),
                },
                OperatorTreeEntry {
                    id: "u2".into(),
                    parent_id: Some("a1".into()),
                    kind: OperatorTreeEntryKind::User,
                    preview: "second prompt".into(),
                },
                OperatorTreeEntry {
                    id: "a2".into(),
                    parent_id: Some("u2".into()),
                    kind: OperatorTreeEntryKind::Assistant { tool_only: false },
                    preview: "second answer".into(),
                },
            ],
        })),
    );

    // 3. Browse state advertises `Tab/Ctrl-f` + `Ctrl-u user-only` + `Ctrl-c clear` + `Esc close`,
    //    and `Ctrl-u` toggles `filter: user-only` <-> `filter: default` without deleting search text!
    d.event_action(Event::Paste("first".into()));
    let browse_screen = draw_text(&mut d, 140, 32);
    for expected in [
        "Tab/Ctrl-f",
        "Ctrl-u user-only",
        "Ctrl-c clear",
        "Esc close",
    ] {
        assert!(
            browse_screen.contains(expected),
            "expected `{expected}` in /tree Browse footer:\n{browse_screen}"
        );
    }
    d.ctrl('u');
    let user_only_screen = draw_text(&mut d, 140, 32);
    assert!(
        user_only_screen.contains("filter: user-only")
            && user_only_screen.contains("Search: first▏")
            && !user_only_screen.contains("first answer"),
        "Ctrl-u must toggle user-only filter while keeping search text:\n{user_only_screen}"
    );
    d.ctrl('u');
    let back_default = draw_text(&mut d, 140, 32);
    assert!(
        back_default.contains("filter: default") && back_default.contains("first answer"),
        "second Ctrl-u must return to default filter:\n{back_default}"
    );

    // 4. ConfirmSummarize advertises `Esc back`, and CustomInstructions advertises `Ctrl-c clear` + `Esc back`!
    d.ctrl('c'); // clear search -> selects `a2`
    d.key(KeyCode::Up); // selects `u2` (rewinding from `a2` prompts ConfirmSummarize)
    d.key(KeyCode::Enter);
    let confirm_screen = draw_text(&mut d, 140, 32);
    assert!(
        confirm_screen.contains("Summarize branch?") && confirm_screen.contains("Esc back"),
        "{confirm_screen}"
    );
    d.key(KeyCode::Down); // Summarize
    d.key(KeyCode::Down); // Summarize with custom prompt
    d.key(KeyCode::Enter);
    let custom_screen = draw_text(&mut d, 140, 32);
    for expected in ["Ctrl-c clear", "Esc back"] {
        assert!(
            custom_screen.contains(expected),
            "expected `{expected}` in CustomInstructions footer:\n{custom_screen}"
        );
    }

    // 5. Running turn with only streaming text has no completed reply -> `/copy` refuses cleanly!
    let mut running_dash = dashboard_fixture();
    let mut running_state = with_model(summary());
    running_state.run = SessionRunId(1);
    running_state.current_turn = Some(turn(1, OperatorTurnState::Running));
    let out_run = opened(&mut running_dash, running_state.clone());
    let read_run = read_request(&out_run);
    respond(
        &mut running_dash,
        read_run.request_id,
        history_of(
            running_state,
            vec![
                user(1, "streaming prompt", OperatorContext::default()),
                delta(2, "Still streaming partial reply..."),
            ],
            None,
        ),
    );
    running_dash.event_action(Event::Paste("/copy".into()));
    assert_eq!(running_dash.key(KeyCode::Enter), DashboardAction::Redraw);
    let running_copy_screen = draw_text(&mut running_dash, 140, 32);
    assert!(
        running_copy_screen.contains("No assistant reply to copy yet."),
        "streaming Running turn must not copy partial text:\n{running_copy_screen}"
    );
}

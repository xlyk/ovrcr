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
        id: OperatorConversationId(7),
        session: SessionId(99),
        model: None,
        title: "Operator".into(),
        current_turn: None,
        current_turn_started_unix_ms: None,
        last_reply_unix_ms: None,
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
        OperatorMessageContent::Worker(OperatorWorkerEvent::Completed),
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

/// The production draw path, retained as cells for real hit coordinates.
fn drawn_buffer(dashboard: &mut Dashboard, cols: u16, rows: u16) -> ratatui::buffer::Buffer {
    dashboard.install_area(Rect::new(0, 0, cols, rows));
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
    dashboard.draw(&mut terminal).unwrap();
    terminal.backend().buffer().clone()
}

fn buffer_row(buffer: &ratatui::buffer::Buffer, row: u16) -> String {
    (0..buffer.area.width)
        .map(|column| buffer[(column, row)].symbol())
        .collect()
}

fn operator_pane_row(buffer: &ratatui::buffer::Buffer, row: u16) -> String {
    buffer_row(buffer, row)
        .split_once('│')
        .map_or_else(String::new, |(_, pane)| pane.to_owned())
}

fn buffer_contains(buffer: &ratatui::buffer::Buffer, needle: &str) -> bool {
    (0..buffer.area.height).any(|row| buffer_row(buffer, row).contains(needle))
}

fn buffer_row_of(buffer: &ratatui::buffer::Buffer, needle: &str) -> u16 {
    (0..buffer.area.height)
        .find(|row| buffer_row(buffer, *row).contains(needle))
        .unwrap_or_else(|| panic!("missing {needle:?} in drawn buffer"))
}

fn buffer_column_of(buffer: &ratatui::buffer::Buffer, row: u16, needle: &str) -> u16 {
    let cells: Vec<&str> = (0..buffer.area.width)
        .map(|column| buffer[(column, row)].symbol())
        .collect();
    let wanted: Vec<char> = needle.chars().collect();
    (0..cells.len())
        .find(|start| {
            *start + wanted.len() <= cells.len()
                && cells[*start..*start + wanted.len()]
                    .iter()
                    .zip(&wanted)
                    .all(|(cell, ch)| cell.chars().eq(std::iter::once(*ch)))
        })
        .map(|column| column as u16)
        .unwrap_or_else(|| panic!("missing {needle:?} in row {row}"))
}

/// Sidebar rows belonging to the Operator section, excluding the shared pane.
fn dock_rows(rows: &[String]) -> Vec<String> {
    let left: Vec<_> = rows
        .iter()
        .map(|row| row.split('│').next().unwrap_or(row).to_string())
        .collect();
    let start = left
        .iter()
        .position(|row| row.contains("OPERATOR") || row.contains("Operator ·"))
        .expect("the Operator section is drawn");
    let height = if left[start].contains("OPERATOR") {
        3
    } else {
        1
    };
    left.into_iter().skip(start).take(height).collect()
}

fn dock_row(rows: &[String]) -> String {
    dock_rows(rows).join("\n")
}

fn dock_status_row(rows: &[String]) -> String {
    let rows = dock_rows(rows);
    rows[usize::from(rows[0].contains("OPERATOR"))].clone()
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
        dock.contains("OPERATOR") && dock.contains("Off"),
        "an unopened conversation has a section and Off state: {dock:?}"
    );
    assert!(
        dock.contains("o opens Operator"),
        "Off explains how to start: {dock:?}"
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
        dock.starts_with('▌'),
        "an open conversation keeps the selected section row: {dock:?}"
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
            message: "Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE.".into(),
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
    assert!(dock.starts_with('▌'), "{dock:?}");
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

    // The three-row section keeps the compact state and shows the current
    // activity as detail instead of adding another state label.
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
    assert!(dock.contains("running bash"), "{dock:?}");
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
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed),
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
    assert!(
        screen.contains("Context · consigint / auth / #1 run 1"),
        "{screen}"
    );
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
                OperatorMessageContent::Worker(OperatorWorkerEvent::Completed),
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
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed),
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
        dock.contains("OPERATOR"),
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
        dock.contains("OPERATOR") && dock.contains("Responding"),
        "{dock:?}"
    );
    assert!(
        dock.contains("running mcp__too…"),
        "detail clips within the narrow section: {dock:?}"
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
    assert!(rows[1].contains("OPERATOR"), "{rows:?}");
    assert!(rows[2].contains("Off"), "{rows:?}");
    assert!(rows[5].contains("CONSIGINT"), "{rows:?}");
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
    dashboard.mouse_action(mouse(38, 3), area);
    let requests = dashboard.drain_outbox();
    assert_eq!(operator_requests(&requests), [&OperatorRequest::Open]);
    // A second Manual Open is coalesced while the first is pending.
    dashboard.mouse_action(mouse(38, 3), area);
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
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ModelSelected(
                        OperatorModel {
                            provider: "openai".into(),
                            id: "gpt-6.1-sol".into(),
                        },
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
                    "Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE.".into(),
                )),
            )],
            None,
        ),
    );
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE."),
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
                        arguments: "{\"cmd\":\"ls\"}".into(),
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
                        .map(|row| format!("BULK_ROW_{row:02}\n"))
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
    assert!(screen.contains("Turn complete"), "{screen}");
    assert!(
        !screen.contains("\u{1b}]52;c;INJECTED"),
        "collapsed receipts never leak active controls: {screen}"
    );
    assert!(!screen.contains("INJECTED"), "{screen}");
    // Rendering the loaded tail acknowledges it once, and only then.
    let reviewed = reviewed_screen(&mut dashboard, 140, 40);
    assert!(!reviewed.contains("1 unread"), "{reviewed}");

    // Expanding the receipt shows the source escaped, under its own ordinal.
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::End);
    dashboard.key(KeyCode::Enter);
    let expanded = draw_text(&mut dashboard, 120, 40);
    assert!(expanded.contains("succeeded"), "{expanded}");
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
                .contains("▾ succeeded")
        })
        .expect("expanded receipt row is drawn at the tail");
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
        collapsed.contains("│"),
        "Working journal guide: {collapsed}"
    );

    // The captured target and the fixed agent workdir are separate disclosures.
    dashboard.key(KeyCode::Home);
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
            .contains("Turn complete")
    );
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::PageUp);
    let scrolled = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(!scrolled.contains("Turn complete"), "{scrolled}");
    for _ in 0..20 {
        dashboard.key(KeyCode::PageDown);
    }
    assert!(
        rendered_rows(&dashboard, 120, 40)
            .join("\n")
            .contains("Turn complete")
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
        .find(|row| {
            (row.contains("OPERATOR") || row.contains("Operator ·"))
                && !row.contains("operator-agent")
        })
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
                            format!("{}LATEST_DURABLE", "older history 界\n".repeat(50)),
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

#[test]
fn thinking_renders_as_a_distinct_block_before_the_answer() {
    let context = OperatorContext::default();
    let mut running = summary();
    running.run = SessionRunId(2);
    running.worker_stopped = false;
    running.current_turn = Some(turn(2, OperatorTurnState::Running));
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let out = opened(&mut dashboard, running.clone());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(
            running,
            vec![
                turn_message(
                    2,
                    1,
                    OperatorMessageContent::User {
                        text: "think about it".into(),
                        context,
                    },
                ),
                turn_message(
                    2,
                    2,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(
                        "First thought. ".into(),
                    )),
                ),
                turn_message(
                    2,
                    3,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(
                        "Second thought.".into(),
                    )),
                ),
                turn_message(
                    2,
                    4,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                        "Answer.".into(),
                    )),
                ),
            ],
            None,
        ),
    );
    assert!(out.is_empty(), "{out:?}");
    let screen = draw_text(&mut dashboard, 120, 40);
    let rows: Vec<&str> = screen.lines().collect();
    let thinking_label = rows
        .iter()
        .position(|row| row.contains("│ ▾ Thinking · 1 lines"))
        .unwrap_or_else(|| panic!("missing expanded short Thinking label: {screen}"));
    let thought = rows
        .iter()
        .position(|row| row.contains("First thought. Second thought."))
        .unwrap_or_else(|| panic!("missing streamed thinking: {screen}"));
    let operator_label = rows
        .iter()
        .position(|row| row.contains("│ Operator"))
        .unwrap_or_else(|| panic!("missing answer label: {screen}"));
    let answer = rows
        .iter()
        .position(|row| row.contains("│ Answer."))
        .unwrap_or_else(|| panic!("missing answer: {screen}"));
    assert!(
        thinking_label < thought && thought < operator_label && operator_label < answer,
        "visible order: {screen}"
    );
}

/// Forty numbered thinking lines across four adjacent deltas of one turn.
fn thinking_journal(turn: u64) -> Vec<OperatorMessage> {
    let lines: Vec<String> = (1..=40)
        .map(|number| format!("thinking line {number:02}"))
        .collect();
    let chunks: Vec<&[String]> = lines.chunks(10).collect();
    let mut messages = vec![turn_message(
        turn,
        1,
        OperatorMessageContent::User {
            text: "think it through".into(),
            context: OperatorContext::default(),
        },
    )];
    for (index, chunk) in chunks.iter().enumerate() {
        let mut text = chunk.join("\n");
        if index + 1 < chunks.len() {
            text.push('\n');
        }
        messages.push(turn_message(
            turn,
            index as u64 + 2,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(text)),
        ));
    }
    messages
}

fn ended(state: OperatorTurnState) -> OperatorConversationSummary {
    let mut value = summary();
    value.run = SessionRunId(2);
    value.current_turn = Some(turn(2, state));
    value
}

#[test]
fn thinking_streams_a_six_line_tail_then_collapses_when_the_turn_ends() {
    let mut running = summary();
    running.run = SessionRunId(2);
    running.worker_stopped = false;
    running.current_turn = Some(turn(2, OperatorTurnState::Running));
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let out = opened(&mut dashboard, running.clone());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(running, thinking_journal(2), None),
    );
    assert!(out.is_empty(), "{out:?}");

    // While the current turn runs, the block streams only its latest six rows.
    let screen = draw_text(&mut dashboard, 120, 40);
    let rows: Vec<&str> = screen.lines().collect();
    assert!(
        screen.contains("│ ▸ Thinking · showing last 6 of 40 lines"),
        "capped header: {screen}"
    );
    let visible: Vec<usize> = (35..=40)
        .map(|number| {
            rows.iter()
                .position(|row| row.contains(&format!("thinking line {number:02}")))
                .unwrap_or_else(|| panic!("missing thinking line {number}: {screen}"))
        })
        .collect();
    assert!(
        visible.windows(2).all(|pair| pair[0] < pair[1]),
        "the latest six rows keep their order: {screen}"
    );
    for hidden in 1..=34 {
        assert!(
            !screen.contains(&format!("thinking line {hidden:02}")),
            "only the six latest rows stream: {screen}"
        );
    }

    // Ending the turn through every terminal state collapses the same block.
    for state in [
        OperatorTurnState::Complete,
        OperatorTurnState::Error,
        OperatorTurnState::Interrupted,
    ] {
        let changed_out = changed(&mut dashboard, ended(state), Vec::new());
        assert!(changed_out.is_empty(), "{changed_out:?}");
        let screen = draw_text(&mut dashboard, 120, 40);
        assert!(
            screen.contains("│ ▸ Thinking · 40 lines"),
            "{state:?} collapses the block: {screen}"
        );
        assert!(
            !screen.contains("thinking line"),
            "{state:?} removes every body row: {screen}"
        );
    }

    // An older turn stays ended while a later turn runs.
    let mut later = summary();
    later.run = SessionRunId(3);
    later.worker_stopped = false;
    later.current_turn = Some(turn(3, OperatorTurnState::Running));
    let changed_out = changed(&mut dashboard, later, Vec::new());
    assert!(changed_out.is_empty(), "{changed_out:?}");
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("│ ▸ Thinking · 40 lines"),
        "an older block stays collapsed while another turn runs: {screen}"
    );
    assert!(!screen.contains("thinking line"), "{screen}");
}

#[test]
fn thinking_keyboard_overrides_survive_stream_completion_and_reopen() {
    let mut running = summary();
    running.run = SessionRunId(2);
    running.worker_stopped = false;
    running.current_turn = Some(turn(2, OperatorTurnState::Running));
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let out = opened(&mut dashboard, running.clone());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(running.clone(), thinking_journal(2), None),
    );
    assert!(out.is_empty(), "{out:?}");

    // Tab focuses the transcript, End selects the block, Enter expands it.
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::End);
    dashboard.key(KeyCode::Enter);
    let expanded = draw_text(&mut dashboard, 140, 70);
    assert!(
        expanded.contains("│ ▾ Thinking · 40 lines"),
        "explicit expansion: {expanded}"
    );
    for number in 1..=40 {
        assert!(
            expanded.contains(&format!("thinking line {number:02}")),
            "expanded body keeps line {number}: {expanded}"
        );
    }

    // Enter again collapses every body row, without dropping the choice.
    dashboard.key(KeyCode::Enter);
    let collapsed = draw_text(&mut dashboard, 120, 40);
    assert!(
        collapsed.contains("│ ▸ Thinking · 40 lines"),
        "explicit collapse: {collapsed}"
    );
    assert!(!collapsed.contains("thinking line"), "{collapsed}");

    // A later delta in the same block does not reopen the manual collapse.
    let out = changed(
        &mut dashboard,
        running.clone(),
        vec![turn_message(
            2,
            6,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(
                "\nlate thought".into(),
            )),
        )],
    );
    assert!(out.is_empty(), "{out:?}");
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("│ ▸ Thinking · 41 lines"),
        "a delta keeps the explicit collapse: {screen}"
    );
    assert!(!screen.contains("late thought"), "{screen}");

    // Up/Down traverse this many-delta block as one entry; Enter re-expands it.
    dashboard.key(KeyCode::Up);
    dashboard.key(KeyCode::Down);
    dashboard.key(KeyCode::Enter);
    let expanded = draw_text(&mut dashboard, 140, 70);
    assert!(
        expanded.contains("│ ▾ Thinking · 41 lines"),
        "the block stayed selected through Up/Down: {expanded}"
    );
    assert!(expanded.contains("late thought"), "{expanded}");
    assert!(expanded.contains("thinking line 01"), "{expanded}");

    // Explicit expansion survives the turn ending and reopening the view.
    let out = changed(
        &mut dashboard,
        ended(OperatorTurnState::Complete),
        Vec::new(),
    );
    assert!(out.is_empty(), "{out:?}");
    let completed = draw_text(&mut dashboard, 140, 70);
    assert!(
        completed.contains("│ ▾ Thinking · 41 lines"),
        "completion keeps the explicit expansion: {completed}"
    );
    assert!(completed.contains("late thought"), "{completed}");

    // Reopening goes through Browse, which is the only route that sends Open.
    dashboard.key(KeyCode::Esc);
    let out = opened(&mut dashboard, ended(OperatorTurnState::Complete));
    assert!(out.is_empty(), "reopening reads no new history: {out:?}");
    let reopened = draw_text(&mut dashboard, 140, 70);
    assert!(
        reopened.contains("│ ▾ Thinking · 41 lines"),
        "reopening keeps the explicit expansion: {reopened}"
    );
    assert!(reopened.contains("thinking line 01"), "{reopened}");
}

#[test]
fn thinking_short_stream_starts_expanded_and_first_enter_collapses() {
    let mut running = summary();
    running.run = SessionRunId(2);
    running.worker_stopped = false;
    running.current_turn = Some(turn(2, OperatorTurnState::Running));
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let out = opened(&mut dashboard, running.clone());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(
            running,
            vec![
                turn_message(
                    2,
                    1,
                    OperatorMessageContent::User {
                        text: "short request".into(),
                        context: OperatorContext::default(),
                    },
                ),
                turn_message(
                    2,
                    2,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(
                        "short first thought\nshort second thought".into(),
                    )),
                ),
            ],
            None,
        ),
    );
    assert!(out.is_empty(), "{out:?}");

    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(
        screen.contains("│ ▾ Thinking · 2 lines"),
        "an untouched short stream is open: {screen}"
    );
    assert!(screen.contains("short first thought"), "{screen}");
    assert!(screen.contains("short second thought"), "{screen}");

    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::End);
    dashboard.key(KeyCode::Enter);
    let collapsed = draw_text(&mut dashboard, 120, 40);
    assert!(
        collapsed.contains("│ ▸ Thinking · 2 lines"),
        "the first Enter collapses the short block: {collapsed}"
    );
    assert!(!collapsed.contains("short first thought"), "{collapsed}");
    assert!(!collapsed.contains("short second thought"), "{collapsed}");
}

/// Numbered thinking deltas split into chunks, concatenating back into one block.
fn thinking_deltas(
    turn: u64,
    first_ordinal: u64,
    prefix: &str,
    count: usize,
    chunks: usize,
) -> Vec<OperatorMessage> {
    let lines: Vec<String> = (1..=count)
        .map(|number| format!("{prefix} line {number:02}"))
        .collect();
    let per_chunk = lines.len().div_ceil(chunks).max(1);
    let chunks: Vec<&[String]> = lines.chunks(per_chunk).collect();
    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let mut text = chunk.join("\n");
            if index + 1 < chunks.len() {
                text.push('\n');
            }
            turn_message(
                turn,
                first_ordinal + index as u64,
                OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(text)),
            )
        })
        .collect()
}

#[test]
fn thinking_blocks_toggle_independently_by_mouse() {
    let context = OperatorContext::default();
    let mut running = summary();
    running.run = SessionRunId(2);
    running.worker_stopped = false;
    running.current_turn = Some(turn(2, OperatorTurnState::Running));
    let mut messages = vec![
        turn_message(
            1,
            1,
            OperatorMessageContent::User {
                text: "older request".into(),
                context: context.clone(),
            },
        ),
        turn_message(
            1,
            2,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(
                "older one\nolder two".into(),
            )),
        ),
        turn_message(
            1,
            3,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed),
        ),
        turn_message(
            2,
            4,
            OperatorMessageContent::User {
                text: "current request".into(),
                context,
            },
        ),
    ];
    messages.extend(thinking_deltas(2, 5, "alpha", 40, 4));
    messages.push(turn_message(
        2,
        9,
        OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
            "between blocks".into(),
        )),
    ));
    messages.extend(thinking_deltas(2, 10, "beta", 8, 2));
    messages.push(turn_message(
        2,
        12,
        OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta("final answer".into())),
    ));

    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 160, 80));
    let out = opened(&mut dashboard, running.clone());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(running, messages, None),
    );
    assert!(out.is_empty(), "{out:?}");

    let initial = drawn_buffer(&mut dashboard, 160, 80);
    assert!(buffer_contains(
        &initial,
        "▸ Thinking · showing last 6 of 40 lines"
    ));
    assert!(buffer_contains(
        &initial,
        "▸ Thinking · showing last 6 of 8 lines"
    ));
    assert!(buffer_contains(&initial, "▸ Thinking · 2 lines"));
    assert!(buffer_contains(&initial, "alpha line 40"));
    assert!(buffer_contains(&initial, "beta line 08"));
    assert!(!buffer_contains(&initial, "alpha line 34"));
    assert!(!buffer_contains(&initial, "beta line 02"));
    assert!(!buffer_contains(&initial, "older one"));
    let alpha_header_row = buffer_row_of(&initial, "showing last 6 of 40");
    let beta_header = operator_pane_row(&initial, buffer_row_of(&initial, "showing last 6 of 8"));

    // The capped body row belongs to a later alpha delta, but toggles this block.
    let alpha_body_row = buffer_row_of(&initial, "alpha line 40");
    let alpha_body_column = buffer_column_of(&initial, alpha_body_row, "alpha line 40");
    dashboard.mouse_action(
        mouse(alpha_body_column, alpha_body_row),
        Rect::new(0, 0, 160, 80),
    );

    let expanded = drawn_buffer(&mut dashboard, 160, 80);
    assert_eq!(
        buffer_row_of(&expanded, "▾ Thinking · 40 lines"),
        alpha_header_row,
        "expanding keeps the header on its screen row"
    );
    assert!(buffer_contains(&expanded, "alpha line 01"));
    assert!(buffer_contains(&expanded, "alpha line 40"));
    assert!(buffer_contains(&expanded, "beta line 08"));
    assert_eq!(
        operator_pane_row(&expanded, buffer_row_of(&expanded, "showing last 6 of 8")),
        beta_header,
        "the second block keeps its capped display"
    );
    assert!(!buffer_contains(&expanded, "beta line 02"));
    assert!(buffer_contains(&expanded, "▸ Thinking · 2 lines"));
    assert!(!buffer_contains(&expanded, "older one"));

    // Clicking a visible body row collapses the same block in place.
    let alpha_body_row = buffer_row_of(&expanded, "alpha line 01");
    let alpha_body_column = buffer_column_of(&expanded, alpha_body_row, "alpha line 01");
    dashboard.mouse_action(
        mouse(alpha_body_column, alpha_body_row),
        Rect::new(0, 0, 160, 80),
    );
    let collapsed = drawn_buffer(&mut dashboard, 160, 80);
    assert_eq!(
        buffer_row_of(&collapsed, "▸ Thinking · 40 lines"),
        alpha_header_row,
        "collapsing keeps the header on its screen row"
    );
    assert!(!buffer_contains(&collapsed, "alpha line"));
    assert_eq!(
        operator_pane_row(&collapsed, buffer_row_of(&collapsed, "showing last 6 of 8")),
        beta_header,
        "the second block is untouched by the first block's collapse"
    );
    assert!(buffer_contains(&collapsed, "▸ Thinking · 2 lines"));
    assert!(!buffer_contains(&collapsed, "older one"));
}

#[test]
fn thinking_viewport_keeps_the_header_while_scrolling_and_collapsing() {
    let context = OperatorContext::default();
    let filler = (1..=20)
        .map(|number| format!("filler line {number:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut running = summary();
    running.run = SessionRunId(3);
    running.worker_stopped = false;
    running.current_turn = Some(turn(3, OperatorTurnState::Running));
    let mut messages = vec![
        turn_message(
            1,
            1,
            OperatorMessageContent::User {
                text: "older request".into(),
                context: context.clone(),
            },
        ),
        turn_message(
            1,
            2,
            OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(filler)),
        ),
        turn_message(
            1,
            3,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed),
        ),
        turn_message(
            2,
            4,
            OperatorMessageContent::User {
                text: "scroll request".into(),
                context,
            },
        ),
    ];
    messages.extend(thinking_deltas(3, 5, "alpha", 40, 4));

    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    let out = opened(&mut dashboard, running.clone());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(running.clone(), messages, None),
    );
    assert!(out.is_empty(), "{out:?}");

    // The capped header sits near the bottom of the tail viewport.
    let initial = drawn_buffer(&mut dashboard, 120, 40);
    assert!(buffer_contains(&initial, "filler line 20"));
    assert!(buffer_contains(&initial, "alpha line 40"));
    let header_row = buffer_row_of(&initial, "showing last 6 of 40");
    assert!(
        header_row > 20,
        "the capped header starts near the viewport bottom: {header_row}"
    );

    // Expanding taller than the viewport keeps that header on the same row.
    dashboard.key(KeyCode::Tab);
    dashboard.key(KeyCode::End);
    dashboard.key(KeyCode::Enter);
    let expanded = drawn_buffer(&mut dashboard, 120, 40);
    assert_eq!(
        buffer_row_of(&expanded, "▾ Thinking · 40 lines"),
        header_row,
        "expansion keeps the header's screen row"
    );
    assert!(buffer_contains(&expanded, "alpha line 01"));
    assert!(!buffer_contains(&expanded, "alpha line 40"));

    // Collapsing must not snap to the tail; the header stays put.
    dashboard.key(KeyCode::Enter);
    let collapsed = drawn_buffer(&mut dashboard, 120, 40);
    assert_eq!(
        buffer_row_of(&collapsed, "▸ Thinking · 40 lines"),
        header_row,
        "collapse keeps the header's screen row instead of snapping"
    );
    assert!(!buffer_contains(&collapsed, "alpha line"));

    // A later answer extends the one transcript; PageDown and the wheel reach it.
    let answer = (1..=20)
        .map(|number| format!("answer line {number:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let out = changed(
        &mut dashboard,
        running.clone(),
        vec![turn_message(
            3,
            9,
            OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(answer)),
        )],
    );
    assert!(out.is_empty(), "{out:?}");
    let answered = drawn_buffer(&mut dashboard, 120, 40);
    assert!(!buffer_contains(&answered, "answer line 20"));
    dashboard.key(KeyCode::PageDown);
    let page = drawn_buffer(&mut dashboard, 120, 40);
    assert!(!buffer_contains(&page, "answer line 20"));
    let wheel = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 60,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    for _ in 0..4 {
        dashboard.mouse_action(wheel, Rect::new(0, 0, 120, 40));
    }
    let wheeled = drawn_buffer(&mut dashboard, 120, 40);
    assert!(buffer_contains(&wheeled, "answer line 20"));

    // Expanding again and scrolling into the body puts its header above the
    // viewport; collapsing by a visible body row lands the header at the top.
    dashboard.key(KeyCode::Enter);
    for _ in 0..3 {
        dashboard.key(KeyCode::PageDown);
    }
    let scrolled = drawn_buffer(&mut dashboard, 120, 40);
    assert!(!buffer_contains(&scrolled, "▸ Thinking · 40 lines"));
    let body_row = buffer_row_of(&scrolled, "alpha line 35");
    let body_column = buffer_column_of(&scrolled, body_row, "alpha line 35");
    dashboard.mouse_action(mouse(body_column, body_row), Rect::new(0, 0, 120, 40));
    let top = drawn_buffer(&mut dashboard, 120, 40);
    assert_eq!(
        buffer_row_of(&top, "▸ Thinking · 40 lines"),
        2,
        "the collapsed header lands at the transcript's top row"
    );
    assert!(buffer_contains(&top, "answer line 20") || buffer_contains(&top, "Responding"));
}

#[test]
fn thinking_capped_header_clips_with_its_count_at_a_narrow_sidebar_view() {
    let mut running = summary();
    running.run = SessionRunId(2);
    running.worker_stopped = false;
    running.current_turn = Some(turn(2, OperatorTurnState::Running));
    let mut dashboard = dashboard_fixture();
    let out = opened(&mut dashboard, running.clone());
    let read = read_request(&out);
    let out = respond(
        &mut dashboard,
        read.request_id,
        history_of(running, thinking_journal(2), None),
    );
    assert!(out.is_empty(), "{out:?}");

    // The on-demand ordinary sidebar halves the 60-column conversation.
    dashboard.install_area(Rect::new(0, 0, 60, 16));
    dashboard.ctrl('l');
    let screen = draw_text(&mut dashboard, 60, 16);
    assert!(
        screen.contains("▸ Thinking · showing last 6…"),
        "the clipped header keeps its partial styled count: {screen}"
    );
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

//! The Operator is one pi conversation. These cases drive the production
//! Dashboard entry points: the fixed navigation row, the full view, Quick
//! message, Stop and history paging. There is no create form, chat list,
//! switching or archive action to exercise.
use super::*;
use crossterm::event::ModifierKeyCode;
use ovrcr_protocol::{
    OperatorContext, OperatorConversationId, OperatorConversationSummary, OperatorHistory,
    OperatorMessage, OperatorMessageContent, OperatorModel, OperatorRequest, OperatorResponse,
    OperatorToolStatus, OperatorTurn, OperatorTurnId, OperatorTurnIdentity, OperatorTurnState,
    OperatorWorkerEvent, SessionKind, SessionRunId,
};

const BUSY_REASON: &str = "The Operator is busy or its worker stop is unconfirmed; draft retained.";

fn control_event(kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(
        KeyCode::Modifier(ModifierKeyCode::LeftControl),
        KeyModifiers::NONE,
        kind,
    ))
}

fn summary() -> OperatorConversationSummary {
    OperatorConversationSummary {
        run: SessionRunId(1),
        worker_stopped: true,
        id: OperatorConversationId(7),
        session: SessionId(99),
        model: None,
        title: "Operator".into(),
        current_turn: None,
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

#[test]
fn operator_is_one_navigation_row_that_opens_the_singleton() {
    let area = Rect::new(0, 0, 120, 40);
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(area);
    dashboard.install_hierarchy(fixture_hierarchy());
    dashboard.install_focus(SessionId(1));

    let rows = rendered_rows(&dashboard, 120, 40);
    // One fixed Operator row sits directly above the Projects tree.
    assert!(rows[1].contains("Operator"), "{rows:?}");
    assert!(rows[1].contains("not opened"), "{rows:?}");
    assert!(rows[2].contains("CONSIGINT"), "{rows:?}");
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

    // The row is navigation, including its right edge; there is no close target.
    dashboard.mouse_action(mouse(38, 1), area);
    let requests = dashboard.drain_outbox();
    assert_eq!(operator_requests(&requests), [&OperatorRequest::Open]);
    // A second Manual Open is coalesced while the first is pending.
    dashboard.mouse_action(mouse(38, 1), area);
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

    // The title-bar control opens the same singleton through Open, not a list.
    dashboard.mouse_action(mouse(9, 0), area);
    let requests = dashboard.drain_outbox();
    assert_eq!(operator_requests(&requests), [&OperatorRequest::Open]);
    let header = requests.into_iter().next().unwrap();
    respond(
        &mut dashboard,
        header.request_id,
        OperatorResponse::Opened(summary()),
    );

    // Ctrl-n is not a shortcut: it neither creates nor resets anything.
    dashboard.event_action(Event::Paste("kept".into()));
    dashboard.ctrl('n');
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.ctrl('g');
    dashboard.ctrl('n');
    assert!(dashboard.drain_outbox().is_empty());

    // Browse help/menu describes the singleton and never a chat list.
    dashboard.key(KeyCode::Char(' '));
    let menu = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(menu.contains("Operator"), "{menu}");
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
    assert!(!screen.contains("unknown"), "{screen}");

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
    dashboard.event_action(control_event(KeyEventKind::Press));
    dashboard.event_action(Event::Key(KeyEvent::new(
        KeyCode::Backspace,
        KeyModifiers::SUPER,
    )));
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(screen.contains("draft ▏界"), "{screen}");
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.event_action(control_event(KeyEventKind::Release));
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
fn title_bar_operator_control_opens_the_singleton_on_narrow_and_short_screens() {
    for (cols, rows) in [(55, 15), (80, 3), (120, 24)] {
        let mut dashboard = Dashboard::new(TerminalSize { rows, cols });
        let area = Rect::new(0, 0, cols, rows);
        dashboard.install_area(area);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
        dashboard.draw(&mut terminal).unwrap();
        let row: String = (0..cols)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect();
        assert!(row.contains("Operator"), "{cols}x{rows}: {row}");
        dashboard.mouse_action(mouse(9, 0), area);
        assert!(
            dashboard
                .drain_outbox()
                .iter()
                .any(|message| matches!(message.request, Request::Operator(OperatorRequest::Open))),
            "header mouse target opens the singleton, never a list"
        );
    }
}

#[test]
fn leaving_operator_restores_terminal_input_and_keeps_the_draft() {
    let area = Rect::new(0, 0, 120, 40);
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

    dashboard.ctrl('g');
    let screen = draw_text(&mut dashboard, 120, 40);
    assert!(!screen.contains("Managing"), "{screen}");
    assert!(!screen.contains("New Conversation"), "{screen}");
    assert!(rendered_footer(&dashboard, 120).starts_with("BROWSE"));
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
    // `o` returns to the same conversation and draft.
    dashboard.ctrl('g');
    dashboard.key(KeyCode::Char('o'));
    assert!(draw_text(&mut dashboard, 120, 40).contains("-draft"));
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
    let mut dashboard = Dashboard::with_input_profile(
        TerminalSize {
            rows: 40,
            cols: 120,
        },
        super::DashboardInputProfile::VerifiedOvrcrGui,
    );
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

    dashboard.event_action(control_event(KeyEventKind::Press));
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
        .expect("the fixed Operator navigation row");
    assert!(
        !operator_row.contains("[1]"),
        "the singleton row is not a session target: {operator_row}"
    );
    assert!(
        rendered_footer(&dashboard, 120).contains("Ctrl+1–9 jump"),
        "the footer explains the active gesture"
    );

    // Cancelling with a non-digit Control chord keeps the draft and cursor.
    dashboard.event_action(Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    let after_ctrl_c = rendered_rows(&dashboard, 120, 40).join("\n");
    assert!(
        !after_ctrl_c.contains("draft"),
        "Ctrl+C keeps its ordinary Operator draft-clear behavior: {after_ctrl_c}"
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(dashboard.drain_outbox().is_empty());
    dashboard.event_action(control_event(KeyEventKind::Release));

    // The jump selects the Agent, never the Operator conversation.
    dashboard.event_action(control_event(KeyEventKind::Press));
    dashboard.event_action(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('1'),
        KeyModifiers::CONTROL,
        KeyEventKind::Press,
    )));
    assert_eq!(dashboard.focused_session(), Some(SessionId(4)));
    assert!(rendered_footer(&dashboard, 120).contains("Session selected"));
    let view = only_request(dashboard.drain_outbox());
    assert!(matches!(
        &view.request,
        Request::SetView { view } if view.focused == Some(SessionId(4))
    ));
    dashboard.event_action(control_event(KeyEventKind::Release));

    // Returning to the shell keeps the conversation, draft and scroll position.
    dashboard.ctrl('g');
    dashboard.key(KeyCode::Char('o'));
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
fn operator_control_jump_does_not_reveal_its_on_demand_sidebar() {
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

    let mut dashboard = Dashboard::with_input_profile(
        TerminalSize { rows: 24, cols: 80 },
        super::DashboardInputProfile::VerifiedOvrcrGui,
    );
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_area(Rect::new(0, 0, 80, 24));
    dashboard.install_focus(SessionId(3));
    open_and_settle(&mut dashboard, summary());

    dashboard.event_action(control_event(KeyEventKind::Press));
    dashboard.event_action(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char('1'),
        KeyModifiers::CONTROL,
        KeyEventKind::Press,
    )));
    dashboard.event_action(control_event(KeyEventKind::Release));
    let hidden = rendered_rows(&dashboard, 80, 24).join("\n");
    assert!(
        !hidden.contains("operator-agent"),
        "Control cannot reveal the sidebar: {hidden}"
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));

    // At narrow widths Ctrl-L explicitly opens the ordinary sidebar.
    dashboard.ctrl('l');
    dashboard.event_action(control_event(KeyEventKind::Press));
    let open = rendered_rows(&dashboard, 80, 24);
    let agent_row = open
        .iter()
        .find(|row| row.contains("operator-agent"))
        .expect("Agent becomes visible after the user opens the sidebar");
    assert!(
        agent_row.contains("[1]"),
        "visible Agent receives a label: {agent_row}"
    );
    let operator_row = open
        .iter()
        .find(|row| row.contains("Operator") && !row.contains("operator-agent"))
        .expect("the fixed Operator navigation row");
    assert!(!operator_row.contains("[1]"), "{operator_row}");
    dashboard.event_action(control_event(KeyEventKind::Release));
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
        let mut dashboard = Dashboard::with_input_profile(
            TerminalSize { rows, cols },
            super::DashboardInputProfile::VerifiedOvrcrGui,
        );
        dashboard.install_hierarchy(hierarchy);
        dashboard.install_area(area);
        dashboard.install_focus(SessionId(3));
        open_and_settle(&mut dashboard, summary());
        if cols < 100 {
            // Operator's on-demand sidebar must be opened explicitly at narrow widths.
            dashboard.ctrl('l');
        }

        let before = rendered_rows(&dashboard, cols, rows);
        if below_operator_floor {
            let text = before
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                text.contains("60×16") && text.contains("Resize to continue"),
                "{cols}×{rows}: existing Operator resize guidance remains: {text}"
            );
        }
        let before_agent = before
            .iter()
            .find(|row| row.contains("jump-target"))
            .unwrap_or_else(|| panic!("{cols}×{rows}: visible Agent row missing: {before:?}"));
        let status_and_title = before_agent.chars().skip(5).collect::<String>();
        dashboard.event_action(control_event(KeyEventKind::Press));
        let held = rendered_rows(&dashboard, cols, rows);
        if below_operator_floor {
            let text = held
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                text.contains("60×16") && text.contains("Resize to continue"),
                "{cols}×{rows}: Control preserves Operator resize guidance: {text}"
            );
        }
        let after_agent = held
            .iter()
            .find(|row| row.contains("jump-target"))
            .unwrap_or_else(|| panic!("{cols}×{rows}: Agent row disappeared: {held:?}"));
        if below_operator_floor {
            assert!(
                !after_agent.contains("[1]"),
                "{cols}×{rows}: below the Operator floor suppress labels: {after_agent}"
            );
            assert!(
                !rendered_footer(&dashboard, cols).contains("Ctrl+1–9"),
                "{cols}×{rows}: the gesture hint stays hidden below the floor"
            );
            dashboard.event_action(Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('1'),
                KeyModifiers::CONTROL,
                KeyEventKind::Press,
            )));
            assert_eq!(
                dashboard.focused_session(),
                Some(SessionId(3)),
                "{cols}×{rows}: Control digits cannot select below the Operator floor"
            );
        } else {
            assert!(
                after_agent.contains("[1]"),
                "{cols}×{rows}: visible Agent gets a label: {after_agent}"
            );
        }
        assert_eq!(
            after_agent.chars().skip(5).collect::<String>(),
            status_and_title,
            "{cols}×{rows}: title/status columns stay fixed"
        );
        assert_eq!(held.len(), before.len(), "{cols}×{rows}: rows do not move");
        dashboard.event_action(control_event(KeyEventKind::Release));
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
    assert!(wide[0].contains("Operator [o]"), "{wide_text}");
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
fn sidebar_reserves_one_row_only_when_four_lines_remain() {
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 40,
        cols: 120,
    });
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    dashboard.install_hierarchy(fixture_hierarchy());
    dashboard.install_focus(SessionId(1));

    // 6 rows leave 4 sidebar lines: the fixed row plus three tree lines.
    let roomy = rendered_rows(&dashboard, 120, 6);
    assert!(roomy[1].contains("not opened"), "{roomy:?}");
    assert!(
        roomy[2].contains("CONSIGINT"),
        "tree starts under the row: {roomy:?}"
    );
    // 5 rows leave 3: the row is not drawn, and the tree takes the space.
    let tight = rendered_rows(&dashboard, 120, 5);
    assert!(
        !tight.iter().skip(1).any(|row| row.contains("not opened")),
        "{tight:?}"
    );
    assert!(tight[1].contains("CONSIGINT"), "{tight:?}");

    // The row is one line whatever the activity and unread state.
    let area = Rect::new(0, 0, 120, 40);
    let before = rendered_rows(&dashboard, 120, 40);
    open_and_settle(&mut dashboard, busy());
    let busy_rows = rendered_rows(&dashboard, 120, 40);
    assert_eq!(busy_rows.len(), before.len());
    assert!(busy_rows[1].contains("Responding"), "{busy_rows:?}");
    assert!(
        !busy_rows[1].contains('▾') && !busy_rows[1].contains('▸'),
        "no collapse arrow: {busy_rows:?}"
    );
    // The right edge is navigation, never a close hit target.
    dashboard.mouse_action(mouse(38, 1), area);
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
        // The quota section sits below the tree on row 17.
        assert_eq!(
            selected,
            vec![16],
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

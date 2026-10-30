use super::*;
use ovrcr_protocol::{
    OperatorConversationId, OperatorConversationSummary, OperatorProvider, OperatorRequest,
    OperatorResponse, SessionRunId,
};

#[test]
fn operator_quick_opens_over_every_input_mode_without_pty_bytes() {
    for mode in [
        "Browse",
        "Terminal",
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
            "Terminal" => {
                d.key(KeyCode::Enter);
            }
            "History" => {
                let request = d.key(KeyCode::PageUp);
                if let DashboardAction::Request(request) = request {
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

#[test]
fn native_operator_submission_captures_stable_context_and_never_pty_input() {
    let mut dashboard = Dashboard::new(TerminalSize {
        rows: 24,
        cols: 120,
    });
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
    let expected = ovrcr_protocol::OperatorContext {
        project: Some(project.name.clone()),
        workspace: Some(workspace.name.clone()),
        workspace_id: Some(workspace.id.clone()),
        cwd: Some(session.cwd.clone()),
        session: Some(session.id),
        run: Some(session.run),
    };
    dashboard.install_hierarchy(hierarchy);
    dashboard.install_focus(SessionId(1));
    assert!(!matches!(
        dashboard.key(KeyCode::Char('o')),
        DashboardAction::PtyBytes(_)
    ));
    dashboard.drain_outbox();
    assert!(!matches!(
        dashboard.key(KeyCode::Enter),
        DashboardAction::PtyBytes(_)
    ));
    let create = dashboard
        .drain_outbox()
        .into_iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Create { .. })
            )
        })
        .unwrap();
    assert_eq!(
        create.request,
        Request::Operator(OperatorRequest::Create {
            provider: OperatorProvider::ChatGpt,
            model: "gpt-6.1-sol".into()
        })
    );
    let conversation = OperatorConversationSummary {
        run: SessionRunId(0),
        archived: false,
        worker_stopped: true,
        id: OperatorConversationId(7),
        session: SessionId(99),
        provider: OperatorProvider::ChatGpt,
        model: "gpt-6.1-sol".into(),
        title: "New Conversation".into(),
        current_turn: None,
    };
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: create.request_id,
        response: Response::Operator(OperatorResponse::Created(conversation)),
    });
    dashboard.drain_outbox();
    assert!(!matches!(
        dashboard.event_action(Event::Paste("hello\n界".into())),
        DashboardAction::PtyBytes(_)
    ));
    assert!(dashboard.drain_outbox().is_empty(), "paste cannot submit");
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
        .unwrap();
    assert_eq!(
        submit.request,
        Request::Operator(OperatorRequest::Submit {
            conversation: OperatorConversationId(7),
            expected_run: SessionRunId(0),
            text: "hello\n界".into(),
            context: expected.clone()
        })
    );
    dashboard.install_focus(SessionId(3));
    assert_eq!(
        submit.request,
        Request::Operator(OperatorRequest::Submit {
            conversation: OperatorConversationId(7),
            expected_run: SessionRunId(0),
            text: "hello\n界".into(),
            context: expected
        })
    );
    assert!(!requests.iter().any(|message| matches!(
        message.request,
        Request::Input { .. } | Request::AssignedHumanInput { .. }
    )));
}

#[test]
fn pinned_operator_control_has_matching_mouse_target_on_narrow_and_short_screens() {
    for (cols, rows) in [(55, 15), (80, 3), (120, 24)] {
        let mut dashboard = Dashboard::new(TerminalSize { rows, cols });
        let area = Rect::new(0, 0, cols, rows);
        dashboard.install_area(area);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
        dashboard.draw(&mut terminal).unwrap();
        let row: String = (0..cols)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect();
        assert!(row.contains("Operator [o]"), "{cols}x{rows}: {row}");
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 9,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert!(
            dashboard.drain_outbox().iter().any(|message| matches!(
                message.request,
                Request::Operator(OperatorRequest::List { .. })
            )),
            "header mouse target opens native conversations"
        );
    }
}

#[test]
fn operator_sidebar_heading_precedes_projects_in_native_dashboard() {
    let mut dashboard = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
    dashboard.install_hierarchy(fixture_hierarchy());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    dashboard.draw(&mut terminal).unwrap();
    let heading: String = (0..39)
        .map(|x| terminal.backend().buffer()[(x, 1)].symbol())
        .collect();
    assert!(heading.contains("OPERATOR"), "{heading}");
}

#[test]
fn operator_working_journal_input_geometry_preserves_context() {
    let mut d = Dashboard::new(TerminalSize {
        rows: 24,
        cols: 120,
    });
    d.install_hierarchy(fixture_hierarchy());
    d.install_focus(SessionId(1));
    d.key(KeyCode::Char('o'));
    let list = d
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::List { .. })))
        .unwrap();
    // Exercise native creation before selecting independently loaded saved chats.
    d.ctrl('n');
    d.key(KeyCode::Enter);
    let create = d
        .drain_outbox()
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Create { .. })))
        .unwrap();
    d.handle_server_message(ServerMessage::Response {
        request_id: create.request_id,
        response: Response::Operator(OperatorResponse::Created(OperatorConversationSummary {
            id: OperatorConversationId(3),
            session: SessionId(93),
            run: SessionRunId(0),
            archived: false,
            worker_stopped: true,
            provider: OperatorProvider::ChatGpt,
            model: "fixture".into(),
            title: "created".into(),
            current_turn: None,
        })),
    });
    let summaries: Vec<_> = (1..=2)
        .map(|id| OperatorConversationSummary {
            id: OperatorConversationId(id),
            session: SessionId(90 + id),
            run: SessionRunId(1),
            archived: false,
            worker_stopped: true,
            provider: OperatorProvider::ChatGpt,
            model: "fixture".into(),
            title: format!("chat-{id}"),
            current_turn: Some(ovrcr_protocol::OperatorTurn {
                identity: ovrcr_protocol::OperatorTurnIdentity {
                    conversation: OperatorConversationId(id),
                    turn: ovrcr_protocol::OperatorTurnId(1),
                    session: SessionId(90 + id),
                    run: SessionRunId(1),
                },
                state: ovrcr_protocol::OperatorTurnState::Complete,
                sequence: 1,
            }),
        })
        .collect();
    let reads = d.handle_server_message(ServerMessage::Response {
        request_id: list.request_id,
        response: Response::Operator(OperatorResponse::Conversations(summaries.clone())),
    });
    for request in reads {
        if let Request::Operator(OperatorRequest::Read { conversation, .. }) = request.request {
            d.handle_server_message(ServerMessage::Response {
                request_id: request.request_id,
                response: Response::Operator(OperatorResponse::History(
                    ovrcr_protocol::OperatorHistory {
                        conversation: summaries[conversation.0 as usize - 1].clone(),
                        messages: vec![
                            ovrcr_protocol::OperatorMessage {
                                ordinal: 1,
                                turn: ovrcr_protocol::OperatorTurnId(1),
                                content: ovrcr_protocol::OperatorMessageContent::Worker(
                                    ovrcr_protocol::OperatorWorkerEvent::ToolCall {
                                        id: "INTEGRATION_CALL_ID".into(),
                                        name: "Bash".into(),
                                        arguments: "INTEGRATION_ARGUMENTS".into(),
                                    },
                                ),
                            },
                            ovrcr_protocol::OperatorMessage {
                                ordinal: 2,
                                turn: ovrcr_protocol::OperatorTurnId(1),
                                content: ovrcr_protocol::OperatorMessageContent::Worker(
                                    ovrcr_protocol::OperatorWorkerEvent::AssistantDelta(
                                        (0..60)
                                            .map(|n| {
                                                format!("CHAT_{}_ROW_{n:02}\n", conversation.0)
                                            })
                                            .collect(),
                                    ),
                                ),
                            },
                        ],
                        next_after: None,
                    },
                )),
            });
        }
    }
    d.drain_outbox();
    let area = Rect::new(0, 0, 120, 24);
    d.install_area(area);
    let mut anchors = Vec::new();
    for (row, draft) in [(2, "draft A\n界é🙂"), (3, "draft B\n猫")] {
        d.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 4,
                row,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        d.key(KeyCode::Enter);
        assert!(!matches!(
            d.event_action(Event::Paste(draft.into())),
            DashboardAction::PtyBytes(_)
        ));
        d.key(KeyCode::Tab);
        d.key(KeyCode::Tab);
        d.key(KeyCode::Home);
        d.key(KeyCode::Enter);
        let mut receipt = Terminal::new(TestBackend::new(120, 24)).unwrap();
        d.draw(&mut receipt).unwrap();
        let expanded_screen = receipt
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(
            expanded_screen.contains("INTEGRATION_ARGUMENTS"),
            "expanded receipt at sidebar {row}: {expanded_screen}"
        );
        d.key(KeyCode::Tab);
        d.key(KeyCode::PageDown);
        d.key(KeyCode::PageDown);
        d.key(KeyCode::PageDown);
        d.key(KeyCode::PageDown);
        d.key(KeyCode::PageDown);
        d.key(KeyCode::PageDown);
        d.key(KeyCode::PageUp);
        if row == 3 {
            d.key(KeyCode::PageUp);
        }
        let mut saved = Terminal::new(TestBackend::new(120, 24)).unwrap();
        d.draw(&mut saved).unwrap();
        let screen = saved
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert_eq!(
            screen.matches('✓').count(),
            2,
            "unseen tails stay Unread while scrolled"
        );
        anchors.push(
            (0..60)
                .find(|n| screen.contains(&format!("CHAT_{}_ROW_{n:02}", row - 1)))
                .unwrap(),
        );
    }
    d.install_focus(SessionId(3));
    d.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    d.key(KeyCode::Enter);
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    d.draw(&mut terminal).unwrap();
    let visible: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        visible.contains("draft A") && !visible.contains("draft B"),
        "{visible}"
    );
    assert!(
        visible.contains(&format!("CHAT_1_ROW_{:02}", anchors[0])),
        "first chat anchor: {visible}"
    );
    assert_eq!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.symbol() == "▌")
            .count(),
        1,
        "only the conversation has a selected row"
    );
    d.ctrl('l');
    d.key(KeyCode::Down);
    d.key(KeyCode::Enter);
    d.draw(&mut terminal).unwrap();
    let second: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(
        second.contains("draft B") && second.contains(&format!("CHAT_2_ROW_{:02}", anchors[1])),
        "independent second chat anchor: {second}"
    );
    // Resize and use the rendered receipt's actual hit row to collapse B independently.
    let narrow = Rect::new(0, 0, 80, 24);
    d.install_area(narrow);
    d.key(KeyCode::Tab);
    d.key(KeyCode::Tab);
    d.key(KeyCode::Home);
    let mut resized = Terminal::new(TestBackend::new(80, 24)).unwrap();
    d.draw(&mut resized).unwrap();
    let hit = (1..22)
        .find(|y| {
            (0..80)
                .map(|x| resized.backend().buffer()[(x, *y)].symbol())
                .collect::<String>()
                .contains("▾ Bash")
        })
        .unwrap();
    d.mouse_action(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 3,
            row: hit,
            modifiers: KeyModifiers::NONE,
        },
        narrow,
    );
    d.draw(&mut resized).unwrap();
    let collapsed = resized
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(
        !collapsed.contains("INTEGRATION_ARGUMENTS") && !collapsed.contains("INTEGRATION_CALL_ID")
    );
    assert!(
        collapsed.contains("│"),
        "Working journal guide: {collapsed}"
    );
    d.key(KeyCode::Tab);
    d.install_area(area);
    d.ctrl('l');
    d.key(KeyCode::Up);
    d.key(KeyCode::Enter);
    d.key(KeyCode::Enter);
    let requests = d.drain_outbox();
    assert!(!requests.iter().any(|m| matches!(
        m.request,
        Request::Input { .. } | Request::AssignedHumanInput { .. }
    )));
    let submit = requests
        .into_iter()
        .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Submit { .. })))
        .unwrap();
    let Request::Operator(OperatorRequest::Submit {
        conversation,
        context,
        text,
        ..
    }) = submit.request
    else {
        unreachable!()
    };
    assert_eq!(conversation, OperatorConversationId(1));
    assert_eq!(text, "draft A\n界é🙂");
    assert_eq!(
        context,
        ovrcr_protocol::OperatorContext::default(),
        "selecting saved history cannot recapture the terminal context"
    );
    assert!(!d.drain_outbox().iter().any(|m| matches!(
        m.request,
        Request::Input { .. } | Request::AssignedHumanInput { .. }
    )));
    d.install_focus(SessionId(3));
    assert_eq!(d.focused_session(), Some(SessionId(3)));
}

// Keep the last session at the bottom of a full project viewport while the
// independently loaded Operator section changes the space above it.
fn operator_project_viewport_fixture() -> (Dashboard, Vec<OperatorConversationSummary>) {
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
    dashboard.key(KeyCode::Char('o'));
    let list = dashboard
        .drain_outbox()
        .into_iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::List { .. })
            )
        })
        .unwrap();
    dashboard.install_focus(SessionId(30));
    let chats = (1..=5)
        .map(|id| OperatorConversationSummary {
            id: OperatorConversationId(id),
            session: SessionId(90 + id),
            run: SessionRunId(0),
            archived: false,
            worker_stopped: true,
            provider: OperatorProvider::ChatGpt,
            model: "fixture".into(),
            title: format!("viewport-chat-{id}"),
            current_turn: None,
        })
        .collect::<Vec<_>>();
    assert_operator_project_selection_at_bottom(&mut dashboard);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: list.request_id,
        response: Response::Operator(OperatorResponse::Conversations(chats.clone())),
    });
    (dashboard, chats)
}
fn assert_operator_project_selection_at_bottom(dashboard: &mut Dashboard) {
    let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
    dashboard.draw(&mut terminal).unwrap();
    let selected: Vec<_> = (0..24)
        .filter(|y| terminal.backend().buffer()[(0, *y)].symbol() == "▌")
        .collect();
    assert_eq!(
        selected,
        vec![16],
        "the selected terminal must remain at the filled tree viewport's bottom"
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(30)));
}
#[test]
fn operator_project_viewport_list_growth_keeps_bottom_selection_visible() {
    let (mut dashboard, _) = operator_project_viewport_fixture();
    assert_operator_project_selection_at_bottom(&mut dashboard);
}
#[test]
fn operator_project_viewport_collapse_and_expand_reconcile_selection_and_fill() {
    let (mut dashboard, _) = operator_project_viewport_fixture();
    // Re-select with the already-loaded section so this case isolates collapse/expand.
    dashboard.install_focus(SessionId(30));
    for _ in 0..2 {
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 1,
                row: 1,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 24),
        );
        assert_operator_project_selection_at_bottom(&mut dashboard);
    }
}
#[test]
fn operator_project_viewport_archive_reclaims_rows_and_clamps_offset() {
    let (mut dashboard, chats) = operator_project_viewport_fixture();
    dashboard.install_focus(SessionId(30));
    for kind in [
        MouseEventKind::Moved,
        MouseEventKind::Down(MouseButton::Left),
    ] {
        dashboard.mouse_action(
            MouseEvent {
                kind,
                column: 38,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 24),
        );
    }
    dashboard.key(KeyCode::Enter);
    let close = dashboard
        .drain_outbox()
        .into_iter()
        .find(|message| {
            matches!(
                message.request,
                Request::Operator(OperatorRequest::Close { .. })
            )
        })
        .unwrap();
    let mut closed = chats[0].clone();
    closed.archived = true;
    closed.worker_stopped = true;
    closed.run = SessionRunId(1);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: close.request_id,
        response: Response::Operator(OperatorResponse::Closed(closed)),
    });
    assert_operator_project_selection_at_bottom(&mut dashboard);
}

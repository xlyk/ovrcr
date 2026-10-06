//! Public Dashboard dispatch for Server-confirmed Bridge navigation. Real callback
//! digest admission/socket/PTY coverage belongs to the desktop fixture in server_lifecycle.

use super::*;
use ovrcr::protocol::{
    BRIDGE_SCHEMA_VERSION, BridgeContext, BridgeNavigationOffer, BridgeNavigationTicket,
    PROTOCOL_VERSION, SessionRunId,
};

const LIFETIME: &str = "10000000-0000-4000-8000-000000000001";
const NAVIGATION: &str = "20000000-0000-4000-8000-000000000002";

fn context() -> BridgeContext {
    static CONTEXT: std::sync::OnceLock<BridgeContext> = std::sync::OnceLock::new();
    CONTEXT
        .get_or_init(|| {
            let root = tempfile::tempdir().expect("private Dashboard context fixture");
            // Dashboard dispatch compares serialized context identity, not file bytes.
            // These private paths have no executable/socket; the scratch root is removed.
            let context = BridgeContext {
                server_socket: root.path().join("server.sock").to_str().unwrap().into(),
                callback_executable: root.path().join("ovrcr").to_str().unwrap().into(),
                callback_executable_sha256: "0".repeat(64),
                server_lifetime: LIFETIME.into(),
            };
            assert!(
                context.validate(),
                "pure context must remain structurally valid"
            );
            context
        })
        .clone()
}

fn ready_dashboard() -> Dashboard {
    let mut dashboard = dashboard_fixture();
    dashboard.install_area(Rect::new(0, 0, 120, 40));
    if let Some(view) = dashboard.request_view_at(Rect::new(0, 0, 120, 40)) {
        dashboard.drain_outbox();
        acknowledge_all_view_targets(&mut dashboard, view);
    }
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::BridgeContext(context())));
    dashboard
}

fn offer(session: SessionId, run: SessionRunId) -> BridgeNavigationOffer {
    let context = context();
    BridgeNavigationOffer {
        navigation: NAVIGATION.into(),
        ticket: BridgeNavigationTicket {
            schema: BRIDGE_SCHEMA_VERSION,
            server_wire: PROTOCOL_VERSION,
            server_socket: context.server_socket,
            callback_executable: context.callback_executable,
            callback_executable_sha256: context.callback_executable_sha256,
            server_lifetime: context.server_lifetime,
            session,
            run,
        },
    }
}

fn request_confirmation(dashboard: &mut Dashboard, offer: &BridgeNavigationOffer) -> ClientMessage {
    let outgoing = dashboard.handle_server_message(ServerMessage::Event(
        ServerEvent::NotificationNavigation(offer.clone()),
    ));
    let confirmations: Vec<_> = outgoing
        .into_iter()
        .filter(|request| {
            matches!(&request.request, Request::ConfirmNotificationNavigation { navigation }
            if navigation == &offer.navigation)
        })
        .collect();
    assert_eq!(
        confirmations.len(),
        1,
        "each offer must cross the Server application fence"
    );
    confirmations.into_iter().next().unwrap()
}

fn confirmed_click(dashboard: &mut Dashboard, offer: BridgeNavigationOffer) -> Vec<ClientMessage> {
    let confirmation = request_confirmation(dashboard, &offer);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: confirmation.request_id,
        response: Response::NotificationNavigationConfirmed(Some(Box::new(offer))),
    })
}

fn assert_navigation_only(requests: &[ClientMessage]) {
    for request in requests {
        assert!(
            !matches!(
                request.request,
                Request::Input { .. }
                    | Request::SendTerminal { .. }
                    | Request::Keystroke { .. }
                    | Request::MarkReviewed { .. }
                    | Request::CreateSession(_)
                    | Request::CreateWorkspaceWithLaunch { .. }
                    | Request::ReopenSession { .. }
                    | Request::RecoverSession { .. }
                    | Request::AcknowledgeSessionStopped { .. }
            ),
            "navigation emitted an input, review or process operation: {:?}",
            request.request
        );
    }
}

fn clicked_view(dashboard: &mut Dashboard, requests: &[ClientMessage]) -> ClientMessage {
    if let Some(request) = requests
        .iter()
        .find(|request| matches!(request.request, Request::SetView { .. }))
    {
        return request.clone();
    }
    let request = dashboard
        .request_view_at(Rect::new(0, 0, 120, 40))
        .expect("changed navigation must request the ordinary acknowledged view");
    dashboard.drain_outbox();
    request
}

#[test]
fn bridge_navigation_focuses_existing_pane_and_retargets_only_the_focused_pane() {
    let mut dashboard = ready_dashboard();
    let split = dashboard.key(KeyCode::Char('v'));
    pump_view(&mut dashboard, split);
    assert_eq!(dashboard.pane_rects(Rect::new(0, 0, 120, 40)).len(), 2);
    let other = dashboard.focused_session().unwrap();
    assert_ne!(other, SessionId(1));

    let outgoing = confirmed_click(&mut dashboard, offer(SessionId(1), SessionRunId(1)));
    assert_navigation_only(&outgoing);
    assert_eq!(dashboard.focused_session(), Some(SessionId(1)));
    assert!(rendered_footer(&dashboard, 120).contains("BROWSE"));
    let view = clicked_view(&mut dashboard, &outgoing);
    let Request::SetView { view: selected } = &view.request else {
        unreachable!()
    };
    assert_eq!(selected.panes.len(), 2);
    assert!(selected.panes.iter().any(|pane| pane.session == other));
    acknowledge_all_view_targets(&mut dashboard, view);

    // Session 3 was not assigned. It replaces the focused pane; the peer
    // assignment remains intact, and no duplicate pane owner is created.
    let outgoing = confirmed_click(&mut dashboard, offer(SessionId(3), SessionRunId(1)));
    assert_navigation_only(&outgoing);
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
    let view = clicked_view(&mut dashboard, &outgoing);
    let Request::SetView { view: selected } = &view.request else {
        unreachable!()
    };
    assert_eq!(selected.panes.len(), 2);
    assert!(selected.panes.iter().any(|pane| pane.session == other));
    assert!(
        !selected
            .panes
            .iter()
            .any(|pane| pane.session == SessionId(1))
    );
}

#[test]
fn bridge_navigation_keeps_old_input_bound_and_requires_complete_current_view_ack() {
    let mut dashboard = ready_dashboard();
    dashboard.key(KeyCode::Enter);
    let queued = dashboard
        .input_request(b"OLD_QUEUED_INPUT".to_vec(), 9001)
        .unwrap();
    assert!(
        matches!(&queued.request, Request::Input { session, run, bytes }
        if *session == SessionId(1) && *run == SessionRunId(1) && bytes.as_slice() == b"OLD_QUEUED_INPUT")
    );
    let outgoing = confirmed_click(&mut dashboard, offer(SessionId(3), SessionRunId(1)));
    assert_navigation_only(&outgoing);
    assert!(rendered_footer(&dashboard, 120).contains("BROWSE"));
    assert!(
        dashboard
            .input_request(b"OLD_QUEUED_INPUT".to_vec(), 9002)
            .is_none()
    );
    let current = clicked_view(&mut dashboard, &outgoing);
    let Request::SetView { view } = &current.request else {
        unreachable!()
    };
    assert_eq!(view.focused, Some(SessionId(3)));

    // A receipt for another request/run cannot make the new target writable.
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: current.request_id.saturating_add(100),
        response: Response::Screen {
            session: SessionId(3),
            run: SessionRunId(1),
            revision: view.revision,
            size: view.panes[0].size,
            bytes: b"STALE_SCREEN_MUST_NOT_APPEAR".to_vec(),
        },
    });
    dashboard.install_terminal_input();
    assert!(dashboard.input_request(b"blocked".to_vec(), 9003).is_none());
    deliver_all_view_screens(&mut dashboard, current.request_id, view);
    assert!(
        dashboard
            .input_request(b"still blocked".to_vec(), 9004)
            .is_none(),
        "snapshots without final acknowledgement grant no input"
    );
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: current.request_id,
        response: Response::Ok,
    });
    assert_view_input_allowed(&mut dashboard);
    dashboard.install_terminal_input();
    let input = dashboard
        .input_request(b"EXPLICIT_NEW_INPUT".to_vec(), 9005)
        .unwrap();
    assert!(matches!(
        input.request,
        Request::Input {
            session: SessionId(3),
            run: SessionRunId(1),
            ..
        }
    ));
    assert!(
        matches!(
            queued.request,
            Request::Input {
                session: SessionId(1),
                run: SessionRunId(1),
                ..
            }
        ),
        "already queued bytes must retain their original destination"
    );
    assert!(
        !rendered_rows(&dashboard, 120, 40)
            .join("\n")
            .contains("STALE_SCREEN_MUST_NOT_APPEAR")
    );
}

#[test]
fn bridge_navigation_revalidates_local_target_after_server_confirmation() {
    for change in ["run", "archive", "remove", "lifetime"] {
        let mut dashboard = ready_dashboard();
        let offer = offer(SessionId(3), SessionRunId(1));
        let confirmation = request_confirmation(&mut dashboard, &offer);
        if change == "lifetime" {
            let mut replacement = context();
            replacement.server_lifetime = "30000000-0000-4000-8000-000000000003".into();
            dashboard.handle_server_message(ServerMessage::Event(ServerEvent::BridgeContext(
                replacement,
            )));
        } else {
            let mut hierarchy = fixture_hierarchy();
            let sessions = &mut hierarchy.projects[0].workspaces[0].sessions;
            match change {
                "run" => sessions[0].run = SessionRunId(2),
                "archive" => sessions[0].archived = true,
                "remove" => sessions.clear(),
                _ => unreachable!(),
            }
            dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
                hierarchy,
            )));
        }
        let outgoing = dashboard.handle_server_message(ServerMessage::Response {
            request_id: confirmation.request_id,
            response: Response::NotificationNavigationConfirmed(Some(Box::new(offer))),
        });
        assert_eq!(
            dashboard.focused_session(),
            Some(SessionId(1)),
            "{change} crossed the application fence"
        );
        assert_navigation_only(&outgoing);
        assert!(
            !outgoing.iter().any(|request| matches!(
                request.request,
                Request::NotificationNavigationApplied { .. }
            )),
            "{change} claimed successful application"
        );
    }
}

#[test]
fn bridge_navigation_requires_matching_pending_confirmation_and_never_applies_a_denial() {
    for wrong in ["request", "navigation", "ticket", "denied"] {
        let mut dashboard = ready_dashboard();
        let original = offer(SessionId(3), SessionRunId(1));
        let request = request_confirmation(&mut dashboard, &original);
        assert_eq!(
            dashboard.focused_session(),
            Some(SessionId(1)),
            "an unconfirmed offer must not select"
        );
        let mut response = original;
        match wrong {
            "navigation" => response.navigation = "40000000-0000-4000-8000-000000000004".into(),
            "ticket" => response.ticket.session = SessionId(5),
            _ => {}
        }
        let outgoing = dashboard.handle_server_message(ServerMessage::Response {
            request_id: request.request_id + u64::from(wrong == "request"),
            response: Response::NotificationNavigationConfirmed(
                (wrong != "denied").then_some(Box::new(response)),
            ),
        });
        assert_eq!(
            dashboard.focused_session(),
            Some(SessionId(1)),
            "{wrong} confirmation selected a session"
        );
        assert_navigation_only(&outgoing);
        assert!(
            !outgoing.iter().any(|request| matches!(
                request.request,
                Request::NotificationNavigationApplied { .. }
            )),
            "{wrong} confirmation claimed application"
        );
    }
}

#[test]
fn bridge_navigation_late_a_b_a_confirmations_cannot_retarget_the_current_offer() {
    let mut dashboard = ready_dashboard();
    let first_a = offer(SessionId(3), SessionRunId(1));
    let old = request_confirmation(&mut dashboard, &first_a);
    let mut b = offer(SessionId(5), SessionRunId(1));
    b.navigation = "60000000-0000-4000-8000-000000000006".into();
    let current = request_confirmation(&mut dashboard, &b);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: old.request_id,
        response: Response::NotificationNavigationConfirmed(Some(Box::new(first_a.clone()))),
    });
    assert_eq!(
        dashboard.focused_session(),
        Some(SessionId(1)),
        "late A cannot replace the pending B confirmation"
    );
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: current.request_id,
        response: Response::NotificationNavigationConfirmed(Some(Box::new(b))),
    });
    assert_navigation_only(&outgoing);
    assert_eq!(dashboard.focused_session(), Some(SessionId(5)));
    let view = clicked_view(&mut dashboard, &outgoing);
    acknowledge_all_view_targets(&mut dashboard, view);

    let mut fresh_a = first_a.clone();
    fresh_a.navigation = "70000000-0000-4000-8000-000000000007".into();
    let current = request_confirmation(&mut dashboard, &fresh_a);
    dashboard.handle_server_message(ServerMessage::Response {
        request_id: old.request_id,
        response: Response::NotificationNavigationConfirmed(Some(Box::new(first_a))),
    });
    assert_eq!(
        dashboard.focused_session(),
        Some(SessionId(5)),
        "the old A token is still stale after A becomes the desired target again"
    );
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: current.request_id,
        response: Response::NotificationNavigationConfirmed(Some(Box::new(fresh_a))),
    });
    assert_navigation_only(&outgoing);
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
}

#[test]
fn bridge_navigation_discards_unsent_palette_draft_and_ignores_submitted_completion() {
    let mut unsent = ready_dashboard();
    palette_search(&mut unsent, "UNSENT_PRIVATE_DRAFT");
    assert!(palette_text(&unsent).contains("UNSENT_PRIVATE_DRAFT"));
    let outgoing = confirmed_click(&mut unsent, offer(SessionId(3), SessionRunId(1)));
    assert_navigation_only(&outgoing);
    assert_eq!(unsent.focused_session(), Some(SessionId(3)));
    assert!(!palette_text(&unsent).contains("UNSENT_PRIVATE_DRAFT"));
    palette_search(&mut unsent, "");
    assert!(!palette_text(&unsent).contains("UNSENT_PRIVATE_DRAFT"));

    let mut submitted = ready_dashboard();
    submitted.key(KeyCode::Char('n'));
    let mut request = None;
    for _ in 0..6 {
        if let DashboardAction::Request(message) = submitted.key(KeyCode::Enter) {
            request = Some(message);
            break;
        }
    }
    let request = request.expect("terminal creation form must submit");
    let Request::CreateSession(original) = &request.request else {
        panic!("expected session creation")
    };
    assert_eq!(
        (&*original.project, &*original.workspace),
        ("consigint", "auth")
    );
    let outgoing = confirmed_click(&mut submitted, offer(SessionId(3), SessionRunId(1)));
    assert_navigation_only(&outgoing);
    let mut created = session_summary(
        99,
        "consigint",
        "auth",
        "already-submitted",
        "zsh",
        Some(999),
        0,
    );
    created.run = SessionRunId(1);
    let late = submitted.handle_server_message(ServerMessage::Response {
        request_id: request.request_id,
        response: Response::CreatedSession(Box::new(created)),
    });
    assert_eq!(
        submitted.focused_session(),
        Some(SessionId(3)),
        "late launch result must not retarget the clicked pane"
    );
    assert_navigation_only(&late);
    assert!(!palette_text(&submitted).contains("Create terminal"));
    assert_eq!(
        (&*original.project, &*original.workspace),
        ("consigint", "auth"),
        "navigation must not rewrite the submitted target"
    );
}

#[test]
fn bridge_navigation_cancels_pending_history_and_releases_late_original_capture() {
    let mut dashboard = ready_dashboard();
    let DashboardAction::Request(begin) = dashboard.key(KeyCode::PageUp) else {
        panic!("real History entry must request a snapshot");
    };
    assert_eq!(
        begin.request,
        Request::HistoryBegin {
            session: SessionId(1)
        }
    );
    let outgoing = confirmed_click(&mut dashboard, offer(SessionId(3), SessionRunId(1)));
    assert_navigation_only(&outgoing);
    let late = dashboard.handle_server_message(ServerMessage::Response {
        request_id: begin.request_id,
        response: Response::HistoryOpened(HistoryOpened {
            session: SessionId(1),
            snapshot: HistorySnapshotId(81),
            revision: 1,
            size: TerminalSize { rows: 10, cols: 20 },
            history_rows: 10,
            total_rows: 20,
        }),
    });
    assert!(
        late.iter().any(|request| matches!(
            request.request,
            Request::HistoryEnd {
                session: SessionId(1),
                snapshot: HistorySnapshotId(81)
            }
        )),
        "late capture must release its original Server snapshot"
    );
    assert_eq!(dashboard.focused_session(), Some(SessionId(3)));
    assert!(
        !rendered_rows(&dashboard, 180, 40)
            .join("\n")
            .contains("HISTORY")
    );
    assert!(rendered_footer(&dashboard, 120).contains("BROWSE"));
    assert!(!matches!(
        dashboard.key(KeyCode::Char('y')),
        DashboardAction::CopyText(_)
    ));
}

#[test]
fn bridge_navigation_releases_active_history_and_rejects_late_original_rows() {
    for target in [SessionId(1), SessionId(3)] {
        let mut dashboard = ready_dashboard();
        let DashboardAction::Request(begin) = dashboard.key(KeyCode::PageUp) else {
            panic!("History entry must use the production snapshot request");
        };
        let pages = dashboard.handle_server_message(ServerMessage::Response {
            request_id: begin.request_id,
            response: Response::HistoryOpened(HistoryOpened {
                session: SessionId(1),
                snapshot: HistorySnapshotId(82),
                revision: 1,
                size: TerminalSize { rows: 10, cols: 20 },
                history_rows: 10,
                total_rows: 20,
            }),
        });
        let page = pages
            .into_iter()
            .find(|request| matches!(request.request, Request::HistoryPage { .. }))
            .expect("opened History must fetch its real rows");
        assert!(
            rendered_rows(&dashboard, 180, 40)
                .join("\n")
                .contains("HISTORY")
        );
        let outgoing = confirmed_click(&mut dashboard, offer(target, SessionRunId(1)));
        assert!(
            outgoing.iter().any(|request| matches!(
                request.request,
                Request::HistoryEnd {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(82)
                }
            )),
            "clicking {target:?} must release the original capture, even when its session stays focused"
        );
        let Request::HistoryPage {
            session,
            snapshot,
            start_row,
            rows,
            start_col,
            cols,
        } = page.request
        else {
            unreachable!();
        };
        let cells: Vec<_> = (0..cols)
            .map(|index| HistoryCell {
                text: "OLD_CAPTURE_MUST_NOT_APPEAR"
                    .chars()
                    .nth(usize::from(index))
                    .unwrap_or(' ')
                    .to_string(),
                width: 1,
                fg: HistoryColor::Default,
                bg: HistoryColor::Default,
                attributes: 0,
            })
            .collect();
        let late = dashboard.handle_server_message(ServerMessage::Response {
            request_id: page.request_id,
            response: Response::HistoryRows(HistoryRows {
                session,
                snapshot,
                start_row,
                start_col,
                rows: (0..rows)
                    .map(|_| HistoryRow {
                        width: start_col.saturating_add(cols),
                        cells: cells.clone(),
                        wrapped: false,
                    })
                    .collect(),
            }),
        });
        assert_navigation_only(&late);
        assert_eq!(dashboard.focused_session(), Some(target));
        let rendered = rendered_rows(&dashboard, 180, 40).join("\n");
        assert!(!rendered.contains("HISTORY"));
        assert!(!rendered.contains("OLD_CAPTURE_MUST_NOT_APPEAR"));
        assert!(!matches!(
            dashboard.key(KeyCode::Char('y')),
            DashboardAction::CopyText(_)
        ));
    }
}

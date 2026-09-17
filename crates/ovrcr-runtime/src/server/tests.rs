use super::connections::{DashboardOwnership, lifecycle_response_with_partial_hierarchy};
use super::dispatch::resize_view_targets;
use super::startup::resolve_bound_socket;
use super::*;
use ovrcr_protocol::AgentReport;
use ovrcr_protocol::exchange_preamble;
use std::time::Instant;

use crate::session::{AgentActivity, NO_REGISTER};
use ovrcr_protocol::{
    DashboardView, HISTORY_ROWS, HistorySnapshotId, PAGE_COLS, PAGE_ROWS, PaneTarget, Response,
    ServerEvent, ServerMessage,
};
use ovrcr_terminal::history::FrozenHistory;
use std::io::Read;
use std::net::Shutdown;

#[test]
fn raw_event_and_dispatch_queues_reject_the_65th_item() {
    let (event_sender, event_receiver) = event_channel(Some(&ReportingQueueMonitor::default()));
    for _ in 0..RAW_EVENT_QUEUE_CAPACITY {
        event_sender
            .try_send(SessionEvent::Output {
                run: ovrcr_protocol::SessionRunId(1),
                id: SessionId(1),
                bytes: vec![0; 8192],
            })
            .unwrap();
    }
    assert!(matches!(
        event_sender.try_send(SessionEvent::Output {
            run: ovrcr_protocol::SessionRunId(1),
            id: SessionId(1),
            bytes: vec![0; 8192],
        }),
        Err(mpsc::TrySendError::Full(_))
    ));
    assert_eq!(
        event_sender.snapshot().pending_items,
        RAW_EVENT_QUEUE_CAPACITY
    );
    assert_eq!(
        event_sender.snapshot().pending_bytes,
        RAW_EVENT_QUEUE_CAPACITY * 8192
    );
    assert_eq!(event_sender.snapshot().rejected, 1);
    drop(event_receiver.recv().unwrap());
    assert_eq!(
        event_sender.snapshot().pending_items,
        RAW_EVENT_QUEUE_CAPACITY - 1
    );

    let (dispatch_sender, dispatch_receiver) =
        dispatch_channel(Some(&ReportingQueueMonitor::default()));
    for _ in 0..RAW_DISPATCH_QUEUE_CAPACITY {
        dispatch_sender.try_send(DispatchMessage::Stop).unwrap();
    }
    assert!(matches!(
        dispatch_sender.try_send(DispatchMessage::Stop),
        Err(mpsc::TrySendError::Full(_))
    ));
    assert_eq!(
        dispatch_sender.snapshot().pending_items,
        RAW_DISPATCH_QUEUE_CAPACITY
    );
    assert_eq!(dispatch_sender.snapshot().rejected, 1);
    drop(dispatch_receiver.recv().unwrap());
    assert_eq!(
        dispatch_sender.snapshot().pending_items,
        RAW_DISPATCH_QUEUE_CAPACITY - 1
    );
    drop(dispatch_receiver);
    assert_eq!(dispatch_sender.snapshot().pending_items, 0);
    assert!(matches!(
        dispatch_sender.try_send(DispatchMessage::Stop),
        Err(mpsc::TrySendError::Disconnected(_))
    ));
    assert_eq!(dispatch_sender.snapshot().rejected, 2);
}

#[test]
fn split_delivery_snapshots_precede_increments() {
    let sink = DashboardSink::new();
    let left = SessionId(1);
    let right = SessionId(2);
    for session in [left, right] {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::Output {
                    run: ovrcr_protocol::SessionRunId(1),
                    session,
                    revision: 1,
                    bytes: b"old".to_vec(),
                }),
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    let view = DashboardView {
        revision: 2,
        panes: vec![
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: left,
                size: TerminalSize { rows: 36, cols: 39 },
            },
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: right,
                size: TerminalSize { rows: 36, cols: 40 },
            },
        ],
        focused: Some(right),
    };
    assert!(sink.replace_view(&view, 10, vec![b"LEFT".to_vec(), b"RIGHT".to_vec()]));
    assert_eq!(
        sink.enqueue(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                run: ovrcr_protocol::SessionRunId(1),
                session: right,
                revision: 2,
                bytes: b"new".to_vec(),
            }),
            completion: None,
        }),
        Enqueue::Queued
    );
    for (session, bytes) in [(left, b"LEFT".as_slice()), (right, b"RIGHT".as_slice())] {
        assert!(matches!(
            sink.next(),
            Some(DashboardDelivery::Message(DashboardOutbound {
                message: ServerMessage::Response {
                    response: Response::Screen { revision: 2, session: actual, bytes: actual_bytes, .. },
                    ..
                },
                ..
            })) if actual == session && actual_bytes == bytes
        ));
    }
    assert!(matches!(
        sink.next(),
        Some(DashboardDelivery::Message(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 10,
                response: Response::Ok,
            },
            ..
        }))
    ));
    assert!(matches!(
        sink.next(),
        Some(DashboardDelivery::Message(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                run: ovrcr_protocol::SessionRunId(1),
                session,
                revision: 2,
                bytes,
            }),
            ..
        })) if session == right && bytes == b"new"
    ));

    let sink = DashboardSink::new();
    for _ in 0..62 {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Response {
                    request_id: 1,
                    response: Response::Ok,
                },
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    assert!(!sink.replace_view(&view, 11, vec![b"LEFT".to_vec(), b"RIGHT".to_vec()]));
    assert!(sink.next().is_none());
}

#[test]
fn dashboard_snapshot_counts_messages_terminal_and_dirty_separately() {
    let sink = DashboardSink::new();
    for _ in 0..DASHBOARD_QUEUE {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::Output {
                    run: ovrcr_protocol::SessionRunId(1),
                    session: SessionId(1),
                    revision: 1,
                    bytes: vec![b'x'; 32],
                }),
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    assert_eq!(
        sink.enqueue(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                run: ovrcr_protocol::SessionRunId(1),
                session: SessionId(1),
                revision: 1,
                bytes: vec![b'y'; 32],
            }),
            completion: None,
        }),
        Enqueue::Queued
    );
    let snapshot = sink.reporting_snapshot();
    assert_eq!(snapshot.message_items + snapshot.dirty_items, 1);
    assert_eq!(snapshot.dirty_items, 1);
    assert_eq!(snapshot.terminal_items, 0);
    assert!(snapshot.pending_bytes <= ovrcr_protocol::MAX_FRAME_BYTES);
    assert_eq!(snapshot.rejected, 0);
    let DashboardDelivery::Dirty {
        run: ovrcr_protocol::SessionRunId(1),
        revision,
        session,
    } = sink.next().unwrap()
    else {
        panic!("expected dirty delivery");
    };
    sink.dirty_sent(revision, session, ovrcr_protocol::SessionRunId(1));
    assert!(sink.replace_view(
        &DashboardView {
            revision: 2,
            panes: Vec::new(),
            focused: None
        },
        7,
        Vec::new(),
    ));
    drop(sink.next().unwrap());
    assert_eq!(sink.reporting_snapshot().pending_items, 0);
}

#[test]
fn split_delivery_dirty_revisions_are_isolated() {
    let sink = DashboardSink::new();
    let first = SessionId(1);
    let second = SessionId(2);
    for _ in 0..(DASHBOARD_QUEUE - 3) {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Response {
                    request_id: 1,
                    response: Response::Ok,
                },
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    for (session, bytes) in [
        (first, b"first".as_slice()),
        (second, b"second".as_slice()),
        (SessionId(3), b"interleaved".as_slice()),
    ] {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::Output {
                    run: ovrcr_protocol::SessionRunId(1),
                    session,
                    revision: 1,
                    bytes: bytes.to_vec(),
                }),
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    assert_eq!(
        sink.enqueue(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                run: ovrcr_protocol::SessionRunId(1),
                session: first,
                revision: 1,
                bytes: b"first-replaced".to_vec(),
            }),
            completion: None,
        }),
        Enqueue::Queued
    );
    assert_eq!(
        sink.enqueue(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                run: ovrcr_protocol::SessionRunId(1),
                session: second,
                revision: 1,
                bytes: b"second-replaced".to_vec(),
            }),
            completion: None,
        }),
        Enqueue::Queued
    );
    assert_eq!(
        sink.enqueue(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                run: ovrcr_protocol::SessionRunId(1),
                session: second,
                revision: 1,
                bytes: b"second-dirty".to_vec(),
            }),
            completion: None,
        }),
        Enqueue::Queued
    );
    for _ in 0..2 {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Response {
                    request_id: 3,
                    response: Response::Ok,
                },
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    let view = DashboardView {
        revision: 2,
        panes: vec![
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: first,
                size: TerminalSize { rows: 24, cols: 80 },
            },
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: second,
                size: TerminalSize { rows: 24, cols: 80 },
            },
        ],
        focused: Some(first),
    };
    let mut initial_messages = Vec::new();
    for _ in 0..DASHBOARD_QUEUE {
        let Some(DashboardDelivery::Message(outbound)) = sink.next() else {
            panic!("expected protected and interleaved messages");
        };
        initial_messages.push(outbound.message);
    }
    assert!(
        initial_messages[..DASHBOARD_QUEUE - 3]
            .iter()
            .all(|message| {
                matches!(
                    message,
                    ServerMessage::Response {
                        request_id: 1,
                        response: Response::Ok,
                    }
                )
            })
    );
    assert!(matches!(
        initial_messages[DASHBOARD_QUEUE - 3],
        ServerMessage::Event(ServerEvent::Output {
            run: ovrcr_protocol::SessionRunId(1),
            session: SessionId(3),
            revision: 1,
            ref bytes,
        }) if bytes == b"interleaved"
    ));
    assert!(
        initial_messages[DASHBOARD_QUEUE - 2..]
            .iter()
            .all(|message| {
                matches!(
                    message,
                    ServerMessage::Response {
                        request_id: 3,
                        response: Response::Ok,
                    }
                )
            })
    );
    let mut old_dirty = Vec::new();
    for _ in 0..2 {
        let Some(DashboardDelivery::Dirty {
            run: ovrcr_protocol::SessionRunId(1),
            revision,
            session,
        }) = sink.next()
        else {
            panic!("expected old dirty delivery");
        };
        old_dirty.push((revision, session));
    }
    old_dirty.sort_unstable_by_key(|(_, session)| session.0);
    assert_eq!(old_dirty, vec![(1, first), (1, second)]);
    assert!(sink.replace_view(
        &view,
        12,
        vec![b"first-screen".to_vec(), b"second-screen".to_vec()],
    ));
    for (revision, session) in old_dirty {
        sink.dirty_sent(revision, session, ovrcr_protocol::SessionRunId(1));
    }
    for _ in 0..(DASHBOARD_QUEUE - 3) {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Response {
                    request_id: 2,
                    response: Response::Ok,
                },
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    for session in [first, second] {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::Output {
                    run: ovrcr_protocol::SessionRunId(1),
                    session,
                    revision: 2,
                    bytes: b"new".to_vec(),
                }),
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    assert!(
        sink.dirty_keys()
            .into_iter()
            .all(|(revision, session, run)| revision == 2
                && run == ovrcr_protocol::SessionRunId(1)
                && [first, second].contains(&session))
    );
    assert_eq!(sink.dirty_keys().len(), 2);
    let mut refreshed_messages = Vec::new();
    for _ in 0..DASHBOARD_QUEUE {
        let Some(DashboardDelivery::Message(outbound)) = sink.next() else {
            panic!("expected refreshed snapshot and protected messages");
        };
        refreshed_messages.push(outbound.message);
    }
    assert!(matches!(
        refreshed_messages[0],
        ServerMessage::Response {
            request_id: 12,
            response: Response::Screen {
                session,
                revision: 2,
                ref bytes,
                ..
            },
        } if session == first && bytes == b"first-screen"
    ));
    assert!(matches!(
        refreshed_messages[1],
        ServerMessage::Response {
            request_id: 12,
            response: Response::Screen {
                session,
                revision: 2,
                ref bytes,
                ..
            },
        } if session == second && bytes == b"second-screen"
    ));
    assert!(matches!(
        refreshed_messages[2],
        ServerMessage::Response {
            request_id: 12,
            response: Response::Ok,
        }
    ));
    assert!(refreshed_messages[3..].iter().all(|message| {
        matches!(
            message,
            ServerMessage::Response {
                request_id: 2,
                response: Response::Ok,
            }
        )
    }));
    let mut refreshed_dirty = Vec::new();
    for _ in 0..2 {
        let Some(DashboardDelivery::Dirty {
            run: ovrcr_protocol::SessionRunId(1),
            revision,
            session,
        }) = sink.next()
        else {
            panic!("expected refreshed dirty delivery");
        };
        refreshed_dirty.push((revision, session));
    }
    refreshed_dirty.sort_unstable_by_key(|(_, session)| session.0);
    assert_eq!(refreshed_dirty, vec![(2, first), (2, second)]);
}

#[test]
fn split_delivery_rejects_stale_owner_and_input() {
    let id = SessionId(21);
    let (_cwd, session, events_receiver) = spawn_live_test_session(id);
    let events = apply_test_session_events(Arc::clone(&session), events_receiver);
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let old_sink = DashboardSink::new();
    let old_owner = Arc::new(());
    let (state, dispatch_receiver) = test_state_with_dispatch(
        Some(old_sink.clone()),
        Some((old_owner.clone(), server_stream)),
    );
    register_test_session(&state, id, session.clone());
    let (new_server, _new_client) = UnixStream::pair().unwrap();
    let new_sink = DashboardSink::new();
    let new_owner = Arc::new(());
    state
        .dashboard
        .claim_with_identity(new_sink, new_owner, new_server);
    let view = DashboardView {
        revision: 1,
        panes: vec![PaneTarget {
            run: ovrcr_protocol::SessionRunId(1),
            session: id,
            size: TerminalSize { rows: 24, cols: 80 },
        }],
        focused: Some(id),
    };
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: old_owner.clone(),
            request_id: 20,
            view,
            completion,
        })
        .unwrap();
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    result.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(state.dashboard.view().is_none());
    assert!(old_sink.queue.lock().unwrap().messages.is_empty());
    let mut role = ClientRole::Dashboard;
    assert!(matches!(
        handle_request_with_id(
            &state,
            &mut role,
            Request::Input {
                run: ovrcr_protocol::SessionRunId(1),
                session: id,
                bytes: b"stale".to_vec(),
            },
            21,
            Some(&old_owner),
        ),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    let (_second_cwd, second_session, second_events_receiver) =
        spawn_live_test_session(SessionId(22));
    let second_events =
        apply_test_session_events(Arc::clone(&second_session), second_events_receiver);
    let mut resize_calls = 0;
    let resize_error = resize_view_targets(
        &[
            (Arc::clone(&session), TerminalSize { rows: 24, cols: 80 }),
            (
                Arc::clone(&second_session),
                TerminalSize { rows: 24, cols: 80 },
            ),
        ],
        |_, _| {
            resize_calls += 1;
            if resize_calls == 2 {
                Err(anyhow::anyhow!("second resize failed"))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(resize_calls, 2);
    assert_eq!(resize_error.to_string(), "second resize failed");
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&second_session, second_events).unwrap();
    cleanup_test_session(&session, events).unwrap();
}

#[test]
fn current_owner_rejects_hidden_input_and_unknown_view_session() {
    let focused_id = SessionId(23);
    let hidden_id = SessionId(24);
    let (_focused_cwd, focused, focused_events) = spawn_live_test_session(focused_id);
    let focused_events = apply_test_session_events(Arc::clone(&focused), focused_events);
    let (_hidden_cwd, hidden, hidden_events) = spawn_live_test_session(hidden_id);
    let hidden_events = apply_test_session_events(Arc::clone(&hidden), hidden_events);
    let owner = Arc::new(());
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    extend_test_sessions(
        &state,
        [(focused_id, focused.clone()), (hidden_id, hidden.clone())],
    );
    state.dashboard.install_view_for_test(Some(DashboardView {
        revision: 7,
        panes: vec![
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: focused_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: hidden_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
        ],
        focused: Some(focused_id),
    }));
    let mut role = ClientRole::Dashboard;
    assert!(matches!(
        handle_request_with_id(
            &state,
            &mut role,
            Request::Input {
                run: ovrcr_protocol::SessionRunId(1),
                session: hidden_id,
                bytes: b"hidden".to_vec(),
            },
            70,
            Some(&owner),
        ),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(matches!(
        handle_request_with_id(
            &state,
            &mut role,
            Request::Input {
                run: ovrcr_protocol::SessionRunId(1),
                session: SessionId(999),
                bytes: b"unknown".to_vec(),
            },
            71,
            Some(&owner),
        ),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    let mut control_role = ClientRole::Control;
    assert!(matches!(
        handle_request_with_id(
            &state,
            &mut control_role,
            Request::Input {
                run: ovrcr_protocol::SessionRunId(1),
                session: focused_id,
                bytes: b"control".to_vec(),
            },
            72,
            None,
        ),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: owner.clone(),
            request_id: 73,
            view: DashboardView {
                revision: 8,
                panes: vec![PaneTarget {
                    run: ovrcr_protocol::SessionRunId(1),
                    session: SessionId(999),
                    size: TerminalSize { rows: 24, cols: 80 },
                }],
                focused: Some(SessionId(999)),
            },
            completion,
        })
        .unwrap();
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    assert!(matches!(
        sink.next(),
        Some(DashboardDelivery::Message(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 73,
                response: Response::Error {
                    code: ErrorCode::NotFound,
                    ..
                },
            },
            ..
        }))
    ));
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&hidden, hidden_events).unwrap();
    cleanup_test_session(&focused, focused_events).unwrap();
}

#[test]
fn oversized_set_view_returns_invalid_request_without_resizing() {
    let id = SessionId(28);
    let (_cwd, session, events_receiver) = spawn_live_test_session(id);
    let events = apply_test_session_events(Arc::clone(&session), events_receiver);
    let owner = Arc::new(());
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    register_test_session(&state, id, session.clone());
    let initial_pty_size = session.master_size().unwrap();
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner,
            request_id: 83,
            view: DashboardView {
                revision: 1,
                panes: vec![PaneTarget {
                    run: ovrcr_protocol::SessionRunId(1),
                    session: id,
                    size: TerminalSize {
                        rows: 5000,
                        cols: 80,
                    },
                }],
                focused: Some(id),
            },
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    assert!(matches!(
        sink.next(),
        Some(DashboardDelivery::Message(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 83,
                response: Response::Error {
                    code: ErrorCode::InvalidRequest,
                    ..
                },
            },
            ..
        }))
    ));
    assert!(state.dashboard.view().is_none());
    assert_eq!(session.master_size().unwrap(), initial_pty_size);
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&session, events).unwrap();
}

#[test]
fn resize_is_rejected_for_a_split_view() {
    let first_id = SessionId(29);
    let second_id = SessionId(30);
    let (_first_cwd, first, first_events) = spawn_live_test_session(first_id);
    let first_events = apply_test_session_events(Arc::clone(&first), first_events);
    let (_second_cwd, second, second_events) = spawn_live_test_session(second_id);
    let second_events = apply_test_session_events(Arc::clone(&second), second_events);
    let owner = Arc::new(());
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let (state, _dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    extend_test_sessions(
        &state,
        [(first_id, first.clone()), (second_id, second.clone())],
    );
    state.dashboard.install_view_for_test(Some(DashboardView {
        revision: 9,
        panes: vec![
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: first_id,
                size: TerminalSize { rows: 36, cols: 39 },
            },
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: second_id,
                size: TerminalSize { rows: 36, cols: 40 },
            },
        ],
        focused: Some(second_id),
    }));
    let first_pty = first.master_size().unwrap();
    let second_pty = second.master_size().unwrap();
    let mut role = ClientRole::Dashboard;
    // The focused pane of a split view is still one of two: a singleton Resize
    // would publish one size for the whole dashboard.
    let response = handle_request_with_id(
        &state,
        &mut role,
        Request::Resize {
            session: second_id,
            size: TerminalSize {
                rows: 40,
                cols: 120,
            },
        },
        91,
        Some(&owner),
    );
    assert!(
        matches!(
            &response,
            Response::Error {
                code: ErrorCode::InvalidRequest,
                message,
            } if message == "use SetView for split geometry"
        ),
        "{response:?}"
    );
    let unfocused = handle_request_with_id(
        &state,
        &mut role,
        Request::Resize {
            session: first_id,
            size: TerminalSize {
                rows: 40,
                cols: 120,
            },
        },
        92,
        Some(&owner),
    );
    assert!(
        matches!(
            &unfocused,
            Response::Error {
                code: ErrorCode::InvalidRequest,
                message,
            } if message == "use SetView for split geometry"
        ),
        "{unfocused:?}"
    );
    assert_eq!(first.master_size().unwrap(), first_pty);
    assert_eq!(second.master_size().unwrap(), second_pty);
    let view = state.dashboard.view().expect("view retained");
    assert_eq!(view.revision, 9);
    assert_eq!(view.panes.len(), 2);
    assert!(sink.queue.lock().unwrap().messages.is_empty());
    assert!(state.dashboard.owns(&owner));
    cleanup_test_session(&second, second_events).unwrap();
    cleanup_test_session(&first, first_events).unwrap();
}

#[test]
fn set_view_overflow_disconnects_instead_of_dropping_lifecycle_frames() {
    let first_id = SessionId(31);
    let second_id = SessionId(32);
    let (_first_cwd, first, first_events) = spawn_live_test_session(first_id);
    let first_events = apply_test_session_events(Arc::clone(&first), first_events);
    let (_second_cwd, second, second_events) = spawn_live_test_session(second_id);
    let second_events = apply_test_session_events(Arc::clone(&second), second_events);
    let owner = Arc::new(());
    let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    extend_test_sessions(
        &state,
        [(first_id, first.clone()), (second_id, second.clone())],
    );
    // Two panes publish two snapshots and one `Ok`; leaving room for two means
    // the view cannot be published without evicting a queued lifecycle frame.
    let lifecycle = ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy()));
    for _ in 0..(DASHBOARD_QUEUE - 2) {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: lifecycle.clone(),
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: Arc::clone(&owner),
            request_id: 93,
            view: DashboardView {
                revision: 1,
                panes: vec![
                    PaneTarget {
                        run: ovrcr_protocol::SessionRunId(1),
                        session: first_id,
                        size: TerminalSize { rows: 36, cols: 39 },
                    },
                    PaneTarget {
                        run: ovrcr_protocol::SessionRunId(1),
                        session: second_id,
                        size: TerminalSize { rows: 36, cols: 40 },
                    },
                ],
                focused: Some(second_id),
            },
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    assert!(!state.dashboard.is_claimed());
    assert!(state.dashboard.view().is_none());
    assert!(sink.is_closing());
    assert_eq!(client_stream.read(&mut [0_u8; 1]).unwrap(), 0);
    let queued = sink.queue.lock().unwrap();
    assert_eq!(queued.messages.len(), DASHBOARD_QUEUE - 2);
    assert!(
        queued.messages.iter().all(|outbound| matches!(
            outbound.message,
            ServerMessage::Event(ServerEvent::HierarchyChanged(_))
        )),
        "the queued lifecycle frames must survive the refused publication"
    );
    drop(queued);
    // Refusing to publish never touches the processes behind the panes.
    assert_eq!(state.sessions.lock().unwrap().len(), 2);
    assert!(first.master_size().is_ok());
    assert!(second.master_size().is_ok());
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&second, second_events).unwrap();
    cleanup_test_session(&first, first_events).unwrap();
}

#[test]
fn invalid_view_preserves_populated_geometry_and_pty_sizes() {
    let first_id = SessionId(26);
    let second_id = SessionId(27);
    let (_first_cwd, first, first_events) = spawn_live_test_session(first_id);
    let first_events = apply_test_session_events(Arc::clone(&first), first_events);
    let (_second_cwd, second, second_events) = spawn_live_test_session(second_id);
    let second_events = apply_test_session_events(Arc::clone(&second), second_events);
    let owner = Arc::new(());
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    extend_test_sessions(
        &state,
        [(first_id, first.clone()), (second_id, second.clone())],
    );
    let first_size = TerminalSize { rows: 30, cols: 90 };
    let second_size = TerminalSize { rows: 31, cols: 91 };
    let committed = DashboardView {
        revision: 4,
        panes: vec![
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: first_id,
                size: first_size,
            },
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: second_id,
                size: second_size,
            },
        ],
        focused: Some(first_id),
    };
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: owner.clone(),
            request_id: 80,
            view: committed.clone(),
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    for _ in 0..3 {
        assert!(matches!(sink.next(), Some(DashboardDelivery::Message(_))));
    }
    assert_eq!(state.dashboard.view(), Some(committed.clone()));
    assert_eq!(first.terminal_text().0, first_size);
    assert_eq!(second.terminal_text().0, second_size);
    let initial_pty_sizes = (first.master_size().unwrap(), second.master_size().unwrap());
    assert_eq!(initial_pty_sizes, (first_size, second_size));

    let mixed = DashboardView {
        revision: 5,
        panes: vec![
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: first_id,
                size: TerminalSize { rows: 32, cols: 92 },
            },
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: SessionId(999),
                size: TerminalSize { rows: 33, cols: 93 },
            },
        ],
        focused: Some(first_id),
    };
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: owner.clone(),
            request_id: 81,
            view: mixed,
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    assert!(matches!(
        sink.next(),
        Some(DashboardDelivery::Message(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 81,
                response: Response::Error {
                    code: ErrorCode::NotFound,
                    ..
                },
            },
            ..
        }))
    ));
    assert_eq!(state.dashboard.view(), Some(committed.clone()));
    assert_eq!(first.terminal_text().0, first_size);
    assert_eq!(second.terminal_text().0, second_size);
    assert_eq!(
        (first.master_size().unwrap(), second.master_size().unwrap()),
        initial_pty_sizes
    );

    let stale = DashboardView {
        revision: 3,
        panes: vec![
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: first_id,
                size: TerminalSize { rows: 34, cols: 94 },
            },
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: second_id,
                size: TerminalSize { rows: 35, cols: 95 },
            },
        ],
        focused: Some(second_id),
    };
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner,
            request_id: 82,
            view: stale,
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    assert!(matches!(
        sink.next(),
        Some(DashboardDelivery::Message(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 82,
                response: Response::Error {
                    code: ErrorCode::InvalidRequest,
                    ..
                },
            },
            ..
        }))
    ));
    assert_eq!(state.dashboard.view(), Some(committed));
    assert_eq!(first.terminal_text().0, first_size);
    assert_eq!(second.terminal_text().0, second_size);
    assert_eq!(
        (first.master_size().unwrap(), second.master_size().unwrap()),
        initial_pty_sizes
    );
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&second, second_events).unwrap();
    cleanup_test_session(&first, first_events).unwrap();
}

#[test]
fn view_publication_aborts_when_owner_disconnects_during_resize() {
    let id = SessionId(25);
    let (_cwd, session, event_receiver) = spawn_live_test_session(id);
    let events = apply_test_session_events(Arc::clone(&session), event_receiver);
    let owner = Arc::new(());
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink), Some((owner.clone(), server_stream)));
    register_test_session(&state, id, session.clone());
    let snapshot = state
        .dashboard
        .snapshot()
        .expect("owner snapshot before dispatch");
    let delayed_snapshot = state
        .dashboard
        .snapshot()
        .expect("second owner snapshot before dispatch");
    let (start_cleanup, start_cleanup_result) = mpsc::sync_channel(1);
    let (cleanup_started, cleanup_started_result) = mpsc::sync_channel(1);
    let (cleanup_done, cleanup_done_result) = mpsc::sync_channel(1);
    let cleanup_state = Arc::clone(&state);
    let replacement_owner = Arc::new(());
    let replacement_sink = DashboardSink::new();
    let (replacement_stream, _replacement_client) = UnixStream::pair().unwrap();
    let replacement_view = DashboardView {
        revision: 9,
        panes: vec![PaneTarget {
            run: ovrcr_protocol::SessionRunId(1),
            session: id,
            size: TerminalSize { rows: 27, cols: 83 },
        }],
        focused: Some(id),
    };
    let cleanup_owner = Arc::clone(&replacement_owner);
    let cleanup_geometry_owner = Arc::clone(&replacement_owner);
    let cleanup_sink = Arc::clone(&replacement_sink);
    let cleanup_view = replacement_view.clone();
    let cleanup_thread = thread::spawn(move || {
        start_cleanup_result.recv().unwrap();
        cleanup_started.send(()).unwrap();
        cleanup_state.dashboard.disconnect(snapshot);
        cleanup_state.dashboard.claim_with_identity(
            cleanup_sink.clone(),
            cleanup_owner,
            replacement_stream,
        );
        cleanup_state
            .dashboard
            .install_view_for_test(Some(cleanup_view));
        cleanup_state
            .dashboard
            .set_geometry(&cleanup_geometry_owner, TerminalSize { rows: 27, cols: 83 });
        cleanup_done.send(()).unwrap();
    });
    let cleanup_started_result = Arc::new(Mutex::new(cleanup_started_result));
    let cleanup_waiter = Arc::clone(&cleanup_started_result);
    let slot_blocked = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let slot_blocked_probe = Arc::clone(&slot_blocked);
    let state_weak = Arc::downgrade(&state);
    *state.before_view_publish_hook.lock().unwrap() = Some(Arc::new(move || {
        let state = state_weak.upgrade().unwrap();
        slot_blocked_probe.store(
            state.dashboard.slot_is_locked_for_test(),
            std::sync::atomic::Ordering::Release,
        );
        start_cleanup.send(()).unwrap();
        cleanup_waiter.lock().unwrap().recv().unwrap();
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: owner.clone(),
            request_id: 74,
            view: DashboardView {
                revision: 1,
                panes: vec![PaneTarget {
                    run: ovrcr_protocol::SessionRunId(1),
                    session: id,
                    size: TerminalSize { rows: 25, cols: 81 },
                }],
                focused: Some(id),
            },
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    cleanup_done_result
        .recv_timeout(Duration::from_secs(2))
        .expect("cleanup completed after publication released the slot lock");
    cleanup_thread.join().unwrap();
    state.dashboard.disconnect(delayed_snapshot);
    let replacement_slot = state
        .dashboard
        .slot_for_test()
        .as_ref()
        .is_some_and(|slot| Arc::ptr_eq(&slot.identity, &replacement_owner));
    let final_view = state.dashboard.view();
    let final_geometry = state.dashboard.geometry();
    assert!(replacement_slot);
    assert_eq!(final_view, Some(replacement_view));
    assert_eq!(final_geometry, Some(TerminalSize { rows: 27, cols: 83 }));
    assert!(slot_blocked.load(std::sync::atomic::Ordering::Acquire));
    if let Some(snapshot) = state.dashboard.snapshot() {
        state.dashboard.disconnect(snapshot);
    }
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&session, events).unwrap();
}

#[test]
fn set_view_drops_a_session_removed_before_publication() {
    let id = SessionId(63);
    let (_cwd, session, receiver) = spawn_exiting_test_session(id);
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    let owner = Arc::new(());
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    register_test_session(&state, id, session.clone());
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline
        && !matches!(session.summary().phase, SessionPhase::Exited { .. })
    {
        thread::yield_now();
    }
    assert!(matches!(
        session.summary().phase,
        SessionPhase::Exited { .. }
    ));
    // Remove the focused session between pane resolution and publication: the
    // resize seam is the window a slow PTY leaves open for a concurrent
    // RemoveSession on a connection thread.
    let hook_state = Arc::downgrade(&state);
    let run = session.run();
    *state.resize_hook.lock().unwrap() = Some(Arc::new(move |_, _| {
        let state = hook_state
            .upgrade()
            .expect("server state outlives the resize hook");
        state
            .acknowledge_session_stopped(id, run)
            .and_then(|()| state.remove_session(id))
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: owner.clone(),
            request_id: 80,
            view: DashboardView {
                revision: 1,
                panes: vec![PaneTarget {
                    run: ovrcr_protocol::SessionRunId(1),
                    session: id,
                    size: TerminalSize { rows: 25, cols: 81 },
                }],
                focused: Some(id),
            },
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    assert!(matches!(
        queued_dashboard_message(&sink),
        ServerMessage::Response {
            request_id: 80,
            response: Response::Error {
                code: ErrorCode::NotFound,
                ..
            },
        }
    ));
    assert!(sink.queue.lock().unwrap().messages.is_empty());
    assert!(state.dashboard.view().is_none());
    let mut role = ClientRole::Dashboard;
    assert!(matches!(
        handle_request_with_id(
            &state,
            &mut role,
            Request::Input {
                run: ovrcr_protocol::SessionRunId(1),
                session: id,
                bytes: b"stale".to_vec(),
            },
            81,
            Some(&owner),
        ),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&session, events).unwrap();
}

#[test]
fn set_view_drops_a_retained_session_removed_before_publication() {
    let retained_id = SessionId(67);
    let live_id = SessionId(68);
    let (_retained_cwd, retained_session, retained_receiver) =
        spawn_exiting_test_session(retained_id);
    let retained_events =
        apply_test_session_events(Arc::clone(&retained_session), retained_receiver);
    let (_live_cwd, live, live_receiver) = spawn_live_test_session(live_id);
    let live_events = apply_test_session_events(Arc::clone(&live), live_receiver);
    let owner = Arc::new(());
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline
        && !matches!(
            retained_session.summary().phase,
            SessionPhase::Exited { .. }
        )
    {
        thread::yield_now();
    }
    assert!(matches!(
        retained_session.summary().phase,
        SessionPhase::Exited { .. }
    ));
    state
        .retained
        .lock()
        .register_fixture(&retained_session)
        .unwrap();
    register_test_session(&state, live_id, live.clone());
    // Remove the focused retained row between pane resolution and publication:
    // the live pane's resize seam is the window a slow PTY leaves open for a
    // concurrent RemoveSession on a connection thread.
    let hook_state = Arc::downgrade(&state);
    *state.resize_hook.lock().unwrap() = Some(Arc::new(move |session, size| {
        let state = hook_state
            .upgrade()
            .expect("server state outlives the resize hook");
        state
            .remove_session(retained_id)
            .and_then(|()| session.resize(size))
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: owner.clone(),
            request_id: 84,
            view: DashboardView {
                revision: 1,
                panes: vec![
                    PaneTarget {
                        run: ovrcr_protocol::SessionRunId(1),
                        session: retained_id,
                        size: TerminalSize { rows: 25, cols: 81 },
                    },
                    PaneTarget {
                        run: ovrcr_protocol::SessionRunId(1),
                        session: live_id,
                        size: TerminalSize { rows: 26, cols: 82 },
                    },
                ],
                focused: Some(retained_id),
            },
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    assert!(matches!(
        queued_dashboard_message(&sink),
        ServerMessage::Response {
            request_id: 84,
            response: Response::Error {
                code: ErrorCode::NotFound,
                ..
            },
        }
    ));
    assert!(sink.queue.lock().unwrap().messages.is_empty());
    assert!(state.dashboard.view().is_none());
    assert!(
        state.retained.lock().get(retained_id).is_none(),
        "removed retained row must stay deleted"
    );
    let mut role = ClientRole::Dashboard;
    assert!(matches!(
        handle_request_with_id(
            &state,
            &mut role,
            Request::Input {
                run: ovrcr_protocol::SessionRunId(1),
                session: retained_id,
                bytes: b"stale".to_vec(),
            },
            85,
            Some(&owner),
        ),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&live, live_events).unwrap();
    cleanup_test_session(&retained_session, retained_events).unwrap();
}

#[test]
fn set_view_publishes_without_an_unfocused_pane_removed_before_publication() {
    let focused_id = SessionId(65);
    let removed_id = SessionId(66);
    let (_focused_cwd, focused, focused_events) = spawn_live_test_session(focused_id);
    let focused_events = apply_test_session_events(Arc::clone(&focused), focused_events);
    let (_removed_cwd, removed, removed_receiver) = spawn_exiting_test_session(removed_id);
    let removed_events = apply_test_session_events(Arc::clone(&removed), removed_receiver);
    let owner = Arc::new(());
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    extend_test_sessions(
        &state,
        [(focused_id, focused.clone()), (removed_id, removed.clone())],
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline
        && !matches!(removed.summary().phase, SessionPhase::Exited { .. })
    {
        thread::yield_now();
    }
    assert!(matches!(
        removed.summary().phase,
        SessionPhase::Exited { .. }
    ));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_for_hook = Arc::clone(&calls);
    let hook_state = Arc::downgrade(&state);
    let removed_run = removed.run();
    *state.resize_hook.lock().unwrap() = Some(Arc::new(move |session, size| {
        if calls_for_hook.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            session.resize(size)
        } else {
            let state = hook_state
                .upgrade()
                .expect("server state outlives the resize hook");
            state
                .acknowledge_session_stopped(removed_id, removed_run)
                .and_then(|()| state.remove_session(removed_id))
        }
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let focused_pane = PaneTarget {
        run: ovrcr_protocol::SessionRunId(1),
        session: focused_id,
        size: TerminalSize { rows: 25, cols: 81 },
    };
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: owner.clone(),
            request_id: 82,
            view: DashboardView {
                revision: 1,
                panes: vec![
                    focused_pane.clone(),
                    PaneTarget {
                        run: ovrcr_protocol::SessionRunId(1),
                        session: removed_id,
                        size: TerminalSize { rows: 26, cols: 82 },
                    },
                ],
                focused: Some(focused_id),
            },
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    // The client still receives one snapshot per requested pane and the final
    // acknowledgement, so its readiness rules still complete.
    for session in [focused_id, removed_id] {
        assert!(matches!(
            queued_dashboard_message(&sink),
            ServerMessage::Response {
                request_id: 82,
                response: Response::Screen {
                    session: queued,
                    revision: 1,
                    ..
                },
            } if queued == session
        ));
    }
    assert!(matches!(
        queued_dashboard_message(&sink),
        ServerMessage::Response {
            request_id: 82,
            response: Response::Ok,
        }
    ));
    // The published view omits the removed pane, so neither output nor input is
    // admitted for it.
    assert_eq!(
        state.dashboard.view(),
        Some(DashboardView {
            revision: 1,
            panes: vec![focused_pane],
            focused: Some(focused_id),
        })
    );
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&removed, removed_events).unwrap();
    cleanup_test_session(&focused, focused_events).unwrap();
}

#[test]
fn terminal_delivery_is_final_entry_and_closing_stays_sticky() {
    let sink = DashboardSink::new();
    assert_eq!(
        sink.enqueue(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 1,
                response: Response::Ok,
            },
            completion: None,
        }),
        Enqueue::Queued
    );
    let (completion, result) = mpsc::sync_channel(1);
    assert!(matches!(
        sink.enqueue_terminal(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 2,
                response: Response::Error {
                    code: ErrorCode::PartialFailure,
                    message: "resize failed".into(),
                },
            },
            completion: Some(completion),
        }),
        Enqueue::Queued
    ));
    assert!(sink.is_closing());
    // A closing queue drains rather than closing: the message is dropped, but
    // the socket stays open so the writer can flush the terminal frame.
    assert!(matches!(
        sink.enqueue(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 3,
                response: Response::Ok,
            },
            completion: None,
        }),
        Enqueue::Draining
    ));
    assert!(matches!(
        sink.enqueue_terminal(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 4,
                response: Response::Ok,
            },
            completion: None,
        }),
        Enqueue::Draining
    ));
    assert!(matches!(
        sink.next(),
        Some(DashboardDelivery::Message(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 1,
                response: Response::Ok,
            },
            ..
        }))
    ));
    let DashboardDelivery::Terminal(outbound) = sink.next().unwrap() else {
        panic!("terminal delivery was not the final queued entry");
    };
    assert!(matches!(
        outbound.message,
        ServerMessage::Response {
            request_id: 2,
            response: Response::Error {
                code: ErrorCode::PartialFailure,
                ..
            },
        }
    ));
    outbound.completion.unwrap().send(Ok(())).unwrap();
    assert!(result.recv_timeout(Duration::from_secs(1)).unwrap().is_ok());
    assert!(!sink.replace_view(
        &DashboardView {
            revision: 1,
            panes: Vec::new(),
            focused: None,
        },
        4,
        Vec::new(),
    ));
}

#[test]
fn view_revision_floor_survives_exit_and_removal() {
    let exited_id = SessionId(41);
    let survivor_id = SessionId(42);
    let (_exited_cwd, exited, exited_receiver) = spawn_exiting_test_session(exited_id);
    let (_survivor_cwd, survivor, survivor_receiver) = spawn_live_test_session(survivor_id);
    let owner = Arc::new(());
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    extend_test_sessions(
        &state,
        [(exited_id, exited.clone()), (survivor_id, survivor.clone())],
    );
    state.dashboard.install_view_for_test(Some(DashboardView {
        revision: 10,
        panes: vec![
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: exited_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: survivor_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
        ],
        focused: Some(exited_id),
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let exited_dispatch = state.dispatch.clone();
    let exited_thread = thread::spawn(move || {
        while let Ok(event) = exited_receiver.recv() {
            let did_exit = matches!(event, SessionEvent::Exited { .. });
            exited_dispatch
                .send(DispatchMessage::Session(event))
                .unwrap();
            if did_exit {
                break;
            }
        }
    });
    let survivor_dispatch = state.dispatch.clone();
    let survivor_thread = thread::spawn(move || {
        while let Ok(event) = survivor_receiver.recv() {
            let did_exit = matches!(event, SessionEvent::Exited { .. });
            survivor_dispatch
                .send(DispatchMessage::Session(event))
                .unwrap();
            if did_exit {
                break;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline
        && !matches!(exited.summary().phase, SessionPhase::Exited { .. })
    {
        thread::yield_now();
    }
    assert!(matches!(
        exited.summary().phase,
        SessionPhase::Exited { .. }
    ));
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline
        && !sink.queue.lock().unwrap().messages.iter().any(|outbound| {
            matches!(
                &outbound.message,
                ServerMessage::Event(ServerEvent::SessionChanged(summary))
                    if summary.id == exited_id
                        && matches!(summary.phase, SessionPhase::Exited { .. })
            )
        })
    {
        thread::yield_now();
    }
    let queued = sink.queue.lock().unwrap();
    let exited_output = queued.messages.iter().position(|outbound| {
        matches!(
            &outbound.message,
            ServerMessage::Event(ServerEvent::Output { session, bytes, .. })
                if *session == exited_id && bytes == b"FINAL"
        )
    });
    let exited_lifecycle = queued.messages.iter().position(|outbound| {
        matches!(
            &outbound.message,
            ServerMessage::Event(ServerEvent::SessionChanged(summary))
                if summary.id == exited_id
                    && matches!(summary.phase, SessionPhase::Exited { .. })
        )
    });
    assert!(
        exited_output
            .is_some_and(|output| { exited_lifecycle.is_some_and(|lifecycle| output < lifecycle) })
    );
    drop(queued);
    assert_eq!(
        state.dashboard.view(),
        Some(DashboardView {
            revision: 10,
            panes: vec![
                PaneTarget {
                    run: ovrcr_protocol::SessionRunId(1),
                    session: exited_id,
                    size: TerminalSize { rows: 24, cols: 80 },
                },
                PaneTarget {
                    run: ovrcr_protocol::SessionRunId(1),
                    session: survivor_id,
                    size: TerminalSize { rows: 24, cols: 80 },
                },
            ],
            focused: Some(exited_id),
        })
    );
    let mut role = ClientRole::Dashboard;
    assert!(matches!(
        handle_request_with_id(
            &state,
            &mut role,
            Request::Input {
                run: ovrcr_protocol::SessionRunId(1),
                session: exited_id,
                bytes: b"stale".to_vec(),
            },
            41,
            Some(&owner),
        ),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));

    state
        .dispatch
        .send(DispatchMessage::Session(SessionEvent::Output {
            run: ovrcr_protocol::SessionRunId(1),
            id: survivor_id,
            bytes: b"SURVIVOR".to_vec(),
        }))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline
        && !sink.queue.lock().unwrap().messages.iter().any(|outbound| {
            matches!(
                &outbound.message,
                ServerMessage::Event(ServerEvent::Output { run: ovrcr_protocol::SessionRunId(1), session, revision, bytes })
                    if *session == survivor_id && *revision == 10 && bytes == b"SURVIVOR"
            )
        })
    {
        thread::yield_now();
    }
    assert!(sink.queue.lock().unwrap().messages.iter().any(|outbound| {
        matches!(
            &outbound.message,
            ServerMessage::Event(ServerEvent::Output { run: ovrcr_protocol::SessionRunId(1), session, revision, bytes })
                if *session == survivor_id && *revision == 10 && bytes == b"SURVIVOR"
        )
    }));

    survivor.terminate(Duration::from_secs(2)).unwrap();
    survivor_thread.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline
        && !matches!(survivor.summary().phase, SessionPhase::Exited { .. })
    {
        thread::yield_now();
    }
    assert_eq!(
        state.dashboard.view().as_ref().map(|view| view.revision),
        Some(10)
    );
    state
        .acknowledge_session_stopped(exited_id, exited.run())
        .unwrap();
    state.remove_session(exited_id).unwrap();
    let after_removal = state.dashboard.view().unwrap();
    assert_eq!(after_removal.revision, 10);
    assert!(after_removal.panes.is_empty());
    assert_eq!(after_removal.focused, None);
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: owner.clone(),
            request_id: 42,
            view: DashboardView {
                revision: 9,
                panes: Vec::new(),
                focused: None,
            },
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    let queued = sink.queue.lock().unwrap();
    assert!(queued.messages.iter().any(|outbound| {
        matches!(
            &outbound.message,
            ServerMessage::Response {
                request_id: 42,
                response: Response::Error {
                    code: ErrorCode::InvalidRequest,
                    ..
                },
            }
        )
    }));
    drop(queued);
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    exited_thread.join().unwrap();
    if let Err(error) = exited.terminate(Duration::from_secs(2)) {
        assert!(error.is::<crate::session::AlreadyExited>(), "{error:#}");
    }
}

#[test]
fn partial_resize_failure_does_not_restore_revision_after_owner_replacement() {
    let first_id = SessionId(55);
    let second_id = SessionId(56);
    let (_first_cwd, first, first_receiver) = spawn_live_test_session(first_id);
    let first_events = apply_test_session_events(Arc::clone(&first), first_receiver);
    let (_second_cwd, second, second_receiver) = spawn_live_test_session(second_id);
    let second_events = apply_test_session_events(Arc::clone(&second), second_receiver);
    let (old_server, _old_client) = UnixStream::pair().unwrap();
    let old_sink = DashboardSink::new();
    let old_owner = Arc::new(());
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(old_sink), Some((old_owner.clone(), old_server)));
    extend_test_sessions(
        &state,
        [(first_id, first.clone()), (second_id, second.clone())],
    );
    state.dashboard.install_view_for_test(Some(DashboardView {
        revision: 10,
        panes: vec![
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: first_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
            PaneTarget {
                run: ovrcr_protocol::SessionRunId(1),
                session: second_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
        ],
        focused: Some(second_id),
    }));

    let replacement_owner = Arc::new(());
    let replacement_sink = DashboardSink::new();
    let (replacement_server, _replacement_client) = UnixStream::pair().unwrap();
    let resize_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let resize_calls_for_hook = Arc::clone(&resize_calls);
    let hook_state = Arc::clone(&state);
    let hook_owner = Arc::clone(&replacement_owner);
    let hook_sink = Arc::clone(&replacement_sink);
    *state.resize_hook.lock().unwrap() = Some(Arc::new(move |session, size| {
        let call = resize_calls_for_hook.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if call == 0 {
            session.resize(size)?;
            let snapshot = hook_state.dashboard.snapshot().expect("old owner snapshot");
            hook_state.dashboard.disconnect(snapshot);
            hook_state.dashboard.claim_with_identity(
                hook_sink.clone(),
                hook_owner.clone(),
                replacement_server.try_clone()?,
            );
            Ok(())
        } else {
            Err(anyhow::anyhow!("second resize failed"))
        }
    }));

    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: old_owner,
            request_id: 56,
            view: DashboardView {
                revision: 11,
                panes: vec![
                    PaneTarget {
                        run: ovrcr_protocol::SessionRunId(1),
                        session: first_id,
                        size: TerminalSize { rows: 25, cols: 81 },
                    },
                    PaneTarget {
                        run: ovrcr_protocol::SessionRunId(1),
                        session: second_id,
                        size: TerminalSize { rows: 26, cols: 82 },
                    },
                ],
                focused: Some(second_id),
            },
            completion,
        })
        .unwrap();
    assert!(matches!(
        result.recv_timeout(Duration::from_secs(2)).unwrap(),
        DispatchCompletion::Complete
    ));
    assert_eq!(resize_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(state.dashboard.view().is_none());
    assert!(
        state
            .dashboard
            .slot_for_test()
            .as_ref()
            .is_some_and(|slot| Arc::ptr_eq(&slot.identity, &replacement_owner))
    );

    *state.resize_hook.lock().unwrap() = None;
    let (replacement_completion, replacement_result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::SetView {
            owner: replacement_owner.clone(),
            request_id: 57,
            view: DashboardView {
                revision: 1,
                panes: vec![PaneTarget {
                    run: ovrcr_protocol::SessionRunId(1),
                    session: first_id,
                    size: TerminalSize { rows: 25, cols: 81 },
                }],
                focused: Some(first_id),
            },
            completion: replacement_completion,
        })
        .unwrap();
    assert!(matches!(
        replacement_result
            .recv_timeout(Duration::from_secs(2))
            .unwrap(),
        DispatchCompletion::Complete
    ));
    assert_eq!(
        state.dashboard.view().as_ref().map(|view| view.revision),
        Some(1)
    );
    assert_eq!(state.dashboard.view().as_ref().unwrap().panes.len(), 1);

    if let Some(snapshot) = state.dashboard.snapshot() {
        state.dashboard.disconnect(snapshot);
    }
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&second, second_events).unwrap();
    cleanup_test_session(&first, first_events).unwrap();
}

#[test]
fn partial_resize_connection_delivers_error_before_owner_close() {
    let first_id = SessionId(51);
    let second_id = SessionId(52);
    let (_first_cwd, first, first_events) = spawn_live_test_session(first_id);
    let first_events = apply_test_session_events(Arc::clone(&first), first_events);
    let (_second_cwd, second, second_events) = spawn_live_test_session(second_id);
    let second_events = apply_test_session_events(Arc::clone(&second), second_events);
    let (state, dispatch_receiver) = test_state_with_dispatch(None, None);
    extend_test_sessions(
        &state,
        [(first_id, first.clone()), (second_id, second.clone())],
    );
    let first_size = TerminalSize { rows: 25, cols: 81 };
    let second_size = TerminalSize { rows: 26, cols: 82 };
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_for_hook = Arc::clone(&calls);
    *state.resize_hook.lock().unwrap() = Some(Arc::new(move |session, size| {
        if calls_for_hook.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            session.resize(size)
        } else {
            Err(anyhow::anyhow!("second resize failed"))
        }
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
    let handler_state = Arc::clone(&state);
    let handler = thread::spawn(move || handle_connection(handler_state, server_stream));
    exchange_preamble(&mut client_stream).unwrap();
    write_frame(
        &mut client_stream,
        &ClientMessage {
            request_id: 51,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut client_stream).unwrap(),
        ServerMessage::Response {
            request_id: 51,
            response: Response::Hierarchy(_),
        }
    ));
    write_frame(
        &mut client_stream,
        &ClientMessage {
            request_id: 53,
            request: Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![
                        PaneTarget {
                            run: ovrcr_protocol::SessionRunId(1),
                            session: first_id,
                            size: first_size,
                        },
                        PaneTarget {
                            run: ovrcr_protocol::SessionRunId(1),
                            session: second_id,
                            size: second_size,
                        },
                    ],
                    focused: Some(second_id),
                },
            },
        },
    )
    .unwrap();
    client_stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let message = read_frame::<ServerMessage>(&mut client_stream).unwrap();
    assert!(matches!(
        message,
        ServerMessage::Response {
            request_id: 53,
            response: Response::Error {
                code: ErrorCode::PartialFailure,
                ..
            },
        }
    ));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(matches!(first.summary().phase, SessionPhase::Running));
    assert!(matches!(second.summary().phase, SessionPhase::Running));
    assert_eq!(first.terminal_text().0, first_size);
    assert_ne!(second.terminal_text().0, second_size);
    let mut eof_probe = [0_u8; 1];
    assert_eq!(client_stream.read(&mut eof_probe).unwrap(), 0);
    handler.join().unwrap();
    assert!(!state.dashboard.is_claimed());
    assert!(state.dashboard.view().is_none());
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&second, second_events).unwrap();
    cleanup_test_session(&first, first_events).unwrap();
}

#[test]
fn partial_resize_blocked_writer_times_out_and_closes_owner() {
    let first_id = SessionId(53);
    let second_id = SessionId(54);
    let (_first_cwd, first, first_events) = spawn_live_test_session(first_id);
    let first_events = apply_test_session_events(Arc::clone(&first), first_events);
    let (_second_cwd, second, second_events) = spawn_live_test_session(second_id);
    let second_events = apply_test_session_events(Arc::clone(&second), second_events);
    let (state, dispatch_receiver) = test_state_with_dispatch(None, None);
    extend_test_sessions(
        &state,
        [(first_id, first.clone()), (second_id, second.clone())],
    );
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_for_hook = Arc::clone(&calls);
    *state.resize_hook.lock().unwrap() = Some(Arc::new(move |session, size| {
        if calls_for_hook.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            session.resize(size)
        } else {
            Err(anyhow::anyhow!("second resize failed"))
        }
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
    let send_buffer = 1_i32;
    let result = unsafe {
        libc::setsockopt(
            server_stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&send_buffer as *const i32).cast(),
            std::mem::size_of_val(&send_buffer) as libc::socklen_t,
        )
    };
    assert_eq!(result, 0);
    let handler_state = Arc::clone(&state);
    let handler = thread::spawn(move || handle_connection(handler_state, server_stream));
    exchange_preamble(&mut client_stream).unwrap();
    write_frame(
        &mut client_stream,
        &ClientMessage {
            request_id: 61,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut client_stream).unwrap(),
        ServerMessage::Response {
            request_id: 61,
            response: Response::Hierarchy(_),
        }
    ));
    let sink = state
        .dashboard
        .slot_for_test()
        .as_ref()
        .unwrap()
        .sink
        .clone();
    // The writer signals from inside its write path: the blocked write is then
    // a fact, not a guess about how many yields it takes to reach one.
    let (write_started, writer_entered) = mpsc::sync_channel(8);
    *state.before_dashboard_write_hook.lock().unwrap() = Some(Arc::new(move || {
        let _ = write_started.try_send(());
    }));
    for revision in 1..=4 {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::Output {
                    run: ovrcr_protocol::SessionRunId(1),
                    session: first_id,
                    revision,
                    bytes: vec![b'x'; ovrcr_protocol::MAX_FRAME_BYTES - 128],
                }),
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    writer_entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the dashboard writer must reach its write path");
    let started = Instant::now();
    write_frame(
        &mut client_stream,
        &ClientMessage {
            request_id: 63,
            request: Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![
                        PaneTarget {
                            run: ovrcr_protocol::SessionRunId(1),
                            session: first_id,
                            size: TerminalSize { rows: 25, cols: 81 },
                        },
                        PaneTarget {
                            run: ovrcr_protocol::SessionRunId(1),
                            session: second_id,
                            size: TerminalSize { rows: 26, cols: 82 },
                        },
                    ],
                    focused: Some(second_id),
                },
            },
        },
    )
    .unwrap();
    handler.join().unwrap();
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(1_500),
        "the blocked writer must be given its close timeout, took {elapsed:?}"
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(!state.dashboard.is_claimed());
    assert!(state.dashboard.view().is_none());
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    let _ = client_stream.shutdown(Shutdown::Both);
    cleanup_test_session(&second, second_events).unwrap();
    cleanup_test_session(&first, first_events).unwrap();
}

#[test]
fn terminal_frame_survives_a_concurrent_lifecycle_event() {
    let first_id = SessionId(61);
    let second_id = SessionId(62);
    let (_first_cwd, first, first_events) = spawn_live_test_session(first_id);
    let first_events = apply_test_session_events(Arc::clone(&first), first_events);
    let (_second_cwd, second, second_events) = spawn_live_test_session(second_id);
    let second_events = apply_test_session_events(Arc::clone(&second), second_events);
    let (state, dispatch_receiver) = test_state_with_dispatch(None, None);
    extend_test_sessions(
        &state,
        [(first_id, first.clone()), (second_id, second.clone())],
    );
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_for_hook = Arc::clone(&calls);
    *state.resize_hook.lock().unwrap() = Some(Arc::new(move |session, size| {
        if calls_for_hook.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            session.resize(size)
        } else {
            Err(anyhow::anyhow!("second resize failed"))
        }
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
    let send_buffer = 1_i32;
    let result = unsafe {
        libc::setsockopt(
            server_stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&send_buffer as *const i32).cast(),
            std::mem::size_of_val(&send_buffer) as libc::socklen_t,
        )
    };
    assert_eq!(result, 0);
    let handler_state = Arc::clone(&state);
    let handler = thread::spawn(move || handle_connection(handler_state, server_stream));
    exchange_preamble(&mut client_stream).unwrap();
    write_frame(
        &mut client_stream,
        &ClientMessage {
            request_id: 71,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut client_stream).unwrap(),
        ServerMessage::Response {
            request_id: 71,
            response: Response::Hierarchy(_),
        }
    ));
    let sink = state
        .dashboard
        .slot_for_test()
        .as_ref()
        .unwrap()
        .sink
        .clone();
    // A frame larger than the socket buffer parks the writer while this client
    // is not reading, so the terminal frame stays queued behind it. Keep it
    // small enough to drain well inside the handler's close timeout.
    assert_eq!(
        sink.enqueue(DashboardOutbound {
            message: response_message(
                72,
                Response::TerminalText {
                    session: first_id,
                    size: TerminalSize { rows: 24, cols: 80 },
                    text: "x".repeat(32 * 1024),
                },
            ),
            completion: None,
        }),
        Enqueue::Queued
    );
    write_frame(
        &mut client_stream,
        &ClientMessage {
            request_id: 73,
            request: Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![
                        PaneTarget {
                            run: ovrcr_protocol::SessionRunId(1),
                            session: first_id,
                            size: TerminalSize { rows: 25, cols: 81 },
                        },
                        PaneTarget {
                            run: ovrcr_protocol::SessionRunId(1),
                            session: second_id,
                            size: TerminalSize { rows: 26, cols: 82 },
                        },
                    ],
                    focused: Some(second_id),
                },
            },
        },
    )
    .unwrap();
    // The deadline has to stay under TERMINAL_FAILURE_CLOSE_TIMEOUT: once that
    // elapses the writer tears the slot down, and the assertions below about a
    // still-registered dashboard would race it.
    let deadline = Instant::now() + Duration::from_millis(1_500);
    while Instant::now() < deadline && !sink.is_closing() {
        thread::yield_now();
    }
    assert!(
        sink.is_closing(),
        "partial resize did not queue the terminal frame"
    );
    // The lifecycle event must be dropped while the sink drains. Treating it as
    // a disconnect shuts the socket down before the writer flushes the
    // terminal frame, and the client sees a bare EOF instead of the error.
    assert!(
        !state
            .dashboard
            .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                state.hierarchy()
            )))
    );
    assert!(state.dashboard.is_claimed());
    client_stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut client_stream).unwrap(),
        ServerMessage::Response {
            request_id: 72,
            response: Response::TerminalText { .. },
        }
    ));
    assert!(matches!(
        read_frame::<ServerMessage>(&mut client_stream).unwrap(),
        ServerMessage::Response {
            request_id: 73,
            response: Response::Error {
                code: ErrorCode::PartialFailure,
                ..
            },
        }
    ));
    assert_eq!(client_stream.read(&mut [0_u8; 1]).unwrap(), 0);
    assert!(join_test_thread_bounded(handler, Duration::from_secs(5)));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(!state.dashboard.is_claimed());
    assert!(state.dashboard.view().is_none());
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&second, second_events).unwrap();
    cleanup_test_session(&first, first_events).unwrap();
}

#[test]
fn agent_report_queue_full_is_a_structured_conflict() {
    let (state, receiver) = test_state_with_dispatch(None, None);
    for sequence in 0..RAW_DISPATCH_QUEUE_CAPACITY {
        let (completion, _) = mpsc::sync_channel(1);
        state
            .dispatch
            .try_send(DispatchMessage::AgentReport {
                report: AgentReport {
                    session: SessionId(sequence as u64 + 1),
                    capability: [0_u8; 32],
                    sequence: None,
                    update: ovrcr_protocol::AgentUpdate::Activity(AgentActivity::Busy),
                },
                completion,
            })
            .unwrap();
    }
    assert_eq!(
        state.dispatch.snapshot().pending_items,
        RAW_DISPATCH_QUEUE_CAPACITY
    );
    assert!(state.dispatch.snapshot().pending_bytes > 0);
    let mut role = ClientRole::Control;
    let response = state.handle_request(
        &mut role,
        Request::AgentReport(AgentReport {
            session: SessionId(999),
            capability: [0_u8; 32],
            sequence: None,
            update: ovrcr_protocol::AgentUpdate::Activity(AgentActivity::Busy),
        }),
    );
    assert_eq!(
        response,
        Response::Error {
            code: ErrorCode::Conflict,
            message: "dispatcher queue is full".into(),
        }
    );
    assert_eq!(state.dispatch.snapshot().rejected, 1);
    for _ in 0..RAW_DISPATCH_QUEUE_CAPACITY {
        drop(receiver.recv().unwrap());
    }
    assert_eq!(state.dispatch.snapshot().pending_items, 0);
}

#[test]
fn hook_socket_resolution_canonicalizes_parent_and_rejects_leaf_symlink() {
    let root = tempfile::tempdir().unwrap();
    let real_parent = root.path().join("real");
    std::fs::create_dir(&real_parent).unwrap();
    let symlink_parent = root.path().join("link");
    std::os::unix::fs::symlink(&real_parent, &symlink_parent).unwrap();
    let socket = symlink_parent.join("server.sock");
    let expected = real_parent.canonicalize().unwrap().join("server.sock");
    assert_eq!(resolve_bound_socket(&socket).unwrap(), expected);

    let listener = UnixListener::bind(&socket).unwrap();
    assert_eq!(resolve_bound_socket(&socket).unwrap(), expected);
    drop(listener);
    std::fs::remove_file(&socket).unwrap();

    let symlink_leaf = root.path().join("symlink.sock");
    std::os::unix::fs::symlink(&expected, &symlink_leaf).unwrap();
    assert!(resolve_bound_socket(&symlink_leaf).is_err());
}

#[test]
fn relative_bound_socket_validates_spawn_and_child_cwd_is_distinct() {
    let root = tempfile::tempdir().unwrap();
    let child_cwd = root.path().join("child");
    std::fs::create_dir(&child_cwd).unwrap();
    let socket = PathBuf::from(format!(".ovrcr-task2-relative-{}.sock", std::process::id()));
    let listener = UnixListener::bind(&socket).unwrap();
    let expected = std::env::current_dir()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join(socket.file_name().unwrap());
    assert_eq!(validate_bound_socket(&socket).unwrap(), expected);

    let identity = root.path().join("identity");
    let registry = Registry {
        projects: vec![crate::config::ProjectRecord {
            name: "project".into(),
            repo: root.path().to_path_buf(),
            workspace_root: root.path().to_path_buf(),
            workspaces: vec![crate::config::WorkspaceRecord {
                name: "workspace".into(),
                path: child_cwd.clone(),
                branch: "main".into(),
            }],
        }],
    };
    let (state, _dispatch_receiver, event_receiver) =
        test_state_with_socket(socket.clone(), registry);
    let summary = state
        .create_session(ovrcr_protocol::CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: "project".into(),
            workspace: "workspace".into(),
            name: "relative".into(),
            label: None,
            argv: vec![
                "sh".into(),
                "-c".into(),
                "printf '%s' \"$OVRCR_HOOK_SOCKET\" > \"$1\"".into(),
                "ovrcr-relative".into(),
                identity.clone().into_os_string(),
            ],
        })
        .unwrap();
    assert_ne!(child_cwd, std::env::current_dir().unwrap());
    let session = state
        .sessions
        .lock()
        .unwrap()
        .get(&summary.id)
        .cloned()
        .unwrap();
    let event_session = Arc::clone(&session);
    let event_thread = thread::spawn(move || {
        while let Ok(event) = event_receiver.recv() {
            let exited = matches!(event, SessionEvent::Exited { .. });
            event_session.apply_event(event);
            if exited {
                break;
            }
        }
    });
    let expected_identity = expected.to_string_lossy().into_owned();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut identity_contents = None;
    while Instant::now() < deadline {
        if let Ok(contents) = std::fs::read_to_string(&identity)
            && contents == expected_identity
        {
            identity_contents = Some(contents);
            break;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    assert_eq!(
        identity_contents.as_deref(),
        Some(expected_identity.as_str())
    );
    session.wait_until_exited(Duration::from_secs(2)).unwrap();
    event_thread.join().unwrap();
    state
        .acknowledge_session_stopped(summary.id, session.run())
        .unwrap();
    state.remove_session(summary.id).unwrap();

    drop(listener);
    std::fs::remove_file(&socket).unwrap();
    let error = state
        .create_session(ovrcr_protocol::CreateSessionRequest {
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: "project".into(),
            workspace: "workspace".into(),
            name: "missing".into(),
            label: None,
            argv: vec!["sh".into()],
        })
        .unwrap_err();
    assert!(error.to_string().contains("bound server socket"));
    assert!(state.sessions.lock().unwrap().is_empty());
}

#[test]
fn lifecycle_response_preserves_error_chain_and_code() {
    let error = lifecycle_error(ErrorCode::NotFound, "missing executable").context("spawn session");
    let response = error_for_lifecycle(error);
    assert_eq!(
        response,
        Response::Error {
            code: ErrorCode::NotFound,
            message: "spawn session: missing executable".into(),
        }
    );
}

fn test_state(
    dashboard: Option<Arc<DashboardSink>>,
    stream: Option<(Arc<()>, UnixStream)>,
) -> Arc<ServerState> {
    test_state_with_dispatch(dashboard, stream).0
}

fn test_state_with_dispatch(
    dashboard: Option<Arc<DashboardSink>>,
    stream: Option<(Arc<()>, UnixStream)>,
) -> (Arc<ServerState>, ReportingReceiver<DispatchMessage>) {
    let (events, _) = event_channel(None);
    let (dispatch, receiver) = dispatch_channel(Some(&ReportingQueueMonitor::default()));
    let (state, receiver) = (
        Arc::new(ServerState {
            tasks: None,
            socket: PathBuf::from("/tmp/ovrcr-test.sock"),
            registry_path: PathBuf::from("config.toml"),
            registry: Mutex::new(Registry::default()),
            sessions: Mutex::new(HashMap::new()),
            dashboard: ActiveDashboard::default(),
            retained: parking_lot::Mutex::new(SessionStore::in_memory()),
            mutation_lock: Mutex::new(()),
            dispatch,
            shutdown: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            events: Mutex::new(Some(events)),
            #[cfg(test)]
            resize_hook: Mutex::new(None),
            before_view_publish_hook: Mutex::new(None),
            before_dashboard_write_hook: Mutex::new(None),
            #[cfg(feature = "acceptance-diagnostics")]
            dashboard_monitor: None,
        }),
        receiver,
    );
    if let Some((identity, stream)) = stream {
        state
            .dashboard
            .claim_with_identity(dashboard.expect("sink"), identity, stream);
    }
    (state, receiver)
}

fn test_state_with_socket(
    socket: PathBuf,
    registry: Registry,
) -> (
    Arc<ServerState>,
    ReportingReceiver<DispatchMessage>,
    ReportingReceiver<SessionEvent>,
) {
    let (events, event_receiver) = event_channel(None);
    let (dispatch, dispatch_receiver) = dispatch_channel(None);
    (
        Arc::new(ServerState {
            tasks: None,
            socket,
            registry_path: PathBuf::from("config.toml"),
            registry: Mutex::new(registry),
            sessions: Mutex::new(HashMap::new()),
            dashboard: ActiveDashboard::default(),
            retained: parking_lot::Mutex::new(SessionStore::in_memory()),
            mutation_lock: Mutex::new(()),
            dispatch,
            shutdown: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            events: Mutex::new(Some(events)),
            #[cfg(test)]
            resize_hook: Mutex::new(None),
            before_view_publish_hook: Mutex::new(None),
            before_dashboard_write_hook: Mutex::new(None),
            #[cfg(feature = "acceptance-diagnostics")]
            dashboard_monitor: None,
        }),
        dispatch_receiver,
        event_receiver,
    )
}

fn spawn_live_test_session(
    id: SessionId,
) -> (
    tempfile::TempDir,
    Arc<Session>,
    ReportingReceiver<SessionEvent>,
) {
    spawn_live_test_session_with_hook(id, None)
}

fn spawn_live_test_session_with_hook(
    id: SessionId,
    hook_env: Option<HookEnvironment>,
) -> (
    tempfile::TempDir,
    Arc<Session>,
    ReportingReceiver<SessionEvent>,
) {
    let cwd = tempfile::tempdir().unwrap();
    let (events, receiver) = event_channel(None);
    let session = Session::spawn_registered(
        id,
        SessionSpec {
            run: ovrcr_protocol::SessionRunId(1),
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: format!("live-{}", id.0),
            label: "sh".into(),
            cwd: cwd.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "trap '' HUP TERM; while :; do sleep 1; done".into(),
            ],
            hook_env,
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
        NO_REGISTER,
    )
    .unwrap();
    (cwd, session, receiver)
}

fn spawn_exiting_test_session(
    id: SessionId,
) -> (
    tempfile::TempDir,
    Arc<Session>,
    ReportingReceiver<SessionEvent>,
) {
    let cwd = tempfile::tempdir().unwrap();
    let (events, receiver) = event_channel(None);
    let session = Session::spawn_registered(
        id,
        SessionSpec {
            run: ovrcr_protocol::SessionRunId(1),
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: format!("exiting-{}", id.0),
            label: "sh".into(),
            cwd: cwd.path().to_path_buf(),
            argv: vec!["sh".into(), "-c".into(), "printf FINAL; exit 0".into()],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
        NO_REGISTER,
    )
    .unwrap();
    (cwd, session, receiver)
}

struct TestSessionEvents {
    cancel: Arc<AtomicBool>,
    finished: Receiver<()>,
    handle: Option<JoinHandle<()>>,
}

impl TestSessionEvents {
    fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    fn finish(mut self, timeout: Duration) -> Result<()> {
        if self.finished.recv_timeout(timeout).is_err() {
            self.cancel();
            if self.finished.recv_timeout(Duration::from_secs(1)).is_err() {
                bail!("test session event consumer did not stop after cancellation")
            }
        }
        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .map_err(|_| anyhow::anyhow!("test session event consumer panicked"))?;
        }
        Ok(())
    }
}

fn apply_test_session_events(
    session: Arc<Session>,
    receiver: ReportingReceiver<SessionEvent>,
) -> TestSessionEvents {
    let cancel = Arc::new(AtomicBool::new(false));
    let (finished_sender, finished) = mpsc::sync_channel(1);
    let consumer_cancel = Arc::clone(&cancel);
    let handle = thread::spawn(move || {
        while !consumer_cancel.load(Ordering::Acquire) {
            match receiver.recv_timeout(Duration::from_millis(10)) {
                Ok(event) => {
                    let exited = matches!(event, SessionEvent::Exited { .. });
                    session.apply_event(event);
                    if exited {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let _ = finished_sender.send(());
    });
    TestSessionEvents {
        cancel,
        finished,
        handle: Some(handle),
    }
}

fn saturated_control_state(
    session: &Arc<Session>,
    id: SessionId,
) -> (
    Arc<ServerState>,
    ReportingReceiver<DispatchMessage>,
    UnixStream,
) {
    let (server_stream, client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let identity = Arc::new(());
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink), Some((identity, server_stream)));
    register_test_session(&state, id, Arc::clone(session));
    for _ in 0..RAW_DISPATCH_QUEUE_CAPACITY {
        state.dispatch.try_send(DispatchMessage::Stop).unwrap();
    }
    (state, dispatch_receiver, client_stream)
}

fn cleanup_test_session(session: &Session, events: TestSessionEvents) -> Result<()> {
    let _ = session.set_paused(false);
    let termination = match session.terminate(Duration::from_secs(2)) {
        Ok(()) => Ok(()),
        Err(error) if error.is::<crate::session::AlreadyExited>() => Ok(()),
        Err(error) => Err(error),
    };

    if termination.is_err() {
        events.cancel();
    }
    let events_result = events.finish(Duration::from_secs(1));
    match (termination, events_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(termination), Ok(())) => Err(termination.context("terminate test session")),
        (Ok(()), Err(events)) => Err(events.context("finish test session event consumer")),
        (Err(termination), Err(events)) => Err(anyhow::anyhow!(
            "terminate test session: {}; finish test session event consumer: {}",
            error_chain_string(&termination),
            error_chain_string(&events),
        )),
    }
}

fn queued_dashboard_message(sink: &Arc<DashboardSink>) -> ServerMessage {
    match sink.next().unwrap() {
        DashboardDelivery::Message(outbound) => outbound.message,
        DashboardDelivery::Terminal(outbound) => {
            panic!(
                "unexpected terminal dashboard delivery: {:?}",
                outbound.message
            )
        }
        DashboardDelivery::Dirty { session, .. } => {
            panic!(
                "unexpected dirty dashboard delivery for session {}",
                session.0
            )
        }
    }
}

/// Put one Dashboard request through the connection entry point under the
/// same bound the other dispatcher fixtures use. The dispatcher answers view
/// requests on its own thread and `await_view_completion` waits without a
/// deadline, so a stalled dispatcher has to fail this test rather than hang
/// the suite.
fn dashboard_request_bounded(
    state: &Arc<ServerState>,
    owner: &Arc<()>,
    request_id: u64,
    request: Request,
) -> Response {
    let (response, result) = mpsc::sync_channel(1);
    let state = Arc::clone(state);
    let owner = Arc::clone(owner);
    thread::spawn(move || {
        let mut role = ClientRole::Dashboard;
        let _ = response.send(handle_request_with_id(
            &state,
            &mut role,
            request,
            request_id,
            Some(&owner),
        ));
    });
    result
        .recv_timeout(Duration::from_secs(2))
        .expect("dashboard request completion")
}

fn send_history_command(
    state: &Arc<ServerState>,
    owner: &Arc<()>,
    request_id: u64,
    request: HistoryRequest,
) {
    let (completion, result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::History {
            owner: Arc::clone(owner),
            request_id,
            request,
            completion,
        })
        .unwrap();
    result
        .recv_timeout(Duration::from_secs(2))
        .expect("history dispatcher completion");
}

#[test]
fn history_owner_and_token_isolation() {
    let id = SessionId(11);
    let (_cwd, session, receiver) = spawn_live_test_session(id);
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    let switched_id = SessionId(12);
    let (_switched_cwd, switched_session, switched_receiver) = spawn_live_test_session(switched_id);
    let switched_events =
        apply_test_session_events(Arc::clone(&switched_session), switched_receiver);
    let (old_server, _old_client) = UnixStream::pair().unwrap();
    let old_sink = DashboardSink::new();
    let old_owner = Arc::new(());
    let (state, dispatch_receiver) = test_state_with_dispatch(
        Some(old_sink.clone()),
        Some((old_owner.clone(), old_server)),
    );
    register_test_session(&state, id, Arc::clone(&session));
    register_test_session(&state, switched_id, Arc::clone(&switched_session));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));

    send_history_command(&state, &old_owner, 1, HistoryRequest::Begin { session: id });
    let old_opened = match queued_dashboard_message(&old_sink) {
        ServerMessage::Response {
            request_id: 1,
            response: Response::HistoryOpened(opened),
        } => opened,
        message => panic!("unexpected old begin response: {message:?}"),
    };
    state.dashboard.install_view_for_test(Some(DashboardView {
        revision: 1,
        panes: vec![PaneTarget {
            run: ovrcr_protocol::SessionRunId(1),
            session: id,
            size: TerminalSize { rows: 24, cols: 80 },
        }],
        focused: Some(id),
    }));
    assert!(matches!(
        dashboard_request_bounded(
            &state,
            &old_owner,
            8,
            Request::SetView {
                view: DashboardView {
                    revision: 2,
                    panes: vec![PaneTarget {
                        run: ovrcr_protocol::SessionRunId(1),
                        session: id,
                        size: TerminalSize { rows: 24, cols: 80 },
                    }],
                    focused: Some(id),
                },
            },
        ),
        Response::Ok
    ));
    assert!(matches!(
        queued_dashboard_message(&old_sink),
        ServerMessage::Response {
            request_id: 8,
            response: Response::Screen { session, .. },
        } if session == id
    ));
    assert!(matches!(
        queued_dashboard_message(&old_sink),
        ServerMessage::Response {
            request_id: 8,
            response: Response::Ok,
        }
    ));
    send_history_command(
        &state,
        &old_owner,
        9,
        HistoryRequest::Page {
            session: id,
            snapshot: old_opened.snapshot,
            start_row: 0,
            rows: 1,
            start_col: 0,
            cols: 1,
        },
    );
    assert!(matches!(
        queued_dashboard_message(&old_sink),
        ServerMessage::Response {
            request_id: 9,
            response: Response::HistoryRows(_),
        }
    ));
    assert!(matches!(
        dashboard_request_bounded(
            &state,
            &old_owner,
            10,
            Request::SetView {
                view: DashboardView {
                    revision: 3,
                    panes: vec![PaneTarget {
                        run: ovrcr_protocol::SessionRunId(1),
                        session: switched_id,
                        size: TerminalSize { rows: 24, cols: 80 },
                    }],
                    focused: Some(switched_id),
                },
            },
        ),
        Response::Ok
    ));
    assert!(matches!(
        queued_dashboard_message(&old_sink),
        ServerMessage::Response {
            request_id: 10,
            response: Response::Screen { session, .. },
        } if session == switched_id
    ));
    assert!(matches!(
        queued_dashboard_message(&old_sink),
        ServerMessage::Response {
            request_id: 10,
            response: Response::Ok,
        }
    ));
    send_history_command(
        &state,
        &old_owner,
        11,
        HistoryRequest::Page {
            session: id,
            snapshot: old_opened.snapshot,
            start_row: 0,
            rows: 1,
            start_col: 0,
            cols: 1,
        },
    );
    assert!(matches!(
        queued_dashboard_message(&old_sink),
        ServerMessage::Response {
            request_id: 11,
            response: Response::Error {
                code: ErrorCode::NotFound,
                ..
            },
        }
    ));

    let (new_server, _new_client) = UnixStream::pair().unwrap();
    let new_sink = DashboardSink::new();
    let new_owner = Arc::new(());
    state
        .dashboard
        .claim_with_identity(new_sink.clone(), new_owner.clone(), new_server);

    send_history_command(
        &state,
        &old_owner,
        2,
        HistoryRequest::End {
            session: id,
            snapshot: old_opened.snapshot,
        },
    );
    assert!(new_sink.queue.lock().unwrap().messages.is_empty());

    send_history_command(&state, &new_owner, 3, HistoryRequest::Begin { session: id });
    let new_opened = match queued_dashboard_message(&new_sink) {
        ServerMessage::Response {
            request_id: 3,
            response: Response::HistoryOpened(opened),
        } => opened,
        message => panic!("unexpected new begin response: {message:?}"),
    };
    send_history_command(&state, &new_owner, 4, HistoryRequest::Begin { session: id });
    let replacement = match queued_dashboard_message(&new_sink) {
        ServerMessage::Response {
            request_id: 4,
            response: Response::HistoryOpened(opened),
        } => opened,
        message => panic!("unexpected replacement begin response: {message:?}"),
    };
    assert!(replacement.snapshot.0 > new_opened.snapshot.0);

    send_history_command(
        &state,
        &new_owner,
        5,
        HistoryRequest::End {
            session: id,
            snapshot: new_opened.snapshot,
        },
    );
    assert!(matches!(
        queued_dashboard_message(&new_sink),
        ServerMessage::Response {
            request_id: 5,
            response: Response::Ok,
        }
    ));
    send_history_command(
        &state,
        &old_owner,
        6,
        HistoryRequest::End {
            session: id,
            snapshot: replacement.snapshot,
        },
    );
    assert!(new_sink.queue.lock().unwrap().messages.is_empty());
    send_history_command(
        &state,
        &new_owner,
        7,
        HistoryRequest::Page {
            session: id,
            snapshot: replacement.snapshot,
            start_row: 0,
            rows: 1,
            start_col: 0,
            cols: 1,
        },
    );
    assert!(matches!(
        queued_dashboard_message(&new_sink),
        ServerMessage::Response {
            request_id: 7,
            response: Response::HistoryRows(_),
        }
    ));
    send_history_command(
        &state,
        &new_owner,
        8,
        HistoryRequest::End {
            session: id,
            snapshot: replacement.snapshot,
        },
    );
    assert!(matches!(
        queued_dashboard_message(&new_sink),
        ServerMessage::Response {
            request_id: 8,
            response: Response::Ok,
        }
    ));

    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&session, events).unwrap();
    cleanup_test_session(&switched_session, switched_events).unwrap();

    let (original_state, original_receiver) = test_state_with_dispatch(None, None);
    let original_dispatcher = thread::spawn(move || {
        if let Ok(DispatchMessage::History { completion, .. }) = original_receiver.recv() {
            drop(completion);
        }
    });
    let original_handler = thread::spawn({
        let state = Arc::clone(&original_state);
        move || {
            let (server, mut client) = UnixStream::pair().unwrap();
            let handler_state = Arc::clone(&state);
            let handler = thread::spawn(move || handle_connection(handler_state, server));
            exchange_preamble(&mut client).unwrap();
            write_frame(
                &mut client,
                &ClientMessage {
                    request_id: 20,
                    request: Request::DashboardHello,
                },
            )
            .unwrap();
            assert!(matches!(
                read_frame::<ServerMessage>(&mut client).unwrap(),
                ServerMessage::Response {
                    request_id: 20,
                    response: Response::Hierarchy(_),
                }
            ));
            write_frame(
                &mut client,
                &ClientMessage {
                    request_id: 21,
                    request: Request::HistoryBegin { session: id },
                },
            )
            .unwrap();
            assert!(matches!(
                read_frame::<ServerMessage>(&mut client).unwrap(),
                ServerMessage::Response {
                    request_id: 21,
                    response: Response::Error {
                        code: ErrorCode::Internal,
                        ..
                    },
                }
            ));
            let _ = client.shutdown(Shutdown::Both);
            handler.join().unwrap();
        }
    });
    original_handler.join().unwrap();
    original_dispatcher.join().unwrap();

    let (replacement_state, replacement_receiver) = test_state_with_dispatch(None, None);
    let (replacement_server, mut replacement_client) = UnixStream::pair().unwrap();
    let replacement_handler_state = Arc::clone(&replacement_state);
    let replacement_handler =
        thread::spawn(move || handle_connection(replacement_handler_state, replacement_server));
    exchange_preamble(&mut replacement_client).unwrap();
    write_frame(
        &mut replacement_client,
        &ClientMessage {
            request_id: 30,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut replacement_client).unwrap(),
        ServerMessage::Response {
            request_id: 30,
            response: Response::Hierarchy(_),
        }
    ));
    let old_identity = replacement_state
        .dashboard
        .slot_for_test()
        .as_ref()
        .unwrap()
        .identity
        .clone();
    let (new_server, _new_client) = UnixStream::pair().unwrap();
    let replacement_sink = DashboardSink::new();
    let replacement_owner = Arc::new(());
    replacement_state.dashboard.claim_with_identity(
        replacement_sink.clone(),
        replacement_owner,
        new_server,
    );
    drop(replacement_receiver);
    write_frame(
        &mut replacement_client,
        &ClientMessage {
            request_id: 31,
            request: Request::HistoryBegin { session: id },
        },
    )
    .unwrap();
    replacement_handler.join().unwrap();
    assert!(replacement_sink.queue.lock().unwrap().messages.is_empty());
    assert!(
        replacement_state
            .dashboard
            .slot_for_test()
            .as_ref()
            .is_some_and(|slot| !Arc::ptr_eq(&slot.identity, &old_identity))
    );
}

#[test]
fn history_page_overflow_disconnects_without_parser_wait() {
    let race_id = SessionId(14);
    let (_race_cwd, race_session, race_receiver) = spawn_live_test_session(race_id);
    let race_events = apply_test_session_events(Arc::clone(&race_session), race_receiver);
    let (race_server, _race_client) = UnixStream::pair().unwrap();
    let race_sink = DashboardSink::new();
    let race_owner = Arc::new(());
    let (race_state, race_dispatch_receiver) = test_state_with_dispatch(
        Some(race_sink.clone()),
        Some((race_owner.clone(), race_server)),
    );
    register_test_session(&race_state, race_id, Arc::clone(&race_session));
    let (capture_entered, capture_entered_result) = mpsc::sync_channel(1);
    let (capture_release, capture_release_result) = mpsc::sync_channel(1);
    let capture_release_result = Arc::new(Mutex::new(capture_release_result));
    let capture_release_waiter = Arc::clone(&capture_release_result);
    race_session.set_history_capture_hook(Some(Arc::new(move || {
        capture_entered.send(()).unwrap();
        capture_release_waiter.lock().unwrap().recv().unwrap();
    })));
    let race_dispatcher_state = Arc::clone(&race_state);
    let race_dispatcher =
        thread::spawn(move || run_dispatcher(race_dispatcher_state, race_dispatch_receiver));
    let (race_completion, race_completion_result) = mpsc::sync_channel(1);
    race_state
        .dispatch
        .send(DispatchMessage::History {
            owner: race_owner.clone(),
            request_id: 100,
            request: HistoryRequest::Begin { session: race_id },
            completion: race_completion,
        })
        .unwrap();
    capture_entered_result
        .recv_timeout(Duration::from_secs(2))
        .expect("capture pause entered");
    let (parser_probe_done, parser_probe_result) = mpsc::sync_channel(1);
    let parser_probe_session = Arc::clone(&race_session);
    let parser_probe = thread::spawn(move || {
        parser_probe_session.current_screen();
        parser_probe_done.send(()).unwrap();
    });
    parser_probe_result
        .recv_timeout(Duration::from_secs(2))
        .expect("capture releases parser lock before install");
    parser_probe.join().unwrap();
    let (removed, removed_result) = mpsc::sync_channel(1);
    let removal_state = Arc::clone(&race_state);
    let removal_session = Arc::clone(&race_session);
    let removal = thread::spawn(move || {
        removal_session.terminate(Duration::from_secs(2)).unwrap();
        removal_state
            .acknowledge_session_stopped(race_id, removal_session.run())
            .unwrap();
        removal_state.remove_session(race_id).unwrap();
        removed.send(()).unwrap();
    });
    removed_result
        .recv_timeout(Duration::from_secs(3))
        .expect("session removed before capture resumes");
    capture_release.send(()).unwrap();
    race_completion_result
        .recv_timeout(Duration::from_secs(2))
        .expect("capture completion after removal");
    assert!(matches!(
        queued_dashboard_message(&race_sink),
        ServerMessage::Response {
            request_id: 100,
            response: Response::Error {
                code: ErrorCode::NotFound,
                ..
            },
        }
    ));
    assert!(
        race_state
            .dashboard
            .slot_for_test()
            .as_ref()
            .unwrap()
            .history
            .is_none()
    );
    race_state.dispatch.send(DispatchMessage::Stop).unwrap();
    race_dispatcher.join().unwrap();
    removal.join().unwrap();
    race_events.finish(Duration::from_secs(1)).unwrap();

    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let owner = Arc::new(());
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    let mut parser = ovrcr_terminal::vt100::Parser::new(24, 80, HISTORY_ROWS);
    parser.process(b"A");
    let history = FrozenHistory::capture(
        SessionId(12),
        HistorySnapshotId(1),
        1,
        parser.screen().clone(),
    )
    .unwrap();
    state.dashboard.slot_for_test().as_mut().unwrap().history = Some(history);
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));

    for (request_id, rows, cols, start_row, start_col) in [
        (1, 0, 1, 0, 0),
        (2, 1, 0, 0, 0),
        (3, PAGE_ROWS + 1, 1, 0, 0),
        (4, 1, PAGE_COLS + 1, 0, 0),
        (5, 1, 1, 26, 0),
        (6, 1, 1, u32::MAX, 0),
        (7, 1, 1, 0, u16::MAX),
    ] {
        send_history_command(
            &state,
            &owner,
            request_id,
            HistoryRequest::Page {
                session: SessionId(12),
                snapshot: HistorySnapshotId(1),
                start_row,
                rows,
                start_col,
                cols,
            },
        );
        assert!(matches!(
            queued_dashboard_message(&sink),
            ServerMessage::Response {
                request_id: id,
                response: Response::Error {
                    code: ErrorCode::InvalidRequest,
                    ..
                },
            } if id == request_id
        ));
    }
    let (_page_cwd, page_session, page_receiver) = spawn_live_test_session(SessionId(12));
    let page_events = apply_test_session_events(Arc::clone(&page_session), page_receiver);
    register_test_session(&state, SessionId(12), Arc::clone(&page_session));
    let (parser_holder_acquired_sender, parser_holder_acquired) = mpsc::sync_channel(1);
    let (parser_holder_release, parser_holder_release_result) = mpsc::sync_channel(1);
    let parser_holder_session = Arc::clone(&page_session);
    let parser_holder = thread::spawn(move || {
        parser_holder_session.with_terminal_lock_for_test(|| {
            parser_holder_acquired_sender.send(()).unwrap();
            parser_holder_release_result.recv().unwrap();
        });
    });
    let parser_was_held = parser_holder_acquired
        .recv_timeout(Duration::from_secs(2))
        .is_ok();
    let mut queue_filled = true;
    if parser_was_held {
        for _ in 0..DASHBOARD_QUEUE {
            if sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
                    projects: Vec::new(),
                })),
                completion: None,
            }) != Enqueue::Queued
            {
                queue_filled = false;
                break;
            }
        }
    }
    let (completion, completion_result) = mpsc::sync_channel(1);
    let page_sent = state
        .dispatch
        .send(DispatchMessage::History {
            owner: owner.clone(),
            request_id: 8,
            request: HistoryRequest::Page {
                session: SessionId(12),
                snapshot: HistorySnapshotId(1),
                start_row: 0,
                rows: 1,
                start_col: 0,
                cols: 1,
            },
            completion,
        })
        .is_ok();
    let completion_before_release = completion_result
        .recv_timeout(Duration::from_secs(2))
        .is_ok();
    let disconnected_before_release = completion_before_release && !state.dashboard.is_claimed();
    let _ = parser_holder_release.send(());
    let parser_holder_joined = parser_holder.join().is_ok();
    let cleanup_result = cleanup_test_session(&page_session, page_events);
    let acknowledged = state
        .acknowledge_session_stopped(SessionId(12), page_session.run())
        .is_ok();
    let removed = state.remove_session(SessionId(12)).is_ok();
    let dispatcher_stopped = state.dispatch.send(DispatchMessage::Stop).is_ok();
    let dispatcher_joined = dispatcher.join().is_ok();
    assert!(parser_was_held);
    assert!(queue_filled);
    assert!(page_sent);
    assert!(completion_before_release);
    assert!(disconnected_before_release);
    assert!(parser_holder_joined);
    assert!(dispatcher_stopped);
    assert!(dispatcher_joined);
    assert!(cleanup_result.is_ok());
    assert!(acknowledged);
    assert!(removed);
}

#[test]
fn history_capture_orders_with_output() {
    let id = SessionId(13);
    let (_cwd, session, receiver) = spawn_live_test_session(id);
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let owner = Arc::new(());
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink.clone()), Some((owner.clone(), server_stream)));
    register_test_session(&state, id, Arc::clone(&session));
    state.dashboard.install_view_for_test(Some(DashboardView {
        revision: 1,
        panes: vec![PaneTarget {
            run: ovrcr_protocol::SessionRunId(1),
            session: id,
            size: TerminalSize { rows: 24, cols: 80 },
        }],
        focused: Some(id),
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));

    state
        .dispatch
        .send(DispatchMessage::Session(SessionEvent::Output {
            run: ovrcr_protocol::SessionRunId(1),
            id,
            bytes: b"A".to_vec(),
        }))
        .unwrap();
    send_history_command(&state, &owner, 2, HistoryRequest::Begin { session: id });
    state
        .dispatch
        .send(DispatchMessage::Session(SessionEvent::Output {
            run: ovrcr_protocol::SessionRunId(1),
            id,
            bytes: b"B".to_vec(),
        }))
        .unwrap();
    send_history_command(
        &state,
        &owner,
        4,
        HistoryRequest::Page {
            session: id,
            snapshot: HistorySnapshotId(1),
            start_row: 0,
            rows: 1,
            start_col: 0,
            cols: PAGE_COLS,
        },
    );

    let messages = (0..4)
        .map(|_| queued_dashboard_message(&sink))
        .collect::<Vec<_>>();
    assert!(matches!(
        &messages[0],
        ServerMessage::Event(ServerEvent::Output {
            run: ovrcr_protocol::SessionRunId(1),
            session,
            revision: _,
            bytes,
        })
            if *session == id && bytes == b"A"
    ));
    assert!(matches!(
        &messages[1],
        ServerMessage::Response {
            request_id: 2,
            response: Response::HistoryOpened(_),
        }
    ));
    assert!(matches!(
        &messages[2],
        ServerMessage::Event(ServerEvent::Output {
            run: ovrcr_protocol::SessionRunId(1),
            session,
            revision: _,
            bytes,
        })
            if *session == id && bytes == b"B"
    ));
    let page = match &messages[3] {
        ServerMessage::Response {
            request_id: 4,
            response: Response::HistoryRows(page),
        } => page,
        message => panic!("unexpected history page response: {message:?}"),
    };
    let page_text = page.rows[0]
        .cells
        .iter()
        .map(|cell| cell.text.as_str())
        .collect::<String>();
    assert!(page_text.contains('A'));
    assert!(!page_text.contains('B'));
    let live = session.current_screen();
    assert!(live.contains(&b'A'));
    assert!(live.contains(&b'B'));

    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&session, events).unwrap();
}

#[test]
fn dashboard_overflow_closes_affected_connection() {
    let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
    client_stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    let sink = DashboardSink::new();
    for _ in 0..DASHBOARD_QUEUE {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
                    projects: Vec::new()
                },)),
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    let identity = Arc::new(());
    let state = test_state(Some(sink), Some((identity, server_stream)));
    state
        .dashboard
        .try_send(ServerMessage::Event(ServerEvent::ScreenDirty {
            run: ovrcr_protocol::SessionRunId(1),
            session: SessionId(2),
            revision: 0,
        }));
    let mut byte = [0_u8; 1];
    assert_eq!(client_stream.read(&mut byte).unwrap(), 0);
    assert!(!state.dashboard.is_claimed());
}

#[test]
fn pending_output_does_not_evict_dashboard_on_lifecycle_event() {
    // A burst can leave the queue full of output frames before the writer
    // thread runs. A lifecycle event arriving then must fold the output
    // into a dirty marker rather than evict a dashboard that is keeping up.
    let sink = DashboardSink::new();
    for _ in 0..DASHBOARD_QUEUE {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::Output {
                    run: ovrcr_protocol::SessionRunId(1),
                    session: SessionId(2),
                    revision: 7,
                    bytes: b"x".to_vec(),
                }),
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    assert_eq!(
        sink.enqueue(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
                projects: Vec::new()
            })),
            completion: None,
        }),
        Enqueue::Queued
    );
    assert!(matches!(
        queued_dashboard_message(&sink),
        ServerMessage::Event(ServerEvent::HierarchyChanged(_))
    ));
    assert!(matches!(
        sink.next(),
        Some(DashboardDelivery::Dirty {
            run: ovrcr_protocol::SessionRunId(1),
            revision: 7,
            session: SessionId(2)
        })
    ));
    // Messages that cannot be coalesced still evict once they fill the queue.
    for _ in 0..DASHBOARD_QUEUE {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
                    projects: Vec::new()
                })),
                completion: None,
            }),
            Enqueue::Queued
        );
    }
    assert!(matches!(
        sink.enqueue(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
                projects: Vec::new()
            })),
            completion: None,
        }),
        Enqueue::Closed
    ));
}

#[test]
fn dashboard_overflow_does_not_clear_replacement_slot() {
    let (old_server, mut old_client) = UnixStream::pair().unwrap();
    let _old_handler_stream = old_server.try_clone().unwrap();
    let _old_writer_stream = old_server.try_clone().unwrap();
    old_client
        .set_read_timeout(Some(Duration::from_millis(250)))
        .unwrap();
    let old_sink = DashboardSink::new();
    let old_identity = Arc::new(());
    let state = test_state(Some(old_sink), Some((old_identity, old_server)));
    let old_snapshot = state.dashboard.snapshot().unwrap();

    let (new_server, _new_client) = UnixStream::pair().unwrap();
    let new_sink = DashboardSink::new();
    let new_identity = Arc::new(());
    state
        .dashboard
        .claim_with_identity(new_sink, Arc::clone(&new_identity), new_server);
    state.dashboard.disconnect(old_snapshot);

    assert!(state.dashboard.owns(&new_identity));
    assert!(matches!(old_client.read(&mut [0_u8; 1]), Ok(0)));
}

/// The connection guard and the writer's disconnect are the same teardown:
/// dropping ownership must leave the slot, the view, and the geometry exactly
/// as [`ActiveDashboard::disconnect`] does.
#[test]
fn dropped_ownership_and_writer_disconnect_leave_identical_state() {
    let (client, server) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let state = test_state(None, None);
    let owner = state.dashboard.claim(Arc::clone(&sink), server).unwrap();
    state.dashboard.install_view_for_test(Some(DashboardView {
        revision: 1,
        panes: Vec::new(),
        focused: None,
    }));
    assert!(
        state
            .dashboard
            .set_geometry(&owner, TerminalSize { rows: 24, cols: 80 })
    );
    drop(DashboardOwnership {
        state: Arc::clone(&state),
        identity: Arc::clone(&owner),
    });
    assert!(!state.dashboard.is_claimed());
    assert!(state.dashboard.view().is_none());
    assert!(state.dashboard.geometry().is_none());
    assert!(!state.dashboard.owns(&owner));
    drop(client);
}

#[test]
fn late_response_does_not_reach_a_replacement_dashboard() {
    let (state, dispatch_receiver) = test_state_with_dispatch(None, None);
    let (first_server, mut first_client) = UnixStream::pair().unwrap();
    let first_state = Arc::clone(&state);
    let first_handler = thread::spawn(move || handle_connection(first_state, first_server));
    exchange_preamble(&mut first_client).unwrap();
    write_frame(
        &mut first_client,
        &ClientMessage {
            request_id: 90,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut first_client).unwrap(),
        ServerMessage::Response {
            request_id: 90,
            response: Response::Hierarchy(_),
        }
    ));
    let first_snapshot = state.dashboard.snapshot().expect("first dashboard slot");
    // The report handler blocks until the dispatcher answers, and this test is
    // the dispatcher, so the response cannot be written before the slot changes.
    write_frame(
        &mut first_client,
        &ClientMessage {
            request_id: 91,
            request: Request::AgentReport(AgentReport {
                session: SessionId(64),
                capability: [0_u8; 32],
                sequence: None,
                update: ovrcr_protocol::AgentUpdate::Activity(AgentActivity::Busy),
            }),
        },
    )
    .unwrap();
    let queued = dispatch_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("agent report reached the dispatcher");
    let DispatchMessage::AgentReport {
        completion: report_completion,
        ..
    } = queued
    else {
        panic!("expected an agent report dispatch");
    };
    state.dashboard.disconnect(first_snapshot);
    let (second_server, mut second_client) = UnixStream::pair().unwrap();
    let second_state = Arc::clone(&state);
    let second_handler = thread::spawn(move || handle_connection(second_state, second_server));
    exchange_preamble(&mut second_client).unwrap();
    write_frame(
        &mut second_client,
        &ClientMessage {
            request_id: 92,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut second_client).unwrap(),
        ServerMessage::Response {
            request_id: 92,
            response: Response::Hierarchy(_),
        }
    ));
    report_completion.send(Response::Ok).unwrap();
    assert!(join_test_thread_bounded(
        first_handler,
        Duration::from_secs(5)
    ));
    write_frame(
        &mut second_client,
        &ClientMessage {
            request_id: 93,
            request: Request::List,
        },
    )
    .unwrap();
    second_client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    // The evicted dashboard's response must not arrive here, so the next frame
    // is the replacement's own answer.
    assert!(matches!(
        read_frame::<ServerMessage>(&mut second_client).unwrap(),
        ServerMessage::Response {
            request_id: 93,
            response: Response::Hierarchy(_),
        }
    ));
    let _ = second_client.shutdown(Shutdown::Both);
    assert!(join_test_thread_bounded(
        second_handler,
        Duration::from_secs(5)
    ));
}

#[test]
fn stale_geometry_does_not_overwrite_replacement_size() {
    let (state, _dispatch_receiver) = test_state_with_dispatch(None, None);
    let (first_server, mut first_client) = UnixStream::pair().unwrap();
    let first_state = Arc::clone(&state);
    let first_handler = thread::spawn(move || handle_connection(first_state, first_server));
    exchange_preamble(&mut first_client).unwrap();
    write_frame(
        &mut first_client,
        &ClientMessage {
            request_id: 95,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut first_client).unwrap(),
        ServerMessage::Response {
            request_id: 95,
            response: Response::Hierarchy(_),
        }
    ));
    // Hold the mutation lock so the geometry request cannot be applied until
    // the slot already belongs to the replacement.
    let mutation = state.mutation_lock.lock().unwrap();
    write_frame(
        &mut first_client,
        &ClientMessage {
            request_id: 96,
            request: Request::DashboardGeometry {
                size: TerminalSize { rows: 11, cols: 41 },
            },
        },
    )
    .unwrap();
    // Half-close so the handler parses the queued request and then reads EOF.
    first_client.shutdown(Shutdown::Write).unwrap();
    // Release the slot the way a cleanup on another thread would, but without
    // closing the socket: the already parsed request must still be answered by
    // the real handler after the replacement takes the slot.
    let evicted = state
        .dashboard
        .slot_for_test()
        .take()
        .expect("first dashboard slot");
    state.dashboard.install_view_for_test(None);
    drop(evicted);
    let (second_server, mut second_client) = UnixStream::pair().unwrap();
    let second_state = Arc::clone(&state);
    let second_handler = thread::spawn(move || handle_connection(second_state, second_server));
    exchange_preamble(&mut second_client).unwrap();
    write_frame(
        &mut second_client,
        &ClientMessage {
            request_id: 97,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut second_client).unwrap(),
        ServerMessage::Response {
            request_id: 97,
            response: Response::Hierarchy(_),
        }
    ));
    let second_identity = state
        .dashboard
        .slot_for_test()
        .as_ref()
        .expect("replacement dashboard slot")
        .identity
        .clone();
    state
        .dashboard
        .set_geometry(&second_identity, TerminalSize { rows: 27, cols: 83 });
    drop(mutation);
    assert!(join_test_thread_bounded(
        first_handler,
        Duration::from_secs(5)
    ));
    assert_eq!(
        state.dashboard.geometry(),
        Some(TerminalSize { rows: 27, cols: 83 })
    );
    assert!(
        state
            .dashboard
            .geometry_owner_for_test()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &second_identity)),
        "geometry belongs to the replacement dashboard"
    );
    write_frame(
        &mut second_client,
        &ClientMessage {
            request_id: 98,
            request: Request::DashboardGeometry {
                size: TerminalSize { rows: 19, cols: 71 },
            },
        },
    )
    .unwrap();
    second_client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut second_client).unwrap(),
        ServerMessage::Response {
            request_id: 98,
            response: Response::Ok,
        }
    ));
    assert_eq!(
        state.dashboard.geometry(),
        Some(TerminalSize { rows: 19, cols: 71 })
    );
    let _ = second_client.shutdown(Shutdown::Both);
    assert!(join_test_thread_bounded(
        second_handler,
        Duration::from_secs(5)
    ));
}

#[test]
fn session_refresh_dispatch_failures_disconnect_without_blocking() {
    let id = SessionId(3);
    let (_cwd, session, receiver) = spawn_live_test_session(id);
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    let (state, dispatch_receiver, mut client_stream) = saturated_control_state(&session, id);
    let mut dispatch_receiver = Some(dispatch_receiver);
    client_stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();

    let (completion, result) = mpsc::sync_channel(1);
    let control_state = Arc::clone(&state);
    let worker = thread::spawn(move || {
        let outcome = control_state.set_session_paused(id, true);
        let _ = completion.send(outcome);
    });
    let outcome = match result.recv_timeout(Duration::from_millis(250)) {
        Ok(outcome) => outcome,
        Err(error) => {
            drop(dispatch_receiver.take());
            let _ = result.recv_timeout(Duration::from_secs(1));
            let _ = worker.join();
            match cleanup_test_session(&session, events) {
                Ok(()) => {
                    panic!("pause control must not block on a full dispatch queue: {error}")
                }
                Err(cleanup_error) => panic!(
                    "pause control must not block on a full dispatch queue: {error}; \
                         cleanup failed: {cleanup_error:#}"
                ),
            }
        }
    };
    worker.join().unwrap();
    let error = outcome.unwrap_err();
    let partial_failure = error
        .downcast_ref::<LifecycleFailure>()
        .is_some_and(|failure| failure.code == ErrorCode::PartialFailure);
    let error_message = error_chain_string(&error);
    let pause_preserved = matches!(session.summary().phase, SessionPhase::Paused);
    let dashboard_closed = matches!(client_stream.read(&mut [0_u8; 1]), Ok(0));
    drop(dispatch_receiver.take());
    cleanup_test_session(&session, events).unwrap();
    assert!(partial_failure);
    assert!(error_message.contains("dispatcher queue is full"));
    assert!(pause_preserved);
    assert!(dashboard_closed);

    let id = SessionId(4);
    let (_cwd, session, receiver) = spawn_live_test_session(id);
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
    client_stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let sink = DashboardSink::new();
    let identity = Arc::new(());
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink), Some((identity, server_stream)));
    register_test_session(&state, id, Arc::clone(&session));
    drop(dispatch_receiver);
    let error = state.set_session_paused(id, true).unwrap_err();
    let pause_preserved = matches!(session.summary().phase, SessionPhase::Paused);
    let dashboard_closed = matches!(client_stream.read(&mut [0_u8; 1]), Ok(0));
    cleanup_test_session(&session, events).unwrap();
    assert!(error.to_string().contains("dispatcher is unavailable"));
    assert!(pause_preserved);
    assert!(dashboard_closed);
}

#[test]
fn kill_and_close_refresh_failures_keep_exited_records() {
    for (id, close) in [(SessionId(5), false), (SessionId(6), true)] {
        let (_cwd, session, receiver) = spawn_live_test_session(id);
        let events = apply_test_session_events(Arc::clone(&session), receiver);
        let (state, dispatch_receiver, mut client_stream) = saturated_control_state(&session, id);
        client_stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let error = if close {
            state.close_terminal(id, session.run(), Duration::from_millis(250))
        } else {
            state.kill_session(id, Duration::from_millis(250))
        }
        .unwrap_err();
        let lifecycle = error.downcast_ref::<LifecycleFailure>().unwrap();
        assert_eq!(lifecycle.code, ErrorCode::PartialFailure);
        assert!(error.to_string().contains("dispatcher queue is full"));
        assert!(matches!(
            session.summary().phase,
            SessionPhase::Exited { .. }
        ));
        assert!(state.sessions.lock().unwrap().contains_key(&id));
        assert_eq!(client_stream.read(&mut [0_u8; 1]).unwrap(), 0);
        drop(dispatch_receiver);
        state.remove_session(id).unwrap();
        cleanup_test_session(&session, events).unwrap();
        assert!(!state.sessions.lock().unwrap().contains_key(&id));
    }
}

#[test]
fn shutdown_refresh_failure_keeps_exited_record_and_server_available() {
    let id = SessionId(7);
    let (_cwd, session, receiver) = spawn_live_test_session(id);
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    let (state, dispatch_receiver, mut client_stream) = saturated_control_state(&session, id);
    client_stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let response = state.request_shutdown(true);
    assert!(matches!(
        response,
        Response::Error {
            code: ErrorCode::PartialFailure,
            ..
        }
    ));
    assert!(!state.stopping.load(Ordering::Acquire));
    assert!(matches!(
        session.summary().phase,
        SessionPhase::Exited { .. }
    ));
    assert!(state.sessions.lock().unwrap().contains_key(&id));
    assert_eq!(client_stream.read(&mut [0_u8; 1]).unwrap(), 0);
    drop(dispatch_receiver);
    state.remove_session(id).unwrap();
    cleanup_test_session(&session, events).unwrap();
    assert_eq!(state.request_shutdown(false), Response::Ok);
}

#[test]
fn control_and_refresh_failures_preserve_both_causes() {
    let id = SessionId(8);
    let (_cwd, session, receiver) = {
        let cwd = tempfile::tempdir().unwrap();
        let (events, receiver) = event_channel(None);
        let session = Session::spawn_registered(
            id,
            SessionSpec {
                run: ovrcr_protocol::SessionRunId(1),
                kind: ovrcr_protocol::SessionKind::Terminal,
                project: "p".into(),
                workspace: "w".into(),
                name: "exited".into(),
                label: "sh".into(),
                cwd: cwd.path().to_path_buf(),
                argv: vec!["sh".into(), "-c".into(), "exit 0".into()],
                hook_env: None,
            },
            TerminalSize { rows: 24, cols: 80 },
            events,
            NO_REGISTER,
        )
        .unwrap();
        (cwd, session, receiver)
    };
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    session.wait_until_exited(Duration::from_secs(2)).unwrap();
    cleanup_test_session(&session, events).unwrap();
    let state = test_state(None, None);
    register_test_session(&state, id, Arc::clone(&session));
    let error = state.set_session_paused(id, true).unwrap_err();
    let lifecycle = error.downcast_ref::<LifecycleFailure>().unwrap();
    assert_eq!(lifecycle.code, ErrorCode::Conflict);
    let message = error_chain_string(&error);
    assert!(message.contains("session has exited"));
    assert!(message.contains("dispatcher is unavailable"));
}

#[test]
fn dashboard_shutdown_completes_when_response_sink_is_closed() {
    let state = test_state(None, None);
    let (server, mut client) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let handler_state = Arc::clone(&state);
    let (done, finished) = mpsc::sync_channel(1);
    let handler = thread::spawn(move || {
        handle_connection(handler_state, server);
        let _ = done.send(());
    });
    exchange_preamble(&mut client).unwrap();
    write_frame(
        &mut client,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut client).unwrap(),
        ServerMessage::Response {
            request_id: 1,
            response: Response::Hierarchy(_)
        }
    ));
    // Force response delivery to fail, independently of writer scheduling.
    state
        .dashboard
        .slot_for_test()
        .as_ref()
        .unwrap()
        .sink
        .close();
    write_frame(
        &mut client,
        &ClientMessage {
            request_id: 2,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    finished
        .recv_timeout(Duration::from_secs(2))
        .expect("shutdown handler must finish");
    handler.join().unwrap();
    assert!(
        state.stopping.load(Ordering::Acquire),
        "shutdown was accepted"
    );
    assert!(
        state.shutdown.load(Ordering::Acquire),
        "accepted shutdown must finish even without its response"
    );
    assert!(!state.dashboard.is_claimed());
}

#[test]
fn dashboard_shutdown_waits_for_stalled_writer_completion() {
    let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
    let send_buffer = 1_i32;
    let result = unsafe {
        libc::setsockopt(
            server_stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&send_buffer as *const i32).cast(),
            std::mem::size_of_val(&send_buffer) as libc::socklen_t,
        )
    };
    assert_eq!(result, 0);
    let state = test_state(None, None);
    state.registry.lock().unwrap().projects = (0..256)
        .map(|index| crate::config::ProjectRecord {
            name: format!("project-{index}-{}", "x".repeat(2_000)),
            repo: PathBuf::from(format!("/repo/{index}")),
            workspace_root: PathBuf::from(format!("/workspace/{index}")),
            workspaces: Vec::new(),
        })
        .collect();
    let handler_state = Arc::clone(&state);
    let handler = thread::spawn(move || handle_connection(handler_state, server_stream));
    exchange_preamble(&mut client_stream).unwrap();
    write_frame(
        &mut client_stream,
        &ClientMessage {
            request_id: 0,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    write_frame(
        &mut client_stream,
        &ClientMessage {
            request_id: 1,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    thread::park_timeout(Duration::from_millis(100));
    let shutdown_before_drain = state.shutdown.load(Ordering::Acquire);

    client_stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let _ = read_frame::<ServerMessage>(&mut client_stream).unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut client_stream).unwrap(),
        ServerMessage::Response {
            request_id: 1,
            response: Response::Ok,
        }
    ));
    handler.join().unwrap();
    assert!(
        !shutdown_before_drain,
        "shutdown must wait for the writer's actual response attempt"
    );
    assert!(state.shutdown.load(Ordering::Acquire));
}

#[test]
fn accepted_shutdown_rejects_late_mutation_while_ack_writer_is_blocked() {
    let (dashboard_server, mut dashboard_client) = UnixStream::pair().unwrap();
    let send_buffer = 1_i32;
    let result = unsafe {
        libc::setsockopt(
            dashboard_server.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_SNDBUF,
            (&send_buffer as *const i32).cast(),
            std::mem::size_of_val(&send_buffer) as libc::socklen_t,
        )
    };
    assert_eq!(result, 0);
    let state = test_state(None, None);
    state.registry.lock().unwrap().projects = (0..256)
        .map(|index| crate::config::ProjectRecord {
            name: format!("project-{index}-{}", "x".repeat(2_000)),
            repo: PathBuf::from(format!("/repo/{index}")),
            workspace_root: PathBuf::from(format!("/workspace/{index}")),
            workspaces: Vec::new(),
        })
        .collect();
    state
        .registry
        .lock()
        .unwrap()
        .projects
        .push(crate::config::ProjectRecord {
            name: "late".into(),
            repo: PathBuf::from("/repo/late"),
            workspace_root: PathBuf::from("/workspace/late"),
            workspaces: Vec::new(),
        });
    let dashboard_state = Arc::clone(&state);
    let dashboard_handler =
        thread::spawn(move || handle_connection(dashboard_state, dashboard_server));
    exchange_preamble(&mut dashboard_client).unwrap();
    let (control_server, mut control_client) = UnixStream::pair().unwrap();
    let control_state = Arc::clone(&state);
    let control_handler = thread::spawn(move || handle_connection(control_state, control_server));
    exchange_preamble(&mut control_client).unwrap();
    write_frame(
        &mut dashboard_client,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    write_frame(
        &mut dashboard_client,
        &ClientMessage {
            request_id: 2,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !state.stopping.load(Ordering::Acquire) {
        assert!(
            Instant::now() < deadline,
            "shutdown did not enter stopping state"
        );
        thread::yield_now();
    }
    control_client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write_frame(
        &mut control_client,
        &ClientMessage {
            request_id: 3,
            request: Request::RemoveProject {
                name: "late".into(),
            },
        },
    )
    .unwrap();
    assert_eq!(
        read_frame::<ServerMessage>(&mut control_client).unwrap(),
        ServerMessage::Response {
            request_id: 3,
            response: Response::Error {
                code: ErrorCode::Conflict,
                message: "server is stopping".into(),
            },
        }
    );
    let _ = control_client.shutdown(Shutdown::Both);
    control_handler.join().unwrap();
    for (request_id, request) in [
        (
            4,
            Request::PauseSession {
                session: SessionId(99),
            },
        ),
        (
            5,
            Request::ResumeSession {
                session: SessionId(99),
            },
        ),
    ] {
        let (control_server, mut control_client) = UnixStream::pair().unwrap();
        control_client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let control_state = Arc::clone(&state);
        let control_handler =
            thread::spawn(move || handle_connection(control_state, control_server));
        exchange_preamble(&mut control_client).unwrap();
        write_frame(
            &mut control_client,
            &ClientMessage {
                request_id,
                request,
            },
        )
        .unwrap();
        assert_eq!(
            read_frame::<ServerMessage>(&mut control_client).unwrap(),
            ServerMessage::Response {
                request_id,
                response: Response::Error {
                    code: ErrorCode::Conflict,
                    message: "server is stopping".into(),
                },
            }
        );
        let _ = control_client.shutdown(Shutdown::Both);
        control_handler.join().unwrap();
    }
    dashboard_client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    loop {
        if matches!(
            read_frame::<ServerMessage>(&mut dashboard_client).unwrap(),
            ServerMessage::Response {
                request_id: 2,
                response: Response::Ok,
            }
        ) {
            break;
        }
    }
    dashboard_handler.join().unwrap();
    assert!(state.shutdown.load(Ordering::Acquire));
}

#[test]
fn uncommitted_partial_failure_does_not_publish_hierarchy() {
    let sink = DashboardSink::new();
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let state = test_state(Some(sink.clone()), Some((Arc::new(()), server_stream)));
    let response = lifecycle_response_with_partial_hierarchy(
        &state,
        lifecycle_error(
            ErrorCode::PartialFailure,
            "registry write failed: worktree remains",
        ),
    );
    assert!(matches!(
        response,
        Response::Error {
            code: ErrorCode::PartialFailure,
            ..
        }
    ));
    assert!(
        sink.queue.lock().unwrap().messages.is_empty(),
        "uncommitted partial failure must not publish unchanged hierarchy"
    );
}

#[test]
fn shutdown_termination_failure_is_partial_and_server_remains_available() {
    let cwd = tempfile::tempdir().unwrap();
    let (events, receiver) = event_channel(None);
    let capability = [0x49; 32];
    let session = Session::spawn_registered(
        SessionId(7),
        crate::session::SessionSpec {
            kind: ovrcr_protocol::SessionKind::Terminal,
            run: ovrcr_protocol::SessionRunId(1),
            project: "p".into(),
            workspace: "w".into(),
            name: "fault".into(),
            label: "sh".into(),
            cwd: cwd.path().to_path_buf(),
            argv: vec!["sh".into()],
            hook_env: Some(HookEnvironment {
                socket: PathBuf::from("/private/test/ovrcr.sock"),
                session: SessionId(7),
                capability,
            }),
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
        NO_REGISTER,
    )
    .unwrap();
    let dispatch_session = Arc::clone(&session);
    let waiter = thread::spawn(move || {
        while let Ok(event) = receiver.recv() {
            let exited = matches!(event, SessionEvent::Exited { .. });
            dispatch_session.apply_event(event);
            if exited {
                break;
            }
        }
    });
    let state = test_state(None, None);
    register_test_session(&state, SessionId(7), Arc::clone(&session));
    let response = handle_shutdown(&state, true, |_| {
        Err(anyhow::anyhow!("controlled ownership failure"))
    });
    assert!(matches!(
        response,
        Response::Error {
            code: ErrorCode::PartialFailure,
            ..
        }
    ));
    assert!(!state.shutdown.load(Ordering::Acquire));
    assert!(matches!(session.summary().phase, SessionPhase::Running));
    assert!(
        session
            .apply_agent_report(&AgentReport {
                session: SessionId(7),
                capability,
                sequence: None,
                update: ovrcr_protocol::AgentUpdate::Activity(AgentActivity::Busy),
            })
            .is_err()
    );
    session.terminate(Duration::from_secs(2)).unwrap();
    waiter.join().unwrap();
}

#[test]
fn kill_session_termination_failure_revokes_and_retains_session() {
    let cwd = tempfile::tempdir().unwrap();
    let (events, receiver) = event_channel(None);
    let capability = [0x5a; 32];
    let refuse_sigcont = Arc::new(AtomicBool::new(true));
    let refusal = Arc::clone(&refuse_sigcont);
    let signal_result_hook = Arc::new(move || {
        if refusal.load(Ordering::Acquire) {
            Some(anyhow::anyhow!(
                "signal PTY process group: Operation not permitted (os error 1)"
            ))
        } else {
            None
        }
    });
    let session = Session::spawn_with_test_hooks(
        SessionId(71),
        SessionSpec {
            run: ovrcr_protocol::SessionRunId(1),
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "kill-failure".into(),
            label: "sh".into(),
            cwd: cwd.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "trap '' HUP TERM; printf READY; while :; do read line; done".into(),
            ],
            hook_env: Some(HookEnvironment {
                socket: PathBuf::from("/private/test/ovrcr.sock"),
                session: SessionId(71),
                capability,
            }),
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
        None,
        None,
        Some(signal_result_hook),
    )
    .unwrap();
    let original_pgid = {
        let pid = session.summary().pid.expect("live session PID");
        let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
        assert_eq!(pgid, pid as libc::pid_t);
        pgid
    };
    let event_session = Arc::clone(&session);
    let waiter = thread::spawn(move || {
        while let Ok(event) = receiver.recv() {
            let exited = matches!(event, SessionEvent::Exited { .. });
            event_session.apply_event(event);
            if exited {
                break;
            }
        }
    });
    let (state, dispatch_receiver) = test_state_with_dispatch(None, None);
    register_test_session(&state, SessionId(71), Arc::clone(&session));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let mut cleanup = KillFailureCleanup::new(
        Arc::clone(&session),
        Arc::clone(&refuse_sigcont),
        Arc::clone(&state),
        original_pgid,
        waiter,
        dispatcher,
        true,
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while !String::from_utf8_lossy(&session.current_screen()).contains("READY") {
        assert!(
            Instant::now() < deadline,
            "termination failure session did not start"
        );
        thread::park_timeout(Duration::from_millis(5));
    }

    let error = state
        .kill_session(SessionId(71), Duration::from_millis(50))
        .unwrap_err();
    assert!(error.to_string().contains("Operation not permitted"));
    assert!(state.sessions.lock().unwrap().contains_key(&SessionId(71)));
    assert!(matches!(session.summary().phase, SessionPhase::Running));
    assert!(unsafe { libc::kill(-(session.summary().pid.unwrap() as libc::pid_t), 0) } == 0);
    let mut role = ClientRole::Control;
    assert!(matches!(
        state.handle_request(
            &mut role,
            Request::AgentReport(AgentReport {
                session: SessionId(71),
                capability,
                sequence: None,
                update: ovrcr_protocol::AgentUpdate::Activity(AgentActivity::Busy),
            }),
        ),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));

    assert!(cleanup.finish());
    state.remove_session(SessionId(71)).unwrap();
}

#[test]
fn kill_failure_cleanup_retains_original_group_after_leader_exit() {
    let cwd = tempfile::tempdir().unwrap();
    let (events, receiver) = event_channel(None);
    let session = Session::spawn_with_test_hooks(
            SessionId(72),
            SessionSpec { run: ovrcr_protocol::SessionRunId(1), kind: ovrcr_protocol::SessionKind::Terminal, project: "p".into(),
            workspace: "w".into(),
            name: "exited-leader".into(),
            label: "sh".into(),
            cwd: cwd.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "(trap '' HUP; printf DESCENDANT_READY; while :; do sleep 1; done) & printf LEADER_READY; while IFS= read -r line; do case \"$line\" in HOST_OWNERSHIP_ACK) printf HOST_OWNERSHIP_ACKED; IFS= read -r line || break; [ \"$line\" = ALLOW_LEADER_EXIT ] && kill -KILL \"$$\";; esac; done".into(),
            ],
            hook_env: None, },
            TerminalSize { rows: 24, cols: 80 },
            events,
            None,
            None,
            None,
        )
        .unwrap();
    // Session::spawn verified that the leader owns its original group. Capture that
    // immutable identity before any readiness handshake, then install the cleanup owner.
    let original_pgid = session.summary().pid.expect("live session PID") as libc::pid_t;
    let event_session = Arc::clone(&session);
    let waiter = thread::spawn(move || {
        while let Ok(event) = receiver.recv() {
            let exited = matches!(event, SessionEvent::Exited { .. });
            event_session.apply_event(event);
            if exited {
                break;
            }
        }
    });
    let (state, dispatch_receiver) = test_state_with_dispatch(None, None);
    register_test_session(&state, SessionId(72), Arc::clone(&session));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let refuse_sigcont = Arc::new(AtomicBool::new(false));
    let mut cleanup = KillFailureCleanup::new(
        Arc::clone(&session),
        refuse_sigcont,
        Arc::clone(&state),
        original_pgid,
        waiter,
        dispatcher,
        false,
    );
    assert_eq!(
        unsafe { libc::getpgid(original_pgid) },
        original_pgid,
        "leader must remain alive while cleanup ownership is installed"
    );
    assert!(
        wait_test_screen(&session, "DESCENDANT_READY", Duration::from_secs(2)),
        "descendant did not acknowledge its HUP handler readiness"
    );
    session.write(b"HOST_OWNERSHIP_ACK\r").unwrap();
    assert!(
        wait_test_screen(&session, "HOST_OWNERSHIP_ACKED", Duration::from_secs(2)),
        "leader did not acknowledge host cleanup ownership"
    );
    session.write(b"ALLOW_LEADER_EXIT\r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while test_pid_exists(original_pgid) && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(5));
    }
    assert!(!test_pid_exists(original_pgid));
    assert!(test_group_exists(original_pgid));
    // This forced Exited event is a test seam only; production waits for its owned
    // process group to disappear before publishing Exited.
    session.apply_event(SessionEvent::Exited {
        run: ovrcr_protocol::SessionRunId(1),
        id: SessionId(72),
        phase: SessionPhase::Exited {
            code: Some(0),
            signal: None,
        },
    });
    assert!(session.summary().pid.is_none());
    assert!(cleanup.finish());
    assert!(wait_test_group_absent(
        original_pgid,
        Duration::from_secs(2)
    ));
    assert!(!test_group_exists(original_pgid));
}

#[test]
fn shutdown_without_kill_rejects_paused_session() {
    let cwd = tempfile::tempdir().unwrap();
    let (events, receiver) = event_channel(None);
    let session = Session::spawn_registered(
        SessionId(8),
        crate::session::SessionSpec {
            kind: ovrcr_protocol::SessionKind::Terminal,
            run: ovrcr_protocol::SessionRunId(1),
            project: "p".into(),
            workspace: "w".into(),
            name: "paused".into(),
            label: "sh".into(),
            cwd: cwd.path().to_path_buf(),
            argv: vec!["sh".into()],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
        NO_REGISTER,
    )
    .unwrap();
    let dispatch_session = Arc::clone(&session);
    let waiter = thread::spawn(move || {
        while let Ok(event) = receiver.recv() {
            let exited = matches!(event, SessionEvent::Exited { .. });
            dispatch_session.apply_event(event);
            if exited {
                break;
            }
        }
    });
    session.set_paused(true).unwrap();
    let state = test_state(None, None);
    register_test_session(&state, SessionId(8), Arc::clone(&session));
    assert!(matches!(
        handle_shutdown(&state, false, |_| panic!("paused session was not guarded")),
        Response::Error {
            code: ErrorCode::SessionsRemain,
            ..
        }
    ));
    session.terminate(Duration::from_secs(2)).unwrap();
    waiter.join().unwrap();
}

#[test]
fn close_failure_retains_record_until_cleanup_can_finish() {
    let cwd = tempfile::tempdir().unwrap();
    let (events, receiver) = event_channel(None);
    let id = SessionId(17);
    let session = Session::spawn_registered(
        id,
        crate::session::SessionSpec {
            kind: ovrcr_protocol::SessionKind::Terminal,
            run: ovrcr_protocol::SessionRunId(1),
            project: "p".into(),
            workspace: "w".into(),
            name: "retained".into(),
            label: "sh".into(),
            cwd: cwd.path().to_path_buf(),
            argv: vec!["sh".into()],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
        NO_REGISTER,
    )
    .unwrap();
    let (state, _dispatch_receiver) = test_state_with_dispatch(None, None);
    register_test_session(&state, id, Arc::clone(&session));
    state.dashboard.install_view_for_test(Some(DashboardView {
        revision: 1,
        panes: vec![PaneTarget {
            run: ovrcr_protocol::SessionRunId(1),
            session: id,
            size: TerminalSize { rows: 24, cols: 80 },
        }],
        focused: Some(id),
    }));

    // Hold the real exit event after the child has been reaped. This test
    // exercises a delayed dispatcher, not a race with process-group signals.
    session.write(b"exit\r").unwrap();
    let mut pending = Vec::new();
    loop {
        let event = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let exited = matches!(event, SessionEvent::Exited { .. });
        pending.push(event);
        if exited {
            break;
        }
    }

    let error = state
        .close_terminal(id, session.run(), Duration::from_millis(20))
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("timed out waiting for session exit"),
        "unexpected close failure: {error:#}"
    );
    assert!(state.sessions.lock().unwrap().contains_key(&id));
    assert_eq!(
        state
            .dashboard
            .view()
            .as_ref()
            .and_then(|view| view.focused),
        Some(id)
    );

    for event in pending {
        session.apply_event(event);
    }
    let error = state
        .close_terminal(id, session.run(), Duration::from_millis(20))
        .unwrap_err();
    assert!(
        state.sessions.lock().unwrap().contains_key(&id),
        "close after natural exit must retain the row: {error:#}"
    );
    assert!(
        !state.retained.lock().get(id).unwrap().stopped,
        "close after natural exit must not certify stopped"
    );
    assert!(
        state
            .session_summary(id)
            .unwrap()
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.requires_ack),
        "close after natural exit must leave acknowledgement required"
    );
}

#[test]
fn close_after_natural_exit_does_not_certify_stopped_or_remove_row() {
    let id = SessionId(19);
    let (_cwd, session, receiver) = spawn_exiting_test_session(id);
    let (state, _dispatch_receiver) = test_state_with_dispatch(None, None);
    register_test_session(&state, id, Arc::clone(&session));
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    session.wait_until_exited(Duration::from_secs(2)).unwrap();
    events.finish(Duration::from_secs(1)).unwrap();
    assert!(
        !state.retained.lock().get(id).unwrap().stopped,
        "exit event must not persist stop proof before close"
    );

    let _ = state.close_terminal(id, session.run(), Duration::from_millis(200));
    assert!(
        state.sessions.lock().unwrap().contains_key(&id),
        "close of an already-exited run must not remove the row"
    );
    let retained = state.retained.lock();
    let record = retained.get(id).expect("retained row missing");
    assert!(
        !record.stopped,
        "already-exited close must not certify stopped"
    );
    drop(retained);
    let summary = state.session_summary(id).unwrap();
    assert!(
        summary
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.requires_ack),
        "already-exited close must leave acknowledgement required: {summary:?}"
    );
}

#[test]
fn close_after_ack_removes_already_exited_in_memory_row() {
    let id = SessionId(20);
    let (_cwd, session, receiver) = spawn_exiting_test_session(id);
    let (state, _dispatch_receiver) = test_state_with_dispatch(None, None);
    register_test_session(&state, id, Arc::clone(&session));
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    session.wait_until_exited(Duration::from_secs(2)).unwrap();
    events.finish(Duration::from_secs(1)).unwrap();
    let run = session.run();
    assert!(
        state
            .session_summary(id)
            .unwrap()
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.requires_ack),
        "natural exit must require acknowledgement before close"
    );

    let mut role = ClientRole::Control;
    assert_eq!(
        state.handle_request(
            &mut role,
            Request::AcknowledgeSessionStopped {
                session: id,
                expected_run: run,
            },
        ),
        Response::Ok
    );
    let acknowledged = state.session_summary(id).unwrap();
    assert!(
        !acknowledged
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.requires_ack),
        "acknowledgement must clear close ownership uncertainty: {acknowledged:?}"
    );
    assert_eq!(
        state.handle_request(
            &mut role,
            Request::CloseTerminal {
                session: id,
                expected_run: run,
            },
        ),
        Response::Ok,
        "acknowledged already-exited close must succeed without treating AlreadyExited as stop proof"
    );
    assert!(
        !state.sessions.lock().unwrap().contains_key(&id),
        "acknowledged close must drop the in-memory row"
    );
    assert!(
        state.retained.lock().get(id).is_none(),
        "acknowledged close must remove the retained record"
    );
}

#[test]
fn stale_close_does_not_revoke_or_stop_current_run() {
    let id = SessionId(22);
    let (_cwd, session, receiver) = spawn_live_test_session(id);
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    let (state, _dispatch_receiver) = test_state_with_dispatch(None, None);
    register_test_session(&state, id, Arc::clone(&session));
    let current = session.run();
    let mut role = ClientRole::Control;
    assert!(
        matches!(
            state.handle_request(
                &mut role,
                Request::CloseTerminal {
                    session: id,
                    expected_run: ovrcr_protocol::SessionRunId(current.0.saturating_add(1)),
                },
            ),
            Response::Error {
                code: ErrorCode::Conflict,
                ..
            }
        ),
        "stale CloseTerminal must conflict without acting on the live run"
    );
    assert!(
        matches!(session.summary().phase, SessionPhase::Running),
        "stale close must not stop the current run: {:?}",
        session.summary().phase
    );
    assert_eq!(state.session_summary(id).unwrap().run, current);
    assert!(state.sessions.lock().unwrap().contains_key(&id));
    cleanup_test_session(&session, events).unwrap();
}

#[test]
fn close_and_kill_without_live_arc_do_not_mark_stopped() {
    let current = crate::retained::current_boot_id();
    let other = "11111111-2222-3333-4444-555555555555";
    assert!(
        crate::retained::different_boot(Some(other), current.as_deref()),
        "need two verified boot ids to reach missing-Arc control; current={current:?}"
    );
    for close in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        crate::config::initialize_registry(&config).unwrap();
        let cwd = {
            use std::os::unix::ffi::OsStrExt;
            dir.path().as_os_str().as_bytes().to_vec()
        };
        let kind = serde_json::to_string(&ovrcr_protocol::SessionKind::Terminal).unwrap();
        let name = if close { "no-arc-close" } else { "no-arc-kill" };
        {
            let connection = crate::config::open_writable_registry(&config).unwrap();
            connection
                .execute(
                    "INSERT INTO retained_sessions
                     (run, project, workspace, name, label, cwd, kind, pinned_title,
                      application_title, title_revision, boot_id, stopped, failure)
                     VALUES (1, 'p', 'w', ?1, 'sh', ?2, ?3, NULL, NULL, 0, ?4, 0, NULL)",
                    rusqlite::params![name, cwd, kind, other],
                )
                .unwrap();
        }
        let store = crate::retained::SessionStore::open(&config).unwrap();
        let id = store.records().next().unwrap().id;
        let (state, _dispatch_receiver) = test_state_with_dispatch(None, None);
        *state.retained.lock() = store;
        assert!(
            state.sessions.lock().unwrap().get(&id).is_none(),
            "fixture must have no live Arc"
        );
        assert!(
            !state.retained.lock().get(id).unwrap().stopped,
            "fixture must not already be certified stopped"
        );
        if close {
            state
                .close_terminal(
                    id,
                    state.session_summary(id).unwrap().run,
                    Duration::from_millis(50),
                )
                .expect("boot-resolved close with no Arc must remove the row");
            assert!(
                state.retained.lock().get(id).is_none(),
                "close must remove the boot-resolved row without a controlled-stop certificate"
            );
        } else {
            state.kill_session(id, Duration::from_millis(50)).expect(
                "boot-resolved kill with no Arc must not fail after boot resolved ownership",
            );
            assert!(
                !state.retained.lock().get(id).unwrap().stopped,
                "missing Arc must not mark_stopped"
            );
        }
        let _ = dir;
    }
}

#[test]
fn listing_failure_pause_kill_close_are_ownership_uncertain_and_do_not_mark_stopped() {
    for (id, op) in [
        (SessionId(90), "pause"),
        (SessionId(91), "kill"),
        (SessionId(92), "close"),
    ] {
        let (_cwd, session, receiver) = spawn_live_test_session(id);
        let events = apply_test_session_events(Arc::clone(&session), receiver);
        let (state, _dispatch_receiver) = test_state_with_dispatch(None, None);
        register_test_session(&state, id, Arc::clone(&session));
        session.set_listing_error_hook(Some(Arc::new(|| {
            Some(anyhow::anyhow!("ovrcr-test-injected-listing-failure"))
        })));
        let error = match op {
            "pause" => state.set_session_paused(id, true),
            "kill" => state.kill_session(id, Duration::from_millis(250)),
            "close" => state.close_terminal(id, session.run(), Duration::from_millis(250)),
            _ => unreachable!(),
        }
        .unwrap_err();
        let response = error_for_lifecycle(error);
        assert_eq!(
            match &response {
                Response::Error { code, .. } => code,
                other => panic!("{op} listing failure mapped to {other:?}"),
            },
            &ErrorCode::OwnershipUncertain,
            "{op} listing failure must be typed OwnershipUncertain"
        );
        assert!(
            !state.retained.lock().get(id).unwrap().stopped,
            "{op} listing failure must not mark_stopped"
        );
        session.set_listing_error_hook(None);
        let _ = cleanup_test_session(&session, events);
    }
}

#[test]
fn close_of_captured_live_target_that_exits_before_terminate_does_not_certify() {
    let id = SessionId(21);
    let cwd = tempfile::tempdir().unwrap();
    let (events, receiver) = event_channel(None);
    let capability = [0x21; 32];
    let (hold_entered_tx, hold_entered_rx) = mpsc::sync_channel(1);
    let (hold_release_tx, hold_release_rx) = mpsc::sync_channel(1);
    let hold_release_rx = Mutex::new(hold_release_rx);
    let session = Session::spawn_with_test_hooks(
        id,
        SessionSpec {
            run: ovrcr_protocol::SessionRunId(1),
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: "p".into(),
            workspace: "w".into(),
            name: "captured-exit".into(),
            label: "sh".into(),
            cwd: cwd.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "trap '' HUP TERM; printf READY; IFS= read -r _; exit 0".into(),
            ],
            hook_env: Some(HookEnvironment {
                socket: PathBuf::from("/private/test/ovrcr.sock"),
                session: id,
                capability,
            }),
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
        None,
        Some(Arc::new(move || {
            let _ = hold_entered_tx.send(());
            let _ = hold_release_rx.lock().unwrap().recv();
        })),
        None,
    )
    .unwrap();
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    let (state, _dispatch_receiver) = test_state_with_dispatch(None, None);
    register_test_session(&state, id, Arc::clone(&session));
    assert!(
        wait_test_screen(&session, "READY", Duration::from_secs(2)),
        "captured-exit fixture did not start"
    );

    let hold_session = Arc::clone(&session);
    let holder = thread::spawn(move || hold_session.terminate(Duration::from_secs(2)));
    hold_entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("holder must enter terminate before close captures");
    assert!(
        matches!(session.summary().phase, SessionPhase::Running),
        "holder SIGTERM must not finish the trapped process"
    );

    let close_state = Arc::clone(&state);
    let close_run = session.run();
    let closer = thread::spawn(move || {
        close_state.close_terminal(id, close_run, Duration::from_millis(200))
    });
    let report = AgentReport {
        session: id,
        capability,
        sequence: None,
        update: ovrcr_protocol::AgentUpdate::Activity(AgentActivity::Busy),
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while session.apply_agent_report(&report).is_ok() {
        assert!(
            Instant::now() < deadline,
            "close did not capture/revoke the live target"
        );
        thread::park_timeout(Duration::from_millis(5));
    }
    assert!(
        matches!(session.summary().phase, SessionPhase::Running),
        "close must capture the target while it is still live"
    );

    session.write(b"\r").unwrap();
    session.wait_until_exited(Duration::from_secs(2)).unwrap();
    events.finish(Duration::from_secs(1)).unwrap();
    let _ = hold_release_tx.send(());
    let _ = holder.join();
    let close_result = closer.join().expect("close thread panicked");

    assert!(
        close_result.is_err(),
        "live capture then natural exit must not close-success: {close_result:?}"
    );
    assert!(
        state.sessions.lock().unwrap().contains_key(&id),
        "captured-before-exit close must not remove the row"
    );
    assert!(
        !state.retained.lock().get(id).unwrap().stopped,
        "captured-before-exit close must not certify stopped"
    );
    let summary = state.session_summary(id).unwrap();
    assert_eq!(summary.run, ovrcr_protocol::SessionRunId(1));
    assert!(
        summary
            .recovery
            .as_ref()
            .is_some_and(|recovery| recovery.requires_ack),
        "captured-before-exit close must leave acknowledgement required: {summary:?}"
    );
}

#[test]
fn registration_publishes_the_session_before_its_events_can_arrive() {
    // The session becomes visible to the dispatcher inside the spawn, before
    // the PTY reader and child waiter start, so a hook report that lands while
    // the spawn is still in flight is answered rather than rejected. The
    // sessions guard is not held for the rest of the spawn.
    let root = tempfile::tempdir().unwrap();
    let (events, event_receiver) = event_channel(None);
    let (dispatch, dispatch_receiver) = dispatch_channel(None);
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let registry = Registry {
        projects: vec![crate::config::ProjectRecord {
            name: "project".into(),
            repo: root.path().to_path_buf(),
            workspace_root: root.path().to_path_buf(),
            workspaces: vec![crate::config::WorkspaceRecord {
                name: "workspace".into(),
                path: workspace.clone(),
                branch: "main".into(),
            }],
        }],
    };
    let socket_path = root.path().join("socket");
    let _socket_guard = UnixListener::bind(&socket_path).unwrap();
    let state = Arc::new(ServerState {
        tasks: None,
        socket: socket_path,
        registry_path: root.path().join("config.toml"),
        registry: Mutex::new(registry),
        sessions: Mutex::new(HashMap::new()),
        dashboard: ActiveDashboard::default(),
        retained: parking_lot::Mutex::new(SessionStore::in_memory()),
        mutation_lock: Mutex::new(()),
        dispatch: dispatch.clone(),
        shutdown: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        events: Mutex::new(Some(events)),
        #[cfg(test)]
        resize_hook: Mutex::new(None),
        before_view_publish_hook: Mutex::new(None),
        before_dashboard_write_hook: Mutex::new(None),
        #[cfg(feature = "acceptance-diagnostics")]
        dashboard_monitor: None,
    });
    let dispatcher_state = Arc::clone(&state);
    let dispatcher_finished = Arc::new(AtomicBool::new(false));
    let dispatcher_done = Arc::clone(&dispatcher_finished);
    let dispatcher = thread::spawn(move || {
        run_dispatcher(dispatcher_state, dispatch_receiver);
        dispatcher_done.store(true, Ordering::Release);
    });
    let bridge_dispatch = dispatch.clone();
    let bridge_finished = Arc::new(AtomicBool::new(false));
    let bridge_done = Arc::clone(&bridge_finished);
    let bridge = thread::spawn(move || {
        bridge_events(event_receiver, bridge_dispatch);
        bridge_done.store(true, Ordering::Release);
    });
    let (gate, entered) = RegistrationGate::new();
    let identity = root.path().join("held-identity");
    let ready: Arc<dyn Fn() + Send + Sync> = {
        let gate = Arc::clone(&gate);
        Arc::new(move || {
            gate.wait();
        })
    };
    let creator_state = Arc::clone(&state);
    let creator_ready = Arc::clone(&ready);
    let creator_identity = identity.clone();
    let creator = thread::spawn(move || {
        creator_state.create_session_with_ready(
                ovrcr_protocol::CreateSessionRequest {
                    kind: ovrcr_protocol::SessionKind::Terminal,
                    project: "project".into(),
                    workspace: "workspace".into(),
                    name: "fast".into(),
                    label: None,
                    argv: vec![
                        "sh".into(),
                        "-c".into(),
                        "printf '%s\\n%s\\n' \"$OVRCR_SESSION_ID\" \"$OVRCR_HOOK_TOKEN\" > \"$1\"; while IFS= read -r line; do :; done".into(),
                        "ovrcr-held".into(),
                        creator_identity.into_os_string(),
                    ],
                },
                creator_ready,
            )
    });
    let mut cleanup = RegistrationCleanup::new(
        Arc::clone(&gate),
        Arc::clone(&state),
        creator,
        dispatcher,
        bridge,
    );
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(
        state.sessions.try_lock().is_ok(),
        "registration must not hold the sessions guard while Session::spawn is paused"
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut identity_contents = None;
    while Instant::now() < deadline {
        if let Ok(contents) = std::fs::read_to_string(&identity) {
            let mut identity_lines = contents.lines();
            let complete = match (
                identity_lines.next(),
                identity_lines.next(),
                identity_lines.next(),
            ) {
                (Some(session_id), Some(capability), None) => {
                    session_id.parse::<u64>().is_ok()
                        && capability.len() == 64
                        && capability.bytes().all(|byte| byte.is_ascii_hexdigit())
                }
                _ => false,
            };
            if complete {
                identity_contents = Some(contents);
                break;
            }
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    let identity_contents = identity_contents.expect("managed identity contents did not complete");
    let mut identity_lines = identity_contents.lines();
    let session_id = identity_lines
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap();
    let capability_text = identity_lines.next().unwrap();
    let capability = parse_test_capability(capability_text);
    let (completion, completion_result) = mpsc::sync_channel(1);
    dispatch
        .try_send(DispatchMessage::AgentReport {
            report: AgentReport {
                session: SessionId(session_id),
                capability,
                sequence: None,
                update: ovrcr_protocol::AgentUpdate::Activity(AgentActivity::Busy),
            },
            completion,
        })
        .unwrap();
    gate.release();
    let summary = cleanup.join_creator().unwrap();
    assert_eq!(summary.name, "fast");
    assert!(state.sessions.lock().unwrap().contains_key(&summary.id));
    assert_eq!(
        completion_result
            .recv_timeout(Duration::from_secs(2))
            .unwrap(),
        Response::Ok
    );
    let session = state
        .sessions
        .lock()
        .unwrap()
        .get(&summary.id)
        .cloned()
        .unwrap();
    assert_eq!(session.summary().activity, AgentActivity::Busy);
    let original_pgid = summary.pid.expect("creator returned live session") as libc::pid_t;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_eq!(session.summary().activity, AgentActivity::Busy);
        panic!("intentional registration post-join assertion failure");
    }));
    assert!(result.is_err());
    drop(cleanup);
    assert!(wait_test_group_absent(
        original_pgid,
        Duration::from_secs(2)
    ));
    assert!(dispatcher_finished.load(Ordering::Acquire));
    assert!(bridge_finished.load(Ordering::Acquire));
}

#[test]
fn session_output_flows_while_another_session_spawns() {
    // Creation must not hold the sessions guard across Session::spawn: the
    // dispatcher takes that same guard for every PTY byte, so a spawn that
    // stalls waiting for its child's process group would stall the whole
    // dashboard with it. The gate channel proves the spawn really is in
    // flight when the output is injected.
    let root = tempfile::tempdir().unwrap();
    let (events, event_receiver) = event_channel(None);
    let (dispatch, dispatch_receiver) = dispatch_channel(None);
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let registry = Registry {
        projects: vec![crate::config::ProjectRecord {
            name: "project".into(),
            repo: root.path().to_path_buf(),
            workspace_root: root.path().to_path_buf(),
            workspaces: vec![crate::config::WorkspaceRecord {
                name: "workspace".into(),
                path: workspace.clone(),
                branch: "main".into(),
            }],
        }],
    };
    let socket_path = root.path().join("socket");
    let _socket_guard = UnixListener::bind(&socket_path).unwrap();
    let owner = Arc::new(());
    let (server_stream, _client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let live_id = SessionId(900);
    // The live session shares the server's event channel, so the fixture's
    // bridge and dispatcher apply its output and exit the way they would for
    // any session the server created.
    let live = Session::spawn_registered(
        live_id,
        SessionSpec {
            run: ovrcr_protocol::SessionRunId(1),
            kind: ovrcr_protocol::SessionKind::Terminal,
            project: "project".into(),
            workspace: "workspace".into(),
            name: "live".into(),
            label: "sh".into(),
            cwd: workspace.clone(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "while IFS= read -r line; do :; done".into(),
            ],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        events.clone(),
        NO_REGISTER,
    )
    .unwrap();
    let state = Arc::new(ServerState {
        tasks: None,
        socket: socket_path,
        registry_path: root.path().join("config.toml"),
        registry: Mutex::new(registry),
        sessions: Mutex::new(HashMap::new()),
        dashboard: ActiveDashboard::default(),
        retained: parking_lot::Mutex::new(SessionStore::in_memory()),
        mutation_lock: Mutex::new(()),
        dispatch: dispatch.clone(),
        shutdown: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        events: Mutex::new(Some(events)),
        #[cfg(test)]
        resize_hook: Mutex::new(None),
        before_view_publish_hook: Mutex::new(None),
        before_dashboard_write_hook: Mutex::new(None),
        #[cfg(feature = "acceptance-diagnostics")]
        dashboard_monitor: None,
    });
    register_test_session(&state, live_id, live.clone());
    state
        .dashboard
        .claim_with_identity(sink.clone(), owner, server_stream);
    state.dashboard.install_view_for_test(Some(DashboardView {
        revision: 7,
        panes: vec![PaneTarget {
            run: ovrcr_protocol::SessionRunId(1),
            session: live_id,
            size: TerminalSize { rows: 24, cols: 80 },
        }],
        focused: Some(live_id),
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let bridge_dispatch = dispatch.clone();
    let bridge = thread::spawn(move || bridge_events(event_receiver, bridge_dispatch));
    let (gate, entered) = RegistrationGate::new();
    let ready: Arc<dyn Fn() + Send + Sync> = {
        let gate = Arc::clone(&gate);
        Arc::new(move || gate.wait())
    };
    let creator_state = Arc::clone(&state);
    let creator = thread::spawn(move || {
        creator_state.create_session_with_ready(
            ovrcr_protocol::CreateSessionRequest {
                kind: ovrcr_protocol::SessionKind::Terminal,
                project: "project".into(),
                workspace: "workspace".into(),
                name: "slow".into(),
                label: None,
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "while IFS= read -r line; do :; done".into(),
                ],
            },
            ready,
        )
    });
    let mut cleanup = RegistrationCleanup::new(
        Arc::clone(&gate),
        Arc::clone(&state),
        creator,
        dispatcher,
        bridge,
    );
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    dispatch
        .try_send(DispatchMessage::Session(SessionEvent::Output {
            run: ovrcr_protocol::SessionRunId(1),
            id: live_id,
            bytes: b"LIVE".to_vec(),
        }))
        .unwrap();
    let delivered = |sink: &DashboardSink| {
        sink.queue.lock().unwrap().messages.iter().any(|outbound| {
            matches!(
                &outbound.message,
                ServerMessage::Event(ServerEvent::Output { run: ovrcr_protocol::SessionRunId(1), session, revision, bytes })
                    if *session == live_id && *revision == 7 && bytes == b"LIVE"
            )
        })
    };
    let deadline = Instant::now() + Duration::from_millis(500);
    while !delivered(&sink) && Instant::now() < deadline {
        thread::park_timeout(Duration::from_millis(5));
    }
    assert!(
        delivered(&sink),
        "live session output must reach the dashboard while another spawn is pending"
    );
    assert!(
        state.sessions.try_lock().is_ok(),
        "creation must not hold the sessions guard across Session::spawn"
    );
    let screen = live.current_screen();
    assert!(
        screen.windows(4).any(|window| window == b"LIVE"),
        "the dispatcher must also apply the bytes to the session: {}",
        String::from_utf8_lossy(&screen)
    );
    gate.release();
    let summary = cleanup.join_creator().unwrap();
    assert_eq!(summary.name, "slow");
    assert!(state.sessions.lock().unwrap().contains_key(&summary.id));
    live.terminate(Duration::from_secs(2)).unwrap();
    remove_test_session(&state, &live_id);
}

fn parse_test_capability(value: &str) -> [u8; 32] {
    assert_eq!(value.len(), 64);
    let mut capability = [0_u8; 32];
    for (index, byte) in capability.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
    }
    capability
}

struct RegistrationGate {
    entered: SyncSender<()>,
    release: SyncSender<()>,
    released: Mutex<Receiver<()>>,
    cancelled: AtomicBool,
}

impl RegistrationGate {
    fn new() -> (Arc<Self>, Receiver<()>) {
        let (entered, entered_receiver) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        (
            Arc::new(Self {
                entered,
                release,
                released: Mutex::new(released),
                cancelled: AtomicBool::new(false),
            }),
            entered_receiver,
        )
    }

    fn wait(&self) {
        let _ = self.entered.send(());
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return;
            }
            match self
                .released
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_millis(25))
            {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn release(&self) {
        let _ = self.release.try_send(());
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.release();
    }
}

struct RegistrationCleanup {
    gate: Arc<RegistrationGate>,
    state: Arc<ServerState>,
    creator: Option<JoinHandle<Result<SessionSummary>>>,
    summary: Option<SessionSummary>,
    dispatcher: Option<JoinHandle<()>>,
    bridge: Option<JoinHandle<()>>,
    complete: bool,
}

impl RegistrationCleanup {
    fn new(
        gate: Arc<RegistrationGate>,
        state: Arc<ServerState>,
        creator: JoinHandle<Result<SessionSummary>>,
        dispatcher: JoinHandle<()>,
        bridge: JoinHandle<()>,
    ) -> Self {
        Self {
            gate,
            state,
            creator: Some(creator),
            summary: None,
            dispatcher: Some(dispatcher),
            bridge: Some(bridge),
            complete: false,
        }
    }

    fn join_creator(&mut self) -> Result<SessionSummary> {
        let creator = self
            .creator
            .as_ref()
            .context("registration creator missing")?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while !creator.is_finished() && Instant::now() < deadline {
            thread::park_timeout(Duration::from_millis(5));
        }
        if !creator.is_finished() {
            bail!("registration creator did not finish before deadline");
        }
        let result = self
            .creator
            .take()
            .context("registration creator missing")?
            .join()
            .map_err(|_| anyhow::anyhow!("registration creator panicked"))??;
        self.summary = Some(result.clone());
        Ok(result)
    }

    fn cleanup(&mut self) -> bool {
        if self.complete {
            return true;
        }
        self.gate.cancel();
        let mut cleaned = self.finish_creator();
        if let Some(summary) = self.summary.as_ref() {
            let session = self
                .state
                .sessions
                .lock()
                .unwrap()
                .get(&summary.id)
                .cloned();
            if let Some(session) = session
                && session.terminate(Duration::from_secs(2)).is_err()
            {
                cleaned = false;
            }
        }
        self.state.events.lock().unwrap().take();
        if self.dispatcher.is_some() {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match self.state.dispatch.try_send(DispatchMessage::Stop) {
                    Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => break,
                    Err(mpsc::TrySendError::Full(_)) if Instant::now() < deadline => {
                        thread::park_timeout(Duration::from_millis(5));
                    }
                    Err(mpsc::TrySendError::Full(_)) => {
                        cleaned = false;
                        break;
                    }
                }
            }
        }
        if !join_test_thread_slot(&mut self.dispatcher, Duration::from_secs(2)) {
            cleaned = false;
        }
        if !join_test_thread_slot(&mut self.bridge, Duration::from_secs(2)) {
            cleaned = false;
        }
        if cleaned {
            self.summary = None;
            self.complete = true;
        }
        cleaned
    }

    fn finish_creator(&mut self) -> bool {
        let Some(creator) = self.creator.take() else {
            return true;
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while !creator.is_finished() && Instant::now() < deadline {
            thread::park_timeout(Duration::from_millis(5));
        }
        if !creator.is_finished() {
            eprintln!("registration cleanup did not finish creator before deadline");
            self.creator = Some(creator);
            return false;
        }
        let Ok(result) = creator.join() else {
            eprintln!("registration cleanup creator panicked");
            return false;
        };
        match result {
            Ok(summary) => {
                self.summary = Some(summary);
                true
            }
            Err(_) => true,
        }
    }
}

impl Drop for RegistrationCleanup {
    fn drop(&mut self) {
        if !self.complete
            && (self.creator.is_some()
                || self.summary.is_some()
                || self.dispatcher.is_some()
                || self.bridge.is_some())
            && !self.cleanup()
        {
            eprintln!("registration cleanup did not complete before its deadlines");
        }
    }
}

struct KillFailureCleanup {
    session: Arc<Session>,
    refuse_sigcont: Arc<AtomicBool>,
    state: Arc<ServerState>,
    original_pgid: libc::pid_t,
    waiter: Option<JoinHandle<()>>,
    dispatcher: Option<JoinHandle<()>>,
    ack_before_stop: bool,
}

impl KillFailureCleanup {
    fn new(
        session: Arc<Session>,
        refuse_sigcont: Arc<AtomicBool>,
        state: Arc<ServerState>,
        original_pgid: libc::pid_t,
        waiter: JoinHandle<()>,
        dispatcher: JoinHandle<()>,
        ack_before_stop: bool,
    ) -> Self {
        Self {
            session,
            refuse_sigcont,
            state,
            original_pgid,
            waiter: Some(waiter),
            dispatcher: Some(dispatcher),
            ack_before_stop,
        }
    }

    fn finish(&mut self) -> bool {
        self.cleanup()
    }

    fn cleanup(&mut self) -> bool {
        self.refuse_sigcont.store(false, Ordering::Release);
        let group_present = test_group_exists(self.original_pgid);
        let exited_with_group =
            group_present && matches!(self.session.summary().phase, SessionPhase::Exited { .. });
        let mut cleaned =
            !exited_with_group && self.session.terminate(Duration::from_secs(2)).is_ok();
        if !cleaned || test_group_exists(self.original_pgid) {
            cleaned = self.force_kill_owned_group()
                && self
                    .session
                    .wait_until_exited(Duration::from_secs(2))
                    .is_ok();
        }

        if cleaned && self.ack_before_stop {
            let id = self.session.id();
            let run = self.session.run();
            if self.state.acknowledge_session_stopped(id, run).is_err() {
                cleaned = false;
            }
        }

        if self.dispatcher.is_some() {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match self.state.dispatch.try_send(DispatchMessage::Stop) {
                    Ok(()) | Err(mpsc::TrySendError::Disconnected(_)) => break,
                    Err(mpsc::TrySendError::Full(_)) if Instant::now() < deadline => {
                        thread::park_timeout(Duration::from_millis(5));
                    }
                    Err(mpsc::TrySendError::Full(_)) => {
                        cleaned = false;
                        break;
                    }
                }
            }
        }
        if let Some(dispatcher) = self.dispatcher.take()
            && !join_test_thread_bounded(dispatcher, Duration::from_secs(2))
        {
            cleaned = false;
        }
        if let Some(waiter) = self.waiter.take()
            && !join_test_thread_bounded(waiter, Duration::from_secs(2))
        {
            cleaned = false;
        }
        cleaned
    }

    fn force_kill_owned_group(&self) -> bool {
        let pgid = self.original_pgid;
        if pgid <= 1 {
            return false;
        }
        let result = unsafe { libc::kill(-pgid, libc::SIGKILL) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return false;
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let result = unsafe { libc::kill(-pgid, 0) };
            if result != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
    }
}

impl Drop for KillFailureCleanup {
    fn drop(&mut self) {
        if (self.waiter.is_some() || self.dispatcher.is_some()) && !self.cleanup() {
            eprintln!("kill failure cleanup did not complete before its deadlines");
        }
    }
}

fn join_test_thread_bounded(handle: JoinHandle<()>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    handle.join().is_ok()
}

fn join_test_thread_slot(handle: &mut Option<JoinHandle<()>>, timeout: Duration) -> bool {
    let Some(handle_ref) = handle.as_ref() else {
        return true;
    };
    let deadline = Instant::now() + timeout;
    while !handle_ref.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    handle.take().unwrap().join().is_ok()
}

fn test_group_exists(pgid: libc::pid_t) -> bool {
    if pgid <= 1 {
        return false;
    }
    let result = unsafe { libc::kill(-pgid, 0) };
    if result == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

fn test_pid_exists(pid: libc::pid_t) -> bool {
    if pid <= 1 {
        return false;
    }
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn wait_test_group_absent(pgid: libc::pid_t, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while test_group_exists(pgid) {
        if Instant::now() >= deadline {
            return false;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    true
}

fn wait_test_screen(session: &Session, marker: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if String::from_utf8_lossy(&session.current_screen()).contains(marker) {
            return true;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    false
}

#[test]
fn dashboard_slot_is_released_when_registration_panics() {
    let state = test_state(None, None);

    let (server, mut client) = UnixStream::pair().unwrap();
    let handler_state = Arc::clone(&state);
    let (armed, armed_rx) = mpsc::channel();
    let (proceed, proceed_rx) = mpsc::channel();
    let handler = thread::spawn(move || {
        connections::PANIC_AFTER_DASHBOARD_REGISTRATION.with(|armed| armed.set(true));
        armed.send(()).unwrap();
        proceed_rx.recv().unwrap();
        handle_connection(handler_state, server);
    });
    armed_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    // Force an unrelated server to register while the intended handler's panic
    // is armed. Parallel tests must not consume one another's injection.
    let other_state = test_state(None, None);
    let (other_server, mut other_client) = UnixStream::pair().unwrap();
    other_client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let other_handler = thread::spawn(move || handle_connection(other_state, other_server));
    exchange_preamble(&mut other_client).unwrap();
    write_frame(
        &mut other_client,
        &ClientMessage {
            request_id: 99,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    let other_response = read_frame::<ServerMessage>(&mut other_client);
    drop(other_client);
    let other_result = other_handler.join();

    proceed.send(()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    exchange_preamble(&mut client).unwrap();
    write_frame(
        &mut client,
        &ClientMessage {
            request_id: 1,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    // Close even if the injection was stolen, so a red test cannot hang on join.
    let _ = read_frame::<ServerMessage>(&mut client);
    drop(client);
    let handler_result = handler.join();
    assert!(
        other_result.is_ok(),
        "another handler consumed the panic injection"
    );
    assert!(matches!(
        other_response,
        Ok(ServerMessage::Response {
            request_id: 99,
            response: Response::Hierarchy(_),
        })
    ));
    assert!(
        handler_result.is_err(),
        "the injected panic must unwind the handler"
    );
    assert!(
        !state.dashboard.is_claimed(),
        "unwinding must release the dashboard slot"
    );

    let (server, mut client) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let handler_state = Arc::clone(&state);
    let handler = thread::spawn(move || handle_connection(handler_state, server));
    exchange_preamble(&mut client).unwrap();
    write_frame(
        &mut client,
        &ClientMessage {
            request_id: 2,
            request: Request::DashboardHello,
        },
    )
    .unwrap();
    assert!(
        matches!(
            read_frame::<ServerMessage>(&mut client).unwrap(),
            ServerMessage::Response {
                request_id: 2,
                response: Response::Hierarchy(_),
            }
        ),
        "a later dashboard must be accepted"
    );
    drop(client);
    handler.join().unwrap();
}

#[test]
fn accept_loop_survives_thread_spawn_failure() {
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("server.sock");
    let registry = root.path().join("config.toml");
    save_registry_atomic(&Registry::default(), &registry).unwrap();
    let paths = ServerPaths {
        socket: socket.clone(),
    };
    let server = thread::spawn(move || run_server(paths, registry));
    let deadline = Instant::now() + Duration::from_secs(3);
    while UnixStream::connect(&socket).is_err() {
        assert!(Instant::now() < deadline, "server did not start");
        thread::sleep(Duration::from_millis(10));
    }

    // The readiness probes above were accepted in order; a completed request
    // proves the accept loop is idle before the seam is armed.
    let mut warm_up = UnixStream::connect(&socket).unwrap();
    exchange_preamble(&mut warm_up).unwrap();
    write_frame(
        &mut warm_up,
        &ClientMessage {
            request_id: 0,
            request: Request::List,
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut warm_up).unwrap();
    drop(warm_up);

    startup::FAIL_NEXT_ACCEPT_SPAWN.store(true, Ordering::Release);
    let mut dropped = UnixStream::connect(&socket).unwrap();
    dropped
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert!(
        exchange_preamble(&mut dropped).is_err(),
        "the client whose thread could not be spawned is closed"
    );
    drop(dropped);

    let mut client = UnixStream::connect(&socket).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    exchange_preamble(&mut client).unwrap();
    write_frame(
        &mut client,
        &ClientMessage {
            request_id: 1,
            request: Request::List,
        },
    )
    .unwrap();
    assert!(
        matches!(
            read_frame::<ServerMessage>(&mut client).unwrap(),
            ServerMessage::Response {
                request_id: 1,
                response: Response::Hierarchy(_),
            }
        ),
        "the server must keep serving after one spawn failure"
    );
    drop(client);

    let mut client = UnixStream::connect(&socket).unwrap();
    exchange_preamble(&mut client).unwrap();
    write_frame(
        &mut client,
        &ClientMessage {
            request_id: 2,
            request: Request::Shutdown { kill: false },
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame::<ServerMessage>(&mut client).unwrap(),
        ServerMessage::Response {
            request_id: 2,
            response: Response::Ok,
        }
    ));
    server.join().unwrap().unwrap();
}

mod agent_reporting {
    use super::*;
    use ovrcr_protocol::*;

    #[test]
    fn retained_conversation_changes_are_fenced_by_binding_and_lease() {
        let fixture = Fixture::new();
        let auth = fixture.acquire();
        let a = "5ebc5f9b-54b5-4928-9955-dc81c23743dd";
        let b = "4ebc5f9b-54b5-4928-9955-dc81c23743dd";
        let reference = |id: &str| {
            Box::new(ConversationReference::Claude(ClaudeConversation {
                conversation: id.into(),
                executable: "/bin/claude".into(),
                history: "/history".into(),
                config_dir: "/config".into(),
                options: vec![],
            }))
        };
        let mut binding = fixture.bind(&auth, None, a, "bind-a");
        let original = binding.clone();
        for (index, id) in [a, b, a].into_iter().enumerate() {
            if index != 0 {
                binding = fixture.bind(&auth, Some(binding), id, &format!("bind-{index}"));
            }
            assert!(matches!(
                fixture.command(
                    &auth,
                    &format!("retain-{index}"),
                    AgentCommand::RetainConversation {
                        binding: binding.clone(),
                        reference: reference(id)
                    }
                ),
                Response::AgentOperation(AgentOperationResult::ConversationRetained)
            ));
            let summary = fixture.state.session_summary(SessionId(900)).unwrap();
            let recovery = summary.recovery.unwrap();
            assert_eq!(recovery.conversation.as_deref(), Some(id));
            assert!(recovery.attached);
        }
        // A -> B -> A must not make the first A generation current again.
        assert!(matches!(
            fixture.command(
                &auth,
                "late-a",
                AgentCommand::RetainConversation {
                    binding: original,
                    reference: reference(a)
                }
            ),
            Response::Error { .. }
        ));
        assert!(matches!(
            fixture.command(
                &auth,
                "wrong-reference",
                AgentCommand::RetainConversation {
                    binding: binding.clone(),
                    reference: reference(b)
                }
            ),
            Response::Error { .. }
        ));
        assert!(matches!(
            fixture.command(
                &auth,
                "release",
                AgentCommand::Release {
                    expected_binding: Some(binding.clone())
                }
            ),
            Response::AgentOperation(AgentOperationResult::Released)
        ));
        assert!(matches!(
            fixture.command(
                &auth,
                "retired-invalidate",
                AgentCommand::InvalidateConversation
            ),
            Response::Error { .. }
        ));
        assert!(matches!(
            fixture.command(
                &auth,
                "retired-retain",
                AgentCommand::RetainConversation {
                    binding,
                    reference: reference(b)
                }
            ),
            Response::Error { .. }
        ));
        assert_eq!(
            fixture
                .state
                .session_summary(SessionId(900))
                .unwrap()
                .recovery
                .unwrap()
                .conversation
                .as_deref(),
            Some(a)
        );
    }

    struct Fixture {
        _cwd: tempfile::TempDir,
        session: Arc<Session>,
        pgid: libc::pid_t,
        owner: Arc<()>,
        state: Arc<ServerState>,
        dispatcher: Option<thread::JoinHandle<()>>,
        bridge: Option<thread::JoinHandle<()>>,
    }
    impl Fixture {
        fn new() -> Self {
            let (cwd, session, events) = spawn_live_test_session_with_hook(
                SessionId(900),
                Some(HookEnvironment {
                    socket: "/tmp/unused-agent-test.sock".into(),
                    session: SessionId(900),
                    capability: [7; 32],
                }),
            );
            let (state, commands) = test_state_with_dispatch(None, None);
            register_test_session(&state, SessionId(900), session.clone());
            let dispatch_state = state.clone();
            let dispatcher = thread::spawn(move || run_dispatcher(dispatch_state, commands));
            let dispatch = state.dispatch.clone();
            let bridge = thread::spawn(move || bridge_events(events, dispatch));
            let pgid = session.summary().pid.unwrap() as libc::pid_t;
            assert_eq!(unsafe { libc::getpgid(pgid) }, pgid);
            eprintln!("agent reporting fixture owned pid/pgid={pgid}");
            Self {
                _cwd: cwd,
                pgid,
                owner: Arc::new(()),
                session,
                state,
                dispatcher: Some(dispatcher),
                bridge: Some(bridge),
            }
        }
        fn request(&self, request: Request) -> Response {
            if matches!(request, Request::ReserveAgent(_)) {
                connections::handle_request_with_id(
                    &self.state,
                    &mut ClientRole::Control,
                    request,
                    1,
                    Some(&self.owner),
                )
            } else {
                self.state.handle_request(&mut ClientRole::Control, request)
            }
        }
        fn reserve(&self, epoch: u64, operation: &str) -> Request {
            Request::ReserveAgent(ReserveAgent {
                session: SessionId(900),
                capability: AgentSecret([7; 32]),
                operation: operation.into(),
                expected_epoch: epoch,
                invocation: format!("invocation-{epoch}"),
                provider: AgentProvider::Claude,
            })
        }
        fn acquire(&self) -> SupervisorAuth {
            let response = self.request(self.reserve(0, "reserve-first"));
            let Response::AgentOperation(AgentOperationResult::Reserved(reservation)) = response
            else {
                panic!("reserve failed: {response:?}");
            };
            assert_eq!(reservation.epoch, 1);
            SupervisorAuth {
                session: SessionId(900),
                lease: reservation.lease,
            }
        }
        fn command(
            &self,
            auth: &SupervisorAuth,
            operation: &str,
            command: AgentCommand,
        ) -> Response {
            self.request(Request::Supervisor(SupervisorRequest {
                auth: auth.clone(),
                operation: operation.into(),
                command,
            }))
        }
        fn bind(
            &self,
            auth: &SupervisorAuth,
            expected: Option<AgentBinding>,
            conversation: &str,
            operation: &str,
        ) -> AgentBinding {
            let response = self.command(
                auth,
                operation,
                AgentCommand::Bind {
                    expected_binding: expected,
                    conversation: conversation.into(),
                },
            );
            let Response::AgentOperation(AgentOperationResult::Bound(binding)) = response else {
                panic!("bind failed: {response:?}");
            };
            binding
        }
        fn report(
            &self,
            binding: &AgentBinding,
            revision: u64,
            observation: AgentObservation,
        ) -> Response {
            self.request(Request::AgentReport(AgentReport {
                session: SessionId(900),
                capability: [7; 32],
                sequence: None,
                update: AgentUpdate::Provider(ProviderReport {
                    binding: binding.clone(),
                    revision,
                    observation,
                }),
            }))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Err(error) = self.session.terminate(Duration::from_millis(100)) {
                assert!(
                    error.is::<crate::session::AlreadyExited>(),
                    "agent reporting fixture terminate: {error:#}"
                );
            }

            self.bridge.take().unwrap().join().unwrap();
            self.state.dispatch.send(DispatchMessage::Stop).unwrap();
            self.dispatcher.take().unwrap().join().unwrap();
            assert!(wait_test_group_absent(self.pgid, Duration::from_secs(2)));
            eprintln!("agent reporting fixture cleaned pgid={}", self.pgid);
        }
    }
    fn activity(state: AgentActivity) -> AgentObservation {
        AgentObservation::Activity(ActivitySample {
            state,
            quality: SampleQuality::Observed,
            turn: None,
        })
    }
    fn measurement<T>(value: T) -> Measurement<T> {
        Measurement {
            value,
            source: "fixture".into(),
        }
    }
    fn metrics() -> MetricsSample {
        MetricsSample {
            model: Some("test".into()),
            context: measurement(ContextSample {
                used_tokens: Some(10),
                capacity_tokens: Some(100),
                quality: SampleQuality::Confirmed,
            }),
            usage: measurement(UsageTotals {
                scope: UsageScope::Conversation,
                coverage: UsageCoverage::Complete,
                input_tokens: Some(20),
                output_tokens: Some(30),
                cache_read_tokens: Some(2),
                cache_write_tokens: Some(3),
                reasoning_output_tokens: Some(4),
            }),
            cost: measurement(Some(UsageCost {
                usd_ticks: 100,
                kind: CostKind::Reported,
                scope: UsageScope::Conversation,
            })),
        }
    }
    fn rejected(response: Response) {
        assert!(
            matches!(response, Response::Error { .. }),
            "expected rejection: {response:?}"
        );
    }

    #[test]
    fn agent_report_reservation_retry_and_stale_compare_exchange() {
        let f = Fixture::new();
        let reserve = f.reserve(0, "reserve-first");
        let first = f.request(reserve.clone());
        assert!(
            matches!(
                first,
                Response::AgentOperation(AgentOperationResult::Reserved(_))
            ),
            "{first:?}"
        );
        assert_eq!(
            f.request(reserve),
            first,
            "lost reservation response must recover same lease"
        );
        rejected(f.request(f.reserve(0, "delayed-reserve")));
        rejected(f.request(f.reserve(1, "active-replacement")));
        assert_eq!(f.session.summary().agent_epoch, 1);
        let Response::AgentOperation(AgentOperationResult::Reserved(r)) = first else {
            unreachable!()
        };
        let auth = SupervisorAuth {
            session: SessionId(900),
            lease: r.lease,
        };
        let a = f.bind(&auth, None, "A", "bind-a");
        let duplicate = f.command(
            &auth,
            "bind-a",
            AgentCommand::Bind {
                expected_binding: None,
                conversation: "A".into(),
            },
        );
        assert_eq!(
            duplicate,
            Response::AgentOperation(AgentOperationResult::Bound(a.clone()))
        );
        let b = f.bind(&auth, Some(a.clone()), "B", "bind-b");
        let a2 = f.bind(&auth, Some(b), "A", "bind-a2");
        assert!(a2.generation > a.generation);
        rejected(f.command(
            &auth,
            "late-bind",
            AgentCommand::Bind {
                expected_binding: Some(a.clone()),
                conversation: "C".into(),
            },
        ));
        rejected(f.command(
            &auth,
            "late-release",
            AgentCommand::Release {
                expected_binding: Some(a.clone()),
            },
        ));
        rejected(f.report(&a, 99, activity(AgentActivity::Idle)));
        assert_eq!(f.session.summary().agent.unwrap().binding, a2);
    }

    #[test]
    fn agent_report_independent_streams_atomic_components_and_authentication() {
        let f = Fixture::new();
        let auth = f.acquire();
        let a = f.bind(&auth, None, "A", "bind");
        assert_eq!(f.report(&a, 8, activity(AgentActivity::Busy)), Response::Ok);
        assert_eq!(
            f.report(&a, 20, AgentObservation::Metrics(metrics().into())),
            Response::Ok
        );
        assert_eq!(
            f.report(&a, 9, activity(AgentActivity::WaitingInput)),
            Response::Ok
        );
        let before = f.session.summary();
        rejected(f.report(&a, 19, AgentObservation::Metrics(metrics().into())));
        rejected(f.request(Request::AgentReport(AgentReport {
            session: SessionId(900),
            capability: [8; 32],
            sequence: None,
            update: AgentUpdate::Provider(ProviderReport {
                binding: a.clone(),
                revision: 100,
                observation: activity(AgentActivity::Idle),
            }),
        })));
        rejected(f.request(Request::AgentReport(AgentReport {
            session: SessionId(901),
            capability: [7; 32],
            sequence: None,
            update: AgentUpdate::Activity(AgentActivity::Idle),
        })));
        rejected(f.request(Request::AgentReport(AgentReport {
            session: SessionId(900),
            capability: [7; 32],
            sequence: None,
            update: AgentUpdate::Activity(AgentActivity::Idle),
        })));
        assert_eq!(f.session.summary(), before);
        let mut next = metrics();
        next.usage.value.input_tokens = Some(22);
        assert_eq!(
            f.report(&a, 21, AgentObservation::Metrics(next.clone().into())),
            Response::Ok
        );
        let new = f.session.summary().agent.unwrap().metrics.unwrap();
        let old = before.agent.unwrap().metrics.unwrap();
        // Only the component that changed takes the new receipt stamp.
        assert_eq!(new.context_received_unix_ms, old.context_received_unix_ms);
        assert_eq!(new.cost_received_unix_ms, old.cost_received_unix_ms);
        assert_eq!(new.usage_received_unix_ms, new.received_unix_ms);
        // A replay of the same sample advances nothing at all.
        assert_eq!(
            f.report(&a, 22, AgentObservation::Metrics(next.clone().into())),
            Response::Ok
        );
        let before = f.session.summary();
        let replayed = before.agent.clone().unwrap().metrics.unwrap();
        assert_eq!(replayed.usage_received_unix_ms, new.usage_received_unix_ms);
        assert_eq!(
            replayed.context_received_unix_ms,
            new.context_received_unix_ms
        );
        next.usage.value.input_tokens = Some(21);
        rejected(f.report(&a, 23, AgentObservation::Metrics(next.clone().into())));
        assert_eq!(
            f.session.summary(),
            before,
            "a rejected snapshot must not move any component, receipt stamp or revision"
        );
        next.usage.value.input_tokens = Some(22);
        next.context.value.used_tokens = None;
        next.cost.value = None;
        assert_eq!(
            f.report(&a, 23, AgentObservation::Metrics(next.into())),
            Response::Ok
        );
        assert_eq!(
            f.session
                .summary()
                .context_usage
                .unwrap()
                .report
                .used_tokens,
            None
        );
        assert_eq!(
            f.session
                .summary()
                .agent
                .unwrap()
                .metrics
                .unwrap()
                .sample
                .cost
                .value,
            None
        );
    }
    fn supervisor_connection(
        f: &Fixture,
        auth: &SupervisorAuth,
    ) -> (UnixStream, thread::JoinHandle<()>) {
        let (server, mut client) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let state = f.state.clone();
        let handle = thread::spawn(move || connections::handle_connection(state, server));
        exchange_preamble(&mut client).unwrap();
        write_frame(
            &mut client,
            &ClientMessage {
                request_id: 1,
                request: Request::SupervisorHello(auth.clone()),
            },
        )
        .unwrap();
        let response = read_frame::<ServerMessage>(&mut client).unwrap();
        assert_eq!(
            response,
            ServerMessage::Response {
                request_id: 1,
                response: Response::Ok
            }
        );
        (client, handle)
    }

    #[test]
    fn response_ready_socket_snapshot_reconnect_and_revision_order() {
        let f = Fixture::new();
        let auth = f.acquire();
        let binding = f.bind(&auth, None, "A", "bind");
        fn connect(f: &Fixture) -> (UnixStream, thread::JoinHandle<()>) {
            let (server, mut client) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let state = f.state.clone();
            let handle = thread::spawn(move || connections::handle_connection(state, server));
            exchange_preamble(&mut client).unwrap();
            assert!(matches!(
                request(&mut client, Request::DashboardHello),
                Response::Hierarchy(_)
            ));
            (client, handle)
        }
        fn request(client: &mut UnixStream, request: Request) -> Response {
            write_frame(
                client,
                &ClientMessage {
                    request_id: 7,
                    request,
                },
            )
            .unwrap();
            loop {
                match read_frame(client).unwrap() {
                    ServerMessage::Response {
                        request_id: 7,
                        response,
                    } => return response,
                    ServerMessage::Event(_) => {}
                    other => panic!("unexpected socket message: {other:?}"),
                }
            }
        }
        fn snapshot(client: &mut UnixStream) -> AgentSnapshot {
            let Response::Inventory { sessions, .. } = request(client, Request::Inspect) else {
                panic!("expected inventory");
            };
            sessions
                .into_iter()
                .find(|s| s.id == SessionId(900))
                .unwrap()
                .agent
                .unwrap()
        }
        let report = |revision, state, turn: &str| {
            Request::AgentReport(AgentReport {
                session: SessionId(900),
                capability: [7; 32],
                sequence: None,
                update: AgentUpdate::Provider(ProviderReport {
                    binding: binding.clone(),
                    revision,
                    observation: AgentObservation::Activity(ActivitySample {
                        state,
                        quality: SampleQuality::Observed,
                        turn: Some(turn.into()),
                    }),
                }),
            })
        };
        let ready: AgentActivity = serde_json::from_str("\"ResponseReady\"").unwrap();
        let (mut client, handler) = connect(&f);
        assert_eq!(
            request(&mut client, report(2, ready, "turn-1")),
            Response::Ok
        );
        let expected = snapshot(&mut client);
        assert_eq!(
            expected.activity.as_ref().unwrap(),
            &ActivitySample {
                state: ready,
                quality: SampleQuality::Observed,
                turn: Some("turn-1".into()),
            }
        );
        assert_eq!(expected.activity_revision, 2);
        assert!(expected.metrics.is_none());
        rejected(request(
            &mut client,
            report(1, AgentActivity::Busy, "stale"),
        ));
        rejected(request(
            &mut client,
            report(2, AgentActivity::Idle, "duplicate"),
        ));
        assert_eq!(snapshot(&mut client), expected);
        drop(client);
        handler.join().unwrap();
        let (mut client, handler) = connect(&f);
        assert_eq!(
            snapshot(&mut client),
            expected,
            "reconnect retains the last observed turn"
        );
        assert_eq!(
            request(&mut client, report(3, AgentActivity::Busy, "turn-2")),
            Response::Ok
        );
        let next = snapshot(&mut client);
        assert_eq!(next.activity_revision, 3);
        assert_eq!(
            next.activity.unwrap(),
            ActivitySample {
                state: AgentActivity::Busy,
                quality: SampleQuality::Observed,
                turn: Some("turn-2".into()),
            }
        );
        assert!(next.metrics.is_none());
        drop(client);
        handler.join().unwrap();
    }

    #[test]
    fn codex_unread_socket_acknowledges_only_expected_ready_and_survives_reconnect() {
        let f = Fixture::new();
        let mut reserve = f.reserve(0, "codex-unread");
        if let Request::ReserveAgent(r) = &mut reserve {
            r.provider = AgentProvider::Codex;
        }
        let Response::AgentOperation(AgentOperationResult::Reserved(reservation)) =
            f.request(reserve)
        else {
            panic!("reservation failed");
        };
        let auth = SupervisorAuth {
            session: SessionId(900),
            lease: reservation.lease,
        };
        let binding = f.bind(&auth, None, "A", "bind");
        fn request(client: &mut UnixStream, request: Request) -> Response {
            write_frame(
                client,
                &ClientMessage {
                    request_id: 61,
                    request,
                },
            )
            .unwrap();
            loop {
                if let ServerMessage::Response {
                    request_id: 61,
                    response,
                } = read_frame(client).unwrap()
                {
                    return response;
                }
            }
        }
        fn connect(f: &Fixture) -> (UnixStream, thread::JoinHandle<()>) {
            let (server, mut client) = UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let state = f.state.clone();
            let handle = thread::spawn(move || connections::handle_connection(state, server));
            exchange_preamble(&mut client).unwrap();
            assert!(matches!(
                request(&mut client, Request::DashboardHello),
                Response::Hierarchy(_)
            ));
            (client, handle)
        }
        fn summary(client: &mut UnixStream) -> serde_json::Value {
            let Response::Inventory { sessions, .. } = request(client, Request::Inspect) else {
                panic!("expected inventory");
            };
            serde_json::to_value(sessions.iter().find(|s| s.id == SessionId(900)).unwrap()).unwrap()
        }
        fn mark(expected: &serde_json::Value) -> Request {
            serde_json::from_value(
                serde_json::json!({"MarkReviewed": {"session": 900, "expected": expected}}),
            )
            .unwrap()
        }
        let report = |binding: &AgentBinding, revision, state, turn: &str| {
            Request::AgentReport(AgentReport {
                session: SessionId(900),
                capability: [7; 32],
                sequence: None,
                update: AgentUpdate::Provider(ProviderReport {
                    binding: binding.clone(),
                    revision,
                    observation: AgentObservation::Activity(ActivitySample {
                        state,
                        quality: SampleQuality::Observed,
                        turn: Some(turn.into()),
                    }),
                }),
            })
        };
        let (mut client, handler) = connect(&f);
        assert_eq!(
            request(
                &mut client,
                report(&binding, 1, AgentActivity::ResponseReady, "one")
            ),
            Response::Ok
        );
        let first = summary(&mut client);
        assert_eq!(
            first["unread"]["turn"], "one",
            "accepted Codex Ready must become unread"
        );
        assert_eq!(
            first["unread"]["binding"],
            serde_json::to_value(&binding).unwrap()
        );
        assert_eq!(first["unread"]["activity_revision"], 1);
        // Viewing and a newer Busy observation must not acknowledge the previous result.
        assert!(matches!(
            request(
                &mut client,
                Request::ReadTerminal {
                    session: SessionId(900),
                    max_lines: Some(2)
                }
            ),
            Response::TerminalText { .. }
        ));
        assert_eq!(
            request(&mut client, report(&binding, 2, AgentActivity::Busy, "two")),
            Response::Ok
        );
        assert_eq!(summary(&mut client)["unread"], first["unread"]);
        drop(client);
        handler.join().unwrap();
        let (mut client, handler) = connect(&f);
        assert_eq!(summary(&mut client)["unread"], first["unread"]);
        // Deterministic race: newer Ready wins the dispatch order before an old acknowledgement.
        assert_eq!(
            request(
                &mut client,
                report(&binding, 3, AgentActivity::ResponseReady, "two")
            ),
            Response::Ok
        );
        let second = summary(&mut client);
        rejected(request(&mut client, mark(&first["unread"])));
        assert_eq!(
            summary(&mut client),
            second,
            "stale acknowledgement changes no state"
        );
        for pointer in [
            "/activity_revision",
            "/turn",
            "/binding/generation",
            "/binding/invocation",
            "/binding/conversation",
        ] {
            let mut wrong = second["unread"].clone();
            let target = wrong.pointer_mut(pointer).unwrap();
            *target = if target.is_number() {
                serde_json::json!(99)
            } else {
                serde_json::json!("wrong")
            };
            rejected(request(&mut client, mark(&wrong)));
            assert_eq!(
                summary(&mut client),
                second,
                "{pointer} is part of the acknowledgement target"
            );
        }
        assert_eq!(request(&mut client, mark(&second["unread"])), Response::Ok);
        let reviewed = summary(&mut client);
        let mut expected = second.clone();
        expected["unread"] = serde_json::Value::Null;
        assert_eq!(reviewed, expected, "acknowledgement changes only unread");
        assert_eq!(request(&mut client, mark(&second["unread"])), Response::Ok);
        assert_eq!(
            summary(&mut client),
            reviewed,
            "duplicate acknowledgement is harmless"
        );
        rejected(request(
            &mut client,
            report(&binding, 3, AgentActivity::ResponseReady, "two"),
        ));
        assert_eq!(
            request(
                &mut client,
                report(&binding, 4, AgentActivity::ResponseReady, "two")
            ),
            Response::Ok
        );
        assert!(
            summary(&mut client)["unread"].is_null(),
            "duplicate Ready at a higher revision cannot reopen reviewed turn"
        );
        assert_eq!(
            request(
                &mut client,
                report(&binding, 5, AgentActivity::ResponseReady, "three")
            ),
            Response::Ok
        );
        let unread = summary(&mut client)["unread"].clone();
        // Rebinding changes native reporting identity without acknowledging the previous turn.
        let rebound = f.bind(&auth, Some(binding), "B", "rebind");
        assert_eq!(summary(&mut client)["unread"], unread);
        assert_eq!(
            f.command(
                &auth,
                "release",
                AgentCommand::Release {
                    expected_binding: Some(rebound)
                }
            ),
            Response::AgentOperation(AgentOperationResult::Released)
        );
        let lost = summary(&mut client);
        assert_eq!(lost["unread"], unread);
        assert_eq!(lost["agent"]["health"]["state"], "Unavailable");
        assert_eq!(
            request(
                &mut client,
                Request::AgentReport(AgentReport {
                    session: SessionId(900),
                    capability: [7; 32],
                    sequence: None,
                    update: AgentUpdate::Activity(AgentActivity::Idle),
                })
            ),
            Response::Ok
        );
        let legacy = summary(&mut client);
        assert!(
            legacy["agent"].is_null(),
            "legacy shell reporting clears the old agent snapshot"
        );
        assert_eq!(legacy["unread"], unread);
        let reserved = f.request(f.reserve(1, "next-reservation"));
        assert!(matches!(
            reserved,
            Response::AgentOperation(AgentOperationResult::Reserved(_))
        ));
        let reserving = summary(&mut client);
        assert!(reserving["agent"].is_null());
        assert_eq!(
            reserving["unread"], unread,
            "reserving another invocation does not review a response"
        );
        assert_eq!(request(&mut client, mark(&unread)), Response::Ok);
        let mut expected = reserving;
        expected["unread"] = serde_json::Value::Null;
        assert_eq!(
            summary(&mut client),
            expected,
            "review remains available after reporter loss"
        );
        drop(client);
        handler.join().unwrap();
    }

    #[test]
    fn agent_report_supervisor_connection_loss_and_late_cleanup() {
        let f = Fixture::new();
        let auth = f.acquire();
        let a = f.bind(&auth, None, "A", "bind");
        assert_eq!(f.report(&a, 1, activity(AgentActivity::Busy)), Response::Ok);
        assert_eq!(
            f.report(&a, 1, AgentObservation::Metrics(metrics().into())),
            Response::Ok
        );
        let (old, old_handler) = supervisor_connection(&f, &auth);
        let before = f.session.summary();
        let (new, new_handler) = supervisor_connection(&f, &auth);
        assert_eq!(
            f.session.summary(),
            before,
            "attachment cannot reset activity/metrics"
        );
        drop(old);
        old_handler.join().unwrap();
        // Join + FIFO dispatcher status gives a deterministic cleanup barrier.
        f.request(Request::AgentStatus {
            auth: auth.clone(),
            operation: "bind".into(),
        });
        assert_eq!(
            f.session.summary(),
            before,
            "old supervisor closure cannot affect replacement connection"
        );
        // Ordinary one-shot report transport closes without changing health.
        let (server, mut report) = UnixStream::pair().unwrap();
        let state = f.state.clone();
        let handler = thread::spawn(move || connections::handle_connection(state, server));
        exchange_preamble(&mut report).unwrap();
        write_frame(
            &mut report,
            &ClientMessage {
                request_id: 2,
                request: Request::AgentReport(AgentReport {
                    session: SessionId(900),
                    capability: [7; 32],
                    sequence: None,
                    update: AgentUpdate::Provider(ProviderReport {
                        binding: a.clone(),
                        revision: 2,
                        observation: activity(AgentActivity::WaitingInput),
                    }),
                }),
            },
        )
        .unwrap();
        assert_eq!(
            read_frame::<ServerMessage>(&mut report).unwrap(),
            ServerMessage::Response {
                request_id: 2,
                response: Response::Ok
            }
        );
        handler.join().unwrap();
        assert_eq!(
            f.session.summary().agent.unwrap().health.state,
            ReporterHealth::Connected
        );
        drop(new);
        new_handler.join().unwrap();
        f.request(Request::AgentStatus {
            auth: auth.clone(),
            operation: "bind".into(),
        });
        let lost = f.session.summary().agent.unwrap();
        assert_eq!(lost.health.state, ReporterHealth::Unavailable);
        assert_eq!(
            lost.health.reason.as_deref(),
            Some("supervisor_disconnected")
        );
        assert_eq!(lost.activity.unwrap().state, AgentActivity::WaitingInput);
        assert_eq!(
            lost.metrics.unwrap().sample.usage.value.coverage,
            UsageCoverage::Partial
        );
        rejected(f.report(&a, 3, activity(AgentActivity::Idle)));
        let response = f.request(f.reserve(1, "reserve-second"));
        assert!(matches!(
            response,
            Response::AgentOperation(AgentOperationResult::Reserved(_))
        ));
        rejected(f.command(
            &auth,
            "old-cleanup",
            AgentCommand::Release {
                expected_binding: Some(a),
            },
        ));
        assert_eq!(f.session.summary().agent_epoch, 2);
    }

    #[test]
    fn agent_report_finalize_atomic_retry_and_exit_rejection() {
        let f = Fixture::new();
        let auth = f.acquire();
        let a = f.bind(&auth, None, "A", "bind");
        assert_eq!(
            f.report(&a, 1, AgentObservation::Metrics(metrics().into())),
            Response::Ok
        );
        let mut invalid = metrics();
        invalid.usage.value.input_tokens = Some(0);
        let before = f.session.summary();
        rejected(f.command(
            &auth,
            "final",
            AgentCommand::Finalize {
                binding: a.clone(),
                revision: 2,
                final_metrics: invalid.into(),
            },
        ));
        assert_eq!(f.session.summary(), before);
        let command = AgentCommand::Finalize {
            binding: a.clone(),
            revision: 2,
            final_metrics: metrics().into(),
        };
        let result = f.command(&auth, "final", command.clone());
        assert_eq!(
            result,
            Response::AgentOperation(AgentOperationResult::Released)
        );
        assert_eq!(f.command(&auth, "final", command), result);
        assert_eq!(
            f.request(Request::AgentStatus {
                auth: auth.clone(),
                operation: "final".into()
            }),
            result
        );
        assert_eq!(
            f.session
                .summary()
                .agent
                .unwrap()
                .metrics
                .unwrap()
                .sample
                .usage
                .value
                .coverage,
            UsageCoverage::Complete
        );
        rejected(f.report(&a, 3, activity(AgentActivity::Idle)));
        let response = f.request(f.reserve(1, "reserve-second"));
        let Response::AgentOperation(AgentOperationResult::Reserved(r)) = response else {
            panic!("{response:?}");
        };
        let new_auth = SupervisorAuth {
            session: SessionId(900),
            lease: r.lease,
        };
        let new_a = f.bind(&new_auth, None, "A", "new-bind");
        assert!(new_a.generation > a.generation);
        rejected(f.request(Request::AgentStatus {
            auth,
            operation: "final".into(),
        }));
        assert_eq!(
            f.report(&new_a, 1, activity(AgentActivity::Busy)),
            Response::Ok
        );
        assert_eq!(
            f.report(&new_a, 1, AgentObservation::Metrics(metrics().into())),
            Response::Ok
        );
        f.session.terminate(Duration::from_millis(100)).unwrap();
        let exited = f.session.summary();
        assert!(matches!(exited.phase, SessionPhase::Exited { .. }));
        assert_eq!(
            exited
                .agent
                .as_ref()
                .unwrap()
                .metrics
                .as_ref()
                .unwrap()
                .sample
                .usage
                .value
                .coverage,
            UsageCoverage::Partial
        );
        rejected(f.report(&new_a, 2, activity(AgentActivity::Idle)));
        assert_eq!(f.session.summary(), exited);
    }

    #[test]
    fn agent_report_rejects_decreases_after_unknown_and_collector_loss() {
        let f = Fixture::new();
        let auth = f.acquire();
        let a = f.bind(&auth, None, "A", "bind");
        assert_eq!(
            f.report(&a, 1, AgentObservation::Metrics(metrics().into())),
            Response::Ok
        );
        let mut unknown = metrics();
        unknown.usage.value.input_tokens = None;
        assert_eq!(
            f.report(&a, 2, AgentObservation::Metrics(unknown.clone().into())),
            Response::Ok
        );
        unknown.usage.value.input_tokens = Some(19);
        let before = f.session.summary();
        rejected(f.report(&a, 3, AgentObservation::Metrics(unknown.into())));
        assert_eq!(f.session.summary(), before);
        let health = ProviderReport {
            binding: a.clone(),
            revision: 1,
            observation: AgentObservation::Health(HealthSample {
                state: ReporterHealth::Unavailable,
                reason: Some("collector_lost".into()),
            }),
        };
        rejected(f.report(&a, 1, health.observation.clone()));
        assert_eq!(
            f.command(&auth, "lost", AgentCommand::Health(health)),
            Response::AgentOperation(AgentOperationResult::HealthUpdated)
        );
        let latest = metrics();
        assert_eq!(
            f.report(&a, 3, AgentObservation::Metrics(latest.into())),
            Response::Ok
        );
        assert_eq!(
            f.session
                .summary()
                .agent
                .unwrap()
                .metrics
                .unwrap()
                .sample
                .usage
                .value
                .coverage,
            UsageCoverage::Partial,
            "report replay cannot repair collector loss"
        );
    }

    #[test]
    fn agent_report_lost_reservation_delivery_and_delayed_reservation_do_not_allocate() {
        let f = Fixture::new();
        let original = f.reserve(0, "lost-reservation");
        let (completion, receiver) = mpsc::sync_channel(1);
        drop(receiver);
        f.state
            .dispatch
            .send(DispatchMessage::AgentCommand {
                request: original.clone(),
                owner: Some(f.owner.clone()),
                completion,
            })
            .unwrap();
        // The following request is processed after the committed but undelivered reservation.
        let response = f.request(original.clone());
        let Response::AgentOperation(AgentOperationResult::Reserved(r)) = &response else {
            panic!("{response:?}");
        };
        assert_eq!(r.epoch, 1);
        assert_eq!(f.request(original), response);
        let mut changed = f.reserve(0, "lost-reservation");
        if let Request::ReserveAgent(r) = &mut changed {
            r.invocation = "different".into();
        }
        rejected(f.request(changed));
        let auth = SupervisorAuth {
            session: SessionId(900),
            lease: r.lease.clone(),
        };
        assert_eq!(
            f.command(
                &auth,
                "release",
                AgentCommand::Release {
                    expected_binding: None
                }
            ),
            Response::AgentOperation(AgentOperationResult::Released)
        );
        rejected(f.request(f.reserve(0, "delayed-original-epoch")));
        assert_eq!(f.session.summary().agent_epoch, 1);
        assert!(matches!(
            f.request(f.reserve(1, "next-invocation")),
            Response::AgentOperation(AgentOperationResult::Reserved(AgentReservation {
                epoch: 2,
                ..
            }))
        ));
        rejected(f.command(
            &auth,
            "release",
            AgentCommand::Release {
                expected_binding: None,
            },
        ));
        assert_eq!(f.session.summary().agent_epoch, 2);
    }
    #[test]
    fn agent_report_reservation_connection_loss_releases_unbound_slot() {
        let f = Fixture::new();
        let original = f.reserve(0, "orphan-reservation");
        let (server, mut client) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let state = f.state.clone();
        let handler = thread::spawn(move || connections::handle_connection(state, server));
        exchange_preamble(&mut client).unwrap();
        write_frame(
            &mut client,
            &ClientMessage {
                request_id: 1,
                request: original.clone(),
            },
        )
        .unwrap();
        let response = read_frame::<ServerMessage>(&mut client).unwrap();
        assert!(matches!(
            response,
            ServerMessage::Response {
                response: Response::AgentOperation(AgentOperationResult::Reserved(_)),
                ..
            }
        ));
        let (new_server, mut new_client) = UnixStream::pair().unwrap();
        new_client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let state = f.state.clone();
        let new_handler = thread::spawn(move || connections::handle_connection(state, new_server));
        exchange_preamble(&mut new_client).unwrap();
        write_frame(
            &mut new_client,
            &ClientMessage {
                request_id: 1,
                request: original.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            read_frame::<ServerMessage>(&mut new_client).unwrap(),
            response
        );
        drop(client);
        handler.join().unwrap();
        // Late close of the original socket must leave the reattached reservation live.
        write_frame(
            &mut new_client,
            &ClientMessage {
                request_id: 1,
                request: original.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            read_frame::<ServerMessage>(&mut new_client).unwrap(),
            response
        );
        drop(new_client);
        new_handler.join().unwrap();
        assert_eq!(
            f.request(original),
            Response::AgentOperation(AgentOperationResult::Released),
            "lost reservation connection must release even before bind/Hello"
        );
        assert_eq!(f.session.summary().agent_epoch, 1);
        assert!(matches!(
            f.request(f.reserve(1, "replacement-reservation")),
            Response::AgentOperation(AgentOperationResult::Reserved(AgentReservation {
                epoch: 2,
                ..
            }))
        ));
    }
    #[test]
    fn agent_report_legacy_remains_available_until_binding() {
        let f = Fixture::new();
        let legacy = |activity| {
            Request::AgentReport(AgentReport {
                session: SessionId(900),
                capability: [7; 32],
                sequence: None,
                update: AgentUpdate::Activity(activity),
            })
        };
        assert_eq!(f.request(legacy(AgentActivity::Busy)), Response::Ok);
        assert_eq!(
            f.request(Request::AgentReport(AgentReport {
                session: SessionId(900),
                capability: [7; 32],
                sequence: None,
                update: AgentUpdate::Context(ovrcr_protocol::context::ContextUsageReport {
                    source: ovrcr_protocol::context::ContextSource::Generic,
                    model: None,
                    conversation: None,
                    used_tokens: Some(10),
                    capacity_tokens: Some(100)
                })
            })),
            Response::Ok
        );
        let context_before = f.session.summary().context_usage;
        let auth = f.acquire();
        assert_eq!(f.session.summary().context_usage, context_before);
        assert_eq!(
            f.session.summary().activity,
            AgentActivity::Busy,
            "reservation is not a new conversation binding"
        );
        assert_eq!(f.request(legacy(AgentActivity::WaitingInput)), Response::Ok);
        let a = f.bind(&auth, None, "A", "bind");
        assert!(
            f.session.summary().context_usage.is_none(),
            "new binding becomes the sole context authority"
        );
        rejected(f.request(legacy(AgentActivity::Idle)));
        assert_eq!(
            f.command(
                &auth,
                "release",
                AgentCommand::Release {
                    expected_binding: Some(a)
                }
            ),
            Response::AgentOperation(AgentOperationResult::Released)
        );
        let sink = DashboardSink::new();
        let (stream, _peer) = UnixStream::pair().unwrap();
        f.state
            .dashboard
            .claim_with_identity(sink.clone(), Arc::new(()), stream);
        assert_eq!(f.request(legacy(AgentActivity::WaitingInput)), Response::Ok);
        assert_eq!(f.session.summary().activity, AgentActivity::WaitingInput);
        assert!(sink.queue.lock().unwrap().messages.iter().any(|outbound| matches!(&outbound.message, ServerMessage::Event(ServerEvent::SessionChanged(summary)) if summary.activity == AgentActivity::WaitingInput)), "legacy takeover must publish even when the old legacy value matches");
    }
}

fn register_test_session(
    state: &ServerState,
    id: SessionId,
    session: Arc<Session>,
) -> Option<Arc<Session>> {
    assert_eq!(id, session.id());
    state.retained.lock().register_fixture(&session).unwrap();
    state.sessions.lock().unwrap().insert(id, session)
}

fn extend_test_sessions(
    state: &ServerState,
    sessions: impl IntoIterator<Item = (SessionId, Arc<Session>)>,
) {
    for (id, session) in sessions {
        register_test_session(state, id, session);
    }
}

fn remove_test_session(state: &ServerState, id: &SessionId) -> Option<Arc<Session>> {
    {
        let mut retained = state.retained.lock();
        if let Some(run) = retained.get(*id).map(|record| record.run) {
            retained.remove(*id, run).unwrap();
        }
    }
    state.sessions.lock().unwrap().remove(id)
}

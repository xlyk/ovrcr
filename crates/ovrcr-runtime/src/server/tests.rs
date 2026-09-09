use super::connections::lifecycle_response_with_partial_hierarchy;
use super::dispatch::resize_view_targets;
use super::startup::resolve_bound_socket;
use super::*;
use ovrcr_protocol::AgentReport;
use ovrcr_protocol::exchange_preamble;
use std::time::Instant;

use crate::session::AgentActivity;
use ovrcr_protocol::{
    DashboardView, HISTORY_ROWS, HistorySnapshotId, PAGE_COLS, PAGE_ROWS, PaneTarget, Response,
    ServerEvent, ServerMessage,
};
use ovrcr_terminal::history::FrozenHistory;
use std::io::Read;
use std::net::Shutdown;

#[test]
fn raw_event_and_dispatch_queues_reject_the_65th_item() {
    let (event_sender, _event_receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    for _ in 0..RAW_EVENT_QUEUE_CAPACITY {
        event_sender
            .try_send(SessionEvent::Output {
                id: SessionId(1),
                bytes: Vec::new(),
            })
            .unwrap();
    }
    assert!(matches!(
        event_sender.try_send(SessionEvent::Output {
            id: SessionId(1),
            bytes: Vec::new(),
        }),
        Err(mpsc::TrySendError::Full(_))
    ));

    let (dispatch_sender, _dispatch_receiver) = mpsc::sync_channel(RAW_DISPATCH_QUEUE_CAPACITY);
    for _ in 0..RAW_DISPATCH_QUEUE_CAPACITY {
        dispatch_sender.try_send(DispatchMessage::Stop).unwrap();
    }
    assert!(matches!(
        dispatch_sender.try_send(DispatchMessage::Stop),
        Err(mpsc::TrySendError::Full(_))
    ));
}

#[test]
fn split_delivery_snapshots_precede_increments() {
    let sink = DashboardSink::new();
    let left = SessionId(1);
    let right = SessionId(2);
    for session in [left, right] {
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                session,
                revision: 1,
                bytes: b"old".to_vec(),
            }),
            completion: None,
        }));
    }
    let view = DashboardView {
        revision: 2,
        panes: vec![
            PaneTarget {
                session: left,
                size: TerminalSize { rows: 36, cols: 39 },
            },
            PaneTarget {
                session: right,
                size: TerminalSize { rows: 36, cols: 40 },
            },
        ],
        focused: Some(right),
    };
    assert!(sink.replace_view(&view, 10, vec![b"LEFT".to_vec(), b"RIGHT".to_vec()]));
    assert!(sink.enqueue_queued(DashboardOutbound {
        message: ServerMessage::Event(ServerEvent::Output {
            session: right,
            revision: 2,
            bytes: b"new".to_vec(),
        }),
        completion: None,
    }));
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
                session,
                revision: 2,
                bytes,
            }),
            ..
        })) if session == right && bytes == b"new"
    ));

    let sink = DashboardSink::new();
    for _ in 0..62 {
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 1,
                response: Response::Ok,
            },
            completion: None,
        }));
    }
    assert!(!sink.replace_view(&view, 11, vec![b"LEFT".to_vec(), b"RIGHT".to_vec()]));
    assert!(sink.next().is_none());
}

#[test]
fn split_delivery_dirty_revisions_are_isolated() {
    let sink = DashboardSink::new();
    let first = SessionId(1);
    let second = SessionId(2);
    for _ in 0..(DASHBOARD_QUEUE - 3) {
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 1,
                response: Response::Ok,
            },
            completion: None,
        }));
    }
    for (session, bytes) in [
        (first, b"first".as_slice()),
        (second, b"second".as_slice()),
        (SessionId(3), b"interleaved".as_slice()),
    ] {
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                session,
                revision: 1,
                bytes: bytes.to_vec(),
            }),
            completion: None,
        }));
    }
    assert!(sink.enqueue_queued(DashboardOutbound {
        message: ServerMessage::Event(ServerEvent::Output {
            session: first,
            revision: 1,
            bytes: b"first-replaced".to_vec(),
        }),
        completion: None,
    }));
    assert!(sink.enqueue_queued(DashboardOutbound {
        message: ServerMessage::Event(ServerEvent::Output {
            session: second,
            revision: 1,
            bytes: b"second-replaced".to_vec(),
        }),
        completion: None,
    }));
    assert!(sink.enqueue_queued(DashboardOutbound {
        message: ServerMessage::Event(ServerEvent::Output {
            session: second,
            revision: 1,
            bytes: b"second-dirty".to_vec(),
        }),
        completion: None,
    }));
    for _ in 0..2 {
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 3,
                response: Response::Ok,
            },
            completion: None,
        }));
    }
    let view = DashboardView {
        revision: 2,
        panes: vec![
            PaneTarget {
                session: first,
                size: TerminalSize { rows: 24, cols: 80 },
            },
            PaneTarget {
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
        let Some(DashboardDelivery::Dirty { revision, session }) = sink.next() else {
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
        sink.dirty_sent(revision, session);
    }
    for _ in 0..(DASHBOARD_QUEUE - 3) {
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Response {
                request_id: 2,
                response: Response::Ok,
            },
            completion: None,
        }));
    }
    for session in [first, second] {
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                session,
                revision: 2,
                bytes: b"new".to_vec(),
            }),
            completion: None,
        }));
    }
    assert!(
        sink.dirty_keys()
            .into_iter()
            .all(|(revision, session)| revision == 2 && [first, second].contains(&session))
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
        let Some(DashboardDelivery::Dirty { revision, session }) = sink.next() else {
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
    state.sessions.lock().unwrap().insert(id, session.clone());
    let (new_server, _new_client) = UnixStream::pair().unwrap();
    let new_sink = DashboardSink::new();
    let new_owner = Arc::new(());
    *state.dashboard_slot.lock().unwrap() = Some(DashboardSlot {
        sink: new_sink.clone(),
        identity: new_owner,
        stream: new_server,
        history: None,
        next_history_id: 1,
    });
    *state.dashboard.lock().unwrap() = Some(new_sink);
    let view = DashboardView {
        revision: 1,
        panes: vec![PaneTarget {
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
    assert!(state.view.lock().unwrap().is_none());
    assert!(old_sink.queue.lock().unwrap().messages.is_empty());
    let mut role = ClientRole::Dashboard;
    assert!(matches!(
        handle_request_with_id(
            &state,
            &mut role,
            Request::Input {
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
    state
        .sessions
        .lock()
        .unwrap()
        .extend([(focused_id, focused.clone()), (hidden_id, hidden.clone())]);
    *state.view.lock().unwrap() = Some(DashboardView {
        revision: 7,
        panes: vec![
            PaneTarget {
                session: focused_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
            PaneTarget {
                session: hidden_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
        ],
        focused: Some(focused_id),
    });
    let mut role = ClientRole::Dashboard;
    assert!(matches!(
        handle_request_with_id(
            &state,
            &mut role,
            Request::Input {
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
    state
        .sessions
        .lock()
        .unwrap()
        .extend([(first_id, first.clone()), (second_id, second.clone())]);
    let first_size = TerminalSize { rows: 30, cols: 90 };
    let second_size = TerminalSize { rows: 31, cols: 91 };
    let committed = DashboardView {
        revision: 4,
        panes: vec![
            PaneTarget {
                session: first_id,
                size: first_size,
            },
            PaneTarget {
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
    assert_eq!(state.view.lock().unwrap().clone(), Some(committed.clone()));
    assert_eq!(first.terminal_text().0, first_size);
    assert_eq!(second.terminal_text().0, second_size);
    let initial_pty_sizes = (first.master_size().unwrap(), second.master_size().unwrap());
    assert_eq!(initial_pty_sizes, (first_size, second_size));

    let mixed = DashboardView {
        revision: 5,
        panes: vec![
            PaneTarget {
                session: first_id,
                size: TerminalSize { rows: 32, cols: 92 },
            },
            PaneTarget {
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
    assert_eq!(state.view.lock().unwrap().clone(), Some(committed.clone()));
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
                session: first_id,
                size: TerminalSize { rows: 34, cols: 94 },
            },
            PaneTarget {
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
    assert_eq!(state.view.lock().unwrap().clone(), Some(committed));
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
    state.sessions.lock().unwrap().insert(id, session.clone());
    let snapshot = dashboard_snapshot(&state).expect("owner snapshot before dispatch");
    let delayed_snapshot =
        dashboard_snapshot(&state).expect("second owner snapshot before dispatch");
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
        disconnect_dashboard(&cleanup_state, snapshot);
        *cleanup_state.dashboard_slot.lock().unwrap() = Some(DashboardSlot {
            sink: cleanup_sink.clone(),
            identity: cleanup_owner,
            stream: replacement_stream,
            history: None,
            next_history_id: 1,
        });
        *cleanup_state.dashboard.lock().unwrap() = Some(cleanup_sink);
        *cleanup_state.view.lock().unwrap() = Some(cleanup_view);
        set_dashboard_geometry(
            &cleanup_state,
            &cleanup_geometry_owner,
            TerminalSize { rows: 27, cols: 83 },
        );
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
            matches!(
                state.dashboard_slot.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ),
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
    disconnect_dashboard(&state, delayed_snapshot);
    let replacement_slot = state
        .dashboard_slot
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|slot| Arc::ptr_eq(&slot.identity, &replacement_owner));
    let final_view = state.view.lock().unwrap().clone();
    let final_geometry = state
        .dashboard_size
        .lock()
        .unwrap()
        .as_ref()
        .map(|geometry| geometry.size);
    assert!(replacement_slot);
    assert_eq!(final_view, Some(replacement_view));
    assert_eq!(final_geometry, Some(TerminalSize { rows: 27, cols: 83 }));
    assert!(slot_blocked.load(std::sync::atomic::Ordering::Acquire));
    if let Some(snapshot) = dashboard_snapshot(&state) {
        disconnect_dashboard(&state, snapshot);
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
    state.sessions.lock().unwrap().insert(id, session.clone());
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
    *state.resize_hook.lock().unwrap() = Some(Arc::new(move |_, _| {
        hook_state
            .upgrade()
            .expect("server state outlives the resize hook")
            .remove_session(id)
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
    assert!(state.view.lock().unwrap().is_none());
    let mut role = ClientRole::Dashboard;
    assert!(matches!(
        handle_request_with_id(
            &state,
            &mut role,
            Request::Input {
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
    state
        .sessions
        .lock()
        .unwrap()
        .extend([(focused_id, focused.clone()), (removed_id, removed.clone())]);
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
    *state.resize_hook.lock().unwrap() = Some(Arc::new(move |session, size| {
        if calls_for_hook.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            session.resize(size)
        } else {
            hook_state
                .upgrade()
                .expect("server state outlives the resize hook")
                .remove_session(removed_id)
        }
    }));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let focused_pane = PaneTarget {
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
        state.view.lock().unwrap().clone(),
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
    assert!(sink.enqueue_queued(DashboardOutbound {
        message: ServerMessage::Response {
            request_id: 1,
            response: Response::Ok,
        },
        completion: None,
    }));
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
    state
        .sessions
        .lock()
        .unwrap()
        .extend([(exited_id, exited.clone()), (survivor_id, survivor.clone())]);
    *state.view.lock().unwrap() = Some(DashboardView {
        revision: 10,
        panes: vec![
            PaneTarget {
                session: exited_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
            PaneTarget {
                session: survivor_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
        ],
        focused: Some(exited_id),
    });
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
        state.view.lock().unwrap().clone(),
        Some(DashboardView {
            revision: 10,
            panes: vec![
                PaneTarget {
                    session: exited_id,
                    size: TerminalSize { rows: 24, cols: 80 },
                },
                PaneTarget {
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
            id: survivor_id,
            bytes: b"SURVIVOR".to_vec(),
        }))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline
        && !sink.queue.lock().unwrap().messages.iter().any(|outbound| {
            matches!(
                &outbound.message,
                ServerMessage::Event(ServerEvent::Output { session, revision, bytes })
                    if *session == survivor_id && *revision == 10 && bytes == b"SURVIVOR"
            )
        })
    {
        thread::yield_now();
    }
    assert!(sink.queue.lock().unwrap().messages.iter().any(|outbound| {
        matches!(
            &outbound.message,
            ServerMessage::Event(ServerEvent::Output { session, revision, bytes })
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
        state
            .view
            .lock()
            .unwrap()
            .as_ref()
            .map(|view| view.revision),
        Some(10)
    );
    state.remove_session(exited_id).unwrap();
    let after_removal = state.view.lock().unwrap().clone().unwrap();
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
    exited.terminate(Duration::from_secs(2)).unwrap();
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
    state
        .sessions
        .lock()
        .unwrap()
        .extend([(first_id, first.clone()), (second_id, second.clone())]);
    *state.view.lock().unwrap() = Some(DashboardView {
        revision: 10,
        panes: vec![
            PaneTarget {
                session: first_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
            PaneTarget {
                session: second_id,
                size: TerminalSize { rows: 24, cols: 80 },
            },
        ],
        focused: Some(second_id),
    });

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
            let snapshot = dashboard_snapshot(&hook_state).expect("old owner snapshot");
            disconnect_dashboard(&hook_state, snapshot);
            *hook_state.dashboard_slot.lock().unwrap() = Some(DashboardSlot {
                sink: hook_sink.clone(),
                identity: hook_owner.clone(),
                stream: replacement_server.try_clone()?,
                history: None,
                next_history_id: 1,
            });
            *hook_state.dashboard.lock().unwrap() = Some(hook_sink.clone());
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
                        session: first_id,
                        size: TerminalSize { rows: 25, cols: 81 },
                    },
                    PaneTarget {
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
    assert!(state.view.lock().unwrap().is_none());
    assert!(
        state
            .dashboard_slot
            .lock()
            .unwrap()
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
        state
            .view
            .lock()
            .unwrap()
            .as_ref()
            .map(|view| view.revision),
        Some(1)
    );
    assert_eq!(state.view.lock().unwrap().as_ref().unwrap().panes.len(), 1);

    if let Some(snapshot) = dashboard_snapshot(&state) {
        disconnect_dashboard(&state, snapshot);
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
    state
        .sessions
        .lock()
        .unwrap()
        .extend([(first_id, first.clone()), (second_id, second.clone())]);
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
                            session: first_id,
                            size: first_size,
                        },
                        PaneTarget {
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
    assert!(state.dashboard_slot.lock().unwrap().is_none());
    assert!(state.view.lock().unwrap().is_none());
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
    state
        .sessions
        .lock()
        .unwrap()
        .extend([(first_id, first.clone()), (second_id, second.clone())]);
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
        .dashboard_slot
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .sink
        .clone();
    for revision in 1..=4 {
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                session: first_id,
                revision,
                bytes: vec![b'x'; ovrcr_protocol::MAX_FRAME_BYTES - 128],
            }),
            completion: None,
        }));
    }
    for _ in 0..256 {
        thread::yield_now();
    }
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
                            session: first_id,
                            size: TerminalSize { rows: 25, cols: 81 },
                        },
                        PaneTarget {
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
        elapsed >= Duration::from_millis(1_500) && elapsed < Duration::from_secs(5),
        "blocked writer close took {elapsed:?}"
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(state.dashboard_slot.lock().unwrap().is_none());
    assert!(state.view.lock().unwrap().is_none());
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
    state
        .sessions
        .lock()
        .unwrap()
        .extend([(first_id, first.clone()), (second_id, second.clone())]);
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
        .dashboard_slot
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .sink
        .clone();
    // A frame larger than the socket buffer parks the writer while this client
    // is not reading, so the terminal frame stays queued behind it. Keep it
    // small enough to drain well inside the handler's close timeout.
    assert!(sink.enqueue_queued(DashboardOutbound {
        message: response_message(
            72,
            Response::TerminalText {
                session: first_id,
                size: TerminalSize { rows: 24, cols: 80 },
                text: "x".repeat(32 * 1024),
            },
        ),
        completion: None,
    }));
    write_frame(
        &mut client_stream,
        &ClientMessage {
            request_id: 73,
            request: Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![
                        PaneTarget {
                            session: first_id,
                            size: TerminalSize { rows: 25, cols: 81 },
                        },
                        PaneTarget {
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
    let deadline = Instant::now() + Duration::from_secs(3);
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
    assert!(!dashboard_try_send(
        &state,
        ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
    ));
    assert!(state.dashboard_slot.lock().unwrap().is_some());
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
    assert!(state.dashboard_slot.lock().unwrap().is_none());
    assert!(state.view.lock().unwrap().is_none());
    state.dispatch.send(DispatchMessage::Stop).unwrap();
    dispatcher.join().unwrap();
    cleanup_test_session(&second, second_events).unwrap();
    cleanup_test_session(&first, first_events).unwrap();
}

#[test]
fn agent_report_queue_full_is_a_structured_conflict() {
    let (state, _receiver) = test_state_with_dispatch(None, None);
    for _ in 0..RAW_DISPATCH_QUEUE_CAPACITY {
        state.dispatch.try_send(DispatchMessage::Stop).unwrap();
    }
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
    state.remove_session(summary.id).unwrap();

    drop(listener);
    std::fs::remove_file(&socket).unwrap();
    let error = state
        .create_session(ovrcr_protocol::CreateSessionRequest {
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
) -> (Arc<ServerState>, Receiver<DispatchMessage>) {
    let (events, _) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let (dispatch, receiver) = mpsc::sync_channel(RAW_DISPATCH_QUEUE_CAPACITY);
    (
        Arc::new(ServerState {
            tasks: None,
            socket: PathBuf::from("/tmp/ovrcr-test.sock"),
            registry_path: PathBuf::from("config.toml"),
            registry: Mutex::new(Registry::default()),
            sessions: Mutex::new(HashMap::new()),
            view: Mutex::new(None),
            dashboard: Mutex::new(dashboard.clone()),
            next_session_id: AtomicU64::new(1),
            mutation_lock: Mutex::new(()),
            dispatch,
            shutdown: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            dashboard_size: Mutex::new(None),
            events: Mutex::new(Some(events)),
            #[cfg(test)]
            resize_hook: Mutex::new(None),
            before_view_publish_hook: Mutex::new(None),
            dashboard_slot: Mutex::new(stream.map(|(identity, stream)| DashboardSlot {
                sink: dashboard.as_ref().unwrap().clone(),
                identity,
                stream,
                history: None,
                next_history_id: 1,
            })),
        }),
        receiver,
    )
}

fn test_state_with_socket(
    socket: PathBuf,
    registry: Registry,
) -> (
    Arc<ServerState>,
    Receiver<DispatchMessage>,
    Receiver<SessionEvent>,
) {
    let (events, event_receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let (dispatch, dispatch_receiver) = mpsc::sync_channel(RAW_DISPATCH_QUEUE_CAPACITY);
    (
        Arc::new(ServerState {
            tasks: None,
            socket,
            registry_path: PathBuf::from("config.toml"),
            registry: Mutex::new(registry),
            sessions: Mutex::new(HashMap::new()),
            view: Mutex::new(None),
            dashboard: Mutex::new(None),
            next_session_id: AtomicU64::new(1),
            mutation_lock: Mutex::new(()),
            dispatch,
            shutdown: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            dashboard_size: Mutex::new(None),
            events: Mutex::new(Some(events)),
            #[cfg(test)]
            resize_hook: Mutex::new(None),
            before_view_publish_hook: Mutex::new(None),
            dashboard_slot: Mutex::new(None),
        }),
        dispatch_receiver,
        event_receiver,
    )
}

fn spawn_live_test_session(
    id: SessionId,
) -> (tempfile::TempDir, Arc<Session>, Receiver<SessionEvent>) {
    let cwd = tempfile::tempdir().unwrap();
    let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let session = Session::spawn(
        id,
        SessionSpec {
            project: "p".into(),
            workspace: "w".into(),
            name: "live".into(),
            label: "sh".into(),
            cwd: cwd.path().to_path_buf(),
            argv: vec![
                "sh".into(),
                "-c".into(),
                "trap '' HUP TERM; while :; do sleep 1; done".into(),
            ],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
    )
    .unwrap();
    (cwd, session, receiver)
}

fn spawn_exiting_test_session(
    id: SessionId,
) -> (tempfile::TempDir, Arc<Session>, Receiver<SessionEvent>) {
    let cwd = tempfile::tempdir().unwrap();
    let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let session = Session::spawn(
        id,
        SessionSpec {
            project: "p".into(),
            workspace: "w".into(),
            name: "exiting".into(),
            label: "sh".into(),
            cwd: cwd.path().to_path_buf(),
            argv: vec!["sh".into(), "-c".into(), "printf FINAL; exit 0".into()],
            hook_env: None,
        },
        TerminalSize { rows: 24, cols: 80 },
        events,
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
    receiver: Receiver<SessionEvent>,
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
) -> (Arc<ServerState>, Receiver<DispatchMessage>, UnixStream) {
    let (server_stream, client_stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let identity = Arc::new(());
    let (state, dispatch_receiver) =
        test_state_with_dispatch(Some(sink), Some((identity, server_stream)));
    state
        .sessions
        .lock()
        .unwrap()
        .insert(id, Arc::clone(session));
    for _ in 0..RAW_DISPATCH_QUEUE_CAPACITY {
        state.dispatch.try_send(DispatchMessage::Stop).unwrap();
    }
    (state, dispatch_receiver, client_stream)
}

fn cleanup_test_session(session: &Session, events: TestSessionEvents) -> Result<()> {
    let _ = session.set_paused(false);
    let termination = session.terminate(Duration::from_secs(2));
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
    state
        .sessions
        .lock()
        .unwrap()
        .insert(id, Arc::clone(&session));
    state
        .sessions
        .lock()
        .unwrap()
        .insert(switched_id, Arc::clone(&switched_session));
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
    *state.view.lock().unwrap() = Some(DashboardView {
        revision: 1,
        panes: vec![PaneTarget {
            session: id,
            size: TerminalSize { rows: 24, cols: 80 },
        }],
        focused: Some(id),
    });
    let (select_completion, select_result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::Select {
            request_id: 8,
            session: id,
            size: TerminalSize { rows: 24, cols: 80 },
            completion: select_completion,
        })
        .unwrap();
    select_result
        .recv_timeout(Duration::from_secs(2))
        .expect("same-session select completion");
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
    let (switch_completion, switch_result) = mpsc::sync_channel(1);
    state
        .dispatch
        .send(DispatchMessage::Select {
            request_id: 10,
            session: switched_id,
            size: TerminalSize { rows: 24, cols: 80 },
            completion: switch_completion,
        })
        .unwrap();
    switch_result
        .recv_timeout(Duration::from_secs(2))
        .expect("switch select completion");
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
    *state.dashboard_slot.lock().unwrap() = Some(DashboardSlot {
        sink: new_sink.clone(),
        identity: new_owner.clone(),
        stream: new_server,
        history: None,
        next_history_id: 1,
    });
    *state.dashboard.lock().unwrap() = Some(new_sink.clone());

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
        .dashboard_slot
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .identity
        .clone();
    let (new_server, _new_client) = UnixStream::pair().unwrap();
    let replacement_sink = DashboardSink::new();
    let replacement_owner = Arc::new(());
    *replacement_state.dashboard_slot.lock().unwrap() = Some(DashboardSlot {
        sink: replacement_sink.clone(),
        identity: replacement_owner,
        stream: new_server,
        history: None,
        next_history_id: 1,
    });
    *replacement_state.dashboard.lock().unwrap() = Some(replacement_sink.clone());
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
            .dashboard_slot
            .lock()
            .unwrap()
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
    race_state
        .sessions
        .lock()
        .unwrap()
        .insert(race_id, Arc::clone(&race_session));
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
            .dashboard_slot
            .lock()
            .unwrap()
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
    state
        .dashboard_slot
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .history = Some(history);
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
    state
        .sessions
        .lock()
        .unwrap()
        .insert(SessionId(12), Arc::clone(&page_session));
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
            if !sink.enqueue_queued(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
                    projects: Vec::new(),
                })),
                completion: None,
            }) {
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
    let disconnected_before_release = completion_before_release
        && state.dashboard_slot.lock().unwrap().is_none()
        && state.dashboard.lock().unwrap().is_none();
    let _ = parser_holder_release.send(());
    let parser_holder_joined = parser_holder.join().is_ok();
    let dispatcher_stopped = state.dispatch.send(DispatchMessage::Stop).is_ok();
    let dispatcher_joined = dispatcher.join().is_ok();
    let cleanup_result = cleanup_test_session(&page_session, page_events);
    let removed = state.remove_session(SessionId(12)).is_ok();
    assert!(parser_was_held);
    assert!(queue_filled);
    assert!(page_sent);
    assert!(completion_before_release);
    assert!(disconnected_before_release);
    assert!(parser_holder_joined);
    assert!(dispatcher_stopped);
    assert!(dispatcher_joined);
    assert!(cleanup_result.is_ok());
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
    state
        .sessions
        .lock()
        .unwrap()
        .insert(id, Arc::clone(&session));
    *state.view.lock().unwrap() = Some(DashboardView {
        revision: 1,
        panes: vec![PaneTarget {
            session: id,
            size: TerminalSize { rows: 24, cols: 80 },
        }],
        focused: Some(id),
    });
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));

    state
        .dispatch
        .send(DispatchMessage::Session(SessionEvent::Output {
            id,
            bytes: b"A".to_vec(),
        }))
        .unwrap();
    send_history_command(&state, &owner, 2, HistoryRequest::Begin { session: id });
    state
        .dispatch
        .send(DispatchMessage::Session(SessionEvent::Output {
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
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
                projects: Vec::new()
            },)),
            completion: None,
        }));
    }
    let identity = Arc::new(());
    let state = test_state(Some(sink), Some((identity, server_stream)));
    dashboard_try_send(
        &state,
        ServerMessage::Event(ServerEvent::ScreenDirty {
            session: SessionId(2),
            revision: 0,
        }),
    );
    let mut byte = [0_u8; 1];
    assert_eq!(client_stream.read(&mut byte).unwrap(), 0);
    assert!(state.dashboard.lock().unwrap().is_none());
}

#[test]
fn pending_output_does_not_evict_dashboard_on_lifecycle_event() {
    // A burst can leave the queue full of output frames before the writer
    // thread runs. A lifecycle event arriving then must fold the output
    // into a dirty marker rather than evict a dashboard that is keeping up.
    let sink = DashboardSink::new();
    for _ in 0..DASHBOARD_QUEUE {
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::Output {
                session: SessionId(2),
                revision: 7,
                bytes: b"x".to_vec(),
            }),
            completion: None,
        }));
    }
    assert!(sink.enqueue_queued(DashboardOutbound {
        message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
            projects: Vec::new()
        })),
        completion: None,
    }));
    assert!(matches!(
        queued_dashboard_message(&sink),
        ServerMessage::Event(ServerEvent::HierarchyChanged(_))
    ));
    assert!(matches!(
        sink.next(),
        Some(DashboardDelivery::Dirty {
            revision: 7,
            session: SessionId(2)
        })
    ));
    // Messages that cannot be coalesced still evict once they fill the queue.
    for _ in 0..DASHBOARD_QUEUE {
        assert!(sink.enqueue_queued(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
                projects: Vec::new()
            })),
            completion: None,
        }));
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
    let old_snapshot = dashboard_snapshot(&state).unwrap();

    let (new_server, _new_client) = UnixStream::pair().unwrap();
    let new_sink = DashboardSink::new();
    let new_identity = Arc::new(());
    *state.dashboard_slot.lock().unwrap() = Some(DashboardSlot {
        sink: new_sink.clone(),
        identity: new_identity,
        stream: new_server,
        history: None,
        next_history_id: 1,
    });
    *state.dashboard.lock().unwrap() = Some(new_sink);
    disconnect_dashboard(&state, old_snapshot);

    assert!(state.dashboard.lock().unwrap().is_some());
    assert!(matches!(old_client.read(&mut [0_u8; 1]), Ok(0)));
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
    let first_snapshot = dashboard_snapshot(&state).expect("first dashboard slot");
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
    disconnect_dashboard(&state, first_snapshot);
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
        .dashboard_slot
        .lock()
        .unwrap()
        .take()
        .expect("first dashboard slot");
    *state.dashboard.lock().unwrap() = None;
    *state.view.lock().unwrap() = None;
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
        .dashboard_slot
        .lock()
        .unwrap()
        .as_ref()
        .expect("replacement dashboard slot")
        .identity
        .clone();
    set_dashboard_geometry(
        &state,
        &second_identity,
        TerminalSize { rows: 27, cols: 83 },
    );
    drop(mutation);
    assert!(join_test_thread_bounded(
        first_handler,
        Duration::from_secs(5)
    ));
    let geometry = state
        .dashboard_size
        .lock()
        .unwrap()
        .as_ref()
        .map(|geometry| (geometry.size, Arc::clone(&geometry.owner)));
    let (size, owner) = geometry.expect("replacement geometry");
    assert_eq!(size, TerminalSize { rows: 27, cols: 83 });
    assert!(Arc::ptr_eq(&owner, &second_identity));
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
        state
            .dashboard_size
            .lock()
            .unwrap()
            .as_ref()
            .map(|geometry| geometry.size),
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
    state
        .sessions
        .lock()
        .unwrap()
        .insert(id, Arc::clone(&session));
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
            state.close_terminal(id, Duration::from_millis(250))
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
        let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
        let session = Session::spawn(
            id,
            SessionSpec {
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
        )
        .unwrap();
        (cwd, session, receiver)
    };
    let events = apply_test_session_events(Arc::clone(&session), receiver);
    session.wait_until_exited(Duration::from_secs(2)).unwrap();
    cleanup_test_session(&session, events).unwrap();
    let state = test_state(None, None);
    state
        .sessions
        .lock()
        .unwrap()
        .insert(id, Arc::clone(&session));
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
    state.dashboard.lock().unwrap().as_ref().unwrap().close();
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
    assert!(state.dashboard_slot.lock().unwrap().is_none());
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
    let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let capability = [0x49; 32];
    let session = Session::spawn(
        SessionId(7),
        crate::session::SessionSpec {
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
    state
        .sessions
        .lock()
        .unwrap()
        .insert(SessionId(7), Arc::clone(&session));
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
    let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
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
    state
        .sessions
        .lock()
        .unwrap()
        .insert(SessionId(71), Arc::clone(&session));
    let dispatcher_state = Arc::clone(&state);
    let dispatcher = thread::spawn(move || run_dispatcher(dispatcher_state, dispatch_receiver));
    let mut cleanup = KillFailureCleanup::new(
        Arc::clone(&session),
        Arc::clone(&refuse_sigcont),
        Arc::clone(&state),
        original_pgid,
        waiter,
        dispatcher,
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
    let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let session = Session::spawn_with_test_hooks(
            SessionId(72),
            SessionSpec {
                project: "p".into(),
                workspace: "w".into(),
                name: "exited-leader".into(),
                label: "sh".into(),
                cwd: cwd.path().to_path_buf(),
                argv: vec![
                    "sh".into(),
                    "-c".into(),
                    "(trap '' HUP; printf DESCENDANT_READY; while :; do sleep 1; done) & printf LEADER_READY; while IFS= read -r line; do case \"$line\" in HOST_OWNERSHIP_ACK) printf HOST_OWNERSHIP_ACKED; IFS= read -r line || break; [ \"$line\" = ALLOW_LEADER_EXIT ] && kill -KILL \"$$\";; esac; done".into(),
                ],
                hook_env: None,
            },
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
    state
        .sessions
        .lock()
        .unwrap()
        .insert(SessionId(72), Arc::clone(&session));
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
    let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let session = Session::spawn(
        SessionId(8),
        crate::session::SessionSpec {
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
    state
        .sessions
        .lock()
        .unwrap()
        .insert(SessionId(8), Arc::clone(&session));
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
    let (events, receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let id = SessionId(17);
    let session = Session::spawn(
        id,
        crate::session::SessionSpec {
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
    )
    .unwrap();
    let (state, _dispatch_receiver) = test_state_with_dispatch(None, None);
    state
        .sessions
        .lock()
        .unwrap()
        .insert(id, Arc::clone(&session));
    *state.view.lock().unwrap() = Some(DashboardView {
        revision: 1,
        panes: vec![PaneTarget {
            session: id,
            size: TerminalSize { rows: 24, cols: 80 },
        }],
        focused: Some(id),
    });

    let error = state
        .close_terminal(id, Duration::from_millis(20))
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
            .view
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|view| view.focused),
        Some(id)
    );

    loop {
        let event = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        let exited = matches!(event, SessionEvent::Exited { .. });
        session.apply_event(event);
        if exited {
            break;
        }
    }
    state.close_terminal(id, Duration::from_millis(20)).unwrap();
    assert!(!state.sessions.lock().unwrap().contains_key(&id));
    assert!(state.view.lock().unwrap().as_ref().is_some_and(|view| {
        view.revision == 1 && view.panes.is_empty() && view.focused.is_none()
    }));
}

#[test]
fn registration_holds_sessions_guard_until_spawn_returns() {
    let root = tempfile::tempdir().unwrap();
    let (events, event_receiver) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
    let (dispatch, dispatch_receiver) = mpsc::sync_channel(RAW_DISPATCH_QUEUE_CAPACITY);
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
        view: Mutex::new(None),
        dashboard: Mutex::new(None),
        next_session_id: AtomicU64::new(1),
        mutation_lock: Mutex::new(()),
        dispatch: dispatch.clone(),
        shutdown: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        dashboard_size: Mutex::new(None),
        events: Mutex::new(Some(events)),
        #[cfg(test)]
        resize_hook: Mutex::new(None),
        before_view_publish_hook: Mutex::new(None),
        dashboard_slot: Mutex::new(None),
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
        state.sessions.try_lock().is_err(),
        "registration must hold sessions guard while Session::spawn is paused"
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
}

impl KillFailureCleanup {
    fn new(
        session: Arc<Session>,
        refuse_sigcont: Arc<AtomicBool>,
        state: Arc<ServerState>,
        original_pgid: libc::pid_t,
        waiter: JoinHandle<()>,
        dispatcher: JoinHandle<()>,
    ) -> Self {
        Self {
            session,
            refuse_sigcont,
            state,
            original_pgid,
            waiter: Some(waiter),
            dispatcher: Some(dispatcher),
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
        state.dashboard_slot.lock().unwrap().is_none(),
        "unwinding must release the dashboard slot"
    );
    assert!(state.dashboard.lock().unwrap().is_none());

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

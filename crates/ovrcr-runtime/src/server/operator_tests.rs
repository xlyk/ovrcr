use super::*;
use crate::retained::Disposition;
use ovrcr_protocol::*;

#[test]
fn operator_current_and_open_require_the_current_dashboard() {
    let (peer, stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let owner = Arc::new(());
    let root = tempfile::tempdir().unwrap();
    let mut state = tests::test_state(Some(sink), Some((owner.clone(), stream)));
    Arc::get_mut(&mut state).unwrap().registry_path = root.path().to_path_buf();
    let stale = Arc::new(());
    for request in [OperatorRequest::Current, OperatorRequest::Open] {
        assert!(matches!(
            state.handle_request(&mut ClientRole::Control, Request::Operator(request.clone())),
            Response::Error { .. }
        ));
        assert!(matches!(
            connections::handle_request_with_id(
                &state,
                &mut ClientRole::Dashboard,
                Request::Operator(request.clone()),
                1,
                Some(&stale)
            ),
            Response::Error { .. }
        ));
    }
    // Current creates nothing, not even an idle retained session.
    assert!(matches!(
        connections::handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            Request::Operator(OperatorRequest::Current),
            2,
            Some(&owner)
        ),
        Response::Operator(OperatorResponse::Current(None))
    ));
    let count: i64 = state
        .retained
        .lock()
        .connection
        .query_row("SELECT count(*) FROM operator_conversations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    assert_eq!(state.retained.lock().records().count(), 0);
    let Response::Operator(OperatorResponse::Opened(c)) = connections::handle_request_with_id(
        &state,
        &mut ClientRole::Dashboard,
        Request::Operator(OperatorRequest::Open),
        3,
        Some(&owner),
    ) else {
        panic!("current dashboard must open the singleton")
    };
    assert!(state.hierarchy().projects.is_empty());
    let Response::Operator(OperatorResponse::Current(Some(again))) =
        connections::handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            Request::Operator(OperatorRequest::Current),
            4,
            Some(&owner),
        )
    else {
        panic!("current must observe the opened singleton")
    };
    assert_eq!(again, c);
    let Response::Operator(OperatorResponse::History(h)) = connections::handle_request_with_id(
        &state,
        &mut ClientRole::Dashboard,
        Request::Operator(OperatorRequest::Read {
            conversation: c.id,
            after: 0,
            limit: 100,
        }),
        5,
        Some(&owner),
    ) else {
        panic!("history")
    };
    assert!(h.messages.is_empty());
    assert_eq!(h.conversation.title, "Operator");
    assert!(h.conversation.model.is_none());
    assert_eq!(
        state.retained.lock().get(c.session).unwrap().run,
        SessionRunId(0)
    );
    assert!(state.sessions.lock().unwrap().is_empty());
    drop(peer);
}

#[test]
fn operator_notifications_coalesce_without_eviction_or_losing_latest_catchup() {
    let sink = DashboardSink::new();
    for _ in 0..1000 {
        assert_eq!(
            sink.enqueue(DashboardOutbound {
                message: ServerMessage::Event(ServerEvent::OperatorChanged(
                    OperatorConversationId(1)
                )),
                completion: None
            }),
            Enqueue::Queued
        );
    }
    assert_eq!(sink.reporting_snapshot().pending_items, 1);
    assert!(matches!(
        sink.next(),
        Some(DashboardDelivery::Message(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::OperatorChanged(OperatorConversationId(1))),
            ..
        }))
    ));
    assert_eq!(
        sink.enqueue(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::OperatorChanged(OperatorConversationId(1))),
            completion: None
        }),
        Enqueue::Queued
    );
    assert_eq!(sink.reporting_snapshot().pending_items, 1);
}

struct LiveOperator {
    root: tempfile::TempDir,
    state: Arc<ServerState>,
    dashboard: Option<UnixStream>,
    next_request: u64,
    accept: Option<JoinHandle<()>>,
    bridge: Option<JoinHandle<()>>,
    dispatcher: Option<JoinHandle<()>>,
}
impl LiveOperator {
    fn new() -> Self {
        Self::from_root(tempfile::tempdir().unwrap())
    }
    fn from_root(root: tempfile::TempDir) -> Self {
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let socket = root.path().join("server.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let script = root.path().join("ovrcr");
        let executable = std::env::current_exe().unwrap();
        // Only the worker process is a fixture. Admission, PTY, lease, framing,
        // dispatcher, journal and Dashboard connection/writer are production code.
        fs::write(
            &script,
            format!(
                r#"#!/bin/sh
export OVRCR_TEST_OPERATOR_IDENT="$4,$6,$8,${{10}}"
export OVRCR_TEST_OPERATOR_ROOT='{}'
exec '{}' --exact server::operator_tests::operator_worker_fixture --ignored --nocapture
"#,
                root.path().display(),
                executable.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let (mut state, dispatch_rx, event_rx) =
            tests::test_state_with_socket(socket.clone(), Registry::default());
        let mutable = Arc::get_mut(&mut state).unwrap();
        mutable.registry_path = root.path().to_path_buf();
        mutable.retained = parking_lot::Mutex::new(SessionStore::open(root.path()).unwrap());
        mutable.operator.executable = script;
        let dispatch_tx = state.dispatch.clone();
        let bridge = thread::spawn(move || bridge_events(event_rx, dispatch_tx));
        let worker_state = state.clone();
        let dispatcher = thread::spawn(move || run_dispatcher(worker_state, dispatch_rx));
        let accept_state = state.clone();
        let accept = thread::spawn(move || {
            let mut handlers = Vec::new();
            while !accept_state.shutdown.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let state = accept_state.clone();
                        handlers.push(thread::spawn(move || {
                            connections::handle_connection(state, stream)
                        }));
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::park_timeout(Duration::from_millis(2))
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            }
            for handler in handlers {
                handler.join().unwrap();
            }
        });
        let mut dashboard = UnixStream::connect(&socket).unwrap();
        dashboard
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        exchange_preamble(&mut dashboard).unwrap();
        let mut fixture = Self {
            root,
            state,
            dashboard: Some(dashboard),
            next_request: 0,
            accept: Some(accept),
            bridge: Some(bridge),
            dispatcher: Some(dispatcher),
        };
        assert!(matches!(
            fixture.request(Request::DashboardHello),
            Response::Hierarchy(_)
        ));
        fixture
    }
    fn request(&mut self, request: Request) -> Response {
        self.next_request += 1;
        let id = self.next_request;
        let stream = self.dashboard.as_mut().unwrap();
        write_frame(
            stream,
            &ClientMessage {
                request_id: id,
                request,
            },
        )
        .unwrap();
        loop {
            match read_frame::<ServerMessage>(stream).unwrap() {
                ServerMessage::Response {
                    request_id,
                    response,
                } if request_id == id => return response,
                _ => {}
            }
        }
    }
    fn current(&mut self) -> Option<OperatorConversationSummary> {
        let Response::Operator(OperatorResponse::Current(current)) =
            self.request(Request::Operator(OperatorRequest::Current))
        else {
            panic!("current")
        };
        current
    }
    fn open(&mut self) -> OperatorConversationSummary {
        let Response::Operator(OperatorResponse::Opened(c)) =
            self.request(Request::Operator(OperatorRequest::Open))
        else {
            panic!("open")
        };
        c
    }
    fn submit(&mut self, c: &OperatorConversationSummary) -> OperatorTurn {
        let Response::Operator(OperatorResponse::Submitted(t)) =
            self.request(Request::Operator(OperatorRequest::Submit {
                conversation: c.id,
                expected_run: c.run,
                text: "fixture prompt".into(),
                context: OperatorContext::default(),
            }))
        else {
            panic!("submit")
        };
        t
    }
    fn ready(&self, c: &OperatorConversationSummary) -> SupervisorAuth {
        let path = self.root.path().join(format!("ready-{}", c.id.0));
        wait_until(|| path.exists());
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }
    fn command(&self, c: &OperatorConversationSummary, command: &str) {
        fs::write(
            self.root.path().join(format!("{command}-{}", c.id.0)),
            b"go",
        )
        .unwrap();
    }
    fn history(&mut self, c: &OperatorConversationSummary) -> OperatorHistory {
        let Response::Operator(OperatorResponse::History(h)) =
            self.request(Request::Operator(OperatorRequest::Read {
                conversation: c.id,
                after: 0,
                limit: 100,
            }))
        else {
            panic!("read")
        };
        h
    }
    fn control(&self, request: Request) -> Response {
        let mut stream = UnixStream::connect(&self.state.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        exchange_preamble(&mut stream).unwrap();
        worker_exchange(&mut stream, request)
    }
}
impl LiveOperator {
    fn stop_server(&mut self) {
        if self.accept.is_none() {
            return;
        }
        let sessions: Vec<_> = self
            .state
            .sessions
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect();
        for session in sessions {
            if session.is_live() {
                session
                    .terminate(Duration::from_secs(2))
                    .expect("fixture owned group cleanup");
            }
        }
        self.dashboard.take();
        self.state.shutdown.store(true, Ordering::Release);
        self.accept.take().unwrap().join().unwrap();
        fs::remove_file(&self.state.socket).unwrap();
        self.state.operator.join_controls().unwrap();
        self.state.events.lock().unwrap().take();
        self.bridge.take().unwrap().join().unwrap();
        let _ = self.state.dispatch.send(DispatchMessage::Stop);
        self.dispatcher.take().unwrap().join().unwrap();
    }
}
impl Drop for LiveOperator {
    fn drop(&mut self) {
        self.stop_server();
    }
}
fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(Instant::now() < deadline, "fixture deadline");
        thread::park_timeout(Duration::from_millis(2));
    }
}
fn worker_exchange(stream: &mut UnixStream, request: Request) -> Response {
    write_frame(
        stream,
        &ClientMessage {
            request_id: 1,
            request,
        },
    )
    .unwrap();
    let ServerMessage::Response { response, .. } = read_frame::<ServerMessage>(stream).unwrap()
    else {
        panic!("worker response")
    };
    response
}

#[test]
#[ignore = "owned managed worker subprocess, launched only by LiveOperator"]
fn operator_worker_fixture() {
    let Some(raw) = std::env::var_os("OVRCR_TEST_OPERATOR_IDENT") else {
        panic!("only launch from owning fixture")
    };
    let ids: Vec<u64> = raw
        .to_str()
        .unwrap()
        .split(',')
        .map(|x| x.parse().unwrap())
        .collect();
    let identity = OperatorTurnIdentity {
        conversation: OperatorConversationId(ids[0]),
        turn: OperatorTurnId(ids[1]),
        session: SessionId(ids[2]),
        run: SessionRunId(ids[3]),
    };
    let root = PathBuf::from(std::env::var_os("OVRCR_TEST_OPERATOR_ROOT").unwrap());
    let socket = std::env::var_os("OVRCR_HOOK_SOCKET").unwrap();
    let token = std::env::var("OVRCR_HOOK_TOKEN").unwrap();
    let mut bytes = [0u8; 32];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&token[i * 2..i * 2 + 2], 16).unwrap();
    }
    let mut stream = UnixStream::connect(socket).unwrap();
    exchange_preamble(&mut stream).unwrap();
    let Response::AgentOperation(AgentOperationResult::Reserved(reservation)) = worker_exchange(
        &mut stream,
        Request::ReserveAgent(ReserveAgent {
            session: identity.session,
            capability: AgentSecret(bytes),
            operation: "reserve".into(),
            invocation: "fixture".into(),
            expected_epoch: 0,
            provider: AgentProvider::Operator,
            pi_resume: None,
        }),
    ) else {
        panic!("reserve")
    };
    let auth = SupervisorAuth {
        session: identity.session,
        lease: reservation.lease,
    };
    let Response::OperatorWorker(OperatorWorkerResult::Start(start)) = worker_exchange(
        &mut stream,
        Request::OperatorWorker(OperatorWorkerRequest {
            auth: auth.clone(),
            command: OperatorWorkerCommand::Start { identity },
        }),
    ) else {
        panic!("start")
    };
    assert_eq!(start.identity, identity);
    assert_eq!(start.text, "fixture prompt");
    assert!(
        start.model.is_none(),
        "no operator.model setting was configured"
    );
    if identity.run.0 > 1 {
        assert!(
            start.checkpoint_turn.is_some(),
            "a follow-up turn must resume the last completed checkpoint"
        );
    } else {
        assert!(start.checkpoint_turn.is_none());
    }
    let batch = |sequence, events| {
        Request::OperatorExchange(OperatorExchange {
            auth: auth.clone(),
            batch: OperatorEventBatch {
                identity,
                sequence,
                events,
            },
        })
    };
    assert!(matches!(
        worker_exchange(
            &mut stream,
            batch(
                1,
                vec![
                    OperatorWorkerEvent::AssistantDelta("partial".into()),
                    OperatorWorkerEvent::ModelSelected(OperatorModel {
                        provider: "fixture".into(),
                        id: "offline".into(),
                    }),
                ]
            )
        ),
        Response::OperatorExchange(_)
    ));
    let ready = root.join(format!("ready-{}", identity.conversation.0));
    fs::write(
        ready.with_extension("tmp"),
        serde_json::to_vec(&auth).unwrap(),
    )
    .unwrap();
    fs::set_permissions(
        ready.with_extension("tmp"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    fs::rename(ready.with_extension("tmp"), &ready).unwrap();
    wait_until(|| {
        root.join(format!("complete-{}", identity.conversation.0))
            .exists()
    });
    assert!(matches!(
        worker_exchange(&mut stream, batch(2, vec![OperatorWorkerEvent::Completed])),
        Response::OperatorExchange(_)
    ));
    fs::write(
        root.join(format!("acked-{}", identity.conversation.0)),
        b"ack",
    )
    .unwrap();
    wait_until(|| {
        root.join(format!("retire-{}", identity.conversation.0))
            .exists()
    });
    // The Server owns our process group; no natural exit substitutes for stop proof.
    let _ = worker_exchange(
        &mut stream,
        Request::OperatorWorker(OperatorWorkerRequest {
            auth,
            command: OperatorWorkerCommand::Retire { identity },
        }),
    );
    loop {
        thread::park_timeout(Duration::from_secs(1));
    }
}

#[test]
fn operator_current_never_creates_and_open_is_idempotent() {
    let mut live = LiveOperator::new();
    assert!(live.current().is_none());
    assert!(live.state.sessions.lock().unwrap().is_empty());
    let first = live.open();
    assert_eq!(first.title, "Operator");
    assert!(first.worker_stopped);
    assert_eq!(first.run, SessionRunId(0));
    assert!(first.current_turn.is_none());
    assert!(first.model.is_none());
    assert_eq!(live.current().unwrap(), first);
    for _ in 0..3 {
        assert_eq!(live.open(), first);
    }
    let count: i64 = live
        .state
        .retained
        .lock()
        .connection
        .query_row(
            "SELECT count(*) FROM operator_conversations WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    assert!(
        live.state.sessions.lock().unwrap().is_empty(),
        "opening must never start a worker"
    );
    assert!(live.history(&first).messages.is_empty());
}

#[test]
fn operator_worker_completes_and_retires_with_stop_proof() {
    let mut live = LiveOperator::new();
    let c = live.open();
    let turn = live.submit(&c);
    live.ready(&c);
    assert_eq!(turn.identity.run, SessionRunId(1));
    assert_eq!(live.history(&c).messages.len(), 2);
    live.command(&c, "complete");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    let h = live.history(&c);
    assert_eq!(
        h.conversation.current_turn.unwrap().state,
        OperatorTurnState::Complete
    );
    assert!(!h.conversation.worker_stopped);
    assert_eq!(
        h.conversation
            .model
            .as_ref()
            .map(|model| (model.provider.as_str(), model.id.as_str())),
        Some(("fixture", "offline")),
        "ModelSelected is retained metadata, not a transcript message"
    );
    assert_eq!(
        h.messages.len(),
        2,
        "model selection and completion are not copied into the transcript"
    );
    assert!(live.state.sessions.lock().unwrap()[&c.session].is_live());
    live.command(&c, "retire");
    wait_until(|| {
        live.state
            .retained
            .lock()
            .operator_conversation(c.id)
            .unwrap()
            .worker_stopped
    });
    assert!(live.history(&c).conversation.worker_stopped);
    assert!(!live.state.sessions.lock().unwrap()[&c.session].is_live());
}

#[test]
fn operator_stop_is_independent_busy_refusal_and_stale_messages_preserve_history() {
    let mut live = LiveOperator::new();
    let c = live.open();
    let turn = live.submit(&c);
    let auth = live.ready(&c);
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: c.id,
            expected_run: turn.identity.run,
            text: "keep draft".into(),
            context: OperatorContext::default()
        })),
        Response::Error { .. }
    ));
    assert_eq!(live.history(&c).messages.len(), 2);
    let mut stale = turn.identity;
    stale.run.0 += 1;
    assert!(matches!(
        live.control(Request::OperatorExchange(OperatorExchange {
            auth: auth.clone(),
            batch: OperatorEventBatch {
                identity: stale,
                sequence: 2,
                events: vec![OperatorWorkerEvent::AssistantDelta("stale".into())]
            }
        })),
        Response::Error { .. }
    ));
    let Response::Operator(OperatorResponse::Stopped(stopped)) =
        live.request(Request::Operator(OperatorRequest::Stop {
            identity: turn.identity,
        }))
    else {
        panic!("stop")
    };
    assert_eq!(stopped.state, OperatorTurnState::Interrupted);
    assert!(live.history(&c).conversation.worker_stopped);
    assert!(matches!(
        live.control(Request::OperatorExchange(OperatorExchange {
            auth,
            batch: OperatorEventBatch {
                identity: turn.identity,
                sequence: 2,
                events: vec![OperatorWorkerEvent::AssistantDelta("late".into())]
            }
        })),
        Response::Error { .. }
    ));
    // Stop preserves the visible journal; the interrupted turn is the last message.
    assert_eq!(
        live.history(&c).messages.last().unwrap().content,
        OperatorMessageContent::Interrupted
    );
}

#[test]
fn operator_authenticated_stop_cannot_target_another_identity() {
    let mut live = LiveOperator::new();
    let c = live.open();
    let turn = live.submit(&c);
    let auth = live.ready(&c);
    let mut other = turn.identity;
    other.turn.0 += 1;
    assert!(matches!(
        live.control(Request::OperatorWorker(OperatorWorkerRequest {
            auth: auth.clone(),
            command: OperatorWorkerCommand::Stop { identity: other }
        })),
        Response::Error { .. }
    ));
    assert!(live.state.sessions.lock().unwrap()[&c.session].is_live());
    assert_eq!(
        live.history(&c).conversation.current_turn.unwrap().state,
        OperatorTurnState::Running
    );
    assert!(
        matches!(live.control(Request::OperatorWorker(OperatorWorkerRequest { auth, command: OperatorWorkerCommand::Stop { identity: turn.identity } })), Response::OperatorWorker(OperatorWorkerResult::Retired { identity }) if identity == turn.identity)
    );
    assert_eq!(
        live.history(&c).conversation.current_turn.unwrap().state,
        OperatorTurnState::Interrupted
    );
    assert!(live.history(&c).conversation.worker_stopped);
}

#[test]
fn operator_journal_failure_is_not_acknowledged_or_published() {
    let mut live = LiveOperator::new();
    let c = live.open();
    let turn = live.submit(&c);
    let auth = live.ready(&c);
    let db = rusqlite::Connection::open(crate::config::database_path(live.root.path())).unwrap();
    db.execute_batch("CREATE TRIGGER reject_batch BEFORE INSERT ON operator_batches BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;").unwrap();
    let batch = OperatorEventBatch {
        identity: turn.identity,
        sequence: 2,
        events: vec![OperatorWorkerEvent::AssistantDelta(
            "must not commit".into(),
        )],
    };
    assert!(matches!(
        live.control(Request::OperatorExchange(OperatorExchange {
            auth: auth.clone(),
            batch: batch.clone()
        })),
        Response::Error { .. }
    ));
    let h = live.history(&c);
    assert_eq!(h.messages.len(), 2);
    assert_eq!(h.conversation.current_turn.unwrap().sequence, 1);
    db.execute_batch("DROP TRIGGER reject_batch").unwrap();
    assert!(matches!(
        live.control(Request::OperatorExchange(OperatorExchange {
            auth: auth.clone(),
            batch: batch.clone()
        })),
        Response::OperatorExchange(OperatorExchangeResult {
            duplicate: false,
            ..
        })
    ));
    assert!(matches!(
        live.control(Request::OperatorExchange(OperatorExchange { auth, batch })),
        Response::OperatorExchange(OperatorExchangeResult {
            duplicate: true,
            ..
        })
    ));
    assert_eq!(live.history(&c).messages.len(), 3);
}

#[test]
fn operator_capacity_refuses_without_consuming_prompt_or_run() {
    let mut live = LiveOperator::new();
    let c = live.open();
    for index in 0..50 {
        let row = live
            .state
            .retained
            .lock()
            .create(SessionMetadata {
                scope: SessionScope::Workspace,
                project: "fixture".into(),
                workspace: "fixture".into(),
                name: format!("slot-{index}"),
                label: "fixture".into(),
                cwd: live.root.path().into(),
                kind: SessionKind::Terminal,
                pinned_title: None,
                application_title: None,
            })
            .unwrap();
        let _mutation = live.state.mutation_lock.lock().unwrap();
        live.state
            .spawn_record_locked(
                row.id,
                row.run,
                vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "while :; do sleep 60; done".into(),
                ],
                SessionSpawnOptions::default(),
            )
            .unwrap();
    }
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: c.id,
            expected_run: SessionRunId(0),
            text: "keep this draft".into(),
            context: OperatorContext::default()
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    let h = live.history(&c);
    assert!(h.messages.is_empty());
    assert!(h.conversation.worker_stopped);
    assert!(h.conversation.current_turn.is_none());
    assert_eq!(
        live.state.retained.lock().get(c.session).unwrap().run,
        SessionRunId(0)
    );
}

#[test]
fn operator_natural_exit_does_not_certify_stop_or_allow_another_turn() {
    let mut live = LiveOperator::new();
    let c = live.open();
    let turn = live.submit(&c);
    live.ready(&c);
    let session = live.state.sessions.lock().unwrap()[&c.session].clone();
    let pid = session.summary().pid.unwrap();
    assert_eq!(unsafe { libc::kill(-(pid as i32), libc::SIGKILL) }, 0);
    wait_until(|| !session.is_live());
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Stop {
            identity: turn.identity
        })),
        Response::Error { .. }
    ));
    let h = live.history(&c);
    assert!(!h.conversation.worker_stopped);
    assert_eq!(
        h.conversation.current_turn.unwrap().state,
        OperatorTurnState::Interrupted
    );
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: c.id,
            expected_run: turn.identity.run,
            text: "do not replay".into(),
            context: OperatorContext::default()
        })),
        Response::Error { .. }
    ));
}

#[test]
fn operator_replaced_dashboard_cannot_commit_delayed_stop() {
    let mut live = LiveOperator::new();
    let c = live.open();
    let turn = live.submit(&c);
    live.ready(&c);
    let old = live.state.dashboard.snapshot().unwrap().identity;
    let held = live.state.mutation_lock.lock().unwrap();
    let state = live.state.clone();
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    *live.state.operator.before_stop.lock().unwrap() = Some(Arc::new(move || {
        started_tx.send(()).unwrap();
    }));
    let delayed = thread::spawn(move || {
        state.operator_request(
            &old,
            OperatorRequest::Stop {
                identity: turn.identity,
            },
        )
    });
    started_rx.recv().unwrap();
    // Replace owner while the request cannot reach durable intent.
    let new_owner = Arc::new(());
    let (peer, server) = UnixStream::pair().unwrap();
    live.state
        .dashboard
        .claim_with_identity(DashboardSink::new(), new_owner, server);
    drop(held);
    assert!(
        delayed.join().unwrap().is_err(),
        "an evicted Dashboard must not interrupt its captured turn"
    );
    assert_eq!(
        live.state
            .retained
            .lock()
            .operator_conversation(c.id)
            .unwrap()
            .current_turn
            .unwrap()
            .state,
        OperatorTurnState::Running
    );
    assert!(live.state.sessions.lock().unwrap()[&c.session].is_live());
    drop(peer);
}

#[test]
fn operator_slow_dashboard_does_not_block_stop_or_terminal_input() {
    let mut live = LiveOperator::new();
    let c = live.open();
    let turn = live.submit(&c);
    let auth = live.ready(&c);
    let row = live
        .state
        .retained
        .lock()
        .create(SessionMetadata {
            scope: SessionScope::Workspace,
            project: "fixture".into(),
            workspace: "fixture".into(),
            name: "terminal".into(),
            label: "terminal".into(),
            cwd: live.root.path().into(),
            kind: SessionKind::Terminal,
            pinned_title: None,
            application_title: None,
        })
        .unwrap();
    {
        let _mutation = live.state.mutation_lock.lock().unwrap();
        live.state
            .spawn_record_locked(
                row.id,
                row.run,
                vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    "while IFS= read -r line; do printf 'accepted:%s\\n' \"$line\"; done".into(),
                ],
                SessionSpawnOptions::default(),
            )
            .unwrap();
    }
    let (hit_tx, hit_rx) = mpsc::sync_channel(1);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let writer_gate = gate.clone();
    let _release = WriterRelease(gate.clone());
    let once = AtomicBool::new(false);
    *live.state.before_dashboard_write_hook.lock().unwrap() = Some(Arc::new(move || {
        if !once.swap(true, Ordering::AcqRel) {
            hit_tx.send(()).unwrap();
            let (lock, wake) = &*writer_gate;
            let mut release = lock.lock().unwrap();
            while !*release {
                release = wake.wait(release).unwrap();
            }
        }
    }));
    live.state.operator_changed(c.id);
    hit_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    // Exercise synchronous durable commits while the real sole writer cannot move.
    for sequence in 2..=130 {
        assert!(matches!(
            live.control(Request::OperatorExchange(OperatorExchange {
                auth: auth.clone(),
                batch: OperatorEventBatch {
                    identity: turn.identity,
                    sequence,
                    events: vec![OperatorWorkerEvent::AssistantDelta("stream".into())]
                }
            })),
            Response::OperatorExchange(_)
        ));
    }
    assert!(matches!(
        live.control(Request::OperatorWorker(OperatorWorkerRequest {
            auth,
            command: OperatorWorkerCommand::Stop {
                identity: turn.identity
            }
        })),
        Response::OperatorWorker(_)
    ));
    assert_eq!(
        live.control(Request::SendTerminal {
            session: row.id,
            text: "responsive".into(),
            submit: true
        }),
        Response::Ok
    );
    let terminal = live.state.sessions.lock().unwrap()[&row.id].clone();
    wait_until(|| terminal.terminal_text().1.contains("accepted:responsive"));
    assert_eq!(
        live.state
            .retained
            .lock()
            .operator_conversation(c.id)
            .unwrap()
            .current_turn
            .unwrap()
            .sequence,
        130
    );
    assert!(
        live.state
            .retained
            .lock()
            .operator_conversation(c.id)
            .unwrap()
            .worker_stopped
    );
    let sink = live.state.dashboard.snapshot().unwrap().sink;
    assert!(!sink.is_closing());
    assert!(
        sink.queue
            .lock()
            .unwrap()
            .messages
            .iter()
            .filter(|outbound| matches!(
                outbound.message,
                ServerMessage::Event(ServerEvent::OperatorChanged(_))
            ))
            .count()
            <= 2
    );
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    *live.state.before_dashboard_write_hook.lock().unwrap() = None;
    let h = live.history(&c);
    assert_eq!(h.next_after, Some(h.messages.last().unwrap().ordinal));
    assert_eq!(h.messages.len(), 100);
}

struct WriterRelease(Arc<(Mutex<bool>, Condvar)>);
impl Drop for WriterRelease {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

#[test]
fn operator_server_restart_retains_partial_history_without_launch_or_replay() {
    let mut live = LiveOperator::new();
    let c = live.open();
    let turn = live.submit(&c);
    live.ready(&c);
    live.stop_server();
    let root = std::mem::replace(&mut live.root, tempfile::tempdir().unwrap());
    let mut restarted = LiveOperator::from_root(root);
    assert!(restarted.state.sessions.lock().unwrap().is_empty());
    let h = restarted.history(&c);
    assert_eq!(h.messages.len(), 3);
    assert_eq!(
        h.conversation.current_turn.unwrap().state,
        OperatorTurnState::Interrupted
    );
    assert!(!h.conversation.worker_stopped);
    assert!(matches!(
        restarted.request(Request::Operator(OperatorRequest::Submit {
            conversation: c.id,
            expected_run: turn.identity.run,
            text: "do not replay uncertain work".into(),
            context: OperatorContext::default()
        })),
        Response::Error { .. }
    ));
    assert_eq!(restarted.history(&c).messages.len(), 3);
    assert!(restarted.state.sessions.lock().unwrap().is_empty());
}

#[test]
fn operator_followup_reuses_only_completed_context_after_controlled_retirement() {
    let mut live = LiveOperator::new();
    let c = live.open();
    let first = live.submit(&c);
    let old_auth = live.ready(&c);
    live.command(&c, "complete");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    // A completed model response still occupies its slot and cannot start a replacement.
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: c.id,
            expected_run: first.identity.run,
            text: "fixture prompt".into(),
            context: OperatorContext::default()
        })),
        Response::Error { .. }
    ));
    live.command(&c, "retire");
    wait_until(|| {
        live.state
            .retained
            .lock()
            .operator_conversation(c.id)
            .unwrap()
            .worker_stopped
    });
    assert_eq!(live.open(), live.current().unwrap());
    for command in ["complete", "acked", "ready", "retire"] {
        fs::remove_file(live.root.path().join(format!("{command}-{}", c.id.0))).unwrap();
    }
    let Response::Operator(OperatorResponse::Submitted(second)) =
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: c.id,
            expected_run: first.identity.run,
            text: "fixture prompt".into(),
            context: OperatorContext::default(),
        }))
    else {
        panic!("followup")
    };
    assert_eq!(second.identity.run, SessionRunId(2));
    assert_ne!(second.identity.turn, first.identity.turn);
    live.ready(&c); // Worker Start asserted the prior completed checkpoint.
    assert!(matches!(
        live.control(Request::OperatorExchange(OperatorExchange {
            auth: old_auth,
            batch: OperatorEventBatch {
                identity: first.identity,
                sequence: 3,
                events: vec![OperatorWorkerEvent::AssistantDelta("old run".into())]
            }
        })),
        Response::Error { .. }
    ));
    assert_eq!(live.history(&c).messages.len(), 4);
}

#[test]
fn operator_completed_control_handles_are_pruned_and_remain_bounded() {
    let mut live = LiveOperator::new();
    let c = live.open();
    for _ in 0..200 {
        assert!(matches!(
            live.request(Request::Operator(OperatorRequest::Submit {
                conversation: c.id,
                expected_run: SessionRunId(0),
                text: "".into(),
                context: OperatorContext::default()
            })),
            Response::Error { .. }
        ));
        assert!(live.state.operator.control_snapshot().1 <= 50);
    }
    wait_until(|| {
        let snapshot = live.state.operator.control_snapshot();
        snapshot.0 == 0 && snapshot.2
    });
    let turn = live.submit(&c);
    live.ready(&c);
    assert_eq!(turn.identity.run, SessionRunId(1));
    assert!(
        live.state.operator.control_snapshot().1 <= 1,
        "completed handles must be pruned before the next control"
    );
}

#[test]
fn operator_request_lock_order_never_holds_dashboard_while_waiting_for_retained() {
    let mut inverted = Vec::new();
    for operation in ["current", "open", "read", "submit"] {
        let root = tempfile::tempdir().unwrap();
        let (peer, stream) = UnixStream::pair().unwrap();
        let owner = Arc::new(());
        let mut state =
            tests::test_state(Some(DashboardSink::new()), Some((owner.clone(), stream)));
        Arc::get_mut(&mut state).unwrap().registry_path = root.path().to_path_buf();
        let c = state
            .retained
            .lock()
            .open_operator_conversation(root.path())
            .unwrap();
        let request = match operation {
            "current" => OperatorRequest::Current,
            "open" => OperatorRequest::Open,
            "read" => OperatorRequest::Read {
                conversation: c.id,
                after: 0,
                limit: 100,
            },
            "submit" => OperatorRequest::Submit {
                conversation: c.id,
                expected_run: SessionRunId(42),
                text: "keep draft".into(),
                context: OperatorContext::default(),
            },
            _ => unreachable!(),
        };
        let (arrived_tx, arrived_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        *state.operator.before_retained.lock().unwrap() = Some(Arc::new(move || {
            arrived_tx.send(()).unwrap();
            let _ = release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(2));
        }));
        // Model the retained owner in assigned_human_input. Never wait for the
        // Dashboard slot: observe with try_lock, release, then join every request.
        let retained = state.retained.lock();
        let worker_state = state.clone();
        let worker = thread::spawn(move || worker_state.operator_request(&owner, request));
        let arrived = arrived_rx.recv_timeout(Duration::from_secs(2)).is_ok();
        let slot_held = arrived && state.dashboard.slot_is_locked_for_test();
        let replacement = if arrived && !slot_held {
            let (replacement_peer, replacement_stream) = UnixStream::pair().unwrap();
            state.dashboard.claim_with_identity(
                DashboardSink::new(),
                Arc::new(()),
                replacement_stream,
            );
            Some(replacement_peer)
        } else {
            None
        };
        release_tx.send(()).unwrap();
        drop(retained);
        let result = worker.join().unwrap();
        assert!(
            arrived,
            "{operation} must reach the actual retained lock boundary"
        );
        if slot_held {
            if operation == "submit" {
                assert!(result.is_err(), "stale run must refuse before spawn");
            } else {
                assert!(
                    result.is_ok(),
                    "{operation} must complete after releasing retained"
                );
            }
            inverted.push(operation);
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("dashboard owner changed"),
                "{operation} must retain its owner fence after waiting for retained"
            );
        }
        drop(replacement);
        drop(peer);
    }
    assert!(
        inverted.is_empty(),
        "Dashboard -> retained inversion in {inverted:?}"
    );
}

#[test]
fn operator_lifecycle_routes_refuse_operator_scope() {
    let mut live = LiveOperator::new();
    let c = live.open();
    // Archive the singleton directly to prove the generic lifecycle entry points
    // refuse it rather than relying on a foreign-key error.
    live.state
        .retained
        .lock()
        .set_archived(c.session, c.run, true)
        .unwrap();
    let archived = live.state.retained.lock().get(c.session).unwrap().run;
    assert!(matches!(
        live.request(Request::UnarchiveSession {
            session: c.session,
            expected_run: archived,
        }),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    assert!(matches!(
        live.request(Request::DeleteArchivedSession {
            session: c.session,
            expected_run: archived,
        }),
        Response::Error { .. }
    ));
    assert!(
        live.state
            .close_terminal(c.session, archived, Duration::from_secs(1))
            .is_err()
    );
    // The lifecycle owner refuses the same scope directly, not through the wire layer.
    for refusal in [
        live.state.unarchive_session(c.session, archived),
        live.state.delete_archived_session(c.session, archived),
        live.state.remove_session(c.session),
        live.state.kill_session(c.session, Duration::from_secs(1)),
    ] {
        assert!(
            matches!(
                refusal.as_ref().unwrap_err().downcast_ref::<LifecycleFailure>(),
                Some(error) if error.code == ErrorCode::Conflict
            ),
            "generic lifecycle route must refuse Operator scope: {refusal:?}"
        );
    }
    assert!(
        live.state
            .retained
            .lock()
            .get(c.session)
            .is_some_and(|record| record.disposition == Disposition::Archived),
        "refused routes must leave the row untouched"
    );
    // Restore the fixture's own row so its teardown sees a consistent state.
    live.state
        .retained
        .lock()
        .set_archived(c.session, archived, false)
        .unwrap();
}

#[test]
fn operator_legacy_lifecycle_routes_are_refused_without_touching_the_row() {
    let mut live = LiveOperator::new();
    let legacy = live.open();
    // Pre-singleton shape: an idle legacy chat with no marker.
    {
        let retained = live.state.retained.lock();
        retained
            .connection
            .execute("UPDATE operator_conversations SET singleton=0", [])
            .unwrap();
    }
    assert!(live.current().is_none());
    assert!(matches!(
        live.request(Request::UnarchiveSession {
            session: legacy.session,
            expected_run: legacy.run,
        }),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));
    for refusal in [
        live.state.unarchive_session(legacy.session, legacy.run),
        live.state
            .delete_archived_session(legacy.session, legacy.run),
        live.state.remove_session(legacy.session),
        live.state
            .close_terminal(legacy.session, legacy.run, Duration::from_secs(1)),
    ] {
        assert!(
            matches!(
                refusal.as_ref().unwrap_err().downcast_ref::<LifecycleFailure>(),
                Some(error) if error.code == ErrorCode::Conflict
            ),
            "a legacy Operator row must be refused explicitly: {refusal:?}"
        );
    }
    let retained = live.state.retained.lock();
    assert!(
        retained.get(legacy.session).is_some(),
        "refused routes must preserve the legacy row"
    );
    assert_eq!(
        retained
            .operator_conversation(legacy.id)
            .unwrap()
            .current_turn,
        None
    );
}

#[test]
fn operator_legacy_conversation_is_hidden_refuses_work_and_keeps_recovery() {
    let mut live = LiveOperator::new();
    let legacy = live.open();
    let turn = live.submit(&legacy);
    live.ready(&legacy);
    // The pre-singleton shape is a row without the singleton marker.
    {
        let retained = live.state.retained.lock();
        retained
            .connection
            .execute("UPDATE operator_conversations SET singleton=0", [])
            .unwrap();
    }
    assert!(live.current().is_none(), "legacy rows are not Current");
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Read {
            conversation: legacy.id,
            after: 0,
            limit: 100,
        })),
        Response::Error { .. }
    ));
    assert!(
        live.state
            .retained
            .lock()
            .operator_conversation(legacy.id)
            .is_ok(),
        "terminal diagnostics still resolve the legacy row"
    );
    // The legacy worker cannot be stopped through Operator Stop; an uncertain exit is
    // what exact-run acknowledgement exists for.
    let session = live.state.sessions.lock().unwrap()[&legacy.session].clone();
    let pid = session.summary().pid.unwrap();
    assert_eq!(unsafe { libc::kill(-(pid as i32), libc::SIGKILL) }, 0);
    wait_until(|| !session.is_live());
    live.state
        .acknowledge_session_stopped(legacy.session, turn.identity.run)
        .unwrap();
    assert_eq!(
        live.state
            .retained
            .lock()
            .operator_conversation(legacy.id)
            .unwrap()
            .current_turn
            .unwrap()
            .state,
        OperatorTurnState::Interrupted
    );
    // No new submit revives it, and Open starts a separate singleton.
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: legacy.id,
            expected_run: turn.identity.run,
            text: "revive".into(),
            context: OperatorContext::default(),
        })),
        Response::Error { .. }
    ));
    let fresh = live.open();
    assert_ne!(fresh.id, legacy.id);
    assert_eq!(fresh.title, "Operator");
    assert!(fresh.worker_stopped);
    assert!(fresh.current_turn.is_none());
    assert_eq!(live.current().unwrap().id, fresh.id);
}

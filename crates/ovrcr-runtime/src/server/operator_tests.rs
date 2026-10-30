use super::*;
use ovrcr_protocol::*;

#[test]
fn operator_requests_require_current_dashboard_and_create_global_history() {
    let (peer, stream) = UnixStream::pair().unwrap();
    let sink = DashboardSink::new();
    let owner = Arc::new(());
    let root = tempfile::tempdir().unwrap();
    let mut state = tests::test_state(Some(sink), Some((owner.clone(), stream)));
    Arc::get_mut(&mut state).unwrap().registry_path = root.path().to_path_buf();
    let create = Request::Operator(OperatorRequest::Create {
        provider: OperatorProvider::ChatGpt,
        model: "fixture".into(),
    });
    assert!(matches!(
        state.handle_request(&mut ClientRole::Control, create.clone()),
        Response::Error { .. }
    ));
    assert!(matches!(
        connections::handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            create.clone(),
            1,
            Some(&Arc::new(()))
        ),
        Response::Error { .. }
    ));
    let Response::Operator(OperatorResponse::Created(c)) = connections::handle_request_with_id(
        &state,
        &mut ClientRole::Dashboard,
        create,
        2,
        Some(&owner),
    ) else {
        panic!("current dashboard must create conversation")
    };
    assert!(state.hierarchy().projects.is_empty());
    let Response::Operator(OperatorResponse::History(h)) = connections::handle_request_with_id(
        &state,
        &mut ClientRole::Dashboard,
        Request::Operator(OperatorRequest::Read {
            conversation: c.id,
            after: 0,
            limit: 100,
        }),
        3,
        Some(&owner),
    ) else {
        panic!("history")
    };
    assert!(h.messages.is_empty());
    assert_eq!(
        state.retained.lock().get(c.session).unwrap().run,
        SessionRunId(0)
    );
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
    fn create(&mut self) -> OperatorConversationSummary {
        let Response::Operator(OperatorResponse::Created(c)) =
            self.request(Request::Operator(OperatorRequest::Create {
                provider: OperatorProvider::ChatGpt,
                model: "offline-fixture".into(),
            }))
        else {
            panic!("create")
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
    if identity.run.0 > 1 {
        assert_eq!(start.history.unwrap().json, "[]");
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
                vec![OperatorWorkerEvent::AssistantDelta("partial".into())]
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
        worker_exchange(
            &mut stream,
            batch(
                2,
                vec![OperatorWorkerEvent::Completed(OperatorModelHistory {
                    version: 1,
                    json: "[]".into()
                })]
            )
        ),
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
fn operator_real_workers_complete_concurrently_and_retire_with_stop_proof() {
    let mut live = LiveOperator::new();
    let a = live.create();
    let b = live.create();
    assert!(a.worker_stopped && b.worker_stopped);
    let ta = live.submit(&a);
    let tb = live.submit(&b);
    live.ready(&a);
    live.ready(&b);
    assert_eq!(ta.identity.run, SessionRunId(1));
    assert_eq!(tb.identity.run, SessionRunId(1));
    assert_eq!(live.history(&a).messages.len(), 2);
    assert_eq!(live.history(&b).messages.len(), 2);
    live.command(&a, "complete");
    live.command(&b, "complete");
    wait_until(|| {
        [a.id, b.id]
            .iter()
            .all(|id| live.root.path().join(format!("acked-{}", id.0)).exists())
    });
    for c in [&a, &b] {
        let h = live.history(c);
        assert_eq!(
            h.conversation.current_turn.unwrap().state,
            OperatorTurnState::Complete
        );
        assert!(!h.conversation.worker_stopped);
        assert!(live.state.sessions.lock().unwrap()[&c.session].is_live());
    }
    live.command(&a, "retire");
    live.command(&b, "retire");
    wait_until(|| {
        [a.id, b.id].iter().all(|id| {
            live.state
                .retained
                .lock()
                .operator_conversation(*id)
                .unwrap()
                .worker_stopped
        })
    });
    for c in [&a, &b] {
        assert!(live.history(c).conversation.worker_stopped);
        assert!(!live.state.sessions.lock().unwrap()[&c.session].is_live());
    }
}

#[test]
fn operator_stop_is_independent_busy_refusal_and_stale_messages_preserve_history() {
    let mut live = LiveOperator::new();
    let a = live.create();
    let b = live.create();
    let ta = live.submit(&a);
    let tb = live.submit(&b);
    let auth = live.ready(&a);
    live.ready(&b);
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: a.id,
            expected_run: ta.identity.run,
            text: "keep draft".into(),
            context: OperatorContext::default()
        })),
        Response::Error { .. }
    ));
    assert_eq!(live.history(&a).messages.len(), 2);
    let mut stale = ta.identity;
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
            identity: ta.identity,
        }))
    else {
        panic!("stop")
    };
    assert_eq!(stopped.state, OperatorTurnState::Interrupted);
    assert!(live.history(&a).conversation.worker_stopped);
    assert_eq!(
        live.history(&b).conversation.current_turn.unwrap().identity,
        tb.identity
    );
    assert!(live.state.sessions.lock().unwrap()[&b.session].is_live());
    assert!(matches!(
        live.control(Request::OperatorExchange(OperatorExchange {
            auth,
            batch: OperatorEventBatch {
                identity: ta.identity,
                sequence: 2,
                events: vec![OperatorWorkerEvent::AssistantDelta("late".into())]
            }
        })),
        Response::Error { .. }
    ));
    live.command(&b, "complete");
    wait_until(|| live.root.path().join(format!("acked-{}", b.id.0)).exists());
    live.command(&b, "retire");
    wait_until(|| {
        live.state
            .retained
            .lock()
            .operator_conversation(b.id)
            .unwrap()
            .worker_stopped
    });
}

#[test]
fn operator_authenticated_timeout_stop_cannot_target_another_worker() {
    let mut live = LiveOperator::new();
    let a = live.create();
    let b = live.create();
    let ta = live.submit(&a);
    let tb = live.submit(&b);
    let auth = live.ready(&a);
    live.ready(&b);
    assert!(matches!(
        live.control(Request::OperatorWorker(OperatorWorkerRequest {
            auth: auth.clone(),
            command: OperatorWorkerCommand::Stop {
                identity: tb.identity
            }
        })),
        Response::Error { .. }
    ));
    assert!(live.state.sessions.lock().unwrap()[&b.session].is_live());
    assert!(
        matches!(live.control(Request::OperatorWorker(OperatorWorkerRequest { auth, command: OperatorWorkerCommand::Stop { identity: ta.identity } })), Response::OperatorWorker(OperatorWorkerResult::Retired { identity }) if identity == ta.identity)
    );
    assert_eq!(
        live.history(&a).conversation.current_turn.unwrap().state,
        OperatorTurnState::Interrupted
    );
    assert!(live.history(&a).conversation.worker_stopped);
    assert!(live.state.sessions.lock().unwrap()[&b.session].is_live());
}

#[test]
fn operator_journal_failure_is_not_acknowledged_or_published() {
    let mut live = LiveOperator::new();
    let c = live.create();
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
    let c = live.create();
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
                None,
                None,
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
    let c = live.create();
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
    let c = live.create();
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
fn operator_slow_dashboard_does_not_block_other_chats_stop_or_terminal_input() {
    let mut live = LiveOperator::new();
    let a = live.create();
    let b = live.create();
    let ta = live.submit(&a);
    let tb = live.submit(&b);
    let auth_a = live.ready(&a);
    let auth_b = live.ready(&b);
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
                None,
                None,
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
    live.state.operator_changed(a.id);
    hit_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    // Exercise synchronous durable commits while the real sole writer cannot move.
    for sequence in 2..=130 {
        assert!(matches!(
            live.control(Request::OperatorExchange(OperatorExchange {
                auth: auth_a.clone(),
                batch: OperatorEventBatch {
                    identity: ta.identity,
                    sequence,
                    events: vec![OperatorWorkerEvent::AssistantDelta("stream".into())]
                }
            })),
            Response::OperatorExchange(_)
        ));
    }
    assert!(matches!(
        live.control(Request::OperatorWorker(OperatorWorkerRequest {
            auth: auth_b,
            command: OperatorWorkerCommand::Stop {
                identity: tb.identity
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
            .operator_conversation(a.id)
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
            .operator_conversation(b.id)
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
    let h = live.history(&a);
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
    let c = live.create();
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
    let c = live.create();
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
    live.ready(&c); // Worker Start asserted the prior completed model history.
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
    let c = live.create();
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
    for operation in ["create", "list", "read", "submit", "close"] {
        let root = tempfile::tempdir().unwrap();
        let (peer, stream) = UnixStream::pair().unwrap();
        let owner = Arc::new(());
        let mut state =
            tests::test_state(Some(DashboardSink::new()), Some((owner.clone(), stream)));
        Arc::get_mut(&mut state).unwrap().registry_path = root.path().to_path_buf();
        let c = state
            .retained
            .lock()
            .create_operator_conversation(OperatorProvider::ChatGpt, "fixture", root.path())
            .unwrap();
        let request = match operation {
            "create" => OperatorRequest::Create {
                provider: OperatorProvider::ChatGpt,
                model: "fixture".into(),
            },
            "list" => OperatorRequest::List {
                after: 0,
                limit: 100,
            },
            "read" => OperatorRequest::Read {
                conversation: c.id,
                after: 0,
                limit: 100,
            },
            "close" => OperatorRequest::Close {
                conversation: c.id,
                expected_run: c.run,
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
            assert_eq!(
                state
                    .retained
                    .lock()
                    .read_operator_history(c.id, 0, 100)
                    .unwrap()
                    .messages
                    .len(),
                0
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
fn operator_close_archives_only_after_controlled_stop() {
    let mut live = LiveOperator::new();
    // Never-run conversations archive without inventing a turn.
    let idle = live.create();
    let Response::Operator(OperatorResponse::Closed(closed)) =
        live.request(Request::Operator(OperatorRequest::Close {
            conversation: idle.id,
            expected_run: idle.run,
        }))
    else {
        panic!("close idle")
    };
    assert!(closed.archived && closed.worker_stopped);
    assert_eq!(closed.run, SessionRunId(1));
    assert!(closed.current_turn.is_none());
    assert!(live.history(&idle).messages.is_empty());
    for completed in [false, true] {
        let c = live.create();
        let turn = live.submit(&c);
        let auth = live.ready(&c);
        if completed {
            live.command(&c, "complete");
            wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
        }
        let db =
            rusqlite::Connection::open(crate::config::database_path(live.root.path())).unwrap();
        let model_history = || {
            db.query_row(
                "SELECT history FROM operator_model_history WHERE conversation=?1",
                [i64::try_from(c.id.0).unwrap()],
                |row| row.get::<_, String>(0),
            )
            .ok()
        };
        let saved_model = model_history();
        if completed {
            assert_eq!(
                serde_json::from_str::<OperatorModelHistory>(saved_model.as_ref().unwrap())
                    .unwrap()
                    .json,
                "[]"
            );
        }
        let before = live.history(&c);
        let owner = live.state.dashboard.snapshot().unwrap().identity;
        assert!(
            live.state
                .close_operator_conversation(&Arc::new(()), c.id, turn.identity.run)
                .is_err()
        );
        assert!(
            live.state
                .close_operator_conversation(&owner, c.id, c.run)
                .is_err()
        );
        let Response::Operator(OperatorResponse::Closed(closed)) =
            live.request(Request::Operator(OperatorRequest::Close {
                conversation: c.id,
                expected_run: turn.identity.run,
            }))
        else {
            panic!("close running")
        };
        assert!(closed.archived && closed.worker_stopped);
        assert_eq!(closed.run.0, turn.identity.run.0 + 1);
        assert!(!live.state.sessions.lock().unwrap().contains_key(&c.session));
        let after = live.history(&c);
        assert_eq!(model_history(), saved_model);
        if completed {
            assert_eq!(after.messages, before.messages);
        } else {
            assert_eq!(&after.messages[..before.messages.len()], before.messages);
            assert_eq!(
                after.messages.last().unwrap().content,
                OperatorMessageContent::Interrupted
            );
        }
        assert!(matches!(
            live.control(Request::OperatorExchange(OperatorExchange {
                auth,
                batch: OperatorEventBatch {
                    identity: turn.identity,
                    sequence: 3,
                    events: vec![OperatorWorkerEvent::AssistantDelta("late".into())]
                }
            })),
            Response::Error { .. }
        ));
        assert!(
            live.state
                .close_operator_conversation(&owner, c.id, turn.identity.run)
                .is_err()
        );
        assert!(
            live.state
                .delete_archived_session(c.session, closed.run)
                .is_err()
        );
        assert_eq!(live.history(&c).messages, after.messages);
        if completed {
            live.state.unarchive_session(c.session, closed.run).unwrap();
            let restored = live.history(&c).conversation;
            assert!(!restored.archived && restored.worker_stopped);
            assert_eq!(restored.run.0, closed.run.0 + 1);
            assert!(!live.state.sessions.lock().unwrap().contains_key(&c.session));
            assert!(matches!(
                live.request(Request::Operator(OperatorRequest::Submit {
                    conversation: c.id,
                    expected_run: turn.identity.run,
                    text: "stale".into(),
                    context: OperatorContext::default()
                })),
                Response::Error { .. }
            ));
            let fresh = live.submit(&restored);
            assert_eq!(fresh.identity.run.0, restored.run.0 + 1);
            wait_until(|| {
                live.state
                    .retained
                    .lock()
                    .operator_conversation(c.id)
                    .unwrap()
                    .current_turn
                    .is_some_and(|turn| turn.identity == fresh.identity && turn.sequence >= 1)
            });
        }
    }
}

#[test]
fn operator_close_failure_leaves_unarchived_history_observable() {
    let mut live = LiveOperator::new();
    let c = live.create();
    let t = live.submit(&c);
    live.ready(&c);
    let db = rusqlite::Connection::open(crate::config::database_path(live.root.path())).unwrap();
    db.execute_batch("CREATE TRIGGER reject_interrupt BEFORE INSERT ON operator_messages BEGIN SELECT RAISE(ABORT,'fixture journal failure'); END;").unwrap();
    let close = || {
        Request::Operator(OperatorRequest::Close {
            conversation: c.id,
            expected_run: t.identity.run,
        })
    };
    assert!(matches!(live.request(close()), Response::Error { .. }));
    let h = live.history(&c);
    assert!(!h.conversation.archived && !h.conversation.worker_stopped);
    assert_eq!(
        h.conversation.current_turn.unwrap().state,
        OperatorTurnState::Running
    );
    db.execute_batch("DROP TRIGGER reject_interrupt; CREATE TRIGGER reject_archive BEFORE UPDATE OF disposition ON retained_sessions BEGIN SELECT RAISE(ABORT,'fixture archive failure'); END;").unwrap();
    assert!(matches!(live.request(close()), Response::Error { .. }));
    let h = live.history(&c);
    assert!(!h.conversation.archived && h.conversation.worker_stopped);
    assert_eq!(h.conversation.run, t.identity.run);
    db.execute_batch("DROP TRIGGER reject_archive").unwrap();
    assert!(matches!(
        live.request(close()),
        Response::Operator(OperatorResponse::Closed(_))
    ));
}

#[test]
fn operator_close_serializes_submit_and_revalidates_replaced_owner() {
    let mut live = LiveOperator::new();
    let c = live.create();
    let turn = live.submit(&c);
    live.ready(&c);
    let owner = live.state.dashboard.snapshot().unwrap().identity;
    let (arrived_tx, arrived_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    *live.state.operator.before_stop.lock().unwrap() = Some(Arc::new(move || {
        arrived_tx.send(()).unwrap();
        release_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    }));
    assert!(matches!(
        super::operator::dispatch_control(
            &live.state,
            &owner,
            1000,
            OperatorRequest::Close {
                conversation: c.id,
                expected_run: turn.identity.run
            }
        ),
        Some(Response::Ok)
    ));
    arrived_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        super::operator::dispatch_control(
            &live.state,
            &owner,
            1001,
            OperatorRequest::Submit {
                conversation: c.id,
                expected_run: turn.identity.run,
                text: "must not submit".into(),
                context: OperatorContext::default()
            }
        ),
        Some(Response::Error { .. })
    ));
    assert!(matches!(
        super::operator::dispatch_control(
            &live.state,
            &owner,
            1002,
            OperatorRequest::Close {
                conversation: c.id,
                expected_run: turn.identity.run
            }
        ),
        Some(Response::Error { .. })
    ));
    let (peer, stream) = UnixStream::pair().unwrap();
    live.state
        .dashboard
        .claim_with_identity(DashboardSink::new(), Arc::new(()), stream);
    release_tx.send(()).unwrap();
    wait_until(|| live.state.operator.control_snapshot().0 == 0);
    let h = live
        .state
        .retained
        .lock()
        .read_operator_history(c.id, 0, 100)
        .unwrap();
    assert!(!h.conversation.archived && !h.conversation.worker_stopped);
    assert_eq!(
        h.conversation.current_turn.unwrap().state,
        OperatorTurnState::Running
    );
    assert_eq!(h.messages.len(), 2);
    drop(peer);
}

#[test]
fn operator_close_without_owned_worker_keeps_stop_uncertainty() {
    let mut live = LiveOperator::new();
    let c = live.create();
    let t = live
        .state
        .retained
        .lock()
        .submit_operator_turn(c.id, c.run, "fixture prompt", OperatorContext::default())
        .unwrap();
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Close {
            conversation: c.id,
            expected_run: t.identity.run
        })),
        Response::Error { .. }
    ));
    let h = live.history(&c);
    assert!(!h.conversation.archived && !h.conversation.worker_stopped);
    assert_eq!(h.conversation.run, t.identity.run);
    assert_eq!(
        h.conversation.current_turn.unwrap().state,
        OperatorTurnState::Interrupted
    );
}

#[test]
fn operator_close_stopped_turn_preserves_journal_and_model_history() {
    let mut live = LiveOperator::new();
    let c = live.create();
    let t = live.submit(&c);
    live.ready(&c);
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Stop {
            identity: t.identity
        })),
        Response::Operator(OperatorResponse::Stopped(_))
    ));
    let before = live.history(&c);
    let Response::Operator(OperatorResponse::Closed(closed)) =
        live.request(Request::Operator(OperatorRequest::Close {
            conversation: c.id,
            expected_run: t.identity.run,
        }))
    else {
        panic!("close stopped")
    };
    assert!(closed.archived && closed.worker_stopped);
    assert_eq!(live.history(&c).messages, before.messages);
}

#[test]
fn operator_archive_restore_publish_durable_changes() {
    let root = tempfile::tempdir().unwrap();
    let (peer, stream) = UnixStream::pair().unwrap();
    let owner = Arc::new(());
    let sink = DashboardSink::new();
    let state = tests::test_state(Some(sink.clone()), Some((owner.clone(), stream)));
    let c = state
        .retained
        .lock()
        .create_operator_conversation(OperatorProvider::ChatGpt, "fixture", root.path())
        .unwrap();
    let closed = state
        .close_operator_conversation(&owner, c.id, c.run)
        .unwrap();
    assert!(closed.archived && closed.worker_stopped);
    assert!(
        matches!(sink.next(), Some(DashboardDelivery::Message(DashboardOutbound { message: ServerMessage::Event(ServerEvent::OperatorChanged(id)), .. })) if id == c.id)
    );
    state.unarchive_session(c.session, closed.run).unwrap();
    assert!(
        matches!(sink.next(), Some(DashboardDelivery::Message(DashboardOutbound { message: ServerMessage::Event(ServerEvent::OperatorChanged(id)), .. })) if id == c.id)
    );
    assert!(state.sessions.lock().unwrap().is_empty());
    drop(peer);
}

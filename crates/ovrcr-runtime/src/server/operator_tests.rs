use super::*;
use crate::retained::Disposition;
use crate::server::operator::operator_worker_argv;
use ovrcr_protocol::*;

#[test]
fn worker_argv_carries_run_identity_without_turn_identity() {
    let argv = operator_worker_argv(
        Path::new("/tmp/ovrcr-worker"),
        OperatorRunIdentity {
            conversation: OperatorConversationId(1),
            session: SessionId(2),
            run: SessionRunId(3),
        },
    );
    let args: Vec<_> = argv.iter().map(|value| value.to_string_lossy()).collect();
    assert_eq!(
        args,
        [
            "/tmp/ovrcr-worker",
            "operator",
            "worker",
            "--conversation",
            "1",
            "--session",
            "2",
            "--run",
            "3",
        ]
    );
    assert!(!args.iter().any(|arg| arg == "--turn"));
}

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
export OVRCR_TEST_OPERATOR_IDENT="$4,$6,$8"
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
    let run_identity = OperatorRunIdentity {
        conversation: OperatorConversationId(ids[0]),
        session: SessionId(ids[1]),
        run: SessionRunId(ids[2]),
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
            session: run_identity.session,
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
        session: run_identity.session,
        lease: reservation.lease,
    };
    let Response::OperatorWorker(OperatorWorkerResult::Start(start)) = worker_exchange(
        &mut stream,
        Request::OperatorWorker(OperatorWorkerRequest {
            auth: auth.clone(),
            command: OperatorWorkerCommand::Start {
                identity: run_identity,
            },
        }),
    ) else {
        panic!("start")
    };
    let identity = start.identity;
    assert_eq!(identity.conversation, run_identity.conversation);
    assert_eq!(identity.session, run_identity.session);
    assert_eq!(identity.run, run_identity.run);
    let initial_events = match &start.submission {
        OperatorWorkerSubmission::Prompt { text, .. } => {
            assert!(!text.is_empty());
            vec![
                OperatorWorkerEvent::AssistantDelta("partial".into()),
                OperatorWorkerEvent::ModelSelected {
                    provider: "fixture".into(),
                    id: "offline".into(),
                    context_window: Some(128_000),
                },
            ]
        }
        OperatorWorkerSubmission::Compact { .. } => {
            assert!(
                start.checkpoint_turn.is_some(),
                "compact must always resume a checkpoint"
            );
            vec![OperatorWorkerEvent::CompactionStarted]
        }
        OperatorWorkerSubmission::BranchSummary {
            target_entry_id, ..
        } => {
            assert!(!target_entry_id.is_empty());
            assert!(
                start.checkpoint_turn.is_some(),
                "branch summary must always resume a checkpoint"
            );
            vec![OperatorWorkerEvent::ModelSelected {
                provider: "fixture".into(),
                id: "offline".into(),
                context_window: Some(128_000),
            }]
        }
    };
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
        worker_exchange(&mut stream, batch(1, initial_events)),
        Response::OperatorExchange(_)
    ));
    let abort_requested = Arc::new(std::sync::atomic::AtomicBool::new(false));
    signal_hook::flag::register(libc::SIGUSR1, Arc::clone(&abort_requested)).unwrap();
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
    let complete = root.join(format!("complete-{}", identity.conversation.0));
    let complete_leased = root.join(format!("complete-leased-{}", identity.conversation.0));
    let fail = root.join(format!("fail-{}", identity.conversation.0));
    wait_until(|| {
        complete.exists()
            || complete_leased.exists()
            || fail.exists()
            || abort_requested.load(Ordering::Acquire)
    });
    if abort_requested.load(Ordering::Acquire) {
        // Model the native worker's soft-stop handshake: acknowledge its exact
        // interrupted turn, then retire the now-idle worker run.
        assert!(matches!(
            worker_exchange(
                &mut stream,
                Request::OperatorWorker(OperatorWorkerRequest {
                    auth: auth.clone(),
                    command: OperatorWorkerCommand::AbortTurn {
                        identity,
                        leased: false,
                    },
                })
            ),
            Response::OperatorWorker(OperatorWorkerResult::Aborted {
                identity: aborted
            }) if aborted == identity
        ));
        let _ = worker_exchange(
            &mut stream,
            Request::OperatorWorker(OperatorWorkerRequest {
                auth,
                command: OperatorWorkerCommand::Retire { identity },
            }),
        );
        return;
    }
    if fail.exists() {
        assert!(matches!(
            worker_exchange(
                &mut stream,
                batch(
                    2,
                    vec![OperatorWorkerEvent::Failed(
                        "Operator request failed; inspect the retained session before retrying"
                            .into()
                    )],
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
    let leased = complete_leased.exists();
    let final_events = match &start.submission {
        OperatorWorkerSubmission::BranchSummary { .. } => vec![
            OperatorWorkerEvent::BranchSummary {
                summary: "Summary of abandoned branch.".into(),
            },
            OperatorWorkerEvent::Completed {
                leased,
                pi_first_entry_id: Some("bs1".into()),
                pi_leaf_entry_id: Some("bs1".into()),
            },
        ],
        _ => vec![OperatorWorkerEvent::Completed {
            leased,
            pi_first_entry_id: None,
            pi_leaf_entry_id: None,
        }],
    };
    assert!(matches!(
        worker_exchange(&mut stream, batch(2, final_events)),
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
fn user_submit_wins_async_wakeup_admission_without_losing_the_report_or_turn() {
    let mut live = LiveOperator::new();
    let c = live.open();
    let first = live.submit(&c);
    let auth = live.ready(&c);
    live.command(&c, "complete-leased");
    wait_until(|| {
        live.history(&c)
            .conversation
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state == OperatorTurnState::Complete)
    });

    let owner = live.state.dashboard.snapshot().unwrap().identity;
    let (arrived_tx, arrived_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let once = AtomicBool::new(false);
    *live.state.operator.before_retained.lock().unwrap() = Some(Arc::new(move || {
        if !once.swap(true, Ordering::AcqRel) {
            arrived_tx.send(()).unwrap();
            let _ = release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5));
        }
    }));

    let submit_state = live.state.clone();
    let submit_owner = owner.clone();
    let (submit_tx, submit_rx) = mpsc::channel();
    let submit = thread::spawn(move || {
        let result = submit_state.operator_request(
            &submit_owner,
            OperatorRequest::Submit {
                conversation: c.id,
                expected_run: first.identity.run,
                text: "human report follow-up".into(),
                context: OperatorContext::default(),
            },
        );
        submit_tx.send(result).unwrap();
    });
    arrived_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("human Submit owns mutation_lock before retained commit");

    let async_state = live.state.clone();
    let run_identity = OperatorRunIdentity {
        conversation: c.id,
        session: c.session,
        run: first.identity.run,
    };
    let (async_tx, async_rx) = mpsc::channel();
    let async_request = thread::spawn(move || {
        let result = async_state.operator_worker(
            OperatorWorkerRequest {
                auth,
                command: OperatorWorkerCommand::BeginAsyncTurn {
                    identity: run_identity,
                    text: "watched session report remains queued".into(),
                },
            },
            None,
        );
        async_tx.send(result).unwrap();
    });
    assert!(
        async_rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "the async admission waits behind the already-owned human Submit"
    );
    release_tx.send(()).unwrap();

    let submitted = submit_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    submit.join().unwrap();
    assert!(matches!(submitted, OperatorResponse::Submitted(_)));
    let wakeup = async_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    async_request.join().unwrap();
    assert!(matches!(wakeup, OperatorWorkerResult::BeginAsyncTurn(None)));
    *live.state.operator.before_retained.lock().unwrap() = None;

    let history = live.history(&c);
    let user_turn = history.conversation.current_turn.as_ref().unwrap();
    assert_eq!(user_turn.state, OperatorTurnState::Running);
    assert_eq!(user_turn.identity.run, first.identity.run);
    assert!(matches!(
        history.messages.last().map(|message| &message.content),
        Some(OperatorMessageContent::User { text, .. }) if text == "human report follow-up"
    ));
    assert!(
        !history.messages.iter().any(|message| {
            matches!(message.content, OperatorMessageContent::AsyncWakeup { .. })
        })
    );
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Stop {
            identity: user_turn.identity,
        })),
        Response::Operator(OperatorResponse::Stopped(_))
    ));
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
    let stop_response = live.request(Request::Operator(OperatorRequest::Stop {
        identity: turn.identity,
    }));
    let Response::Operator(OperatorResponse::Stopped(stopped)) = stop_response else {
        panic!("unexpected Stop response: {stop_response:?}")
    };
    assert_eq!(stopped.state, OperatorTurnState::Interrupted);
    // AbortTurn publishes Interrupted before Retire publishes stop proof.
    wait_until(|| {
        let conversation = live.history(&c).conversation;
        conversation.run == turn.identity.run && conversation.worker_stopped
    });
    let stopped_history = live.history(&c);
    assert_eq!(stopped_history.conversation.run, turn.identity.run);
    assert!(stopped_history.conversation.worker_stopped);
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
fn operator_uses_dedicated_capacity_at_fifty_workspace_sessions() {
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
    let turn = live.submit(&c);
    assert_eq!(turn.identity.run, SessionRunId(1));
    live.ready(&c);

    let h = live.history(&c);
    assert!(h.messages.iter().any(|message| matches!(
        &message.content,
        OperatorMessageContent::User { text, .. } if text == "fixture prompt"
    )));
    assert_eq!(h.conversation.run, SessionRunId(1));
    let current_turn = h.conversation.current_turn.unwrap();
    assert_eq!(current_turn.identity, turn.identity);
    assert_eq!(current_turn.state, OperatorTurnState::Running);
    assert!(!h.conversation.worker_stopped);

    live.command(&c, "complete");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    assert_eq!(
        live.history(&c).conversation.current_turn.unwrap().state,
        OperatorTurnState::Complete
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
    for operation in [
        "current",
        "open",
        "read",
        "submit",
        "new_session",
        "session_info",
    ] {
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
            "new_session" => OperatorRequest::NewSession {
                conversation: c.id,
                expected_run: c.run,
            },
            "session_info" => OperatorRequest::SessionInfo { conversation: c.id },
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

#[test]
fn new_session_fences_busy_and_leased_workers_and_deletes_prior_checkpoints_outside_locks() {
    let mut live = LiveOperator::new();
    let c = live.open();

    // Stale expected_run refuses cleanly.
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::NewSession {
            conversation: c.id,
            expected_run: SessionRunId(99),
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));

    // Running turn refuses cleanly.
    let turn = live.submit(&c);
    let _auth = live.ready(&c);
    match live.request(Request::Operator(OperatorRequest::NewSession {
        conversation: c.id,
        expected_run: turn.identity.run,
    })) {
        Response::Error {
            code: ErrorCode::Conflict,
            message,
        } => assert!(
            message.contains("running") || message.contains("live") || message.contains("active"),
            "unexpected busy message: {message}"
        ),
        other => panic!("expected Conflict while running, got {other:?}"),
    }

    // Complete turn with an active warm-worker watch lease (`complete-leased`) and
    // seed a promoted checkpoint on disk.
    live.command(&c, "complete-leased");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    let leased_summary = live.current().unwrap();
    assert_eq!(
        leased_summary.current_turn.as_ref().map(|t| t.state),
        Some(OperatorTurnState::Complete)
    );
    assert!(!leased_summary.worker_stopped);
    let checkpoint_dir =
        crate::operator_session_file::checkpoint_dir(&live.state.registry_path, turn.identity.turn);
    fs::create_dir_all(&checkpoint_dir).unwrap();
    fs::set_permissions(&checkpoint_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let checkpoint_file = checkpoint_dir.join("session.jsonl");
    let workdir = live.state.registry_path.join("operator-workdir");
    let header = serde_json::json!({
        "type": "session",
        "version": 3,
        "id": format!("operator-{}", c.id.0),
        "cwd": workdir.to_str().unwrap(),
    });
    fs::write(&checkpoint_file, format!("{header}\n")).unwrap();
    fs::set_permissions(&checkpoint_file, fs::Permissions::from_mode(0o600)).unwrap();

    // /new must refuse while the warm-worker watch lease / live worker is active!
    match live.request(Request::Operator(OperatorRequest::NewSession {
        conversation: c.id,
        expected_run: leased_summary.run,
    })) {
        Response::Error {
            code: ErrorCode::Conflict,
            message,
        } => assert!(
            message.contains("lease") || message.contains("live") || message.contains("retiring"),
            "unexpected lease/busy message: {message}"
        ),
        other => panic!("expected Conflict while leased, got {other:?}"),
    }
    assert!(
        checkpoint_file.exists(),
        "refused /new must not delete checkpoint"
    );

    // Retire the leased worker -> once idle and unleased, /new succeeds, increments
    // session_epoch, and removes the prior-epoch checkpoint outside locks!
    live.command(&c, "retire");
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });
    let completed = live.current().unwrap();
    let Response::Operator(OperatorResponse::Conversation(reset)) =
        live.request(Request::Operator(OperatorRequest::NewSession {
            conversation: c.id,
            expected_run: completed.run,
        }))
    else {
        panic!("expected Conversation response from NewSession");
    };
    assert_eq!(reset.session_epoch, 1);
    assert_eq!(reset.current_turn, None);
    assert!(
        !checkpoint_file.exists(),
        "superseded checkpoint must be deleted"
    );
    assert!(live.history(&reset).messages.is_empty());
}

#[test]
fn session_info_reads_empty_populated_and_reset_epochs_with_zero_spawns() {
    let mut live = LiveOperator::new();
    let c = live.open();

    // Empty epoch before any turn: zero-spawn, clean zeroed stats.
    let Response::Operator(OperatorResponse::SessionInfo(empty)) =
        live.request(Request::Operator(OperatorRequest::SessionInfo {
            conversation: c.id,
        }))
    else {
        panic!("expected SessionInfo on empty epoch");
    };
    assert_eq!(empty.conversation, c.id);
    assert_eq!(empty.session, c.session);
    assert_eq!(empty.session_epoch, 0);
    assert_eq!(empty.pi_session_id, None);
    assert_eq!(empty.checkpoint_turn, None);
    assert_eq!(empty.turns, 0);
    assert_eq!(empty.completed_turns, 0);
    assert_eq!(empty.total_messages, 0);
    assert_eq!(empty.tokens.total, 0);
    assert_eq!(empty.cost_usd_nanos, 0);
    assert_eq!(empty.context_window.used_tokens, None);

    // Complete one turn, retire worker cleanly, and seed a promoted checkpoint + models.json.
    let turn = live.submit(&c);
    let _auth = live.ready(&c);
    live.command(&c, "complete");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    live.command(&c, "retire");
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });
    let workdir = live.state.registry_path.join("operator-workdir");
    let pi_dir = live.state.registry_path.join("operator-pi");
    fs::create_dir_all(&pi_dir).unwrap();
    fs::set_permissions(&pi_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let models_file = pi_dir.join("models.json");
    fs::write(
        &models_file,
        serde_json::to_string(&serde_json::json!({
            "providers": {
                "fixture": {
                    "apiKey": "do-not-leak",
                    "models": [{ "id": "offline", "contextWindow": 128_000 }]
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&models_file, fs::Permissions::from_mode(0o600)).unwrap();

    let checkpoint_dir =
        crate::operator_session_file::checkpoint_dir(&live.state.registry_path, turn.identity.turn);
    fs::create_dir_all(&checkpoint_dir).unwrap();
    fs::set_permissions(&checkpoint_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let checkpoint_file = checkpoint_dir.join("session.jsonl");
    let session_body = [
        serde_json::json!({
            "type": "session",
            "version": 3,
            "id": format!("operator-{}", c.id.0),
            "cwd": workdir.to_str().unwrap(),
        }),
        serde_json::json!({
            "type": "message",
            "message": { "role": "user", "content": [{ "type": "text", "text": "hi" }] }
        }),
        serde_json::json!({
            "type": "message",
            "message": {
                "role": "assistant",
                "provider": "fixture",
                "model": "offline",
                "stopReason": "stop",
                "content": [{ "type": "text", "text": "hello" }],
                "usage": {
                    "input": 1000,
                    "output": 250,
                    "cacheRead": 500,
                    "cacheWrite": 50,
                    "totalTokens": 1800,
                    "cost": { "total": 0.0042 }
                }
            }
        }),
    ]
    .iter()
    .map(|v| format!("{}\n", serde_json::to_string(v).unwrap()))
    .collect::<String>();
    fs::write(&checkpoint_file, session_body).unwrap();
    fs::set_permissions(&checkpoint_file, fs::Permissions::from_mode(0o600)).unwrap();

    let Response::Operator(OperatorResponse::SessionInfo(populated)) =
        live.request(Request::Operator(OperatorRequest::SessionInfo {
            conversation: c.id,
        }))
    else {
        panic!("expected SessionInfo on populated epoch");
    };
    assert_eq!(populated.pi_session_id.as_deref(), Some("operator-1"));
    assert_eq!(populated.checkpoint_turn, Some(turn.identity.turn));
    assert_eq!(populated.turns, 1);
    assert_eq!(populated.completed_turns, 1);
    assert_eq!(populated.user_messages, 1);
    assert_eq!(populated.assistant_messages, 1);
    assert_eq!(populated.total_messages, 2);
    assert_eq!(populated.tokens.total, 1800);
    assert_eq!(populated.cost_usd_nanos, 4_200_000);
    assert_eq!(populated.context_window.used_tokens, Some(1800));
    assert_eq!(populated.context_window.capacity_tokens, Some(128_000));

    // Reset with /new -> SessionInfo immediately reflects clean epoch 1!
    let settled = live.current().unwrap();
    let Response::Operator(OperatorResponse::Conversation(_)) =
        live.request(Request::Operator(OperatorRequest::NewSession {
            conversation: c.id,
            expected_run: settled.run,
        }))
    else {
        panic!("expected Conversation from NewSession");
    };
    let Response::Operator(OperatorResponse::SessionInfo(after_reset)) =
        live.request(Request::Operator(OperatorRequest::SessionInfo {
            conversation: c.id,
        }))
    else {
        panic!("expected SessionInfo after /new");
    };
    assert_eq!(after_reset.session_epoch, 1);
    assert_eq!(after_reset.pi_session_id, None);
    assert_eq!(after_reset.checkpoint_turn, None);
    assert_eq!(after_reset.turns, 0);
    assert_eq!(after_reset.completed_turns, 0);
    assert_eq!(after_reset.total_messages, 0);
    assert_eq!(after_reset.tokens.total, 0);
    assert_eq!(after_reset.cost_usd_nanos, 0);
    assert_eq!(after_reset.context_window.used_tokens, None);
}

#[test]
fn operator_models_runs_off_dashboard_reader_with_isolated_home_and_serial_owner() {
    let mut live = LiveOperator::new();
    assert!(matches!(
        live.control(Request::Operator(OperatorRequest::Models)),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));

    let started = live.root.path().join("models-started");
    let release = live.root.path().join("models-release");
    let receipt = live.root.path().join("models-env");
    let script = live.root.path().join("ovrcr");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
printf '%s\n' "$1 $2 $3" > '{receipt}'
printf 'home=%s\n' "${{OVRCR_HOME:-}}" >> '{receipt}'
printf 'config=%s\n' "${{OVRCR_CONFIG:-}}" >> '{receipt}'
printf 'socket=%s\n' "${{OVRCR_SOCKET:-}}" >> '{receipt}'
printf 'agent_token=%s\n' "${{OVRCR_AGENT_TOKEN:-}}" >> '{receipt}'
printf 'delegation_token=%s\n' "${{OVRCR_DELEGATION_TOKEN:-}}" >> '{receipt}'
: > '{started}'
while [ ! -f '{release}' ]; do
  sleep 0.01
done
printf 'No Pi model selected; run ovrcr operator setup.\n' >&2
printf '{{"models":[{{"provider":"openai","id":"gpt-5.4","name":"GPT-5.4"}},{{"provider":"vendor","id":"sub/id","name":"sub/id"}}]}}'
"#,
            receipt = receipt.display(),
            started = started.display(),
            release = release.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();

    // Send Models without waiting yet so we can prove the Dashboard reader stays live.
    live.next_request += 1;
    let models_req_id = live.next_request;
    write_frame(
        live.dashboard.as_mut().unwrap(),
        &ClientMessage {
            request_id: models_req_id,
            request: Request::Operator(OperatorRequest::Models),
        },
    )
    .unwrap();
    wait_until(|| started.exists());

    // While the catalog subprocess is blocked on `release`, the Dashboard reader
    // still services `Current` immediately, and a second concurrent `Models`
    // request is refused cleanly instead of spawning a second catalog process.
    assert!(live.current().is_none());
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Models)),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));

    fs::write(&release, b"go").unwrap();
    let models_response = loop {
        match read_frame::<ServerMessage>(live.dashboard.as_mut().unwrap()).unwrap() {
            ServerMessage::Response {
                request_id,
                response,
            } if request_id == models_req_id => break response,
            _ => {}
        }
    };
    assert_eq!(
        models_response,
        Response::Operator(OperatorResponse::Models(vec![
            OperatorModelCatalogEntry {
                provider: "openai".into(),
                id: "gpt-5.4".into(),
                name: Some("GPT-5.4".into()),
            },
            OperatorModelCatalogEntry {
                provider: "vendor".into(),
                id: "sub/id".into(),
                name: None,
            },
        ]))
    );
    assert!(
        live.current().is_none(),
        "listing models must not open or create an Operator conversation"
    );

    let recorded = fs::read_to_string(&receipt).unwrap();
    assert!(recorded.contains("operator models --json\n"), "{recorded}");
    assert!(
        recorded.contains(&format!("home={}\n", live.root.path().display())),
        "{recorded}"
    );
    assert!(recorded.contains("config=\n"), "{recorded}");
    assert!(recorded.contains("socket=\n"), "{recorded}");
    assert!(recorded.contains("agent_token=\n"), "{recorded}");
    assert!(recorded.contains("delegation_token=\n"), "{recorded}");
}

#[test]
fn operator_models_redacts_failures_and_reaps_timed_out_process_group() {
    let mut live = LiveOperator::new();
    let script = live.root.path().join("ovrcr");

    // 1. Non-zero exit with secret material on stdout/stderr is redacted.
    fs::write(
        &script,
        r#"#!/bin/sh
printf 'Bearer sk-secret-stderr-123\n' >&2
printf 'sk-secret-stdout-456\n'
exit 1
"#,
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();

    let Response::Error { code, message } =
        live.request(Request::Operator(OperatorRequest::Models))
    else {
        panic!("expected redacted error")
    };
    assert_eq!(code, ErrorCode::Internal);
    assert!(
        !message.contains("sk-secret-stderr-123") && !message.contains("sk-secret-stdout-456"),
        "native output leaked in error: {message:?}"
    );

    // 2. Timeout reaps the entire process group including background descendants.
    *live.state.operator.models_deadline.lock().unwrap() = Some(Duration::from_millis(120));
    let descendant_pid_file = live.root.path().join("descendant.pid");
    fs::write(
        &script,
        format!(
            r#"#!/bin/sh
sleep 60 &
printf '%s\n' "$!" > '{}'
wait
"#,
            descendant_pid_file.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();

    let Response::Error { message, .. } = live.request(Request::Operator(OperatorRequest::Models))
    else {
        panic!("expected timeout error")
    };
    assert!(
        message.contains("timed out"),
        "unexpected timeout message: {message:?}"
    );
    let pid: libc::pid_t = fs::read_to_string(&descendant_pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    wait_until(|| unsafe { libc::kill(pid, 0) != 0 });
}

#[test]
fn operator_compact_refuses_empty_or_busy_and_dispatches_cold_and_warm_submissions() {
    let mut live = LiveOperator::new();
    let c = live.open();

    // Non-dashboard client cannot invoke Compact.
    assert!(matches!(
        live.control(Request::Operator(OperatorRequest::Compact {
            conversation: c.id,
            expected_run: c.run,
            instructions: None,
        })),
        Response::Error {
            code: ErrorCode::InvalidRequest,
            ..
        }
    ));

    // Before the first completed turn, Compact is refused cleanly without starting a worker.
    let Response::Error { code, message } =
        live.request(Request::Operator(OperatorRequest::Compact {
            conversation: c.id,
            expected_run: c.run,
            instructions: None,
        }))
    else {
        panic!("expected refusal before first completed turn")
    };
    assert_eq!(code, ErrorCode::Conflict);
    assert!(
        message.contains("before the first completed turn"),
        "{message}"
    );
    assert!(live.history(&c).messages.is_empty());

    // Start turn 1 and verify Compact refuses while turn 1 is Running.
    let turn1 = live.submit(&c);
    let _auth1 = live.ready(&c);
    let Response::Error {
        code: busy_code,
        message: busy_msg,
    } = live.request(Request::Operator(OperatorRequest::Compact {
        conversation: c.id,
        expected_run: turn1.identity.run,
        instructions: None,
    }))
    else {
        panic!("expected busy refusal while turn 1 is running")
    };
    assert_eq!(busy_code, ErrorCode::Conflict);
    assert!(busy_msg.contains("busy"), "{busy_msg}");

    // Complete turn 1 with a warm lease (`complete-leased`), then submit warm Compact.
    live.command(&c, "complete-leased");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    let leased_summary = live.current().unwrap();
    let Response::Operator(OperatorResponse::Submitted(warm_compact)) =
        live.request(Request::Operator(OperatorRequest::Compact {
            conversation: c.id,
            expected_run: leased_summary.run,
            instructions: Some("keep open branch names".into()),
        }))
    else {
        panic!("expected warm Compact submission")
    };
    assert_eq!(warm_compact.identity.run, leased_summary.run);
    let warm_history = live.history(&c);
    assert_eq!(
        warm_history.messages.last().map(|m| &m.content),
        Some(&OperatorMessageContent::Compaction {
            instructions: Some("keep open branch names".into()),
        })
    );

    // Interrupt and retire the warm worker, then verify cold Compact spawns a new run.
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Stop {
            identity: warm_compact.identity,
        })),
        Response::Operator(OperatorResponse::Stopped(_))
    ));
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });
    let stopped_summary = live.current().unwrap();
    let _ = fs::remove_file(live.root.path().join(format!("ready-{}", c.id.0)));
    let _ = fs::remove_file(live.root.path().join(format!("acked-{}", c.id.0)));
    let _ = fs::remove_file(live.root.path().join(format!("complete-leased-{}", c.id.0)));
    let Response::Operator(OperatorResponse::Submitted(cold_compact)) =
        live.request(Request::Operator(OperatorRequest::Compact {
            conversation: c.id,
            expected_run: stopped_summary.run,
            instructions: None,
        }))
    else {
        panic!("expected cold Compact submission")
    };
    assert_eq!(cold_compact.identity.run.0, stopped_summary.run.0 + 1);
    let _cold_auth = live.ready(&c);
    let cold_history = live.history(&c);
    assert!(cold_history.messages.iter().any(|m| {
        m.turn == cold_compact.identity.turn
            && m.content == OperatorMessageContent::Compaction { instructions: None }
    }));
    assert!(cold_history.messages.iter().any(|m| {
        m.turn == cold_compact.identity.turn
            && m.content == OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted)
    }));
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Stop {
            identity: cold_compact.identity,
        })),
        Response::Operator(OperatorResponse::Stopped(_))
    ));
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });

    // After /new resets onto epoch 1, Compact is refused again until a turn completes in epoch 1.
    let before_new = live.current().unwrap();
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::NewSession {
            conversation: c.id,
            expected_run: before_new.run,
        })),
        Response::Operator(OperatorResponse::Conversation(_))
    ));
    let after_new = live.current().unwrap();
    let Response::Error {
        code: post_new_code,
        message: post_new_msg,
    } = live.request(Request::Operator(OperatorRequest::Compact {
        conversation: c.id,
        expected_run: after_new.run,
        instructions: None,
    }))
    else {
        panic!("expected refusal after /new")
    };
    assert_eq!(post_new_code, ErrorCode::Conflict);
    assert!(
        post_new_msg.contains("before the first completed turn"),
        "{post_new_msg}"
    );
}

#[test]
fn tree_and_navigate_tree_inspect_busy_sessions_switch_branches_and_restore_prompts() {
    let mut live = LiveOperator::new();
    let c = live.open();

    // 1. Empty epoch: Tree returns default empty tree; NavigateTree refuses cleanly.
    let Response::Operator(OperatorResponse::Tree(empty_tree)) =
        live.request(Request::Operator(OperatorRequest::Tree {
            conversation: c.id,
        }))
    else {
        panic!("expected empty Tree on fresh epoch");
    };
    assert_eq!(empty_tree.pi_session_id, None);
    assert!(empty_tree.entries.is_empty());

    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::NavigateTree {
            conversation: c.id,
            expected_run: c.run,
            target_entry_id: "u1".into(),
            summarize: None,
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));

    // 2. Complete Turn 1 with a leased watch first so we can verify leased refusal + read-only Tree.
    let ctx1 = OperatorContext {
        project: Some("ovrcr".into()),
        workspace: Some("root-ws".into()),
        cwd: Some(live.root.path().to_path_buf()),
        ..OperatorContext::default()
    };
    let Response::Operator(OperatorResponse::Submitted(t1)) =
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: c.id,
            expected_run: c.run,
            text: "first prompt".into(),
            context: ctx1.clone(),
        }))
    else {
        panic!("submit t1");
    };
    let _auth1 = live.ready(&c);
    live.command(&c, "complete-leased");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());

    // Seed promoted checkpoint for t1 (`u1 -> a1`).
    let registry_path = live.state.registry_path.clone();
    let workdir = registry_path.join("operator-workdir");
    fs::create_dir_all(&workdir).unwrap();
    fs::set_permissions(&workdir, fs::Permissions::from_mode(0o700)).unwrap();
    let write_checkpoint = |turn: OperatorTurnId, entries: &[serde_json::Value]| {
        let dir = crate::operator_session_file::checkpoint_dir(&registry_path, turn);
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let file = dir.join("session.jsonl");
        let mut body = format!(
            "{}\n",
            serde_json::to_string(&serde_json::json!({
                "type": "session",
                "version": 3,
                "id": format!("operator-{}", c.id.0),
                "cwd": workdir.to_str().unwrap(),
            }))
            .unwrap()
        );
        for entry in entries {
            body.push_str(&serde_json::to_string(entry).unwrap());
            body.push('\n');
        }
        fs::write(&file, body).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    };
    let t1_entries = vec![
        serde_json::json!({
            "type": "message",
            "id": "u1",
            "parentId": null,
            "message": { "role": "user", "content": "wrapped first prompt" }
        }),
        serde_json::json!({
            "type": "message",
            "id": "a1",
            "parentId": "u1",
            "message": { "role": "assistant", "stopReason": "stop", "content": [{ "type": "text", "text": "hello" }] }
        }),
    ];
    write_checkpoint(t1.identity.turn, &t1_entries);

    // Read-only Tree succeeds while leased worker is live, and unwraps `u1` from SQLite!
    let Response::Operator(OperatorResponse::Tree(leased_tree)) =
        live.request(Request::Operator(OperatorRequest::Tree {
            conversation: c.id,
        }))
    else {
        panic!("expected Tree while leased");
    };
    assert_eq!(leased_tree.current_leaf_id.as_deref(), Some("a1"));
    assert_eq!(leased_tree.entries.len(), 2);
    assert_eq!(leased_tree.entries[0].preview, "first prompt");

    // NavigateTree MUST refuse while leased worker is live!
    let leased_summary = live.current().unwrap();
    let Response::Error {
        code: leased_code,
        message: leased_msg,
    } = live.request(Request::Operator(OperatorRequest::NavigateTree {
        conversation: c.id,
        expected_run: leased_summary.run,
        target_entry_id: "u1".into(),
        summarize: None,
    }))
    else {
        panic!("expected leased NavigateTree refusal");
    };
    assert_eq!(leased_code, ErrorCode::Conflict);
    assert!(leased_msg.contains("active"), "{leased_msg}");

    // Retire worker cleanly.
    let _ = fs::remove_file(live.root.path().join(format!("complete-leased-{}", c.id.0)));
    live.command(&c, "retire");
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });

    // 3. Submit Turn 2 (Branch A) and verify Tree works while Turn 2 is Running, while NavigateTree refuses.
    let _ = fs::remove_file(live.root.path().join(format!("acked-{}", c.id.0)));
    let _ = fs::remove_file(live.root.path().join(format!("retire-{}", c.id.0)));
    let _ = fs::remove_file(live.root.path().join(format!("complete-{}", c.id.0)));
    let _ = fs::remove_file(live.root.path().join(format!("ready-{}", c.id.0)));
    let after_t1 = live.current().unwrap();
    let ctx2a = OperatorContext {
        project: Some("ovrcr".into()),
        workspace: Some("branch-a".into()),
        cwd: Some(live.root.path().to_path_buf()),
        ..OperatorContext::default()
    };
    let Response::Operator(OperatorResponse::Submitted(t2)) =
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: c.id,
            expected_run: after_t1.run,
            text: "branch A prompt".into(),
            context: ctx2a.clone(),
        }))
    else {
        panic!("submit t2");
    };
    let _auth2 = live.ready(&c);
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Tree {
            conversation: c.id
        })),
        Response::Operator(OperatorResponse::Tree(_))
    ));
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::NavigateTree {
            conversation: c.id,
            expected_run: t2.identity.run,
            target_entry_id: "a1".into(),
            summarize: None,
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));

    live.command(&c, "complete");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    live.command(&c, "retire");
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });

    let mut t2_entries = t1_entries.clone();
    t2_entries.push(serde_json::json!({
        "type": "message",
        "id": "u2a",
        "parentId": "a1",
        "message": { "role": "user", "content": "wrapped branch A prompt" }
    }));
    t2_entries.push(serde_json::json!({
        "type": "message",
        "id": "a2a",
        "parentId": "u2a",
        "message": { "role": "assistant", "stopReason": "stop", "content": [{ "type": "text", "text": "hello A" }] }
    }));
    write_checkpoint(t2.identity.turn, &t2_entries);

    // Record entry IDs on t1 & t2 in SQLite so multi-branch correlation matches worker Completed.
    {
        let retained = live.state.retained.lock();
        retained
            .connection
            .execute(
                "UPDATE operator_turns SET pi_first_entry_id='u1', pi_leaf_entry_id='a1' WHERE id=?1",
                [t1.identity.turn.0 as i64],
            )
            .unwrap();
        retained
            .connection
            .execute(
                "UPDATE operator_turns SET pi_first_entry_id='u2a', pi_leaf_entry_id='a2a' WHERE id=?1",
                [t2.identity.turn.0 as i64],
            )
            .unwrap();
    }

    // 4. Selecting the already-active assistant leaf `a2a` is a clean noop.
    let idle = live.current().unwrap();
    let Response::Operator(OperatorResponse::TreeNavigated(noop_nav)) =
        live.request(Request::Operator(OperatorRequest::NavigateTree {
            conversation: c.id,
            expected_run: idle.run,
            target_entry_id: "a2a".into(),
            summarize: None,
        }))
    else {
        panic!("expected noop TreeNavigated");
    };
    assert!(noop_nav.noop);
    assert_eq!(noop_nav.submitted, None);
    assert_eq!(noop_nav.current_leaf_id.as_deref(), Some("a2a"));
    assert_eq!(noop_nav.editor_text, None);

    // 5. Selecting user entry `u2a` rewinds active branch to `a1` (`t1`) and restores unwrapped prompt + context!
    let Response::Operator(OperatorResponse::TreeNavigated(rewind_nav)) =
        live.request(Request::Operator(OperatorRequest::NavigateTree {
            conversation: c.id,
            expected_run: idle.run,
            target_entry_id: "u2a".into(),
            summarize: None,
        }))
    else {
        panic!("expected user rewind TreeNavigated");
    };
    assert!(!rewind_nav.noop);
    assert_eq!(rewind_nav.submitted, None);
    assert_eq!(rewind_nav.current_leaf_id.as_deref(), Some("a1"));
    assert_eq!(rewind_nav.editor_text.as_deref(), Some("branch A prompt"));
    assert_eq!(rewind_nav.restored_context, Some(ctx2a));
    assert_eq!(live.history(&c).messages.len(), 2);

    // SessionInfo reflects active branch (1 turn, 2 messages) vs epoch (2 turns, 4 messages)!
    let Response::Operator(OperatorResponse::SessionInfo(rewound_info)) =
        live.request(Request::Operator(OperatorRequest::SessionInfo {
            conversation: c.id,
        }))
    else {
        panic!("expected SessionInfo after rewind");
    };
    assert_eq!(rewound_info.active_tip_turn, Some(t1.identity.turn));
    assert_eq!(rewound_info.branch_turns, 1);
    assert_eq!(rewound_info.completed_turns, 2);
    assert_eq!(rewound_info.total_messages, 2);
    assert_eq!(rewound_info.epoch_total_messages, 4);

    // Switch back forward to `a2a` -> Branch A feed (4 messages) is restored!
    let Response::Operator(OperatorResponse::TreeNavigated(forward_nav)) =
        live.request(Request::Operator(OperatorRequest::NavigateTree {
            conversation: c.id,
            expected_run: idle.run,
            target_entry_id: "a2a".into(),
            summarize: None,
        }))
    else {
        panic!("expected forward TreeNavigated");
    };
    assert!(!forward_nav.noop);
    assert_eq!(forward_nav.submitted, None);
    assert_eq!(forward_nav.current_leaf_id.as_deref(), Some("a2a"));
    assert_eq!(forward_nav.editor_text, None);
    assert_eq!(live.history(&c).messages.len(), 4);
}

#[test]
fn branch_summary_turn_submits_completes_and_preserves_branch_on_stop_or_error() {
    use ovrcr_protocol::operator::OperatorBranchSummaryRequest;

    let mut live = LiveOperator::new();
    let c = live.open();
    let registry_path = live.state.registry_path.clone();
    let workdir = registry_path.join("operator-workdir");
    fs::create_dir_all(&workdir).unwrap();
    fs::set_permissions(&workdir, fs::Permissions::from_mode(0o700)).unwrap();
    let write_checkpoint = |turn: OperatorTurnId, entries: &[serde_json::Value]| {
        let dir = crate::operator_session_file::checkpoint_dir(&registry_path, turn);
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let file = dir.join("session.jsonl");
        let mut body = format!(
            "{}\n",
            serde_json::to_string(&serde_json::json!({
                "type": "session",
                "version": 3,
                "id": format!("operator-{}", c.id.0),
                "cwd": workdir.to_str().unwrap(),
            }))
            .unwrap()
        );
        for entry in entries {
            body.push_str(&serde_json::to_string(entry).unwrap());
            body.push('\n');
        }
        fs::write(&file, body).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    };
    let root_path = live.root.path().to_path_buf();
    let cleanup_worker_flags = |id: u64| {
        for prefix in [
            "acked",
            "retire",
            "complete",
            "complete-leased",
            "fail",
            "ready",
        ] {
            let _ = fs::remove_file(root_path.join(format!("{prefix}-{id}")));
        }
    };

    // Turn 1 (`u1 -> a1`) and Turn 2 (`u2a -> a2a`).
    let Response::Operator(OperatorResponse::Submitted(t1)) =
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: c.id,
            expected_run: c.run,
            text: "root prompt".into(),
            context: OperatorContext::default(),
        }))
    else {
        panic!("submit t1");
    };
    let _ = live.ready(&c);
    live.command(&c, "complete");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    live.command(&c, "retire");
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });
    cleanup_worker_flags(c.id.0);

    let after_t1 = live.current().unwrap();
    let ctx2a = OperatorContext {
        project: Some("ovrcr".into()),
        workspace: Some("branch-a".into()),
        cwd: Some(live.root.path().to_path_buf()),
        ..OperatorContext::default()
    };
    let Response::Operator(OperatorResponse::Submitted(t2)) =
        live.request(Request::Operator(OperatorRequest::Submit {
            conversation: c.id,
            expected_run: after_t1.run,
            text: "branch A prompt".into(),
            context: ctx2a.clone(),
        }))
    else {
        panic!("submit t2");
    };
    let _ = live.ready(&c);
    live.command(&c, "complete");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    live.command(&c, "retire");
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });
    cleanup_worker_flags(c.id.0);

    let entries = vec![
        serde_json::json!({
            "type": "message",
            "id": "u1",
            "parentId": null,
            "message": { "role": "user", "content": "wrapped root prompt" }
        }),
        serde_json::json!({
            "type": "message",
            "id": "a1",
            "parentId": "u1",
            "message": { "role": "assistant", "stopReason": "stop", "content": [{ "type": "text", "text": "root answer" }] }
        }),
        serde_json::json!({
            "type": "message",
            "id": "u2a",
            "parentId": "a1",
            "message": { "role": "user", "content": "wrapped branch A prompt" }
        }),
        serde_json::json!({
            "type": "message",
            "id": "a2a",
            "parentId": "u2a",
            "message": { "role": "assistant", "stopReason": "stop", "content": [{ "type": "text", "text": "branch A answer" }] }
        }),
    ];
    write_checkpoint(t2.identity.turn, &entries);
    {
        let retained = live.state.retained.lock();
        retained
            .connection
            .execute(
                "UPDATE operator_turns SET pi_first_entry_id='u1', pi_leaf_entry_id='a1' WHERE id=?1",
                [t1.identity.turn.0 as i64],
            )
            .unwrap();
        retained
            .connection
            .execute(
                "UPDATE operator_turns SET pi_first_entry_id='u2a', pi_leaf_entry_id='a2a' WHERE id=?1",
                [t2.identity.turn.0 as i64],
            )
            .unwrap();
    }

    // 1. Summarizing the already-active leaf `a2a` refuses cleanly (zero abandoned entries).
    let idle = live.current().unwrap();
    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::NavigateTree {
            conversation: c.id,
            expected_run: idle.run,
            target_entry_id: "a2a".into(),
            summarize: Some(OperatorBranchSummaryRequest { instructions: None }),
        })),
        Response::Error {
            code: ErrorCode::Conflict,
            ..
        }
    ));

    // 2. Summarize while rewinding `u2a`, then Stop mid-summarization -> preserves `t2` active tip & checkpoint!
    let Response::Operator(OperatorResponse::TreeNavigated(stop_nav)) =
        live.request(Request::Operator(OperatorRequest::NavigateTree {
            conversation: c.id,
            expected_run: idle.run,
            target_entry_id: "u2a".into(),
            summarize: Some(OperatorBranchSummaryRequest {
                instructions: Some("preserve findings".into()),
            }),
        }))
    else {
        panic!("expected TreeNavigated with submitted turn");
    };
    let stop_turn = stop_nav
        .submitted
        .expect("expected submitted BranchSummary turn");
    assert_eq!(stop_nav.editor_text.as_deref(), Some("branch A prompt"));
    assert_eq!(stop_nav.restored_context, Some(ctx2a.clone()));
    let _ = live.ready(&c);

    // While running, live history includes `t1`, `t2`, and the running BranchSummary opener.
    let running_history = live.history(&c);
    assert!(running_history.messages.iter().any(|m| matches!(
        &m.content,
        OperatorMessageContent::BranchSummary {
            target_entry_id,
            instructions: Some(inst),
        } if target_entry_id == "u2a" && inst == "preserve findings"
    )));

    assert!(matches!(
        live.request(Request::Operator(OperatorRequest::Stop {
            identity: stop_turn.identity,
        })),
        Response::Operator(OperatorResponse::Stopped(_))
    ));
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });
    cleanup_worker_flags(c.id.0);

    let Response::Operator(OperatorResponse::SessionInfo(after_stop_info)) =
        live.request(Request::Operator(OperatorRequest::SessionInfo {
            conversation: c.id,
        }))
    else {
        panic!("expected SessionInfo after Stop");
    };
    assert_eq!(after_stop_info.checkpoint_turn, Some(t2.identity.turn));
    assert_eq!(after_stop_info.active_tip_turn, Some(t2.identity.turn));

    // 3. Summarize while rewinding `u2a`, then Fail -> preserves `t2` active tip & checkpoint!
    let after_stop = live.current().unwrap();
    let Response::Operator(OperatorResponse::TreeNavigated(fail_nav)) =
        live.request(Request::Operator(OperatorRequest::NavigateTree {
            conversation: c.id,
            expected_run: after_stop.run,
            target_entry_id: "u2a".into(),
            summarize: Some(OperatorBranchSummaryRequest { instructions: None }),
        }))
    else {
        panic!("expected TreeNavigated for failing BranchSummary");
    };
    assert!(fail_nav.submitted.is_some());
    let _ = live.ready(&c);
    live.command(&c, "fail");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    live.command(&c, "retire");
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });
    cleanup_worker_flags(c.id.0);

    let Response::Operator(OperatorResponse::SessionInfo(after_fail_info)) =
        live.request(Request::Operator(OperatorRequest::SessionInfo {
            conversation: c.id,
        }))
    else {
        panic!("expected SessionInfo after Fail");
    };
    assert_eq!(after_fail_info.checkpoint_turn, Some(t2.identity.turn));
    assert_eq!(after_fail_info.active_tip_turn, Some(t2.identity.turn));

    // 4. Summarize while rewinding `u2a`, then Complete -> advances `active_tip_turn` to the
    // BranchSummary turn parented at `t1` (`u1 -> a1`), hiding `t2` (`u2a -> a2a`) from active feed!
    let after_fail = live.current().unwrap();
    let Response::Operator(OperatorResponse::TreeNavigated(ok_nav)) =
        live.request(Request::Operator(OperatorRequest::NavigateTree {
            conversation: c.id,
            expected_run: after_fail.run,
            target_entry_id: "u2a".into(),
            summarize: Some(OperatorBranchSummaryRequest {
                instructions: Some("focus on root cause".into()),
            }),
        }))
    else {
        panic!("expected TreeNavigated for completing BranchSummary");
    };
    let ok_turn = ok_nav.submitted.expect("expected submitted turn");
    let _ = live.ready(&c);
    live.command(&c, "complete");
    wait_until(|| live.root.path().join(format!("acked-{}", c.id.0)).exists());
    live.command(&c, "retire");
    wait_until(|| {
        live.current()
            .is_some_and(|summary| summary.worker_stopped && !summary.worker_live)
    });
    cleanup_worker_flags(c.id.0);

    let mut summary_entries = entries.clone();
    summary_entries.push(serde_json::json!({
        "type": "branch_summary",
        "id": "bs1",
        "parentId": "a1",
        "fromId": "a2a",
        "summary": "Summary of abandoned branch."
    }));
    write_checkpoint(ok_turn.identity.turn, &summary_entries);

    let completed_history = live.history(&c);
    let turns_in_feed: Vec<OperatorTurnId> =
        completed_history.messages.iter().map(|m| m.turn).collect();
    assert!(turns_in_feed.contains(&t1.identity.turn));
    assert!(!turns_in_feed.contains(&t2.identity.turn));
    assert!(turns_in_feed.contains(&ok_turn.identity.turn));
    assert!(completed_history.messages.iter().any(|m| matches!(
        &m.content,
        OperatorMessageContent::Worker(OperatorWorkerEvent::BranchSummary { summary })
            if summary == "Summary of abandoned branch."
    )));
}

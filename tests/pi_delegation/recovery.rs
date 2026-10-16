//! Actual registered lifecycle hooks -> production CLI -> isolated Server and surviving PTYs.
use super::followups::{OwnedFollowup, complete};
use super::*;

fn inspect(fixture: &RecoveryFixture, handle: &str) -> AssignmentSnapshot {
    serde_json::from_value(fixture.tool("ovrcr_inspect", serde_json::json!({"handle":handle})))
        .unwrap()
}
fn workflow(fixture: &RecoveryFixture) -> WorkflowSnapshot {
    serde_json::from_value(fixture.tool("ovrcr_recover", serde_json::json!({}))).unwrap()
}
fn event(fixture: &RecoveryFixture, value: &str) {
    assert_eq!(
        fixture.live.request(Request::SendTerminal {
            session: fixture.parent,
            text: value.into(),
            submit: true
        }),
        Response::Ok
    );
}
fn parent(fixture: &RecoveryFixture) -> SessionSummary {
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory");
    };
    sessions
        .into_iter()
        .find(|s| s.id == fixture.parent)
        .unwrap()
}
fn index() -> serde_json::Value {
    serde_json::json!({"schema":1,"objective":"objective-v1.json","decisions":["decision-v1.json"],"unresolved_assignments":[],"questions":["question-v1.json"],"receipts":["original dispatch receipt"],"accepted_evidence":[],"next_actions":["inspect surviving child before new work"]})
}
fn original_auth(fixture: &RecoveryFixture) -> DelegationAuth {
    let DelegationRequest::Prepare { auth, .. } =
        fixture.proxy.original(DispatchOperation::Prepare)
    else {
        unreachable!()
    };
    auth
}

#[test]
fn parent_recovery_compaction_and_orderly_reload_preserve_original_children_and_fence_old_source() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    let before = workflow(&fixture);
    let saved: WorkflowSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_checkpoint", serde_json::json!({"index":index()})),
    )
    .unwrap();
    let child_bytes = fixture.byte_count(&first);
    let original = fs::read(&first.result_path).unwrap();
    for command in ["session_compact", "session_compact_failed"] {
        event(&fixture, command);
        let after = workflow(&fixture);
        assert_eq!(
            after.authority, before.authority,
            "compaction is lookup, not transfer"
        );
        assert_eq!(after.pointer, saved.pointer);
        assert_eq!(after.continuation, saved.continuation);
        assert!(after.handles.contains(&first.handle));
    }
    let native = before.authority.binding.conversation.clone();
    event(&fixture, &format!("session_replace:{native}:reload"));
    let after = workflow(&fixture);
    assert_eq!(after.authority.epoch, before.authority.epoch + 1);
    assert_eq!(after.authority.binding, before.authority.binding);
    assert_ne!(after.authority.producer, before.authority.producer);
    assert_eq!(after.authority.workflow, before.authority.workflow);
    assert_eq!(after.continuation, saved.continuation);
    let again: AssignmentSnapshot = serde_json::from_value(fixture.recover(&first.handle)).unwrap();
    assert_eq!(again.identity, first.identity);
    assert_eq!(again.receipts, first.receipts);
    assert_eq!(fixture.child_count(), 1);
    assert_eq!(fixture.byte_count(&first), child_bytes);
    assert_eq!(fs::read(&first.result_path).unwrap(), original);
    event(
        &fixture,
        &format!("forge_workflow_source:{}:2", after.authority.producer),
    );
    // A rooted request with the exact next sequence cannot overwrite an admitted source key.
    assert_eq!(workflow(&fixture).authority, after.authority);
    let mut stale = original_auth(&fixture);
    // Public replacement identity/epoch and the shared run capability are insufficient.
    stale.controller = Some(after.authority.fence());
    for source in [stale.source.clone(), None, Some(AgentSecret([0x12; 32]))] {
        stale.source = source;
        assert!(matches!(
            fixture
                .live
                .request(Request::Workflow(Box::new(WorkflowRequest::Recover {
                    auth: stale.clone(),
                    workflow: Some(after.authority.workflow.clone())
                }))),
            Response::Error { .. }
        ));
    }
    let native_history = fixture
        .live
        .root
        .path()
        .join("history")
        .join(format!("{native}.jsonl"));
    let native_history = fs::read_to_string(native_history).unwrap();
    assert!(native_history.contains("ovrcr_workflow_pointer"));
    let secret = serde_json::to_string(&original_auth(&fixture).source).unwrap();
    assert!(
        !native_history.contains(&secret),
        "native history must not retain source capability"
    );
    assert!(
        !serde_json::to_string(&after).unwrap().contains(&secret),
        "snapshots must not expose source capability"
    );
    let inspected = inspect(&fixture, &first.handle);
    assert_eq!(inspected.control, first.control);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn parent_recovery_retired_source_cannot_save_after_unproven_replacement_at_final_fence() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    let original = workflow(&fixture);
    let bytes = fixture.byte_count(&first);
    let auth = original_auth(&fixture);
    let fault = fixture.live.root.path().join("fault");
    let release = fault.with_extension("release");
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "1");
        }
    }
    let _release = Release(release.clone());
    fs::write(
        &fault,
        format!("workflow-before-save|{}|0", original.authority.workflow),
    )
    .unwrap();
    let socket = fixture.live.socket.clone();
    let workflow = original.authority.workflow.clone();
    let pending = std::thread::spawn(move || {
        let mut stream = connect_server(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        client::request(
            &mut stream,
            1,
            Request::Workflow(Box::new(WorkflowRequest::Save {
                auth,
                workflow,
                expected_version: 0,
                index: serde_json::from_value(index()).unwrap(),
            })),
        )
        .unwrap()
    });
    wait_file(&fault.with_extension("hit"));
    // No shutdown/handoff: the workflow authority row remains exactly unchanged.
    event(&fixture, "producer_replace:unproven-replacement");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if parent(&fixture).agent.as_ref().is_some_and(|a| {
            a.binding.conversation == "unproven-replacement"
                && a.health.state == ReporterHealth::Connected
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "replacement did not reach the real Reporter boundary"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(&release, "1").unwrap();
    assert!(matches!(pending.join().unwrap(), Response::Error { .. }));
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT metadata FROM delegation_workflows WHERE workflow = ?1",
            [&original.authority.workflow],
            |row| row.get(0),
        )
        .unwrap();
    let durable: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        durable["authority"],
        serde_json::to_value(&original.authority).unwrap()
    );
    assert!(durable["pointer"].is_null());
    assert!(
        !fixture
            .live
            .config
            .join("workflow-continuations")
            .join(&original.authority.workflow)
            .join("index-v1.json")
            .exists()
    );
    assert_eq!(fixture.byte_count(&first), bytes);
    assert_eq!(fixture.child_count(), 1);
}

fn wait_parent(
    fixture: &RecoveryFixture,
    predicate: impl Fn(&SessionSummary) -> bool,
) -> SessionSummary {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        let summary = parent(fixture);
        if predicate(&summary) {
            return summary;
        }
        assert!(
            Instant::now() < deadline,
            "parent did not reach expected state: {summary:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn durable_workflow(fixture: &RecoveryFixture, workflow: &str) -> serde_json::Value {
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT metadata FROM delegation_workflows WHERE workflow = ?1",
            [workflow],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn parent_recovery_owned_reopen_and_same_run_explicit_resume_reclaim_exact_original_workflow() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let before: WorkflowSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_checkpoint", serde_json::json!({"index":index()})),
    )
    .unwrap();
    let old_auth = original_auth(&fixture);
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO delegation_workflows(workflow,metadata) VALUES('unrelated-orphan','{')",
        [],
    )
    .unwrap();
    let bytes = fixture.byte_count(&first);
    assert!(
        !first.result_path.exists(),
        "first restart precedes native child publication"
    );
    let old_group = unsafe { libc::getpgid(parent(&fixture).pid.unwrap() as i32) };
    fixture.live.own_group(old_group);
    event(&fixture, "exit");
    wait_parent(&fixture, |s| !s.phase.is_live());
    let departed = durable_workflow(&fixture, &before.authority.workflow);
    assert!(
        departed["departure"]["Native"].is_object(),
        "successful exact native reap must commit departure: {departed}"
    );
    assert_eq!(
        departed["authority"],
        serde_json::to_value(&before.authority).unwrap()
    );
    assert!(matches!(
        fixture
            .live
            .request(Request::Workflow(Box::new(WorkflowRequest::Recover {
                auth: old_auth,
                workflow: Some(before.authority.workflow.clone())
            }))),
        Response::Error { .. }
    ));
    let reopened_reply = fixture.live.request(Request::ReopenSession {
        session: fixture.parent,
        expected_run: before.authority.parent_run,
        acknowledge_stopped: false,
    });
    assert!(
        matches!(
            reopened_reply,
            Response::Error {
                code: ErrorCode::OwnershipUncertain,
                ..
            }
        ),
        "existing stopped-process acknowledgement remains required: {reopened_reply:?}"
    );
    assert!(
        live::wait_group_absent(old_group, Duration::from_secs(5)),
        "explicit acknowledgement follows proven absence of this fixture-owned old group"
    );
    let reopened_reply = fixture.live.request(Request::ReopenSession {
        session: fixture.parent,
        expected_run: before.authority.parent_run,
        acknowledge_stopped: true,
    });
    assert!(
        matches!(reopened_reply, Response::CreatedSession(_)),
        "{reopened_reply:?}"
    );
    wait_parent(&fixture, |s| {
        s.run.0 == before.authority.parent_run.0 + 1
            && s.agent
                .as_ref()
                .is_some_and(|a| a.health.state == ReporterHealth::Connected)
    });
    let reopened = workflow(&fixture);
    assert_eq!(reopened.authority.epoch, before.authority.epoch + 1);
    assert_eq!(reopened.authority.parent, before.authority.parent);
    assert_eq!(
        reopened.authority.parent_run.0,
        before.authority.parent_run.0 + 1
    );
    assert_ne!(
        reopened.authority.binding.invocation,
        before.authority.binding.invocation
    );
    assert_eq!(
        reopened.authority.binding.conversation,
        before.authority.binding.conversation
    );
    assert_eq!(reopened.pointer, before.pointer);
    assert_eq!(reopened.continuation, before.continuation);
    assert_eq!(inspect(&fixture, &first.handle).execution_attempts, Some(1));
    assert_eq!(inspect(&fixture, &first.handle).receipts, first.receipts);
    let completed = complete(&fixture, &first);
    let result = fs::read(&completed.result_path).unwrap();
    // Reopen deliberately leaves an interactive shell. A later native Pi exit
    // therefore preserves this Session/run; only actual explicit argv can resume it.
    event(&fixture, "exit");
    let deadline = Instant::now() + Duration::from_secs(12);
    let reference = loop {
        let row = durable_workflow(&fixture, &before.authority.workflow);
        if row["departure"]["Native"].is_object() {
            break row["departure"]["Native"]["reference"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "same-run native departure was not committed"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let shell = parent(&fixture);
    assert!(
        shell.phase.is_live(),
        "normal reopened parent keeps its shell alive"
    );
    assert_eq!(shell.run, reopened.authority.parent_run);
    let reference: ExtensionConversation = serde_json::from_value(reference).unwrap();
    let argv = vec![
        env!("CARGO_BIN_EXE_ovrcr").to_owned(),
        "agent".into(),
        "run".into(),
        "pi".into(),
        "--".into(),
        reference.executable.to_string_lossy().into_owned(),
        "--model".into(),
        "fixture-model".into(),
        "--session".into(),
        reference.history.unwrap().to_string_lossy().into_owned(),
    ];
    let copied = fixture.live.root.path().join("copied-parent-history.jsonl");
    fs::copy(&argv[9], &copied).unwrap();
    // A copied original UUID/pointer and duplicate selectors can launch ordinary
    // Pi, but neither is an authenticated continuation admission.
    for arguments in [
        {
            let mut args = argv.clone();
            args[9] = copied.to_string_lossy().into_owned();
            args
        },
        {
            let mut args = argv.clone();
            args.extend(["--session".into(), argv[9].clone()]);
            args
        },
    ] {
        let command = arguments
            .iter()
            .map(|arg| format!("'{}'", arg.replace('\'', "'\"'\"'")))
            .collect::<Vec<_>>()
            .join(" ");
        event(&fixture, &command);
        let admitted = wait_parent(&fixture, |s| {
            s.agent.as_ref().is_some_and(|a| {
                a.health.state == ReporterHealth::Connected
                    && a.binding.invocation != reopened.authority.binding.invocation
            })
        });
        let refused = tool_reply(
            &fixture.live,
            fixture.parent,
            "ovrcr_recover",
            serde_json::json!({"workflow":before.authority.workflow}),
            &fixture
                .live
                .root
                .path()
                .join(format!("unproven-{}.json", admitted.agent_epoch)),
        );
        assert!(
            refused.get("error").is_some(),
            "copied path or ambiguous selector must not gain authority: {refused}"
        );
        assert_eq!(
            durable_workflow(&fixture, &before.authority.workflow)["authority"],
            serde_json::to_value(&reopened.authority).unwrap()
        );
        event(&fixture, "exit");
        wait_parent(&fixture, |s| {
            s.agent
                .as_ref()
                .is_none_or(|a| a.health.state != ReporterHealth::Connected)
        });
    }
    let command = argv
        .iter()
        .map(|arg| format!("'{}'", arg.replace('\'', "'\"'\"'")))
        .collect::<Vec<_>>()
        .join(" ");
    event(&fixture, &command);
    wait_parent(&fixture, |s| {
        s.agent.as_ref().is_some_and(|a| {
            a.health.state == ReporterHealth::Connected
                && a.binding.invocation != reopened.authority.binding.invocation
        })
    });
    let resumed = workflow(&fixture);
    assert_eq!(resumed.authority.epoch, reopened.authority.epoch + 1);
    assert_eq!(resumed.authority.parent_run, reopened.authority.parent_run);
    assert_eq!(
        resumed.authority.binding.conversation,
        before.authority.binding.conversation
    );
    assert_eq!(resumed.pointer, before.pointer);
    let again = inspect(&fixture, &first.handle);
    assert_eq!(again.identity, first.identity);
    assert_eq!(again.receipts, first.receipts);
    assert_eq!(again.execution_attempts, Some(1));
    assert_eq!(fixture.child_count(), 1);
    assert_eq!(fixture.byte_count(&first), bytes);
    assert_eq!(fs::read(&first.result_path).unwrap(), result);
    let row = durable_workflow(&fixture, &before.authority.workflow);
    assert!(
        row["departure"].is_null() && row["admission"].is_null(),
        "continuation claim is one-use"
    );
    assert_eq!(row["transfers"].as_array().unwrap().len(), 2);
}

fn create_parent(fixture: &RecoveryFixture) -> SessionSummary {
    let unique = fixture.files.get();
    fixture.files.set(unique + 1);
    let Response::CreatedSession(summary) =
        fixture
            .live
            .request(Request::CreateSession(CreateSessionRequest {
                project: PROJECT.into(),
                workspace: fixture.workspace.clone(),
                name: format!("new-parent-{unique}"),
                label: None,
                argv: vec![
                    env!("CARGO_BIN_EXE_ovrcr").into(),
                    "agent".into(),
                    "run".into(),
                    "pi".into(),
                    "--".into(),
                    "pi".into(),
                    "--model".into(),
                    "fixture-model".into(),
                ],
                kind: SessionKind::Agent { name: "pi".into() },
            }))
    else {
        panic!("create second managed parent");
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory");
        };
        if let Some(current) = sessions.into_iter().find(|s| {
            s.id == summary.id
                && s.agent
                    .as_ref()
                    .is_some_and(|a| a.health.state == ReporterHealth::Connected)
        }) {
            return current;
        }
        assert!(Instant::now() < deadline, "second parent did not bind");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn parent_tool(
    fixture: &RecoveryFixture,
    target: SessionId,
    name: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let id = fixture.files.get();
    fixture.files.set(id + 1);
    tool(
        &fixture.live,
        target,
        name,
        args,
        &fixture
            .live
            .root
            .path()
            .join(format!("target-reply-{id}.json")),
    )
}
fn target(fixture: &RecoveryFixture, id: SessionId) -> WorkflowParent {
    let values: Vec<WorkflowParent> =
        serde_json::from_value(fixture.tool("ovrcr_parents", serde_json::json!({}))).unwrap();
    values
        .into_iter()
        .find(|p| p.session == id)
        .expect("public discovery must include a parent with no workflow")
}
fn dashboard(fixture: &RecoveryFixture) -> std::os::unix::net::UnixStream {
    let mut stream = connect_server(&fixture.live.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    assert!(matches!(
        client::request(&mut stream, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    stream
}

#[test]
fn parent_recovery_public_targets_dashboard_transfer_lost_reply_preserves_human_barrier_and_one_execution()
 {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    let original = workflow(&fixture);
    let bytes = fixture.byte_count(&first);
    let original_result = fs::read(&first.result_path).unwrap();
    let second = create_parent(&fixture);
    assert!(parent_tool(&fixture, second.id, "ovrcr_recover", serde_json::json!({})).is_null());
    let stale = target(&fixture, second.id);
    let mut transfer = WorkflowTransfer {
        request: "person-transfer".into(),
        expected: original.authority.clone(),
        parent: stale.session,
        parent_run: stale.run,
        binding: stale.binding.clone(),
        producer: stale.producer.clone(),
    };
    assert!(
        matches!(
            fixture.live.request(Request::DashboardWorkflow(
                DashboardWorkflowRequest::Transfer(Box::new(transfer.clone()))
            )),
            Response::Error { .. }
        ),
        "ordinary connection cannot impersonate the active Dashboard"
    );
    let mut stream = dashboard(&fixture);
    assert_eq!(
        fixture.live.request(Request::SendTerminal {
            session: second.id,
            text: "producer_replace:replacement-parent".into(),
            submit: true
        }),
        Response::Ok
    );
    // Tool execution waits behind actual lifecycle replacement; old target tuple must fail.
    parent_tool(&fixture, second.id, "ovrcr_parents", serde_json::json!({}));
    assert!(matches!(
        dashboard_view(
            &mut stream,
            20,
            Request::DashboardWorkflow(DashboardWorkflowRequest::Transfer(Box::new(
                transfer.clone()
            )))
        )
        .unwrap(),
        Response::Error { .. }
    ));
    let fresh = target(&fixture, second.id);
    assert_ne!(fresh.producer, stale.producer);
    transfer.binding = fresh.binding;
    transfer.producer = fresh.producer;
    let child = first.identity.session.unwrap();
    let run = first.identity.run.unwrap();
    assert_eq!(
        dashboard_view(
            &mut stream,
            21,
            Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![PaneTarget {
                        session: child,
                        run,
                        size: TerminalSize {
                            rows: 24,
                            cols: 120
                        }
                    }],
                    focused: Some(child)
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    assert_eq!(
        dashboard_view(
            &mut stream,
            22,
            Request::AssignmentControl(AssignmentControlRequest::Takeover {
                request: "person-takeover".into(),
                session: child,
                run,
                expected_epoch: first.control.epoch,
                view_revision: 1
            })
        )
        .unwrap(),
        Response::Ok
    );
    let human = inspect(&fixture, &first.handle);
    assert_eq!(human.control.owner, ControlOwner::Human);
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_parent_transfer BEFORE UPDATE ON delegation_workflows BEGIN SELECT RAISE(FAIL, 'fixture persistence failure'); END;").unwrap();
    assert!(matches!(
        dashboard_view(
            &mut stream,
            23,
            Request::DashboardWorkflow(DashboardWorkflowRequest::Transfer(Box::new(
                transfer.clone()
            )))
        )
        .unwrap(),
        Response::Error { .. }
    ));
    assert_eq!(workflow(&fixture).authority, original.authority);
    db.execute_batch("DROP TRIGGER fail_parent_transfer;")
        .unwrap();
    // Commit through the actual Dashboard connection, then lose its reply.
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 24,
            request: Request::DashboardWorkflow(DashboardWorkflowRequest::Transfer(Box::new(
                transfer.clone(),
            ))),
        },
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let row = durable_workflow(&fixture, &original.authority.workflow);
        if row["authority"]["parent"] == serde_json::to_value(second.id).unwrap() {
            break;
        }
        assert!(Instant::now() < deadline, "transfer did not commit");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(stream);
    let mut stream = dashboard(&fixture);
    let Response::Workflows(rows) = dashboard_view(
        &mut stream,
        25,
        Request::DashboardWorkflow(DashboardWorkflowRequest::Transfer(Box::new(
            transfer.clone(),
        ))),
    )
    .unwrap() else {
        panic!("lost transfer reply must reconcile");
    };
    let transferred = &rows[0];
    assert_eq!(transferred.authority.epoch, original.authority.epoch + 1);
    assert_eq!(
        transferred.receipt.as_ref().unwrap().previous,
        original.authority
    );
    let mut competing = transfer;
    competing.request = "competing-transfer".into();
    assert!(
        matches!(
            dashboard_view(
                &mut stream,
                26,
                Request::DashboardWorkflow(DashboardWorkflowRequest::Transfer(Box::new(competing)))
            )
            .unwrap(),
            Response::Error { .. }
        ),
        "stale expected authority cannot claim a second epoch"
    );
    let stale_reply = tool_reply(
        &fixture.live,
        fixture.parent,
        "ovrcr_collect",
        serde_json::json!({"handle":first.handle}),
        &fixture.live.root.path().join("old-owner-rejected.json"),
    );
    assert!(stale_reply.get("error").is_some());
    let recovered: WorkflowSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow":original.authority.workflow}),
    ))
    .unwrap();
    assert_eq!(recovered.authority, transferred.authority);
    let current: AssignmentSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_inspect",
        serde_json::json!({"handle":first.handle}),
    ))
    .unwrap();
    assert_eq!(current.control, human.control);
    assert_eq!(current.identity, first.identity);
    assert_eq!(current.receipts, human.receipts);
    assert_eq!(current.execution_attempts, Some(1));
    assert_eq!(fixture.byte_count(&first), bytes);
    assert_eq!(fs::read(&first.result_path).unwrap(), original_result);
    assert_eq!(
        fixture.child_count(),
        2,
        "exactly original child and second live parent"
    );
    assert_eq!(
        dashboard_view(
            &mut stream,
            27,
            Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![PaneTarget {
                        session: child,
                        run,
                        size: TerminalSize {
                            rows: 24,
                            cols: 120
                        }
                    }],
                    focused: Some(child)
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    assert_eq!(
        dashboard_view(
            &mut stream,
            28,
            Request::AssignmentControl(AssignmentControlRequest::Return {
                request: "person-return".into(),
                session: child,
                run,
                expected_epoch: current.control.epoch,
                view_revision: 1,
                evidence: Box::new(current.control_evidence.clone().unwrap())
            })
        )
        .unwrap(),
        Response::Ok
    );
    let returned: AssignmentSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_inspect",
        serde_json::json!({"handle":first.handle}),
    ))
    .unwrap();
    assert!(matches!(
        returned.control.barrier,
        Some(ControlBarrier::HumanReturn { .. })
    ));
    let confirmed: AssignmentSnapshot = serde_json::from_value(parent_tool(&fixture, second.id, "ovrcr_confirm_return", serde_json::json!({"handle":first.handle,"request":"successor-confirm","returned_request":"person-return","expected_control_epoch":returned.control.epoch,"evidence":returned.control_evidence}))).unwrap();
    assert!(confirmed.control.barrier.is_none());
    let followed: AssignmentSnapshot = serde_json::from_value(parent_tool(&fixture, second.id, "ovrcr_followup", serde_json::json!({"handle":first.handle,"expected_identity":confirmed.identity,"expected_result_version":confirmed.result_version,"expected_control_epoch":confirmed.control.epoch,"text":"Read the same README and return fresh evidence after explicit control return."}))).unwrap();
    assert_eq!(followed.identity, first.identity);
    assert_eq!(followed.result_version, 2);
    assert_eq!(followed.execution_attempts, Some(1));
    assert_eq!(fixture.input_count(&followed), 1);
    assert_eq!(fixture.child_count(), 2);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects() {
    for (operation, fault, expected) in [
        ("Launch", None, None),
        (
            "InitialInput",
            Some(("input-target-captured", 0)),
            Some(DispatchOutcome::Accepted),
        ),
        (
            "InitialInput",
            Some(("input", usize::MAX)),
            Some(DispatchOutcome::Unknown),
        ),
        (
            "InitialInput",
            Some(("input", 7)),
            Some(DispatchOutcome::Partial),
        ),
    ] {
        let fixture = OwnedFollowup::new();
        fixture.proxy.arm(ControlRule {
            operation,
            loss: if fault.is_some_and(|(point, _)| point == "input-target-captured") {
                1
            } else if fault.is_some() {
                2
            } else {
                0
            },
            fault,
        });
        let started = fixture.start();
        let handle = started["handle"].as_str().unwrap();
        let original = fixture.reconcile(handle);
        let before = workflow(&fixture);
        assert!(before.handles.contains(&original.handle));
        if operation == "Launch" {
            assert!(original.identity.session.is_none());
        }
        if let Some(expected) = expected {
            let op = if operation == "Launch" {
                DispatchOperation::Launch
            } else {
                DispatchOperation::InitialInput
            };
            assert_eq!(
                original
                    .receipts
                    .iter()
                    .find(|r| r.operation == op)
                    .unwrap()
                    .outcome,
                expected
            );
        }
        let count = fixture.child_count();
        let bytes = original
            .identity
            .session
            .map_or(0, |_| fixture.byte_count(&original));
        let second = create_parent(&fixture);
        let target = target(&fixture, second.id);
        let moved: WorkflowSnapshot = serde_json::from_value(fixture.tool("ovrcr_transfer", serde_json::json!({"request":"owner-transfer","parent":target.session,"parent_run":target.run,"binding":target.binding,"producer":target.producer}))).unwrap();
        assert_eq!(moved.authority.epoch, before.authority.epoch + 1);
        assert!(
            moved.handles.contains(&original.handle),
            "prepared and older-epoch obligations are discovered independently of current authority"
        );
        parent_tool(
            &fixture,
            second.id,
            "ovrcr_recover",
            serde_json::json!({"workflow":before.authority.workflow}),
        );
        let replay = parent_tool(
            &fixture,
            second.id,
            "ovrcr_start",
            serde_json::json!({"handle":original.handle}),
        );
        assert!(
            matches!(
                replay["dispatch"].as_str(),
                Some("Refused" | "Unknown" | "InputUnknown")
            ),
            "retired effects stay suspended: {replay}"
        );
        let current: AssignmentSnapshot = serde_json::from_value(parent_tool(
            &fixture,
            second.id,
            "ovrcr_inspect",
            serde_json::json!({"handle":original.handle}),
        ))
        .unwrap();
        assert_eq!(current.identity, original.identity);
        assert_eq!(current.receipts, original.receipts);
        assert_eq!(current.execution_attempts, original.execution_attempts);
        assert_eq!(
            original
                .identity
                .session
                .map_or(0, |_| fixture.byte_count(&original)),
            bytes
        );
        assert_eq!(
            fixture.child_count(),
            count + 1,
            "transfer creates no child execution"
        );
    }
}

#[test]
fn parent_recovery_corrupt_index_stopped_child_and_server_loss_preserve_obligations_until_explicit_transfer()
 {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    let saved: WorkflowSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_checkpoint", serde_json::json!({"index":index()})),
    )
    .unwrap();
    let path = fixture
        .live
        .config
        .join("workflow-continuations")
        .join(&saved.authority.workflow)
        .join("index-v1.json");
    fs::write(&path, "{truncated").unwrap();
    assert_eq!(
        fixture.live.request(Request::KillSession {
            session: first.identity.session.unwrap()
        }),
        Response::Ok
    );
    let gap = workflow(&fixture);
    assert_eq!(gap.pointer, saved.pointer);
    assert!(gap.continuation.is_none());
    assert!(gap.handles.contains(&first.handle));
    assert!(gap.gaps.iter().any(|g| g.contains("stopped or missing")));
    assert!(
        gap.gaps
            .iter()
            .any(|g| g.contains("missing, malformed or conflicting"))
    );
    assert_eq!(fixture.child_count(), 1);
    let groups = fixture.live.session_groups();
    for group in &groups {
        fixture.live.own_group(*group);
    }
    let old_server = fixture.live.server_pid().unwrap();
    assert_eq!(
        fixture.live.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    fixture.live.join();
    assert_eq!(unsafe { libc::kill(old_server as i32, 0) }, -1);
    for group in groups {
        assert!(live::wait_group_absent(group, Duration::from_secs(5)));
    }
    // This restart is an explicit test action. No recover/transfer operation starts a Server.
    let root = fixture.live.root.path();
    let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/pi_host.mjs");
    let history = root.join("history");
    let gate = root.join("allow-result");
    let fault = root.join("fault");
    fixture.live.start_binary_env(&[
        ("OVRCR_TEST_DELEGATION", "1".as_ref()),
        ("OVRCR_TEST_PI_HOST", host.as_os_str()),
        (
            "OVRCR_TEST_CONTROL_BINARY",
            env!("CARGO_BIN_EXE_ovrcr").as_ref(),
        ),
        ("OVRCR_TEST_DELEGATION_RESULT_GATE", gate.as_os_str()),
        (
            "OVRCR_TEST_DELEGATION_CONTROL_SOCKET",
            fixture.proxy.path.as_os_str(),
        ),
        ("OVRCR_TEST_DELEGATION_FAULT", fault.as_os_str()),
        ("OVRCR_TEST_DELEGATION_BYTES", fixture.bytes.as_os_str()),
        ("OVRCR_TEST_HISTORY", history.as_os_str()),
    ]);
    let second = create_parent(&fixture);
    let forbidden = tool_reply(
        &fixture.live,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow":saved.authority.workflow}),
        &root.join("new-lifetime-refused.json"),
    );
    assert!(
        forbidden.get("error").is_some(),
        "Server loss cannot infer current authority from pointers"
    );
    let public: Vec<WorkflowParent> = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_parents",
        serde_json::json!({}),
    ))
    .unwrap();
    let target = public.into_iter().find(|p| p.session == second.id).unwrap();
    let transfer = WorkflowTransfer {
        request: "person-after-server-loss".into(),
        expected: saved.authority.clone(),
        parent: target.session,
        parent_run: target.run,
        binding: target.binding,
        producer: target.producer,
    };
    let mut stream = dashboard(&fixture);
    let Response::Workflows(rows) = dashboard_view(
        &mut stream,
        30,
        Request::DashboardWorkflow(DashboardWorkflowRequest::Transfer(Box::new(transfer))),
    )
    .unwrap() else {
        panic!("explicit Dashboard recovery");
    };
    let moved = &rows[0];
    assert_ne!(
        moved.authority.server_lifetime,
        saved.authority.server_lifetime
    );
    assert_eq!(moved.receipt.as_ref().unwrap().previous, saved.authority);
    assert!(moved.handles.contains(&first.handle));
    assert!(moved.continuation.is_none());
    parent_tool(
        &fixture,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow":saved.authority.workflow}),
    );
    let current: AssignmentSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_inspect",
        serde_json::json!({"handle":first.handle}),
    ))
    .unwrap();
    assert_eq!(current.identity, first.identity);
    assert_eq!(current.receipts, first.receipts);
    assert_eq!(current.execution_attempts, Some(1));
    assert!(
        !current
            .resources
            .current_session
            .as_ref()
            .unwrap()
            .phase
            .is_live()
    );
    let fresh_reply = parent_tool(
        &fixture,
        second.id,
        "ovrcr_start",
        serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Read a distinct README point","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]}),
    );
    let fresh: AssignmentSnapshot = serde_json::from_value(fresh_reply.clone())
        .unwrap_or_else(|error| panic!("new epoch assignment refused: {error}; {fresh_reply}"));
    assert_eq!(fresh.identity.controller_epoch, moved.authority.epoch);
    let recovered: WorkflowSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow":saved.authority.workflow}),
    ))
    .unwrap();
    assert!(recovered.handles.contains(&first.handle) && recovered.handles.contains(&fresh.handle));
    assert_eq!(fresh.identity.workflow, first.identity.workflow);
}

#[test]
fn parent_recovery_concurrent_current_owner_transfers_have_one_atomic_winner() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    let before = workflow(&fixture);
    let second = create_parent(&fixture);
    let third = create_parent(&fixture);
    let targets = [target(&fixture, second.id), target(&fixture, third.id)];
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let bytes = fixture.byte_count(&first);
    let threads: Vec<_> = targets
        .into_iter()
        .enumerate()
        .map(|(n, target)| {
            let socket = fixture.live.socket.clone();
            let barrier = barrier.clone();
            let auth = original_auth(&fixture);
            let transfer = WorkflowTransfer {
                request: format!("concurrent-{n}"),
                expected: before.authority.clone(),
                parent: target.session,
                parent_run: target.run,
                binding: target.binding,
                producer: target.producer,
            };
            std::thread::spawn(move || {
                let mut stream = connect_server(&socket).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                barrier.wait();
                client::request(
                    &mut stream,
                    1,
                    Request::Workflow(Box::new(WorkflowRequest::Transfer { auth, transfer })),
                )
                .unwrap()
            })
        })
        .collect();
    barrier.wait();
    let replies: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(
        replies
            .iter()
            .filter(|r| matches!(r, Response::Workflows(_)))
            .count(),
        1
    );
    assert_eq!(
        replies
            .iter()
            .filter(|r| matches!(r, Response::Error { .. }))
            .count(),
        1
    );
    let row = durable_workflow(&fixture, &before.authority.workflow);
    assert_eq!(row["authority"]["epoch"], before.authority.epoch + 1);
    assert_eq!(row["transfers"].as_array().unwrap().len(), 1);
    assert_eq!(fixture.byte_count(&first), bytes);
    assert_eq!(
        fixture.child_count(),
        3,
        "only original child and two explicit target parents"
    );
}

#[test]
fn parent_recovery_missing_legacy_authority_stays_visible_read_only_and_does_not_poison_valid_choices()
 {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    let before = workflow(&fixture);
    let second = create_parent(&fixture);
    let other: AssignmentSnapshot = serde_json::from_value(parent_tool(&fixture, second.id, "ovrcr_start", serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Read independent README","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]}))).unwrap();
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let original_metadata: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [&first.handle],
            |r| r.get(0),
        )
        .unwrap();
    let original_result = fs::read(&first.result_path).unwrap();
    let bytes = fixture.byte_count(&first);
    db.execute(
        "DELETE FROM delegation_workflows WHERE workflow=?1",
        [&before.authority.workflow],
    )
    .unwrap();
    let gaps = fixture.tool(
        "ovrcr_recover",
        serde_json::json!({"workflow":before.authority.workflow}),
    );
    assert_eq!(
        gaps["recovery_gaps"][0]["identity"],
        serde_json::to_value(&first.identity).unwrap()
    );
    assert!(
        gaps["recovery_gaps"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("transfer are unavailable")
    );
    let forbidden = fixture.live.request(Request::Delegation(Box::new(
        fixture.proxy.original(DispatchOperation::InitialInput),
    )));
    assert!(matches!(forbidden, Response::Error { .. }));
    let mut stream = dashboard(&fixture);
    let Response::WorkflowCandidates(choices) = dashboard_view(
        &mut stream,
        33,
        Request::DashboardWorkflow(DashboardWorkflowRequest::Candidates {
            parent: fixture.parent,
            run: before.authority.parent_run,
        }),
    )
    .unwrap() else {
        panic!("operator recovery choices");
    };
    assert!(
        choices
            .candidates
            .iter()
            .any(|t| t.expected.workflow == other.identity.workflow)
    );
    assert!(
        choices
            .gaps
            .iter()
            .any(|gap| gap.identity.as_ref() == Some(&first.identity))
    );
    let after: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [&first.handle],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after, original_metadata);
    assert_eq!(fs::read(&first.result_path).unwrap(), original_result);
    assert_eq!(fixture.byte_count(&first), bytes);
    // Another authenticated source is not the original exact legacy association.
    let foreign = tool_reply(
        &fixture.live,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow":before.authority.workflow}),
        &fixture.live.root.path().join("legacy-foreign.json"),
    );
    assert!(foreign.get("error").is_some());
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn parent_recovery_r1_accepted_followup_stays_suspended_with_immutable_receipt_after_parent_transfer()
 {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    workflow(&fixture);
    fixture.proxy.arm(ControlRule {
        operation: "Followup",
        loss: 2,
        fault: Some(("followup-after-files", 0)),
    });
    let pending: AssignmentSnapshot =
        serde_json::from_value(super::followups::followup(&fixture, &first, None)).unwrap();
    let receipt = pending
        .receipts
        .iter()
        .find(|r| r.operation == DispatchOperation::Followup)
        .unwrap();
    assert_eq!(receipt.outcome, DispatchOutcome::Accepted);
    let request = receipt.request.clone();
    let root = first.brief.parent().unwrap();
    let files: Vec<_> = [
        format!("followup-{request}.json"),
        "contract-v1.json".into(),
        "brief-v2.json".into(),
        "submission-v1.json".into(),
    ]
    .into_iter()
    .map(|name| {
        let bytes = fs::read(root.join(&name)).unwrap();
        (name, bytes)
    })
    .collect();
    let bytes = fixture.byte_count(&first);
    let second = create_parent(&fixture);
    let target = target(&fixture, second.id);
    let moved: WorkflowSnapshot = serde_json::from_value(fixture.tool("ovrcr_transfer", serde_json::json!({"request":"r1-move","parent":target.session,"parent_run":target.run,"binding":target.binding,"producer":target.producer}))).unwrap();
    parent_tool(
        &fixture,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow":moved.authority.workflow}),
    );
    parent_tool(
        &fixture,
        second.id,
        "ovrcr_followup",
        serde_json::json!({"handle":first.handle,"request":request}),
    );
    let after: AssignmentSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_inspect",
        serde_json::json!({"handle":first.handle}),
    ))
    .unwrap();
    assert_eq!(
        after.receipts, pending.receipts,
        "parent transfer suspends old Accepted receipt; child-control rejection is a different rule"
    );
    assert_eq!(after.identity, pending.identity);
    assert_eq!(after.result_version, pending.result_version);
    assert_eq!(after.dispatch, pending.dispatch);
    assert_eq!(after.execution_attempts, Some(1));
    let fresh = parent_tool(
        &fixture,
        second.id,
        "ovrcr_followup",
        serde_json::json!({"handle":first.handle,"expected_identity":first.identity,"expected_result_version":first.result_version,"expected_control_epoch":first.control.epoch,"text":"Do not bypass the suspended original followup"}),
    );
    assert_eq!(fresh["dispatch"], "Refused");
    for (name, original) in files {
        assert_eq!(fs::read(root.join(name)).unwrap(), original);
    }
    assert_eq!(fixture.byte_count(&first), bytes);
    assert_eq!(fixture.child_count(), 2);
}

#[test]
fn parent_recovery_r1_malformed_workflow_is_scoped_read_only_gap_while_valid_workflow_remains_usable()
 {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    let before = workflow(&fixture);
    let result = fs::read(&first.result_path).unwrap();
    let second = create_parent(&fixture);
    let other: AssignmentSnapshot = serde_json::from_value(parent_tool(&fixture, second.id, "ovrcr_start", serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Independent valid work","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]}))).unwrap();
    let valid: WorkflowSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_recover",
        serde_json::json!({}),
    ))
    .unwrap();
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let old = durable_workflow(&fixture, &before.authority.workflow);
    let mut unsupported = old.clone();
    unsupported["schema"] = serde_json::json!(999);
    let mut conflicting = old;
    conflicting["authority"]["epoch"] = serde_json::json!(2);
    for broken in [
        "{private-truncated".to_owned(),
        serde_json::to_string(&unsupported).unwrap(),
        serde_json::to_string(&conflicting).unwrap(),
        "x".repeat(262_145),
    ] {
        db.execute(
            "UPDATE delegation_workflows SET metadata=?1 WHERE workflow=?2",
            rusqlite::params![broken, before.authority.workflow],
        )
        .unwrap();
        let current: WorkflowSnapshot = serde_json::from_value(parent_tool(
            &fixture,
            second.id,
            "ovrcr_recover",
            serde_json::json!({"workflow":valid.authority.workflow}),
        ))
        .unwrap();
        assert_eq!(current.authority, valid.authority);
        assert!(current.handles.contains(&other.handle));
        assert!(
            !serde_json::to_string(&current)
                .unwrap()
                .contains(&first.handle),
            "foreign invalid workflow contents are not disclosed"
        );
        let gaps = fixture.tool(
            "ovrcr_recover",
            serde_json::json!({"workflow":before.authority.workflow}),
        );
        assert_eq!(
            gaps["recovery_gaps"][0]["identity"],
            serde_json::to_value(&first.identity).unwrap()
        );
        assert!(
            !serde_json::to_string(&gaps)
                .unwrap()
                .contains("private-truncated")
        );
        assert!(matches!(
            fixture.live.request(Request::Delegation(Box::new(
                fixture.proxy.original(DispatchOperation::InitialInput)
            ))),
            Response::Error { .. }
        ));
        let mut stream = dashboard(&fixture);
        let Response::WorkflowCandidates(choices) = dashboard_view(
            &mut stream,
            70,
            Request::DashboardWorkflow(DashboardWorkflowRequest::Candidates {
                parent: fixture.parent,
                run: before.authority.parent_run,
            }),
        )
        .unwrap() else {
            panic!("operator choices");
        };
        assert!(
            choices
                .candidates
                .iter()
                .any(|c| c.expected.workflow == valid.authority.workflow)
        );
        assert!(
            choices
                .gaps
                .iter()
                .any(|g| g.identity.as_ref() == Some(&first.identity))
        );
        let raw: String = db
            .query_row(
                "SELECT metadata FROM delegation_workflows WHERE workflow=?1",
                [&before.authority.workflow],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, broken);
        assert_eq!(fs::read(&first.result_path).unwrap(), result);
    }
    db.execute(
        "UPDATE delegations SET metadata='{private-assignment' WHERE handle=?1",
        [&first.handle],
    )
    .unwrap();
    let still_valid: WorkflowSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow":valid.authority.workflow}),
    ))
    .unwrap();
    assert_eq!(still_valid.authority, valid.authority);
    let mut stream = dashboard(&fixture);
    let Response::WorkflowCandidates(choices) = dashboard_view(
        &mut stream,
        71,
        Request::DashboardWorkflow(DashboardWorkflowRequest::Candidates {
            parent: fixture.parent,
            run: before.authority.parent_run,
        }),
    )
    .unwrap() else {
        panic!("operator malformed assignment choices");
    };
    assert!(
        choices
            .candidates
            .iter()
            .any(|c| c.expected.workflow == valid.authority.workflow)
    );
    assert!(
        choices
            .gaps
            .iter()
            .any(|gap| gap.handle.as_deref() == Some(first.handle.as_str())
                && gap.identity.is_none()
                && gap.brief.is_none()
                && gap.result_path.is_none())
    );
    assert!(
        !serde_json::to_string(&choices)
            .unwrap()
            .contains("private-assignment")
    );
    let retained: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [&first.handle],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(retained, "{private-assignment");
    db.execute("INSERT INTO delegations(handle,workflow,session,metadata) VALUES('untrusted key','untrusted workflow',NULL,'{private-key')", []).unwrap();
    let Response::WorkflowCandidates(unknown) = dashboard_view(
        &mut stream,
        72,
        Request::DashboardWorkflow(DashboardWorkflowRequest::Candidates {
            parent: fixture.parent,
            run: before.authority.parent_run,
        }),
    )
    .unwrap() else {
        panic!("unknown operator gap");
    };
    assert!(
        unknown
            .candidates
            .iter()
            .any(|c| c.expected.workflow == valid.authority.workflow)
    );
    assert!(
        unknown
            .gaps
            .iter()
            .any(|gap| gap.handle.is_none() && gap.workflow.is_none() && gap.identity.is_none())
    );
    let visible = serde_json::to_string(&unknown).unwrap();
    assert!(
        !visible.contains("untrusted key")
            && !visible.contains("untrusted workflow")
            && !visible.contains("private-key")
    );
    let third = create_parent(&fixture);
    let parents: Vec<WorkflowParent> = serde_json::from_value(parent_tool(
        &fixture,
        third.id,
        "ovrcr_parents",
        serde_json::json!({}),
    ))
    .unwrap();
    assert!(
        parents.iter().any(|p| p.session == third.id),
        "unrelated malformed row cannot prevent certified source registration"
    );
    let native = valid.authority.binding.conversation.clone();
    assert_eq!(
        fixture.live.request(Request::SendTerminal {
            session: second.id,
            text: format!("session_replace:{native}:reload"),
            submit: true
        }),
        Response::Ok
    );
    let reloaded: WorkflowSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow":valid.authority.workflow}),
    ))
    .unwrap();
    assert_eq!(reloaded.authority.epoch, valid.authority.epoch + 1);
}

#[test]
fn parent_recovery_explicit_new_and_fork_start_distinct_workflows_without_old_effects() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    let original = workflow(&fixture);
    let original_row = durable_workflow(&fixture, &original.authority.workflow);
    let original_files = (
        fs::read(&first.brief).unwrap(),
        fs::read(&first.result_path).unwrap(),
    );
    let original_bytes = fixture.byte_count(&first);
    let mut prior = original.authority.clone();
    for reason in ["new", "fork"] {
        event(
            &fixture,
            &format!("session_start:distinct-{reason}:{reason}"),
        );
        let retired = fixture.recover(&first.handle);
        assert_eq!(
            retired["dispatch"], "Refused",
            "copied old intent cannot acquire new-context control: {retired}"
        );
        let next = fixture.start();
        assert_eq!(
            next["dispatch"], "Applied",
            "explicit {reason} must permit distinct work: {next}"
        );
        let next: AssignmentSnapshot = serde_json::from_value(next).unwrap();
        let current = workflow(&fixture);
        assert_ne!(current.authority.workflow, prior.workflow);
        assert_ne!(current.authority.controller, prior.controller);
        assert_eq!(current.authority.epoch, 1);
        assert_eq!(
            current.handles.as_slice(),
            std::slice::from_ref(&next.handle)
        );
        let collected = complete(&fixture, &next);
        assert_eq!(collected.identity, next.identity);
        assert_eq!(collected.dispatch, DispatchStatus::Applied);
        assert_eq!(
            durable_workflow(&fixture, &original.authority.workflow),
            original_row
        );
        assert_eq!(fs::read(&first.brief).unwrap(), original_files.0);
        assert_eq!(fs::read(&first.result_path).unwrap(), original_files.1);
        assert_eq!(fixture.byte_count(&first), original_bytes);
        assert_eq!(fixture.input_count(&first), 1);
        prior = current.authority;
    }
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let raw: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [&first.handle],
            |r| r.get(0),
        )
        .unwrap();
    let stored: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        stored["identity"],
        serde_json::to_value(&first.identity).unwrap()
    );
    assert_eq!(
        stored["receipts"],
        serde_json::to_value(&first.receipts).unwrap()
    );
    assert_eq!(
        stored["control"],
        serde_json::to_value(&first.control).unwrap()
    );
}

#[test]
fn parent_recovery_r1_corrupt_member_is_gap_beside_valid_obligation_and_foreign_workflow() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    let member: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let member = complete(&fixture, &member);
    let before = workflow(&fixture);
    let second = create_parent(&fixture);
    let other: AssignmentSnapshot = serde_json::from_value(parent_tool(&fixture, second.id,"ovrcr_start",serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Unrelated valid work","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"]}))).unwrap();
    let foreign: WorkflowSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_recover",
        serde_json::json!({}),
    ))
    .unwrap();
    let authority_row = durable_workflow(&fixture, &before.authority.workflow);
    let files = [
        (&first.brief, fs::read(&first.brief).unwrap()),
        (&first.result_path, fs::read(&first.result_path).unwrap()),
        (&member.brief, fs::read(&member.brief).unwrap()),
        (&member.result_path, fs::read(&member.result_path).unwrap()),
    ];
    let counts = (fixture.byte_count(&first), fixture.byte_count(&member));
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    db.execute(
        "UPDATE delegations SET metadata='{private-corrupt-member' WHERE handle=?1",
        [&member.handle],
    )
    .unwrap();
    let current = workflow(&fixture);
    assert_eq!(current.authority, before.authority);
    assert_eq!(
        current.handles.as_slice(),
        std::slice::from_ref(&first.handle)
    );
    assert!(
        current
            .gaps
            .iter()
            .any(|gap| gap.contains(&member.handle) && gap.contains("unverifiable"))
    );
    assert!(
        !serde_json::to_string(&current)
            .unwrap()
            .contains("private-corrupt-member")
    );
    let unaffected: WorkflowSnapshot = serde_json::from_value(parent_tool(
        &fixture,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow":foreign.authority.workflow}),
    ))
    .unwrap();
    assert_eq!(unaffected.authority, foreign.authority);
    assert_eq!(
        unaffected.handles.as_slice(),
        std::slice::from_ref(&other.handle)
    );
    assert!(
        !serde_json::to_string(&unaffected)
            .unwrap()
            .contains(&member.handle)
    );
    assert!(matches!(
        fixture.live.request(Request::Delegation(Box::new(
            DelegationRequest::Reconcile {
                auth: original_auth(&fixture),
                request: member.handle.clone()
            }
        ))),
        Response::Error { .. }
    ));
    assert_eq!(
        durable_workflow(&fixture, &before.authority.workflow),
        authority_row
    );
    let destination = target(&fixture, second.id);
    let transferred: WorkflowSnapshot = serde_json::from_value(fixture.tool("ovrcr_transfer", serde_json::json!({"request":"member-gap-transfer","parent":destination.session,"parent_run":destination.run,"binding":destination.binding,"producer":destination.producer}))).unwrap();
    assert_eq!(transferred.authority.epoch, before.authority.epoch + 1);
    assert_eq!(
        transferred.handles.as_slice(),
        std::slice::from_ref(&first.handle)
    );
    assert!(
        transferred
            .gaps
            .iter()
            .any(|gap| gap.contains(&member.handle) && gap.contains("unverifiable")),
        "post-commit transfer response retains the damaged member gap"
    );
    let raw: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [&member.handle],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(raw, "{private-corrupt-member");
    for (path, bytes) in files {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert_eq!(
        (fixture.byte_count(&first), fixture.byte_count(&member)),
        counts
    );
    assert_eq!(fixture.input_count(&first), 1);
    assert_eq!(fixture.input_count(&member), 1);
}

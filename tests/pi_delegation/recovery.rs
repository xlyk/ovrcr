//! Actual registered lifecycle hooks -> production CLI -> isolated Server and surviving PTYs.
use super::*;
use super::followups::{OwnedFollowup, complete};

fn inspect(fixture: &RecoveryFixture, handle: &str) -> AssignmentSnapshot {
    serde_json::from_value(fixture.tool("ovrcr_inspect", serde_json::json!({"handle":handle}))).unwrap()
}
fn workflow(fixture: &RecoveryFixture) -> WorkflowSnapshot {
    serde_json::from_value(fixture.tool("ovrcr_recover", serde_json::json!({}))).unwrap()
}
fn event(fixture: &RecoveryFixture, value: &str) {
    assert_eq!(fixture.live.request(Request::SendTerminal { session: fixture.parent, text: value.into(), submit: true }), Response::Ok);
}
fn parent(fixture: &RecoveryFixture) -> SessionSummary {
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else { panic!("inventory"); };
    sessions.into_iter().find(|s| s.id == fixture.parent).unwrap()
}
fn index() -> serde_json::Value {
    serde_json::json!({"schema":1,"objective":"objective-v1.json","decisions":["decision-v1.json"],"unresolved_assignments":[],"questions":["question-v1.json"],"receipts":["original dispatch receipt"],"accepted_evidence":[],"next_actions":["inspect surviving child before new work"]})
}
fn original_auth(fixture: &RecoveryFixture) -> DelegationAuth {
    let DelegationRequest::Prepare { auth, .. } = fixture.proxy.original(DispatchOperation::Prepare) else { unreachable!() }; auth
}

#[test]
fn parent_recovery_compaction_and_orderly_reload_preserve_original_children_and_fence_old_source() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let first = complete(&fixture, &first);
    let before = workflow(&fixture);
    let saved: WorkflowSnapshot = serde_json::from_value(fixture.tool("ovrcr_checkpoint", serde_json::json!({"index":index()}))).unwrap();
    let child_bytes = fixture.byte_count(&first);
    let original = fs::read(&first.result_path).unwrap();
    for command in ["session_compact", "session_compact_failed"] {
        event(&fixture, command);
        let after = workflow(&fixture);
        assert_eq!(after.authority, before.authority, "compaction is lookup, not transfer");
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
    event(&fixture, &format!("forge_workflow_source:{}:2", after.authority.producer));
    // A rooted request with the exact next sequence cannot overwrite an admitted source key.
    assert_eq!(workflow(&fixture).authority, after.authority);
    let mut stale = original_auth(&fixture);
    // Public replacement identity/epoch and the shared run capability are insufficient.
    stale.controller = Some(after.authority.fence());
    for source in [stale.source.clone(), None, Some(AgentSecret([0x12;32]))] {
        stale.source = source;
        assert!(matches!(fixture.live.request(Request::Workflow(Box::new(WorkflowRequest::Recover { auth: stale.clone(), workflow: Some(after.authority.workflow.clone()) }))), Response::Error { .. }));
    }
    let native_history = fixture.live.root.path().join("history").join(format!("{native}.jsonl"));
    let native_history = fs::read_to_string(native_history).unwrap();
    assert!(native_history.contains("ovrcr_workflow_pointer"));
    let secret = serde_json::to_string(&original_auth(&fixture).source).unwrap();
    assert!(!native_history.contains(&secret), "native history must not retain source capability");
    assert!(!serde_json::to_string(&after).unwrap().contains(&secret), "snapshots must not expose source capability");
    let inspected = inspect(&fixture, &first.handle);
    assert_eq!(inspected.control, first.control);
}

#[cfg(feature="acceptance-diagnostics")]
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
    impl Drop for Release { fn drop(&mut self) { let _ = fs::write(&self.0, "1"); } }
    let _release = Release(release.clone());
    fs::write(&fault, format!("workflow-before-save|{}|0", original.authority.workflow)).unwrap();
    let socket = fixture.live.socket.clone();
    let workflow = original.authority.workflow.clone();
    let pending = std::thread::spawn(move || {
        let mut stream = connect_server(&socket).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        client::request(&mut stream, 1, Request::Workflow(Box::new(WorkflowRequest::Save { auth, workflow, expected_version: 0, index: serde_json::from_value(index()).unwrap() }))).unwrap()
    });
    wait_file(&fault.with_extension("hit"));
    // No shutdown/handoff: the workflow authority row remains exactly unchanged.
    event(&fixture, "producer_replace:unproven-replacement");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if parent(&fixture).agent.as_ref().is_some_and(|a| a.binding.conversation == "unproven-replacement" && a.health.state == ReporterHealth::Connected) { break; }
        assert!(Instant::now() < deadline, "replacement did not reach the real Reporter boundary");
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(&release,"1").unwrap();
    assert!(matches!(pending.join().unwrap(), Response::Error { .. }));
    let db = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let raw: String = db.query_row("SELECT metadata FROM delegation_workflows WHERE workflow = ?1", [&original.authority.workflow], |row| row.get(0)).unwrap();
    let durable: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(durable["authority"], serde_json::to_value(&original.authority).unwrap());
    assert!(durable["pointer"].is_null());
    assert!(!fixture.live.config.join("workflow-continuations").join(&original.authority.workflow).join("index-v1.json").exists());
    assert_eq!(fixture.byte_count(&first), bytes);
    assert_eq!(fixture.child_count(), 1);
}

//! Followups exercise the registered tools through the shared binary/PTY fixture.
use super::*;

struct OwnedFollowup(Option<RecoveryFixture>);
impl OwnedFollowup {
    fn new() -> Self {
        Self(Some(RecoveryFixture::new()))
    }
    fn with_model(model: Option<&str>) -> Self {
        Self(Some(RecoveryFixture::with_model(model)))
    }
}
impl std::ops::Deref for OwnedFollowup {
    type Target = RecoveryFixture;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().unwrap()
    }
}
impl Drop for OwnedFollowup {
    fn drop(&mut self) {
        let fixture = self.0.take().unwrap();
        let root = fixture.live.root.path().to_owned();
        let server = fixture.live.server_pid().unwrap();
        let mut groups = fixture.live.owned_groups();
        groups.extend(fixture.live.session_groups());
        groups.sort_unstable();
        groups.dedup();
        for group in &groups {
            fixture.live.own_group(*group);
        }
        drop(fixture);
        let groups_absent = groups.iter().all(|group| {
            let absent = unsafe { libc::kill(-*group, 0) } == -1;
            absent && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        });
        let server_absent = unsafe { libc::kill(server as i32, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
        eprintln!(
            "FOLLOWUP_CLEANUP root={} server={} groups={:?} groups_absent={} server_absent={} root_removed={}",
            root.display(),
            server,
            groups,
            groups_absent,
            server_absent,
            !root.exists()
        );
        if !std::thread::panicking() {
            assert!(
                groups_absent && server_absent && !root.exists(),
                "owned fixture cleanup must be proven"
            );
        }
    }
}

fn complete(fixture: &RecoveryFixture, snapshot: &AssignmentSnapshot) -> AssignmentSnapshot {
    fs::write(fixture.live.root.path().join("allow-result"), "1").unwrap();
    wait_file(&snapshot.brief.with_extension("json.published"));
    serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":snapshot.handle}),
    ))
    .unwrap()
}

fn followup(
    fixture: &RecoveryFixture,
    current: &AssignmentSnapshot,
    revised: Option<ResearchContract>,
) -> serde_json::Value {
    fixture.tool("ovrcr_followup", serde_json::json!({"handle":current.handle,"expected_identity":current.identity,"expected_result_version":current.result_version,"text":"Clarify the README conclusion and return fresh evidence.","revised_contract":revised}))
}

fn accept(fixture: &RecoveryFixture, current: &AssignmentSnapshot) -> AssignmentSnapshot {
    let publication = current.publication.as_ref().expect("current publication");
    serde_json::from_value(fixture.tool("ovrcr_accept", serde_json::json!({"handle":current.handle,"identity":current.identity,"result_version":current.result_version,"result_digest":publication.identity.digest}))).unwrap()
}

#[test]
fn public_followup_versions_contract_results_and_parent_acceptance_in_one_child() {
    let fixture = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let collected = complete(&fixture, &started);
    assert_eq!(collected.acceptance, "pending");
    let forged = collected.brief.parent().unwrap().join("acceptance-v1.json");
    fs::write(&forged, "local file edits do not grant parent acceptance").unwrap();
    let local_edit: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":collected.handle}),
    ))
    .unwrap();
    assert!(local_edit.acceptance_ref.is_none());
    assert_eq!(local_edit.acceptance, "pending");
    fs::remove_file(forged).unwrap();
    let child_acceptance: serde_json::Value = serde_json::from_slice(
        &fs::read(collected.brief.with_extension("json.child-acceptance")).unwrap(),
    )
    .unwrap();
    assert_eq!(child_acceptance["details"]["dispatch"], "Refused");
    let original = accept(&fixture, &collected);
    let root = original.brief.parent().unwrap();
    let original_bytes: Vec<_> = [
        "contract-v1.json",
        "brief-v1.json",
        "submission-v1.json",
        "acceptance-v1.json",
    ]
    .into_iter()
    .map(|name| (name, fs::read(root.join(name)).unwrap()))
    .collect();
    let again = accept(&fixture, &original);
    assert_eq!(
        again.acceptance_ref, original.acceptance_ref,
        "acceptance retries are idempotent"
    );
    let other: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let other = accept(&fixture, &complete(&fixture, &other));

    let clarified: AssignmentSnapshot =
        serde_json::from_value(followup(&fixture, &original, None)).unwrap();
    assert_eq!(
        clarified.identity, original.identity,
        "clarification retains assignment and contract identity"
    );
    assert_eq!(clarified.result_version, 2);
    assert!(clarified.submission.is_none());
    assert!(clarified.acceptance_ref.is_none());
    let stale_followup = followup(&fixture, &original, None);
    assert_eq!(
        stale_followup["dispatch"], "Refused",
        "an old same-contract snapshot cannot supersede the new expected result"
    );
    let initial_replay = fixture.control(fixture.proxy.original(DispatchOperation::InitialInput));
    assert_eq!(initial_replay.result_version, 2);
    let clarified = complete(&fixture, &clarified);
    assert_eq!(fixture.input_count(&clarified), 1);
    let clarified = accept(&fixture, &clarified);
    let mut contract: ResearchContract =
        serde_json::from_slice(&fs::read(root.join("contract-v1.json")).unwrap()).unwrap();
    contract.scope = "Read README and specifically explain its installation prerequisites.".into();
    contract.acceptance_evidence = vec!["README prerequisite citation".into()];
    let revised: AssignmentSnapshot =
        serde_json::from_value(followup(&fixture, &clarified, Some(contract))).unwrap();
    assert_eq!(revised.identity.contract, 2);
    let mut stable = revised.identity.clone();
    stable.contract = 1;
    assert_eq!(stable, original.identity);
    assert_eq!(revised.result_version, 3);
    assert_eq!(revised.conversation, original.conversation);
    assert!(revised.publication.is_none());
    assert!(revised.acceptance_ref.is_none());
    assert_eq!(revised.history[0].acceptance, original.acceptance_ref);
    assert_eq!(revised.history[1].acceptance, clarified.acceptance_ref);
    let revised = complete(&fixture, &revised);
    assert_eq!(revised.submission.as_ref().unwrap().identity.contract, 2);
    assert_eq!(revised.submission.as_ref().unwrap().version, 3);
    let late: serde_json::Value = serde_json::from_slice(
        &fs::read(revised.brief.with_extension("json.late-result")).unwrap(),
    )
    .unwrap();
    assert!(
        late["error"].as_str().unwrap().contains("stale"),
        "late old callback must not publish a current result: {late}"
    );
    assert_eq!(fixture.input_count(&revised), 1);
    let first_request = clarified
        .receipts
        .iter()
        .find(|r| r.operation == DispatchOperation::Followup)
        .unwrap();
    let recovered_original: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_followup",
        serde_json::json!({"handle":revised.handle,"request":first_request.request}),
    ))
    .unwrap();
    assert_eq!(recovered_original.result_version, 3);
    let original_receipt = recovered_original
        .receipts
        .iter()
        .find(|r| r.request == first_request.request)
        .unwrap();
    assert_eq!(
        original_receipt, first_request,
        "a request keeps its immutable target after current version advances"
    );
    assert_eq!(original_receipt.result_version, 2);
    let recovered_initial: AssignmentSnapshot =
        serde_json::from_value(fixture.recover(&revised.handle)).unwrap();
    assert_eq!(
        recovered_initial.identity, revised.identity,
        "initial request recovery never resets a revised contract"
    );

    assert_eq!(
        fixture.child_count(),
        2,
        "only the independent task creates another child"
    );
    for (name, bytes) in original_bytes {
        assert_eq!(
            fs::read(root.join(name)).unwrap(),
            bytes,
            "immutable {name}"
        );
    }
    for version in [1, 2] {
        let old: AssignmentSnapshot = serde_json::from_value(fixture.tool(
            "ovrcr_collect",
            serde_json::json!({"handle":revised.handle,"version":version}),
        ))
        .unwrap();
        assert_eq!(old.identity, revised.identity);
        assert_eq!(old.selected_version, Some(version));
        assert_eq!(old.historical_submission.unwrap().version, version);
        assert!(
            old.submission.is_none(),
            "historical collection never supplies a current result"
        );
    }
    let other_now: AssignmentSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_collect", serde_json::json!({"handle":other.handle})),
    )
    .unwrap();
    assert_eq!(
        other_now.acceptance_ref, other.acceptance_ref,
        "unaffected assignment approval survives"
    );
    let prior_acceptance = fs::read(root.join("acceptance-v1.json")).unwrap();
    for (name, reason) in [
        ("contract-v1.json", "immutable contract changed"),
        ("brief-v1.json", "immutable brief changed"),
    ] {
        let path = root.join(name);
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, "tampered historical bytes").unwrap();
        let rejected = tool_reply(
            &fixture.live,
            fixture.parent,
            "ovrcr_collect",
            serde_json::json!({"handle":revised.handle,"version":1}),
            &fixture.live.root.path().join(format!("tampered-{name}")),
        );
        assert!(
            rejected["error"].as_str().unwrap().contains(reason),
            "production historical collection must validate {name}: {rejected}"
        );
        let current: AssignmentSnapshot = serde_json::from_value(fixture.tool(
            "ovrcr_collect",
            serde_json::json!({"handle":revised.handle}),
        ))
        .unwrap();
        assert_eq!(
            current.submission, revised.submission,
            "unaffected current version remains collectable"
        );
        assert_eq!(current.history[0].acceptance, original.acceptance_ref);
        assert_eq!(
            fs::read(root.join("acceptance-v1.json")).unwrap(),
            prior_acceptance
        );
        fs::write(path, bytes).unwrap();
    }
    let database =
        rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let metadata: String = database
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [&revised.handle],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!metadata.contains("Clarify the README conclusion"));
    assert!(!metadata.contains("specifically explain its installation prerequisites"));
    let stale = fixture.tool("ovrcr_accept", serde_json::json!({"handle":revised.handle,"identity":original.identity,"result_version":1,"result_digest":original.publication.unwrap().identity.digest}));
    assert_eq!(stale["dispatch"], "Refused");
}

#[test]
fn public_followup_lost_request_or_reply_recovers_original_receipt_once() {
    for loss in [0, 1] {
        let fixture = OwnedFollowup::new();
        let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
        let current = complete(&fixture, &started);
        fixture.proxy.arm(ControlRule {
            operation: "Followup",
            loss,
            fault: None,
        });
        let lost = followup(&fixture, &current, None);
        assert_eq!(lost["dispatch"], "Unknown", "{lost}");
        assert!(lost.get("receipts").is_none());
        let request = lost["request"]
            .as_str()
            .expect("recoverable original request");
        let recovered: AssignmentSnapshot = serde_json::from_value(fixture.tool(
            "ovrcr_followup",
            serde_json::json!({"handle":current.handle,"request":request}),
        ))
        .unwrap();
        assert_eq!(recovered.identity, current.identity);
        assert_eq!(recovered.result_version, 2);
        assert_eq!(recovered.dispatch, DispatchStatus::Applied);
        let recovered = complete(&fixture, &recovered);
        assert_eq!(fixture.input_count(&recovered), 1);
        let before = fixture.byte_count(&recovered);
        let replay: AssignmentSnapshot = serde_json::from_value(fixture.tool(
            "ovrcr_followup",
            serde_json::json!({"handle":current.handle,"request":request}),
        ))
        .unwrap();
        assert_eq!(replay.receipts, recovered.receipts);
        assert_eq!(fixture.byte_count(&recovered), before);
        assert_eq!(fixture.child_count(), 1);
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_followup_partial_unknown_and_fresh_request_cannot_bypass_original_effect() {
    for (point, limit, outcome) in [
        ("input", 7, DispatchOutcome::Partial),
        ("input", usize::MAX, DispatchOutcome::Unknown),
        ("input-after-effect", 0, DispatchOutcome::Unknown),
    ] {
        let fixture = OwnedFollowup::new();
        let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
        let current = complete(&fixture, &started);
        fixture.proxy.arm(ControlRule {
            operation: "Followup",
            loss: 2,
            fault: Some((point, limit)),
        });
        let uncertain: AssignmentSnapshot =
            serde_json::from_value(followup(&fixture, &current, None)).unwrap();
        let receipt = uncertain
            .receipts
            .iter()
            .find(|r| r.operation == DispatchOperation::Followup)
            .unwrap();
        assert_eq!(receipt.outcome, outcome);
        if limit == 7 {
            assert_eq!(receipt.input.as_ref().unwrap().written_bytes, 7);
        } else {
            wait_file(&uncertain.brief.with_extension("json.started"));
        }
        let before = fixture.byte_count(&uncertain);
        let replay = fixture.tool(
            "ovrcr_followup",
            serde_json::json!({"handle":current.handle,"request":receipt.request}),
        );
        assert_eq!(replay["result_version"], 2);
        let bypass = followup(&fixture, &uncertain, None);
        assert_eq!(bypass["dispatch"], "Refused", "{bypass}");
        assert_eq!(
            fixture.byte_count(&uncertain),
            before,
            "fresh ID must not add bytes"
        );
        assert_eq!(fixture.child_count(), 1);
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_followup_recovers_interrupted_files_before_input() {
    for point in ["followup-before-files", "followup-after-files"] {
        let fixture = OwnedFollowup::new();
        let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
        let current = accept(&fixture, &complete(&fixture, &started));
        fixture.proxy.arm(ControlRule {
            operation: "Followup",
            loss: 2,
            fault: Some((point, 0)),
        });
        let pending: AssignmentSnapshot =
            serde_json::from_value(followup(&fixture, &current, None)).unwrap();
        assert_eq!(pending.result_version, 1);
        assert_eq!(pending.acceptance_ref, current.acceptance_ref);
        let receipt = pending
            .receipts
            .iter()
            .find(|r| r.operation == DispatchOperation::Followup)
            .unwrap();
        assert_eq!(receipt.outcome, DispatchOutcome::Accepted);
        assert_eq!(receipt.result_version, 2);
        let bypass = followup(&fixture, &pending, None);
        assert_eq!(bypass["dispatch"], "Refused");
        let recovered: AssignmentSnapshot = serde_json::from_value(fixture.tool(
            "ovrcr_followup",
            serde_json::json!({"handle":pending.handle,"request":receipt.request}),
        ))
        .unwrap();
        let recovered = complete(&fixture, &recovered);
        assert_eq!(recovered.result_version, 2);
        assert_eq!(fixture.input_count(&recovered), 1);
        assert_eq!(fixture.child_count(), 1);
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_followup_zero_write_is_refused_and_requires_explicit_new_request() {
    let fixture = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let current = complete(&fixture, &started);
    fixture.proxy.arm(ControlRule {
        operation: "Followup",
        loss: 2,
        fault: Some(("input", 0)),
    });
    let rejected: AssignmentSnapshot =
        serde_json::from_value(followup(&fixture, &current, None)).unwrap();
    let receipt = rejected
        .receipts
        .iter()
        .find(|r| r.operation == DispatchOperation::Followup)
        .unwrap();
    assert_eq!(receipt.outcome, DispatchOutcome::Rejected);
    assert_eq!(receipt.input.as_ref().unwrap().written_bytes, 0);
    let before = fixture.byte_count(&rejected);
    let replay: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_followup",
        serde_json::json!({"handle":rejected.handle,"request":receipt.request}),
    ))
    .unwrap();
    assert_eq!(replay.receipts, rejected.receipts);
    assert_eq!(fixture.byte_count(&replay), before);
    let fresh: AssignmentSnapshot =
        serde_json::from_value(followup(&fixture, &rejected, None)).unwrap();
    let fresh = complete(&fixture, &fresh);
    assert_eq!(fresh.result_version, 3);
    assert_eq!(fixture.input_count(&fresh), 1);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_followup_child_a_to_b_to_a_at_write_boundary_rejects_input() {
    let fixture = OwnedFollowup::with_model(Some("fixture-model"));
    let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let current = complete(&fixture, &started);
    let before = fixture.byte_count(&current);
    fixture.proxy.arm(ControlRule {
        operation: "Followup",
        loss: 2,
        fault: Some(("input-before-write", 0)),
    });
    let root = fixture.live.root.path().to_owned();
    let rebind = std::thread::spawn(move || {
        wait_file(&root.join("fault.hit"));
        fs::write(root.join("fault.rebind"), "rebind").unwrap();
        wait_file(&root.join("fault.rebind.done"));
        fs::write(root.join("fault.release"), "release").unwrap();
    });
    let rejected: AssignmentSnapshot =
        serde_json::from_value(followup(&fixture, &current, None)).unwrap();
    rebind.join().unwrap();
    let receipt = rejected
        .receipts
        .iter()
        .find(|r| r.operation == DispatchOperation::Followup)
        .unwrap();
    assert_eq!(receipt.outcome, DispatchOutcome::Rejected);
    assert!(
        receipt.input.is_none(),
        "binding rejection precedes the counted write"
    );
    assert_eq!(rejected.result_version, current.result_version);
    assert_eq!(fixture.byte_count(&rejected), before);
    assert_eq!(
        rejected.conversation, current.conversation,
        "returning to A does not restore its generation"
    );
    assert_eq!(followup(&fixture, &rejected, None)["dispatch"], "Refused");
    assert_eq!(fixture.byte_count(&rejected), before);
}

#[test]
fn public_followup_replaced_run_preserves_history_and_rejects_new_input() {
    let fixture = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let current = complete(&fixture, &started);
    let child = current.identity.session.unwrap();
    for group in fixture.live.session_groups() {
        fixture.live.own_group(group);
    }
    assert_eq!(
        fixture
            .live
            .request(Request::KillSession { session: child }),
        Response::Ok
    );
    assert!(matches!(
        fixture.live.request(Request::ReopenSession {
            session: child,
            expected_run: current.identity.run.unwrap(),
            acknowledge_stopped: false
        }),
        Response::CreatedSession(_)
    ));
    assert_eq!(followup(&fixture, &current, None)["dispatch"], "Refused");
    let history: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":current.handle,"version":1}),
    ))
    .unwrap();
    assert_eq!(
        history.historical_submission.unwrap(),
        current.submission.unwrap()
    );
    assert!(history.submission.is_none());
    assert_ne!(
        history.resources.current_session.unwrap().run,
        current.identity.run.unwrap()
    );
}

#[test]
fn public_followup_storage_limit_preserves_last_result_acceptance_and_all_receipts() {
    let fixture = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let mut current = accept(&fixture, &complete(&fixture, &started));
    for version in 2..=65 {
        let next: AssignmentSnapshot =
            serde_json::from_value(followup(&fixture, &current, None)).unwrap();
        assert_eq!(next.result_version, version);
        current = accept(&fixture, &complete(&fixture, &next));
    }
    assert_eq!(current.history.len(), 64);
    assert_eq!(current.receipts.len(), 67);
    let before = fixture.byte_count(&current);
    let refused = followup(&fixture, &current, None);
    assert_eq!(refused["dispatch"], "Refused");
    assert!(refused["reason"].as_str().unwrap().contains("64 followups"));
    let unchanged: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":current.handle}),
    ))
    .unwrap();
    assert_eq!(
        unchanged, current,
        "exhaustion cannot invalidate current references or evict old receipts"
    );
    assert_eq!(fixture.byte_count(&current), before);
    let first: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":current.handle,"version":1}),
    ))
    .unwrap();
    assert_eq!(first.historical_submission.unwrap().version, 1);
    assert_eq!(fixture.child_count(), 1);
}

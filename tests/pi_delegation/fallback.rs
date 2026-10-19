//! Registered production-path fallback regressions.
//! Actual managed child PTY emits correlated artifacts; registered parent tools
//! capture, publish and explicitly accept. File gates drive only fixture model work.
use super::followups::OwnedFollowup;
use super::*;
use std::os::unix::fs::OpenOptionsExt;

fn start(f: &RecoveryFixture, mode: &str) -> AssignmentSnapshot {
    let child: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_start", serde_json::json!({
        "project":PROJECT,"workspace":f.workspace,
        "scope":format!("{mode}: return the complete README verbatim. TASK_PAYLOAD_FALLBACK_PRIVATE"),
        "model_provider":"test","model":"fixture-model",
        "acceptance_evidence":["Complete README content"]
    }))).unwrap();
    assert_eq!(child.dispatch, DispatchStatus::Applied);
    wait_fallback_screen(&f.live, &child);
    for group in f.live.session_groups() {
        f.live.own_group(group);
    }
    serde_json::from_value(f.tool("ovrcr_inspect", serde_json::json!({"handle":child.handle})))
        .unwrap()
}

fn wait_fallback_screen(live: &Live, child: &AssignmentSnapshot) {
    wait_file(&child.brief.with_extension("json.fallback-ready"));
    let emitted = fs::read_to_string(child.brief.with_extension("json.fallback-ready")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::TerminalText { text, .. } = live.request(Request::ReadTerminal {
            session: child.identity.session.unwrap(),
            max_lines: None,
        }) else {
            panic!("terminal text");
        };
        // Synchronization only; the trusted capture makes the attribution decision.
        if text.contains(emitted.trim_end()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "child marker did not reach actual parser"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn capture_args(child: &AssignmentSnapshot) -> serde_json::Value {
    serde_json::json!({"handle":child.handle,"expected_identity":child.identity,
        "expected_result_version":child.result_version,"expected_control_epoch":child.control.epoch})
}

fn capture(f: &RecoveryFixture, child: &AssignmentSnapshot) -> FallbackCapture {
    serde_json::from_value(f.tool("ovrcr_capture_fallback", capture_args(child))).unwrap()
}

fn file_digest(path: &Path) -> String {
    let output = std::process::Command::new("node")
        .args(["--input-type=module", "-e", "import {createHash} from 'node:crypto';import {readFileSync} from 'node:fs';console.log(createHash('sha256').update(readFileSync(process.argv[1])).digest('hex'));"])
        .arg(path).output().unwrap();
    assert!(output.status.success(), "fixture file hashing failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn parent_draft(
    f: &RecoveryFixture,
    child: &AssignmentSnapshot,
    captured: &FallbackCapture,
) -> FallbackRecoveryDraft {
    parent_draft_in(&f.live.repo, child, captured)
}

fn parent_draft_in(
    repo: &Path,
    child: &AssignmentSnapshot,
    captured: &FallbackCapture,
) -> FallbackRecoveryDraft {
    let root = child.brief.parent().unwrap();
    let bytes = fs::read(root.join("fallback-evidence.json")).unwrap();
    let artifact: FallbackArtifact = serde_json::from_slice(&bytes).unwrap();
    // Independent inspection proves the entire requested deliverable, not a
    // fixture child's self-reported success or an arbitrary matching excerpt.
    let source = fs::read(repo.join("README")).unwrap();
    assert_eq!(artifact.deliverable.as_bytes(), source);
    let name = format!("independent-{}.txt", captured.id);
    let path = root.join(&name);
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&source).unwrap();
    file.sync_all().unwrap();
    FallbackRecoveryDraft {
        capture_id: captured.id.clone(), version: 1,
        artifact: FallbackFileReference { name:"fallback-evidence.json".into(),digest:file_digest(&root.join("fallback-evidence.json")) },
        original: captured.original.clone(),
        evidence: vec![FallbackEvidence {
            requirement:"Complete README content".into(),
            file:FallbackFileReference {name:name.clone(),digest:file_digest(&root.join(&name))},
            excerpt:String::from_utf8(source).unwrap(),
            assessment:"Independently read the actual repository README; the artifact contains every byte requested by the scope.".into()
        }],
    }
}

fn recover_args(child: &AssignmentSnapshot, draft: &FallbackRecoveryDraft) -> serde_json::Value {
    serde_json::json!({"handle":child.handle,"draft":draft})
}

fn refused(f: &RecoveryFixture, name: &str, args: serde_json::Value, label: &str) {
    let reply = tool_reply(
        &f.live,
        f.parent,
        name,
        args,
        &f.live
            .root
            .path()
            .join(format!("fallback-refused-{label}.json")),
    );
    assert!(reply.get("error").is_some(), "{reply}");
}

fn takeover(f: &RecoveryFixture, child: &AssignmentSnapshot, request: &str) {
    let mut dashboard = connect_server(&f.live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert!(matches!(
        dashboard_view(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            2,
            Request::SetView {
                view: DashboardView {
                    revision: 1,
                    panes: vec![PaneTarget {
                        session: child.identity.session.unwrap(),
                        run: child.identity.run.unwrap(),
                        size: TerminalSize {
                            rows: 24,
                            cols: 120
                        }
                    }],
                    focused: child.identity.session
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            3,
            Request::AssignmentControl(AssignmentControlRequest::Takeover {
                request: request.into(),
                session: child.identity.session.unwrap(),
                run: child.identity.run.unwrap(),
                expected_epoch: child.control.epoch,
                view_revision: 1
            })
        )
        .unwrap(),
        Response::Ok
    );
}

#[test]
fn draft_public_complete_recovery_preserves_missing_or_invalid_original_without_acceptance() {
    let mut copied_recovery = None;
    for mode in ["FALLBACK_COMPLETE_MISSING", "FALLBACK_COMPLETE_INVALID"] {
        let f = OwnedFollowup::new();
        let child = start(&f, mode);
        let original = fs::read(&child.result_path).ok();
        let before_bytes = f.byte_count(&child);
        let receipts = child.receipts.clone();
        let invalid = mode == "FALLBACK_COMPLETE_INVALID";
        if invalid {
            refusal_as(
                &f,
                f.parent,
                "ovrcr_collect",
                serde_json::json!({"handle":child.handle}),
            );
            assert!(stored_record(&f, &child.handle)["result_digest"].is_null());
        }
        let captured = capture(&f, &child);
        assert_eq!(captured.context.identity, child.identity);
        assert_eq!(captured.context.result_version, child.result_version);
        assert_eq!(captured.context.control, child.control);
        assert_eq!(captured.original_bytes, original);
        let mut draft = parent_draft(&f, &child, &captured);
        let copied_path = child
            .brief
            .parent()
            .unwrap()
            .join(format!("parent-fallback-r{}-v1.json", child.result_version));
        if invalid {
            // A foreign frozen recovery copied into the right directory is not registration.
            fs::write(&copied_path, copied_recovery.as_ref().unwrap()).unwrap();
            refusal_as(
                &f,
                f.parent,
                "ovrcr_collect",
                serde_json::json!({"handle":child.handle}),
            );
            assert!(f.reconcile(&child.handle).recoveries.is_empty());
            draft.version = 2;
        }
        #[cfg(feature = "acceptance-diagnostics")]
        let orphan_bytes = if invalid {
            fs::write(
                f.live.root.path().join("fault"),
                format!("fallback-recovery-before-save|{}|0", child.handle),
            )
            .unwrap();
            refused(
                &f,
                "ovrcr_recover_fallback",
                recover_args(&child, &draft),
                "complete-orphan",
            );
            let orphan = child
                .brief
                .parent()
                .unwrap()
                .join(format!("parent-fallback-r{}-v2.json", child.result_version));
            let bytes = fs::read(orphan).unwrap();
            // Even an exact current-context recovery exposed before registry save
            // cannot grant collection authority or become a child submission.
            refusal_as(
                &f,
                f.parent,
                "ovrcr_collect",
                serde_json::json!({"handle":child.handle}),
            );
            assert!(f.reconcile(&child.handle).recoveries.is_empty());
            assert!(stored_record(&f, &child.handle)["result_digest"].is_null());
            Some(bytes)
        } else {
            None
        };
        // Proposed response: immutable private path + digest + full recovery.
        let reply = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
        let path = Path::new(reply["path"].as_str().unwrap());
        let bytes = fs::read(path).unwrap();
        let recovery: FallbackRecovery = serde_json::from_slice(&bytes).unwrap();
        if invalid {
            assert_eq!(
                fs::read(&copied_path).unwrap(),
                *copied_recovery.as_ref().unwrap()
            );
            #[cfg(feature = "acceptance-diagnostics")]
            assert_eq!(
                Some(&bytes),
                orphan_bytes.as_ref(),
                "authorized retry preserves orphan bytes"
            );
        } else {
            copied_recovery = Some(bytes.clone());
        }
        assert_eq!(recovery.capture, captured);
        assert_eq!(recovery.capture.original_bytes, original);
        assert_eq!(recovery.author, recovery.capture.authority);
        assert_eq!(
            recovery.evidence[0].assessment,
            draft.evidence[0].assessment
        );
        assert_eq!(
            recovery.evidence_files[0].bytes,
            fs::read(f.live.repo.join("README")).unwrap()
        );
        let current: AssignmentSnapshot = serde_json::from_value(
            f.tool("ovrcr_collect", serde_json::json!({"handle":child.handle})),
        )
        .unwrap();
        assert_eq!(current.acceptance, "pending");
        assert!(current.acceptance_ref.is_none());
        assert!(current.publication.is_none());
        assert!(current.submission.is_none());
        assert_eq!(current.submission_issue.is_some(), invalid);
        assert_eq!(current.recoveries.len(), 1);
        assert_eq!(current.recoveries[0].version, draft.version);
        assert!(current.accepted_recovery.is_none());
        assert!(stored_record(&f, &child.handle)["result_digest"].is_null());
        assert_eq!(current.receipts, receipts);
        assert_eq!(current.execution_attempts, Some(1));
        assert_eq!(fs::read(&child.result_path).ok(), original);
        assert_eq!(
            f.byte_count(&child),
            before_bytes,
            "capture/recovery send no input"
        );
        assert_eq!(f.child_count(), 1);
        assert_eq!(
            f.tool("ovrcr_recover_fallback", recover_args(&child, &draft)),
            reply
        );
        let events = serde_json::to_string(&cli_json(&f.live, &["events"])).unwrap();
        assert!(!events.contains("TASK_PAYLOAD_FALLBACK_PRIVATE"));
        assert!(!events.contains("OVRCR_FALLBACK:"));
        assert!(!events.contains(&draft.evidence[0].assessment));
    }
}

#[test]
fn draft_public_same_run_old_future_unrelated_and_truncated_markers_stay_incomplete() {
    for mode in [
        "FALLBACK_OLD_SAME_RUN",
        "FALLBACK_PREPARED_FUTURE",
        "FALLBACK_UNRELATED_ARTIFACT",
        "FALLBACK_TRUNCATED_SUCCESS",
    ] {
        let f = OwnedFollowup::new();
        let child = start(&f, mode);
        let captured = capture(&f, &child);
        let before = f.byte_count(&child);
        // The parent inspects source bytes even on a rejected candidate. Marker
        // modes leave a full current artifact but display stale/unrelated output.
        let draft = parent_draft(&f, &child, &captured);
        refused(
            &f,
            "ovrcr_recover_fallback",
            recover_args(&child, &draft),
            mode,
        );
        let current = f.reconcile(&child.handle);
        assert_eq!(current.identity.session, child.identity.session);
        assert_eq!(current.identity.run, child.identity.run);
        assert!(current.acceptance_ref.is_none());
        assert_eq!(current.acceptance, "pending");
        assert_eq!(f.byte_count(&child), before);
        assert!(
            !child
                .brief
                .parent()
                .unwrap()
                .join(format!("parent-fallback-r{}-v1.json", child.result_version))
                .exists()
        );
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_sparse_current_result_uses_actual_reserved_and_rejected_followups() {
    let f = OwnedFollowup::new();
    let mut child = start(&f, "FALLBACK_SPARSE_CURRENT_RESULT");
    let mut dashboard = super::assignment_control::dashboard(&f, &child);
    for round in 0..3 {
        let before = child.result_version;
        f.proxy.arm(ControlRule {
            operation: "Followup",
            loss: 2,
            fault: Some(("followup-after-files", 0)),
        });
        let pending: AssignmentSnapshot =
            serde_json::from_value(super::followups::followup(&f, &child, None)).unwrap();
        assert_eq!(
            pending.result_version, before,
            "prepared future brief is not a current version"
        );
        let intent = pending
            .receipts
            .iter()
            .rev()
            .find(|r| {
                r.operation == DispatchOperation::Followup && r.outcome == DispatchOutcome::Accepted
            })
            .unwrap();
        let request = intent.request.clone();
        let returned_name = format!("sparse-return-{round}");
        super::assignment_control::transition(
            &f,
            &mut dashboard,
            child.identity.session.unwrap(),
            &format!("sparse-take-{round}"),
            true,
            1,
        );
        let returned = super::assignment_control::transition(
            &f,
            &mut dashboard,
            child.identity.session.unwrap(),
            &returned_name,
            false,
            1,
        );
        let confirmed = super::assignment_control::confirm(
            &f,
            &returned,
            &returned_name,
            &format!("sparse-confirm-{round}"),
        );
        let rejected: AssignmentSnapshot = serde_json::from_value(f.tool(
            "ovrcr_followup",
            serde_json::json!({"handle":child.handle,"request":request}),
        ))
        .unwrap();
        assert!(
            rejected
                .receipts
                .iter()
                .any(|r| r.request == request && r.outcome == DispatchOutcome::Rejected)
        );
        assert_eq!(rejected.result_version, before);
        assert_eq!(rejected.control, confirmed.control);
        child = serde_json::from_value(super::followups::followup(&f, &rejected, None)).unwrap();
        assert_eq!(
            child.result_version,
            before + 2,
            "reserved rejected result remains a gap"
        );
        wait_fallback_screen(&f.live, &child);
    }
    assert_eq!(child.result_version, 7);
    let captured = capture(&f, &child);
    let draft = parent_draft(&f, &child, &captured);
    let mut wrong = capture_args(&child);
    wrong["expected_result_version"] = serde_json::json!(8);
    refused(&f, "ovrcr_capture_fallback", wrong, "prepared-version");
    let saved = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
    assert!(
        saved["path"]
            .as_str()
            .unwrap()
            .ends_with("parent-fallback-r7-v1.json")
    );
    assert_eq!(f.reconcile(&child.handle).execution_attempts, Some(1));
}

#[test]
fn draft_public_changed_original_or_evidence_after_capture_cannot_publish_recovery() {
    for changed in ["original", "evidence", "artifact"] {
        let f = OwnedFollowup::new();
        let child = start(&f, "FALLBACK_COMPLETE_INVALID");
        let captured = capture(&f, &child);
        let draft = parent_draft(&f, &child, &captured);
        let root = child.brief.parent().unwrap();
        let target = match changed {
            "original" => child.result_path.clone(),
            "evidence" => root.join(&draft.evidence[0].file.name),
            "artifact" => root.join(&draft.artifact.name),
            _ => unreachable!(),
        };
        fs::write(&target, b"substituted after capture").unwrap();
        refused(
            &f,
            "ovrcr_recover_fallback",
            recover_args(&child, &draft),
            changed,
        );
        assert_eq!(fs::read(target).unwrap(), b"substituted after capture");
        assert!(f.reconcile(&child.handle).acceptance_ref.is_none());
        assert!(!root.join("parent-fallback-r1-v1.json").exists());
    }
}

#[test]
fn draft_public_human_and_uncertain_context_cannot_be_cleared_by_capture() {
    for mode in [
        "FALLBACK_COMPLETE_MISSING",
        "FALLBACK_COMPLETE_MISSING REBIND_AFTER_RESULT",
    ] {
        let f = OwnedFollowup::new();
        let initial = start(&f, mode);
        if mode.contains("REBIND") {
            fs::write(
                initial.brief.with_extension("json.rebind"),
                "change real child reporting binding",
            )
            .unwrap();
            wait_file(&initial.brief.with_extension("json.rebound"));
        } else {
            takeover(&f, &initial, "take-before-fallback");
        }
        let child: AssignmentSnapshot = serde_json::from_value(f.tool(
            "ovrcr_inspect",
            serde_json::json!({"handle":initial.handle}),
        ))
        .unwrap();
        assert!(child.control.owner != ControlOwner::Controller || child.control.barrier.is_some());
        let before = f.byte_count(&child);
        let receipts = child.receipts.clone();
        refused(
            &f,
            "ovrcr_capture_fallback",
            capture_args(&child),
            if mode.contains("REBIND") {
                "uncertain"
            } else {
                "human"
            },
        );
        let after: AssignmentSnapshot = serde_json::from_value(
            f.tool("ovrcr_inspect", serde_json::json!({"handle":child.handle})),
        )
        .unwrap();
        assert_eq!(after.control, child.control);
        assert_eq!(after.receipts, receipts);
        assert_eq!(f.byte_count(&child), before);
        assert!(after.acceptance_ref.is_none());
    }
}

#[test]
fn draft_public_parent_source_retirement_invalidates_capture_without_rewriting_provenance() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let captured = capture(&f, &child);
    let draft = parent_draft(&f, &child, &captured);
    let before = f.byte_count(&child);
    assert_eq!(
        f.live.request(Request::SendTerminal {
            session: f.parent,
            text: "producer_replace:unproven-fallback-replacement".into(),
            submit: true
        }),
        Response::Ok
    );
    // Existing host command crosses the actual Reporter source-registration fence.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        if sessions
            .iter()
            .find(|s| s.id == f.parent)
            .unwrap()
            .agent
            .as_ref()
            .is_some_and(|a| a.binding.conversation == "unproven-fallback-replacement")
        {
            break;
        }
        assert!(Instant::now() < deadline, "Reporter source did not change");
        std::thread::sleep(Duration::from_millis(10));
    }
    refused(
        &f,
        "ovrcr_recover_fallback",
        recover_args(&child, &draft),
        "retired-source",
    );
    assert_ne!(
        captured.authority.binding.conversation,
        "unproven-fallback-replacement"
    );
    assert_eq!(f.byte_count(&child), before);
    assert!(
        !child
            .brief
            .parent()
            .unwrap()
            .join("parent-fallback-r1-v1.json")
            .exists()
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn draft_public_takeover_during_screen_capture_rejects_at_final_attribution_fence() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let fault = f.live.root.path().join("fault");
    let release = fault.with_extension("release");
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "release");
        }
    }
    let _release = Release(release.clone());
    fs::write(&fault, format!("fallback-after-screen|{}|0", child.handle)).unwrap();
    let output = f.live.root.path().join("pending-fallback-capture.json");
    assert_eq!(
        f.live.request(Request::SendTerminal {
            session: f.parent,
            text: format!(
                "tool:{}",
                serde_json::json!({
        "name":"ovrcr_capture_fallback","args":capture_args(&child),"output":output})
            ),
            submit: true
        }),
        Response::Ok
    );
    wait_file(&fault.with_extension("hit"));
    takeover(&f, &child, "take-during-capture");
    fs::write(release, "release").unwrap();
    wait_file(&output);
    let reply: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert!(reply.get("error").is_some(), "{reply}");
    let current = f.reconcile(&child.handle);
    assert_eq!(current.control.owner, ControlOwner::Human);
    assert!(current.acceptance_ref.is_none());
    assert!(current.publication.is_none());
}

#[test]
fn draft_public_parent_transfer_preserves_human_barrier_and_rejects_old_capture() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let captured = capture(&f, &child);
    let draft = parent_draft(&f, &child, &captured);
    takeover(&f, &child, "human-before-parent-transfer");
    let before = f.byte_count(&child);
    let Response::CreatedSession(second) =
        f.live.request(Request::CreateSession(CreateSessionRequest {
            project: PROJECT.into(),
            workspace: f.workspace.clone(),
            name: "fallback-successor".into(),
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
        panic!("second parent")
    };
    for group in f.live.session_groups() {
        f.live.own_group(group);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let target = loop {
        let parents: Vec<WorkflowParent> =
            serde_json::from_value(f.tool("ovrcr_parents", serde_json::json!({}))).unwrap();
        if let Some(target) = parents.into_iter().find(|p| p.session == second.id) {
            break target;
        }
        assert!(Instant::now() < deadline, "successor source not certified");
        std::thread::sleep(Duration::from_millis(10));
    };
    let transferred = f.tool(
        "ovrcr_transfer",
        serde_json::json!({"request":"fallback-transfer","parent":target.session,
        "parent_run":target.run,"binding":target.binding,"producer":target.producer}),
    );
    assert_eq!(
        transferred["authority"]["parent"],
        serde_json::json!(second.id)
    );
    refused(
        &f,
        "ovrcr_recover_fallback",
        recover_args(&child, &draft),
        "retired-parent",
    );
    let recovered = tool_as(
        &f,
        second.id,
        "ovrcr_recover",
        serde_json::json!({"workflow":child.identity.workflow}),
    );
    assert_eq!(recovered["authority"], transferred["authority"]);
    let reply = tool_reply(
        &f.live,
        second.id,
        "ovrcr_recover_fallback",
        recover_args(&child, &draft),
        &f.live.root.path().join("successor-old-capture.json"),
    );
    assert!(
        reply.get("error").is_some(),
        "transfer cannot clear Human control or adopt an old capture"
    );
    let current: AssignmentSnapshot = serde_json::from_value(tool(
        &f.live,
        second.id,
        "ovrcr_inspect",
        serde_json::json!({"handle":child.handle}),
        &f.live.root.path().join("successor-inspect.json"),
    ))
    .unwrap();
    assert_eq!(current.control.owner, ControlOwner::Human);
    assert_eq!(current.identity, child.identity);
    assert_eq!(current.execution_attempts, Some(1));
    assert_eq!(f.byte_count(&child), before);
    assert!(current.acceptance_ref.is_none());
    assert!(
        !child
            .brief
            .parent()
            .unwrap()
            .join("parent-fallback-r1-v1.json")
            .exists()
    );
}

fn accept_recovery(
    f: &RecoveryFixture,
    child: &AssignmentSnapshot,
    recovery: &serde_json::Value,
) -> AssignmentSnapshot {
    serde_json::from_value(f.tool("ovrcr_accept", accept_args(child, recovery))).unwrap()
}

fn accept_args(child: &AssignmentSnapshot, recovery: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({"handle":child.handle,"identity":child.identity,"result_version":child.result_version,
        "expected_control_epoch":child.control.epoch,
        "target":{"ParentRecovery":{"recovery_version":recovery["recovery"]["version"],"digest":recovery["digest"]}}})
}

#[test]
fn public_explicit_recovery_acceptance_is_single_immutable_decision_and_frozen_history() {
    for mode in ["FALLBACK_COMPLETE_MISSING", "FALLBACK_COMPLETE_INVALID"] {
        let f = OwnedFollowup::new();
        let child = start(&f, mode);
        let original = fs::read(&child.result_path).ok();
        let captured = capture(&f, &child);
        let draft = parent_draft(&f, &child, &captured);
        let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
        assert_eq!(f.reconcile(&child.handle).acceptance, "pending");
        let accepted = accept_recovery(&f, &child, &published);
        let reference = accepted.acceptance_ref.clone().unwrap();
        assert_eq!(
            reference.target,
            AcceptanceTarget::ParentRecovery {
                recovery_version: 1,
                digest: published["digest"].as_str().unwrap().into()
            }
        );
        assert!(accepted.submission.is_none());
        assert!(accepted.publication.is_none());
        assert_eq!(accepted.receipts, child.receipts);
        assert_eq!(accepted.execution_attempts, Some(1));
        assert_eq!(fs::read(&child.result_path).ok(), original);
        assert_eq!(
            accept_recovery(&f, &child, &published).acceptance_ref,
            Some(reference.clone())
        );
        let conflict = f.tool(
            "ovrcr_accept",
            serde_json::json!({"handle":child.handle,"identity":child.identity,
            "result_version":child.result_version,"expected_control_epoch":child.control.epoch,
            "target":{"ParentRecovery":{"recovery_version":2,"digest":published["digest"]}}}),
        );
        assert_eq!(conflict["dispatch"], "Refused");
        fs::write(
            child
                .brief
                .parent()
                .unwrap()
                .join(&draft.evidence[0].file.name),
            "mutable source changed after acceptance",
        )
        .unwrap();
        let historical: AssignmentSnapshot = serde_json::from_value(f.tool(
            "ovrcr_collect",
            serde_json::json!({"handle":child.handle,"version":1}),
        ))
        .unwrap();
        assert_eq!(historical.selected_acceptance_ref, Some(reference));
        assert_eq!(
            historical.accepted_recovery.unwrap().evidence_files[0].bytes,
            fs::read(f.live.repo.join("README")).unwrap()
        );
    }
}

#[test]
fn public_later_child_publication_stays_separate_from_accepted_recovery() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let captured = capture(&f, &child);
    let draft = parent_draft(&f, &child, &captured);
    let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
    let accepted = accept_recovery(&f, &child, &published);
    fs::write(
        child.brief.with_extension("json.publish-later"),
        "publish through registered child tool",
    )
    .unwrap();
    wait_file(&child.brief.with_extension("json.published-later"));
    let collected: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", serde_json::json!({"handle":child.handle})))
            .unwrap();
    assert!(collected.submission.is_some());
    assert!(collected.publication.is_some());
    assert_eq!(collected.acceptance_ref, accepted.acceptance_ref);
    assert!(collected.accepted_recovery.is_some());
    assert_eq!(collected.execution_attempts, Some(1));
}

#[test]
fn public_unregistered_or_changed_recovery_cannot_be_selected() {
    for change in ["unregistered", "substituted"] {
        let f = OwnedFollowup::new();
        let child = start(&f, "FALLBACK_COMPLETE_MISSING");
        let captured = capture(&f, &child);
        let draft = parent_draft(&f, &child, &captured);
        let mut published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
        if change == "unregistered" {
            published["recovery"]["version"] = 2.into();
        } else {
            fs::write(
                published["path"].as_str().unwrap(),
                "substituted recovery bytes",
            )
            .unwrap();
        }
        let refused = f.tool("ovrcr_accept", serde_json::json!({"handle":child.handle,"identity":child.identity,
            "result_version":child.result_version,"expected_control_epoch":child.control.epoch,
            "target":{"ParentRecovery":{"recovery_version":published["recovery"]["version"],"digest":published["digest"]}}}));
        assert_eq!(refused["dispatch"], "Refused");
        assert!(f.reconcile(&child.handle).acceptance_ref.is_none());
    }
}

#[test]
fn public_followup_archives_recovery_and_selection_without_inheriting_acceptance() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let captured = capture(&f, &child);
    let draft = parent_draft(&f, &child, &captured);
    let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
    let accepted = accept_recovery(&f, &child, &published);
    let next: AssignmentSnapshot =
        serde_json::from_value(super::followups::followup(&f, &accepted, None)).unwrap();
    wait_fallback_screen(&f.live, &next);
    assert_eq!(next.result_version, 2);
    assert!(next.acceptance_ref.is_none());
    assert!(next.accepted_recovery.is_none());
    assert!(next.recoveries.is_empty());
    assert_eq!(next.history[0].acceptance, accepted.acceptance_ref);
    assert_eq!(next.history[0].recoveries, accepted.recoveries);
    let prior: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":child.handle,"version":1}),
    ))
    .unwrap();
    assert_eq!(prior.selected_acceptance_ref, accepted.acceptance_ref);
    assert_eq!(prior.selected_recoveries, accepted.recoveries);
    assert!(prior.historical_submission.is_none());
    assert!(prior.accepted_recovery.is_some());
    assert_eq!(prior.execution_attempts, Some(1));
}

#[cfg(feature = "acceptance-diagnostics")]
fn pending_tool(
    f: &RecoveryFixture,
    child: &AssignmentSnapshot,
    point: &str,
    name: &str,
    args: serde_json::Value,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let fault = f.live.root.path().join("fault");
    fs::write(&fault, format!("{point}|{}|0", child.handle)).unwrap();
    let output = f.live.root.path().join(format!("pending-{point}.json"));
    assert_eq!(
        f.live.request(Request::SendTerminal {
            session: f.parent,
            text: format!(
                "tool:{}",
                serde_json::json!({"name":name,"args":args,"output":output,"background":true})
            ),
            submit: true
        }),
        Response::Ok
    );
    wait_file(&fault.with_extension("hit"));
    (output, fault.with_extension("release"))
}

#[cfg(feature = "acceptance-diagnostics")]
fn wait_workflow_reload_departure(f: &RecoveryFixture, expected: &WorkflowAuthority) {
    let db = rusqlite::Connection::open_with_flags(
        f.live.config.join("registry.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let departure = serde_json::json!({"Reload":{"producer":expected.producer}});
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let raw: String = db
            .query_row(
                "SELECT metadata FROM delegation_workflows WHERE workflow = ?1",
                [&expected.workflow],
                |row| row.get(0),
            )
            .unwrap();
        let row: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let authority: WorkflowAuthority =
            serde_json::from_value(row["authority"].clone()).unwrap();
        assert_eq!(
            &authority, expected,
            "reload must preserve the exact captured authority"
        );
        // Reporter reload preserves the binding and records this durable departure.
        if row["departure"] == departure {
            eprintln!(
                "FALLBACK_RELOAD_OBSERVED workflow={} epoch={} producer={}",
                expected.workflow, expected.epoch, expected.producer
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "actual Reporter reload departure not observed: {row}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_original_mutation_inside_capture_refuses_and_preserves_new_bytes() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let (output, release) = pending_tool(
        &f,
        &child,
        "fallback-after-original",
        "ovrcr_capture_fallback",
        capture_args(&child),
    );
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "release");
        }
    }
    let _release = Release(release.clone());
    fs::write(
        &child.result_path,
        "{changed while capture held its attribution fences",
    )
    .unwrap();
    fs::set_permissions(&child.result_path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(release, "release").unwrap();
    wait_file(&output);
    let reply: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert!(reply.get("error").is_some(), "{reply}");
    assert_eq!(
        fs::read_to_string(&child.result_path).unwrap(),
        "{changed while capture held its attribution fences"
    );
    assert_eq!(f.reconcile(&child.handle).control, child.control);
    assert!(
        !child
            .brief
            .parent()
            .unwrap()
            .join("fallback-capture-r1-v1.json")
            .exists()
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_retired_source_before_recovery_final_fence_cannot_publish() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let captured = capture(&f, &child);
    let draft = parent_draft(&f, &child, &captured);
    let (output, release) = pending_tool(
        &f,
        &child,
        "fallback-before-publication",
        "ovrcr_recover_fallback",
        recover_args(&child, &draft),
    );
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "release");
        }
    }
    let _release = Release(release.clone());
    assert_eq!(
        f.live.request(Request::SendTerminal {
            session: f.parent,
            text: "session_shutdown:reload".into(),
            submit: true
        }),
        Response::Ok
    );
    wait_workflow_reload_departure(&f, &captured.authority);
    fs::write(release, "release").unwrap();
    wait_file(&output);
    let reply: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert!(reply.get("error").is_some(), "{reply}");
    assert!(
        !child
            .brief
            .parent()
            .unwrap()
            .join("parent-fallback-r1-v1.json")
            .exists()
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_failed_registry_save_retains_orphans_without_registering_or_accepting_them() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let captured = capture(&f, &child);
    let draft = parent_draft(&f, &child, &captured);
    let fault = f.live.root.path().join("fault");
    fs::write(
        &fault,
        format!("fallback-recovery-before-save|{}|0", child.handle),
    )
    .unwrap();
    refused(
        &f,
        "ovrcr_recover_fallback",
        recover_args(&child, &draft),
        "recovery-save",
    );
    let path = child
        .brief
        .parent()
        .unwrap()
        .join("parent-fallback-r1-v1.json");
    let bytes = fs::read(&path).unwrap();
    assert!(f.reconcile(&child.handle).recoveries.is_empty());
    let orphan_digest = file_digest(&path);
    let orphan = serde_json::json!({"digest":orphan_digest,"recovery":{"version":1}});
    let rejected = f.tool(
        "ovrcr_accept",
        serde_json::json!({"handle":child.handle,"identity":child.identity,
        "result_version":1,"expected_control_epoch":child.control.epoch,
        "target":{"ParentRecovery":{"recovery_version":1,"digest":orphan["digest"]}}}),
    );
    assert_eq!(rejected["dispatch"], "Refused");
    let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
    assert_eq!(
        fs::read(&path).unwrap(),
        bytes,
        "exact authorized retry preserves orphan bytes"
    );
    fs::write(
        &fault,
        format!("fallback-acceptance-before-save|{}|0", child.handle),
    )
    .unwrap();
    let failed = f.tool(
        "ovrcr_accept",
        serde_json::json!({"handle":child.handle,"identity":child.identity,
        "result_version":1,"expected_control_epoch":child.control.epoch,
        "target":{"ParentRecovery":{"recovery_version":1,"digest":published["digest"]}}}),
    );
    assert_ne!(failed["acceptance"], "accepted");
    assert!(f.reconcile(&child.handle).acceptance_ref.is_none());
    let decision = fs::read(child.brief.parent().unwrap().join("acceptance-v1.json")).unwrap();
    let accepted = accept_recovery(&f, &child, &published);
    assert_eq!(accepted.acceptance, "accepted");
    assert_eq!(
        fs::read(child.brief.parent().unwrap().join("acceptance-v1.json")).unwrap(),
        decision
    );
}

#[test]
fn public_invalid_occupied_child_publication_can_repair_only_after_explicit_recovery_acceptance() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_INVALID");
    let original = fs::read(&child.result_path).unwrap();
    fs::write(
        child.brief.with_extension("json.publish-later"),
        "attempt actual child publication into occupied original",
    )
    .unwrap();
    wait_file(&child.brief.with_extension("json.published-later"));
    let publication: serde_json::Value = serde_json::from_slice(
        &fs::read(child.brief.with_extension("json.published-later")).unwrap(),
    )
    .unwrap();
    assert!(
        publication.get("error").is_some(),
        "child immutable publisher must preserve the invalid occupied file"
    );
    let blocked = super::followups::followup(&f, &child, None);
    assert_eq!(blocked["dispatch"], "Refused");
    let captured = capture(&f, &child);
    let draft = parent_draft(&f, &child, &captured);
    let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
    let accepted = accept_recovery(&f, &child, &published);
    let next: AssignmentSnapshot =
        serde_json::from_value(super::followups::followup(&f, &accepted, None)).unwrap();
    assert_eq!(next.result_version, 2);
    assert!(next.acceptance_ref.is_none());
    assert_eq!(fs::read(&child.result_path).unwrap(), original);
    assert_eq!(next.history[0].acceptance, accepted.acceptance_ref);
    assert_eq!(next.execution_attempts, Some(1));
}

fn tool_as(
    f: &RecoveryFixture,
    parent: SessionId,
    name: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let id = f.files.get();
    f.files.set(id + 1);
    tool(
        &f.live,
        parent,
        name,
        args,
        &f.live
            .root
            .path()
            .join(format!("fallback-parent-{id}.json")),
    )
}

fn refusal_as(
    f: &RecoveryFixture,
    parent: SessionId,
    name: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let id = f.files.get();
    f.files.set(id + 1);
    let reply = tool_reply(
        &f.live,
        parent,
        name,
        args,
        &f.live
            .root
            .path()
            .join(format!("fallback-refusal-{id}.json")),
    );
    assert!(
        reply.get("error").is_some() || reply["details"]["dispatch"] == "Refused",
        "{reply}"
    );
    reply
}

fn new_parent(f: &RecoveryFixture, name: &str) -> WorkflowParent {
    let Response::CreatedSession(parent) =
        f.live.request(Request::CreateSession(CreateSessionRequest {
            project: PROJECT.into(),
            workspace: f.workspace.clone(),
            name: name.into(),
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
        panic!("new managed parent")
    };
    for group in f.live.session_groups() {
        f.live.own_group(group);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        if sessions
            .iter()
            .find(|s| s.id == parent.id)
            .unwrap()
            .agent
            .is_none()
        {
            assert!(Instant::now() < deadline, "new parent reporting not ready");
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        let targets: Vec<WorkflowParent> = serde_json::from_value(tool_as(
            f,
            parent.id,
            "ovrcr_parents",
            serde_json::json!({}),
        ))
        .unwrap();
        if let Some(target) = targets.into_iter().find(|t| t.session == parent.id) {
            return target;
        }
        assert!(
            Instant::now() < deadline,
            "new parent source never certified"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn stored_record(f: &RecoveryFixture, handle: &str) -> serde_json::Value {
    let db = rusqlite::Connection::open_with_flags(
        f.live.config.join("registry.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let bytes: String = db
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [handle],
            |r| r.get(0),
        )
        .unwrap();
    serde_json::from_str(&bytes).unwrap()
}

#[cfg(feature = "acceptance-diagnostics")]
struct FaultRelease(std::path::PathBuf);
#[cfg(feature = "acceptance-diagnostics")]
impl Drop for FaultRelease {
    fn drop(&mut self) {
        let _ = fs::write(&self.0, "release");
    }
}

#[test]
fn public_stopped_or_replaced_child_refuses_fallback_without_resuming_or_retargeting() {
    for reopen in [false, true] {
        let f = OwnedFollowup::new();
        let child = start(&f, "FALLBACK_COMPLETE_MISSING");
        let captured = capture(&f, &child);
        let draft = parent_draft(&f, &child, &captured);
        let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
        let recovery_bytes = fs::read(published["path"].as_str().unwrap()).unwrap();
        let before = f.byte_count(&child);
        assert_eq!(
            f.live.request(Request::KillSession {
                session: child.identity.session.unwrap()
            }),
            Response::Ok
        );
        let expected_run = if reopen {
            let Response::CreatedSession(replacement) = f.live.request(Request::ReopenSession {
                session: child.identity.session.unwrap(),
                expected_run: child.identity.run.unwrap(),
                acknowledge_stopped: false,
            }) else {
                panic!("reopen owned fixture child")
            };
            assert_ne!(Some(replacement.run), child.identity.run);
            replacement.run
        } else {
            child.identity.run.unwrap()
        };
        refusal_as(&f, f.parent, "ovrcr_capture_fallback", capture_args(&child));
        refusal_as(
            &f,
            f.parent,
            "ovrcr_recover_fallback",
            recover_args(&child, &draft),
        );
        refusal_as(
            &f,
            f.parent,
            "ovrcr_accept",
            accept_args(&child, &published),
        );
        let current = f.reconcile(&child.handle);
        assert_eq!(current.identity, child.identity);
        assert_eq!(
            current.resources.current_session.as_ref().unwrap().run,
            expected_run
        );
        if !reopen {
            assert!(!current.resources.current_session.unwrap().phase.is_live());
        }
        assert!(current.acceptance_ref.is_none());
        assert_eq!(current.execution_attempts, Some(1));
        assert_eq!(f.byte_count(&child), before);
        assert_eq!(
            fs::read(published["path"].as_str().unwrap()).unwrap(),
            recovery_bytes
        );
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_source_retirement_at_explicit_acceptance_preserves_pending_recovery() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let captured = capture(&f, &child);
    let draft = parent_draft(&f, &child, &captured);
    let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
    let recovery_bytes = fs::read(published["path"].as_str().unwrap()).unwrap();
    let (output, release) = pending_tool(
        &f,
        &child,
        "fallback-before-acceptance",
        "ovrcr_accept",
        accept_args(&child, &published),
    );
    let _release = FaultRelease(release.clone());
    assert_eq!(
        f.live.request(Request::SendTerminal {
            session: f.parent,
            text: "session_shutdown:reload".into(),
            submit: true
        }),
        Response::Ok
    );
    wait_workflow_reload_departure(&f, &captured.authority);
    fs::write(release, "release").unwrap();
    wait_file(&output);
    let reply: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert!(
        reply.get("error").is_some()
            || matches!(
                reply["details"]["dispatch"].as_str(),
                Some("Refused" | "Unknown")
            ),
        "retired response cannot announce acceptance: {reply}"
    );
    let row = stored_record(&f, &child.handle);
    assert!(row["acceptance"].is_null());
    assert!(row["result_digest"].is_null());
    assert_eq!(row["recoveries"].as_array().unwrap().len(), 1);
    assert!(
        !child
            .brief
            .parent()
            .unwrap()
            .join("acceptance-v1.json")
            .exists()
    );
    assert_eq!(
        fs::read(published["path"].as_str().unwrap()).unwrap(),
        recovery_bytes
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_human_input_revision_during_capture_survives_return_and_refuses_final_effect() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let before = f.byte_count(&child);
    let (output, release) = pending_tool(
        &f,
        &child,
        "fallback-after-screen",
        "ovrcr_capture_fallback",
        capture_args(&child),
    );
    let _release = FaultRelease(release.clone());
    let mut dashboard = super::assignment_control::dashboard(&f, &child);
    let human = super::assignment_control::transition(
        &f,
        &mut dashboard,
        child.identity.session.unwrap(),
        "capture-human-take",
        true,
        1,
    );
    assert_eq!(
        dashboard_view(
            &mut dashboard,
            300,
            Request::AssignedHumanInput {
                session: child.identity.session.unwrap(),
                run: child.identity.run.unwrap(),
                control_epoch: human.control.epoch,
                view_revision: 1,
                bytes: b"human_marker:FALLBACK_REVISION\r".to_vec(),
            }
        )
        .unwrap(),
        Response::Ok
    );
    let returned = super::assignment_control::transition(
        &f,
        &mut dashboard,
        child.identity.session.unwrap(),
        "capture-human-return",
        false,
        1,
    );
    let DelegationRequest::Prepare { auth, .. } = f.proxy.original(DispatchOperation::Prepare)
    else {
        unreachable!()
    };
    // The registered capture holds the serial fault proxy; this concurrent
    // confirmation uses the existing authenticated production control client.
    let confirmed = f.control(DelegationRequest::ConfirmHumanReturn {
        auth,
        handle: child.handle.clone(),
        request: "capture-human-confirm".into(),
        returned_request: "capture-human-return".into(),
        expected_control_epoch: returned.control.epoch,
        evidence: Box::new(returned.control_evidence.clone().unwrap()),
    });
    assert!(confirmed.control.human_input_revision > child.control.human_input_revision);
    assert!(confirmed.control.attribution_invalidated);
    fs::write(release, "release").unwrap();
    wait_file(&output);
    let reply: serde_json::Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert!(reply.get("error").is_some(), "{reply}");
    let after = f.reconcile(&child.handle);
    assert_eq!(after.control, confirmed.control);
    assert!(after.acceptance_ref.is_none());
    assert!(f.byte_count(&child) > before);
    assert_eq!(after.execution_attempts, Some(1));
    assert!(
        stored_record(&f, &child.handle)["captures"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn public_capture_and_recovery_capacity_preserves_every_original_and_reference() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let before = f.byte_count(&child);
    let receipts = child.receipts.clone();
    let mut first = None;
    let mut capture_bytes = vec![];
    for slot in 1..=64 {
        let captured = capture(&f, &child);
        if first.is_none() {
            first = Some(captured);
        }
        let path = child
            .brief
            .parent()
            .unwrap()
            .join(format!("fallback-capture-r1-v{slot}.json"));
        capture_bytes.push((path.clone(), fs::read(path).unwrap()));
    }
    refusal_as(&f, f.parent, "ovrcr_capture_fallback", capture_args(&child));
    let mut draft = parent_draft(&f, &child, &first.unwrap());
    let mut recoveries = vec![];
    for version in 1..=64 {
        draft.version = version;
        let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
        let path = std::path::PathBuf::from(published["path"].as_str().unwrap());
        recoveries.push((path.clone(), fs::read(path).unwrap()));
    }
    draft.version = 65;
    refusal_as(
        &f,
        f.parent,
        "ovrcr_recover_fallback",
        recover_args(&child, &draft),
    );
    let current = f.reconcile(&child.handle);
    assert_eq!(
        stored_record(&f, &child.handle)["captures"]
            .as_array()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(current.recoveries.len(), 64);
    assert!(current.acceptance_ref.is_none());
    assert_eq!(current.receipts, receipts);
    assert_eq!(current.execution_attempts, Some(1));
    assert_eq!(f.byte_count(&child), before);
    for (path, bytes) in capture_bytes.into_iter().chain(recoveries) {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    let root = child.brief.parent().unwrap();
    assert!(!root.join("fallback-capture-r1-v65.json").exists());
    assert!(!root.join("parent-fallback-r1-v65.json").exists());
}

#[test]
fn public_parent_transfer_requires_fresh_unaccepted_capture_and_preserves_committed_selection() {
    for accept_before in [false, true] {
        let f = OwnedFollowup::new();
        let child = start(&f, "FALLBACK_COMPLETE_MISSING");
        let captured = capture(&f, &child);
        let draft = parent_draft(&f, &child, &captured);
        let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
        let reference = accept_before.then(|| {
            accept_recovery(&f, &child, &published)
                .acceptance_ref
                .unwrap()
        });
        let before = f.byte_count(&child);
        let target = new_parent(&f, "fallback-transfer-target");
        let transferred = f.tool("ovrcr_transfer", serde_json::json!({"request":"fallback-evidence-transfer","parent":target.session,"parent_run":target.run,"binding":target.binding,"producer":target.producer}));
        let recovered = tool_as(
            &f,
            target.session,
            "ovrcr_recover",
            serde_json::json!({"workflow":child.identity.workflow}),
        );
        assert_eq!(recovered["authority"], transferred["authority"]);
        refusal_as(&f, f.parent, "ovrcr_capture_fallback", capture_args(&child));
        let current: AssignmentSnapshot = serde_json::from_value(tool_as(
            &f,
            target.session,
            "ovrcr_inspect",
            serde_json::json!({"handle":child.handle}),
        ))
        .unwrap();
        assert_eq!(current.control, child.control);
        assert_eq!(current.identity, child.identity);
        if let Some(reference) = reference {
            let historical: AssignmentSnapshot = serde_json::from_value(tool_as(
                &f,
                target.session,
                "ovrcr_collect",
                serde_json::json!({"handle":child.handle,"version":1}),
            ))
            .unwrap();
            assert_eq!(historical.selected_acceptance_ref, Some(reference.clone()));
            assert_eq!(
                historical.accepted_recovery.unwrap().author.parent,
                f.parent
            );
            let same: AssignmentSnapshot = serde_json::from_value(tool_as(
                &f,
                target.session,
                "ovrcr_accept",
                accept_args(&child, &published),
            ))
            .unwrap();
            assert_eq!(same.acceptance_ref, Some(reference));
        } else {
            refusal_as(
                &f,
                target.session,
                "ovrcr_accept",
                accept_args(&child, &published),
            );
            let fresh: FallbackCapture = serde_json::from_value(tool_as(
                &f,
                target.session,
                "ovrcr_capture_fallback",
                capture_args(&child),
            ))
            .unwrap();
            assert_eq!(fresh.authority.parent, target.session);
            assert_ne!(fresh.authority, captured.authority);
            let mut new_draft = parent_draft(&f, &child, &fresh);
            new_draft.version = 2;
            let recovery = tool_as(
                &f,
                target.session,
                "ovrcr_recover_fallback",
                recover_args(&child, &new_draft),
            );
            let accepted: AssignmentSnapshot = serde_json::from_value(tool_as(
                &f,
                target.session,
                "ovrcr_accept",
                accept_args(&child, &recovery),
            ))
            .unwrap();
            assert_eq!(accepted.recoveries.len(), 2);
            assert_eq!(
                accepted.acceptance_ref.unwrap().target,
                AcceptanceTarget::ParentRecovery {
                    recovery_version: 2,
                    digest: recovery["digest"].as_str().unwrap().into()
                }
            );
        }
        assert_eq!(f.byte_count(&child), before);
    }
}

#[cfg(feature = "acceptance-diagnostics")]
fn restart_owned_server(f: &RecoveryFixture) {
    let groups = f.live.session_groups();
    for group in &groups {
        f.live.own_group(*group);
    }
    let server = f.live.server_pid().unwrap();
    assert_eq!(
        f.live.request(Request::Shutdown { kill: true }),
        Response::Ok
    );
    f.live.join();
    assert_eq!(unsafe { libc::kill(server as i32, 0) }, -1);
    for group in groups {
        assert!(live::wait_group_absent(group, Duration::from_secs(5)));
    }
    let root = f.live.root.path();
    let host = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pi/pi_host.mjs");
    let history = root.join("history");
    let gate = root.join("allow-result");
    let fault = root.join("fault");
    f.live.start_binary_env(&[
        ("OVRCR_TEST_DELEGATION", "1".as_ref()),
        ("OVRCR_TEST_PI_HOST", host.as_os_str()),
        (
            "OVRCR_TEST_CONTROL_BINARY",
            env!("CARGO_BIN_EXE_ovrcr").as_ref(),
        ),
        ("OVRCR_TEST_DELEGATION_RESULT_GATE", gate.as_os_str()),
        (
            "OVRCR_TEST_DELEGATION_CONTROL_SOCKET",
            f.proxy.path.as_os_str(),
        ),
        ("OVRCR_TEST_DELEGATION_FAULT", fault.as_os_str()),
        ("OVRCR_TEST_DELEGATION_BYTES", f.bytes.as_os_str()),
        ("OVRCR_TEST_HISTORY", history.as_os_str()),
    ]);
    assert_ne!(f.live.server_pid(), Some(server));
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_restart_keeps_uncommitted_recovery_or_decision_as_evidence_without_adoption() {
    for fail_decision in [false, true] {
        let f = OwnedFollowup::new();
        let child = start(&f, "FALLBACK_COMPLETE_MISSING");
        let captured = capture(&f, &child);
        let draft = parent_draft(&f, &child, &captured);
        let fault = f.live.root.path().join("fault");
        let recovery = if fail_decision {
            let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
            fs::write(
                &fault,
                format!("fallback-acceptance-before-save|{}|0", child.handle),
            )
            .unwrap();
            let reply = f.tool("ovrcr_accept", accept_args(&child, &published));
            assert_ne!(reply["acceptance"], "accepted");
            published
        } else {
            fs::write(
                &fault,
                format!("fallback-recovery-before-save|{}|0", child.handle),
            )
            .unwrap();
            refusal_as(
                &f,
                f.parent,
                "ovrcr_recover_fallback",
                recover_args(&child, &draft),
            );
            let path = child
                .brief
                .parent()
                .unwrap()
                .join("parent-fallback-r1-v1.json");
            serde_json::json!({"path":path,"digest":file_digest(&path),"recovery":{"version":1}})
        };
        let recovery_path = Path::new(recovery["path"].as_str().unwrap());
        let bytes = fs::read(recovery_path).unwrap();
        let decision_path = child.brief.parent().unwrap().join("acceptance-v1.json");
        let decision = fs::read(&decision_path).ok();
        let old_authority = captured.authority.clone();
        restart_owned_server(&f);
        assert_eq!(fs::read(recovery_path).unwrap(), bytes);
        assert_eq!(fs::read(&decision_path).ok(), decision);
        let target = new_parent(&f, "fallback-after-restart");
        let mut dashboard = connect_server(&f.live.socket).unwrap();
        assert!(matches!(
            dashboard_view(&mut dashboard, 1, Request::DashboardHello).unwrap(),
            Response::Hierarchy(_)
        ));
        let Response::Workflows(rows) = dashboard_view(
            &mut dashboard,
            2,
            Request::DashboardWorkflow(DashboardWorkflowRequest::Transfer(Box::new(
                WorkflowTransfer {
                    request: "recover-fallback-after-server-loss".into(),
                    expected: old_authority.clone(),
                    parent: target.session,
                    parent_run: target.run,
                    binding: target.binding,
                    producer: target.producer,
                },
            ))),
        )
        .unwrap() else {
            panic!("explicit transfer after Server loss")
        };
        assert_ne!(
            rows[0].authority.server_lifetime,
            old_authority.server_lifetime
        );
        tool_as(
            &f,
            target.session,
            "ovrcr_recover",
            serde_json::json!({"workflow":child.identity.workflow}),
        );
        let current: AssignmentSnapshot = serde_json::from_value(tool_as(
            &f,
            target.session,
            "ovrcr_inspect",
            serde_json::json!({"handle":child.handle}),
        ))
        .unwrap();
        assert!(current.acceptance_ref.is_none());
        assert!(current.accepted_recovery.is_none());
        assert_eq!(current.recoveries.len(), usize::from(fail_decision));
        assert_eq!(current.identity, child.identity);
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
        refusal_as(
            &f,
            target.session,
            "ovrcr_accept",
            accept_args(&child, &recovery),
        );
        assert!(stored_record(&f, &child.handle)["acceptance"].is_null());
        assert_eq!(fs::read(recovery_path).unwrap(), bytes);
        assert_eq!(fs::read(&decision_path).ok(), decision);
    }
}

#[test]
fn public_cancelled_workflow_requires_interrupt_reconciliation_but_allows_metadata_only_recovery() {
    let f = super::observation::Fixture::new(10000);
    let child:AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_start", serde_json::json!({
        "project":PROJECT,"workspace":f.workspace,"scope":"FALLBACK_COMPLETE_MISSING FALLBACK_INTERRUPT_RUNNING INTERRUPT_SETTLEMENT_GATE: return the complete README verbatim",
        "model_provider":"test","model":"fixture-model","acceptance_evidence":["Complete README content"]
    }))).unwrap();
    wait_fallback_screen(&f.live, &child);
    f.until(|| {
        f.summary(child.identity.session.unwrap())
            .agent
            .as_ref()
            .is_some_and(|a| {
                a.native_control
                    .as_ref()
                    .is_some_and(|n| n.interrupt == NativeInterruptCapability::Escape && !n.settled)
            })
    });
    let cancelled:WorkflowSnapshot = serde_json::from_value(f.tool("ovrcr_cancel_workflow", serde_json::json!({"workflow":child.identity.workflow,"request":"cancel-fallback-metadata"}))).unwrap();
    let cancellation = cancelled.cancellation.unwrap();
    assert_eq!(cancellation.dispatch_closure, DispatchOutcome::Applied);
    let interrupted_request = cancellation
        .targets
        .iter()
        .find(|t| t.handle == child.handle)
        .unwrap()
        .interrupt_request
        .clone();
    wait_file(&child.brief.with_extension("json.interrupt-consumed"));
    let blocked: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_inspect", serde_json::json!({"handle":child.handle})))
            .unwrap();
    assert!(matches!(
        blocked.control.barrier,
        Some(ControlBarrier::Interrupt { .. })
    ));
    let rejection = tool_reply(
        &f.live,
        f.parent,
        "ovrcr_capture_fallback",
        capture_args(&blocked),
        &f.live
            .root
            .path()
            .join("cancelled-capture-before-confirm.json"),
    );
    assert!(rejection.get("error").is_some());
    fs::write(
        child.brief.with_extension("json.interrupt-settle"),
        "explicit native settlement gate",
    )
    .unwrap();
    wait_file(&child.brief.with_extension("json.interrupt-settled"));
    let settled: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_inspect", serde_json::json!({"handle":child.handle})))
            .unwrap();
    let confirmed:AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_confirm_interrupt", serde_json::json!({
        "handle":child.handle,"request":"confirm-cancel-fallback","interrupted_request":interrupted_request,
        "expected_control_epoch":settled.control.epoch,"evidence":settled.control_evidence
    }))).unwrap();
    assert!(confirmed.control.barrier.is_none());
    let before = fs::read_to_string(
        f.live
            .root
            .path()
            .join("input-bytes")
            .join(child.identity.session.unwrap().0.to_string()),
    )
    .unwrap();
    let captured: FallbackCapture =
        serde_json::from_value(f.tool("ovrcr_capture_fallback", capture_args(&confirmed))).unwrap();
    let draft = parent_draft_in(&f.live.repo, &confirmed, &captured);
    let published = f.tool("ovrcr_recover_fallback", recover_args(&confirmed, &draft));
    let accepted: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_accept", accept_args(&confirmed, &published)))
            .unwrap();
    assert_eq!(accepted.acceptance, "accepted");
    assert_eq!(accepted.receipts, confirmed.receipts);
    assert_eq!(accepted.execution_attempts, Some(1));
    let followup = f.tool("ovrcr_followup", serde_json::json!({"handle":child.handle,"expected_identity":child.identity,"expected_result_version":1,
        "expected_control_epoch":confirmed.control.epoch,"text":"A cancelled workflow must refuse this repair dispatch"}));
    assert_eq!(followup["dispatch"], "Refused");
    assert_eq!(
        fs::read_to_string(
            f.live
                .root
                .path()
                .join("input-bytes")
                .join(child.identity.session.unwrap().0.to_string())
        )
        .unwrap(),
        before
    );
    let root = f.live.root.path().to_owned();
    let server = f.live.server_pid().unwrap();
    let groups = f.live.session_groups();
    for group in &groups {
        f.live.own_group(*group);
    }
    drop(f);
    assert!(!root.exists());
    assert_eq!(unsafe { libc::kill(server as i32, 0) }, -1);
    for group in &groups {
        assert!(live::wait_group_absent(*group, Duration::from_secs(5)));
    }
    eprintln!(
        "FOLLOWUP_CLEANUP root={} server={} groups={groups:?} groups_absent=true server_absent=true root_removed=true",
        root.display(),
        server
    );
}

#[test]
fn public_changed_contract_archives_exact_recovery_without_inheriting_selection() {
    let f = OwnedFollowup::new();
    let child = start(&f, "FALLBACK_COMPLETE_MISSING");
    let captured = capture(&f, &child);
    let draft = parent_draft(&f, &child, &captured);
    let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
    let accepted = accept_recovery(&f, &child, &published);
    let old_contract = fs::read(child.brief.parent().unwrap().join("contract-v1.json")).unwrap();
    let old_brief = fs::read(&child.brief).unwrap();
    let decision_path = child.brief.parent().unwrap().join("acceptance-v1.json");
    let old_decision = fs::read(&decision_path).unwrap();
    let mut revised: ResearchContract = serde_json::from_slice(&old_contract).unwrap();
    revised
        .scope
        .push_str(" Also independently verify the complete README for the changed contract.");
    let next: AssignmentSnapshot =
        serde_json::from_value(super::followups::followup(&f, &accepted, Some(revised))).unwrap();
    wait_fallback_screen(&f.live, &next);
    assert_eq!(next.identity.contract, 2);
    assert_eq!(next.result_version, 2);
    assert!(next.recoveries.is_empty());
    assert!(next.acceptance_ref.is_none());
    assert!(next.accepted_recovery.is_none());
    assert_eq!(
        fs::read(child.brief.parent().unwrap().join("contract-v1.json")).unwrap(),
        old_contract
    );
    assert_eq!(fs::read(&child.brief).unwrap(), old_brief);
    assert_eq!(fs::read(decision_path).unwrap(), old_decision);
    let prior: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":child.handle,"version":1}),
    ))
    .unwrap();
    assert_eq!(prior.selected_acceptance_ref, accepted.acceptance_ref);
    assert_eq!(prior.selected_recoveries, accepted.recoveries);
    assert_eq!(
        prior.accepted_recovery.unwrap().capture.context.identity,
        child.identity
    );
    refusal_as(
        &f,
        f.parent,
        "ovrcr_accept",
        accept_args(&child, &published),
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_partial_or_unknown_scoped_input_is_never_cleared_by_fallback_evidence() {
    for unknown in [false, true] {
        let f = OwnedFollowup::new();
        let child = start(&f, "FALLBACK_COMPLETE_MISSING");
        f.proxy.arm(ControlRule {
            operation: "Followup",
            loss: 2,
            fault: Some(if unknown {
                ("input-after-effect", 0)
            } else {
                ("input", 7)
            }),
        });
        let current: AssignmentSnapshot =
            serde_json::from_value(super::followups::followup(&f, &child, None)).unwrap();
        let receipt = current
            .receipts
            .iter()
            .rev()
            .find(|r| r.operation == DispatchOperation::Followup)
            .unwrap()
            .clone();
        assert_eq!(
            receipt.outcome,
            if unknown {
                DispatchOutcome::Unknown
            } else {
                DispatchOutcome::Partial
            }
        );
        if unknown {
            wait_fallback_screen(&f.live, &current);
        }
        let captured = capture(&f, &current);
        let draft = parent_draft(&f, &current, &captured);
        if unknown {
            let published = f.tool("ovrcr_recover_fallback", recover_args(&current, &draft));
            let accepted = accept_recovery(&f, &current, &published);
            assert_eq!(
                accepted.receipts, current.receipts,
                "explicit evidence judgment does not resolve uncertain input transport"
            );
        } else {
            refusal_as(
                &f,
                f.parent,
                "ovrcr_recover_fallback",
                recover_args(&current, &draft),
            );
            assert!(f.reconcile(&child.handle).acceptance_ref.is_none());
        }
        let before = f.byte_count(&current);
        let blocked = super::followups::followup(&f, &current, None);
        assert_eq!(blocked["dispatch"], "Refused");
        let after = f.reconcile(&child.handle);
        assert_eq!(
            after.receipts.iter().find(|r| r.request == receipt.request),
            Some(&receipt)
        );
        assert_eq!(f.byte_count(&current), before);
        assert_eq!(after.execution_attempts, Some(1));
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_recovery_preserves_unanswered_question_and_accepted_answer_without_input() {
    for accepted_answer in [false, true] {
        let f = OwnedFollowup::new();
        let child = start(&f, "FALLBACK_WITH_CURRENT_QUESTION");
        wait_file(&child.brief.with_extension("json.question-published"));
        assert_eq!(child.questions.len(), 1);
        let question = child.questions[0].reference.clone();
        let question_path = child.brief.parent().unwrap().join(format!(
            "question-r{}-{}-v{}.json",
            question.result_version, question.question_id, question.question_version
        ));
        let question_bytes = fs::read(&question_path).unwrap();
        let body: AssignmentQuestion = serde_json::from_slice(&question_bytes).unwrap();
        assert_eq!(
            body.kind,
            QuestionKind::Clarification,
            "This independently fulfilled README scope requires no human decision; a scope-blocking HumanDecision must remain pending"
        );
        let answer_args = serde_json::json!({"handle":child.handle,"question":question,
            "expected_control_epoch":child.control.epoch,"basis":"ExistingAuthority",
            "scope":"Read only the already assigned complete README.",
            "answer":"ANSWER_BODY_FALLBACK_PRIVATE: The existing README filename is the citation label."});
        let original = fs::read(&child.result_path).ok();
        let before = f.byte_count(&child);
        let current: AssignmentSnapshot = if accepted_answer {
            f.proxy.arm(ControlRule {
                operation: "AnswerQuestion",
                loss: 2,
                fault: Some(("answer-before-input", 0)),
            });
            serde_json::from_value(f.tool("ovrcr_answer_question", answer_args.clone())).unwrap()
        } else {
            child.clone()
        };
        let answer_file = current.questions[0].answer.as_ref().map(|receipt| {
            assert_eq!(receipt.outcome, DispatchOutcome::Accepted);
            let path = child
                .brief
                .parent()
                .unwrap()
                .join(format!("answer-{}.json", receipt.request));
            let bytes = fs::read(&path).unwrap();
            let answer: QuestionAnswer = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(answer.question, question);
            assert_eq!(answer.request, receipt.request);
            (path, bytes)
        });
        assert_eq!(answer_file.is_some(), accepted_answer);
        let assert_preserved = |snapshot: &AssignmentSnapshot| {
            assert_eq!(snapshot.questions, current.questions);
            assert_eq!(snapshot.receipts, current.receipts);
            assert_eq!(snapshot.control, current.control);
            assert_eq!(snapshot.identity, current.identity);
            assert_eq!(snapshot.execution_attempts, Some(1));
            assert!(snapshot.submission.is_none());
            assert!(snapshot.publication.is_none());
            assert_eq!(fs::read(&question_path).unwrap(), question_bytes);
            assert_eq!(fs::read(&child.result_path).ok(), original);
            if let Some((path, bytes)) = &answer_file {
                assert_eq!(&fs::read(path).unwrap(), bytes);
                assert_eq!(
                    snapshot.questions[0].answer.as_ref().unwrap().outcome,
                    DispatchOutcome::Accepted,
                    "recovery cannot manufacture Applied delivery"
                );
            } else {
                assert!(snapshot.questions[0].answer.is_none());
            }
            assert_eq!(f.byte_count(&child), before);
        };
        let captured = capture(&f, &current);
        assert_preserved(&f.reconcile(&child.handle));
        let draft = parent_draft(&f, &current, &captured);
        let published = f.tool("ovrcr_recover_fallback", recover_args(&current, &draft));
        let recovered = f.reconcile(&child.handle);
        assert_preserved(&recovered);
        assert_eq!(recovered.recoveries.len(), 1);
        assert!(
            recovered.acceptance_ref.is_none(),
            "publication is not acceptance or human approval"
        );
        let collected: AssignmentSnapshot = serde_json::from_value(
            f.tool("ovrcr_collect", serde_json::json!({"handle":child.handle})),
        )
        .unwrap();
        assert_preserved(&collected);
        let question_collected: AssignmentSnapshot = serde_json::from_value(f.tool(
            "ovrcr_collect_question",
            serde_json::json!({"handle":child.handle,"reference":question}),
        ))
        .unwrap();
        assert_preserved(&question_collected);
        assert_eq!(question_collected.collected_question, Some(body));
        // The full verbatim README is independently verified; this selects only
        // that result and does not answer a question or grant human authority.
        let accepted = accept_recovery(&f, &current, &published);
        assert_preserved(&accepted);
        assert!(matches!(
            accepted.acceptance_ref.as_ref().unwrap().target,
            AcceptanceTarget::ParentRecovery { .. }
        ));
        let observed: DelegationObservationBatch = serde_json::from_value(f.tool(
            "ovrcr_wait",
            serde_json::json!({"handles":[child.handle],"wait_ms":0}),
        ))
        .unwrap();
        assert_eq!(observed.observations.len(), 1);
        assert_eq!(observed.observations[0].questions, current.questions);
        assert_eq!(observed.observations[0].control, current.control);
        assert!(observed.observations[0].publication.is_none());
        if accepted_answer {
            assert_eq!(
                f.tool("ovrcr_answer_question", answer_args)["dispatch"],
                "Refused"
            );
            assert_eq!(
                super::followups::followup(&f, &accepted, None)["dispatch"],
                "Refused"
            );
        }
        assert_preserved(&f.reconcile(&child.handle));
        let events =
            serde_json::to_string(&f.live.request(Request::Events { follow: false })).unwrap();
        for payload in [
            "QUESTION_BODY_FALLBACK_PRIVATE",
            "ANSWER_BODY_FALLBACK_PRIVATE",
        ] {
            assert!(
                !events.contains(payload),
                "fallback question/answer payload entered Server Events"
            );
        }
    }
}

fn coding_arguments(workspace: &str, scope: &str) -> serde_json::Value {
    serde_json::json!({
        "project":PROJECT,"workspace":workspace,"scope":scope,
        "model_provider":"test","model":"fixture-model",
        "acceptance_evidence":["Complete README content"],
        "permitted_actions":["read","write","edit","check"],
        "coding":{"requested_base":"main","branch":format!("coding/{workspace}"),
            "authorized_paths":["README","change.txt"],
            "required_checks":["fixture-failure","required-but-missing","deliberately-unrun"]}
    })
}

#[test]
fn public_coding_parent_recovery_cannot_fabricate_candidate_or_acceptance() {
    for (mode, workspace) in [
        ("FALLBACK_COMPLETE_MISSING", "fallback-code-missing"),
        ("FALLBACK_COMPLETE_INVALID", "fallback-code-invalid"),
    ] {
        let f = OwnedFollowup::new();
        let started: AssignmentSnapshot = serde_json::from_value(f.tool(
            "ovrcr_start",
            coding_arguments(
                workspace,
                &format!("{mode}: return the complete README verbatim."),
            ),
        ))
        .unwrap();
        assert_eq!(started.dispatch, DispatchStatus::Applied);
        wait_fallback_screen(&f.live, &started);
        for group in f.live.session_groups() {
            f.live.own_group(group);
        }
        let child = f.reconcile(&started.handle);
        assert!(child.resources.coding.is_some());
        let contract: ResearchContract = serde_json::from_slice(
            &fs::read(child.brief.parent().unwrap().join("contract-v1.json")).unwrap(),
        )
        .unwrap();
        assert!(contract.coding.is_some());
        let original = fs::read(&child.result_path).ok();
        let before = f.byte_count(&child);
        let captured = capture(&f, &child);
        let draft = parent_draft(&f, &child, &captured);
        let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
        let frozen = fs::read(published["path"].as_str().unwrap()).unwrap();
        let registered = f.reconcile(&child.handle);
        let refused = f.tool("ovrcr_accept", accept_args(&registered, &published));
        assert_eq!(refused["dispatch"], "Refused", "{refused}");
        let after = f.reconcile(&child.handle);
        assert_eq!(after.acceptance, "pending");
        assert!(after.acceptance_ref.is_none());
        assert!(after.accepted_recovery.is_none());
        assert!(after.submission.is_none() && after.publication.is_none());
        assert_eq!(after.identity, child.identity);
        assert_eq!(after.control, child.control);
        assert_eq!(after.resources, child.resources);
        assert_eq!(after.receipts, child.receipts);
        assert_eq!(after.execution_attempts, Some(1));
        assert_eq!(after.recoveries, registered.recoveries);
        assert_eq!(
            after.obligation_revision, registered.obligation_revision,
            "a refused coding selection must not commit new evidence or a decision"
        );
        assert_eq!(fs::read(&child.result_path).ok(), original);
        assert_eq!(
            fs::read(published["path"].as_str().unwrap()).unwrap(),
            frozen
        );
        let root = child.brief.parent().unwrap();
        for name in ["candidate-v1.json", "acceptance-v1.json"] {
            assert!(
                !root.join(name).exists(),
                "parent prose cannot manufacture {name}"
            );
        }
        assert_eq!(f.byte_count(&child), before);
        assert_eq!(f.child_count(), 1);
        let selected: AssignmentSnapshot = serde_json::from_value(f.tool(
            "ovrcr_collect",
            serde_json::json!({"handle":child.handle,"version":child.result_version}),
        ))
        .unwrap();
        assert!(selected.selected_acceptance_ref.is_none());
        assert!(selected.accepted_recovery.is_none());
        assert_eq!(
            selected.selected_recoveries, registered.recoveries,
            "unsupported coding selection retains inspectable evidence"
        );
    }
}

fn settled_coding_result(f: &RecoveryFixture, child: &AssignmentSnapshot) -> AssignmentSnapshot {
    let completed = super::followups::complete(f, child);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let current = f.reconcile(&child.handle);
        assert_eq!(current.identity, completed.identity);
        assert_eq!(current.result_version, completed.result_version);
        if current.activity == AgentActivity::ResponseReady {
            return serde_json::from_value(
                f.tool("ovrcr_collect", serde_json::json!({"handle":child.handle})),
            )
            .unwrap();
        }
        assert!(Instant::now() < deadline, "coding result did not settle");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn public_ordinary_coding_acceptance_keeps_selected_candidate_and_named_checks() {
    let f = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_start",
        coding_arguments(
            "fallback-ordinary-code",
            "Implement the bounded fixture change.",
        ),
    ))
    .unwrap();
    let first = settled_coding_result(&f, &started);
    let result = first.submission.as_ref().unwrap();
    let coding = result.coding.as_ref().unwrap();
    for (name, outcome) in [
        ("fixture-failure", CheckOutcome::Failed),
        ("required-but-missing", CheckOutcome::Missing),
        ("deliberately-unrun", CheckOutcome::Unrun),
    ] {
        assert_eq!(
            coding
                .checks
                .iter()
                .find(|check| check.name == name)
                .unwrap()
                .outcome,
            outcome
        );
    }
    let candidate = coding.candidate.as_ref().unwrap();
    let candidate_bytes = fs::read(&candidate.path).unwrap();
    let result_bytes = fs::read(&first.result_path).unwrap();
    let accepted: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_accept", serde_json::json!({
        "handle":first.handle,"identity":first.identity,"result_version":first.result_version,
        "expected_control_epoch":first.control.epoch,
        "target":{"ChildSubmission":{"digest":first.publication.as_ref().unwrap().identity.digest}}
    }))).unwrap();
    assert_eq!(accepted.acceptance, "accepted");
    assert!(matches!(
        accepted.acceptance_ref.as_ref().unwrap().target,
        AcceptanceTarget::ChildSubmission { .. }
    ));
    assert!(accepted.accepted_recovery.is_none());
    let decision = accepted.acceptance_ref.as_ref().unwrap();
    let decision_bytes = fs::read(&decision.path).unwrap();
    let next: AssignmentSnapshot =
        serde_json::from_value(super::followups::followup(&f, &accepted, None)).unwrap();
    let next = settled_coding_result(&f, &next);
    assert_eq!(next.identity, first.identity);
    assert_eq!(next.result_version, first.result_version + 1);
    let before = f.byte_count(&next);
    let args = serde_json::json!({"handle":next.handle,"version":first.result_version});
    let selected: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", args.clone())).unwrap();
    assert_eq!(selected.historical_submission, first.submission);
    assert_eq!(selected.selected_acceptance_ref, accepted.acceptance_ref);
    // A newer valid candidate cannot authenticate a corrupted selected candidate.
    fs::write(&candidate.path, b"{changed selected coding candidate").unwrap();
    let refused = tool_reply(
        &f.live,
        f.parent,
        "ovrcr_collect",
        args.clone(),
        &f.live
            .root
            .path()
            .join("fallback-selected-candidate-refused.json"),
    );
    fs::write(&candidate.path, &candidate_bytes).unwrap();
    assert!(refused["error"].is_string(), "{refused}");
    let current: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", serde_json::json!({"handle":next.handle})))
            .unwrap();
    assert_eq!(current.submission, next.submission);
    let restored: AssignmentSnapshot =
        serde_json::from_value(f.tool("ovrcr_collect", args)).unwrap();
    assert_eq!(restored.historical_submission, first.submission);
    assert_eq!(restored.selected_acceptance_ref, accepted.acceptance_ref);
    assert_eq!(fs::read(&candidate.path).unwrap(), candidate_bytes);
    assert_eq!(fs::read(&first.result_path).unwrap(), result_bytes);
    assert_eq!(fs::read(&decision.path).unwrap(), decision_bytes);
    assert_eq!(f.byte_count(&next), before);
    assert_eq!(f.child_count(), 1);
}

fn recovery_delivery(
    f: &RecoveryFixture,
    child: &AssignmentSnapshot,
    request: &str,
    previous: Option<DeliveryRef>,
) -> (DeliveryDeclaration, DeliveryRef) {
    let body = DeliveryDeclaration {
        request: request.into(),
        workflow: child.identity.workflow.clone(),
        scope: "Deliver the independently verified complete README".into(),
        provenance: "Parent retention decision against exact current evidence".into(),
        resources: vec![DeliveryResource {
            handle: child.handle.clone(),
            identity: child.identity.clone(),
            result_version: child.result_version,
            obligation_revision: child.obligation_revision,
            publication: child.publication.as_ref().map(|p| p.identity.clone()),
            acceptance: child.acceptance_ref.clone(),
            evidence: vec![],
            completed_obligations: vec!["Independently verify the README bytes".into()],
            retention: RetentionChoice::ArchiveEligible,
            previous,
        }],
    };
    let workflow: WorkflowSnapshot =
        serde_json::from_value(f.tool("ovrcr_deliver", serde_json::json!({"declaration":body})))
            .unwrap();
    let reference = workflow
        .deliveries
        .iter()
        .find(|d| d.reference.request == request)
        .unwrap()
        .reference
        .clone();
    assert_eq!(
        workflow
            .deliveries
            .iter()
            .find(|d| d.reference == reference)
            .unwrap()
            .declaration,
        body
    );
    (body, reference)
}

fn stale_recovery_delivery_cannot_stop(
    f: &RecoveryFixture,
    old: &AssignmentSnapshot,
    new: &AssignmentSnapshot,
    delivery: &DeliveryRef,
    request: &str,
) {
    let bytes = f.byte_count(new);
    let result: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_archive", serde_json::json!({
        "request":request,"handle":old.handle,"expected_identity":old.identity,
        "expected_result_version":old.result_version,"expected_obligation_revision":old.obligation_revision,
        "expected_control_epoch":old.control.epoch,"delivery":delivery,"continuation_of":null
    }))).unwrap();
    let receipt = result
        .archive_receipts
        .iter()
        .find(|r| r.intent.request.request == request)
        .unwrap();
    assert_eq!(
        receipt.outcome,
        ArchiveOutcome::Refused {
            reason: ArchiveRefusal::ObligationChanged
        }
    );
    assert_eq!(result.identity, new.identity);
    assert_eq!(result.control, new.control);
    assert_eq!(result.obligation_revision, new.obligation_revision);
    assert_eq!(result.receipts, new.receipts);
    assert_eq!(result.acceptance_ref, new.acceptance_ref);
    let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
        panic!("inventory");
    };
    let row = sessions
        .iter()
        .find(|row| Some(row.id) == new.identity.session)
        .unwrap();
    assert!(
        !row.archived && row.phase.is_live(),
        "a stale declaration must not stop its owned run"
    );
    assert_eq!(Some(row.run), new.identity.run);
    assert_eq!(f.byte_count(new), bytes);
}

#[test]
fn public_recovery_delivery_uses_typed_acceptance_and_new_evidence_stales_retention() {
    for mode in ["FALLBACK_COMPLETE_MISSING", "FALLBACK_COMPLETE_INVALID"] {
        let f = OwnedFollowup::new();
        let child = start(&f, mode);
        let original = fs::read(&child.result_path).ok();
        let bytes = f.byte_count(&child);
        let (_, initial_delivery) = recovery_delivery(&f, &child, "before-capture", None);
        let captured = capture(&f, &child);
        let after_capture = f.reconcile(&child.handle);
        assert_eq!(
            after_capture.obligation_revision,
            child.obligation_revision + 1
        );
        stale_recovery_delivery_cannot_stop(
            &f,
            &child,
            &after_capture,
            &initial_delivery,
            "stale-capture",
        );
        let (_, captured_delivery) = recovery_delivery(
            &f,
            &after_capture,
            "before-recovery",
            Some(initial_delivery),
        );
        let draft = parent_draft(&f, &child, &captured);
        let published = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
        let recovery_bytes = fs::read(published["path"].as_str().unwrap()).unwrap();
        let recovered = f.reconcile(&child.handle);
        assert_eq!(
            recovered.obligation_revision,
            after_capture.obligation_revision + 1
        );
        stale_recovery_delivery_cannot_stop(
            &f,
            &after_capture,
            &recovered,
            &captured_delivery,
            "stale-recovery",
        );
        let repeated = f.tool("ovrcr_recover_fallback", recover_args(&child, &draft));
        assert_eq!(
            repeated, published,
            "a retry returns the identical immutable recovery publication"
        );
        assert_eq!(
            f.reconcile(&child.handle).obligation_revision,
            recovered.obligation_revision
        );
        let (_, recovered_delivery) =
            recovery_delivery(&f, &recovered, "before-selection", Some(captured_delivery));
        let accepted = accept_recovery(&f, &recovered, &published);
        assert_eq!(
            accepted.obligation_revision,
            recovered.obligation_revision + 1
        );
        assert!(accepted.submission.is_none() && accepted.publication.is_none());
        let reference = accepted.acceptance_ref.as_ref().unwrap();
        assert_eq!(
            reference.target,
            AcceptanceTarget::ParentRecovery {
                recovery_version: 1,
                digest: published["digest"].as_str().unwrap().into()
            }
        );
        let decision_bytes = fs::read(&reference.path).unwrap();
        stale_recovery_delivery_cannot_stop(
            &f,
            &recovered,
            &accepted,
            &recovered_delivery,
            "stale-selection",
        );
        let retry = accept_recovery(&f, &accepted, &published);
        assert_eq!(retry.acceptance_ref, accepted.acceptance_ref);
        assert_eq!(retry.obligation_revision, accepted.obligation_revision);
        assert_eq!(fs::read(&reference.path).unwrap(), decision_bytes);
        let (body, final_delivery) =
            recovery_delivery(&f, &retry, "accepted-recovery", Some(recovered_delivery));
        assert!(body.resources[0].publication.is_none());
        assert_eq!(body.resources[0].acceptance, accepted.acceptance_ref);
        let replay: WorkflowSnapshot = serde_json::from_value(
            f.tool("ovrcr_deliver", serde_json::json!({"declaration":body})),
        )
        .unwrap();
        assert_eq!(
            replay
                .deliveries
                .iter()
                .find(|d| d.reference == final_delivery)
                .unwrap()
                .declaration,
            body
        );
        assert_eq!(
            f.reconcile(&child.handle).obligation_revision,
            accepted.obligation_revision
        );
        assert_eq!(
            f.byte_count(&child),
            bytes,
            "capture, recovery, selection and delivery do not dispatch input"
        );
        let archived: AssignmentSnapshot = serde_json::from_value(f.tool("ovrcr_archive", serde_json::json!({
            "request":"archive-selected-recovery","handle":accepted.handle,"expected_identity":accepted.identity,
            "expected_result_version":accepted.result_version,"expected_obligation_revision":accepted.obligation_revision,
            "expected_control_epoch":accepted.control.epoch,"delivery":final_delivery,"continuation_of":null
        }))).unwrap();
        assert_eq!(
            archived
                .archive_receipts
                .iter()
                .find(|r| r.intent.request.request == "archive-selected-recovery")
                .unwrap()
                .outcome,
            ArchiveOutcome::Archived {
                previous_run: child.identity.run.unwrap(),
                archived_run: SessionRunId(child.identity.run.unwrap().0 + 1)
            }
        );
        assert_eq!(fs::read(&child.result_path).ok(), original);
        assert_eq!(
            fs::read(published["path"].as_str().unwrap()).unwrap(),
            recovery_bytes
        );
        assert_eq!(fs::read(&reference.path).unwrap(), decision_bytes);
        assert_eq!(archived.acceptance_ref, accepted.acceptance_ref);
        assert_eq!(archived.recoveries, accepted.recoveries);
    }
}

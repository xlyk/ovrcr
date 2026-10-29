//! Registered tools, public control CLI, binary Server, real managed PTY.
use super::followups::{OwnedFollowup, complete};
use super::*;
use std::os::unix::net::UnixStream;

fn current(fixture: &RecoveryFixture, child: SessionId) -> SessionSummary {
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory");
    };
    sessions
        .into_iter()
        .find(|summary| summary.id == child)
        .unwrap()
}

fn view(stream: &mut UnixStream, child: SessionId, run: SessionRunId, revision: u64) {
    assert_eq!(
        dashboard_view(
            stream,
            revision + 10,
            Request::SetView {
                view: DashboardView {
                    revision,
                    panes: vec![PaneTarget {
                        session: child,
                        run,
                        size: TerminalSize {
                            rows: 24,
                            cols: 120
                        }
                    }],
                    focused: Some(child),
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
}

pub(super) fn dashboard(fixture: &RecoveryFixture, assignment: &AssignmentSnapshot) -> UnixStream {
    let mut stream = connect_server(&fixture.live.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert!(matches!(
        client::request(&mut stream, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    view(
        &mut stream,
        assignment.identity.session.unwrap(),
        assignment.identity.run.unwrap(),
        1,
    );
    stream
}

pub(super) fn transition(
    fixture: &RecoveryFixture,
    stream: &mut UnixStream,
    child: SessionId,
    name: &str,
    take: bool,
    revision: u64,
) -> AssignmentSnapshot {
    let summary = current(fixture, child);
    let projected = summary.assignment_control.unwrap();
    let request = if take {
        AssignmentControlRequest::Takeover {
            request: name.into(),
            session: child,
            run: summary.run,
            expected_epoch: projected.control.epoch,
            view_revision: revision,
        }
    } else {
        AssignmentControlRequest::Return {
            request: name.into(),
            session: child,
            run: summary.run,
            expected_epoch: projected.control.epoch,
            view_revision: revision,
            evidence: Box::new(projected.evidence.unwrap()),
        }
    };
    assert_eq!(
        dashboard_view(
            stream,
            100 + projected.control.epoch,
            Request::AssignmentControl(request.clone())
        )
        .unwrap(),
        Response::Ok
    );
    let after = fixture.reconcile(&projected.handle);
    assert_eq!(
        dashboard_view(
            stream,
            200 + projected.control.epoch,
            Request::AssignmentControl(request)
        )
        .unwrap(),
        Response::Ok,
        "lost transition reply is idempotent"
    );
    assert_eq!(fixture.reconcile(&projected.handle).control, after.control);
    after
}

pub(super) fn confirm(
    fixture: &RecoveryFixture,
    snapshot: &AssignmentSnapshot,
    returned: &str,
    request: &str,
) -> AssignmentSnapshot {
    let reply = fixture.tool(
        "ovrcr_confirm_return",
        serde_json::json!({
            "handle":snapshot.handle,"request":request,"returned_request":returned,
            "expected_control_epoch":snapshot.control.epoch,"evidence":snapshot.control_evidence,
        }),
    );
    serde_json::from_value(reply.clone()).unwrap_or_else(|error| {
        let current = fixture.reconcile(&snapshot.handle);
        panic!("confirmation {request} after {returned}: {error}; reply={reply}; reconciled={:?}; current={:?}", snapshot.control_evidence, current.control_evidence)
    })
}

fn settled(fixture: &RecoveryFixture, snapshot: &AssignmentSnapshot) -> AssignmentSnapshot {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let current = fixture.reconcile(&snapshot.handle);
        assert_eq!(current.identity, snapshot.identity);
        assert_eq!(current.control, snapshot.control);
        if current.activity == AgentActivity::ResponseReady {
            return current;
        }
        assert!(
            Instant::now() < deadline,
            "child did not settle: {current:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn read_marker(fixture: &RecoveryFixture, child: SessionId, marker: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Response::TerminalText { text, .. } = fixture.live.request(Request::ReadTerminal {
            session: child,
            max_lines: None,
        }) else {
            panic!("terminal read")
        };
        if text.contains(marker) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "missing child-emitted marker {marker}; terminal={text}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn public_assignment_human_takeover_return_fences_generics_and_preserves_history() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let original = complete(&fixture, &first);
    settled(&fixture, &original);
    let sibling: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let sibling = complete(&fixture, &sibling);
    let child = original.identity.session.unwrap();
    let run = original.identity.run.unwrap();
    let original_bytes = fs::read(&original.result_path).unwrap();
    let before = fixture.byte_count(&original);
    let mut stream = dashboard(&fixture, &original);
    // Inspection/focus and geometry do not transfer authority or stop the child.
    view(&mut stream, child, run, 2);
    view(&mut stream, child, run, 3);
    assert_eq!(
        fixture.reconcile(&original.handle).control,
        AssignmentControl::default()
    );
    for request in [
        Request::Input {
            session: child,
            run,
            bytes: b"human_marker:FORBIDDEN\r".to_vec(),
        },
        Request::AssignedHumanInput {
            session: child,
            run,
            control_epoch: 1,
            view_revision: 3,
            bytes: b"human_marker:FORBIDDEN\r".to_vec(),
        },
    ] {
        assert!(matches!(
            dashboard_view(&mut stream, 30, request).unwrap(),
            Response::Error { .. }
        ));
    }
    let human = transition(&fixture, &mut stream, child, "take-one", true, 3);
    assert_eq!(human.control.owner, ControlOwner::Human);
    assert_eq!(human.control.epoch, 2);
    for request in [
        Request::SendTerminal {
            session: child,
            text: "FORBIDDEN".into(),
            submit: true,
        },
        Request::Keystroke {
            session: child,
            expected_run: run,
            key: ":enter:".into(),
        },
        Request::AssignedHumanInput {
            session: child,
            run,
            control_epoch: 2,
            view_revision: 3,
            bytes: b"FORBIDDEN".to_vec(),
        },
    ] {
        assert!(
            matches!(
                fixture.live.request(request),
                Response::Error {
                    code: ErrorCode::Conflict,
                    ..
                }
            ),
            "generic/control role cannot use Human authority"
        );
    }
    assert_eq!(fixture.byte_count(&original), before);
    assert_eq!(
        dashboard_view(
            &mut stream,
            40,
            Request::AssignedHumanInput {
                session: child,
                run,
                control_epoch: 2,
                view_revision: 3,
                bytes: b"human_marker:TAKEOVER_INPUT_OK_323\r".to_vec()
            }
        )
        .unwrap(),
        Response::Ok
    );
    read_marker(&fixture, child, "RECEIVED_HUMAN_TAKEOVER_INPUT_OK_323");
    let Response::TerminalText { text, .. } = fixture.live.request(Request::ReadTerminal {
        session: sibling.identity.session.unwrap(),
        max_lines: None,
    }) else {
        panic!()
    };
    assert!(!text.contains("TAKEOVER_INPUT_OK_323"));
    let returned = transition(&fixture, &mut stream, child, "return-one", false, 3);
    assert_eq!(returned.control.epoch, 3);
    assert!(returned.control.attribution_invalidated);
    assert!(matches!(
        returned.control.barrier,
        Some(ControlBarrier::HumanReturn { .. })
    ));
    assert!(matches!(
        dashboard_view(
            &mut stream,
            41,
            Request::AssignedHumanInput {
                session: child,
                run,
                control_epoch: 2,
                view_revision: 3,
                bytes: b"human_marker:STALE\r".to_vec()
            }
        )
        .unwrap(),
        Response::Error { .. }
    ));
    let observed = fixture.tool(
        "ovrcr_wait",
        serde_json::json!({"handles":[returned.handle],"wait_ms":0}),
    );
    assert_eq!(
        observed["observations"][0]["control"],
        serde_json::to_value(&returned.control).unwrap()
    );
    assert_eq!(observed["status"], "ReconcileRequired");
    let again = fixture.tool(
        "ovrcr_wait",
        serde_json::json!({"handles":[returned.handle],"wait_ms":0}),
    );
    assert_eq!(
        again["observations"][0]["control"],
        observed["observations"][0]["control"]
    );
    assert_eq!(
        fixture.reconcile(&returned.handle).control,
        returned.control,
        "observation cannot clear the return barrier"
    );
    let confirmed = confirm(&fixture, &returned, "return-one", "confirm-one");
    assert_eq!(confirmed.control.epoch, 4);
    assert!(confirmed.control.barrier.is_none());
    assert!(confirmed.control.attribution_invalidated);
    let rejected=fixture.tool("ovrcr_accept",serde_json::json!({"handle":confirmed.handle,
        "identity":confirmed.identity,"result_version":confirmed.result_version,
        "result_digest":original.publication.as_ref().unwrap().identity.digest,"expected_control_epoch":4}));
    assert_eq!(rejected["dispatch"], "Refused", "{rejected}");
    let hold = original
        .brief
        .parent()
        .unwrap()
        .join("brief-v2.json.hold-settlement");
    fs::write(&hold, "force settlement after return evidence is captured").unwrap();
    let fresh:AssignmentSnapshot=serde_json::from_value(fixture.tool("ovrcr_followup",serde_json::json!({
        "handle":confirmed.handle,"expected_identity":confirmed.identity,"expected_result_version":confirmed.result_version,
        "expected_control_epoch":4,"text":"Reconcile the human clarification and publish fresh evidence."}))).unwrap();
    assert_eq!(fresh.result_version, 2);
    assert!(!fresh.control.attribution_invalidated);
    let fresh = complete(&fixture, &fresh);
    wait_file(&fresh.brief.with_extension("json.settlement-held"));
    assert_eq!(
        fs::read(&original.result_path).unwrap(),
        original_bytes,
        "immutable historical result survives human work"
    );
    assert_eq!(
        fixture.reconcile(&sibling.handle).control,
        AssignmentControl::default(),
        "sibling authority is independent"
    );
    let human = transition(&fixture, &mut stream, child, "take-two", true, 3);
    assert_eq!(human.control.epoch, 5);
    let returned = transition(&fixture, &mut stream, child, "return-two", false, 3);
    assert_eq!(returned.activity, AgentActivity::Busy);
    fs::remove_file(&hold).unwrap();
    // Publication precedes settlement; confirmation needs evidence from after both.
    let returned = settled(&fixture, &returned);
    let confirmed = confirm(&fixture, &returned, "return-two", "confirm-two");
    assert_eq!(confirmed.control.epoch, 7);
    let stale=fixture.tool("ovrcr_followup",serde_json::json!({"handle":fresh.handle,"expected_identity":fresh.identity,
        "expected_result_version":fresh.result_version,"expected_control_epoch":4,"text":"Must never replay old epoch"}));
    assert_eq!(stale["dispatch"], "Refused");
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn accepted_parent_followup_never_replays_after_passive_human_control_aba() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let original = complete(&fixture, &first);
    assert_eq!(original.execution_attempts, Some(1));
    let before = fixture.byte_count(&original);
    fixture.proxy.arm(ControlRule {
        operation: "Followup",
        loss: 2,
        fault: Some(("followup-after-files", 0)),
    });
    let mut reserved_contract: ResearchContract = serde_json::from_slice(
        &fs::read(original.brief.parent().unwrap().join("contract-v1.json")).unwrap(),
    )
    .unwrap();
    reserved_contract.scope = "Reserved revision must never be silently applied".into();
    let pending: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_followup",
        serde_json::json!({
        "handle":original.handle,"expected_identity":original.identity,"expected_result_version":1,
        "expected_control_epoch":1,"text":"Frozen old-epoch continuation","revised_contract":reserved_contract}),
    ))
    .unwrap();
    let receipt = pending
        .receipts
        .iter()
        .find(|receipt| receipt.operation == DispatchOperation::Followup)
        .unwrap();
    assert_eq!(receipt.outcome, DispatchOutcome::Accepted);
    let mut stream = dashboard(&fixture, &original);
    let child = original.identity.session.unwrap();
    transition(&fixture, &mut stream, child, "passive-take", true, 1);
    let returned = transition(&fixture, &mut stream, child, "passive-return", false, 1);
    let confirmed = confirm(&fixture, &returned, "passive-return", "passive-confirm");
    assert_eq!(confirmed.control.epoch, 4);
    assert!(!confirmed.control.attribution_invalidated);
    let recovered: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_followup",
        serde_json::json!({
        "handle":original.handle,"request":receipt.request}),
    ))
    .unwrap();
    let final_receipt = recovered
        .receipts
        .iter()
        .find(|candidate| candidate.request == receipt.request)
        .unwrap();
    assert_eq!(final_receipt.outcome, DispatchOutcome::Rejected);
    assert_eq!(final_receipt.identity, receipt.identity);
    assert_eq!(final_receipt.result_version, receipt.result_version);
    assert_eq!(recovered.result_version, 1);
    assert_eq!(fixture.byte_count(&original), before);
    let again: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_followup",
        serde_json::json!({
        "handle":original.handle,"request":receipt.request}),
    ))
    .unwrap();
    assert_eq!(again.receipts, recovered.receipts);
    assert_eq!(fixture.byte_count(&original), before);
    let reserved = fs::read(original.brief.parent().unwrap().join("brief-v2.json")).unwrap();
    let reserved_contract_bytes =
        fs::read(original.brief.parent().unwrap().join("contract-v2.json")).unwrap();
    let continued: AssignmentSnapshot = serde_json::from_value(fixture.tool("ovrcr_followup", serde_json::json!({
        "handle":original.handle,"expected_identity":original.identity,"expected_result_version":1,
        "expected_control_epoch":4,"text":"Fresh continuation after explicit return reconciliation"}))).unwrap();
    assert_eq!(
        continued.result_version, 3,
        "never reuse the rejected intent's immutable reserved version"
    );
    assert_eq!(
        continued.identity.contract, 1,
        "ordinary continuation keeps current contract despite rejected reserved revision"
    );
    assert_eq!(continued.execution_attempts, Some(1));
    let continued = complete(&fixture, &continued);
    assert_eq!(
        continued.execution_attempts,
        Some(1),
        "collection must preserve the proven execution across reserved result2"
    );
    assert_eq!(
        fixture.reconcile(&continued.handle).execution_attempts,
        Some(1)
    );
    assert_eq!(fixture.input_count(&continued), 1);
    assert_eq!(
        fs::read(original.brief.parent().unwrap().join("brief-v2.json")).unwrap(),
        reserved
    );
    assert_eq!(
        fs::read(original.brief.parent().unwrap().join("contract-v2.json")).unwrap(),
        reserved_contract_bytes
    );
    reserved_contract.scope = "Fresh explicit revision after rejected reserved contract".into();
    let revised:AssignmentSnapshot=serde_json::from_value(fixture.tool("ovrcr_followup",serde_json::json!({
        "handle":continued.handle,"expected_identity":continued.identity,"expected_result_version":continued.result_version,
        "expected_control_epoch":4,"text":"Use this newly approved revision","revised_contract":reserved_contract}))).unwrap();
    assert_eq!(
        revised.identity.contract, 3,
        "explicit revision skips every durable reserved contract version"
    );
    assert_eq!(revised.result_version, 4);
    assert_eq!(revised.execution_attempts, Some(1));
    let revised = complete(&fixture, &revised);
    assert_eq!(
        revised.execution_attempts,
        Some(1),
        "a reserved contract gap does not consume or erase execution proof"
    );
    assert_eq!(
        fixture.reconcile(&revised.handle).execution_attempts,
        Some(1)
    );
    assert_eq!(fixture.input_count(&revised), 1);
    assert_eq!(
        fs::read(original.brief.parent().unwrap().join("contract-v2.json")).unwrap(),
        reserved_contract_bytes
    );
    let hole = tool_reply(
        &fixture.live,
        fixture.parent,
        "ovrcr_collect",
        serde_json::json!({"handle":original.handle,"version":2}),
        &fixture.live.root.path().join("rejected-hole.json"),
    );
    assert!(
        hole["error"]
            .as_str()
            .unwrap()
            .contains("result version not found"),
        "a rejected reserved version is not a completed historical publication: {hole}"
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn failed_control_or_human_revision_save_prevents_authority_and_bytes() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let original = complete(&fixture, &first);
    let child = original.identity.session.unwrap();
    let run = original.identity.run.unwrap();
    let before = fixture.byte_count(&original);
    let mut stream = dashboard(&fixture, &original);
    fs::write(
        fixture.live.root.path().join("fault"),
        "control-before-save|failed-take|0",
    )
    .unwrap();
    assert!(matches!(
        dashboard_view(
            &mut stream,
            300,
            Request::AssignmentControl(AssignmentControlRequest::Takeover {
                request: "failed-take".into(),
                session: child,
                run,
                expected_epoch: 1,
                view_revision: 1
            })
        )
        .unwrap(),
        Response::Error { .. }
    ));
    assert_eq!(
        fixture.reconcile(&original.handle).control,
        AssignmentControl::default()
    );
    transition(&fixture, &mut stream, child, "successful-take", true, 1);
    fs::write(
        fixture.live.root.path().join("fault"),
        format!("human-before-save|{}|0", original.handle),
    )
    .unwrap();
    assert!(matches!(
        dashboard_view(
            &mut stream,
            301,
            Request::AssignedHumanInput {
                session: child,
                run,
                control_epoch: 2,
                view_revision: 1,
                bytes: b"human_marker:MUST_NOT_APPLY\r".to_vec()
            }
        )
        .unwrap(),
        Response::Error { .. }
    ));
    assert_eq!(fixture.byte_count(&original), before);
    let projected = current(&fixture, child).assignment_control.unwrap();
    assert_eq!(projected.control.human_input_revision, 0);
    assert!(!projected.control.attribution_invalidated);
}

#[test]
fn authorized_human_and_ordinary_input_continue_before_unrelated_blocked_git_releases() {
    let fixture = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let original = complete(&fixture, &started);
    let child = original.identity.session.unwrap();
    let run = original.identity.run.unwrap();
    let mut stream = dashboard(&fixture, &original);
    transition(&fixture, &mut stream, child, "take-before-git", true, 1);
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let ordinary = sessions.iter().find(|s| s.name == "local").unwrap();
    let actual_git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("git"))
        .find(|p| p.is_file())
        .unwrap();
    let entered = fixture.live.root.path().join("git-entered");
    let release = fixture.live.root.path().join("git-release");
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "1");
        }
    }
    let _release = Release(release.clone());
    let shim = fixture.live.claude.command.parent().unwrap().join("git");
    fs::write(&shim,format!("#!/bin/sh\ncase \"$*\" in *\"worktree add\"*\"blocked-human-git\"*) : > '{}'; while [ ! -f '{}' ]; do sleep 0.01; done;; esac\nexec '{}' \"$@\"\n",entered.display(),release.display(),actual_git.display())).unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        dashboard_view(
            &mut stream,
            400,
            Request::CreateWorkspace {
                project: PROJECT.into(),
                id: "blocked-human-git".into(),
                branch: BranchRequest::New {
                    branch: "blocked-human-git".into(),
                    base: "main".into()
                }
            }
        )
        .unwrap(),
        Response::Ok
    );
    wait_file(&entered);
    assert_eq!(
        dashboard_view(
            &mut stream,
            401,
            Request::AssignedHumanInput {
                session: child,
                run,
                control_epoch: 2,
                view_revision: 1,
                bytes: b"human_marker:DURING_BLOCKED_GIT_323\r".to_vec()
            }
        )
        .unwrap(),
        Response::Ok
    );
    read_marker(&fixture, child, "RECEIVED_HUMAN_DURING_BLOCKED_GIT_323");
    view(&mut stream, ordinary.id, ordinary.run, 2);
    let marker = fixture.live.root.path().join("ordinary-during-git");
    assert_eq!(
        dashboard_view(
            &mut stream,
            402,
            Request::Input {
                session: ordinary.id,
                run: ordinary.run,
                bytes: format!("printf delivered > '{}'\r", marker.display()).into_bytes()
            }
        )
        .unwrap(),
        Response::Ok
    );
    wait_file(&marker);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "delivered");
    assert!(
        !release.exists(),
        "both markers and SetView completed while Git still holds lifecycle mutation_lock"
    );
    fs::write(release, "1").unwrap();
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn disconnected_dashboard_waiting_at_final_human_writer_fence_cannot_emit_bytes() {
    let fixture = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let original = complete(&fixture, &started);
    let child = original.identity.session.unwrap();
    let run = original.identity.run.unwrap();
    let before = fixture.byte_count(&original);
    let mut stream = dashboard(&fixture, &original);
    transition(
        &fixture,
        &mut stream,
        child,
        "take-before-owner-loss",
        true,
        1,
    );
    let release = fixture.live.root.path().join("fault.release");
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "1");
        }
    }
    let _release = Release(release.clone());
    fs::write(
        fixture.live.root.path().join("fault"),
        format!("human-before-view|{}|0", original.handle),
    )
    .unwrap();
    write_frame(
        &mut stream,
        &ClientMessage {
            request_id: 500,
            request: Request::AssignedHumanInput {
                session: child,
                run,
                control_epoch: 2,
                view_revision: 1,
                bytes: b"human_marker:OLD_DASHBOARD_MUST_NOT_WRITE\r".to_vec(),
            },
        },
    )
    .unwrap();
    wait_file(&fixture.live.root.path().join("fault.hit"));
    // The actual writer has been acquired, but no slot/view guard or byte has been admitted.
    stream.shutdown(std::net::Shutdown::Both).unwrap();
    drop(stream);
    fs::write(&release, "1").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut replacement = loop {
        let mut stream = connect_server(&fixture.live.socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        if matches!(
            client::request(&mut stream, 1, Request::DashboardHello).unwrap(),
            Response::Hierarchy(_)
        ) {
            break stream;
        }
        assert!(
            Instant::now() < deadline,
            "replacement Dashboard did not become available"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    view(&mut replacement, child, run, 1);
    assert_eq!(fixture.byte_count(&original), before);
    let projected = current(&fixture, child).assignment_control.unwrap();
    assert_eq!(projected.control.human_input_revision, 0);
    assert_eq!(
        dashboard_view(
            &mut replacement,
            501,
            Request::AssignedHumanInput {
                session: child,
                run,
                control_epoch: 2,
                view_revision: 1,
                bytes: b"human_marker:REPLACEMENT_OWNS_INPUT_323\r".to_vec()
            }
        )
        .unwrap(),
        Response::Ok
    );
    read_marker(&fixture, child, "RECEIVED_HUMAN_REPLACEMENT_OWNS_INPUT_323");
}

#[test]
fn unexpected_human_conversation_change_suspends_only_that_assignment() {
    let fixture = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let original = complete(&fixture, &started);
    let child = original.identity.session.unwrap();
    let run = original.identity.run.unwrap();
    let evidence = fs::read(&original.result_path).unwrap();
    let mut stream = dashboard(&fixture, &original);
    transition(
        &fixture,
        &mut stream,
        child,
        "take-before-conversation-change",
        true,
        1,
    );
    assert_eq!(
        dashboard_view(
            &mut stream,
            600,
            Request::AssignedHumanInput {
                session: child,
                run,
                control_epoch: 2,
                view_revision: 1,
                bytes: b"session_start:changed-human-conversation\r".to_vec()
            }
        )
        .unwrap(),
        Response::Ok
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    let uncertain = loop {
        let projected = current(&fixture, child).assignment_control.unwrap();
        if projected.control.owner == ControlOwner::Uncertain {
            break projected;
        }
        assert!(
            Instant::now() < deadline,
            "changed reporting binding did not suspend control"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(uncertain.evidence.is_none());
    assert!(uncertain.control.attribution_invalidated);
    assert!(matches!(
        dashboard_view(
            &mut stream,
            601,
            Request::AssignedHumanInput {
                session: child,
                run,
                control_epoch: uncertain.control.epoch,
                view_revision: 1,
                bytes: b"FORBIDDEN\r".to_vec()
            }
        )
        .unwrap(),
        Response::Error { .. }
    ));
    let reconciled = fixture.reconcile(&original.handle);
    assert_eq!(
        reconciled.control, uncertain.control,
        "Reconcile cannot clear uncertainty"
    );
    let denied=fixture.tool("ovrcr_confirm_return",serde_json::json!({"handle":original.handle,"request":"cannot-confirm-uncertain",
        "returned_request":"nonexistent-return","expected_control_epoch":uncertain.control.epoch,"evidence":original.control_evidence}));
    assert_eq!(denied["dispatch"], "Refused", "{denied}");
    let history: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":original.handle,"version":1}),
    ))
    .unwrap();
    assert_eq!(history.historical_submission, original.submission);
    assert_eq!(fs::read(&original.result_path).unwrap(), evidence);
}

#[test]
fn pause_resume_preserves_exact_controller_and_human_authority_without_paused_bytes() {
    let fixture = OwnedFollowup::new();
    let first: AssignmentSnapshot = serde_json::from_value(fixture.start()).unwrap();
    let original = complete(&fixture, &first);
    let child = original.identity.session.unwrap();
    let run = original.identity.run.unwrap();
    let mut stream = dashboard(&fixture, &original);
    for human in [false, true] {
        if human {
            transition(&fixture, &mut stream, child, "pause-take", true, 1);
        }
        let before = fixture.reconcile(&original.handle);
        let bytes = fixture.byte_count(&original);
        assert_eq!(
            fixture
                .live
                .request(Request::PauseSession { session: child }),
            Response::Ok
        );
        let paused = current(&fixture, child);
        assert_eq!(paused.phase, SessionPhase::Paused);
        assert_eq!(
            paused.assignment_control.unwrap().control,
            before.control,
            "pause is not an attribution change"
        );
        assert!(matches!(
            dashboard_view(
                &mut stream,
                80,
                Request::AssignedHumanInput {
                    session: child,
                    run,
                    control_epoch: before.control.epoch,
                    view_revision: 1,
                    bytes: b"human_marker:PAUSED_FORBIDDEN\r".to_vec(),
                }
            )
            .unwrap(),
            Response::Error { .. }
        ));
        assert_eq!(fixture.byte_count(&original), bytes);
        assert_eq!(
            fixture
                .live
                .request(Request::ResumeSession { session: child }),
            Response::Ok
        );
        let resumed = current(&fixture, child);
        assert_eq!(resumed.phase, SessionPhase::Running);
        assert_eq!(resumed.run, run);
        assert_eq!(resumed.assignment_control.unwrap().control, before.control);
    }
    assert_eq!(
        dashboard_view(
            &mut stream,
            81,
            Request::AssignedHumanInput {
                session: child,
                run,
                control_epoch: 2,
                view_revision: 1,
                bytes: b"human_marker:RESUMED_CONTROL_OK_323\r".to_vec(),
            }
        )
        .unwrap(),
        Response::Ok
    );
    read_marker(&fixture, child, "RECEIVED_HUMAN_RESUMED_CONTROL_OK_323");
    let returned = transition(&fixture, &mut stream, child, "pause-return", false, 1);
    let confirmed = confirm(&fixture, &returned, "pause-return", "pause-confirm");
    assert_eq!(confirmed.control.epoch, 4);
    assert!(confirmed.control.barrier.is_none());
}

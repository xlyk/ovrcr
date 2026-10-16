//! Registered Pi tools -> production CLI -> private binary Server and real PTYs.
use super::followups::{OwnedFollowup, complete, followup};
use super::*;

fn current(f: &RecoveryFixture, id: SessionId) -> SessionSummary {
    let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
        panic!("inventory");
    };
    sessions
        .into_iter()
        .find(|s| s.id == id)
        .expect("retained row survives")
}
fn accept(f: &RecoveryFixture, child: &AssignmentSnapshot) -> AssignmentSnapshot {
    serde_json::from_value(f.tool("ovrcr_accept", serde_json::json!({
        "handle":child.handle,"identity":child.identity,"result_version":child.result_version,
        "result_digest":child.publication.as_ref().unwrap().identity.digest,"expected_control_epoch":child.control.epoch
    }))).unwrap()
}
fn declaration(
    child: &AssignmentSnapshot,
    request: &str,
    previous: Option<DeliveryRef>,
    retention: RetentionChoice,
) -> DeliveryDeclaration {
    DeliveryDeclaration {
        request: request.into(),
        workflow: child.identity.workflow.clone(),
        scope: "Deliver the reviewed README finding".into(),
        provenance: "Parent delivery decision for this task".into(),
        resources: vec![DeliveryResource {
            handle: child.handle.clone(),
            identity: child.identity.clone(),
            result_version: child.result_version,
            obligation_revision: child.obligation_revision,
            publication: child.publication.as_ref().map(|p| p.identity.clone()),
            acceptance: child.acceptance_ref.clone(),
            evidence: vec![],
            completed_obligations: vec!["README follow-up and review".into()],
            retention,
            previous,
        }],
    }
}
fn declare(f: &RecoveryFixture, declaration: &DeliveryDeclaration) -> WorkflowSnapshot {
    serde_json::from_value(f.tool(
        "ovrcr_deliver",
        serde_json::json!({"declaration":declaration}),
    ))
    .unwrap()
}
fn reference(workflow: &WorkflowSnapshot, request: &str) -> DeliveryRef {
    workflow
        .deliveries
        .iter()
        .find(|d| d.reference.request == request)
        .unwrap()
        .reference
        .clone()
}
fn archive_request(
    child: &AssignmentSnapshot,
    request: &str,
    delivery: DeliveryRef,
) -> ArchiveRequest {
    ArchiveRequest {
        request: request.into(),
        handle: child.handle.clone(),
        expected_identity: child.identity.clone(),
        expected_result_version: child.result_version,
        expected_obligation_revision: child.obligation_revision,
        expected_control_epoch: child.control.epoch,
        delivery,
        continuation_of: None,
    }
}
fn archive(f: &RecoveryFixture, request: &ArchiveRequest) -> AssignmentSnapshot {
    serde_json::from_value(f.tool("ovrcr_archive", serde_json::to_value(request).unwrap())).unwrap()
}
fn receipt<'a>(child: &'a AssignmentSnapshot, request: &str) -> &'a ArchiveReceipt {
    child
        .archive_receipts
        .iter()
        .find(|r| r.intent.request.request == request)
        .unwrap()
}
fn auth(f: &RecoveryFixture) -> DelegationAuth {
    let DelegationRequest::Prepare { auth, .. } = f.proxy.original(DispatchOperation::Prepare)
    else {
        panic!("original auth");
    };
    auth
}
fn database(f: &RecoveryFixture) -> rusqlite::Connection {
    rusqlite::Connection::open(f.live.config.join("registry.sqlite3")).unwrap()
}
fn persisted(f: &RecoveryFixture, handle: &str) -> serde_json::Value {
    let raw: String = database(f)
        .query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [handle],
            |r| r.get(0),
        )
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}
fn stopped_row(f: &RecoveryFixture, id: SessionId) -> (u64, bool, u8) {
    let (run, stopped, disposition): (i64, bool, u8) = database(f)
        .query_row(
            "SELECT run,stopped,disposition FROM retained_sessions WHERE id=?1",
            [i64::try_from(id.0).expect("session identity fits SQLite integer")],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    (
        u64::try_from(run).expect("retained run must be nonnegative"),
        stopped,
        disposition,
    )
}
fn assert_archived(child: &AssignmentSnapshot, request: &str) {
    let run = child.identity.run.unwrap();
    assert_eq!(
        receipt(child, request).outcome,
        ArchiveOutcome::Archived {
            previous_run: run,
            archived_run: SessionRunId(run.0 + 1)
        }
    );
    assert!(child.control.barrier.is_none());
}

#[test]
fn delivery_retains_same_child_followup_then_archives_once_preserving_all_evidence() {
    let f = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let first = accept(&f, &complete(&f, &started));
    let id = first.identity.session.unwrap();
    assert!(!current(&f, id).archived, "acceptance alone is not archive");
    let second: AssignmentSnapshot = serde_json::from_value(followup(&f, &first, None)).unwrap();
    let second = accept(&f, &complete(&f, &second));
    assert_eq!(second.identity.session, first.identity.session);
    assert_eq!(second.identity.run, first.identity.run);
    assert!(second.obligation_revision > first.obligation_revision);
    assert!(!current(&f, id).archived);
    let root = first.brief.parent().unwrap();
    let evidence: Vec<_> = [
        "contract-v1.json",
        "brief-v1.json",
        "submission-v1.json",
        "acceptance-v1.json",
        "brief-v2.json",
        "submission-v2.json",
        "acceptance-v2.json",
    ]
    .into_iter()
    .map(|name| (root.join(name), fs::read(root.join(name)).unwrap()))
    .collect();
    let workspace_before = fs::canonicalize(current(&f, id).cwd).unwrap();
    let history_path = f
        .live
        .root
        .path()
        .join("history")
        .join(format!("{}.jsonl", second.conversation.as_ref().unwrap()));
    let history_before = fs::read(&history_path).unwrap();
    let body = declaration(&second, "delivered", None, RetentionChoice::ArchiveEligible);
    let delivered = declare(&f, &body);
    assert_eq!(
        declare(&f, &body).deliveries,
        delivered.deliveries,
        "delivery replay is immutable"
    );
    let request = archive_request(&second, "archive-one", reference(&delivered, "delivered"));
    let archived = archive(&f, &request);
    assert_archived(&archived, "archive-one");
    let row = current(&f, id);
    assert!(row.archived && !row.phase.is_live());
    assert_eq!(row.run.0, first.identity.run.unwrap().0 + 1);
    assert_eq!(
        archive(&f, &request).archive_receipts,
        archived.archive_receipts,
        "lost reply replay never increments again"
    );
    assert_eq!(current(&f, id).run, row.run);
    for (path, bytes) in evidence {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert!(root.exists() && workspace_before.exists());
    assert_eq!(
        fs::read(&history_path).unwrap(),
        history_before,
        "child provider history survives archive"
    );
    let historical = f.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":first.handle,"version":first.result_version}),
    );
    let historical: AssignmentSnapshot = serde_json::from_value(historical).unwrap();
    assert_eq!(historical.selected_version, Some(first.result_version));
    assert_eq!(
        historical.historical_submission.as_ref().unwrap().identity,
        first.identity
    );
    assert!(
        historical.submission.is_none(),
        "archived evidence never becomes a current result"
    );
}

#[test]
fn explicit_keep_and_scoped_successor_head_control_archive_eligibility() {
    let f = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    let eligible = declare(
        &f,
        &declaration(&child, "eligible", None, RetentionChoice::ArchiveEligible),
    );
    let old = reference(&eligible, "eligible");
    let keep = declare(
        &f,
        &declaration(
            &child,
            "keep",
            Some(old.clone()),
            RetentionChoice::Keep {
                reason: "retain for requested review".into(),
            },
        ),
    );
    let stale = archive(&f, &archive_request(&child, "stale-delivery", old));
    assert_eq!(
        receipt(&stale, "stale-delivery").outcome,
        ArchiveOutcome::Refused {
            reason: ArchiveRefusal::DeliverySuperseded
        }
    );
    let kept = archive(
        &f,
        &archive_request(&child, "required-keep", reference(&keep, "keep")),
    );
    assert_eq!(
        receipt(&kept, "required-keep").outcome,
        ArchiveOutcome::Refused {
            reason: ArchiveRefusal::RequiredRetention
        }
    );
    assert!(current(&f, child.identity.session.unwrap()).phase.is_live());
    let released = declare(
        &f,
        &declaration(
            &child,
            "explicit-release",
            Some(reference(&keep, "keep")),
            RetentionChoice::ArchiveEligible,
        ),
    );
    assert_eq!(
        released.deliveries[1], keep.deliveries[1],
        "retention change appends without rewriting"
    );
    assert_archived(
        &archive(
            &f,
            &archive_request(
                &child,
                "archive-release",
                reference(&released, "explicit-release"),
            ),
        ),
        "archive-release",
    );
}

#[test]
fn stale_or_foreign_archive_identity_never_controls_a_replacement_or_other_child() {
    let f = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    let other: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let delivered = declare(
        &f,
        &declaration(&child, "delivered", None, RetentionChoice::ArchiveEligible),
    );
    let mut request = archive_request(&child, "foreign-target", reference(&delivered, "delivered"));
    request.expected_identity.session = other.identity.session;
    request.expected_identity.run = other.identity.run;
    let refused = archive(&f, &request);
    assert_eq!(
        receipt(&refused, "foreign-target").outcome,
        ArchiveOutcome::Refused {
            reason: ArchiveRefusal::StaleTarget
        }
    );
    assert!(current(&f, child.identity.session.unwrap()).phase.is_live());
    assert!(current(&f, other.identity.session.unwrap()).phase.is_live());
    assert!(!current(&f, other.identity.session.unwrap()).archived);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn followup_admission_invalidates_delivery_before_new_result_files_or_version() {
    let f = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    let delivered = declare(
        &f,
        &declaration(&child, "delivered", None, RetentionChoice::ArchiveEligible),
    );
    f.proxy.arm(ControlRule {
        operation: "Followup",
        loss: 2,
        fault: Some(("followup-before-files", 0)),
    });
    let pending: AssignmentSnapshot = serde_json::from_value(followup(&f, &child, None)).unwrap();
    assert_eq!(pending.result_version, child.result_version);
    assert!(
        pending.obligation_revision > child.obligation_revision,
        "new obligation begins at admission"
    );
    assert!(!child.brief.parent().unwrap().join("brief-v2.json").exists());
    let stale = archive(
        &f,
        &archive_request(
            &child,
            "stale-obligation",
            reference(&delivered, "delivered"),
        ),
    );
    assert_eq!(
        receipt(&stale, "stale-obligation").outcome,
        ArchiveOutcome::Refused {
            reason: ArchiveRefusal::ObligationChanged
        }
    );
    assert!(current(&f, child.identity.session.unwrap()).phase.is_live());
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn lost_archive_reply_reconciles_original_receipt_without_another_owned_stop() {
    let f = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    let delivered = declare(
        &f,
        &declaration(&child, "delivered", None, RetentionChoice::ArchiveEligible),
    );
    let request = archive_request(&child, "lost-archive", reference(&delivered, "delivered"));
    f.proxy.arm(ControlRule {
        operation: "Archive",
        loss: 1,
        fault: None,
    });
    let unknown = f.tool("ovrcr_archive", serde_json::to_value(&request).unwrap());
    assert_eq!(unknown["transport"], "Unknown");
    assert!(
        unknown.get("archive_receipts").is_none(),
        "transport does not fabricate a Server receipt"
    );
    let replay = archive(&f, &request);
    assert_archived(&replay, "lost-archive");
    assert_eq!(replay.archive_receipts.len(), 1);
    assert_eq!(
        stopped_row(&f, child.identity.session.unwrap()).0,
        child.identity.run.unwrap().0 + 1
    );
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn archive_transaction_rollback_keeps_sql_cache_and_predecessor_until_atomic_continuation() {
    let f = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    let delivered = declare(
        &f,
        &declaration(&child, "delivered", None, RetentionChoice::ArchiveEligible),
    );
    let request = archive_request(&child, "stop-then-fail", reference(&delivered, "delivered"));
    database(&f).execute_batch("CREATE TRIGGER fail_archive BEFORE UPDATE OF disposition ON retained_sessions WHEN NEW.disposition=1 BEGIN SELECT RAISE(FAIL,'owned archive filing failure'); END;").unwrap();
    let failed = f.tool("ovrcr_archive", serde_json::to_value(&request).unwrap());
    assert_eq!(failed["transport"], "Unknown");
    let pending = f.reconcile(&child.handle);
    let original = receipt(&pending, "stop-then-fail").clone();
    assert_eq!(original.outcome, ArchiveOutcome::StoppedArchivePending);
    let id = child.identity.session.unwrap();
    let run = child.identity.run.unwrap();
    assert_eq!(stopped_row(&f, id), (run.0, true, 0));
    assert_eq!(current(&f, id).run, run);
    assert!(!current(&f, id).archived);
    let mut continuation = archive_request(&pending, "continue-one", request.delivery.clone());
    continuation.continuation_of = Some("stop-then-fail".into());
    for label in ["continue-one", "continue-two", "continue-three"] {
        continuation.request = label.into();
        let _ = f.tool(
            "ovrcr_archive",
            serde_json::to_value(&continuation).unwrap(),
        );
        let raw = persisted(&f, &child.handle);
        assert_eq!(
            raw["archive_receipts"].as_array().unwrap().len(),
            1,
            "failed transaction consumes no protected slot"
        );
        assert_eq!(
            f.reconcile(&child.handle).archive_receipts,
            vec![original.clone()]
        );
        assert_eq!(stopped_row(&f, id), (run.0, true, 0));
    }
    database(&f)
        .execute_batch("DROP TRIGGER fail_archive;")
        .unwrap();
    continuation.request = "continue-success".into();
    let archived = archive(&f, &continuation);
    assert_archived(&archived, "continue-success");
    assert_eq!(
        receipt(&archived, "stop-then-fail"),
        &original,
        "predecessor stays immutable"
    );
    assert_eq!(archived.archive_receipts.len(), 2);
    assert_eq!(
        archive(&f, &continuation).archive_receipts,
        archived.archive_receipts
    );
    assert_eq!(stopped_row(&f, id), (run.0 + 1, true, 1));
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn unknown_archive_barrier_blocks_fresh_requests_and_stop_unknown_cannot_continue() {
    let f = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    let delivered = declare(
        &f,
        &declaration(&child, "delivered", None, RetentionChoice::ArchiveEligible),
    );
    let request = archive_request(
        &child,
        "original-unknown",
        reference(&delivered, "delivered"),
    );
    // Abandon after durable admission and before termination; a replacement
    // request cannot resume the unknown original chain.
    fs::write(
        f.live.root.path().join("fault"),
        "archive-abandon-admission|original-unknown|0",
    )
    .unwrap();
    let _ = f.tool("ovrcr_archive", serde_json::to_value(&request).unwrap());
    let pending = f.reconcile(&child.handle);
    assert_eq!(
        receipt(&pending, "original-unknown").outcome,
        ArchiveOutcome::StopUnknown
    );
    assert!(current(&f, child.identity.session.unwrap()).phase.is_live());
    let mut continuation = archive_request(&pending, "cannot-continue", request.delivery.clone());
    continuation.continuation_of = Some("original-unknown".into());
    let _ = f.tool(
        "ovrcr_archive",
        serde_json::to_value(&continuation).unwrap(),
    );
    assert_eq!(
        f.reconcile(&child.handle).archive_receipts,
        pending.archive_receipts
    );
    let fresh = archive(
        &f,
        &archive_request(&pending, "cannot-bypass", request.delivery),
    );
    assert_eq!(
        receipt(&fresh, "cannot-bypass").outcome,
        ArchiveOutcome::Refused {
            reason: ArchiveRefusal::UnresolvedControl
        }
    );
    assert_eq!(
        receipt(&fresh, "original-unknown"),
        receipt(&pending, "original-unknown")
    );
}

#[test]
fn dashboard_human_takeover_refuses_archive_without_affecting_its_current_run() {
    let f = OwnedFollowup::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    let delivered = declare(
        &f,
        &declaration(&child, "delivered", None, RetentionChoice::ArchiveEligible),
    );
    let id = child.identity.session.unwrap();
    let run = child.identity.run.unwrap();
    let mut dashboard = connect_server(&f.live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    assert!(matches!(
        client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
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
                        session: id,
                        run,
                        size: TerminalSize {
                            rows: 24,
                            cols: 120
                        }
                    }],
                    focused: Some(id)
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
                request: "person-owns-child".into(),
                session: id,
                run,
                expected_epoch: child.control.epoch,
                view_revision: 1
            })
        )
        .unwrap(),
        Response::Ok
    );
    let owned = f.reconcile(&child.handle);
    assert_eq!(owned.control.owner, ControlOwner::Human);
    let refused = archive(
        &f,
        &archive_request(&owned, "human-refused", reference(&delivered, "delivered")),
    );
    assert_eq!(
        receipt(&refused, "human-refused").outcome,
        ArchiveOutcome::Refused {
            reason: ArchiveRefusal::HumanOwned
        }
    );
    let after = current(&f, id);
    assert_eq!(after.run, run);
    assert!(!after.archived);
    assert!(after.phase.is_live());
    assert_eq!(refused.control.owner, ControlOwner::Human);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn parent_exit_after_owned_stop_leaves_pending_for_current_owner_without_second_stop() {
    parent_exit_before_archive_filing(false, false);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn parent_exit_after_final_authority_check_prevents_commit_then_current_owner_files() {
    parent_exit_before_archive_filing(true, false);
}

#[cfg(feature = "acceptance-diagnostics")]
fn parent_exit_before_archive_filing(final_parent_pin: bool, remove_parent: bool) {
    let f = OwnedFollowup::new();
    let signals = count_owned_child_terms(&f);
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    let delivered = declare(
        &f,
        &declaration(&child, "delivered", None, RetentionChoice::ArchiveEligible),
    );
    let Response::CreatedSession(successor) =
        f.live.request(Request::CreateSession(CreateSessionRequest {
            project: PROJECT.into(),
            workspace: f.workspace.clone(),
            name: "delivery-successor".into(),
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
        panic!("successor");
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while current(&f, successor.id).agent.is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let targets: Vec<WorkflowParent> =
        serde_json::from_value(f.tool("ovrcr_parents", serde_json::json!({}))).unwrap();
    let target = targets
        .into_iter()
        .find(|p| p.session == successor.id)
        .unwrap();
    let request = archive_request(&child, "parent-exits", reference(&delivered, "delivered"));
    let wire = DelegationRequest::Archive {
        auth: auth(&f),
        archive: request.clone(),
    };
    let socket = f.live.socket.clone();
    let fault = f.live.root.path().join("fault");
    let parent = current(&f, f.parent);
    let disconnects = final_parent_pin.then(|| {
        SupervisorDisconnectGates::arm(
            f.live.root.path(),
            &[child.identity.session.unwrap(), f.parent],
        )
    });
    let mut dashboard = connect_server(&f.live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    assert!(matches!(
        client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    let gate = "archive-after-stop";
    fs::write(&fault, format!("{gate}|parent-exits|0")).unwrap();
    let action = std::thread::spawn(move || {
        let mut stream = connect_server(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        client::request(&mut stream, 777, Request::Delegation(Box::new(wire))).unwrap()
    });
    wait_exact_fault_marker(&fault.with_extension("hit"), gate);
    if final_parent_pin {
        fs::copy(
            fault.with_extension("hit"),
            f.live.root.path().join("archive-after-stop.hit"),
        )
        .unwrap();
        // Child Exited is already applied. The actual dispatcher acknowledgement
        // drains its persistence/publication before final filing takes retained.
        assert_eq!(
            dashboard_view(
                &mut dashboard,
                2,
                Request::SetView {
                    view: DashboardView {
                        revision: 1,
                        panes: vec![],
                        focused: None
                    }
                }
            )
            .unwrap(),
            Response::Ok
        );
        fs::remove_file(fault.with_extension("hit")).unwrap();
        fs::write(&fault, "archive-before-parent-pin|parent-exits|0").unwrap();
        fs::write(fault.with_extension("release"), "release").unwrap();
        wait_exact_fault_marker(&fault.with_extension("hit"), "archive-before-parent-pin");
        // Preserve the first acknowledgement before reusing its owned marker path.
        let original_hit = f.live.root.path().join("archive-before-parent-pin.hit");
        fs::copy(fault.with_extension("hit"), &original_hit).unwrap();
        assert_eq!(
            fs::read(&original_hit).unwrap(),
            b"archive-before-parent-pin"
        );
        fs::remove_file(fault.with_extension("hit")).unwrap();
        fs::write(
            &fault,
            format!("session-exit-applied|{}-{}|0", f.parent.0, parent.run.0),
        )
        .unwrap();
    }
    let parent_pid = parent.pid.expect("owned parent pid") as i32;
    f.live.own_group(parent_pid);
    if remove_parent {
        assert!(
            !final_parent_pin,
            "public removal requires the unlocked after-stop gate"
        );
    }
    assert_eq!(unsafe { libc::kill(-parent_pid, libc::SIGKILL) }, 0);
    if final_parent_pin {
        // Inspect would wait for retained, which final filing still owns. The
        // dispatcher acknowledges actual state application before taking retained.
        wait_exact_fault_marker(&fault.with_extension("hit"), "session-exit-applied");
        assert!(
            !fault.exists(),
            "only the exact parent id/run consumes the acknowledgement rule"
        );
        assert_eq!(
            fs::read(f.live.root.path().join("archive-before-parent-pin.hit")).unwrap(),
            b"archive-before-parent-pin"
        );
        assert!(
            !action.is_finished(),
            "filing must still be held at the actual parent exit acknowledgement"
        );
        assert!(
            !fault.with_extension("release").exists(),
            "final gate has not been explicitly released"
        );
        disconnects.as_ref().unwrap().assert_no_timeout();
    } else {
        let deadline = Instant::now() + Duration::from_secs(10);
        while current(&f, f.parent).phase.is_live() {
            assert!(
                Instant::now() < deadline,
                "parent PTY exit must publish before filing"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if remove_parent {
        assert!(live::wait_group_absent(parent_pid, Duration::from_secs(3)));
        assert_eq!(
            f.live.request(Request::AcknowledgeSessionStopped {
                session: f.parent,
                expected_run: parent.run,
            }),
            Response::Ok
        );
        assert_eq!(
            f.live.request(Request::RemoveSession { session: f.parent }),
            Response::Ok
        );
        let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
            panic!("Inspect remains responsive after parent removal");
        };
        assert!(
            sessions.iter().all(|session| session.id != f.parent),
            "public removal must actually remove the old parent"
        );
    }
    fs::write(fault.with_extension("release"), "release").unwrap();
    let Response::Delegation(pending) = action.join().unwrap() else {
        panic!("pending after source loss");
    };
    if let Some(disconnects) = &disconnects {
        disconnects.release();
        disconnects.wait_enqueued();
    }
    let original = receipt(&pending, "parent-exits").clone();
    assert_eq!(original.outcome, ArchiveOutcome::StoppedArchivePending);
    let stopped_signals = assert_one_owned_term(&signals);
    let id = child.identity.session.unwrap();
    let run = child.identity.run.unwrap();
    assert_eq!(stopped_row(&f, id), (run.0, true, 0));
    let transfer = WorkflowTransfer {
        request: "current-owner".into(),
        expected: delivered.authority.clone(),
        parent: target.session,
        parent_run: target.run,
        binding: target.binding,
        producer: target.producer,
    };
    assert!(matches!(
        dashboard_view(
            &mut dashboard,
            3,
            Request::DashboardWorkflow(DashboardWorkflowRequest::Transfer(Box::new(transfer)))
        )
        .unwrap(),
        Response::Workflows(_)
    ));
    let invoke = |name: &str, args: serde_json::Value, label: &str| {
        tool(
            &f.live,
            successor.id,
            name,
            args,
            &f.live.root.path().join(format!("successor-{label}.json")),
        )
    };
    invoke(
        "ovrcr_recover",
        serde_json::json!({"workflow":child.identity.workflow}),
        "recover",
    );
    let mut continuation = archive_request(&pending, "current-owner-files", request.delivery);
    continuation.continuation_of = Some("parent-exits".into());
    let archived: AssignmentSnapshot = serde_json::from_value(invoke(
        "ovrcr_archive",
        serde_json::to_value(&continuation).unwrap(),
        "archive",
    ))
    .unwrap();
    assert_archived(&archived, "current-owner-files");
    assert_eq!(receipt(&archived, "parent-exits"), &original);
    assert_eq!(stopped_row(&f, id), (run.0 + 1, true, 1));
    assert!(!current(&f, successor.id).archived);
    let replay: AssignmentSnapshot = serde_json::from_value(invoke(
        "ovrcr_archive",
        serde_json::to_value(&continuation).unwrap(),
        "replay",
    ))
    .unwrap();
    assert_eq!(replay.archive_receipts, archived.archive_receipts);
    assert_eq!(
        fs::read(&signals).unwrap(),
        stopped_signals,
        "current-owner continuation never signals again"
    );
}

#[cfg(feature = "acceptance-diagnostics")]
pub(super) fn count_owned_child_terms(f: &RecoveryFixture) -> std::path::PathBuf {
    let log = f.live.root.path().join("owned-child-sigterm.jsonl");
    let preload = f.live.root.path().join("record-owned-sigterm.mjs");
    fs::write(&preload, format!(
        "import {{appendFileSync}} from 'node:fs';\nprocess.on('SIGTERM',()=>{{appendFileSync({},JSON.stringify({{pid:process.pid}})+'\\n');process.exit(0);}});\n",
        serde_json::to_string(&log).unwrap()
    )).unwrap();
    let quoted = format!("'{}'", preload.to_str().unwrap().replace('\'', "'\"'\"'"));
    let provider = include_str!("../fixtures/pi/research_pi.sh")
        .replace("exec node ", &format!("exec node --import {quoted} "));
    let pi = f.live.claude.command.parent().unwrap().join("pi");
    fs::write(&pi, provider).unwrap();
    fs::set_permissions(pi, fs::Permissions::from_mode(0o700)).unwrap();
    log
}

#[cfg(feature = "acceptance-diagnostics")]
fn assert_one_owned_term(log: &Path) -> Vec<u8> {
    let bytes = fs::read(log).expect("actual native SIGTERM handler must record delivery");
    let entries: Vec<serde_json::Value> = std::str::from_utf8(&bytes)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "one SIGTERM reaches the instrumented owned child"
    );
    assert!(entries[0]["pid"].as_u64().unwrap() > 0);
    bytes
}

#[cfg(feature = "acceptance-diagnostics")]
fn stored_assignment_bytes(f: &RecoveryFixture, handle: &str) -> Vec<u8> {
    database(f)
        .query_row(
            "SELECT CAST(metadata AS BLOB) FROM delegations WHERE handle=?1",
            [handle],
            |row| row.get(0),
        )
        .unwrap()
}

#[cfg(feature = "acceptance-diagnostics")]
fn near_full_assignment(f: &RecoveryFixture, child: &AssignmentSnapshot) {
    let mut record = persisted(f, &child.handle);
    for index in 0..62 {
        let request = format!("retained-budget-control-{index}");
        record["control_intents"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "request":request,"operation":"Takeover","digest":"a".repeat(64)
            }));
        record["receipts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "request":request,"operation":"Takeover","identity":child.identity,
                "result_version":child.result_version,"outcome":"Applied","input":null,"reason":null
            }));
    }
    record["reason"] = serde_json::json!("");
    let padding = 240_000 - serde_json::to_vec(&record).unwrap().len();
    record["reason"] = serde_json::json!("x".repeat(padding));
    let bytes = serde_json::to_vec(&record).unwrap();
    assert_eq!(bytes.len(), 240_000);
    database(f)
        .execute(
            "UPDATE delegations SET metadata=?1 WHERE handle=?2",
            rusqlite::params![std::str::from_utf8(&bytes).unwrap(), child.handle],
        )
        .unwrap();
    let decoded = f.reconcile(&child.handle);
    assert_eq!(
        decoded.execution_attempts,
        Some(1),
        "bounded fixture keeps paired existing operation evidence valid"
    );
    assert_eq!(decoded.receipts.len(), child.receipts.len() + 62);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn production_near_full_archive_keeps_reserved_continuation_across_rollback_and_lost_reply() {
    let f = OwnedFollowup::new();
    let signals = count_owned_child_terms(&f);
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    f.live.own_group(
        current(&f, child.identity.session.unwrap())
            .pid
            .expect("owned child process group") as i32,
    );
    let delivered = declare(
        &f,
        &declaration(
            &child,
            "bounded-delivery",
            None,
            RetentionChoice::ArchiveEligible,
        ),
    );
    near_full_assignment(&f, &child);
    let request = archive_request(
        &child,
        "bounded-original",
        reference(&delivered, "bounded-delivery"),
    );
    let fault = f.live.root.path().join("fault");
    fs::write(&fault, "archive-after-stage|bounded-original|0").unwrap();
    let failed = f.tool("ovrcr_archive", serde_json::to_value(&request).unwrap());
    assert_eq!(failed["transport"], "Unknown");
    let pending = f.reconcile(&child.handle);
    let original = receipt(&pending, "bounded-original").clone();
    assert_eq!(original.outcome, ArchiveOutcome::StoppedArchivePending);
    let original_bytes = stored_assignment_bytes(&f, &child.handle);
    assert!(original_bytes.len() > 240_000 && original_bytes.len() < 262_144);
    let row: serde_json::Value = serde_json::from_slice(&original_bytes).unwrap();
    assert_eq!(
        row["control_intents"].as_array().unwrap().len()
            + row["archive_receipts"].as_array().unwrap().len(),
        63
    );
    let signal_bytes = assert_one_owned_term(&signals);
    let id = child.identity.session.unwrap();
    let run = child.identity.run.unwrap();
    assert_eq!(stopped_row(&f, id), (run.0, true, 0));

    // A different control operation cannot spend the reserved final slot.
    let blocked = tool_reply(
        &f.live,
        f.parent,
        "ovrcr_interrupt",
        serde_json::json!({
            "handle":child.handle,"request":"cannot-spend-reserved-slot","expected_identity":child.identity,
            "expected_result_version":child.result_version,"expected_control_epoch":pending.control.epoch
        }),
        &f.live.root.path().join("reserved-slot-refusal.json"),
    );
    assert!(
        blocked.get("error").is_some(),
        "full protected journal must refuse before a native interrupt: {blocked}"
    );
    assert_eq!(stored_assignment_bytes(&f, &child.handle), original_bytes);

    let mut continuation =
        archive_request(&pending, "bounded-after-stage", request.delivery.clone());
    continuation.continuation_of = Some("bounded-original".into());
    for (point, label) in [
        ("archive-after-stage", "bounded-after-stage"),
        ("archive-before-commit", "bounded-before-commit"),
    ] {
        continuation.request = label.into();
        fs::write(&fault, format!("{point}|{label}|0")).unwrap();
        let failed = f.tool(
            "ovrcr_archive",
            serde_json::to_value(&continuation).unwrap(),
        );
        assert_eq!(failed["transport"], "Unknown");
        assert_eq!(
            stored_assignment_bytes(&f, &child.handle),
            original_bytes,
            "failed atomic filing consumes neither protected bytes nor slot"
        );
        assert_eq!(stopped_row(&f, id), (run.0, true, 0));
        let cache = current(&f, id);
        assert_eq!(cache.run, run);
        assert!(!cache.archived);
        assert_eq!(fs::read(&signals).unwrap(), signal_bytes);
    }
    continuation.request = "bounded-lost-final-reply".into();
    f.proxy.arm(ControlRule {
        operation: "Archive",
        loss: 1,
        fault: None,
    });
    let lost = f.tool(
        "ovrcr_archive",
        serde_json::to_value(&continuation).unwrap(),
    );
    assert_eq!(lost["transport"], "Unknown");
    let recovered: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_archive",
        serde_json::json!({"handle":child.handle,"request":continuation.request}),
    ))
    .unwrap();
    assert_archived(&recovered, "bounded-lost-final-reply");
    assert_eq!(receipt(&recovered, "bounded-original"), &original);
    assert_eq!(recovered.archive_receipts.len(), 2);
    let final_record = persisted(&f, &child.handle);
    assert_eq!(
        final_record["control_intents"].as_array().unwrap().len()
            + final_record["archive_receipts"].as_array().unwrap().len(),
        64
    );
    assert_eq!(stopped_row(&f, id), (run.0 + 1, true, 1));
    assert_eq!(
        fs::read(&signals).unwrap(),
        signal_bytes,
        "continuations and reconciliation deliver no second SIGTERM"
    );
    let replay = archive(&f, &request);
    assert_eq!(replay.archive_receipts, recovered.archive_receipts);
    assert_eq!(stopped_row(&f, id), (run.0 + 1, true, 1));
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn production_stopped_proof_transaction_failure_keeps_unknown_and_never_authorizes_continuation() {
    let f = OwnedFollowup::new();
    let signals = count_owned_child_terms(&f);
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    f.live.own_group(
        current(&f, child.identity.session.unwrap())
            .pid
            .expect("owned child process group") as i32,
    );
    let evidence = fs::read(&child.result_path).unwrap();
    let delivered = declare(
        &f,
        &declaration(
            &child,
            "stop-proof-delivery",
            None,
            RetentionChoice::ArchiveEligible,
        ),
    );
    database(&f).execute_batch("CREATE TRIGGER reject_archive_stop_proof BEFORE UPDATE OF metadata ON delegations WHEN json_extract(NEW.metadata,'$.archive_receipts[0].outcome')='StoppedArchivePending' BEGIN SELECT RAISE(FAIL,'owned stopped-proof receipt failure'); END;").unwrap();
    let request = archive_request(
        &child,
        "stop-proof-fails",
        reference(&delivered, "stop-proof-delivery"),
    );
    let failed = f.tool("ovrcr_archive", serde_json::to_value(&request).unwrap());
    assert_eq!(failed["transport"], "Unknown");
    let pending = f.reconcile(&child.handle);
    assert_eq!(
        receipt(&pending, "stop-proof-fails").outcome,
        ArchiveOutcome::StopUnknown
    );
    let retained = stored_assignment_bytes(&f, &child.handle);
    let signal_bytes = assert_one_owned_term(&signals);
    let id = child.identity.session.unwrap();
    let run = child.identity.run.unwrap();
    assert_eq!(
        stopped_row(&f, id),
        (run.0, false, 0),
        "receipt failure rolls back the staged stopped bit too"
    );
    let cache = current(&f, id);
    assert_eq!(cache.run, run);
    assert!(!cache.archived);
    assert!(
        !cache.phase.is_live(),
        "an observed process exit cannot substitute for committed owned-stop proof"
    );
    database(&f)
        .execute_batch("DROP TRIGGER reject_archive_stop_proof;")
        .unwrap();
    assert_eq!(
        archive(&f, &request).archive_receipts,
        pending.archive_receipts
    );
    let mut continuation = archive_request(&pending, "cannot-upgrade-unknown", request.delivery);
    continuation.continuation_of = Some("stop-proof-fails".into());
    let refused = f.tool(
        "ovrcr_archive",
        serde_json::to_value(&continuation).unwrap(),
    );
    assert_eq!(refused["transport"], "Unknown");
    assert_eq!(
        stored_assignment_bytes(&f, &child.handle),
        retained,
        "failed certification cannot be inferred or retried from process absence"
    );
    assert_eq!(stopped_row(&f, id), (run.0, false, 0));
    assert_eq!(fs::read(&signals).unwrap(), signal_bytes);
    assert_eq!(fs::read(&child.result_path).unwrap(), evidence);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn production_transfer_after_archive_admission_stops_only_captured_child_then_current_owner_files()
{
    let f = OwnedFollowup::new();
    let signals = count_owned_child_terms(&f);
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    f.live.own_group(
        current(&f, child.identity.session.unwrap())
            .pid
            .expect("owned child process group") as i32,
    );
    let delivered = declare(
        &f,
        &declaration(
            &child,
            "transfer-delivery",
            None,
            RetentionChoice::ArchiveEligible,
        ),
    );
    let Response::CreatedSession(successor) =
        f.live.request(Request::CreateSession(CreateSessionRequest {
            project: PROJECT.into(),
            workspace: f.workspace.clone(),
            name: "archive-transfer-target".into(),
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
        panic!("successor");
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while current(&f, successor.id).agent.is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let parents: Vec<WorkflowParent> =
        serde_json::from_value(f.tool("ovrcr_parents", serde_json::json!({}))).unwrap();
    let target = parents
        .into_iter()
        .find(|parent| parent.session == successor.id)
        .unwrap();
    let request = archive_request(
        &child,
        "admitted-before-transfer",
        reference(&delivered, "transfer-delivery"),
    );
    let wire = DelegationRequest::Archive {
        auth: auth(&f),
        archive: request.clone(),
    };
    let socket = f.live.socket.clone();
    let fault = f.live.root.path().join("fault");
    fs::write(&fault, "archive-after-admission|admitted-before-transfer|0").unwrap();
    let action = std::thread::spawn(move || {
        let mut stream = connect_server(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        client::request(&mut stream, 777, Request::Delegation(Box::new(wire))).unwrap()
    });
    wait_file(&fault.with_extension("hit"));
    assert!(
        !signals.exists(),
        "admission precedes the first native stop signal"
    );
    let before = persisted(&f, &child.handle);
    assert_eq!(before["archive_receipts"][0]["outcome"], "StopUnknown");
    let mut dashboard = connect_server(&f.live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    assert!(matches!(
        client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    let transfer = WorkflowTransfer {
        request: "transfer-during-owned-stop".into(),
        expected: delivered.authority.clone(),
        parent: target.session,
        parent_run: target.run,
        binding: target.binding,
        producer: target.producer,
    };
    let Response::Workflows(transferred) = dashboard_view(
        &mut dashboard,
        2,
        Request::DashboardWorkflow(DashboardWorkflowRequest::Transfer(Box::new(transfer))),
    )
    .unwrap() else {
        panic!("transfer");
    };
    assert_eq!(transferred[0].authority.parent, successor.id);
    fs::write(fault.with_extension("release"), "release").unwrap();
    let Response::Delegation(pending) = action.join().unwrap() else {
        panic!("pending captured stop");
    };
    let original = receipt(&pending, "admitted-before-transfer").clone();
    assert_eq!(original.outcome, ArchiveOutcome::StoppedArchivePending);
    assert_eq!(original.intent.authority, delivered.authority);
    assert_eq!(original.intent.request.expected_identity, child.identity);
    let signal_bytes = assert_one_owned_term(&signals);
    let id = child.identity.session.unwrap();
    let run = child.identity.run.unwrap();
    assert_eq!(stopped_row(&f, id), (run.0, true, 0));
    assert!(current(&f, successor.id).phase.is_live());
    assert!(current(&f, f.parent).phase.is_live());
    let retained = stored_assignment_bytes(&f, &child.handle);
    let mut continuation = archive_request(&pending, "retired-owner-cannot-file", request.delivery);
    continuation.continuation_of = Some("admitted-before-transfer".into());
    let retired = f.tool(
        "ovrcr_archive",
        serde_json::to_value(&continuation).unwrap(),
    );
    assert_eq!(retired["transport"], "Unknown");
    assert_eq!(stored_assignment_bytes(&f, &child.handle), retained);
    let invoke = |name: &str, args: serde_json::Value, label: &str| {
        tool(
            &f.live,
            successor.id,
            name,
            args,
            &f.live
                .root
                .path()
                .join(format!("transfer-target-{label}.json")),
        )
    };
    let recovered = invoke(
        "ovrcr_recover",
        serde_json::json!({"workflow":child.identity.workflow}),
        "recover",
    );
    assert_eq!(recovered["authority"]["parent"], successor.id.0);
    continuation.request = "new-owner-wrong-barrier".into();
    continuation.expected_control_epoch += 1;
    let wrong = invoke(
        "ovrcr_archive",
        serde_json::to_value(&continuation).unwrap(),
        "wrong-barrier",
    );
    assert_eq!(wrong["transport"], "Unknown");
    assert_eq!(stored_assignment_bytes(&f, &child.handle), retained);
    continuation.request = "new-owner-files-stopped".into();
    continuation.expected_control_epoch = pending.control.epoch;
    let archived: AssignmentSnapshot = serde_json::from_value(invoke(
        "ovrcr_archive",
        serde_json::to_value(&continuation).unwrap(),
        "files",
    ))
    .unwrap();
    assert_archived(&archived, "new-owner-files-stopped");
    assert_eq!(receipt(&archived, "admitted-before-transfer"), &original);
    assert_eq!(
        receipt(&archived, "new-owner-files-stopped")
            .intent
            .authority,
        transferred[0].authority
    );
    assert_eq!(stopped_row(&f, id), (run.0 + 1, true, 1));
    assert_eq!(fs::read(&signals).unwrap(), signal_bytes);
    assert!(current(&f, successor.id).phase.is_live());
}

#[cfg(feature = "acceptance-diagnostics")]
fn wait_exact_fault_marker(path: &Path, point: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if fs::read(path).ok().as_deref() == Some(point.as_bytes()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "missing exact {point} acknowledgement at {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(feature = "acceptance-diagnostics")]
struct SupervisorDisconnectGates {
    gates: Vec<(SessionId, std::path::PathBuf)>,
    archive_release: std::path::PathBuf,
}

#[cfg(feature = "acceptance-diagnostics")]
impl SupervisorDisconnectGates {
    fn arm(root: &Path, sessions: &[SessionId]) -> Self {
        let gates = sessions
            .iter()
            .map(|id| {
                let gate = root.join(format!("supervisor-disconnect-{}", id.0));
                fs::write(gate.with_extension("hold"), "hold").unwrap();
                (*id, gate)
            })
            .collect();
        Self {
            gates,
            archive_release: root.join("fault.release"),
        }
    }

    fn release(&self) {
        for (_, gate) in &self.gates {
            let _ = fs::write(gate.with_extension("release"), "release");
        }
    }

    fn assert_no_timeout(&self) {
        for (_, gate) in &self.gates {
            assert!(
                !gate.with_extension("timeout").exists(),
                "disconnect gate timed out"
            );
        }
    }

    fn wait_enqueued(&self) {
        for (id, gate) in &self.gates {
            wait_exact_fault_marker(&gate.with_extension("hit"), &id.0.to_string());
            wait_exact_fault_marker(&gate.with_extension("enqueued"), &id.0.to_string());
        }
        self.assert_no_timeout();
    }
}

#[cfg(feature = "acceptance-diagnostics")]
impl Drop for SupervisorDisconnectGates {
    fn drop(&mut self) {
        self.release();
        let _ = fs::write(&self.archive_release, "release");
    }
}

#[test]
fn absent_transfer_parent_refuses_boundedly_and_keeps_archive_recovery_owner() {
    // Raw fixture permits explicit owned cleanup if the inherited control path
    // times out; an unwinding OwnedFollowup must not query a blocked Server.
    let mut f = RecoveryFixture::new();
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    let authority = super::parent_recovery::workflow(&f).authority;
    let groups = f.live.session_groups();
    for group in &groups {
        f.live.own_group(*group);
    }
    let server = f.live.server_pid().unwrap();
    let mut dashboard = connect_server(&f.live.socket).unwrap();
    dashboard
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    dashboard
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    assert!(matches!(
        client::request(&mut dashboard, 1, Request::DashboardHello).unwrap(),
        Response::Hierarchy(_)
    ));
    let before = persisted(&f, &child.handle);
    let owner_auth = auth(&f);
    // The remainder uses direct public control. Retire the fixture proxy while
    // its Server is healthy, before an intentional failure shutdown can cut a read.
    f.proxy.retire();
    let transfer = WorkflowTransfer {
        request: "missing-continuation-owner".into(),
        expected: authority.clone(),
        parent: SessionId(u64::MAX),
        parent_run: SessionRunId(1),
        binding: authority.binding.clone(),
        producer: authority.producer.clone(),
    };
    let response = dashboard_view(
        &mut dashboard,
        2,
        Request::DashboardWorkflow(DashboardWorkflowRequest::Transfer(Box::new(transfer))),
    );
    if response.is_err() {
        // Preserve a bounded red result without leaving the task-owned deadlock
        // or triggering recursive fixture requests during unwinding.
        assert_eq!(unsafe { libc::kill(server as i32, libc::SIGKILL) }, 0);
        for group in &groups {
            let _ = unsafe { libc::kill(-*group, libc::SIGKILL) };
            assert!(live::wait_group_absent(*group, Duration::from_secs(3)));
        }
        let _ = f.live.join_within(Duration::from_secs(5));
    }
    assert!(
        matches!(response, Ok(Response::Error { .. })),
        "absent proposed parent must refuse without relocking retained: {response:?}"
    );
    assert_eq!(persisted(&f, &child.handle), before);
    assert!(matches!(
        f.live.request(Request::Inspect),
        Response::Inventory { .. }
    ));
    let Response::WorkflowRecovery(recovery) =
        f.live
            .request(Request::Workflow(Box::new(WorkflowRequest::Recover {
                auth: owner_auth,
                workflow: Some(authority.workflow.clone()),
            })))
    else {
        panic!("current owner recovery remains responsive");
    };
    assert_eq!(recovery.workflows[0].authority, authority);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn removed_parent_after_owned_stop_leaves_pending_for_one_current_owner_continuation() {
    parent_exit_before_archive_filing(false, true);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn pending_accepted_unknown_and_partial_answers_refuse_archive_without_stop_or_advancement() {
    for (point, limit, expected) in [
        ("answer-before-input", 0, DispatchOutcome::Accepted),
        ("input", usize::MAX, DispatchOutcome::Unknown),
        ("input", 7, DispatchOutcome::Partial),
    ] {
        let f = OwnedFollowup::new();
        let signals = count_owned_child_terms(&f);
        let child = super::questions::start(&f);
        f.proxy.arm(ControlRule {
            operation: "AnswerQuestion",
            loss: 2,
            fault: Some((point, limit)),
        });
        let answered: AssignmentSnapshot = serde_json::from_value(super::questions::answer(
            &f,
            &child,
            &super::questions::reference(&child),
        ))
        .unwrap();
        assert_eq!(
            answered
                .receipts
                .iter()
                .find(|r| r.operation == DispatchOperation::AnswerQuestion)
                .unwrap()
                .outcome,
            expected
        );
        let delivered = declare(
            &f,
            &declaration(
                &answered,
                "answer-pending-delivery",
                None,
                RetentionChoice::ArchiveEligible,
            ),
        );
        let before = persisted(&f, &child.handle);
        let input = f.byte_count(&answered);
        let id = child.identity.session.unwrap();
        let run = child.identity.run.unwrap();
        let stopped = stopped_row(&f, id);
        let request = archive_request(
            &answered,
            "answer-pending-archive",
            reference(&delivered, "answer-pending-delivery"),
        );
        let refused = archive(&f, &request);
        assert_eq!(
            receipt(&refused, "answer-pending-archive").outcome,
            ArchiveOutcome::Refused {
                reason: ArchiveRefusal::UnresolvedControl
            }
        );
        assert_eq!(refused.control, answered.control, "{expected:?}");
        assert_eq!(refused.obligation_revision, answered.obligation_revision);
        assert_eq!(refused.receipts, answered.receipts);
        assert_eq!(refused.result_version, answered.result_version);
        assert_eq!(stopped_row(&f, id), stopped);
        assert_eq!(current(&f, id).run, run);
        assert!(!current(&f, id).archived);
        assert_eq!(f.byte_count(&answered), input);
        assert!(
            fs::read(&signals).unwrap_or_default().is_empty(),
            "no SIGTERM before answer resolution"
        );
        let after = persisted(&f, &child.handle);
        assert_eq!(after["answers"], before["answers"]);
        assert_eq!(after["questions"], before["questions"]);
        assert_eq!(after["control"], before["control"]);
        assert!(matches!(
            f.live.request(Request::Inspect),
            Response::Inventory { .. }
        ));
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn delivery_and_answer_request_collisions_refuse_in_both_directions_before_effect() {
    for answer_first in [true, false] {
        let f = OwnedFollowup::new();
        let child = super::questions::start(&f);
        let question = super::questions::reference(&child);
        if answer_first {
            let reply = super::questions::answer(&f, &child, &question);
            let applied: AssignmentSnapshot = serde_json::from_value(reply.clone()).unwrap();
            let request = reply["request"].as_str().unwrap();
            let before = persisted(&f, &child.handle);
            let input = f.byte_count(&applied);
            let denied = f.tool("ovrcr_deliver", serde_json::json!({"declaration": declaration(&applied, request, None, RetentionChoice::ArchiveEligible)}));
            assert_eq!(denied["dispatch"], "Refused", "{denied}");
            assert_eq!(persisted(&f, &child.handle), before);
            assert_eq!(f.byte_count(&applied), input);
            assert!(super::parent_recovery::workflow(&f).deliveries.is_empty());
        } else {
            let delivered = declare(
                &f,
                &declaration(
                    &child,
                    "delivery-before-answer",
                    None,
                    RetentionChoice::ArchiveEligible,
                ),
            );
            let before = persisted(&f, &child.handle);
            let input = f.byte_count(&child);
            let response = f.live.request(Request::Delegation(Box::new(
                DelegationRequest::AnswerQuestion {
                    auth: auth(&f),
                    handle: child.handle.clone(),
                    expected_parent_epoch: delivered.authority.epoch,
                    answer: QuestionAnswer {
                        schema: 1,
                        request: "delivery-before-answer".into(),
                        question,
                        control_epoch: child.control.epoch,
                        basis: AnswerBasis::ExistingAuthority,
                        scope: "Original README scope".into(),
                        answer: "Use the Introduction".into(),
                    },
                },
            )));
            assert!(matches!(response, Response::Error { .. }), "{response:?}");
            assert_eq!(persisted(&f, &child.handle), before);
            assert_eq!(f.byte_count(&child), input);
            assert!(
                !child
                    .brief
                    .parent()
                    .unwrap()
                    .join("answer-delivery-before-answer.json")
                    .exists()
            );
            assert_eq!(
                super::parent_recovery::workflow(&f).deliveries,
                delivered.deliveries
            );
        }
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn removed_parent_before_archive_refuses_boundedly_and_inspect_remains_responsive() {
    let f = OwnedFollowup::new();
    let signals = count_owned_child_terms(&f);
    let started: AssignmentSnapshot = serde_json::from_value(f.start()).unwrap();
    let child = complete(&f, &started);
    let delivered = declare(
        &f,
        &declaration(
            &child,
            "before-remove",
            None,
            RetentionChoice::ArchiveEligible,
        ),
    );
    let request = archive_request(
        &child,
        "removed-parent-archive",
        reference(&delivered, "before-remove"),
    );
    let owner = auth(&f);
    let parent = current(&f, f.parent);
    let parent_pid = parent.pid.expect("owned parent pid") as i32;
    f.live.own_group(parent_pid);
    assert_eq!(unsafe { libc::kill(-parent_pid, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    while current(&f, f.parent).phase.is_live() {
        assert!(
            Instant::now() < deadline,
            "parent exit must precede removal"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(live::wait_group_absent(parent_pid, Duration::from_secs(3)));
    assert_eq!(
        f.live.request(Request::AcknowledgeSessionStopped {
            session: f.parent,
            expected_run: parent.run,
        }),
        Response::Ok
    );
    assert_eq!(
        f.live.request(Request::RemoveSession { session: f.parent }),
        Response::Ok
    );
    let Response::Inventory { sessions, .. } = f.live.request(Request::Inspect) else {
        panic!("Inspect remains responsive after parent removal");
    };
    assert!(sessions.iter().all(|session| session.id != f.parent));
    let before = persisted(&f, &child.handle);
    let row = stopped_row(&f, child.identity.session.unwrap());
    let mut stream = connect_server(&f.live.socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let response = client::request(
        &mut stream,
        73,
        Request::Delegation(Box::new(DelegationRequest::Archive {
            auth: owner,
            archive: request,
        })),
    )
    .unwrap();
    assert!(matches!(response, Response::Error { .. }), "{response:?}");
    assert_eq!(persisted(&f, &child.handle), before);
    assert_eq!(stopped_row(&f, child.identity.session.unwrap()), row);
    assert!(fs::read(&signals).unwrap_or_default().is_empty());
    assert!(matches!(
        f.live.request(Request::Inspect),
        Response::Inventory { .. }
    ));
}

#[cfg(feature = "acceptance-diagnostics")]
pub(super) fn archive_full_question_history(
    f: &RecoveryFixture,
    child: &AssignmentSnapshot,
    signals: &Path,
) {
    assert_eq!(child.questions.len(), 64);
    assert_eq!(
        child
            .receipts
            .iter()
            .filter(|r| r.operation == DispatchOperation::AnswerQuestion)
            .count(),
        64
    );
    let root = child.brief.parent().unwrap();
    let private: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("question-")
                || p.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("answer-")
        })
        .map(|p| {
            let bytes = fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect();
    assert_eq!(private.len(), 128);
    let completed = complete(f, child);
    let result = fs::read(&completed.result_path).unwrap();
    let delivered = declare(
        f,
        &declaration(
            &completed,
            "all-questions-delivered",
            None,
            RetentionChoice::ArchiveEligible,
        ),
    );
    near_full_assignment(f, &completed);
    let request = archive_request(
        &completed,
        "mixed-history-stop",
        reference(&delivered, "all-questions-delivered"),
    );
    fs::write(
        f.live.root.path().join("fault"),
        "archive-after-stage|mixed-history-stop|0",
    )
    .unwrap();
    let failed = f.tool("ovrcr_archive", serde_json::to_value(&request).unwrap());
    assert_eq!(failed["transport"], "Unknown");
    let pending = f.reconcile(&child.handle);
    let original = receipt(&pending, "mixed-history-stop").clone();
    assert_eq!(original.outcome, ArchiveOutcome::StoppedArchivePending);
    let one_stop = assert_one_owned_term(signals);
    let latest = child
        .questions
        .iter()
        .find(|q| q.current)
        .unwrap()
        .reference
        .clone();
    let handled: AssignmentSnapshot = serde_json::from_value(f.tool(
        "ovrcr_mark_handled",
        serde_json::json!({"handle":child.handle,"publication":latest.publication()}),
    ))
    .unwrap();
    assert_eq!(receipt(&handled, "mixed-history-stop"), &original);
    let frozen = stored_assignment_bytes(f, &child.handle);
    let refused = f.tool(
        "ovrcr_answer_question",
        super::questions::answer_args(&pending, &latest),
    );
    assert_eq!(refused["dispatch"], "Refused");
    assert_eq!(stored_assignment_bytes(f, &child.handle), frozen);
    let mut continuation =
        archive_request(&pending, "mixed-history-files", request.delivery.clone());
    continuation.continuation_of = Some("mixed-history-stop".into());
    let archived = archive(f, &continuation);
    assert_archived(&archived, "mixed-history-files");
    assert_eq!(receipt(&archived, "mixed-history-stop"), &original);
    assert_eq!(archived.questions.len(), 64);
    assert_eq!(
        archived
            .receipts
            .iter()
            .filter(|r| r.operation == DispatchOperation::AnswerQuestion)
            .count(),
        64
    );
    let final_row = persisted(f, &child.handle);
    assert_eq!(
        final_row["control_intents"].as_array().unwrap().len()
            + final_row["archive_receipts"].as_array().unwrap().len(),
        64
    );
    for (path, bytes) in private {
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "private question/answer evidence survives archive"
        );
    }
    assert_eq!(fs::read(&completed.result_path).unwrap(), result);
    assert_eq!(fs::read(signals).unwrap(), one_stop);
}

//! Real registered tools, managed PTYs, immutable no-commit Git artifacts and repair.
use super::*;
use sha2::{Digest, Sha256};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn publication_snapshot(
    fixture: &RecoveryFixture,
    snapshot: &AssignmentSnapshot,
) -> AssignmentSnapshot {
    // A supplied immutable result remains the caller's evidence, even when stale or foreign.
    if snapshot.publication.is_some() || snapshot.submission.is_some() {
        return snapshot.clone();
    }
    let collected: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle": snapshot.handle}),
    ))
    .unwrap();
    assert_eq!(collected.handle, snapshot.handle);
    assert_eq!(collected.identity, snapshot.identity);
    assert_eq!(collected.result_version, snapshot.result_version);
    collected
}

fn target(fixture: &RecoveryFixture, snapshot: &AssignmentSnapshot) -> CodingTargetReference {
    let published = publication_snapshot(fixture, snapshot);
    let contract = snapshot
        .brief
        .parent()
        .unwrap()
        .join(format!("contract-v{}.json", snapshot.identity.contract));
    CodingTargetReference {
        assignment: snapshot.identity.clone(),
        publication: published.publication.as_ref().unwrap().identity.clone(),
        candidate: published
            .submission
            .as_ref()
            .unwrap()
            .coding
            .as_ref()
            .unwrap()
            .candidate
            .clone()
            .unwrap(),
        source_contract_digest: digest(&fs::read(contract).unwrap()),
    }
}
fn source(kind: ReviewSourceKind, name: &str, content: &str) -> ReviewSource {
    ReviewSource {
        source: ReviewSourceReference {
            kind,
            reference: name.into(),
            digest: digest(content.as_bytes()),
        },
        content: content.into(),
    }
}
fn sources() -> Vec<ReviewSource> {
    vec![
        source(
            ReviewSourceKind::Method,
            "fixture://installed-code-review",
            "Standards uses repository rules before the Fowler smell baseline. Smells are judgment calls. Skip tooling-enforced rules. Review independently of Spec and the implementation conversation.",
        ),
        source(
            ReviewSourceKind::Standards,
            "fixture://AGENTS.md",
            "Preserve one server owner and private source artifacts.",
        ),
        source(
            ReviewSourceKind::Spec,
            "fixture://task",
            "calc.cjs must export add(a, b) returning a + b. add(2, 3) must equal 5.",
        ),
    ]
}
fn review(
    fixture: &RecoveryFixture,
    snapshot: &AssignmentSnapshot,
    operation: ReviewOperation,
) -> serde_json::Value {
    fixture.tool(
        "ovrcr_review",
        serde_json::json!({"handle":snapshot.handle,"operation":operation}),
    )
}
fn declared(
    fixture: &RecoveryFixture,
    snapshot: &AssignmentSnapshot,
    both: bool,
    limit: Option<RepairLimit>,
) -> AssignmentSnapshot {
    let documents = sources();
    let mut requirements = vec![ReviewRequirement {
        axis: ReviewAxis::Spec,
        sources: vec![documents[2].source.clone()],
    }];
    if both {
        requirements.push(ReviewRequirement {
            axis: ReviewAxis::Standards,
            sources: documents[..2]
                .iter()
                .map(|source| source.source.clone())
                .collect(),
        });
    }
    serde_json::from_value(review(
        fixture,
        snapshot,
        ReviewOperation::Declare(Box::new(ReviewDeclaration {
            request: format!("review-{}", snapshot.result_version),
            target: target(fixture, snapshot),
            requirements,
            sources: documents,
            repair_limit: limit,
            context_paths: vec![],
        })),
    ))
    .unwrap()
}
fn worker(fixture: &RecoveryFixture, mode: &str) -> AssignmentSnapshot {
    fs::write(
        fixture.live.repo.join("obsolete.txt"),
        "original base context\n",
    )
    .unwrap();
    fs::write(
        fixture.live.repo.join("executable.sh"),
        "#!/bin/sh\nexit 1\n",
    )
    .unwrap();
    live::git(
        &fixture.live.repo,
        &["add", "obsolete.txt", "executable.sh"],
    );
    live::git(&fixture.live.repo, &["commit", "-m", "fixture base"]);
    fs::write(fixture.live.root.path().join("allow-result"), "1").unwrap();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.tool("ovrcr_start", serde_json::json!({
        "project":PROJECT,"workspace":"review-worker","scope":format!("REVIEW_FIXTURE_{mode}"),"model_provider":"test","model":"fixture-model", "acceptance_evidence":["addition assertion and independent review"],
        "coding":{"requested_base":"main","branch":"coding/review-worker","authorized_paths":["README","calc.cjs","obsolete.txt","executable.sh","calc-link"],"required_checks":["addition"]}
    }))).unwrap();
    assert_eq!(started.dispatch, DispatchStatus::Applied, "{started:?}");
    super::followups::complete(fixture, &started)
}
fn axis(
    fixture: &RecoveryFixture,
    implementation: &AssignmentSnapshot,
    axis: ReviewAxis,
) -> AssignmentSnapshot {
    let selected: AssignmentSnapshot = serde_json::from_value(review(
        fixture,
        implementation,
        ReviewOperation::Assignment { axis: axis.clone() },
    ))
    .unwrap();
    let assignment = selected.review_assignment.unwrap();
    assert!(
        assignment
            .source_documents
            .iter()
            .all(|document| match axis {
                ReviewAxis::Spec => document.source.kind == ReviewSourceKind::Spec,
                ReviewAxis::Standards => document.source.kind != ReviewSourceKind::Spec,
            })
    );
    let started: AssignmentSnapshot = serde_json::from_value(fixture.tool("ovrcr_start", serde_json::json!({
        "project":PROJECT,"workspace":fixture.workspace,"scope":selected.review_scope.unwrap(),"review":assignment,
        "model_provider":"test","model":"fixture-model","acceptance_evidence":["typed exact-axis independent report"]
    }))).unwrap();
    assert_eq!(started.dispatch, DispatchStatus::Applied, "{started:?}");
    let completed = super::followups::complete(fixture, &started);
    let recorded: AssignmentSnapshot = serde_json::from_value(review(
        fixture,
        implementation,
        ReviewOperation::Collect {
            axis,
            reviewer: completed.handle.clone(),
        },
    ))
    .unwrap();
    assert!(
        recorded
            .review
            .unwrap()
            .evidence
            .iter()
            .any(|value| value.reviewer == completed.identity)
    );
    completed
}
fn repaired(
    fixture: &RecoveryFixture,
    snapshot: &AssignmentSnapshot,
    request: &str,
) -> AssignmentSnapshot {
    serde_json::from_value(review(
        fixture,
        snapshot,
        ReviewOperation::Repair {
            request: request.into(),
            expected_control_epoch: snapshot.control.epoch,
        },
    ))
    .unwrap()
}
fn collected_digest(fixture: &RecoveryFixture, snapshot: &AssignmentSnapshot) -> String {
    let collected = publication_snapshot(fixture, snapshot);
    assert_eq!(collected.identity, snapshot.identity);
    assert_eq!(collected.result_version, snapshot.result_version);
    let publication = collected.publication.as_ref().unwrap();
    if let Some(original) = &snapshot.publication {
        assert_eq!(publication.identity, original.identity);
    }
    publication.identity.digest.clone()
}
fn accept(fixture: &RecoveryFixture, snapshot: &AssignmentSnapshot) -> serde_json::Value {
    let result_digest = collected_digest(fixture, snapshot);
    fixture.tool("ovrcr_accept", serde_json::json!({"handle":snapshot.handle,"identity":snapshot.identity,"result_version":snapshot.result_version,"result_digest":result_digest,"expected_control_epoch":snapshot.control.epoch}))
}

#[test]
fn public_review_catches_untracked_defect_repairs_same_child_and_reviews_exact_new_candidate() {
    let fixture = super::followups::OwnedFollowup::new();
    let initial = worker(&fixture, "REPAIR");
    let coding = initial.resources.coding.as_ref().unwrap();
    let head = live::git(&coding.path, &["rev-parse", "HEAD"]);
    let refs = live::git(&fixture.live.repo, &["show-ref"]);
    let original_target = target(&fixture, &initial);
    let original_candidate = fs::read(&original_target.candidate.path).unwrap();
    let snapshot: serde_json::Value = serde_json::from_slice(&original_candidate).unwrap();
    let files = snapshot["snapshot"]["files"].as_array().unwrap();
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "calc.cjs" && file["base_mode"] == 0)
    );
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "obsolete.txt" && file["mode"] == 0)
    );
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "executable.sh" && file["mode"] == 0o100755)
    );
    assert!(
        files
            .iter()
            .any(|file| file["path"] == "calc-link" && file["mode"] == 0o120000)
    );
    let current = declared(&fixture, &initial, true, None);
    assert_eq!(
        current.review.as_ref().unwrap().limit.source,
        RepairLimitSource::CoordinatorDefault
    );
    assert_eq!(current.review.as_ref().unwrap().limit.rounds, 3);
    // Reviews must read the immutable publication, even when the worker checkout changes.
    fs::write(
        coding.path.join("calc.cjs"),
        "module.exports={add:(a,b)=>a+b};\n",
    )
    .unwrap();
    let spec = axis(&fixture, &current, ReviewAxis::Spec);
    let standards = axis(&fixture, &current, ReviewAxis::Standards);
    assert_ne!(spec.identity.session, initial.identity.session);
    assert_ne!(spec.conversation, standards.conversation);
    assert_ne!(spec.conversation, initial.conversation);
    let spec_result = spec.submission.as_ref().unwrap().review.as_ref().unwrap();
    assert_eq!(spec_result.outcome, ReviewOutcome::ChangesRequired);
    assert_eq!(
        standards
            .submission
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .outcome,
        ReviewOutcome::Passed
    );
    let executed: serde_json::Value = serde_json::from_slice(
        &fs::read(spec.brief.with_extension("json.artifact-check")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        executed["exit"], 7,
        "reviewer executes the defect-bearing pinned untracked bytes"
    );
    assert_eq!(accept(&fixture, &current)["dispatch"], "Refused");
    let correction = repaired(&fixture, &current, "repair-original");
    assert_eq!(correction.identity, initial.identity);
    assert_eq!(correction.result_version, 2);
    assert_eq!(correction.conversation, initial.conversation);
    assert_eq!(correction.resources.coding, initial.resources.coding);
    let correction = super::followups::complete(&fixture, &correction);
    assert_eq!(fixture.input_count(&correction), 1);
    assert_eq!(correction.review.as_ref().unwrap().consumed_rounds, 1);
    assert!(
        !correction
            .review
            .as_ref()
            .unwrap()
            .independent_review_satisfied
    );
    let same = repaired(&fixture, &correction, "repair-original");
    assert_eq!(same.result_version, 2);
    assert_eq!(same.review.as_ref().unwrap().consumed_rounds, 1);
    assert_eq!(fixture.input_count(&same), 1);
    assert_eq!(
        review(
            &fixture,
            &same,
            ReviewOperation::Collect {
                axis: ReviewAxis::Spec,
                reviewer: spec.handle.clone()
            }
        )["dispatch"],
        "Refused"
    );
    let current = declared(&fixture, &correction, true, None);
    let spec_again = axis(&fixture, &current, ReviewAxis::Spec);
    assert_eq!(
        accept(&fixture, &current)["dispatch"],
        "Refused",
        "a passing Spec axis does not replace the required Standards axis"
    );
    let standards_again = axis(&fixture, &current, ReviewAxis::Standards);
    for reviewer in [&spec_again, &standards_again] {
        assert_eq!(
            reviewer
                .submission
                .as_ref()
                .unwrap()
                .review
                .as_ref()
                .unwrap()
                .outcome,
            ReviewOutcome::Passed
        );
        assert_ne!(reviewer.identity.session, spec.identity.session);
    }
    let accepted: AssignmentSnapshot = serde_json::from_value(accept(&fixture, &current)).unwrap();
    assert_eq!(accepted.acceptance, "accepted");
    assert!(
        accepted
            .review
            .as_ref()
            .unwrap()
            .independent_review_satisfied
    );
    assert_eq!(
        fs::read(&original_target.candidate.path).unwrap(),
        original_candidate
    );
    assert_eq!(live::git(&coding.path, &["rev-parse", "HEAD"]), head);
    assert_eq!(live::git(&fixture.live.repo, &["show-ref"]), refs);
    assert_eq!(
        target(&fixture, &correction).candidate.base,
        original_target.candidate.base
    );
    assert_ne!(
        target(&fixture, &correction).candidate.digest,
        original_target.candidate.digest
    );
    assert_eq!(
        fixture.child_count(),
        5,
        "one worker and exactly four required independent assignments"
    );
}

#[test]
fn public_review_stops_stalled_repairs_without_replacement_or_extra_input() {
    let fixture = super::followups::OwnedFollowup::new();
    let first = worker(&fixture, "STALLED");
    let policy = Some(RepairLimit {
        rounds: 2,
        source: RepairLimitSource::ExplicitTask,
        reference: "fixture task explicitly permits at most two rounds".into(),
    });
    let first = declared(&fixture, &first, false, policy.clone());
    axis(&fixture, &first, ReviewAxis::Spec);
    let second = repaired(&fixture, &first, "repair-one");
    let second = super::followups::complete(&fixture, &second);
    let second = declared(&fixture, &second, false, policy);
    axis(&fixture, &second, ReviewAxis::Spec);
    let before = fixture.byte_count(&second);
    let stopped = repaired(&fixture, &second, "repair-two");
    assert_eq!(
        stopped.review.as_ref().unwrap().stopped,
        Some(ReviewStop::NoProgress)
    );
    assert_eq!(stopped.review.as_ref().unwrap().consumed_rounds, 1);
    assert_eq!(stopped.result_version, 2);
    assert_eq!(fixture.byte_count(&stopped), before);
    assert_eq!(fixture.child_count(), 3);
    assert_eq!(accept(&fixture, &stopped)["dispatch"], "Refused");
}

#[test]
fn public_review_missing_worker_checks_stop_repairs_even_after_review_found_defect() {
    let fixture = super::followups::OwnedFollowup::new();
    let first = worker(&fixture, "MISSING");
    let first = declared(&fixture, &first, false, None);
    axis(&fixture, &first, ReviewAxis::Spec);
    let before = fixture.byte_count(&first);
    let stopped = repaired(&fixture, &first, "missing-checks");
    assert_eq!(
        stopped.review.as_ref().unwrap().stopped,
        Some(ReviewStop::MissingChecks)
    );
    assert_eq!(stopped.review.as_ref().unwrap().consumed_rounds, 0);
    assert_eq!(fixture.byte_count(&stopped), before);
    assert_eq!(fixture.child_count(), 2);
}

#[test]
fn public_review_enforces_explicit_finite_budget_despite_new_behavioral_evidence() {
    let fixture = super::followups::OwnedFollowup::new();
    let mut current = worker(&fixture, "PROGRESS");
    let policy = Some(RepairLimit {
        rounds: 2,
        source: RepairLimitSource::ExplicitTask,
        reference: "explicit fixture limit of two corrective rounds".into(),
    });
    for round in 0..=2 {
        current = declared(
            &fixture,
            &current,
            false,
            if round == 0 { policy.clone() } else { None },
        );
        assert_eq!(
            current.review.as_ref().unwrap().limit,
            policy.clone().unwrap()
        );
        assert_eq!(current.review.as_ref().unwrap().consumed_rounds, round);
        axis(&fixture, &current, ReviewAxis::Spec);
        let before = fixture.byte_count(&current);
        let next = repaired(&fixture, &current, &format!("finite-repair-{round}"));
        if round == 2 {
            assert_eq!(
                next.review.as_ref().unwrap().stopped,
                Some(ReviewStop::LimitExhausted)
            );
            assert_eq!(next.review.as_ref().unwrap().consumed_rounds, 2);
            assert_eq!(next.result_version, 3);
            assert_eq!(fixture.byte_count(&next), before);
        } else {
            assert_eq!(next.result_version, round + 2);
            current = super::followups::complete(&fixture, &next);
        }
    }
    assert_eq!(
        fixture.child_count(),
        4,
        "one worker and one independent Spec reviewer for each immutable candidate"
    );
}

#[test]
fn public_review_refuses_foreign_changed_source_and_self_axis_claims_before_input() {
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "REPAIR");
    let current = declared(&fixture, &current, true, None);
    let selected: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &current,
        ReviewOperation::Assignment {
            axis: ReviewAxis::Spec,
        },
    ))
    .unwrap();
    let original = selected.review_assignment.unwrap();
    let before = fixture.child_count();
    for changed in [
        "candidate",
        "path",
        "publication",
        "source-contract",
        "base",
        "generation",
        "source",
        "assignment",
        "workflow",
    ] {
        let mut assignment = original.clone();
        match changed {
            "candidate" => assignment.target.coding.candidate.digest = "0".repeat(64),
            "path" => {
                assignment.target.coding.candidate.path =
                    "/private/foreign-candidate-v1.json".into()
            }
            "publication" => assignment.target.coding.publication.digest = "0".repeat(64),
            "source-contract" => assignment.target.coding.source_contract_digest = "0".repeat(64),
            "base" => assignment.target.coding.candidate.base = "0".repeat(40),
            "generation" => assignment.target.coding.candidate.git_identity = "replacement".into(),
            "source" => {
                assignment.source_documents[0]
                    .content
                    .push_str(" Changed source.");
                let hash = digest(assignment.source_documents[0].content.as_bytes());
                assignment.source_documents[0].source.digest = hash.clone();
                assignment.target.sources[0].digest = hash;
            }
            "assignment" => {
                assignment.target.coding.assignment.attempt += 1;
                assignment.target.coding.publication.assignment.attempt += 1;
            }
            "workflow" => {
                assignment.target.coding.assignment.workflow = "foreign".into();
                assignment.target.coding.publication.assignment.workflow = "foreign".into();
            }
            _ => unreachable!(),
        }
        // Match the deliberately changed target's fixed context, so authentication
        // rather than a stale scope string must reject this request.
        let scope = selected
            .review_scope
            .as_ref()
            .unwrap()
            .replace(
                &original.target.coding.candidate.digest,
                &assignment.target.coding.candidate.digest,
            )
            .replace(
                &original.target.coding.candidate.base,
                &assignment.target.coding.candidate.base,
            )
            .replace(
                original.target.coding.candidate.path.to_str().unwrap(),
                assignment.target.coding.candidate.path.to_str().unwrap(),
            );
        let output = fixture.tool("ovrcr_start",serde_json::json!({
            "project":PROJECT,"workspace":fixture.workspace,"scope":scope,"review":assignment,
            "model_provider":"test","model":"fixture-model","acceptance_evidence":["typed independent report"]
        }));
        assert_eq!(output["dispatch"], "Refused", "{changed}: {output}");
        let reason = output["reason"].as_str().unwrap();
        if changed == "source" {
            assert!(
                reason.contains("readable review view has stale/swapped"),
                "{changed}: {reason}"
            );
        } else {
            assert!(
                reason.contains("review target is stale"),
                "{changed}: {reason}"
            );
        }
        assert_eq!(
            fixture.child_count(),
            before,
            "{changed} cannot launch an invalid review context"
        );
    }
    assert_eq!(
        review(
            &fixture,
            &current,
            ReviewOperation::Collect {
                axis: ReviewAxis::Spec,
                reviewer: current.handle.clone()
            }
        )["dispatch"],
        "Refused"
    );
    let spec = axis(&fixture, &current, ReviewAxis::Spec);
    assert_eq!(
        review(
            &fixture,
            &current,
            ReviewOperation::Collect {
                axis: ReviewAxis::Standards,
                reviewer: spec.handle
            }
        )["dispatch"],
        "Refused"
    );
}

#[test]
fn public_review_keeps_original_repair_request_and_budget_after_lost_reply() {
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "REPAIR");
    let current = declared(&fixture, &current, false, None);
    axis(&fixture, &current, ReviewAxis::Spec);
    fixture.proxy.arm(ControlRule {
        operation: "Review",
        loss: 1,
        fault: None,
    });
    let lost = review(
        &fixture,
        &current,
        ReviewOperation::Repair {
            request: "lost-repair".into(),
            expected_control_epoch: current.control.epoch,
        },
    );
    assert_eq!(lost["dispatch"], "Unknown");
    let collected: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":current.handle}),
    ))
    .unwrap();
    let collected = super::followups::complete(&fixture, &collected);
    assert_eq!(collected.result_version, 2);
    assert_eq!(collected.review.as_ref().unwrap().consumed_rounds, 1);
    let bytes = fixture.byte_count(&collected);
    let recovered = repaired(&fixture, &collected, "lost-repair");
    assert_eq!(recovered.result_version, 2);
    assert_eq!(recovered.review.as_ref().unwrap().consumed_rounds, 1);
    assert_eq!(fixture.byte_count(&recovered), bytes);
    assert_eq!(fixture.input_count(&recovered), 1);
    assert_eq!(fixture.child_count(), 2);
}

#[test]
fn public_review_refuses_missing_base_and_corrupt_complete_snapshot_without_recapture() {
    let fixture = super::followups::OwnedFollowup::new();
    let initial = worker(&fixture, "REPAIR");
    let initial = declared(&fixture, &initial, false, None);
    let candidate = target(&fixture, &initial).candidate;
    let bytes = fs::read(&candidate.path).unwrap();
    let count = fixture.child_count();
    fs::write(&candidate.path, "{partial immutable publication").unwrap();
    assert_eq!(
        review(
            &fixture,
            &initial,
            ReviewOperation::Assignment {
                axis: ReviewAxis::Spec
            }
        )["dispatch"],
        "Refused"
    );
    assert_eq!(
        fs::read(&candidate.path).unwrap(),
        b"{partial immutable publication"
    );
    assert_eq!(fixture.child_count(), count);
    fs::write(&candidate.path, &bytes).unwrap();
    let object_path = format!("objects/{}/{}", &candidate.base[..2], &candidate.base[2..]);
    let location = live::git(
        &fixture.live.repo,
        &["rev-parse", "--git-path", &object_path],
    );
    let object = std::path::PathBuf::from(location.trim());
    let object = if object.is_absolute() {
        object
    } else {
        fixture.live.repo.join(object)
    };
    let original = fs::read(&object).unwrap();
    fs::remove_file(&object).unwrap();
    let refused = review(
        &fixture,
        &initial,
        ReviewOperation::Assignment {
            axis: ReviewAxis::Spec,
        },
    );
    fs::write(&object, original).unwrap();
    assert_eq!(refused["dispatch"], "Refused");
    assert_eq!(fs::read(&candidate.path).unwrap(), bytes);
    assert_eq!(fixture.child_count(), count);
}

#[test]
fn public_review_capacity_refusal_retries_one_fresh_seat_without_duplicate_team() {
    capacity_review_retry(true);
}

#[test]
fn public_review_direct_capacity_retry_preserves_original_readable_view_and_round() {
    capacity_review_retry(false);
}

fn capacity_review_retry(declared_policy: bool) {
    let fixture = super::followups::OwnedFollowup::new();
    if !declared_policy {
        fs::write(
            fixture.live.repo.join("review-context.txt"),
            "original unchanged caller\n",
        )
        .unwrap();
        live::git(&fixture.live.repo, &["add", "review-context.txt"]);
        live::git(
            &fixture.live.repo,
            &["commit", "-m", "direct review context"],
        );
    }
    let initial = worker(&fixture, if declared_policy { "REPAIR" } else { "CLEAN" });
    let (current, original_assignment, review_scope) = if declared_policy {
        let current = declared(&fixture, &initial, false, None);
        let selected: AssignmentSnapshot = serde_json::from_value(review(
            &fixture,
            &current,
            ReviewOperation::Assignment {
                axis: ReviewAxis::Spec,
            },
        ))
        .unwrap();
        (
            current,
            selected.review_assignment.unwrap(),
            selected.review_scope.unwrap(),
        )
    } else {
        let document = sources().pop().unwrap();
        let assignment = ReviewAssignment {
            implementation: initial.handle.clone(),
            axis: ReviewAxis::Spec,
            target: ReviewTarget {
                coding: target(&fixture, &initial),
                sources: vec![document.source.clone()],
                view: None,
            },
            source_documents: vec![document],
            context_paths: vec!["review-context.txt".into()],
        };
        (
            initial.clone(),
            assignment,
            "Direct read-only review needs no declared stages".into(),
        )
    };
    let set_limit = |limit: &str| {
        let output = fixture
            .live
            .command()
            .args(["settings", "set", "delegation.max_live_children", limit])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    set_limit("3");
    let baseline: DelegationCapabilities = serde_json::from_value(fixture.tool(
        "ovrcr_capabilities",
        serde_json::json!({"model_provider":"test","model":"fixture-model"}),
    ))
    .unwrap();
    assert_eq!(baseline.live_children, 1);
    let headroom = 49u16.checked_sub(baseline.live_sessions).unwrap();
    let Response::CreatedSession(occupant) =
        fixture
            .live
            .request(Request::CreateSession(CreateSessionRequest {
                project: PROJECT.into(),
                workspace: fixture.workspace.clone(),
                name: "owned-review-capacity-occupant".into(),
                label: None,
                argv: vec!["/bin/sh".into()],
                kind: SessionKind::Terminal,
            }))
    else {
        panic!("create the task-owned ordinary capacity occupant")
    };
    let occupant_group = unsafe { libc::getpgid(occupant.pid.unwrap() as i32) };
    assert!(occupant_group > 0);
    fixture.live.own_group(occupant_group);
    let capacity = || -> DelegationCapabilities {
        serde_json::from_value(fixture.tool(
            "ovrcr_capabilities",
            serde_json::json!({"model_provider":"test","model":"fixture-model",
                "required_limits":{"min_free_slots_after_launch":headroom}}),
        ))
        .unwrap()
    };
    let occupied = capacity();
    assert_eq!(occupied.live_sessions, baseline.live_sessions + 1);
    assert_eq!(occupied.live_children, baseline.live_children);
    assert_eq!(occupied.available_slots, 0);
    let refused: AssignmentSnapshot = serde_json::from_value(fixture.tool("ovrcr_start",serde_json::json!({
        "project":PROJECT,"workspace":fixture.workspace,"scope":review_scope,"review":original_assignment,
        "model_provider":"test","model":"fixture-model","acceptance_evidence":["typed independent report"],
        "required_limits":{"min_free_slots_after_launch":headroom}
    }))).unwrap();
    assert_eq!(refused.dispatch, DispatchStatus::Refused);
    assert!(refused.retryable_capacity_refusal);
    assert!(refused.identity.session.is_none());
    assert_eq!(fixture.child_count(), 1);
    assert_eq!(refused.execution_attempts, Some(0));
    let original_contract_path = refused.brief.parent().unwrap().join("contract-v1.json");
    let original_contract_bytes = fs::read(&original_contract_path).unwrap();
    let original_contract: ResearchContract =
        serde_json::from_slice(&original_contract_bytes).unwrap();
    let original_view = original_contract
        .review
        .as_ref()
        .unwrap()
        .target
        .view
        .as_ref()
        .unwrap()
        .clone();
    let original_view_bytes = fs::read(&original_view.path).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&original_view_bytes).unwrap();
    let mut original_images = std::collections::BTreeMap::new();
    for file in manifest["files"].as_array().unwrap() {
        for side in ["before", "after"] {
            if let Some(path) = file[side]["path"].as_str() {
                original_images.insert(std::path::PathBuf::from(path), fs::read(path).unwrap());
            }
        }
    }
    if !declared_policy {
        assert_eq!(original_view.round, refused.handle);
        assert!(
            current.review.is_none(),
            "direct review adds no declared policy"
        );
        fs::write(
            fixture.live.repo.join("review-context.txt"),
            "moving HEAD must not replace original context\n",
        )
        .unwrap();
        live::git(&fixture.live.repo, &["add", "review-context.txt"]);
        live::git(
            &fixture.live.repo,
            &["commit", "-m", "move context after refusal"],
        );
        fs::write(
            current
                .resources
                .coding
                .as_ref()
                .unwrap()
                .path
                .join("calc.cjs"),
            "mutable checkout must not become retry evidence\n",
        )
        .unwrap();
    }
    assert_eq!(original_contract.limits, occupied.effective_limits);
    assert_eq!(original_contract.limits.max_live_children, Some(3));
    assert_eq!(
        fixture.live.request(Request::CloseTerminal {
            session: occupant.id,
            expected_run: occupant.run,
        }),
        Response::Ok
    );
    assert!(live::wait_group_absent(
        occupant_group,
        Duration::from_secs(5)
    ));
    // The original effective policy remains fixed; only our ordinary occupant left.
    set_limit("3");
    let restored = capacity();
    assert_eq!(restored.live_sessions, baseline.live_sessions);
    assert_eq!(restored.live_children, baseline.live_children);
    assert_eq!(restored.effective_limits, original_contract.limits);
    assert_eq!(restored.available_slots, 1);
    let retry_result = fixture.tool("ovrcr_retry", serde_json::json!({"handle":refused.handle}));
    eprintln!(
        "DIRECT_REVIEW_CAPACITY_RETRY declared={declared_policy} original_view={original_view:?} result={retry_result}"
    );
    if !declared_policy {
        assert_eq!(
            retry_result["dispatch"], "Applied",
            "direct capacity retry must preserve the original authenticated view and exact contract: {retry_result}"
        );
    }
    let retry: AssignmentSnapshot = serde_json::from_value(retry_result).unwrap();
    assert_eq!(retry.dispatch, DispatchStatus::Applied, "{retry:?}");
    assert_eq!(retry.identity.attempt, 2);
    assert_eq!(retry.identity.task, refused.identity.task);
    let retry_contract_path = retry.brief.parent().unwrap().join("contract-v1.json");
    let mut retry_contract: ResearchContract =
        serde_json::from_slice(&fs::read(retry_contract_path).unwrap()).unwrap();
    retry_contract.request = original_contract.request.clone();
    retry_contract.previous_request = original_contract.previous_request.clone();
    assert_eq!(retry_contract, original_contract);
    assert_eq!(
        fs::read(original_contract_path).unwrap(),
        original_contract_bytes
    );
    let retry = super::followups::complete(&fixture, &retry);
    if declared_policy {
        let result: AssignmentSnapshot = serde_json::from_value(review(
            &fixture,
            &current,
            ReviewOperation::Collect {
                axis: ReviewAxis::Spec,
                reviewer: retry.handle.clone(),
            },
        ))
        .unwrap();
        assert_eq!(result.review.as_ref().unwrap().evidence.len(), 1);
        assert_eq!(
            result.review.as_ref().unwrap().evidence[0].reviewer,
            retry.identity
        );
    } else {
        let result = publication_snapshot(&fixture, &retry);
        let report = result.submission.as_ref().unwrap().review.as_ref().unwrap();
        assert_eq!(report.outcome, ReviewOutcome::Passed);
        assert_eq!(report.target.view, Some(original_view.clone()));
        assert_eq!(fs::read(&original_view.path).unwrap(), original_view_bytes);
        for (path, bytes) in &original_images {
            assert_eq!(fs::read(path).unwrap(), *bytes);
        }
    }
    let again: AssignmentSnapshot = serde_json::from_value(
        fixture.tool("ovrcr_retry", serde_json::json!({"handle":refused.handle})),
    )
    .unwrap();
    assert_eq!(again.identity, retry.identity);
    assert_eq!(
        fixture.child_count(),
        2,
        "the original capacity refusal never created a native reviewer context"
    );
    if declared_policy {
        let duplicate = fixture.tool("ovrcr_start",serde_json::json!({
            "project":PROJECT,"workspace":fixture.workspace,"scope":review_scope,"review":original_assignment,
            "model_provider":"test","model":"fixture-model","acceptance_evidence":["typed independent report"],
            "required_limits":{"min_free_slots_after_launch":headroom}
        }));
        assert_eq!(duplicate["dispatch"], "Refused");
    } else {
        let private_files = || -> std::collections::BTreeSet<_> {
            fs::read_dir(original_view.path.parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect()
        };
        let metadata = || {
            let db = rusqlite::Connection::open_with_flags(
                fixture.live.config.join("registry.sqlite3"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            db.prepare("SELECT handle,metadata FROM delegations ORDER BY handle")
                .unwrap()
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        let before_files = private_files();
        let before_metadata = metadata();
        let DelegationRequest::Prepare { auth, .. } =
            fixture.proxy.original(DispatchOperation::Prepare)
        else {
            unreachable!()
        };
        for damage in [
            "scope",
            "source",
            "foreign-task",
            "live-context",
            "foreign-view",
        ] {
            let mut changed = original_contract.clone();
            changed.request = format!("invalid-direct-retry-{damage}");
            changed.previous_request = Some(refused.handle.clone());
            match damage {
                "scope" => changed.scope.push_str(" different logical task"),
                "source" => {
                    let assignment = changed.review.as_mut().unwrap();
                    assignment.source_documents[0]
                        .content
                        .push_str(" changed originating source");
                    let document = &mut assignment.source_documents[0];
                    document.source.digest = digest(document.content.as_bytes());
                    assignment.target.sources = vec![document.source.clone()];
                }
                "foreign-task" => changed.previous_request = Some(current.handle.clone()),
                "live-context" => changed.previous_request = Some(retry.handle.clone()),
                "foreign-view" => {
                    changed
                        .review
                        .as_mut()
                        .unwrap()
                        .target
                        .view
                        .as_mut()
                        .unwrap()
                        .digest = "0".repeat(64)
                }
                _ => unreachable!(),
            }
            let response =
                fixture
                    .live
                    .request(Request::Delegation(Box::new(DelegationRequest::Prepare {
                        auth: auth.clone(),
                        contract: changed,
                    })));
            assert!(
                matches!(response, Response::Error { .. }),
                "{damage}: {response:?}"
            );
            assert_eq!(
                private_files(),
                before_files,
                "{damage} cannot publish a replacement view"
            );
            assert_eq!(
                metadata(),
                before_metadata,
                "{damage} cannot register or rewrite an assignment"
            );
            assert_eq!(fixture.child_count(), 2);
        }
        let image = manifest["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["kind"] == "candidate" && file["path"] == "calc.cjs")
            .unwrap()["after"]["path"]
            .as_str()
            .unwrap();
        let mut tampered = fs::read(image).unwrap();
        tampered.extend_from_slice(b"// changed original direct review view\n");
        fs::write(image, tampered).unwrap();
        let rejected = fixture.tool("ovrcr_retry", serde_json::json!({"handle":refused.handle}));
        assert_eq!(rejected["dispatch"], "Refused", "{rejected}");
        assert_eq!(private_files(), before_files);
        assert_eq!(metadata(), before_metadata);
        assert_eq!(fixture.child_count(), 2);
        fs::write(
            image,
            original_images.get(std::path::Path::new(image)).unwrap(),
        )
        .unwrap();
        assert_eq!(fs::read(&original_view.path).unwrap(), original_view_bytes);
        for (path, bytes) in &original_images {
            assert_eq!(fs::read(path).unwrap(), *bytes);
        }
    }
    assert_eq!(fixture.child_count(), 2);
    assert_eq!(
        fixture.reconcile(&initial.handle).identity,
        initial.identity
    );
}

#[test]
fn public_review_new_round_requires_fresh_independent_axes_even_for_unchanged_source() {
    let fixture = super::followups::OwnedFollowup::new();
    let initial = worker(&fixture, "REPAIR");
    let current = declared(&fixture, &initial, true, None);
    let spec = axis(&fixture, &current, ReviewAxis::Spec);
    let standards = axis(&fixture, &current, ReviewAxis::Standards);
    let original_spec_bytes = fs::read(&spec.result_path).unwrap();
    let before: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_inspect",
        serde_json::json!({"handle":current.handle}),
    ))
    .unwrap();
    let mut documents = sources();
    documents[1]
        .content
        .push_str(" New applicable repository rule.");
    documents[1].source.digest = digest(documents[1].content.as_bytes());
    let updated: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &current,
        ReviewOperation::Declare(Box::new(ReviewDeclaration {
            request: "updated-standards-source".into(),
            target: target(&fixture, &current),
            requirements: vec![
                ReviewRequirement {
                    axis: ReviewAxis::Spec,
                    sources: vec![documents[2].source.clone()],
                },
                ReviewRequirement {
                    axis: ReviewAxis::Standards,
                    sources: documents[..2]
                        .iter()
                        .map(|source| source.source.clone())
                        .collect(),
                },
            ],
            sources: documents,
            repair_limit: None,
            context_paths: vec![],
        })),
    ))
    .unwrap();
    assert!(updated.review.as_ref().unwrap().evidence.is_empty());
    assert_eq!(before.review.as_ref().unwrap().evidence.len(), 2);
    assert_eq!(
        review(
            &fixture,
            &updated,
            ReviewOperation::Collect {
                axis: ReviewAxis::Standards,
                reviewer: standards.handle
            }
        )["dispatch"],
        "Refused"
    );
    let stale_spec = review(
        &fixture,
        &updated,
        ReviewOperation::Collect {
            axis: ReviewAxis::Spec,
            reviewer: spec.handle.clone(),
        },
    );
    assert_eq!(stale_spec["dispatch"], "Refused");
    axis(&fixture, &updated, ReviewAxis::Spec);
    axis(&fixture, &updated, ReviewAxis::Standards);
    assert_eq!(fs::read(&spec.result_path).unwrap(), original_spec_bytes);
    assert_eq!(
        fixture.child_count(),
        5,
        "each axis needs the new immutable round even when its source is unchanged"
    );
}

#[test]
fn public_review_human_takeover_rejects_repair_before_any_round_or_input() {
    let fixture = super::followups::OwnedFollowup::new();
    let initial = worker(&fixture, "REPAIR");
    let current = declared(&fixture, &initial, false, None);
    axis(&fixture, &current, ReviewAxis::Spec);
    let bytes = fixture.byte_count(&current);
    let mut dashboard = super::assignment_control::dashboard(&fixture, &current);
    let takeover = super::assignment_control::transition(
        &fixture,
        &mut dashboard,
        current.identity.session.unwrap(),
        "review-human-takeover",
        true,
        1,
    );
    assert_eq!(
        review(
            &fixture,
            &current,
            ReviewOperation::Repair {
                request: "stale-human-repair".into(),
                expected_control_epoch: current.control.epoch
            }
        )["dispatch"],
        "Refused"
    );
    assert_eq!(
        review(
            &fixture,
            &takeover,
            ReviewOperation::Repair {
                request: "human-owned-repair".into(),
                expected_control_epoch: takeover.control.epoch
            }
        )["dispatch"],
        "Refused"
    );
    let inspected: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_inspect",
        serde_json::json!({"handle":current.handle}),
    ))
    .unwrap();
    assert_eq!(inspected.review.as_ref().unwrap().consumed_rounds, 0);
    assert_eq!(fixture.byte_count(&current), bytes);
    assert_eq!(fixture.child_count(), 2);
}

#[test]
fn public_review_native_parent_restart_recovers_original_repair_without_input_replay() {
    let fixture = super::followups::OwnedFollowup::new();
    let initial = worker(&fixture, "REPAIR");
    let current = declared(&fixture, &initial, false, None);
    axis(&fixture, &current, ReviewAxis::Spec);
    fixture.proxy.arm(ControlRule {
        operation: "Review",
        loss: 1,
        fault: None,
    });
    assert_eq!(
        review(
            &fixture,
            &current,
            ReviewOperation::Repair {
                request: "restart-repair".into(),
                expected_control_epoch: current.control.epoch
            }
        )["dispatch"],
        "Unknown"
    );
    let before = super::parent_recovery::workflow(&fixture);
    let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
        panic!("inventory")
    };
    let parent = sessions
        .iter()
        .find(|session| session.id == fixture.parent)
        .unwrap();
    let group = unsafe { libc::getpgid(parent.pid.unwrap() as i32) };
    assert!(group > 0);
    fixture.live.own_group(group);
    assert_eq!(
        fixture.live.request(Request::SendTerminal {
            session: fixture.parent,
            text: "exit".into(),
            submit: true
        }),
        Response::Ok
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        if sessions
            .iter()
            .any(|session| session.id == fixture.parent && !session.phase.is_live())
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "parent native process did not exit"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(live::wait_group_absent(group, Duration::from_secs(5)));
    let reopened = fixture.live.request(Request::ReopenSession {
        session: fixture.parent,
        expected_run: before.authority.parent_run,
        acknowledge_stopped: true,
    });
    assert!(
        matches!(reopened, Response::CreatedSession(_)),
        "{reopened:?}"
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect) else {
            panic!("inventory")
        };
        if sessions.iter().any(|session| {
            session.id == fixture.parent
                && session.run.0 == before.authority.parent_run.0 + 1
                && session
                    .agent
                    .as_ref()
                    .is_some_and(|agent| agent.health.state == ReporterHealth::Connected)
        }) {
            break;
        }
        assert!(Instant::now() < deadline, "reopened parent did not bind");
        std::thread::sleep(Duration::from_millis(5));
    }
    let after = super::parent_recovery::workflow(&fixture);
    assert_eq!(after.authority.epoch, before.authority.epoch + 1);
    let current: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":current.handle}),
    ))
    .unwrap();
    let current = super::followups::complete(&fixture, &current);
    let bytes = fixture.byte_count(&current);
    let recovered = repaired(&fixture, &current, "restart-repair");
    assert_eq!(recovered.result_version, 2);
    assert_eq!(recovered.review.as_ref().unwrap().consumed_rounds, 1);
    assert_eq!(fixture.input_count(&recovered), 1);
    assert_eq!(fixture.byte_count(&recovered), bytes);
    assert_eq!(recovered.conversation, initial.conversation);
    assert_eq!(fixture.child_count(), 2);
}

#[test]
fn public_direct_coding_without_review_does_not_create_an_unrequested_team() {
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "CLEAN");
    assert!(current.review.is_none());
    assert_eq!(fixture.child_count(), 1);
    let accepted: AssignmentSnapshot = serde_json::from_value(accept(&fixture, &current)).unwrap();
    assert_eq!(accepted.acceptance, "accepted");
    assert!(accepted.review.is_none());
    let explicit_none: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &current,
        ReviewOperation::Declare(Box::new(ReviewDeclaration {
            request: "direct-no-independent-axes".into(),
            target: target(&fixture, &current),
            requirements: vec![],
            sources: vec![],
            repair_limit: None,
            context_paths: vec![],
        })),
    ))
    .unwrap();
    assert!(
        explicit_none
            .review
            .as_ref()
            .unwrap()
            .requirements
            .is_empty()
    );
    assert!(
        explicit_none
            .review
            .as_ref()
            .unwrap()
            .independent_review_satisfied
    );
    assert_eq!(fixture.child_count(), 1);
    let before_input = fixture.byte_count(&current);
    let unrequested = review(
        &fixture,
        &current,
        ReviewOperation::Assignment {
            axis: ReviewAxis::Spec,
        },
    );
    assert_eq!(unrequested["dispatch"], "Refused", "{unrequested}");
    assert_eq!(
        unrequested["reason"], "requested axis is not required",
        "{unrequested}"
    );
    assert_eq!(fixture.child_count(), 1);
    assert_eq!(fixture.byte_count(&current), before_input);
}

#[test]
fn public_direct_coding_declares_only_one_requested_independent_axis() {
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "CLEAN");
    let current = declared(&fixture, &current, false, None);
    assert_eq!(current.review.as_ref().unwrap().requirements.len(), 1);
    assert_eq!(
        accept(&fixture, &current)["dispatch"],
        "Refused",
        "the declared Spec publication is still missing"
    );
    assert_eq!(
        review(
            &fixture,
            &current,
            ReviewOperation::Assignment {
                axis: ReviewAxis::Standards
            }
        )["dispatch"],
        "Refused"
    );
    let reviewer = axis(&fixture, &current, ReviewAxis::Spec);
    assert_eq!(
        reviewer
            .submission
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .outcome,
        ReviewOutcome::Passed
    );
    let accepted: AssignmentSnapshot = serde_json::from_value(accept(&fixture, &current)).unwrap();
    assert!(
        accepted
            .review
            .as_ref()
            .unwrap()
            .independent_review_satisfied
    );
    assert_eq!(accepted.review.as_ref().unwrap().evidence.len(), 1);
    assert_eq!(
        accepted.review.as_ref().unwrap().evidence[0].axis,
        ReviewAxis::Spec
    );
    assert_eq!(fixture.child_count(), 2);
}

#[test]
fn public_review_optional_check_outcomes_remain_visible_without_widening_required_gates() {
    let fixture = super::followups::OwnedFollowup::new();
    let initial = worker(&fixture, "OPTIONAL");
    let optional = |snapshot: &AssignmentSnapshot| {
        snapshot
            .submission
            .as_ref()
            .unwrap()
            .coding
            .as_ref()
            .unwrap()
            .checks
            .iter()
            .filter(|check| check.name.starts_with("optional-"))
            .cloned()
            .collect::<Vec<_>>()
    };
    let original_optional = optional(&initial);
    assert_eq!(
        original_optional
            .iter()
            .map(|check| check.outcome.clone())
            .collect::<Vec<_>>(),
        vec![
            CheckOutcome::Failed,
            CheckOutcome::Missing,
            CheckOutcome::Unrun
        ]
    );
    let current = declared(&fixture, &initial, false, None);
    axis(&fixture, &current, ReviewAxis::Spec);
    let corrected = repaired(&fixture, &current, "required-addition-only");
    assert_eq!(
        corrected.result_version, 2,
        "optional Missing/Unrun checks do not stop an authorized required-check repair"
    );
    let corrected = super::followups::complete(&fixture, &corrected);
    assert_eq!(optional(&corrected), original_optional);
    let current = declared(&fixture, &corrected, false, None);
    axis(&fixture, &current, ReviewAxis::Spec);
    let accepted: AssignmentSnapshot = serde_json::from_value(accept(&fixture, &current)).unwrap();
    assert_eq!(
        accepted.acceptance, "accepted",
        "the parent judges visible optional outcomes separately"
    );
    assert_eq!(optional(&accepted), original_optional);
    assert_eq!(
        accepted
            .submission
            .as_ref()
            .unwrap()
            .coding
            .as_ref()
            .unwrap()
            .checks
            .iter()
            .find(|check| check.name == "addition")
            .unwrap()
            .outcome,
        CheckOutcome::Passed
    );
    assert_eq!(fixture.child_count(), 3);
}

#[test]
fn public_review_blocked_human_decision_stops_without_inventing_an_answer_or_repair() {
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "CLEAN");
    let document = source(
        ReviewSourceKind::Spec,
        "fixture://unresolved-human-decision",
        "REVIEW_FIXTURE_HUMAN_DECISION: The person must choose the authorized alternate output before this assignment can be accepted. No answer has been supplied.",
    );
    let current: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &current,
        ReviewOperation::Declare(Box::new(ReviewDeclaration {
            request: "unanswered-decision".into(),
            target: target(&fixture, &current),
            requirements: vec![ReviewRequirement {
                axis: ReviewAxis::Spec,
                sources: vec![document.source.clone()],
            }],
            sources: vec![document],
            repair_limit: None,
            context_paths: vec![],
        })),
    ))
    .unwrap();
    let reviewer = axis(&fixture, &current, ReviewAxis::Spec);
    assert_eq!(
        reviewer
            .submission
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .outcome,
        ReviewOutcome::Blocked
    );
    let before = fixture.byte_count(&current);
    let stopped = repaired(&fixture, &current, "no-invented-decision");
    assert_eq!(
        stopped.review.as_ref().unwrap().stopped,
        Some(ReviewStop::MissingDecision)
    );
    assert_eq!(stopped.review.as_ref().unwrap().consumed_rounds, 0);
    assert_eq!(stopped.result_version, current.result_version);
    assert_eq!(fixture.byte_count(&stopped), before);
    assert!(
        stopped.questions.is_empty(),
        "review does not fabricate a child's question or a human answer"
    );
    assert_eq!(accept(&fixture, &stopped)["dispatch"], "Refused");
    assert_eq!(fixture.child_count(), 2);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_review_partial_and_unknown_repairs_keep_original_receipt_without_replay() {
    for (point, limit, outcome) in [
        ("input", 7, DispatchOutcome::Partial),
        ("input", usize::MAX, DispatchOutcome::Unknown),
        ("input-after-effect", 0, DispatchOutcome::Unknown),
    ] {
        let fixture = super::followups::OwnedFollowup::new();
        let current = worker(&fixture, "REPAIR");
        let current = declared(&fixture, &current, false, None);
        axis(&fixture, &current, ReviewAxis::Spec);
        let before = fixture.byte_count(&current);
        let request = "original-uncertain-repair";
        fs::write(
            fixture.live.root.path().join("fault"),
            format!("{point}|{request}|{limit}"),
        )
        .unwrap();
        let uncertain = repaired(&fixture, &current, request);
        let receipt = uncertain
            .receipts
            .iter()
            .find(|receipt| {
                receipt.operation == DispatchOperation::Followup && receipt.request == request
            })
            .unwrap();
        assert_eq!(receipt.outcome, outcome);
        assert_eq!(uncertain.review.as_ref().unwrap().consumed_rounds, 1);
        assert_eq!(
            uncertain.review.as_ref().unwrap().pending_repair.as_deref(),
            Some(request)
        );
        if outcome == DispatchOutcome::Partial {
            assert_eq!(receipt.input.as_ref().unwrap().written_bytes, 7);
            let deadline = Instant::now() + Duration::from_secs(10);
            while fixture.byte_count(&uncertain) < before + 7 {
                assert!(
                    Instant::now() < deadline,
                    "fixture never observed the actual partial prefix"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        } else {
            wait_file(&uncertain.brief.with_extension("json.started"));
        }
        let after = fixture.byte_count(&uncertain);
        let recovered = repaired(&fixture, &uncertain, request);
        assert_eq!(
            recovered
                .receipts
                .iter()
                .find(|saved| saved.request == request
                    && saved.operation == DispatchOperation::Followup),
            Some(receipt)
        );
        assert_eq!(recovered.review.as_ref().unwrap().consumed_rounds, 1);
        assert_eq!(
            review(
                &fixture,
                &uncertain,
                ReviewOperation::Repair {
                    request: "fresh-bypass-repair".into(),
                    expected_control_epoch: uncertain.control.epoch
                }
            )["dispatch"],
            "Refused"
        );
        let inspected: AssignmentSnapshot = serde_json::from_value(fixture.tool(
            "ovrcr_inspect",
            serde_json::json!({"handle":current.handle}),
        ))
        .unwrap();
        assert_eq!(inspected.review.as_ref().unwrap().consumed_rounds, 1);
        assert_eq!(
            fixture.byte_count(&inspected),
            after,
            "neither recovery nor a fresh request may add input"
        );
        assert_eq!(fixture.child_count(), 2);
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_review_conversation_aba_at_final_input_fence_refuses_stale_repair_bytes() {
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "REPAIR_REBIND_AT_REPAIR");
    let current = declared(&fixture, &current, false, None);
    axis(&fixture, &current, ReviewAxis::Spec);
    let before = fixture.byte_count(&current);
    let request = "conversation-fenced-repair";
    let root = fixture.live.root.path().to_owned();
    fs::write(
        root.join("fault"),
        format!("input-before-write|{request}|0"),
    )
    .unwrap();
    let brief = current.brief.clone();
    let rebind = std::thread::spawn(move || {
        wait_file(&root.join("fault.hit"));
        assert_eq!(
            fs::read_to_string(root.join("fault.hit")).unwrap(),
            "input-before-write"
        );
        fs::write(
            brief.with_extension("json.repair-rebind"),
            "replace only this worker's actual native conversation",
        )
        .unwrap();
        wait_file(&brief.with_extension("json.repair-rebound"));
        assert!(!brief.with_extension("json.repair-rebind-error").exists());
        fs::write(
            root.join("fault.release"),
            "release after actual same-worker A-to-B-to-A reports",
        )
        .unwrap();
    });
    let rejected = repaired(&fixture, &current, request);
    rebind.join().unwrap();
    let receipt = rejected
        .receipts
        .iter()
        .find(|receipt| {
            receipt.operation == DispatchOperation::Followup && receipt.request == request
        })
        .unwrap();
    assert_eq!(receipt.outcome, DispatchOutcome::Rejected);
    assert!(
        receipt.input.is_none(),
        "conversation replacement rejects before the counted write"
    );
    assert_eq!(rejected.result_version, current.result_version);
    assert_eq!(
        rejected.conversation, current.conversation,
        "returning to the same display conversation does not restore its generation"
    );
    assert_eq!(rejected.review.as_ref().unwrap().consumed_rounds, 1);
    assert_eq!(fixture.byte_count(&rejected), before);
    let again = repaired(&fixture, &rejected, request);
    assert_eq!(again.result_version, current.result_version);
    assert_eq!(
        review(
            &fixture,
            &rejected,
            ReviewOperation::Repair {
                request: "replacement-context-bypass".into(),
                expected_control_epoch: rejected.control.epoch
            }
        )["dispatch"],
        "Refused"
    );
    assert_eq!(fixture.byte_count(&rejected), before);
    assert_eq!(fixture.child_count(), 2);
}

#[test]
fn public_review_refuses_unqualified_native_typed_publisher_before_prepare_or_launch() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "CLEAN");
    let DelegationRequest::Prepare { auth, .. } =
        fixture.proxy.original(DispatchOperation::Prepare)
    else {
        unreachable!()
    };
    let native_called = fixture.live.root.path().join("unsupported-native-called");
    for provider in ["claude", "codex", "grok"] {
        let executable = fixture.live.claude.command.parent().unwrap().join(provider);
        fs::write(
            &executable,
            format!("#!/bin/sh\n: > '{}'\nexit 99\n", native_called.display()),
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let metadata = || {
        let row = rusqlite::Connection::open_with_flags(
            fixture.live.config.join("registry.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        row.prepare("SELECT handle,metadata FROM delegations ORDER BY handle")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    };
    let private_files = || -> std::collections::BTreeSet<_> {
        fs::read_dir(current.brief.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
    };
    let original_metadata = metadata();
    let original_files = private_files();
    let document = sources().pop().unwrap();
    let direct = ReviewAssignment {
        implementation: current.handle.clone(),
        axis: ReviewAxis::Spec,
        target: ReviewTarget {
            coding: target(&fixture, &current),
            sources: vec![document.source.clone()],
            view: None,
        },
        source_documents: vec![document],
        context_paths: vec![],
    };
    let mut contract: ResearchContract = serde_json::from_slice(
        &fs::read(current.brief.parent().unwrap().join("contract-v1.json")).unwrap(),
    )
    .unwrap();
    contract.coding = None;
    contract.review = Some(direct);
    contract.permitted_actions = vec!["read".into()];
    contract.mandatory_capabilities = vec!["read".into(), "result_file".into()];
    contract.workspace = fixture.workspace.clone();
    contract.scope = "Direct read-only review without a coordinator policy".into();
    for provider in ["claude", "codex", "grok"] {
        contract.provider = provider.into();
        contract.request = format!("unsupported-direct-{provider}-review");
        contract.task = format!("unsupported-direct-{provider}-task");
        let response =
            fixture
                .live
                .request(Request::Delegation(Box::new(DelegationRequest::Prepare {
                    auth: auth.clone(),
                    contract: contract.clone(),
                })));
        let Response::Error { code, message, .. } = response else {
            panic!("unsupported direct typed publisher was admitted: {response:?}")
        };
        assert_eq!(code, ErrorCode::InvalidRequest);
        assert!(message.contains("typed independent-review"), "{message}");
        assert!(
            !native_called.exists(),
            "unsupported provider must not trigger native discovery"
        );
        assert_eq!(fixture.child_count(), 1);
        assert_eq!(
            metadata(),
            original_metadata,
            "unsupported direct Prepare must not save metadata or an assignment"
        );
        let after = private_files();
        eprintln!(
            "UNSUPPORTED_DIRECT_PRIVATE_FILES provider={provider} before={original_files:?} after={after:?}"
        );
        assert_eq!(
            after, original_files,
            "unsupported direct Prepare must not publish readable view files"
        );
    }
    let current = declared(&fixture, &current, false, None);
    let selected: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &current,
        ReviewOperation::Assignment {
            axis: ReviewAxis::Spec,
        },
    ))
    .unwrap();
    let assignment = selected.review_assignment.unwrap();
    let DelegationRequest::Prepare { auth, .. } =
        fixture.proxy.original(DispatchOperation::Prepare)
    else {
        unreachable!()
    };
    let mut original: ResearchContract = serde_json::from_slice(
        &fs::read(current.brief.parent().unwrap().join("contract-v1.json")).unwrap(),
    )
    .unwrap();
    original.coding = None;
    original.review = Some(assignment.clone());
    original.permitted_actions = vec!["read".into()];
    original.mandatory_capabilities = vec!["read".into(), "result_file".into()];
    original.workspace = fixture.workspace.clone();
    original.scope = selected.review_scope.clone().unwrap();
    for provider in ["claude", "codex", "grok"] {
        original.provider = provider.into();
        original.request = format!("unsupported-{provider}-review");
        original.task = format!("unsupported-{provider}-task");
        let response =
            fixture
                .live
                .request(Request::Delegation(Box::new(DelegationRequest::Prepare {
                    auth: auth.clone(),
                    contract: original.clone(),
                })));
        let Response::Error { code, message, .. } = response else {
            panic!("unsupported typed publisher was admitted: {response:?}")
        };
        assert_eq!(code, ErrorCode::InvalidRequest);
        assert!(message.contains("typed independent-review"), "{message}");
        let output=fixture.tool("ovrcr_start",serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":selected.review_scope,"review":assignment,"provider":provider,"model_provider":"test","model":"fixture-model","acceptance_evidence":["typed exact-axis independent report"]}));
        assert_eq!(output["dispatch"], "Refused");
        assert_eq!(output["transport"], "NotSent");
        assert_eq!(fixture.child_count(), 1);
    }
    let row = rusqlite::Connection::open(fixture.live.config.join("registry.sqlite3")).unwrap();
    let count: i64 = row
        .query_row(
            "SELECT count(*) FROM delegations WHERE handle LIKE 'unsupported-%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 0,
        "unsupported publishers are refused before Prepare persistence"
    );
}

#[test]
fn public_review_changed_requirements_clear_acceptance_and_fence_old_delivery_until_reaccepted() {
    let fixture = super::followups::OwnedFollowup::new();
    let initial = worker(&fixture, "CLEAN");
    let accepted: AssignmentSnapshot = serde_json::from_value(accept(&fixture, &initial)).unwrap();
    let old_acceptance = accepted.acceptance_ref.clone().unwrap();
    let old_bytes = fs::read(&old_acceptance.path).unwrap();
    let declaration = DeliveryDeclaration {
        request: "before-required-review".into(),
        workflow: accepted.identity.workflow.clone(),
        scope: "Deliver originally accepted coding candidate".into(),
        provenance: "Explicit fixture parent judgment before changed review requirements".into(),
        resources: vec![DeliveryResource {
            handle: accepted.handle.clone(),
            identity: accepted.identity.clone(),
            result_version: accepted.result_version,
            obligation_revision: accepted.obligation_revision,
            publication: accepted
                .publication
                .as_ref()
                .map(|publication| publication.identity.clone()),
            acceptance: accepted.acceptance_ref.clone(),
            evidence: vec![],
            completed_obligations: vec!["original coding task".into()],
            retention: RetentionChoice::ArchiveEligible,
            previous: None,
        }],
    };
    let delivery: WorkflowSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_deliver",
        serde_json::json!({"declaration":declaration}),
    ))
    .unwrap();
    let old_delivery = delivery
        .deliveries
        .iter()
        .find(|delivery| delivery.reference.request == "before-required-review")
        .unwrap()
        .reference
        .clone();
    let changed = declared(&fixture, &accepted, false, None);
    assert!(changed.acceptance_ref.is_none());
    assert_eq!(changed.acceptance, "pending");
    assert!(changed.obligation_revision > accepted.obligation_revision);
    assert_eq!(fs::read(&old_acceptance.path).unwrap(), old_bytes);
    assert_eq!(accept(&fixture, &changed)["dispatch"], "Refused");
    let refused:AssignmentSnapshot=serde_json::from_value(fixture.tool("ovrcr_archive",serde_json::json!({"request":"old-delivery-cannot-archive","handle":changed.handle,"expected_identity":changed.identity,"expected_result_version":changed.result_version,"expected_obligation_revision":accepted.obligation_revision,"expected_control_epoch":changed.control.epoch,"delivery":old_delivery,"continuation_of":null}))).unwrap();
    assert_eq!(
        refused.archive_receipts.last().unwrap().outcome,
        ArchiveOutcome::Refused {
            reason: ArchiveRefusal::ObligationChanged
        }
    );
    axis(&fixture, &changed, ReviewAxis::Spec);
    let reaccepted: AssignmentSnapshot =
        serde_json::from_value(accept(&fixture, &changed)).unwrap();
    assert_eq!(reaccepted.acceptance_ref, Some(old_acceptance.clone()));
    assert_eq!(
        reaccepted.obligation_revision,
        changed.obligation_revision.checked_add(1).unwrap()
    );
    assert_eq!(fs::read(&old_acceptance.path).unwrap(), old_bytes);
    assert_eq!(fixture.child_count(), 2);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_review_stop_keeps_an_accepted_unsent_repair_intent_from_resuming() {
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "REPAIR");
    let current = declared(&fixture, &current, false, None);
    axis(&fixture, &current, ReviewAxis::Spec);
    let before = fixture.byte_count(&current);
    fs::write(
        fixture.live.root.path().join("fault"),
        "followup-before-files|stopped-original-repair|0",
    )
    .unwrap();
    let pending = repaired(&fixture, &current, "stopped-original-repair");
    assert_eq!(pending.result_version, current.result_version);
    assert_eq!(pending.review.as_ref().unwrap().consumed_rounds, 1);
    assert_eq!(
        pending
            .receipts
            .iter()
            .find(|receipt| receipt.operation == DispatchOperation::Followup
                && receipt.request == "stopped-original-repair")
            .unwrap()
            .outcome,
        DispatchOutcome::Accepted
    );
    let stopped: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &pending,
        ReviewOperation::Stop {
            reason: ReviewStop::MissingDecision,
        },
    ))
    .unwrap();
    assert_eq!(
        review(
            &fixture,
            &stopped,
            ReviewOperation::Repair {
                request: "stopped-original-repair".into(),
                expected_control_epoch: current.control.epoch
            }
        )["dispatch"],
        "Refused"
    );
    assert_eq!(
        review(
            &fixture,
            &stopped,
            ReviewOperation::Assignment {
                axis: ReviewAxis::Spec
            }
        )["dispatch"],
        "Refused"
    );
    assert_eq!(fixture.byte_count(&stopped), before);
    assert_eq!(fixture.child_count(), 2);
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_review_report_change_after_preauthentication_refuses_final_acceptance_without_save() {
    let fixture = super::followups::OwnedFollowup::new();
    let initial = worker(&fixture, "CLEAN");
    let accepted: AssignmentSnapshot = serde_json::from_value(accept(&fixture, &initial)).unwrap();
    let previous = accepted.acceptance_ref.clone().unwrap();
    let previous_bytes = fs::read(&previous.path).unwrap();
    let current = declared(&fixture, &accepted, false, None);
    let reviewer = axis(&fixture, &current, ReviewAxis::Spec);
    let current = fixture.reconcile(&current.handle);
    let result_digest = collected_digest(&fixture, &current);
    let original_report = fs::read(&reviewer.result_path).unwrap();
    let byte_count = fixture.byte_count(&current);
    let metadata = || {
        let db = rusqlite::Connection::open_with_flags(
            fixture.live.config.join("registry.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        db.query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [&current.handle],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    };
    let before = metadata();
    let fault = fixture.live.root.path().join("fault");
    let release = fault.with_extension("release");
    struct Release(std::path::PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = fs::write(&self.0, "release");
        }
    }
    let _release = Release(release.clone());
    fs::write(&fault, format!("review-final-auth|{}|0", current.handle)).unwrap();
    let output = fixture.live.root.path().join("review-final-auth.json");
    assert_eq!(
        fixture.live.request(Request::SendTerminal {
            session: fixture.parent,
            text: format!(
                "tool:{}",
                serde_json::json!({
                    "name":"ovrcr_accept", "background":true, "output":output,
                    "args":{"handle":current.handle,"identity":current.identity,
                        "result_version":current.result_version,
                        "result_digest":result_digest,
                        "expected_control_epoch":current.control.epoch}
                })
            ),
            submit: true,
        }),
        Response::Ok
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read_to_string(fault.with_extension("hit"))
        .ok()
        .as_deref()
        != Some("review-final-auth")
    {
        assert!(
            Instant::now() < deadline,
            "full preauthentication never reached its exact final barrier"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    eprintln!(
        "REVIEW_FINAL_AUTHENTICATED handle={} report={} sha256={}",
        current.handle,
        reviewer.result_path.display(),
        digest(&original_report)
    );
    let mut corrupt: serde_json::Value = serde_json::from_slice(&original_report).unwrap();
    corrupt["summary"] = "changed only after complete initial review authentication".into();
    let corrupt = serde_json::to_vec(&corrupt).unwrap();
    assert_ne!(digest(&corrupt), digest(&original_report));
    fs::write(&reviewer.result_path, &corrupt).unwrap();
    fs::write(&release, "release").unwrap();
    wait_file(&output);
    let reply: serde_json::Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert!(
        reply.get("error").is_some()
            || matches!(
                reply["details"]["dispatch"].as_str(),
                Some("Refused" | "Unknown")
            ),
        "changed independent report cannot be accepted from cached preauthentication: {reply}"
    );
    assert_eq!(
        metadata(),
        before,
        "final refusal must not save an acceptance or obligation change"
    );
    assert!(serde_json::from_str::<serde_json::Value>(&before).unwrap()["acceptance"].is_null());
    assert_eq!(
        fs::read(&previous.path).unwrap(),
        previous_bytes,
        "the previously published typed decision remains immutable and unselected"
    );
    assert_eq!(fs::read(&reviewer.result_path).unwrap(), corrupt);
    assert_eq!(fixture.byte_count(&current), byte_count);
    assert_eq!(fixture.child_count(), 2);
}

#[test]
fn public_review_preserves_direct_contracts_but_never_falls_back_from_governed_stale_seats() {
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "CLEAN");
    assert!(current.review.is_none());
    let document = sources().pop().unwrap();
    let direct = ReviewAssignment {
        implementation: current.handle.clone(),
        axis: ReviewAxis::Spec,
        target: ReviewTarget {
            coding: target(&fixture, &current),
            sources: vec![document.source.clone()],
            view: None,
        },
        source_documents: vec![document],
        context_paths: vec![],
    };
    let started: AssignmentSnapshot = serde_json::from_value(fixture.tool("ovrcr_start", serde_json::json!({
        "project":PROJECT,"workspace":fixture.workspace,"scope":"Direct independent Spec review from its exact immutable typed contract",
        "review":direct,"model_provider":"test","model":"fixture-model","acceptance_evidence":["authenticated direct independent report"]
    }))).unwrap();
    assert_eq!(started.dispatch, DispatchStatus::Applied, "{started:?}");
    let completed = super::followups::complete(&fixture, &started);
    let published = publication_snapshot(&fixture, &completed);
    assert_ne!(published.identity.session, current.identity.session);
    let actual_target = &published
        .submission
        .as_ref()
        .unwrap()
        .review
        .as_ref()
        .unwrap()
        .target;
    assert_eq!(actual_target.coding, direct.target.coding);
    assert_eq!(actual_target.sources, direct.target.sources);
    assert!(
        actual_target.view.is_some(),
        "Prepare enriches the legacy direct request"
    );
    assert_eq!(
        published
            .submission
            .as_ref()
            .unwrap()
            .review
            .as_ref()
            .unwrap()
            .outcome,
        ReviewOutcome::Passed
    );
    let unchanged: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle":current.handle}),
    ))
    .unwrap();
    assert!(
        unchanged.review.is_none(),
        "direct review never invents a coordinator policy or seat"
    );
    assert_eq!(fixture.child_count(), 2);

    let governed = declared(&fixture, &current, false, None);
    let selected: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &governed,
        ReviewOperation::Assignment {
            axis: ReviewAxis::Spec,
        },
    ))
    .unwrap();
    let assignment = selected.review_assignment.unwrap();
    let scope = selected.review_scope.unwrap();
    let args = serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":scope,"review":assignment,
        "model_provider":"test","model":"fixture-model","acceptance_evidence":["governed independent report"]});
    let reviewer: AssignmentSnapshot =
        serde_json::from_value(fixture.tool("ovrcr_start", args.clone())).unwrap();
    assert_eq!(reviewer.dispatch, DispatchStatus::Applied, "{reviewer:?}");
    let reviewer = super::followups::complete(&fixture, &reviewer);
    let before = fixture.byte_count(&current);
    let duplicate = fixture.tool("ovrcr_start", args.clone());
    assert_eq!(duplicate["dispatch"], "Refused", "{duplicate}");
    assert!(
        duplicate["reason"]
            .as_str()
            .unwrap()
            .contains("original reviewer assignment"),
        "{duplicate}"
    );

    let mut changed = direct.source_documents.clone();
    changed[0].content.push_str(" Changed governing source.");
    changed[0].source.digest = digest(changed[0].content.as_bytes());
    let redeclared: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &governed,
        ReviewOperation::Declare(Box::new(ReviewDeclaration {
            request: "direct-compatibility-source-change".into(),
            target: direct.target.coding.clone(),
            requirements: vec![ReviewRequirement {
                axis: ReviewAxis::Spec,
                sources: vec![changed[0].source.clone()],
            }],
            sources: changed,
            repair_limit: None,
            context_paths: vec![],
        })),
    ))
    .unwrap();
    assert!(redeclared.review.as_ref().unwrap().evidence.is_empty());
    let stale = fixture.tool("ovrcr_start", args);
    assert_eq!(stale["dispatch"], "Refused", "{stale}");
    assert!(
        stale["reason"]
            .as_str()
            .unwrap()
            .contains("view is not the current declared axis/round"),
        "{stale}"
    );
    assert_eq!(
        fixture.child_count(),
        3,
        "governed refusal never opens a direct replacement seat"
    );
    assert_eq!(
        fixture.byte_count(&current),
        before,
        "no implementation input on either refused review"
    );
    assert_eq!(
        fixture.input_count(&reviewer),
        1,
        "the original governed reviewer is never replayed"
    );
}

fn incomplete_outer_review_cannot_satisfy_acceptance(
    mode: &str,
    outcome: &str,
    unresolved: &[&str],
) {
    let fixture = super::followups::OwnedFollowup::new();
    let worker = worker(&fixture, "CLEAN");
    let worker_input_count = fixture.input_count(&worker);
    assert_eq!(worker_input_count, 1);
    let worker_result = publication_snapshot(&fixture, &worker);
    assert_eq!(
        worker_result.submission.as_ref().unwrap().outcome,
        "success"
    );
    let checks = &worker_result
        .submission
        .as_ref()
        .unwrap()
        .coding
        .as_ref()
        .unwrap()
        .checks;
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].name, "addition");
    assert_eq!(checks[0].outcome, CheckOutcome::Passed);
    let document = source(
        ReviewSourceKind::Spec,
        "fixture://incomplete-review-completion",
        &format!(
            "REVIEW_FIXTURE_OUTER_{mode}: addition passes, but the enclosing independent review is incomplete."
        ),
    );
    let current: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &worker,
        ReviewOperation::Declare(Box::new(ReviewDeclaration {
            request: "incomplete-review-completion".into(),
            target: target(&fixture, &worker),
            requirements: vec![ReviewRequirement {
                axis: ReviewAxis::Spec,
                sources: vec![document.source.clone()],
            }],
            sources: vec![document],
            repair_limit: None,
            context_paths: vec![],
        })),
    ))
    .unwrap();
    let selected: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &current,
        ReviewOperation::Assignment {
            axis: ReviewAxis::Spec,
        },
    ))
    .unwrap();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_start",
        serde_json::json!({
            "project": PROJECT,
            "workspace": fixture.workspace,
            "scope": selected.review_scope.unwrap(),
            "review": selected.review_assignment.unwrap(),
            "model_provider": "test",
            "model": "fixture-model",
            "acceptance_evidence": ["exact typed review with incomplete enclosing completion"]
        }),
    ))
    .unwrap();
    assert_eq!(started.dispatch, DispatchStatus::Applied, "{started:?}");
    let reviewer = super::followups::complete(&fixture, &started);
    let published = publication_snapshot(&fixture, &reviewer);
    let submission = published.submission.as_ref().unwrap();
    assert_eq!(submission.outcome, outcome);
    assert_eq!(
        submission.unresolved,
        unresolved
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>()
    );
    let report = submission.review.as_ref().unwrap();
    assert_eq!(report.outcome, ReviewOutcome::Passed);
    assert!(report.findings.iter().all(|finding| !finding.blocking));
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(reviewer.brief.with_extension("json.artifact-check")).unwrap(),
    )
    .unwrap();
    assert_eq!(artifact["exit"], 0);
    let original_path = reviewer
        .brief
        .parent()
        .unwrap()
        .join(format!("submission-v{}.json", reviewer.result_version));
    let original_bytes = fs::read(&original_path).unwrap();
    assert_eq!(
        digest(&original_bytes),
        published.publication.as_ref().unwrap().identity.digest
    );
    let collected = review(
        &fixture,
        &current,
        ReviewOperation::Collect {
            axis: ReviewAxis::Spec,
            reviewer: reviewer.handle.clone(),
        },
    );
    let observed: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle": current.handle}),
    ))
    .unwrap();
    let acceptance = accept(&fixture, &current);
    assert_eq!(fs::read(&original_path).unwrap(), original_bytes);
    assert_eq!(fixture.child_count(), 2);
    assert_eq!(fixture.input_count(&current), worker_input_count);
    let status = observed.review.as_ref().unwrap();
    assert!(
        !status.independent_review_satisfied,
        "typed Passed with outer outcome={outcome}, unresolved={unresolved:?}: collected={collected}, status={status:?}, acceptance={acceptance}"
    );
    assert_eq!(acceptance["dispatch"], "Refused", "{acceptance}");
    let after: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_collect",
        serde_json::json!({"handle": current.handle}),
    ))
    .unwrap();
    assert!(after.acceptance_ref.is_none());
}

#[test]
fn public_review_passed_report_with_blocked_submission_never_satisfies_acceptance() {
    incomplete_outer_review_cannot_satisfy_acceptance("BLOCKED", "blocked", &[]);
}

#[test]
fn public_review_passed_report_with_failed_submission_never_satisfies_acceptance() {
    incomplete_outer_review_cannot_satisfy_acceptance("FAILED", "failed", &[]);
}

#[test]
fn public_review_passed_report_with_unresolved_submission_never_satisfies_acceptance() {
    incomplete_outer_review_cannot_satisfy_acceptance(
        "UNRESOLVED",
        "success",
        &["Required review coverage remains incomplete."],
    );
}

#[test]
fn public_review_direct_readable_context_uses_exact_base_and_replays_original_contract() {
    let fixture = super::followups::OwnedFollowup::new();
    fs::write(
        fixture.live.repo.join("context.txt"),
        "original unchanged caller\n",
    )
    .unwrap();
    live::git(&fixture.live.repo, &["add", "context.txt"]);
    live::git(&fixture.live.repo, &["commit", "-m", "original context"]);
    let current = worker(&fixture, "CLEAN");
    let document = sources().pop().unwrap();
    let direct = ReviewAssignment {
        implementation: current.handle.clone(),
        axis: ReviewAxis::Spec,
        target: ReviewTarget {
            coding: target(&fixture, &current),
            sources: vec![document.source.clone()],
            view: None,
        },
        source_documents: vec![document],
        context_paths: vec!["context.txt".into()],
    };
    // Both the original repository HEAD and implementation checkout move after capture.
    fs::write(fixture.live.repo.join("context.txt"), "later moving HEAD\n").unwrap();
    live::git(&fixture.live.repo, &["add", "context.txt"]);
    live::git(&fixture.live.repo, &["commit", "-m", "moving context"]);
    fs::write(
        current
            .resources
            .coding
            .as_ref()
            .unwrap()
            .path
            .join("calc.cjs"),
        "mutable checkout is not review input\n",
    )
    .unwrap();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.tool("ovrcr_start", serde_json::json!({
        "project":PROJECT,"workspace":fixture.workspace,"scope":"Preserve my direct review scope", "review":direct,
        "model_provider":"test","model":"fixture-model","acceptance_evidence":["complete readable exact-base context"]
    }))).unwrap();
    assert_eq!(started.dispatch, DispatchStatus::Applied, "{started:?}");
    let completed = super::followups::complete(&fixture, &started);
    let brief: serde_json::Value =
        serde_json::from_slice(&fs::read(&completed.brief).unwrap()).unwrap();
    assert_eq!(
        brief["contract"]["scope"],
        "Preserve my direct review scope"
    );
    let reference = &brief["contract"]["review"]["target"]["view"];
    assert_eq!(reference["round"], completed.handle);
    let path = reference["path"].as_str().unwrap();
    let original = fs::read(path).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let files = manifest["files"].as_array().unwrap();
    let context = files
        .iter()
        .find(|entry| entry["kind"] == "context")
        .unwrap();
    assert_eq!(
        fs::read_to_string(context["after"]["path"].as_str().unwrap()).unwrap(),
        "original unchanged caller\n"
    );
    let candidate = files
        .iter()
        .find(|entry| entry["path"] == "calc.cjs")
        .unwrap();
    assert_eq!(
        fs::read_to_string(candidate["after"]["path"].as_str().unwrap()).unwrap(),
        "module.exports = { add: (a, b) => a + b };\n"
    );
    let before = fs::read(&completed.brief).unwrap();
    let replay: AssignmentSnapshot = serde_json::from_value(fixture.tool(
        "ovrcr_start",
        serde_json::json!({"handle":completed.handle}),
    ))
    .unwrap();
    assert_eq!(replay.identity, completed.identity);
    assert_eq!(fixture.input_count(&completed), 1);
    assert_eq!(fs::read(path).unwrap(), original);
    assert_eq!(fs::read(&completed.brief).unwrap(), before);
    assert!(
        fixture.reconcile(&current.handle).review.is_none(),
        "direct review adds no coordinator stages or policy"
    );
    let private_files = || -> std::collections::BTreeSet<_> {
        fs::read_dir(direct.target.coding.candidate.path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
    };
    let original_files = private_files();
    let stale: ReviewAssignment =
        serde_json::from_value(brief["contract"]["review"].clone()).unwrap();
    let refused = fixture.tool("ovrcr_start", serde_json::json!({
        "project":PROJECT,"workspace":fixture.workspace,"scope":"Foreign direct request", "review":stale,
        "model_provider":"test","model":"fixture-model","acceptance_evidence":["fresh direct request binding"]
    }));
    assert_eq!(refused["dispatch"], "Refused", "{refused}");
    assert_eq!(
        private_files(),
        original_files,
        "a foreign view cannot leave private files"
    );
    assert_eq!(fixture.child_count(), 2);
}

#[test]
fn public_review_readable_artifact_tamper_modes_escape_and_swapped_axis_refuse_before_launch() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "CLEAN");
    let current = declared(&fixture, &current, true, None);
    let selected: AssignmentSnapshot = serde_json::from_value(review(
        &fixture,
        &current,
        ReviewOperation::Assignment {
            axis: ReviewAxis::Spec,
        },
    ))
    .unwrap();
    let assignment = selected.review_assignment.unwrap();
    let reference = assignment.target.view.as_ref().unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&reference.path).unwrap()).unwrap();
    let image = std::path::PathBuf::from(manifest["files"][0]["after"]["path"].as_str().unwrap());
    let original = fs::read(&image).unwrap();
    let count = fixture.child_count();
    for damage in ["tamper", "missing", "mode", "symlink"] {
        match damage {
            "tamper" => fs::write(&image, "swapped readable bytes").unwrap(),
            "missing" => fs::remove_file(&image).unwrap(),
            "mode" => fs::set_permissions(&image, fs::Permissions::from_mode(0o644)).unwrap(),
            "symlink" => {
                fs::remove_file(&image).unwrap();
                std::os::unix::fs::symlink("/etc/hosts", &image).unwrap();
            }
            _ => unreachable!(),
        }
        let result = fixture.tool("ovrcr_start", serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":selected.review_scope,"review":assignment,"model_provider":"test","model":"fixture-model","acceptance_evidence":["exact authenticated view"]}));
        assert_eq!(result["dispatch"], "Refused", "{damage}: {result}");
        assert_eq!(fixture.child_count(), count);
        if fs::symlink_metadata(&image).is_ok() {
            fs::remove_file(&image).unwrap();
        }
        fs::write(&image, &original).unwrap();
        fs::set_permissions(&image, fs::Permissions::from_mode(0o600)).unwrap();
    }
    for damage in ["axis", "round", "digest", "escape", "context"] {
        let mut changed = assignment.clone();
        let view = changed.target.view.as_mut().unwrap();
        match damage {
            "axis" => changed.axis = ReviewAxis::Standards,
            "round" => view.round = "another-round".into(),
            "digest" => view.digest = "0".repeat(64),
            "escape" => view.path = "/etc/hosts".into(),
            "context" => {
                view.context_paths.push("README".into());
                changed.context_paths = view.context_paths.clone();
            }
            _ => unreachable!(),
        }
        let result = fixture.tool("ovrcr_start", serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":selected.review_scope,"review":changed,"model_provider":"test","model":"fixture-model","acceptance_evidence":["exact authenticated view"]}));
        assert_eq!(result["dispatch"], "Refused", "{damage}: {result}");
        assert_eq!(fixture.child_count(), count);
    }
}

#[test]
fn public_review_necessary_binary_missing_private_and_directory_context_blocks_before_launch() {
    let fixture = super::followups::OwnedFollowup::new();
    fs::write(fixture.live.repo.join("binary-context"), [0, 255, 0]).unwrap();
    fs::create_dir(fixture.live.repo.join("context-dir")).unwrap();
    fs::write(fixture.live.repo.join("context-dir/file"), "caller\n").unwrap();
    live::git(
        &fixture.live.repo,
        &["add", "binary-context", "context-dir"],
    );
    live::git(
        &fixture.live.repo,
        &["commit", "-m", "context refusal inputs"],
    );
    let current = worker(&fixture, "CLEAN");
    let count = fixture.child_count();
    for path in [
        "binary-context",
        "missing",
        ".git/config",
        ".env",
        "../escape",
        "context-dir",
    ] {
        let document = sources().pop().unwrap();
        let assignment = ReviewAssignment {
            implementation: current.handle.clone(),
            axis: ReviewAxis::Spec,
            target: ReviewTarget {
                coding: target(&fixture, &current),
                sources: vec![document.source.clone()],
                view: None,
            },
            source_documents: vec![document],
            context_paths: vec![path.into()],
        };
        let result = fixture.tool("ovrcr_start",serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Necessary context must be readable", "review":assignment,"model_provider":"test","model":"fixture-model","acceptance_evidence":["complete necessary context"]}));
        assert_eq!(result["dispatch"], "Refused", "{path}: {result}");
        assert_eq!(fixture.child_count(), count);
    }
}

#[test]
fn public_review_necessary_line_beyond_installed_read_limit_blocks_without_truncation() {
    let fixture = super::followups::OwnedFollowup::new();
    // Pi 0.87.1 read has line offsets only and returns no bytes above 50 KiB/line.
    let original = "x".repeat(50 * 1024 + 1);
    fs::write(fixture.live.repo.join("necessary-long-line"), &original).unwrap();
    for (path, text) in [
        ("gif-text", "GIF89aordinary readable source\n"),
        ("webp-text", "RIFFtextWEBPordinary readable source\n"),
    ] {
        fs::write(fixture.live.repo.join(path), text).unwrap();
    }
    live::git(
        &fixture.live.repo,
        &["add", "necessary-long-line", "gif-text", "webp-text"],
    );
    live::git(
        &fixture.live.repo,
        &["commit", "-m", "necessary readonly context"],
    );
    let current = worker(&fixture, "CLEAN");
    let document = sources().pop().unwrap();
    let assignment = ReviewAssignment {
        implementation: current.handle.clone(),
        axis: ReviewAxis::Spec,
        target: ReviewTarget {
            coding: target(&fixture, &current),
            sources: vec![document.source.clone()],
            view: None,
        },
        source_documents: vec![document],
        context_paths: vec!["necessary-long-line".into()],
    };
    let candidate = fs::read(&assignment.target.coding.candidate.path).unwrap();
    // Directory::write uses exactly .<64 lower hex> while atomically replacing
    // observation checkpoints. Its concurrent scratch lifetime is not a publication.
    let publication_name = |name: &std::ffi::OsStr| {
        !name.to_str().is_some_and(|name| {
            name.len() == 65
                && name.starts_with('.')
                && name.as_bytes()[1..]
                    .iter()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
        })
    };
    assert!(!publication_name(std::ffi::OsStr::new(&format!(
        ".{}",
        "a0".repeat(32)
    ))));
    for retained in [
        format!(".{}", "A0".repeat(32)),
        format!(".{}", "g0".repeat(32)),
        format!(".{}", "a".repeat(63)),
        format!(".{}", "a".repeat(65)),
        "a".repeat(64),
        ".private-publication".into(),
        "review-view-round-spec.json".into(),
    ] {
        assert!(publication_name(std::ffi::OsStr::new(&retained)));
    }
    let publications = || -> std::collections::BTreeSet<_> {
        fs::read_dir(assignment.target.coding.candidate.path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| publication_name(name))
            .collect()
    };
    let before = publications();
    let result = fixture.tool("ovrcr_start",serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Read every byte of the necessary context", "review":assignment,"model_provider":"test","model":"fixture-model","acceptance_evidence":["complete ordinary readonly representation"]}));
    assert_eq!(result["dispatch"], "Refused", "{result}");
    let reason = result["reason"].as_str().unwrap();
    assert!(
        reason.contains("51200") && reason.contains("line"),
        "{result}"
    );
    assert_eq!(fixture.child_count(), 1);
    let after = publications();
    assert_eq!(
        after, before,
        "unsupported context must not publish partial views"
    );
    assert_eq!(
        fs::read(&assignment.target.coding.candidate.path).unwrap(),
        candidate
    );
    assert_eq!(
        fs::read_to_string(fixture.live.repo.join("necessary-long-line")).unwrap(),
        original
    );
    // Raw source fits 64 KiB, but escaping quotes puts >50 KiB on one brief line.
    let mut source_assignment = assignment.clone();
    source_assignment.context_paths.clear();
    let document = &mut source_assignment.source_documents[0];
    document.content = "\"".repeat(26 * 1024);
    document.source.digest = digest(document.content.as_bytes());
    source_assignment.target.sources = vec![document.source.clone()];
    let result = fixture.tool("ovrcr_start",serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Read the complete originating source", "review":source_assignment,"model_provider":"test","model":"fixture-model","acceptance_evidence":["complete serialized source without truncation"]}));
    assert_eq!(result["dispatch"], "Refused", "{result}");
    let reason = result["reason"].as_str().unwrap();
    assert!(
        reason.contains("51200") && reason.contains("line"),
        "{result}"
    );
    assert_eq!(fixture.child_count(), 1);
    let after_source = publications();
    assert_eq!(
        after_source, before,
        "unreadable source must not publish views"
    );
    for path in ["gif-text", "webp-text"] {
        let mut image_sniffed = assignment.clone();
        image_sniffed.context_paths = vec![path.into()];
        let result = fixture.tool("ovrcr_start",serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Read complete ordinary context text", "review":image_sniffed,"model_provider":"test","model":"fixture-model","acceptance_evidence":["context must be returned as text"]}));
        assert_eq!(result["dispatch"], "Refused", "{path}: {result}");
        assert!(
            result["reason"]
                .as_str()
                .unwrap()
                .contains("interprets these bytes as an image"),
            "{result}"
        );
        assert_eq!(fixture.child_count(), 1);
        let after = publications();
        assert_eq!(after, before, "unsupported context must not publish views");
    }
}

#[test]
fn public_review_binary_candidate_blocks_readonly_launch_without_omitting_the_file() {
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "BINARY_CLEAN");
    let document = sources().pop().unwrap();
    let assignment = ReviewAssignment {
        implementation: current.handle.clone(),
        axis: ReviewAxis::Spec,
        target: ReviewTarget {
            coding: target(&fixture, &current),
            sources: vec![document.source.clone()],
            view: None,
        },
        source_documents: vec![document],
        context_paths: vec![],
    };
    let original = fs::read(&assignment.target.coding.candidate.path).unwrap();
    let result = fixture.tool("ovrcr_start",serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Review every changed file", "review":assignment,"model_provider":"test","model":"fixture-model","acceptance_evidence":["complete readable candidate"]}));
    assert_eq!(result["dispatch"], "Refused", "{result}");
    assert!(
        result["reason"]
            .as_str()
            .unwrap()
            .contains("binary review content"),
        "{result}"
    );
    assert_eq!(fixture.child_count(), 1);
    assert_eq!(
        fs::read(&assignment.target.coding.candidate.path).unwrap(),
        original
    );
}

#[test]
fn public_review_publication_refuses_exact_passed_report_after_initial_input_view_tamper() {
    let fixture = super::followups::OwnedFollowup::new();
    let current = worker(&fixture, "CLEAN");
    let document = sources().pop().unwrap();
    let assignment = ReviewAssignment {
        implementation: current.handle.clone(),
        axis: ReviewAxis::Spec,
        target: ReviewTarget {
            coding: target(&fixture, &current),
            sources: vec![document.source.clone()],
            view: None,
        },
        source_documents: vec![document],
        context_paths: vec![],
    };
    let original_candidate = fs::read(&assignment.target.coding.candidate.path).unwrap();
    let original_submission = fs::read(&current.result_path).unwrap();
    fs::remove_file(fixture.live.root.path().join("allow-result")).unwrap();
    let started: AssignmentSnapshot = serde_json::from_value(fixture.tool("ovrcr_start",serde_json::json!({"project":PROJECT,"workspace":fixture.workspace,"scope":"Direct read-only semantic review", "review":assignment,"model_provider":"test","model":"fixture-model","acceptance_evidence":["exact typed Passed only for authenticated readable bytes"]}))).unwrap();
    assert_eq!(started.dispatch, DispatchStatus::Applied, "{started:?}");
    wait_file(&started.brief.with_extension("json.started"));
    assert_eq!(
        fixture.input_count(&started),
        1,
        "tamper occurs after actual initial input"
    );
    let brief: serde_json::Value =
        serde_json::from_slice(&fs::read(&started.brief).unwrap()).unwrap();
    let target: ReviewTarget =
        serde_json::from_value(brief["contract"]["review"]["target"].clone()).unwrap();
    let view = target.view.as_ref().unwrap();
    let original_manifest = fs::read(&view.path).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&original_manifest).unwrap();
    let image = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["kind"] == "candidate" && file["path"] == "calc.cjs")
        .unwrap()["after"]["path"]
        .as_str()
        .unwrap();
    let mut changed = fs::read(image).unwrap();
    changed.extend_from_slice(b"// changed after initial input; addition remains correct\n");
    fs::write(image, &changed).unwrap();
    fs::write(fixture.live.root.path().join("allow-result"), "release").unwrap();
    wait_file(&started.brief.with_extension("json.published"));
    let attempted: ResearchSubmission = serde_json::from_slice(
        &fs::read(started.brief.with_extension("json.review-attempted")).unwrap(),
    )
    .unwrap();
    let report = attempted.review.as_ref().unwrap();
    assert_eq!(report.target, target);
    assert_eq!(report.axis, ReviewAxis::Spec);
    assert_eq!(
        report.outcome,
        ReviewOutcome::Passed,
        "the harmless comment leaves the actual semantic control passing"
    );
    let reply: serde_json::Value =
        serde_json::from_slice(&fs::read(started.brief.with_extension("json.published")).unwrap())
            .unwrap();
    eprintln!(
        "READABLE_VIEW_PUBLICATION_ATTEMPT report={} reply={reply}",
        serde_json::to_string(report).unwrap()
    );
    assert!(
        reply.get("error").is_some(),
        "a post-admission byte change must refuse the otherwise exact Passed report before publication: {reply}"
    );
    assert!(
        !started.result_path.exists(),
        "refused review must not acquire durable submission bytes"
    );
    let reconciled = fixture.reconcile(&started.handle);
    assert!(reconciled.publication.is_none() && reconciled.submission.is_none());
    assert_eq!(fs::read(&view.path).unwrap(), original_manifest);
    assert_eq!(
        fs::read(&assignment.target.coding.candidate.path).unwrap(),
        original_candidate
    );
    assert_eq!(fs::read(&current.result_path).unwrap(), original_submission);
    assert_eq!(fixture.child_count(), 2);
}

#[test]
fn public_review_exact_accepted_receipt_replays_after_later_view_tamper() {
    let fixture = super::followups::OwnedFollowup::new();
    let initial = worker(&fixture, "CLEAN");
    let current = declared(&fixture, &initial, false, None);
    let reviewer = axis(&fixture, &current, ReviewAxis::Spec);
    let current = fixture.reconcile(&current.handle);
    let published = publication_snapshot(&fixture, &current);
    let args = serde_json::json!({
        "handle":current.handle,"identity":current.identity,"result_version":current.result_version,
        "result_digest":published.publication.as_ref().unwrap().identity.digest,
        "expected_control_epoch":current.control.epoch
    });
    let accepted: AssignmentSnapshot =
        serde_json::from_value(fixture.tool("ovrcr_accept", args.clone())).unwrap();
    assert_eq!(accepted.dispatch, DispatchStatus::Applied, "{accepted:?}");
    let receipt = accepted
        .acceptance_ref
        .clone()
        .expect("original acceptance committed");
    let receipt_bytes = fs::read(&receipt.path).unwrap();
    let reference = &accepted.review.as_ref().unwrap().evidence[0];
    let view = reference.target.view.as_ref().unwrap();
    let manifest_bytes = fs::read(&view.path).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
    let image = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["kind"] == "candidate" && file["path"] == "calc.cjs")
        .unwrap()["after"]["path"]
        .as_str()
        .unwrap();
    let image_bytes = fs::read(image).unwrap();
    let report_bytes = fs::read(&reviewer.result_path).unwrap();
    let candidate_bytes = fs::read(&reference.target.coding.candidate.path).unwrap();
    let metadata = || {
        let db = rusqlite::Connection::open_with_flags(
            fixture.live.config.join("registry.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        db.query_row(
            "SELECT metadata FROM delegations WHERE handle=?1",
            [&current.handle],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    };
    let original_metadata = metadata();
    let input_count = fixture.input_count(&accepted);
    let byte_count = fixture.byte_count(&accepted);
    let mut tampered = image_bytes.clone();
    tampered.extend_from_slice(b"// private view changed after durable acceptance\n");
    fs::write(image, &tampered).unwrap();
    let replay = fixture.tool("ovrcr_accept", args.clone());
    eprintln!("EXACT_ACCEPTED_RECEIPT_REPLAY original={receipt:?} reply={replay}");
    assert_eq!(
        replay["dispatch"], "Applied",
        "an exact committed receipt replay must not become a first acceptance: {replay}"
    );
    let replay: AssignmentSnapshot = serde_json::from_value(replay).unwrap();
    assert_eq!(replay.acceptance_ref, Some(receipt.clone()));
    assert_eq!(replay.obligation_revision, accepted.obligation_revision);
    assert_eq!(metadata(), original_metadata);
    assert_eq!(fs::read(&receipt.path).unwrap(), receipt_bytes);
    assert_eq!(fs::read(image).unwrap(), tampered);
    assert_eq!(fs::read(&view.path).unwrap(), manifest_bytes);
    assert_eq!(fs::read(&reviewer.result_path).unwrap(), report_bytes);
    assert_eq!(
        fs::read(&reference.target.coding.candidate.path).unwrap(),
        candidate_bytes
    );
    assert_eq!(fixture.input_count(&accepted), input_count);
    assert_eq!(fixture.byte_count(&accepted), byte_count);
    assert_eq!(fixture.child_count(), 2);
    // Check request fences with a valid view, so tamper cannot mask their rejection.
    fs::write(image, &image_bytes).unwrap();
    for damage in ["target", "identity", "version", "control"] {
        let mut changed = args.clone();
        match damage {
            "target" => changed["result_digest"] = "0".repeat(64).into(),
            "identity" => changed["identity"]["contract"] = (current.identity.contract + 1).into(),
            "version" => changed["result_version"] = (current.result_version + 1).into(),
            "control" => changed["expected_control_epoch"] = (current.control.epoch + 1).into(),
            _ => unreachable!(),
        }
        let rejected = fixture.tool("ovrcr_accept", changed);
        assert_eq!(rejected["dispatch"], "Refused", "{damage}: {rejected}");
        assert_eq!(metadata(), original_metadata);
        assert_eq!(fs::read(&receipt.path).unwrap(), receipt_bytes);
        assert_eq!(fixture.input_count(&accepted), input_count);
        assert_eq!(fixture.byte_count(&accepted), byte_count);
    }
}

#[test]
fn public_review_first_parent_coding_acceptance_requires_the_exact_passed_reference_view() {
    for tamper in [false, true] {
        let fixture = super::followups::OwnedFollowup::new();
        let initial = worker(&fixture, "CLEAN");
        let current = declared(&fixture, &initial, false, None);
        let reviewer = axis(&fixture, &current, ReviewAxis::Spec);
        let current = fixture.reconcile(&current.handle);
        let reference = current.review.as_ref().unwrap().evidence[0].clone();
        let report_bytes = fs::read(&reviewer.result_path).unwrap();
        let report: ResearchSubmission = serde_json::from_slice(&report_bytes).unwrap();
        assert_eq!(
            report.review.as_ref().unwrap().outcome,
            ReviewOutcome::Passed
        );
        assert_eq!(report.review.as_ref().unwrap().target, reference.target);
        assert!(
            current.acceptance_ref.is_none(),
            "each case reaches its first parent acceptance independently"
        );
        let original_candidate = fs::read(&reference.target.coding.candidate.path).unwrap();
        let view = reference.target.view.as_ref().unwrap();
        let original_manifest = fs::read(&view.path).unwrap();
        let metadata = || {
            let db = rusqlite::Connection::open_with_flags(
                fixture.live.config.join("registry.sqlite3"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            db.query_row(
                "SELECT metadata FROM delegations WHERE handle=?1",
                [&current.handle],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        let before = metadata();
        if tamper {
            let manifest: serde_json::Value = serde_json::from_slice(&original_manifest).unwrap();
            let image = manifest["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|file| file["path"] == "calc.cjs")
                .unwrap()["after"]["path"]
                .as_str()
                .unwrap();
            let mut changed = fs::read(image).unwrap();
            changed.extend_from_slice(b"// tampered after exact typed Passed publication\n");
            fs::write(image, changed).unwrap();
        }
        let outcome = accept(&fixture, &current);
        eprintln!("READABLE_VIEW_FIRST_PARENT_ACCEPTANCE tamper={tamper} outcome={outcome}");
        if tamper {
            assert_eq!(outcome["dispatch"], "Refused", "{outcome}");
            assert_eq!(
                metadata(),
                before,
                "refusal cannot add acceptance or rewrite retained provenance"
            );
            assert!(
                !current
                    .result_path
                    .parent()
                    .unwrap()
                    .join("acceptance-v1.json")
                    .exists()
            );
        } else {
            let accepted: AssignmentSnapshot = serde_json::from_value(outcome).unwrap();
            assert!(
                accepted.acceptance_ref.is_some(),
                "clean exact Passed control must accept"
            );
            assert_eq!(
                accepted.review.as_ref().unwrap().evidence,
                vec![reference.clone()]
            );
        }
        assert_eq!(fs::read(&reviewer.result_path).unwrap(), report_bytes);
        assert_eq!(fs::read(&view.path).unwrap(), original_manifest);
        assert_eq!(
            fs::read(&reference.target.coding.candidate.path).unwrap(),
            original_candidate
        );
        assert_eq!(fixture.child_count(), 2);
    }
}

#[cfg(feature = "acceptance-diagnostics")]
#[test]
fn public_review_retired_authority_after_view_preparation_publishes_no_files_or_assignment() {
    for direct in [true, false] {
        let fixture = super::followups::OwnedFollowup::new();
        let current = worker(&fixture, "CLEAN");
        let document = sources().pop().unwrap();
        let request = if direct {
            "direct-view-authority-race"
        } else {
            "declared-view-authority-race"
        };
        let implementation_root = current.result_path.parent().unwrap();
        let names = || {
            let mut names: Vec<_> = fs::read_dir(implementation_root)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        let metadata = || {
            let db = rusqlite::Connection::open_with_flags(
                fixture.live.config.join("registry.sqlite3"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let original: String = db
                .query_row(
                    "SELECT metadata FROM delegations WHERE handle=?1",
                    [&current.handle],
                    |row| row.get(0),
                )
                .unwrap();
            let rows: u32 = db
                .query_row("SELECT count(*) FROM delegations", [], |row| row.get(0))
                .unwrap();
            (original, rows)
        };
        let original_names = names();
        let original_metadata = metadata();
        let original_binding = || {
            let Response::Inventory { sessions, .. } = fixture.live.request(Request::Inspect)
            else {
                panic!("inventory")
            };
            sessions
                .into_iter()
                .find(|session| session.id == fixture.parent)
                .unwrap()
                .agent
                .unwrap()
                .binding
        };
        let original = original_binding();
        let fault = fixture.live.root.path().join("fault");
        let release = fault.with_extension("release");
        struct Release(std::path::PathBuf);
        impl Drop for Release {
            fn drop(&mut self) {
                let _ = fs::write(&self.0, "release");
            }
        }
        let _release = Release(release.clone());
        fs::write(&fault, format!("review-view-prepared|{request}|0")).unwrap();
        let output = fixture.live.root.path().join("review-view-authority.json");
        let tool = if direct {
            serde_json::json!({"name":"ovrcr_start","background":true,"output":output,"args":{
                "request":request,"project":PROJECT,"workspace":fixture.workspace,"scope":"Direct bounded review","model_provider":"test","model":"fixture-model","acceptance_evidence":["current authority at readable publication"],
                "review":{"implementation":current.handle,"axis":"Spec","target":{"coding":target(&fixture,&current),"sources":[document.source]},"source_documents":[document]}
            }})
        } else {
            serde_json::json!({"name":"ovrcr_review","background":true,"output":output,"args":{"handle":current.handle,"operation":{"Declare":{
                "request":request,"target":target(&fixture,&current),"requirements":[{"axis":"Spec","sources":[document.source]}],"sources":[document],"repair_limit":null
            }}}})
        };
        assert_eq!(
            fixture.live.request(Request::SendTerminal {
                session: fixture.parent,
                text: format!("tool:{tool}"),
                submit: true
            }),
            Response::Ok
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while fs::read_to_string(fault.with_extension("hit"))
            .ok()
            .as_deref()
            != Some("review-view-prepared")
        {
            assert!(
                Instant::now() < deadline,
                "readable content preparation never reached its production publication boundary"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            names(),
            original_names,
            "in-memory preparation creates no private view files"
        );
        for conversation in ["review-parent-replacement", original.conversation.as_str()] {
            assert_eq!(
                fixture.live.request(Request::SendTerminal {
                    session: fixture.parent,
                    text: format!("session_start:{conversation}:new"),
                    submit: true
                }),
                Response::Ok
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            while {
                let observed = original_binding();
                observed.conversation != conversation || observed.generation <= original.generation
            } {
                assert!(
                    Instant::now() < deadline,
                    "actual parent source retirement not observed"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        fs::write(&release, "release").unwrap();
        wait_file(&output);
        let reply: serde_json::Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert!(
            reply.get("error").is_some()
                || matches!(
                    reply["details"]["dispatch"].as_str(),
                    Some("Refused" | "Unknown")
                ),
            "retired source cannot publish a readable view: {reply}"
        );
        assert_eq!(
            names(),
            original_names,
            "stale authority must leave no readable images, manifest or declaration"
        );
        assert_eq!(
            metadata(),
            original_metadata,
            "stale authority must not save review policy or a reviewer assignment"
        );
        assert_eq!(fixture.child_count(), 1);
        assert_eq!(fixture.input_count(&current), 1);
    }
}

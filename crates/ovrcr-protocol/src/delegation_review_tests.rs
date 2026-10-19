use super::*;

fn target() -> CodingTargetReference {
    serde_json::from_value(serde_json::json!({
        "assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"implementation","attempt":1,"contract":1,"request":"implementation","session":2,"run":1},
        "publication":{"assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"implementation","attempt":1,"contract":1,"request":"implementation","session":2,"run":1},"kind":"Submission","version":1,"digest":"a".repeat(64)},
        "candidate":{"path":"/private/task/candidate-v1.json","digest":"b".repeat(64),"workspace":"coding","base":"c".repeat(40),"git_identity":"generation","changed_artifacts":["untracked.cjs"]},
        "source_contract_digest":"d".repeat(64)
    })).unwrap()
}
fn document(kind: ReviewSourceKind) -> ReviewSource {
    ReviewSource {
        source: ReviewSourceReference {
            kind,
            reference: "fixture://actual-source".into(),
            digest: "e".repeat(64),
        },
        content: "actual source document".into(),
    }
}
fn assignment(axis: ReviewAxis) -> ReviewAssignment {
    let kind = match axis {
        ReviewAxis::Standards => ReviewSourceKind::Method,
        ReviewAxis::Spec => ReviewSourceKind::Spec,
    };
    let source = document(kind);
    ReviewAssignment {
        implementation: "implementation".into(),
        axis,
        target: ReviewTarget {
            coding: target(),
            sources: vec![source.source.clone()],
            view: None,
        },
        source_documents: vec![source],
        context_paths: vec![],
    }
}

#[test]
fn review_axes_have_separate_exact_source_contracts_and_actual_documents() {
    for axis in [ReviewAxis::Standards, ReviewAxis::Spec] {
        let original = assignment(axis);
        original.validate().unwrap();
        let mut foreign = original.clone();
        foreign.target.sources[0].kind = match original.axis {
            ReviewAxis::Standards => ReviewSourceKind::Spec,
            ReviewAxis::Spec => ReviewSourceKind::Standards,
        };
        foreign.source_documents[0].source = foreign.target.sources[0].clone();
        assert!(foreign.validate().is_err());
        let mut missing = original;
        missing.source_documents.clear();
        assert!(missing.validate().is_err());
    }
    let mut permuted = assignment(ReviewAxis::Standards);
    let mut additional = document(ReviewSourceKind::Standards);
    additional.source.reference = "fixture://repository-standards".into();
    permuted.target.sources.push(additional.source.clone());
    permuted.source_documents.insert(0, additional);
    permuted.validate().unwrap();
    let mut duplicate = permuted.clone();
    duplicate.source_documents[0] = duplicate.source_documents[1].clone();
    assert!(duplicate.validate().is_err());
}
#[test]
fn review_report_cannot_pass_with_blockers_or_borrow_another_axis_source() {
    let assignment = assignment(ReviewAxis::Spec);
    let finding = ReviewFinding {
        id: "defect".into(),
        blocking: true,
        location: "untracked.cjs:1".into(),
        finding: "Actual executable assertion failed.".into(),
        source: assignment.target.sources[0].clone(),
    };
    let mut report = ReviewReport {
        axis: assignment.axis,
        target: assignment.target,
        outcome: ReviewOutcome::ChangesRequired,
        findings: vec![finding.clone()],
    };
    report.validate().unwrap();
    report.outcome = ReviewOutcome::Passed;
    assert!(report.validate().is_err());
    report.outcome = ReviewOutcome::ChangesRequired;
    report.findings[0].source.digest = "f".repeat(64);
    assert!(report.validate().is_err());
    report.findings = vec![finding.clone(), finding];
    assert!(report.validate().is_err());
}
#[test]
fn review_target_refuses_inconsistent_publication_identity_and_head_only_inputs() {
    let original = assignment(ReviewAxis::Spec);
    let mut stale = original.clone();
    stale.target.coding.publication.assignment.attempt += 1;
    assert!(stale.validate().is_err());
    let mut head_only = original;
    head_only.target.coding.candidate.digest.clear();
    assert!(head_only.validate().is_err());
}
#[test]
fn repair_policy_is_finite_and_default_provenance_does_not_claim_approval() {
    let mut limit = RepairLimit::default();
    assert_eq!(limit.rounds, 3);
    assert_eq!(limit.source, RepairLimitSource::CoordinatorDefault);
    assert!(limit.reference.contains("not user approval"));
    limit.validate().unwrap();
    for invalid in [0, 33, u32::MAX] {
        limit.rounds = invalid;
        assert!(limit.validate().is_err());
    }
}
#[test]
fn review_policy_and_exact_report_roundtrip_over_the_production_codec() {
    let mut assignment = assignment(ReviewAxis::Spec);
    assignment.context_paths = vec!["src/caller.rs".into()];
    assignment.target.view = Some(ReviewViewReference {
        path: "/private/task/review-view-round-spec.json".into(),
        digest: "f".repeat(64),
        round: "round".into(),
        context_paths: assignment.context_paths.clone(),
    });
    assignment.validate().unwrap();
    let report = ReviewReport {
        axis: assignment.axis,
        target: assignment.target,
        outcome: ReviewOutcome::Passed,
        findings: vec![],
    };
    let bytes = bincode::serde::encode_to_vec(&report, bincode::config::standard()).unwrap();
    let (decoded, used): (ReviewReport, usize) =
        bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
    assert_eq!(decoded, report);
    assert_eq!(used, bytes.len());
}

#[test]
fn readable_review_context_refuses_recursion_duplicates_traversal_and_stale_selection() {
    let mut original = assignment(ReviewAxis::Spec);
    original.context_paths = vec!["src/caller.rs".into()];
    original.target.view = Some(ReviewViewReference {
        path: "/private/view.json".into(),
        digest: "f".repeat(64),
        round: "round".into(),
        context_paths: original.context_paths.clone(),
    });
    original.validate().unwrap();
    for paths in [
        vec!["../private".into()],
        vec!["/absolute".into()],
        vec!["src//file".into()],
        vec!["same".into(), "same".into()],
        (0..33).map(|i| format!("file-{i}")).collect(),
    ] {
        assert!(validate_context_paths(&paths).is_err(), "{paths:?}");
    }
    let mut stale = original.clone();
    stale.context_paths.clear();
    assert!(stale.validate().is_err());
    original.target.view.as_mut().unwrap().round = "../escape".into();
    assert!(original.validate().is_err());
}

#[test]
fn canonical_submission_bytes_preserve_literal_old_research_and_coding_and_bind_review() {
    const RESEARCH: &str = r#"{"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"success","summary":"Original report.","evidence":[],"unresolved":[]}"#;
    const CODING: &str = r#"{"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"success","summary":"Original report.","evidence":[],"unresolved":[],"coding":{"checks":[],"candidate":null}}"#;
    // Intentional accepted332 union compatibility: review Some retains coding:null.
    // These are literal authenticated bytes, independent of the serializer under test.
    const RESEARCH_REVIEW: &str = r#"{"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"success","summary":"Original report.","evidence":[],"unresolved":[],"coding":null,"review":{"target":{"coding":{"assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"implementation","attempt":1,"contract":1,"request":"implementation","session":2,"run":1},"publication":{"assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"implementation","attempt":1,"contract":1,"request":"implementation","session":2,"run":1},"kind":"Submission","version":1,"digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"candidate":{"path":"/private/task/candidate-v1.json","digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","workspace":"coding","base":"cccccccccccccccccccccccccccccccccccccccc","git_identity":"generation","changed_artifacts":["untracked.cjs"]},"source_contract_digest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"},"sources":[{"kind":"Spec","reference":"fixture://actual-source","digest":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"}]},"axis":"Spec","outcome":"Passed","findings":[]}}"#;
    const CODING_REVIEW: &str = r#"{"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"success","summary":"Original report.","evidence":[],"unresolved":[],"coding":{"checks":[],"candidate":null},"review":{"target":{"coding":{"assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"implementation","attempt":1,"contract":1,"request":"implementation","session":2,"run":1},"publication":{"assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"implementation","attempt":1,"contract":1,"request":"implementation","session":2,"run":1},"kind":"Submission","version":1,"digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"candidate":{"path":"/private/task/candidate-v1.json","digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","workspace":"coding","base":"cccccccccccccccccccccccccccccccccccccccc","git_identity":"generation","changed_artifacts":["untracked.cjs"]},"source_contract_digest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"},"sources":[{"kind":"Spec","reference":"fixture://actual-source","digest":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"}]},"axis":"Spec","outcome":"Passed","findings":[]}}"#;
    for (original, expected_review) in [(RESEARCH, RESEARCH_REVIEW), (CODING, CODING_REVIEW)] {
        let mut submission: crate::ResearchSubmission = serde_json::from_str(original).unwrap();
        assert_eq!(submission.publication_bytes().unwrap(), original.as_bytes());
        let selected = assignment(ReviewAxis::Spec);
        submission.review = Some(ReviewReport {
            target: selected.target,
            axis: selected.axis,
            outcome: ReviewOutcome::Passed,
            findings: vec![],
        });
        assert_eq!(
            submission.publication_bytes().unwrap(),
            expected_review.as_bytes()
        );
        let literal: crate::ResearchSubmission = serde_json::from_str(expected_review).unwrap();
        assert_eq!(literal, submission);
        assert_eq!(
            literal.publication_bytes().unwrap(),
            expected_review.as_bytes()
        );
        let (roundtrip, used): (crate::ResearchSubmission, usize) =
            bincode::serde::decode_from_slice(
                &bincode::serde::encode_to_vec(&submission, bincode::config::standard()).unwrap(),
                bincode::config::standard(),
            )
            .unwrap();
        assert_eq!(roundtrip, submission);
        assert!(used > 0);
    }
}

#[test]
fn absent_review_contract_keeps_literal_human_bytes_and_binary_option() {
    const OLD: &str = r#"{"schema":1,"workflow":"workflow","controller":"parent","task":"task","request":"request","project":"project","workspace":"workspace","scope":"Read README","permitted_actions":["read"],"provider":"pi","model_provider":"test","model":"fixture-model","acceptance_evidence":["citation"],"mandatory_capabilities":["read","result_file"],"limits":{"max_live_children":null,"max_execution_attempts":null,"min_free_slots_after_launch":0,"max_cost_microusd":null,"max_output_tokens":null,"max_native_children":null},"previous_request":null,"coding":null}"#;
    let contract: crate::ResearchContract = serde_json::from_str(OLD).unwrap();
    assert!(contract.review.is_none());
    assert_eq!(serde_json::to_vec(&contract).unwrap(), OLD.as_bytes());
    let encoded = bincode::serde::encode_to_vec(&contract, bincode::config::standard()).unwrap();
    let (decoded, used): (crate::ResearchContract, usize) =
        bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
    assert_eq!(decoded, contract);
    assert_eq!(used, encoded.len());
    assert!(crate::require_typed_review_provider("pi").is_ok());
    for provider in ["claude", "codex", "grok"] {
        assert!(
            crate::require_typed_review_provider(provider)
                .unwrap_err()
                .contains("typed independent-review")
        );
    }
}

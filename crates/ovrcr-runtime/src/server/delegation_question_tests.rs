use super::*;
use ovrcr_protocol::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn identity() -> AssignmentIdentity {
    AssignmentIdentity {
        instance: "instance".into(),
        workflow: "workflow".into(),
        controller: "parent".into(),
        controller_epoch: 1,
        task: "task".into(),
        attempt: 1,
        contract: 1,
        request: "start".into(),
        session: Some(SessionId(2)),
        run: Some(SessionRunId(3)),
    }
}

fn binding() -> AgentBinding {
    AgentBinding {
        provider: AgentProvider::Pi,
        invocation: "invocation".into(),
        conversation: "conversation".into(),
        generation: 1,
    }
}

fn question() -> AssignmentQuestion {
    AssignmentQuestion {
        schema: DELEGATION_SCHEMA,
        identity: identity(),
        result_version: 3,
        question_id: "source".into(),
        question_version: 1,
        supersedes: None,
        kind: QuestionKind::Clarification,
        question: "Which source should I use?\nKeep these exact bytes. λ".into(),
        context: "The code and an old document disagree.".into(),
        choices: vec!["Current implementation".into(), "Old document".into()],
    }
}

fn prepare(question: &AssignmentQuestion, saved: &[QuestionPublicationIntent]) -> PreparedQuestion {
    PreparedQuestion::prepare(question, &identity(), 3, &binding(), saved).unwrap()
}

fn private_root() -> tempfile::TempDir {
    // macOS temp paths can contain /var -> /private/var. The production Directory
    // intentionally refuses every symlink component, including that system alias.
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    println!("QUESTION_FILE_ROOT={}", root.path().display());
    root
}

#[test]
fn complete_files_require_authenticated_intent_and_remain_private() {
    let temp = private_root();
    let directory = Directory::open(temp.path(), false).unwrap();
    let question = question();
    let prepared = prepare(&question, &[]);
    let saved = vec![prepared.intent.clone()];
    // Production saves this same intent inside the binding fence before linking.
    prepared.publish(&directory).unwrap();
    let reference = &saved[0].reference;
    assert_eq!(
        read_question(&directory, reference, &[]).unwrap_err(),
        PublicationIssue::Uncommitted
    );
    assert_eq!(
        read_question(&directory, reference, &saved).unwrap(),
        question
    );
    let path = temp.path().join(question_name(reference).unwrap());
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::read(path).unwrap(),
        serde_json::to_vec(&question).unwrap()
    );
}

#[test]
fn exact_replay_recovers_linked_bytes_but_changed_payload_or_binding_cannot_replace() {
    let temp = private_root();
    let directory = Directory::open(temp.path(), false).unwrap();
    let mut question = question();
    let prepared = prepare(&question, &[]);
    let saved = vec![prepared.intent.clone()];
    prepared.publish(&directory).unwrap();
    prepare(&question, &saved).publish(&directory).unwrap();
    question.question.push_str(" Altered content.");
    assert!(PreparedQuestion::prepare(&question, &identity(), 3, &binding(), &saved).is_err());
    question = self::question();
    let mut changed_binding = binding();
    changed_binding.generation += 1;
    assert!(
        PreparedQuestion::prepare(&question, &identity(), 3, &changed_binding, &saved).is_err()
    );
    assert_eq!(
        read_question(&directory, &saved[0].reference, &saved).unwrap(),
        question
    );
}

#[test]
fn question_revisions_preserve_history_without_restoring_old_publication_authority() {
    let temp = private_root();
    let directory = Directory::open(temp.path(), false).unwrap();
    let original = question();
    let first = prepare(&original, &[]);
    let mut saved = vec![first.intent.clone()];
    first.publish(&directory).unwrap();
    let mut revised = original.clone();
    revised.question_version = 2;
    revised.supersedes = Some(first.intent.reference.clone());
    revised.question = "Which part of the current implementation?".into();
    let second = prepare(&revised, &saved);
    let mut changed_binding = binding();
    changed_binding.generation += 1;
    assert!(PreparedQuestion::prepare(&revised, &identity(), 3, &changed_binding, &saved).is_err());
    saved.push(second.intent.clone());
    second.publish(&directory).unwrap();
    assert_eq!(
        read_question(&directory, &first.intent.reference, &saved).unwrap(),
        original
    );
    assert_eq!(
        read_question(&directory, &second.intent.reference, &saved).unwrap(),
        revised
    );
    let error = PreparedQuestion::prepare(&original, &identity(), 3, &binding(), &saved)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("superseded"));
    assert!(PreparedQuestion::prepare(&revised, &identity(), 4, &binding(), &saved).is_err());
    let mut forged = revised.clone();
    forged.question_version = 3;
    forged.supersedes = Some(first.intent.reference.clone());
    assert!(PreparedQuestion::prepare(&forged, &identity(), 3, &binding(), &saved).is_err());
}

#[test]
fn full_history_refuses_new_questions_before_files_change_and_preserves_replay() {
    let temp = private_root();
    let directory = Directory::open(temp.path(), false).unwrap();
    let mut saved = Vec::new();
    let mut first = None;
    for index in 0..MAX_ASSIGNMENT_QUESTIONS {
        let mut next = question();
        next.question_id = format!("question-{index}");
        let prepared = prepare(&next, &saved);
        saved.push(prepared.intent.clone());
        prepared.publish(&directory).unwrap();
        first.get_or_insert(next);
    }
    let count = std::fs::read_dir(temp.path()).unwrap().count();
    assert_eq!(count, MAX_ASSIGNMENT_QUESTIONS);
    assert!(PreparedQuestion::prepare(&question(), &identity(), 3, &binding(), &saved).is_err());
    prepare(&first.unwrap(), &saved)
        .publish(&directory)
        .unwrap();
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), count);
    assert_eq!(saved.len(), MAX_ASSIGNMENT_QUESTIONS);
}

#[test]
fn partial_foreign_and_symlinked_question_bytes_cannot_satisfy_saved_intent() {
    let temp = private_root();
    let directory = Directory::open(temp.path(), false).unwrap();
    let prepared = prepare(&question(), &[]);
    let saved = vec![prepared.intent.clone()];
    let reference = &saved[0].reference;
    let name = question_name(reference).unwrap();
    directory.publish(&name, b"{\"schema\":1").unwrap();
    assert_eq!(
        read_question(&directory, reference, &saved).unwrap_err(),
        PublicationIssue::Invalid
    );
    assert!(prepared.publish(&directory).is_err());
    let mut foreign = question();
    foreign.identity.run = Some(SessionRunId(99));
    directory
        .replace(&name, &serde_json::to_vec(&foreign).unwrap())
        .unwrap();
    assert_eq!(
        read_question(&directory, reference, &saved).unwrap_err(),
        PublicationIssue::Invalid
    );
    std::fs::remove_file(temp.path().join(&name)).unwrap();
    let outside = private_root();
    let outside_path = outside.path().join("outside.json");
    std::fs::write(&outside_path, serde_json::to_vec(&question()).unwrap()).unwrap();
    symlink(&outside_path, temp.path().join(&name)).unwrap();
    assert_eq!(
        read_question(&directory, reference, &saved).unwrap_err(),
        PublicationIssue::Unavailable
    );
    assert!(prepared.publish(&directory).is_err());
    assert_eq!(
        std::fs::read(outside_path).unwrap(),
        serde_json::to_vec(&question()).unwrap()
    );
}

#[test]
fn answer_artifact_is_immutable_and_retains_exact_question_result_and_epoch() {
    let temp = private_root();
    let directory = Directory::open(temp.path(), false).unwrap();
    let prepared_question = prepare(&question(), &[]);
    let reference = prepared_question.intent.reference;
    let mut answer = QuestionAnswer {
        schema: DELEGATION_SCHEMA,
        request: "answer-once".into(),
        question: reference.clone(),
        control_epoch: 7,
        basis: AnswerBasis::ExistingAuthority,
        scope: "Choose a source for this question only.".into(),
        answer: "Use the current implementation.".into(),
    };
    let first = PreparedAnswer::prepare(&answer, &reference, 7, 1, None).unwrap();
    first.publish(&directory).unwrap();
    let replay = PreparedAnswer::prepare(&answer, &reference, 7, 1, Some(&first.intent)).unwrap();
    replay.publish(&directory).unwrap();
    assert_eq!(first.intent, replay.intent);
    assert_eq!(replay.intent.question.result_version, 3);
    assert_eq!(replay.intent.control_epoch, 7);
    assert_eq!(replay.intent.expected_parent_epoch, 1);
    assert!(
        PreparedAnswer::prepare(&answer, &reference, 7, 2, Some(&first.intent)).is_err(),
        "a new parent epoch cannot adopt a saved unsent answer"
    );
    assert!(PreparedAnswer::prepare(&answer, &reference, 7, 0, None).is_err());
    answer.scope.push_str(" Broader authority.");
    assert!(PreparedAnswer::prepare(&answer, &reference, 7, 1, Some(&first.intent)).is_err());
    answer.scope = "Choose a source for this question only.".into();
    assert!(PreparedAnswer::prepare(&answer, &reference, 8, 1, Some(&first.intent)).is_err());
    let mut superseded = reference.clone();
    superseded.question_version += 1;
    assert!(PreparedAnswer::prepare(&answer, &superseded, 7, 1, Some(&first.intent)).is_err());
    let bytes = directory.read("answer-answer-once.json").unwrap();
    let saved: QuestionAnswer = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved.question, reference);
    assert_eq!(saved.scope, "Choose a source for this question only.");
    assert_eq!(saved.answer, "Use the current implementation.");
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
}

fn answer_record(outcome: DispatchOutcome) -> (tempfile::TempDir, Record) {
    let temp = private_root();
    let mut record =
        super::super::tests::record(temp.path().join("assignment"), DispatchStatus::Applied);
    record.upgrade_legacy();
    let mut body = question();
    body.identity = record.identity.clone();
    body.result_version = record.result_version;
    let prepared = PreparedQuestion::prepare(
        &body,
        &record.identity,
        record.result_version,
        record.binding.as_ref().unwrap(),
        &[],
    )
    .unwrap();
    record.questions.push(prepared.intent.clone());
    let answer = QuestionAnswer {
        schema: 1,
        request: "answer-evidence".into(),
        question: prepared.intent.reference,
        control_epoch: 1,
        basis: AnswerBasis::ExistingAuthority,
        scope: "Use the original read-only scope".into(),
        answer: "Use the implementation".into(),
    };
    record.answers.push(
        PreparedAnswer::prepare(&answer, &answer.question, 1, 1, None)
            .unwrap()
            .intent,
    );
    let input = match outcome {
        DispatchOutcome::Applied => Some(InputDelivery {
            written_bytes: 100,
            total_bytes: 100,
            submit_written: true,
        }),
        DispatchOutcome::Partial => Some(InputDelivery {
            written_bytes: 3,
            total_bytes: 100,
            submit_written: false,
        }),
        DispatchOutcome::Rejected => Some(InputDelivery {
            written_bytes: 0,
            total_bytes: 100,
            submit_written: false,
        }),
        _ => None,
    };
    record.receipts.push(DispatchReceipt {
        request: answer.request,
        operation: DispatchOperation::AnswerQuestion,
        result_version: record.result_version,
        identity: record.identity.clone(),
        outcome,
        input,
        reason: None,
    });
    (temp, record)
}

#[test]
fn all_answer_outcomes_retain_one_execution_and_never_create_result_versions() {
    for outcome in [
        DispatchOutcome::Accepted,
        DispatchOutcome::Applied,
        DispatchOutcome::Rejected,
        DispatchOutcome::Partial,
        DispatchOutcome::Unknown,
    ] {
        let (_temp, record) = answer_record(outcome);
        assert_eq!(record.spent_execution_attempts(), Some(1), "{outcome:?}");
        assert_eq!(record.result_version, 1);
        assert!(record.previous_versions.is_empty());
        assert_eq!(
            record.pending_answer(),
            outcome == DispatchOutcome::Accepted
        );
        assert_eq!(
            record.unresolved_input(),
            matches!(outcome, DispatchOutcome::Partial | DispatchOutcome::Unknown)
        );
    }
}

#[test]
fn malformed_answer_evidence_cannot_reconstruct_execution_or_authority() {
    for field in 0..10 {
        let (_temp, mut record) = answer_record(DispatchOutcome::Applied);
        match field {
            0 => record.answers[0].expected_parent_epoch = 0,
            1 => record.answers[0].control_epoch = 0,
            2 => record.answers[0].question.digest = "b".repeat(64),
            3 => record.questions[0].binding.generation += 1,
            4 => record.receipts.last_mut().unwrap().result_version += 1,
            5 => record.receipts.last_mut().unwrap().identity.contract += 1,
            6 => record.receipts.last_mut().unwrap().input = None,
            7 => {
                record
                    .receipts
                    .last_mut()
                    .unwrap()
                    .input
                    .as_mut()
                    .unwrap()
                    .submit_written = false
            }
            8 => {
                record
                    .receipts
                    .last_mut()
                    .unwrap()
                    .input
                    .as_mut()
                    .unwrap()
                    .written_bytes = 101
            }
            _ => record.questions.push(record.questions[0].clone()),
        }
        assert_eq!(
            record.spent_execution_attempts(),
            None,
            "malformed field {field}"
        );
    }
}

#[test]
fn fresh_answer_evidence_cannot_bypass_prior_applied_partial_unknown_or_pending_answer() {
    for outcome in [
        DispatchOutcome::Applied,
        DispatchOutcome::Partial,
        DispatchOutcome::Unknown,
        DispatchOutcome::Accepted,
        DispatchOutcome::Rejected,
    ] {
        let (_temp, mut record) = answer_record(outcome);
        let mut another = record.answers[0].clone();
        another.request = "another-answer".into();
        record.answers.push(another.clone());
        let mut receipt = record.receipts.last().unwrap().clone();
        receipt.request = another.request;
        receipt.outcome = DispatchOutcome::Accepted;
        receipt.input = None;
        record.receipts.push(receipt);
        assert_eq!(
            record.spent_execution_attempts(),
            (outcome == DispatchOutcome::Rejected).then_some(1),
            "{outcome:?}"
        );
    }
}

#[test]
fn held_directory_prevents_path_replacement_from_redirecting_publication() {
    let parent = private_root();
    let original = parent.path().join("questions");
    let retired = parent.path().join("retired");
    let directory = Directory::open(&original, true).unwrap();
    let outside = private_root();
    std::fs::rename(&original, &retired).unwrap();
    symlink(outside.path(), &original).unwrap();
    let prepared = prepare(&question(), &[]);
    let saved = vec![prepared.intent.clone()];
    prepared.publish(&directory).unwrap();
    let name = question_name(&prepared.intent.reference).unwrap();
    assert!(retired.join(&name).is_file());
    assert!(!outside.path().join(&name).exists());
    assert!(Directory::open(&original, false).is_err());
    assert_eq!(
        read_question(&directory, &prepared.intent.reference, &saved).unwrap(),
        question()
    );
}

#[test]
fn serialized_pending_answer_preserves_final_outcome_margin_through_common_record_guard() {
    for outcome in [DispatchOutcome::Accepted, DispatchOutcome::Unknown] {
        let (_temp, mut record) = answer_record(outcome);
        record.reason = Some(String::new());
        let padding = 262_144 - 512 - serde_json::to_vec(&record).unwrap().len();
        record.reason = Some("x".repeat(padding));
        let original = serde_json::to_vec(&record).unwrap();
        assert_eq!(original.len(), 261_632);
        let mut decoded: Record = serde_json::from_slice(&original).unwrap();
        decoded.validate_archive_storage().unwrap();
        decoded.reason.as_mut().unwrap().push('x');
        assert!(serde_json::to_vec(&decoded).unwrap().len() < 262_144);
        assert!(
            decoded.validate_archive_storage().is_err(),
            "plain row room cannot spend final answer delivery capacity"
        );
        let mut completed: Record = serde_json::from_slice(&original).unwrap();
        completed.answer_outcome(
            "answer-evidence",
            DispatchOutcome::Unknown,
            Some(InputDelivery {
                written_bytes: u64::MAX - 1,
                total_bytes: u64::MAX,
                submit_written: false,
            }),
            Some("answer delivery completion is uncertain; do not replay"),
        );
        completed.validate_archive_storage().unwrap();
        assert!(serde_json::to_vec(&completed).unwrap().len() <= 262_144);
        assert_eq!(record.answers, completed.answers);
    }
}

use ovrcr_protocol::*;

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
        session: Some(ovrcr_protocol::SessionId(2)),
        run: Some(ovrcr_protocol::SessionRunId(3)),
    }
}

fn reference() -> QuestionReference {
    QuestionReference {
        assignment: identity(),
        result_version: 3,
        question_id: "which-source".into(),
        question_version: 1,
        digest: "a".repeat(64),
    }
}

#[test]
fn exact_question_reference_roundtrips_through_the_common_publication_identity() {
    let reference = reference();
    let publication = reference.publication();
    assert_eq!(publication.version, reference.question_version);
    assert_eq!(
        QuestionReference::from_publication(&publication).unwrap(),
        reference
    );
    let mut foreign = publication;
    foreign.kind = PublicationKind::Submission;
    assert!(QuestionReference::from_publication(&foreign).is_err());
}

fn question() -> AssignmentQuestion {
    AssignmentQuestion {
        schema: DELEGATION_SCHEMA,
        identity: identity(),
        result_version: 3,
        question_id: "which-source".into(),
        question_version: 1,
        supersedes: None,
        kind: QuestionKind::Clarification,
        question: "Which source is authoritative?".into(),
        context: "The implementation and a stale document disagree.".into(),
        choices: vec!["Implementation".into(), "Document".into()],
    }
}

#[test]
fn questions_require_exact_assignment_and_result_even_without_contract_change() {
    let q = question();
    q.validate(&identity(), 3, None).unwrap();
    assert!(q.validate(&identity(), 4, None).is_err());
    for field in 0..5 {
        let mut stale = identity();
        match field {
            0 => stale.contract += 1,
            1 => stale.controller_epoch += 1,
            2 => stale.attempt += 1,
            3 => stale.session = Some(ovrcr_protocol::SessionId(7)),
            _ => stale.run = Some(ovrcr_protocol::SessionRunId(8)),
        }
        assert!(q.validate(&stale, 3, None).is_err());
    }
}

#[test]
fn supersession_requires_the_authenticated_exact_predecessor() {
    let previous = reference();
    let mut q = question();
    q.question_version = 2;
    q.supersedes = Some(previous.clone());
    q.validate(&identity(), 3, Some(&previous)).unwrap();
    assert!(q.validate(&identity(), 3, None).is_err());
    for field in 0..6 {
        let mut claimed = previous.clone();
        match field {
            0 => claimed.assignment.attempt += 1,
            1 => claimed.result_version += 1,
            2 => claimed.question_id = "another".into(),
            3 => claimed.question_version += 1,
            4 => claimed.digest = "b".repeat(64),
            _ => claimed.digest = "../../escape".into(),
        }
        q.supersedes = Some(claimed);
        assert!(q.validate(&identity(), 3, Some(&previous)).is_err());
    }
    q.supersedes = Some(previous.clone());
    q.question_version = 3;
    assert!(q.validate(&identity(), 3, Some(&previous)).is_err());
}

#[test]
fn foreign_lineage_cannot_validate_by_claiming_its_own_predecessor() {
    let mut previous = reference();
    previous.assignment.contract += 1;
    let mut q = question();
    q.question_version = 2;
    q.supersedes = Some(previous.clone());
    assert!(q.validate(&identity(), 3, Some(&previous)).is_err());
    previous = reference();
    previous.result_version += 1;
    q.supersedes = Some(previous.clone());
    assert!(q.validate(&identity(), 3, Some(&previous)).is_err());
    previous = reference();
    previous.question_id = "different-question".into();
    q.supersedes = Some(previous.clone());
    assert!(q.validate(&identity(), 3, Some(&previous)).is_err());
}

#[test]
fn text_and_choices_are_bounded_without_rewriting_exact_question_bytes() {
    let mut q = question();
    q.question = "  Preserve whitespace, Unicode λ, and newlines.\n".into();
    q.validate(&identity(), 3, None).unwrap();
    let roundtrip: AssignmentQuestion =
        serde_json::from_slice(&serde_json::to_vec(&q).unwrap()).unwrap();
    assert_eq!(q, roundtrip);
    q.question = " ".into();
    assert!(q.validate(&identity(), 3, None).is_err());
    q = question();
    q.context = "x".repeat(32_769);
    assert!(q.validate(&identity(), 3, None).is_err());
    q = question();
    q.choices = vec!["choice".into(); 33];
    assert!(q.validate(&identity(), 3, None).is_err());
    q = question();
    q.choices = vec!["".into()];
    assert!(q.validate(&identity(), 3, None).is_err());
    q = question();
    q.question_id = "../question".into();
    assert!(q.validate(&identity(), 3, None).is_err());
    assert_eq!(MAX_ASSIGNMENT_QUESTIONS, 64);
}

#[test]
fn answers_are_scoped_to_exact_question_and_captured_epoch() {
    let current = reference();
    let mut a = QuestionAnswer {
        schema: DELEGATION_SCHEMA,
        request: "answer-once".into(),
        question: current.clone(),
        control_epoch: 5,
        basis: AnswerBasis::ExistingAuthority,
        scope: "Select the source for this clarification only.".into(),
        answer: "Use the implementation, as the requested task already requires.".into(),
    };
    a.validate(&current, 5).unwrap();
    assert!(a.validate(&current, 6).is_err());
    for field in 0..4 {
        let mut stale = current.clone();
        match field {
            0 => stale.result_version += 1,
            1 => stale.question_version += 1,
            2 => stale.digest = "b".repeat(64),
            _ => stale.assignment.run = None,
        }
        assert!(a.validate(&stale, 5).is_err());
    }
    a.scope.clear();
    assert!(a.validate(&current, 5).is_err());
    a.scope = "Bounded answer".into();
    a.answer = "x".repeat(32_769);
    assert!(a.validate(&current, 5).is_err());
}

#[test]
fn artifacts_reject_unknown_fields_and_native_gaps_carry_no_screen_payload() {
    let mut json = serde_json::to_value(question()).unwrap();
    json["approve_by_default"] = serde_json::json!(true);
    assert!(serde_json::from_value::<AssignmentQuestion>(json).is_err());
    for gap in [
        NativeQuestionGap::IncompletePromptObservation,
        NativeQuestionGap::NoPromptObservation,
        NativeQuestionGap::CredentialInput,
        NativeQuestionGap::IncompleteScreen,
        NativeQuestionGap::AttributionChanged,
        NativeQuestionGap::UnavailableBinding,
        NativeQuestionGap::UnsupportedAdapter,
    ] {
        let encoded = serde_json::to_string(&gap).unwrap();
        assert_eq!(
            serde_json::from_str::<NativeQuestionGap>(&encoded).unwrap(),
            gap
        );
        assert!(!encoded.contains('{'));
    }
}

#[test]
fn public_prompt_observation_and_unexplained_deadline_are_distinct_gaps() {
    assert_eq!(
        NativeQuestionGap::from_observation(true, true, true, false),
        Some(NativeQuestionGap::IncompletePromptObservation)
    );
    assert_eq!(
        NativeQuestionGap::from_observation(true, true, false, true),
        Some(NativeQuestionGap::NoPromptObservation)
    );
    assert_eq!(
        NativeQuestionGap::from_observation(true, true, false, false),
        None
    );
    assert_eq!(
        NativeQuestionGap::from_observation(false, true, true, true),
        Some(NativeQuestionGap::AttributionChanged)
    );
    assert_eq!(
        NativeQuestionGap::from_observation(true, false, false, true),
        Some(NativeQuestionGap::UnavailableBinding)
    );
    assert_eq!(
        NativeQuestionGap::from_observation(false, false, false, true),
        Some(NativeQuestionGap::UnavailableBinding)
    );
}

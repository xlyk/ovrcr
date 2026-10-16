//! Private question evidence, nested under the existing delegation owner when wired.
//! These helpers never authenticate a caller or authorize a PTY effect.
use super::*;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The existing assignment Record owns these authenticated pre-link intents.
/// Bodies remain private files; handling and answering are separate operations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct QuestionPublicationIntent {
    pub reference: QuestionReference,
    pub binding: AgentBinding,
    pub handled: bool,
}

pub(super) struct PreparedQuestion {
    pub intent: QuestionPublicationIntent,
    bytes: Vec<u8>,
}

impl PreparedQuestion {
    pub(super) fn prepare(
        question: &AssignmentQuestion,
        identity: &AssignmentIdentity,
        result_version: u32,
        binding: &AgentBinding,
        intents: &[QuestionPublicationIntent],
    ) -> Result<Self> {
        if &question.identity != identity || question.result_version != result_version {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "foreign or stale question assignment",
            ));
        }
        let same_lineage = |intent: &&QuestionPublicationIntent| {
            intent.reference.assignment == *identity
                && intent.reference.result_version == result_version
                && intent.reference.question_id == question.question_id
        };
        let previous = intents.iter().filter(same_lineage).find(|intent| {
            intent.reference.question_version.checked_add(1) == Some(question.question_version)
        });
        question
            .validate(
                identity,
                result_version,
                previous.map(|intent| &intent.reference),
            )
            .map_err(|reason| lifecycle_error(ErrorCode::InvalidRequest, reason))?;
        let latest = intents
            .iter()
            .filter(same_lineage)
            .max_by_key(|intent| intent.reference.question_version);
        if latest.is_some_and(|latest| &latest.binding != binding) {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "question lineage belongs to a different child binding",
            ));
        }
        let bytes = serde_json::to_vec(question)?;
        let reference = QuestionReference {
            assignment: identity.clone(),
            result_version,
            question_id: question.question_id.clone(),
            question_version: question.question_version,
            digest: digest(&bytes),
        };
        reference
            .validate()
            .map_err(|reason| lifecycle_error(ErrorCode::InvalidRequest, reason))?;
        if let Some(latest) = latest {
            if latest.reference.question_version > reference.question_version {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "question has been superseded",
                ));
            }
            if latest.reference.question_version == reference.question_version {
                if latest.reference != reference || &latest.binding != binding {
                    return Err(lifecycle_error(
                        ErrorCode::Conflict,
                        "question version has a different authenticated publication intent",
                    ));
                }
                return Ok(Self {
                    intent: latest.clone(),
                    bytes,
                });
            }
        }
        if intents.len() >= MAX_ASSIGNMENT_QUESTIONS {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "question storage exhausted; unresolved evidence is retained",
            ));
        }
        Ok(Self {
            intent: QuestionPublicationIntent {
                reference,
                binding: binding.clone(),
                handled: false,
            },
            bytes,
        })
    }

    /// Call only inside the child-binding fence AFTER saving `intent` to Record.
    /// Repeating a link is safe only for exactly the originally authenticated bytes.
    pub(super) fn publish(&self, directory: &Directory) -> Result<()> {
        publish_matching(
            directory,
            &question_name(&self.intent.reference)?,
            &self.bytes,
        )
    }
}

pub(super) fn read_question(
    directory: &Directory,
    reference: &QuestionReference,
    intents: &[QuestionPublicationIntent],
) -> Result<AssignmentQuestion, PublicationIssue> {
    reference
        .validate()
        .map_err(|_| PublicationIssue::Invalid)?;
    // A parseable private file cannot create its own child-authorship claim.
    if !intents.iter().any(|intent| &intent.reference == reference) {
        return Err(PublicationIssue::Uncommitted);
    }
    let name = question_name(reference).map_err(|_| PublicationIssue::Invalid)?;
    let bytes = directory
        .read(&name)
        .map_err(|_| PublicationIssue::Unavailable)?;
    if digest(&bytes) != reference.digest {
        return Err(PublicationIssue::Invalid);
    }
    let question: AssignmentQuestion =
        serde_json::from_slice(&bytes).map_err(|_| PublicationIssue::Invalid)?;
    if question.question_id != reference.question_id
        || question.question_version != reference.question_version
    {
        return Err(PublicationIssue::Invalid);
    }
    let previous = intents.iter().find(|intent| {
        intent.reference.assignment == reference.assignment
            && intent.reference.result_version == reference.result_version
            && intent.reference.question_id == reference.question_id
            && intent.reference.question_version.checked_add(1) == Some(reference.question_version)
    });
    question
        .validate(
            &reference.assignment,
            reference.result_version,
            previous.map(|intent| &intent.reference),
        )
        .map_err(|_| PublicationIssue::Invalid)?;
    // Complete a link whose final directory sync was interrupted, without new authority.
    directory
        .sync()
        .map_err(|_| PublicationIssue::Unavailable)?;
    Ok(question)
}

pub(super) fn question_name(reference: &QuestionReference) -> Result<String> {
    reference
        .validate()
        .map_err(|reason| lifecycle_error(ErrorCode::InvalidRequest, reason))?;
    Ok(format!(
        "question-r{}-{}-v{}.json",
        reference.result_version, reference.question_id, reference.question_version
    ))
}

/// Immutable payload identity paired with the existing request-correlated receipt.
/// Dispatch outcome and unresolved-input barriers stay in the existing receipt owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AnswerIntent {
    pub request: String,
    pub question: QuestionReference,
    pub control_epoch: u64,
    pub expected_parent_epoch: u64,
    pub digest: String,
}

pub(super) struct PreparedAnswer {
    pub intent: AnswerIntent,
    bytes: Vec<u8>,
}

impl PreparedAnswer {
    pub(super) fn prepare(
        answer: &QuestionAnswer,
        current_question: &QuestionReference,
        control_epoch: u64,
        expected_parent_epoch: u64,
        previous: Option<&AnswerIntent>,
    ) -> Result<Self> {
        answer
            .validate(current_question, control_epoch)
            .map_err(|reason| lifecycle_error(ErrorCode::InvalidRequest, reason))?;
        if expected_parent_epoch == 0 {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "answer requires captured current parent authority",
            ));
        }
        let bytes = serde_json::to_vec(answer)?;
        let intent = AnswerIntent {
            request: answer.request.clone(),
            question: answer.question.clone(),
            control_epoch: answer.control_epoch,
            expected_parent_epoch,
            digest: digest(&bytes),
        };
        if previous.is_some_and(|previous| previous != &intent) {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "answer request identity or payload changed",
            ));
        }
        Ok(Self { intent, bytes })
    }

    /// The caller saves this intent with the existing Accepted receipt before linking.
    /// Publishing this artifact cannot authorize, retry, or settle PTY input.
    pub(super) fn publish(&self, directory: &Directory) -> Result<()> {
        publish_matching(
            directory,
            &format!("answer-{}.json", self.intent.request),
            &self.bytes,
        )
    }
}

const RECORD_BYTES: usize = 262_144;

fn check_storage(record: &Record) -> Result<()> {
    // Reserve room for the counted delivery and fixed outcome reason before bytes.
    if serde_json::to_vec(record)?.len() > RECORD_BYTES - 512 {
        return Err(lifecycle_error(
            ErrorCode::Conflict,
            "assignment metadata storage exhausted; original obligations are retained",
        ));
    }
    Ok(())
}

pub(super) const QUESTION_INSTRUCTIONS: &str = "Before waiting on a model-originated question, call ovrcr_publish_question with schema 1, this brief's exact identity and result_version, a bounded question_id, question_version 1, supersedes null, kind Clarification or HumanDecision, exact question, context and choices. Only one unanswered current question is supported; same-ID revisions increment question_version and name the exact predecessor reference. Publication does not approve anything. Wait for the scoped answer; never infer approval from silence, a default or a timeout. Native approval/login prompts are separate and may remain unresolved; never publish credential values.";

impl Record {
    fn current_question(&self) -> Option<&QuestionPublicationIntent> {
        self.questions.iter().rev().find(|q| {
            q.reference.assignment == self.identity
                && q.reference.result_version == self.result_version
        })
    }

    fn applied_answer(&self, question: &QuestionReference) -> bool {
        self.answers.iter().any(|a| {
            &a.question == question
                && self
                    .request_receipt(&a.request, DispatchOperation::AnswerQuestion)
                    .is_some_and(|r| r.outcome == DispatchOutcome::Applied)
        })
    }

    pub(super) fn pending_answer(&self) -> bool {
        self.answers.iter().any(|a| {
            self.request_receipt(&a.request, DispatchOperation::AnswerQuestion)
                .is_none_or(|r| r.outcome == DispatchOutcome::Accepted)
        })
    }

    pub(super) fn valid_question_evidence(&self, versions: &[VersionState]) -> bool {
        if self.questions.len() > MAX_ASSIGNMENT_QUESTIONS
            || self.answers.len() > MAX_ASSIGNMENT_QUESTIONS
        {
            return false;
        }
        for (index, q) in self.questions.iter().enumerate() {
            let r = &q.reference;
            if r.validate().is_err()
                || Some(&q.binding) != self.binding.as_ref()
                || !versions
                    .iter()
                    .any(|v| v.identity == r.assignment && v.result_version == r.result_version)
                || self.questions[..index].iter().any(|old| {
                    old.reference.assignment == r.assignment
                        && old.reference.result_version == r.result_version
                        && old.reference.question_id == r.question_id
                        && old.reference.question_version == r.question_version
                })
                || (r.question_version > 1
                    && !self.questions[..index].iter().any(|old| {
                        old.reference.assignment == r.assignment
                            && old.reference.result_version == r.result_version
                            && old.reference.question_id == r.question_id
                            && old.reference.question_version.checked_add(1)
                                == Some(r.question_version)
                            && old.binding == q.binding
                    }))
            {
                return false;
            }
        }
        for (index, a) in self.answers.iter().enumerate() {
            if !followups::valid_request(&a.request)
                || a.control_epoch == 0
                || a.expected_parent_epoch == 0
                || a.digest.len() != 64
                || !a
                    .digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || !self.questions.iter().any(|q| q.reference == a.question)
                || self
                    .request_receipt(&a.request, DispatchOperation::AnswerQuestion)
                    .is_none()
                || self.answers[..index]
                    .iter()
                    .any(|old| old.request == a.request)
            {
                return false;
            }
            if self.answers[..index].iter().any(|old| {
                self.request_receipt(&old.request, DispatchOperation::AnswerQuestion)
                    .is_some_and(|r| {
                        (old.question == a.question && r.outcome != DispatchOutcome::Rejected)
                            || matches!(
                                r.outcome,
                                DispatchOutcome::Accepted
                                    | DispatchOutcome::Unknown
                                    | DispatchOutcome::Partial
                            )
                    })
            }) {
                return false;
            }
            if let Some(receipt) =
                self.request_receipt(&a.request, DispatchOperation::AnswerQuestion)
            {
                if let Some(input) = &receipt.input {
                    if input.written_bytes > input.total_bytes
                        || input.total_bytes == 0
                        || (input.submit_written && input.written_bytes != input.total_bytes)
                        || (receipt.outcome == DispatchOutcome::Applied
                            && (!input.submit_written || input.written_bytes != input.total_bytes))
                        || (receipt.outcome == DispatchOutcome::Rejected
                            && input.written_bytes != 0)
                        || (receipt.outcome == DispatchOutcome::Partial
                            && (input.written_bytes == 0
                                || input.written_bytes >= input.total_bytes))
                    {
                        return false;
                    }
                } else if matches!(
                    receipt.outcome,
                    DispatchOutcome::Applied | DispatchOutcome::Partial
                ) {
                    return false;
                }
            }
        }
        true
    }

    fn answer_outcome(
        &mut self,
        request: &str,
        outcome: DispatchOutcome,
        input: Option<InputDelivery>,
        reason: Option<&str>,
    ) {
        let receipt = self
            .receipts
            .iter_mut()
            .find(|r| r.request == request && r.operation == DispatchOperation::AnswerQuestion)
            .expect("durable answer receipt");
        receipt.outcome = outcome;
        receipt.input = input;
        receipt.reason = reason.map(str::to_owned);
    }

    fn require_question(&self, reference: &QuestionReference) -> Result<()> {
        if reference.assignment != self.identity || reference.result_version != self.result_version
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "question belongs to a stale assignment or result",
            ));
        }
        if self.applied_answer(reference) {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "question has already received an applied answer",
            ));
        }
        if self
            .current_question()
            .is_none_or(|q| &q.reference != reference || Some(&q.binding) != self.binding.as_ref())
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "question is unknown, superseded or has a foreign binding",
            ));
        }
        if self.control.attribution_invalidated {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "human changes require a fresh result before an automated answer",
            ));
        }
        Ok(())
    }
}

impl ServerState {
    /// This role can publish only for its allocated child. It grants no parent operations.
    pub(super) fn assignment_publisher(
        &self,
        auth: &DelegationAuth,
        record: &Record,
    ) -> Result<std::sync::Arc<crate::session::Session>> {
        let session = self.session_for_control(auth.session)?;
        let contract = read_contract(record)?;
        if Some(auth.session) != record.identity.session
            || Some(session.run()) != record.identity.run
            || !session.authorizes_delegation(&auth.capability)
            || !matches!(session.summary().kind, SessionKind::Agent { ref name } if name == &contract.provider)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "publication requires this assignment's current child capability",
            ));
        }
        self.assignment_session(record)?;
        Ok(session)
    }

    pub(in crate::server) fn question_states(
        &self,
        record: &Record,
    ) -> Vec<QuestionPublicationState> {
        let current = record.current_question().map(|q| &q.reference);
        let attributed =
            self.assignment_session(record).is_ok() && !record.control.attribution_invalidated;
        let directory = Directory::open(&record.root, false);
        record
            .questions
            .iter()
            .map(|q| {
                let selected = current == Some(&q.reference);
                let issue = if selected && Some(&q.binding) != record.binding.as_ref() {
                    Some(PublicationIssue::Invalid)
                } else if selected {
                    match &directory {
                        Ok(d) => read_question(d, &q.reference, &record.questions).err(),
                        Err(_) => Some(PublicationIssue::Unavailable),
                    }
                } else {
                    None
                };
                let answer = record
                    .answers
                    .iter()
                    .rev()
                    .find(|a| a.question == q.reference)
                    .and_then(|a| {
                        record.request_receipt(&a.request, DispatchOperation::AnswerQuestion)
                    })
                    .cloned();
                QuestionPublicationState {
                    reference: q.reference.clone(),
                    handled: q.handled,
                    current: selected && attributed,
                    issue,
                    answer,
                }
            })
            .collect()
    }

    pub(super) fn publish_question(
        &self,
        auth: DelegationAuth,
        handle: String,
        question: AssignmentQuestion,
    ) -> Result<AssignmentSnapshot> {
        let store = self.retained.lock();
        let mut record = store.assignment(&handle)?.context("assignment not found")?;
        let child = self.assignment_publisher(&auth, &record)?;
        let binding = record.binding.clone().context("child is not bound")?;
        if !matches!(
            record.dispatch,
            DispatchStatus::Applied | DispatchStatus::InputUnknown
        ) {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "initial assignment has not been delivered",
            ));
        }
        child.with_delegation_binding(&auth.capability, &binding, || {
            let prepared = PreparedQuestion::prepare(&question, &record.identity, record.result_version, &binding, &record.questions)?;
            let existing = record.questions.iter().any(|q| q.reference == prepared.intent.reference);
            if let Some(current) = record.current_question() {
                if existing && current.reference != prepared.intent.reference { return Err(lifecycle_error(ErrorCode::Conflict, "historical question cannot become current again")); }
                if !existing && current.reference.question_id != question.question_id && !record.applied_answer(&current.reference) { return Err(lifecycle_error(ErrorCode::Conflict, "one current unanswered question is supported; answer or explicitly supersede it first")); }
            }
            if !existing { record.questions.push(prepared.intent.clone()); }
            check_storage(&record)?;
            store.save_assignment(&record)?;
            if take_fault("question-before-link", &handle).is_some() { return Ok(()); }
            prepared.publish(&Directory::open(&record.root, false)?)
        })?;
        drop(store);
        if take_fault("question-after-link", &handle).is_none() {
            self.delegation_observations.invalidate(auth.session);
        }
        self.reconciliation_snapshot(record)
    }

    pub(super) fn collect_question(
        &self,
        auth: DelegationAuth,
        handle: String,
        reference: QuestionReference,
    ) -> Result<AssignmentSnapshot> {
        let record = self.owned_assignment(&auth, &handle)?;
        let question = read_question(
            &Directory::open(&record.root, false)?,
            &reference,
            &record.questions,
        )
        .map_err(|_| anyhow::anyhow!("question bytes are unavailable, uncommitted or invalid"))?;
        let mut snapshot = self.reconciliation_snapshot(record)?;
        snapshot.collected_question = Some(question);
        Ok(snapshot)
    }

    pub(super) fn handle_question(
        &self,
        auth: DelegationAuth,
        handle: String,
        publication: PublicationIdentity,
    ) -> Result<AssignmentSnapshot> {
        let reference = QuestionReference::from_publication(&publication)
            .map_err(|reason| lifecycle_error(ErrorCode::InvalidRequest, reason))?;
        self.owned_assignment(&auth, &handle)?;
        let store = self.retained.lock();
        let mut record = store
            .assignment(&handle)?
            .context("assignment disappeared")?;
        self.require_workflow(&store, &auth, &record)?;
        read_question(
            &Directory::open(&record.root, false)?,
            &reference,
            &record.questions,
        )
        .map_err(|_| anyhow::anyhow!("handled question does not match authenticated bytes"))?;
        record
            .questions
            .iter_mut()
            .find(|q| q.reference == reference)
            .context("question not found")?
            .handled = true;
        store.save_assignment(&record)?;
        drop(store);
        if let Some(id) = record.identity.session {
            self.delegation_observations.invalidate(id);
        }
        self.reconciliation_snapshot(record)
    }

    pub(super) fn answer_question(
        &self,
        auth: DelegationAuth,
        handle: String,
        answer: QuestionAnswer,
        expected_parent_epoch: u64,
    ) -> Result<AssignmentSnapshot> {
        let mut record = self.owned_assignment(&auth, &handle)?;
        let saved = record.answers.iter().find(|a| a.request == answer.request);
        let prepared = PreparedAnswer::prepare(
            &answer,
            &answer.question,
            answer.control_epoch,
            expected_parent_epoch,
            saved,
        )?;
        if saved.is_some() {
            let receipt = record
                .request_receipt(&answer.request, DispatchOperation::AnswerQuestion)
                .context("answer receipt is missing")?;
            if receipt.outcome != DispatchOutcome::Accepted {
                return self.reconciliation_snapshot(record);
            }
        } else {
            if record.receipts.iter().any(|r| r.request == answer.request) {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "request already identifies another operation",
                ));
            }
            if record.answers.len() >= MAX_ASSIGNMENT_QUESTIONS {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "answer storage exhausted; original receipts are retained",
                ));
            }
            if record.unresolved_input()
                || record.pending_answer()
                || record.followups.iter().any(|f| {
                    record
                        .request_receipt(&f.request, DispatchOperation::Followup)
                        .is_none_or(|r| r.outcome == DispatchOutcome::Accepted)
                })
            {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "original input remains unresolved; no fresh answer is authorized",
                ));
            }
            if !record
                .receipt(DispatchOperation::InitialInput)
                .is_some_and(|r| r.outcome == DispatchOutcome::Applied)
            {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "initial input is not confirmed delivered",
                ));
            }
        }
        self.require_dispatch_open(&self.retained.lock(), &record.identity.workflow)?;
        let authority = self.current_workflow_authority(&auth, &record.identity.workflow)?;
        if authority.epoch != expected_parent_epoch {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "answer belongs to a retired workflow controller",
            ));
        }
        control::require_controller(&record.control, answer.control_epoch)?;
        record.require_question(&answer.question)?;
        let parent = self.session_for_control(auth.session)?;
        let child = self.assignment_session(&record)?;
        let model = read_contract(&record)?.model;
        let child_binding = record.binding.clone().context("child is not bound")?;
        let store = self.retained.lock();
        record = store
            .assignment(&handle)?
            .context("assignment disappeared")?;
        let authority = self.require_workflow(&store, &auth, &record)?;
        self.require_dispatch_open(&store, &record.identity.workflow)?;
        if authority.epoch != expected_parent_epoch {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "captured answer parent authority changed",
            ));
        }
        control::require_controller(&record.control, answer.control_epoch)?;
        record.require_question(&answer.question)?;
        let directory = Directory::open(&record.root, false)?;
        read_question(&directory, &answer.question, &record.questions).map_err(|_| {
            anyhow::anyhow!("current question bytes do not match authenticated intent")
        })?;
        let captured_control = record.control.clone();
        let input = crate::session::DelegatedInput {
            request: &answer.request,
            control: &captured_control,
            control_epoch: answer.control_epoch,
            authority: crate::session::DelegatedAuthority::QuestionAnswer,
            parent: &parent,
            parent_producer: Some(&authority.producer),
            parent_source: auth.source.as_ref(),
            parent_binding: &authority.binding,
            capability: &auth.capability,
            child_binding: &child_binding,
            model: &model,
            text: "",
        };
        child
            .with_delegated_context(&input, |snapshot| {
                if !snapshot.input_requests.is_empty() {
                    return Err(lifecycle_error(
                        ErrorCode::Conflict,
                        "native input is unresolved; structured answer input is refused",
                    ));
                }
                if !record.answers.iter().any(|a| a.request == answer.request) {
                    record.answers.push(prepared.intent.clone());
                    record.receipts.push(DispatchReceipt {
                        request: answer.request.clone(),
                        operation: DispatchOperation::AnswerQuestion,
                        result_version: answer.question.result_version,
                        identity: answer.question.assignment.clone(),
                        outcome: DispatchOutcome::Accepted,
                        input: None,
                        reason: None,
                    });
                }
                check_storage(&record)?;
                store.save_assignment(&record)?;
                prepared.publish(&directory)
            })?
            .ok_or_else(|| {
                lifecycle_error(
                    ErrorCode::Conflict,
                    "answer parent/child/native-input context changed; no bytes written",
                )
            })?;
        drop(store);
        if take_fault("answer-before-input", &answer.request).is_some() {
            return self.reconciliation_snapshot(record);
        }
        hold_before_effect("answer", &answer.request)?;
        let store = self.retained.lock();
        record = store
            .assignment(&handle)?
            .context("assignment disappeared")?;
        let authority = self.require_workflow(&store, &auth, &record)?;
        self.require_dispatch_open(&store, &record.identity.workflow)?;
        if authority.epoch != expected_parent_epoch {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "answer controller changed before input; original intent is suspended",
            ));
        }
        if record
            .answers
            .iter()
            .find(|intent| intent.request == answer.request)
            != Some(&prepared.intent)
            || !record
                .request_receipt(&answer.request, DispatchOperation::AnswerQuestion)
                .is_some_and(|receipt| receipt.outcome == DispatchOutcome::Accepted)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "original answer intent or receipt changed before input",
            ));
        }
        control::require_controller(&record.control, answer.control_epoch)?;
        record.require_question(&answer.question)?;
        read_question(
            &Directory::open(&record.root, false)?,
            &answer.question,
            &record.questions,
        )
        .map_err(|_| anyhow::anyhow!("question changed before answer input"))?;
        let bytes = Directory::open(&record.root, false)?
            .read(&format!("answer-{}.json", answer.request))?;
        if digest(&bytes) != prepared.intent.digest
            || serde_json::from_slice::<QuestionAnswer>(&bytes)? != answer
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "immutable answer bytes changed before input",
            ));
        }
        let prompt = format!(
            "Read the scoped answer in {} for question {} version {}. It applies only to that exact published question; continue the same assignment and result version. This is not permission to widen scope or answer a native approval/login prompt.",
            record
                .root
                .join(format!("answer-{}.json", answer.request))
                .display(),
            answer.question.question_id,
            answer.question.question_version
        );
        record.answer_outcome(
            &answer.request,
            DispatchOutcome::Unknown,
            None,
            Some("answer input effect is unknown; do not replay"),
        );
        check_storage(&record)?;
        store.save_assignment(&record)?;
        match child.send_delegated_text(crate::session::DelegatedInput {
            request: &answer.request,
            control: &record.control,
            control_epoch: answer.control_epoch,
            authority: crate::session::DelegatedAuthority::QuestionAnswer,
            parent: &parent,
            parent_producer: Some(&authority.producer),
            parent_source: auth.source.as_ref(),
            parent_binding: &authority.binding,
            capability: &auth.capability,
            child_binding: &child_binding,
            model: &model,
            text: &prompt,
        }) {
            Ok(crate::session::DelegatedDelivery::Applied(input)) => {
                if take_fault("answer-after-effect", &answer.request).is_some() {
                    drop(store);
                    self.delegation_observations.invalidate(child.id());
                    return self.reconciliation_snapshot(record);
                }
                record.answer_outcome(&answer.request, DispatchOutcome::Applied, Some(input), None);
            }
            Ok(crate::session::DelegatedDelivery::Rejected(input)) => record.answer_outcome(
                &answer.request,
                DispatchOutcome::Rejected,
                Some(input),
                Some("answer rejected without writing bytes"),
            ),
            Ok(crate::session::DelegatedDelivery::Partial(input)) => record.answer_outcome(
                &answer.request,
                DispatchOutcome::Partial,
                Some(input),
                Some("answer prefix reached PTY; do not replay"),
            ),
            Ok(crate::session::DelegatedDelivery::Unknown(input)) => record.answer_outcome(
                &answer.request,
                DispatchOutcome::Unknown,
                Some(input),
                Some("answer delivery completion is uncertain; do not replay"),
            ),
            Err(_) => (),
        }
        store.save_assignment(&record)?;
        drop(store);
        self.delegation_observations.invalidate(child.id());
        self.reconciliation_snapshot(record)
    }
}

#[cfg(test)]
#[path = "delegation_question_tests.rs"]
mod tests;

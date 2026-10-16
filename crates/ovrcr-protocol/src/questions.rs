//! Private question artifacts. Validation never grants input or human authority.
use crate::{
    AssignmentIdentity, DELEGATION_SCHEMA, DispatchReceipt, PublicationIdentity, PublicationIssue,
    PublicationKind,
};
use serde::{Deserialize, Serialize};

/// Storage limits refuse new work rather than evict unanswered evidence.
pub const MAX_ASSIGNMENT_QUESTIONS: usize = 64;
const MAX_QUESTION_TEXT: usize = 32_768;
const MAX_CHOICES: usize = 32;
const MAX_CHOICE_TEXT: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionReference {
    pub assignment: AssignmentIdentity,
    pub result_version: u32,
    pub question_id: String,
    pub question_version: u32,
    pub digest: String,
}

impl QuestionReference {
    pub fn publication(&self) -> PublicationIdentity {
        PublicationIdentity {
            assignment: self.assignment.clone(),
            kind: PublicationKind::Question {
                result_version: self.result_version,
                question_id: self.question_id.clone(),
            },
            version: self.question_version,
            digest: self.digest.clone(),
        }
    }

    pub fn from_publication(publication: &PublicationIdentity) -> Result<Self, &'static str> {
        let PublicationKind::Question {
            result_version,
            question_id,
        } = &publication.kind
        else {
            return Err("publication is not a question");
        };
        let reference = Self {
            assignment: publication.assignment.clone(),
            result_version: *result_version,
            question_id: question_id.clone(),
            question_version: publication.version,
            digest: publication.digest.clone(),
        };
        reference.validate()?;
        Ok(reference)
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.result_version == 0 || self.question_version == 0 {
            return Err("question versions must be positive");
        }
        if !valid_id(&self.question_id) {
            return Err("invalid question identity");
        }
        if self.digest.len() != 64
            || !self
                .digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("invalid question digest");
        }
        Ok(())
    }
}

/// Bounded metadata only. The body is fetched separately by its exact reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionPublicationState {
    pub reference: QuestionReference,
    pub handled: bool,
    pub current: bool,
    pub issue: Option<PublicationIssue>,
    pub answer: Option<DispatchReceipt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestionKind {
    Clarification,
    HumanDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssignmentQuestion {
    pub schema: u32,
    pub identity: AssignmentIdentity,
    pub result_version: u32,
    pub question_id: String,
    pub question_version: u32,
    pub supersedes: Option<QuestionReference>,
    pub kind: QuestionKind,
    pub question: String,
    pub context: String,
    pub choices: Vec<String>,
}

impl AssignmentQuestion {
    /// `previous` is the authenticated stored predecessor, never a file's claim.
    pub fn validate(
        &self,
        identity: &AssignmentIdentity,
        result_version: u32,
        previous: Option<&QuestionReference>,
    ) -> Result<(), &'static str> {
        if self.schema != DELEGATION_SCHEMA
            || result_version == 0
            || self.result_version != result_version
            || &self.identity != identity
        {
            return Err("foreign or stale question assignment");
        }
        if !valid_id(&self.question_id) || self.question_version == 0 {
            return Err("invalid question identity or version");
        }
        match (previous, self.supersedes.as_ref()) {
            (None, None) if self.question_version == 1 => {}
            (Some(previous), Some(supersedes))
                if previous == supersedes
                    && previous.validate().is_ok()
                    && &previous.assignment == identity
                    && previous.result_version == result_version
                    && previous.question_id == self.question_id
                    && previous.question_version.checked_add(1) == Some(self.question_version) => {}
            _ => return Err("question predecessor does not match current lineage"),
        }
        if self.question.trim().is_empty()
            || self.question.len() > MAX_QUESTION_TEXT
            || self.context.len() > MAX_QUESTION_TEXT
            || self.choices.len() > MAX_CHOICES
            || self
                .choices
                .iter()
                .any(|choice| choice.trim().is_empty() || choice.len() > MAX_CHOICE_TEXT)
        {
            return Err("question exceeds content limits");
        }
        Ok(())
    }
}

/// Declared provenance for the parent conversation; this is not an approval token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnswerBasis {
    ExistingAuthority,
    ExplicitHumanDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionAnswer {
    pub schema: u32,
    pub request: String,
    pub question: QuestionReference,
    pub control_epoch: u64,
    pub basis: AnswerBasis,
    pub scope: String,
    pub answer: String,
}

impl QuestionAnswer {
    /// Admission must separately authenticate the parent and current control owner.
    pub fn validate(
        &self,
        question: &QuestionReference,
        control_epoch: u64,
    ) -> Result<(), &'static str> {
        self.question.validate()?;
        if self.schema != DELEGATION_SCHEMA
            || &self.question != question
            || self.control_epoch == 0
            || self.control_epoch != control_epoch
            || !valid_id(&self.request)
        {
            return Err("foreign or stale question answer");
        }
        if self.scope.trim().is_empty()
            || self.scope.len() > MAX_CHOICE_TEXT
            || self.answer.trim().is_empty()
            || self.answer.len() > MAX_QUESTION_TEXT
        {
            return Err("answer requires bounded scope and content");
        }
        Ok(())
    }
}

/// Content-free outcome when a provider cannot supply the exact native question.
/// None of these outcomes authorizes answering a guessed prompt or credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeQuestionGap {
    IncompletePromptObservation,
    /// No exact question is available; a deadline does not identify its cause.
    NoPromptObservation,
    CredentialInput,
    IncompleteScreen,
    AttributionChanged,
    UnavailableBinding,
    UnsupportedAdapter,
}

impl NativeQuestionGap {
    /// The caller supplies attribution from the authoritative current observation.
    /// Public Pi prompt events omit bodies/options; no screen text enters this path.
    pub fn from_observation(
        current: bool,
        binding_available: bool,
        has_reported_input: bool,
        stalled: bool,
    ) -> Option<Self> {
        if !binding_available {
            Some(Self::UnavailableBinding)
        } else if !current {
            Some(Self::AttributionChanged)
        } else if has_reported_input {
            Some(Self::IncompletePromptObservation)
        } else if stalled {
            Some(Self::NoPromptObservation)
        } else {
            None
        }
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

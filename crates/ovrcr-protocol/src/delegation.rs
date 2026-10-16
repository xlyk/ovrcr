//! The first delegation slice: one read-only Pi assignment and explicit collection.
use crate::{
    ActivitySample, AgentActivity, AgentBinding, InputRequest, ReporterHealth, SessionId,
    SessionPhase, SessionRunId,
};
use crate::{AssignmentControl, AssignmentControlEvidence};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DELEGATION_SCHEMA: u32 = 1;

/// One exact advertised child selection; configuration alone is not availability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationChoice {
    pub provider: crate::DelegationHarness,
    pub model_provider: String,
    pub model: String,
}

/// Mandatory per-assignment bounds. Unsupported bounds are refused, never
/// represented as enforced estimates. Zero-effect refusal consumes no execution.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DelegationLimits {
    pub max_live_children: Option<u16>,
    pub max_execution_attempts: Option<u32>,
    /// Admission leaves this many shared slots free; it does not reserve them.
    pub min_free_slots_after_launch: u16,
    pub max_cost_microusd: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub max_native_children: Option<u16>,
}

impl DelegationLimits {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self
            .max_live_children
            .is_some_and(|limit| !(1..=50).contains(&limit))
            || self.max_execution_attempts == Some(0)
            || self.min_free_slots_after_launch >= 50
        {
            return Err("invalid mandatory delegation limit");
        }
        if self.max_cost_microusd.is_some()
            || self.max_output_tokens.is_some()
            || self.max_native_children.is_some()
        {
            return Err("unsupported mandatory cost, token or native-descendant limit");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DelegationSelection {
    pub provider: Option<crate::DelegationHarness>,
    pub model_provider: Option<String>,
    pub model: Option<String>,
    pub required_limits: DelegationLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationCapabilities {
    pub choices: Vec<DelegationChoice>,
    pub resolved_choice: Option<DelegationChoice>,
    pub effective_limits: DelegationLimits,
    pub live_sessions: u16,
    pub live_children: u16,
    pub available_slots: u16,
    pub capabilities: Vec<String>,
    pub enforceable_limits: Vec<String>,
    pub native_helpers: bool,
    pub native_descendant_usage: bool,
    pub reason: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationAuth {
    pub session: SessionId,
    pub capability: String,
    #[serde(default)]
    pub controller: Option<crate::ControllerFence>,
    #[serde(default)]
    pub source: Option<crate::AgentSecret>,
}

impl std::fmt::Debug for DelegationAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DelegationAuth")
            .field("session", &self.session)
            .field("capability", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchContract {
    pub schema: u32,
    pub workflow: String,
    pub controller: String,
    pub task: String,
    pub request: String,
    pub project: String,
    pub workspace: String,
    pub scope: String,
    pub permitted_actions: Vec<String>,
    pub provider: String,
    pub model_provider: String,
    pub model: String,
    pub acceptance_evidence: Vec<String>,
    pub mandatory_capabilities: Vec<String>,
    #[serde(default)]
    pub limits: DelegationLimits,
    #[serde(default)]
    pub previous_request: Option<String>,
}

impl ResearchContract {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DELEGATION_SCHEMA {
            return Err("unsupported contract schema");
        }
        self.limits.validate()?;
        if self.previous_request.as_ref() == Some(&self.request) {
            return Err("retry cannot refer to itself");
        }
        for id in [&self.workflow, &self.controller, &self.task, &self.request]
            .into_iter()
            .chain(self.previous_request.iter())
        {
            if id.is_empty()
                || id.len() > 128
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err("invalid delegation identity");
            }
        }
        if self.provider != "pi" {
            return Err("this flow supports only Pi research assignments");
        }
        if self.permitted_actions != ["read"] {
            return Err("this flow permits read-only research only");
        }
        if self
            .mandatory_capabilities
            .iter()
            .any(|value| value != "read" && value != "result_file")
        {
            return Err("unsupported mandatory capability");
        }
        for text in [
            &self.project,
            &self.workspace,
            &self.model_provider,
            &self.model,
        ] {
            if text.is_empty() || text.len() > 256 || text.chars().any(char::is_control) {
                return Err("invalid selection");
            }
        }
        if self.scope.trim().is_empty() || self.scope.len() > 32_768 {
            return Err("scope must contain 1..32768 bytes");
        }
        if self.acceptance_evidence.is_empty()
            || self.acceptance_evidence.len() > 32
            || self
                .acceptance_evidence
                .iter()
                .any(|v| v.is_empty() || v.len() > 4096)
        {
            return Err("bounded acceptance evidence is required");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentIdentity {
    pub instance: String,
    pub workflow: String,
    pub controller: String,
    pub controller_epoch: u64,
    pub task: String,
    pub attempt: u32,
    pub contract: u32,
    pub request: String,
    pub session: Option<SessionId>,
    pub run: Option<SessionRunId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DispatchStatus {
    Prepared,
    LaunchUnknown,
    Launched,
    InputUnknown,
    Applied,
    Refused,
}

pub fn initial_obligation_revision() -> u64 {
    1
}

fn initial_receipt_version() -> u32 {
    1
}

/// Delivery of one operation, never provider completion or parent acceptance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DispatchOperation {
    Prepare,
    Launch,
    InitialInput,
    Followup,
    Takeover,
    HumanReturn,
    ConfirmHumanReturn,
    Interrupt,
    ConfirmInterrupt,
    AnswerQuestion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DispatchOutcome {
    Accepted,
    Applied,
    Rejected,
    Partial,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDelivery {
    pub written_bytes: u64,
    pub total_bytes: u64,
    pub submit_written: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchReceipt {
    pub request: String,
    pub operation: DispatchOperation,
    #[serde(default = "initial_receipt_version")]
    pub result_version: u32,
    pub identity: AssignmentIdentity,
    pub outcome: DispatchOutcome,
    pub input: Option<InputDelivery>,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentAssignmentSession {
    pub session: SessionId,
    pub run: SessionRunId,
    pub phase: SessionPhase,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentResources {
    pub project: Option<String>,
    pub workspace: Option<String>,
    pub workspace_present: bool,
    /// May identify a replacement run; compare it with the allocated identity.
    pub current_session: Option<CurrentAssignmentSession>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchSubmission {
    pub schema: u32,
    pub identity: AssignmentIdentity,
    pub version: u32,
    pub outcome: String,
    pub summary: String,
    pub evidence: Vec<String>,
    pub unresolved: Vec<String>,
}

impl ResearchSubmission {
    pub fn validate(&self, identity: &AssignmentIdentity) -> Result<(), &'static str> {
        if self.schema != DELEGATION_SCHEMA || self.version == 0 {
            return Err("unsupported or stale submission version");
        }
        if &self.identity != identity {
            return Err("foreign or stale submission identity");
        }
        if !["success", "failed", "blocked"].contains(&self.outcome.as_str()) {
            return Err("invalid outcome");
        }
        if self.summary.trim().is_empty() || self.summary.len() > 65536 {
            return Err("invalid summary");
        }
        if self.evidence.len() > 64
            || self.unresolved.len() > 64
            || self
                .evidence
                .iter()
                .chain(&self.unresolved)
                .any(|s| s.len() > 4096)
        {
            return Err("submission exceeds limits");
        }
        if self.outcome == "success" && self.evidence.is_empty() {
            return Err("success requires evidence");
        }
        Ok(())
    }
}

/// Exact immutable submission reference; handling never implies acceptance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicationKind {
    Submission,
    Question {
        result_version: u32,
        question_id: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicationIdentity {
    pub assignment: AssignmentIdentity,
    pub kind: PublicationKind,
    pub version: u32,
    pub digest: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationState {
    pub identity: PublicationIdentity,
    pub handled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceReference {
    pub identity: AssignmentIdentity,
    pub result_version: u32,
    pub result_digest: String,
    pub path: PathBuf,
    pub digest: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentVersion {
    pub identity: AssignmentIdentity,
    pub result_version: u32,
    pub contract: PathBuf,
    pub brief: PathBuf,
    pub publication: Option<PublicationState>,
    pub acceptance: Option<AcceptanceReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentSnapshot {
    pub handle: String,
    pub identity: AssignmentIdentity,
    pub dispatch: DispatchStatus,
    pub reason: Option<String>,
    pub activity: AgentActivity,
    pub conversation: Option<String>,
    pub brief: PathBuf,
    pub result_path: PathBuf,
    pub submission: Option<ResearchSubmission>,
    /// Only an explicit parent RecordAcceptance changes acceptance.
    pub acceptance: String,
    pub result_version: u32,
    pub publication: Option<PublicationState>,
    pub acceptance_ref: Option<AcceptanceReference>,
    pub history: Vec<AssignmentVersion>,
    pub selected_version: Option<u32>,
    pub historical_submission: Option<ResearchSubmission>,
    pub receipts: Vec<DispatchReceipt>,
    pub resources: AssignmentResources,
    pub control: AssignmentControl,
    pub control_evidence: Option<AssignmentControlEvidence>,
    pub retryable_capacity_refusal: bool,
    pub successor_request: Option<String>,
    pub execution_attempts: Option<u32>,
    #[serde(default)]
    pub questions: Vec<crate::QuestionPublicationState>,
    #[serde(default)]
    pub collected_question: Option<crate::AssignmentQuestion>,
    #[serde(default = "initial_obligation_revision")]
    pub obligation_revision: u64,
    #[serde(default)]
    pub archive_receipts: Vec<crate::ArchiveReceipt>,
}

/// A hint cursor is valid only for one Server lifetime. It is not task completion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationCursor {
    pub server_epoch: String,
    pub sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicationIssue {
    Uncommitted,
    Unavailable,
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationObservation {
    pub handle: String,
    pub identity: AssignmentIdentity,
    pub binding: Option<AgentBinding>,
    pub activity: Option<ActivitySample>,
    pub activity_revision: u64,
    pub input_requests: Vec<InputRequest>,
    pub input_revision: u64,
    pub health: Option<ReporterHealth>,
    /// False means retained evidence must not satisfy the current assignment.
    pub current: bool,
    pub publication: Option<PublicationState>,
    pub publication_issue: Option<PublicationIssue>,
    pub last_progress_unix_ms: u64,
    pub stalled: bool,
    /// A gap/epoch change cannot manufacture fresh progress from a snapshot.
    pub progress_unresolved: bool,
    /// Current durable authority; a wake is never confirmation or permission.
    pub control: AssignmentControl,
    /// Content-free journal change marker, never delivery or progress proof.
    pub control_receipt_digest: String,
    pub native_control_revision: u64,
    pub workflow_cancelled: bool,
    #[serde(default)]
    pub questions: Vec<crate::QuestionPublicationState>,
    #[serde(default)]
    pub native_question_gap: Option<crate::NativeQuestionGap>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationStatus {
    Changed,
    Deadline,
    ReconcileRequired,
    Stalled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationObservationBatch {
    pub cursor: ObservationCursor,
    pub status: ObservationStatus,
    pub progress_deadline_ms: u64,
    pub observations: Vec<DelegationObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelegationRequest {
    Prepare {
        auth: DelegationAuth,
        contract: ResearchContract,
    },
    Launch {
        auth: DelegationAuth,
        handle: String,
        identity: AssignmentIdentity,
    },
    SendInitial {
        auth: DelegationAuth,
        handle: String,
        identity: AssignmentIdentity,
        expected_control_epoch: u64,
    },
    Inspect {
        auth: DelegationAuth,
        handle: String,
    },
    Reconcile {
        auth: DelegationAuth,
        request: String,
    },
    Collect {
        auth: DelegationAuth,
        handle: String,
    },
    Followup {
        auth: DelegationAuth,
        handle: String,
        request: String,
        expected_identity: AssignmentIdentity,
        expected_result_version: u32,
        expected_control_epoch: u64,
        text: String,
        revised_contract: Option<Box<ResearchContract>>,
    },
    CollectVersion {
        auth: DelegationAuth,
        handle: String,
        version: u32,
    },
    RecordAcceptance {
        auth: DelegationAuth,
        handle: String,
        identity: AssignmentIdentity,
        result_version: u32,
        result_digest: String,
        expected_control_epoch: u64,
    },
    ConfirmHumanReturn {
        auth: DelegationAuth,
        handle: String,
        request: String,
        returned_request: String,
        expected_control_epoch: u64,
        evidence: Box<AssignmentControlEvidence>,
    },
    Publish {
        auth: DelegationAuth,
        handle: String,
        submission: ResearchSubmission,
    },
    Observe {
        auth: DelegationAuth,
        handles: Vec<String>,
        after: Option<ObservationCursor>,
        wait_ms: u64,
    },
    MarkHandled {
        auth: DelegationAuth,
        handle: String,
        publication: PublicationIdentity,
    },
    Capabilities {
        auth: DelegationAuth,
        selection: DelegationSelection,
    },
    Interrupt {
        auth: DelegationAuth,
        handle: String,
        request: String,
        expected_identity: AssignmentIdentity,
        expected_result_version: u32,
        expected_control_epoch: u64,
    },
    ConfirmInterrupt {
        auth: DelegationAuth,
        handle: String,
        request: String,
        interrupted_request: String,
        expected_control_epoch: u64,
        evidence: Box<AssignmentControlEvidence>,
    },
    PublishQuestion {
        auth: DelegationAuth,
        handle: String,
        question: crate::AssignmentQuestion,
    },
    CollectQuestion {
        auth: DelegationAuth,
        handle: String,
        reference: crate::QuestionReference,
    },
    AnswerQuestion {
        auth: DelegationAuth,
        handle: String,
        answer: crate::QuestionAnswer,
        expected_parent_epoch: u64,
    },
    Archive {
        auth: DelegationAuth,
        archive: crate::ArchiveRequest,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mandatory_limits_refuse_unsupported_budgets_and_out_of_range_admission() {
        assert!(DelegationLimits::default().validate().is_ok());
        for limit in [
            DelegationLimits {
                max_live_children: Some(0),
                ..Default::default()
            },
            DelegationLimits {
                max_live_children: Some(51),
                ..Default::default()
            },
            DelegationLimits {
                max_execution_attempts: Some(0),
                ..Default::default()
            },
            DelegationLimits {
                min_free_slots_after_launch: 50,
                ..Default::default()
            },
            DelegationLimits {
                max_cost_microusd: Some(1),
                ..Default::default()
            },
            DelegationLimits {
                max_output_tokens: Some(1),
                ..Default::default()
            },
            DelegationLimits {
                max_native_children: Some(0),
                ..Default::default()
            },
        ] {
            assert!(limit.validate().is_err(), "{limit:?}");
        }
        assert!(
            DelegationLimits {
                max_live_children: Some(50),
                max_execution_attempts: Some(1),
                min_free_slots_after_launch: 49,
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn research_contract_rejects_unenforceable_permissions_and_capabilities() {
        let mut c = ResearchContract {
            schema: 1,
            workflow: "wf".into(),
            controller: "parent".into(),
            task: "task".into(),
            request: "req".into(),
            project: "proj".into(),
            workspace: "ws".into(),
            scope: "Read README".into(),
            permitted_actions: vec!["read".into()],
            provider: "pi".into(),
            model_provider: "test".into(),
            model: "model".into(),
            acceptance_evidence: vec!["README citation".into()],
            mandatory_capabilities: vec!["result_file".into()],
            limits: DelegationLimits::default(),
            previous_request: None,
        };
        assert!(c.validate().is_ok());
        c.permitted_actions.push("write".into());
        assert!(c.validate().is_err());
        c.permitted_actions.pop();
        c.mandatory_capabilities.push("network_budget".into());
        assert!(c.validate().is_err());
        c.mandatory_capabilities.pop();
        c.request = "../escape".into();
        assert!(c.validate().is_err());
    }
}

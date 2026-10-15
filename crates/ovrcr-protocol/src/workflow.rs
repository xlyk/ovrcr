//! Current workflow authority is separate from immutable assignment provenance.
use crate::{AgentBinding, DelegationAuth, SessionId, SessionRunId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerFence {
    pub workflow: String,
    pub epoch: u64,
    pub producer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowAuthority {
    pub instance: String,
    pub workflow: String,
    pub controller: String,
    pub epoch: u64,
    pub parent: SessionId,
    pub parent_run: SessionRunId,
    pub binding: AgentBinding,
    pub producer: String,
    pub server_lifetime: String,
}
impl WorkflowAuthority {
    pub fn fence(&self) -> ControllerFence {
        ControllerFence { workflow: self.workflow.clone(), epoch: self.epoch, producer: self.producer.clone() }
    }
}

/// A small index into original private artifacts, never control permission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuationIndex {
    pub schema: u32,
    pub objective: String,
    pub decisions: Vec<String>,
    pub unresolved_assignments: Vec<String>,
    pub questions: Vec<String>,
    pub receipts: Vec<String>,
    pub accepted_evidence: Vec<String>,
    pub next_actions: Vec<String>,
}
impl ContinuationIndex {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != 1 || self.objective.len() > 4096 || self.objective.chars().any(|c| c == '\0') { return Err("invalid continuation schema or objective"); }
        for list in [&self.decisions, &self.unresolved_assignments, &self.questions, &self.receipts, &self.accepted_evidence, &self.next_actions] {
            if list.len() > 256 || list.iter().any(|v| v.is_empty() || v.len() > 4096 || v.contains('\0')) { return Err("continuation index exceeds bounds"); }
        }
        if serde_json::to_vec(self).map_err(|_| "invalid continuation")?.len() > 65536 { return Err("continuation index exceeds 65536 bytes"); }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuationPointer {
    pub schema: u32,
    pub instance: String,
    pub workflow: String,
    pub version: u64,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTransitionReceipt {
    pub request: String,
    pub digest: String,
    pub previous: WorkflowAuthority,
    pub current: WorkflowAuthority,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowSnapshot {
    pub authority: WorkflowAuthority,
    pub active: bool,
    /// Includes prepared assignments and assignments from earlier controller epochs.
    pub handles: Vec<String>,
    pub pointer: Option<ContinuationPointer>,
    pub continuation: Option<ContinuationIndex>,
    pub gaps: Vec<String>,
    pub receipt: Option<WorkflowTransitionReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTransfer {
    pub request: String,
    pub expected: WorkflowAuthority,
    pub parent: SessionId,
    pub parent_run: SessionRunId,
    pub binding: AgentBinding,
    pub producer: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowRequest {
    /// Omitting workflow discovers only workflows already owned by this exact source.
    Recover { auth: DelegationAuth, workflow: Option<String> },
    Save { auth: DelegationAuth, workflow: String, expected_version: u64, index: ContinuationIndex },
    Transfer { auth: DelegationAuth, transfer: WorkflowTransfer },
}

/// Only the real active Dashboard can list or apply these transfers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DashboardWorkflowRequest {
    Candidates { parent: SessionId, run: SessionRunId },
    Transfer(WorkflowTransfer),
}

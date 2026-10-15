//! Content-free, Server-owned authority for one assigned child.
use crate::{AgentBinding, AssignmentIdentity, PublicationIdentity, SessionId, SessionRunId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlOwner {
    Controller,
    Human,
    Uncertain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlBarrier {
    HumanReturn { request: String },
    AttributionChanged,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentControl {
    pub epoch: u64,
    pub owner: ControlOwner,
    pub barrier: Option<ControlBarrier>,
    pub human_input_revision: u64,
    pub attribution_invalidated: bool,
}

impl Default for AssignmentControl {
    fn default() -> Self {
        Self {
            epoch: 1,
            owner: ControlOwner::Controller,
            barrier: None,
            human_input_revision: 0,
            attribution_invalidated: false,
        }
    }
}

/// Bounded reasons; publication contents and filesystem error text are not authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlPublicationIssue {
    Missing,
    Invalid,
    Unattributed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentControlEvidence {
    pub server_lifetime: String,
    pub identity: AssignmentIdentity,
    pub binding: AgentBinding,
    pub result_version: u32,
    pub activity_revision: u64,
    pub input_revision: u64,
    pub publication: Option<PublicationIdentity>,
    pub publication_issue: Option<ControlPublicationIssue>,
    pub human_input_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentControlView {
    pub handle: String,
    pub control: AssignmentControl,
    pub evidence: Option<AssignmentControlEvidence>,
}

/// Dashboard-only mutations. A view/focus change never creates one of these.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssignmentControlRequest {
    Takeover {
        request: String,
        session: SessionId,
        run: SessionRunId,
        expected_epoch: u64,
        view_revision: u64,
    },
    Return {
        request: String,
        session: SessionId,
        run: SessionRunId,
        expected_epoch: u64,
        view_revision: u64,
        evidence: AssignmentControlEvidence,
    },
}

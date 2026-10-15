//! The first delegation slice: one read-only Pi assignment and explicit collection.
use crate::{AgentActivity, SessionId, SessionRunId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DELEGATION_SCHEMA: u32 = 1;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationAuth {
    pub session: SessionId,
    pub capability: String,
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
}

impl ResearchContract {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != DELEGATION_SCHEMA {
            return Err("unsupported contract schema");
        }
        for id in [&self.workflow, &self.controller, &self.task, &self.request] {
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
        if self.schema != DELEGATION_SCHEMA || self.version != 1 {
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
    /// A child submission never changes parent acceptance. This slice only collects.
    pub acceptance: String,
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
    },
    SendInitial {
        auth: DelegationAuth,
        handle: String,
        identity: AssignmentIdentity,
    },
    Inspect {
        auth: DelegationAuth,
        handle: String,
    },
    Collect {
        auth: DelegationAuth,
        handle: String,
    },
    Publish {
        auth: DelegationAuth,
        handle: String,
        submission: ResearchSubmission,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
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

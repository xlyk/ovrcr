use crate::context::ContextUsageSnapshot;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

/// Generation of a retained session row. Launch, archive and unarchive invalidate
/// prior process ownership and requests. Distinct from scheduled-task [`crate::RunId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionRunId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

/// How the session was launched. Agent `name` is the selected preset or detected
/// executable; it is not proof of reporting or resume support.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionKind {
    Terminal,
    Agent { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecovery {
    pub conversation: Option<String>,
    pub attached: bool,
    pub requires_ack: bool,
    pub unavailable: Option<String>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionPhase {
    Running,
    Paused,
    Exited {
        code: Option<u32>,
        signal: Option<String>,
    },
    Stopped,
    Interrupted,
}

impl SessionPhase {
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentActivity {
    Unknown,
    Idle,
    Busy,
    WaitingInput,
    Error,
    ResponseReady,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub archived: bool,
    /// Original working directory, retained even after workspace removal.
    pub cwd: std::path::PathBuf,
    pub run: SessionRunId,
    pub kind: SessionKind,
    pub recovery: Option<SessionRecovery>,
    pub project: String,
    pub workspace: String,
    pub name: String,
    pub label: String,
    pub pid: Option<u32>,
    pub started_unix_ms: Option<u64>,
    pub phase: SessionPhase,
    pub activity: AgentActivity,
    pub agent: Option<crate::AgentSnapshot>,
    pub agent_epoch: u64,
    pub unread: Option<crate::ReadyObservation>,
    pub context_usage: Option<ContextUsageSnapshot>,
    /// Effective display title; the stable `name` remains the command identity.
    pub title: Option<String>,
}

impl SessionSummary {
    pub fn display_name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_ready_and_existing_activity_wire_spellings_round_trip() {
        for state in [
            "Unknown",
            "Idle",
            "Busy",
            "WaitingInput",
            "Error",
            "ResponseReady",
        ] {
            let wire = format!("\"{state}\"");
            let decoded: AgentActivity = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
        }
    }

    #[test]
    fn live_phases_are_running_and_paused_only() {
        assert!(SessionPhase::Running.is_live());
        assert!(SessionPhase::Paused.is_live());
        assert!(!SessionPhase::Stopped.is_live());
        assert!(!SessionPhase::Interrupted.is_live());
        assert!(
            !SessionPhase::Exited {
                code: Some(0),
                signal: None
            }
            .is_live()
        );
    }
}

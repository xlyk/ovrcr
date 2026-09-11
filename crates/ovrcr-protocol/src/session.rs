use crate::context::ContextUsageSnapshot;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionPhase {
    Running,
    Paused,
    Exited {
        code: Option<u32>,
        signal: Option<String>,
    },
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
    pub project: String,
    pub workspace: String,
    pub name: String,
    pub label: String,
    pub pid: Option<u32>,
    pub started_unix_ms: u64,
    pub phase: SessionPhase,
    pub activity: AgentActivity,
    pub agent: Option<crate::AgentSnapshot>,
    pub agent_epoch: u64,
    pub context_usage: Option<ContextUsageSnapshot>,
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
}

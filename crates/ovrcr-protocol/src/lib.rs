pub mod agent;
pub use agent::*;
pub mod client;
mod codec;
pub mod context;
pub mod freshness;
mod registry;
mod session;
pub mod task;
mod wire;

pub use codec::{
    MAX_FRAME_BYTES, PROTOCOL_VERSION, connect_server, exchange_preamble, read_frame,
    read_preamble, write_frame, write_preamble,
};
pub use registry::{ProjectRecord, Registry, WorkspaceRecord, new_workspace_id, validate_name};
pub use session::{
    AgentActivity, SessionId, SessionKind, SessionPhase, SessionRecovery, SessionRunId,
    SessionSummary, TerminalSize,
};
pub use task::{
    Run, RunId, RunStatus, RunTrigger, Schedule, Task, TaskId, TaskRequest, TaskResponse, TaskSpec,
    TaskTarget,
};
pub use wire::*;

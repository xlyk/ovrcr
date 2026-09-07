mod codec;
pub mod context;
mod registry;
mod session;
pub mod task;
mod wire;

pub use codec::{MAX_FRAME_BYTES, read_frame, write_frame};
pub use registry::{ProjectRecord, Registry, WorkspaceRecord};
pub use session::{AgentActivity, SessionId, SessionPhase, SessionSummary, TerminalSize};
pub use task::{
    Run, RunId, RunStatus, RunTrigger, Schedule, Task, TaskId, TaskRequest, TaskResponse, TaskSpec,
    TaskTarget,
};
pub use wire::*;

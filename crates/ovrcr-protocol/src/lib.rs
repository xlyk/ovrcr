mod codec;
pub mod context;
mod registry;
mod session;
mod wire;

pub use codec::{MAX_FRAME_BYTES, read_frame, write_frame};
pub use registry::{ProjectRecord, Registry, WorkspaceRecord};
pub use session::{AgentActivity, SessionId, SessionPhase, SessionSummary, TerminalSize};
pub use wire::*;

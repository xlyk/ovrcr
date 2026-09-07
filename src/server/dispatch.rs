use crate::protocol::{AgentReport, Response};
use crate::session::{SessionEvent, SessionId, TerminalSize};

pub enum DispatchMessage {
    Session(SessionEvent),
    AgentReport {
        report: AgentReport,
        completion: std::sync::mpsc::SyncSender<Response>,
    },
    RefreshSession {
        session: SessionId,
    },
    Select {
        request_id: u64,
        session: SessionId,
        size: TerminalSize,
        completion: std::sync::mpsc::SyncSender<()>,
    },
    Stop,
}

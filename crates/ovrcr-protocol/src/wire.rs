use crate::context::ContextUsageReport;
use crate::{
    AgentActivity, Registry, SessionId, SessionSummary, TaskRequest, TaskResponse, TerminalSize,
};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::PathBuf;

pub const HISTORY_ROWS: usize = 512;
pub const PAGE_ROWS: u16 = 16;
pub const PAGE_COLS: u16 = 128;
pub const PAGE_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistorySnapshotId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCell {
    pub text: String,
    pub width: u8,
    pub fg: HistoryColor,
    pub bg: HistoryColor,
    pub attributes: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRow {
    pub width: u16,
    pub cells: Vec<HistoryCell>,
    pub wrapped: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryOpened {
    pub session: SessionId,
    pub snapshot: HistorySnapshotId,
    pub revision: u64,
    pub size: TerminalSize,
    pub history_rows: u32,
    pub total_rows: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRows {
    pub session: SessionId,
    pub snapshot: HistorySnapshotId,
    pub start_row: u32,
    pub start_col: u16,
    pub rows: Vec<HistoryRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientMessage {
    pub request_id: u64,
    pub request: Request,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneTarget {
    pub session: SessionId,
    pub size: TerminalSize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardView {
    pub revision: u64,
    pub panes: Vec<PaneTarget>,
    pub focused: Option<SessionId>,
}

impl DashboardView {
    pub fn validate(&self) -> Result<(), String> {
        if self.revision == 0 {
            return Err("view revision must be greater than zero".into());
        }
        if self.panes.len() > 2 {
            return Err("view cannot contain more than two panes".into());
        }
        for (index, pane) in self.panes.iter().enumerate() {
            if pane.size.rows == 0 || pane.size.cols == 0 {
                return Err("pane dimensions must be greater than zero".into());
            }
            if self.panes[index + 1..]
                .iter()
                .any(|other| other.session == pane.session)
            {
                return Err("view cannot contain duplicate sessions".into());
            }
        }
        match (self.panes.is_empty(), self.focused) {
            (true, None) => Ok(()),
            (true, Some(_)) => Err("an empty view cannot have focus".into()),
            (false, None) => Err("a nonempty view must have focus".into()),
            (false, Some(session)) if self.panes.iter().any(|pane| pane.session == session) => {
                Ok(())
            }
            (false, Some(_)) => Err("focused session must be in the pane list".into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentUpdate {
    Activity(AgentActivity),
    Context(ContextUsageReport),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReport {
    pub session: SessionId,
    pub capability: [u8; 32],
    pub sequence: Option<u64>,
    pub update: AgentUpdate,
}

impl std::fmt::Debug for AgentReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "AgentReport {{ session: {:?}, capability: [redacted], sequence: {:?}, update: {:?} }}",
            self.session, self.sequence, self.update
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    Response { request_id: u64, response: Response },
    Event(ServerEvent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientRole {
    Control,
    Dashboard,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BranchRequest {
    New { branch: String, base: String },
    Existing { branch: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub project: String,
    pub workspace: String,
    pub name: String,
    pub label: Option<String>,
    pub argv: Vec<OsString>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    DashboardHello,
    DashboardGeometry {
        size: TerminalSize,
    },
    List,
    AddProject {
        name: String,
        repo: PathBuf,
        workspace_root: PathBuf,
    },
    RemoveProject {
        name: String,
    },
    CreateWorkspace {
        project: String,
        name: String,
        branch: BranchRequest,
    },
    RemoveWorkspace {
        project: String,
        name: String,
    },
    CreateSession(CreateSessionRequest),
    RemoveSession {
        session: SessionId,
    },
    KillSession {
        session: SessionId,
    },
    PauseSession {
        session: SessionId,
    },
    ResumeSession {
        session: SessionId,
    },
    Select {
        session: SessionId,
        size: TerminalSize,
    },
    Input {
        session: SessionId,
        bytes: Vec<u8>,
    },
    Resize {
        session: SessionId,
        size: TerminalSize,
    },
    Shutdown {
        kill: bool,
    },
    Inspect,
    ReadTerminal {
        session: SessionId,
        max_lines: Option<usize>,
    },
    SendTerminal {
        session: SessionId,
        text: String,
        submit: bool,
    },
    CloseTerminal {
        session: SessionId,
    },
    Task(Box<TaskRequest>),
    AgentReport(AgentReport),
    HistoryBegin {
        session: SessionId,
    },
    HistoryPage {
        session: SessionId,
        snapshot: HistorySnapshotId,
        start_row: u32,
        rows: u16,
        start_col: u16,
        cols: u16,
    },
    HistoryEnd {
        session: SessionId,
        snapshot: HistorySnapshotId,
    },
    SetView {
        view: DashboardView,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    InvalidRequest,
    NotFound,
    AlreadyExists,
    Conflict,
    DirtyWorktree,
    SessionRunning,
    SessionsRemain,
    PartialFailure,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HierarchySnapshot {
    pub projects: Vec<ProjectSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub name: String,
    pub workspaces: Vec<WorkspaceSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub project: String,
    pub name: String,
    pub path: PathBuf,
    pub sessions: Vec<SessionSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Ok,
    Hierarchy(HierarchySnapshot),
    CreatedSession(SessionSummary),
    Screen {
        session: SessionId,
        revision: u64,
        size: TerminalSize,
        bytes: Vec<u8>,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
    Inventory {
        registry: Registry,
        sessions: Vec<SessionSummary>,
    },
    TerminalText {
        session: SessionId,
        size: TerminalSize,
        text: String,
    },
    Task(Box<TaskResponse>),
    HistoryOpened(HistoryOpened),
    HistoryRows(HistoryRows),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerEvent {
    HierarchyChanged(HierarchySnapshot),
    Output {
        session: SessionId,
        revision: u64,
        bytes: Vec<u8>,
    },
    ScreenDirty {
        session: SessionId,
        revision: u64,
    },
    SessionChanged(SessionSummary),
}

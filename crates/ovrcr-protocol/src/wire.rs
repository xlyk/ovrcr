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
    CreatedSession(Box<SessionSummary>),
    Screen {
        session: SessionId,
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
    Output { session: SessionId, bytes: Vec<u8> },
    ScreenDirty { session: SessionId },
    SessionChanged(Box<SessionSummary>),
}

#[cfg(test)]
mod wire_snapshot {
    //! Pins the bincode encoding of every enum variant that crosses the
    //! socket. bincode numbers variants by declaration order, so inserting a
    //! variant mid-enum silently changes the meaning of every later index.
    //! When this test fails, either restore the order or bump
    //! `PROTOCOL_VERSION` and replace `EXPECTED` with the printed block.
    use super::*;
    use crate::context::{ContextSource, ContextUsageReport, ContextUsageSnapshot};
    use crate::{AgentActivity, RunId, SessionPhase, TaskId, TaskRequest, TaskResponse};

    fn encode<T: Serialize>(value: &T) -> String {
        bincode::serde::encode_to_vec(value, bincode::config::standard())
            .unwrap()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn summary() -> SessionSummary {
        SessionSummary {
            id: SessionId(1),
            project: "p".into(),
            workspace: "w".into(),
            name: "n".into(),
            label: "l".into(),
            pid: Some(2),
            started_unix_ms: 3,
            phase: SessionPhase::Running,
            activity: AgentActivity::Unknown,
            context_usage: Some(ContextUsageSnapshot {
                report: ContextUsageReport {
                    source: ContextSource::Generic,
                    model: None,
                    conversation: None,
                    used_tokens: Some(4),
                    capacity_tokens: Some(5),
                },
                received_unix_ms: 6,
            }),
        }
    }

    fn size() -> TerminalSize {
        TerminalSize { rows: 1, cols: 2 }
    }

    fn requests() -> Vec<(&'static str, Request)> {
        vec![
            ("DashboardHello", Request::DashboardHello),
            (
                "DashboardGeometry",
                Request::DashboardGeometry { size: size() },
            ),
            ("List", Request::List),
            (
                "AddProject",
                Request::AddProject {
                    name: "a".into(),
                    repo: "/r".into(),
                    workspace_root: "/w".into(),
                },
            ),
            ("RemoveProject", Request::RemoveProject { name: "a".into() }),
            (
                "CreateWorkspace",
                Request::CreateWorkspace {
                    project: "a".into(),
                    name: "b".into(),
                    branch: BranchRequest::Existing { branch: "c".into() },
                },
            ),
            (
                "RemoveWorkspace",
                Request::RemoveWorkspace {
                    project: "a".into(),
                    name: "b".into(),
                },
            ),
            (
                "CreateSession",
                Request::CreateSession(CreateSessionRequest {
                    project: "a".into(),
                    workspace: "b".into(),
                    name: "c".into(),
                    label: None,
                    argv: vec!["sh".into()],
                }),
            ),
            (
                "RemoveSession",
                Request::RemoveSession {
                    session: SessionId(1),
                },
            ),
            (
                "KillSession",
                Request::KillSession {
                    session: SessionId(1),
                },
            ),
            (
                "PauseSession",
                Request::PauseSession {
                    session: SessionId(1),
                },
            ),
            (
                "ResumeSession",
                Request::ResumeSession {
                    session: SessionId(1),
                },
            ),
            (
                "Select",
                Request::Select {
                    session: SessionId(1),
                    size: size(),
                },
            ),
            (
                "Input",
                Request::Input {
                    session: SessionId(1),
                    bytes: vec![7],
                },
            ),
            (
                "Resize",
                Request::Resize {
                    session: SessionId(1),
                    size: size(),
                },
            ),
            ("Shutdown", Request::Shutdown { kill: true }),
            ("Inspect", Request::Inspect),
            (
                "ReadTerminal",
                Request::ReadTerminal {
                    session: SessionId(1),
                    max_lines: Some(3),
                },
            ),
            (
                "SendTerminal",
                Request::SendTerminal {
                    session: SessionId(1),
                    text: "t".into(),
                    submit: true,
                },
            ),
            (
                "CloseTerminal",
                Request::CloseTerminal {
                    session: SessionId(1),
                },
            ),
            ("Task", Request::Task(Box::new(TaskRequest::ListTasks))),
            (
                "AgentReport",
                Request::AgentReport(AgentReport {
                    session: SessionId(1),
                    capability: [9; 32],
                    sequence: Some(1),
                    update: AgentUpdate::Activity(AgentActivity::Busy),
                }),
            ),
            (
                "HistoryBegin",
                Request::HistoryBegin {
                    session: SessionId(1),
                },
            ),
            (
                "HistoryPage",
                Request::HistoryPage {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(2),
                    start_row: 3,
                    rows: 4,
                    start_col: 5,
                    cols: 6,
                },
            ),
            (
                "HistoryEnd",
                Request::HistoryEnd {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(2),
                },
            ),
        ]
    }

    fn responses() -> Vec<(&'static str, Response)> {
        vec![
            ("Ok", Response::Ok),
            (
                "Hierarchy",
                Response::Hierarchy(HierarchySnapshot {
                    projects: Vec::new(),
                }),
            ),
            (
                "CreatedSession",
                Response::CreatedSession(Box::new(summary())),
            ),
            (
                "Screen",
                Response::Screen {
                    session: SessionId(1),
                    size: size(),
                    bytes: vec![7],
                },
            ),
            (
                "Error",
                Response::Error {
                    code: ErrorCode::NotFound,
                    message: "m".into(),
                },
            ),
            (
                "Inventory",
                Response::Inventory {
                    registry: Registry::default(),
                    sessions: vec![summary()],
                },
            ),
            (
                "TerminalText",
                Response::TerminalText {
                    session: SessionId(1),
                    size: size(),
                    text: "t".into(),
                },
            ),
            (
                "Task",
                Response::Task(Box::new(TaskResponse::Concurrency(1))),
            ),
            (
                "HistoryOpened",
                Response::HistoryOpened(HistoryOpened {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(2),
                    revision: 3,
                    size: size(),
                    history_rows: 4,
                    total_rows: 5,
                }),
            ),
            (
                "HistoryRows",
                Response::HistoryRows(HistoryRows {
                    session: SessionId(1),
                    snapshot: HistorySnapshotId(2),
                    start_row: 3,
                    start_col: 4,
                    rows: vec![HistoryRow {
                        width: 1,
                        cells: vec![HistoryCell {
                            text: "x".into(),
                            width: 1,
                            fg: HistoryColor::Default,
                            bg: HistoryColor::Rgb(1, 2, 3),
                            attributes: 0,
                        }],
                        wrapped: false,
                    }],
                }),
            ),
        ]
    }

    fn events() -> Vec<(&'static str, ServerEvent)> {
        vec![
            (
                "HierarchyChanged",
                ServerEvent::HierarchyChanged(HierarchySnapshot {
                    projects: Vec::new(),
                }),
            ),
            (
                "Output",
                ServerEvent::Output {
                    session: SessionId(1),
                    bytes: vec![7],
                },
            ),
            (
                "ScreenDirty",
                ServerEvent::ScreenDirty {
                    session: SessionId(1),
                },
            ),
            (
                "SessionChanged",
                ServerEvent::SessionChanged(Box::new(summary())),
            ),
        ]
    }

    fn task_requests() -> Vec<(&'static str, TaskRequest)> {
        vec![
            ("ListTasks", TaskRequest::ListTasks),
            ("GetTask", TaskRequest::GetTask(TaskId(1))),
            ("Cancel", TaskRequest::Cancel(RunId(1))),
            (
                "Clean",
                TaskRequest::Clean {
                    id: RunId(1),
                    confirmed: true,
                },
            ),
        ]
    }

    fn actual() -> Vec<(String, String)> {
        let mut all = Vec::new();
        all.extend(
            requests()
                .iter()
                .map(|(n, v)| (format!("Request::{n}"), encode(v))),
        );
        all.extend(
            responses()
                .iter()
                .map(|(n, v)| (format!("Response::{n}"), encode(v))),
        );
        all.extend(
            events()
                .iter()
                .map(|(n, v)| (format!("ServerEvent::{n}"), encode(v))),
        );
        all.extend(
            task_requests()
                .iter()
                .map(|(n, v)| (format!("TaskRequest::{n}"), encode(v))),
        );
        all
    }

    // Regenerate with `cargo test -p ovrcr-protocol wire_snapshot -- --nocapture`
    // after bumping PROTOCOL_VERSION.
    const EXPECTED: &[(&str, &str)] = &[
        ("Request::DashboardHello", "00"),
        ("Request::DashboardGeometry", "010102"),
        ("Request::List", "02"),
        ("Request::AddProject", "030161022f72022f77"),
        ("Request::RemoveProject", "040161"),
        ("Request::CreateWorkspace", "0501610162010163"),
        ("Request::RemoveWorkspace", "0601610162"),
        ("Request::CreateSession", "07016101620163000100027368"),
        ("Request::RemoveSession", "0801"),
        ("Request::KillSession", "0901"),
        ("Request::PauseSession", "0a01"),
        ("Request::ResumeSession", "0b01"),
        ("Request::Select", "0c010102"),
        ("Request::Input", "0d010107"),
        ("Request::Resize", "0e010102"),
        ("Request::Shutdown", "0f01"),
        ("Request::Inspect", "10"),
        ("Request::ReadTerminal", "11010103"),
        ("Request::SendTerminal", "1201017401"),
        ("Request::CloseTerminal", "1301"),
        ("Request::Task", "1400"),
        (
            "Request::AgentReport",
            "1501090909090909090909090909090909090909090909090909090909090909090901010002",
        ),
        ("Request::HistoryBegin", "1601"),
        ("Request::HistoryPage", "17010203040506"),
        ("Request::HistoryEnd", "180102"),
        ("Response::Ok", "00"),
        ("Response::Hierarchy", "0100"),
        (
            "Response::CreatedSession",
            "020101700177016e016c0102030000010000000104010506",
        ),
        ("Response::Screen", "030101020107"),
        ("Response::Error", "0401016d"),
        (
            "Response::Inventory",
            "0500010101700177016e016c0102030000010000000104010506",
        ),
        ("Response::TerminalText", "060101020174"),
        ("Response::Task", "070601"),
        ("Response::HistoryOpened", "0801020301020405"),
        (
            "Response::HistoryRows",
            "090102030401010101780100020102030000",
        ),
        ("ServerEvent::HierarchyChanged", "0000"),
        ("ServerEvent::Output", "01010107"),
        ("ServerEvent::ScreenDirty", "0201"),
        (
            "ServerEvent::SessionChanged",
            "030101700177016e016c0102030000010000000104010506",
        ),
        ("TaskRequest::ListTasks", "00"),
        ("TaskRequest::GetTask", "0301"),
        ("TaskRequest::Cancel", "0e01"),
        ("TaskRequest::Clean", "0f0101"),
    ];

    #[test]
    fn wire_encoding_matches_snapshot_for_protocol_version() {
        let actual = actual();
        let expected = EXPECTED
            .iter()
            .map(|(name, hex)| (name.to_string(), hex.to_string()))
            .collect::<Vec<_>>();
        if actual != expected {
            eprintln!(
                "wire snapshot for PROTOCOL_VERSION {}:",
                crate::PROTOCOL_VERSION
            );
            for (name, hex) in &actual {
                eprintln!("        (\"{name}\", \"{hex}\"),");
            }
            panic!(
                "wire encoding changed; bump PROTOCOL_VERSION in codec.rs and replace EXPECTED with the block above"
            );
        }
    }
}

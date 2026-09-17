use crate::context::ContextUsageReport;
use crate::{
    AgentActivity, Registry, SessionId, SessionKind, SessionRunId, SessionSummary, TaskRequest,
    TaskResponse, TerminalSize,
};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::PathBuf;

pub const HISTORY_ROWS: usize = 512;
pub const PAGE_ROWS: u16 = 16;
pub const PAGE_COLS: u16 = 128;
pub const PAGE_BYTES: usize = 128 * 1024;

/// Upper bound on a pane's terminal size. A pane beyond this allocates an
/// oversized `vt100::Screen` and PTY before the resulting frame is found to
/// exceed `MAX_FRAME_BYTES`, so it is rejected in `DashboardView::validate`
/// before any PTY resize.
pub const MAX_PANE_ROWS: u16 = 1000;
pub const MAX_PANE_COLS: u16 = 1000;

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
    pub run: SessionRunId,
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
            if pane.size.rows > MAX_PANE_ROWS || pane.size.cols > MAX_PANE_COLS {
                return Err(format!(
                    "pane dimensions must not exceed {MAX_PANE_ROWS} rows by {MAX_PANE_COLS} columns"
                ));
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
    Provider(crate::ProviderReport),
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
    /// Empty requests a unique workspace-based identity and automatic display title.
    /// An explicit name pins the display title until SetSessionTitle resets it.
    pub name: String,
    pub label: Option<String>,
    pub argv: Vec<OsString>,
    pub kind: SessionKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionLaunch {
    pub argv: Vec<OsString>,
    pub label: Option<String>,
    pub kind: SessionKind,
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
        run: SessionRunId,
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
        expected_run: SessionRunId,
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
    ReserveAgent(crate::ReserveAgent),
    Supervisor(crate::SupervisorRequest),
    AgentStatus {
        auth: crate::SupervisorAuth,
        operation: String,
    },
    SupervisorHello(crate::SupervisorAuth),
    MarkReviewed {
        session: SessionId,
        expected: crate::ReadyObservation,
    },
    CreateWorkspaceWithLaunch {
        project: String,
        name: String,
        branch: BranchRequest,
        launch: Option<SessionLaunch>,
    },
    SetSessionTitle {
        session: SessionId,
        title: Option<String>,
    },
    ReopenSession {
        session: SessionId,
        expected_run: SessionRunId,
        acknowledge_stopped: bool,
    },
    AcknowledgeSessionStopped {
        session: SessionId,
        expected_run: SessionRunId,
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
    WorkspacesRemain,
    OwnershipUncertain,
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
        run: SessionRunId,
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
    AgentOperation(crate::AgentOperationResult),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerEvent {
    HierarchyChanged(HierarchySnapshot),
    Output {
        session: SessionId,
        run: SessionRunId,
        revision: u64,
        bytes: Vec<u8>,
    },
    ScreenDirty {
        session: SessionId,
        run: SessionRunId,
        revision: u64,
    },
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

    fn unread() -> crate::ReadyObservation {
        crate::ReadyObservation {
            binding: crate::AgentBinding {
                provider: crate::AgentProvider::Codex,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
            turn: Some("turn".into()),
            activity_revision: 2,
        }
    }

    fn summary() -> SessionSummary {
        SessionSummary {
            id: SessionId(1),
            run: SessionRunId(4),
            kind: SessionKind::Terminal,
            recovery: None,
            project: "p".into(),
            workspace: "w".into(),
            name: "n".into(),
            label: "l".into(),
            pid: Some(2),
            started_unix_ms: Some(3),
            phase: SessionPhase::Running,
            activity: AgentActivity::Unknown,
            agent: Some(crate::AgentSnapshot {
                binding: unread().binding.clone(),
                activity: Some(crate::ActivitySample {
                    state: AgentActivity::Busy,
                    quality: crate::SampleQuality::Observed,
                    turn: Some("turn".into()),
                }),
                metrics: None,
                health: crate::HealthSample {
                    state: crate::ReporterHealth::Connected,
                    reason: None,
                },
                activity_revision: 2,
                metrics_revision: 0,
                health_revision: 0,
                input_requests: vec![crate::InputRequest {
                    id: "approval:req".into(),
                    kind: crate::InputKind::Approval,
                }],
                input_revision: 3,
            }),
            agent_epoch: 0,
            unread: Some(unread()),
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
            title: None,
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
                    kind: SessionKind::Terminal,
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
                    run: SessionRunId(4),
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
                    expected_run: SessionRunId(1),
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
            (
                "SetView",
                Request::SetView {
                    view: DashboardView {
                        revision: 3,
                        panes: vec![PaneTarget {
                            session: SessionId(1),
                            run: SessionRunId(4),
                            size: size(),
                        }],
                        focused: Some(SessionId(1)),
                    },
                },
            ),
            (
                "CreateWorkspaceWithLaunch",
                Request::CreateWorkspaceWithLaunch {
                    project: "a".into(),
                    name: "b".into(),
                    branch: BranchRequest::Existing { branch: "c".into() },
                    launch: Some(SessionLaunch {
                        argv: vec!["sh".into()],
                        label: None,
                        kind: SessionKind::Terminal,
                    }),
                },
            ),
            (
                "CreateWorkspaceWithoutLaunch",
                Request::CreateWorkspaceWithLaunch {
                    project: "a".into(),
                    name: "b".into(),
                    branch: BranchRequest::Existing { branch: "c".into() },
                    launch: None,
                },
            ),
            (
                "SetSessionTitle",
                Request::SetSessionTitle {
                    session: SessionId(1),
                    title: Some("title".into()),
                },
            ),
            (
                "ResetSessionTitle",
                Request::SetSessionTitle {
                    session: SessionId(1),
                    title: None,
                },
            ),
            (
                "ReopenSession",
                Request::ReopenSession {
                    session: SessionId(1),
                    expected_run: SessionRunId(4),
                    acknowledge_stopped: false,
                },
            ),
            (
                "AcknowledgeSessionStopped",
                Request::AcknowledgeSessionStopped {
                    session: SessionId(1),
                    expected_run: SessionRunId(4),
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
                    run: SessionRunId(4),
                    revision: 3,
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
                    run: SessionRunId(4),
                    revision: 3,
                    bytes: vec![7],
                },
            ),
            (
                "ScreenDirty",
                ServerEvent::ScreenDirty {
                    session: SessionId(1),
                    run: SessionRunId(4),
                    revision: 3,
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
        all.push((
            "Request::MarkReviewed".into(),
            encode(&Request::MarkReviewed {
                session: SessionId(1),
                expected: unread(),
            }),
        ));
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
        all.extend(
            crate::agent::tests::request_fixtures()
                .iter()
                .enumerate()
                .map(|(i, v)| (format!("AgentRequest::{i}"), encode(v))),
        );
        all.extend(
            crate::agent::tests::response_fixtures()
                .iter()
                .enumerate()
                .map(|(i, v)| (format!("AgentResponse::{i}"), encode(v))),
        );
        all.extend(
            [
                AgentActivity::Unknown,
                AgentActivity::Idle,
                AgentActivity::Busy,
                AgentActivity::WaitingInput,
                AgentActivity::Error,
                AgentActivity::ResponseReady,
            ]
            .iter()
            .map(|state| (format!("AgentActivity::{state:?}"), encode(state))),
        );
        all.extend(
            [
                crate::AgentProvider::Claude,
                crate::AgentProvider::Codex,
                crate::AgentProvider::Grok,
                crate::AgentProvider::Pi,
                crate::AgentProvider::Hermes,
                crate::AgentProvider::Omp,
            ]
            .iter()
            .map(|provider| (format!("AgentProvider::{provider:?}"), encode(provider))),
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
        ("Request::CreateSession", "0701610162016300010002736800"),
        ("Request::RemoveSession", "0801"),
        ("Request::KillSession", "0901"),
        ("Request::PauseSession", "0a01"),
        ("Request::ResumeSession", "0b01"),
        ("Request::Select", "0c010102"),
        ("Request::Input", "0d01040107"),
        ("Request::Resize", "0e010102"),
        ("Request::Shutdown", "0f01"),
        ("Request::Inspect", "10"),
        ("Request::ReadTerminal", "11010103"),
        ("Request::SendTerminal", "1201017401"),
        ("Request::CloseTerminal", "130101"),
        ("Request::Task", "1400"),
        (
            "Request::AgentReport",
            "1501090909090909090909090909090909090909090909090909090909090909090901010002",
        ),
        ("Request::HistoryBegin", "1601"),
        ("Request::HistoryPage", "17010203040506"),
        ("Request::HistoryEnd", "180102"),
        ("Request::SetView", "190301010401020101"),
        (
            "Request::CreateWorkspaceWithLaunch",
            "1f016101620101630101000273680000",
        ),
        (
            "Request::CreateWorkspaceWithoutLaunch",
            "1f0161016201016300",
        ),
        ("Request::SetSessionTitle", "200101057469746c65"),
        ("Request::ResetSessionTitle", "200100"),
        ("Request::ReopenSession", "21010400"),
        ("Request::AcknowledgeSessionStopped", "220104"),
        (
            "Request::MarkReviewed",
            "1e010103696e7604636f6e760101047475726e02",
        ),
        ("Response::Ok", "00"),
        ("Response::Hierarchy", "0100"),
        (
            "Response::CreatedSession",
            "020104000001700177016e016c010201030000010103696e7604636f6e760101020101047475726e000000020000010c617070726f76616c3a726571050300010103696e7604636f6e760101047475726e0201000000010401050600",
        ),
        ("Response::Screen", "0301040301020107"),
        ("Response::Error", "0401016d"),
        (
            "Response::Inventory",
            "0500010104000001700177016e016c010201030000010103696e7604636f6e760101020101047475726e000000020000010c617070726f76616c3a726571050300010103696e7604636f6e760101047475726e0201000000010401050600",
        ),
        ("Response::TerminalText", "060101020174"),
        ("Response::Task", "070601"),
        ("Response::HistoryOpened", "0801020301020405"),
        (
            "Response::HistoryRows",
            "090102030401010101780100020102030000",
        ),
        ("ServerEvent::HierarchyChanged", "0000"),
        ("ServerEvent::Output", "010104030107"),
        ("ServerEvent::ScreenDirty", "02010403"),
        (
            "ServerEvent::SessionChanged",
            "030104000001700177016e016c010201030000010103696e7604636f6e760101020101047475726e000000020000010c617070726f76616c3a726571050300010103696e7604636f6e760101047475726e0201000000010401050600",
        ),
        ("TaskRequest::ListTasks", "00"),
        ("TaskRequest::GetTask", "0301"),
        ("TaskRequest::Cancel", "0e01"),
        ("TaskRequest::Clean", "0f0101"),
        (
            "AgentRequest::0",
            "1a01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a507726573657276650003696e7600",
        ),
        (
            "AgentRequest::1",
            "1d01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5",
        ),
        (
            "AgentRequest::2",
            "1c01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70",
        ),
        (
            "AgentRequest::3",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70000004636f6e76",
        ),
        (
            "AgentRequest::4",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70010003696e7604636f6e76010300010a01640007666978747572650000010a01050103010201010766697874757265010001010766697874757265",
        ),
        (
            "AgentRequest::5",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f7002010003696e7604636f6e7601",
        ),
        (
            "AgentRequest::6",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70030003696e7604636f6e7601010201010e636f6c6c6563746f725f6c6f7374",
        ),
        (
            "AgentRequest::7",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70040003696e7604636f6e760104636f6e760b2f62696e2f636c61756465132f686973746f72792f636f6e762e6a736f6e6c0e2f636f6e6669672f636c6175646500",
        ),
        (
            "AgentRequest::8",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f7005",
        ),
        (
            "AgentRequest::9",
            "1501a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a500020003696e7604636f6e76010100020101047475726e",
        ),
        (
            "AgentRequest::10",
            "1501a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a500020003696e7604636f6e7601010100010a01640007666978747572650000010a01050103010201010766697874757265010001010766697874757265",
        ),
        (
            "AgentRequest::11",
            "1501a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a500020003696e7604636f6e7601010201010e636f6c6c6563746f725f6c6f7374",
        ),
        (
            "AgentRequest::12",
            "1501a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a500020003696e7604636f6e76010103010c7175657374696f6e3a72657100",
        ),
        (
            "AgentRequest::13",
            "1501a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a500020003696e7604636f6e7601010300",
        ),
        (
            "AgentResponse::0",
            "0a0001a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5",
        ),
        ("AgentResponse::1", "0a010003696e7604636f6e7601"),
        ("AgentResponse::2", "0a02"),
        ("AgentResponse::3", "0a03"),
        ("AgentResponse::4", "0a04"),
        ("AgentResponse::5", "0a05"),
        ("AgentActivity::Unknown", "00"),
        ("AgentActivity::Idle", "01"),
        ("AgentActivity::Busy", "02"),
        ("AgentActivity::WaitingInput", "03"),
        ("AgentActivity::Error", "04"),
        ("AgentActivity::ResponseReady", "05"),
        ("AgentProvider::Claude", "00"),
        ("AgentProvider::Codex", "01"),
        ("AgentProvider::Grok", "02"),
        ("AgentProvider::Pi", "03"),
        ("AgentProvider::Hermes", "04"),
        ("AgentProvider::Omp", "05"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn view_with(size: TerminalSize) -> DashboardView {
        DashboardView {
            revision: 1,
            panes: vec![PaneTarget {
                session: SessionId(1),
                run: SessionRunId(4),
                size,
            }],
            focused: Some(SessionId(1)),
        }
    }

    #[test]
    fn validate_rejects_oversized_panes() {
        let oversized_rows = view_with(TerminalSize {
            rows: 1001,
            cols: 80,
        });
        let error = oversized_rows
            .validate()
            .expect_err("1001 rows must be rejected");
        assert!(
            error.contains("1000"),
            "error should name the limit: {error}"
        );

        let oversized_cols = view_with(TerminalSize {
            rows: 24,
            cols: 1001,
        });
        let error = oversized_cols
            .validate()
            .expect_err("1001 cols must be rejected");
        assert!(
            error.contains("1000"),
            "error should name the limit: {error}"
        );

        let at_limit = view_with(TerminalSize {
            rows: 1000,
            cols: 1000,
        });
        assert!(at_limit.validate().is_ok(), "1000 by 1000 must pass");
    }
}

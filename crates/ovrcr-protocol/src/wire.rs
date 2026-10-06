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
    /// Empty requests a unique workspace-based session name.
    /// The display title stays at this name unless the user renames it.
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
        id: String,
        branch: BranchRequest,
    },
    RemoveWorkspace {
        project: String,
        name: String,
        /// Acknowledge ownership-uncertain stopped sessions and discard uncommitted
        /// changes. Live sessions and active task runs still block removal.
        force: bool,
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
        id: String,
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
    DeleteArchivedSession {
        session: SessionId,
        expected_run: SessionRunId,
    },
    UnarchiveSession {
        session: SessionId,
        expected_run: SessionRunId,
    },
    AcknowledgeSessionStopped {
        session: SessionId,
        expected_run: SessionRunId,
    },
    /// First display of an interrupted agent; never substitutes for explicit Retry or acknowledgement.
    RecoverSession {
        session: SessionId,
        expected_run: SessionRunId,
    },
    /// One named key, such as `:enter:`, written to the current run as the
    /// keyboard would send it. Refused while the Dashboard is focused on it.
    Keystroke {
        session: SessionId,
        expected_run: SessionRunId,
        key: String,
    },
    /// Edit one setting in the settings document. `path` is a setting path
    /// such as `quota.codex.command`, `picker_roots[2]` or
    /// `launch_choices.myproj.kind`; `value` is TOML value text, and `None`
    /// removes the key or element. The Server validates, writes, re-reads and
    /// republishes `SettingsChanged`.
    SetSetting {
        path: String,
        value: Option<String>,
    },
    /// Mark enabled native quota workers (all when `None`) due now. Refused within
    /// 30 s of the last accepted refresh; never skips a provider Retry-After.
    RefreshQuota {
        provider: Option<crate::QuotaProvider>,
    },
    /// The in-memory ring, oldest first. `follow` keeps a control connection
    /// open and delivers later lines as `ServerEvent::Recorded`. The file is
    /// not read.
    Events {
        follow: bool,
    },
    /// Connect-only notification callback. Never launches or recovers a session.
    NavigateNotification {
        ticket: crate::BridgeNavigationTicket,
    },
    ConfirmNotificationNavigation {
        navigation: String,
    },
    NotificationNavigationApplied {
        navigation: String,
    },
    /// Terminal context supplied only by the already admitted Dashboard owner.
    DashboardBridgeIdentity {
        iterm_session_id: Option<String>,
    },
    /// Only the currently admitted Dashboard may start explicit iTerm setup.
    PrepareITermFocus,
    /// Connect-only native helper: validate current owner, optionally publish
    /// a bounded typed outcome to that owner's existing Dashboard channel.
    BridgeOwner(crate::BridgeOwnerCall),
    /// Commit unignored changes onto `refs/heads/wip/<branch>` and push that
    /// ref to `origin`. Does not move the checkout's branch or remove the
    /// workspace.
    SaveWorkspaceWip {
        project: String,
        name: String,
    },
    /// Answer one shutdown prompt. `workspace` is the workspace id.
    /// `save` false continues shutdown without pushing that worktree.
    AnswerWipSave {
        project: String,
        workspace: String,
        save: bool,
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
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub root: bool,
    pub warning: Option<String>,
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
    /// A manual quota refresh refused by the Server's cooldown.
    QuotaCooldown {
        remaining_ms: u64,
    },
    /// The Server's event ring, oldest first, newest last.
    Events(Vec<crate::Event>),
    NotificationNavigation(crate::BridgeNavigationResult),
    NotificationNavigationConfirmed(Option<Box<crate::BridgeNavigationOffer>>),
    ITermFocusPrepared(Option<crate::BridgeActivationTarget>),
    BridgeOwner(crate::BridgeOwnerResult),
}

/// Which create/remove/close a Lifecycle job performed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleOp {
    CreateWorkspace,
    RemoveWorkspace,
    CreateSession,
    CloseTerminal,
    CreateWorkspaceWithLaunch,
}

/// Result of a finished Lifecycle job, correlated by `client_token`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleOutcome {
    Succeeded,
    CreatedSession(Box<SessionSummary>),
    Failed { code: ErrorCode, message: String },
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
    QuotaChanged(Box<crate::QuotaSnapshot>),
    /// The Server's reading of the settings document, sent after the hello
    /// response and whenever that reading changes. The Dashboard has no other
    /// source of settings.
    SettingsChanged(Box<crate::SettingsReport>),
    /// One newly recorded event. The attached Dashboard appends it while
    /// its Events popup is open.
    Recorded(crate::Event),
    BridgeContext(crate::BridgeContext),
    NotificationNavigation(crate::BridgeNavigationOffer),
    ITermFocus(crate::ITermFocusStatus),
    /// One dirty feature worktree the attached Dashboard should ask about
    /// before server shutdown continues.
    WipSavePrompt {
        project: String,
        workspace: String,
        branch: String,
    },
    /// A Lifecycle job finished. `client_token` is the request_id that accepted
    /// the job on the Dashboard connection.
    LifecycleCompleted {
        client_token: u64,
        op: LifecycleOp,
        outcome: LifecycleOutcome,
    },
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

    fn settings_report() -> crate::SettingsReport {
        let mut settings = crate::Settings {
            title_model: Some("pi/m".into()),
            picker_roots: vec!["/c".into()],
            agents: vec![crate::AgentOverride {
                name: "a".into(),
                argv: vec!["b".into()],
            }],
            ..Default::default()
        };
        settings
            .launch_choices
            .insert("p".into(), crate::LaunchChoice::Agent("a".into()));
        settings
            .launch_choices
            .insert("q".into(), crate::LaunchChoice::Terminal);
        // Keep the wire fixture's settings values explicit; quota collection
        // is enabled by default for real settings documents.
        settings.quota.enabled = false;
        settings.quota.codex.home = Some("/h".into());
        crate::SettingsReport {
            path: "/d.toml".into(),
            read_unix_ms: 5,
            settings,
            rows: vec![crate::SettingRow {
                key: "quota.enabled".into(),
                owner: crate::SettingOwner::Server,
                value: Some("false".into()),
                source: crate::SettingSource::Default,
                default: Some("false".into()),
                off_state: Some("off".into()),
            }],
            findings: vec![crate::SettingsFinding {
                key: Some("k".into()),
                message: "m".into(),
                line: Some(3),
            }],
            unparseable: false,
        }
    }

    /// bincode has no self-describing format, so a serde shape that needs
    /// `deserialize_any` (an internally tagged enum, for one) would encode
    /// and then fail to decode. The adjacently tagged `LaunchChoice` must not.
    #[test]
    fn settings_report_round_trips() {
        let report = settings_report();
        let bytes = bincode::serde::encode_to_vec(
            ServerMessage::Event(ServerEvent::SettingsChanged(Box::new(report.clone()))),
            bincode::config::standard(),
        )
        .unwrap();
        let (decoded, _): (ServerMessage, _) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert!(matches!(
            decoded,
            ServerMessage::Event(ServerEvent::SettingsChanged(decoded)) if *decoded == report
        ));
    }

    fn summary() -> SessionSummary {
        SessionSummary {
            archived: false,
            cwd: "/work".into(),
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
            manual_title: None,
        }
    }

    fn size() -> TerminalSize {
        TerminalSize { rows: 1, cols: 2 }
    }

    #[test]
    fn session_summary_preserves_manual_title_separately_from_effective_title() {
        for manual_title in [None, Some("User title".to_owned())] {
            let mut session = summary();
            session.title = Some("Conversation subject".into());
            session.manual_title = manual_title;
            let message = ServerMessage::Event(ServerEvent::SessionChanged(Box::new(session)));
            let mut bytes = Vec::new();
            crate::write_frame(&mut bytes, &message).unwrap();
            assert_eq!(
                crate::read_frame::<ServerMessage>(&mut bytes.as_slice()).unwrap(),
                message
            );
        }
    }

    #[test]
    fn json_summary_without_manual_title_defaults_to_none() {
        let mut session = summary();
        session.title = Some("Conversation subject".into());
        let mut json = serde_json::to_value(&session).unwrap();
        json.as_object_mut().unwrap().remove("manual_title");
        let decoded: SessionSummary = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, session);
        assert_eq!(decoded.manual_title, None);
        assert_eq!(decoded.title.as_deref(), Some("Conversation subject"));
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
                    id: "b".into(),
                    branch: BranchRequest::Existing { branch: "c".into() },
                },
            ),
            (
                "RemoveWorkspace",
                Request::RemoveWorkspace {
                    project: "a".into(),
                    name: "b".into(),
                    force: false,
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
                    id: "b".into(),
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
                    id: "b".into(),
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
                "UnarchiveSession",
                Request::UnarchiveSession {
                    session: SessionId(1),
                    expected_run: SessionRunId(4),
                },
            ),
            (
                "DeleteArchivedSession",
                Request::DeleteArchivedSession {
                    session: SessionId(1),
                    expected_run: SessionRunId(4),
                },
            ),
            (
                "RecoverSession",
                Request::RecoverSession {
                    session: SessionId(1),
                    expected_run: SessionRunId(4),
                },
            ),
            (
                "Keystroke",
                Request::Keystroke {
                    session: SessionId(1),
                    expected_run: SessionRunId(4),
                    key: ":j:".into(),
                },
            ),
            (
                "SetSetting",
                Request::SetSetting {
                    path: "picker_roots[2]".into(),
                    value: Some("\"~/x\"".into()),
                },
            ),
            (
                "AcknowledgeSessionStopped",
                Request::AcknowledgeSessionStopped {
                    session: SessionId(1),
                    expected_run: SessionRunId(4),
                },
            ),
            (
                "RefreshQuota",
                Request::RefreshQuota {
                    provider: Some(crate::QuotaProvider::Grok),
                },
            ),
            (
                "RefreshQuotaCursor",
                Request::RefreshQuota {
                    provider: Some(crate::QuotaProvider::Cursor),
                },
            ),
            ("Events", Request::Events { follow: true }),
            (
                "NavigateNotification",
                Request::NavigateNotification {
                    ticket: navigation_ticket(),
                },
            ),
            (
                "ConfirmNotificationNavigation",
                Request::ConfirmNotificationNavigation {
                    navigation: "n".into(),
                },
            ),
            (
                "NotificationNavigationApplied",
                Request::NotificationNavigationApplied {
                    navigation: "n".into(),
                },
            ),
            (
                "DashboardBridgeIdentity",
                Request::DashboardBridgeIdentity {
                    iterm_session_id: Some("i".into()),
                },
            ),
            ("PrepareITermFocus", Request::PrepareITermFocus),
            (
                "BridgeOwner",
                Request::BridgeOwner(crate::BridgeOwnerCall {
                    owner: owner_ticket(),
                    outcome: Some(crate::ITermFocusStatus::Denied),
                }),
            ),
            (
                "SaveWorkspaceWip",
                Request::SaveWorkspaceWip {
                    project: "a".into(),
                    name: "b".into(),
                },
            ),
            (
                "AnswerWipSave",
                Request::AnswerWipSave {
                    project: "a".into(),
                    workspace: "b".into(),
                    save: true,
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
            (
                "QuotaCooldown",
                Response::QuotaCooldown { remaining_ms: 300 },
            ),
            (
                "Events",
                Response::Events(vec![crate::Event {
                    time_unix_ms: 1,
                    component: crate::EventComponent::Titles,
                    subject: Some("7".into()),
                    message: "title applied".into(),
                }]),
            ),
            (
                "NotificationNavigation",
                Response::NotificationNavigation(crate::BridgeNavigationResult::ignored()),
            ),
            (
                "NotificationNavigationConfirmed",
                Response::NotificationNavigationConfirmed(Some(Box::new(navigation_offer()))),
            ),
            (
                "ITermFocusPrepared",
                Response::ITermFocusPrepared(Some(activation_target())),
            ),
            (
                "BridgeOwner",
                Response::BridgeOwner(crate::BridgeOwnerResult {
                    schema: crate::BRIDGE_SCHEMA_VERSION,
                    server_wire: crate::PROTOCOL_VERSION,
                    target: Some(activation_target()),
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
            ("QuotaChanged", ServerEvent::QuotaChanged(Box::default())),
            (
                "SettingsChanged",
                ServerEvent::SettingsChanged(Box::new(settings_report())),
            ),
            (
                "Recorded",
                ServerEvent::Recorded(crate::Event {
                    time_unix_ms: 1,
                    component: crate::EventComponent::Quota,
                    subject: Some("Codex".into()),
                    message: "Checking -> Unavailable: HTTP 503".into(),
                }),
            ),
            (
                "BridgeContext",
                ServerEvent::BridgeContext(crate::BridgeContext {
                    server_socket: "s".into(),
                    callback_executable: "c".into(),
                    callback_executable_sha256: "h".into(),
                    server_lifetime: "l".into(),
                }),
            ),
            (
                "NotificationNavigation",
                ServerEvent::NotificationNavigation(navigation_offer()),
            ),
            (
                "ITermFocus",
                ServerEvent::ITermFocus(crate::ITermFocusStatus::Denied),
            ),
            (
                "WipSavePrompt",
                ServerEvent::WipSavePrompt {
                    project: "a".into(),
                    workspace: "b".into(),
                    branch: "feature/topic".into(),
                },
            ),
            (
                "LifecycleCompleted",
                ServerEvent::LifecycleCompleted {
                    client_token: 7,
                    op: LifecycleOp::CreateSession,
                    outcome: LifecycleOutcome::Succeeded,
                },
            ),
        ]
    }

    fn owner_ticket() -> crate::BridgeOwnerTicket {
        crate::BridgeOwnerTicket {
            schema: crate::BRIDGE_SCHEMA_VERSION,
            server_wire: crate::PROTOCOL_VERSION,
            context: crate::BridgeContext {
                server_socket: "s".into(),
                callback_executable: "c".into(),
                callback_executable_sha256: "h".into(),
                server_lifetime: "l".into(),
            },
            dashboard_owner: "d".into(),
        }
    }
    fn activation_target() -> crate::BridgeActivationTarget {
        crate::BridgeActivationTarget {
            dashboard_pid: 7,
            dashboard_start_seconds: 8,
            dashboard_start_microseconds: 9,
            iterm_session_id: Some("i".into()),
            iterm_focus: true,
            owner: Some(Box::new(owner_ticket())),
        }
    }

    fn navigation_ticket() -> crate::BridgeNavigationTicket {
        crate::BridgeNavigationTicket {
            schema: crate::BRIDGE_SCHEMA_VERSION,
            server_wire: crate::PROTOCOL_VERSION,
            server_socket: "s".into(),
            callback_executable: "c".into(),
            callback_executable_sha256: "h".into(),
            server_lifetime: "l".into(),
            session: SessionId(1),
            run: SessionRunId(4),
        }
    }

    fn navigation_offer() -> crate::BridgeNavigationOffer {
        crate::BridgeNavigationOffer {
            navigation: "n".into(),
            ticket: navigation_ticket(),
        }
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
        for state in [
            crate::QuotaState::Waiting,
            crate::QuotaState::Current,
            crate::QuotaState::Unavailable,
            crate::QuotaState::NotSignedIn,
            crate::QuotaState::Unsupported,
            crate::QuotaState::Invalid,
            crate::QuotaState::SourceConflict,
            crate::QuotaState::Disabled,
            crate::QuotaState::Checking,
        ] {
            all.push((format!("QuotaState::{state:?}"), encode(&state)));
        }
        let window = crate::QuotaWindow {
            id: "native/primary".into(),
            label: "5h".into(),
            general: true,
            used_basis_points: Some(4200),
            over_limit: false,
            resets_unix_ms: Some(1000000),
        };
        all.push(("QuotaWindow".into(), encode(&window)));
        all.push((
            "QuotaSource::NativeProfile".into(),
            encode(&crate::QuotaSource::NativeProfile {
                profile: "native".into(),
                generation: 2,
            }),
        ));
        all.push((
            "QuotaSource::Probe".into(),
            encode(&crate::QuotaSource::Probe {
                probed_unix_ms: 1_000,
            }),
        ));
        all.push((
            "QuotaSource::Session".into(),
            encode(&crate::QuotaSource::Session {
                session: SessionId(1),
                run: SessionRunId(4),
                binding: crate::AgentBinding {
                    provider: crate::AgentProvider::Claude,
                    invocation: "inv".into(),
                    conversation: "conv".into(),
                    generation: 2,
                },
            }),
        ));
        all.push((
            "ProviderQuota".into(),
            encode(&crate::ProviderQuota {
                reason: Some("HTTP 503".into()),
                next_check_unix_ms: Some(60_000),
                ..crate::ProviderQuota::unknown(
                    crate::QuotaProvider::Codex,
                    crate::QuotaState::Unavailable,
                )
            }),
        ));
        all.push((
            "AgentObservation::Quota".into(),
            encode(&crate::AgentObservation::Quota(Box::new(
                crate::QuotaReport {
                    windows: Some(vec![window]),
                    state: crate::QuotaState::Current,
                },
            ))),
        ));
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
                crate::AgentProvider::Cursor,
            ]
            .iter()
            .map(|provider| (format!("AgentProvider::{provider:?}"), encode(provider))),
        );
        all.extend(
            [
                crate::ITermFocusStatus::Authorized,
                crate::ITermFocusStatus::SelectionRequested,
                crate::ITermFocusStatus::AuthorizationRequired,
                crate::ITermFocusStatus::Denied,
                crate::ITermFocusStatus::Unavailable,
            ]
            .iter()
            .map(|status| (format!("ITermFocusStatus::{status:?}"), encode(status))),
        );
        let extension = crate::ExtensionConversation {
            conversation: "native".into(),
            executable: "/bin/provider".into(),
            history: Some("/history.jsonl".into()),
            config_dir: "/config".into(),
            options: vec![],
        };
        all.push((
            "ConversationReference::Pi".into(),
            encode(&crate::ConversationReference::Pi(extension.clone())),
        ));
        all.push((
            "ConversationReference::Omp".into(),
            encode(&crate::ConversationReference::Omp(extension)),
        ));
        all.push((
            "ConversationReference::Codex".into(),
            encode(&crate::ConversationReference::Codex(
                crate::CodexConversation {
                    conversation: "native".into(),
                    executable: "/bin/provider".into(),
                    history: Some("/history.jsonl".into()),
                    config_dir: "/config".into(),
                    options: vec![],
                },
            )),
        ));
        all.push((
            "ConversationReference::Grok".into(),
            encode(&crate::ConversationReference::Grok(
                crate::GrokConversation {
                    conversation: "native".into(),
                    history: "/history.jsonl".into(),
                },
            )),
        ));
        all.push((
            "ConversationReference::Hermes".into(),
            encode(&crate::ConversationReference::Hermes(
                crate::HermesConversation {
                    conversation: "20261006_101500_ab12cd".into(),
                    executable: "/bin/hermes".into(),
                    state_db: "/home/state.db".into(),
                },
            )),
        ));
        all.push((
            "WorkspaceRecord".into(),
            encode(&crate::WorkspaceRecord {
                id: "stable".into(),
                path: "/work".into(),
                branch: "feature/topic".into(),
                git_identity: Some("1:2".into()),
                setup_pending: false,
            }),
        ));
        all.push((
            "WorkspaceSummary".into(),
            encode(&WorkspaceSummary {
                project: "p".into(),
                id: "stable".into(),
                name: "feature/topic".into(),
                path: "/work".into(),
                root: true,
                warning: Some("expected main".into()),
                sessions: Vec::new(),
            }),
        ));
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
        ("Request::RemoveWorkspace", "060161016200"),
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
        ("Request::AgentReport", "1501090909090909090909090909090909090909090909090909090909090909090901010002"),
        ("Request::HistoryBegin", "1601"),
        ("Request::HistoryPage", "17010203040506"),
        ("Request::HistoryEnd", "180102"),
        ("Request::SetView", "190301010401020101"),
        ("Request::CreateWorkspaceWithLaunch", "1f016101620101630101000273680000"),
        ("Request::CreateWorkspaceWithoutLaunch", "1f0161016201016300"),
        ("Request::SetSessionTitle", "200101057469746c65"),
        ("Request::ResetSessionTitle", "200100"),
        ("Request::ReopenSession", "21010400"),
        ("Request::UnarchiveSession", "230104"),
        ("Request::DeleteArchivedSession", "220104"),
        ("Request::RecoverSession", "250104"),
        ("Request::Keystroke", "260104033a6a3a"),
        ("Request::SetSetting", "270f7069636b65725f726f6f74735b325d0105227e2f7822"),
        ("Request::AcknowledgeSessionStopped", "240104"),
        ("Request::RefreshQuota", "280102"),
        ("Request::RefreshQuotaCursor", "280103"),
        ("Request::Events", "2901"),
        ("Request::NavigateNotification", "2a0429017301630168016c0104"),
        ("Request::ConfirmNotificationNavigation", "2b016e"),
        ("Request::NotificationNavigationApplied", "2c016e"),
        ("Request::DashboardBridgeIdentity", "2d010169"),
        ("Request::PrepareITermFocus", "2e"),
        ("Request::BridgeOwner", "2f0429017301630168016c01640103"),
        ("Request::SaveWorkspaceWip", "3001610162"),
        ("Request::AnswerWipSave", "310161016201"),
        ("Request::MarkReviewed", "1e010103696e7604636f6e760101047475726e02"),
        ("Response::Ok", "00"),
        ("Response::Hierarchy", "0100"),
        ("Response::CreatedSession", "020100052f776f726b04000001700177016e016c010201030000010103696e7604636f6e760101020101047475726e000000020000010c617070726f76616c3a726571050300010103696e7604636f6e760101047475726e020100000001040105060000"),
        ("Response::Screen", "0301040301020107"),
        ("Response::Error", "0401016d"),
        ("Response::Inventory", "0500010100052f776f726b04000001700177016e016c010201030000010103696e7604636f6e760101020101047475726e000000020000010c617070726f76616c3a726571050300010103696e7604636f6e760101047475726e020100000001040105060000"),
        ("Response::TerminalText", "060101020174"),
        ("Response::Task", "070601"),
        ("Response::HistoryOpened", "0801020301020405"),
        ("Response::HistoryRows", "090102030401010101780100020102030000"),
        ("Response::QuotaCooldown", "0bfb2c01"),
        ("Response::Events", "0c0101000101370d7469746c65206170706c696564"),
        ("Response::NotificationNavigation", "0d04290000"),
        ("Response::NotificationNavigationConfirmed", "0e01016e0429017301630168016c0104"),
        ("Response::ITermFocusPrepared", "0f0107080901016901010429017301630168016c0164"),
        ("Response::BridgeOwner", "1004290107080901016901010429017301630168016c0164"),
        ("ServerEvent::HierarchyChanged", "0000"),
        ("ServerEvent::Output", "010104030107"),
        ("ServerEvent::ScreenDirty", "02010403"),
        ("ServerEvent::SessionChanged", "030100052f776f726b04000001700177016e016c010201030000010103696e7604636f6e760101020101047475726e000000020000010c617070726f76616c3a726571050300010103696e7604636f6e760101047475726e020100000001040105060000"),
        ("ServerEvent::QuotaChanged", "04000000000008013577616974696e6720666f722061206d616e6167656420436c617564652073657373696f6e277320666972737420726573706f6e7365000100000000070142436f6465782f47726f6b207573616765206f66663a20736574206071756f74612e656e61626c6564203d20747275656020696e2064617368626f6172642e746f6d6c000200000000070142436f6465782f47726f6b207573616765206f66663a20736574206071756f74612e656e61626c6564203d20747275656020696e2064617368626f6172642e746f6d6c000300000000070147437572736f72207573616765206f66663a20736574206071756f74612e637572736f722e64617368626f617264203d20747275656020696e2064617368626f6172642e746f6d6c00"),
        ("ServerEvent::SettingsChanged", "05072f642e746f6d6c05000001000002010470692f6d08666561747572652f01022f63010161010162020170010161017100000005636f64657801022f680467726f6b00000000010d71756f74612e656e61626c656400010566616c736500010566616c736501036f66660101016b016d010300"),
        ("ServerEvent::Recorded", "0601020105436f64657821436865636b696e67202d3e20556e617661696c61626c653a204854545020353033"),
        ("ServerEvent::BridgeContext", "07017301630168016c"),
        ("ServerEvent::NotificationNavigation", "08016e0429017301630168016c0104"),
        ("ServerEvent::ITermFocus", "0903"),
        ("ServerEvent::WipSavePrompt", "0a016101620d666561747572652f746f706963"),
        ("ServerEvent::LifecycleCompleted", "0b070200"),
        ("QuotaState::Waiting", "00"),
        ("QuotaState::Current", "01"),
        ("QuotaState::Unavailable", "02"),
        ("QuotaState::NotSignedIn", "03"),
        ("QuotaState::Unsupported", "04"),
        ("QuotaState::Invalid", "05"),
        ("QuotaState::SourceConflict", "06"),
        ("QuotaState::Disabled", "07"),
        ("QuotaState::Checking", "08"),
        ("QuotaWindow", "0e6e61746976652f7072696d6172790235680101fb68100001fc40420f00"),
        ("QuotaSource::NativeProfile", "01066e617469766502"),
        ("QuotaSource::Probe", "02fbe803"),
        ("QuotaSource::Session", "0001040003696e7604636f6e7602"),
        ("ProviderQuota", "0100000000020108485454502035303301fb60ea"),
        ("AgentObservation::Quota", "0401010e6e61746976652f7072696d6172790235680101fb68100001fc40420f0001"),
        ("TaskRequest::ListTasks", "00"),
        ("TaskRequest::GetTask", "0301"),
        ("TaskRequest::Cancel", "0e01"),
        ("TaskRequest::Clean", "0f0101"),
        ("AgentRequest::0", "1a01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a507726573657276650003696e7600"),
        ("AgentRequest::1", "1d01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5"),
        ("AgentRequest::2", "1c01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70"),
        ("AgentRequest::3", "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70000004636f6e76"),
        ("AgentRequest::4", "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70010003696e7604636f6e76010300010a01640007666978747572650000010a01050103010201010766697874757265010001010766697874757265"),
        ("AgentRequest::5", "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f7002010003696e7604636f6e7601"),
        ("AgentRequest::6", "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70030003696e7604636f6e7601010201010e636f6c6c6563746f725f6c6f7374"),
        ("AgentRequest::7", "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70040003696e7604636f6e76010004636f6e760b2f62696e2f636c61756465132f686973746f72792f636f6e762e6a736f6e6c0e2f636f6e6669672f636c6175646500"),
        ("AgentRequest::8", "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f7005"),
        ("AgentRequest::9", "1501a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a500020003696e7604636f6e76010100020101047475726e"),
        ("AgentRequest::10", "1501a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a500020003696e7604636f6e7601010100010a01640007666978747572650000010a01050103010201010766697874757265010001010766697874757265"),
        ("AgentRequest::11", "1501a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a500020003696e7604636f6e7601010201010e636f6c6c6563746f725f6c6f7374"),
        ("AgentRequest::12", "1501a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a500020003696e7604636f6e76010103010c7175657374696f6e3a72657100"),
        ("AgentRequest::13", "1501a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a500020003696e7604636f6e7601010300"),
        ("AgentResponse::0", "0a0001a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5"),
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
        ("AgentProvider::Cursor", "06"),
        ("ITermFocusStatus::Authorized", "00"),
        ("ITermFocusStatus::SelectionRequested", "01"),
        ("ITermFocusStatus::AuthorizationRequired", "02"),
        ("ITermFocusStatus::Denied", "03"),
        ("ITermFocusStatus::Unavailable", "04"),
        ("ConversationReference::Pi", "01066e61746976650d2f62696e2f70726f7669646572010e2f686973746f72792e6a736f6e6c072f636f6e66696700"),
        ("ConversationReference::Omp", "02066e61746976650d2f62696e2f70726f7669646572010e2f686973746f72792e6a736f6e6c072f636f6e66696700"),
        ("ConversationReference::Codex", "03066e61746976650d2f62696e2f70726f7669646572010e2f686973746f72792e6a736f6e6c072f636f6e66696700"),
        ("ConversationReference::Grok", "04066e61746976650e2f686973746f72792e6a736f6e6c"),
        ("ConversationReference::Hermes", "051632303236313030365f3130313530305f6162313263640b2f62696e2f6865726d65730e2f686f6d652f73746174652e6462"),
        ("WorkspaceRecord", "06737461626c65052f776f726b0d666561747572652f746f7069630103313a3200"),
        ("WorkspaceSummary", "017006737461626c650d666561747572652f746f706963052f776f726b01010d6578706563746564206d61696e00"),
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

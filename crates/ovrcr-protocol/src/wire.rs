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
    Delegation(Box<crate::DelegationRequest>),
    Workflow(Box<crate::WorkflowRequest>),
    DashboardWorkflow(crate::DashboardWorkflowRequest),
    AssignmentControl(crate::AssignmentControlRequest),
    /// Human authority is captured when enqueued and checked at the final PTY fence.
    AssignedHumanInput {
        session: SessionId,
        run: SessionRunId,
        control_epoch: u64,
        view_revision: u64,
        bytes: Vec<u8>,
    },
    CancelCloudCreation {
        session: SessionId,
        expected_run: SessionRunId,
    },
    DiscardCloudCreation {
        session: SessionId,
        expected_run: SessionRunId,
    },
    Operator(crate::OperatorRequest),
    OperatorExchange(crate::OperatorExchange),
    OperatorWorker(crate::OperatorWorkerRequest),
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
    Delegation(Box<crate::AssignmentSnapshot>),
    Workflows(Vec<crate::WorkflowSnapshot>),
    WorkflowCandidates(Box<crate::WorkflowCandidates>),
    DelegationObservation(Box<crate::DelegationObservationBatch>),
    DelegationCapabilities(Box<crate::DelegationCapabilities>),
    WorkflowParents(Vec<crate::WorkflowParent>),
    WorkflowRecovery(Box<crate::WorkflowRecovery>),
    AssignedRead(Box<crate::AssignedReadResult>),
    Operator(crate::OperatorResponse),
    OperatorExchange(crate::OperatorExchangeResult),
    OperatorWorker(crate::OperatorWorkerResult),
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
    OperatorChanged(crate::OperatorConversationId),
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
            cursor_cloud_accounts: vec![crate::CursorCloudAccount {
                name: "work".into(),
                keychain_service: "ovrcr".into(),
                keychain_account: "work".into(),
                model: "composer-2.5".into(),
                repositories: vec!["https://github.com/acme/app".into()],
                owners: vec![],
                bridge_archive: "/sdk.tar.gz".into(),
            }],
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
            scope: crate::SessionScope::Workspace,
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
                native_control: None,
                native_control_revision: 0,
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
            assignment_control: None,
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
                "CancelCloudCreation",
                Request::CancelCloudCreation {
                    session: SessionId(1),
                    expected_run: SessionRunId(4),
                },
            ),
            (
                "DiscardCloudCreation",
                Request::DiscardCloudCreation {
                    session: SessionId(1),
                    expected_run: SessionRunId(4),
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
                crate::AgentProvider::CursorCloud,
                crate::AgentProvider::Operator,
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
        let auth = crate::DelegationAuth {
            controller: None,
            source: None,
            session: SessionId(1),
            capability: "fixture".into(),
        };
        let identity = crate::AssignmentIdentity {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            controller_epoch: 1,
            task: "t".into(),
            attempt: 1,
            contract: 1,
            request: "r".into(),
            session: Some(SessionId(2)),
            run: Some(crate::SessionRunId(1)),
        };
        let contract = crate::ResearchContract {
            review: None,
            coding: None,
            schema: 1,
            workflow: "w".into(),
            controller: "c".into(),
            task: "t".into(),
            request: "r".into(),
            project: "p".into(),
            workspace: "ws".into(),
            scope: "read".into(),
            permitted_actions: vec!["read".into()],
            provider: "pi".into(),
            model_provider: "test".into(),
            model: "m".into(),
            acceptance_evidence: vec!["cite".into()],
            mandatory_capabilities: vec!["read".into()],
            limits: crate::DelegationLimits {
                max_live_children: Some(6),
                max_execution_attempts: Some(2),
                min_free_slots_after_launch: 1,
                ..Default::default()
            },
            previous_request: Some("prev".into()),
        };
        let submission = crate::ResearchSubmission {
            review: None,
            coding: None,
            schema: 1,
            identity: identity.clone(),
            version: 1,
            outcome: "success".into(),
            summary: "done".into(),
            evidence: vec!["citation".into()],
            unresolved: vec![],
        };
        for (name, request) in [
            (
                "Prepare",
                crate::DelegationRequest::Prepare {
                    auth: auth.clone(),
                    contract: contract.clone(),
                },
            ),
            (
                "Launch",
                crate::DelegationRequest::Launch {
                    auth: auth.clone(),
                    handle: "r".into(),
                    identity: identity.clone(),
                },
            ),
            (
                "SendInitial",
                crate::DelegationRequest::SendInitial {
                    auth: auth.clone(),
                    handle: "r".into(),
                    identity: identity.clone(),
                    expected_control_epoch: 1,
                },
            ),
            (
                "Inspect",
                crate::DelegationRequest::Inspect {
                    auth: auth.clone(),
                    handle: "r".into(),
                },
            ),
            (
                "Collect",
                crate::DelegationRequest::Collect {
                    auth: auth.clone(),
                    handle: "r".into(),
                },
            ),
            (
                "Reconcile",
                crate::DelegationRequest::Reconcile {
                    auth: auth.clone(),
                    request: "r".into(),
                },
            ),
            (
                "Followup",
                crate::DelegationRequest::Followup {
                    auth: auth.clone(),
                    handle: "r".into(),
                    request: "f".into(),
                    expected_identity: identity.clone(),
                    expected_result_version: 1,
                    expected_control_epoch: 1,
                    text: "clarify".into(),
                    revised_contract: Some(Box::new(contract)),
                },
            ),
            (
                "CollectVersion",
                crate::DelegationRequest::CollectVersion {
                    auth: auth.clone(),
                    handle: "r".into(),
                    version: 1,
                },
            ),
            (
                "RecordAcceptance",
                crate::DelegationRequest::RecordAcceptance {
                    auth: auth.clone(),
                    handle: "r".into(),
                    identity: identity.clone(),
                    result_version: 1,
                    target: crate::AcceptanceTarget::ChildSubmission {
                        digest: "digest".into(),
                    },
                    expected_control_epoch: 1,
                },
            ),
            (
                "Publish",
                crate::DelegationRequest::Publish {
                    auth: auth.clone(),
                    handle: "r".into(),
                    submission: submission.clone(),
                },
            ),
            (
                "Observe",
                crate::DelegationRequest::Observe {
                    auth: auth.clone(),
                    handles: vec!["r".into()],
                    after: Some(crate::ObservationCursor {
                        server_epoch: "epoch".into(),
                        sequence: 3,
                    }),
                    wait_ms: 1000,
                },
            ),
            (
                "MarkHandled",
                crate::DelegationRequest::MarkHandled {
                    auth: auth.clone(),
                    handle: "r".into(),
                    publication: crate::PublicationIdentity {
                        assignment: identity.clone(),
                        kind: crate::PublicationKind::Submission,
                        version: 1,
                        digest: "digest".into(),
                    },
                },
            ),
        ] {
            all.push((
                format!("Delegation::{name}"),
                encode(&Request::Delegation(Box::new(request))),
            ));
        }
        let evidence = crate::AssignmentControlEvidence {
            server_lifetime: "l".into(),
            identity: identity.clone(),
            binding: crate::AgentBinding {
                provider: crate::AgentProvider::Pi,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
            result_version: 1,
            activity_revision: 2,
            input_revision: 3,
            publication: None,
            publication_issue: None,
            human_input_revision: 4,
        };
        all.push((
            "Request::AssignedHumanInput".into(),
            encode(&Request::AssignedHumanInput {
                session: SessionId(2),
                run: SessionRunId(1),
                control_epoch: 2,
                view_revision: 3,
                bytes: vec![7],
            }),
        ));
        all.push((
            "Request::AssignmentTakeover".into(),
            encode(&Request::AssignmentControl(
                crate::AssignmentControlRequest::Takeover {
                    request: "take".into(),
                    session: SessionId(2),
                    run: SessionRunId(1),
                    expected_epoch: 1,
                    view_revision: 3,
                },
            )),
        ));
        all.push((
            "Request::AssignmentReturn".into(),
            encode(&Request::AssignmentControl(
                crate::AssignmentControlRequest::Return {
                    request: "return".into(),
                    session: SessionId(2),
                    run: SessionRunId(1),
                    expected_epoch: 2,
                    view_revision: 3,
                    evidence: Box::new(evidence.clone()),
                },
            )),
        ));
        all.push((
            "Delegation::ConfirmHumanReturn".into(),
            encode(&Request::Delegation(Box::new(
                crate::DelegationRequest::ConfirmHumanReturn {
                    auth: crate::DelegationAuth {
                        controller: None,
                        source: None,
                        session: SessionId(1),
                        capability: "fixture".into(),
                    },
                    handle: "r".into(),
                    request: "confirm".into(),
                    returned_request: "return".into(),
                    expected_control_epoch: 3,
                    evidence: Box::new(evidence),
                },
            ))),
        ));
        all.push((
            "Delegation::Capabilities".into(),
            encode(&Request::Delegation(Box::new(
                crate::DelegationRequest::Capabilities {
                    auth,
                    selection: crate::DelegationSelection {
                        provider: Some(crate::DelegationHarness::Pi),
                        model_provider: Some("test".into()),
                        model: Some("m".into()),
                        required_limits: crate::DelegationLimits::default(),
                    },
                },
            ))),
        ));
        all.push((
            "Response::DelegationCapabilities".into(),
            encode(&Response::DelegationCapabilities(Box::new(
                crate::DelegationCapabilities {
                    choices: vec![crate::DelegationChoice {
                        provider: crate::DelegationHarness::Pi,
                        model_provider: "test".into(),
                        model: "m".into(),
                    }],
                    resolved_choice: None,
                    effective_limits: crate::DelegationLimits {
                        max_live_children: Some(50),
                        ..Default::default()
                    },
                    live_sessions: 3,
                    live_children: 1,
                    available_slots: 47,
                    capabilities: vec!["read".into(), "result_file".into()],
                    enforceable_limits: vec!["max_live_children".into()],
                    native_helpers: false,
                    native_descendant_usage: false,
                    reason: None,
                },
            ))),
        ));
        all.push((
            "Response::Delegation".into(),
            encode(&Response::Delegation(Box::new(crate::AssignmentSnapshot {
                coding_target: None,
                dependency_gap: None,
                review: None,
                review_assignment: None,
                review_scope: None,
                questions: vec![],
                collected_question: None,
                obligation_revision: 1,
                archive_receipts: vec![],
                recoveries: vec![],
                fallback_capture: None,
                recovered: None,
                accepted_recovery: None,
                submission_issue: None,
                selected_acceptance_ref: None,
                selected_recoveries: vec![],
                handle: "r".into(),
                control: crate::AssignmentControl::default(),
                control_evidence: None,
                identity: identity.clone(),
                dispatch: crate::DispatchStatus::Applied,
                reason: None,
                activity: AgentActivity::ResponseReady,
                conversation: Some("conv".into()),
                brief: "/brief".into(),
                result_path: "/result".into(),
                submission: Some(submission),
                acceptance: "pending".into(),
                result_version: 1,
                publication: None,
                acceptance_ref: None,
                history: vec![],
                selected_version: None,
                historical_submission: None,
                receipts: vec![],
                resources: crate::AssignmentResources {
                    coding: None,
                    project: Some("p".into()),
                    workspace: Some("ws".into()),
                    workspace_present: true,
                    current_session: None,
                },
                retryable_capacity_refusal: false,
                successor_request: Some("next".into()),
                execution_attempts: Some(1),
            }))),
        ));
        all.push((
            "Response::DelegationObservation".into(),
            encode(&Response::DelegationObservation(Box::new(
                crate::DelegationObservationBatch {
                    cursor: crate::ObservationCursor {
                        server_epoch: "epoch".into(),
                        sequence: 3,
                    },
                    status: crate::ObservationStatus::ReconcileRequired,
                    progress_deadline_ms: 300000,
                    observations: vec![crate::DelegationObservation {
                        control_receipt_digest: String::new(),
                        native_control_revision: 0,
                        workflow_cancelled: false,
                        questions: vec![],
                        native_question_gap: None,
                        handle: "r".into(),
                        identity: identity.clone(),
                        binding: None,
                        activity: None,
                        activity_revision: 1,
                        input_requests: vec![],
                        input_revision: 2,
                        health: Some(crate::ReporterHealth::Connected),
                        current: true,
                        publication: Some(crate::PublicationState {
                            identity: crate::PublicationIdentity {
                                assignment: identity.clone(),
                                kind: crate::PublicationKind::Submission,
                                version: 1,
                                digest: "digest".into(),
                            },
                            handled: false,
                        }),
                        publication_issue: Some(crate::PublicationIssue::Invalid),
                        last_progress_unix_ms: 100,
                        stalled: false,
                        progress_unresolved: true,
                        control: crate::AssignmentControl::default(),
                    }],
                },
            ))),
        ));
        let auth = crate::DelegationAuth {
            session: SessionId(1),
            capability: "fixture".into(),
            controller: None,
            source: None,
        };
        let authority = crate::WorkflowAuthority {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            epoch: 1,
            parent: SessionId(1),
            parent_run: crate::SessionRunId(1),
            binding: crate::AgentBinding {
                provider: crate::AgentProvider::Pi,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
            producer: "producer".into(),
            server_lifetime: "server".into(),
        };
        let index = crate::ContinuationIndex {
            schema: 1,
            objective: "objective.md".into(),
            decisions: vec![],
            unresolved_assignments: vec!["r".into()],
            questions: vec![],
            receipts: vec![],
            accepted_evidence: vec![],
            next_actions: vec![],
        };
        let transfer = crate::WorkflowTransfer {
            request: "transfer".into(),
            expected: authority.clone(),
            parent: SessionId(3),
            parent_run: crate::SessionRunId(2),
            binding: authority.binding.clone(),
            producer: "successor".into(),
        };
        for (name, request) in [
            (
                "SetDependencies",
                crate::WorkflowRequest::SetDependencies {
                    auth: auth.clone(),
                    workflow: "w".into(),
                    expected_version: 0,
                    controller: "c".into(),
                    graph: crate::DependencyGraph {
                        tasks: vec![crate::DependencyTask {
                            task: "t".into(),
                            canonical_task: "task.md".into(),
                            request: "r".into(),
                            project: "p".into(),
                            workspace: "ws".into(),
                            revision: 1,
                            integrate_snapshots: false,
                        }],
                        edges: vec![],
                    },
                },
            ),
            (
                "Parents",
                crate::WorkflowRequest::Parents { auth: auth.clone() },
            ),
            (
                "Recover",
                crate::WorkflowRequest::Recover {
                    auth: auth.clone(),
                    workflow: Some("w".into()),
                },
            ),
            (
                "Save",
                crate::WorkflowRequest::Save {
                    auth: auth.clone(),
                    workflow: "w".into(),
                    expected_version: 0,
                    index: index.clone(),
                },
            ),
            (
                "Transfer",
                crate::WorkflowRequest::Transfer {
                    auth,
                    transfer: transfer.clone(),
                },
            ),
        ] {
            all.push((
                format!("Workflow::{name}"),
                encode(&Request::Workflow(Box::new(request))),
            ));
        }
        all.push((
            "Request::DashboardWorkflowCandidates".into(),
            encode(&Request::DashboardWorkflow(
                crate::DashboardWorkflowRequest::Candidates {
                    parent: SessionId(1),
                    run: crate::SessionRunId(1),
                },
            )),
        ));
        all.push((
            "Request::DashboardWorkflowTransfer".into(),
            encode(&Request::DashboardWorkflow(
                crate::DashboardWorkflowRequest::Transfer(Box::new(transfer.clone())),
            )),
        ));
        let snapshot = crate::WorkflowSnapshot {
            dependencies: None,
            deliveries: vec![],
            cancellation: None,
            authority: authority.clone(),
            active: true,
            handles: vec!["r".into()],
            pointer: Some(crate::ContinuationPointer {
                schema: 1,
                instance: "i".into(),
                workflow: "w".into(),
                version: 1,
                digest: "digest".into(),
            }),
            continuation: Some(index),
            gaps: vec![],
            receipt: None,
        };
        let gaps = vec![
            crate::WorkflowRecoveryGap {
                workflow: Some("w".into()),
                handle: Some("r".into()),
                identity: Some(identity),
                brief: Some("/brief".into()),
                result_path: Some("/result".into()),
                reason: "missing authority".into(),
            },
            crate::WorkflowRecoveryGap {
                workflow: None,
                handle: None,
                identity: None,
                brief: None,
                result_path: None,
                reason: "unverifiable retained record".into(),
            },
        ];
        for (name, response) in [
            ("Workflows", Response::Workflows(vec![snapshot.clone()])),
            (
                "WorkflowParents",
                Response::WorkflowParents(vec![crate::WorkflowParent {
                    session: authority.parent,
                    run: authority.parent_run,
                    binding: authority.binding.clone(),
                    producer: authority.producer.clone(),
                }]),
            ),
            (
                "WorkflowRecovery",
                Response::WorkflowRecovery(Box::new(crate::WorkflowRecovery {
                    workflows: vec![snapshot],
                    gaps: gaps.clone(),
                })),
            ),
            (
                "WorkflowCandidates",
                Response::WorkflowCandidates(Box::new(crate::WorkflowCandidates {
                    candidates: vec![transfer],
                    gaps,
                })),
            ),
        ] {
            all.push((format!("Response::{name}"), encode(&response)));
        }
        cancellation_cases(&mut all);
        question_cases(&mut all);
        delivery_cases(&mut all);
        fallback_cases(&mut all);
        review_cases(&mut all);
        native_read_cases(&mut all);
        operator_cases(&mut all);
        all
    }

    fn operator_cases(all: &mut Vec<(String, String)>) {
        use crate::operator::*;
        let identity = OperatorTurnIdentity {
            conversation: OperatorConversationId(1),
            turn: OperatorTurnId(2),
            session: SessionId(3),
            run: SessionRunId(4),
        };
        let context = OperatorContext {
            project: Some("p".into()),
            workspace: Some("label".into()),
            workspace_id: Some("stable".into()),
            cwd: Some("/w".into()),
            session: Some(SessionId(5)),
            run: Some(SessionRunId(6)),
        };
        let turn = OperatorTurn {
            identity,
            state: OperatorTurnState::Running,
            sequence: 0,
        };
        let summary = OperatorConversationSummary {
            worker_stopped: false,
            id: identity.conversation,
            session: identity.session,
            provider: OperatorProvider::ChatGpt,
            model: "model".into(),
            title: "hello".into(),
            current_turn: Some(turn.clone()),
        };
        let history = OperatorModelHistory {
            version: 1,
            json: "[]".into(),
        };
        let auth = crate::SupervisorAuth {
            session: identity.session,
            lease: crate::AgentSecret([7; 32]),
        };
        let result = OperatorExchangeResult {
            identity,
            sequence: 1,
            duplicate: false,
        };
        for (name, request) in [
            (
                "Create",
                OperatorRequest::Create {
                    provider: OperatorProvider::Grok,
                    model: "model".into(),
                },
            ),
            (
                "List",
                OperatorRequest::List {
                    after: 1,
                    limit: 10,
                },
            ),
            (
                "Read",
                OperatorRequest::Read {
                    conversation: identity.conversation,
                    after: 1,
                    limit: 10,
                },
            ),
            (
                "Submit",
                OperatorRequest::Submit {
                    conversation: identity.conversation,
                    expected_run: SessionRunId(0),
                    text: "hello".into(),
                    context: context.clone(),
                },
            ),
            ("Stop", OperatorRequest::Stop { identity }),
        ] {
            all.push((
                format!("Request::Operator::{name}"),
                encode(&Request::Operator(request)),
            ));
        }
        for (name, event) in [
            (
                "AssistantDelta",
                OperatorWorkerEvent::AssistantDelta("hello".into()),
            ),
            (
                "ToolCall",
                OperatorWorkerEvent::ToolCall {
                    id: "call".into(),
                    name: "bash".into(),
                    arguments: "{}".into(),
                },
            ),
            (
                "ToolOutput",
                OperatorWorkerEvent::ToolOutput {
                    id: "call".into(),
                    output: "output".into(),
                    truncated: true,
                    status: OperatorToolStatus::Succeeded,
                },
            ),
            ("Completed", OperatorWorkerEvent::Completed(history.clone())),
            ("Failed", OperatorWorkerEvent::Failed("error".into())),
        ] {
            all.push((
                format!("Request::OperatorExchange::{name}"),
                encode(&Request::OperatorExchange(OperatorExchange {
                    auth: auth.clone(),
                    batch: OperatorEventBatch {
                        identity,
                        sequence: 1,
                        events: vec![event],
                    },
                })),
            ));
        }
        for (name, command) in [
            ("Start", OperatorWorkerCommand::Start { identity }),
            ("Retire", OperatorWorkerCommand::Retire { identity }),
            ("Stop", OperatorWorkerCommand::Stop { identity }),
        ] {
            all.push((
                format!("Request::OperatorWorker::{name}"),
                encode(&Request::OperatorWorker(OperatorWorkerRequest {
                    auth: auth.clone(),
                    command,
                })),
            ));
        }
        for (name, response) in [
            ("Created", OperatorResponse::Created(summary.clone())),
            (
                "Conversations",
                OperatorResponse::Conversations(vec![summary.clone()]),
            ),
            (
                "History",
                OperatorResponse::History(OperatorHistory {
                    conversation: summary,
                    messages: vec![OperatorMessage {
                        ordinal: 1,
                        turn: identity.turn,
                        content: OperatorMessageContent::User {
                            text: "hello".into(),
                            context: context.clone(),
                        },
                    }],
                    next_after: Some(1),
                }),
            ),
            ("Submitted", OperatorResponse::Submitted(turn.clone())),
            (
                "Stopped",
                OperatorResponse::Stopped(OperatorTurn {
                    state: OperatorTurnState::Interrupted,
                    ..turn
                }),
            ),
        ] {
            all.push((
                format!("Response::Operator::{name}"),
                encode(&Response::Operator(response)),
            ));
        }
        all.push((
            "Response::OperatorExchange".into(),
            encode(&Response::OperatorExchange(result)),
        ));
        all.push((
            "Response::OperatorWorker::Start".into(),
            encode(&Response::OperatorWorker(OperatorWorkerResult::Start(
                Box::new(OperatorWorkerStart {
                    identity,
                    provider: OperatorProvider::ChatGpt,
                    model: "model".into(),
                    text: "hello".into(),
                    context,
                    history: Some(history),
                }),
            ))),
        ));
        all.push((
            "Response::OperatorWorker::Retired".into(),
            encode(&Response::OperatorWorker(OperatorWorkerResult::Retired {
                identity,
            })),
        ));
        all.push((
            "ServerEvent::OperatorChanged".into(),
            encode(&ServerEvent::OperatorChanged(identity.conversation)),
        ));
        all.push((
            "SessionScope::Workspace".into(),
            encode(&crate::SessionScope::Workspace),
        ));
        all.push((
            "SessionScope::OperatorConversation".into(),
            encode(&crate::SessionScope::OperatorConversation {
                conversation: identity.conversation,
            }),
        ));
        for (name, status) in [
            ("Succeeded", OperatorToolStatus::Succeeded),
            ("Failed", OperatorToolStatus::Failed { exit_code: Some(7) }),
            ("TimedOut", OperatorToolStatus::TimedOut),
        ] {
            all.push((format!("OperatorToolStatus::{name}"), encode(&status)));
        }
        for state in [
            OperatorTurnState::Running,
            OperatorTurnState::Complete,
            OperatorTurnState::Interrupted,
            OperatorTurnState::Error,
        ] {
            all.push((format!("OperatorTurnState::{state:?}"), encode(&state)));
        }
        all.push((
            "OperatorMessageContent::Worker".into(),
            encode(&OperatorMessageContent::Worker(
                OperatorWorkerEvent::AssistantDelta("a".into()),
            )),
        ));
        all.push((
            "OperatorMessageContent::Interrupted".into(),
            encode(&OperatorMessageContent::Interrupted),
        ));
    }

    fn fallback_cases(all: &mut Vec<(String, String)>) {
        let identity = crate::AssignmentIdentity {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            controller_epoch: 1,
            task: "t".into(),
            attempt: 1,
            contract: 1,
            request: "r".into(),
            session: Some(crate::SessionId(2)),
            run: Some(crate::SessionRunId(3)),
        };
        let auth = crate::DelegationAuth {
            session: crate::SessionId(1),
            capability: "fixture".into(),
            controller: None,
            source: None,
        };
        let binding = crate::AgentBinding {
            provider: crate::AgentProvider::Pi,
            invocation: "child".into(),
            conversation: "child-conv".into(),
            generation: 1,
        };
        let control = crate::AssignmentControl {
            epoch: 1,
            owner: crate::ControlOwner::Controller,
            barrier: None,
            human_input_revision: 0,
            attribution_invalidated: false,
        };
        let authority = crate::WorkflowAuthority {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            epoch: 1,
            parent: crate::SessionId(1),
            parent_run: crate::SessionRunId(1),
            binding: crate::AgentBinding {
                provider: crate::AgentProvider::Pi,
                invocation: "parent".into(),
                conversation: "parent-conv".into(),
                generation: 1,
            },
            producer: "producer".into(),
            server_lifetime: "server".into(),
        };
        let original = crate::FallbackOriginalCondition::Invalid {
            digest: "a".repeat(64),
        };
        let artifact = crate::FallbackFileReference {
            name: "artifact.json".into(),
            digest: "b".repeat(64),
        };
        let evidence_file = crate::FallbackFileReference {
            name: "independent.txt".into(),
            digest: "d".repeat(64),
        };
        let evidence = crate::FallbackEvidence {
            requirement: "Complete README".into(),
            file: evidence_file.clone(),
            excerpt: "README".into(),
            assessment: "Independent parent review".into(),
        };
        let capture = crate::FallbackCapture {
            schema: 1,
            id: "capture".into(),
            server_lifetime: "server".into(),
            authority: authority.clone(),
            context: crate::FallbackContext {
                identity: identity.clone(),
                result_version: 3,
                contract_digest: "c".repeat(64),
                brief_digest: "f".repeat(64),
                freshness_nonce: "nonce".into(),
                binding: binding.clone(),
                control: control.clone(),
            },
            evidence: crate::AssignmentControlEvidence {
                server_lifetime: "server".into(),
                identity: identity.clone(),
                binding,
                result_version: 3,
                activity_revision: 7,
                input_revision: 0,
                publication: None,
                publication_issue: Some(crate::ControlPublicationIssue::Invalid),
                human_input_revision: 0,
            },
            captured_unix_ms: 1,
            size: TerminalSize { rows: 24, cols: 80 },
            screen: format!("OVRCR_FALLBACK:{}", "b".repeat(64)),
            truncated: false,
            original: original.clone(),
            original_bytes: Some(b"{".to_vec()),
        };
        // Literal wire values pin shape; they do not claim filesystem content proof.
        let recovery = crate::FallbackRecovery {
            schema: 1,
            version: 2,
            author: authority,
            capture: capture.clone(),
            artifact: crate::FrozenFallbackFile {
                reference: artifact.clone(),
                bytes: b"artifact".to_vec(),
            },
            contract: crate::FrozenFallbackFile {
                reference: crate::FallbackFileReference {
                    name: "contract-v1.json".into(),
                    digest: "c".repeat(64),
                },
                bytes: b"contract".to_vec(),
            },
            brief: crate::FrozenFallbackFile {
                reference: crate::FallbackFileReference {
                    name: "brief-v3.json".into(),
                    digest: "f".repeat(64),
                },
                bytes: b"brief".to_vec(),
            },
            evidence: vec![evidence.clone()],
            evidence_files: vec![crate::FrozenFallbackFile {
                reference: evidence_file,
                bytes: b"README".to_vec(),
            }],
        };
        let accepted = crate::AcceptanceReference {
            identity: identity.clone(),
            result_version: 3,
            target: crate::AcceptanceTarget::ParentRecovery {
                recovery_version: 2,
                digest: "e".repeat(64),
            },
            path: "/acceptance-v3.json".into(),
            digest: "f".repeat(64),
        };
        let reference = crate::RecoveryReference {
            version: 2,
            digest: "e".repeat(64),
        };
        for (name, request) in [
            (
                "CaptureFallback",
                crate::DelegationRequest::CaptureFallback {
                    auth: auth.clone(),
                    handle: "r".into(),
                    expected_identity: identity.clone(),
                    expected_result_version: 3,
                    expected_control_epoch: 1,
                },
            ),
            (
                "RecoverFallback",
                crate::DelegationRequest::RecoverFallback {
                    auth: auth.clone(),
                    handle: "r".into(),
                    draft: Box::new(crate::FallbackRecoveryDraft {
                        capture_id: "capture".into(),
                        version: 2,
                        artifact,
                        original,
                        evidence: vec![evidence],
                    }),
                },
            ),
            (
                "RecordRecoveryAcceptance",
                crate::DelegationRequest::RecordAcceptance {
                    auth,
                    handle: "r".into(),
                    identity: identity.clone(),
                    result_version: 3,
                    target: crate::AcceptanceTarget::ParentRecovery {
                        recovery_version: 2,
                        digest: "e".repeat(64),
                    },
                    expected_control_epoch: 1,
                },
            ),
        ] {
            all.push((
                format!("Delegation::{name}"),
                encode(&Request::Delegation(Box::new(request))),
            ));
        }
        all.push((
            "Response::FallbackRecovery".into(),
            encode(&Response::Delegation(Box::new(crate::AssignmentSnapshot {
                coding_target: None,
                dependency_gap: None,
                handle: "r".into(),
                identity,
                dispatch: crate::DispatchStatus::Applied,
                reason: None,
                activity: AgentActivity::ResponseReady,
                conversation: Some("child-conv".into()),
                brief: "/brief-v3.json".into(),
                result_path: "/submission-v3.json".into(),
                submission: None,
                acceptance: "accepted".into(),
                result_version: 3,
                publication: None,
                acceptance_ref: Some(accepted.clone()),
                history: vec![],
                selected_version: Some(3),
                historical_submission: None,
                receipts: vec![],
                resources: crate::AssignmentResources {
                    project: Some("p".into()),
                    workspace: Some("ws".into()),
                    workspace_present: true,
                    current_session: None,
                    coding: None,
                },
                control,
                control_evidence: Some(capture.evidence.clone()),
                retryable_capacity_refusal: false,
                successor_request: None,
                execution_attempts: Some(1),
                questions: vec![],
                collected_question: None,
                obligation_revision: 4,
                archive_receipts: vec![],
                recoveries: vec![reference.clone()],
                fallback_capture: Some(capture),
                recovered: Some(crate::RecoveryPublication {
                    path: "/parent-fallback-r3-v2.json".into(),
                    digest: "e".repeat(64),
                    recovery: recovery.clone(),
                }),
                accepted_recovery: Some(recovery),
                submission_issue: Some("invalid child submission".into()),
                selected_acceptance_ref: Some(accepted),
                selected_recoveries: vec![reference],
                review: None,
                review_assignment: None,
                review_scope: None,
            }))),
        ));
    }

    fn delivery_cases(all: &mut Vec<(String, String)>) {
        let identity = crate::AssignmentIdentity {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            controller_epoch: 1,
            task: "t".into(),
            attempt: 1,
            contract: 1,
            request: "r".into(),
            session: Some(SessionId(2)),
            run: Some(crate::SessionRunId(1)),
        };
        let authority = crate::WorkflowAuthority {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            parent: SessionId(1),
            parent_run: crate::SessionRunId(1),
            binding: crate::AgentBinding {
                provider: crate::AgentProvider::Pi,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
            producer: "producer".into(),
            epoch: 1,
            server_lifetime: "server".into(),
        };
        let auth = crate::DelegationAuth {
            session: SessionId(1),
            capability: "fixture".into(),
            controller: None,
            source: None,
        };
        let declaration = crate::DeliveryDeclaration {
            request: "delivered".into(),
            workflow: "w".into(),
            scope: "README".into(),
            provenance: "parent decision".into(),
            resources: vec![crate::DeliveryResource {
                handle: "r".into(),
                identity: identity.clone(),
                result_version: 3,
                obligation_revision: 4,
                publication: None,
                acceptance: None,
                evidence: vec![crate::DeliveryEvidence {
                    kind: crate::DeliveryEvidenceKind::Review,
                    reference: "review-index".into(),
                    digest: "a".repeat(64),
                }],
                completed_obligations: vec!["review".into()],
                retention: crate::RetentionChoice::ArchiveEligible,
                previous: None,
            }],
        };
        declaration.validate().unwrap();
        let reference = crate::DeliveryRef {
            request: "delivered".into(),
            digest: "b".repeat(64),
        };
        all.push((
            "Request::WorkflowDeclareDelivery".into(),
            encode(&Request::Workflow(Box::new(
                crate::WorkflowRequest::DeclareDelivery {
                    auth: auth.clone(),
                    declaration: declaration.clone(),
                },
            ))),
        ));
        all.push((
            "RecordedDelivery".into(),
            encode(&crate::RecordedDelivery {
                reference: reference.clone(),
                authority: authority.clone(),
                declaration,
            }),
        ));
        let request = crate::ArchiveRequest {
            request: "archive".into(),
            handle: "r".into(),
            expected_identity: identity,
            expected_result_version: 3,
            expected_obligation_revision: 4,
            expected_control_epoch: 2,
            delivery: reference,
            continuation_of: Some("original".into()),
        };
        request.validate().unwrap();
        all.push((
            "Delegation::Archive".into(),
            encode(&Request::Delegation(Box::new(
                crate::DelegationRequest::Archive {
                    auth,
                    archive: request.clone(),
                },
            ))),
        ));
        all.push((
            "ControlBarrier::Archive".into(),
            encode(&crate::ControlBarrier::Archive {
                request: "archive".into(),
            }),
        ));
        for outcome in [
            crate::ArchiveOutcome::StopUnknown,
            crate::ArchiveOutcome::StoppedArchivePending,
            crate::ArchiveOutcome::Archived {
                previous_run: crate::SessionRunId(1),
                archived_run: crate::SessionRunId(2),
            },
        ] {
            let label = format!("ArchiveReceipt::{outcome:?}");
            all.push((
                label,
                encode(&crate::ArchiveReceipt {
                    intent: crate::ArchiveIntent {
                        request: request.clone(),
                        digest: "c".repeat(64),
                        authority: authority.clone(),
                        control_epoch_admitted: Some(2),
                        child_binding: Some(authority.binding.clone()),
                    },
                    outcome,
                }),
            ));
        }
        for reason in [
            crate::ArchiveRefusal::RequiredRetention,
            crate::ArchiveRefusal::DeliverySuperseded,
            crate::ArchiveRefusal::ObligationChanged,
            crate::ArchiveRefusal::StaleTarget,
            crate::ArchiveRefusal::HumanOwned,
            crate::ArchiveRefusal::OwnershipUncertain,
            crate::ArchiveRefusal::UnresolvedControl,
            crate::ArchiveRefusal::AttributionChanged,
            crate::ArchiveRefusal::Busy,
            crate::ArchiveRefusal::NoOwnedSession,
            crate::ArchiveRefusal::Capacity,
        ] {
            all.push((
                format!("ArchiveRefusal::{reason:?}"),
                encode(&crate::ArchiveOutcome::Refused { reason }),
            ));
        }
        all.push((
            "RetentionChoice::Keep".into(),
            encode(&crate::RetentionChoice::Keep {
                reason: "review pending".into(),
            }),
        ));
        for kind in [
            crate::DeliveryEvidenceKind::Recovery,
            crate::DeliveryEvidenceKind::Review,
            crate::DeliveryEvidenceKind::Check,
            crate::DeliveryEvidenceKind::Continuation,
            crate::DeliveryEvidenceKind::OtherAuthorizedEvidence,
        ] {
            all.push((format!("DeliveryEvidenceKind::{kind:?}"), encode(&kind)));
        }
    }

    fn question_cases(all: &mut Vec<(String, String)>) {
        let identity = crate::AssignmentIdentity {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            controller_epoch: 1,
            task: "t".into(),
            attempt: 1,
            contract: 1,
            request: "r".into(),
            session: Some(SessionId(2)),
            run: Some(crate::SessionRunId(1)),
        };
        let previous = crate::QuestionReference {
            assignment: identity.clone(),
            result_version: 3,
            question_id: "section".into(),
            question_version: 1,
            digest: "a".repeat(64),
        };
        let reference = crate::QuestionReference {
            question_version: 2,
            digest: "b".repeat(64),
            ..previous.clone()
        };
        let question = crate::AssignmentQuestion {
            schema: 1,
            identity: identity.clone(),
            result_version: 3,
            question_id: "section".into(),
            question_version: 2,
            supersedes: Some(previous.clone()),
            kind: crate::QuestionKind::Clarification,
            question: "Which section?".into(),
            context: "Read-only scope".into(),
            choices: vec!["Introduction".into(), "Installation".into()],
        };
        question.validate(&identity, 3, Some(&previous)).unwrap();
        let answer = crate::QuestionAnswer {
            schema: 1,
            request: "answer".into(),
            question: reference.clone(),
            control_epoch: 7,
            basis: crate::AnswerBasis::ExistingAuthority,
            scope: "README only".into(),
            answer: "Introduction".into(),
        };
        answer.validate(&reference, 7).unwrap();
        let auth = crate::DelegationAuth {
            session: SessionId(1),
            capability: "fixture".into(),
            source: None,
            controller: Some(crate::ControllerFence {
                workflow: "w".into(),
                epoch: 5,
                producer: "producer".into(),
            }),
        };
        for (name, fields) in [
            (
                "PublishQuestion",
                crate::DelegationRequest::PublishQuestion {
                    auth: auth.clone(),
                    handle: "r".into(),
                    question: question.clone(),
                },
            ),
            (
                "CollectQuestion",
                crate::DelegationRequest::CollectQuestion {
                    auth: auth.clone(),
                    handle: "r".into(),
                    reference: reference.clone(),
                },
            ),
            (
                "AnswerQuestion",
                crate::DelegationRequest::AnswerQuestion {
                    auth,
                    handle: "r".into(),
                    answer,
                    expected_parent_epoch: 5,
                },
            ),
        ] {
            let request = Request::Delegation(Box::new(fields));
            let bytes =
                bincode::serde::encode_to_vec(&request, bincode::config::standard()).unwrap();
            let (decoded, used): (Request, usize) =
                bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
            assert_eq!(decoded, request);
            assert_eq!(used, bytes.len());
            all.push((format!("Delegation::{name}"), encode(&request)));
        }
        let receipt = crate::DispatchReceipt {
            request: "answer".into(),
            operation: crate::DispatchOperation::AnswerQuestion,
            identity: identity.clone(),
            result_version: 3,
            outcome: crate::DispatchOutcome::Partial,
            input: Some(crate::InputDelivery {
                written_bytes: 3,
                total_bytes: 10,
                submit_written: false,
            }),
            reason: Some("prefix delivered".into()),
        };
        let state = crate::QuestionPublicationState {
            reference: reference.clone(),
            handled: false,
            current: true,
            issue: None,
            answer: Some(receipt.clone()),
        };
        let snapshot = crate::AssignmentSnapshot {
            coding_target: None,
            dependency_gap: None,
            review: None,
            review_assignment: None,
            review_scope: None,
            obligation_revision: 1,
            archive_receipts: vec![],
            handle: "r".into(),
            identity: identity.clone(),
            dispatch: crate::DispatchStatus::Applied,
            reason: None,
            activity: AgentActivity::Busy,
            conversation: Some("conv".into()),
            brief: "/brief".into(),
            result_path: "/result".into(),
            submission: None,
            acceptance: "pending".into(),
            result_version: 3,
            publication: None,
            acceptance_ref: None,
            history: vec![],
            selected_version: None,
            historical_submission: None,
            receipts: vec![receipt],
            resources: crate::AssignmentResources {
                coding: None,
                project: Some("p".into()),
                workspace: Some("ws".into()),
                workspace_present: true,
                current_session: None,
            },
            retryable_capacity_refusal: false,
            successor_request: None,
            execution_attempts: Some(1),
            control: crate::AssignmentControl::default(),
            control_evidence: None,
            questions: vec![state.clone()],
            collected_question: Some(question),
            recoveries: vec![],
            fallback_capture: None,
            recovered: None,
            accepted_recovery: None,
            submission_issue: None,
            selected_acceptance_ref: None,
            selected_recoveries: vec![],
        };
        let observation = crate::DelegationObservationBatch {
            cursor: crate::ObservationCursor {
                server_epoch: "epoch".into(),
                sequence: 4,
            },
            status: crate::ObservationStatus::Changed,
            progress_deadline_ms: 300000,
            observations: vec![crate::DelegationObservation {
                handle: "r".into(),
                identity,
                binding: None,
                activity: None,
                activity_revision: 1,
                input_requests: vec![],
                input_revision: 2,
                health: Some(crate::ReporterHealth::Connected),
                current: true,
                publication: None,
                publication_issue: None,
                last_progress_unix_ms: 100,
                stalled: false,
                progress_unresolved: true,
                control: crate::AssignmentControl::default(),
                control_receipt_digest: String::new(),
                native_control_revision: 0,
                workflow_cancelled: false,
                questions: vec![state],
                native_question_gap: Some(crate::NativeQuestionGap::IncompletePromptObservation),
            }],
        };
        for (name, response) in [
            (
                "QuestionCollection",
                Response::Delegation(Box::new(snapshot)),
            ),
            (
                "QuestionObservation",
                Response::DelegationObservation(Box::new(observation)),
            ),
        ] {
            let bytes =
                bincode::serde::encode_to_vec(&response, bincode::config::standard()).unwrap();
            let (decoded, used): (Response, usize) =
                bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
            assert_eq!(decoded, response);
            assert_eq!(used, bytes.len());
            all.push((format!("Response::{name}"), encode(&response)));
        }
        all.push((
            "PublicationIdentity::Question".into(),
            encode(&reference.publication()),
        ));
        for gap in [
            crate::NativeQuestionGap::IncompletePromptObservation,
            crate::NativeQuestionGap::NoPromptObservation,
            crate::NativeQuestionGap::CredentialInput,
            crate::NativeQuestionGap::IncompleteScreen,
            crate::NativeQuestionGap::AttributionChanged,
            crate::NativeQuestionGap::UnavailableBinding,
            crate::NativeQuestionGap::UnsupportedAdapter,
        ] {
            all.push((format!("NativeQuestionGap::{gap:?}"), encode(&gap)));
        }
    }

    fn cancellation_cases(all: &mut Vec<(String, String)>) {
        let auth = crate::DelegationAuth {
            session: SessionId(1),
            capability: "fixture".into(),
            controller: None,
            source: None,
        };
        let identity = crate::AssignmentIdentity {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            controller_epoch: 1,
            task: "t".into(),
            attempt: 1,
            contract: 1,
            request: "r".into(),
            session: Some(SessionId(2)),
            run: Some(crate::SessionRunId(1)),
        };
        let binding = crate::AgentBinding {
            provider: crate::AgentProvider::Pi,
            invocation: "inv".into(),
            conversation: "conv".into(),
            generation: 1,
        };
        let evidence = crate::AssignmentControlEvidence {
            server_lifetime: "server".into(),
            identity: identity.clone(),
            binding: binding.clone(),
            result_version: 1,
            activity_revision: 3,
            input_revision: 0,
            publication: None,
            publication_issue: None,
            human_input_revision: 0,
        };
        for (name, request) in [
            (
                "Delegation::Interrupt",
                Request::Delegation(Box::new(crate::DelegationRequest::Interrupt {
                    auth: auth.clone(),
                    handle: "r".into(),
                    request: "interrupt".into(),
                    expected_identity: identity.clone(),
                    expected_result_version: 1,
                    expected_control_epoch: 1,
                })),
            ),
            (
                "Delegation::ConfirmInterrupt",
                Request::Delegation(Box::new(crate::DelegationRequest::ConfirmInterrupt {
                    auth: auth.clone(),
                    handle: "r".into(),
                    request: "confirm".into(),
                    interrupted_request: "interrupt".into(),
                    expected_control_epoch: 2,
                    evidence: Box::new(evidence),
                })),
            ),
            (
                "Workflow::Cancel",
                Request::Workflow(Box::new(crate::WorkflowRequest::Cancel {
                    auth,
                    workflow: "w".into(),
                    request: "cancel".into(),
                })),
            ),
        ] {
            let bytes =
                bincode::serde::encode_to_vec(&request, bincode::config::standard()).unwrap();
            let (decoded, used): (Request, usize) =
                bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
            assert_eq!(used, bytes.len());
            assert_eq!(decoded, request);
            all.push((name.into(), encode(&request)));
        }
        let report = crate::ProviderReport {
            binding: binding.clone(),
            revision: 4,
            observation: crate::AgentObservation::NativeControl(Some(crate::NativeControlSample {
                turn: "turn".into(),
                interrupt: crate::NativeInterruptCapability::Escape,
                settled: false,
            })),
        };
        report.validate().unwrap();
        all.push(("ProviderReport::NativeControl".into(), encode(&report)));
        let cancelled = crate::WorkflowSnapshot {
            dependencies: None,
            deliveries: vec![],
            authority: crate::WorkflowAuthority {
                instance: "i".into(),
                workflow: "w".into(),
                controller: "c".into(),
                epoch: 1,
                parent: SessionId(1),
                parent_run: crate::SessionRunId(1),
                binding,
                producer: "producer".into(),
                server_lifetime: "server".into(),
            },
            active: true,
            handles: vec!["r".into()],
            pointer: None,
            continuation: None,
            gaps: vec![],
            receipt: None,
            cancellation: None,
        };
        let mut cancelled = cancelled;
        cancelled.cancellation = Some(crate::WorkflowCancellationView {
            request: "cancel".into(),
            captured_authority: cancelled.authority.clone(),
            dispatch_closure: crate::DispatchOutcome::Applied,
            target_outcome: crate::DispatchOutcome::Unknown,
            targets: vec![crate::WorkflowCancellationTarget {
                handle: "r".into(),
                identity,
                result_version: 1,
                control_epoch: 1,
                interrupt_request: "interrupt".into(),
                outcome: crate::DispatchOutcome::Unknown,
                reason: None,
            }],
            gaps: vec!["unverifiable member".into()],
            unverifiable_members: 1,
        });
        let response = Response::Workflows(vec![cancelled]);
        let bytes = bincode::serde::encode_to_vec(&response, bincode::config::standard()).unwrap();
        let (decoded, used): (Response, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(decoded, response);
        all.push(("Response::CancelledWorkflow".into(), encode(&response)));
        all.push((
            "ControlBarrier::Interrupt".into(),
            encode(&crate::ControlBarrier::Interrupt {
                request: "interrupt".into(),
            }),
        ));
    }

    fn review_cases(all: &mut Vec<(String, String)>) {
        let auth = crate::DelegationAuth {
            session: SessionId(1),
            capability: "fixture".into(),
            controller: None,
            source: None,
        };
        let identity = crate::AssignmentIdentity {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            controller_epoch: 1,
            task: "t".into(),
            attempt: 1,
            contract: 1,
            request: "implementation".into(),
            session: Some(SessionId(2)),
            run: Some(crate::SessionRunId(1)),
        };
        let publication = crate::PublicationIdentity {
            assignment: identity.clone(),
            kind: crate::PublicationKind::Submission,
            version: 1,
            digest: "a".repeat(64),
        };
        let coding = crate::CodingTargetReference {
            assignment: identity.clone(),
            publication: publication.clone(),
            candidate: crate::CandidateReference {
                input_baseline: None,
                path: "/private/candidate.json".into(),
                digest: "b".repeat(64),
                workspace: "coding".into(),
                base: "c".repeat(40),
                git_identity: "generation".into(),
                changed_artifacts: vec!["untracked.cjs".into()],
            },
            source_contract_digest: "d".repeat(64),
        };
        let source = crate::ReviewSource {
            source: crate::ReviewSourceReference {
                kind: crate::ReviewSourceKind::Spec,
                reference: "spec.md".into(),
                digest: "e".repeat(64),
            },
            content: "The actual originating specification.".into(),
        };
        let declaration = crate::ReviewDeclaration {
            request: "review".into(),
            target: coding.clone(),
            requirements: vec![crate::ReviewRequirement {
                axis: crate::ReviewAxis::Spec,
                sources: vec![source.source.clone()],
            }],
            sources: vec![source.clone()],
            repair_limit: None,
            context_paths: vec![],
        };
        let assignment = crate::ReviewAssignment {
            implementation: "implementation".into(),
            axis: crate::ReviewAxis::Spec,
            target: crate::ReviewTarget {
                coding: coding.clone(),
                sources: vec![source.source.clone()],
                view: None,
            },
            source_documents: vec![source],
            context_paths: vec![],
        };
        let report = crate::ReviewReport {
            target: assignment.target.clone(),
            axis: crate::ReviewAxis::Spec,
            outcome: crate::ReviewOutcome::Passed,
            findings: vec![],
        };
        let status = crate::ReviewStatus {
            target: coding,
            requirements: declaration.requirements.clone(),
            evidence: vec![],
            limit: crate::RepairLimit::default(),
            consumed_rounds: 1,
            pending_repair: Some("repair".into()),
            stopped: Some(crate::ReviewStop::MissingDecision),
            independent_review_satisfied: false,
        };
        let snapshot:crate::AssignmentSnapshot=serde_json::from_value(serde_json::json!({
            "coding_target":null,"dependency_gap":null,"handle":"implementation","identity":identity,"dispatch":"Applied","reason":null,"activity":"ResponseReady","conversation":"conversation","brief":"/brief","result_path":"/result","submission":{"schema":1,"identity":publication.assignment,"version":1,"outcome":"success","summary":"Independent review completed.","evidence":[],"unresolved":[],"review":report},"acceptance":"pending","result_version":1,"publication":null,"acceptance_ref":null,"history":[],"selected_version":null,"historical_submission":null,"receipts":[],"resources":{"project":"p","workspace":"ws","workspace_present":true,"current_session":null,"coding":null},"control":crate::AssignmentControl::default(),"control_evidence":null,"retryable_capacity_refusal":false,"successor_request":null,"execution_attempts":1,"questions":[],"collected_question":null,"obligation_revision":2,"archive_receipts":[],"review":status,"review_assignment":assignment,"review_scope":"Independent axis context"
        })).unwrap();
        let response = Response::Delegation(Box::new(snapshot));
        let bytes = bincode::serde::encode_to_vec(&response, bincode::config::standard()).unwrap();
        let (decoded, used): (Response, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(decoded, response);
        all.push(("Response::IndependentReview".into(), encode(&response)));
        for (name, operation) in [
            (
                "Declare",
                crate::ReviewOperation::Declare(Box::new(declaration)),
            ),
            (
                "Assignment",
                crate::ReviewOperation::Assignment {
                    axis: crate::ReviewAxis::Spec,
                },
            ),
            (
                "Collect",
                crate::ReviewOperation::Collect {
                    axis: crate::ReviewAxis::Standards,
                    reviewer: "reviewer".into(),
                },
            ),
            (
                "Repair",
                crate::ReviewOperation::Repair {
                    request: "repair".into(),
                    expected_control_epoch: 1,
                },
            ),
            (
                "Stop",
                crate::ReviewOperation::Stop {
                    reason: crate::ReviewStop::MissingDecision,
                },
            ),
        ] {
            let request = Request::Delegation(Box::new(crate::DelegationRequest::Review {
                auth: auth.clone(),
                handle: "implementation".into(),
                operation,
            }));
            let bytes =
                bincode::serde::encode_to_vec(&request, bincode::config::standard()).unwrap();
            let (decoded, used): (Request, usize) =
                bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
            assert_eq!(used, bytes.len());
            assert_eq!(decoded, request);
            all.push((format!("Request::Review::{name}"), encode(&request)));
        }
    }

    fn native_read_cases(all: &mut Vec<(String, String)>) {
        let identity = crate::AssignmentIdentity {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            controller_epoch: 1,
            task: "t".into(),
            attempt: 1,
            contract: 1,
            request: "r".into(),
            session: Some(SessionId(2)),
            run: Some(crate::SessionRunId(3)),
        };
        for (name, target) in [
            ("Brief", crate::AssignedReadTarget::Brief),
            ("Contract", crate::AssignedReadTarget::Contract),
            ("Submission", crate::AssignedReadTarget::Submission),
            (
                "Workspace",
                crate::AssignedReadTarget::Workspace {
                    path: "README.md".into(),
                },
            ),
            (
                "Answer",
                crate::AssignedReadTarget::Answer {
                    request: "answer".into(),
                },
            ),
        ] {
            let request = Request::Delegation(Box::new(crate::DelegationRequest::Read {
                auth: crate::DelegationAuth {
                    session: SessionId(1),
                    capability: "fixture".into(),
                    source: None,
                    controller: None,
                },
                read: crate::AssignedRead {
                    handle: "r".into(),
                    identity: identity.clone(),
                    result_version: 3,
                    control_epoch: 1,
                    target,
                    offset: 0,
                },
            }));
            let bytes =
                bincode::serde::encode_to_vec(&request, bincode::config::standard()).unwrap();
            let (decoded, used): (Request, usize) =
                bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
            assert_eq!(used, bytes.len());
            assert_eq!(decoded, request);
            all.push((format!("Delegation::Read::{name}"), encode(&request)));
        }
        let response = Response::AssignedRead(Box::new(crate::AssignedReadResult {
            identity,
            result_version: 3,
            control_epoch: 1,
            offset: 0,
            next_offset: 4,
            total_bytes: 4,
            text: "read".into(),
        }));
        let bytes = bincode::serde::encode_to_vec(&response, bincode::config::standard()).unwrap();
        let (decoded, used): (Response, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(decoded, response);
        all.push(("Response::AssignedRead".into(), encode(&response)));
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
        ("Request::CancelCloudCreation", "370104"),
        ("Request::DiscardCloudCreation", "380104"),
        ("Request::UnarchiveSession", "230104"),
        ("Request::DeleteArchivedSession", "220104"),
        ("Request::RecoverSession", "250104"),
        ("Request::Keystroke", "260104033a6a3a"),
        (
            "Request::SetSetting",
            "270f7069636b65725f726f6f74735b325d0105227e2f7822",
        ),
        ("Request::AcknowledgeSessionStopped", "240104"),
        ("Request::RefreshQuota", "280102"),
        ("Request::RefreshQuotaCursor", "280103"),
        ("Request::Events", "2901"),
        (
            "Request::NavigateNotification",
            "2a0444017301630168016c0104",
        ),
        ("Request::ConfirmNotificationNavigation", "2b016e"),
        ("Request::NotificationNavigationApplied", "2c016e"),
        ("Request::DashboardBridgeIdentity", "2d010169"),
        ("Request::PrepareITermFocus", "2e"),
        ("Request::BridgeOwner", "2f0444017301630168016c01640103"),
        ("Request::SaveWorkspaceWip", "3001610162"),
        ("Request::AnswerWipSave", "310161016201"),
        (
            "Request::MarkReviewed",
            "1e010103696e7604636f6e760101047475726e02",
        ),
        ("Response::Ok", "00"),
        ("Response::Hierarchy", "0100"),
        (
            "Response::CreatedSession",
            "02000100052f776f726b04000001700177016e016c010201030000010103696e7604636f6e760101020101047475726e000000020000010c617070726f76616c3a7265710503000000010103696e7604636f6e760101047475726e02010000000104010506000000",
        ),
        ("Response::Screen", "0301040301020107"),
        ("Response::Error", "0401016d"),
        (
            "Response::Inventory",
            "050001000100052f776f726b04000001700177016e016c010201030000010103696e7604636f6e760101020101047475726e000000020000010c617070726f76616c3a7265710503000000010103696e7604636f6e760101047475726e02010000000104010506000000",
        ),
        ("Response::TerminalText", "060101020174"),
        ("Response::Task", "070601"),
        ("Response::HistoryOpened", "0801020301020405"),
        (
            "Response::HistoryRows",
            "090102030401010101780100020102030000",
        ),
        ("Response::QuotaCooldown", "0bfb2c01"),
        (
            "Response::Events",
            "0c0101000101370d7469746c65206170706c696564",
        ),
        ("Response::NotificationNavigation", "0d04440000"),
        (
            "Response::NotificationNavigationConfirmed",
            "0e01016e0444017301630168016c0104",
        ),
        (
            "Response::ITermFocusPrepared",
            "0f0107080901016901010444017301630168016c0164",
        ),
        (
            "Response::BridgeOwner",
            "1004440107080901016901010444017301630168016c0164",
        ),
        ("ServerEvent::HierarchyChanged", "0000"),
        ("ServerEvent::Output", "010104030107"),
        ("ServerEvent::ScreenDirty", "02010403"),
        (
            "ServerEvent::SessionChanged",
            "03000100052f776f726b04000001700177016e016c010201030000010103696e7604636f6e760101020101047475726e000000020000010c617070726f76616c3a7265710503000000010103696e7604636f6e760101047475726e02010000000104010506000000",
        ),
        (
            "ServerEvent::QuotaChanged",
            "04000000000008013577616974696e6720666f722061206d616e6167656420436c617564652073657373696f6e277320666972737420726573706f6e7365000100000000070142436f6465782f47726f6b207573616765206f66663a20736574206071756f74612e656e61626c6564203d20747275656020696e2064617368626f6172642e746f6d6c000200000000070142436f6465782f47726f6b207573616765206f66663a20736574206071756f74612e656e61626c6564203d20747275656020696e2064617368626f6172642e746f6d6c000300000000070147437572736f72207573616765206f66663a20736574206071756f74612e637572736f722e64617368626f617264203d20747275656020696e2064617368626f6172642e746f6d6c00",
        ),
        (
            "ServerEvent::SettingsChanged",
            "05072f642e746f6d6c0500000000000100000002010470692f6d08666561747572652f01022f630101610101620104776f726b056f7672637204776f726b0c636f6d706f7365722d322e35011b68747470733a2f2f6769746875622e636f6d2f61636d652f617070000b2f73646b2e7461722e677a020170010161017100000005636f64657801022f680467726f6b000000fce0930400010103000300320000010d71756f74612e656e61626c656400010566616c736500010566616c736501036f66660101016b016d010300",
        ),
        (
            "ServerEvent::Recorded",
            "0601020105436f64657821436865636b696e67202d3e20556e617661696c61626c653a204854545020353033",
        ),
        ("ServerEvent::BridgeContext", "07017301630168016c"),
        (
            "ServerEvent::NotificationNavigation",
            "08016e0444017301630168016c0104",
        ),
        ("ServerEvent::ITermFocus", "0903"),
        (
            "ServerEvent::WipSavePrompt",
            "0a016101620d666561747572652f746f706963",
        ),
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
        (
            "QuotaWindow",
            "0e6e61746976652f7072696d6172790235680101fb68100001fc40420f00",
        ),
        ("QuotaSource::NativeProfile", "01066e617469766502"),
        ("QuotaSource::Probe", "02fbe803"),
        ("QuotaSource::Session", "0001040003696e7604636f6e7602"),
        ("ProviderQuota", "0100000000020108485454502035303301fb60ea"),
        (
            "AgentObservation::Quota",
            "0401010e6e61746976652f7072696d6172790235680101fb68100001fc40420f0001",
        ),
        ("TaskRequest::ListTasks", "00"),
        ("TaskRequest::GetTask", "0301"),
        ("TaskRequest::Cancel", "0e01"),
        ("TaskRequest::Clean", "0f0101"),
        (
            "AgentRequest::0",
            "1a01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a50772657365727665000003696e7600",
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
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70020004636f6e76",
        ),
        (
            "AgentRequest::4",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70030003696e7604636f6e76010300010a01640007666978747572650000010a01050103010201010766697874757265010001010766697874757265",
        ),
        (
            "AgentRequest::5",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f7004010003696e7604636f6e7601",
        ),
        (
            "AgentRequest::6",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70050003696e7604636f6e7601010201010e636f6c6c6563746f725f6c6f7374",
        ),
        (
            "AgentRequest::7",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f70060003696e7604636f6e76010004636f6e760b2f62696e2f636c61756465132f686973746f72792f636f6e762e6a736f6e6c0e2f636f6e6669672f636c6175646500",
        ),
        (
            "AgentRequest::8",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5026f7007",
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
            "AgentRequest::14",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5086372656174696f6e0800406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616104776f726b06637572736f7204776f726b0c636f6d706f7365722d322e35011b68747470733a2f2f6769746875622e636f6d2f61636d652f617070000b2f73646b2e7461722e677a40626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262621b68747470733a2f2f6769746875622e636f6d2f61636d652f6170702863636363636363636363636363636363636363636363636363636363636363636363636363636363",
        ),
        (
            "AgentRequest::15",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5086372656174696f6e08044061616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161",
        ),
        (
            "AgentRequest::16",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5086372656174696f6e08054061616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161096167656e742d3337330c636f6d706f7365722d322e35",
        ),
        (
            "AgentRequest::17",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5086372656174696f6e0801",
        ),
        (
            "AgentRequest::18",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5086372656174696f6e08024061616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161096167656e742d3337350772756e2d333735",
        ),
        (
            "AgentRequest::19",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5086372656174696f6e08034061616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161096167656e742d3337350772756e2d333735",
        ),
        (
            "AgentRequest::20",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5086372656174696f6e080c406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616104776f726b06637572736f7204776f726b0c636f6d706f7365722d322e35011b68747470733a2f2f6769746875622e636f6d2f61636d652f617070000b2f73646b2e7461722e677a40626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262621b68747470733a2f2f6769746875622e636f6d2f61636d652f6170702863636363636363636363636363636363636363636363636363636363636363636363636363636363096167656e742d3337360c636f6d706f7365722d322e3540646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464640772756e2d33373601",
        ),
        (
            "AgentRequest::21",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5086372656174696f6e080d406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616104776f726b06637572736f7204776f726b0c636f6d706f7365722d322e35011b68747470733a2f2f6769746875622e636f6d2f61636d652f617070000b2f73646b2e7461722e677a40626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262621b68747470733a2f2f6769746875622e636f6d2f61636d652f6170702863636363636363636363636363636363636363636363636363636363636363636363636363636363096167656e742d3337360c636f6d706f7365722d322e3540646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464640772756e2d3337360100",
        ),
        (
            "AgentRequest::22",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5086372656174696f6e080e406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616104776f726b06637572736f7204776f726b0c636f6d706f7365722d322e35011b68747470733a2f2f6769746875622e636f6d2f61636d652f617070000b2f73646b2e7461722e677a40626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262621b68747470733a2f2f6769746875622e636f6d2f61636d652f6170702863636363636363636363636363636363636363636363636363636363636363636363636363636363096167656e742d3337360c636f6d706f7365722d322e3540646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464640772756e2d33373601",
        ),
        (
            "AgentRequest::23",
            "1b01a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5086372656174696f6e080f4064646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464096167656e742d3337360772756e2d333736406565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656501013001",
        ),
        (
            "AgentResponse::0",
            "0a0101a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5",
        ),
        ("AgentResponse::1", "0a020003696e7604636f6e7601"),
        ("AgentResponse::2", "0a03"),
        ("AgentResponse::3", "0a04"),
        ("AgentResponse::4", "0a05"),
        ("AgentResponse::5", "0a06"),
        ("AgentResponse::6", "0a07"),
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
        ("AgentProvider::CursorCloud", "07"),
        ("AgentProvider::Operator", "08"),
        ("ITermFocusStatus::Authorized", "00"),
        ("ITermFocusStatus::SelectionRequested", "01"),
        ("ITermFocusStatus::AuthorizationRequired", "02"),
        ("ITermFocusStatus::Denied", "03"),
        ("ITermFocusStatus::Unavailable", "04"),
        (
            "ConversationReference::Pi",
            "01066e61746976650d2f62696e2f70726f7669646572010e2f686973746f72792e6a736f6e6c072f636f6e66696700",
        ),
        (
            "ConversationReference::Omp",
            "02066e61746976650d2f62696e2f70726f7669646572010e2f686973746f72792e6a736f6e6c072f636f6e66696700",
        ),
        (
            "ConversationReference::Codex",
            "03066e61746976650d2f62696e2f70726f7669646572010e2f686973746f72792e6a736f6e6c072f636f6e66696700",
        ),
        (
            "ConversationReference::Grok",
            "04066e61746976650e2f686973746f72792e6a736f6e6c",
        ),
        (
            "ConversationReference::Hermes",
            "051632303236313030365f3130313530305f6162313263640b2f62696e2f6865726d65730e2f686f6d652f73746174652e6462",
        ),
        (
            "WorkspaceRecord",
            "06737461626c65052f776f726b0d666561747572652f746f7069630103313a3200",
        ),
        (
            "WorkspaceSummary",
            "017006737461626c650d666561747572652f746f706963052f776f726b01010d6578706563746564206d61696e00",
        ),
        (
            "Delegation::Prepare",
            "32000107666978747572650000010177016301740172017002777304726561640104726561640270690474657374016d01046369746501047265616401060102010000000104707265760000",
        ),
        (
            "Delegation::Launch",
            "3201010766697874757265000001720169017701630101740101017201020101",
        ),
        (
            "Delegation::SendInitial",
            "320201076669787475726500000172016901770163010174010101720102010101",
        ),
        ("Delegation::Inspect", "320301076669787475726500000172"),
        ("Delegation::Collect", "320501076669787475726500000172"),
        ("Delegation::Reconcile", "320401076669787475726500000172"),
        (
            "Delegation::Followup",
            "32060107666978747572650000017201660169017701630101740101017201020101010107636c617269667901010177016301740172017002777304726561640104726561640270690474657374016d01046369746501047265616401060102010000000104707265760000",
        ),
        (
            "Delegation::CollectVersion",
            "32070107666978747572650000017201",
        ),
        (
            "Delegation::RecordAcceptance",
            "320801076669787475726500000172016901770163010174010101720102010101000664696765737401",
        ),
        (
            "Delegation::Publish",
            "320a0107666978747572650000017201016901770163010174010101720102010101077375636365737304646f6e6501086369746174696f6e000000",
        ),
        (
            "Delegation::Observe",
            "320b0107666978747572650000010172010565706f636803fbe803",
        ),
        (
            "Delegation::MarkHandled",
            "320c010766697874757265000001720169017701630101740101017201020101000106646967657374",
        ),
        ("Request::AssignedHumanInput", "36020102030107"),
        ("Request::AssignmentTakeover", "35000474616b6502010103"),
        (
            "Request::AssignmentReturn",
            "35010672657475726e02010203016c01690177016301017401010172010201010303696e7604636f6e7601010203000004",
        ),
        (
            "Delegation::ConfirmHumanReturn",
            "32090107666978747572650000017207636f6e6669726d0672657475726e03016c01690177016301017401010172010201010303696e7604636f6e7601010203000004",
        ),
        (
            "Delegation::Capabilities",
            "320d0107666978747572650000010301047465737401016d000000000000",
        ),
        (
            "Response::DelegationCapabilities",
            "1501030474657374016d000132000000000003012f0204726561640b726573756c745f66696c6501116d61785f6c6976655f6368696c6472656e000000",
        ),
        (
            "Response::Delegation",
            "110000017201690177016301017401010172010201010400050104636f6e76062f6272696566072f726573756c740101016901770163010174010101720102010101077375636365737304646f6e6501086369746174696f6e0000000770656e64696e6701000000000000010170010277730100000100000000000001046e65787401010000010000000000000000000000",
        ),
        (
            "Response::DelegationObservation",
            "140565706f63680302fce09304000101720169017701630101740101017201020101000001000201000101016901770163010174010101720102010100010664696765737400010264000101000000000000000000",
        ),
        (
            "Workflow::SetDependencies",
            "330001076669787475726500000177000163010174077461736b2e6d6401720170027773010000",
        ),
        ("Workflow::Parents", "33010107666978747572650000"),
        ("Workflow::Recover", "33020107666978747572650000010177"),
        (
            "Workflow::Save",
            "33030107666978747572650000017700010c6f626a6563746976652e6d640001017200000000",
        ),
        (
            "Workflow::Transfer",
            "33040107666978747572650000087472616e736665720169017701630101010303696e7604636f6e76010870726f64756365720673657276657203020303696e7604636f6e760109737563636573736f72",
        ),
        ("Request::DashboardWorkflowCandidates", "34000101"),
        (
            "Request::DashboardWorkflowTransfer",
            "3401087472616e736665720169017701630101010303696e7604636f6e76010870726f64756365720673657276657203020303696e7604636f6e760109737563636573736f72",
        ),
        (
            "Response::Workflows",
            "1201000169017701630101010303696e7604636f6e76010870726f64756365720673657276657201010172010101690177010664696765737401010c6f626a6563746976652e6d64000101720000000000000000",
        ),
        (
            "Response::WorkflowParents",
            "160101010303696e7604636f6e76010870726f6475636572",
        ),
        (
            "Response::WorkflowRecovery",
            "1701000169017701630101010303696e7604636f6e76010870726f64756365720673657276657201010172010101690177010664696765737401010c6f626a6563746976652e6d640001017200000000000000000201017701017201016901770163010174010101720102010101062f627269656601072f726573756c74116d697373696e6720617574686f7269747900000000001c756e76657269666961626c652072657461696e6564207265636f7264",
        ),
        (
            "Response::WorkflowCandidates",
            "1301087472616e736665720169017701630101010303696e7604636f6e76010870726f64756365720673657276657203020303696e7604636f6e760109737563636573736f720201017701017201016901770163010174010101720102010101062f627269656601072f726573756c74116d697373696e6720617574686f7269747900000000001c756e76657269666961626c652072657461696e6564207265636f7264",
        ),
        (
            "Delegation::Interrupt",
            "320e0107666978747572650000017209696e7465727275707401690177016301017401010172010201010101",
        ),
        (
            "Delegation::ConfirmInterrupt",
            "320f0107666978747572650000017207636f6e6669726d09696e74657272757074020673657276657201690177016301017401010172010201010303696e7604636f6e7601010300000000",
        ),
        (
            "Workflow::Cancel",
            "3305010766697874757265000001770663616e63656c",
        ),
        (
            "ProviderReport::NativeControl",
            "0303696e7604636f6e7601040501047475726e0000",
        ),
        (
            "Response::CancelledWorkflow",
            "1201000169017701630101010303696e7604636f6e76010870726f6475636572067365727665720101017200000000010663616e63656c0169017701630101010303696e7604636f6e76010870726f64756365720673657276657201040101720169017701630101740101017201020101010109696e7465727275707404000113756e76657269666961626c65206d656d6265720100",
        ),
        ("ControlBarrier::Interrupt", "0209696e74657272757074"),
        (
            "Delegation::PublishQuestion",
            "3210010766697874757265010177050870726f6475636572000172010169017701630101740101017201020101030773656374696f6e02010169017701630101740101017201020101030773656374696f6e014061616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161000e57686963682073656374696f6e3f0f526561642d6f6e6c792073636f7065020c496e74726f64756374696f6e0c496e7374616c6c6174696f6e",
        ),
        (
            "Delegation::CollectQuestion",
            "3211010766697874757265010177050870726f64756365720001720169017701630101740101017201020101030773656374696f6e024062626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262",
        ),
        (
            "Delegation::AnswerQuestion",
            "3212010766697874757265010177050870726f64756365720001720106616e737765720169017701630101740101017201020101030773656374696f6e02406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626207000b524541444d45206f6e6c790c496e74726f64756374696f6e05",
        ),
        (
            "Response::QuestionCollection",
            "110000017201690177016301017401010172010201010400020104636f6e76062f6272696566072f726573756c74000770656e64696e670300000000000106616e73776572090301690177016301017401010172010201010301030a0001107072656669782064656c6976657265640101700102777301000001000000000000000101010169017701630101740101017201020101030773656374696f6e0240626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262620001000106616e73776572090301690177016301017401010172010201010301030a0001107072656669782064656c69766572656401010169017701630101740101017201020101030773656374696f6e02010169017701630101740101017201020101030773656374696f6e014061616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161000e57686963682073656374696f6e3f0f526561642d6f6e6c792073636f7065020c496e74726f64756374696f6e0c496e7374616c6c6174696f6e010000000000000000000000",
        ),
        (
            "Response::QuestionObservation",
            "140565706f63680400fce09304000101720169017701630101740101017201020101000001000201000100006400010100000000000000010169017701630101740101017201020101030773656374696f6e0240626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262620001000106616e73776572090301690177016301017401010172010201010301030a0001107072656669782064656c6976657265640100",
        ),
        (
            "PublicationIdentity::Question",
            "016901770163010174010101720102010101030773656374696f6e024062626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262",
        ),
        ("NativeQuestionGap::IncompletePromptObservation", "00"),
        ("NativeQuestionGap::NoPromptObservation", "01"),
        ("NativeQuestionGap::CredentialInput", "02"),
        ("NativeQuestionGap::IncompleteScreen", "03"),
        ("NativeQuestionGap::AttributionChanged", "04"),
        ("NativeQuestionGap::UnavailableBinding", "05"),
        ("NativeQuestionGap::UnsupportedAdapter", "06"),
        (
            "Request::WorkflowDeclareDelivery",
            "330601076669787475726500000964656c697665726564017706524541444d450f706172656e74206465636973696f6e01017201690177016301017401010172010201010304000001010c7265766965772d696e646578406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616101067265766965770100",
        ),
        (
            "RecordedDelivery",
            "0964656c69766572656440626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262620169017701630101010303696e7604636f6e76010870726f6475636572067365727665720964656c697665726564017706524541444d450f706172656e74206465636973696f6e01017201690177016301017401010172010201010304000001010c7265766965772d696e646578406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616101067265766965770100",
        ),
        (
            "Delegation::Archive",
            "321301076669787475726500000761726368697665017201690177016301017401010172010201010304020964656c697665726564406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626201086f726967696e616c",
        ),
        ("ControlBarrier::Archive", "030761726368697665"),
        (
            "ArchiveReceipt::StopUnknown",
            "0761726368697665017201690177016301017401010172010201010304020964656c697665726564406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626201086f726967696e616c40636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363630169017701630101010303696e7604636f6e76010870726f6475636572067365727665720102010303696e7604636f6e760101",
        ),
        (
            "ArchiveReceipt::StoppedArchivePending",
            "0761726368697665017201690177016301017401010172010201010304020964656c697665726564406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626201086f726967696e616c40636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363630169017701630101010303696e7604636f6e76010870726f6475636572067365727665720102010303696e7604636f6e760102",
        ),
        (
            "ArchiveReceipt::Archived { previous_run: SessionRunId(1), archived_run: SessionRunId(2) }",
            "0761726368697665017201690177016301017401010172010201010304020964656c697665726564406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626201086f726967696e616c40636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363630169017701630101010303696e7604636f6e76010870726f6475636572067365727665720102010303696e7604636f6e7601030102",
        ),
        ("ArchiveRefusal::RequiredRetention", "0000"),
        ("ArchiveRefusal::DeliverySuperseded", "0001"),
        ("ArchiveRefusal::ObligationChanged", "0002"),
        ("ArchiveRefusal::StaleTarget", "0003"),
        ("ArchiveRefusal::HumanOwned", "0004"),
        ("ArchiveRefusal::OwnershipUncertain", "0005"),
        ("ArchiveRefusal::UnresolvedControl", "0006"),
        ("ArchiveRefusal::AttributionChanged", "0007"),
        ("ArchiveRefusal::Busy", "0008"),
        ("ArchiveRefusal::NoOwnedSession", "0009"),
        ("ArchiveRefusal::Capacity", "000a"),
        ("RetentionChoice::Keep", "000e7265766965772070656e64696e67"),
        ("DeliveryEvidenceKind::Recovery", "00"),
        ("DeliveryEvidenceKind::Review", "01"),
        ("DeliveryEvidenceKind::Check", "02"),
        ("DeliveryEvidenceKind::Continuation", "03"),
        ("DeliveryEvidenceKind::OtherAuthorizedEvidence", "04"),
        (
            "Delegation::CaptureFallback",
            "32140107666978747572650000017201690177016301017401010172010201030301",
        ),
        (
            "Delegation::RecoverFallback",
            "3215010766697874757265000001720763617074757265020d61727469666163742e6a736f6e4062626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262014061616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161010f436f6d706c65746520524541444d450f696e646570656e64656e742e747874406464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646406524541444d4519496e646570656e64656e7420706172656e7420726576696577",
        ),
        (
            "Delegation::RecordRecoveryAcceptance",
            "3208010766697874757265000001720169017701630101740101017201020103030102406565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656501",
        ),
        (
            "Response::FallbackRecovery",
            "11000001720169017701630101740101017201020103040005010a6368696c642d636f6e760e2f62726965662d76332e6a736f6e132f7375626d697373696f6e2d76332e6a736f6e0008616363657074656403000101690177016301017401010172010201030301024065656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565132f616363657074616e63652d76332e6a736f6e406666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666600010300000101700102777301000001000000000106736572766572016901770163010174010101720102010303056368696c640a6368696c642d636f6e76010307000001010000000101000004000102406565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656501010763617074757265067365727665720169017701630101010306706172656e740b706172656e742d636f6e76010870726f64756365720673657276657201690177016301017401010172010201030340636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363634066666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666056e6f6e636503056368696c640a6368696c642d636f6e7601010000000006736572766572016901770163010174010101720102010303056368696c640a6368696c642d636f6e7601030700000101000118504f4f565243525f46414c4c4241434b3a626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262620001406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616101017b011b2f706172656e742d66616c6c6261636b2d72332d76322e6a736f6e406565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656501020169017701630101010306706172656e740b706172656e742d636f6e76010870726f647563657206736572766572010763617074757265067365727665720169017701630101010306706172656e740b706172656e742d636f6e76010870726f64756365720673657276657201690177016301017401010172010201030340636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363634066666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666056e6f6e636503056368696c640a6368696c642d636f6e7601010000000006736572766572016901770163010174010101720102010303056368696c640a6368696c642d636f6e7601030700000101000118504f4f565243525f46414c4c4241434b3a626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262620001406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616101017b0d61727469666163742e6a736f6e406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626208617274696661637410636f6e74726163742d76312e6a736f6e406363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636308636f6e74726163740d62726965662d76332e6a736f6e4066666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666056272696566010f436f6d706c65746520524541444d450f696e646570656e64656e742e747874406464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646406524541444d4519496e646570656e64656e7420706172656e7420726576696577010f696e646570656e64656e742e747874406464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646406524541444d450101020169017701630101010306706172656e740b706172656e742d636f6e76010870726f647563657206736572766572010763617074757265067365727665720169017701630101010306706172656e740b706172656e742d636f6e76010870726f64756365720673657276657201690177016301017401010172010201030340636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363634066666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666056e6f6e636503056368696c640a6368696c642d636f6e7601010000000006736572766572016901770163010174010101720102010303056368696c640a6368696c642d636f6e7601030700000101000118504f4f565243525f46414c4c4241434b3a626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262620001406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616101017b0d61727469666163742e6a736f6e406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626208617274696661637410636f6e74726163742d76312e6a736f6e406363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636363636308636f6e74726163740d62726965662d76332e6a736f6e4066666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666056272696566010f436f6d706c65746520524541444d450f696e646570656e64656e742e747874406464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646406524541444d4519496e646570656e64656e7420706172656e7420726576696577010f696e646570656e64656e742e747874406464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646406524541444d450118696e76616c6964206368696c64207375626d697373696f6e0101690177016301017401010172010201030301024065656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565132f616363657074616e63652d76332e6a736f6e406666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666666601024065656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565000000",
        ),
        (
            "Response::IndependentReview",
            "1100000e696d706c656d656e746174696f6e01690177016301017401010e696d706c656d656e746174696f6e01020101040005010c636f6e766572736174696f6e062f6272696566072f726573756c74010101690177016301017401010e696d706c656d656e746174696f6e010201010107737563636573731d496e646570656e64656e742072657669657720636f6d706c657465642e0000000101690177016301017401010e696d706c656d656e746174696f6e0102010101690177016301017401010e696d706c656d656e746174696f6e010201010001406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616100172f707269766174652f63616e6469646174652e6a736f6e406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626206636f64696e6728636363636363636363636363636363636363636363636363636363636363636363636363636363630a67656e65726174696f6e010d756e747261636b65642e636a734064646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464010107737065632e6d644065656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565000100000770656e64696e6701000000000000010170010277730100000100000000000000010100000200000000000000000101690177016301017401010e696d706c656d656e746174696f6e0102010101690177016301017401010e696d706c656d656e746174696f6e010201010001406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616100172f707269766174652f63616e6469646174652e6a736f6e406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626206636f64696e6728636363636363636363636363636363636363636363636363636363636363636363636363636363630a67656e65726174696f6e010d756e747261636b65642e636a7340646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464640101010107737065632e6d6440656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565650003023f636f6f7264696e61746f722064656661756c743a20746872656520636f727265637469766520726f756e64733b206e6f74207573657220617070726f76616c010106726570616972010300010e696d706c656d656e746174696f6e0101690177016301017401010e696d706c656d656e746174696f6e0102010101690177016301017401010e696d706c656d656e746174696f6e010201010001406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616100172f707269766174652f63616e6469646174652e6a736f6e406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626206636f64696e6728636363636363636363636363636363636363636363636363636363636363636363636363636363630a67656e65726174696f6e010d756e747261636b65642e636a734064646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464010107737065632e6d64406565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656500010107737065632e6d644065656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565255468652061637475616c206f726967696e6174696e672073706563696669636174696f6e2e000118496e646570656e64656e74206178697320636f6e74657874",
        ),
        (
            "Request::Review::Declare",
            "321601076669787475726500000e696d706c656d656e746174696f6e000672657669657701690177016301017401010e696d706c656d656e746174696f6e0102010101690177016301017401010e696d706c656d656e746174696f6e010201010001406161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616100172f707269766174652f63616e6469646174652e6a736f6e406262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626262626206636f64696e6728636363636363636363636363636363636363636363636363636363636363636363636363636363630a67656e65726174696f6e010d756e747261636b65642e636a7340646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464646464640101010107737065632e6d644065656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565010107737065632e6d644065656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565656565255468652061637475616c206f726967696e6174696e672073706563696669636174696f6e2e0000",
        ),
        (
            "Request::Review::Assignment",
            "321601076669787475726500000e696d706c656d656e746174696f6e0101",
        ),
        (
            "Request::Review::Collect",
            "321601076669787475726500000e696d706c656d656e746174696f6e0200087265766965776572",
        ),
        (
            "Request::Review::Repair",
            "321601076669787475726500000e696d706c656d656e746174696f6e030672657061697201",
        ),
        (
            "Request::Review::Stop",
            "321601076669787475726500000e696d706c656d656e746174696f6e0403",
        ),
        (
            "Delegation::Read::Brief",
            "321701076669787475726500000172016901770163010174010101720102010303010000",
        ),
        (
            "Delegation::Read::Contract",
            "321701076669787475726500000172016901770163010174010101720102010303010100",
        ),
        (
            "Delegation::Read::Submission",
            "321701076669787475726500000172016901770163010174010101720102010303010200",
        ),
        (
            "Delegation::Read::Workspace",
            "321701076669787475726500000172016901770163010174010101720102010303010309524541444d452e6d6400",
        ),
        (
            "Delegation::Read::Answer",
            "321701076669787475726500000172016901770163010174010101720102010303010406616e7377657200",
        ),
        (
            "Response::AssignedRead",
            "18016901770163010174010101720102010303010004040472656164",
        ),
        ("Request::Operator::Create", "390001056d6f64656c"),
        ("Request::Operator::List", "3901010a"),
        ("Request::Operator::Read", "390201010a"),
        (
            "Request::Operator::Submit",
            "390301000568656c6c6f01017001056c6162656c0106737461626c6501022f7701050106",
        ),
        ("Request::Operator::Stop", "390401020304"),
        (
            "Request::OperatorExchange::AssistantDelta",
            "3a030707070707070707070707070707070707070707070707070707070707070707010203040101000568656c6c6f",
        ),
        (
            "Request::OperatorExchange::ToolCall",
            "3a030707070707070707070707070707070707070707070707070707070707070707010203040101010463616c6c0462617368027b7d",
        ),
        (
            "Request::OperatorExchange::ToolOutput",
            "3a030707070707070707070707070707070707070707070707070707070707070707010203040101020463616c6c066f75747075740100",
        ),
        (
            "Request::OperatorExchange::Completed",
            "3a0307070707070707070707070707070707070707070707070707070707070707070102030401010301025b5d",
        ),
        (
            "Request::OperatorExchange::Failed",
            "3a03070707070707070707070707070707070707070707070707070707070707070701020304010104056572726f72",
        ),
        (
            "Request::OperatorWorker::Start",
            "3b0307070707070707070707070707070707070707070707070707070707070707070001020304",
        ),
        (
            "Request::OperatorWorker::Retire",
            "3b0307070707070707070707070707070707070707070707070707070707070707070101020304",
        ),
        (
            "Request::OperatorWorker::Stop",
            "3b0307070707070707070707070707070707070707070707070707070707070707070201020304",
        ),
        (
            "Response::Operator::Created",
            "190000010300056d6f64656c0568656c6c6f01010203040000",
        ),
        (
            "Response::Operator::Conversations",
            "19010100010300056d6f64656c0568656c6c6f01010203040000",
        ),
        (
            "Response::Operator::History",
            "190200010300056d6f64656c0568656c6c6f01010203040000010102000568656c6c6f01017001056c6162656c0106737461626c6501022f77010501060101",
        ),
        ("Response::Operator::Submitted", "1903010203040000"),
        ("Response::Operator::Stopped", "1904010203040200"),
        ("Response::OperatorExchange", "1a010203040100"),
        (
            "Response::OperatorWorker::Start",
            "1b000102030400056d6f64656c0568656c6c6f01017001056c6162656c0106737461626c6501022f77010501060101025b5d",
        ),
        ("Response::OperatorWorker::Retired", "1b0101020304"),
        ("ServerEvent::OperatorChanged", "0c01"),
        ("SessionScope::Workspace", "00"),
        ("SessionScope::OperatorConversation", "0101"),
        ("OperatorToolStatus::Succeeded", "00"),
        ("OperatorToolStatus::Failed", "01010e"),
        ("OperatorToolStatus::TimedOut", "02"),
        ("OperatorTurnState::Running", "00"),
        ("OperatorTurnState::Complete", "01"),
        ("OperatorTurnState::Interrupted", "02"),
        ("OperatorTurnState::Error", "03"),
        ("OperatorMessageContent::Worker", "01000161"),
        ("OperatorMessageContent::Interrupted", "02"),
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

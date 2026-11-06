//! Provisional rows and the Lifecycle status strip (ADR 0010 / issue #297).

use super::{Dashboard, DashboardAction, TreeRow};
use crate::protocol::{
    ClientMessage, ErrorCode, LifecycleOp, LifecycleOutcome, Request, ServerEvent, SessionId,
    SessionKind,
};
use crate::session::SessionSummary;

/// The Server runs one create, remove or close at a time.
pub(super) const BUSY: &str =
    "Another create, remove or close is still running; try again when it finishes";

/// Fiction until the Server completes the Lifecycle job or a failed row is dismissed.
pub(super) struct Provisional {
    pub token: u64,
    pub kind: ProvisionalKind,
    pub initial_prompt_intent: Option<crate::protocol::InitialPromptIntentId>,
    pub failed: Option<(ErrorCode, String)>,
    /// The form or confirmation that submitted the job, out of view while it
    /// runs; a failure brings it back with the person's values.
    pub form: Option<Box<super::palette::Palette>>,
    /// Requests to send after this job succeeds, in order: the rest of a
    /// workspace removal that first stops its running sessions.
    pub then: Vec<Request>,
}

#[derive(Clone, Debug)]
pub(super) enum ProvisionalKind {
    CreatingWorkspace {
        project: String,
        id: String,
        label: String,
        /// A first program was requested, so an empty workspace is not done:
        /// its launch may still fail.
        launch: bool,
    },
    RemovingWorkspace {
        project: String,
        id: String,
    },
    CreatingSession {
        project: String,
        workspace: String,
        name: String,
        label: Option<String>,
        #[allow(dead_code)]
        kind: SessionKind,
    },
    ClosingSession {
        session: SessionId,
    },
}

impl ProvisionalKind {
    pub(super) fn strip_message(&self) -> String {
        match self {
            Self::CreatingWorkspace { label, .. } => format!("Creating workspace {label}…"),
            Self::RemovingWorkspace { .. } => "Removing workspace…".into(),
            Self::CreatingSession { name, label, .. } => {
                let shown = label.as_deref().filter(|s| !s.is_empty()).unwrap_or(name);
                let shown = if shown.is_empty() { "session" } else { shown };
                format!("Creating {shown}…")
            }
            Self::ClosingSession { .. } => "Closing terminal…".into(),
        }
    }

    #[allow(dead_code)]
    pub(super) fn op(&self) -> LifecycleOp {
        match self {
            Self::CreatingWorkspace { .. } => LifecycleOp::CreateWorkspace,
            Self::RemovingWorkspace { .. } => LifecycleOp::RemoveWorkspace,
            Self::CreatingSession { .. } => LifecycleOp::CreateSession,
            Self::ClosingSession { .. } => LifecycleOp::CloseTerminal,
        }
    }
}

impl Provisional {
    pub(super) fn from_request(token: u64, request: &Request) -> Option<Self> {
        let kind = match request {
            Request::CreateWorkspace {
                project,
                id,
                branch,
            } => ProvisionalKind::CreatingWorkspace {
                project: project.clone(),
                id: id.clone(),
                label: branch_label(branch),
                launch: false,
            },
            Request::CreateWorkspaceWithLaunch {
                project,
                id,
                branch,
                launch,
            } => ProvisionalKind::CreatingWorkspace {
                project: project.clone(),
                id: id.clone(),
                label: branch_label(branch),
                launch: launch.is_some(),
            },
            Request::RemoveWorkspace { project, name, .. } => ProvisionalKind::RemovingWorkspace {
                project: project.clone(),
                id: name.clone(),
            },
            Request::CreateSession(session) => ProvisionalKind::CreatingSession {
                project: session.project.clone(),
                workspace: session.workspace.clone(),
                name: session.name.clone(),
                label: session.label.clone(),
                kind: session.kind.clone(),
            },
            Request::CreateAgentSession(request) => ProvisionalKind::CreatingSession {
                project: request.session.project.clone(),
                workspace: request.session.workspace.clone(),
                name: request.session.name.clone(),
                label: request.session.label.clone(),
                kind: request.session.kind.clone(),
            },
            Request::CreateAgentSessionWithPrompt(request) => ProvisionalKind::CreatingSession {
                project: request.launch.session.project.clone(),
                workspace: request.launch.session.workspace.clone(),
                name: request.launch.session.name.clone(),
                label: request.launch.session.label.clone(),
                kind: request.launch.session.kind.clone(),
            },
            Request::CloseTerminal { session, .. } => {
                ProvisionalKind::ClosingSession { session: *session }
            }
            _ => return None,
        };
        let initial_prompt_intent = match request {
            Request::CreateAgentSessionWithPrompt(request) => Some(request.intent.clone()),
            _ => None,
        };
        Some(Self {
            token,
            kind,
            initial_prompt_intent,
            failed: None,
            form: None,
            then: Vec::new(),
        })
    }

    /// Short enough that a narrow sidebar never clips the way to dismiss it;
    /// the strip names the target and the reason.
    pub(super) fn row_label(&self) -> String {
        if self.failed.is_some() {
            let what = match &self.kind {
                ProvisionalKind::CreatingWorkspace { .. } => "Not created",
                ProvisionalKind::RemovingWorkspace { .. } => "Remove failed",
                ProvisionalKind::CreatingSession { .. } => "Not started",
                ProvisionalKind::ClosingSession { .. } => "Close failed",
            };
            return format!("{what} · Esc dismiss");
        }
        match &self.kind {
            ProvisionalKind::CreatingWorkspace { label, .. } => format!("Creating… {label}"),
            ProvisionalKind::RemovingWorkspace { .. } => "Removing…".into(),
            ProvisionalKind::CreatingSession { name, label, .. } => {
                let shown = label.as_deref().filter(|s| !s.is_empty()).unwrap_or(name);
                let shown = if shown.is_empty() { "session" } else { shown };
                format!("Creating… {shown}")
            }
            ProvisionalKind::ClosingSession { .. } => "Closing…".into(),
        }
    }
}

fn branch_label(branch: &crate::protocol::BranchRequest) -> String {
    match branch {
        crate::protocol::BranchRequest::New { branch, .. }
        | crate::protocol::BranchRequest::Existing { branch } => branch.clone(),
    }
}

impl Dashboard {
    /// Select a just-created workspace once it appears in hierarchy (empty or with local shell).
    pub(super) fn attach_lifecycle_workspace(
        &mut self,
        project: String,
        id: String,
    ) -> DashboardAction {
        let Some(workspace) = super::state::find_workspace(self, &project, &id).cloned() else {
            return DashboardAction::Redraw;
        };
        if workspace.sessions.is_empty() {
            self.provisional = None;
            self.lifecycle_strip = None;
            self.dismiss_error_banner();
            self.select_container(TreeRow::Workspace {
                project: workspace.project.clone(),
                id: workspace.id.clone(),
            });
            let request_id = self.next_request_id();
            return match self.view_request(self.outer_area, request_id) {
                Ok(Some(message)) => {
                    self.enqueue_request(message);
                    DashboardAction::Redraw
                }
                _ => DashboardAction::Redraw,
            };
        }
        if let Some(session) = workspace
            .sessions
            .iter()
            .find(|session| {
                session.name == "local" && session.phase == crate::session::SessionPhase::Running
            })
            .map(|session| session.id)
        {
            let summary = super::state::find_session(self, session)
                .cloned()
                .unwrap_or_else(|| workspace.sessions[0].clone());
            self.provisional = None;
            self.lifecycle_strip = None;
            self.dismiss_error_banner();
            return self.select_created_session(summary);
        }
        // Launched first terminal is named after the workspace, not `local`.
        // Wait for LifecycleCompleted::CreatedSession rather than dropping fiction here.
        DashboardAction::Redraw
    }

    /// Hierarchy may land before LifecycleCompleted. Only auto-attach empty workspaces
    /// nothing was asked to start in; a requested launch waits for CreatedSession or
    /// its failure, so a launch that failed after creation is not dropped unseen.
    pub(super) fn try_attach_lifecycle_workspace_from_hierarchy(
        &mut self,
    ) -> Option<DashboardAction> {
        let Some(row) = &self.provisional else {
            return None;
        };
        let ProvisionalKind::CreatingWorkspace {
            project,
            id,
            launch,
            ..
        } = &row.kind
        else {
            return None;
        };
        let workspace = super::state::find_workspace(self, project, id)?;
        let ready = (workspace.sessions.is_empty() && !launch)
            || workspace.sessions.iter().any(|session| {
                session.name == "local" && session.phase == crate::session::SessionPhase::Running
            });
        if !ready {
            // Named launch session: wait for LifecycleCompleted::CreatedSession.
            return None;
        }
        let project = project.clone();
        let id = id.clone();
        Some(self.attach_lifecycle_workspace(project, id))
    }

    /// Id-requiring actions soft-fail while a Provisional row stands in for a real identity.
    pub(super) fn soft_fail_if_provisional(&mut self) -> Option<DashboardAction> {
        let Some(row) = &self.provisional else {
            return None;
        };
        if row.failed.is_some() {
            self.lifecycle_strip = Some("dismiss the failed row first (Esc)".into());
            return Some(DashboardAction::Redraw);
        }
        match &row.kind {
            ProvisionalKind::CreatingSession { .. } | ProvisionalKind::CreatingWorkspace { .. } => {
                self.lifecycle_strip =
                    Some("wait for the Lifecycle job to finish before using this action".into());
                Some(DashboardAction::Redraw)
            }
            ProvisionalKind::RemovingWorkspace { .. } | ProvisionalKind::ClosingSession { .. } => {
                self.lifecycle_strip = Some(BUSY.into());
                Some(DashboardAction::Redraw)
            }
        }
    }

    pub(super) fn lifecycle_busy(&self) -> bool {
        self.provisional
            .as_ref()
            .is_some_and(|row| row.failed.is_none())
            || self.lifecycle_pending.is_some()
    }

    pub(super) fn begin_lifecycle_pending(&mut self, token: u64, request: &Request) {
        if let Some(row) = Provisional::from_request(token, request) {
            self.lifecycle_pending = Some(row);
        }
    }

    /// Accept response for a Lifecycle job: show the Provisional row and strip.
    /// If HierarchyChanged already landed, attach immediately (either-order).
    pub(super) fn accept_lifecycle_job(&mut self, token: u64) -> Option<DashboardAction> {
        let mut row = self.lifecycle_pending.take()?;
        if row.token != token {
            self.lifecycle_pending = Some(row);
            return None;
        }
        // The submitting palette closes on accept and keeps its launch
        // preference, so success can still ask the Server to remember it. A
        // later removal step carries the form it already took.
        if let Some(form) = self.take_palette_for_job(token) {
            row.form = Some(form);
        }
        self.lifecycle_strip = Some(row.kind.strip_message());
        self.provisional = Some(row);
        self.try_attach_lifecycle_workspace_from_hierarchy()
    }

    pub(super) fn refuse_lifecycle_job(&mut self, message: String) {
        self.lifecycle_pending = None;
        self.lifecycle_strip = Some(message);
    }

    /// Esc on the strip. After a failure the strip and its row go together.
    pub(super) fn dismiss_lifecycle_strip(&mut self) -> bool {
        let failed = self
            .provisional
            .as_ref()
            .is_some_and(|row| row.failed.is_some());
        if failed {
            self.provisional = None;
        }
        self.lifecycle_strip.take().is_some() || failed
    }

    pub(super) fn dismiss_failed_provisional(&mut self) -> bool {
        if self
            .provisional
            .as_ref()
            .is_some_and(|row| row.failed.is_some())
        {
            self.provisional = None;
            self.lifecycle_strip = None;
            true
        } else {
            false
        }
    }

    pub(super) fn drop_provisional_fiction(&mut self) {
        self.provisional = None;
        self.lifecycle_pending = None;
        self.lifecycle_strip = None;
        self.lifecycle_preference = None;
    }

    #[cfg(test)]
    pub(super) fn on_lifecycle_completed(
        &mut self,
        client_token: u64,
        op: LifecycleOp,
        outcome: LifecycleOutcome,
    ) -> DashboardAction {
        self.on_lifecycle_completed_with_target(client_token, op, outcome, None)
    }

    pub(super) fn on_lifecycle_completed_with_target(
        &mut self,
        client_token: u64,
        op: LifecycleOp,
        outcome: LifecycleOutcome,
        target_workspace: Option<crate::protocol::WorkspaceLifecycleIdentity>,
    ) -> DashboardAction {
        if let Some(action) =
            self.on_branch_lifecycle_completed(client_token, op, outcome.clone(), target_workspace)
        {
            return action;
        }
        if self
            .provisional
            .as_ref()
            .is_some_and(|row| row.token == client_token && row.initial_prompt_intent.is_some())
            || self
                .lifecycle_pending
                .as_ref()
                .is_some_and(|row| row.token == client_token && row.initial_prompt_intent.is_some())
        {
            // Prompt launches complete by stable Server intent. A generic
            // connection token may collide with work after Dashboard replacement.
            return DashboardAction::None;
        }
        // CreateWorkspaceWithLaunch shares the CreatingWorkspace provisional.
        let _ = op;
        match outcome {
            LifecycleOutcome::CreatedSession(summary) => {
                // Hierarchy may have arrived first; still select from the typed outcome.
                if let Some(row) = &self.provisional
                    && row.token != client_token
                {
                    return DashboardAction::None;
                }
                self.provisional = None;
                self.lifecycle_strip = None;
                self.select_created_session(*summary)
            }
            LifecycleOutcome::Succeeded => {
                let Some(row) = self.provisional.take() else {
                    return DashboardAction::Redraw;
                };
                if row.token != client_token {
                    self.provisional = Some(row);
                    return DashboardAction::None;
                }
                self.lifecycle_strip = None;
                if !row.then.is_empty() {
                    return self.next_lifecycle_step(row);
                }
                match &row.kind {
                    ProvisionalKind::CreatingWorkspace { project, id, .. } => {
                        let project = project.clone();
                        let id = id.clone();
                        // Keep provisional until hierarchy has the row, then attach.
                        self.provisional = Some(row);
                        let action = self.attach_lifecycle_workspace(project, id);
                        if self.provisional.is_some() {
                            // Job finished but no empty/local attach target (e.g. exited local).
                            self.provisional = None;
                            self.lifecycle_strip = None;
                        }
                        action
                    }
                    _ => DashboardAction::Redraw,
                }
            }
            LifecycleOutcome::Failed { code, message } => {
                let Some(row) = self.provisional.take() else {
                    return DashboardAction::Redraw;
                };
                if row.token != client_token {
                    self.provisional = Some(row);
                    return DashboardAction::None;
                }
                self.fail_lifecycle_job(row, code, message);
                DashboardAction::Redraw
            }
        }
    }

    /// Sends the next request of a removal that stops sessions first. The new
    /// job inherits the confirmation and the steps still to come.
    fn next_lifecycle_step(&mut self, row: Provisional) -> DashboardAction {
        let Provisional { form, mut then, .. } = row;
        let request = then.remove(0);
        if let Request::CreateAgentSession(launch) = request.clone()
            && matches!(&launch.session.kind, SessionKind::Agent { .. })
        {
            self.queue_chained_initial_prompt_launch(launch, form, then);
            return DashboardAction::Redraw;
        }
        let request_id = self.next_request_id();
        self.begin_lifecycle_pending(request_id, &request);
        if let Some(next) = self.lifecycle_pending.as_mut() {
            next.form = form;
            next.then = then;
        }
        self.push_request(ClientMessage {
            request_id,
            request,
        });
        DashboardAction::Redraw
    }

    /// A refused or failed job returns to the form that sent it, values kept,
    /// with no failed row left behind. Only when that form cannot come back
    /// without taking keys meant for something else does a failed row stay,
    /// and the strip says how to dismiss it.
    pub(super) fn fail_lifecycle_job(
        &mut self,
        mut row: Provisional,
        code: ErrorCode,
        message: String,
    ) {
        self.lifecycle_preference = None;
        self.lifecycle_strip = None;
        if self.can_return_to_form()
            && let Some(form) = row.form.take()
        {
            // A session the removal had to stop is the one that refused.
            let stopping = match (&row.kind, row.then.is_empty()) {
                (ProvisionalKind::ClosingSession { session }, false) => Some(*session),
                _ => None,
            };
            // A task's session follows its new workspace as a second job.
            let after_workspace = matches!(row.kind, ProvisionalKind::CreatingSession { .. })
                && form.creates_workspace();
            self.return_to_form(form, row.token, stopping, after_workspace, code, message);
            return;
        }
        let created = match &row.kind {
            ProvisionalKind::CreatingWorkspace {
                project, id, label, ..
            } => Some((project.clone(), id.clone(), label.clone())),
            _ => None,
        };
        let what = match &row.kind {
            ProvisionalKind::CreatingWorkspace { .. } => "Couldn't create the workspace".to_owned(),
            ProvisionalKind::CreatingSession { .. } => "Couldn't start the session".to_owned(),
            ProvisionalKind::RemovingWorkspace { project, id } => {
                let name = super::state::find_workspace(self, project, id)
                    .map(|workspace| super::state::workspace_heading(self, workspace))
                    .unwrap_or_else(|| "the workspace".into());
                format!("Couldn't remove {name}")
            }
            ProvisionalKind::ClosingSession { session } => {
                let name = super::state::find_session(self, *session)
                    .map(|session| session.display_name().to_owned())
                    .unwrap_or_else(|| format!("#{}", session.0));
                format!("Couldn't close {name}")
            }
        };
        let reason = self.plain_reason(&message, created);
        self.lifecycle_strip = Some(format!("{what}: {reason} · Esc dismiss"));
        row.form = None;
        row.then.clear();
        row.failed = Some((code, message));
        self.provisional = Some(row);
    }

    pub(super) fn select_created_session(&mut self, session: SessionSummary) -> DashboardAction {
        self.select_session(session.id);
        self.mode = super::InputMode::Terminal;
        let request_id = self.next_request_id();
        if let Ok(Some(message)) = self.view_request(self.outer_area, request_id) {
            self.enqueue_request(message);
        }
        if let Some((project, choice)) = self.lifecycle_preference.take() {
            let request = super::settings::launch_choice_request(&project, &choice);
            self.settings.launch_choices.insert(project, choice);
            let message = crate::protocol::ClientMessage {
                request_id: self.error_owning_request_id(),
                request,
            };
            self.enqueue_request(message);
        }
        DashboardAction::Redraw
    }

    pub(super) fn provisional_tree_rows(&self) -> Vec<(TreeRow, String)> {
        let Some(row) = &self.provisional else {
            return Vec::new();
        };
        let label = row.row_label();
        match &row.kind {
            ProvisionalKind::CreatingWorkspace { project, id, .. } => {
                // If the Server already published the workspace (partial), overlay label only.
                if super::state::find_workspace(self, project, id).is_some() {
                    vec![(
                        TreeRow::Workspace {
                            project: project.clone(),
                            id: id.clone(),
                        },
                        label,
                    )]
                } else {
                    vec![(
                        TreeRow::ProvisionalWorkspace {
                            project: project.clone(),
                            id: id.clone(),
                        },
                        label,
                    )]
                }
            }
            ProvisionalKind::RemovingWorkspace { project, id } => vec![(
                TreeRow::Workspace {
                    project: project.clone(),
                    id: id.clone(),
                },
                label,
            )],
            ProvisionalKind::CreatingSession {
                project, workspace, ..
            } => vec![(
                TreeRow::ProvisionalSession {
                    project: project.clone(),
                    workspace: workspace.clone(),
                    token: row.token,
                },
                label,
            )],
            ProvisionalKind::ClosingSession { session } => {
                vec![(TreeRow::Session { id: *session }, label)]
            }
        }
    }

    pub(super) fn provisional_overlay_for(&self, row: &TreeRow) -> Option<String> {
        self.provisional_tree_rows()
            .into_iter()
            .find(|(tree, _)| tree == row)
            .map(|(_, label)| label)
    }

    #[allow(dead_code)]
    pub(super) fn handle_lifecycle_event(
        &mut self,
        event: &ServerEvent,
    ) -> Option<DashboardAction> {
        match event {
            ServerEvent::LifecycleCompleted {
                client_token,
                op,
                outcome,
                target_workspace,
            } => Some(self.on_lifecycle_completed_with_target(
                *client_token,
                op.clone(),
                outcome.clone(),
                target_workspace.clone(),
            )),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        BranchRequest, CreateSessionRequest, ErrorCode, LifecycleOp, LifecycleOutcome, SessionKind,
    };
    use std::ffi::OsString;

    #[test]
    fn provisional_from_create_session_has_no_session_id() {
        let request = Request::CreateSession(CreateSessionRequest {
            project: "p".into(),
            workspace: "w".into(),
            name: String::new(),
            label: Some("codex".into()),
            argv: vec![OsString::from("codex")],
            kind: SessionKind::Agent {
                name: "codex".into(),
            },
        });
        let row = Provisional::from_request(9, &request).unwrap();
        assert_eq!(row.token, 9);
        assert!(matches!(row.kind, ProvisionalKind::CreatingSession { .. }));
        assert!(row.row_label().contains("Creating"));
    }

    #[test]
    fn provisional_from_create_workspace_uses_branch_label() {
        let request = Request::CreateWorkspace {
            project: "p".into(),
            id: "stable".into(),
            branch: BranchRequest::New {
                branch: "feature/x".into(),
                base: "main".into(),
            },
        };
        let row = Provisional::from_request(1, &request).unwrap();
        assert!(row.row_label().contains("feature/x"));
    }

    /// Esc hides only the strip while a job runs. After a failure with no form
    /// to return to, the row and strip both say "Esc dismiss" and one Esc
    /// clears both.
    #[test]
    fn one_esc_dismisses_a_failed_row_with_its_strip() {
        let mut d =
            crate::dashboard::Dashboard::new(crate::session::TerminalSize { rows: 24, cols: 80 });
        d.begin_lifecycle_pending(
            3,
            &Request::CreateSession(CreateSessionRequest {
                project: "p".into(),
                workspace: "w".into(),
                name: "n".into(),
                label: None,
                argv: vec![OsString::from("true")],
                kind: SessionKind::Terminal,
            }),
        );
        d.accept_lifecycle_job(3);
        assert!(d.lifecycle_strip.is_some());
        assert!(d.provisional.is_some());
        assert!(d.dismiss_lifecycle_strip());
        assert!(d.lifecycle_strip.is_none());
        assert!(d.provisional.is_some(), "a running job keeps its row");
        d.on_lifecycle_completed(
            3,
            LifecycleOp::CreateSession,
            LifecycleOutcome::Failed {
                code: ErrorCode::Conflict,
                message: "boom".into(),
            },
        );
        let row = d.provisional.as_ref().unwrap();
        assert!(row.failed.is_some());
        assert_eq!(row.row_label(), "Not started · Esc dismiss");
        assert_eq!(
            d.lifecycle_strip.as_deref(),
            Some("Couldn't start the session: boom · Esc dismiss")
        );
        assert!(d.dismiss_lifecycle_strip());
        assert!(d.provisional.is_none());
        assert!(d.lifecycle_strip.is_none());
        assert!(!d.dismiss_failed_provisional());
    }

    #[test]
    fn created_session_selects_even_without_matching_provisional() {
        use crate::protocol::{
            LifecycleOp, LifecycleOutcome, SessionKind, SessionPhase, SessionRunId,
        };
        use crate::session::{AgentActivity, SessionSummary};
        use std::path::PathBuf;
        let mut d =
            crate::dashboard::Dashboard::new(crate::session::TerminalSize { rows: 24, cols: 80 });
        let summary = SessionSummary {
            scope: ovrcr_protocol::SessionScope::Workspace,
            id: crate::protocol::SessionId(42),
            archived: false,
            cwd: PathBuf::from("/tmp"),
            run: SessionRunId(1),
            kind: SessionKind::Terminal,
            recovery: None,
            project: "p".into(),
            workspace: "wid".into(),
            name: "wizard".into(),
            label: String::new(),
            pid: None,
            started_unix_ms: None,
            phase: SessionPhase::Running,
            activity: AgentActivity::Idle,
            agent: None,
            agent_epoch: 0,
            unread: None,
            context_usage: None,
            title: None,
            manual_title: None,
            assignment_control: None,
        };
        let action = d.on_lifecycle_completed(
            7,
            LifecycleOp::CreateWorkspaceWithLaunch,
            LifecycleOutcome::CreatedSession(Box::new(summary)),
        );
        assert!(d.provisional.is_none());
        assert_eq!(d.focused_session(), Some(crate::protocol::SessionId(42)));
        assert_eq!(action, DashboardAction::Redraw);
        assert!(matches!(
            d.drain_one_request().unwrap().request,
            Request::SetView { .. }
        ));
    }

    #[test]
    fn second_lifecycle_op_is_refused_while_busy() {
        let mut d =
            crate::dashboard::Dashboard::new(crate::session::TerminalSize { rows: 24, cols: 80 });
        d.begin_lifecycle_pending(
            1,
            &Request::CreateWorkspace {
                project: "p".into(),
                id: "id".into(),
                branch: BranchRequest::Existing { branch: "b".into() },
            },
        );
        d.accept_lifecycle_job(1);
        assert!(d.lifecycle_busy());
        d.refuse_lifecycle_job("a lifecycle job is already running".into());
        assert_eq!(
            d.lifecycle_strip.as_deref(),
            Some("a lifecycle job is already running")
        );
        assert!(
            d.provisional.is_some(),
            "provisional stays while strip shows refuse"
        );
    }
}

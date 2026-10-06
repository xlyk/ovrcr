//! Provisional rows and the Lifecycle status strip (ADR 0010 / issue #297).

use super::{Dashboard, DashboardAction, TreeRow};
use crate::protocol::{
    ErrorCode, LifecycleOp, LifecycleOutcome, Request, ServerEvent, SessionId, SessionKind,
};
use crate::session::SessionSummary;

/// Fiction until the Server completes the Lifecycle job or a failed row is dismissed.
#[derive(Clone, Debug)]
pub(super) struct Provisional {
    pub token: u64,
    pub kind: ProvisionalKind,
    pub failed: Option<(ErrorCode, String)>,
}

#[derive(Clone, Debug)]
pub(super) enum ProvisionalKind {
    CreatingWorkspace {
        project: String,
        id: String,
        label: String,
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
            },
            Request::CreateWorkspaceWithLaunch {
                project,
                id,
                branch,
                ..
            } => ProvisionalKind::CreatingWorkspace {
                project: project.clone(),
                id: id.clone(),
                label: branch_label(branch),
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
            Request::CloseTerminal { session, .. } => {
                ProvisionalKind::ClosingSession { session: *session }
            }
            _ => return None,
        };
        Some(Self {
            token,
            kind,
            failed: None,
        })
    }

    pub(super) fn row_label(&self) -> String {
        if let Some((_, message)) = &self.failed {
            return format!("Failed: {message}");
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
        self.provisional = None;
        self.lifecycle_strip = None;
        self.dismiss_error_banner();
        if workspace.sessions.is_empty() {
            self.select_container(TreeRow::Workspace {
                project: workspace.project.clone(),
                id: workspace.id.clone(),
            });
            let request_id = self.next_request_id();
            return match self.view_request(self.outer_area, request_id) {
                Ok(Some(message)) => DashboardAction::Request(message),
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
            return self.select_created_session(summary);
        }
        DashboardAction::Redraw
    }

    /// Hierarchy may land before or after LifecycleCompleted; attach when both truth and fiction agree.
    pub(super) fn try_attach_lifecycle_workspace_from_hierarchy(
        &mut self,
    ) -> Option<DashboardAction> {
        let Some(row) = &self.provisional else {
            return None;
        };
        let ProvisionalKind::CreatingWorkspace { project, id, .. } = &row.kind else {
            return None;
        };
        super::state::find_workspace(self, project, id)?;
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
                self.lifecycle_strip = Some("a lifecycle job is already running".into());
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
        let row = self.lifecycle_pending.take()?;
        if row.token != token {
            self.lifecycle_pending = Some(row);
            return None;
        }
        // Keep the launch preference across the Provisional wait so success can
        // still ask the Server to remember it (palette closes on accept).
        let preference = self
            .palette
            .as_mut()
            .and_then(|palette| palette.launch_preference.take());
        self.lifecycle_preference = preference;
        self.lifecycle_strip = Some(row.kind.strip_message());
        self.provisional = Some(row);
        self.palette = None;
        self.try_attach_lifecycle_workspace_from_hierarchy()
    }

    pub(super) fn refuse_lifecycle_job(&mut self, message: String) {
        self.lifecycle_pending = None;
        self.lifecycle_strip = Some(message);
    }

    pub(super) fn dismiss_lifecycle_strip(&mut self) -> bool {
        self.lifecycle_strip.take().is_some()
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

    pub(super) fn on_lifecycle_completed(
        &mut self,
        client_token: u64,
        op: LifecycleOp,
        outcome: LifecycleOutcome,
    ) -> DashboardAction {
        let Some(mut row) = self.provisional.take() else {
            return DashboardAction::Redraw;
        };
        if row.token != client_token {
            self.provisional = Some(row);
            return DashboardAction::None;
        }
        // CreateWorkspaceWithLaunch shares the CreatingWorkspace provisional.
        let _ = op;
        match outcome {
            LifecycleOutcome::Succeeded => {
                self.lifecycle_strip = None;
                match &row.kind {
                    ProvisionalKind::CreatingWorkspace { project, id, .. } => {
                        let project = project.clone();
                        let id = id.clone();
                        // Keep provisional until hierarchy has the row, then attach.
                        self.provisional = Some(row);
                        self.attach_lifecycle_workspace(project, id)
                    }
                    _ => DashboardAction::Redraw,
                }
            }
            LifecycleOutcome::CreatedSession(summary) => {
                self.lifecycle_strip = None;
                self.select_created_session(*summary)
            }
            LifecycleOutcome::Failed { code, message } => {
                // Preserve the sync remove UX: DirtyWorktree / SessionsRemain reopen as force.
                if let ProvisionalKind::RemovingWorkspace { project, id } = &row.kind
                    && matches!(code, ErrorCode::DirtyWorktree | ErrorCode::SessionsRemain)
                {
                    let project = project.clone();
                    let id = id.clone();
                    self.lifecycle_strip = Some(format!("{code:?}: {message}"));
                    self.open_forced_remove_workspace(project, id, code, message);
                    return DashboardAction::Redraw;
                }
                self.lifecycle_preference = None;
                row.failed = Some((code.clone(), message.clone()));
                self.lifecycle_strip = Some(format!("{code:?}: {message}"));
                self.provisional = Some(row);
                DashboardAction::Redraw
            }
        }
    }

    fn select_created_session(&mut self, session: SessionSummary) -> DashboardAction {
        self.select_session(session.id);
        self.mode = super::InputMode::Terminal;
        let request_id = self.next_request_id();
        let mut outgoing = match self.view_request(self.outer_area, request_id) {
            Ok(Some(message)) => vec![message],
            _ => Vec::new(),
        };
        if let Some((project, choice)) = self.lifecycle_preference.take() {
            let request = super::settings::launch_choice_request(&project, &choice);
            self.settings.launch_choices.insert(project, choice);
            outgoing.push(crate::protocol::ClientMessage {
                request_id: self.error_owning_request_id(),
                request,
            });
        }
        match outgoing.len() {
            0 => DashboardAction::Redraw,
            1 => DashboardAction::Request(outgoing.pop().unwrap()),
            _ => DashboardAction::RequestBatch(outgoing),
        }
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
            } => Some(self.on_lifecycle_completed(*client_token, op.clone(), outcome.clone())),
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

    #[test]
    fn esc_dismisses_strip_then_failed_provisional() {
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
        assert!(d.provisional.is_some());
        d.on_lifecycle_completed(
            3,
            LifecycleOp::CreateSession,
            LifecycleOutcome::Failed {
                code: ErrorCode::Conflict,
                message: "boom".into(),
            },
        );
        assert!(d.provisional.as_ref().unwrap().failed.is_some());
        assert!(d.dismiss_lifecycle_strip());
        assert!(d.dismiss_failed_provisional());
        assert!(d.provisional.is_none());
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

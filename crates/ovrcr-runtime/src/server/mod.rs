use crate::config::{ProjectRecord, Registry, load_registry, save_registry_atomic};
use crate::git::{self, BranchSpec};
use crate::session::{
    HookEnvironment, InputAdmissionError, Session, SessionEvent, SessionId, SessionPhase,
    SessionSpec, SessionSummary, TerminalSize,
};
use crate::task_manager::TaskManager;
use anyhow::{Context, Result, bail};
use ovrcr_protocol::{
    BranchRequest, ClientMessage, ClientRole, ErrorCode, HierarchySnapshot, ProjectSummary,
    Request, Response, ServerEvent, ServerMessage, WorkspaceSummary, read_frame, write_frame,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

mod connections;
mod dispatch;
mod outbound;
mod startup;

use connections::{
    combine_control_and_refresh, error_chain_string, error_for_lifecycle, error_response,
    handle_connection, handle_request_with_id, input_error_code, response_message,
};
pub use dispatch::{DispatchMessage, HistoryRequest, run_dispatcher};
use dispatch::{bridge_events, clear_dashboard_geometry, set_dashboard_geometry};
use outbound::{
    DashboardDelivery, DashboardSlot, DashboardSnapshot, dashboard_owner_matches, dashboard_send,
    dashboard_send_owner, dashboard_snapshot, dashboard_try_send, disconnect_dashboard,
};
pub use outbound::{DashboardOutbound, DashboardSink};
pub use startup::{ServerPaths, run_server};
use startup::{generate_hook_capability, validate_bound_socket, wake_accept};

#[cfg(test)]
use connections::handle_shutdown;

#[derive(Debug)]
struct LifecycleFailure {
    code: ErrorCode,
    message: String,
    publish_hierarchy: bool,
}

impl std::fmt::Display for LifecycleFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LifecycleFailure {}

fn lifecycle_error(code: ErrorCode, message: impl Into<String>) -> anyhow::Error {
    lifecycle_error_with_hierarchy(code, message, false)
}

fn lifecycle_error_with_hierarchy(
    code: ErrorCode,
    message: impl Into<String>,
    publish_hierarchy: bool,
) -> anyhow::Error {
    anyhow::Error::new(LifecycleFailure {
        code,
        message: message.into(),
        publish_hierarchy,
    })
}

pub const RAW_EVENT_QUEUE_CAPACITY: usize = 64;
pub const RAW_DISPATCH_QUEUE_CAPACITY: usize = 64;
const DASHBOARD_QUEUE: usize = 64;

pub struct ServerState {
    pub tasks: Option<Arc<TaskManager>>,
    socket: PathBuf,
    pub registry_path: PathBuf,
    pub registry: Mutex<Registry>,
    pub sessions: Mutex<HashMap<SessionId, Arc<Session>>>,
    pub selected: Mutex<Option<SessionId>>,
    pub dashboard: Mutex<Option<Arc<DashboardSink>>>,
    pub next_session_id: AtomicU64,
    pub mutation_lock: Mutex<()>,
    pub dispatch: SyncSender<DispatchMessage>,
    pub shutdown: AtomicBool,
    pub stopping: AtomicBool,
    pub dashboard_size: Mutex<Option<DashboardGeometry>>,
    pub events: Mutex<Option<SyncSender<SessionEvent>>>,
    dashboard_slot: Mutex<Option<DashboardSlot>>,
}

pub struct DashboardGeometry {
    owner: Arc<()>,
    size: TerminalSize,
}

impl ServerState {
    pub fn hierarchy(&self) -> HierarchySnapshot {
        snapshot_from_state(self)
    }

    pub fn inventory(&self) -> (Registry, Vec<SessionSummary>) {
        let _mutation = self.mutation_lock.lock().unwrap();
        let registry = self.registry.lock().unwrap().clone();
        let sessions = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .map(|session| session.summary())
            .collect();
        (registry, sessions)
    }

    pub fn read_terminal(
        &self,
        id: SessionId,
        max_lines: Option<usize>,
    ) -> Result<(TerminalSize, String)> {
        if max_lines == Some(0) {
            return Err(lifecycle_error(
                ErrorCode::InvalidRequest,
                "max_lines must be greater than zero",
            ));
        }
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("session {} not found", id.0))
            })?;
        let (size, text) = session.terminal_text();
        let text = match max_lines {
            Some(max_lines) => {
                let lines = text.lines().collect::<Vec<_>>();
                lines[lines.len().saturating_sub(max_lines)..].join("\n")
            }
            None => text,
        };
        Ok((size, text))
    }

    pub fn send_terminal(&self, id: SessionId, text: &str, submit: bool) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("session {} not found", id.0))
            })?;
        if matches!(session.summary().phase, SessionPhase::Exited { .. }) {
            return Err(lifecycle_error(ErrorCode::Conflict, "session has exited"));
        }
        session
            .send_text(text, submit)
            .map_err(|error| lifecycle_error(input_error_code(&error), error_chain_string(&error)))
    }

    pub fn close_terminal(&self, id: SessionId, grace: Duration) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let session = self.session_for_control(id)?;
        session.revoke_hook_capability();
        let termination = session.terminate(grace).map(|_| ());
        let refresh = self.refresh_session_locked(id);
        combine_control_and_refresh(id, termination, refresh)?;
        self.remove_session_locked(id)
    }

    pub fn set_session_paused(&self, id: SessionId, paused: bool) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let session = self.session_for_control(id)?;
        let control = session.set_paused(paused).map(|_| ());
        let refresh = self.refresh_session_locked(id);
        combine_control_and_refresh(id, control, refresh)
    }

    pub fn handle_request(&self, role: &mut ClientRole, request: Request) -> Response {
        handle_request_with_id(self, role, request, 0, None)
    }

    pub fn create_session(
        &self,
        request: ovrcr_protocol::CreateSessionRequest,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.create_session_locked(request, None)
    }

    #[cfg(test)]
    pub(crate) fn create_session_with_ready(
        &self,
        request: ovrcr_protocol::CreateSessionRequest,
        ready: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.create_session_locked(request, Some(ready))
    }

    fn create_session_locked(
        &self,
        request: ovrcr_protocol::CreateSessionRequest,
        ready: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<SessionSummary> {
        let (cwd, label) = {
            let registry = self.registry.lock().unwrap();
            let workspace = registry
                .workspace(&request.project, &request.workspace)
                .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?;
            let label = request.label.clone().unwrap_or_else(|| {
                request
                    .argv
                    .first()
                    .and_then(|arg| Path::new(arg).file_name())
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            (workspace.path.clone(), label)
        };
        let id = SessionId(self.next_session_id.fetch_add(1, Ordering::Relaxed));
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.values().any(|session| {
            let summary = session.summary();
            summary.project == request.project
                && summary.workspace == request.workspace
                && summary.name == request.name
        }) {
            return Err(lifecycle_error(
                ErrorCode::AlreadyExists,
                format!(
                    "duplicate session: {}/{}:{}",
                    request.project, request.workspace, request.name
                ),
            ));
        }
        let spec = SessionSpec {
            project: request.project,
            workspace: request.workspace,
            name: request.name,
            label,
            cwd,
            argv: request.argv,
            hook_env: Some(HookEnvironment {
                socket: validate_bound_socket(&self.socket)?,
                session: id,
                capability: generate_hook_capability()?,
            }),
        };
        let size = self.dashboard_size.lock().unwrap().as_ref().map_or(
            TerminalSize {
                rows: 40,
                cols: 120,
            },
            |geometry| geometry.size,
        );
        let events = self
            .events
            .lock()
            .unwrap()
            .as_ref()
            .context("server event channel closed")?
            .clone();
        let session = match ready {
            Some(ready) => Session::spawn_with_ready(id, spec, size, events, ready),
            None => Session::spawn(id, spec, size, events),
        }
        .context("spawn session")?;
        let summary = session.summary();
        sessions.insert(id, session);
        Ok(summary)
    }

    pub fn kill_session(&self, id: SessionId, grace: Duration) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let session = self.session_for_control(id)?;
        session.revoke_hook_capability();
        let termination = session.terminate(grace).map(|_| ());
        let refresh = self.refresh_session_locked(id);
        combine_control_and_refresh(id, termination, refresh)
    }

    pub fn remove_session(&self, id: SessionId) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.remove_session_locked(id)
    }

    fn remove_session_locked(&self, id: SessionId) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("session {} not found", id.0))
            })?;
        if !matches!(session.summary().phase, SessionPhase::Exited { .. }) {
            return Err(lifecycle_error(
                ErrorCode::SessionRunning,
                "session is still live; kill it before removal",
            ));
        }
        session.revoke_hook_capability();
        self.sessions.lock().unwrap().remove(&id);
        if let Some(slot) = self.dashboard_slot.lock().unwrap().as_mut()
            && slot
                .history
                .as_ref()
                .is_some_and(|history| history.opened().session == id)
        {
            slot.history.take();
        }
        let mut selected = self.selected.lock().unwrap();
        if selected.as_ref() == Some(&id) {
            *selected = None;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn with_tasks_for_test(
        tasks: Arc<TaskManager>,
        registry_path: PathBuf,
    ) -> Arc<Self> {
        let (events, _) = mpsc::sync_channel(RAW_EVENT_QUEUE_CAPACITY);
        let (dispatch, _) = mpsc::sync_channel(RAW_DISPATCH_QUEUE_CAPACITY);
        Arc::new(Self {
            tasks: Some(tasks),
            socket: registry_path.with_extension("sock"),
            registry_path,
            registry: Mutex::new(Registry::default()),
            sessions: Mutex::new(HashMap::new()),
            selected: Mutex::new(None),
            dashboard: Mutex::new(None),
            next_session_id: AtomicU64::new(1),
            mutation_lock: Mutex::new(()),
            dispatch,
            shutdown: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            dashboard_size: Mutex::new(None),
            events: Mutex::new(Some(events)),
            dashboard_slot: Mutex::new(None),
        })
    }
    pub fn request_shutdown(&self, kill: bool) -> Response {
        let admission = self.tasks.as_ref().map(|tasks| tasks.admission_guard());
        if let Some(tasks) = &self.tasks
            && !kill
            && tasks.has_active()
        {
            return error_response(ErrorCode::SessionsRemain, "task runs remain");
        }
        let _mutation = self.mutation_lock.lock().unwrap();
        if self.stopping.load(Ordering::Acquire) {
            return error_response(ErrorCode::Conflict, "server is stopping");
        }
        let sessions = self
            .sessions
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if !kill && !sessions.is_empty() {
            return error_response(ErrorCode::SessionsRemain, "sessions remain");
        }
        if kill {
            let mut failures = Vec::new();
            for session in sessions {
                let id = session.summary().id;
                session.revoke_hook_capability();
                let termination = session.terminate(Duration::from_secs(5)).map(|_| ());
                let refresh = self.refresh_session_locked(id);
                if let Err(error) = combine_control_and_refresh(id, termination, refresh) {
                    failures.push(format!("session {}: {}", id.0, error_chain_string(&error)));
                }
            }
            if !failures.is_empty() {
                return error_response(ErrorCode::PartialFailure, failures.join("; "));
            }
        }
        self.stopping.store(true, Ordering::Release);
        if let Some(tasks) = &self.tasks {
            tasks.quiesce();
        }
        drop(_mutation);
        drop(admission);
        if let Some(tasks) = &self.tasks
            && let Err(error) = tasks.stop()
        {
            return error_response(
                ErrorCode::PartialFailure,
                format!("task shutdown: {error:#}"),
            );
        }
        Response::Ok
    }

    fn reject_if_stopping(&self) -> Result<()> {
        if self.stopping.load(Ordering::Acquire) {
            bail!("server is stopping")
        }
        Ok(())
    }

    fn session_for_control(&self, id: SessionId) -> Result<Arc<Session>> {
        self.sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("session {} not found", id.0))
            })
    }

    fn refresh_session_locked(&self, id: SessionId) -> Result<()> {
        let error = match self
            .dispatch
            .try_send(DispatchMessage::RefreshSession { session: id })
        {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(_)) => "dispatcher queue is full",
            Err(mpsc::TrySendError::Disconnected(_)) => "dispatcher is unavailable",
        };
        if let Some(snapshot) = dashboard_snapshot(self) {
            disconnect_dashboard(self, snapshot);
        }
        bail!("{error}")
    }

    pub fn add_project(&self, name: String, repo: PathBuf, workspace_root: PathBuf) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let (repo, workspace_root) = git::validate_project(&repo, &workspace_root)?;
        let mut registry = self.registry.lock().unwrap();
        let mut next = registry.clone();
        if next.projects.iter().any(|project| project.name == name) {
            return Err(lifecycle_error(
                ErrorCode::AlreadyExists,
                format!("duplicate project: {name}"),
            ));
        }
        next.add_project(ProjectRecord {
            name,
            repo,
            workspace_root,
            workspaces: Vec::new(),
        })?;
        save_registry_atomic(&next, &self.registry_path)?;
        *registry = next;
        Ok(())
    }

    pub(crate) fn task_mutation_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.mutation_lock.lock().unwrap()
    }
    pub fn remove_project(&self, name: &str) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        if self
            .tasks
            .as_ref()
            .is_some_and(|tasks| tasks.references_project(name))
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "project is referenced by a scheduled task",
            ));
        }
        let mut registry = self.registry.lock().unwrap();
        let mut next = registry.clone();
        let project = next
            .projects
            .iter()
            .find(|project| project.name == name)
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::NotFound, format!("project not found: {name}"))
            })?;
        if !project.workspaces.is_empty() {
            return Err(lifecycle_error(
                ErrorCode::SessionsRemain,
                format!("cannot remove project {name}: workspaces remain"),
            ));
        }
        next.remove_project(name)?;
        save_registry_atomic(&next, &self.registry_path)?;
        *registry = next;
        Ok(())
    }

    pub fn create_workspace(
        &self,
        project: String,
        name: String,
        branch: BranchRequest,
    ) -> Result<()> {
        self.create_workspace_inner(project, name, branch, true)
    }

    pub fn create_task_workspace(
        &self,
        project: String,
        name: String,
        branch: String,
        base: String,
    ) -> Result<()> {
        self.create_workspace_inner(project, name, BranchRequest::New { branch, base }, false)
    }

    fn create_workspace_inner(
        &self,
        project: String,
        name: String,
        branch: BranchRequest,
        start_shell: bool,
    ) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let project_record = self
            .registry
            .lock()
            .unwrap()
            .project(&project)
            .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
            .clone();
        if project_record
            .workspaces
            .iter()
            .any(|workspace| workspace.name == name)
        {
            return Err(lifecycle_error(
                ErrorCode::AlreadyExists,
                format!("duplicate workspace: {project}/{name}"),
            ));
        }
        let branch = match branch {
            BranchRequest::New { branch, base } => BranchSpec::New { branch, base },
            BranchRequest::Existing { branch } => BranchSpec::Existing { branch },
        };
        let workspace = git::create_worktree(&project_record, &name, branch)
            .with_context(|| format!("create workspace {project}/{name}"))?;
        {
            let mut registry = self.registry.lock().unwrap();
            let mut next = registry.clone();
            next.add_workspace(&project, workspace.clone())?;
            if let Err(error) = save_registry_atomic(&next, &self.registry_path) {
                return Err(lifecycle_error(
                    ErrorCode::PartialFailure,
                    format!(
                        "registry write failed: worktree remains at {}: {}",
                        workspace.path.display(),
                        error_chain_string(&error)
                    ),
                ));
            }
            *registry = next;
        }
        if !start_shell {
            return Ok(());
        }
        let shell = std::env::var_os("SHELL").ok_or_else(|| {
            lifecycle_error_with_hierarchy(
                ErrorCode::PartialFailure,
                format!(
                    "worktree for workspace {} exists at {} but SHELL is unset",
                    name,
                    workspace.path.display()
                ),
                true,
            )
        })?;
        if let Err(error) = self.create_session_locked(
            ovrcr_protocol::CreateSessionRequest {
                project,
                workspace: name.clone(),
                name: "local".into(),
                label: None,
                argv: vec![shell],
            },
            None,
        ) {
            return Err(lifecycle_error_with_hierarchy(
                ErrorCode::PartialFailure,
                format!(
                    "worktree for workspace {} remains at {}: {}",
                    name,
                    workspace.path.display(),
                    error_chain_string(&error)
                ),
                true,
            ));
        }
        Ok(())
    }

    pub fn remove_workspace(&self, project: &str, name: &str) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        self.remove_workspace_locked(project, name, None)
    }

    pub(crate) fn remove_task_workspace(
        &self,
        snapshot: &crate::tasks::Run,
        run_dir: &Path,
    ) -> Result<()> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        // Another cleanup may have consumed ownership while this request waited for the lock.
        let run = if run_dir.join("run.json").exists() {
            crate::tasks::read_run(run_dir)?
        } else {
            snapshot.clone()
        };
        if !run.status.is_terminal() {
            bail!("run is still active");
        }
        let Some(cwd) = &run.directory else {
            return Ok(());
        };
        let crate::tasks::TaskTarget::Git { project, .. } = &run.spec.target else {
            bail!("run does not own a Git workspace");
        };
        if !cwd.exists() {
            if run.workspace.is_some() {
                let mut cleaned = run;
                cleaned.workspace = None;
                crate::tasks::write_run(run_dir, &cleaned)?;
            }
            return Ok(());
        }
        let name = run
            .workspace
            .as_deref()
            .context("retained worktree is not owned by this run; inspect it before cleanup")?;
        self.registry
            .lock()
            .unwrap()
            .workspace(project, name)
            .context("retained worktree is not registered; inspect it before cleanup")?;
        self.remove_workspace_locked(project, name, Some((&run, run_dir)))
    }

    fn remove_workspace_locked(
        &self,
        project: &str,
        name: &str,
        cleanup: Option<(&crate::tasks::Run, &Path)>,
    ) -> Result<()> {
        if let Some(tasks) = &self.tasks
            && tasks.occupies_workspace(project, name)?
        {
            return Err(lifecycle_error(
                ErrorCode::SessionsRemain,
                format!("task run remains for workspace {project}/{name}"),
            ));
        }
        let occupied = self.sessions.lock().unwrap().values().any(|session| {
            let summary = session.summary();
            summary.project == project && summary.workspace == name
        });
        if occupied {
            return Err(lifecycle_error(
                ErrorCode::SessionsRemain,
                format!("sessions remain for workspace {project}/{name}"),
            ));
        }
        let (project_record, workspace) = {
            let registry = self.registry.lock().unwrap();
            (
                registry
                    .project(project)
                    .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
                    .clone(),
                registry
                    .workspace(project, name)
                    .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?
                    .clone(),
            )
        };
        if git::inspect_worktree(&project_record, &workspace)
            .map_err(|error| lifecycle_error(ErrorCode::Conflict, error.to_string()))?
            .dirty
        {
            return Err(lifecycle_error(
                ErrorCode::DirtyWorktree,
                "worktree has changes",
            ));
        }
        if let Some((run, _)) = cleanup
            && (name != format!("task-{}-run-{}", run.task_id.0, run.id.0)
                || run.directory.as_ref() != Some(&workspace.path)
                || workspace.branch != format!("ovrcr/task-{}/run-{}", run.task_id.0, run.id.0))
        {
            bail!("registered worktree does not match this run's ownership");
        }
        if let Some((run, run_dir)) = cleanup {
            // Release ownership durably before deletion, so a crash or a reused name cannot
            // let the old run authorize removal of a later workspace at the same path.
            let mut cleaned = run.clone();
            cleaned.workspace = None;
            crate::tasks::write_run(run_dir, &cleaned)?;
        } else if let Some(tasks) = &self.tasks {
            // Ordinary workspace removal also consumes any historical run's ownership.
            tasks.release_workspace_ownership(project, &workspace)?;
        }
        git::remove_worktree(&project_record, &workspace).with_context(|| {
            if cleanup.is_some() {
                format!(
                    "cleanup ownership released; inspect retained worktree {} before manual cleanup",
                    workspace.path.display()
                )
            } else {
                format!("remove workspace {project}/{name}")
            }
        })?;
        let mut registry = self.registry.lock().unwrap();
        let mut next = registry.clone();
        next.remove_workspace(project, name)?;
        if let Err(error) = save_registry_atomic(&next, &self.registry_path) {
            *registry = next;
            return Err(lifecycle_error_with_hierarchy(
                ErrorCode::PartialFailure,
                format!(
                    "worktree was removed at {} but registry update failed; live state reflects removal: {}",
                    workspace.path.display(),
                    error_chain_string(&error)
                ),
                true,
            ));
        }
        *registry = next;
        Ok(())
    }
}
fn snapshot(state: &Arc<ServerState>) -> HierarchySnapshot {
    snapshot_from_state(state)
}

fn snapshot_from_state(state: &ServerState) -> HierarchySnapshot {
    let registry = state.registry.lock().unwrap().clone();
    let sessions = state.sessions.lock().unwrap();
    let mut projects = registry
        .projects
        .into_iter()
        .map(|project| {
            let mut workspaces = project
                .workspaces
                .into_iter()
                .map(|workspace| WorkspaceSummary {
                    project: project.name.clone(),
                    name: workspace.name,
                    path: workspace.path,
                    sessions: Vec::new(),
                })
                .collect::<Vec<_>>();
            for workspace in &mut workspaces {
                workspace.sessions = sessions
                    .values()
                    .filter_map(|session| {
                        let summary = session.summary();
                        (summary.project == project.name && summary.workspace == workspace.name)
                            .then_some(summary)
                    })
                    .collect();
                workspace.sessions.sort_by(|left, right| {
                    (left.name != "local", left.id.0).cmp(&(right.name != "local", right.id.0))
                });
            }
            workspaces.sort_by(|left, right| left.name.cmp(&right.name));
            ProjectSummary {
                name: project.name,
                workspaces,
            }
        })
        .collect::<Vec<_>>();
    projects.sort_by(|left, right| left.name.cmp(&right.name));
    HierarchySnapshot { projects }
}

#[cfg(test)]
mod tests;

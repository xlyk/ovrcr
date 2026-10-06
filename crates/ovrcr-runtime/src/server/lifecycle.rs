//! One-at-a-time Lifecycle jobs off the Dashboard connection reader (ADR 0010).

use super::*;
use ovrcr_protocol::{LifecycleOp, LifecycleOutcome, SessionLaunch};

pub(super) enum JobKind {
    CreateWorkspace {
        project: String,
        id: String,
        branch: BranchRequest,
    },
    CreateWorkspaceWithLaunch {
        project: String,
        id: String,
        branch: BranchRequest,
        launch: Option<SessionLaunch>,
    },
    RemoveWorkspace {
        project: String,
        name: String,
        force: bool,
    },
    CreateSession(ovrcr_protocol::CreateSessionRequest),
    CloseTerminal {
        session: SessionId,
        expected_run: SessionRunId,
    },
}

impl JobKind {
    pub(super) fn op(&self) -> LifecycleOp {
        match self {
            Self::CreateWorkspace { .. } => LifecycleOp::CreateWorkspace,
            Self::CreateWorkspaceWithLaunch { .. } => LifecycleOp::CreateWorkspaceWithLaunch,
            Self::RemoveWorkspace { .. } => LifecycleOp::RemoveWorkspace,
            Self::CreateSession(_) => LifecycleOp::CreateSession,
            Self::CloseTerminal { .. } => LifecycleOp::CloseTerminal,
        }
    }
}

pub(super) struct Job {
    pub client_token: u64,
    pub kind: JobKind,
}

/// Accepts at most one Lifecycle job; the worker runs accepted work.
pub(crate) struct Lifecycle {
    inflight: Mutex<Option<u64>>,
    tx: Mutex<Option<SyncSender<Job>>>,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            inflight: Mutex::new(None),
            tx: Mutex::new(None),
        }
    }
}

impl Lifecycle {
    pub(super) fn attach(&self, tx: SyncSender<Job>) {
        *self.tx.lock().unwrap() = Some(tx);
    }

    pub(super) fn detach(&self) {
        *self.tx.lock().unwrap() = None;
    }

    #[cfg(test)]
    pub(super) fn inflight_token(&self) -> Option<u64> {
        *self.inflight.lock().unwrap()
    }

    /// True when a Dashboard Lifecycle job can be queued (worker attached).
    pub(super) fn async_ready(&self) -> bool {
        self.tx.lock().unwrap().is_some()
    }
}

pub(super) fn try_accept(state: &ServerState, client_token: u64, kind: JobKind) -> Response {
    let tx = {
        let guard = state.lifecycle.tx.lock().unwrap();
        match guard.as_ref() {
            Some(tx) => tx.clone(),
            None => {
                return error_response(
                    ErrorCode::Internal,
                    "lifecycle worker is not attached",
                );
            }
        }
    };
    {
        let mut slot = state.lifecycle.inflight.lock().unwrap();
        if slot.is_some() {
            return error_response(
                ErrorCode::Conflict,
                "a lifecycle job is already running",
            );
        }
        *slot = Some(client_token);
    }
    match tx.try_send(Job {
        client_token,
        kind,
    }) {
        Ok(()) => Response::Ok,
        Err(_) => {
            *state.lifecycle.inflight.lock().unwrap() = None;
            error_response(ErrorCode::Internal, "lifecycle worker is unavailable")
        }
    }
}

fn clear_inflight(state: &ServerState, client_token: u64) {
    let mut slot = state.lifecycle.inflight.lock().unwrap();
    if *slot == Some(client_token) {
        *slot = None;
    }
}

fn publish_completed(
    state: &ServerState,
    client_token: u64,
    op: LifecycleOp,
    outcome: LifecycleOutcome,
    publish_hierarchy: bool,
) {
    if publish_hierarchy {
        state
            .dashboard
            .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                state.hierarchy(),
            )));
    }
    state
        .dashboard
        .try_send(ServerMessage::Event(ServerEvent::LifecycleCompleted {
            client_token,
            op,
            outcome,
        }));
}

fn outcome_from_error(error: anyhow::Error) -> (LifecycleOutcome, bool) {
    let publish_hierarchy = error
        .downcast_ref::<LifecycleFailure>()
        .is_some_and(|failure| failure.publish_hierarchy);
    let code = super::connections::lifecycle_code(&error);
    let message = error_chain_string(&error);
    (
        LifecycleOutcome::Failed { code, message },
        publish_hierarchy,
    )
}

/// Runs accepted Lifecycle jobs until the channel closes or the server shuts down.
pub(super) fn run(state: Arc<ServerState>, rx: std::sync::mpsc::Receiver<Job>) {
    while let Ok(job) = rx.recv() {
        if state.shutdown.load(Ordering::Acquire) {
            clear_inflight(&state, job.client_token);
            break;
        }
        let op = job.kind.op();
        let (outcome, publish_hierarchy) = match job.kind {
            JobKind::CreateWorkspace {
                project,
                id,
                branch,
            } => match state.create_workspace(project, id, branch) {
                Ok(()) => (LifecycleOutcome::Succeeded, true),
                Err(error) => outcome_from_error(error),
            },
            JobKind::CreateWorkspaceWithLaunch {
                project,
                id,
                branch,
                launch,
            } => match state.create_workspace_with_launch(project, id, branch, launch) {
                Ok(None) => (LifecycleOutcome::Succeeded, true),
                Ok(Some(summary)) => (
                    LifecycleOutcome::CreatedSession(Box::new(summary)),
                    true,
                ),
                Err(error) => outcome_from_error(error),
            },
            JobKind::RemoveWorkspace {
                project,
                name,
                force,
            } => match state.remove_workspace(&project, &name, force) {
                Ok(()) => (LifecycleOutcome::Succeeded, true),
                Err(error) => outcome_from_error(error),
            },
            JobKind::CreateSession(request) => match state.create_session(request) {
                Ok(summary) => (
                    LifecycleOutcome::CreatedSession(Box::new(summary)),
                    true,
                ),
                Err(error) => outcome_from_error(error),
            },
            JobKind::CloseTerminal {
                session,
                expected_run,
            } => match state.close_terminal(session, expected_run, requested_kill_grace()) {
                Ok(()) => (LifecycleOutcome::Succeeded, true),
                Err(error) => outcome_from_error(error),
            },
        };
        publish_completed(&state, job.client_token, op, outcome, publish_hierarchy);
        clear_inflight(&state, job.client_token);
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{CreateSessionRequest, SessionKind};
    use std::ffi::OsString;
    use std::sync::mpsc;
    use std::time::Duration;

    fn attach_worker(state: &Arc<ServerState>) -> std::thread::JoinHandle<()> {
        let (tx, rx) = mpsc::sync_channel(4);
        state.lifecycle.attach(tx);
        let worker_state = Arc::clone(state);
        std::thread::spawn(move || run(worker_state, rx))
    }

    #[test]
    fn dashboard_create_session_accepts_immediately_and_refuses_a_second_job() {
        let state = crate::server::tests::test_state(None, None);
        let _worker = attach_worker(&state);

        // Hold mutation_lock so the accepted job cannot finish before we refuse a second.
        let held = state.mutation_lock.lock().unwrap();

        let first = handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            Request::CreateSession(CreateSessionRequest {
                project: "p".into(),
                workspace: "w".into(),
                name: "s".into(),
                label: None,
                argv: vec![OsString::from("/bin/true")],
                kind: SessionKind::Terminal,
            }),
            42,
            None,
        );
        assert_eq!(first, Response::Ok);
        assert_eq!(state.lifecycle.inflight_token(), Some(42));

        let second = handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            Request::CloseTerminal {
                session: SessionId(1),
                expected_run: SessionRunId(1),
            },
            43,
            None,
        );
        assert_eq!(
            second,
            Response::Error {
                code: ErrorCode::Conflict,
                message: "a lifecycle job is already running".into(),
            }
        );

        drop(held);
        // Allow the worker to fail the create (no project) and clear the slot.
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while state.lifecycle.inflight_token().is_some() {
            assert!(std::time::Instant::now() < deadline, "job did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
        state.lifecycle.detach();
    }

    #[test]
    fn control_create_session_stays_synchronous() {
        let state = crate::server::tests::test_state(None, None);
        let _worker = attach_worker(&state);
        let response = handle_request_with_id(
            &state,
            &mut ClientRole::Control,
            Request::CreateSession(CreateSessionRequest {
                project: "missing".into(),
                workspace: "w".into(),
                name: "s".into(),
                label: None,
                argv: vec![OsString::from("/bin/true")],
                kind: SessionKind::Terminal,
            }),
            1,
            None,
        );
        assert!(
            matches!(response, Response::Error { .. }),
            "control path must run sync and return the create error: {response:?}"
        );
        assert!(state.lifecycle.inflight_token().is_none());
        state.lifecycle.detach();
    }
}

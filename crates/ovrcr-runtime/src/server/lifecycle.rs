//! One-at-a-time Lifecycle jobs off the Dashboard connection reader (ADR 0010).

use super::*;
use ovrcr_protocol::{
    InitialPromptDelivery, InitialPromptIntentId, InitialPromptIntentSnapshot,
    InitialPromptIntentState, InitialPromptLaunchRequest, LifecycleOp, LifecycleOutcome,
    SessionLaunch,
};

const MAX_PROMPT_INTENT_RECORDS: usize = 16;

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
    CreateAgentSession(ovrcr_protocol::CreateAgentSessionRequest),
    CreateAgentSessionWithPrompt(InitialPromptIntentId),
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
            Self::CreateSession(_)
            | Self::CreateAgentSession(_)
            | Self::CreateAgentSessionWithPrompt(_) => LifecycleOp::CreateSession,
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
    prompt_intents: Mutex<PromptIntentBook>,
}

#[derive(Default)]
struct PromptIntentBook {
    next_sequence: u64,
    latest_issued: u64,
    consumed_through: u64,
    records: HashMap<u64, PromptIntentRecord>,
}

struct PromptIntentRecord {
    snapshot: InitialPromptIntentSnapshot,
    launch: Option<ovrcr_protocol::CreateAgentSessionRequest>,
    prompt: Option<String>,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self {
            inflight: Mutex::new(None),
            tx: Mutex::new(None),
            prompt_intents: Mutex::new(PromptIntentBook::default()),
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

    pub(super) fn allocate_prompt_intent(
        &self,
        server_lifetime: &str,
    ) -> Result<InitialPromptIntentId, Box<Response>> {
        let mut book = self.prompt_intents.lock().unwrap();
        retire_prompt_intents(&mut book);
        if book.records.len() >= MAX_PROMPT_INTENT_RECORDS {
            return Err(Box::new(error_response(
                ErrorCode::Conflict,
                "too many initial-prompt launches are still pending reconciliation",
            )));
        }
        let sequence = book.next_sequence.checked_add(1).ok_or_else(|| {
            Box::new(error_response(
                ErrorCode::Internal,
                "initial-prompt identity space is exhausted",
            ))
        })?;
        book.next_sequence = sequence;
        book.latest_issued = sequence;
        let id = InitialPromptIntentId {
            server_lifetime: server_lifetime.to_owned(),
            sequence,
        };
        book.records.insert(
            sequence,
            PromptIntentRecord {
                snapshot: empty_prompt_intent(id.clone(), InitialPromptIntentState::Issued),
                launch: None,
                prompt: None,
            },
        );
        Ok(id)
    }

    pub(super) fn prompt_intents(&self) -> Vec<InitialPromptIntentSnapshot> {
        let book = self.prompt_intents.lock().unwrap();
        let mut records: Vec<_> = book
            .records
            .values()
            .map(|record| record.snapshot.clone())
            .collect();
        records.sort_by_key(|record| record.id.sequence);
        records
    }

    fn prompt_launch(&self, id: &InitialPromptIntentId) -> Option<InitialPromptLaunchRequest> {
        let book = self.prompt_intents.lock().unwrap();
        let record = book.records.get(&id.sequence)?;
        if record.snapshot.id != *id || record.snapshot.state != InitialPromptIntentState::Pending {
            return None;
        }
        Some(InitialPromptLaunchRequest {
            intent: id.clone(),
            launch: record.launch.clone()?,
            prompt: record.prompt.clone()?,
        })
    }

    /// Bind the intent as soon as `begin_run` allocates its exact generation.
    /// This is deliberately before the native spawn can become uncertain.
    pub(super) fn bind_prompt_intent(
        &self,
        id: &InitialPromptIntentId,
        session: SessionId,
        run: ovrcr_protocol::SessionRunId,
        name: &str,
        label: &str,
    ) -> anyhow::Result<()> {
        let mut book = self.prompt_intents.lock().unwrap();
        let record = book
            .records
            .get_mut(&id.sequence)
            .context("initial-prompt intent disappeared before session allocation")?;
        anyhow::ensure!(
            record.snapshot.id == *id && record.snapshot.state == InitialPromptIntentState::Pending,
            "initial-prompt intent changed before session allocation"
        );
        if let (Some(existing_session), Some(existing_run)) =
            (record.snapshot.session, record.snapshot.run)
        {
            anyhow::ensure!(
                existing_session == session && existing_run == run,
                "initial-prompt intent is already bound to another session run"
            );
        } else {
            anyhow::ensure!(
                record.snapshot.session.is_none() && record.snapshot.run.is_none(),
                "initial-prompt intent has a partial session binding"
            );
        }
        record.snapshot.session = Some(session);
        record.snapshot.run = Some(run);
        record.snapshot.name = name.to_owned();
        record.snapshot.label = Some(label.to_owned());
        Ok(())
    }

    fn finish_prompt_intent(
        &self,
        id: &InitialPromptIntentId,
        outcome: &Result<SessionSummary>,
        prompt: &str,
    ) -> Option<InitialPromptIntentSnapshot> {
        let mut book = self.prompt_intents.lock().unwrap();
        let record = book.records.get_mut(&id.sequence)?;
        if record.snapshot.id != *id || record.snapshot.state != InitialPromptIntentState::Pending {
            return None;
        }
        let launch = record.launch.take();
        match outcome {
            Ok(session) => {
                record.prompt = None;
                record.snapshot.name = session.name.clone();
                record.snapshot.label = Some(session.label.clone());
                record.snapshot.state = InitialPromptIntentState::LaunchAccepted;
                record.snapshot.session = Some(session.id);
                record.snapshot.run = Some(session.run);
                record.snapshot.delivery = Some(InitialPromptDelivery::Unknown);
                record.snapshot.recoverable_prompt = None;
                record.snapshot.recoverable_launch = None;
                record.snapshot.message = None;
            }
            Err(error) if error.is::<crate::session::NoProcessStarted>() => {
                record.snapshot.state = InitialPromptIntentState::Refused;
                record.snapshot.recoverable_prompt =
                    (!prompt.trim().is_empty()).then(|| prompt.to_owned());
                record.snapshot.recoverable_launch = launch;
                record.snapshot.message = Some(redact_prompt(error_chain_string(error), prompt));
                record.prompt = None;
            }
            Err(error) => {
                record.prompt = None;
                record.snapshot.state = InitialPromptIntentState::Unknown;
                record.snapshot.recoverable_prompt = None;
                record.snapshot.recoverable_launch = None;
                record.snapshot.message = Some(redact_prompt(error_chain_string(error), prompt));
            }
        }
        Some(record.snapshot.clone())
    }
}

fn empty_prompt_intent(
    id: InitialPromptIntentId,
    state: InitialPromptIntentState,
) -> InitialPromptIntentSnapshot {
    InitialPromptIntentSnapshot {
        id,
        state,
        project: String::new(),
        workspace: String::new(),
        name: String::new(),
        label: None,
        session: None,
        run: None,
        recoverable_prompt: None,
        recoverable_launch: None,
        message: None,
        delivery: None,
    }
}

fn retire_prompt_intents(book: &mut PromptIntentBook) {
    while book.records.len() >= MAX_PROMPT_INTENT_RECORDS {
        let retire = book
            .records
            .iter()
            .filter(|(sequence, record)| {
                record.snapshot.state != InitialPromptIntentState::Pending
                    && !(record.snapshot.state == InitialPromptIntentState::Issued
                        && **sequence == book.latest_issued)
            })
            .min_by_key(|(sequence, _)| **sequence)
            .map(|(sequence, _)| *sequence);
        let Some(sequence) = retire else {
            break;
        };
        book.records.remove(&sequence);
    }
}

fn redact_prompt(mut message: String, prompt: &str) -> String {
    if !prompt.is_empty() {
        message = message.replace(prompt, "[redacted]");
    }
    message
}

pub(super) fn try_accept(state: &ServerState, client_token: u64, kind: JobKind) -> Response {
    let tx = {
        let guard = state.lifecycle.tx.lock().unwrap();
        match guard.as_ref() {
            Some(tx) => tx.clone(),
            None => {
                return error_response(ErrorCode::Internal, "lifecycle worker is not attached");
            }
        }
    };
    {
        let mut slot = state.lifecycle.inflight.lock().unwrap();
        if slot.is_some() {
            return error_response(ErrorCode::Conflict, "a lifecycle job is already running");
        }
        *slot = Some(client_token);
    }
    match tx.try_send(Job { client_token, kind }) {
        Ok(()) => Response::Ok,
        Err(_) => {
            *state.lifecycle.inflight.lock().unwrap() = None;
            error_response(ErrorCode::Internal, "lifecycle worker is unavailable")
        }
    }
}

pub(super) fn try_accept_prompt(
    state: &ServerState,
    client_token: u64,
    request: InitialPromptLaunchRequest,
) -> Response {
    if let Err(message) = request.validate() {
        return error_response(ErrorCode::InvalidRequest, message);
    }
    let mut book = state.lifecycle.prompt_intents.lock().unwrap();
    if request.intent.server_lifetime != state.server_lifetime {
        return Response::InitialPromptIntents(vec![empty_prompt_intent(
            request.intent,
            InitialPromptIntentState::Unknown,
        )]);
    }
    if request.intent.sequence <= book.consumed_through {
        let snapshot = book
            .records
            .get(&request.intent.sequence)
            .filter(|record| record.snapshot.id == request.intent)
            .map(|record| record.snapshot.clone())
            .unwrap_or_else(|| {
                empty_prompt_intent(request.intent, InitialPromptIntentState::Unknown)
            });
        return Response::InitialPromptIntents(vec![snapshot]);
    }
    if request.intent.sequence != book.latest_issued
        || !book
            .records
            .get(&request.intent.sequence)
            .is_some_and(|record| {
                record.snapshot.id == request.intent
                    && record.snapshot.state == InitialPromptIntentState::Issued
            })
    {
        return Response::InitialPromptIntents(vec![empty_prompt_intent(
            request.intent,
            InitialPromptIntentState::Unknown,
        )]);
    }
    let tx = {
        let guard = state.lifecycle.tx.lock().unwrap();
        match guard.as_ref() {
            Some(tx) => tx.clone(),
            None => {
                return error_response(ErrorCode::Internal, "lifecycle worker is not attached");
            }
        }
    };
    let mut inflight = state.lifecycle.inflight.lock().unwrap();
    if inflight.is_some() {
        return error_response(ErrorCode::Conflict, "a lifecycle job is already running");
    }

    let previous_consumed = book.consumed_through;
    let record = book.records.get_mut(&request.intent.sequence).unwrap();
    record.snapshot.state = InitialPromptIntentState::Pending;
    record
        .snapshot
        .project
        .clone_from(&request.launch.session.project);
    record
        .snapshot
        .workspace
        .clone_from(&request.launch.session.workspace);
    record
        .snapshot
        .name
        .clone_from(&request.launch.session.name);
    record
        .snapshot
        .label
        .clone_from(&request.launch.session.label);
    record.launch = Some(request.launch);
    record.prompt = Some(request.prompt);
    book.consumed_through = request.intent.sequence;
    *inflight = Some(client_token);
    match tx.try_send(Job {
        client_token,
        kind: JobKind::CreateAgentSessionWithPrompt(request.intent.clone()),
    }) {
        Ok(()) => Response::Ok,
        Err(_) => {
            *inflight = None;
            book.consumed_through = previous_consumed;
            if let Some(record) = book.records.get_mut(&request.intent.sequence) {
                record.snapshot.state = InitialPromptIntentState::Issued;
                record.snapshot.project.clear();
                record.snapshot.workspace.clear();
                record.snapshot.name.clear();
                record.snapshot.label = None;
                record.launch = None;
                record.prompt = None;
            }
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
                Ok(Some(summary)) => (LifecycleOutcome::CreatedSession(Box::new(summary)), true),
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
                Ok(summary) => (LifecycleOutcome::CreatedSession(Box::new(summary)), true),
                Err(error) => outcome_from_error(error),
            },
            JobKind::CreateAgentSession(request) => match state.create_agent_session(request) {
                Ok(summary) => (LifecycleOutcome::CreatedSession(Box::new(summary)), true),
                Err(error) => outcome_from_error(error),
            },
            JobKind::CreateAgentSessionWithPrompt(intent) => {
                let request = state.lifecycle.prompt_launch(&intent);
                let prompt = request.as_ref().map(|request| request.prompt.clone());
                let launch = request.as_ref().map(|request| request.launch.clone());
                let result = match request {
                    Some(request) => state.create_agent_session_with_prompt(request),
                    None => Err(crate::session::NoProcessStarted::context(anyhow::anyhow!(
                        "accepted initial-prompt intent is unavailable"
                    ))),
                };
                if result.is_ok()
                    && let Some(launch) = launch.as_ref()
                {
                    state.remember_initial_prompt_launch(launch);
                }
                let snapshot = state.lifecycle.finish_prompt_intent(
                    &intent,
                    &result,
                    prompt.as_deref().unwrap_or(""),
                );
                let (_outcome, publish_hierarchy) = match result {
                    Ok(summary) => (LifecycleOutcome::CreatedSession(Box::new(summary)), true),
                    Err(error) => {
                        let (mut outcome, publish_hierarchy) = outcome_from_error(error);
                        if let (LifecycleOutcome::Failed { message, .. }, Some(prompt)) =
                            (&mut outcome, prompt.as_deref())
                        {
                            *message = redact_prompt(std::mem::take(message), prompt);
                        }
                        (outcome, publish_hierarchy)
                    }
                };
                clear_inflight(&state, job.client_token);
                if publish_hierarchy {
                    state
                        .dashboard
                        .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                            state.hierarchy(),
                        )));
                }
                if let Some(snapshot) = snapshot {
                    state.dashboard.try_send(ServerMessage::Event(
                        ServerEvent::InitialPromptIntentChanged(Box::new(snapshot)),
                    ));
                }
                // The stable intent event owns completion for this launch. Do
                // not publish the connection-local client token: a replacement
                // Dashboard can reuse it for unrelated work.
                continue;
            }
            JobKind::CloseTerminal {
                session,
                expected_run,
            } => match state.close_terminal(session, expected_run, requested_kill_grace()) {
                Ok(()) => (LifecycleOutcome::Succeeded, true),
                Err(error) => outcome_from_error(error),
            },
        };
        // Free the slot before announcing completion: a Dashboard may answer the
        // completion with its next job (stop sessions, then remove a workspace),
        // and that job must not be refused as overlapping this finished one.
        clear_inflight(&state, job.client_token);
        publish_completed(&state, job.client_token, op, outcome, publish_hierarchy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{
        AgentModelPolicy, CreateAgentSessionRequest, CreateSessionRequest,
        InitialPromptLaunchRequest, ProjectRecord, SessionKind, WorkspaceRecord,
    };
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
    fn initial_prompt_run_binding_survives_unknown_and_duplicate_reconciles_without_spawn() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let socket = root.path().join("server.sock");
        let _listener = UnixListener::bind(&socket).unwrap();
        let registry = Registry {
            projects: vec![ProjectRecord {
                name: "p".into(),
                repo: root.path().to_path_buf(),
                workspace_root: root.path().to_path_buf(),
                workspaces: vec![WorkspaceRecord {
                    id: "w".into(),
                    path: workspace,
                    branch: "main".into(),
                    git_identity: None,
                    setup_pending: false,
                }],
            }],
        };
        let (state, _dispatch_receiver, _event_receiver) =
            crate::server::tests::test_state_with_socket(socket, registry);
        let dashboard_owner = Arc::new(());
        let (dashboard_server, _dashboard_client) = UnixStream::pair().unwrap();
        state.dashboard.claim_with_identity(
            DashboardSink::new(),
            Arc::clone(&dashboard_owner),
            dashboard_server,
        );
        let launched = root.path().join("launch-count");
        let prompt = "T392_BOUND_PROMPT_MUST_NOT_REPLAY";
        let launch = CreateAgentSessionRequest {
            session: CreateSessionRequest {
                project: "p".into(),
                workspace: "w".into(),
                name: "bound-agent".into(),
                label: Some("claude".into()),
                argv: vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    format!("printf x >> '{}'", launched.display()).into(),
                ],
                kind: SessionKind::Agent {
                    name: "claude".into(),
                },
            },
            model_policy: AgentModelPolicy::HarnessDefault,
            model_binding: None,
            expected_model: None,
        };
        let intent = state
            .lifecycle
            .allocate_prompt_intent(&state.server_lifetime)
            .unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        state.lifecycle.attach(tx);
        let request = InitialPromptLaunchRequest {
            intent: intent.clone(),
            launch,
            prompt: prompt.into(),
        };
        assert_eq!(try_accept_prompt(&state, 90, request.clone()), Response::Ok);
        let queued = rx.try_recv().unwrap();
        assert!(
            matches!(queued.kind, JobKind::CreateAgentSessionWithPrompt(ref id) if id == &intent)
        );
        let accepted = state.lifecycle.prompt_launch(&intent).unwrap();

        let (slot_held_tx, slot_held_rx) = mpsc::sync_channel(1);
        let (release_slot_tx, release_slot_rx) = mpsc::sync_channel(1);
        let slot_state = Arc::clone(&state);
        let slot_owner = Arc::clone(&dashboard_owner);
        let slot_thread = std::thread::spawn(move || {
            slot_state
                .dashboard
                .with_owned(&slot_owner, |_| {
                    slot_held_tx.send(()).unwrap();
                    release_slot_rx.recv().unwrap();
                })
                .expect("test Dashboard still owns its slot");
        });
        slot_held_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let (spawn_result_tx, spawn_result_rx) = mpsc::sync_channel(1);
        let spawn_state = Arc::clone(&state);
        let spawn_intent = intent.clone();
        let spawn_prompt = accepted.prompt.clone();
        let spawn_launch = accepted.launch.session.clone();
        let spawn_thread = std::thread::spawn(move || {
            let result = spawn_state
                .create_session_locked(
                    spawn_launch,
                    None,
                    SessionSpawnOptions {
                        initial_prompt: Some(spawn_prompt),
                        initial_prompt_intent: Some(spawn_intent),
                        ..SessionSpawnOptions::default()
                    },
                )
                .map(|summary| (summary.id, summary.run, summary.name, summary.label));
            spawn_result_tx.send(result).unwrap();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let pending = loop {
            let pending = state
                .lifecycle
                .prompt_intents()
                .into_iter()
                .find(|snapshot| snapshot.id == intent)
                .unwrap();
            if pending.session.is_some() {
                break pending;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "intent was not bound after run allocation"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(pending.state, InitialPromptIntentState::Pending);
        let session = pending.session.unwrap();
        let run = pending.run.unwrap();
        assert!(
            matches!(spawn_result_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "spawn must remain blocked before native launch while the slot is held"
        );
        assert!(
            !launched.exists(),
            "the native child must not start until after the bound snapshot"
        );
        release_slot_tx.send(()).unwrap();
        let (summary_session, summary_run, summary_name, summary_label) = spawn_result_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        slot_thread.join().unwrap();
        spawn_thread.join().unwrap();
        assert_eq!(summary_session, session);
        assert_eq!(summary_run, run);
        assert_eq!(pending.name, summary_name);
        assert_eq!(pending.label.as_deref(), Some(summary_label.as_str()));

        // A generic error represents an outcome that can no longer prove
        // whether the native process started. The exact allocation survives.
        let unknown = state
            .lifecycle
            .finish_prompt_intent(
                &intent,
                &Err(anyhow::anyhow!("spawn completion became uncertain")),
                prompt,
            )
            .unwrap();
        clear_inflight(&state, 90);
        assert_eq!(unknown.state, InitialPromptIntentState::Unknown);
        assert_eq!(unknown.session, Some(session));
        assert_eq!(unknown.run, Some(run));
        assert_eq!(unknown.recoverable_prompt, None);
        assert_eq!(unknown.recoverable_launch, None);

        let repeated = try_accept_prompt(&state, 91, request);
        assert_eq!(repeated, Response::InitialPromptIntents(vec![unknown]));
        assert_eq!(
            state.sessions.lock().unwrap().len(),
            1,
            "repeating an uncertain intent does not create another session"
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(output) = std::fs::read_to_string(&launched) {
                assert_eq!(output, "x");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the single accepted native launch did not create its marker"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
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
    fn model_query_and_create_reject_a_stale_dashboard_owner_without_effects() {
        let sink = DashboardSink::new();
        let (server_stream, _client) = UnixStream::pair().unwrap();
        let current_owner = Arc::new(());
        let state =
            crate::server::tests::test_state(Some(sink), Some((current_owner, server_stream)));
        let _worker = attach_worker(&state);
        let stale_owner = Arc::new(());

        let query = handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            Request::QueryAgentModels {
                preset: "claude".into(),
                origin: ovrcr_protocol::AgentLaunchOrigin::Detected,
                argv: vec!["/bin/true".into()],
                project: "p".into(),
                workspace: "w".into(),
            },
            50,
            Some(&stale_owner),
        );
        assert!(matches!(
            query,
            Response::Error {
                code: ErrorCode::InvalidRequest,
                ..
            }
        ));

        let unowned_query = handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            Request::QueryAgentModels {
                preset: "claude".into(),
                origin: ovrcr_protocol::AgentLaunchOrigin::Detected,
                argv: vec!["/bin/true".into()],
                project: "p".into(),
                workspace: "w".into(),
            },
            52,
            None,
        );
        assert!(matches!(
            unowned_query,
            Response::Error {
                code: ErrorCode::InvalidRequest,
                ..
            }
        ));

        let create = handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            Request::CreateAgentSession(ovrcr_protocol::CreateAgentSessionRequest {
                session: CreateSessionRequest {
                    project: "p".into(),
                    workspace: "w".into(),
                    name: "stale-owner".into(),
                    label: Some("claude".into()),
                    argv: vec!["/bin/true".into()],
                    kind: SessionKind::Agent {
                        name: "claude".into(),
                    },
                },
                model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
                model_binding: None,
                expected_model: None,
            }),
            51,
            Some(&stale_owner),
        );
        assert!(matches!(
            create,
            Response::Error {
                code: ErrorCode::InvalidRequest,
                ..
            }
        ));

        let unowned_create = handle_request_with_id(
            &state,
            &mut ClientRole::Dashboard,
            Request::CreateAgentSession(ovrcr_protocol::CreateAgentSessionRequest {
                session: CreateSessionRequest {
                    project: "p".into(),
                    workspace: "w".into(),
                    name: "unowned-session".into(),
                    label: Some("claude".into()),
                    argv: vec!["/bin/true".into()],
                    kind: SessionKind::Agent {
                        name: "claude".into(),
                    },
                },
                model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
                model_binding: None,
                expected_model: None,
            }),
            53,
            None,
        );
        assert!(matches!(
            unowned_create,
            Response::Error {
                code: ErrorCode::InvalidRequest,
                ..
            }
        ));
        assert_eq!(state.lifecycle.inflight_token(), None);
        assert!(state.sessions.lock().unwrap().is_empty());
        state.lifecycle.detach();
    }

    /// A Dashboard chains removal steps by answering each completion with the
    /// next job, so a completion must never be visible while its slot is taken.
    #[test]
    fn completion_is_published_only_after_the_slot_is_free() {
        let sink = DashboardSink::new();
        let (server_stream, _client) = UnixStream::pair().unwrap();
        let state = crate::server::tests::test_state(
            Some(Arc::clone(&sink)),
            Some((Arc::new(()), server_stream)),
        );
        let _worker = attach_worker(&state);
        let create = |name: &str| {
            Request::CreateSession(CreateSessionRequest {
                project: "missing".into(),
                workspace: "w".into(),
                name: name.into(),
                label: None,
                argv: vec![OsString::from("/bin/true")],
                kind: SessionKind::Terminal,
            })
        };
        let completed = |token: u64| {
            sink.queue.lock().unwrap().messages.iter().any(|outbound| {
                matches!(
                    outbound.message,
                    ServerMessage::Event(ServerEvent::LifecycleCompleted { client_token, .. })
                        if client_token == token
                )
            })
        };
        let wait = |token: u64| {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !completed(token) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "job {token} never completed"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        };

        let held = state.mutation_lock.lock().unwrap();
        let first =
            handle_request_with_id(&state, &mut ClientRole::Dashboard, create("a"), 42, None);
        assert_eq!(first, Response::Ok);
        // While this lock is held the finished job cannot release its slot.
        let slot = state.lifecycle.inflight.lock().unwrap();
        drop(held);
        let deadline = std::time::Instant::now() + Duration::from_millis(300);
        while std::time::Instant::now() < deadline {
            assert!(
                !completed(42),
                "completion was published while its slot was still taken"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(slot);
        wait(42);
        let next =
            handle_request_with_id(&state, &mut ClientRole::Dashboard, create("b"), 43, None);
        assert_eq!(next, Response::Ok, "a job sent in reply to a completion");
        wait(43);
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

//! Native conversation control on scoped connection threads, outside the dispatcher.
use super::*;
use ovrcr_protocol::SupervisorAuth;
use ovrcr_protocol::operator::*;
use std::collections::HashSet;
use std::ffi::OsString;

const SOFT_STOP_GRACE: Duration = Duration::from_secs(5);

pub(super) struct OperatorRuntime {
    pub(super) executable: PathBuf,
    controls: Mutex<Controls>,
    #[cfg(test)]
    pub(super) before_stop: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pub(super) before_retained: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}
#[derive(Default)]
struct Controls {
    conversations: HashSet<OperatorConversationId>,
    workers: Vec<JoinHandle<()>>,
    closed: bool,
    panicked: bool,
}
impl Controls {
    fn prune_finished(&mut self) {
        let mut index = 0;
        while index < self.workers.len() {
            if self.workers[index].is_finished() {
                // Finished means the control and its guards have already returned/unwound.
                self.panicked |= self.workers.swap_remove(index).join().is_err();
            } else {
                index += 1;
            }
        }
    }
}
impl OperatorRuntime {
    #[cfg(test)]
    pub(super) fn control_snapshot(&self) -> (usize, usize, bool) {
        let controls = self.controls.lock().unwrap();
        (
            controls.conversations.len(),
            controls.workers.len(),
            controls.workers.iter().all(JoinHandle::is_finished),
        )
    }
    pub(super) fn join_controls(&self) -> Result<()> {
        let (workers, mut panicked) = {
            let mut controls = self.controls.lock().unwrap();
            controls.closed = true;
            (std::mem::take(&mut controls.workers), controls.panicked)
        };
        for worker in workers {
            panicked |= worker.join().is_err();
        }
        if panicked {
            self.controls.lock().unwrap().panicked = true;
            bail!("operator control panicked");
        }
        Ok(())
    }
}
impl Default for OperatorRuntime {
    fn default() -> Self {
        Self {
            executable: std::env::current_exe().unwrap_or_default(),
            controls: Mutex::default(),
            #[cfg(test)]
            before_stop: Mutex::default(),
            #[cfg(test)]
            before_retained: Mutex::default(),
        }
    }
}

/// Same executable; only nonsecret identities cross argv. Payload uses authenticated Start.
pub fn operator_worker_argv(executable: &Path, identity: OperatorRunIdentity) -> Vec<OsString> {
    [
        executable.as_os_str().to_owned(),
        "operator".into(),
        "worker".into(),
        "--conversation".into(),
        identity.conversation.0.to_string().into(),
        "--session".into(),
        identity.session.0.to_string().into(),
        "--run".into(),
        identity.run.0.to_string().into(),
    ]
    .into()
}

struct ControlJob {
    state: Arc<ServerState>,
    conversation: OperatorConversationId,
}
impl Drop for ControlJob {
    fn drop(&mut self) {
        self.state
            .operator
            .controls
            .lock()
            .unwrap()
            .conversations
            .remove(&self.conversation);
    }
}

/// Keep submit/spawn and owned termination off the Dashboard reader. No global execution queue.
pub(super) fn dispatch_control(
    state: &Arc<ServerState>,
    owner: &Arc<()>,
    request_id: u64,
    request: OperatorRequest,
) -> Option<Response> {
    let conversation = match &request {
        OperatorRequest::Submit { conversation, .. } => *conversation,
        OperatorRequest::Stop { identity } => identity.conversation,
        _ => return None,
    };
    if !state.dashboard.owns(owner) {
        return Some(error_response(
            ErrorCode::Conflict,
            "dashboard owner changed",
        ));
    }
    let mut controls = state.operator.controls.lock().unwrap();
    controls.prune_finished();
    if controls.closed
        || controls.workers.len() >= MAX_LIVE_SESSIONS
        || !controls.conversations.insert(conversation)
    {
        return Some(error_response(
            ErrorCode::Conflict,
            "operator control is busy; retry explicitly",
        ));
    }
    let worker_state = state.clone();
    let owner = owner.clone();
    match thread::Builder::new()
        .name("ovrcr-operator-control".into())
        .spawn(move || {
            let _job = ControlJob {
                state: worker_state.clone(),
                conversation,
            };
            let response = worker_state
                .operator_request(&owner, request)
                .map_or_else(error_for_lifecycle, Response::Operator);
            worker_state
                .dashboard
                .send_owner(&owner, response_message(request_id, response));
        }) {
        Ok(worker) => {
            controls.workers.push(worker);
            Some(Response::Ok)
        }
        Err(error) => {
            controls.conversations.remove(&conversation);
            Some(error_response(ErrorCode::Internal, error.to_string()))
        }
    }
}

impl ServerState {
    pub(super) fn operator_changed(&self, conversation: OperatorConversationId) {
        self.dashboard
            .try_send(ServerMessage::Event(ServerEvent::OperatorChanged(
                conversation,
            )));
    }

    pub(super) fn operator_request(
        &self,
        owner: &Arc<()>,
        request: OperatorRequest,
    ) -> Result<OperatorResponse> {
        request
            .validate()
            .map_err(|error| lifecycle_error(ErrorCode::InvalidRequest, error))?;
        match request {
            OperatorRequest::Current => {
                let _mutation = self.mutation_lock.lock().unwrap();
                self.reject_if_stopping()?;
                #[cfg(test)]
                if let Some(hook) = self.operator.before_retained.lock().unwrap().clone() {
                    hook();
                }
                let current = {
                    let retained = self.retained.lock();
                    self.dashboard
                        .with_owned(owner, |_| retained.current_operator_conversation())
                        .context("dashboard owner changed")??
                };
                Ok(OperatorResponse::Current(
                    current
                        .map(|summary| self.publish_worker_live(summary))
                        .transpose()?,
                ))
            }
            OperatorRequest::Open => {
                let opened = {
                    let _mutation = self.mutation_lock.lock().unwrap();
                    self.reject_if_stopping()?;
                    let cwd = self.registry_path.join("operator-workdir");
                    fs::create_dir_all(&cwd)?;
                    fs::set_permissions(&cwd, fs::Permissions::from_mode(0o700))?;
                    #[cfg(test)]
                    if let Some(hook) = self.operator.before_retained.lock().unwrap().clone() {
                        hook();
                    }
                    let opened = {
                        let mut retained = self.retained.lock();
                        self.dashboard
                            .with_owned(owner, |_| retained.open_operator_conversation(&cwd))
                            .context("dashboard owner changed")??
                    };
                    self.publish_worker_live(opened)?
                };
                self.operator_changed(opened.id);
                Ok(OperatorResponse::Opened(opened))
            }
            OperatorRequest::Stop { identity } => {
                // Acceptance is fenced to the owner; subsequent stop proof belongs to that turn.
                #[cfg(test)]
                if let Some(hook) = self.operator.before_stop.lock().unwrap().clone() {
                    hook();
                }
                self.stop_operator_dashboard(identity, owner)
                    .map(OperatorResponse::Stopped)
            }
            OperatorRequest::Submit {
                conversation,
                expected_run,
                text,
                context,
            } => {
                let _mutation = self.mutation_lock.lock().unwrap();
                self.reject_if_stopping()?;
                let warm = self.operator_warm_dispatchable(conversation, expected_run)?;
                self.validate_operator_context(&context)?;
                // The accepted turn captures the current model and reasoning settings, so a
                // later settings edit cannot change what this turn asked the worker to use.
                let settings = self
                    .settings
                    .lock()
                    .unwrap()
                    .report
                    .settings
                    .operator
                    .clone();
                let turn = {
                    // Assigned human input also acquires retained before the Dashboard slot.
                    let mut retained = self.retained.lock();
                    self.dashboard
                        .with_owned(owner, |_| {
                            if warm {
                                retained.submit_operator_turn_warm(
                                    conversation,
                                    expected_run,
                                    &text,
                                    context,
                                    settings,
                                )
                            } else {
                                retained.submit_operator_turn(
                                    conversation,
                                    expected_run,
                                    &text,
                                    context,
                                    settings,
                                )
                            }
                        })
                        .context("dashboard owner changed")??
                };
                if warm {
                    self.operator_changed(conversation);
                    return Ok(OperatorResponse::Submitted(turn));
                }
                let result = self.spawn_prepared_record_locked(
                    turn.identity.session,
                    turn.identity.run,
                    operator_worker_argv(
                        &self.operator.executable,
                        OperatorRunIdentity {
                            conversation: turn.identity.conversation,
                            session: turn.identity.session,
                            run: turn.identity.run,
                        },
                    ),
                    SessionSpawnOptions {
                        prepared: true,
                        ..SessionSpawnOptions::default()
                    },
                );
                if let Err(error) = result {
                    let mut retained = self.retained.lock();
                    retained.apply_operator_batch(&OperatorEventBatch {
                        identity: turn.identity,
                        sequence: 1,
                        events: vec![OperatorWorkerEvent::Failed(
                            "Worker launch failed; inspect the retained session before retrying"
                                .into(),
                        )],
                    })?;
                    if error.is::<crate::session::NoProcessStarted>() {
                        retained.mark_stopped(turn.identity.session, turn.identity.run)?;
                    }
                    drop(retained);
                    self.operator_changed(conversation);
                    return Err(error);
                }
                self.operator_changed(conversation);
                Ok(OperatorResponse::Submitted(turn))
            }
            request => {
                let _mutation = self.mutation_lock.lock().unwrap();
                self.reject_if_stopping()?;
                #[cfg(test)]
                if let Some(hook) = self.operator.before_retained.lock().unwrap().clone() {
                    hook();
                }
                let result = {
                    let retained = self.retained.lock();
                    self.dashboard
                        .with_owned(owner, |_| -> Result<_> {
                            match request {
                                OperatorRequest::Read {
                                    conversation,
                                    after,
                                    limit,
                                } => retained.read_operator_history(conversation, after, limit),
                                _ => unreachable!(),
                            }
                        })
                        .context("dashboard owner changed")??
                };
                Ok(OperatorResponse::History(ovrcr_protocol::OperatorHistory {
                    conversation: self.publish_worker_live(result.conversation)?,
                    ..result
                }))
            }
        }
    }

    fn publish_worker_live(
        &self,
        mut summary: ovrcr_protocol::OperatorConversationSummary,
    ) -> Result<ovrcr_protocol::OperatorConversationSummary> {
        let leased = self.retained.lock().operator_worker_leased(summary.id)?;
        let active_turn = summary.current_turn.as_ref().is_some_and(|turn| {
            turn.state == OperatorTurnState::Running && turn.identity.run == summary.run
        });
        if !leased && !active_turn {
            summary.worker_live = false;
            return Ok(summary);
        }
        let session = self.sessions.lock().unwrap().get(&summary.session).cloned();
        summary.worker_live = session.is_some_and(|session| {
            session.run() == summary.run
                && session.is_live()
                && session.summary().scope
                    == (ovrcr_protocol::SessionScope::OperatorConversation {
                        conversation: summary.id,
                    })
        });
        Ok(summary)
    }

    fn operator_warm_dispatchable(
        &self,
        conversation: OperatorConversationId,
        run: SessionRunId,
    ) -> Result<bool> {
        // Submit's warm check is its first retained boundary. Keep the lock-order
        // seam here so the test observes waits before any Dashboard slot is held.
        #[cfg(test)]
        if let Some(hook) = self.operator.before_retained.lock().unwrap().clone() {
            hook();
        }
        let summary = self.retained.lock().operator_conversation(conversation)?;
        if summary.run != run
            || !self.retained.lock().operator_worker_leased(conversation)?
            || summary.current_turn.as_ref().is_none_or(|turn| {
                !matches!(
                    turn.state,
                    OperatorTurnState::Complete
                        | OperatorTurnState::Interrupted
                        | OperatorTurnState::Error
                ) || turn.identity.run != run
            })
        {
            return Ok(false);
        }
        let session = self.sessions.lock().unwrap().get(&summary.session).cloned();
        Ok(session.is_some_and(|session| {
            session.run() == run
                && session.is_live()
                && session.summary().scope
                    == (ovrcr_protocol::SessionScope::OperatorConversation { conversation })
        }))
    }

    fn validate_operator_context(&self, context: &OperatorContext) -> Result<()> {
        if let Some(session) = context.session {
            let current = self
                .session_summary(session)
                .context("captured session is unavailable")?;
            if Some(current.run) != context.run {
                bail!("captured session run changed");
            }
        }
        if let Some(id) = &context.workspace_id {
            let registry = self.registry.lock().unwrap();
            let project = context
                .project
                .as_ref()
                .context("captured workspace needs project")?;
            let workspace = registry
                .projects
                .iter()
                .find(|p| &p.name == project)
                .and_then(|p| p.workspaces.iter().find(|w| &w.id == id))
                .context("captured workspace changed")?;
            if context.cwd.as_ref() != Some(&workspace.path) {
                bail!("captured workspace directory changed");
            }
        }
        Ok(())
    }

    pub(super) fn operator_exchange(
        &self,
        exchange: OperatorExchange,
        owner: Option<&Arc<()>>,
    ) -> Result<OperatorExchangeResult> {
        let identity = exchange.batch.identity;
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&identity.session)
            .cloned()
            .context("operator worker unavailable")?;
        let mut retained = self.retained.lock();
        let receipt = session.with_operator_authority(&exchange.auth, owner, identity, || {
            retained.apply_operator_batch(&exchange.batch)
        })?;
        drop(retained);
        if !receipt.duplicate {
            self.operator_changed(identity.conversation);
        }
        Ok(receipt)
    }

    pub(super) fn operator_worker(
        &self,
        request: OperatorWorkerRequest,
        owner: Option<&Arc<()>>,
    ) -> Result<OperatorWorkerResult> {
        match request.command {
            OperatorWorkerCommand::Start { identity } => {
                let session = self
                    .sessions
                    .lock()
                    .unwrap()
                    .get(&identity.session)
                    .cloned()
                    .context("operator worker unavailable")?;
                let retained = self.retained.lock();
                session
                    .with_operator_run_authority(&request.auth, owner, identity, || {
                        retained.operator_worker_start_for_run(identity)
                    })
                    .map(|start| OperatorWorkerResult::Start(Box::new(start)))
            }
            OperatorWorkerCommand::StartTurn { identity } => {
                let session = self
                    .sessions
                    .lock()
                    .unwrap()
                    .get(&identity.session)
                    .cloned()
                    .context("operator worker unavailable")?;
                let retained = self.retained.lock();
                session
                    .with_operator_run_authority(&request.auth, owner, identity, || {
                        retained.operator_worker_next_turn(identity)
                    })
                    .map(|start| OperatorWorkerResult::StartTurn(start.map(Box::new)))
            }
            OperatorWorkerCommand::BeginAsyncTurn { identity, text } => {
                // The async wakeup and human Submit share this exact admission
                // lock. A winner durably owns the one-running-turn slot before
                // Pi is allowed to continue its native asynchronous prompt.
                let _mutation = self.mutation_lock.lock().unwrap();
                self.reject_if_stopping()?;
                let session = self
                    .sessions
                    .lock()
                    .unwrap()
                    .get(&identity.session)
                    .cloned()
                    .context("operator worker unavailable")?;
                let dispatchable =
                    self.operator_warm_dispatchable(identity.conversation, identity.run)?;
                let result = if dispatchable {
                    let mut retained = self.retained.lock();
                    session.with_operator_run_authority(&request.auth, owner, identity, || {
                        retained.begin_async_operator_turn(identity, &text)
                    })?
                } else {
                    // Still authenticate the exact current worker before a
                    // losing request receives a safe refusal.
                    session
                        .with_operator_run_authority(&request.auth, owner, identity, || Ok(None))?
                };
                if result.is_some() {
                    self.operator_changed(identity.conversation);
                }
                Ok(OperatorWorkerResult::BeginAsyncTurn(result.map(Box::new)))
            }
            OperatorWorkerCommand::AbortTurn { identity, leased } => {
                let _mutation = self.mutation_lock.lock().unwrap();
                self.reject_if_stopping()?;
                let session = self
                    .sessions
                    .lock()
                    .unwrap()
                    .get(&identity.session)
                    .cloned()
                    .context("operator worker unavailable")?;
                let mut retained = self.retained.lock();
                let current = retained.operator_conversation(identity.conversation)?;
                if current.current_turn.as_ref().map(|turn| turn.identity) != Some(identity) {
                    bail!("stale operator turn identity");
                }
                let abort_requested = session.operator_abort_requested(identity);
                session.with_operator_authority(
                    &request.auth,
                    owner,
                    identity,
                    || match current.current_turn.as_ref().map(|turn| turn.state) {
                        Some(OperatorTurnState::Running) if abort_requested => {
                            retained.interrupt_operator_turn(identity, leased)
                        }
                        Some(OperatorTurnState::Error) => {
                            retained.record_operator_turn_lease(identity, leased)?;
                            retained
                                .operator_conversation(identity.conversation)?
                                .current_turn
                                .context("operator turn missing")
                        }
                        Some(OperatorTurnState::Interrupted) => retained
                            .operator_conversation(identity.conversation)?
                            .current_turn
                            .context("operator turn missing"),
                        _ => bail!("operator turn abort was not requested"),
                    },
                )?;
                session.clear_operator_abort(identity);
                drop(retained);
                self.operator_changed(identity.conversation);
                Ok(OperatorWorkerResult::Aborted { identity })
            }
            OperatorWorkerCommand::Stop { identity } => {
                self.stop_operator(identity, Some((&request.auth, owner)), false, None)?;
                Ok(OperatorWorkerResult::Retired { identity })
            }
            OperatorWorkerCommand::Retire { identity } => {
                self.stop_operator(identity, Some((&request.auth, owner)), true, None)?;
                Ok(OperatorWorkerResult::Retired { identity })
            }
        }
    }

    fn stop_operator_dashboard(
        &self,
        identity: OperatorTurnIdentity,
        owner: &Arc<()>,
    ) -> Result<OperatorTurn> {
        let session = {
            let _mutation = self.mutation_lock.lock().unwrap();
            self.reject_if_stopping()?;
            let retained = self.retained.lock();
            retained.require_current_operator(identity.conversation)?;
            let conversation = retained.operator_conversation(identity.conversation)?;
            let turn = conversation.current_turn.context("operator turn missing")?;
            if turn.identity != identity {
                bail!("stale operator turn identity");
            }
            self.dashboard
                .with_owned(owner, |_| ())
                .context("dashboard owner changed")?;
            let session = self
                .sessions
                .lock()
                .unwrap()
                .get(&identity.session)
                .cloned();
            if conversation.worker_stopped
                || turn.state != OperatorTurnState::Running
                || !session.as_ref().is_some_and(|session| session.is_live())
            {
                None
            } else {
                let session = session.context("operator worker unavailable")?;
                if session.request_operator_abort(identity).is_err() {
                    None
                } else {
                    Some(session)
                }
            }
        };
        let Some(session) = session else {
            return self.stop_operator(identity, None, false, None);
        };
        self.operator_changed(identity.conversation);

        let deadline = Instant::now() + SOFT_STOP_GRACE;
        loop {
            let turn = self
                .retained
                .lock()
                .operator_conversation(identity.conversation)?
                .current_turn
                .context("operator turn missing")?;
            if turn.identity != identity || turn.state != OperatorTurnState::Running {
                return Ok(turn);
            }
            if !session.is_live() || Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        // The live worker did not confirm rollback inside the grace period.
        // Fall back to the same exact-run, owned process-group stop path.
        self.stop_operator(identity, None, false, None)
    }

    fn stop_operator(
        &self,
        identity: OperatorTurnIdentity,
        auth: Option<(&SupervisorAuth, Option<&Arc<()>>)>,
        retiring: bool,
        dashboard_owner: Option<&Arc<()>>,
    ) -> Result<OperatorTurn> {
        let (turn, target) = {
            let _mutation = self.mutation_lock.lock().unwrap();
            self.reject_if_stopping()?;
            let mut retained = self.retained.lock();
            retained.require_current_operator(identity.conversation)?;
            let conversation = retained.operator_conversation(identity.conversation)?;
            let turn = conversation.current_turn.context("operator turn missing")?;
            if turn.identity != identity {
                bail!("stale operator turn identity");
            }
            let session = self
                .sessions
                .lock()
                .unwrap()
                .get(&identity.session)
                .cloned();
            if let Some((auth, owner)) = auth {
                session
                    .as_ref()
                    .context("operator worker unavailable")?
                    .with_operator_authority(auth, owner, identity, || Ok(()))?;
                if retiring && turn.state == OperatorTurnState::Running {
                    bail!("running operator turn cannot retire; commit completion or use Stop");
                }
            }
            if conversation.worker_stopped {
                if let Some(owner) = dashboard_owner {
                    self.dashboard
                        .with_owned(owner, |_| ())
                        .context("dashboard owner changed")?;
                }
                return Ok(turn);
            }
            let turn = if let Some(owner) = dashboard_owner {
                self.dashboard
                    .with_owned(owner, |_| retained.interrupt_operator_turn(identity, false))
                    .context("dashboard owner changed")??
            } else {
                retained.interrupt_operator_turn(identity, false)?
            };
            if let Some(session) = &session {
                session.clear_operator_abort(identity);
                session.revoke_hook_capability();
            }
            (
                turn,
                SessionControlTarget {
                    run: identity.run,
                    session,
                    already_exited: false,
                },
            )
        };
        self.operator_changed(identity.conversation);
        // Termination must never execute under mutation, retained, Session, or dispatcher locks.
        let termination = match &target.session {
            Some(session) => session.terminate(requested_kill_grace()),
            None => Err(anyhow::Error::new(AlreadyExited)),
        };
        let _mutation = self.mutation_lock.lock().unwrap();
        self.ensure_control_target(identity.session, &target)?;
        self.finish_control_stop(identity.session, &target, termination)?;
        self.refresh_session_locked(identity.session)?;
        self.operator_changed(identity.conversation);
        Ok(turn)
    }
}

#[cfg(test)]
mod control_tests {
    use super::*;

    #[test]
    fn pruned_control_panic_remains_a_shutdown_failure_without_blocking_admission() {
        let (peer, stream) = UnixStream::pair().unwrap();
        let owner = Arc::new(());
        let state = crate::server::tests::test_state(
            Some(DashboardSink::new()),
            Some((owner.clone(), stream)),
        );
        let panicked = thread::spawn(|| panic!("owned control panic fixture"));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !panicked.is_finished() {
            assert!(
                Instant::now() < deadline,
                "fixture thread must finish before pruning"
            );
            thread::yield_now();
        }
        state
            .operator
            .controls
            .lock()
            .unwrap()
            .workers
            .push(panicked);
        // A new real control goes through normal admission/pruning, then refuses
        // invalid text before journal/process effects. Pruning must retain its predecessor's failure.
        assert!(matches!(
            dispatch_control(
                &state,
                &owner,
                1,
                OperatorRequest::Submit {
                    conversation: OperatorConversationId(1),
                    expected_run: SessionRunId(0),
                    text: String::new(),
                    context: OperatorContext::default()
                }
            ),
            Some(Response::Ok)
        ));
        assert!(
            state.operator.control_snapshot().1 <= 1,
            "finished handle must leave the bounded admission set"
        );
        let shutdown = state.operator.join_controls();
        assert!(
            shutdown.is_err(),
            "pruning must not erase a finished control panic before shutdown"
        );
        drop(peer);
    }
}

//! Native conversation control on scoped connection threads, outside the dispatcher.
use super::*;
use ovrcr_protocol::SupervisorAuth;
use ovrcr_protocol::operator::*;
use std::collections::HashSet;
use std::ffi::OsString;

const SOFT_STOP_GRACE: Duration = Duration::from_secs(5);
const OPERATOR_MODEL_CATALOG_LIMIT: usize = 512 * 1024;
const OPERATOR_MODEL_DIAGNOSTIC_LIMIT: usize = 16 * 1024;
const OPERATOR_MODEL_CATALOG_DEADLINE: Duration = Duration::from_secs(20);
const OPERATOR_MODEL_UNAVAILABLE_MSG: &str = "Operator model catalog is unavailable; run ovrcr operator setup or ovrcr operator models to check the dedicated Pi profile.";

pub(super) struct OperatorRuntime {
    pub(super) executable: PathBuf,
    controls: Mutex<Controls>,
    watches: Mutex<Option<(OperatorRunIdentity, OperatorWatchUpdate)>>,
    #[cfg(test)]
    pub(super) before_stop: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pub(super) before_retained: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    pub(super) models_deadline: Mutex<Option<Duration>>,
}
#[derive(Default)]
struct Controls {
    conversations: HashSet<OperatorConversationId>,
    models_in_flight: bool,
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
            watches: Mutex::default(),
            #[cfg(test)]
            before_stop: Mutex::default(),
            #[cfg(test)]
            before_retained: Mutex::default(),
            #[cfg(test)]
            models_deadline: Mutex::default(),
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOperatorModelCatalog {
    models: Vec<RawOperatorModelEntry>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOperatorModelEntry {
    provider: String,
    id: String,
    #[serde(default)]
    name: Option<String>,
}

fn parse_operator_model_catalog(stdout: &[u8]) -> Result<Vec<OperatorModelCatalogEntry>> {
    anyhow::ensure!(
        stdout.len() <= OPERATOR_MODEL_CATALOG_LIMIT,
        "operator model catalog output bound"
    );
    let raw: RawOperatorModelCatalog =
        serde_json::from_slice(stdout).context("parse operator model catalog json")?;
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for item in raw.models {
        let name = item
            .name
            .filter(|name| !name.is_empty() && name != &item.id);
        let entry = OperatorModelCatalogEntry {
            provider: item.provider,
            id: item.id,
            name,
        };
        entry.validate().map_err(anyhow::Error::msg)?;
        if seen.insert((entry.provider.clone(), entry.id.clone())) {
            entries.push(entry);
        }
    }
    validate_operator_model_catalog(&entries).map_err(anyhow::Error::msg)?;
    Ok(entries)
}

#[derive(Clone, Copy)]
enum ControlSlot {
    Conversation(OperatorConversationId),
    Models,
}

struct ControlJob {
    state: Arc<ServerState>,
    slot: ControlSlot,
}
impl Drop for ControlJob {
    fn drop(&mut self) {
        let mut controls = self.state.operator.controls.lock().unwrap();
        match self.slot {
            ControlSlot::Conversation(conversation) => {
                controls.conversations.remove(&conversation);
            }
            ControlSlot::Models => {
                controls.models_in_flight = false;
            }
        }
    }
}

/// Keep submit/spawn, owned termination, and catalog discovery off the Dashboard reader.
pub(super) fn dispatch_control(
    state: &Arc<ServerState>,
    owner: &Arc<()>,
    request_id: u64,
    request: OperatorRequest,
) -> Option<Response> {
    let slot = match &request {
        OperatorRequest::Submit { conversation, .. }
        | OperatorRequest::Compact { conversation, .. }
        | OperatorRequest::NavigateTree {
            conversation,
            summarize: Some(_),
            ..
        } => ControlSlot::Conversation(*conversation),
        OperatorRequest::Stop { identity } => ControlSlot::Conversation(identity.conversation),
        OperatorRequest::Models => ControlSlot::Models,
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
    let busy_message = match slot {
        ControlSlot::Conversation(_) => "operator control is busy; retry explicitly",
        ControlSlot::Models => "Operator model discovery is busy; retry in a moment.",
    };
    if controls.closed || controls.workers.len() >= MAX_LIVE_SESSIONS {
        return Some(error_response(ErrorCode::Conflict, busy_message));
    }
    match slot {
        ControlSlot::Conversation(conversation) => {
            if !controls.conversations.insert(conversation) {
                return Some(error_response(ErrorCode::Conflict, busy_message));
            }
        }
        ControlSlot::Models => {
            if controls.models_in_flight {
                return Some(error_response(ErrorCode::Conflict, busy_message));
            }
            controls.models_in_flight = true;
        }
    }
    let worker_state = state.clone();
    let owner = owner.clone();
    match thread::Builder::new()
        .name("ovrcr-operator-control".into())
        .spawn(move || {
            let _job = ControlJob {
                state: worker_state.clone(),
                slot,
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
            match slot {
                ControlSlot::Conversation(conversation) => {
                    controls.conversations.remove(&conversation);
                }
                ControlSlot::Models => {
                    controls.models_in_flight = false;
                }
            }
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
            OperatorRequest::Models => self.operator_models(owner).map(OperatorResponse::Models),
            OperatorRequest::Submit {
                skill,
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
                                    skill,
                                )
                            } else {
                                retained.submit_operator_turn(
                                    conversation,
                                    expected_run,
                                    &text,
                                    context,
                                    settings,
                                    skill,
                                )
                            }
                        })
                        .context("dashboard owner changed")??
                };
                self.finish_submitted_operator_turn(conversation, turn, warm)
            }
            OperatorRequest::Compact {
                conversation,
                expected_run,
                instructions,
            } => {
                let _mutation = self.mutation_lock.lock().unwrap();
                self.reject_if_stopping()?;
                let warm = self.operator_warm_dispatchable(conversation, expected_run)?;
                let settings = self
                    .settings
                    .lock()
                    .unwrap()
                    .report
                    .settings
                    .operator
                    .clone();
                let turn = {
                    let mut retained = self.retained.lock();
                    self.dashboard
                        .with_owned(owner, |_| {
                            if warm {
                                retained.submit_operator_compact_turn_warm(
                                    conversation,
                                    expected_run,
                                    instructions,
                                    settings,
                                )
                            } else {
                                retained.submit_operator_compact_turn(
                                    conversation,
                                    expected_run,
                                    instructions,
                                    settings,
                                )
                            }
                        })
                        .context("dashboard owner changed")??
                };
                self.finish_submitted_operator_turn(conversation, turn, warm)
            }
            OperatorRequest::NewSession {
                conversation,
                expected_run,
            } => {
                let (published, superseded) = {
                    let _mutation = self.mutation_lock.lock().unwrap();
                    self.reject_if_stopping()?;
                    #[cfg(test)]
                    if let Some(hook) = self.operator.before_retained.lock().unwrap().clone() {
                        hook();
                    }
                    self.ensure_operator_worker_not_live(
                        owner,
                        conversation,
                        "cannot start a new Operator session while an Operator worker or watched-session lease is active",
                    )?;
                    let (summary, superseded) = {
                        let mut retained = self.retained.lock();
                        self.dashboard
                            .with_owned(owner, |_| {
                                retained.reset_operator_session(conversation, expected_run)
                            })
                            .context("dashboard owner changed")??
                    };
                    (self.publish_worker_live(summary)?, superseded)
                };
                crate::operator_session_file::remove_superseded_checkpoints(
                    &self.registry_path,
                    &superseded,
                );
                self.operator_changed(published.id);
                Ok(OperatorResponse::Conversation(published))
            }
            OperatorRequest::SessionInfo { conversation } => {
                self.reject_if_stopping()?;
                #[cfg(test)]
                if let Some(hook) = self.operator.before_retained.lock().unwrap().clone() {
                    hook();
                }
                let settings = self
                    .settings
                    .lock()
                    .unwrap()
                    .report
                    .settings
                    .operator
                    .clone();
                let snapshot = {
                    let retained = self.retained.lock();
                    self.dashboard
                        .with_owned(owner, |_| retained.operator_epoch_snapshot(conversation))
                        .context("dashboard owner changed")??
                };
                let checkpoint = if let Some(turn) = snapshot.checkpoint_turn {
                    Some(
                        crate::operator_session_file::read_promoted_checkpoint_for_branch(
                            &self.registry_path,
                            conversation,
                            turn,
                            snapshot.pi_tree_target_id.as_deref(),
                        )?,
                    )
                } else {
                    None
                };
                let active_model = snapshot
                    .summary
                    .model
                    .clone()
                    .or_else(|| checkpoint.as_ref().and_then(|c| c.active_model.clone()));
                let configured_model = settings
                    .model
                    .as_deref()
                    .and_then(|raw| OperatorModel::parse(raw).ok());
                let capacity_tokens = crate::operator_session_file::resolve_context_window_capacity(
                    &self.registry_path,
                    snapshot.summary.model.as_ref(),
                    snapshot.context_window_tokens,
                    checkpoint.as_ref().and_then(|c| c.active_model.as_ref()),
                    configured_model.as_ref(),
                    checkpoint.as_ref().and_then(|c| c.inline_context_window),
                );
                let verified = {
                    let retained = self.retained.lock();
                    self.dashboard
                        .with_owned(owner, |_| retained.operator_epoch_snapshot(conversation))
                        .context("dashboard owner changed")??
                };
                if verified.summary.session_epoch != snapshot.summary.session_epoch
                    || verified.checkpoint_turn != snapshot.checkpoint_turn
                    || verified.pi_tree_target_id != snapshot.pi_tree_target_id
                    || verified.active_tip_turn != snapshot.active_tip_turn
                {
                    bail!("Operator session changed while reading checkpoint; retry /session.");
                }
                let (
                    pi_session_id,
                    user_messages,
                    assistant_messages,
                    tool_calls,
                    tool_results,
                    epoch_tool_calls,
                    epoch_tool_results,
                    total_messages,
                    epoch_total_messages,
                    tokens,
                    cost_usd_nanos,
                    used_tokens,
                ) = if let Some(checkpoint) = checkpoint {
                    (
                        Some(checkpoint.pi_session_id),
                        checkpoint.user_messages,
                        checkpoint.assistant_messages,
                        checkpoint.tool_calls,
                        checkpoint.tool_results,
                        checkpoint.epoch_tool_calls,
                        checkpoint.epoch_tool_results,
                        checkpoint.total_messages,
                        checkpoint.epoch_total_messages,
                        checkpoint.tokens,
                        checkpoint.cost_usd_nanos,
                        checkpoint.latest_context_tokens,
                    )
                } else {
                    (
                        None,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        0,
                        OperatorTokenStats::default(),
                        0,
                        None,
                    )
                };
                Ok(OperatorResponse::SessionInfo(Box::new(
                    OperatorSessionInfo {
                        conversation: verified.summary.id,
                        session: verified.summary.session,
                        run: verified.summary.run,
                        session_epoch: verified.summary.session_epoch,
                        pi_session_id,
                        checkpoint_turn: verified.checkpoint_turn,
                        active_tip_turn: verified.active_tip_turn,
                        configured_model: settings.model,
                        active_model,
                        configured_thinking_level: settings.thinking_level,
                        reasoning: verified.summary.reasoning,
                        turns: verified.turns,
                        completed_turns: verified.completed_turns,
                        branch_turns: verified.branch_turns,
                        user_messages,
                        assistant_messages,
                        tool_calls,
                        tool_results,
                        epoch_tool_calls,
                        epoch_tool_results,
                        total_messages,
                        epoch_total_messages,
                        tokens,
                        cost_usd_nanos,
                        context_window: OperatorContextWindowUsage {
                            used_tokens,
                            capacity_tokens,
                        },
                    },
                )))
            }
            OperatorRequest::Tree { conversation } => {
                self.reject_if_stopping()?;
                #[cfg(test)]
                if let Some(hook) = self.operator.before_retained.lock().unwrap().clone() {
                    hook();
                }
                let snapshot = {
                    let retained = self.retained.lock();
                    self.dashboard
                        .with_owned(owner, |_| retained.operator_tree_snapshot(conversation))
                        .context("dashboard owner changed")??
                };
                let Some(turn) = snapshot.checkpoint_turn else {
                    return Ok(OperatorResponse::Tree(Box::default()));
                };
                let tree = crate::operator_session_file::read_session_tree(
                    &self.registry_path,
                    conversation,
                    turn,
                    snapshot.pi_tree_target_id.as_deref(),
                    &snapshot.completed_turns,
                )?;
                let verified = {
                    let retained = self.retained.lock();
                    self.dashboard
                        .with_owned(owner, |_| retained.operator_tree_snapshot(conversation))
                        .context("dashboard owner changed")??
                };
                if verified.summary.session_epoch != snapshot.summary.session_epoch
                    || verified.checkpoint_turn != snapshot.checkpoint_turn
                    || verified.pi_tree_target_id != snapshot.pi_tree_target_id
                {
                    bail!("Operator session changed while reading tree; retry /tree.");
                }
                Ok(OperatorResponse::Tree(Box::new(tree)))
            }
            OperatorRequest::NavigateTree {
                conversation,
                expected_run,
                target_entry_id,
                summarize,
            } => {
                let (nav, changed) = {
                    let _mutation = self.mutation_lock.lock().unwrap();
                    self.reject_if_stopping()?;
                    #[cfg(test)]
                    if let Some(hook) = self.operator.before_retained.lock().unwrap().clone() {
                        hook();
                    }
                    self.ensure_operator_worker_not_live(
                        owner,
                        conversation,
                        "cannot switch Operator tree branch while an Operator worker or watched-session lease is active",
                    )?;
                    let snapshot = {
                        let retained = self.retained.lock();
                        self.dashboard
                            .with_owned(owner, |_| {
                                retained.ensure_operator_tree_navigable(conversation, expected_run)
                            })
                            .context("dashboard owner changed")??
                    };
                    let Some(checkpoint_turn) = snapshot.checkpoint_turn else {
                        bail!(
                            "Cannot navigate Operator tree before the first completed turn in this session."
                        );
                    };
                    let resolved = crate::operator_session_file::resolve_tree_target(
                        &self.registry_path,
                        conversation,
                        checkpoint_turn,
                        snapshot.pi_tree_target_id.as_deref(),
                        &snapshot.completed_turns,
                        &target_entry_id,
                    )?;
                    match summarize {
                        None => {
                            if resolved.noop {
                                let published = self.publish_worker_live(snapshot.summary)?;
                                (
                                    OperatorTreeNavigation {
                                        conversation: published,
                                        current_leaf_id: resolved.current_leaf_id,
                                        noop: true,
                                        submitted: None,
                                        editor_text: None,
                                        restored_context: None,
                                    },
                                    false,
                                )
                            } else {
                                let summary = {
                                    let mut retained = self.retained.lock();
                                    self.dashboard
                                        .with_owned(owner, |_| {
                                            retained.apply_operator_tree_navigation(
                                                conversation,
                                                expected_run,
                                                snapshot.summary.session_epoch,
                                                checkpoint_turn,
                                                resolved.active_tip_turn,
                                                resolved.pi_tree_target_id.as_deref(),
                                            )
                                        })
                                        .context("dashboard owner changed")??
                                };
                                let published = self.publish_worker_live(summary)?;
                                (
                                    OperatorTreeNavigation {
                                        conversation: published,
                                        current_leaf_id: resolved.current_leaf_id,
                                        noop: false,
                                        submitted: None,
                                        editor_text: resolved.editor_text,
                                        restored_context: resolved.restored_context,
                                    },
                                    true,
                                )
                            }
                        }
                        Some(summary_req) => {
                            if resolved.noop || !resolved.can_summarize {
                                bail!(
                                    "Selected tree target has no abandoned branch entries to summarize."
                                );
                            }
                            let settings = self
                                .settings
                                .lock()
                                .unwrap()
                                .report
                                .settings
                                .operator
                                .clone();
                            let turn = {
                                let mut retained = self.retained.lock();
                                self.dashboard
                                    .with_owned(owner, |_| {
                                        retained.submit_operator_branch_summary_turn(
                                            conversation,
                                            expected_run,
                                            snapshot.summary.session_epoch,
                                            checkpoint_turn,
                                            resolved.active_tip_turn,
                                            target_entry_id,
                                            summary_req.instructions,
                                            settings,
                                        )
                                    })
                                    .context("dashboard owner changed")??
                            };
                            let _ = self.finish_submitted_operator_turn(
                                conversation,
                                turn.clone(),
                                false,
                            )?;
                            let summary =
                                self.retained.lock().operator_conversation(conversation)?;
                            let published = self.publish_worker_live(summary)?;
                            (
                                OperatorTreeNavigation {
                                    conversation: published,
                                    current_leaf_id: resolved.current_leaf_id,
                                    noop: false,
                                    submitted: Some(turn),
                                    editor_text: resolved.editor_text,
                                    restored_context: resolved.restored_context,
                                },
                                false,
                            )
                        }
                    }
                };
                if changed {
                    self.operator_changed(nav.conversation.id);
                }
                Ok(OperatorResponse::TreeNavigated(Box::new(nav)))
            }
            OperatorRequest::Read {
                conversation,
                after,
                limit,
            } => {
                let _mutation = self.mutation_lock.lock().unwrap();
                self.reject_if_stopping()?;
                #[cfg(test)]
                if let Some(hook) = self.operator.before_retained.lock().unwrap().clone() {
                    hook();
                }
                let result = {
                    let retained = self.retained.lock();
                    self.dashboard
                        .with_owned(owner, |_| {
                            retained.read_operator_history(conversation, after, limit)
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

    fn finish_submitted_operator_turn(
        &self,
        conversation: OperatorConversationId,
        turn: OperatorTurn,
        warm: bool,
    ) -> Result<OperatorResponse> {
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
                    "Worker launch failed; inspect the retained session before retrying".into(),
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

    fn operator_models(&self, owner: &Arc<()>) -> Result<Vec<OperatorModelCatalogEntry>> {
        self.reject_if_stopping()?;
        if !self.dashboard.owns(owner) {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "dashboard owner changed",
            ));
        }
        let cwd = self.registry_path.join("operator-workdir");
        fs::create_dir_all(&cwd)
            .and_then(|()| fs::set_permissions(&cwd, fs::Permissions::from_mode(0o700)))
            .map_err(|_| lifecycle_error(ErrorCode::Internal, OPERATOR_MODEL_UNAVAILABLE_MSG))?;
        if self.operator.executable.as_os_str().is_empty() || !self.operator.executable.is_file() {
            return Err(lifecycle_error(
                ErrorCode::Internal,
                OPERATOR_MODEL_UNAVAILABLE_MSG,
            ));
        }
        let mut command = std::process::Command::new(&self.operator.executable);
        command
            .current_dir(&cwd)
            .args(["operator", "models", "--json"])
            .env(crate::config::HOME_ENV, &self.registry_path)
            .env_remove(crate::config::CONFIG_ENV)
            .env_remove("OVRCR_SOCKET");
        for name in [
            "OVRCR_AGENT_SOCKET",
            "OVRCR_AGENT_TOKEN",
            "OVRCR_HOOK_SOCKET",
            "OVRCR_HOOK_TOKEN",
            "OVRCR_SESSION_ID",
            "OVRCR_DELEGATION_SESSION",
            "OVRCR_DELEGATION_TOKEN",
            "OVRCR_DELEGATION_SOCKET",
            "OVRCR_DELEGATION_HOME",
            "OVRCR_DELEGATION_INSTANCE",
            "OVRCR_DELEGATION_EPOCH",
            "OVRCR_PI_ENTRY",
            crate::claude_delegation::ARTIFACT_ENV,
            crate::codex_delegation::ARTIFACT_ENV,
        ] {
            command.env_remove(name);
        }
        #[cfg(test)]
        let deadline = self
            .operator
            .models_deadline
            .lock()
            .unwrap()
            .unwrap_or(OPERATOR_MODEL_CATALOG_DEADLINE);
        #[cfg(not(test))]
        let deadline = OPERATOR_MODEL_CATALOG_DEADLINE;

        let output = self
            .agent_model_catalog
            .with_process_owner(|process_owner| {
                process_owner.run_command(
                    "Operator Pi",
                    &mut command,
                    OPERATOR_MODEL_CATALOG_LIMIT,
                    OPERATOR_MODEL_DIAGNOSTIC_LIMIT,
                    deadline,
                    &self.shutdown,
                )
            })
            .map_err(|error| {
                let detail = error.to_string();
                if self.shutdown.load(Ordering::Acquire) {
                    lifecycle_error(ErrorCode::Conflict, "server is stopping")
                } else if detail.contains("native model catalog process is active") {
                    lifecycle_error(
                        ErrorCode::Conflict,
                        "Operator model discovery is busy; retry in a moment.",
                    )
                } else if detail.contains("model discovery deadline") {
                    lifecycle_error(
                        ErrorCode::Internal,
                        "Operator model discovery timed out; run ovrcr operator models to diagnose.",
                    )
                } else {
                    lifecycle_error(ErrorCode::Internal, OPERATOR_MODEL_UNAVAILABLE_MSG)
                }
            })?;
        if !self.dashboard.owns(owner) {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "dashboard owner changed",
            ));
        }
        parse_operator_model_catalog(&output.stdout)
            .map_err(|_| lifecycle_error(ErrorCode::Internal, OPERATOR_MODEL_UNAVAILABLE_MSG))
    }

    fn is_operator_worker_live(
        &self,
        session_id: SessionId,
        conversation: OperatorConversationId,
        run: SessionRunId,
    ) -> bool {
        let session = self.sessions.lock().unwrap().get(&session_id).cloned();
        session.is_some_and(|session| {
            session.run() == run
                && session.is_live()
                && session.summary().scope
                    == (ovrcr_protocol::SessionScope::OperatorConversation { conversation })
        })
    }

    fn ensure_operator_worker_not_live(
        &self,
        owner: &Arc<()>,
        conversation: OperatorConversationId,
        busy_message: &'static str,
    ) -> Result<()> {
        let current = {
            let retained = self.retained.lock();
            self.dashboard
                .with_owned(owner, |_| -> Result<_> {
                    retained.require_current_operator(conversation)?;
                    retained.operator_conversation(conversation)
                })
                .context("dashboard owner changed")??
        };
        if self.is_operator_worker_live(current.session, current.id, current.run) {
            bail!("{busy_message}");
        }
        Ok(())
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
        summary.worker_live =
            self.is_operator_worker_live(summary.session, summary.id, summary.run);
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
        Ok(self.is_operator_worker_live(summary.session, conversation, run))
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
        exchange.validate().map_err(anyhow::Error::msg)?;
        let identity = exchange.batch.identity;
        let terminal = exchange
            .batch
            .events
            .last()
            .is_some_and(OperatorWorkerEvent::is_terminal);
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&identity.session)
            .cloned()
            .context("operator worker unavailable")?;
        let mut retained = self.retained.lock();
        let receipt = session.with_operator_authority(&exchange.auth, owner, identity, || {
            retained.apply_operator_batch_with_lease(&exchange.batch, exchange.leased)
        })?;
        if terminal {
            session.clear_operator_abort(identity);
        }
        drop(retained);
        if !receipt.duplicate {
            self.operator_changed(identity.conversation);
        }
        Ok(receipt)
    }

    /// Caller holds mutation_lock, shared with acknowledged watch updates.
    pub(super) fn reject_watched_archive(&self, id: SessionId, run: SessionRunId) -> Result<()> {
        let retained = self.retained.lock();
        let Some(current) = retained.current_operator_conversation()? else {
            return Ok(());
        };
        let record = retained
            .get(current.session)
            .context("Operator record unavailable")?;
        if !record.requires_ack(retained.boot_id()) {
            return Ok(());
        }
        let watches = self.operator.watches.lock().unwrap();
        let Some((_, update)) = watches.as_ref().filter(|(identity, _)| {
            identity.conversation == current.id
                && identity.session == current.session
                && identity.run == current.run
        }) else {
            return Err(lifecycle_error(
                ErrorCode::OwnershipUncertain,
                "Operator watch state unavailable; preserve the stopped record",
            ));
        };
        if update
            .targets
            .iter()
            .any(|target| target.session == id && target.run == run)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "session run is protected by an Operator watch or queued report",
            ));
        }
        Ok(())
    }

    pub(super) fn operator_worker(
        &self,
        request: OperatorWorkerRequest,
        owner: Option<&Arc<()>>,
    ) -> Result<OperatorWorkerResult> {
        match request.command {
            OperatorWorkerCommand::SetWatches { identity, update } => {
                update.validate().map_err(anyhow::Error::msg)?;
                let _mutation = self.mutation_lock.lock().unwrap();
                self.reject_if_stopping()?;
                let session = self
                    .sessions
                    .lock()
                    .unwrap()
                    .get(&identity.session)
                    .cloned()
                    .context("operator worker unavailable")?;
                let retained = self.retained.lock();
                retained.require_current_operator(identity.conversation)?;
                let current = retained.operator_conversation(identity.conversation)?;
                if current.run != identity.run
                    || current.session != identity.session
                    || current.worker_stopped
                {
                    bail!("stale Operator watch owner");
                }
                session.with_operator_run_authority(&request.auth, owner, identity, || {
                    let mut watches = self.operator.watches.lock().unwrap();
                    let previous = watches
                        .as_ref()
                        .filter(|(owner, _)| *owner == identity)
                        .map(|(_, update)| update);
                    if previous == Some(&update) {
                        return Ok(()); // Reconcile the original update after a lost acknowledgement.
                    }
                    if update.revision
                        != previous
                            .map_or(Some(1), |old| old.revision.checked_add(1))
                            .context("watch revision exhausted")?
                    {
                        bail!("Operator watch revision changed; reconcile the original update");
                    }
                    for target in &update.targets {
                        if previous.is_some_and(|old| old.targets.contains(target)) {
                            continue;
                        }
                        let record = retained
                            .get(target.session)
                            .context("watched session unavailable")?;
                        if record.run != target.run
                            || record.disposition == crate::retained::Disposition::Archived
                            || record.metadata.scope != ovrcr_protocol::SessionScope::Workspace
                        {
                            bail!("watched session changed before admission");
                        }
                    }
                    *watches = Some((identity, update.clone()));
                    Ok(())
                })?;
                Ok(OperatorWorkerResult::WatchesSet {
                    identity,
                    revision: update.revision,
                })
            }
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
                    skill: None,
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

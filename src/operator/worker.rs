//! Blocking native pi RPC Operator worker. The Server alone certifies termination.
//!
//! One managed same-executable worker drives one turn: authenticated Start, an exclusive
//! attempt directory, a fresh native pi session in it, the optional completed-context
//! checkpoint copy, exact model resolution, one prompt, ordered journal batches, and an
//! authenticated Retire. Pi owns the agent loop; there is no second scheduler.
use super::{
    pi::{self, PiPaths},
    rpc::{RpcChild, RpcMessage, RpcResponse},
};
use crate::report::{reporter::Reporter, reserve_invocation_for};
use anyhow::{Result, anyhow};
use ovrcr_protocol::{
    AgentProvider, OperatorEventBatch, OperatorModel, OperatorToolStatus, OperatorTurnId,
    OperatorTurnIdentity, OperatorWorkerEvent, OperatorWorkerStart,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Fixed user-facing failure messages. Native stderr and provider bodies never join them.
const NO_MODEL: &str = "No Pi model selected; run ovrcr operator setup.";
const MODEL_UNAVAILABLE: &str =
    "Configured Operator model is unavailable in the dedicated Pi profile.";
const CONTEXT_UNAVAILABLE: &str = "Operator Pi context is unavailable; saved data was preserved.";
const INTEGRATION_UNAVAILABLE: &str = "Pi Operator integration is unavailable; no prompt was sent.";
const REQUEST_FAILED: &str = "Pi request failed; inspect the partial journal before retrying.";
const UNSUPPORTED_INTERACTION: &str = "Unsupported Pi interaction; the turn was stopped.";

/// The trusted extension's contract status key and the only four admitted tools.
const CONTRACT_KEY: &str = "ovrcr_operator_contract";
const TOOLS: [&str; 4] = ["read", "bash", "edit", "write"];
const ADMISSION_TITLE: &str = "OVRCR Operator tool admission";
const RESULT_TITLE: &str = "OVRCR Operator tool result";

/// Pi's extensions stay off except the OVRCR lifecycle adapter; the four tool
/// definitions, validation, truncation and context management are pi's own.
const POLICY: &str = "You are OVRCR Operator, the agent inside a user's OVRCR instance. Act only on the user's requested task. Operate the instance through bash by running the matching OVRCR CLI: \"$OVRCR_EXECUTABLE\" --json, with the inherited instance configuration and socket. Use the exact project, workspace, session and run identities captured in the context; honor refusals and never guess control authority. The captured context is fixed for this message, and a later Dashboard selection change never retargets this turn. Your process working directory stays <instance>/operator-workdir for the whole conversation and is not the selected workspace: pass the captured absolute paths to the read, edit and write tools, and put \"cd <captured cwd>\" inside a bash command when operating the selected workspace. The read, bash, edit and write tools use pi-native schemas. Do not read or upload terminal content unless the user asked for it. Do not inspect credentials, tokens or personal agent profiles. Do not start watchers, schedulers or workflow-controller authority. Stop cancels this turn only: previously accepted OVRCR operations may finish, so never claim rollback and never replay an interrupted command. Pi compacts this same conversation automatically as context fills; there is no new-conversation action.";

const BATCH_BYTES: usize = 64 * 1024;
const MAX_OPERATOR_ARGUMENT_BYTES: usize = 64 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_HEADER_BYTES: usize = 64 * 1024;
/// One complete native session record; the checkpoint copy never buffers more.
const MAX_CHECKPOINT_RECORD: usize = 16 * 1024 * 1024;
/// Bound on the admitted-but-not-ended tool calls remembered for cross-checking
/// the ordinary execution notifications. Sequential execution keeps this at one.
const MAX_ADMITTED_CALLS: usize = 1024;
const MAX_MODELS: usize = 4096;
/// Bound on one assistant message's content blocks, including streamed indexes.
const MAX_CONTENT_BLOCKS: usize = 1024;
/// Deferred redaction display reuses per-block text; bound it by the native
/// complete-record/RPC frame ceiling rather than adding copies.
const MAX_DEFERRED_BYTES: usize = 16 * 1024 * 1024;
const TEXT_CADENCE: Duration = Duration::from_millis(50);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const CONTROL_DEADLINE: Duration = Duration::from_secs(20);
const CONTRACT_DEADLINE: Duration = Duration::from_secs(20);
const REPORT_DEADLINE: Duration = Duration::from_secs(5);
const RETIRE_DEADLINE: Duration = Duration::from_secs(15);
const SHUTDOWN_REPLY: Duration = Duration::from_secs(5);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// Best-effort abort budget while the Server is already terminating this group.
const TERMINATING_REPLY: Duration = Duration::from_millis(250);
const TERMINATING_GRACE: Duration = Duration::from_millis(750);
/// Fallback exit after a termination signal, inside the Server's five-second grace.
const WATCHDOG_DELAY: Duration = Duration::from_millis(2000);

pub fn run(identity: OperatorTurnIdentity) -> Result<()> {
    identity.validate().map_err(anyhow::Error::msg)?;
    let termination = Termination::install();
    let lease = reserve_invocation_for(AgentProvider::Operator)?
        .ok_or_else(|| anyhow!("managed operator reservation required"))?;
    let mut reporter = Reporter::new(AgentProvider::Operator, Some(lease), None);
    // Authenticated Start precedes every pi resource: a refused Start creates nothing.
    let start = match reporter.operator_start(identity, Instant::now() + REPORT_DEADLINE) {
        Ok(start) => start,
        Err(_) => {
            if termination.requested() {
                return Ok(());
            }
            eprintln!("Operator bootstrap unavailable; stop this turn before retrying.");
            hold();
        }
    };
    let mut journal = Journal::new(&mut reporter, identity);
    match attempt(&mut journal, &start, &termination) {
        Ok(()) => {}
        Err(Failure::Failed(message)) => {
            // A revoking termination is not a reason to attempt another journal write.
            if !termination.requested() {
                match journal.commit(OperatorWorkerEvent::Failed(message.to_string())) {
                    // A failed reporting commit blocks retirement just as it blocks tools.
                    Ok(()) => {
                        let _ = journal.retire(RETIRE_DEADLINE, false);
                    }
                    Err(_) => {
                        let _ = journal.retire(RETIRE_DEADLINE, true);
                    }
                }
            }
        }
        Err(Failure::ToolTimedOut | Failure::ReportFailed) => {
            let _ = journal.retire(RETIRE_DEADLINE, true);
        }
        // The connection was dropped while `attempt` unwound, which already ran the
        // bounded clear_queue/abort/drain (`Connection::drop` -> `abort_quietly`); no
        // journal write may follow a revoked authority.
        Err(Failure::Terminated) => std::process::exit(0),
        // An uncertain tool result is never fed back into the agent loop: hold until
        // the Server's owned termination (or an explicit Stop) resolves it.
        Err(Failure::Uncertain) => {}
    }
    hold();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// Commit this fixed message as `Failed`, then authenticate Retire.
    Failed(&'static str),
    /// The attached Bash adapter reported a timeout; request authenticated Stop.
    ToolTimedOut,
    /// A reporting commit failed: no further admissions or commits; request Stop.
    ReportFailed,
    /// The Server is terminating this owned group; commit nothing more.
    Terminated,
    /// A tool result could not be acknowledged; abort and hold without a decision.
    Uncertain,
}

fn pi_failure(error: pi::PiError) -> Failure {
    Failure::Failed(error.user_message())
}

fn attempt(
    journal: &mut Journal<'_>,
    start: &OperatorWorkerStart,
    termination: &Termination,
) -> std::result::Result<(), Failure> {
    let instance = ovrcr_runtime::config::RegistryPath::resolve()
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?
        .0;
    let paths = pi::paths(&instance).map_err(pi_failure)?;
    let attempt_dir = paths.attempt_dir(start.identity.turn);
    create_attempt(&paths, &attempt_dir)?;
    let _extension = pi::materialize_extension(&paths).map_err(pi_failure)?;
    let expected_session = pi::conversation_session_id(start.identity.conversation);
    let mut command = pi::rpc_command(&paths, &attempt_dir, start.identity.conversation, POLICY)
        .map_err(pi_failure)?;
    let child = RpcChild::spawn(&mut command, false).map_err(pi_failure)?;
    let mut connection = Connection::new(child, termination.clone());
    let mut state = TurnState::new(expected_session, paths.workdir.clone());

    // Step 2: a fresh empty session resolves the profile's current defaults.
    let response = connection.request(
        journal,
        &mut state,
        "get_state",
        json!({}),
        CONTROL_DEADLINE,
    )?;
    if !response.success {
        // A structured refusal before any prompt is an integration failure; a
        // transport failure below stays REQUEST_FAILED.
        return Err(Failure::Failed(INTEGRATION_UNAVAILABLE));
    }
    state.capture_fresh(&response.data)?;
    connection.await_contract(journal, &mut state)?;

    // Step 3: copy the completed checkpoint in and resume only from that copy.
    if let Some(previous) = start.checkpoint_turn {
        restore_checkpoint(
            &mut connection,
            journal,
            &mut state,
            &paths,
            &attempt_dir,
            start,
            previous,
        )?;
    }

    // Step 4: exact model resolution, then the accepted reasoning choice.
    let model = resolve_model(&mut connection, journal, &mut state, start.model.as_deref())?;
    let reasoning = resolve_reasoning(&mut connection, journal, &mut state, start.thinking_level)?;

    // Step 5: one prompt carrying the captured context ahead of the user text.
    if termination.requested() {
        return Err(Failure::Terminated);
    }
    journal.commit(OperatorWorkerEvent::ModelSelected(model))?;
    journal.commit(OperatorWorkerEvent::ReasoningSelected(reasoning))?;
    let prompt = prompt_text(start)?;
    state.prompted = true;
    let response = connection.request(
        journal,
        &mut state,
        "prompt",
        json!({ "message": prompt }),
        CONTROL_DEADLINE,
    )?;
    // `prompt.success` is acceptance only.
    if !response.success {
        return Err(Failure::Failed(REQUEST_FAILED));
    }

    // Step 6: stream until pi has no remaining automatic work.
    connection.stream(journal, &mut state)?;
    if state.pending.is_some() {
        return Err(Failure::Failed(REQUEST_FAILED));
    }
    if state.last_assistant_stop().as_deref() != Some("stop") {
        return Err(Failure::Failed(REQUEST_FAILED));
    }

    // Step 7: settle, drain and reap pi, then promote the native session file at
    // the path the final native state reports.
    let response = connection.request(
        journal,
        &mut state,
        "get_state",
        json!({}),
        CONTROL_DEADLINE,
    )?;
    require_success(&response)?;
    state.capture(&response.data)?;
    if termination.requested() {
        return Err(Failure::Terminated);
    }
    connection.settle(journal, &mut state, SHUTDOWN_GRACE)?;
    let native = state
        .session_file
        .clone()
        .ok_or(Failure::Failed(CONTEXT_UNAVAILABLE))?;
    promote_session(&attempt_dir, &native, &state.session_id, &state.workdir)?;
    if termination.requested() {
        return Err(Failure::Terminated);
    }

    // Only an acknowledged Completed transaction promotes the checkpoint turn.
    journal.commit(OperatorWorkerEvent::Completed)?;
    // Step 9: the superseded copy is disposable only after that acknowledgement.
    delete_previous_checkpoint(&paths, start);
    // Step 7 (continued): an authenticated Retire hands ownership back to the Server.
    let _ = journal.retire(RETIRE_DEADLINE, false);
    Ok(())
}

// ---------------------------------------------------------------- journal

/// The two displayable assistant content kinds. Both coalesce on the same cadence,
/// but a kind or content-block boundary flushes before the next block starts so
/// display order always matches the provider's content order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisplayKind {
    Answer,
    Thinking,
}

impl DisplayKind {
    fn event(self, text: String) -> OperatorWorkerEvent {
        match self {
            Self::Answer => OperatorWorkerEvent::AssistantDelta(text),
            Self::Thinking => OperatorWorkerEvent::ThinkingDelta(text),
        }
    }
}

struct Journal<'a> {
    reporter: &'a mut Reporter,
    identity: OperatorTurnIdentity,
    sequence: u64,
    pending: String,
    pending_kind: Option<DisplayKind>,
    last_flush: Instant,
    poisoned: bool,
}

impl<'a> Journal<'a> {
    fn new(reporter: &'a mut Reporter, identity: OperatorTurnIdentity) -> Self {
        Self {
            reporter,
            identity,
            sequence: 1,
            pending: String::new(),
            pending_kind: None,
            last_flush: Instant::now(),
            poisoned: false,
        }
    }

    fn commit(&mut self, event: OperatorWorkerEvent) -> std::result::Result<(), Failure> {
        if self.poisoned {
            return Err(Failure::ReportFailed);
        }
        let batch = OperatorEventBatch {
            identity: self.identity,
            sequence: self.sequence,
            events: vec![event],
        };
        match self
            .reporter
            .operator_batch(batch, Instant::now() + REPORT_DEADLINE)
        {
            Ok(()) => {
                self.sequence += 1;
                Ok(())
            }
            Err(_) => {
                self.poisoned = true;
                Err(Failure::ReportFailed)
            }
        }
    }

    fn retire(&mut self, deadline: Duration, stop: bool) -> Result<()> {
        self.reporter
            .operator_retire(self.identity, stop, Instant::now() + deadline)
    }

    /// Append display text of one kind, splitting only on UTF-8 boundaries and
    /// never exceeding one batch. A kind change flushes first: answer and thinking
    /// events never interleave inside one commit.
    fn push(&mut self, kind: DisplayKind, fragment: &str) -> std::result::Result<(), Failure> {
        if fragment.is_empty() {
            return Ok(());
        }
        if self.pending_kind != Some(kind) {
            self.flush()?;
        }
        let mut remaining = fragment;
        while !remaining.is_empty() {
            if self.pending.is_empty() {
                self.pending_kind = Some(kind);
                self.last_flush = Instant::now();
            }
            let room = BATCH_BYTES.saturating_sub(self.pending.len());
            if room == 0 {
                self.flush()?;
                continue;
            }
            let mut count = remaining.len().min(room);
            while count > 0 && !remaining.is_char_boundary(count) {
                count -= 1;
            }
            if count == 0 {
                self.flush()?;
                continue;
            }
            self.pending.push_str(&remaining[..count]);
            remaining = &remaining[count..];
            if self.pending.len() >= BATCH_BYTES {
                self.flush()?;
            }
        }
        Ok(())
    }

    /// Commit the current display prefix, whatever its kind. Used before the next
    /// content block, before a durable tool receipt, and after settlement.
    fn flush(&mut self) -> std::result::Result<(), Failure> {
        let Some(kind) = self.pending_kind.take() else {
            return Ok(());
        };
        if self.pending.is_empty() {
            return Ok(());
        }
        let text = std::mem::take(&mut self.pending);
        self.commit(kind.event(text))
    }

    /// Coalesce display text on the existing cadence.
    fn tick(&mut self) -> std::result::Result<(), Failure> {
        if !self.pending.is_empty() && self.last_flush.elapsed() >= TEXT_CADENCE {
            self.flush()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- child

struct Connection {
    child: Option<RpcChild>,
    next_id: u64,
    termination: Termination,
}

impl Connection {
    fn new(child: RpcChild, termination: Termination) -> Self {
        Self {
            child: Some(child),
            next_id: 1,
            termination,
        }
    }

    fn send(&mut self, value: &Value) -> std::result::Result<(), Failure> {
        self.child
            .as_mut()
            .ok_or(Failure::Failed(REQUEST_FAILED))?
            .send(value)
            .map_err(|_| Failure::Failed(REQUEST_FAILED))
    }

    fn next_message(
        &mut self,
        journal: &mut Journal<'_>,
        deadline: Option<Instant>,
    ) -> std::result::Result<RpcMessage, Failure> {
        loop {
            // Authority is revoked by termination; no further journal work may follow.
            if self.termination.requested() {
                return Err(Failure::Terminated);
            }
            journal.tick()?;
            // The bound applies to every iteration, not only to an idle poll: a
            // peer that keeps streaming ordinary notifications while withholding
            // the required response cannot defer it.
            if deadline.is_some_and(|until| Instant::now() >= until) {
                return Err(Failure::Failed(REQUEST_FAILED));
            }
            let message = self
                .child
                .as_mut()
                .ok_or(Failure::Failed(REQUEST_FAILED))?
                .next_message(Some(POLL_INTERVAL));
            match message {
                Ok(message) => {
                    // A record that arrived at or after the bound is too late.
                    if deadline.is_some_and(|until| Instant::now() >= until) {
                        return Err(Failure::Failed(REQUEST_FAILED));
                    }
                    return Ok(message);
                }
                // A poll timeout is a chance to flush display text, not a failure.
                Err(error) if error.timed_out_error() => {}
                Err(_) => return Err(Failure::Failed(REQUEST_FAILED)),
            }
        }
    }

    fn request(
        &mut self,
        journal: &mut Journal<'_>,
        state: &mut TurnState,
        command: &str,
        params: Value,
        deadline: Duration,
    ) -> std::result::Result<RpcResponse, Failure> {
        let id = format!("ovrcr-{}", self.next_id);
        self.next_id += 1;
        let mut payload = match params {
            Value::Object(map) => Value::Object(map),
            _ => json!({}),
        };
        payload["type"] = json!(command);
        payload["id"] = json!(&id);
        self.send(&payload)?;
        let until = Instant::now() + deadline;
        loop {
            match self.next_message(journal, Some(until))? {
                RpcMessage::Response(response) if response.id == id => return Ok(response),
                RpcMessage::Response(_) => return Err(Failure::Failed(REQUEST_FAILED)),
                RpcMessage::Event(event) => self.event(journal, state, event)?,
            }
        }
    }

    /// Bounded post-completion control used only on the terminating path. Replies
    /// and dialogs are drained without journaling; a dialog still open is
    /// cancelled. Failures are deliberately ignored here.
    fn quiet_command(&mut self, command: &str, deadline: Duration) {
        let id = format!("ovrcr-{}", self.next_id);
        self.next_id += 1;
        let payload = json!({ "type": command, "id": &id });
        if self.send(&payload).is_err() {
            return;
        }
        let until = Instant::now() + deadline;
        loop {
            // A streaming peer must not extend the terminating bound either.
            if Instant::now() >= until {
                return;
            }
            let message = {
                let Some(child) = self.child.as_mut() else {
                    return;
                };
                child.next_message(Some(POLL_INTERVAL))
            };
            match message {
                Ok(RpcMessage::Response(response)) if response.id == id => return,
                Ok(RpcMessage::Response(_)) => {}
                Ok(RpcMessage::Event(event)) => {
                    if is_dialog(&event)
                        && let Some(dialog) = event["id"].as_str()
                    {
                        let _ = self.send(&json!({
                            "type": "extension_ui_response",
                            "id": dialog,
                            "cancelled": true,
                        }));
                    }
                }
                Err(error) if error.timed_out_error() => {}
                Err(_) => return,
            }
        }
    }

    /// Successful-path shutdown: nothing here is best-effort. Both control
    /// replies must be successful, stdout must end without a protocol fault, and
    /// the exact child must exit successfully within `grace`; otherwise the turn
    /// cannot be completed or promoted.
    fn settle(
        &mut self,
        journal: &mut Journal<'_>,
        state: &mut TurnState,
        grace: Duration,
    ) -> std::result::Result<(), Failure> {
        for command in ["clear_queue", "abort"] {
            let response = self.request(journal, state, command, json!({}), SHUTDOWN_REPLY)?;
            require_success(&response)?;
        }
        self.shutdown(grace)
    }

    /// Close stdin, drain the exact child and reap it within `grace`. A managed
    /// child stays in the Session's group, so only stdin EOF and reaping are
    /// ours. `Ok` means pi exited successfully, so its native session file is no
    /// longer being written; a protocol fault or a nonzero exit fails the turn
    /// while still reaping whatever we can.
    fn shutdown(&mut self, grace: Duration) -> std::result::Result<(), Failure> {
        let Some(mut child) = self.child.take() else {
            return Err(Failure::Failed(REQUEST_FAILED));
        };
        child.close_stdin();
        let expires = Instant::now() + grace;
        let mut ended = false;
        let mut exit_success: Option<bool> = None;
        loop {
            if Instant::now() >= expires {
                // Ownership uncertainty: never promote the session file.
                let _ = child.terminate(Duration::ZERO);
                return Err(Failure::Failed(REQUEST_FAILED));
            }
            if !ended {
                match child.next_message(Some(POLL_INTERVAL)) {
                    // No reply can be outstanding after the two settled controls.
                    Ok(RpcMessage::Response(_)) => {
                        let _ = child.terminate(Duration::ZERO);
                        return Err(Failure::Failed(REQUEST_FAILED));
                    }
                    Ok(RpcMessage::Event(event)) => {
                        if is_dialog(&event) {
                            // A tool started after abort: never a clean end.
                            if let Some(dialog) = event["id"].as_str() {
                                let _ = child.send(&json!({
                                    "type": "extension_ui_response",
                                    "id": dialog,
                                    "cancelled": true,
                                }));
                            }
                            let _ = child.terminate(Duration::ZERO);
                            return Err(Failure::Failed(REQUEST_FAILED));
                        }
                    }
                    Err(error) if error.timed_out_error() => {}
                    Err(error) if clean_stream_end(&error) => ended = true,
                    // Malformed records or an unreadable stream are faults.
                    Err(_) => {
                        let _ = child.terminate(Duration::ZERO);
                        return Err(Failure::Failed(REQUEST_FAILED));
                    }
                }
            }
            if exit_success.is_none() {
                exit_success = child.try_exit().map(|status| status.success());
            }
            // A nonzero exit always fails the turn (the status is already
            // reaped). A zero exit is clean only once stdout ended, so every
            // buffered record was validated.
            if exit_success == Some(false) {
                return Err(Failure::Failed(REQUEST_FAILED));
            }
            if exit_success == Some(true) && ended {
                return Ok(());
            }
            if ended {
                std::thread::sleep(POLL_INTERVAL);
            }
        }
    }

    /// Best-effort abort used only after a failure or a termination request.
    fn abort_quietly(&mut self) {
        if self.child.is_none() {
            return;
        }
        self.quiet_command("clear_queue", TERMINATING_REPLY);
        self.quiet_command("abort", TERMINATING_REPLY);
        let _ = self.shutdown(TERMINATING_GRACE);
    }

    fn await_contract(
        &mut self,
        journal: &mut Journal<'_>,
        state: &mut TurnState,
    ) -> std::result::Result<(), Failure> {
        let until = Instant::now() + CONTRACT_DEADLINE;
        while state.ready_count == 0 {
            let message = match self.next_message(journal, Some(until)) {
                Ok(message) => message,
                // Before the prompt, any startup/transport failure means the trusted
                // extension never bound; no prompt may follow.
                Err(Failure::Failed(_)) => return Err(Failure::Failed(INTEGRATION_UNAVAILABLE)),
                Err(failure) => return Err(failure),
            };
            match message {
                RpcMessage::Event(event) => self.event(journal, state, event)?,
                RpcMessage::Response(_) => {
                    return Err(Failure::Failed(INTEGRATION_UNAVAILABLE));
                }
            }
        }
        Ok(())
    }

    fn flush_admission(
        &mut self,
        journal: &mut Journal<'_>,
        state: &mut TurnState,
        event: Value,
    ) -> std::result::Result<(), Failure> {
        let dialog = event["id"]
            .as_str()
            .ok_or(Failure::Failed(REQUEST_FAILED))?;
        let payload: Value = event["message"]
            .as_str()
            .and_then(|text| serde_json::from_str(text).ok())
            .ok_or(Failure::Failed(REQUEST_FAILED))?;
        let (id, name, arguments) = match admit(&payload, state) {
            Ok(admitted) => admitted,
            Err(failure) => {
                let _ = self.send(&json!({
                    "type": "extension_ui_response",
                    "id": dialog,
                    "confirmed": false,
                }));
                return Err(failure);
            }
        };
        // Flush preceding assistant display before the durable tool receipt.
        journal.flush()?;
        journal.commit(OperatorWorkerEvent::ToolCall {
            id: id.clone(),
            name: name.clone(),
            arguments,
        })?;
        state.admitted.push(id.clone());
        state.pending = Some(PendingCall { id });
        self.send(&json!({
            "type": "extension_ui_response",
            "id": dialog,
            "confirmed": true,
        }))
    }

    fn flush_result(
        &mut self,
        journal: &mut Journal<'_>,
        state: &mut TurnState,
        event: Value,
    ) -> std::result::Result<(), Failure> {
        let dialog = event["id"]
            .as_str()
            .ok_or(Failure::Failed(REQUEST_FAILED))?;
        let payload: Value = event["message"]
            .as_str()
            .and_then(|text| serde_json::from_str(text).ok())
            .ok_or(Failure::Failed(REQUEST_FAILED))?;
        let call_id = payload["toolCallId"]
            .as_str()
            .ok_or(Failure::Failed(REQUEST_FAILED))?;
        let pending = state
            .pending
            .as_ref()
            .ok_or(Failure::Failed(REQUEST_FAILED))?;
        if pending.id != call_id {
            let _ = self.send(&json!({
                "type": "extension_ui_response",
                "id": dialog,
                "confirmed": false,
            }));
            return Err(Failure::Failed(REQUEST_FAILED));
        }
        let output = payload["output"]
            .as_str()
            .ok_or(Failure::Failed(REQUEST_FAILED))?;
        let native_truncated = payload["truncated"].as_bool().unwrap_or(false);
        let status = tool_status(payload["status"].as_str().unwrap_or(""))
            .ok_or(Failure::Failed(REQUEST_FAILED))?;
        let (output, cut) = sanitize_output(output);
        // No display can cross a durable tool receipt.
        journal.flush()?;
        journal.commit(OperatorWorkerEvent::ToolOutput {
            id: pending.id.clone(),
            output,
            truncated: native_truncated || cut,
            status: status.clone(),
        })?;
        state.pending = None;
        if status == OperatorToolStatus::TimedOut {
            // A timed-out result is never confirmed and never fed back into the
            // agent loop. Cancel the open dialog (the extension aborts on a
            // refused result), then abort pi and hold until the owned termination
            // the Server proves.
            let _ = self.send(&json!({
                "type": "extension_ui_response",
                "id": dialog,
                "cancelled": true,
            }));
            self.abort_quietly();
            return Err(Failure::ToolTimedOut);
        }
        // Durable result before the acknowledgement, before the next model step.
        self.send(&json!({
            "type": "extension_ui_response",
            "id": dialog,
            "confirmed": true,
        }))
        .map_err(|_| Failure::Uncertain)
    }

    fn event(
        &mut self,
        journal: &mut Journal<'_>,
        state: &mut TurnState,
        event: Value,
    ) -> std::result::Result<(), Failure> {
        match event["type"].as_str() {
            Some("message_start") => {
                if event["message"]["role"] == "assistant" {
                    state.reset_stream();
                }
            }
            Some("message_update") => {
                // Pi's compaction summary is its own model request, never part of
                // the user-visible assistant conversation.
                if !state.compacting {
                    let update = &event["assistantMessageEvent"];
                    match update["type"].as_str() {
                        Some("text_delta") => {
                            state.stream_delta(journal, DisplayKind::Answer, update)?
                        }
                        Some("thinking_delta") => {
                            state.stream_delta(journal, DisplayKind::Thinking, update)?
                        }
                        Some("thinking_end") => {
                            if let Some(content) = update["content"].as_str() {
                                let index = content_index(update)?;
                                state.end_thinking(journal, index, content)?;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some("message_end") => {
                if !state.compacting && event["message"]["role"] == "assistant" {
                    end_assistant(&event["message"], journal, state)?;
                }
            }
            Some("agent_settled") => state.settled = true,
            // Ordinary lifecycle notifications are never journaled (the durable
            // call/result dialogs are the receipt), but an execution that ends
            // without the OVRCR admission dialog proves Pi refused it before the
            // extension hook (unknown tool or invalid arguments). That must fail
            // closed instead of reaching the model with no durable receipt.
            Some("tool_execution_end") => {
                let id = event["toolCallId"]
                    .as_str()
                    .filter(|id| valid_token(id))
                    .ok_or(Failure::Failed(REQUEST_FAILED))?;
                if state
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.id == id)
                    || !state.admitted.iter().any(|admitted| admitted == id)
                {
                    return Err(Failure::Failed(REQUEST_FAILED));
                }
                state.admitted.retain(|admitted| admitted != id);
            }
            Some("extension_ui_request") => self.dialog(journal, state, &event)?,
            Some("extension_error") => {
                return Err(Failure::Failed(if state.prompted {
                    REQUEST_FAILED
                } else {
                    INTEGRATION_UNAVAILABLE
                }));
            }
            // pi 0.87.1's AgentSession emits these exact session event names and
            // RPC mode forwards them unchanged (not auto_compaction_*).
            Some("compaction_start") => {
                // Make the visible prefix durable before the pause, then isolate
                // the compaction request from the next assistant message.
                journal.flush()?;
                state.reset_stream();
                state.compacting = true;
                journal.commit(OperatorWorkerEvent::CompactionStarted)?;
            }
            Some("compaction_end") => {
                if event.get("errorMessage").is_some() || event["aborted"] == true {
                    return Err(Failure::Failed(REQUEST_FAILED));
                }
                state.compacting = false;
                state.reset_stream();
                journal.commit(OperatorWorkerEvent::CompactionEnded)?;
            }
            Some("auto_retry_start") => return Err(Failure::Failed(REQUEST_FAILED)),
            Some("auto_retry_end") if event["success"] == false => {
                return Err(Failure::Failed(REQUEST_FAILED));
            }
            Some("summarization_retry_finished") => {}
            // Ordinary notifications, queue updates, partial tool updates and
            // duplicate execution notifications are not journaled.
            _ => {}
        }
        Ok(())
    }

    fn dialog(
        &mut self,
        journal: &mut Journal<'_>,
        state: &mut TurnState,
        event: &Value,
    ) -> std::result::Result<(), Failure> {
        let method = event["method"].as_str().unwrap_or("");
        if method == "setStatus" {
            if event["statusKey"].as_str() == Some(CONTRACT_KEY) {
                let text = event["statusText"]
                    .as_str()
                    .ok_or(Failure::Failed(INTEGRATION_UNAVAILABLE))?;
                validate_contract(text, &state.session_id)?;
                state.ready_count += 1;
            }
            // A duplicate binding is harmless; unrelated status entries are display only.
            return Ok(());
        }
        if !matches!(method, "confirm" | "select" | "input" | "editor") {
            // notify, setWidget, setTitle, set_editor_text and unknown display methods.
            return Ok(());
        }
        if method == "confirm" {
            match event["title"].as_str() {
                Some(ADMISSION_TITLE) => {
                    return self.flush_admission(journal, state, event.clone());
                }
                Some(RESULT_TITLE) => {
                    return self.flush_result(journal, state, event.clone());
                }
                _ => {}
            }
        }
        // Only the two exact integration dialogs may be answered.
        if let Some(dialog) = event["id"].as_str() {
            let _ = self.send(&json!({
                "type": "extension_ui_response",
                "id": dialog,
                "cancelled": true,
            }));
        }
        Err(Failure::Failed(UNSUPPORTED_INTERACTION))
    }

    /// Stream until `agent_settled`. Prompt execution has no added wall-clock deadline.
    fn stream(
        &mut self,
        journal: &mut Journal<'_>,
        state: &mut TurnState,
    ) -> std::result::Result<(), Failure> {
        loop {
            match self.next_message(journal, None)? {
                RpcMessage::Event(event) => {
                    self.event(journal, state, event)?;
                    if state.settled {
                        journal.flush()?;
                        return Ok(());
                    }
                }
                // `prompt` acceptance was already consumed; no other response belongs here.
                RpcMessage::Response(_) => return Err(Failure::Failed(REQUEST_FAILED)),
            }
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        if self.child.is_some() {
            self.abort_quietly();
        }
    }
}

struct PendingCall {
    id: String,
}

struct TurnState {
    session_id: String,
    workdir: PathBuf,
    /// Trusted-extension binding notifications seen (a switch binds again).
    ready_count: usize,
    default_model: Option<OperatorModel>,
    current_model: Option<OperatorModel>,
    session_file: Option<PathBuf>,
    /// Display text already streamed for the current assistant message, keyed by
    /// pi's content index. One content block has exactly one display kind.
    stream_blocks: BTreeMap<usize, StreamBlock>,
    /// Index of the most recent display event, used only to flush on a content
    /// block change. Providers may interleave updates to existing blocks.
    last_stream_index: Option<usize>,
    /// Highest content index that has displayed anything. A final-only suffix for
    /// a lower block cannot be appended after that later block.
    max_display_index: Option<usize>,
    /// An end-only thinking block awaits final redaction metadata. Subsequent
    /// display is held in arrival order so the final message can release it
    /// after deciding whether the thought is redacted.
    deferred_display: bool,
    deferred_runs: Vec<DeferredRun>,
    deferred_bytes: usize,
    /// Pi's automatic compaction can replace context between assistant messages.
    /// Its summary stream is never journaled and never shares message state.
    compacting: bool,
    last_assistant: Option<AssistantTurn>,
    /// Admitted tool-call ids whose ordinary execution end has not arrived yet.
    admitted: Vec<String>,
    pending: Option<PendingCall>,
    settled: bool,
    prompted: bool,
}

struct DeferredRun {
    index: usize,
    kind: DisplayKind,
    /// Byte range in that stream block's text.
    start: usize,
    end: usize,
    /// True only for an end-only thinking block that final redaction may discard.
    end_only: bool,
}

struct StreamBlock {
    kind: DisplayKind,
    /// All streamed text for this block, including `held`.
    text: String,
    /// Trailing whitespace already streamed but not journaled yet. An ending or
    /// the final message decides whether it is a real suffix or a synthetic
    /// separator pi retracts.
    held: String,
    /// Authoritative block text from a provider content-ending event, when one
    /// was emitted. pi's OpenAI Responses adapter may normalize trailing
    /// separators after the last streamed delta.
    authoritative: Option<String>,
    /// Whether any part of this block has entered the journal.
    displayed: bool,
}

impl TurnState {
    fn new(session_id: String, workdir: PathBuf) -> Self {
        Self {
            session_id,
            workdir,
            ready_count: 0,
            default_model: None,
            current_model: None,
            session_file: None,
            stream_blocks: BTreeMap::new(),
            last_stream_index: None,
            max_display_index: None,
            deferred_display: false,
            deferred_runs: Vec::new(),
            deferred_bytes: 0,
            compacting: false,
            last_assistant: None,
            admitted: Vec::new(),
            pending: None,
            settled: false,
            prompted: false,
        }
    }

    fn reset_stream(&mut self) {
        self.stream_blocks.clear();
        self.last_stream_index = None;
        self.max_display_index = None;
        self.deferred_display = false;
        self.deferred_runs.clear();
        self.deferred_bytes = 0;
    }

    fn note_display(&mut self, index: usize) {
        self.max_display_index = Some(self.max_display_index.map_or(index, |max| max.max(index)));
    }

    /// Record one contiguous deferred display segment by offset. Adjacent deltas
    /// of the same block reuse the previous range, so no delta text is copied.
    fn defer_run(
        &mut self,
        index: usize,
        kind: DisplayKind,
        start: usize,
        end: usize,
        end_only: bool,
    ) -> std::result::Result<(), Failure> {
        self.deferred_bytes = self
            .deferred_bytes
            .saturating_add(end.saturating_sub(start));
        if self.deferred_bytes > MAX_DEFERRED_BYTES {
            return Err(Failure::Failed(REQUEST_FAILED));
        }
        if let Some(last) = self.deferred_runs.last_mut()
            && !end_only
            && !last.end_only
            && last.index == index
            && last.kind == kind
            && last.end == start
        {
            last.end = end;
            return Ok(());
        }
        if self.deferred_runs.len() >= MAX_CONTENT_BLOCKS {
            return Err(Failure::Failed(REQUEST_FAILED));
        }
        self.deferred_runs.push(DeferredRun {
            index,
            kind,
            start,
            end,
            end_only,
        });
        Ok(())
    }

    /// Truncate buffered ranges after an authoritative ending retracts only a
    /// trailing separator. The substantive displayed prefix is untouched.
    fn trim_deferred_runs(&mut self, index: usize, new_len: usize) {
        let mut removed = 0usize;
        self.deferred_runs.retain_mut(|run| {
            if run.index != index {
                return true;
            }
            if run.start >= new_len {
                removed = removed.saturating_add(run.end - run.start);
                return false;
            }
            let old_end = run.end;
            run.end = run.end.min(new_len);
            removed = removed.saturating_add(old_end - run.end);
            run.start < run.end
        });
        self.deferred_bytes = self.deferred_bytes.saturating_sub(removed);
    }

    /// Record one display delta at pi's content index and journal it. A new
    /// content block flushes the previous block first, so a final-only suffix for
    /// an earlier block can never be appended after a later block was displayed.
    fn flush_at_index(
        &mut self,
        journal: &mut Journal<'_>,
        index: usize,
    ) -> std::result::Result<(), Failure> {
        if self.last_stream_index != Some(index) {
            journal.flush()?;
            self.last_stream_index = Some(index);
        }
        Ok(())
    }

    /// Whitespace already streamed before a different block is real content, not
    /// a separator an authoritative ending may retract. Release it in arrival
    /// order before the next block displays.
    fn release_held(
        &mut self,
        journal: &mut Journal<'_>,
        index: usize,
    ) -> std::result::Result<(), Failure> {
        let held = match self.stream_blocks.get_mut(&index) {
            Some(block) if !block.held.is_empty() => std::mem::take(&mut block.held),
            _ => return Ok(()),
        };
        self.flush_at_index(journal, index)?;
        self.note_display(index);
        if let Some(block) = self.stream_blocks.get_mut(&index) {
            block.displayed = true;
        }
        journal.push(DisplayKind::Thinking, &held)
    }

    fn stream_delta(
        &mut self,
        journal: &mut Journal<'_>,
        kind: DisplayKind,
        update: &Value,
    ) -> std::result::Result<(), Failure> {
        let delta = update["delta"].as_str().unwrap_or("");
        if delta.is_empty() {
            return Ok(());
        }
        let index = content_index(update)?;
        if self.deferred_display {
            let (start, end) = {
                let block = self
                    .stream_blocks
                    .entry(index)
                    .or_insert_with(|| StreamBlock {
                        kind,
                        text: String::new(),
                        held: String::new(),
                        authoritative: None,
                        displayed: false,
                    });
                if block.kind != kind || block.authoritative.is_some() || block.displayed {
                    return Err(Failure::Failed(REQUEST_FAILED));
                }
                let start = block.text.len();
                block.text.push_str(delta);
                (start, start + delta.len())
            };
            return self.defer_run(index, kind, start, end, false);
        }
        if let Some(last) = self.last_stream_index
            && last != index
        {
            self.release_held(journal, last)?;
        }
        if kind == DisplayKind::Answer {
            {
                let block = self
                    .stream_blocks
                    .entry(index)
                    .or_insert_with(|| StreamBlock {
                        kind,
                        text: String::new(),
                        held: String::new(),
                        authoritative: None,
                        displayed: false,
                    });
                if block.kind != kind || block.authoritative.is_some() {
                    return Err(Failure::Failed(REQUEST_FAILED));
                }
                block.text.push_str(delta);
                block.displayed = true;
            }
            self.flush_at_index(journal, index)?;
            self.note_display(index);
            return journal.push(kind, delta);
        }

        // Hold trailing thinking whitespace. pi's OpenAI Responses adapter emits
        // "\n\n" between reasoning summary parts and then retracts the final one
        // when the authoritative ending replaces the streamed text.
        let release = {
            let block = self
                .stream_blocks
                .entry(index)
                .or_insert_with(|| StreamBlock {
                    kind,
                    text: String::new(),
                    held: String::new(),
                    authoritative: None,
                    displayed: false,
                });
            if block.kind != kind || block.authoritative.is_some() {
                return Err(Failure::Failed(REQUEST_FAILED));
            }
            block.text.push_str(delta);
            let body = delta.trim_end_matches(char::is_whitespace);
            if body.is_empty() {
                block.held.push_str(delta);
                return Ok(());
            }
            block.displayed = true;
            let body_len = body.len();
            let mut visible = std::mem::take(&mut block.held);
            visible.push_str(&delta[..body_len]);
            block.held.push_str(&delta[body_len..]);
            visible
        };
        self.flush_at_index(journal, index)?;
        self.note_display(index);
        journal.push(kind, &release)
    }

    /// Reconcile one content block against an authoritative ending or the final
    /// message. Thinking may have trailing whitespace held back; the authority
    /// either keeps it, truncates it, or extends it. A substantive rewrite of
    /// already displayed text fails closed.
    fn reconcile_stream(
        &mut self,
        journal: &mut Journal<'_>,
        index: usize,
        text: &str,
        authoritative: bool,
    ) -> std::result::Result<(), Failure> {
        let mut emit = None;
        let mut late = false;
        let kind;
        {
            let block = self
                .stream_blocks
                .get_mut(&index)
                .ok_or(Failure::Failed(REQUEST_FAILED))?;
            kind = block.kind;
            if let Some(stored) = &block.authoritative {
                return if text == stored.as_str() {
                    Ok(())
                } else {
                    Err(Failure::Failed(REQUEST_FAILED))
                };
            }
            match kind {
                DisplayKind::Thinking => {
                    if block.held.is_empty() {
                        if !text.starts_with(block.text.as_str()) {
                            return Err(Failure::Failed(REQUEST_FAILED));
                        }
                        if text.len() > block.text.len() {
                            emit = Some(text[block.text.len()..].to_owned());
                            late = true;
                        }
                    } else {
                        let displayed = block.text.len() - block.held.len();
                        if text.len() < displayed || !text.starts_with(&block.text[..displayed]) {
                            return Err(Failure::Failed(REQUEST_FAILED));
                        }
                        let suffix = &text[displayed..];
                        let accepted = if suffix.len() <= block.held.len() {
                            block.held.starts_with(suffix)
                        } else {
                            suffix.starts_with(block.held.as_str())
                        };
                        if !accepted {
                            return Err(Failure::Failed(REQUEST_FAILED));
                        }
                        if !suffix.is_empty() {
                            // Releasing already-streamed whitespace is in arrival
                            // order even if another block has since displayed.
                            late = suffix.len() > block.held.len();
                            emit = Some(suffix.to_owned());
                        }
                    }
                }
                DisplayKind::Answer => {
                    if !text.starts_with(block.text.as_str()) {
                        return Err(Failure::Failed(REQUEST_FAILED));
                    }
                    if text.len() > block.text.len() {
                        emit = Some(text[block.text.len()..].to_owned());
                        late = true;
                    }
                }
            }
            block.text.clear();
            block.text.push_str(text);
            block.held.clear();
            if authoritative {
                block.authoritative = Some(text.to_owned());
            }
        }
        if let Some(suffix) = emit {
            if late && self.max_display_index.is_some_and(|max| max > index) {
                return Err(Failure::Failed(REQUEST_FAILED));
            }
            self.flush_at_index(journal, index)?;
            self.note_display(index);
            journal.push(kind, &suffix)?;
            if let Some(block) = self.stream_blocks.get_mut(&index) {
                block.displayed = true;
            }
        }
        Ok(())
    }

    /// Apply pi's authoritative `thinking_end` content. An empty ending with no
    /// block creates nothing; an end-only ending is deferred until the final
    /// block decides whether it is redacted.
    fn end_thinking(
        &mut self,
        journal: &mut Journal<'_>,
        index: usize,
        content: &str,
    ) -> std::result::Result<(), Failure> {
        if !self.stream_blocks.contains_key(&index) && content.is_empty() {
            return Ok(());
        }
        let defer = {
            let block = self
                .stream_blocks
                .entry(index)
                .or_insert_with(|| StreamBlock {
                    kind: DisplayKind::Thinking,
                    text: String::new(),
                    held: String::new(),
                    authoritative: None,
                    displayed: false,
                });
            if block.kind != DisplayKind::Thinking {
                return Err(Failure::Failed(REQUEST_FAILED));
            }
            !content.is_empty()
                && !block.displayed
                && block.text.is_empty()
                && block.held.is_empty()
                && block.authoritative.is_none()
        };
        if defer {
            // pi's Anthropic adapter can end with a redacted placeholder before
            // the final block says `redacted: true`. Hold this and all following
            // display until the final metadata decides.
            if let Some(last) = self.last_stream_index {
                self.release_held(journal, last)?;
            }
            let len = {
                let block = self
                    .stream_blocks
                    .get_mut(&index)
                    .ok_or(Failure::Failed(REQUEST_FAILED))?;
                block.text.push_str(content);
                block.authoritative = Some(content.to_owned());
                block.text.len()
            };
            self.deferred_display = true;
            self.defer_run(index, DisplayKind::Thinking, 0, len, true)?;
            return Ok(());
        }
        if self.deferred_display {
            let mut extension = None;
            let mut trim_to = None;
            {
                let block = self
                    .stream_blocks
                    .get_mut(&index)
                    .ok_or(Failure::Failed(REQUEST_FAILED))?;
                if !block.displayed {
                    if let Some(stored) = &block.authoritative {
                        return if stored == content {
                            Ok(())
                        } else {
                            Err(Failure::Failed(REQUEST_FAILED))
                        };
                    }
                    if content == block.text {
                        block.authoritative = Some(content.to_owned());
                        return Ok(());
                    }
                    if content.starts_with(block.text.as_str()) {
                        let start = block.text.len();
                        let end = content.len();
                        block.text.push_str(&content[start..]);
                        block.authoritative = Some(content.to_owned());
                        extension = Some((start, end));
                    } else if block.text.starts_with(content)
                        && block.text[content.len()..].chars().all(char::is_whitespace)
                    {
                        let new_len = content.len();
                        block.text.truncate(new_len);
                        block.held.clear();
                        block.authoritative = Some(content.to_owned());
                        trim_to = Some(new_len);
                    } else {
                        return Err(Failure::Failed(REQUEST_FAILED));
                    }
                }
            }
            if let Some((start, end)) = extension {
                return self.defer_run(index, DisplayKind::Thinking, start, end, false);
            }
            if let Some(new_len) = trim_to {
                self.trim_deferred_runs(index, new_len);
                return Ok(());
            }
        }
        self.reconcile_stream(journal, index, content, true)
    }

    fn capture(&mut self, data: &Value) -> std::result::Result<(), Failure> {
        let session_id = data["sessionId"]
            .as_str()
            .ok_or(Failure::Failed(CONTEXT_UNAVAILABLE))?;
        if session_id != self.session_id {
            return Err(Failure::Failed(CONTEXT_UNAVAILABLE));
        }
        self.session_file = data["sessionFile"].as_str().map(PathBuf::from);
        self.current_model = operator_model(&data["model"]);
        Ok(())
    }

    /// The first fresh-session state also fixes the profile defaults used for this turn.
    fn capture_fresh(&mut self, data: &Value) -> std::result::Result<(), Failure> {
        self.capture(data)?;
        self.default_model = self.current_model.clone();
        Ok(())
    }

    fn last_assistant_stop(&self) -> Option<String> {
        self.last_assistant.as_ref().map(|turn| turn.stop.clone())
    }
}

struct AssistantTurn {
    stop: String,
    calls: Vec<ToolCallBlock>,
}

struct ToolCallBlock {
    id: String,
    name: String,
    arguments: Value,
}

fn operator_model(value: &Value) -> Option<OperatorModel> {
    // Pi reports a placeholder model object (provider/id `unknown`) when the
    // dedicated profile has no model selected; that is no model, not a choice.
    let provider = value["provider"].as_str()?;
    let id = value["id"].as_str()?;
    if provider.is_empty() || id.is_empty() || provider == "unknown" || id == "unknown" {
        return None;
    }
    Some(OperatorModel {
        provider: provider.to_owned(),
        id: id.to_owned(),
    })
}

/// Pi's assistant message events name their content block. A missing or
/// non-integer index cannot be related to the final message, so it fails closed.
fn content_index(update: &Value) -> std::result::Result<usize, Failure> {
    update
        .get("contentIndex")
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < MAX_CONTENT_BLOCKS)
        .ok_or(Failure::Failed(REQUEST_FAILED))
}

fn require_success(response: &RpcResponse) -> std::result::Result<(), Failure> {
    if response.success {
        Ok(())
    } else {
        Err(Failure::Failed(REQUEST_FAILED))
    }
}

/// An extension dialog the worker must answer (or refuse).
fn is_dialog(event: &Value) -> bool {
    event["type"] == "extension_ui_request"
        && event["method"]
            .as_str()
            .is_some_and(|method| matches!(method, "confirm" | "select" | "input" | "editor"))
}

/// The child driver reports the ordinary end of Pi's stdout as a request-level
/// error. Every other request error (a malformed record, an unreadable stream)
/// is a protocol fault that cannot end a successful turn.
fn clean_stream_end(error: &pi::PiError) -> bool {
    error.kind() == pi::PiErrorKind::Request
        && error.detail() == "Pi closed its output before a reply"
}

fn prompt_text(start: &OperatorWorkerStart) -> std::result::Result<String, Failure> {
    let context =
        serde_json::to_string(&start.context).map_err(|_| Failure::Failed(REQUEST_FAILED))?;
    // The captured context is always ahead of the user text so input such as `/new`
    // cannot become a native pi command.
    Ok(format!(
        "Captured OVRCR context (identities are fixed for this turn):\n{context}\n\nUser request:\n{}",
        start.text
    ))
}

// ---------------------------------------------------------------- steps

fn create_attempt(paths: &PiPaths, attempt: &Path) -> std::result::Result<(), Failure> {
    fs::create_dir_all(&paths.turns).map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    match DirBuilder::new().mode(0o700).create(attempt) {
        Ok(()) => Ok(()),
        // An existing directory is an uncertain prior attempt, never permission to replay.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            Err(Failure::Failed(CONTEXT_UNAVAILABLE))
        }
        Err(_) => Err(Failure::Failed(CONTEXT_UNAVAILABLE)),
    }
}

fn restore_checkpoint(
    connection: &mut Connection,
    journal: &mut Journal<'_>,
    state: &mut TurnState,
    paths: &PiPaths,
    attempt_dir: &Path,
    start: &OperatorWorkerStart,
    previous: OperatorTurnId,
) -> std::result::Result<(), Failure> {
    if previous == start.identity.turn || previous.0 == 0 {
        return Err(Failure::Failed(CONTEXT_UNAVAILABLE));
    }
    let source = paths.attempt_dir(previous).join("session.jsonl");
    let target = attempt_dir.join("session.jsonl");
    copy_checkpoint(&source, &target, &state.session_id, &state.workdir)?;
    let before = state.ready_count;
    let response = connection.request(
        journal,
        state,
        "switch_session",
        json!({ "sessionPath": path_string(&target)? }),
        CONTROL_DEADLINE,
    )?;
    if !response.success || response.data["cancelled"].as_bool() != Some(false) {
        return Err(Failure::Failed(CONTEXT_UNAVAILABLE));
    }
    // The switched session must bind the trusted extension again.
    let until = Instant::now() + CONTRACT_DEADLINE;
    while state.ready_count == before {
        let message = match connection.next_message(journal, Some(until)) {
            Ok(message) => message,
            Err(Failure::Failed(_)) => return Err(Failure::Failed(CONTEXT_UNAVAILABLE)),
            Err(failure) => return Err(failure),
        };
        match message {
            RpcMessage::Event(event) => connection.event(journal, state, event)?,
            RpcMessage::Response(_) => {
                return Err(Failure::Failed(CONTEXT_UNAVAILABLE));
            }
        }
    }
    let response = connection.request(journal, state, "get_state", json!({}), CONTROL_DEADLINE)?;
    require_success(&response)?;
    state.capture(&response.data)?;
    match state.session_file.as_deref() {
        Some(reported) => match (reported.canonicalize(), target.canonicalize()) {
            (Ok(reported), Ok(expected)) if reported == expected => Ok(()),
            _ => Err(Failure::Failed(CONTEXT_UNAVAILABLE)),
        },
        None => Err(Failure::Failed(CONTEXT_UNAVAILABLE)),
    }
}

/// One model-configuration step whose structured refusal is a profile/model problem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelStep {
    /// The dedicated profile's discoverable model set.
    Catalog,
    /// Applying the exact selected model.
    Model,
}

/// A refused model step is a configuration answer, never a transport failure: with
/// no explicit `operator.model` the profile has no usable model, so setup guidance
/// is the only correct message; a chosen model that cannot be applied is
/// unavailable. Only the structured `success` field decides.
fn refused_model_step(step: ModelStep, explicit: bool) -> Failure {
    match step {
        ModelStep::Catalog if !explicit => Failure::Failed(NO_MODEL),
        _ => Failure::Failed(MODEL_UNAVAILABLE),
    }
}

/// Classify one model-step reply. A transport/protocol error is passed through
/// unchanged (REQUEST_FAILED, never a model message); a structured refusal maps
/// through [`refused_model_step`].
fn model_step_outcome(
    result: std::result::Result<RpcResponse, Failure>,
    step: ModelStep,
    explicit: bool,
) -> std::result::Result<RpcResponse, Failure> {
    match result {
        Err(failure) => Err(failure),
        Ok(response) if response.success => Ok(response),
        Ok(_) => Err(refused_model_step(step, explicit)),
    }
}

fn resolve_model(
    connection: &mut Connection,
    journal: &mut Journal<'_>,
    state: &mut TurnState,
    selection: Option<&str>,
) -> std::result::Result<OperatorModel, Failure> {
    let explicit = selection.is_some();
    let desired = match selection {
        Some(selection) => {
            OperatorModel::parse(selection).map_err(|_| Failure::Failed(MODEL_UNAVAILABLE))?
        }
        None => state
            .default_model
            .clone()
            .ok_or(Failure::Failed(NO_MODEL))?,
    };
    let response = model_step_outcome(
        connection.request(
            journal,
            state,
            "get_available_models",
            json!({}),
            CONTROL_DEADLINE,
        ),
        ModelStep::Catalog,
        explicit,
    )?;
    let available = response.data["models"]
        .as_array()
        // A success reply without the catalog array is a protocol fault, not a
        // model configuration answer.
        .ok_or(Failure::Failed(REQUEST_FAILED))?;
    let found = available.iter().take(MAX_MODELS).any(|model| {
        model["provider"].as_str() == Some(desired.provider.as_str())
            && model["id"].as_str() == Some(desired.id.as_str())
    });
    if !found {
        return Err(Failure::Failed(MODEL_UNAVAILABLE));
    }
    let response = model_step_outcome(
        connection.request(
            journal,
            state,
            "set_model",
            json!({ "provider": &desired.provider, "modelId": &desired.id }),
            CONTROL_DEADLINE,
        ),
        ModelStep::Model,
        explicit,
    )?;
    if operator_model(&response.data).as_ref() != Some(&desired) {
        // The command succeeded but a different model is active.
        return Err(Failure::Failed(MODEL_UNAVAILABLE));
    }
    for (command, params) in [
        ("set_auto_retry", json!({ "enabled": false })),
        ("set_auto_compaction", json!({ "enabled": true })),
    ] {
        require_success(&connection.request(journal, state, command, params, CONTROL_DEADLINE)?)?;
    }
    let response = connection.request(journal, state, "get_state", json!({}), CONTROL_DEADLINE)?;
    require_success(&response)?;
    state.capture(&response.data)?;
    if state.current_model.as_ref() != Some(&desired) {
        return Err(Failure::Failed(MODEL_UNAVAILABLE));
    }
    Ok(desired)
}

fn resolve_reasoning(
    connection: &mut Connection,
    journal: &mut Journal<'_>,
    state: &mut TurnState,
    requested: ovrcr_protocol::OperatorThinkingLevel,
) -> std::result::Result<ovrcr_protocol::OperatorReasoning, Failure> {
    if requested != ovrcr_protocol::OperatorThinkingLevel::Default {
        require_success(&connection.request(
            journal,
            state,
            "set_thinking_level",
            json!({ "level": requested.as_str() }),
            CONTROL_DEADLINE,
        )?)?;
    }
    // Pi reports success even when it substitutes another supported level.
    let response = connection.request(journal, state, "get_state", json!({}), CONTROL_DEADLINE)?;
    require_success(&response)?;
    state.capture(&response.data)?;
    let level = serde_json::from_value(response.data["thinkingLevel"].clone())
        .map_err(|_| Failure::Failed(REQUEST_FAILED))?;
    let response = connection.request(
        journal,
        state,
        "get_available_thinking_levels",
        json!({}),
        CONTROL_DEADLINE,
    )?;
    require_success(&response)?;
    let available = serde_json::from_value(response.data["levels"].clone())
        .map_err(|_| Failure::Failed(REQUEST_FAILED))?;
    let reasoning = ovrcr_protocol::OperatorReasoning {
        requested,
        level,
        available,
    };
    reasoning
        .validate()
        .map_err(|_| Failure::Failed(REQUEST_FAILED))?;
    Ok(reasoning)
}

fn delete_previous_checkpoint(paths: &PiPaths, start: &OperatorWorkerStart) {
    let Some(previous) = start.checkpoint_turn else {
        return;
    };
    if previous == start.identity.turn || previous.0 == 0 {
        return;
    }
    let directory = paths.attempt_dir(previous);
    let _ = fs::remove_file(directory.join("session.jsonl"));
    // Only an empty superseded attempt directory is removed.
    let _ = fs::remove_dir(&directory);
}

// ---------------------------------------------------------------- prompt events

fn end_assistant(
    message: &Value,
    journal: &mut Journal<'_>,
    state: &mut TurnState,
) -> std::result::Result<(), Failure> {
    let stop = message["stopReason"].as_str().unwrap_or("pending");
    let blocks = message["content"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if blocks.len() > MAX_CONTENT_BLOCKS {
        return Err(Failure::Failed(REQUEST_FAILED));
    }
    let mut calls = Vec::new();
    let mut matched = vec![false; blocks.len()];
    let mut deferred_drop = Vec::new();
    let mut deferred_final_only = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        let (kind, text) = match block["type"].as_str() {
            Some("text") => (DisplayKind::Answer, block["text"].as_str().unwrap_or("")),
            // Only displayable thinking crosses the journal. Signatures and
            // redacted reasoning remain provider-private even when pi keeps the
            // redacted block in the assistant content array.
            Some("thinking") if block["redacted"].as_bool() == Some(true) => {
                if let Some(streamed) = state.stream_blocks.get(&index) {
                    if streamed.displayed {
                        return Err(Failure::Failed(REQUEST_FAILED));
                    }
                    // An end-only placeholder is discarded by this metadata.
                    matched[index] = true;
                    if state.deferred_display {
                        deferred_drop.push(index);
                    }
                }
                continue;
            }
            Some("thinking") => (
                DisplayKind::Thinking,
                block["thinking"].as_str().unwrap_or(""),
            ),
            Some("toolCall") => {
                if state.stream_blocks.contains_key(&index) {
                    return Err(Failure::Failed(REQUEST_FAILED));
                }
                if let (Some(id), Some(name)) = (block["id"].as_str(), block["name"].as_str()) {
                    calls.push(ToolCallBlock {
                        id: id.to_owned(),
                        name: name.to_owned(),
                        arguments: block["arguments"].clone(),
                    });
                }
                continue;
            }
            _ => {
                // Redacted/opaque blocks and unknown block types never display;
                // a display stream for one of them is contradictory.
                if state.stream_blocks.contains_key(&index) {
                    return Err(Failure::Failed(REQUEST_FAILED));
                }
                continue;
            }
        };
        matched[index] = true;
        if let Some(streamed) = state.stream_blocks.get(&index) {
            if streamed.kind != kind {
                return Err(Failure::Failed(REQUEST_FAILED));
            }
            if state.deferred_display && !streamed.displayed {
                let extension = match &streamed.authoritative {
                    Some(auth) => {
                        if auth == text {
                            None
                        } else {
                            return Err(Failure::Failed(REQUEST_FAILED));
                        }
                    }
                    None if text.starts_with(streamed.text.as_str()) => (text.len()
                        > streamed.text.len())
                    .then(|| text[streamed.text.len()..].to_owned()),
                    None => return Err(Failure::Failed(REQUEST_FAILED)),
                };
                if let Some(extension) = extension {
                    deferred_final_only.push((kind, extension));
                }
                // Buffered ranges are emitted in arrival order after this pass.
                continue;
            }
            let deferred = !streamed.displayed
                && !text.is_empty()
                && streamed.authoritative.as_deref() == Some(text);
            state.reconcile_stream(journal, index, text, false)?;
            if deferred {
                if state.max_display_index.is_some_and(|max| max > index) {
                    return Err(Failure::Failed(REQUEST_FAILED));
                }
                state.flush_at_index(journal, index)?;
                state.note_display(index);
                journal.push(kind, text)?;
                if let Some(block) = state.stream_blocks.get_mut(&index) {
                    block.displayed = true;
                }
            }
            continue;
        }
        if !text.is_empty() {
            // A final-only block cannot be appended after a later block already
            // displayed; that would put earlier content after later output.
            if state.max_display_index.is_some_and(|max| max > index) {
                return Err(Failure::Failed(REQUEST_FAILED));
            }
            if state.deferred_display {
                deferred_final_only.push((kind, text.to_owned()));
            } else {
                journal.push(kind, text)?;
            }
        }
    }
    if state.deferred_display {
        for run in std::mem::take(&mut state.deferred_runs) {
            if deferred_drop.contains(&run.index) {
                continue;
            }
            let text = state
                .stream_blocks
                .get(&run.index)
                .and_then(|block| block.text.get(run.start..run.end))
                .ok_or(Failure::Failed(REQUEST_FAILED))?;
            journal.push(run.kind, text)?;
            if let Some(block) = state.stream_blocks.get_mut(&run.index) {
                block.displayed = true;
            }
        }
        for (kind, text) in deferred_final_only {
            journal.push(kind, &text)?;
        }
    }
    // Every streamed block must exist in the same position of the final message.
    if state
        .stream_blocks
        .keys()
        .any(|index| !matched.get(*index).copied().unwrap_or(false))
    {
        return Err(Failure::Failed(REQUEST_FAILED));
    }
    state.reset_stream();
    state.last_assistant = Some(AssistantTurn {
        stop: stop.to_owned(),
        calls,
    });
    match stop {
        // A non-empty partial/tool message is not completion.
        "stop" | "toolUse" | "pending" => Ok(()),
        _ => Err(Failure::Failed(REQUEST_FAILED)),
    }
}

fn admit(
    payload: &Value,
    state: &TurnState,
) -> std::result::Result<(String, String, String), Failure> {
    let id = payload["toolCallId"]
        .as_str()
        .ok_or(Failure::Failed(REQUEST_FAILED))?;
    let name = payload["toolName"]
        .as_str()
        .ok_or(Failure::Failed(REQUEST_FAILED))?;
    let input = &payload["input"];
    let arguments = serde_json::to_string(input).map_err(|_| Failure::Failed(REQUEST_FAILED))?;
    let well_formed = valid_token(id)
        && TOOLS.contains(&name)
        && input.is_object()
        && arguments.len() <= MAX_OPERATOR_ARGUMENT_BYTES
        && !arguments.contains('\0');
    // Admissions are serialized: the previous result must already be durable.
    let serial = state.pending.is_none();
    // Bound the admitted-call ledger; a peer that never ends calls cannot grow it.
    let bounded = state.admitted.len() < MAX_ADMITTED_CALLS;
    let matched = state.last_assistant.as_ref().is_some_and(|turn| {
        matches!(turn.stop.as_str(), "toolUse" | "stop")
            && turn
                .calls
                .iter()
                .any(|call| call.id == id && call.name == name && &call.arguments == input)
    });
    if !well_formed || !serial || !bounded || !matched {
        return Err(Failure::Failed(REQUEST_FAILED));
    }
    Ok((id.to_owned(), name.to_owned(), arguments))
}

fn valid_token(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn tool_status(value: &str) -> Option<OperatorToolStatus> {
    match value {
        "succeeded" => Some(OperatorToolStatus::Succeeded),
        "failed" => Some(OperatorToolStatus::Failed { exit_code: None }),
        "timedout" => Some(OperatorToolStatus::TimedOut),
        _ => None,
    }
}

fn sanitize_output(output: &str) -> (String, bool) {
    // Native text never carries NUL into the journal or the visible receipt.
    let replaced = output.replace('\0', "\u{FFFD}");
    let mut cut = replaced.len().min(MAX_TOOL_OUTPUT_BYTES);
    while cut > 0 && !replaced.is_char_boundary(cut) {
        cut -= 1;
    }
    let truncated = cut < replaced.len();
    (replaced[..cut].to_owned(), truncated)
}

fn validate_contract(text: &str, expected_session: &str) -> std::result::Result<(), Failure> {
    let contract: Value =
        serde_json::from_str(text).map_err(|_| Failure::Failed(INTEGRATION_UNAVAILABLE))?;
    if contract["version"].as_u64() != Some(1)
        || contract["sessionId"].as_str() != Some(expected_session)
    {
        return Err(Failure::Failed(INTEGRATION_UNAVAILABLE));
    }
    let tools = contract["tools"]
        .as_array()
        .ok_or(Failure::Failed(INTEGRATION_UNAVAILABLE))?;
    // Exactly the four native tools, each once.
    if tools.len() != TOOLS.len() {
        return Err(Failure::Failed(INTEGRATION_UNAVAILABLE));
    }
    for tool in TOOLS {
        if tools
            .iter()
            .filter(|value| value.as_str() == Some(tool))
            .count()
            != 1
        {
            return Err(Failure::Failed(INTEGRATION_UNAVAILABLE));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- native session

fn copy_checkpoint(
    source: &Path,
    target: &Path,
    expected_session: &str,
    workdir: &Path,
) -> std::result::Result<(), Failure> {
    let metadata =
        fs::symlink_metadata(source).map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    if !metadata.file_type().is_file()
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err(Failure::Failed(CONTEXT_UNAVAILABLE));
    }
    let input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    let mut reader = BufReader::new(input);
    let header = read_header(&mut reader)?;
    validate_header(&header, expected_session, workdir)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    if let Err(failure) = copy_records(&mut reader, &mut output, &header) {
        // A refused copy never leaves a partial session file behind; the
        // untouched source and the attempt directory remain as evidence.
        drop(output);
        let _ = fs::remove_file(target);
        return Err(failure);
    }
    fs::set_permissions(target, fs::Permissions::from_mode(0o600))
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    Ok(())
}

/// Validate every native session record while streaming it after the header.
///
/// Pi's session manager silently skips malformed JSON lines, so a checkpoint
/// that is valid only in its header would resume a shortened history. No complete
/// record is buffered above the bound, and the source file is never modified.
fn copy_records(
    reader: &mut BufReader<File>,
    output: &mut File,
    header: &[u8],
) -> std::result::Result<(), Failure> {
    output
        .write_all(header)
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader
            .take((MAX_CHECKPOINT_RECORD + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
        if read == 0 {
            break;
        }
        if line.len() > MAX_CHECKPOINT_RECORD || !line.ends_with(b"\n") {
            return Err(Failure::Failed(CONTEXT_UNAVAILABLE));
        }
        validate_record(&line)?;
        output
            .write_all(&line)
            .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    }
    output
        .sync_all()
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    Ok(())
}

/// One non-blank native session record must parse as a JSON object; Pi accepts
/// nothing less, so copying anything less would be trusting corrupt history.
fn validate_record(line: &[u8]) -> std::result::Result<(), Failure> {
    if line.iter().all(u8::is_ascii_whitespace) {
        // Pi skips blank lines; they carry no history.
        return Ok(());
    }
    let value: Value =
        serde_json::from_slice(line).map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    if value.is_object() {
        Ok(())
    } else {
        Err(Failure::Failed(CONTEXT_UNAVAILABLE))
    }
}

fn promote_session(
    attempt: &Path,
    native: &Path,
    expected_session: &str,
    workdir: &Path,
) -> std::result::Result<(), Failure> {
    let metadata =
        fs::symlink_metadata(native).map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    if !metadata.file_type().is_file()
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err(Failure::Failed(CONTEXT_UNAVAILABLE));
    }
    let input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(native)
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    let mut reader = BufReader::new(input);
    let header = read_header(&mut reader)?;
    validate_header(&header, expected_session, workdir)?;
    let root = attempt
        .canonicalize()
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    let native = native
        .canonicalize()
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    if !native.starts_with(&root) {
        return Err(Failure::Failed(CONTEXT_UNAVAILABLE));
    }
    let target = attempt.join("session.jsonl");
    if native != target {
        fs::rename(&native, &target).map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    }
    File::open(&target)
        .and_then(|file| file.sync_all())
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    File::open(attempt)
        .and_then(|file| file.sync_all())
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    Ok(())
}

fn read_header(reader: &mut BufReader<File>) -> std::result::Result<Vec<u8>, Failure> {
    let mut header = Vec::new();
    reader
        .by_ref()
        .take(MAX_HEADER_BYTES as u64)
        .read_until(b'\n', &mut header)
        .map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    if header.is_empty() || !header.ends_with(b"\n") {
        return Err(Failure::Failed(CONTEXT_UNAVAILABLE));
    }
    Ok(header)
}

fn validate_header(
    header: &[u8],
    expected_session: &str,
    workdir: &Path,
) -> std::result::Result<(), Failure> {
    let value: Value =
        serde_json::from_slice(header).map_err(|_| Failure::Failed(CONTEXT_UNAVAILABLE))?;
    let valid = value["type"] == "session"
        && value["id"].as_str() == Some(expected_session)
        && value["version"]
            .as_u64()
            .is_some_and(|version| (1..=3).contains(&version))
        && value["cwd"].as_str().is_some_and(|cwd| {
            let path = Path::new(cwd);
            path.is_absolute()
                && match (path.canonicalize(), workdir.canonicalize()) {
                    (Ok(captured), Ok(expected)) => captured == expected,
                    _ => false,
                }
        });
    if valid {
        Ok(())
    } else {
        Err(Failure::Failed(CONTEXT_UNAVAILABLE))
    }
}

fn path_string(path: &Path) -> std::result::Result<String, Failure> {
    path.to_str()
        .filter(|value| !value.contains('\0'))
        .map(str::to_owned)
        .ok_or(Failure::Failed(CONTEXT_UNAVAILABLE))
}

// ---------------------------------------------------------------- shutdown

#[derive(Clone)]
struct Termination {
    terminate: Arc<AtomicBool>,
    hangup: Arc<AtomicBool>,
}

impl Termination {
    /// SIGTERM/SIGHUP handlers let the group keep the worker alive long enough to
    /// abort pi. A bounded watchdog still exits inside the Server's termination grace
    /// when the main thread is blocked on a reporting reply.
    fn install() -> Self {
        let terminate = Arc::new(AtomicBool::new(false));
        let hangup = Arc::new(AtomicBool::new(false));
        let installed = signal_hook::flag::register(libc::SIGTERM, terminate.clone()).is_ok()
            && signal_hook::flag::register(libc::SIGHUP, hangup.clone()).is_ok();
        if installed {
            let watched = terminate.clone();
            let hup = hangup.clone();
            let _ = std::thread::Builder::new()
                .name("operator-shutdown".into())
                .spawn(move || {
                    loop {
                        if watched.load(Ordering::Acquire) || hup.load(Ordering::Acquire) {
                            std::thread::sleep(WATCHDOG_DELAY);
                            std::process::exit(0);
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                });
        }
        Self { terminate, hangup }
    }

    fn requested(&self) -> bool {
        self.terminate.load(Ordering::Acquire) || self.hangup.load(Ordering::Acquire)
    }
}

/// Only the Server's scoped termination (or an explicit Stop) finishes this worker.
fn hold() -> ! {
    loop {
        std::thread::park_timeout(Duration::from_secs(3600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_replaces_nul_and_bounds_utf8() {
        let (text, truncated) = sanitize_output("a\0b");
        assert_eq!(text, "a\u{FFFD}b");
        assert!(!truncated);
        let long = "é".repeat(MAX_TOOL_OUTPUT_BYTES);
        let (text, truncated) = sanitize_output(&long);
        assert!(truncated);
        assert!(text.len() <= MAX_TOOL_OUTPUT_BYTES);
        assert!(text.is_char_boundary(text.len()));
    }

    fn reply(command: &str, success: bool) -> RpcResponse {
        RpcResponse {
            id: "ovrcr-1".into(),
            command: command.into(),
            success,
            error: (!success).then(|| "structured refusal".into()),
            data: Value::Null,
        }
    }

    #[test]
    fn model_refusals_map_to_setup_guidance_and_transport_errors_do_not() {
        // No explicit operator.model: a refused catalog means the profile has no
        // usable model, which is the setup guidance, not a transport failure.
        assert_eq!(
            model_step_outcome(
                Ok(reply("get_available_models", false)),
                ModelStep::Catalog,
                false
            ),
            Err(Failure::Failed(NO_MODEL))
        );
        // An explicit operator.model that cannot be discovered/applied is unavailable.
        assert_eq!(
            model_step_outcome(
                Ok(reply("get_available_models", false)),
                ModelStep::Catalog,
                true
            ),
            Err(Failure::Failed(MODEL_UNAVAILABLE))
        );
        assert_eq!(
            model_step_outcome(Ok(reply("set_model", false)), ModelStep::Model, false),
            Err(Failure::Failed(MODEL_UNAVAILABLE))
        );
        assert_eq!(
            model_step_outcome(Ok(reply("set_model", false)), ModelStep::Model, true),
            Err(Failure::Failed(MODEL_UNAVAILABLE))
        );
        // A structured success is not a failure, whatever its command.
        assert!(
            model_step_outcome(
                Ok(reply("get_available_models", true)),
                ModelStep::Catalog,
                false
            )
            .is_ok()
        );
        // EOF/malformed/timeout arrive as Err and are never relabelled.
        for failure in [
            Failure::Failed(REQUEST_FAILED),
            Failure::ReportFailed,
            Failure::Terminated,
        ] {
            assert_eq!(
                model_step_outcome(Err(failure), ModelStep::Catalog, false),
                Err(failure)
            );
        }
    }

    #[test]
    fn placeholder_model_is_not_a_selection() {
        assert_eq!(
            operator_model(&json!({"provider": "unknown", "id": "unknown"})),
            None
        );
        assert_eq!(operator_model(&json!({})), None);
        assert_eq!(
            operator_model(&json!({"provider": "fixture", "id": "smoke"})),
            Some(OperatorModel {
                provider: "fixture".into(),
                id: "smoke".into()
            })
        );
    }

    #[test]
    fn checkpoint_header_needs_the_native_identity_and_cwd() {
        let root = tempfile::tempdir().unwrap();
        let workdir = root.path().join("operator-workdir");
        std::fs::create_dir(&workdir).unwrap();
        let header = |id: &str, cwd: &std::path::Path| {
            format!(
                "{{\"type\":\"session\",\"version\":3,\"id\":\"{id}\",\"timestamp\":\"2026-11-08T00:00:00.000Z\",\"cwd\":{}}}\n",
                serde_json::to_string(&cwd.display().to_string()).unwrap()
            )
        };
        let valid = header("operator-1", &workdir);
        assert!(validate_header(valid.as_bytes(), "operator-1", &workdir).is_ok());
        let wrong_id = header("other", &workdir);
        assert!(validate_header(wrong_id.as_bytes(), "operator-1", &workdir).is_err());
        let wrong_cwd = header("operator-1", root.path());
        assert!(validate_header(wrong_cwd.as_bytes(), "operator-1", &workdir).is_err());
        assert!(validate_header(b"not json\n", "operator-1", &workdir).is_err());
        assert!(validate_header(b"", "operator-1", &workdir).is_err());
    }

    #[test]
    fn clean_stream_end_matches_the_child_driver_eof_contract() {
        // The driver reports the ordinary end of stdout as a request error and
        // has no distinct EOF kind; if its wording changes, a successful
        // shutdown would be mistaken for a protocol fault. Fail loudly here.
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "exit 0"]);
        let mut child = RpcChild::spawn(&mut command, true).unwrap();
        let error = child
            .next_message(Some(Duration::from_secs(5)))
            .expect_err("a closed stream is an error");
        assert!(
            clean_stream_end(&error),
            "rpc EOF contract changed: {:?} / {}",
            error.kind(),
            error.detail()
        );
        child.terminate(Duration::from_millis(100));
    }

    #[test]
    fn tool_status_is_explicit_and_never_guessed() {
        assert_eq!(
            tool_status("succeeded"),
            Some(OperatorToolStatus::Succeeded)
        );
        assert_eq!(
            tool_status("failed"),
            Some(OperatorToolStatus::Failed { exit_code: None })
        );
        assert_eq!(tool_status("timedout"), Some(OperatorToolStatus::TimedOut));
        for unknown in ["", "Succeeded", "exit 0", "error"] {
            assert_eq!(tool_status(unknown), None);
        }
    }

    #[test]
    fn contract_requires_the_exact_four_tools_and_session() {
        let text = |tools: &str, session: &str| {
            format!("{{\"version\":1,\"sessionId\":\"{session}\",\"tools\":{tools}}}")
        };
        assert!(validate_contract(&text(r#"["read","bash","edit","write"]"#, "s"), "s").is_ok());
        assert!(validate_contract(&text(r#"["write","edit","bash","read"]"#, "s"), "s").is_ok());
        for (tools, session) in [
            (r#"["read","bash","edit"]"#, "s"),
            (r#"["read","bash","edit","write","grep"]"#, "s"),
            (r#"["read","bash","edit","write","bash"]"#, "s"),
            (r#"["read","bash","edit","write"]"#, "other"),
        ] {
            assert!(validate_contract(&text(tools, session), "s").is_err());
        }
    }

    #[test]
    fn admission_requires_a_matching_completed_assistant_call() {
        let call = json!({"id":"call-1","name":"bash","arguments":{"command":"true"}});
        let mut state = TurnState::new("session".into(), PathBuf::from("/tmp"));
        let payload = json!({"toolCallId":"call-1","toolName":"bash","input":{"command":"true"}});
        assert!(admit(&payload, &state).is_err());
        state.last_assistant = Some(AssistantTurn {
            stop: "toolUse".into(),
            calls: vec![ToolCallBlock {
                id: "call-1".into(),
                name: "bash".into(),
                arguments: call["arguments"].clone(),
            }],
        });
        assert!(admit(&payload, &state).is_ok());
        // Serialized: an unresolved previous call refuses the next admission.
        state.pending = Some(PendingCall {
            id: "call-1".into(),
        });
        assert!(admit(&payload, &state).is_err());
        state.pending = None;
        // A pending assistant turn is not completion.
        state.last_assistant.as_mut().unwrap().stop = "pending".into();
        assert!(admit(&payload, &state).is_err());
        state.last_assistant.as_mut().unwrap().stop = "toolUse".into();
        let other = json!({"toolCallId":"call-1","toolName":"bash","input":{"command":"false"}});
        assert!(admit(&other, &state).is_err());
        let unknown = json!({"toolCallId":"call-1","toolName":"grep","input":{"command":"true"}});
        assert!(admit(&unknown, &state).is_err());
    }

    #[test]
    fn message_end_appends_only_the_unstreamed_suffix() {
        let reporter = crate::report::reporter::Reporter::new(AgentProvider::Operator, None, None);
        let identity = ovrcr_protocol::OperatorTurnIdentity {
            conversation: ovrcr_protocol::OperatorConversationId(1),
            turn: ovrcr_protocol::OperatorTurnId(1),
            session: ovrcr_protocol::SessionId(1),
            run: ovrcr_protocol::SessionRunId(1),
        };
        let mut reporter = reporter;
        let mut journal = Journal::new(&mut reporter, identity);
        let mut state = TurnState::new("session".into(), PathBuf::from("/tmp"));
        state.stream_blocks.insert(
            0,
            StreamBlock {
                kind: DisplayKind::Answer,
                text: "hel".into(),
                held: String::new(),
                authoritative: None,
                displayed: true,
            },
        );
        state.last_stream_index = Some(0);
        state.max_display_index = Some(0);
        let message = json!({
            "role":"assistant",
            "stopReason":"stop",
            "content":[{"type":"text","text":"hello"}],
        });
        end_assistant(&message, &mut journal, &mut state).unwrap();
        assert_eq!(journal.pending, "lo");
        assert_eq!(journal.pending_kind, Some(DisplayKind::Answer));
        // A contradiction with an already streamed prefix fails.
        state.stream_blocks.insert(
            0,
            StreamBlock {
                kind: DisplayKind::Answer,
                text: "other".into(),
                held: String::new(),
                authoritative: None,
                displayed: true,
            },
        );
        state.last_stream_index = Some(0);
        state.max_display_index = Some(0);
        let message = json!({
            "role":"assistant",
            "stopReason":"stop",
            "content":[{"type":"text","text":"hello"}],
        });
        assert!(end_assistant(&message, &mut journal, &mut state).is_err());
    }

    #[test]
    fn prompt_text_always_keeps_the_context_ahead_of_user_text() {
        let start = OperatorWorkerStart {
            thinking_level: Default::default(),
            identity: ovrcr_protocol::OperatorTurnIdentity {
                conversation: ovrcr_protocol::OperatorConversationId(1),
                turn: ovrcr_protocol::OperatorTurnId(1),
                session: ovrcr_protocol::SessionId(1),
                run: ovrcr_protocol::SessionRunId(1),
            },
            model: None,
            text: "/new".into(),
            context: ovrcr_protocol::OperatorContext::default(),
            checkpoint_turn: None,
        };
        let prompt = prompt_text(&start).unwrap();
        assert!(prompt.starts_with("Captured OVRCR context"));
        assert!(prompt.ends_with("User request:\n/new"));
    }
}

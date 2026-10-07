use super::*;
use ovrcr_protocol::{AgentProvider, ConversationReference};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::SystemTime;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const TAIL_LIMIT: u64 = 256 * 1024;
const EXCERPT_LIMIT: usize = 8 * 1024;
const MESSAGE_LIMIT: usize = 8;
const TITLE_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
type AfterStdoutRead = Box<dyn Fn(&mut Child) + Send + Sync>;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TitleModel {
    provider: String,
    model: String,
}

impl TitleModel {
    pub(super) fn parse(raw: &str) -> Option<Self> {
        let (provider, model) = raw.split_once('/')?;
        let provider = provider.trim();
        let model = model.trim();
        (!provider.is_empty() && !model.is_empty()).then(|| Self {
            provider: provider.to_owned(),
            model: model.to_owned(),
        })
    }
}

#[derive(Clone, Debug, Default)]
struct SeenFile {
    path: PathBuf,
    len: u64,
    mtime: Option<SystemTime>,
}

pub(super) struct TitleWorker {
    root: PathBuf,
    seen: HashMap<SessionId, SeenFile>,
    /// Sessions that became due and still need a title call. Survives across
    /// ticks so a session observed while another call runs is not forgotten
    /// when its history file does not change again.
    due: HashSet<SessionId>,
    /// The model whose Pi executable was missing; a different model retries.
    missing_pi: Option<TitleModel>,
    serial: u64,
    /// Reasons already recorded for a quiet stretch, so a 2s tick does not
    /// append the same line again until the file or the model changes.
    unchanged_noted: HashSet<SessionId>,
    dismissed_noted: HashSet<SessionId>,
    noted_no_model: bool,
    noted_missing_pi: bool,
    #[cfg(test)]
    live_override: Option<HashMap<SessionId, SessionRunId>>,
    #[cfg(test)]
    pi_executable: Option<PathBuf>,
    #[cfg(test)]
    after_stdout_read: Option<AfterStdoutRead>,
    #[cfg(test)]
    timeout: Option<Duration>,
}

impl TitleWorker {
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            root,
            seen: HashMap::new(),
            due: HashSet::new(),
            missing_pi: None,
            serial: 0,
            unchanged_noted: HashSet::new(),
            dismissed_noted: HashSet::new(),
            noted_no_model: false,
            noted_missing_pi: false,
            #[cfg(test)]
            live_override: None,
            #[cfg(test)]
            pi_executable: None,
            #[cfg(test)]
            after_stdout_read: None,
            #[cfg(test)]
            timeout: None,
        }
    }

    pub(super) fn run(mut self, state: Arc<ServerState>) {
        let mut last = Instant::now() - POLL_INTERVAL;
        while !state.shutdown.load(Ordering::Acquire) {
            thread::park_timeout(Duration::from_millis(200));
            if state.shutdown.load(Ordering::Acquire) {
                break;
            }
            if last.elapsed() >= POLL_INTERVAL {
                last = Instant::now();
                // Hermes already named the session. Apply that title without a
                // Dashboard and without another model call.
                self.apply_hermes_titles(&state);
                if state.dashboard.is_claimed() {
                    self.tick(&state);
                }
            }
        }
    }

    fn emit(&self, state: &ServerState, session: Option<SessionId>, message: &str) {
        state.record_event(
            ovrcr_protocol::EventComponent::Titles,
            session.map(|session| session.0.to_string()),
            message,
        );
    }

    fn session_ready(&self, state: &ServerState, id: SessionId, run: SessionRunId) -> bool {
        #[cfg(test)]
        if let Some(live) = &self.live_override {
            return live.get(&id).copied() == Some(run);
        }
        let Some(session) = state.sessions.lock().unwrap().get(&id).cloned() else {
            return false;
        };
        session.run() == run && session.is_live()
    }

    fn note_closed_window(
        &mut self,
        state: &ServerState,
        record: &crate::retained::RetainedSession,
        conversation: &str,
    ) {
        self.due.remove(&record.id);
        let dismissed = record
            .subjects
            .get(conversation)
            .is_some_and(|subject| subject.dismissed);
        if dismissed && self.dismissed_noted.insert(record.id) {
            self.emit(state, Some(record.id), "title dismissed");
        }
        if !dismissed {
            self.dismissed_noted.remove(&record.id);
        }
    }

    fn apply_hermes_titles(&mut self, state: &Arc<ServerState>) {
        let records: Vec<_> = state.retained.lock().records().cloned().collect();
        for record in records {
            let Some(candidate) = Candidate::from_record(&record) else {
                continue;
            };
            if candidate.provider != AgentProvider::Hermes || record.metadata.pinned_title.is_some()
            {
                continue;
            }
            if record
                .subjects
                .get(&candidate.conversation)
                .is_some_and(|subject| !subject.title_window_open())
            {
                continue;
            }
            let Ok(Some(topic)) = hermes_title(&candidate.history, &candidate.conversation) else {
                continue;
            };
            let changed = {
                let mut retained = state.retained.lock();
                retained
                    .save_conversation_subject(
                        record.id,
                        record.run,
                        &candidate.conversation,
                        topic,
                    )
                    .unwrap_or(false)
            };
            if changed {
                super::dispatch::publish_session_changed(state, record.id);
                self.emit(state, Some(record.id), "title applied");
            }
        }
    }

    fn tick(&mut self, state: &Arc<ServerState>) {
        let Some(model) = state.title_model() else {
            if !self.noted_no_model {
                self.noted_no_model = true;
                let ids: Vec<SessionId> = state
                    .retained
                    .lock()
                    .records()
                    .map(|record| record.id)
                    .collect();
                if ids.is_empty() {
                    self.emit(state, None, "no title_model");
                } else {
                    for id in ids {
                        self.emit(state, Some(id), "no title_model");
                    }
                }
            }
            return;
        };
        self.noted_no_model = false;
        if self.missing_pi.as_ref() == Some(&model) {
            if !self.noted_missing_pi {
                self.noted_missing_pi = true;
                self.emit(state, None, "Pi missing for this model");
            }
            return;
        }
        self.noted_missing_pi = false;
        let records: Vec<_> = {
            let retained = state.retained.lock();
            retained.records().cloned().collect()
        };

        // Phase 1: remember every eligible session whose history file changed.
        // Marking seen here must not drop a sibling that becomes due in the
        // same poll or while a call is in flight — those stay in `due`.
        for record in &records {
            if state.shutdown.load(Ordering::Acquire) {
                return;
            }
            let Some(candidate) = Candidate::from_record(record) else {
                self.due.remove(&record.id);
                continue;
            };
            if candidate.provider == AgentProvider::Hermes {
                self.due.remove(&record.id);
                continue;
            }
            if record.metadata.pinned_title.is_some()
                || record
                    .subjects
                    .get(&candidate.conversation)
                    .is_some_and(|subject| !subject.title_window_open())
            {
                self.note_closed_window(state, record, &candidate.conversation);
                continue;
            }
            self.dismissed_noted.remove(&record.id);
            if !self.session_ready(state, record.id, record.run) {
                self.due.remove(&record.id);
                continue;
            }
            let Ok(metadata) = fs::metadata(&candidate.history) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let current = SeenFile {
                path: candidate.history.clone(),
                len: metadata.len(),
                mtime: metadata.modified().ok(),
            };
            if self.seen.get(&record.id).is_some_and(|seen| {
                seen.path == current.path && seen.len == current.len && seen.mtime == current.mtime
            }) {
                if self.unchanged_noted.insert(record.id) {
                    self.emit(state, Some(record.id), "history unchanged");
                }
                continue;
            }
            self.unchanged_noted.remove(&record.id);
            let path_changed = self
                .seen
                .get(&record.id)
                .is_none_or(|seen| seen.path != current.path);
            self.seen.insert(record.id, current);
            // A recorded switch (new history path) that already has a stored subject
            // only needs the display recompute from retain; do not fire a title call.
            if path_changed
                && record
                    .subjects
                    .get(&candidate.conversation)
                    .is_some_and(|subject| !subject.dismissed && subject.topic.is_some())
            {
                self.due.remove(&record.id);
                continue;
            }
            self.due.insert(record.id);
        }

        // Phase 2: run at most one due call. Remaining dues wait for the next
        // tick even if their files do not change again.
        for record in records {
            if state.shutdown.load(Ordering::Acquire) {
                return;
            }
            if !self.due.contains(&record.id) {
                continue;
            }
            let Some(candidate) = Candidate::from_record(&record) else {
                self.due.remove(&record.id);
                continue;
            };
            if record.metadata.pinned_title.is_some()
                || record
                    .subjects
                    .get(&candidate.conversation)
                    .is_some_and(|subject| !subject.title_window_open())
            {
                self.note_closed_window(state, &record, &candidate.conversation);
                continue;
            }
            if !self.session_ready(state, record.id, record.run) {
                self.due.remove(&record.id);
                continue;
            }
            let Ok(Some(excerpt)) = excerpt(&candidate) else {
                self.due.remove(&record.id);
                continue;
            };
            self.due.remove(&record.id);
            self.emit(state, Some(record.id), "call sent");
            match self.call(&model, &excerpt, state) {
                CallResult::Title(topic) => {
                    let changed = {
                        let mut retained = state.retained.lock();
                        match retained.save_conversation_subject(
                            record.id,
                            record.run,
                            &candidate.conversation,
                            topic,
                        ) {
                            Ok(changed) => changed,
                            // The topic is a bound parameter, not part of this error.
                            Err(error) => {
                                eprintln!(
                                    "conversation subject save failed for session {}: {error}",
                                    record.id.0
                                );
                                false
                            }
                        }
                    };
                    if changed {
                        super::dispatch::publish_session_changed(state, record.id);
                        self.emit(state, Some(record.id), "title applied");
                    } else if state.retained.lock().get(record.id).is_some_and(|saved| {
                        saved
                            .subjects
                            .get(&candidate.conversation)
                            .is_some_and(|subject| subject.dismissed)
                    }) {
                        self.emit(state, Some(record.id), "title dismissed");
                    }
                }
                CallResult::MissingPi => {
                    self.missing_pi = Some(model.clone());
                    self.noted_missing_pi = true;
                    self.emit(state, Some(record.id), "Pi missing for this model");
                }
                CallResult::Failed(failure) => {
                    self.emit(state, Some(record.id), &failure.event());
                    let _ = state.retained.lock().record_subject_attempt(
                        record.id,
                        record.run,
                        &candidate.conversation,
                    );
                }
            }
            return;
        }
    }

    fn call(&mut self, model: &TitleModel, excerpt: &str, state: &ServerState) -> CallResult {
        self.serial = self.serial.saturating_add(1);
        let dir = self
            .root
            .join(format!("call-{}-{}", std::process::id(), self.serial));
        if let Err(error) = fs::create_dir_all(&dir) {
            return CallFailure::error(format!("could not create call directory: {error}"));
        }
        let result = self.call_in_dir(model, excerpt, &dir, state);
        let _ = fs::remove_dir_all(&dir);
        result
    }

    fn call_in_dir(
        &self,
        model: &TitleModel,
        excerpt: &str,
        dir: &Path,
        state: &ServerState,
    ) -> CallResult {
        let executable = {
            #[cfg(test)]
            if let Some(executable) = &self.pi_executable {
                executable.clone()
            } else {
                std::env::var_os("OVRCR_PI_EXECUTABLE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("pi"))
            }
            #[cfg(not(test))]
            {
                std::env::var_os("OVRCR_PI_EXECUTABLE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("pi"))
            }
        };
        let timeout = {
            #[cfg(test)]
            {
                self.timeout.unwrap_or(TITLE_TIMEOUT)
            }
            #[cfg(not(test))]
            {
                TITLE_TIMEOUT
            }
        };
        // Pi's diagnostics go to a file in the call directory (removed after
        // the call) so a failure can name its reason without parsing stderr
        // as protocol data.
        let stderr_path = dir.join(STDERR_FILE);
        let stderr = match File::create(&stderr_path) {
            Ok(file) => Stdio::from(file),
            Err(_) => Stdio::null(),
        };
        let mut child = match Command::new(&executable)
            .current_dir(dir)
            .args([
                "--mode",
                "rpc",
                "--provider",
                &model.provider,
                "--model",
                &model.model,
                "--thinking",
                "off",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-approve",
                "--offline",
                // A title call is one throwaway prompt: never persist the
                // excerpt as a Pi session under ~/.pi/agent/sessions.
                "--no-session",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .process_group(0)
            .spawn()
        {
            Ok(child) => child,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return CallResult::MissingPi,
            Err(error) => return CallFailure::error(format!("could not start Pi: {error}")),
        };
        // Pi's RPC mode treats stdin EOF as an orderly shutdown request and
        // disposes the in-flight run, so stdin stays open until the run
        // settles (or the call ends for another reason).
        let mut stdin = child.stdin.take();
        if let Some(input) = stdin.as_mut() {
            let prompt = format!(
                "Name this conversation in a few words. Return only the topic. No quotes, status, secrets, or explanation.\n\n{excerpt}"
            );
            let mut request =
                match serde_json::to_vec(&json!({"type":"prompt","id":"title","message":prompt})) {
                    Ok(request) => request,
                    Err(error) => {
                        return kill_failed(child, CallFailure::Error(error.to_string()));
                    }
                };
            request.push(b'\n');
            if let Err(error) = input.write_all(&request).and_then(|()| input.flush()) {
                let reason = stderr_reason(&stderr_path).map_or_else(
                    || format!("could not send the prompt: {error}"),
                    |tail| format!("could not send the prompt: {error}: {tail}"),
                );
                return kill_failed(child, CallFailure::Error(reason));
            }
        }
        let mut stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => return kill_failed(child, CallFailure::error_text("Pi stdout unavailable")),
        };
        if let Err(error) = nonblocking(stdout.as_raw_fd()) {
            return kill_failed(child, CallFailure::Error(error.to_string()));
        }
        let deadline = Instant::now() + timeout;
        let mut pending = Vec::new();
        let mut reply = Reply::default();
        let mut exited = false;
        loop {
            let fail = move |child, failure| {
                if exited {
                    CallResult::Failed(failure)
                } else {
                    kill_failed(child, failure)
                }
            };
            if state.shutdown.load(Ordering::Acquire) {
                return fail(child, CallFailure::error_text("server shutting down"));
            }
            if Instant::now() >= deadline {
                // A usable title that arrived without a settle marker still counts.
                if let Some(title) = reply.title.take() {
                    drop(stdin);
                    return finish(child, exited, CallResult::Title(title));
                }
                return fail(child, CallFailure::TimedOut(timeout));
            }
            let mut bytes = [0; 4096];
            let stdout_done = match stdout.read(&mut bytes) {
                Ok(0) => true,
                Ok(n) => {
                    pending.extend_from_slice(&bytes[..n]);
                    while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                        let line: Vec<_> = pending.drain(..=end).collect();
                        if line.iter().all(u8::is_ascii_whitespace) {
                            continue;
                        }
                        if let Ok(event) = serde_json::from_slice::<Value>(&line) {
                            reply.observe(&event);
                        }
                    }
                    if let Some(reason) = reply.rejected.take() {
                        return fail(child, CallFailure::Error(reason));
                    }
                    if pending.len() > 1024 * 1024 {
                        return fail(
                            child,
                            CallFailure::error_text("Pi reply line exceeded 1 MiB"),
                        );
                    }
                    if reply.done() {
                        drop(stdin);
                        return finish(child, exited, reply.into_result(None));
                    }
                    false
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => true,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => false,
                Err(error) => {
                    return fail(
                        child,
                        CallFailure::Error(format!("reading Pi output: {error}")),
                    );
                }
            };
            if exited {
                if stdout_done {
                    return reply.into_result(Some(exit_reason(None, &stderr_path)));
                }
                continue;
            }
            #[cfg(test)]
            if let Some(after_stdout_read) = &self.after_stdout_read {
                after_stdout_read(&mut child);
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        return CallResult::Failed(CallFailure::Error(exit_reason(
                            Some(status),
                            &stderr_path,
                        )));
                    }
                    // Successful exit can precede consuming the final pipe
                    // bytes. Drain available frames without waiting on writers.
                    exited = true;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    return fail(child, CallFailure::Error(error.to_string()));
                }
            }
        }
    }
}

const STDERR_FILE: &str = "pi-stderr.log";
/// How long Pi gets to exit on its own after stdin closes.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// Upper bound for a reason carried in a titles event.
const REASON_LIMIT: usize = 160;

/// What the Pi RPC stream said about the title prompt.
#[derive(Default)]
struct Reply {
    title: Option<String>,
    /// The last assistant reply had text, but not a usable title.
    unusable: bool,
    /// Provider or run failure reported in an assistant `message_end`.
    model_error: Option<String>,
    /// Pi refused the prompt command itself (`success: false`).
    rejected: Option<String>,
    ended: bool,
    settled: bool,
}

impl Reply {
    fn observe(&mut self, event: &Value) {
        match event["type"].as_str() {
            Some("response") if event["success"] == false => {
                let error = event["error"].as_str().unwrap_or("no reason given");
                self.rejected = Some(format!("Pi rejected the prompt: {error}"));
            }
            Some("message_end") if event["message"]["role"] == "assistant" => {
                let message = &event["message"];
                if matches!(message["stopReason"].as_str(), Some("error" | "aborted")) {
                    let error = message["errorMessage"]
                        .as_str()
                        .unwrap_or_else(|| message["stopReason"].as_str().unwrap_or("error"));
                    self.model_error = Some(error.to_owned());
                    return;
                }
                match message_text(message) {
                    Some(text) if !text.trim().is_empty() => match clean_title(text) {
                        Some(cleaned) => {
                            self.title = Some(cleaned);
                            self.unusable = false;
                            self.model_error = None;
                        }
                        None => self.unusable = true,
                    },
                    _ => {}
                }
            }
            Some("agent_end") => self.ended = true,
            Some("agent_settled") => self.settled = true,
            _ => {}
        }
    }

    /// Pi will not continue on its own, or a run ended with a usable title.
    fn done(&self) -> bool {
        self.settled || (self.ended && self.title.is_some())
    }

    fn into_result(self, exited_early: Option<String>) -> CallResult {
        if let Some(title) = self.title {
            return CallResult::Title(title);
        }
        if let Some(error) = self.model_error {
            return CallResult::Failed(CallFailure::Error(format!("model error: {error}")));
        }
        if self.unusable {
            return CallFailure::error("reply was not a usable title (empty or over 60 chars)");
        }
        CallFailure::error(
            exited_early.unwrap_or_else(|| "Pi finished without an assistant reply".into()),
        )
    }
}

#[derive(Debug, PartialEq)]
enum CallFailure {
    TimedOut(Duration),
    Error(String),
}

impl CallFailure {
    fn error(reason: impl Into<String>) -> CallResult {
        CallResult::Failed(Self::Error(reason.into()))
    }

    fn error_text(reason: &str) -> Self {
        Self::Error(reason.to_owned())
    }

    /// The titles event line: names the specific failure, never the excerpt.
    fn event(&self) -> String {
        match self {
            Self::TimedOut(after) => format!("call timed out after {}", seconds(*after)),
            Self::Error(reason) => format!("call failed: {}", short_reason(reason)),
        }
    }
}

fn seconds(duration: Duration) -> String {
    if duration.subsec_millis() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{:.1}s", duration.as_secs_f64())
    }
}

/// One line, whitespace collapsed, bounded to `REASON_LIMIT` characters.
fn short_reason(reason: &str) -> String {
    let line = reason.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= REASON_LIMIT {
        return if line.is_empty() {
            "no reason given".into()
        } else {
            line
        };
    }
    let mut short: String = line.chars().take(REASON_LIMIT - 1).collect();
    short.push('…');
    short
}

/// Last non-empty stderr line Pi wrote, if any.
fn stderr_reason(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(4096))).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    String::from_utf8_lossy(&tail)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

fn exit_reason(status: Option<std::process::ExitStatus>, stderr: &Path) -> String {
    use std::os::unix::process::ExitStatusExt;
    let what = match status {
        None => "Pi exited before replying".to_owned(),
        Some(status) => match (status.code(), status.signal()) {
            (Some(code), _) => format!("Pi exited with code {code}"),
            (None, Some(signal)) => format!("Pi killed by signal {signal}"),
            (None, None) => format!("Pi exited: {status}"),
        },
    };
    match stderr_reason(stderr) {
        Some(tail) => format!("{what}: {tail}"),
        None => what,
    }
}

/// Close out a call that already has its answer: let Pi shut down after
/// stdin closed, then make sure the process group is gone.
fn finish(mut child: Child, exited: bool, result: CallResult) -> CallResult {
    if exited {
        return result;
    }
    let deadline = Instant::now() + SHUTDOWN_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
                let _ = child.wait();
                break;
            }
        }
    }
    result
}

fn kill_failed(mut child: Child, failure: CallFailure) -> CallResult {
    let _ = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    let _ = child.wait();
    CallResult::Failed(failure)
}

fn nonblocking(fd: std::os::fd::RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

enum CallResult {
    Title(String),
    MissingPi,
    Failed(CallFailure),
}

struct Candidate {
    provider: AgentProvider,
    conversation: String,
    history: PathBuf,
}

impl Candidate {
    fn from_record(record: &RetainedSession) -> Option<Self> {
        let SessionKind::Agent { name } = &record.metadata.kind else {
            return None;
        };
        let expected = AgentProvider::from_name(name)?;
        let reference = record.conversation.as_ref()?;
        if reference.provider() != expected {
            return None;
        }
        match reference {
            ConversationReference::Claude(reference) => Some(Self {
                provider: AgentProvider::Claude,
                conversation: reference.conversation.clone(),
                history: reference.history.clone(),
            }),
            ConversationReference::Codex(reference) => Some(Self {
                provider: AgentProvider::Codex,
                conversation: reference.conversation.clone(),
                history: reference.history.clone()?,
            }),
            ConversationReference::Pi(reference) => Some(Self {
                provider: AgentProvider::Pi,
                conversation: reference.conversation.clone(),
                history: reference.history.clone()?,
            }),
            ConversationReference::Omp(reference) => Some(Self {
                provider: AgentProvider::Omp,
                conversation: reference.conversation.clone(),
                history: reference.history.clone()?,
            }),
            ConversationReference::Grok(reference) => Some(Self {
                provider: AgentProvider::Grok,
                conversation: reference.conversation.clone(),
                history: reference.history.clone(),
            }),
            ConversationReference::Hermes(reference) => Some(Self {
                provider: AgentProvider::Hermes,
                conversation: reference.conversation.clone(),
                history: reference.state_db.clone(),
            }),
        }
    }
}

fn hermes_title(state_db: &std::path::Path, conversation: &str) -> Result<Option<String>> {
    if !crate::hermes_recovery::valid_session_id(conversation) || !state_db.is_file() {
        return Ok(None);
    }
    let connection = rusqlite::Connection::open_with_flags(
        state_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let title = connection
        .query_row(
            "SELECT title FROM sessions WHERE id = ?1",
            [conversation],
            |row| row.get::<_, Option<String>>(0),
        )
        .unwrap_or(None);
    Ok(title.filter(|value| {
        !value.trim().is_empty() && ovrcr_protocol::validate_agent_id(value).is_ok()
    }))
}

fn excerpt(candidate: &Candidate) -> Result<Option<String>> {
    match candidate.provider {
        AgentProvider::Pi | AgentProvider::Omp => {
            if !first_line_matches(&candidate.history, |value| {
                value["type"] == "session"
                    && json_id(value) == Some(candidate.conversation.as_str())
            })? {
                return Ok(None);
            }
        }
        AgentProvider::Codex => {
            if !first_line_matches(&candidate.history, |value| {
                value["type"] == "session_meta"
                    && value["payload"]["id"].as_str() == Some(candidate.conversation.as_str())
            })? {
                return Ok(None);
            }
        }
        AgentProvider::Claude => {}
        AgentProvider::Grok => {
            if !first_line_matches(&candidate.history, |value| {
                value["params"]["sessionId"].as_str() == Some(candidate.conversation.as_str())
            })? {
                return Ok(None);
            }
        }
        AgentProvider::Hermes => {
            return hermes_title(&candidate.history, &candidate.conversation);
        }
        AgentProvider::Cursor => return Ok(None),
    }
    let tail = read_tail(&candidate.history)?;
    let mut messages = Vec::new();
    let mut claude_match = candidate.provider != AgentProvider::Claude;
    for line in tail.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if candidate.provider == AgentProvider::Claude {
            if value["sessionId"].as_str() != Some(candidate.conversation.as_str()) {
                continue;
            }
            claude_match = true;
        }
        if let Some((role, text)) = role_text(&value)
            && (role == "user" || role == "assistant")
            && !text.trim().is_empty()
        {
            messages.push((role.to_owned(), text));
        }
    }
    if !claude_match || !messages.iter().any(|(role, _)| role == "assistant") {
        return Ok(None);
    }
    let mut out = String::new();
    for (role, text) in messages
        .into_iter()
        .rev()
        .take(MESSAGE_LIMIT)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if out.len() >= EXCERPT_LIMIT {
            break;
        }
        let line = format!("{role}: {text}\n");
        out.push_str(&line);
        truncate_to_char_boundary(&mut out, EXCERPT_LIMIT);
    }
    Ok((!out.trim().is_empty()).then_some(out))
}

fn truncate_to_char_boundary(value: &mut String, limit: usize) {
    if value.len() <= limit {
        return;
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

fn first_line_matches(path: &Path, matches: impl FnOnce(&Value) -> bool) -> Result<bool> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    let mut one = [0; 1];
    while file.read(&mut one)? == 1 {
        bytes.push(one[0]);
        if one[0] == b'\n' || bytes.len() > 64 * 1024 {
            break;
        }
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    Ok(matches(&value))
}

fn read_tail(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(TAIL_LIMIT);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(TAIL_LIMIT).read_to_end(&mut bytes)?;
    if start > 0
        && let Some(pos) = bytes.iter().position(|b| *b == b'\n')
    {
        bytes.drain(..=pos);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn json_id(value: &Value) -> Option<&str> {
    value["id"]
        .as_str()
        .or_else(|| value["session_id"].as_str())
        .or_else(|| value["sessionId"].as_str())
}

fn role_text(value: &Value) -> Option<(&str, String)> {
    let message = if value.get("message").is_some() {
        &value["message"]
    } else if value.get("payload").is_some() {
        &value["payload"]
    } else {
        value
    };
    let role = message["role"].as_str()?;
    Some((role, message_text(message)?))
}

fn message_text(message: &Value) -> Option<String> {
    if let Some(text) = message["text"].as_str() {
        return Some(text.to_owned());
    }
    if let Some(text) = message["content"].as_str() {
        return Some(text.to_owned());
    }
    let parts = message["content"]
        .as_array()?
        .iter()
        .filter(|item| {
            !matches!(
                item["type"].as_str(),
                Some("tool" | "tool_call" | "tool_use" | "tool_result")
            )
        })
        .filter_map(|item| item["text"].as_str())
        .collect::<Vec<_>>();
    Some(parts.join("\n"))
}

fn clean_title(value: String) -> Option<String> {
    let value = crate::session::sanitize_title(&value)?;
    (value.chars().count() <= 60).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::retained::Disposition;
    use ovrcr_protocol::{ClaudeConversation, CodexConversation, ExtensionConversation};

    fn candidate(provider: AgentProvider, history: PathBuf, conversation: &str) -> Candidate {
        Candidate {
            provider,
            history,
            conversation: conversation.into(),
        }
    }

    #[test]
    fn cleaner_reuses_session_title_rules_then_applies_sixty_char_limit() {
        assert_eq!(clean_title("  Topic \n".into()).as_deref(), Some("Topic"));
        assert_eq!(clean_title("bad\u{202e}".into()).as_deref(), Some("bad"));
        assert!(clean_title("x".repeat(61)).is_none());
    }

    #[test]
    fn successful_exit_drains_all_reply_frames_after_empty_read() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("gated-pi");
        fs::write(
            &executable,
            "#!/bin/sh\nIFS= read -r request || exit 2\nattempt=0\nwhile [ ! -f go ]; do\n  attempt=$((attempt + 1))\n  [ \"$attempt\" -le 200 ] || exit 3\n  sleep 0.01\ndone\ncat reply\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let frame = |text: &str| {
            format!(
                "{}\n",
                json!({"type":"message_end","message":{"role":"assistant","content":text}})
            )
        };
        // Fits in the pipe while exceeding one 4096-byte read. The final
        // assistant frame must replace the earlier title after the exit.
        fs::write(
            root.path().join("reply"),
            format!(
                "{}{}{}",
                frame("Earlier Topic"),
                " \n".repeat(3000),
                frame("Exited Topic")
            ),
        )
        .unwrap();
        let go = root.path().join("go");
        let mut worker = TitleWorker::new(root.path().to_path_buf());
        worker.pi_executable = Some(executable);
        worker.after_stdout_read = Some(Box::new(move |child| {
            // No provider output is possible until the first read completed.
            fs::write(&go, []).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        assert!(status.success(), "fake provider must exit successfully");
                        break;
                    }
                    Ok(None) if Instant::now() < deadline => thread::yield_now(),
                    result => {
                        let _ = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
                        let _ = child.wait();
                        panic!("fake provider failed to exit before try_wait: {result:?}");
                    }
                }
            }
        }));
        let state = super::super::tests::test_state(None, None);
        let result = worker.call_in_dir(
            &TitleModel::parse("fake/model").unwrap(),
            "private fake excerpt",
            root.path(),
            &state,
        );
        assert!(
            matches!(result, CallResult::Title(ref title) if title == "Exited Topic"),
            "successful exit must consume the last queued assistant frame"
        );
    }

    fn fake_pi(root: &Path, name: &str, body: &str) -> PathBuf {
        let executable = root.join(name);
        fs::write(&executable, format!("#!/bin/sh\n{body}")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        executable
    }

    fn call_fake(root: &Path, executable: PathBuf, timeout: Option<Duration>) -> CallResult {
        let dir = root.join("call");
        fs::create_dir_all(&dir).unwrap();
        let mut worker = TitleWorker::new(root.to_path_buf());
        worker.pi_executable = Some(executable);
        worker.timeout = timeout;
        let state = super::super::tests::test_state(None, None);
        worker.call_in_dir(
            &TitleModel::parse("fake/model").unwrap(),
            "private fake excerpt",
            &dir,
            &state,
        )
    }

    fn failure_event(result: CallResult) -> String {
        match result {
            CallResult::Failed(failure) => failure.event(),
            CallResult::Title(title) => panic!("expected a failure, got title {title:?}"),
            CallResult::MissingPi => panic!("expected a failure, got MissingPi"),
        }
    }

    /// Pi's RPC mode disposes the in-flight run when stdin closes. This fake
    /// does the same: EOF before the reply exits 0 with no assistant frame.
    /// The title call must hold stdin open until the run settles.
    #[test]
    fn stdin_stays_open_until_the_rpc_run_settles() {
        let root = tempfile::tempdir().unwrap();
        let executable = fake_pi(
            root.path(),
            "eof-shutdown-pi",
            r#"IFS= read -r request || exit 2
printf '%s\n' '{"id":"title","type":"response","command":"prompt","success":true}' '{"type":"agent_start"}'
exec 3<&0
{ cat <&3 >/dev/null; : > stdin-closed; } >/dev/null 2>&1 &
sleep 1
[ -f stdin-closed ] && exit 0
printf '%s\n' '{"type":"message_end","message":{"role":"assistant","content":[{"type":"thinking","thinking":""},{"type":"text","text":"Login page CSS fix"}],"stopReason":"stop"}}' '{"type":"agent_end"}' '{"type":"agent_settled"}'
wait
exit 0
"#,
        );
        let result = call_fake(root.path(), executable, None);
        assert!(
            matches!(result, CallResult::Title(ref title) if title == "Login page CSS fix"),
            "title call must keep stdin open until agent_settled"
        );
    }

    #[test]
    fn pi_exit_before_reply_names_the_reason() {
        let root = tempfile::tempdir().unwrap();
        let executable = fake_pi(
            root.path(),
            "early-exit-pi",
            "IFS= read -r request || exit 2\nprintf '%s\\n' '{\"type\":\"agent_start\"}'\nexit 0\n",
        );
        assert_eq!(
            failure_event(call_fake(root.path(), executable, None)),
            "call failed: Pi exited before replying"
        );
    }

    #[test]
    fn provider_error_reaches_the_event() {
        let root = tempfile::tempdir().unwrap();
        let executable = fake_pi(
            root.path(),
            "model-error-pi",
            r#"IFS= read -r request || exit 2
printf '%s\n' '{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"xai API error (404): model gone\nsecond line"}}' '{"type":"agent_end"}' '{"type":"agent_settled"}'
while IFS= read -r line; do :; done
"#,
        );
        assert_eq!(
            failure_event(call_fake(root.path(), executable, None)),
            "call failed: model error: xai API error (404): model gone second line"
        );
    }

    #[test]
    fn rejected_prompt_and_stderr_are_reported() {
        let root = tempfile::tempdir().unwrap();
        let rejected = fake_pi(
            root.path(),
            "rejecting-pi",
            r#"IFS= read -r request || exit 2
printf '%s\n' '{"id":"title","type":"response","command":"prompt","success":false,"error":"Model not found: fake/model"}'
while IFS= read -r line; do :; done
"#,
        );
        assert_eq!(
            failure_event(call_fake(root.path(), rejected, None)),
            "call failed: Pi rejected the prompt: Model not found: fake/model"
        );

        let crashing = fake_pi(
            root.path(),
            "crashing-pi",
            "echo 'warming up' >&2\necho 'error: unknown option --no-themes' >&2\nexit 3\n",
        );
        assert_eq!(
            failure_event(call_fake(root.path(), crashing, None)),
            "call failed: Pi exited with code 3: error: unknown option --no-themes"
        );
    }

    #[test]
    fn timeout_is_distinct_from_failure() {
        let root = tempfile::tempdir().unwrap();
        let executable = fake_pi(
            root.path(),
            "silent-pi",
            "while IFS= read -r line; do :; done\n",
        );
        let started = Instant::now();
        assert_eq!(
            failure_event(call_fake(
                root.path(),
                executable,
                Some(Duration::from_millis(300))
            )),
            "call timed out after 0.3s"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            CallFailure::TimedOut(TITLE_TIMEOUT).event(),
            "call timed out after 30s"
        );
    }

    #[test]
    fn unusable_reply_and_long_reasons_are_bounded() {
        let root = tempfile::tempdir().unwrap();
        let long = "x".repeat(61);
        let executable = fake_pi(
            root.path(),
            "long-title-pi",
            &format!(
                "IFS= read -r request || exit 2\nprintf '%s\\n' '{{\"type\":\"message_end\",\"message\":{{\"role\":\"assistant\",\"content\":\"{long}\"}}}}' '{{\"type\":\"agent_settled\"}}'\nwhile IFS= read -r line; do :; done\n"
            ),
        );
        assert_eq!(
            failure_event(call_fake(root.path(), executable, None)),
            "call failed: reply was not a usable title (empty or over 60 chars)"
        );
        let event = CallFailure::Error("y ".repeat(400)).event();
        assert!(event.chars().count() <= "call failed: ".len() + REASON_LIMIT);
        assert!(event.ends_with('…'));
    }

    #[test]
    fn bounded_tail_extracts_provider_messages() {
        let root = tempfile::tempdir().unwrap();
        let history = root.path().join("pi.jsonl");
        let big = "x".repeat(300 * 1024);
        fs::write(
            &history,
            format!(
                "{{\"type\":\"session\",\"id\":\"c\"}}\n{big}\n{{\"role\":\"user\",\"content\":\"tail prompt\"}}\n{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"tail reply\"}}]}}\n"
            ),
        )
        .unwrap();
        let text = excerpt(&candidate(AgentProvider::Pi, history, "c"))
            .unwrap()
            .unwrap();
        assert!(text.contains("tail reply"));
        assert!(!text.contains(&big));
    }

    #[test]
    fn excerpt_limit_truncates_at_utf8_boundary() {
        let root = tempfile::tempdir().unwrap();
        let history = root.path().join("pi.jsonl");
        let prefix = "assistant: ";
        let text = format!("{}é", "a".repeat(EXCERPT_LIMIT - prefix.len() - 1));
        fs::write(
            &history,
            format!(
                "{{\"type\":\"session\",\"id\":\"c\"}}\n{{\"role\":\"assistant\",\"content\":\"{text}\"}}\n"
            ),
        )
        .unwrap();
        let text = excerpt(&candidate(AgentProvider::Pi, history, "c"))
            .unwrap()
            .unwrap();
        assert!(text.is_char_boundary(text.len()));
        assert!(text.len() <= EXCERPT_LIMIT);
    }

    #[test]
    fn provider_identity_checks_are_distinct() {
        let root = tempfile::tempdir().unwrap();
        let codex = root.path().join("codex.jsonl");
        fs::write(
            &codex,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"ok\"}}\n{\"role\":\"assistant\",\"content\":\"reply\"}\n",
        )
        .unwrap();
        assert!(
            excerpt(&candidate(AgentProvider::Codex, codex.clone(), "ok"))
                .unwrap()
                .is_some()
        );
        assert!(
            excerpt(&candidate(AgentProvider::Codex, codex.clone(), "bad"))
                .unwrap()
                .is_none()
        );
        fs::write(
            &codex,
            "{\"type\":\"session_meta\",\"id\":\"ok\",\"payload\":{\"id\":\"other\"}}\n{\"role\":\"assistant\",\"content\":\"reply\"}\n",
        )
        .unwrap();
        assert!(
            excerpt(&candidate(AgentProvider::Codex, codex.clone(), "ok"))
                .unwrap()
                .is_none(),
            "Codex must require payload.id, not a top-level id"
        );
        fs::write(
            &codex,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"ok\",\"source\":\"cli\"}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"prompt\"}]}}\n{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"codex reply\"}]}}\n",
        )
        .unwrap();
        let text = excerpt(&candidate(AgentProvider::Codex, codex, "ok"))
            .unwrap()
            .expect("Codex response_item messages must yield an excerpt");
        assert!(text.contains("codex reply"));
        assert!(text.contains("prompt"));

        let claude = root.path().join("claude.jsonl");
        fs::write(
            &claude,
            "{\"type\":\"mode\"}\n{\"sessionId\":\"ok\",\"role\":\"assistant\",\"content\":\"reply\"}\n",
        )
        .unwrap();
        assert!(
            excerpt(&candidate(AgentProvider::Claude, claude.clone(), "ok"))
                .unwrap()
                .is_some()
        );
        assert!(
            excerpt(&candidate(AgentProvider::Pi, claude.clone(), "ok"))
                .unwrap()
                .is_none(),
            "Claude mode first line must not pass the Pi session-header check"
        );
        assert!(
            excerpt(&candidate(AgentProvider::Claude, claude, "bad"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn retained_record_candidates_only_use_supported_references() {
        let extension = ExtensionConversation {
            conversation: "c".into(),
            executable: "pi".into(),
            history: Some("h".into()),
            config_dir: "cfg".into(),
            options: vec![],
        };
        let codex = CodexConversation {
            conversation: "c".into(),
            executable: "codex".into(),
            history: Some("h".into()),
            config_dir: "cfg".into(),
            options: vec![],
        };
        let claude = ClaudeConversation {
            conversation: "c".into(),
            executable: "claude".into(),
            history: "h".into(),
            config_dir: "cfg".into(),
            options: vec![],
        };
        for (name, reference) in [
            ("pi", ConversationReference::Pi(extension.clone())),
            ("omp", ConversationReference::Omp(extension)),
            ("codex", ConversationReference::Codex(codex)),
            ("claude", ConversationReference::Claude(claude)),
        ] {
            let mut record = RetainedSession {
                id: SessionId(1),
                run: ovrcr_protocol::SessionRunId(1),
                metadata: SessionMetadata {
                    project: "p".into(),
                    workspace: "w".into(),
                    name: "n".into(),
                    label: "l".into(),
                    cwd: "/".into(),
                    kind: SessionKind::Agent { name: name.into() },
                    pinned_title: None,
                    application_title: None,
                },
                title_revision: 0,
                boot_id: None,
                stopped: false,
                disposition: Disposition::Active,
                failure: None,
                conversation: Some(reference),
                identity_invalid: false,
                subjects: HashMap::new(),
            };
            assert!(Candidate::from_record(&record).is_some(), "{name}");
            record.metadata.kind = SessionKind::Terminal;
            assert!(Candidate::from_record(&record).is_none());
        }
    }

    #[test]
    fn title_decisions_are_events_and_omit_the_excerpt() {
        let root = tempfile::tempdir().unwrap();
        let state = super::super::tests::test_state(None, None);
        let history = root.path().join("pi.jsonl");
        let secret = "EXCERPT_SECRET_9f3a secret-body account@example.invalid";
        let write_history = |extra: &str| {
            fs::write(
                &history,
                format!(
                    "{{\"type\":\"session\",\"id\":\"c\"}}\n{{\"role\":\"user\",\"content\":\"{secret}\"}}\n{{\"role\":\"assistant\",\"content\":\"tail reply{extra}\"}}\n"
                ),
            )
            .unwrap();
        };
        write_history("");
        let reference = ConversationReference::Pi(ExtensionConversation {
            conversation: "c".into(),
            executable: "pi".into(),
            history: Some(history.clone()),
            config_dir: root.path().to_path_buf(),
            options: vec![],
        });
        let record = {
            let mut retained = state.retained.lock();
            let created = retained
                .create(SessionMetadata {
                    project: "p".into(),
                    workspace: "w".into(),
                    name: "n".into(),
                    label: "l".into(),
                    cwd: root.path().into(),
                    kind: SessionKind::Agent { name: "pi".into() },
                    pinned_title: None,
                    application_title: None,
                })
                .unwrap();
            let record = retained.begin_run(created.id, created.run).unwrap();
            retained
                .retain_conversation(record.id, record.run, Some(&reference))
                .unwrap();
            record
        };
        let mut worker = TitleWorker::new(root.path().join("titles"));
        worker.live_override = Some(HashMap::from([(record.id, record.run)]));
        worker.tick(&state);
        let messages = || {
            state
                .event_snapshot()
                .into_iter()
                .map(|event| event.message)
                .collect::<Vec<_>>()
        };
        assert_eq!(messages(), ["no title_model"]);
        assert_eq!(
            state.event_snapshot()[0].subject.as_deref(),
            Some(record.id.0.to_string()).as_deref()
        );

        state.settings.lock().unwrap().report.settings.title_model = Some("pi/missing".into());
        worker.pi_executable = Some(root.path().join("no-such-pi"));
        worker.tick(&state);
        assert!(messages().contains(&"call sent".into()));
        assert!(messages().contains(&"Pi missing for this model".into()));

        let failing = root.path().join("fail-pi");
        fs::write(&failing, "#!/bin/sh\nexit 2\n").unwrap();
        fs::set_permissions(&failing, fs::Permissions::from_mode(0o700)).unwrap();
        state.settings.lock().unwrap().report.settings.title_model = Some("pi/fail".into());
        worker.pi_executable = Some(failing);
        write_history(" v2");
        worker.tick(&state);
        assert!(
            messages().contains(&"call failed: Pi exited with code 2".into()),
            "{:?}",
            messages()
        );

        worker.tick(&state);
        assert!(messages().contains(&"history unchanged".into()));

        let ok = root.path().join("ok-pi");
        fs::write(
            &ok,
            "#!/bin/sh\nwhile IFS= read -r line; do printf '%s\\n' '{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Logged Topic\"}]}}'; exit 0; done\n",
        )
        .unwrap();
        fs::set_permissions(&ok, fs::Permissions::from_mode(0o700)).unwrap();
        state.settings.lock().unwrap().report.settings.title_model = Some("pi/ok".into());
        worker.pi_executable = Some(ok);
        write_history(" v3");
        worker.tick(&state);
        assert!(messages().contains(&"title applied".into()));

        state
            .retained
            .lock()
            .dismiss_conversation_subject(record.id, record.run, "c")
            .unwrap();
        write_history(" v4");
        worker.tick(&state);
        assert!(messages().contains(&"title dismissed".into()));

        let rendered = messages().join("\n");
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains("EXCERPT_SECRET_9f3a"));
        assert!(!rendered.contains("account@example.invalid"));
        assert!(!rendered.contains("Name this conversation"));
        assert!(!rendered.contains("Logged Topic"));
    }

    #[test]
    fn hermes_title_is_applied_from_the_session_row_without_a_model_call() {
        let root = tempfile::tempdir().unwrap();
        let state_db = root.path().join("state.db");
        let connection = rusqlite::Connection::open(&state_db).unwrap();
        connection
            .execute(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, title TEXT)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO sessions (id, title) VALUES ('20261006_101500_ab12cd', 'Ship the title')",
                [],
            )
            .unwrap();
        drop(connection);
        let state = super::super::tests::test_state(None, None);
        let reference =
            ovrcr_protocol::ConversationReference::Hermes(ovrcr_protocol::HermesConversation {
                conversation: "20261006_101500_ab12cd".into(),
                executable: "hermes".into(),
                state_db: state_db.clone(),
            });
        let record = {
            let mut retained = state.retained.lock();
            let created = retained
                .create(SessionMetadata {
                    project: "p".into(),
                    workspace: "w".into(),
                    name: "hermes-ready".into(),
                    label: "hermes".into(),
                    cwd: root.path().into(),
                    kind: SessionKind::Agent {
                        name: "hermes".into(),
                    },
                    pinned_title: None,
                    application_title: None,
                })
                .unwrap();
            let record = retained.begin_run(created.id, created.run).unwrap();
            retained
                .retain_conversation(record.id, record.run, Some(&reference))
                .unwrap();
            record
        };
        let mut worker = TitleWorker::new(root.path().join("titles"));
        worker.apply_hermes_titles(&state);
        let saved = state.retained.lock().get(record.id).unwrap().clone();
        assert_eq!(saved.effective_title().as_deref(), Some("Ship the title"));
        worker.apply_hermes_titles(&state);
        let messages = state
            .event_snapshot()
            .into_iter()
            .map(|event| event.message)
            .collect::<Vec<_>>();
        assert_eq!(
            messages
                .iter()
                .filter(|message| *message == "title applied")
                .count(),
            1
        );
        assert!(!messages.iter().any(|message| message.contains("call sent")));
        assert!(
            !messages
                .iter()
                .any(|message| message.contains("Ship the title"))
        );
    }
}

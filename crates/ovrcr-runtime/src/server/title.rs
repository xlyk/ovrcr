use super::*;
use ovrcr_protocol::{AgentProvider, ConversationReference};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::SystemTime;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const TAIL_LIMIT: u64 = 256 * 1024;
const EXCERPT_LIMIT: usize = 8 * 1024;
const MESSAGE_LIMIT: usize = 8;
const TITLE_TIMEOUT: Duration = Duration::from_secs(30);

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
                CallResult::Failed => {
                    self.emit(state, Some(record.id), "call failed or timed out");
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
        if fs::create_dir_all(&dir).is_err() {
            return CallResult::Failed;
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
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
        {
            Ok(child) => child,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return CallResult::MissingPi,
            Err(_) => return CallResult::Failed,
        };
        if let Some(mut stdin) = child.stdin.take() {
            let prompt = format!(
                "Name this conversation in a few words. Return only the topic. No quotes, status, secrets, or explanation.\n\n{excerpt}"
            );
            let mut request =
                match serde_json::to_vec(&json!({"type":"prompt","id":"title","message":prompt})) {
                    Ok(request) => request,
                    Err(_) => return kill_failed(child),
                };
            request.push(b'\n');
            if stdin.write_all(&request).is_err() {
                return kill_failed(child);
            }
        }
        let mut stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => return kill_failed(child),
        };
        if nonblocking(stdout.as_raw_fd()).is_err() {
            return kill_failed(child);
        }
        let deadline = Instant::now() + TITLE_TIMEOUT;
        let mut pending = Vec::new();
        let mut title = None;
        loop {
            if state.shutdown.load(Ordering::Acquire) {
                return kill_failed(child);
            }
            if Instant::now() >= deadline {
                return kill_failed(child);
            }
            let mut bytes = [0; 4096];
            match stdout.read(&mut bytes) {
                Ok(0) => {}
                Ok(n) => {
                    pending.extend_from_slice(&bytes[..n]);
                    while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                        let line: Vec<_> = pending.drain(..=end).collect();
                        if line.iter().all(u8::is_ascii_whitespace) {
                            continue;
                        }
                        if let Ok(event) = serde_json::from_slice::<Value>(&line)
                            && event["type"] == "message_end"
                            && event["message"]["role"] == "assistant"
                            && let Some(cleaned) =
                                message_text(&event["message"]).and_then(clean_title)
                        {
                            title = Some(cleaned);
                        }
                    }
                    if pending.len() > 1024 * 1024 {
                        return kill_failed(child);
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => return kill_failed(child),
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    return if status.success() {
                        title.map_or(CallResult::Failed, CallResult::Title)
                    } else {
                        CallResult::Failed
                    };
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => return kill_failed(child),
            }
        }
    }
}

fn kill_failed(mut child: Child) -> CallResult {
    let _ = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    let _ = child.wait();
    CallResult::Failed
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
    Failed,
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
        assert!(messages().contains(&"call failed or timed out".into()));

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

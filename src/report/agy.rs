//! Authenticated Antigravity status-line and Stop observations.
//!
//! The status line is the only source that may bind a conversation: Stop can run
//! from a child context and is accepted only after that foreground identity exists.
//! Its reported transcript path is intentionally discarded; recovery derives the
//! exact database path from the trusted local profile and UUID.
use super::InvocationLease;
use super::reporter::{self, Frames, Reporter};
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, AgentProvider, AgyConversation, ContextSample,
    ConversationReference, Measurement, MetricsSample, SampleQuality, UsageCoverage, UsageScope,
    UsageTotals, validate_agent_id,
};
use ovrcr_runtime::agent_runner::HookHandler;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    time::Instant,
};

const PROVIDER: &str = "agy";
const STATUSLINE_ORIGIN: &str = "statusline";
const STOP_ORIGIN: &str = "stop";
const MODEL_SOURCE: &str = "agy-statusline";
#[derive(Clone, Debug, PartialEq, Eq)]
enum Launch {
    Fresh,
    Resume(String),
}

/// Exact `agy` interactive CLI invocations. Unknown flags and bare subcommands
/// suppress reporting instead of broadening the launch contract by accident.
fn eligible_launch(argv: &[OsString]) -> Option<Launch> {
    if argv.first().and_then(|value| Path::new(value).file_name()) != Some(OsStr::new("agy")) {
        return None;
    }
    let mut launch = Launch::Fresh;
    let mut conversation_seen = false;
    let mut prompt_seen = false;
    let mut index = 1;
    while index < argv.len() {
        let raw = argv[index].to_str()?;
        if raw == "--" {
            if index + 1 < argv.len() {
                return None;
            }
            break;
        }
        let (name, inline) = raw
            .split_once('=')
            .map_or((raw, None), |(name, value)| (name, Some(value)));
        match name {
            "--model" | "--effort" | "--agent" | "--add-dir" => {
                let value = option_value(argv, &mut index, inline)?;
                if value.is_empty() || value.starts_with('-') {
                    return None;
                }
            }
            "--mode" => {
                let value = option_value(argv, &mut index, inline)?;
                if !matches!(value, "accept-edits" | "auto" | "plan") {
                    return None;
                }
            }
            "--sandbox" => match inline {
                None | Some("true" | "false") => {}
                Some(_) => return None,
            },
            "--conversation" => {
                let value = option_value(argv, &mut index, inline)?;
                if conversation_seen || !ovrcr_protocol::is_exact_agy_conversation_id(value) {
                    return None;
                }
                conversation_seen = true;
                launch = Launch::Resume(value.to_owned());
            }
            "--prompt-interactive" | "-i" => {
                if prompt_seen {
                    return None;
                }
                prompt_seen = true;
                let value = option_value(argv, &mut index, inline)?;
                if value.is_empty() {
                    return None;
                }
            }
            "--continue" | "-c" | "--new-project" | "--dangerously-skip-permissions" => {
                if inline.is_some() {
                    return None;
                }
            }
            _ => return None,
        }
        index += 1;
    }
    Some(launch)
}

fn option_value<'a>(
    argv: &'a [OsString],
    index: &mut usize,
    inline: Option<&'a str>,
) -> Option<&'a str> {
    if let Some(value) = inline {
        return Some(value);
    }
    *index += 1;
    argv.get(*index)?.to_str()
}

pub fn eligible_argv(argv: &[OsString]) -> bool {
    eligible_launch(argv).is_some()
}

fn executable_path(argv: &[OsString]) -> Option<PathBuf> {
    let value = PathBuf::from(argv.first()?);
    if value.is_absolute() {
        return Some(value);
    }
    if value.components().count() > 1 {
        return std::env::current_dir().ok().map(|cwd| cwd.join(value));
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(&value))
        .find(|path| path.is_absolute() && path.is_file())
}

fn recovery_reference(argv: &[OsString], launch: &Launch) -> Option<AgyConversation> {
    let conversation = match launch {
        Launch::Fresh => String::new(),
        Launch::Resume(conversation) => conversation.clone(),
    };
    Some(AgyConversation {
        conversation,
        executable: executable_path(argv)?,
        config_dir: ovrcr_runtime::agy_recovery::config_dir().ok()?,
        options: ovrcr_runtime::agy_recovery::launch_options(argv).ok()?,
        title: None,
    })
}

/// Install the authenticated foreground receiver on one managed interactive launch.
pub fn receiver(lease: Option<InvocationLease>, argv: &[OsString]) -> HookHandler {
    let reserved = lease.is_some();
    let launch = eligible_launch(argv);
    let mut reporter = Reporter::new(AgentProvider::Agy, lease, None);
    let unavailable = reporter::preflight(reserved, argv, eligible_argv, reporter::interactive())
        .map(str::to_owned)
        .or_else(|| {
            launch
                .is_none()
                .then(|| "unsupported Antigravity launch arguments".into())
        });
    let recovery = launch
        .as_ref()
        .and_then(|launch| recovery_reference(argv, launch));
    let unavailable = unavailable.or_else(|| {
        recovery
            .is_none()
            .then(|| "Antigravity recovery profile or executable is unavailable".into())
    });
    if let Some(reason) = unavailable {
        reporter.unavailable("Antigravity", &reason);
    }
    reporter.handler(Events {
        recovery,
        expected: match launch {
            Some(Launch::Resume(conversation)) => Some(conversation),
            Some(Launch::Fresh) | None => None,
        },
        bound: None,
        retained: None,
        model: None,
        activity: None,
        cycle: 0,
        cycle_active: false,
        seen_stop_execution_nums: HashSet::new(),
        stop_tracking_exhausted: false,
        successful_stop_cycle: None,
        completion_candidate: false,
        mismatch_reported: false,
    })
}

fn valid_text(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && !value.chars().any(char::is_control)
        && validate_agent_id(value).is_ok()
}

fn exact_id(value: &str) -> bool {
    ovrcr_protocol::is_exact_agy_conversation_id(value)
}

fn aliased<'a>(value: &'a Value, first: &str, second: &str) -> Option<&'a Value> {
    match (value.get(first), value.get(second)) {
        (Some(a), Some(b)) if a != b => None,
        (Some(a), _) => Some(a),
        (_, Some(b)) => Some(b),
        _ => None,
    }
}

fn raw_conversation_id(value: &Value) -> Option<&str> {
    let values = ["conversation_id", "conversationId", "session_id"]
        .into_iter()
        .filter_map(|key| value.get(key))
        .collect::<Vec<_>>();
    let first = values.first()?.as_str()?;
    values
        .iter()
        .all(|candidate| candidate.as_str() == Some(first))
        .then_some(first)
        .filter(|id| exact_id(id))
}

fn safe_title(value: Option<&Value>) -> Option<String> {
    let title = value?.as_str()?.trim();
    valid_text(title, 512).then(|| title.to_owned())
}

fn normalized_model(value: &Value) -> Option<String> {
    let model = value.get("model")?;
    let label = model
        .get("display_name")
        .and_then(Value::as_str)
        .filter(|value| valid_text(value, 256))
        .or_else(|| {
            model
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| valid_text(value, 256))
        })
        .or_else(|| model.as_str().filter(|value| valid_text(value, 256)))?;
    Some(label.to_owned())
}

/// Normalize a native status-line object to only identity, title, model and state.
/// In particular, transcript paths, prompts and arbitrary nested provider fields drop.
fn statusline_payload(input: &[u8]) -> Option<Value> {
    if input.is_empty() || input.len() > super::HOOK_INPUT_LIMIT {
        return None;
    }
    let value: Value = serde_json::from_slice(input).ok()?;
    let conversation = raw_conversation_id(&value)?;
    let state = value.get("agent_state")?.as_str()?;
    if !matches!(
        state,
        "idle" | "working" | "tool_use" | "reviewing" | "authenticating" | "initializing" | "error"
    ) {
        return None;
    }
    let mut payload = json!({
        "conversation_id": conversation,
        "agent_state": state,
    });
    if let Some(title) = safe_title(value.get("conversation_title")) {
        payload["conversation_title"] = json!(title);
    }
    if let Some(model) = normalized_model(&value) {
        payload["model"] = json!(model);
    }
    Some(payload)
}

fn has_error(value: &Value) -> bool {
    match value.get("error") {
        Some(Value::Null) => false,
        Some(Value::String(value)) if value.is_empty() => false,
        _ => true,
    }
}

const STOP_REASONS: &[&str] = &["NO_TOOL_CALL", "TOOL_CALL", "MAX_TURNS", "ERROR"];

/// Keep only bounded Stop fields; an arbitrary native error message never crosses the
/// private channel. Missing error is conservatively treated as an error.
fn stop_payload(input: &[u8]) -> Option<Value> {
    if input.is_empty() || input.len() > super::HOOK_INPUT_LIMIT {
        return None;
    }
    let value: Value = serde_json::from_slice(input).ok()?;
    let conversation = raw_conversation_id(&value)?;
    let execution_num = aliased(&value, "execution_num", "executionNum")?.as_u64()?;
    let termination_reason =
        aliased(&value, "termination_reason", "terminationReason")?.as_str()?;
    if !STOP_REASONS.contains(&termination_reason) {
        return None;
    }
    let fully_idle = aliased(&value, "fully_idle", "fullyIdle")?.as_bool()?;
    let continuation_active =
        aliased(&value, "continuation_active", "continuationActive")?.as_bool()?;
    Some(json!({
        "conversation_id": conversation,
        "execution_num": execution_num,
        "termination_reason": termination_reason,
        "fully_idle": fully_idle,
        "continuation_active": continuation_active,
        "has_error": has_error(&value),
    }))
}

/// Forward a sanitized status line only when a managed invocation channel exists.
pub fn send_agy_statusline(input: &[u8], deadline: Instant) -> anyhow::Result<()> {
    if std::env::var_os("OVRCR_AGENT_SOCKET").is_none() {
        return Ok(());
    }
    let Some(payload) = statusline_payload(input) else {
        return Ok(());
    };
    super::send_payload(
        &serde_json::to_vec(&payload)?,
        PROVIDER,
        STATUSLINE_ORIGIN,
        deadline,
    )
}

/// Forward only bounded Stop telemetry. Native Stop always remains allowed to finish.
pub fn send_agy_stop(input: &[u8], deadline: Instant) -> anyhow::Result<()> {
    if std::env::var_os("OVRCR_AGENT_SOCKET").is_none() {
        return Ok(());
    }
    let Some(payload) = stop_payload(input) else {
        return Ok(());
    };
    super::send_payload(
        &serde_json::to_vec(&payload)?,
        PROVIDER,
        STOP_ORIGIN,
        deadline,
    )
}

fn state(value: &str) -> AgentActivity {
    match value {
        "idle" => AgentActivity::Idle,
        "error" => AgentActivity::Error,
        "working" | "tool_use" | "reviewing" | "authenticating" | "initializing" => {
            AgentActivity::Busy
        }
        _ => AgentActivity::Unknown,
    }
}

fn active_foreground_state(value: &str) -> bool {
    matches!(value, "working" | "tool_use" | "reviewing")
}

/// One Antigravity launch. Status-line identity changes rebind through the reporter's
/// server generation only after the exact UUID is found in the launch's local profile.
struct Events {
    recovery: Option<AgyConversation>,
    expected: Option<String>,
    bound: Option<String>,
    retained: Option<AgyConversation>,
    model: Option<String>,
    activity: Option<AgentActivity>,
    cycle: u64,
    cycle_active: bool,
    seen_stop_execution_nums: HashSet<u64>,
    stop_tracking_exhausted: bool,
    successful_stop_cycle: Option<(u64, u64)>,
    completion_candidate: bool,
    mismatch_reported: bool,
}

impl Events {
    fn bind_if_foreground(
        &mut self,
        reporter: &mut Reporter,
        conversation: &str,
        deadline: Instant,
    ) -> Option<Vec<u8>> {
        if self.bound.as_deref() == Some(conversation) {
            return None;
        }
        if self
            .expected
            .as_deref()
            .is_some_and(|expected| expected != conversation)
        {
            if !self.mismatch_reported {
                eprintln!(
                    "Antigravity exact resume identity mismatch; waiting for the requested foreground conversation."
                );
                self.mismatch_reported = true;
            }
            return Some(reporter::IGNORED.to_vec());
        }
        let Some(mut reference) = self.recovery.clone() else {
            return Some(reporter::IGNORED.to_vec());
        };
        reference.conversation = conversation.to_owned();
        let switching = self.bound.is_some();
        if switching {
            reference.title = None;
        }
        if ovrcr_runtime::agy_recovery::validate_profile(&reference, &reference.config_dir).is_err()
        {
            return Some(reporter::IGNORED.to_vec());
        }
        if !reporter.bind(conversation, deadline, false) {
            return Some(if reporter.closed() {
                reporter::UNAVAILABLE.to_vec()
            } else {
                // A lost bind receipt remains pending and is resolved by the next
                // foreground status line before any observation is published.
                reporter::IGNORED.to_vec()
            });
        }
        self.bound = Some(conversation.to_owned());
        self.expected = None;
        self.mismatch_reported = false;
        self.recovery = Some(reference);
        self.model = None;
        self.activity = None;
        if switching {
            self.cycle = 0;
        }
        self.cycle_active = false;
        self.seen_stop_execution_nums.clear();
        self.stop_tracking_exhausted = false;
        self.successful_stop_cycle = None;
        self.completion_candidate = false;
        None
    }

    fn retain(
        &mut self,
        reporter: &mut Reporter,
        conversation: &str,
        title: Option<&str>,
        deadline: Instant,
    ) {
        let Some(recovery) = self.recovery.as_mut() else {
            return;
        };
        if let Some(title) = title {
            recovery.title = Some(title.to_owned());
        }
        let mut reference = recovery.clone();
        reference.conversation = conversation.to_owned();
        if self.retained.as_ref() == Some(&reference) {
            return;
        }
        if reporter.retain_conversation(ConversationReference::Agy(reference.clone()), deadline) {
            self.retained = Some(reference);
        }
    }

    fn publish_model(
        &mut self,
        reporter: &mut Reporter,
        model: Option<&str>,
        deadline: Instant,
    ) -> Vec<u8> {
        let Some(model) = model else {
            return reporter::IGNORED.to_vec();
        };
        if self.model.as_deref() == Some(model) {
            return reporter::IGNORED.to_vec();
        }
        if !valid_text(model, 256) {
            return reporter::IGNORED.to_vec();
        }
        let source = MODEL_SOURCE.to_owned();
        let result = reporter.publish(
            AgentObservation::Metrics(Box::new(MetricsSample {
                model: Some(model.to_owned()),
                context: Measurement {
                    value: ContextSample {
                        used_tokens: None,
                        capacity_tokens: None,
                        quality: SampleQuality::Observed,
                    },
                    source: source.clone(),
                },
                usage: Measurement {
                    value: UsageTotals {
                        scope: UsageScope::Conversation,
                        coverage: UsageCoverage::Partial,
                        input_tokens: None,
                        output_tokens: None,
                        cache_read_tokens: None,
                        cache_write_tokens: None,
                        reasoning_output_tokens: None,
                    },
                    source: source.clone(),
                },
                cost: Measurement {
                    value: None,
                    source,
                },
            })),
            deadline,
        );
        if result == reporter::ACCEPTED {
            self.model = Some(model.to_owned());
        }
        result
    }

    fn statusline(
        &mut self,
        reporter: &mut Reporter,
        payload: &Value,
        deadline: Instant,
    ) -> Vec<u8> {
        let Some(conversation) = payload["conversation_id"]
            .as_str()
            .filter(|id| exact_id(id))
        else {
            return reporter::IGNORED.to_vec();
        };
        let title = safe_title(payload.get("conversation_title"));
        if let Some(answer) = self.bind_if_foreground(reporter, conversation, deadline) {
            return answer;
        }
        self.retain(reporter, conversation, title.as_deref(), deadline);

        let model_result = self.publish_model(reporter, payload["model"].as_str(), deadline);
        if model_result == reporter::UNAVAILABLE {
            return model_result;
        }

        let Some(native_state) = payload["agent_state"].as_str() else {
            return reporter::IGNORED.to_vec();
        };
        let next_activity = state(native_state);
        if next_activity == AgentActivity::Unknown {
            return reporter::IGNORED.to_vec();
        }
        self.advance_cycle(native_state);
        let readiness_enabled = reporter
            .binding()
            .is_some_and(|binding| binding.provider.supports_readiness());
        let report_state = if self.completion_candidate && readiness_enabled {
            AgentActivity::ResponseReady
        } else {
            next_activity
        };
        if self.activity == Some(report_state) {
            return if model_result == reporter::ACCEPTED {
                reporter::ACCEPTED.to_vec()
            } else {
                reporter::IGNORED.to_vec()
            };
        }
        let turn = (report_state == AgentActivity::ResponseReady)
            .then(|| format!("agy-cycle-{}", self.cycle));
        let result = reporter.publish(
            AgentObservation::Activity(ActivitySample {
                state: report_state,
                quality: SampleQuality::Observed,
                turn,
            }),
            deadline,
        );
        if result == reporter::ACCEPTED {
            self.activity = Some(report_state);
        }
        result
    }

    fn advance_cycle(&mut self, native_state: &str) {
        if active_foreground_state(native_state) {
            let continued_after_stop = self.successful_stop_cycle.is_some();
            let new_submission = !self.cycle_active && !continued_after_stop;
            if new_submission {
                // Antigravity restarts execution numbers on a new submission.
                // A continued submission must keep rejecting already seen Stops.
                self.seen_stop_execution_nums.clear();
                self.stop_tracking_exhausted = false;
            }
            if continued_after_stop || new_submission {
                self.cycle = self.cycle.saturating_add(1);
                self.successful_stop_cycle = None;
                self.completion_candidate = false;
            }
            self.cycle_active = true;
            return;
        }
        if native_state == "idle" {
            if self.cycle_active
                && self
                    .successful_stop_cycle
                    .is_some_and(|(cycle, _)| cycle == self.cycle)
            {
                self.completion_candidate = true;
            }
            // Keep the completion candidate for this settled cycle, but retire the
            // Stop marker so a later active state starts a fresh submission.
            self.successful_stop_cycle = None;
            self.cycle_active = false;
        } else if native_state == "error" {
            self.cycle_active = false;
            self.successful_stop_cycle = None;
            self.completion_candidate = false;
        } else if matches!(native_state, "initializing" | "authenticating") {
            // Startup and authentication can report Busy without a user response
            // turn. They retire old candidates and require a later foreground work
            // state before a Stop can qualify.
            self.cycle_active = false;
            self.successful_stop_cycle = None;
            self.completion_candidate = false;
        }
    }

    fn stop(&mut self, reporter: &mut Reporter, payload: &Value) -> Vec<u8> {
        let Some(conversation) = payload["conversation_id"]
            .as_str()
            .filter(|id| exact_id(id))
        else {
            return reporter::IGNORED.to_vec();
        };
        if self.bound.as_deref() != Some(conversation)
            || !reporter.binding().is_some_and(|binding| {
                binding.provider == AgentProvider::Agy && binding.conversation == conversation
            })
            || !self.cycle_active
        {
            return reporter::IGNORED.to_vec();
        }
        let Some(execution_num) = payload["execution_num"].as_u64() else {
            return reporter::IGNORED.to_vec();
        };
        if self.stop_tracking_exhausted {
            return reporter::IGNORED.to_vec();
        }
        if !self.seen_stop_execution_nums.insert(execution_num) {
            return reporter::IGNORED.to_vec();
        }
        if self.seen_stop_execution_nums.len() > 1024 {
            self.seen_stop_execution_nums.clear();
            self.stop_tracking_exhausted = true;
            self.successful_stop_cycle = None;
            self.completion_candidate = false;
            return reporter::IGNORED.to_vec();
        }
        let successful = payload["termination_reason"] == "NO_TOOL_CALL"
            && payload["fully_idle"] == true
            && payload["continuation_active"] == false
            && payload["has_error"] == false;
        self.successful_stop_cycle = successful.then_some((self.cycle, execution_num));
        if !successful {
            self.completion_candidate = false;
        }
        reporter::IGNORED.to_vec()
    }
}

impl Frames for Events {
    fn frame(
        &mut self,
        reporter: &mut Reporter,
        input: &[u8],
        native_root: bool,
        deadline: Instant,
    ) -> Vec<u8> {
        if let Some(answer) = reporter.own_frame(input, native_root) {
            return answer;
        }
        let Ok(envelope) = serde_json::from_slice::<Value>(input) else {
            return reporter::IGNORED.to_vec();
        };
        if envelope["provider"] != PROVIDER {
            return reporter::IGNORED.to_vec();
        }
        match envelope["origin"].as_str() {
            Some(STATUSLINE_ORIGIN) => self.statusline(reporter, &envelope["payload"], deadline),
            Some(STOP_ORIGIN) => self.stop(reporter, &envelope["payload"]),
            _ => reporter::IGNORED.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::reporter::scripted::{Observed, Supervisor};

    const A: &str = "01a08e5c-7480-7052-9964-9224aadebef0";
    const B: &str = "01a08e94-0deb-76c3-a2b5-540c4874a53f";

    fn deadline() -> Instant {
        Instant::now() + std::time::Duration::from_secs(5)
    }

    fn profile() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let config_dir = root.path().join(".gemini/antigravity-cli");
        let conversations = config_dir.join("conversations");
        std::fs::create_dir_all(&conversations).unwrap();
        for identity in [A, B] {
            let connection =
                rusqlite::Connection::open(conversations.join(format!("{identity}.db"))).unwrap();
            connection
                .execute_batch("CREATE TABLE trajectory_meta (cascade_id TEXT NOT NULL);")
                .unwrap();
            connection
                .execute(
                    "INSERT INTO trajectory_meta (cascade_id) VALUES (?1)",
                    [identity],
                )
                .unwrap();
        }
        (root, config_dir)
    }

    fn start(expected: Option<&str>) -> (Events, Reporter, Supervisor, tempfile::TempDir) {
        let (profile_root, config_dir) = profile();
        let (reporter, supervisor) = Supervisor::reporter(AgentProvider::Agy);
        (
            Events {
                recovery: Some(AgyConversation {
                    conversation: expected.unwrap_or_default().into(),
                    executable: "/bin/agy".into(),
                    config_dir,
                    options: vec!["--dangerously-skip-permissions".into()],
                    title: None,
                }),
                expected: expected.map(str::to_owned),
                bound: None,
                retained: None,
                model: None,
                activity: None,
                cycle: 0,
                cycle_active: false,
                seen_stop_execution_nums: HashSet::new(),
                stop_tracking_exhausted: false,
                successful_stop_cycle: None,
                completion_candidate: false,
                mismatch_reported: false,
            },
            reporter,
            supervisor,
            profile_root,
        )
    }

    fn frame(origin: &str, payload: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"provider":"agy", "origin":origin, "payload":payload})).unwrap()
    }

    fn status(id: &str, state: &str) -> Vec<u8> {
        frame(
            STATUSLINE_ORIGIN,
            json!({"conversation_id":id, "agent_state":state}),
        )
    }

    fn stop(
        id: &str,
        execution: u64,
        reason: &str,
        idle: bool,
        continuation: bool,
        error: bool,
    ) -> Vec<u8> {
        frame(
            STOP_ORIGIN,
            json!({
                "conversation_id":id,
                "execution_num":execution,
                "termination_reason":reason,
                "fully_idle":idle,
                "continuation_active":continuation,
                "has_error":error
            }),
        )
    }

    fn drive(events: &mut Events, reporter: &mut Reporter, bytes: &[u8]) -> Vec<u8> {
        events.frame(reporter, bytes, true, deadline())
    }

    #[test]
    fn statusline_allowlist_keeps_only_identity_title_model_and_state() {
        let input = json!({
            "conversation_id":A,
            "conversation_title":"  Native topic  ",
            "transcript_path":"/secret/path",
            "last_user_input":"private prompt",
            "model":{"id":"model-id","display_name":"Gemini Pro"},
            "agent_state":"working",
            "unrelated":{"secret":"drop"}
        });
        let normalized = statusline_payload(&serde_json::to_vec(&input).unwrap()).unwrap();
        assert_eq!(normalized["conversation_id"], A);
        assert_eq!(normalized["conversation_title"], "Native topic");
        assert_eq!(normalized["model"], "Gemini Pro");
        assert_eq!(normalized["agent_state"], "working");
        assert_eq!(normalized.as_object().unwrap().len(), 4);
        let text = normalized.to_string();
        assert!(!text.contains("secret"));
        assert!(!text.contains("private prompt"));
        assert!(
            statusline_payload(br#"{"conversation_id":"wrong","agent_state":"working"}"#).is_none()
        );
    }

    #[test]
    fn stop_allowlist_drops_raw_errors_and_requires_conservative_signal_fields() {
        let input = json!({
            "conversationId":A,
            "executionNum":0,
            "terminationReason":"NO_TOOL_CALL",
            "fullyIdle":true,
            "continuationActive":false,
            "error":"account secret",
            "transcriptPath":"/not-retained"
        });
        let normalized = stop_payload(&serde_json::to_vec(&input).unwrap()).unwrap();
        assert_eq!(normalized["conversation_id"], A);
        assert_eq!(normalized["execution_num"], 0);
        assert_eq!(normalized["has_error"], true);
        assert_eq!(normalized.as_object().unwrap().len(), 6);
        assert!(!normalized.to_string().contains("account secret"));
        let mut no_error = input.clone();
        no_error["error"] = Value::Null;
        assert_eq!(
            stop_payload(&serde_json::to_vec(&no_error).unwrap()).unwrap()["has_error"],
            false
        );
        let missing = json!({
            "conversationId":A,"executionNum":0,"terminationReason":"NO_TOOL_CALL",
            "fullyIdle":true,"continuationActive":false
        });
        assert_eq!(
            stop_payload(&serde_json::to_vec(&missing).unwrap()).unwrap()["has_error"],
            true
        );
    }

    #[test]
    fn eligible_launches_preserve_exact_resume_and_reject_subcommands_or_remote_profiles() {
        assert_eq!(
            eligible_launch(&["/usr/bin/agy".into(), "--conversation".into(), A.into()]),
            Some(Launch::Resume(A.into()))
        );
        assert!(eligible_argv(&[
            "agy".into(),
            "--dangerously-skip-permissions".into(),
            "--mode".into(),
            "plan".into(),
            "--prompt-interactive".into(),
            "secret prompt".into()
        ]));
        assert!(eligible_argv(&[
            "agy".into(),
            "--mode".into(),
            "accept-edits".into(),
            "--sandbox".into()
        ]));
        assert!(!eligible_argv(&["agy".into(), "models".into()]));
        for args in [
            vec!["agy", "--settings"],
            vec!["agy", "--settings", "settings.json"],
            vec!["agy", "--settings=custom.json"],
            vec!["agy", "--sandbox", "false"],
        ] {
            assert!(
                !eligible_argv(&args.iter().copied().map(OsString::from).collect::<Vec<_>>()),
                "{args:?}"
            );
        }
        assert!(!eligible_argv(&[
            "agy".into(),
            "--project".into(),
            "remote".into()
        ]));
        assert!(!eligible_argv(&[
            "agy".into(),
            "--conversation".into(),
            "../escape".into()
        ]));
        assert!(!eligible_argv(&[
            "agy".into(),
            "--conversation".into(),
            A.into(),
            "--conversation".into(),
            B.into()
        ]));
        assert!(!eligible_argv(&[
            "agy".into(),
            "--prompt-interactive".into(),
            "one".into(),
            "-i".into(),
            "two".into()
        ]));
        for args in [
            vec!["agy", "--prompt-interactive="],
            vec!["agy", "--", "secret"],
            vec!["agy", "--model", "-hidden"],
            vec!["agy", "--effort", "-hidden"],
            vec!["agy", "--agent", "-hidden"],
            vec!["agy", "--model=-hidden"],
            vec!["agy", "--effort=-hidden"],
            vec!["agy", "--agent=-hidden"],
        ] {
            assert!(
                !eligible_argv(&args.iter().copied().map(OsString::from).collect::<Vec<_>>()),
                "{args:?}"
            );
        }
        assert!(eligible_argv(&["agy".into(), "--".into()]));
        assert!(eligible_argv(&[
            "agy".into(),
            "--prompt-interactive".into(),
            "--".into()
        ]));
    }

    #[test]
    fn statusline_binds_foreground_publishes_model_title_and_busy_but_stop_never_binds() {
        let (mut events, mut reporter, supervisor, _profile) = start(None);
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &stop(A, 0, "NO_TOOL_CALL", true, false, false)
            ),
            reporter::IGNORED
        );
        assert!(reporter.binding().is_none());

        let status_frame = frame(
            STATUSLINE_ORIGIN,
            json!({
                "conversation_id":A,
                "conversation_title":"Native title",
                "model":"Gemini Pro",
                "agent_state":"working"
            }),
        );
        assert_eq!(
            drive(&mut events, &mut reporter, &status_frame),
            reporter::ACCEPTED
        );
        assert_eq!(reporter.binding().unwrap().conversation, A);
        let repeated_without_title = status(A, "working");
        assert_eq!(
            drive(&mut events, &mut reporter, &repeated_without_title),
            reporter::IGNORED
        );
        let observed = supervisor.observed();
        assert!(observed.iter().any(|row| matches!(row, Observed::Retain { reference: ConversationReference::Agy(reference), .. } if reference.conversation == A && reference.title.as_deref() == Some("Native title"))));
        assert_eq!(
            observed
                .iter()
                .filter(|row| matches!(row, Observed::Retain { .. }))
                .count(),
            1,
            "a missing title does not erase the old one or repeat a durable write"
        );
        assert_eq!(Supervisor::models(&observed), [Some("Gemini Pro".into())]);
        assert_eq!(
            Supervisor::activity(&observed)
                .into_iter()
                .map(|(_, _, state, _, _)| state)
                .collect::<Vec<_>>(),
            [AgentActivity::Busy]
        );
    }

    #[test]
    fn expected_resume_mismatch_is_ignored_until_the_native_uuid_matches() {
        let (mut events, mut reporter, supervisor, _profile) = start(Some(A));
        assert_eq!(
            drive(&mut events, &mut reporter, &status(B, "working")),
            reporter::IGNORED
        );
        assert!(events.mismatch_reported);
        assert!(!reporter.closed(), "a mismatch keeps the terminal usable");
        assert_eq!(
            drive(&mut events, &mut reporter, &status(A, "idle")),
            reporter::ACCEPTED
        );
        assert!(!reporter.closed());
        let observed = supervisor.observed();
        let bindings = observed
            .iter()
            .filter(|row| matches!(row, Observed::Bind { .. }))
            .count();
        assert_eq!(bindings, 1, "the mismatched resume id never binds");
        assert_eq!(reporter.binding().unwrap().conversation, A);
    }

    #[test]
    fn verified_foreground_switches_get_new_generations_and_can_switch_back() {
        let (mut events, mut reporter, supervisor, _profile) = start(None);
        assert_eq!(
            drive(&mut events, &mut reporter, &status(A, "idle")),
            reporter::ACCEPTED
        );
        assert_eq!(reporter.binding().unwrap().generation, 1);
        assert_eq!(
            drive(&mut events, &mut reporter, &status(B, "working")),
            reporter::ACCEPTED
        );
        assert_eq!(reporter.binding().unwrap().conversation, B);
        assert_eq!(reporter.binding().unwrap().generation, 2);
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &stop(A, 0, "NO_TOOL_CALL", true, false, false)
            ),
            reporter::IGNORED
        );

        let missing = "01a08e94-0deb-76c3-a2b5-540c4874a540";
        assert_eq!(
            drive(&mut events, &mut reporter, &status(missing, "idle")),
            reporter::IGNORED
        );
        assert_eq!(
            reporter.binding().unwrap().conversation,
            B,
            "an unverified UUID cannot displace the current generation"
        );

        // A later native foreground callback can legitimately return to a prior UUID.
        assert_eq!(
            drive(&mut events, &mut reporter, &status(A, "working")),
            reporter::ACCEPTED
        );
        assert_eq!(reporter.binding().unwrap().conversation, A);
        assert_eq!(reporter.binding().unwrap().generation, 3);
        let observed = supervisor.observed();
        let generations = observed
            .iter()
            .filter_map(|row| match row {
                Observed::Bind {
                    conversation,
                    generation,
                    ..
                } => Some((conversation.as_str(), *generation)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(generations, [(A, 1), (B, 2), (A, 3)]);
    }

    #[test]
    fn non_native_peer_cannot_bind_and_stop_from_a_different_conversation_is_ignored() {
        let (mut events, mut reporter, supervisor, _profile) = start(None);
        let request = status(A, "working");
        assert_eq!(
            events.frame(&mut reporter, &request, false, deadline()),
            reporter::IGNORED
        );
        assert!(reporter.binding().is_none());
        assert_eq!(
            drive(&mut events, &mut reporter, &request),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &stop(B, 0, "NO_TOOL_CALL", true, false, false)
            ),
            reporter::IGNORED
        );
        let observed = supervisor.observed();
        assert_eq!(
            observed
                .iter()
                .filter(|row| matches!(row, Observed::Bind { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn stop_and_post_stop_idle_follow_provider_readiness_capability() {
        let (mut events, mut reporter, supervisor, _profile) = start(None);
        assert_eq!(
            drive(&mut events, &mut reporter, &status(A, "working")),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &stop(A, 0, "NO_TOOL_CALL", true, false, false)
            ),
            reporter::IGNORED
        );
        assert!(!events.completion_candidate, "idle must follow Stop");
        assert_eq!(
            drive(&mut events, &mut reporter, &status(A, "idle")),
            reporter::ACCEPTED
        );
        assert!(events.completion_candidate);
        assert_eq!(
            drive(&mut events, &mut reporter, &status(A, "working")),
            reporter::ACCEPTED
        );
        assert!(
            !events.completion_candidate,
            "a new foreground cycle retires the old candidate"
        );

        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &stop(A, 0, "NO_TOOL_CALL", true, false, false)
            ),
            reporter::IGNORED
        );
        assert_eq!(
            drive(&mut events, &mut reporter, &status(A, "idle")),
            reporter::ACCEPTED
        );
        assert!(events.completion_candidate);
        let observed = supervisor.observed();
        let ready_count = Supervisor::activity(&observed)
            .iter()
            .filter(|(_, _, state, _, _)| *state == AgentActivity::ResponseReady)
            .count();
        assert_eq!(
            ready_count,
            if AgentProvider::Agy.supports_readiness() {
                2
            } else {
                0
            },
            "Ready follows the provider capability gate"
        );
    }

    #[test]
    fn continued_stop_invalidates_its_candidate_and_resets_execution_dedup() {
        let (mut events, mut reporter, supervisor, _profile) = start(None);
        drive(&mut events, &mut reporter, &status(A, "working"));
        drive(
            &mut events,
            &mut reporter,
            &stop(A, 0, "NO_TOOL_CALL", true, false, false),
        );
        assert_eq!(events.successful_stop_cycle, Some((events.cycle, 0)));
        drive(&mut events, &mut reporter, &status(A, "working"));
        assert_eq!(events.successful_stop_cycle, None);
        assert!(!events.completion_candidate);
        drive(&mut events, &mut reporter, &status(A, "working"));
        assert_eq!(
            events.cycle, 2,
            "repeated Busy does not create another segment"
        );

        drive(
            &mut events,
            &mut reporter,
            &stop(A, 1, "NO_TOOL_CALL", true, false, false),
        );
        drive(
            &mut events,
            &mut reporter,
            &stop(A, 1, "NO_TOOL_CALL", true, false, false),
        );
        drive(&mut events, &mut reporter, &status(A, "idle"));
        assert!(events.completion_candidate);
        let observed = supervisor.observed();
        let activities = Supervisor::activity(&observed);
        let ready = activities
            .iter()
            .filter(|(_, _, state, _, _)| *state == AgentActivity::ResponseReady)
            .count();
        assert_eq!(
            ready,
            usize::from(AgentProvider::Agy.supports_readiness()),
            "only the post-continuation Stop and later Idle can produce Ready, once"
        );
    }

    #[test]
    fn stale_stop_after_continuation_cannot_create_unread_but_new_submission_can_reset() {
        let (mut events, mut reporter, supervisor, _profile) = start(None);
        drive(&mut events, &mut reporter, &status(A, "working"));
        drive(
            &mut events,
            &mut reporter,
            &stop(A, 0, "NO_TOOL_CALL", true, false, false),
        );

        // The native continuation proves the first Stop did not settle. A delayed
        // Stop(0), even if it reports cancellation, is still the same execution.
        drive(&mut events, &mut reporter, &status(A, "working"));
        assert_eq!(events.cycle, 2);
        drive(
            &mut events,
            &mut reporter,
            &stop(A, 0, "ERROR", true, false, true),
        );
        drive(&mut events, &mut reporter, &status(A, "idle"));
        assert!(!events.completion_candidate);

        // A fresh user submission begins after idle and can legitimately reset the
        // native execution number back to zero.
        drive(&mut events, &mut reporter, &status(A, "working"));
        assert_eq!(events.cycle, 3);
        drive(
            &mut events,
            &mut reporter,
            &stop(A, 0, "NO_TOOL_CALL", true, false, false),
        );
        drive(&mut events, &mut reporter, &status(A, "idle"));
        assert!(events.completion_candidate);
        let observed = supervisor.observed();
        assert_eq!(
            Supervisor::activity(&observed)
                .iter()
                .filter(|(_, _, state, _, _)| *state == AgentActivity::ResponseReady)
                .count(),
            usize::from(AgentProvider::Agy.supports_readiness())
        );
    }

    #[test]
    fn canceled_initializing_submission_without_stop_retires_prior_ready_candidate() {
        let (mut events, mut reporter, supervisor, _profile) = start(None);
        drive(&mut events, &mut reporter, &status(A, "working"));
        drive(
            &mut events,
            &mut reporter,
            &stop(A, 0, "NO_TOOL_CALL", true, false, false),
        );
        drive(&mut events, &mut reporter, &status(A, "idle"));
        assert!(events.completion_candidate);

        // A new request can be interrupted during initialization, before a native
        // work state is observed. This startup Busy signal retires the old candidate
        // but cannot admit Stop as evidence of a response turn.
        drive(&mut events, &mut reporter, &status(A, "initializing"));
        assert!(!events.completion_candidate);
        assert!(!events.cycle_active);
        drive(
            &mut events,
            &mut reporter,
            &stop(A, 1, "NO_TOOL_CALL", true, false, false),
        );
        drive(&mut events, &mut reporter, &status(A, "idle"));
        assert!(!events.completion_candidate);

        let observed = supervisor.observed();
        let ready = Supervisor::activity(&observed)
            .iter()
            .filter(|(_, _, state, _, _)| *state == AgentActivity::ResponseReady)
            .count();
        assert_eq!(
            ready,
            usize::from(AgentProvider::Agy.supports_readiness()),
            "cancellation without Stop cannot create another Ready"
        );
    }

    #[test]
    fn failed_stop_or_wrong_identity_cannot_create_a_completion_candidate() {
        let (mut events, mut reporter, supervisor, _profile) = start(None);
        drive(&mut events, &mut reporter, &status(A, "working"));
        drive(
            &mut events,
            &mut reporter,
            &stop(A, 0, "TOOL_CALL", true, false, false),
        );
        drive(&mut events, &mut reporter, &status(A, "idle"));
        assert!(!events.completion_candidate);
        let _observed = supervisor.observed();

        drive(&mut events, &mut reporter, &status(A, "working"));
        drive(
            &mut events,
            &mut reporter,
            &stop(B, 0, "NO_TOOL_CALL", true, false, false),
        );
        drive(&mut events, &mut reporter, &status(A, "idle"));
        assert!(!events.completion_candidate);
    }
}

//! Hermes shell-hook observations. The hook process forwards only the fields
//! named here; prompts, transcripts and tool arguments never reach the server.
use super::InvocationLease;
use super::reporter::{self, Frames, Reporter};
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, AgentProvider, ContextSample,
    ConversationReference, HermesConversation, InputKind, InputRequest, MAX_INPUT_REQUESTS,
    Measurement, MetricsSample, SampleQuality, UsageCoverage, UsageScope, UsageTotals,
};
use ovrcr_runtime::agent_runner::HookHandler;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::time::Instant;

const EVENTS: &[&str] = &[
    "on_session_start",
    "pre_api_request",
    "post_api_request",
    "post_llm_call",
    "on_session_end",
    "pre_approval_request",
    "post_approval_response",
];

const VALUE_FLAGS: &[&str] = &[
    "-z",
    "--oneshot",
    "-m",
    "--model",
    "--provider",
    "--reasoning",
    "-t",
    "--toolsets",
    "-r",
    "--resume",
    "-s",
    "--skills",
    "--usage-file",
    "--in",
    "-p",
    "--profile",
    "--continue",
    "-c",
];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UsageNotice {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Notice {
    pub event: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub model: Option<String>,
    pub interrupted: bool,
    pub completed: bool,
    pub approval_id: Option<String>,
    pub approval_open: bool,
    pub usage: Option<UsageNotice>,
    pub state_db: Option<String>,
}

pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|value| Path::new(value).file_name()) != Some(OsStr::new("hermes")) {
        return false;
    }
    let mut skip_value = false;
    let mut positional = None;
    for arg in &argv[1..] {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        if skip_value {
            skip_value = false;
            continue;
        }
        if arg == "--" {
            break;
        }
        if let Some((name, _)) = arg.split_once('=')
            && name.starts_with('-')
        {
            continue;
        }
        if arg.starts_with('-') {
            if VALUE_FLAGS.contains(&arg) {
                skip_value = true;
            }
            continue;
        }
        positional = Some(arg);
        break;
    }
    matches!(positional, None | Some("chat"))
}

/// Keep the identity, turn, model, approval id and token counts. Drop everything else.
pub(crate) fn notice_from_hook(input: &[u8], state_db: Option<&str>) -> Option<Notice> {
    if input.is_empty() || input.len() > 8 * 1024 * 1024 {
        return None;
    }
    let value: Value = serde_json::from_slice(input).ok()?;
    let event = value.get("hook_event_name")?.as_str()?;
    if !EVENTS.contains(&event) {
        return None;
    }
    let session_id = value
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !ovrcr_runtime::hermes_recovery::valid_session_id(session_id) {
        return None;
    }
    let extra = value.get("extra");
    let turn_id = extra
        .and_then(|extra| extra.get("turn_id"))
        .and_then(Value::as_str)
        .filter(|value| ovrcr_protocol::validate_agent_id(value).is_ok())
        .map(str::to_owned);
    let model = extra
        .and_then(|extra| extra.get("model"))
        .and_then(Value::as_str)
        .filter(|value| ovrcr_protocol::validate_agent_id(value).is_ok())
        .map(str::to_owned);
    let interrupted = extra
        .and_then(|extra| extra.get("interrupted"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let completed = extra
        .and_then(|extra| extra.get("completed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let approval_id = extra
        .and_then(|extra| extra.get("tool_call_id"))
        .and_then(Value::as_str)
        .filter(|value| ovrcr_protocol::validate_agent_id(value).is_ok())
        .map(|value| format!("approval:{value}"));
    let approval_open = event == "pre_approval_request";
    if matches!(event, "pre_approval_request" | "post_approval_response") && approval_id.is_none() {
        return None;
    }
    let usage = (event == "post_api_request")
        .then(|| extra.and_then(|extra| extra.get("usage")))
        .flatten()
        .and_then(usage_notice);
    Some(Notice {
        event: event.to_owned(),
        session_id: session_id.to_owned(),
        turn_id,
        model,
        interrupted,
        completed,
        approval_id,
        approval_open,
        usage,
        state_db: state_db.map(str::to_owned),
    })
}

fn usage_notice(value: &Value) -> Option<UsageNotice> {
    let object = value.as_object()?;
    if object.contains_key("raw_usage") {
        return None;
    }
    Some(UsageNotice {
        input_tokens: number(object.get("input_tokens"))?,
        output_tokens: number(object.get("output_tokens"))?,
        cache_read_tokens: number(object.get("cache_read_tokens")).unwrap_or(0),
        cache_write_tokens: number(object.get("cache_write_tokens")).unwrap_or(0),
        reasoning_tokens: number(object.get("reasoning_tokens")).unwrap_or(0),
    })
}

fn number(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64).or_else(|| {
        value
            .and_then(Value::as_i64)
            .filter(|value| *value >= 0)
            .map(|value| value as u64)
    })
}

pub fn receiver(lease: Option<InvocationLease>, argv: &[OsString]) -> HookHandler {
    let unavailable = reporter::preflight(
        lease.is_some(),
        argv,
        eligible_argv,
        reporter::interactive(),
    );
    let mut reporter = Reporter::new(AgentProvider::Hermes, lease, None);
    if let Some(reason) = unavailable {
        reporter.unavailable("Hermes", reason);
    }
    let executable = argv
        .first()
        .cloned()
        .unwrap_or_else(|| OsString::from("hermes"));
    reporter.handler(Events {
        executable,
        approvals: Vec::new(),
        usage: UsageNotice::default(),
        model: None,
    })
}

struct Events {
    executable: OsString,
    approvals: Vec<String>,
    usage: UsageNotice,
    model: Option<String>,
}

impl Events {
    fn ensure_bound(
        &mut self,
        reporter: &mut Reporter,
        notice: &Notice,
        deadline: Instant,
    ) -> bool {
        if reporter
            .binding()
            .is_some_and(|binding| binding.conversation == notice.session_id)
        {
            return true;
        }
        if reporter.binding().is_some() {
            return false;
        }
        let Some(state_db) = notice.state_db.as_deref() else {
            return false;
        };
        if !reporter.bind(&notice.session_id, deadline, false) {
            return false;
        }
        reporter.retain_conversation(
            ConversationReference::Hermes(HermesConversation {
                conversation: notice.session_id.clone(),
                executable: Path::new(&self.executable).to_path_buf(),
                state_db: state_db.into(),
            }),
            deadline,
        );
        true
    }

    fn activity(
        &self,
        reporter: &mut Reporter,
        state: AgentActivity,
        turn: Option<String>,
        deadline: Instant,
    ) -> Vec<u8> {
        reporter.publish(
            AgentObservation::Activity(ActivitySample {
                state,
                quality: SampleQuality::Observed,
                turn,
            }),
            deadline,
        )
    }

    fn publish_approvals(&self, reporter: &mut Reporter, deadline: Instant) -> Vec<u8> {
        let requests = self
            .approvals
            .iter()
            .map(|id| InputRequest {
                id: id.clone(),
                kind: InputKind::Approval,
            })
            .collect();
        reporter.publish(AgentObservation::Input(requests), deadline)
    }

    fn publish_usage(
        &mut self,
        reporter: &mut Reporter,
        notice: &Notice,
        deadline: Instant,
    ) -> Vec<u8> {
        let Some(usage) = &notice.usage else {
            return reporter::IGNORED.to_vec();
        };
        self.usage.input_tokens = self.usage.input_tokens.saturating_add(usage.input_tokens);
        self.usage.output_tokens = self.usage.output_tokens.saturating_add(usage.output_tokens);
        self.usage.cache_read_tokens = self
            .usage
            .cache_read_tokens
            .saturating_add(usage.cache_read_tokens);
        self.usage.cache_write_tokens = self
            .usage
            .cache_write_tokens
            .saturating_add(usage.cache_write_tokens);
        self.usage.reasoning_tokens = self
            .usage
            .reasoning_tokens
            .saturating_add(usage.reasoning_tokens);
        if let Some(model) = &notice.model {
            self.model = Some(model.clone());
        }
        let source = "hermes-hook".to_owned();
        reporter.publish(
            AgentObservation::Metrics(Box::new(MetricsSample {
                model: self.model.clone(),
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
                        input_tokens: Some(self.usage.input_tokens),
                        output_tokens: Some(self.usage.output_tokens),
                        cache_read_tokens: Some(self.usage.cache_read_tokens),
                        cache_write_tokens: Some(self.usage.cache_write_tokens),
                        reasoning_output_tokens: Some(self.usage.reasoning_tokens),
                    },
                    source: source.clone(),
                },
                cost: Measurement {
                    value: None,
                    source,
                },
            })),
            deadline,
        )
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
        if reporter.closed() || !native_root {
            return reporter::IGNORED.to_vec();
        }
        #[derive(Deserialize)]
        struct Envelope {
            provider: String,
            origin: String,
            payload: Notice,
        }
        let Ok(envelope) = serde_json::from_slice::<Envelope>(input) else {
            return reporter::IGNORED.to_vec();
        };
        if envelope.provider != "hermes" || envelope.origin != "hermes-hook" {
            return reporter::IGNORED.to_vec();
        }
        let notice = envelope.payload;
        if !ovrcr_runtime::hermes_recovery::valid_session_id(&notice.session_id)
            || !self.ensure_bound(reporter, &notice, deadline)
        {
            return reporter::IGNORED.to_vec();
        }
        match notice.event.as_str() {
            "pre_api_request" => self.activity(
                reporter,
                AgentActivity::Busy,
                notice.turn_id.clone(),
                deadline,
            ),
            "post_llm_call" => {
                let Some(turn) = notice.turn_id.clone() else {
                    return reporter::IGNORED.to_vec();
                };
                self.activity(reporter, AgentActivity::ResponseReady, Some(turn), deadline)
            }
            "on_session_end" if notice.completed && !notice.interrupted => {
                let turn = notice
                    .turn_id
                    .clone()
                    .unwrap_or_else(|| notice.session_id.clone());
                self.activity(reporter, AgentActivity::ResponseReady, Some(turn), deadline)
            }
            "on_session_end" => self.activity(
                reporter,
                AgentActivity::Unknown,
                notice.turn_id.clone(),
                deadline,
            ),
            "pre_approval_request" => {
                let Some(id) = notice.approval_id.clone() else {
                    return reporter::IGNORED.to_vec();
                };
                if self.approvals.iter().any(|open| open == &id)
                    || self.approvals.len() >= MAX_INPUT_REQUESTS
                {
                    return reporter::IGNORED.to_vec();
                }
                self.approvals.push(id);
                self.publish_approvals(reporter, deadline)
            }
            "post_approval_response" => {
                let Some(id) = notice.approval_id.clone() else {
                    return reporter::IGNORED.to_vec();
                };
                let before = self.approvals.len();
                self.approvals.retain(|open| open != &id);
                if self.approvals.len() == before {
                    return reporter::IGNORED.to_vec();
                }
                self.publish_approvals(reporter, deadline)
            }
            "post_api_request" => self.publish_usage(reporter, &notice, deadline),
            _ => reporter::IGNORED.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eligible_launch_is_interactive_chat_including_profile_and_resume() {
        assert!(eligible_argv(&["hermes".into()]));
        assert!(eligible_argv(&[
            "/bin/hermes".into(),
            "-p".into(),
            "researcher".into(),
            "chat".into()
        ]));
        assert!(eligible_argv(&[
            "hermes".into(),
            "--resume".into(),
            "20261006_101500_ab12cd".into()
        ]));
        assert!(!eligible_argv(&["hermes".into(), "doctor".into()]));
        assert!(!eligible_argv(&["codex".into()]));
    }

    #[test]
    fn hook_notice_drops_prompt_and_transcript_fields() {
        let raw = serde_json::json!({
            "hook_event_name": "post_llm_call",
            "session_id": "20261006_101500_ab12cd",
            "cwd": "/tmp/work",
            "extra": {
                "turn_id": "turn-1",
                "model": "grok-4.6",
                "user_message": "secret prompt",
                "assistant_response": "secret answer",
                "conversation_history": [{"role": "user", "content": "secret"}]
            }
        });
        let notice =
            notice_from_hook(&serde_json::to_vec(&raw).unwrap(), Some("/home/state.db")).unwrap();
        let encoded = serde_json::to_string(&notice).unwrap();
        assert!(!encoded.contains("secret"));
        assert_eq!(notice.event, "post_llm_call");
        assert_eq!(notice.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(notice.model.as_deref(), Some("grok-4.6"));
        assert_eq!(notice.state_db.as_deref(), Some("/home/state.db"));
    }

    #[test]
    fn usage_notice_rejects_raw_provider_payloads() {
        let raw = serde_json::json!({
            "hook_event_name": "post_api_request",
            "session_id": "20261006_101500_ab12cd",
            "extra": {"usage": {"input_tokens": 3, "output_tokens": 4, "raw_usage": {"key": "no"}}}
        });
        let notice = notice_from_hook(&serde_json::to_vec(&raw).unwrap(), None).unwrap();
        assert!(notice.usage.is_none());
    }
}

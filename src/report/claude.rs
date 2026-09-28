//! Claude hook identity and observed activity; no source ordering or settling claim.
use anyhow::{Result, bail};
use ovrcr_protocol::AgentActivity;
use serde_json::{Map, Value};

#[derive(Debug, PartialEq, Eq)]
pub struct ClaudeEvent {
    pub session: String,
    pub prompt: Option<String>,
    pub transcript_path: Option<String>,
    pub kind: ClaudeEventKind,
}
#[derive(Debug, PartialEq, Eq)]
pub enum ClaudeEventKind {
    SessionStart { source: String },
    SessionEnd { reason: Option<String> },
    Prompt,
    Tool,
    PermissionPrompt,
    Stop,
    ApiFailure { error: String },
}
impl ClaudeEventKind {
    pub fn activity(&self) -> Option<AgentActivity> {
        Some(match self {
            Self::Prompt | Self::Tool => AgentActivity::Busy,
            Self::PermissionPrompt => AgentActivity::WaitingInput,
            Self::Stop => AgentActivity::ResponseReady,
            Self::ApiFailure { .. } => AgentActivity::Error,
            _ => return None,
        })
    }
}
fn optional_string(input: &Map<String, Value>, key: &str) -> Result<Option<String>> {
    match input.get(key) {
        None => Ok(None),
        Some(Value::String(value)) if !value.is_empty() => Ok(Some(value.clone())),
        _ => bail!("invalid Claude hook field"),
    }
}
fn required_string(input: &Map<String, Value>, key: &str) -> Result<String> {
    optional_string(input, key)?.ok_or_else(|| anyhow::anyhow!("missing Claude hook field"))
}
pub fn parse_claude_hook(input: &[u8]) -> Result<Option<ClaudeEvent>> {
    if input.len() > super::HOOK_INPUT_LIMIT {
        bail!("hook input exceeds limit");
    }
    let value: Value = serde_json::from_slice(input)?;
    let input = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("invalid Claude hook"))?;
    let session = required_string(input, "session_id")?;
    ovrcr_protocol::validate_agent_id(&session)?;
    let event = required_string(input, "hook_event_name")?;
    if optional_string(input, "agent_id")?.is_some() || event.starts_with("Subagent") {
        return Ok(None);
    }
    let prompt = optional_string(input, "prompt_id")?;
    if let Some(prompt) = &prompt {
        ovrcr_protocol::validate_agent_id(prompt)?;
    }
    let transcript_path = optional_string(input, "transcript_path")?;
    let kind = match event.as_str() {
        "SessionStart" => ClaudeEventKind::SessionStart {
            source: required_string(input, "source")?,
        },
        "SessionEnd" => ClaudeEventKind::SessionEnd {
            reason: optional_string(input, "reason")?,
        },
        "UserPromptSubmit" => ClaudeEventKind::Prompt,
        "PreToolUse" | "PostToolUse" | "PostToolUseFailure" => ClaudeEventKind::Tool,
        "Notification"
            if optional_string(input, "notification_type")?.as_deref()
                == Some("permission_prompt") =>
        {
            ClaudeEventKind::PermissionPrompt
        }
        "Stop" => ClaudeEventKind::Stop,
        "StopFailure" => ClaudeEventKind::ApiFailure {
            error: required_string(input, "error")?,
        },
        _ => return Ok(None),
    };
    Ok(Some(ClaudeEvent {
        session,
        prompt,
        transcript_path,
        kind,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn claude_identity_is_typed_and_never_inferred_from_agent_type() {
        let root = br#"{"session_id":"root","prompt_id":"turn","transcript_path":"/exact/path","hook_event_name":"Stop","agent_type":"named-root"}"#;
        let event = parse_claude_hook(root).unwrap().unwrap();
        assert_eq!(event.session, "root");
        assert_eq!(event.prompt.as_deref(), Some("turn"));
        assert_eq!(event.transcript_path.as_deref(), Some("/exact/path"));
        for field in ["agent_id", "session_id", "prompt_id", "transcript_path"] {
            for bad in [Value::Null, Value::from(3), Value::from("")] {
                let mut payload: Value = serde_json::from_slice(root).unwrap();
                payload[field] = bad;
                assert!(
                    parse_claude_hook(&serde_json::to_vec(&payload).unwrap()).is_err(),
                    "{field}"
                );
            }
        }
        let mut child: Value = serde_json::from_slice(root).unwrap();
        child["agent_id"] = "child".into();
        assert!(
            parse_claude_hook(&serde_json::to_vec(&child).unwrap())
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn claude_protocol_ids_validate_before_receiver_mutation() {
        for field in ["session_id", "prompt_id"] {
            for invalid in ["bad\nidentity".to_owned(), "x".repeat(257)] {
                let mut value = serde_json::json!({"session_id":"root","prompt_id":"prompt","hook_event_name":"UserPromptSubmit"});
                value[field] = invalid.into();
                assert!(parse_claude_hook(&serde_json::to_vec(&value).unwrap()).is_err());
            }
        }
        let value = serde_json::json!({"session_id":"s".repeat(256),"prompt_id":"p".repeat(256),"transcript_path":"/".repeat(300),"hook_event_name":"UserPromptSubmit"});
        assert!(
            parse_claude_hook(&serde_json::to_vec(&value).unwrap())
                .unwrap()
                .is_some()
        );
    }
    #[test]
    fn claude_native_permission_notification_correlates_to_prompt() {
        let capture = include_str!(
            "../../research/claude-reporting-acceptance/attempt-10-sources/live-hooks-and-statusline.jsonl"
        );
        let mut prompt = None;
        let mut waiting = 0;
        for line in capture.lines() {
            let value: Value = serde_json::from_str(line).unwrap();
            if value["capture_kind"] != "hook" {
                continue;
            }
            let Some(event) = parse_claude_hook(line.as_bytes()).unwrap() else {
                continue;
            };
            if event.kind == ClaudeEventKind::Prompt {
                prompt = event.prompt.clone();
            }
            if event.kind == ClaudeEventKind::PermissionPrompt {
                assert!(prompt.is_some());
                assert_eq!(event.prompt, prompt);
                assert_eq!(event.kind.activity(), Some(AgentActivity::WaitingInput));
                waiting += 1;
            }
        }
        assert_eq!(waiting, 1);
    }
    #[test]
    fn claude_unknown_and_uncertified_waits_do_not_map() {
        for name in [
            "Unknown",
            "PermissionRequest",
            "Elicitation",
            "SubagentStop",
        ] {
            let payload = serde_json::json!({"session_id":"root","hook_event_name":name});
            assert!(
                parse_claude_hook(&serde_json::to_vec(&payload).unwrap())
                    .unwrap()
                    .is_none()
            );
        }
        for notification in ["idle_prompt", "auth_success", "other"] {
            let payload = serde_json::json!({"session_id":"root","hook_event_name":"Notification","notification_type":notification});
            assert!(
                parse_claude_hook(&serde_json::to_vec(&payload).unwrap())
                    .unwrap()
                    .is_none()
            );
        }
        assert!(
            parse_claude_hook(br#"{"session_id":"root","hook_event_name":"StopFailure"}"#).is_err()
        );
    }
    #[test]
    fn claude_stop_maps_to_observed_response_ready() {
        let stop = br#"{"session_id":"root","prompt_id":"turn","hook_event_name":"Stop"}"#;
        let event = parse_claude_hook(stop).unwrap().unwrap();
        assert_eq!(event.kind, ClaudeEventKind::Stop);
        assert_eq!(event.kind.activity(), Some(AgentActivity::ResponseReady));
        assert_eq!(event.prompt.as_deref(), Some("turn"));
    }
}

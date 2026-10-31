//! SDK-independent native Operator conversation and worker protocol.
use crate::{SessionId, SessionRunId, SupervisorAuth};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_OPERATOR_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_OPERATOR_HISTORY_BYTES: usize = 256 * 1024;
pub const MAX_OPERATOR_BATCH_EVENTS: usize = 32;
pub const MAX_OPERATOR_PAGE_MESSAGES: u16 = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorProvider {
    #[default]
    ChatGpt,
    Grok,
}
impl OperatorProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChatGpt => "chat_gpt",
            Self::Grok => "grok",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperatorConversationId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperatorTurnId(pub u64);

/// Stable submission context. Terminal contents are never captured implicitly.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorContext {
    pub project: Option<String>,
    pub workspace: Option<String>,
    /// Registry WorkspaceRecord.id, captured separately from its display label.
    pub workspace_id: Option<String>,
    pub cwd: Option<PathBuf>,
    pub session: Option<SessionId>,
    pub run: Option<SessionRunId>,
}
impl OperatorContext {
    pub fn validate(&self) -> Result<(), String> {
        if self.session.is_some() != self.run.is_some()
            || self.session.is_some_and(|id| id.0 == 0)
            || self.run.is_some_and(|id| id.0 == 0)
        {
            return Err("invalid captured session/run".into());
        }
        for value in [&self.project, &self.workspace, &self.workspace_id]
            .into_iter()
            .flatten()
        {
            if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
                return Err("invalid captured context name".into());
            }
        }
        if self
            .cwd
            .as_ref()
            .is_some_and(|path| !path.is_absolute() || path.as_os_str().len() > 4096)
        {
            return Err("invalid captured directory".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorTurnIdentity {
    pub conversation: OperatorConversationId,
    pub turn: OperatorTurnId,
    pub session: SessionId,
    pub run: SessionRunId,
}
impl OperatorTurnIdentity {
    pub fn validate(&self) -> Result<(), String> {
        if [self.conversation.0, self.turn.0, self.session.0, self.run.0].contains(&0) {
            Err("zero operator identity".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorTurnState {
    Running,
    Complete,
    Interrupted,
    Error,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorTurn {
    pub identity: OperatorTurnIdentity,
    pub state: OperatorTurnState,
    pub sequence: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorConversationSummary {
    /// Current retained authority; the latest turn can belong to an older run.
    pub run: SessionRunId,
    pub archived: bool,
    /// Derived only from the retained exact-run controlled-stop proof.
    pub worker_stopped: bool,
    pub id: OperatorConversationId,
    pub session: SessionId,
    pub provider: OperatorProvider,
    pub model: String,
    pub title: String,
    pub current_turn: Option<OperatorTurn>,
}
/// Version 1 contains the SDK's JSON message array; interpreted only by the worker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorModelHistory {
    pub version: u16,
    pub json: String,
}
impl OperatorModelHistory {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.json.len() > MAX_OPERATOR_HISTORY_BYTES
            || !serde_json::from_str::<serde_json::Value>(&self.json).is_ok_and(|v| v.is_array())
        {
            return Err("invalid or unsupported operator model history".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorToolStatus {
    Succeeded,
    Failed { exit_code: Option<i32> },
    TimedOut,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorWorkerEvent {
    AssistantDelta(String),
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolOutput {
        id: String,
        output: String,
        truncated: bool,
        status: OperatorToolStatus,
    },
    Completed(OperatorModelHistory),
    Failed(String),
}
impl OperatorWorkerEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed(_) | Self::Failed(_))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorEventBatch {
    pub identity: OperatorTurnIdentity,
    pub sequence: u64,
    pub events: Vec<OperatorWorkerEvent>,
}
impl OperatorEventBatch {
    pub fn validate(&self) -> Result<(), String> {
        self.identity.validate()?;
        if self.sequence == 0
            || self.events.is_empty()
            || self.events.len() > MAX_OPERATOR_BATCH_EVENTS
        {
            return Err("invalid operator batch bounds".into());
        }
        let mut bytes = 0usize;
        for (index, event) in self.events.iter().enumerate() {
            if event.is_terminal() && index + 1 != self.events.len() {
                return Err("terminal operator event must be last".into());
            }
            match event {
                OperatorWorkerEvent::Completed(history) => history.validate()?,
                OperatorWorkerEvent::AssistantDelta(text) | OperatorWorkerEvent::Failed(text) => {
                    validate_text(text)?;
                    bytes += text.len();
                }
                OperatorWorkerEvent::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    validate_token(id)?;
                    validate_token(name)?;
                    validate_text(arguments)?;
                    bytes += arguments.len();
                }
                OperatorWorkerEvent::ToolOutput {
                    id, output, status, ..
                } => {
                    validate_token(id)?;
                    if output.len() > MAX_OPERATOR_TEXT_BYTES
                        || output.contains('\0')
                        || matches!(status, OperatorToolStatus::Failed { exit_code: Some(0) })
                    {
                        return Err("invalid operator tool output".into());
                    }
                    bytes += output.len();
                }
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| "invalid operator batch")?
            .len()
            > 512 * 1024
        {
            return Err("operator batch encoded capacity exceeded".into());
        }
        if bytes > MAX_OPERATOR_TEXT_BYTES {
            return Err("operator batch text capacity exceeded".into());
        }
        Ok(())
    }
}
fn validate_token(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        Err("invalid operator token".into())
    } else {
        Ok(())
    }
}
pub fn validate_operator_model(value: &str) -> Result<(), String> {
    validate_token(value)?;
    if value.trim() != value || value.trim().is_empty() {
        Err("invalid operator model".into())
    } else {
        Ok(())
    }
}
fn validate_text(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_OPERATOR_TEXT_BYTES || value.contains('\0') {
        Err("invalid operator text bounds".into())
    } else {
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorMessageContent {
    User {
        text: String,
        context: OperatorContext,
    },
    Worker(OperatorWorkerEvent),
    Interrupted,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorMessage {
    pub ordinal: u64,
    pub turn: OperatorTurnId,
    pub content: OperatorMessageContent,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorHistory {
    pub conversation: OperatorConversationSummary,
    pub messages: Vec<OperatorMessage>,
    pub next_after: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorRequest {
    Create {
        provider: OperatorProvider,
        model: String,
    },
    List {
        after: u64,
        limit: u16,
    },
    Read {
        conversation: OperatorConversationId,
        after: u64,
        limit: u16,
    },
    Submit {
        conversation: OperatorConversationId,
        expected_run: SessionRunId,
        text: String,
        context: OperatorContext,
    },
    Stop {
        identity: OperatorTurnIdentity,
    },
    Close {
        conversation: OperatorConversationId,
        expected_run: SessionRunId,
    },
}
impl OperatorRequest {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Create { model, .. } => validate_operator_model(model),
            Self::List { limit, .. } | Self::Read { limit, .. }
                if *limit == 0 || *limit > MAX_OPERATOR_PAGE_MESSAGES =>
            {
                Err("invalid operator page limit".into())
            }
            Self::Read { conversation, .. } | Self::Close { conversation, .. }
                if conversation.0 == 0 =>
            {
                Err("zero conversation identity".into())
            }
            Self::Submit {
                conversation,
                text,
                context,
                ..
            } => {
                if conversation.0 == 0 {
                    return Err("zero conversation identity".into());
                }
                validate_text(text)?;
                if text.trim().is_empty() {
                    return Err("empty operator message".into());
                }
                context.validate()
            }
            Self::Stop { identity } => identity.validate(),
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorResponse {
    Created(OperatorConversationSummary),
    Conversations(Vec<OperatorConversationSummary>),
    History(OperatorHistory),
    Submitted(OperatorTurn),
    Stopped(OperatorTurn),
    Closed(OperatorConversationSummary),
}
/// Auth must be verified against the current owned worker by the Server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorExchange {
    pub auth: SupervisorAuth,
    pub batch: OperatorEventBatch,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorExchangeResult {
    pub identity: OperatorTurnIdentity,
    pub sequence: u64,
    pub duplicate: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OperatorSettings {
    pub provider: OperatorProvider,
    pub chat_gpt_model: Option<String>,
    pub grok_model: Option<String>,
}
impl Default for OperatorSettings {
    fn default() -> Self {
        Self {
            provider: OperatorProvider::ChatGpt,
            chat_gpt_model: None,
            grok_model: None,
        }
    }
}

/// Immutable bootstrap data read only by the authenticated current worker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorWorkerStart {
    pub identity: OperatorTurnIdentity,
    pub provider: OperatorProvider,
    pub model: String,
    pub text: String,
    pub context: OperatorContext,
    pub history: Option<OperatorModelHistory>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorWorkerCommand {
    Start {
        identity: OperatorTurnIdentity,
    },
    Retire {
        identity: OperatorTurnIdentity,
    },
    /// Authenticated timeout cancellation uses the same exact-turn Stop path.
    Stop {
        identity: OperatorTurnIdentity,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorWorkerRequest {
    pub auth: SupervisorAuth,
    pub command: OperatorWorkerCommand,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorWorkerResult {
    Start(Box<OperatorWorkerStart>),
    Retired { identity: OperatorTurnIdentity },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn close_validates_conversation_and_accepts_never_run_zero() {
        assert!(
            OperatorRequest::Close {
                conversation: OperatorConversationId(1),
                expected_run: SessionRunId(0)
            }
            .validate()
            .is_ok()
        );
        assert!(
            OperatorRequest::Close {
                conversation: OperatorConversationId(0),
                expected_run: SessionRunId(0)
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn operator_validation_rejects_zero_ids_gaps_oversize_and_terminal_followups() {
        let mut batch = OperatorEventBatch {
            identity: OperatorTurnIdentity {
                conversation: OperatorConversationId(1),
                turn: OperatorTurnId(1),
                session: SessionId(1),
                run: SessionRunId(1),
            },
            sequence: 1,
            events: vec![OperatorWorkerEvent::AssistantDelta("hello".into())],
        };
        assert!(batch.validate().is_ok());
        batch.sequence = 0;
        assert!(batch.validate().is_err());
        batch.sequence = 1;
        batch.events = vec![OperatorWorkerEvent::AssistantDelta(
            "x".repeat(MAX_OPERATOR_TEXT_BYTES + 1),
        )];
        assert!(batch.validate().is_err());
        batch.events = vec![
            OperatorWorkerEvent::Failed("failed".into()),
            OperatorWorkerEvent::AssistantDelta("late".into()),
        ];
        assert!(batch.validate().is_err());
        batch.events = vec![OperatorWorkerEvent::Completed(OperatorModelHistory {
            version: 2,
            json: "[]".into(),
        })];
        assert!(batch.validate().is_err());
        batch.events = vec![OperatorWorkerEvent::Completed(OperatorModelHistory {
            version: 1,
            json: "not json".into(),
        })];
        assert!(batch.validate().is_err());
    }
    #[test]
    fn empty_tool_output_has_explicit_outcome_without_relaxing_other_text() {
        let mut batch = OperatorEventBatch {
            identity: OperatorTurnIdentity {
                conversation: OperatorConversationId(1),
                turn: OperatorTurnId(1),
                session: SessionId(1),
                run: SessionRunId(1),
            },
            sequence: 1,
            events: vec![OperatorWorkerEvent::ToolOutput {
                id: "call".into(),
                output: String::new(),
                truncated: false,
                status: OperatorToolStatus::Succeeded,
            }],
        };
        assert!(batch.validate().is_ok());
        batch.events = vec![OperatorWorkerEvent::ToolOutput {
            id: "call".into(),
            output: "x".repeat(MAX_OPERATOR_TEXT_BYTES),
            truncated: true,
            status: OperatorToolStatus::Succeeded,
        }];
        assert!(
            batch.validate().is_ok(),
            "the full retained Bash output budget must fit"
        );
        batch.events = vec![OperatorWorkerEvent::AssistantDelta(String::new())];
        assert!(batch.validate().is_err());
        batch.events = vec![OperatorWorkerEvent::ToolOutput {
            id: "call".into(),
            output: String::new(),
            truncated: false,
            status: OperatorToolStatus::Failed { exit_code: Some(0) },
        }];
        assert!(batch.validate().is_err());
    }
    #[test]
    fn operator_requests_and_worker_batches_round_trip_framed() {
        let request = crate::Request::Operator(OperatorRequest::Create {
            provider: OperatorProvider::ChatGpt,
            model: "model".into(),
        });
        let mut bytes = Vec::new();
        crate::write_frame(&mut bytes, &request).unwrap();
        let decoded: crate::Request =
            crate::read_frame::<crate::Request>(&mut bytes.as_slice()).unwrap();
        assert_eq!(request, decoded);
        assert!(
            OperatorRequest::Read {
                conversation: OperatorConversationId(1),
                after: 0,
                limit: 0
            }
            .validate()
            .is_err()
        );
        assert!(
            OperatorRequest::Create {
                provider: OperatorProvider::Grok,
                model: " ".into()
            }
            .validate()
            .is_err()
        );
    }
}

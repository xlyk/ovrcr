//! SDK-independent native Operator conversation and worker protocol.
use crate::{SessionId, SessionRunId, SupervisorAuth};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_OPERATOR_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_OPERATOR_BATCH_EVENTS: usize = 32;
pub const MAX_OPERATOR_PAGE_MESSAGES: u16 = 100;
pub const MAX_OPERATOR_MODEL_CATALOG_ENTRIES: usize = 4096;

/// `Default` leaves Pi's model-specific choice alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperatorThinkingLevel {
    #[default]
    Default,
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}
impl OperatorThinkingLevel {
    pub const ALL: [Self; 8] = [
        Self::Default,
        Self::Off,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Xhigh,
        Self::Max,
    ];
    pub const NAMES: [&'static str; 8] = [
        "default", "off", "minimal", "low", "medium", "high", "xhigh", "max",
    ];

    pub fn as_str(self) -> &'static str {
        Self::NAMES[self as usize]
    }
}

/// Pi's readback for the selected model, distinct from the saved request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorReasoning {
    pub requested: OperatorThinkingLevel,
    pub level: OperatorThinkingLevel,
    pub available: Vec<OperatorThinkingLevel>,
}
impl OperatorReasoning {
    pub fn validate(&self) -> Result<(), String> {
        if self.level == OperatorThinkingLevel::Default
            || self.available.is_empty()
            || self.available.len() > 7
            || self.available.contains(&OperatorThinkingLevel::Default)
            || !self.available.contains(&self.level)
            || self
                .available
                .iter()
                .enumerate()
                .any(|(i, level)| self.available[..i].contains(level))
        {
            return Err("invalid operator reasoning state".into());
        }
        Ok(())
    }
}

/// One exact Pi model identity: its provider and its model id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorModel {
    pub provider: String,
    pub id: String,
}
impl OperatorModel {
    /// Splits `provider/id` at the first slash; the id may contain more slashes.
    pub fn parse(selection: &str) -> Result<Self, String> {
        let (provider, id) = selection
            .split_once('/')
            .ok_or_else(|| "invalid operator model".to_string())?;
        let model = Self {
            provider: provider.into(),
            id: id.into(),
        };
        model.validate()?;
        Ok(model)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_token(&self.provider)?;
        validate_token(&self.id)?;
        if self.provider.trim() != self.provider
            || self.id.trim() != self.id
            || self.provider.contains('/')
        {
            return Err("invalid operator model".into());
        }
        Ok(())
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

/// Stable identity for one live Operator worker, shared across its turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperatorRunIdentity {
    pub conversation: OperatorConversationId,
    pub session: SessionId,
    pub run: SessionRunId,
}
impl OperatorRunIdentity {
    pub fn validate(&self) -> Result<(), String> {
        if [self.conversation.0, self.session.0, self.run.0].contains(&0) {
            Err("zero operator run identity".into())
        } else {
            Ok(())
        }
    }

    pub fn matches_turn(&self, turn: &OperatorTurnIdentity) -> bool {
        self.conversation == turn.conversation
            && self.session == turn.session
            && self.run == turn.run
    }
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
    /// Derived only from the retained exact-run controlled-stop proof.
    pub worker_stopped: bool,
    /// True only while the exact retained worker run is live in this Server.
    pub worker_live: bool,
    pub id: OperatorConversationId,
    pub session: SessionId,
    /// Monotonic `/new` epoch on the singleton Operator conversation (`0` initially).
    pub session_epoch: u64,
    /// The last pi model the worker selected; `None` before the first selection.
    pub model: Option<OperatorModel>,
    pub title: String,
    pub current_turn: Option<OperatorTurn>,
    pub reasoning: Option<OperatorReasoning>,
    /// Start of the current turn when the store recorded it; older turns may be unknown.
    pub current_turn_started_unix_ms: Option<u64>,
    /// Latest completed reply time; legacy turns without a stored time are unknown.
    pub last_reply_unix_ms: Option<u64>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorTokenStats {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorContextWindowUsage {
    pub used_tokens: Option<u64>,
    pub capacity_tokens: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorSessionInfo {
    pub conversation: OperatorConversationId,
    pub session: SessionId,
    pub run: SessionRunId,
    pub session_epoch: u64,
    pub pi_session_id: Option<String>,
    pub checkpoint_turn: Option<OperatorTurnId>,
    pub configured_model: Option<String>,
    pub active_model: Option<OperatorModel>,
    pub configured_thinking_level: OperatorThinkingLevel,
    pub reasoning: Option<OperatorReasoning>,
    pub turns: u64,
    pub completed_turns: u64,
    pub user_messages: u64,
    pub assistant_messages: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub total_messages: u64,
    pub tokens: OperatorTokenStats,
    /// Cumulative cost in nano-USD (`1_000_000_000 == $1.00`).
    pub cost_usd_nanos: u64,
    pub context_window: OperatorContextWindowUsage,
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
    ModelSelected(OperatorModel),
    Completed {
        leased: bool,
    },
    Failed(String),
    /// Displayable incremental thinking text; provider signatures are never carried.
    ThinkingDelta(String),
    /// Native pi automatic compaction began in this turn. Durable phase state
    /// that drives transient UI and never renders as a permanent journal row.
    CompactionStarted,
    /// Native pi automatic compaction finished successfully. Failure and Stop
    /// remain represented by the turn's terminal event/state, never by an end.
    CompactionEnded,
    ReasoningSelected(OperatorReasoning),
    /// Durable receipt emitted when manual `/compact` finishes summarizing prior context.
    CompactionSummary {
        summary: String,
        tokens_before: u64,
    },
}
impl OperatorWorkerEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed(_))
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
                OperatorWorkerEvent::ReasoningSelected(reasoning) => reasoning.validate()?,
                OperatorWorkerEvent::ModelSelected(model) => model.validate()?,
                OperatorWorkerEvent::Completed { .. }
                | OperatorWorkerEvent::CompactionStarted
                | OperatorWorkerEvent::CompactionEnded => {}
                OperatorWorkerEvent::CompactionSummary { summary, .. } => {
                    if summary.len() > MAX_OPERATOR_TEXT_BYTES || summary.contains('\0') {
                        return Err("invalid operator compaction summary".into());
                    }
                    bytes += summary.len();
                }
                OperatorWorkerEvent::AssistantDelta(text)
                | OperatorWorkerEvent::Failed(text)
                | OperatorWorkerEvent::ThinkingDelta(text) => {
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
    if crate::DelegationSettings::valid_model(value) {
        Ok(())
    } else {
        Err("invalid operator model".into())
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
    /// A queued watched-session report admitted while the leased Pi worker was idle.
    AsyncWakeup {
        text: String,
    },
    /// User-initiated `/compact` turn opener with optional summarization instructions.
    Compaction {
        instructions: Option<String>,
    },
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
/// One model entry discovered from the dedicated Operator Pi profile catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorModelCatalogEntry {
    pub provider: String,
    pub id: String,
    pub name: Option<String>,
}
impl OperatorModelCatalogEntry {
    pub fn full_id(&self) -> String {
        format!("{}/{}", self.provider, self.id)
    }

    pub fn validate(&self) -> Result<(), String> {
        OperatorModel {
            provider: self.provider.clone(),
            id: self.id.clone(),
        }
        .validate()?;
        if let Some(name) = &self.name {
            validate_token(name)?;
        }
        Ok(())
    }
}

pub fn validate_operator_model_catalog(
    entries: &[OperatorModelCatalogEntry],
) -> Result<(), String> {
    if entries.len() > MAX_OPERATOR_MODEL_CATALOG_ENTRIES {
        return Err("operator model catalog bounds exceeded".into());
    }
    for entry in entries {
        entry.validate()?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorRequest {
    /// Reads the sole conversation; never creates a record or a process.
    Current,
    /// Atomically gets or creates the sole conversation; never starts pi.
    Open,
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
    /// Lists models available in the dedicated Operator Pi profile without starting a turn.
    Models,
    /// Atomically resets the singleton conversation onto a new session epoch when idle and unleased.
    NewSession {
        conversation: OperatorConversationId,
        expected_run: SessionRunId,
    },
    /// Reads current-epoch session metadata and promoted checkpoint stats without spawning pi.
    SessionInfo {
        conversation: OperatorConversationId,
    },
    /// Compacts the current-epoch Operator conversation context via Pi's native `compact` RPC.
    Compact {
        conversation: OperatorConversationId,
        expected_run: SessionRunId,
        instructions: Option<String>,
    },
}
impl OperatorRequest {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Read { limit, .. } if *limit == 0 || *limit > MAX_OPERATOR_PAGE_MESSAGES => {
                Err("invalid operator page limit".into())
            }
            Self::Read { conversation, .. }
            | Self::NewSession { conversation, .. }
            | Self::SessionInfo { conversation }
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
            Self::Compact {
                conversation,
                instructions,
                ..
            } => {
                if conversation.0 == 0 {
                    return Err("zero conversation identity".into());
                }
                if let Some(instructions) = instructions {
                    validate_text(instructions)?;
                    if instructions.trim().is_empty() {
                        return Err("empty compaction instructions".into());
                    }
                }
                Ok(())
            }
            Self::Stop { identity } => identity.validate(),
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorResponse {
    Current(Option<OperatorConversationSummary>),
    Opened(OperatorConversationSummary),
    History(OperatorHistory),
    Submitted(OperatorTurn),
    Stopped(OperatorTurn),
    Models(Vec<OperatorModelCatalogEntry>),
    Conversation(OperatorConversationSummary),
    SessionInfo(Box<OperatorSessionInfo>),
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
    /// `provider/model`; unset means the dedicated Pi profile's current default.
    pub model: Option<String>,
    pub thinking_level: OperatorThinkingLevel,
    /// Whether the Dashboard shows the Operator section in its sidebar.
    pub sidebar: bool,
}

impl Default for OperatorSettings {
    fn default() -> Self {
        Self {
            model: None,
            thinking_level: OperatorThinkingLevel::default(),
            sidebar: true,
        }
    }
}

/// Worker turn payload generalized across prompt, manual compaction, and future branch summary turns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorWorkerSubmission {
    Prompt {
        text: String,
        context: OperatorContext,
        /// Pi has already started this prompt through its native async input API.
        async_wakeup: bool,
    },
    Compact {
        instructions: Option<String>,
    },
}

/// Immutable bootstrap data read only by the authenticated current worker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorWorkerStart {
    pub identity: OperatorTurnIdentity,
    /// The `operator.model` override captured when the turn was durably accepted.
    pub model: Option<String>,
    pub submission: OperatorWorkerSubmission,
    /// The last completed turn whose native session is this turn's context base.
    pub checkpoint_turn: Option<OperatorTurnId>,
    pub thinking_level: OperatorThinkingLevel,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorWorkerCommand {
    Start {
        identity: OperatorRunIdentity,
    },
    /// Claims the next submitted user turn for an already leased worker run.
    StartTurn {
        identity: OperatorRunIdentity,
    },
    /// Confirms that native rollback settled before a foreground Stop is published.
    AbortTurn {
        identity: OperatorTurnIdentity,
        leased: bool,
    },
    Retire {
        identity: OperatorTurnIdentity,
    },
    /// Authenticated timeout cancellation uses the same exact-turn Stop path.
    Stop {
        identity: OperatorTurnIdentity,
    },
    /// Atomically claims a settled watch report as a new turn on the leased run.
    BeginAsyncTurn {
        identity: OperatorRunIdentity,
        text: String,
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
    StartTurn(Option<Box<OperatorWorkerStart>>),
    Aborted { identity: OperatorTurnIdentity },
    Retired { identity: OperatorTurnIdentity },
    BeginAsyncTurn(Option<Box<OperatorWorkerStart>>),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stored_failed_worker_rows_keep_their_historical_json_shape() {
        let raw = r#"{"Worker":{"Failed":"Pi request failed; inspect the partial journal before retrying."}}"#;
        let decoded: OperatorMessageContent = serde_json::from_str(raw).unwrap();
        assert_eq!(
            decoded,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(
                "Pi request failed; inspect the partial journal before retrying.".into()
            ))
        );
        assert_eq!(serde_json::to_string(&decoded).unwrap(), raw);
    }

    #[test]
    fn read_validates_conversation_and_current_open_need_no_identity() {
        assert!(
            OperatorRequest::Read {
                conversation: OperatorConversationId(1),
                after: 0,
                limit: 100
            }
            .validate()
            .is_ok()
        );
        assert!(
            OperatorRequest::Read {
                conversation: OperatorConversationId(0),
                after: 0,
                limit: 100
            }
            .validate()
            .is_err()
        );
        assert!(OperatorRequest::Current.validate().is_ok());
        assert!(OperatorRequest::Open.validate().is_ok());
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
        batch.events = vec![OperatorWorkerEvent::Completed { leased: false }];
        assert!(batch.validate().is_ok());
        batch.events = vec![
            OperatorWorkerEvent::Failed("failed".into()),
            OperatorWorkerEvent::AssistantDelta("late".into()),
        ];
        assert!(batch.validate().is_err());
        batch.events = vec![
            OperatorWorkerEvent::Completed { leased: false },
            OperatorWorkerEvent::AssistantDelta("late".into()),
        ];
        assert!(batch.validate().is_err());
        batch.events = vec![OperatorWorkerEvent::ModelSelected(OperatorModel {
            provider: String::new(),
            id: "id".into(),
        })];
        assert!(batch.validate().is_err());
        batch.events = vec![OperatorWorkerEvent::ModelSelected(OperatorModel {
            provider: "openai".into(),
            id: "gpt".into(),
        })];
        assert!(batch.validate().is_ok());
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
        batch.events = vec![OperatorWorkerEvent::ThinkingDelta("thinking".into())];
        assert!(batch.validate().is_ok());
        batch.events = vec![OperatorWorkerEvent::ThinkingDelta(String::new())];
        assert!(batch.validate().is_err());
        batch.events = vec![OperatorWorkerEvent::ThinkingDelta(
            "x".repeat(MAX_OPERATOR_TEXT_BYTES + 1),
        )];
        assert!(batch.validate().is_err());
        batch.events = vec![
            OperatorWorkerEvent::CompactionStarted,
            OperatorWorkerEvent::CompactionEnded,
            OperatorWorkerEvent::Completed { leased: false },
        ];
        assert!(batch.validate().is_ok());
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
        let request = crate::Request::Operator(OperatorRequest::Open);
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
        assert_eq!(
            OperatorModel::parse("openai/gpt-5.1-codex/mini").unwrap(),
            OperatorModel {
                provider: "openai".into(),
                id: "gpt-5.1-codex/mini".into()
            }
        );
        assert!(
            OperatorModel::parse("openai").is_err(),
            "a bare model id has no provider"
        );
        assert!(OperatorModel::parse("/id").is_err());
        assert!(OperatorModel::parse("openai/").is_err());
        assert!(OperatorModel::parse(" openai/id").is_err());
        assert!(validate_operator_model("openai/id/with/slashes").is_ok());
        assert!(validate_operator_model(" ").is_err());
        assert!(validate_operator_model("openai").is_err());
    }

    #[test]
    fn new_session_and_session_info_validate_and_round_trip_framed() {
        assert!(
            OperatorRequest::NewSession {
                conversation: OperatorConversationId(0),
                expected_run: SessionRunId(0),
            }
            .validate()
            .is_err()
        );
        assert!(
            OperatorRequest::NewSession {
                conversation: OperatorConversationId(1),
                expected_run: SessionRunId(0),
            }
            .validate()
            .is_ok()
        );
        assert!(
            OperatorRequest::SessionInfo {
                conversation: OperatorConversationId(0),
            }
            .validate()
            .is_err()
        );
        assert!(
            OperatorRequest::SessionInfo {
                conversation: OperatorConversationId(1),
            }
            .validate()
            .is_ok()
        );

        let summary = OperatorConversationSummary {
            run: SessionRunId(2),
            worker_stopped: true,
            worker_live: false,
            id: OperatorConversationId(1),
            session: SessionId(3),
            session_epoch: 4,
            model: Some(OperatorModel {
                provider: "openai".into(),
                id: "gpt-5.4".into(),
            }),
            title: "Operator".into(),
            current_turn: None,
            reasoning: None,
            current_turn_started_unix_ms: None,
            last_reply_unix_ms: None,
        };
        let info = OperatorSessionInfo {
            conversation: OperatorConversationId(1),
            session: SessionId(3),
            run: SessionRunId(2),
            session_epoch: 4,
            pi_session_id: Some("operator-1".into()),
            checkpoint_turn: Some(OperatorTurnId(9)),
            configured_model: Some("openai/gpt-5.4".into()),
            active_model: Some(OperatorModel {
                provider: "openai".into(),
                id: "gpt-5.4".into(),
            }),
            configured_thinking_level: OperatorThinkingLevel::High,
            reasoning: Some(OperatorReasoning {
                requested: OperatorThinkingLevel::High,
                level: OperatorThinkingLevel::High,
                available: vec![OperatorThinkingLevel::Low, OperatorThinkingLevel::High],
            }),
            turns: 2,
            completed_turns: 2,
            user_messages: 2,
            assistant_messages: 2,
            tool_calls: 1,
            tool_results: 1,
            total_messages: 5,
            tokens: OperatorTokenStats {
                input: 1200,
                output: 300,
                cache_read: 400,
                cache_write: 100,
                total: 2000,
            },
            cost_usd_nanos: 12_500_000,
            context_window: OperatorContextWindowUsage {
                used_tokens: Some(1800),
                capacity_tokens: Some(200_000),
            },
        };
        for response in [
            crate::Response::Operator(OperatorResponse::Conversation(summary)),
            crate::Response::Operator(OperatorResponse::SessionInfo(Box::new(info))),
        ] {
            let mut bytes = Vec::new();
            crate::write_frame(&mut bytes, &response).unwrap();
            let decoded: crate::Response =
                crate::read_frame::<crate::Response>(&mut bytes.as_slice()).unwrap();
            assert_eq!(response, decoded);
        }
    }

    #[test]
    fn operator_model_catalog_validates_entries_and_round_trips_framed() {
        assert!(OperatorRequest::Models.validate().is_ok());
        let models_request = crate::Request::Operator(OperatorRequest::Models);
        let mut bytes = Vec::new();
        crate::write_frame(&mut bytes, &models_request).unwrap();
        let decoded_request: crate::Request =
            crate::read_frame::<crate::Request>(&mut bytes.as_slice()).unwrap();
        assert_eq!(models_request, decoded_request);

        let entries = vec![
            OperatorModelCatalogEntry {
                provider: "openai".into(),
                id: "gpt-5.4".into(),
                name: Some("GPT-5.4".into()),
            },
            OperatorModelCatalogEntry {
                provider: "vendor".into(),
                id: "nested/model-id".into(),
                name: None,
            },
        ];
        assert_eq!(entries[0].full_id(), "openai/gpt-5.4");
        assert_eq!(entries[1].full_id(), "vendor/nested/model-id");
        assert!(validate_operator_model_catalog(&entries).is_ok());

        let models_response = crate::Response::Operator(OperatorResponse::Models(entries));
        let mut response_bytes = Vec::new();
        crate::write_frame(&mut response_bytes, &models_response).unwrap();
        let decoded_response: crate::Response =
            crate::read_frame::<crate::Response>(&mut response_bytes.as_slice()).unwrap();
        assert_eq!(models_response, decoded_response);

        for invalid in [
            OperatorModelCatalogEntry {
                provider: String::new(),
                id: "gpt-5".into(),
                name: None,
            },
            OperatorModelCatalogEntry {
                provider: "open/ai".into(),
                id: "gpt-5".into(),
                name: None,
            },
            OperatorModelCatalogEntry {
                provider: "openai".into(),
                id: " gpt-5".into(),
                name: None,
            },
            OperatorModelCatalogEntry {
                provider: "openai".into(),
                id: "gpt-5".into(),
                name: Some(String::new()),
            },
            OperatorModelCatalogEntry {
                provider: "openai".into(),
                id: "gpt-5".into(),
                name: Some("bad\nname".into()),
            },
        ] {
            assert!(invalid.validate().is_err(), "{invalid:?}");
        }

        let oversized = vec![
            OperatorModelCatalogEntry {
                provider: "openai".into(),
                id: "gpt-5".into(),
                name: None,
            };
            MAX_OPERATOR_MODEL_CATALOG_ENTRIES + 1
        ];
        assert!(validate_operator_model_catalog(&oversized).is_err());
    }

    #[test]
    fn compact_request_worker_submission_and_compaction_summary_validate_and_round_trip() {
        assert!(
            OperatorRequest::Compact {
                conversation: OperatorConversationId(0),
                expected_run: SessionRunId(1),
                instructions: None,
            }
            .validate()
            .is_err()
        );
        for invalid in [
            Some(String::new()),
            Some("   ".into()),
            Some("bad\0text".into()),
            Some("x".repeat(MAX_OPERATOR_TEXT_BYTES + 1)),
        ] {
            assert!(
                OperatorRequest::Compact {
                    conversation: OperatorConversationId(1),
                    expected_run: SessionRunId(1),
                    instructions: invalid,
                }
                .validate()
                .is_err()
            );
        }
        for valid in [None, Some("preserve branches and open PR numbers".into())] {
            let request = crate::Request::Operator(OperatorRequest::Compact {
                conversation: OperatorConversationId(1),
                expected_run: SessionRunId(2),
                instructions: valid.clone(),
            });
            if let crate::Request::Operator(inner) = &request {
                assert!(inner.validate().is_ok());
            }
            let mut bytes = Vec::new();
            crate::write_frame(&mut bytes, &request).unwrap();
            let decoded: crate::Request =
                crate::read_frame::<crate::Request>(&mut bytes.as_slice()).unwrap();
            assert_eq!(request, decoded);

            let message = OperatorMessageContent::Compaction {
                instructions: valid.clone(),
            };
            let json = serde_json::to_string(&message).unwrap();
            assert_eq!(
                serde_json::from_str::<OperatorMessageContent>(&json).unwrap(),
                message
            );

            let start = OperatorWorkerResult::Start(Box::new(OperatorWorkerStart {
                identity: OperatorTurnIdentity {
                    conversation: OperatorConversationId(1),
                    turn: OperatorTurnId(3),
                    session: SessionId(2),
                    run: SessionRunId(4),
                },
                model: Some("openai/gpt-5.4".into()),
                submission: OperatorWorkerSubmission::Compact {
                    instructions: valid,
                },
                checkpoint_turn: Some(OperatorTurnId(2)),
                thinking_level: OperatorThinkingLevel::Medium,
            }));
            let response = crate::Response::OperatorWorker(start);
            let mut resp_bytes = Vec::new();
            crate::write_frame(&mut resp_bytes, &response).unwrap();
            let decoded_resp: crate::Response =
                crate::read_frame::<crate::Response>(&mut resp_bytes.as_slice()).unwrap();
            assert_eq!(response, decoded_resp);
        }

        let identity = OperatorTurnIdentity {
            conversation: OperatorConversationId(1),
            turn: OperatorTurnId(2),
            session: SessionId(1),
            run: SessionRunId(1),
        };
        let mut batch = OperatorEventBatch {
            identity,
            sequence: 1,
            events: vec![
                OperatorWorkerEvent::CompactionStarted,
                OperatorWorkerEvent::CompactionEnded,
                OperatorWorkerEvent::CompactionSummary {
                    summary: "Kept active workspace decisions.".into(),
                    tokens_before: 42_500,
                },
                OperatorWorkerEvent::Completed { leased: false },
            ],
        };
        assert!(batch.validate().is_ok());
        batch.events[2] = OperatorWorkerEvent::CompactionSummary {
            summary: String::new(),
            tokens_before: 0,
        };
        assert!(batch.validate().is_ok());
        batch.events[2] = OperatorWorkerEvent::CompactionSummary {
            summary: "bad\0summary".into(),
            tokens_before: 10,
        };
        assert!(batch.validate().is_err());
        batch.events[2] = OperatorWorkerEvent::CompactionSummary {
            summary: "x".repeat(MAX_OPERATOR_TEXT_BYTES + 1),
            tokens_before: 10,
        };
        assert!(batch.validate().is_err());
    }
}

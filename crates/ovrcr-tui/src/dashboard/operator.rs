//! One durable pi conversation is protocol data, never a terminal parser or a
//! PTY input target. The shared sidebar row, the full view and the Quick
//! message overlay all address that same singleton.
mod feed;
mod markdown;
mod render;
mod skills;
mod slash;
mod tree;

use super::text_cursor::TextCursor;
use super::{Dashboard, DashboardAction, InputMode};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr_protocol::{
    ClientMessage, MAX_OPERATOR_TEXT_BYTES, OperatorBranchSummaryRequest, OperatorContext,
    OperatorConversationId as Id, OperatorConversationSummary as Summary, OperatorHistory,
    OperatorMessage, OperatorMessageContent, OperatorModelCatalogEntry, OperatorRequest,
    OperatorResponse, OperatorSessionInfo, OperatorThinkingLevel as ThinkingLevel,
    OperatorToolStatus, OperatorTreeEntryKind, OperatorTurnId, OperatorTurnIdentity,
    OperatorTurnState, OperatorWorkerEvent, Request, Response, SessionId, SessionRunId,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tree::{BranchSummaryChoice, MAX_TREE_SEARCH_BYTES, TreeModal, TreePicker, TreePickerStep};
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub(super) struct Hub {
    pub visible: bool,
    editing: bool,
    transcript_focus: bool,
    sidebar_open: bool,
    /// Preserve an unavailable-Pi status that was actually displayed before
    /// opening the singleton; logical loaded-chat state alone is not evidence
    /// that this host can launch Pi.
    sidebar_setup_unavailable: bool,
    chat: Option<Chat>,
    /// One Open or history read is in flight for the singleton.
    loading: bool,
    pending: HashMap<u64, Pending>,
    context: OperatorContext,
    error: Option<String>,
    quick: Option<QuickMessage>,
    quick_generation: u64,
    overlay: Option<OperatorOverlay>,
    pending_overlay: Option<PendingOperatorOverlay>,
    slash_selected: usize,
    slash_dismissed_for: Option<String>,
}
enum OperatorOverlay {
    Reasoning(ReasoningPicker),
    Model(ModelPicker),
    Skills(skills::SkillPicker),
    Session(SessionModal),
    Tree(TreeModal),
}
enum PendingOperatorOverlay {
    Skills {
        text: String,
        context: OperatorContext,
    },
    Reasoning,
    Model {
        picker: ModelPicker,
        notice: Option<String>,
    },
    Session,
    Tree,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OperatorOverlayKeymap {
    SkillPicker,
    ModelPicker,
    ReasoningPicker,
    SessionModal,
    TreeLoading,
    TreeBrowse,
    TreeConfirmSummarize,
    TreeCustomInstructions,
}
impl Hub {
    fn clear_overlays(&mut self) {
        self.overlay = None;
        self.pending_overlay = None;
    }
    fn set_overlay(&mut self, overlay: OperatorOverlay) {
        self.pending_overlay = None;
        self.overlay = Some(overlay);
    }
    fn reasoning_picker(&self) -> Option<&ReasoningPicker> {
        match &self.overlay {
            Some(OperatorOverlay::Reasoning(picker)) => Some(picker),
            _ => None,
        }
    }
    fn reasoning_picker_mut(&mut self) -> Option<&mut ReasoningPicker> {
        match &mut self.overlay {
            Some(OperatorOverlay::Reasoning(picker)) => Some(picker),
            _ => None,
        }
    }
    fn model_picker(&self) -> Option<&ModelPicker> {
        match &self.overlay {
            Some(OperatorOverlay::Model(picker)) => Some(picker),
            _ => None,
        }
    }
    fn model_picker_mut(&mut self) -> Option<&mut ModelPicker> {
        match &mut self.overlay {
            Some(OperatorOverlay::Model(picker)) => Some(picker),
            _ => None,
        }
    }
    fn session_modal(&self) -> Option<&SessionModal> {
        match &self.overlay {
            Some(OperatorOverlay::Session(modal)) => Some(modal),
            _ => None,
        }
    }
    fn tree_modal(&self) -> Option<&TreeModal> {
        match &self.overlay {
            Some(OperatorOverlay::Tree(modal)) => Some(modal),
            _ => None,
        }
    }
    fn tree_modal_mut(&mut self) -> Option<&mut TreeModal> {
        match &mut self.overlay {
            Some(OperatorOverlay::Tree(modal)) => Some(modal),
            _ => None,
        }
    }
    fn tree_picker_mut(&mut self) -> Option<&mut TreePicker> {
        match &mut self.overlay {
            Some(OperatorOverlay::Tree(TreeModal::Loaded(picker))) => Some(picker),
            _ => None,
        }
    }
}
enum SessionModal {
    Loading,
    Loaded(Box<OperatorSessionInfo>),
}
struct ReasoningPicker {
    choices: Vec<ThinkingLevel>,
    selected: usize,
}
const MAX_MODEL_FILTER_BYTES: usize = 256;
pub(super) const EMPTY_MODEL_CATALOG_NOTICE: &str =
    "No Pi models configured; run ovrcr operator setup.";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModelChoice {
    HarnessDefault,
    Catalog(usize),
}
pub(super) struct ModelPicker {
    pub(super) entries: Vec<OperatorModelCatalogEntry>,
    pub(super) query: String,
    pub(super) cursor: TextCursor,
    pub(super) selected: usize,
}
impl ModelPicker {
    fn new(
        entries: Vec<OperatorModelCatalogEntry>,
        initial_query: String,
        active_model: Option<&str>,
    ) -> Self {
        let mut query = String::new();
        let mut cursor = TextCursor::default();
        cursor.insert(&mut query, &initial_query);
        let mut picker = Self {
            entries,
            query,
            cursor,
            selected: 0,
        };
        picker.select_preferred(active_model);
        picker
    }
    pub(super) fn filtered_choices(&self) -> Vec<ModelChoice> {
        let needle = self.query.trim().to_lowercase();
        let mut choices = Vec::new();
        if needle.is_empty() || "harness default (pi profile default)".contains(&needle) {
            choices.push(ModelChoice::HarnessDefault);
        }
        for (idx, entry) in self.entries.iter().enumerate() {
            let id_match = entry.full_id().to_lowercase().contains(&needle);
            let name_match = entry
                .name
                .as_deref()
                .is_some_and(|name| name.to_lowercase().contains(&needle));
            if needle.is_empty() || id_match || name_match {
                choices.push(ModelChoice::Catalog(idx));
            }
        }
        choices
    }
    pub(super) fn choice_is_active(&self, choice: ModelChoice, active_model: Option<&str>) -> bool {
        match choice {
            ModelChoice::HarnessDefault => active_model.is_none(),
            ModelChoice::Catalog(idx) => {
                active_model.is_some_and(|active| active == self.entries[idx].full_id())
            }
        }
    }
    fn select_preferred(&mut self, active_model: Option<&str>) {
        let filtered = self.filtered_choices();
        self.selected = filtered
            .iter()
            .position(|choice| self.choice_is_active(*choice, active_model))
            .unwrap_or(0);
    }
    fn on_query_changed(&mut self, active_model: Option<&str>) {
        if self.query.trim().is_empty() {
            self.select_preferred(active_model);
        } else {
            self.selected = 0;
        }
    }
}
struct QuickMessage {
    generation: u64,
    visible: bool,
    text: String,
    cursor: TextCursor,
    context: OperatorContext,
    error: Option<String>,
    phase: QuickPhase,
    /// The overlay mirrors the singleton's draft once that conversation exists,
    /// so both entry points edit one draft instead of competing copies.
    seeded: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum QuickPhase {
    Editing,
    CheckingModels,
    Opening,
    Submitting,
}
struct QuickSubmission {
    generation: u64,
    text: String,
    context: OperatorContext,
}
/// Why an Open was sent: the full view, or the first explicit Quick Send / `/new` / `/compact`.
enum OpenOrigin {
    Manual,
    Quick(QuickSubmission),
    QuickNew {
        generation: u64,
    },
    QuickCompact {
        generation: u64,
        draft: String,
        instructions: Option<String>,
    },
}
#[derive(Clone, Copy)]
enum SubmitOrigin {
    Composer,
    Quick { generation: u64 },
}
#[derive(Clone, Debug)]
struct PendingBranchSummaryUi {
    turn: OperatorTurnId,
    editor_text: Option<String>,
    restored_context: Option<OperatorContext>,
}

struct Chat {
    summary: Summary,
    /// The fixed agent working directory; the captured target is per message.
    workdir: Option<std::path::PathBuf>,
    error: Option<String>,
    notice: Option<String>,
    draft: String,
    cursor: TextCursor,
    context: OperatorContext,
    messages: Vec<OperatorMessage>,
    ordinal: u64,
    confirmed_tail: Option<u64>,
    reading: bool,
    dirty: bool,
    submitting: bool,
    stopping: bool,
    scroll: usize,
    anchor: Option<(u64, usize, usize)>,
    /// Missing is automatic; `true` is explicitly expanded and `false`
    /// explicitly collapsed, including a collapsed streaming thinking block.
    expanded: BTreeMap<u64, bool>,
    entry_focus: Option<u64>,
    follow: bool,
    unread: Option<OperatorTurnId>,
    reviewed: Option<OperatorTurnId>,
    compact_turn: Option<OperatorTurnId>,
    branch_summary_turn: Option<OperatorTurnId>,
    branch_summary_pending: Option<PendingBranchSummaryUi>,
    /// Wrapped journal rows, reused until an input in their key changes.
    rows: std::cell::RefCell<Option<(render::RowsKey, Vec<render::TranscriptRow>)>>,
}
impl Chat {
    fn new(summary: Summary, context: OperatorContext) -> Self {
        Self {
            summary,
            workdir: None,
            error: None,
            notice: None,
            draft: String::new(),
            cursor: TextCursor::default(),
            context,
            messages: Vec::new(),
            ordinal: 0,
            confirmed_tail: None,
            reading: false,
            dirty: true,
            submitting: false,
            stopping: false,
            scroll: 0,
            anchor: None,
            expanded: BTreeMap::new(),
            entry_focus: None,
            follow: true,
            unread: None,
            reviewed: None,
            compact_turn: None,
            branch_summary_turn: None,
            branch_summary_pending: None,
            rows: std::cell::RefCell::new(None),
        }
    }
    fn can_submit(&self) -> bool {
        !self.submitting
            && (self.summary.worker_stopped || self.summary.worker_live)
            && self
                .summary
                .current_turn
                .as_ref()
                .is_none_or(|turn| turn.state != OperatorTurnState::Running)
    }
    fn can_compact(&self) -> Result<(), &'static str> {
        if !self.can_submit() || self.stopping {
            return Err(BUSY_REASON);
        }
        if self.summary.current_turn.is_none() {
            return Err(
                "Cannot compact Operator context before the first completed turn in this session.",
            );
        }
        Ok(())
    }
    fn can_reset_session(&self) -> Result<(), &'static str> {
        if self.submitting || self.stopping {
            return Err("Cannot start a new Operator session while a turn or stop is in progress.");
        }
        if self
            .summary
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state == OperatorTurnState::Running)
        {
            return Err("Cannot start a new Operator session while a turn is running.");
        }
        if !self.summary.worker_stopped || self.summary.worker_live {
            return Err(
                "Cannot start a new Operator session while a watched-session lease or worker is active.",
            );
        }
        Ok(())
    }
    fn can_navigate_tree(&self) -> Result<(), &'static str> {
        if self.submitting || self.stopping {
            return Err("Cannot switch Operator tree branch while a turn or stop is in progress.");
        }
        if self
            .summary
            .current_turn
            .as_ref()
            .is_some_and(|turn| turn.state == OperatorTurnState::Running)
        {
            return Err("Cannot switch Operator tree branch while a turn is running.");
        }
        if !self.summary.worker_stopped || self.summary.worker_live {
            return Err(
                "Cannot switch Operator tree branch while a watched-session lease or worker is active.",
            );
        }
        Ok(())
    }
    /// Enter and mouse share this toggle; the choice outlives the default.
    fn toggle_entry(&mut self, entry: render::TranscriptEntry) {
        self.expanded.insert(entry.ordinal, !entry.expanded);
    }
}
#[derive(Clone)]
enum ModelIntent {
    OpenPicker,
    ResolveArg {
        query: String,
        from_quick: Option<u64>,
    },
}
enum Pending {
    ReasoningSetting,
    ModelSetting,
    Models {
        intent: ModelIntent,
    },
    Current,
    Open {
        origin: OpenOrigin,
    },
    Read {
        id: Id,
        after: u64,
    },
    Submit {
        id: Id,
        text: String,
        session: SessionId,
        expected_run: SessionRunId,
        warm: bool,
        origin: SubmitOrigin,
    },
    Compact {
        id: Id,
        draft: String,
        session: SessionId,
        expected_run: SessionRunId,
        warm: bool,
        origin: SubmitOrigin,
    },
    Stop {
        identity: OperatorTurnIdentity,
    },
    NewSession {
        id: Id,
        draft: String,
        origin: SubmitOrigin,
    },
    SessionInfo {
        id: Id,
    },
    Tree {
        id: Id,
    },
    NavigateTree {
        id: Id,
    },
}
const BUSY_REASON: &str = "The Operator is busy or its worker stop is unconfirmed; draft retained.";
const UNAVAILABLE_REASON: &str = "Operator response did not match the singleton conversation.";

/// Display escaped control characters literally. In particular ESC, OSC, CSI and bidi controls
/// can never reach the terminal backend as active controls. Newlines and tabs retain structure.
pub(super) fn display_text(text: &str) -> String {
    let mut safe = String::new();
    for ch in text.chars() {
        if ch == '\n' {
            safe.push(ch);
        } else if ch == '\t' {
            safe.push_str("    ");
        } else if ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            safe.extend(ch.escape_default());
        } else {
            safe.push(ch);
        }
    }
    safe
}
fn paste_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect()
}
fn edit_modal_line(
    text: &mut String,
    cursor: &mut TextCursor,
    key: KeyEvent,
    max_bytes: usize,
) -> bool {
    let before = text.clone();
    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
        text.clear();
        *cursor = TextCursor::default();
    } else if !cursor.edit_key(text, key) {
        match key.code {
            KeyCode::Char(ch)
                if !ch.is_control()
                    && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT) =>
            {
                let ch_str = ch.to_string();
                if text.len().saturating_add(ch_str.len()) <= max_bytes {
                    cursor.insert(text, &ch_str);
                }
            }
            KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End => cursor.key(text, key.code),
            _ => {}
        }
    }
    *text != before
}
fn paste_modal_line(
    text: &mut String,
    cursor: &mut TextCursor,
    pasted: &str,
    max_bytes: usize,
) -> bool {
    let clean: String = pasted.chars().filter(|ch| !ch.is_control()).collect();
    if clean.is_empty() || text.len().saturating_add(clean.len()) > max_bytes {
        return false;
    }
    let before = text.clone();
    cursor.insert(text, &clean);
    *text != before
}
fn content_area(dashboard: &Dashboard, area: Rect) -> Rect {
    let sidebar = super::sidebar_width_for(area, dashboard.sidebar_preference());
    Rect::new(
        area.x.saturating_add(sidebar),
        area.y.saturating_add(1),
        area.width.saturating_sub(sidebar),
        area.height.saturating_sub(2),
    )
}
fn stop_rect(right: Rect) -> Rect {
    let width = right.width.min(13);
    Rect::new(
        right.right().saturating_sub(width),
        right.bottom().saturating_sub(1),
        width,
        right.height.min(1),
    )
}
#[cfg(test)]
use render::wrapped_lines;
use render::{transcript_offset, transcript_pad, transcript_rows};

fn context_label(context: &OperatorContext) -> String {
    let mut label = context.project.clone().unwrap_or_else(|| "global".into());
    if let Some(workspace) = &context.workspace {
        label.push_str(&format!(" / {workspace}"));
    }
    if let (Some(session), Some(run)) = (context.session, context.run) {
        label.push_str(&format!(" / #{} run {}", session.0, run.0));
    }
    display_text(&label)
}
/// The fixed agent working directory every pi turn runs in. The captured
/// target is separate: pi's cwd never follows the Dashboard selection.
fn agent_workdir(dashboard: &Dashboard) -> Option<std::path::PathBuf> {
    (!dashboard.config_dir.as_os_str().is_empty())
        .then(|| dashboard.config_dir.join("operator-workdir"))
}
fn status(chat: &Chat) -> &'static str {
    if chat.stopping {
        return "stopping · accepted operations may finish";
    }
    match chat.summary.current_turn.as_ref().map(|turn| turn.state) {
        Some(OperatorTurnState::Running) => "running",
        Some(OperatorTurnState::Complete)
            if !chat.summary.worker_live && !chat.summary.worker_stopped =>
        {
            "finishing · stop unconfirmed"
        }
        Some(OperatorTurnState::Interrupted)
            if !chat.summary.worker_live && !chat.summary.worker_stopped =>
        {
            "interrupted · stop unconfirmed"
        }
        Some(OperatorTurnState::Error)
            if !chat.summary.worker_live && !chat.summary.worker_stopped =>
        {
            "error · stop unconfirmed"
        }
        Some(OperatorTurnState::Complete) => "complete",
        Some(OperatorTurnState::Interrupted) => "interrupted",
        Some(OperatorTurnState::Error) => "error",
        None => "ready",
    }
}
/// `Operator · pi · provider/id` once the worker has selected a model, else
/// `Operator · pi`. The model is never guessed from settings, and the worker's
/// reserved `unknown/unknown` placeholder is treated as no selection.
fn identity(chat: &Chat) -> String {
    match &chat.summary.model {
        Some(model) if !(model.provider == "unknown" && model.id == "unknown") => format!(
            "Operator · pi · {}/{}",
            display_text(&model.provider),
            display_text(&model.id)
        ),
        Some(_) | None => "Operator · pi".into(),
    }
}

struct ConversationActivity {
    label: String,
    animated: bool,
}
fn conversation_activity(chat: &Chat) -> Option<ConversationActivity> {
    let label = if chat.submitting {
        "Sending".into()
    } else if chat.stopping {
        "Stopping".into()
    } else {
        let turn = chat.summary.current_turn.as_ref()?;
        if turn.state != OperatorTurnState::Running {
            return (!chat.summary.worker_live && !chat.summary.worker_stopped).then(|| {
                ConversationActivity {
                    label: "Stop unconfirmed".into(),
                    animated: false,
                }
            });
        }
        if chat.compact_turn == Some(turn.identity.turn)
            || chat.messages.iter().any(|message| {
                message.turn == turn.identity.turn
                    && matches!(message.content, OperatorMessageContent::Compaction { .. })
            })
        {
            return Some(ConversationActivity {
                label: "Compacting context…".into(),
                animated: true,
            });
        }
        if chat.branch_summary_turn == Some(turn.identity.turn)
            || chat.messages.iter().any(|message| {
                message.turn == turn.identity.turn
                    && matches!(
                        message.content,
                        OperatorMessageContent::BranchSummary { .. }
                    )
            })
        {
            return Some(ConversationActivity {
                label: "Summarizing branch…".into(),
                animated: true,
            });
        }
        let caught_up = !chat.reading && !chat.dirty && chat.confirmed_tail == Some(chat.ordinal);
        if caught_up {
            // Durable start/end events survive catch-up and reopen; only the
            // current turn's last phase decides, so older compaction never sticks.
            let mut compacting = false;
            for message in chat
                .messages
                .iter()
                .filter(|message| message.turn == turn.identity.turn)
            {
                match &message.content {
                    OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted) => {
                        compacting = true;
                    }
                    OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionEnded) => {
                        compacting = false;
                    }
                    _ => {}
                }
            }
            if compacting {
                return Some(ConversationActivity {
                    label: "Compacting context…".into(),
                    animated: true,
                });
            }
        }
        let mut outputs = BTreeSet::new();
        let mut pending = BTreeMap::new();
        if caught_up {
            for message in chat
                .messages
                .iter()
                .rev()
                .filter(|message| message.turn == turn.identity.turn)
            {
                match &message.content {
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                        id, ..
                    }) => {
                        outputs.insert(id);
                    }
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                        id,
                        name,
                        ..
                    }) if !outputs.contains(id) => {
                        pending.insert(id, name);
                    }
                    _ => {}
                }
            }
        }
        match pending.len() {
            0 => "Responding".into(),
            1 => format!(
                "Waiting for {} result",
                super::render::clip_text(
                    &display_text(pending.first_key_value().unwrap().1).replace('\n', "\\n"),
                    16
                )
            ),
            count => format!("Waiting for {count} tool results"),
        }
    };
    Some(ConversationActivity {
        label,
        animated: true,
    })
}

impl Dashboard {
    /// The singleton is worth a spinner while its visible journal animates.
    pub(super) fn operator_conversation_busy(&self) -> bool {
        self.operator.visible
            && self.outer_area.width >= 60
            && self.outer_area.height >= 16
            && self
                .operator
                .chat
                .as_ref()
                .and_then(conversation_activity)
                .is_some_and(|activity| activity.animated)
    }
    pub(super) fn operator_quick_visible(&self) -> bool {
        self.operator
            .quick
            .as_ref()
            .is_some_and(|quick| quick.visible)
    }
    pub(super) fn open_operator_quick_message(&mut self) -> DashboardAction {
        // The captured target belongs to the next message, so opening an
        // editable Quick draft re-captures the Dashboard's current selection. A
        // frozen Open or Submit keeps the context it captured for itself.
        let context = self.operator_next_context();
        if let Some(quick) = &mut self.operator.quick {
            quick.visible = true;
            if quick.phase == QuickPhase::Editing {
                quick.context = context;
                if let Some(chat) = &mut self.operator.chat {
                    if quick.seeded {
                        // The overlay is another editor for the singleton's one draft.
                        quick.text = chat.draft.clone();
                        quick.cursor = chat.cursor;
                    } else {
                        // Adopt a draft typed before the singleton resolved rather
                        // than replacing it with the empty full draft.
                        chat.draft = quick.text.clone();
                        chat.cursor = quick.cursor;
                        quick.seeded = true;
                    }
                }
            }
        } else {
            let seeded = self.operator.chat.is_some();
            let (text, cursor) = self
                .operator
                .chat
                .as_ref()
                .map_or((String::new(), TextCursor::default()), |chat| {
                    (chat.draft.clone(), chat.cursor)
                });
            self.operator.quick_generation = self.operator.quick_generation.wrapping_add(1);
            self.operator.quick = Some(QuickMessage {
                generation: self.operator.quick_generation,
                visible: true,
                text,
                cursor,
                context,
                error: None,
                phase: QuickPhase::Editing,
                seeded,
            });
        }
        DashboardAction::Redraw
    }
    pub(super) fn operator_dismiss_quick(&mut self) {
        if self.operator.quick.as_ref().is_some_and(|q| {
            matches!(q.phase, QuickPhase::Editing | QuickPhase::CheckingModels)
                && q.error.is_none()
                && !q.seeded
        }) {
            // A cancelled unsent Quick draft creates nothing and leaves no overlay.
            self.operator.quick = None;
        } else if let Some(quick) = &mut self.operator.quick {
            if quick.phase == QuickPhase::CheckingModels {
                quick.phase = QuickPhase::Editing;
            }
            quick.visible = false;
        }
    }
    pub(super) fn operator_quick_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind != KeyEventKind::Press {
            return DashboardAction::None;
        }
        self.copy_notice = None;
        if key.code == KeyCode::Esc {
            self.operator_dismiss_quick();
            return DashboardAction::Redraw;
        }
        let Some(quick) = &mut self.operator.quick else {
            return DashboardAction::None;
        };
        if quick.phase != QuickPhase::Editing {
            return DashboardAction::None;
        }
        let edited = if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            quick.text.clear();
            quick.cursor = TextCursor::default();
            true
        } else {
            quick.cursor.edit_key(&mut quick.text, key)
        };
        if !edited {
            match key.code {
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    return self.operator_quick_paste("\n");
                }
                KeyCode::Enter => {
                    if quick.text.trim().is_empty() {
                        return DashboardAction::None;
                    }
                    quick.error = None;
                    let quick_text = quick.text.clone();
                    if let slash::ParsedDraft::Slash { name, args } =
                        slash::parse_draft(&quick_text)
                    {
                        let Some(spec) = slash::find_command(name) else {
                            let error = slash::unknown_command_error(name);
                            if let Some(q) = &mut self.operator.quick {
                                q.error = Some(display_text(&error));
                            }
                            return DashboardAction::Redraw;
                        };
                        return match spec.kind {
                            slash::SlashCommandKind::Copy => self.operator_run_copy(args, true),
                            slash::SlashCommandKind::Thinking => {
                                self.operator_run_thinking(args, true)
                            }
                            slash::SlashCommandKind::Skills => self.operator_run_skills(true),
                            slash::SlashCommandKind::Model => self.operator_run_model(args, true),
                            slash::SlashCommandKind::New => self.operator_run_new(args, true),
                            slash::SlashCommandKind::Session => {
                                self.operator_run_session(args, true)
                            }
                            slash::SlashCommandKind::Compact => {
                                self.operator_run_compact(args, true)
                            }
                            slash::SlashCommandKind::Tree => self.operator_run_tree(args, true),
                        };
                    }
                    let Some(quick) = &mut self.operator.quick else {
                        return DashboardAction::Redraw;
                    };
                    let generation = quick.generation;
                    if self.operator.chat.is_some() {
                        quick.phase = QuickPhase::Submitting;
                        self.operator_submit_to(SubmitOrigin::Quick { generation });
                    } else {
                        let submission = QuickSubmission {
                            generation,
                            text: quick.text.clone(),
                            context: quick.context.clone(),
                        };
                        quick.phase = QuickPhase::Opening;
                        self.operator_open(OpenOrigin::Quick(submission));
                    }
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    return self.operator_quick_paste(&ch.to_string());
                }
                KeyCode::Up | KeyCode::Down => quick
                    .cursor
                    .vertical(&quick.text, key.code == KeyCode::Down),
                code => quick.cursor.key(&mut quick.text, code),
            }
        }
        self.operator_sync_quick_draft();
        DashboardAction::Redraw
    }
    pub(super) fn operator_quick_paste(&mut self, text: &str) -> DashboardAction {
        self.copy_notice = None;
        if let Some(quick) = &mut self.operator.quick
            && quick.phase == QuickPhase::Editing
        {
            let text = paste_text(text);
            if quick.text.len().saturating_add(text.len()) <= MAX_OPERATOR_TEXT_BYTES {
                quick.cursor.insert(&mut quick.text, &text);
            }
        }
        self.operator_sync_quick_draft();
        DashboardAction::Redraw
    }
    /// Mirror Quick edits into the singleton draft while the overlay owns it.
    fn operator_sync_quick_draft(&mut self) {
        let Some(quick) = &self.operator.quick else {
            return;
        };
        if quick.phase != QuickPhase::Editing || !quick.seeded {
            return;
        }
        let (text, cursor) = (quick.text.clone(), quick.cursor);
        if let Some(chat) = &mut self.operator.chat {
            chat.draft = text;
            chat.cursor = cursor;
        }
    }
    pub(super) fn operator_quick_mouse(
        &mut self,
        _event: MouseEvent,
        _area: Rect,
    ) -> Option<DashboardAction> {
        self.operator_quick_visible()
            .then_some(DashboardAction::None)
    }
    fn operator_quick_failed(&mut self, generation: u64, error: &str) {
        if let Some(quick) = &mut self.operator.quick
            && quick.generation == generation
        {
            quick.phase = QuickPhase::Editing;
            quick.error = Some(display_text(error));
        }
    }
    pub(super) fn operator_sidebar_requested(&self) -> bool {
        self.operator.visible && self.operator.sidebar_open
    }
    pub(super) fn operator_sidebar_on_demand(&self) -> bool {
        self.operator.visible && self.outer_area.width < 100 && !self.operator.sidebar_open
    }
    pub(super) fn operator_transcript_focused(&self) -> bool {
        self.operator.visible && self.operator.transcript_focus
    }
    pub(super) fn operator_submit_available(&self) -> bool {
        self.operator.chat.as_ref().is_some_and(|chat| {
            let trimmed = chat.draft.trim();
            if trimmed.is_empty() {
                return false;
            }
            if let slash::ParsedDraft::Slash { name, .. } = slash::parse_draft(&chat.draft)
                && slash::find_command(name).is_some()
            {
                return true;
            }
            chat.can_submit()
        })
    }
    pub(super) fn operator_stop_available(&self) -> bool {
        self.operator.chat.as_ref().is_some_and(|chat| {
            !chat.summary.worker_stopped
                || chat
                    .summary
                    .current_turn
                    .as_ref()
                    .is_some_and(|turn| turn.state == OperatorTurnState::Running)
        })
    }
    fn capture_operator_context(&self) -> OperatorContext {
        let (project, workspace_id) = self.creation_context();
        let workspace = super::state::find_workspace(self, &project, &workspace_id);
        let session = self
            .action_session()
            .and_then(|id| super::state::find_session(self, id));
        OperatorContext {
            project: (!project.is_empty()).then_some(project),
            workspace: workspace.map(|workspace| workspace.name.clone()),
            workspace_id: workspace.map(|workspace| workspace.id.clone()),
            cwd: session
                .map(|session| session.cwd.clone())
                .or_else(|| workspace.map(|workspace| workspace.path.clone())),
            session: session.map(|session| session.id),
            run: session.map(|session| session.run),
        }
    }
    /// The target the next message will carry. A visible full view already owns
    /// the next-message target in `chat.context`; any other view captures the
    /// current selection now. Opening either composer uses this one rule, so
    /// Quick and the full composer never disagree about the next message.
    fn operator_next_context(&self) -> OperatorContext {
        if self.operator.visible {
            self.operator.chat.as_ref().map_or_else(
                || self.operator.context.clone(),
                |chat| chat.context.clone(),
            )
        } else {
            self.capture_operator_context()
        }
    }
    /// The full view opens the singleton directly, composer focused. Open is
    /// idempotent, so reopening never resets the conversation or its draft.
    pub(super) fn open_operator(&mut self) -> DashboardAction {
        if !self.operator.visible {
            // The row above the composer names the target for the next message,
            // so opening the composer re-captures the current selection. A
            // message already being sent keeps its own frozen target.
            self.operator.context = self.capture_operator_context();
            if let Some(chat) = self.operator.chat.as_mut() {
                chat.context = self.operator.context.clone();
            }
            self.retarget(super::state::PaneChange::Focus);
        }
        self.operator.visible = true;
        self.operator.editing = true;
        self.operator.transcript_focus = false;
        self.operator.error = None;
        self.mode = InputMode::Browse;
        self.operator_open(OpenOrigin::Manual);
        self.operator_read();
        DashboardAction::Redraw
    }
    /// Startup probes the singleton without creating a record or a process.
    pub(super) fn operator_start_current(&mut self) {
        if self
            .operator
            .pending
            .values()
            .any(|pending| matches!(pending, Pending::Current))
        {
            return;
        }
        self.operator_request(Pending::Current, OperatorRequest::Current);
    }
    fn operator_request(&mut self, pending: Pending, request: OperatorRequest) {
        let request_id = self.next_request_id();
        self.operator.pending.insert(request_id, pending);
        self.push_request(ClientMessage {
            request_id,
            request: Request::Operator(request),
        });
    }
    fn operator_open(&mut self, origin: OpenOrigin) {
        if matches!(origin, OpenOrigin::Manual)
            && self.operator.pending.values().any(|pending| {
                matches!(
                    pending,
                    Pending::Open {
                        origin: OpenOrigin::Manual
                    }
                )
            })
        {
            return;
        }
        self.capture_operator_setup_for_open();
        self.operator.loading = true;
        self.operator_request(Pending::Open { origin }, OperatorRequest::Open);
    }
    /// Load the next page of the singleton journal when it is dirty.
    fn operator_read(&mut self) {
        let Some(chat) = &mut self.operator.chat else {
            return;
        };
        if chat.reading || !chat.dirty {
            return;
        }
        chat.dirty = false;
        chat.reading = true;
        chat.confirmed_tail = None;
        let (id, after) = (chat.summary.id, chat.ordinal);
        self.operator.loading = true;
        self.operator_request(
            Pending::Read { id, after },
            OperatorRequest::Read {
                conversation: id,
                after,
                limit: 100,
            },
        );
    }
    pub(super) fn operator_changed(&mut self, id: Id) {
        let Some(chat) = &mut self.operator.chat else {
            return;
        };
        if chat.summary.id != id {
            // Legacy rows stay on disk but never enter this Dashboard view.
            return;
        }
        chat.dirty = true;
        chat.confirmed_tail = None;
        self.operator_read();
    }
    fn operator_submit(&mut self) {
        self.operator_submit_to(SubmitOrigin::Composer);
    }
    fn operator_submit_to(&mut self, origin: SubmitOrigin) {
        let Some(chat) = self.operator.chat.as_mut() else {
            return;
        };
        let (text, context) = match origin {
            SubmitOrigin::Composer => (chat.draft.clone(), chat.context.clone()),
            SubmitOrigin::Quick { generation } => {
                let Some(quick) = self
                    .operator
                    .quick
                    .as_ref()
                    .filter(|quick| quick.generation == generation)
                else {
                    return;
                };
                (quick.text.clone(), quick.context.clone())
            }
        };
        if let slash::ParsedDraft::Slash { name, .. } = slash::parse_draft(&text)
            && slash::find_command(name).is_none()
        {
            let error = slash::unknown_command_error(name);
            chat.notice = None;
            chat.error = Some(display_text(&error));
            if matches!(origin, SubmitOrigin::Composer) {
                self.operator.slash_dismissed_for = Some(text);
            }
            if let SubmitOrigin::Quick { generation } = origin {
                self.operator_quick_failed(generation, &error);
            }
            return;
        }
        let (skill, request) = match slash::parse_draft(&text) {
            slash::ParsedDraft::Skill { name, request } => {
                match ovrcr_protocol::OperatorSkillSelection::named(name) {
                    Ok(skill) => (Some(skill), request.to_owned()),
                    Err(error) => {
                        chat.notice = None;
                        chat.error = Some(display_text(&error));
                        if let SubmitOrigin::Quick { generation } = origin {
                            self.operator_quick_failed(generation, &error);
                        }
                        return;
                    }
                }
            }
            _ => (None, text.clone()),
        };
        if !chat.can_submit() {
            chat.notice = None;
            chat.error = Some(BUSY_REASON.into());
            if let SubmitOrigin::Quick { generation } = origin {
                self.operator_quick_failed(generation, BUSY_REASON);
            }
            return;
        }
        if text.trim().is_empty() {
            return;
        }
        let expected_run = chat.summary.run;
        let session = chat.summary.session;
        let id = chat.summary.id;
        let warm = chat.summary.worker_live;
        chat.submitting = true;
        chat.context = context.clone();
        chat.error = None;
        chat.notice = None;
        self.operator_request(
            Pending::Submit {
                id,
                text: text.clone(),
                session,
                expected_run,
                warm,
                origin,
            },
            OperatorRequest::Submit {
                skill,
                conversation: id,
                expected_run,
                text: request,
                context,
            },
        );
    }
    fn operator_stop(&mut self) {
        let Some(chat) = self.operator.chat.as_mut() else {
            return;
        };
        let Some(turn) = chat.summary.current_turn.as_ref() else {
            return;
        };
        if chat.stopping
            || (chat.summary.worker_stopped && turn.state != OperatorTurnState::Running)
        {
            return;
        }
        let identity = turn.identity;
        chat.stopping = true;
        self.operator_request(
            Pending::Stop { identity },
            OperatorRequest::Stop { identity },
        );
    }
    fn operator_reasoning_label(&self) -> String {
        let saved = self.settings.operator.thinking_level;
        let chat = self.operator.chat.as_ref();
        let reasoning = chat.and_then(|chat| chat.summary.reasoning.as_ref());
        let level = reasoning.map_or("—", |state| state.level.as_str());
        if chat.is_some_and(|chat| !chat.can_submit())
            && reasoning.is_some_and(|state| state.requested != saved)
        {
            format!("Reasoning {level} → {} next turn", saved.as_str())
        } else if reasoning.is_some_and(|state| state.requested != saved)
            || (saved != ThinkingLevel::Default
                && reasoning.is_none_or(|state| state.level != saved))
        {
            format!("Reasoning {level} (set: {})", saved.as_str())
        } else {
            format!("Reasoning {level}")
        }
    }

    fn operator_reasoning_rect(&self, header: Rect) -> Rect {
        let right = header
            .right()
            .saturating_sub(if self.operator_stop_available() {
                13
            } else {
                0
            });
        let width = (self.operator_reasoning_label().width() as u16 + 13)
            .min(right.saturating_sub(header.x));
        Rect::new(
            right.saturating_sub(width),
            header.bottom().saturating_sub(1),
            width,
            header.height.min(1),
        )
    }

    pub(super) fn operator_overlay_keymap(&self) -> Option<OperatorOverlayKeymap> {
        match &self.operator.overlay {
            Some(OperatorOverlay::Skills(_)) => Some(OperatorOverlayKeymap::SkillPicker),
            Some(OperatorOverlay::Model(_)) => Some(OperatorOverlayKeymap::ModelPicker),
            Some(OperatorOverlay::Reasoning(_)) => Some(OperatorOverlayKeymap::ReasoningPicker),
            Some(OperatorOverlay::Session(_)) => Some(OperatorOverlayKeymap::SessionModal),
            Some(OperatorOverlay::Tree(TreeModal::Loading)) => {
                Some(OperatorOverlayKeymap::TreeLoading)
            }
            Some(OperatorOverlay::Tree(TreeModal::Loaded(picker))) => match picker.step {
                TreePickerStep::Browse => Some(OperatorOverlayKeymap::TreeBrowse),
                TreePickerStep::ConfirmSummarize { .. } => {
                    Some(OperatorOverlayKeymap::TreeConfirmSummarize)
                }
                TreePickerStep::CustomInstructions { .. } => {
                    Some(OperatorOverlayKeymap::TreeCustomInstructions)
                }
            },
            None => None,
        }
    }

    fn operator_slash_menu_specs(&self) -> Vec<&'static slash::SlashCommandSpec> {
        if !self.operator.visible
            || !self.operator.editing
            || self.operator.transcript_focus
            || self.operator.overlay.is_some()
        {
            return Vec::new();
        }
        let Some(chat) = self.operator.chat.as_ref() else {
            return Vec::new();
        };
        if self.operator.slash_dismissed_for.as_deref() == Some(chat.draft.as_str()) {
            return Vec::new();
        }
        slash::menu_candidates(&chat.draft)
    }

    pub(super) fn operator_slash_menu_open(&self) -> bool {
        !self.operator_slash_menu_specs().is_empty()
    }

    fn operator_reset_slash_menu(&mut self) {
        self.operator.slash_selected = 0;
        self.operator.slash_dismissed_for = None;
        if let Some(chat) = self.operator.chat.as_mut() {
            chat.notice = None;
        }
    }

    fn operator_run_copy(&mut self, args: &str, from_quick: bool) -> DashboardAction {
        if !args.is_empty() {
            let error = "Usage: /copy (takes no arguments).";
            if from_quick {
                if let Some(quick) = &mut self.operator.quick {
                    quick.error = Some(error.into());
                }
            } else if let Some(chat) = self.operator.chat.as_mut() {
                chat.notice = None;
                chat.error = Some(error.into());
            }
            return DashboardAction::Redraw;
        }
        let reply = self
            .operator
            .chat
            .as_ref()
            .and_then(feed::latest_assistant_reply);
        let Some(text) = reply else {
            let msg = "No assistant reply to copy yet.";
            if from_quick {
                if let Some(quick) = &mut self.operator.quick {
                    quick.error = Some(msg.into());
                }
            } else {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.draft.clear();
                    chat.cursor = TextCursor::default();
                    chat.notice = None;
                    chat.error = Some(msg.into());
                }
                self.operator.slash_selected = 0;
                self.operator.slash_dismissed_for = None;
            }
            return DashboardAction::Redraw;
        };
        if text.len() > super::copy::MAX_COPY_BYTES {
            let msg = "Latest assistant reply exceeds 64 KiB clipboard limit.";
            if from_quick {
                if let Some(quick) = &mut self.operator.quick {
                    quick.error = Some(msg.into());
                }
            } else {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.draft.clear();
                    chat.cursor = TextCursor::default();
                    chat.notice = None;
                    chat.error = Some(msg.into());
                }
                self.operator.slash_selected = 0;
                self.operator.slash_dismissed_for = None;
            }
            return DashboardAction::Redraw;
        }
        let confirmation = "Copied latest assistant reply to clipboard.";
        if let Some(chat) = self.operator.chat.as_mut() {
            chat.draft.clear();
            chat.cursor = TextCursor::default();
            chat.error = None;
            chat.notice = Some(confirmation.into());
        }
        self.operator.slash_selected = 0;
        self.operator.slash_dismissed_for = None;
        if from_quick {
            self.operator.quick = None;
            self.desktop.notice = Some(confirmation.into());
        }
        DashboardAction::CopyText(text)
    }

    fn operator_write_thinking_setting(&mut self, level: ThinkingLevel) {
        let request_id = self.next_request_id();
        self.operator.error = None;
        self.operator
            .pending
            .insert(request_id, Pending::ReasoningSetting);
        self.enqueue_request(ClientMessage {
            request_id,
            request: Request::SetSetting {
                path: "operator.thinking_level".into(),
                value: Some(format!("\"{}\"", level.as_str())),
            },
        });
    }

    fn operator_run_thinking(&mut self, args: &str, from_quick: bool) -> DashboardAction {
        if args.is_empty() {
            if let Some(chat) = self.operator.chat.as_mut() {
                chat.draft.clear();
                chat.cursor = TextCursor::default();
                chat.error = None;
                chat.notice = None;
            }
            self.operator.slash_selected = 0;
            self.operator.slash_dismissed_for = None;
            if from_quick {
                self.operator.quick = None;
                self.operator.pending_overlay = Some(PendingOperatorOverlay::Reasoning);
                self.open_operator();
                if self.operator.chat.is_some() {
                    self.operator_open_reasoning();
                }
            } else {
                self.operator_open_reasoning();
            }
            return DashboardAction::Redraw;
        }
        let parsed = slash::parse_thinking_level(args);
        let reasoning = self
            .operator
            .chat
            .as_ref()
            .and_then(|chat| chat.summary.reasoning.as_ref());
        let allowed = slash::allowed_thinking_levels(reasoning);
        let Some(level) = parsed.filter(|level| allowed.contains(level)) else {
            let error = slash::thinking_level_error(args, reasoning, parsed.is_some());
            if from_quick {
                if let Some(quick) = &mut self.operator.quick {
                    quick.error = Some(display_text(&error));
                }
            } else if let Some(chat) = self.operator.chat.as_mut() {
                chat.notice = None;
                chat.error = Some(display_text(&error));
            }
            return DashboardAction::Redraw;
        };
        self.operator_write_thinking_setting(level);
        let confirmation = format!("Set thinking level to {}.", level.as_str());
        if let Some(chat) = self.operator.chat.as_mut() {
            chat.draft.clear();
            chat.cursor = TextCursor::default();
            chat.error = None;
            chat.notice = Some(confirmation.clone());
        }
        self.operator.slash_selected = 0;
        self.operator.slash_dismissed_for = None;
        if from_quick {
            self.operator.quick = None;
            self.desktop.notice = Some(confirmation);
        }
        DashboardAction::Redraw
    }

    fn operator_send_new_session(&mut self, origin: SubmitOrigin) {
        let Some(chat) = self.operator.chat.as_mut() else {
            return;
        };
        let id = chat.summary.id;
        let expected_run = chat.summary.run;
        let draft = chat.draft.clone();
        chat.error = None;
        chat.notice = None;
        if matches!(origin, SubmitOrigin::Composer) {
            self.operator.slash_dismissed_for = Some(draft.clone());
        }
        self.operator_request(
            Pending::NewSession { id, draft, origin },
            OperatorRequest::NewSession {
                conversation: id,
                expected_run,
            },
        );
    }

    fn operator_run_new(&mut self, args: &str, from_quick: bool) -> DashboardAction {
        if !args.is_empty() {
            let error = "Usage: /new (takes no arguments).";
            if from_quick {
                if let Some(quick) = &mut self.operator.quick {
                    quick.error = Some(error.into());
                }
            } else if let Some(chat) = self.operator.chat.as_mut() {
                chat.notice = None;
                chat.error = Some(error.into());
            }
            return DashboardAction::Redraw;
        }
        if self
            .operator
            .pending
            .values()
            .any(|pending| matches!(pending, Pending::NewSession { .. }))
        {
            let error = "A new Operator session reset is already in progress.";
            if from_quick {
                if let Some(quick) = &mut self.operator.quick {
                    quick.error = Some(error.into());
                }
            } else if let Some(chat) = self.operator.chat.as_mut() {
                chat.notice = None;
                chat.error = Some(error.into());
            }
            return DashboardAction::Redraw;
        }
        if from_quick {
            let Some(generation) = self.operator.quick.as_ref().map(|q| q.generation) else {
                return DashboardAction::Redraw;
            };
            if let Some(chat) = self.operator.chat.as_ref() {
                if let Err(reason) = chat.can_reset_session() {
                    if let Some(quick) = &mut self.operator.quick {
                        quick.error = Some(reason.into());
                    }
                    return DashboardAction::Redraw;
                }
                if let Some(quick) = &mut self.operator.quick {
                    quick.phase = QuickPhase::Submitting;
                }
                self.operator_send_new_session(SubmitOrigin::Quick { generation });
            } else {
                if let Some(quick) = &mut self.operator.quick {
                    quick.phase = QuickPhase::Opening;
                }
                self.operator_open(OpenOrigin::QuickNew { generation });
            }
            return DashboardAction::Redraw;
        }
        let Some(chat) = self.operator.chat.as_mut() else {
            return DashboardAction::Redraw;
        };
        if let Err(reason) = chat.can_reset_session() {
            let dismissed = chat.draft.clone();
            chat.notice = None;
            chat.error = Some(reason.into());
            self.operator.slash_dismissed_for = Some(dismissed);
            return DashboardAction::Redraw;
        }
        self.operator_send_new_session(SubmitOrigin::Composer);
        DashboardAction::Redraw
    }

    fn operator_send_compact(&mut self, instructions: Option<String>, origin: SubmitOrigin) {
        let Some(chat) = self.operator.chat.as_mut() else {
            return;
        };
        let id = chat.summary.id;
        let session = chat.summary.session;
        let expected_run = chat.summary.run;
        let warm = chat.summary.worker_live;
        let draft = match origin {
            SubmitOrigin::Composer => chat.draft.clone(),
            SubmitOrigin::Quick { generation } => self
                .operator
                .quick
                .as_ref()
                .filter(|quick| quick.generation == generation)
                .map_or_else(|| chat.draft.clone(), |quick| quick.text.clone()),
        };
        chat.submitting = true;
        chat.error = None;
        chat.notice = None;
        if matches!(origin, SubmitOrigin::Composer) {
            self.operator.slash_dismissed_for = Some(draft.clone());
        }
        self.operator_request(
            Pending::Compact {
                id,
                draft,
                session,
                expected_run,
                warm,
                origin,
            },
            OperatorRequest::Compact {
                conversation: id,
                expected_run,
                instructions,
            },
        );
    }

    fn operator_run_compact(&mut self, args: &str, from_quick: bool) -> DashboardAction {
        let trimmed = args.trim();
        let instructions = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        if let Some(ref text) = instructions
            && (text.len() > MAX_OPERATOR_TEXT_BYTES || text.contains('\0'))
        {
            let error = "Compaction instructions exceed maximum length.";
            if from_quick {
                if let Some(quick) = &mut self.operator.quick {
                    quick.error = Some(error.into());
                }
            } else if let Some(chat) = self.operator.chat.as_mut() {
                chat.notice = None;
                chat.error = Some(error.into());
            }
            return DashboardAction::Redraw;
        }
        if from_quick {
            let Some(quick_snapshot) = self
                .operator
                .quick
                .as_ref()
                .map(|q| (q.generation, q.text.clone()))
            else {
                return DashboardAction::Redraw;
            };
            let (generation, quick_draft) = quick_snapshot;
            if let Some(chat) = self.operator.chat.as_ref() {
                if let Err(reason) = chat.can_compact() {
                    if let Some(quick) = &mut self.operator.quick {
                        quick.error = Some(reason.into());
                    }
                    return DashboardAction::Redraw;
                }
                if let Some(quick) = &mut self.operator.quick {
                    quick.seeded = true;
                    quick.phase = QuickPhase::Submitting;
                }
                self.operator_send_compact(instructions, SubmitOrigin::Quick { generation });
            } else {
                if let Some(quick) = &mut self.operator.quick {
                    quick.phase = QuickPhase::Opening;
                }
                self.operator_open(OpenOrigin::QuickCompact {
                    generation,
                    draft: quick_draft,
                    instructions,
                });
            }
            return DashboardAction::Redraw;
        }
        let Some(chat) = self.operator.chat.as_mut() else {
            return DashboardAction::Redraw;
        };
        if let Err(reason) = chat.can_compact() {
            let dismissed = chat.draft.clone();
            chat.notice = None;
            chat.error = Some(reason.into());
            self.operator.slash_dismissed_for = Some(dismissed);
            return DashboardAction::Redraw;
        }
        self.operator_send_compact(instructions, SubmitOrigin::Composer);
        DashboardAction::Redraw
    }

    fn operator_open_session_info(&mut self) {
        if matches!(
            self.operator.pending_overlay,
            Some(PendingOperatorOverlay::Session)
        ) {
            self.operator.pending_overlay = None;
        }
        let Some(id) = self.operator.chat.as_ref().map(|chat| chat.summary.id) else {
            return;
        };
        self.operator
            .set_overlay(OperatorOverlay::Session(SessionModal::Loading));
        if !self
            .operator
            .pending
            .values()
            .any(|pending| matches!(pending, Pending::SessionInfo { .. }))
        {
            self.operator_request(
                Pending::SessionInfo { id },
                OperatorRequest::SessionInfo { conversation: id },
            );
        }
    }

    fn operator_open_tree(&mut self) {
        if matches!(
            self.operator.pending_overlay,
            Some(PendingOperatorOverlay::Tree)
        ) {
            self.operator.pending_overlay = None;
        }
        let Some(id) = self.operator.chat.as_ref().map(|chat| chat.summary.id) else {
            return;
        };
        self.operator
            .set_overlay(OperatorOverlay::Tree(TreeModal::Loading));
        if !self
            .operator
            .pending
            .values()
            .any(|pending| matches!(pending, Pending::Tree { .. }))
        {
            self.operator_request(
                Pending::Tree { id },
                OperatorRequest::Tree { conversation: id },
            );
        }
    }

    fn operator_run_tree(&mut self, args: &str, from_quick: bool) -> DashboardAction {
        if !args.is_empty() {
            let error = "Usage: /tree (takes no arguments).";
            if from_quick {
                if let Some(quick) = &mut self.operator.quick {
                    quick.error = Some(error.into());
                }
            } else if let Some(chat) = self.operator.chat.as_mut() {
                chat.notice = None;
                chat.error = Some(error.into());
            }
            return DashboardAction::Redraw;
        }
        if let Some(chat) = self.operator.chat.as_mut() {
            chat.draft.clear();
            chat.cursor = TextCursor::default();
            chat.error = None;
            chat.notice = None;
        }
        self.operator.slash_selected = 0;
        self.operator.slash_dismissed_for = None;
        if from_quick {
            self.operator.quick = None;
            self.operator.pending_overlay = Some(PendingOperatorOverlay::Tree);
            self.open_operator();
            if self.operator.chat.is_some() {
                self.operator_open_tree();
            }
        } else {
            self.operator_open_tree();
        }
        DashboardAction::Redraw
    }

    fn operator_submit_navigate_tree(
        &mut self,
        target_entry_id: String,
        summarize: Option<OperatorBranchSummaryRequest>,
    ) -> DashboardAction {
        let check = self
            .operator
            .chat
            .as_ref()
            .map(|chat| (chat.summary.id, chat.summary.run, chat.can_navigate_tree()));
        let Some((id, expected_run, result)) = check else {
            return DashboardAction::Redraw;
        };
        if let Err(reason) = result {
            if let Some(picker) = self.operator.tree_picker_mut() {
                picker.error = Some(reason.into());
            }
            if let Some(chat) = self.operator.chat.as_mut() {
                chat.notice = None;
                chat.error = Some(reason.into());
            }
            return DashboardAction::Redraw;
        }
        if let Some(picker) = self.operator.tree_picker_mut() {
            picker.submitting = true;
            picker.error = None;
        }
        self.operator_request(
            Pending::NavigateTree { id },
            OperatorRequest::NavigateTree {
                conversation: id,
                expected_run,
                target_entry_id,
                summarize,
            },
        );
        DashboardAction::Redraw
    }

    fn operator_tree_modal_key(&mut self, key: KeyEvent) -> DashboardAction {
        let Some(modal) = self.operator.tree_modal_mut() else {
            return DashboardAction::Redraw;
        };
        let TreeModal::Loaded(picker) = modal else {
            if key.code == KeyCode::Esc {
                self.operator.clear_overlays();
            }
            return DashboardAction::Redraw;
        };

        if picker.submitting {
            return DashboardAction::Redraw;
        }

        match &mut picker.step {
            TreePickerStep::CustomInstructions {
                target_entry_id,
                target_preview,
                input,
                cursor,
            } => {
                if key.code == KeyCode::Esc {
                    let target_entry_id = target_entry_id.clone();
                    let target_preview = target_preview.clone();
                    picker.step = TreePickerStep::ConfirmSummarize {
                        target_entry_id,
                        target_preview,
                        choice: BranchSummaryChoice::CustomPrompt,
                    };
                    picker.error = None;
                    return DashboardAction::Redraw;
                }
                if key.code == KeyCode::Enter && key.modifiers.is_empty() {
                    let trimmed = input.trim().to_owned();
                    if trimmed.is_empty() {
                        picker.error = Some(
                            "Enter custom summarization instructions, or press Esc to go back."
                                .into(),
                        );
                        return DashboardAction::Redraw;
                    }
                    let target_entry_id = target_entry_id.clone();
                    return self.operator_submit_navigate_tree(
                        target_entry_id,
                        Some(OperatorBranchSummaryRequest {
                            instructions: Some(trimmed),
                        }),
                    );
                }
                edit_modal_line(input, cursor, key, MAX_OPERATOR_TEXT_BYTES);
                picker.error = None;
                return DashboardAction::Redraw;
            }
            TreePickerStep::ConfirmSummarize {
                target_entry_id,
                target_preview,
                choice,
            } => {
                match key.code {
                    KeyCode::Esc => {
                        picker.step = TreePickerStep::Browse;
                        picker.error = None;
                    }
                    KeyCode::Up | KeyCode::BackTab => {
                        *choice = choice.prev();
                    }
                    KeyCode::Down | KeyCode::Tab => {
                        *choice = choice.next();
                    }
                    KeyCode::Enter if key.modifiers.is_empty() => match *choice {
                        BranchSummaryChoice::NoSummary => {
                            let target_entry_id = target_entry_id.clone();
                            return self.operator_submit_navigate_tree(target_entry_id, None);
                        }
                        BranchSummaryChoice::Summarize => {
                            let target_entry_id = target_entry_id.clone();
                            return self.operator_submit_navigate_tree(
                                target_entry_id,
                                Some(OperatorBranchSummaryRequest { instructions: None }),
                            );
                        }
                        BranchSummaryChoice::CustomPrompt => {
                            let target_entry_id = target_entry_id.clone();
                            let target_preview = target_preview.clone();
                            picker.step = TreePickerStep::CustomInstructions {
                                target_entry_id,
                                target_preview,
                                input: String::new(),
                                cursor: TextCursor::default(),
                            };
                            picker.error = None;
                        }
                    },
                    _ => {}
                }
                return DashboardAction::Redraw;
            }
            TreePickerStep::Browse => {}
        }

        if key.code == KeyCode::Esc {
            self.operator.clear_overlays();
            return DashboardAction::Redraw;
        }
        if (key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('f'))
            || (key.modifiers.is_empty() && key.code == KeyCode::Tab)
        {
            picker.cycle_filter(true);
            return DashboardAction::Redraw;
        }
        if key.code == KeyCode::BackTab {
            picker.cycle_filter(false);
            return DashboardAction::Redraw;
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('u') {
            picker.toggle_user_only();
            return DashboardAction::Redraw;
        }
        match key.code {
            KeyCode::Up if key.modifiers.is_empty() => {
                picker.selected = picker.selected.saturating_sub(1);
                return DashboardAction::Redraw;
            }
            KeyCode::Down if key.modifiers.is_empty() => {
                let len = picker.visible_rows().len();
                if len > 0 {
                    picker.selected = (picker.selected + 1).min(len - 1);
                }
                return DashboardAction::Redraw;
            }
            KeyCode::PageUp if key.modifiers.is_empty() => {
                picker.selected = picker.selected.saturating_sub(8);
                return DashboardAction::Redraw;
            }
            KeyCode::PageDown if key.modifiers.is_empty() => {
                let len = picker.visible_rows().len();
                if len > 0 {
                    picker.selected = (picker.selected + 8).min(len - 1);
                }
                return DashboardAction::Redraw;
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                if picker.tree.entries.is_empty() {
                    self.operator.clear_overlays();
                    return DashboardAction::Redraw;
                }
                let Some(entry) = picker.selected_entry().cloned() else {
                    return DashboardAction::Redraw;
                };
                if TreePicker::is_noop_selection(&picker.tree, &entry) {
                    self.operator.clear_overlays();
                    return DashboardAction::Redraw;
                }
                let can_nav = self
                    .operator
                    .chat
                    .as_ref()
                    .map(|chat| chat.can_navigate_tree());
                let Some(can_nav) = can_nav else {
                    return DashboardAction::Redraw;
                };
                if let Err(reason) = can_nav {
                    if let Some(picker) = self.operator.tree_picker_mut() {
                        picker.error = Some(reason.into());
                    }
                    if let Some(chat) = self.operator.chat.as_mut() {
                        chat.notice = None;
                        chat.error = Some(reason.into());
                    }
                    return DashboardAction::Redraw;
                }
                if let Some(TreeModal::Loaded(picker)) = self.operator.tree_modal()
                    && TreePicker::should_prompt_summarize(&picker.tree, &entry)
                {
                    if let Some(picker) = self.operator.tree_picker_mut() {
                        picker.step = TreePickerStep::ConfirmSummarize {
                            target_entry_id: entry.id,
                            target_preview: format!(
                                "{}: {}",
                                tree::entry_role_label(entry.kind),
                                entry.preview
                            ),
                            choice: BranchSummaryChoice::NoSummary,
                        };
                        picker.error = None;
                    }
                    return DashboardAction::Redraw;
                }
                return self.operator_submit_navigate_tree(entry.id, None);
            }
            _ => {}
        }
        if edit_modal_line(
            &mut picker.query,
            &mut picker.cursor,
            key,
            MAX_TREE_SEARCH_BYTES,
        ) {
            picker.on_query_changed();
        }
        DashboardAction::Redraw
    }

    fn operator_run_session(&mut self, args: &str, from_quick: bool) -> DashboardAction {
        if !args.is_empty() {
            let error = "Usage: /session (takes no arguments).";
            if from_quick {
                if let Some(quick) = &mut self.operator.quick {
                    quick.error = Some(error.into());
                }
            } else if let Some(chat) = self.operator.chat.as_mut() {
                chat.notice = None;
                chat.error = Some(error.into());
            }
            return DashboardAction::Redraw;
        }
        if let Some(chat) = self.operator.chat.as_mut() {
            chat.draft.clear();
            chat.cursor = TextCursor::default();
            chat.error = None;
            chat.notice = None;
        }
        self.operator.slash_selected = 0;
        self.operator.slash_dismissed_for = None;
        if from_quick {
            self.operator.quick = None;
            self.operator.pending_overlay = Some(PendingOperatorOverlay::Session);
            self.open_operator();
            if self.operator.chat.is_some() {
                self.operator_open_session_info();
            }
        } else {
            self.operator_open_session_info();
        }
        DashboardAction::Redraw
    }

    fn operator_session_modal_key(&mut self, key: KeyEvent) -> DashboardAction {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter)
            || (key.code == KeyCode::Char('q') && key.modifiers.is_empty())
        {
            self.operator.clear_overlays();
        }
        DashboardAction::Redraw
    }

    fn operator_open_reasoning(&mut self) {
        let choices = slash::allowed_thinking_levels(Some(
            &self
                .operator
                .chat
                .as_ref()
                .and_then(|chat| chat.summary.reasoning.clone())
                .unwrap_or(ovrcr_protocol::OperatorReasoning {
                    requested: ThinkingLevel::Default,
                    level: ThinkingLevel::Default,
                    available: Vec::new(),
                }),
        ));
        let selected = choices
            .iter()
            .position(|level| *level == self.settings.operator.thinking_level)
            .unwrap_or(0);
        self.operator
            .set_overlay(OperatorOverlay::Reasoning(ReasoningPicker {
                choices,
                selected,
            }));
    }

    fn operator_write_model_setting(&mut self, model: Option<&str>) {
        let request_id = self.next_request_id();
        self.operator.error = None;
        self.operator
            .pending
            .insert(request_id, Pending::ModelSetting);
        self.enqueue_request(ClientMessage {
            request_id,
            request: Request::SetSetting {
                path: "operator.model".into(),
                value: model.map(|id| toml_edit::Value::from(id).to_string()),
            },
        });
    }

    fn operator_apply_model_selection(&mut self, model: Option<&str>, from_quick: bool) {
        if matches!(self.operator.overlay, Some(OperatorOverlay::Model(_))) {
            self.operator.overlay = None;
        }
        if matches!(
            self.operator.pending_overlay,
            Some(PendingOperatorOverlay::Model { .. })
        ) {
            self.operator.pending_overlay = None;
        }
        self.operator_write_model_setting(model);
        let confirmation = match model {
            Some(id) => format!("Set Operator model to {}.", display_text(id)),
            None => "Set Operator model to Harness default.".into(),
        };
        if let Some(chat) = self.operator.chat.as_mut() {
            chat.draft.clear();
            chat.cursor = TextCursor::default();
            chat.error = None;
            chat.notice = Some(confirmation.clone());
        }
        self.operator.slash_selected = 0;
        self.operator.slash_dismissed_for = None;
        if from_quick {
            self.operator.quick = None;
            self.desktop.notice = Some(confirmation);
        }
    }

    fn operator_request_models(&mut self, intent: ModelIntent) {
        for pending in self.operator.pending.values_mut() {
            if let Pending::Models { intent: existing } = pending {
                *existing = intent;
                return;
            }
        }
        self.operator_request(Pending::Models { intent }, OperatorRequest::Models);
    }

    fn operator_install_model_picker(&mut self, picker: ModelPicker, notice: Option<String>) {
        self.operator.clear_overlays();
        self.operator.error = None;
        if let Some(chat) = self.operator.chat.as_mut() {
            chat.error = None;
            chat.notice = notice;
            if self.operator.visible {
                self.operator.set_overlay(OperatorOverlay::Model(picker));
            }
        } else if self.operator.visible {
            self.operator.pending_overlay = Some(PendingOperatorOverlay::Model { picker, notice });
        }
    }

    fn operator_run_model(&mut self, args: &str, from_quick: bool) -> DashboardAction {
        let trimmed = args.trim();
        if trimmed.eq_ignore_ascii_case("default") {
            self.operator_apply_model_selection(None, from_quick);
            return DashboardAction::Redraw;
        }
        if trimmed.is_empty() {
            if let Some(chat) = self.operator.chat.as_mut() {
                chat.draft.clear();
                chat.cursor = TextCursor::default();
                chat.error = None;
                chat.notice = None;
            }
            self.operator.slash_selected = 0;
            self.operator.slash_dismissed_for = None;
            if from_quick {
                self.operator.quick = None;
                self.open_operator();
            }
            self.operator_request_models(ModelIntent::OpenPicker);
            return DashboardAction::Redraw;
        }
        if from_quick {
            let Some(quick) = self.operator.quick.as_mut() else {
                return DashboardAction::Redraw;
            };
            quick.error = None;
            quick.phase = QuickPhase::CheckingModels;
            let generation = quick.generation;
            self.operator_request_models(ModelIntent::ResolveArg {
                query: trimmed.to_owned(),
                from_quick: Some(generation),
            });
        } else {
            if let Some(chat) = self.operator.chat.as_mut() {
                chat.error = None;
                chat.notice = None;
            }
            self.operator.slash_selected = 0;
            self.operator.slash_dismissed_for = None;
            self.operator_request_models(ModelIntent::ResolveArg {
                query: trimmed.to_owned(),
                from_quick: None,
            });
        }
        DashboardAction::Redraw
    }

    fn operator_model_key(&mut self, key: KeyEvent) -> DashboardAction {
        let active = self.settings.operator.model.clone();
        let Some(picker) = self.operator.model_picker_mut() else {
            return DashboardAction::None;
        };
        match key.code {
            KeyCode::Esc => {
                self.operator.clear_overlays();
                return DashboardAction::Redraw;
            }
            KeyCode::Up if key.modifiers.is_empty() => {
                picker.selected = picker.selected.saturating_sub(1);
                return DashboardAction::Redraw;
            }
            KeyCode::Down if key.modifiers.is_empty() => {
                let len = picker.filtered_choices().len();
                if len > 0 {
                    picker.selected = (picker.selected + 1).min(len - 1);
                }
                return DashboardAction::Redraw;
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                let filtered = picker.filtered_choices();
                let Some(choice) = filtered.get(picker.selected).copied() else {
                    return DashboardAction::Redraw;
                };
                let selection = match choice {
                    ModelChoice::HarnessDefault => None,
                    ModelChoice::Catalog(idx) => Some(picker.entries[idx].full_id()),
                };
                self.operator_apply_model_selection(selection.as_deref(), false);
                return DashboardAction::Redraw;
            }
            _ => {}
        }
        if edit_modal_line(
            &mut picker.query,
            &mut picker.cursor,
            key,
            MAX_MODEL_FILTER_BYTES,
        ) {
            picker.on_query_changed(active.as_deref());
        }
        DashboardAction::Redraw
    }

    fn operator_reasoning_key(&mut self, key: KeyEvent) -> DashboardAction {
        let picker = self.operator.reasoning_picker_mut().unwrap();
        match key.code {
            KeyCode::Esc => {
                self.operator.clear_overlays();
            }
            KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
            KeyCode::Down => picker.selected = (picker.selected + 1).min(picker.choices.len() - 1),
            KeyCode::Enter => {
                let level = picker.choices[picker.selected];
                self.operator.clear_overlays();
                if !self
                    .operator
                    .pending
                    .values()
                    .any(|pending| matches!(pending, Pending::ReasoningSetting))
                {
                    self.operator_write_thinking_setting(level);
                }
            }
            _ => {}
        }
        DashboardAction::Redraw
    }

    pub(super) fn operator_key(&mut self, key: KeyEvent) -> DashboardAction {
        // Rows built here are the rows the next draw reuses, so build them
        // under the same theme.
        let _theme = crate::theme::ThemeScope::enter(self.theme_tokens());
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
        }
        if key.kind == KeyEventKind::Press {
            self.copy_notice = None;
        }
        match &self.operator.overlay {
            Some(OperatorOverlay::Skills(_)) => return self.operator_skills_key(key),
            Some(OperatorOverlay::Model(_)) => return self.operator_model_key(key),
            Some(OperatorOverlay::Reasoning(_)) => return self.operator_reasoning_key(key),
            Some(OperatorOverlay::Session(_)) => return self.operator_session_modal_key(key),
            Some(OperatorOverlay::Tree(_)) => return self.operator_tree_modal_key(key),
            None => {}
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('t') {
            self.operator_open_reasoning();
            return DashboardAction::Redraw;
        }
        if !self.operator.transcript_focus
            && key.modifiers != KeyModifiers::NONE
            && key.modifiers != KeyModifiers::SHIFT
            && key.modifiers != KeyModifiers::CONTROL
            && key.modifiers != KeyModifiers::ALT
        {
            return DashboardAction::None;
        }
        let slash_specs = self.operator_slash_menu_specs();
        if !slash_specs.is_empty() && key.modifiers.is_empty() {
            let selected = self
                .operator
                .slash_selected
                .min(slash_specs.len().saturating_sub(1));
            match key.code {
                KeyCode::Esc => {
                    if let Some(chat) = self.operator.chat.as_ref() {
                        self.operator.slash_dismissed_for = Some(chat.draft.clone());
                    }
                    return DashboardAction::Redraw;
                }
                KeyCode::Up => {
                    self.operator.slash_selected = selected.saturating_sub(1);
                    return DashboardAction::Redraw;
                }
                KeyCode::Down => {
                    self.operator.slash_selected =
                        (selected + 1).min(slash_specs.len().saturating_sub(1));
                    return DashboardAction::Redraw;
                }
                KeyCode::Tab => {
                    let completed = slash::completion_text(slash_specs[selected]);
                    if let Some(chat) = self.operator.chat.as_mut() {
                        chat.draft = completed.clone();
                        chat.cursor = TextCursor::default();
                        chat.notice = None;
                    }
                    self.operator.slash_selected = 0;
                    self.operator.slash_dismissed_for = Some(completed);
                    return DashboardAction::Redraw;
                }
                KeyCode::Enter => {
                    return match slash_specs[selected].kind {
                        slash::SlashCommandKind::Copy => self.operator_run_copy("", false),
                        slash::SlashCommandKind::Thinking => self.operator_run_thinking("", false),
                        slash::SlashCommandKind::Skills => self.operator_run_skills(false),
                        slash::SlashCommandKind::Model => self.operator_run_model("", false),
                        slash::SlashCommandKind::New => self.operator_run_new("", false),
                        slash::SlashCommandKind::Session => self.operator_run_session("", false),
                        slash::SlashCommandKind::Compact => self.operator_run_compact("", false),
                        slash::SlashCommandKind::Tree => self.operator_run_tree("", false),
                    };
                }
                _ => {}
            }
        }
        if super::input::is_browse_key(key) || key.code == KeyCode::Esc {
            // Back to the terminal panes; the conversation keeps its draft.
            self.operator.editing = false;
            self.operator.visible = false;
            self.operator.clear_overlays();
            self.retarget(super::state::PaneChange::Focus);
            // Ctrl-g is the navigation key: leaving Operator with it also
            // opens the label picker. Esc only leaves.
            if super::input::is_browse_key(key) {
                self.begin_sidebar_jump();
            }
            return DashboardAction::EnterBrowse;
        }
        if !self.operator.transcript_focus
            && let Some(chat) = self.operator.chat.as_mut()
        {
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                if key.kind == KeyEventKind::Press {
                    if chat.draft.is_empty() {
                        self.operator_stop();
                    } else {
                        chat.draft.clear();
                        chat.cursor = TextCursor::default();
                        self.operator_reset_slash_menu();
                    }
                }
                return DashboardAction::Redraw;
            }
            // Ctrl-l and Ctrl-x are commands, never draft text.
            let before = chat.draft.clone();
            if !(key.modifiers == KeyModifiers::CONTROL
                && matches!(key.code, KeyCode::Char('l' | 'x')))
                && chat.cursor.edit_key(&mut chat.draft, key)
            {
                if chat.draft != before {
                    self.operator_reset_slash_menu();
                }
                return DashboardAction::Redraw;
            }
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('l') => {
                    let shown = self.sidebar_preference() != Some(0);
                    self.operator.sidebar_open = !shown;
                    self.sidebar_hidden = shown;
                    self.mouse.hovered = None;
                    self.apply_pane_sizes(self.outer_area);
                }
                // Ctrl-x is Stop in both composer and transcript focus.
                KeyCode::Char('x') => self.operator_stop(),
                _ => {}
            }
            return DashboardAction::Redraw;
        }
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.operator.transcript_focus = !self.operator.transcript_focus;
            let layout = self.operator_layout(self.outer_area);
            if self.operator.transcript_focus
                && let Some(chat) = self.operator.chat.as_mut()
            {
                let rows = transcript_rows(chat, layout.transcript.width, 0);
                let offset = transcript_offset(chat, &rows, layout.transcript.height as usize);
                let visible = &rows[offset.min(rows.len())
                    ..rows.len().min(offset + layout.transcript.height as usize)];
                if !visible.iter().any(|row| {
                    row.entry
                        .is_some_and(|entry| Some(entry.ordinal) == chat.entry_focus)
                }) {
                    chat.entry_focus = visible
                        .iter()
                        .find_map(|row| row.entry.map(|entry| entry.ordinal));
                }
            }
            return DashboardAction::Redraw;
        }
        if self.operator.transcript_focus {
            if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
                self.operator_scroll(if key.code == KeyCode::PageUp { -10 } else { 10 });
                return DashboardAction::Redraw;
            }
            let layout = self.operator_layout(self.outer_area);
            if let Some(chat) = self.operator.chat.as_mut() {
                let height = layout.transcript.height as usize;
                let rows = transcript_rows(chat, layout.transcript.width, 0);
                let offset = transcript_offset(chat, &rows, height);
                let entries: Vec<u64> = rows
                    .iter()
                    .filter_map(|row| row.entry.map(|entry| entry.ordinal))
                    .fold(Vec::new(), |mut entries, ordinal| {
                        if entries.last() != Some(&ordinal) {
                            entries.push(ordinal);
                        }
                        entries
                    });
                let index = entries.iter().position(|id| Some(*id) == chat.entry_focus);
                match key.code {
                    KeyCode::Enter => {
                        if let Some(entry) = rows
                            .iter()
                            .find(|row| {
                                row.entry
                                    .is_some_and(|entry| Some(entry.ordinal) == chat.entry_focus)
                            })
                            .and_then(|row| row.entry)
                        {
                            chat.toggle_entry(entry);
                        }
                    }
                    KeyCode::Home => chat.entry_focus = entries.first().copied(),
                    KeyCode::End => chat.entry_focus = entries.last().copied(),
                    KeyCode::Up => {
                        chat.entry_focus =
                            entries.get(index.unwrap_or(0).saturating_sub(1)).copied()
                    }
                    KeyCode::Down => {
                        chat.entry_focus = entries
                            .get(index.map_or(0, |index| {
                                (index + 1).min(entries.len().saturating_sub(1))
                            }))
                            .copied()
                    }
                    _ => {}
                }
                if matches!(
                    key.code,
                    KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End
                ) && let Some(target) = rows.iter().position(|row| {
                    row.entry
                        .is_some_and(|entry| Some(entry.ordinal) == chat.entry_focus)
                }) && (target < offset || target >= offset + height)
                {
                    chat.follow = false;
                    chat.anchor = rows[target].anchor;
                    chat.scroll = rows.len().saturating_sub(height).saturating_sub(target);
                }
            }
            return DashboardAction::Redraw;
        }
        if self.operator.chat.is_none() {
            return DashboardAction::Redraw;
        }
        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.operator_insert("\n")
            }
            KeyCode::Enter => {
                if let Some(draft) = self.operator.chat.as_ref().map(|c| c.draft.clone())
                    && let slash::ParsedDraft::Slash { name, args } = slash::parse_draft(&draft)
                    && let Some(spec) = slash::find_command(name)
                {
                    return match spec.kind {
                        slash::SlashCommandKind::Copy => self.operator_run_copy(args, false),
                        slash::SlashCommandKind::Thinking => {
                            self.operator_run_thinking(args, false)
                        }
                        slash::SlashCommandKind::Skills => self.operator_run_skills(false),
                        slash::SlashCommandKind::Model => self.operator_run_model(args, false),
                        slash::SlashCommandKind::New => self.operator_run_new(args, false),
                        slash::SlashCommandKind::Session => self.operator_run_session(args, false),
                        slash::SlashCommandKind::Compact => self.operator_run_compact(args, false),
                        slash::SlashCommandKind::Tree => self.operator_run_tree(args, false),
                    };
                }
                self.operator_submit();
            }
            KeyCode::Up | KeyCode::Down => {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.cursor.vertical(&chat.draft, key.code == KeyCode::Down);
                }
            }
            KeyCode::PageUp => self.operator_scroll(-10),
            KeyCode::PageDown => self.operator_scroll(10),
            KeyCode::Char(ch) if !ch.is_control() => self.operator_insert(&ch.to_string()),
            code => {
                let mut changed = false;
                if let Some(chat) = self.operator.chat.as_mut() {
                    let before = chat.draft.clone();
                    chat.cursor.key(&mut chat.draft, code);
                    changed = chat.draft != before;
                }
                if changed {
                    self.operator_reset_slash_menu();
                }
            }
        }
        DashboardAction::Redraw
    }
    pub(super) fn operator_is_editing(&self) -> bool {
        self.operator.visible && self.operator.editing
    }
    fn operator_insert(&mut self, text: &str) {
        let mut changed = false;
        if let Some(chat) = self.operator.chat.as_mut() {
            // Match the protocol's user-text bound without splitting UTF-8.
            if !text.is_empty()
                && chat.draft.len().saturating_add(text.len()) <= MAX_OPERATOR_TEXT_BYTES
            {
                chat.cursor.insert(&mut chat.draft, text);
                changed = true;
            }
        }
        if changed {
            self.operator_reset_slash_menu();
        }
    }
    pub(super) fn operator_paste(&mut self, text: &str) -> DashboardAction {
        self.copy_notice = None;
        if let Some(OperatorOverlay::Skills(picker)) = &mut self.operator.overlay {
            if paste_modal_line(
                &mut picker.query,
                &mut picker.cursor,
                text,
                MAX_MODEL_FILTER_BYTES,
            ) {
                picker.selected = 0;
            }
            return DashboardAction::Redraw;
        }
        if let Some(picker) = self.operator.model_picker_mut() {
            if paste_modal_line(
                &mut picker.query,
                &mut picker.cursor,
                text,
                MAX_MODEL_FILTER_BYTES,
            ) {
                let active = self.settings.operator.model.clone();
                picker.on_query_changed(active.as_deref());
            }
            return DashboardAction::Redraw;
        }
        if let Some(modal) = self.operator.tree_modal_mut() {
            if let TreeModal::Loaded(picker) = modal {
                match &mut picker.step {
                    TreePickerStep::Browse => {
                        if paste_modal_line(
                            &mut picker.query,
                            &mut picker.cursor,
                            text,
                            MAX_TREE_SEARCH_BYTES,
                        ) {
                            picker.on_query_changed();
                        }
                        return DashboardAction::Redraw;
                    }
                    TreePickerStep::CustomInstructions { input, cursor, .. } => {
                        if paste_modal_line(input, cursor, text, MAX_OPERATOR_TEXT_BYTES) {
                            picker.error = None;
                        }
                        return DashboardAction::Redraw;
                    }
                    TreePickerStep::ConfirmSummarize { .. } => return DashboardAction::None,
                }
            }
            return DashboardAction::None;
        }
        if self.operator.overlay.is_some() {
            return DashboardAction::None;
        }
        if self.operator.transcript_focus {
            return DashboardAction::None;
        }
        self.operator_insert(&paste_text(text));
        DashboardAction::Redraw
    }
    fn operator_scroll(&mut self, delta: isize) {
        let layout = self.operator_layout(self.outer_area);
        let height = layout.transcript.height as usize;
        if let Some(chat) = self.operator.chat.as_mut() {
            let rows = transcript_rows(chat, layout.transcript.width, 0);
            let offset = transcript_offset(chat, &rows, height);
            let target = if delta < 0 {
                chat.follow = false;
                chat.scroll = chat.scroll.saturating_add(delta.unsigned_abs());
                offset.saturating_sub(delta.unsigned_abs())
            } else {
                let bottom = rows.len().saturating_sub(height);
                let target = offset.saturating_add(delta as usize).min(bottom);
                chat.scroll = bottom.saturating_sub(target);
                chat.follow = target == bottom;
                target
            };
            chat.anchor = rows.get(target).and_then(|row| row.anchor);
        }
    }
    pub(super) fn operator_mouse(
        &mut self,
        mouse: MouseEvent,
        area: Rect,
    ) -> Option<DashboardAction> {
        let _theme = crate::theme::ThemeScope::enter(self.theme_tokens());
        if matches!(self.operator.overlay, Some(OperatorOverlay::Skills(_))) {
            return Some(self.operator_skills_mouse(mouse, area));
        }
        if self.operator.overlay.is_some() {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                self.copy_notice = None;
                self.operator.clear_overlays();
            }
            return Some(DashboardAction::Redraw);
        }
        if let Some(action) = self.operator_sidebar_mouse(mouse, area) {
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                self.copy_notice = None;
            }
            return Some(action);
        }
        if !self.operator.visible || self.operator.chat.is_none() {
            return None;
        }
        let point = ratatui::layout::Position::new(mouse.column, mouse.row);
        let content = content_area(self, area);
        if !content.contains(point) {
            return None;
        }
        if matches!(
            mouse.kind,
            MouseEventKind::Down(_) | MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            self.copy_notice = None;
        }
        let layout = self.operator_layout(area);
        let transcript = layout.transcript;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if !transcript.is_empty() => {
                if self.operator_reasoning_rect(layout.header).contains(point) {
                    self.operator_open_reasoning();
                } else if stop_rect(layout.header).contains(point) && self.operator_stop_available()
                {
                    self.operator_stop();
                } else {
                    self.operator.editing = true;
                    self.operator.sidebar_open = false;
                    self.operator.transcript_focus = transcript.contains(point);
                    if self.operator.transcript_focus
                        && let Some(chat) = self.operator.chat.as_mut()
                    {
                        let rows = transcript_rows(chat, transcript.width, 0);
                        let offset = transcript_offset(chat, &rows, transcript.height as usize);
                        let pad = transcript_pad(rows.len(), transcript.height as usize);
                        let hit = usize::from(mouse.row - transcript.y)
                            .checked_sub(pad)
                            .and_then(|row| rows.get(offset + row))
                            .and_then(|row| row.entry);
                        chat.entry_focus = hit.map(|entry| entry.ordinal);
                        if let Some(entry) = hit {
                            chat.toggle_entry(entry);
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp => self.operator_scroll(-3),
            MouseEventKind::ScrollDown => self.operator_scroll(3),
            _ => {}
        }
        Some(DashboardAction::Redraw)
    }
    pub(super) fn operator_response(&mut self, request_id: u64, response: &Response) -> bool {
        let Some(pending) = self.operator.pending.remove(&request_id) else {
            return false;
        };
        match &pending {
            Pending::ReasoningSetting | Pending::ModelSetting | Pending::Models { .. } => {}
            Pending::Current => {}
            Pending::Open { .. } => self.operator.loading = false,
            Pending::Read { .. } => {
                self.operator.loading = false;
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.reading = false;
                }
            }
            Pending::Submit { .. } | Pending::Compact { .. } => {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.submitting = false;
                }
            }
            Pending::Stop { .. } => {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.stopping = false;
                }
            }
            Pending::NewSession { .. }
            | Pending::SessionInfo { .. }
            | Pending::Tree { .. }
            | Pending::NavigateTree { .. } => {}
        }
        if let Response::Error { message, .. } = response {
            if matches!(pending, Pending::Open { .. }) {
                self.operator.pending_overlay = None;
            }
            if matches!(pending, Pending::SessionInfo { .. })
                && matches!(self.operator.overlay, Some(OperatorOverlay::Session(_)))
            {
                self.operator.overlay = None;
            }
            if matches!(pending, Pending::Tree { .. })
                && matches!(self.operator.overlay, Some(OperatorOverlay::Tree(_)))
            {
                self.operator.overlay = None;
            }
            if matches!(pending, Pending::NavigateTree { .. })
                && let Some(picker) = self.operator.tree_picker_mut()
            {
                picker.submitting = false;
                picker.error = Some(display_text(message));
            }
            if let Pending::Models { intent } = &pending {
                match intent {
                    ModelIntent::ResolveArg {
                        from_quick: Some(generation),
                        ..
                    } => {
                        self.operator_quick_failed(*generation, message);
                        return true;
                    }
                    ModelIntent::OpenPicker
                    | ModelIntent::ResolveArg {
                        from_quick: None, ..
                    } => {
                        if matches!(
                            self.operator.pending_overlay,
                            Some(PendingOperatorOverlay::Model { .. })
                        ) {
                            self.operator.pending_overlay = None;
                        }
                        if let Some(chat) = self.operator.chat.as_mut() {
                            chat.notice = None;
                            chat.error = Some(display_text(message));
                        } else {
                            self.operator.error = Some(display_text(message));
                        }
                        return true;
                    }
                }
            }
            match &pending {
                Pending::Open {
                    origin: OpenOrigin::Quick(submission),
                } => self.operator_quick_failed(submission.generation, message),
                Pending::Open {
                    origin:
                        OpenOrigin::QuickNew { generation }
                        | OpenOrigin::QuickCompact { generation, .. },
                }
                | Pending::Submit {
                    origin: SubmitOrigin::Quick { generation },
                    ..
                }
                | Pending::Compact {
                    origin: SubmitOrigin::Quick { generation },
                    ..
                }
                | Pending::NewSession {
                    origin: SubmitOrigin::Quick { generation },
                    ..
                } => self.operator_quick_failed(*generation, message),
                _ => {}
            }
            let chat_error = matches!(
                pending,
                Pending::Read { .. }
                    | Pending::Submit { .. }
                    | Pending::Compact { .. }
                    | Pending::Stop { .. }
                    | Pending::NewSession { .. }
                    | Pending::SessionInfo { .. }
                    | Pending::Tree { .. }
                    | Pending::NavigateTree { .. }
            );
            if chat_error {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.notice = None;
                    chat.error = Some(display_text(message));
                }
            } else {
                self.operator.error = Some(display_text(message));
            }
            // No automatic retry of a refused mutation or failed read.
            return true;
        }
        match (pending, response) {
            (Pending::ReasoningSetting | Pending::ModelSetting, Response::Ok) => {}
            (Pending::Models { intent }, Response::Operator(OperatorResponse::Models(entries))) => {
                match intent {
                    ModelIntent::OpenPicker => {
                        let notice = entries
                            .is_empty()
                            .then(|| EMPTY_MODEL_CATALOG_NOTICE.to_owned());
                        let picker = ModelPicker::new(
                            entries.clone(),
                            String::new(),
                            self.settings.operator.model.as_deref(),
                        );
                        self.operator_install_model_picker(picker, notice);
                    }
                    ModelIntent::ResolveArg { query, from_quick } => {
                        if let Some(generation) = from_quick {
                            let still_waiting = self.operator.quick.as_ref().is_some_and(|q| {
                                q.generation == generation
                                    && q.phase == QuickPhase::CheckingModels
                                    && q.visible
                            });
                            if !still_waiting {
                                return true;
                            }
                        }
                        if let Some(exact) = entries.iter().find(|entry| entry.full_id() == query) {
                            let exact_id = exact.full_id();
                            self.operator_apply_model_selection(
                                Some(&exact_id),
                                from_quick.is_some(),
                            );
                        } else {
                            if let Some(chat) = self.operator.chat.as_mut() {
                                chat.draft.clear();
                                chat.cursor = TextCursor::default();
                            }
                            self.operator.slash_selected = 0;
                            self.operator.slash_dismissed_for = None;
                            if from_quick.is_some() {
                                self.operator.quick = None;
                                self.open_operator();
                            }
                            let picker = ModelPicker::new(
                                entries.clone(),
                                query.clone(),
                                self.settings.operator.model.as_deref(),
                            );
                            let notice = if entries.is_empty() {
                                Some(EMPTY_MODEL_CATALOG_NOTICE.to_owned())
                            } else if picker.filtered_choices().is_empty() {
                                Some(format!(
                                    "No Operator models match \"{}\".",
                                    display_text(&query)
                                ))
                            } else {
                                None
                            };
                            self.operator_install_model_picker(picker, notice);
                        }
                    }
                }
            }
            (Pending::Current, Response::Operator(OperatorResponse::Current(current))) => {
                if let Some(summary) = current {
                    let context = self.operator.context.clone();
                    if self.operator_attach(summary.clone(), context) {
                        self.operator_read();
                    }
                }
            }
            (Pending::Open { origin }, Response::Operator(OperatorResponse::Opened(summary))) => {
                let context = match &origin {
                    OpenOrigin::Manual
                    | OpenOrigin::QuickNew { .. }
                    | OpenOrigin::QuickCompact { .. } => self.operator.context.clone(),
                    OpenOrigin::Quick(submission) => submission.context.clone(),
                };
                if !self.operator_attach(summary.clone(), context) {
                    self.operator.pending_overlay = None;
                    match origin {
                        OpenOrigin::Manual => self.operator.error = Some(UNAVAILABLE_REASON.into()),
                        OpenOrigin::Quick(submission) => {
                            self.operator_quick_failed(submission.generation, UNAVAILABLE_REASON)
                        }
                        OpenOrigin::QuickNew { generation }
                        | OpenOrigin::QuickCompact { generation, .. } => {
                            self.operator_quick_failed(generation, UNAVAILABLE_REASON)
                        }
                    }
                    return true;
                }
                match &origin {
                    OpenOrigin::Manual => {
                        if let Some(pending_overlay) = self.operator.pending_overlay.take()
                            && self.operator.visible
                        {
                            match pending_overlay {
                                PendingOperatorOverlay::Model { picker, notice } => {
                                    self.operator_install_model_picker(picker, notice);
                                }
                                PendingOperatorOverlay::Skills { text, context } => {
                                    self.operator_install_skills(text, context);
                                }
                                PendingOperatorOverlay::Reasoning => {
                                    self.operator_open_reasoning();
                                }
                                PendingOperatorOverlay::Session => {
                                    self.operator_open_session_info();
                                }
                                PendingOperatorOverlay::Tree => {
                                    self.operator_open_tree();
                                }
                            }
                        }
                        self.operator_read();
                    }
                    OpenOrigin::QuickNew { generation } => {
                        let Some(chat) = self.operator.chat.as_ref() else {
                            return true;
                        };
                        if let Err(reason) = chat.can_reset_session() {
                            self.operator_quick_failed(*generation, reason);
                            return true;
                        }
                        if let Some(quick) = &mut self.operator.quick
                            && quick.generation == *generation
                        {
                            quick.seeded = true;
                            quick.phase = QuickPhase::Submitting;
                        }
                        self.operator_send_new_session(SubmitOrigin::Quick {
                            generation: *generation,
                        });
                    }
                    OpenOrigin::QuickCompact {
                        generation,
                        draft,
                        instructions,
                    } => {
                        let Some(chat) = self.operator.chat.as_mut() else {
                            return true;
                        };
                        if let Err(reason) = chat.can_compact() {
                            self.operator_quick_failed(*generation, reason);
                            return true;
                        }
                        chat.draft = draft.clone();
                        chat.cursor = TextCursor::default();
                        if let Some(quick) = &mut self.operator.quick
                            && quick.generation == *generation
                        {
                            quick.seeded = true;
                            quick.phase = QuickPhase::Submitting;
                        }
                        self.operator_send_compact(
                            instructions.clone(),
                            SubmitOrigin::Quick {
                                generation: *generation,
                            },
                        );
                        self.operator_read();
                    }
                    OpenOrigin::Quick(submission) => {
                        let Some(chat) = self.operator.chat.as_mut() else {
                            return true;
                        };
                        if !chat.can_submit() {
                            self.operator_quick_failed(submission.generation, BUSY_REASON);
                            return true;
                        }
                        // Simulate the captured text on the existing chat so the
                        // ordinary Submit path freezes and clears it exactly once.
                        chat.context = submission.context.clone();
                        chat.draft = submission.text.clone();
                        chat.cursor = TextCursor::default();
                        if let Some(quick) = &mut self.operator.quick
                            && quick.generation == submission.generation
                        {
                            quick.seeded = true;
                            quick.phase = QuickPhase::Submitting;
                        }
                        self.operator_submit_to(SubmitOrigin::Quick {
                            generation: submission.generation,
                        });
                        self.operator_read();
                    }
                }
            }
            (
                Pending::Read { id, after },
                Response::Operator(OperatorResponse::History(history)),
            ) if history.conversation.id == id => {
                self.operator_history(id, after, history);
                self.operator_read();
            }
            (
                Pending::Submit {
                    id,
                    text,
                    session,
                    expected_run,
                    warm,
                    origin,
                },
                Response::Operator(OperatorResponse::Submitted(turn)),
            ) if turn.identity.conversation == id
                && turn.identity.session == session
                && (turn.identity.run == expected_run
                    || (!warm && turn.identity.run.0 == expected_run.0.saturating_add(1)))
                && turn.identity.validate().is_ok() =>
            {
                let clear_draft = match origin {
                    SubmitOrigin::Composer => true,
                    SubmitOrigin::Quick { generation } => self
                        .operator
                        .quick
                        .as_ref()
                        .is_some_and(|quick| quick.generation == generation && quick.seeded),
                };
                if let Some(chat) = self.operator.chat.as_mut() {
                    if clear_draft
                        && chat.summary.run.0 <= turn.identity.run.0
                        && chat.draft == text
                    {
                        chat.draft.clear();
                        chat.cursor = TextCursor::default();
                    }
                    let current = chat.summary.current_turn.as_ref();
                    if chat.summary.run.0 <= turn.identity.run.0
                        && current.is_none_or(|current| {
                            current.identity.run.0 < turn.identity.run.0
                                || (current.identity.run == turn.identity.run
                                    && current.identity.turn.0 < turn.identity.turn.0)
                                || (current.identity == turn.identity
                                    && current.sequence <= turn.sequence
                                    && !chat.summary.worker_stopped)
                        })
                    {
                        chat.summary.run = turn.identity.run;
                        chat.summary.current_turn = Some(turn.clone());
                        chat.summary.worker_stopped = false;
                    }
                    chat.dirty = true;
                    chat.follow = true;
                    chat.scroll = 0;
                    chat.anchor = None;
                }
                if let SubmitOrigin::Quick { generation } = origin
                    && self
                        .operator
                        .quick
                        .as_ref()
                        .is_some_and(|quick| quick.generation == generation)
                {
                    let quick = self.operator.quick.take().unwrap();
                    if quick.visible
                        && self
                            .operator
                            .chat
                            .as_ref()
                            .is_some_and(|c| c.summary.id == id)
                    {
                        // Show the accepted turn; Open is not re-sent.
                        self.operator.visible = true;
                        self.operator.editing = true;
                        self.operator.transcript_focus = false;
                        self.mode = InputMode::Browse;
                        self.retarget(super::state::PaneChange::Focus);
                    }
                }
                self.operator_read();
            }
            (
                Pending::Compact {
                    id,
                    draft,
                    session,
                    expected_run,
                    warm,
                    origin,
                },
                Response::Operator(OperatorResponse::Submitted(turn)),
            ) if turn.identity.conversation == id
                && turn.identity.session == session
                && (turn.identity.run == expected_run
                    || (!warm && turn.identity.run.0 == expected_run.0.saturating_add(1)))
                && turn.identity.validate().is_ok() =>
            {
                if let Some(chat) = self.operator.chat.as_mut() {
                    if chat.summary.run.0 <= turn.identity.run.0
                        && (chat.draft == draft || matches!(origin, SubmitOrigin::Quick { .. }))
                    {
                        chat.draft.clear();
                        chat.cursor = TextCursor::default();
                    }
                    let current = chat.summary.current_turn.as_ref();
                    if chat.summary.run.0 <= turn.identity.run.0
                        && current.is_none_or(|current| {
                            current.identity.run.0 < turn.identity.run.0
                                || (current.identity.run == turn.identity.run
                                    && current.identity.turn.0 < turn.identity.turn.0)
                                || (current.identity == turn.identity
                                    && current.sequence <= turn.sequence
                                    && !chat.summary.worker_stopped)
                        })
                    {
                        chat.summary.run = turn.identity.run;
                        chat.summary.current_turn = Some(turn.clone());
                        chat.summary.worker_stopped = false;
                        chat.compact_turn = Some(turn.identity.turn);
                    }
                    chat.dirty = true;
                    chat.follow = true;
                    chat.scroll = 0;
                    chat.anchor = None;
                }
                self.operator.slash_selected = 0;
                self.operator.slash_dismissed_for = None;
                if let SubmitOrigin::Quick { generation } = origin
                    && self
                        .operator
                        .quick
                        .as_ref()
                        .is_some_and(|quick| quick.generation == generation)
                {
                    let quick = self.operator.quick.take().unwrap();
                    if quick.visible
                        && self
                            .operator
                            .chat
                            .as_ref()
                            .is_some_and(|c| c.summary.id == id)
                    {
                        self.operator.visible = true;
                        self.operator.editing = true;
                        self.operator.transcript_focus = false;
                        self.mode = InputMode::Browse;
                        self.retarget(super::state::PaneChange::Focus);
                    }
                }
                self.operator_read();
            }
            (Pending::Stop { identity }, Response::Operator(OperatorResponse::Stopped(turn)))
                if turn.identity == identity =>
            {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.dirty = true;
                    chat.confirmed_tail = None;
                }
                // The response retires authority; only a refreshed summary certifies process stop.
                self.operator_read();
            }
            (
                Pending::NewSession { id, draft, origin },
                Response::Operator(OperatorResponse::Conversation(summary)),
            ) if summary.id == id => {
                let context = self.operator.chat.as_ref().map_or_else(
                    || self.operator.context.clone(),
                    |chat| chat.context.clone(),
                );
                if !self.operator_attach(summary.clone(), context) {
                    if let SubmitOrigin::Quick { generation } = origin {
                        self.operator_quick_failed(generation, UNAVAILABLE_REASON);
                    } else {
                        self.operator.error = Some(UNAVAILABLE_REASON.into());
                    }
                    return true;
                }
                let confirmation = format!(
                    "Started new Operator session (epoch {}).",
                    summary.session_epoch
                );
                if let Some(chat) = self.operator.chat.as_mut() {
                    if matches!(origin, SubmitOrigin::Composer | SubmitOrigin::Quick { .. })
                        && (chat.draft == draft || matches!(origin, SubmitOrigin::Quick { .. }))
                    {
                        chat.draft.clear();
                        chat.cursor = TextCursor::default();
                    }
                    chat.error = None;
                    chat.notice = Some(confirmation.clone());
                }
                self.operator.slash_selected = 0;
                self.operator.slash_dismissed_for = None;
                if let SubmitOrigin::Quick { generation } = origin
                    && self
                        .operator
                        .quick
                        .as_ref()
                        .is_some_and(|quick| quick.generation == generation)
                {
                    self.operator.quick = None;
                    self.desktop.notice = Some(confirmation);
                }
                self.operator_read();
            }
            (
                Pending::SessionInfo { id },
                Response::Operator(OperatorResponse::SessionInfo(info)),
            ) if info.conversation == id => {
                if matches!(self.operator.overlay, Some(OperatorOverlay::Session(_))) {
                    self.operator.overlay =
                        Some(OperatorOverlay::Session(SessionModal::Loaded(info.clone())));
                }
            }
            (Pending::Tree { id }, Response::Operator(OperatorResponse::Tree(tree)))
                if self
                    .operator
                    .chat
                    .as_ref()
                    .is_some_and(|chat| chat.summary.id == id) =>
            {
                if matches!(self.operator.overlay, Some(OperatorOverlay::Tree(_))) {
                    self.operator.overlay = Some(OperatorOverlay::Tree(TreeModal::Loaded(
                        Box::new(TreePicker::new((**tree).clone())),
                    )));
                }
            }
            (
                Pending::NavigateTree { id },
                Response::Operator(OperatorResponse::TreeNavigated(nav)),
            ) if nav.conversation.id == id => {
                self.operator.clear_overlays();
                if let Some(turn) = nav.submitted.clone() {
                    let current_context = self.operator.chat.as_ref().map_or_else(
                        || self.operator.context.clone(),
                        |chat| chat.context.clone(),
                    );
                    if !self.operator_attach(nav.conversation.clone(), current_context) {
                        self.operator.error = Some(UNAVAILABLE_REASON.into());
                        return true;
                    }
                    if let Some(chat) = self.operator.chat.as_mut() {
                        chat.branch_summary_turn = Some(turn.identity.turn);
                        chat.branch_summary_pending = Some(PendingBranchSummaryUi {
                            turn: turn.identity.turn,
                            editor_text: nav.editor_text.clone(),
                            restored_context: nav.restored_context.clone(),
                        });
                        chat.dirty = true;
                        chat.follow = true;
                        chat.scroll = 0;
                        chat.anchor = None;
                        chat.error = None;
                        chat.notice = None;
                    }
                    self.operator.editing = true;
                    self.operator.transcript_focus = false;
                    self.operator.slash_selected = 0;
                    self.operator.slash_dismissed_for = None;
                    self.operator_read();
                } else {
                    let attach_context = nav.restored_context.clone().unwrap_or_else(|| {
                        self.operator.chat.as_ref().map_or_else(
                            || self.operator.context.clone(),
                            |chat| chat.context.clone(),
                        )
                    });
                    if !self.operator_attach(nav.conversation.clone(), attach_context) {
                        self.operator.error = Some(UNAVAILABLE_REASON.into());
                        return true;
                    }
                    if !nav.noop {
                        self.operator
                            .pending
                            .retain(|_, pending| !matches!(pending, Pending::Read { id: pid, .. } if *pid == id));
                        if let Some(chat) = self.operator.chat.as_mut() {
                            chat.messages.clear();
                            chat.ordinal = 0;
                            chat.confirmed_tail = None;
                            chat.reading = false;
                            chat.dirty = true;
                            chat.scroll = 0;
                            chat.anchor = None;
                            chat.expanded.clear();
                            chat.entry_focus = None;
                            chat.follow = true;
                            chat.compact_turn = None;
                            chat.branch_summary_turn = None;
                            chat.branch_summary_pending = None;
                            *chat.rows.borrow_mut() = None;
                            chat.error = None;
                            if let Some(text) = &nav.editor_text {
                                chat.draft = text.clone();
                                chat.cursor = TextCursor::default();
                                if let Some(ctx) = &nav.restored_context {
                                    chat.context = ctx.clone();
                                }
                                chat.notice = Some(
                                    "Rewound Operator branch; prompt restored to composer.".into(),
                                );
                            } else {
                                chat.draft.clear();
                                chat.cursor = TextCursor::default();
                                chat.notice = Some("Switched Operator branch.".into());
                            }
                            if let Some(quick) = self.operator.quick.as_mut()
                                && quick.seeded
                            {
                                quick.text = chat.draft.clone();
                                quick.cursor = chat.cursor;
                                if let Some(ctx) = &nav.restored_context {
                                    quick.context = ctx.clone();
                                }
                            }
                        }
                        self.operator.editing = true;
                        self.operator.transcript_focus = false;
                        self.operator.slash_selected = 0;
                        self.operator.slash_dismissed_for = None;
                        self.operator_read();
                    }
                }
            }
            (pending, _) => {
                match pending {
                    Pending::Open {
                        origin: OpenOrigin::Quick(submission),
                    } => self.operator_quick_failed(submission.generation, UNAVAILABLE_REASON),
                    Pending::Open {
                        origin:
                            OpenOrigin::QuickNew { generation }
                            | OpenOrigin::QuickCompact { generation, .. },
                    }
                    | Pending::Submit {
                        origin: SubmitOrigin::Quick { generation },
                        ..
                    }
                    | Pending::Compact {
                        origin: SubmitOrigin::Quick { generation },
                        ..
                    }
                    | Pending::NewSession {
                        origin: SubmitOrigin::Quick { generation },
                        ..
                    } => self.operator_quick_failed(generation, UNAVAILABLE_REASON),
                    _ => {}
                }
                self.operator.error = Some(UNAVAILABLE_REASON.into());
            }
        }
        true
    }
    /// Adopt an unseeded Quick draft into the singleton the moment it exists, so
    /// the overlay and the full composer edit one shared draft instead of
    /// competing copies. The text and cursor are the user's pending draft and
    /// are never replaced by the empty full draft.
    fn operator_quick_adopt_draft(&mut self) {
        let Some(quick) = self.operator.quick.as_mut() else {
            return;
        };
        if quick.seeded {
            return;
        }
        let Some(chat) = self.operator.chat.as_mut() else {
            return;
        };
        chat.draft = quick.text.clone();
        chat.cursor = quick.cursor;
        quick.seeded = true;
    }
    /// Merge one summary into the singleton, refusing any regression that
    /// could revive older authority. Returns false when the identity differs.
    fn operator_attach(&mut self, summary: Summary, context: OperatorContext) -> bool {
        if summary.current_turn.as_ref().is_some_and(|turn| {
            turn.identity.validate().is_err()
                || turn.identity.conversation != summary.id
                || turn.identity.session != summary.session
                || turn.identity.run.0 > summary.run.0
        }) {
            return false;
        }
        if self.operator.chat.is_none() {
            let workdir = agent_workdir(self);
            let mut chat = Chat::new(summary, context);
            chat.workdir = workdir;
            self.operator.chat = Some(chat);
            self.operator_quick_adopt_draft();
            return true;
        }
        let Some(chat) = self.operator.chat.as_mut() else {
            return false;
        };
        if chat.summary.id != summary.id || chat.summary.session != summary.session {
            return false;
        }
        if summary.session_epoch < chat.summary.session_epoch {
            return false;
        }
        if summary.run.0 < chat.summary.run.0 {
            return false;
        }
        if summary.session_epoch > chat.summary.session_epoch {
            chat.messages.clear();
            chat.ordinal = 0;
            chat.confirmed_tail = Some(0);
            chat.reading = false;
            chat.dirty = true;
            chat.submitting = false;
            chat.stopping = false;
            chat.scroll = 0;
            chat.anchor = None;
            chat.expanded.clear();
            chat.entry_focus = None;
            chat.follow = true;
            chat.unread = None;
            chat.reviewed = None;
            chat.compact_turn = None;
            chat.branch_summary_turn = None;
            chat.branch_summary_pending = None;
            *chat.rows.borrow_mut() = None;
            chat.summary = summary;
            self.operator.clear_overlays();
            self.operator_quick_adopt_draft();
            return true;
        }
        if chat.summary.current_turn.is_some() && summary.current_turn.is_none() {
            return false;
        }
        if let (Some(old), Some(new)) = (&chat.summary.current_turn, &summary.current_turn)
            && (new.identity.run.0 < old.identity.run.0
                || (new.identity.run == old.identity.run
                    && (new.identity.turn.0 < old.identity.turn.0
                        || (new.identity.turn == old.identity.turn
                            && (new.sequence < old.sequence
                                || (chat.summary.worker_stopped && !summary.worker_stopped))))))
        {
            return false;
        }
        if (chat.summary.model != summary.model || chat.summary.reasoning != summary.reasoning)
            && matches!(self.operator.overlay, Some(OperatorOverlay::Reasoning(_)))
        {
            self.operator.overlay = None;
        }
        chat.summary = summary;
        self.operator_quick_adopt_draft();
        true
    }
    fn operator_history(&mut self, id: Id, after: u64, history: &OperatorHistory) {
        // A page still belongs to this journal when a newer summary refused the
        // page's older authority metadata; install only what the guards allow.
        let context = self
            .operator
            .chat
            .as_ref()
            .map(|chat| chat.context.clone())
            .unwrap_or_default();
        self.operator_attach(history.conversation.clone(), context);
        let Some(chat) = self.operator.chat.as_mut() else {
            return;
        };
        if history.conversation.id != id
            || chat.summary.id != id
            || history.conversation.session_epoch != chat.summary.session_epoch
            || chat.ordinal != after
            || history.conversation.session != chat.summary.session
        {
            return;
        }
        let mut previous = after;
        for message in &history.messages {
            if message.ordinal <= previous || message.turn.0 == 0 {
                chat.error = Some("Invalid Operator history cursor or identity.".into());
                return;
            }
            previous = message.ordinal;
        }
        if history
            .next_after
            .is_some_and(|next| next != previous || next <= after)
        {
            chat.error = Some("Invalid Operator history continuation.".into());
            return;
        }
        for message in &history.messages {
            // The row above the composer shows the captured target for the next
            // message; an older journal page never reloads it over that capture.
            match &message.content {
                OperatorMessageContent::Worker(OperatorWorkerEvent::ModelSelected {
                    provider,
                    id,
                    ..
                }) => {
                    chat.summary.model = Some(ovrcr_protocol::OperatorModel {
                        provider: provider.clone(),
                        id: id.clone(),
                    });
                }
                OperatorMessageContent::Worker(
                    OperatorWorkerEvent::Completed { .. } | OperatorWorkerEvent::Failed(_),
                )
                | OperatorMessageContent::Interrupted
                    if chat.reviewed != Some(message.turn) =>
                {
                    chat.unread = Some(message.turn);
                }
                _ => {}
            }
            chat.messages.push(message.clone());
            chat.ordinal = message.ordinal;
        }
        if let Some(turn) = &chat.summary.current_turn
            && turn.state == OperatorTurnState::Complete
            && chat.reviewed != Some(turn.identity.turn)
        {
            chat.unread = Some(turn.identity.turn);
        }
        if let Some(pending_turn) = chat.branch_summary_pending.as_ref().map(|p| p.turn) {
            let summary_state = chat
                .summary
                .current_turn
                .as_ref()
                .filter(|t| t.identity.turn == pending_turn)
                .map(|t| t.state);
            let completed = summary_state == Some(OperatorTurnState::Complete)
                || history.messages.iter().any(|m| {
                    m.turn == pending_turn
                        && matches!(
                            m.content,
                            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed { .. })
                        )
                });
            let aborted_or_failed = matches!(
                summary_state,
                Some(OperatorTurnState::Interrupted | OperatorTurnState::Error)
            ) || history.messages.iter().any(|m| {
                m.turn == pending_turn
                    && matches!(
                        m.content,
                        OperatorMessageContent::Interrupted
                            | OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(_))
                    )
            });
            if completed {
                let pending_ui = chat.branch_summary_pending.take().unwrap();
                chat.branch_summary_turn = None;
                chat.error = None;
                if let Some(text) = pending_ui.editor_text {
                    chat.draft = text;
                    chat.cursor = TextCursor::default();
                    if let Some(ctx) = pending_ui.restored_context.clone() {
                        chat.context = ctx;
                    }
                    chat.notice = Some("Summarized branch; prompt restored to composer.".into());
                } else {
                    chat.draft.clear();
                    chat.cursor = TextCursor::default();
                    chat.notice = Some("Summarized and switched Operator branch.".into());
                }
                if let Some(quick) = self.operator.quick.as_mut()
                    && quick.seeded
                {
                    quick.text = chat.draft.clone();
                    quick.cursor = chat.cursor;
                    if let Some(ctx) = pending_ui.restored_context {
                        quick.context = ctx;
                    }
                }
                if after > 0 {
                    chat.messages.clear();
                    chat.ordinal = 0;
                    chat.confirmed_tail = None;
                    chat.dirty = true;
                    chat.scroll = 0;
                    chat.anchor = None;
                    chat.expanded.clear();
                    chat.entry_focus = None;
                    chat.follow = true;
                    *chat.rows.borrow_mut() = None;
                    return;
                }
            } else if aborted_or_failed {
                chat.branch_summary_pending = None;
                chat.branch_summary_turn = None;
            }
        }
        if history.next_after.is_some() {
            chat.dirty = true;
        } else if !chat.dirty {
            chat.confirmed_tail = Some(chat.ordinal);
        }
    }
    pub(super) fn operator_commit_presented(&mut self) {
        if !self.operator.visible
            || !self.operator.editing
            || self.operator.transcript_focus
            || self.operator_slash_menu_open()
            || self.operator.overlay.is_some()
        {
            return;
        }
        let layout = self.operator_layout(self.outer_area);
        let activity_rows = u16::from(
            self.operator
                .chat
                .as_ref()
                .and_then(conversation_activity)
                .is_some(),
        );
        if self.outer_area.width < 60
            || self.outer_area.height < 16
            || layout.transcript.height < 3 + activity_rows
            || layout.transcript.width < 6
            || (self.outer_area.width < 100 && self.operator.sidebar_open)
        {
            return;
        }
        if let Some(chat) = self.operator.chat.as_mut()
            && !chat.messages.is_empty()
            && chat.follow
            && !chat.reading
            && !chat.dirty
            && chat.confirmed_tail == Some(chat.ordinal)
        {
            // A drawn page is reviewable only after the durable tail has been confirmed.
            chat.reviewed = chat.unread.take().or(chat.reviewed);
        }
    }
    pub(super) fn draw_operator(&self, frame: &mut Frame<'_>, now_unix_ms: u64) {
        if !self.operator.visible {
            return;
        }
        let area = content_area(self, frame.area());
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default().style(
                Style::default()
                    .bg(super::render::BASE())
                    .fg(super::render::TEXT()),
            ),
            area,
        );
        if frame.area().width < 60 || frame.area().height < 16 {
            frame.render_widget(
                Paragraph::new(format!("Operator needs 60×16 · current {}×{}\nResize to continue · Esc / Ctrl-g Browse", frame.area().width, frame.area().height))
                    .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        if area.is_empty() {
            return;
        }
        let Some(chat) = &self.operator.chat else {
            let (text, failed) = match &self.operator.error {
                Some(error) => (
                    format!("Operator unavailable\n\n{}", display_text(error)),
                    true,
                ),
                None if self.operator.loading => ("Opening Operator…".into(), false),
                None => (
                    "Operator is not available yet; run `ovrcr operator setup` if pi is not configured.\nReopen with o."
                        .into(),
                    false,
                ),
            };
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .style(Style::default().fg(if failed {
                        super::render::RED()
                    } else {
                        super::render::SUBTEXT()
                    })),
                area,
            );
            return;
        };
        self.draw_operator_conversation(frame, chat, now_unix_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::Event;
    use ovrcr_protocol::{OperatorModel, OperatorTurn, ServerEvent, ServerMessage, TerminalSize};
    use ratatui::{Terminal, backend::TestBackend};

    fn summary() -> Summary {
        Summary {
            reasoning: None,
            run: SessionRunId(1),
            worker_stopped: true,
            worker_live: false,
            id: Id(1),
            session: SessionId(11),
            session_epoch: 0,
            model: None,
            title: "Operator".into(),
            current_turn: None,
            current_turn_started_unix_ms: None,
            last_reply_unix_ms: None,
        }
    }
    fn with_model(mut value: Summary) -> Summary {
        value.model = Some(OperatorModel {
            provider: "openai".into(),
            id: "gpt-6.1-sol".into(),
        });
        value
    }
    fn turn(run: u64, state: OperatorTurnState) -> OperatorTurn {
        turn_for(run, run, state)
    }
    fn turn_for(run: u64, turn: u64, state: OperatorTurnState) -> OperatorTurn {
        OperatorTurn {
            identity: OperatorTurnIdentity {
                conversation: Id(1),
                turn: OperatorTurnId(turn),
                session: SessionId(11),
                run: SessionRunId(run),
            },
            state,
            sequence: 1,
        }
    }
    fn busy() -> Summary {
        let mut value = summary();
        value.run = SessionRunId(2);
        value.worker_stopped = false;
        value.current_turn = Some(turn(2, OperatorTurnState::Running));
        value
    }
    fn stop_unconfirmed() -> Summary {
        let mut value = summary();
        value.worker_stopped = false;
        value.current_turn = Some(turn(1, OperatorTurnState::Complete));
        value
    }
    fn running_turn() -> Summary {
        let mut value = summary();
        value.worker_stopped = false;
        value.current_turn = Some(turn(1, OperatorTurnState::Running));
        value
    }
    fn dashboard() -> Dashboard {
        Dashboard::new(TerminalSize {
            rows: 24,
            cols: 140,
        })
    }
    fn operator_requests(messages: &[ClientMessage]) -> Vec<&OperatorRequest> {
        messages
            .iter()
            .filter_map(|message| match &message.request {
                Request::Operator(request) => Some(request),
                _ => None,
            })
            .collect()
    }
    fn open(d: &mut Dashboard) -> ClientMessage {
        d.open_operator();
        d.drain_outbox()
            .into_iter()
            .find(|message| matches!(message.request, Request::Operator(OperatorRequest::Open)))
            .expect("the full view sends Open")
    }
    fn respond(
        d: &mut Dashboard,
        request_id: u64,
        response: OperatorResponse,
    ) -> Vec<ClientMessage> {
        d.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Operator(response),
        })
    }
    fn opened(d: &mut Dashboard, value: Summary) -> Vec<ClientMessage> {
        let request = open(d);
        respond(d, request.request_id, OperatorResponse::Opened(value))
    }
    fn read_request(messages: &[ClientMessage]) -> ClientMessage {
        messages
            .iter()
            .find(|message| {
                matches!(
                    message.request,
                    Request::Operator(OperatorRequest::Read { .. })
                )
            })
            .cloned()
            .expect("a dirty journal is read")
    }
    fn history_of(
        value: Summary,
        messages: Vec<OperatorMessage>,
        next_after: Option<u64>,
    ) -> OperatorResponse {
        OperatorResponse::History(OperatorHistory {
            conversation: value,
            messages,
            next_after,
        })
    }
    fn message(ordinal: u64, content: OperatorMessageContent) -> OperatorMessage {
        OperatorMessage {
            ordinal,
            turn: OperatorTurnId(1),
            content,
        }
    }
    fn delta(ordinal: u64, text: &str) -> OperatorMessage {
        message(
            ordinal,
            OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text.into())),
        )
    }
    fn completed(ordinal: u64) -> OperatorMessage {
        message(
            ordinal,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                leased: false,
                pi_first_entry_id: None,
                pi_leaf_entry_id: None,
            }),
        )
    }
    fn draw(d: &mut Dashboard, cols: u16, rows: u16) -> String {
        d.install_area(Rect::new(0, 0, cols, rows));
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
        d.draw(&mut terminal).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }
    fn frame_lines(d: &mut Dashboard, cols: u16, rows: u16) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        frame_lines_at(d, cols, rows, now)
    }
    fn frame_lines_at(d: &mut Dashboard, cols: u16, rows: u16, now: u64) -> String {
        d.install_area(Rect::new(0, 0, cols, rows));
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
        terminal
            .draw(|frame| super::super::draw_dashboard_at(frame, d, now))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..rows)
            .map(|row| {
                (0..cols)
                    .map(|col| buffer[(col, row)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }
    fn request(ordinal: u64, text: &str) -> OperatorMessage {
        message(
            ordinal,
            OperatorMessageContent::User {
                skill: None,
                text: text.into(),
                context: OperatorContext::default(),
            },
        )
    }
    fn thinking(ordinal: u64, text: &str) -> OperatorMessage {
        message(
            ordinal,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(text.into())),
        )
    }
    fn call(ordinal: u64, id: &str, command: &str) -> OperatorMessage {
        message(
            ordinal,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                id: id.into(),
                name: "bash".into(),
                arguments: serde_json::json!({"command": command, "timeout": 30}).to_string(),
            }),
        )
    }
    fn output(ordinal: u64, id: &str, text: &str, status: OperatorToolStatus) -> OperatorMessage {
        message(
            ordinal,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                id: id.into(),
                output: text.into(),
                truncated: false,
                status,
            }),
        )
    }
    fn on_turn(turn: u64, mut message: OperatorMessage) -> OperatorMessage {
        message.turn = OperatorTurnId(turn);
        message
    }
    /// Two finished requests: one that changed something, one whose command failed.
    fn two_requests() -> Vec<OperatorMessage> {
        let mut messages = vec![
            request(1, "create the workspace"),
            thinking(2, "SECRET_PLAN"),
            call(
                3,
                "a",
                "cd /repo && \"$OVRCR_EXECUTABLE\" --json workspace list",
            ),
            output(4, "a", "[]", OperatorToolStatus::Succeeded),
            call(
                5,
                "b",
                "\"$OVRCR_EXECUTABLE\" workspace create ovrcr issue-1",
            ),
            output(6, "b", "CREATED_OUTPUT", OperatorToolStatus::Succeeded),
            delta(7, "Created it."),
            completed(8),
        ];
        messages.extend(
            [
                request(9, "now remove it"),
                call(10, "c", "ovrcr workspace remove issue-1 --force"),
                output(
                    11,
                    "c",
                    "REMOVE_OUTPUT",
                    OperatorToolStatus::Failed { exit_code: Some(2) },
                ),
                delta(12, "It failed."),
                completed(13),
            ]
            .map(|message| on_turn(2, message)),
        );
        messages
    }
    fn feed(chat: &Chat, width: u16) -> Vec<String> {
        transcript_rows(chat, width, 0)
            .iter()
            .map(|row| row.line.to_string().trim_end().to_owned())
            .collect()
    }
    fn loaded(value: Summary, messages: Vec<OperatorMessage>) -> Dashboard {
        let mut d = dashboard();
        let out = opened(&mut d, value.clone());
        let read = read_request(&out);
        respond(&mut d, read.request_id, history_of(value, messages, None));
        d
    }
    fn click(d: &mut Dashboard, column: u16, row: u16) {
        d.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            d.outer_area,
        );
    }

    #[test]
    fn operator_section_opens_the_singleton_directly() {
        if super::super::pi_available_test::run_with_pi_available(
            "dashboard::operator::tests::operator_section_opens_the_singleton_directly",
        ) {
            return;
        }
        let mut d = dashboard();
        d.install_area(Rect::new(0, 0, 140, 40));
        let rects = d.sidebar_rects(d.outer_area);
        let row = rects.operator;
        assert_eq!(
            rects.attention.height, 0,
            "no counts means no attention row"
        );
        assert_eq!(
            row.height, 4,
            "the tile has top border, title, status and bottom border"
        );
        assert_eq!(row.y, 1, "the section starts below the title");
        assert_eq!(
            rects.tree.y,
            row.bottom() + 1,
            "one blank row follows Operator"
        );
        let screen = draw(&mut d, 140, 24);
        assert!(screen.contains("Operator"), "{screen}");
        for removed in ["[+]", "▸", "[x]", "Chats", "New Conversation"] {
            assert!(
                !screen.contains(removed),
                "no conversation list or creation controls: {removed}"
            );
        }
        click(&mut d, row.x + 1, row.y);
        assert!(d.operator.visible);
        let requests = d.drain_outbox();
        assert_eq!(operator_requests(&requests), [&OperatorRequest::Open]);
        assert_eq!(
            d.operator
                .pending
                .values()
                .filter(|pending| matches!(pending, Pending::Open { .. }))
                .count(),
            1,
            "the row click sent exactly one Open"
        );
        // Ctrl-n is not a shortcut: it neither creates a conversation nor edits text.
        d.key(KeyCode::Char('o'));
        let before = d.operator.chat.as_ref().map(|chat| chat.draft.clone());
        assert!(before.is_none());
        d.ctrl('n');
        assert!(d.drain_outbox().is_empty());
        assert!(d.operator.chat.is_none());
    }

    #[test]
    fn startup_current_creates_nothing_and_repeated_open_keeps_state() {
        let mut d = dashboard();
        d.operator_start_current();
        let current = d.drain_outbox();
        assert_eq!(operator_requests(&current), [&OperatorRequest::Current]);
        let out = respond(
            &mut d,
            current[0].request_id,
            OperatorResponse::Current(None),
        );
        assert!(out.is_empty());
        assert!(d.operator.chat.is_none());
        assert!(d.drain_outbox().is_empty());

        let first = open(&mut d);
        let out = respond(
            &mut d,
            first.request_id,
            OperatorResponse::Opened(summary()),
        );
        let read = read_request(&out);
        assert_eq!(read.request_id, out[0].request_id);
        respond(
            &mut d,
            read.request_id,
            history_of(summary(), Vec::new(), None),
        );
        assert_eq!(d.operator.chat.as_ref().unwrap().summary.id, Id(1));
        d.operator.chat.as_mut().unwrap().draft = "keep me".into();

        let again = open(&mut d);
        let out = respond(
            &mut d,
            again.request_id,
            OperatorResponse::Opened(summary()),
        );
        assert!(out.is_empty(), "a repeated Open resets and reloads nothing");
        let chat = d.operator.chat.as_ref().unwrap();
        assert_eq!(
            chat.summary.run,
            SessionRunId(1),
            "repeated Open resets nothing"
        );
        assert_eq!(chat.draft, "keep me", "the draft survives reopening");
        assert!(d.operator.visible && d.operator.editing && !d.operator.transcript_focus);
    }

    #[test]
    fn header_shows_pi_and_the_selected_model_only_once_known() {
        let mut d = dashboard();
        d.config_dir = std::path::PathBuf::from("/instance");
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        let context = OperatorContext {
            project: Some("proj".into()),
            workspace: Some("work".into()),
            cwd: Some("/captured/cwd".into()),
            session: Some(SessionId(7)),
            run: Some(SessionRunId(3)),
            ..Default::default()
        };
        let out = respond(
            &mut d,
            read.request_id,
            history_of(
                summary(),
                vec![
                    message(
                        1,
                        OperatorMessageContent::User {
                            skill: None,
                            text: "Review this.".into(),
                            context,
                        },
                    ),
                    message(
                        2,
                        OperatorMessageContent::Worker(OperatorWorkerEvent::model_selected(
                            OperatorModel {
                                provider: "openai".into(),
                                id: "gpt-6.1-sol".into(),
                            },
                            None,
                        )),
                    ),
                    delta(3, "Inspecting."),
                ],
                None,
            ),
        );
        assert!(out.is_empty());
        let screen = draw(&mut d, 140, 24);
        assert!(
            screen.contains("Operator · pi · openai/gpt-6.1-sol · ready"),
            "{screen}"
        );
        assert!(
            screen.contains("→ proj / work"),
            "the journal keeps each message's captured context: {screen}"
        );
        assert!(
            screen.contains("Managing global"),
            "the composer names the next message's target, not older history: {screen}"
        );
        assert!(
            !screen.contains("Managing proj / work"),
            "a loaded message never retargets the next Send: {screen}"
        );
        assert!(
            !screen.contains("ModelSelected"),
            "the selected model is metadata, not a journal row"
        );
        let chat = d.operator.chat.as_mut().unwrap();
        chat.expanded.insert(1 | feed::CONTEXT_KEY, true);
        let screen = draw(&mut d, 140, 24);
        assert!(screen.contains("captured cwd: /captured/cwd"), "{screen}");
        assert!(
            screen.contains("agent workdir: /instance/operator-workdir"),
            "{screen}"
        );
    }

    #[test]
    fn unknown_model_placeholder_and_setup_errors_never_show_a_bogus_identifier() {
        let mut d = dashboard();
        let mut placeholder = summary();
        placeholder.model = Some(OperatorModel {
            provider: "unknown".into(),
            id: "unknown".into(),
        });
        opened(&mut d, placeholder);
        let screen = draw(&mut d, 140, 24);
        assert!(screen.contains("Operator · pi · ready"), "{screen}");
        assert!(
            !screen.contains("unknown"),
            "the placeholder stays hidden: {screen}"
        );

        // The worker's fixed setup failures are shown verbatim, with their command.
        let mut d = dashboard();
        let open = open(&mut d);
        respond(&mut d, open.request_id, OperatorResponse::Opened(summary()));
        d.operator.error = Some("No Pi model selected; run ovrcr operator setup.".into());
        let screen = draw(&mut d, 140, 24);
        assert!(
            screen.contains("No Pi model selected; run ovrcr operator setup."),
            "{screen}"
        );
        // Journal ordinals are never reused, so each failure gets its own.
        for (ordinal, failure) in [
            (
                1,
                "Configured Operator model is unavailable in the dedicated Pi profile.",
            ),
            (
                2,
                "Pi is unavailable; install pi or set OVRCR_PI_EXECUTABLE.",
            ),
        ] {
            let chat = d.operator.chat.as_mut().unwrap();
            chat.messages = vec![message(
                ordinal,
                OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(failure.into())),
            )];
            chat.dirty = false;
            chat.confirmed_tail = Some(ordinal);
            let screen = draw(&mut d, 140, 24);
            assert!(screen.contains(failure), "{screen}");
        }
    }

    #[test]
    fn named_skill_submission_from_both_composers_is_typed_and_keeps_context() {
        for quick in [false, true] {
            let mut d = dashboard();
            opened(&mut d, summary());
            d.drain_outbox();
            if quick {
                d.ctrl('o');
            }
            d.event_action(Event::Paste(
                "/skill:ovrcr-status inspect this target".into(),
            ));
            let context = if quick {
                d.operator.quick.as_ref().unwrap().context.clone()
            } else {
                d.operator.chat.as_ref().unwrap().context.clone()
            };
            d.key(KeyCode::Enter);
            let out = d.drain_outbox();
            let requests = operator_requests(&out);
            assert_eq!(requests.len(), 1, "named Send must submit, quick={quick}");
            let value = serde_json::to_value(requests[0]).unwrap();
            assert_eq!(value["Submit"]["skill"]["name"], "ovrcr-status");
            assert_eq!(value["Submit"]["skill"]["revision"], 1);
            assert_eq!(value["Submit"]["text"], "inspect this target");
            assert_eq!(
                value["Submit"]["context"],
                serde_json::to_value(context).unwrap()
            );
        }
    }

    #[test]
    fn named_skill_refusals_keep_the_draft_and_bare_history_names_the_procedure() {
        for quick in [false, true] {
            for draft in ["/skill:missing keep me", "/skill:../ovrcr-status keep me"] {
                let mut d = dashboard();
                opened(&mut d, summary());
                d.drain_outbox();
                if quick {
                    d.ctrl('o');
                }
                d.event_action(Event::Paste(draft.into()));
                d.key(KeyCode::Enter);
                assert!(d.drain_outbox().is_empty(), "{draft}");
                assert!(draw(&mut d, 140, 24).contains("unavailable Operator skill"));
                assert_eq!(d.operator.chat.as_ref().unwrap().draft, draft);
            }
        }
        let mut d = dashboard();
        opened(&mut d, summary());
        let chat = d.operator.chat.as_mut().unwrap();
        chat.messages = vec![message(
            1,
            OperatorMessageContent::User {
                text: String::new(),
                context: OperatorContext::default(),
                skill: Some(
                    ovrcr_protocol::OperatorSkillSelection::named("ovrcr-coordinator").unwrap(),
                ),
            },
        )];
        chat.dirty = false;
        chat.confirmed_tail = Some(1);
        assert!(draw(&mut d, 140, 24).contains("/skill:ovrcr-coordinator"));
    }

    #[test]
    fn header_without_a_model_never_guesses_one() {
        let mut d = dashboard();
        opened(&mut d, summary());
        let screen = draw(&mut d, 140, 24);
        assert!(screen.contains("Operator · pi"), "{screen}");
        assert!(!screen.contains("openai"), "no invented model identifier");
        assert!(
            !screen.contains("gpt-6.1-sol"),
            "no invented model identifier"
        );
    }

    #[test]
    fn quick_creates_nothing_when_cancelled_and_shares_the_singleton() {
        let mut d = dashboard();
        d.ctrl('o');
        d.event_action(Event::Paste("abandoned".into()));
        d.key(KeyCode::Esc);
        assert!(
            d.operator.quick.is_none(),
            "cancelled unsent Quick is discarded"
        );
        assert!(d.drain_outbox().is_empty());
        assert!(d.operator.chat.is_none());
        assert!(!d.operator.visible);

        d.ctrl('o');
        d.event_action(Event::Paste("first message".into()));
        d.key(KeyCode::Enter);
        let out = d.drain_outbox();
        assert_eq!(
            operator_requests(&out),
            [&OperatorRequest::Open],
            "the first explicit Send opens the single conversation"
        );
        let out = respond(
            &mut d,
            out[0].request_id,
            OperatorResponse::Opened(summary()),
        );
        let submits = out
            .iter()
            .filter(|message| {
                matches!(
                    &message.request,
                    Request::Operator(OperatorRequest::Submit {
                        skill: None,
                        conversation: Id(1),
                        expected_run: SessionRunId(1),
                        text,
                        ..
                    }) if text == "first message"
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(submits.len(), 1);
        assert!(matches!(
            &out[0].request,
            Request::Operator(OperatorRequest::Submit { .. })
        ));
        // Acceptance reuses the singleton, shows the accepted turn and clears only that draft.
        respond(
            &mut d,
            submits[0].request_id,
            OperatorResponse::Submitted(turn(2, OperatorTurnState::Running)),
        );
        assert!(d.operator.visible);
        assert!(d.operator.quick.is_none());
        assert_eq!(d.operator.chat.as_ref().unwrap().draft, "");
        assert_eq!(
            d.operator
                .chat
                .as_ref()
                .unwrap()
                .summary
                .current_turn
                .as_ref()
                .unwrap()
                .state,
            OperatorTurnState::Running
        );
        assert_eq!(
            d.operator
                .pending
                .values()
                .filter(|pending| matches!(pending, Pending::Read { .. }))
                .count(),
            1,
            "the in-flight read is the only catch-up"
        );
    }

    #[test]
    fn quick_draft_survives_startup_attach_and_a_busy_refusal() {
        let mut d = dashboard();
        d.operator_start_current();
        let current = d.drain_outbox();
        assert_eq!(operator_requests(&current), [&OperatorRequest::Current]);
        // Quick can open while the startup `Current` is still unanswered, so its
        // text exists only in the overlay until the singleton resolves.
        d.ctrl('o');
        d.event_action(Event::Paste("kept text".into()));
        let out = respond(
            &mut d,
            current[0].request_id,
            OperatorResponse::Current(Some(busy())),
        );
        assert!(
            out.iter().any(|message| matches!(
                message.request,
                Request::Operator(OperatorRequest::Read { .. })
            )),
            "attach reads the singleton journal: {out:?}"
        );
        assert_eq!(
            d.operator.chat.as_ref().unwrap().draft,
            "kept text",
            "the overlay draft is adopted into the one shared draft"
        );
        // The attached conversation is busy, so Send refuses and keeps the draft.
        d.key(KeyCode::Enter);
        assert!(d.drain_outbox().is_empty(), "nothing is queued or created");
        let quick = d.operator.quick.as_ref().unwrap();
        assert_eq!(quick.text, "kept text");
        assert_eq!(quick.error.as_deref(), Some(BUSY_REASON));
        // Dismissing and reopening must not replace it with an empty full draft.
        d.key(KeyCode::Esc);
        assert!(!d.operator_quick_visible());
        d.ctrl('o');
        assert_eq!(d.operator.quick.as_ref().unwrap().text, "kept text");
        assert_eq!(d.operator.chat.as_ref().unwrap().draft, "kept text");
        let screen = draw(&mut d, 140, 24);
        assert!(screen.contains("kept text"), "{screen}");
        assert!(screen.contains(BUSY_REASON), "{screen}");
        // The adopted cursor continues the draft where the user left it.
        d.key(KeyCode::Backspace);
        assert_eq!(d.operator.quick.as_ref().unwrap().text, "kept tex");
        assert_eq!(d.operator.chat.as_ref().unwrap().draft, "kept tex");
    }

    #[test]
    fn async_wakeup_attach_preserves_composer_and_quick_drafts_while_busy() {
        for quick_origin in [false, true] {
            let mut leased = summary();
            leased.worker_stopped = false;
            leased.worker_live = true;
            leased.current_turn = Some(turn_for(1, 1, OperatorTurnState::Complete));
            let mut d = dashboard();
            let out = opened(&mut d, leased.clone());
            let read = read_request(&out);
            respond(
                &mut d,
                read.request_id,
                history_of(leased.clone(), Vec::new(), None),
            );

            if quick_origin {
                d.ctrl('o');
                d.event_action(Event::Paste("keep this draft".into()));
            } else {
                d.event_action(Event::Paste("keep this draft".into()));
            }
            let mut async_running = leased;
            async_running.current_turn = Some(turn_for(1, 2, OperatorTurnState::Running));
            d.operator_changed(Id(1));
            let changed = d.drain_outbox();
            let read = read_request(&changed);
            let wakeup = OperatorMessage {
                ordinal: 1,
                turn: OperatorTurnId(2),
                content: OperatorMessageContent::AsyncWakeup {
                    text: "A watched session settled.".into(),
                },
            };
            respond(
                &mut d,
                read.request_id,
                history_of(async_running, vec![wakeup], None),
            );

            let chat = d.operator.chat.as_ref().unwrap();
            assert_eq!(chat.draft, "keep this draft");
            if quick_origin {
                assert_eq!(d.operator.quick.as_ref().unwrap().text, "keep this draft");
            }
            d.key(KeyCode::Enter);
            assert!(
                d.drain_outbox().is_empty(),
                "a running wakeup owns the slot"
            );
            let chat = d.operator.chat.as_ref().unwrap();
            assert_eq!(chat.draft, "keep this draft");
            assert_eq!(chat.error.as_deref(), Some(BUSY_REASON));
            if quick_origin {
                assert_eq!(d.operator.quick.as_ref().unwrap().text, "keep this draft");
                assert_eq!(
                    d.operator.quick.as_ref().unwrap().error.as_deref(),
                    Some(BUSY_REASON)
                );
            }
        }
    }

    #[test]
    fn async_wakeup_is_one_woke_entry_and_terminal_outcomes_mark_one_unread() {
        if super::super::pi_available_test::run_with_pi_available(
            "dashboard::operator::tests::async_wakeup_is_one_woke_entry_and_terminal_outcomes_mark_one_unread",
        ) {
            return;
        }
        for outcome in ["complete", "failed", "interrupted"] {
            let mut d = dashboard();
            let out = opened(&mut d, summary());
            let read = read_request(&out);
            respond(
                &mut d,
                read.request_id,
                history_of(summary(), Vec::new(), None),
            );

            let state = match outcome {
                "complete" => OperatorTurnState::Complete,
                "failed" => OperatorTurnState::Error,
                "interrupted" => OperatorTurnState::Interrupted,
                _ => unreachable!(),
            };
            let mut final_summary = summary();
            final_summary.current_turn = Some(turn_for(1, 2, state));
            let wakeup = OperatorMessage {
                ordinal: 1,
                turn: OperatorTurnId(2),
                content: OperatorMessageContent::AsyncWakeup {
                    text: "WATCH_REPORT_FOR_TRANSCRIPT".into(),
                },
            };
            let answer = OperatorMessage {
                ordinal: 2,
                turn: OperatorTurnId(2),
                content: OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(
                    "WATCH_WAKEUP_ANSWER".into(),
                )),
            };
            let terminal = match outcome {
                "complete" => OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                    leased: false,
                    pi_first_entry_id: None,
                    pi_leaf_entry_id: None,
                }),
                "failed" => OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(
                    "wake up failed".into(),
                )),
                "interrupted" => OperatorMessageContent::Interrupted,
                _ => unreachable!(),
            };
            let finish = OperatorMessage {
                ordinal: 3,
                turn: OperatorTurnId(2),
                content: terminal,
            };
            d.operator_changed(Id(1));
            let changed = d.drain_outbox();
            let read = read_request(&changed);
            respond(
                &mut d,
                read.request_id,
                history_of(final_summary.clone(), vec![wakeup, answer, finish], None),
            );

            let chat = d.operator.chat.as_ref().unwrap();
            assert_eq!(chat.unread, Some(OperatorTurnId(2)));
            let transcript = feed(chat, 140);
            let glyph = match outcome {
                "complete" => '◆',
                "failed" => '✕',
                "interrupted" => '■',
                _ => unreachable!(),
            };
            assert_eq!(
                transcript
                    .iter()
                    .filter(|line| line.contains("Woke ·"))
                    .collect::<Vec<_>>(),
                [&format!("▌ {glyph} Woke · WATCH_REPORT_FOR_TRANSCRIPT")],
                "the wake-up and its answer are one entry"
            );
            assert!(
                transcript
                    .iter()
                    .any(|line| line == "▌   WATCH_WAKEUP_ANSWER"),
                "{transcript:?}"
            );
            assert_eq!(
                transcript
                    .iter()
                    .filter(|line| line.starts_with('▌'))
                    .count(),
                transcript.iter().filter(|line| !line.is_empty()).count(),
                "every row belongs to the wake-up entry: {transcript:?}"
            );

            d.key(KeyCode::Esc);
            let screen = draw(&mut d, 140, 24);
            assert!(
                screen.contains("Reply ready · 1 unread"),
                "one unseen terminal wakeup marks the Operator dock unread: {screen}"
            );
            let reopened = open(&mut d);
            respond(
                &mut d,
                reopened.request_id,
                OperatorResponse::Opened(final_summary),
            );
            let _ = draw(&mut d, 140, 24);
            assert_eq!(
                d.operator.chat.as_ref().unwrap().unread,
                None,
                "drawing the confirmed transcript tail marks this turn reviewed"
            );
        }
    }

    #[test]
    fn busy_or_stop_unconfirmed_send_preserves_the_draft_with_a_reason() {
        for draft in [
            "keep this",
            "/skill:ovrcr-status keep this",
            "/skill:ovrcr-coordinator",
        ] {
            for value in [busy(), stop_unconfirmed()] {
                let mut d = dashboard();
                let out = opened(&mut d, value);
                d.drain_outbox();
                assert!(!d.operator_submit_available());
                d.operator.chat.as_mut().unwrap().draft = draft.into();
                d.key(KeyCode::Enter);
                assert!(d.drain_outbox().is_empty(), "never queues or steers");
                let chat = d.operator.chat.as_ref().unwrap();
                assert_eq!(chat.draft, draft);
                assert_eq!(chat.error.as_deref(), Some(BUSY_REASON));
                assert!(!chat.submitting);

                // Quick shares the same draft and refuses with its own visible reason.
                let _ = out;
                d.ctrl('o');
                d.key(KeyCode::Enter);
                assert!(d.drain_outbox().is_empty());
                let quick = d.operator.quick.as_ref().unwrap();
                assert_eq!(quick.text, draft);
                assert_eq!(quick.error.as_deref(), Some(BUSY_REASON));
                assert_eq!(d.operator.chat.as_ref().unwrap().draft, draft);
            }
        }
    }

    #[test]
    fn late_quick_receipt_after_dismissal_never_steals_the_view() {
        let mut d = dashboard();
        d.ctrl('o');
        d.event_action(Event::Paste("later".into()));
        d.key(KeyCode::Enter);
        let open = d.drain_outbox().remove(0);
        d.key(KeyCode::Esc);
        assert!(!d.operator_quick_visible());
        assert!(!d.operator.visible);
        let out = respond(&mut d, open.request_id, OperatorResponse::Opened(summary()));
        assert!(
            out.iter().any(|message| matches!(
                message.request,
                Request::Operator(OperatorRequest::Submit { .. })
            )),
            "the accepted Send still submits exactly once"
        );
        assert!(
            !d.operator.visible,
            "a dismissed overlay cannot open the view"
        );
        let submit = out
            .iter()
            .find(|message| {
                matches!(
                    message.request,
                    Request::Operator(OperatorRequest::Submit { .. })
                )
            })
            .unwrap();
        respond(
            &mut d,
            submit.request_id,
            OperatorResponse::Submitted(turn(2, OperatorTurnState::Running)),
        );
        assert!(!d.operator.visible, "acceptance cannot steal the view");
        assert!(d.operator.quick.is_none());
    }

    #[test]
    fn wrong_authority_receipts_retain_drafts_and_exact_identity() {
        let mut d = dashboard();
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(summary(), Vec::new(), None),
        );
        d.operator.chat.as_mut().unwrap().draft = "one".into();
        d.key(KeyCode::Enter);
        let submit = d.drain_outbox().remove(0);
        // A receipt for another session/run cannot clear the captured draft.
        let mut wrong = turn(2, OperatorTurnState::Running);
        wrong.identity.session = SessionId(99);
        respond(
            &mut d,
            submit.request_id,
            OperatorResponse::Submitted(wrong),
        );
        let chat = d.operator.chat.as_ref().unwrap();
        assert_eq!(chat.draft, "one");
        assert!(!chat.submitting);
        assert!(chat.summary.current_turn.is_none());
        assert_eq!(d.operator.error.as_deref(), Some(UNAVAILABLE_REASON));
    }

    #[test]
    fn accepted_submission_clears_its_draft_and_stop_proof_gates_the_next_send() {
        let mut d = dashboard();
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(summary(), Vec::new(), None),
        );
        d.operator.chat.as_mut().unwrap().draft = "one".into();
        d.key(KeyCode::Enter);
        let submit = d.drain_outbox().remove(0);
        respond(
            &mut d,
            submit.request_id,
            OperatorResponse::Submitted(turn(2, OperatorTurnState::Running)),
        );
        assert_eq!(d.operator.chat.as_ref().unwrap().draft, "");
        assert!(!d.operator_submit_available());
        // Complete without controlled-stop proof still cannot submit.
        let mut finishing = with_model(summary());
        finishing.run = SessionRunId(2);
        finishing.worker_stopped = false;
        finishing.current_turn = Some(turn(2, OperatorTurnState::Complete));
        d.operator_attach(finishing.clone(), OperatorContext::default());
        d.operator.chat.as_mut().unwrap().draft = "two".into();
        d.key(KeyCode::Enter);
        assert!(d.drain_outbox().is_empty());
        assert_eq!(
            d.operator.chat.as_ref().unwrap().error.as_deref(),
            Some(BUSY_REASON)
        );
        // The refreshed summary carries the exact-run stop proof and unlocks Send.
        let mut certified = finishing;
        certified.worker_stopped = true;
        d.operator_attach(certified, OperatorContext::default());
        d.key(KeyCode::Enter);
        let out = d.drain_outbox();
        assert_eq!(out.len(), 1);
        assert!(matches!(
            out[0].request,
            Request::Operator(OperatorRequest::Submit {
                expected_run: SessionRunId(2),
                ..
            })
        ));
    }

    #[test]
    fn leased_idle_worker_accepts_a_followup_on_the_same_run_without_stop_control() {
        let mut leased = summary();
        leased.worker_stopped = false;
        leased.worker_live = true;
        leased.current_turn = Some(turn_for(1, 1, OperatorTurnState::Complete));
        let mut d = dashboard();
        let out = opened(&mut d, leased);
        d.drain_outbox();
        d.operator.chat.as_mut().unwrap().draft = "follow up".into();
        assert!(d.operator_submit_available());
        let screen = frame_lines(&mut d, 140, 24);
        assert!(!screen.contains("[Stop Ctrl-x]"), "{screen}");

        d.key(KeyCode::Enter);
        let submit = d.drain_outbox().remove(0);
        assert!(matches!(
            submit.request,
            Request::Operator(OperatorRequest::Submit {
                expected_run: SessionRunId(1),
                ..
            })
        ));
        respond(
            &mut d,
            submit.request_id,
            OperatorResponse::Submitted(turn_for(1, 2, OperatorTurnState::Running)),
        );
        assert_eq!(d.operator.chat.as_ref().unwrap().draft, "");
        assert_eq!(
            d.operator
                .chat
                .as_ref()
                .unwrap()
                .summary
                .current_turn
                .as_ref()
                .unwrap()
                .identity,
            turn_for(1, 2, OperatorTurnState::Running).identity
        );
        assert_eq!(
            out.len(),
            1,
            "opening the existing conversation stays read-only"
        );
    }

    #[test]
    fn a_pending_submission_keeps_sidebar_busy_without_claiming_a_server_turn() {
        let mut d = dashboard();
        d.settings.operator.sidebar = true;
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(summary(), Vec::new(), None),
        );
        assert!(!d.operator_sidebar_busy());

        d.event_action(Event::Paste("pending message".into()));
        d.key(KeyCode::Enter);
        assert!(d.operator.chat.as_ref().unwrap().submitting);
        assert!(d.operator_sidebar_busy());
        assert_eq!(d.redraw_interval(), super::super::status::SPINNER_INTERVAL);
        assert!(
            !d.operator_stop_available(),
            "a pending submission has no Server-owned turn to stop"
        );
        assert!(
            !d.operator_submit_available(),
            "a pending submission remains single-flight"
        );
    }

    #[test]
    fn ctrl_x_stops_the_exact_turn_from_composer_and_transcript() {
        let mut d = dashboard();
        opened(&mut d, busy());
        d.drain_outbox();
        d.ctrl('x');
        let out = d.drain_outbox();
        assert_eq!(out.len(), 1);
        assert!(matches!(
            out[0].request,
            Request::Operator(OperatorRequest::Stop { identity })
                if identity == turn(2, OperatorTurnState::Running).identity
        ));
        assert!(d.operator.chat.as_ref().unwrap().stopping);
        // Once stopping, Ctrl-x does not repeat the request.
        d.ctrl('x');
        assert!(d.drain_outbox().is_empty());
        // Transcript focus keeps the same exact-turn Stop.
        let mut d = dashboard();
        opened(&mut d, busy());
        d.drain_outbox();
        d.key(KeyCode::Tab);
        assert!(d.operator.transcript_focus);
        d.ctrl('x');
        let out = d.drain_outbox();
        assert_eq!(out.len(), 1);
        assert!(matches!(
            out[0].request,
            Request::Operator(OperatorRequest::Stop { .. })
        ));
        // A ready conversation has nothing to stop.
        let mut d = dashboard();
        opened(&mut d, summary());
        d.drain_outbox();
        d.ctrl('x');
        assert!(d.drain_outbox().is_empty());
    }

    #[test]
    fn ctrl_c_clears_before_stopping_and_quick_empty_ctrl_c_is_a_noop() {
        let mut d = dashboard();
        opened(&mut d, busy());
        d.drain_outbox();
        d.operator.chat.as_mut().unwrap().draft = "text".into();
        d.ctrl('c');
        assert_eq!(d.operator.chat.as_ref().unwrap().draft, "");
        assert!(d.drain_outbox().is_empty(), "clearing is not a Stop");
        d.ctrl('c');
        let out = d.drain_outbox();
        assert_eq!(out.len(), 1);
        assert!(matches!(
            out[0].request,
            Request::Operator(OperatorRequest::Stop { identity }) if identity == turn(2, OperatorTurnState::Running).identity
        ));
        // Quick's empty Ctrl-c does nothing at all.
        d.ctrl('o');
        d.ctrl('c');
        d.ctrl('c');
        assert!(d.drain_outbox().is_empty());
        assert!(d.operator_quick_visible());
    }

    #[test]
    fn paged_history_scrolls_receipts_and_escapes_controls() {
        let mut d = dashboard();
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        let out = respond(
            &mut d,
            read.request_id,
            history_of(
                summary(),
                vec![
                    message(
                        1,
                        OperatorMessageContent::User {
                            skill: None,
                            text: "run it".into(),
                            context: OperatorContext::default(),
                        },
                    ),
                    message(
                        2,
                        OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                            id: "call-1".into(),
                            name: "bash".into(),
                            arguments: "{\"command\":\"ls\"}".into(),
                        }),
                    ),
                ],
                Some(2),
            ),
        );
        // A paged read marks the journal dirty again and continues from the last ordinal.
        let next = read_request(&out);
        assert!(matches!(
            next.request,
            Request::Operator(OperatorRequest::Read { after: 2, .. })
        ));
        let out = respond(
            &mut d,
            next.request_id,
            history_of(
                summary(),
                vec![
                    message(
                        3,
                        OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                            id: "call-1".into(),
                            output: "a\u{1b}]52;c;INJECTED\u{7}".into(),
                            truncated: false,
                            status: OperatorToolStatus::Succeeded,
                        }),
                    ),
                    completed(4),
                ],
                None,
            ),
        );
        assert!(out.is_empty());
        let chat = d.operator.chat.as_ref().unwrap();
        assert_eq!(chat.ordinal, 4);
        assert_eq!(chat.confirmed_tail, Some(4));
        let screen = draw(&mut d, 140, 24);
        assert!(!screen.contains("Team"));
        assert!(screen.contains("run it"), "{screen}");
        assert!(screen.contains("▌ ✓ run it"), "{screen}");
        assert!(
            !screen.contains("INJECTED"),
            "a command row does not show its output: {screen}"
        );
        // Expanding keeps the source escaped and expands under the call's ordinal.
        d.operator.chat.as_mut().unwrap().expanded.insert(2, true);
        let screen = draw(&mut d, 140, 24);
        assert!(screen.contains("$ ls"), "{screen}");
        assert!(screen.contains("\\u{1b}]52;c;INJECTED\\u{7}"), "{screen}");
        assert!(
            !screen.contains("\u{1b}]52;c;INJECTED\u{7}}"),
            "no active control reaches the terminal"
        );
        // Scrolling to older rows keeps a durable anchor; returning to the tail follows again.
        d.operator_scroll(-10);
        assert!(!d.operator.chat.as_ref().unwrap().follow);
        let anchor = d.operator.chat.as_ref().unwrap().anchor;
        assert!(anchor.is_some());
        d.operator_scroll(40);
        assert!(d.operator.chat.as_ref().unwrap().follow);
    }

    #[test]
    fn unread_clears_only_after_the_loaded_tail_is_drawn() {
        if super::super::pi_available_test::run_with_pi_available(
            "dashboard::operator::tests::unread_clears_only_after_the_loaded_tail_is_drawn",
        ) {
            return;
        }
        let mut d = dashboard();
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(
                with_model(summary()),
                vec![
                    message(
                        1,
                        OperatorMessageContent::User {
                            skill: None,
                            text: "go".into(),
                            context: OperatorContext::default(),
                        },
                    ),
                    delta(2, "done"),
                    completed(3),
                ],
                None,
            ),
        );
        assert_eq!(
            d.operator.chat.as_ref().unwrap().unread,
            Some(OperatorTurnId(1))
        );
        // A too-small view cannot acknowledge the tail.
        draw(&mut d, 40, 12);
        assert_eq!(
            d.operator.chat.as_ref().unwrap().unread,
            Some(OperatorTurnId(1))
        );
        let screen = draw(&mut d, 140, 24);
        assert!(screen.contains("1 unread"), "{screen}");
        assert_eq!(d.operator.chat.as_ref().unwrap().unread, None);
        let screen = draw(&mut d, 140, 24);
        assert!(!screen.contains("1 unread"), "{screen}");
    }

    #[test]
    fn narrow_and_wide_layouts_keep_the_composer_and_floor_guidance() {
        let mut d = dashboard();
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(summary(), Vec::new(), None),
        );
        d.operator.chat.as_mut().unwrap().draft = "preserve draft".into();
        // Wide: the ordinary sidebar keeps its width.
        draw(&mut d, 140, 40);
        assert_eq!(content_area(&d, d.outer_area).x, 40);
        // Narrow: the conversation wins; Ctrl-l reveals the ordinary sidebar only.
        draw(&mut d, 80, 24);
        assert_eq!(content_area(&d, d.outer_area).x, 0);
        assert!(!d.operator.sidebar_open);
        d.ctrl('l');
        assert!(d.operator.sidebar_open);
        let screen = draw(&mut d, 80, 24);
        assert!(!screen.contains("Chats") && !screen.contains("Conversation"));
        d.ctrl('l');
        assert!(!d.operator.sidebar_open);
        // Below the 60x16 floor the view asks for a resize and keeps the draft.
        for (cols, rows) in [(59, 16), (60, 15)] {
            let screen = draw(&mut d, cols, rows);
            assert!(screen.contains("60×16"), "{cols}x{rows}: {screen}");
            assert_eq!(d.operator.chat.as_ref().unwrap().draft, "preserve draft");
        }
        // The composer keeps its two-row minimum at the floor.
        draw(&mut d, 60, 16);
        assert_eq!(d.operator_layout(d.outer_area).composer.height, 2);
    }

    #[test]
    fn frames_at_floor_and_wide_sizes_match_the_singleton_layout() {
        if super::super::pi_available_test::run_with_pi_available(
            "dashboard::operator::tests::frames_at_floor_and_wide_sizes_match_the_singleton_layout",
        ) {
            return;
        }
        let mut d = dashboard();
        let out = opened(&mut d, with_model(summary()));
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(
                with_model(summary()),
                vec![
                    message(
                        1,
                        OperatorMessageContent::User {
                            skill: None,
                            text: "Review this workspace.".into(),
                            context: OperatorContext::default(),
                        },
                    ),
                    delta(2, "I will inspect its sessions."),
                    message(
                        3,
                        OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                            id: "inspect".into(),
                            output: String::new(),
                            truncated: false,
                            status: OperatorToolStatus::Succeeded,
                        }),
                    ),
                ],
                None,
            ),
        );
        for (cols, rows) in [(140, 40), (100, 24), (80, 24), (60, 24), (60, 16)] {
            let frame = frame_lines_at(&mut d, cols, rows, 0);
            let mut first = frame.lines();
            let header = first.next().unwrap();
            assert!(
                header.starts_with("󰚩 OVRCR") && !header.contains("Operator"),
                "title bar stays clear at {cols}x{rows}: {header}"
            );
            let content = content_area(&d, d.outer_area);
            let main: String = frame
                .lines()
                .skip(content.y as usize)
                .take(content.height as usize)
                .map(|line| line.chars().skip(content.x as usize).collect::<String>())
                .collect();
            assert!(
                main.contains("Operator · pi · openai/gpt-6.1-sol · ready"),
                "conversation header at {cols}x{rows}: {main}"
            );
            assert!(
                !main.contains('┌') && !main.contains('┐'),
                "no full frame at {cols}x{rows}: {main}"
            );
            assert!(main.contains("Review this workspace."));
            let snapshot = frame;
            let path = format!(
                "{}/src/dashboard/operator/snapshots/singleton-{cols}x{rows}.txt",
                env!("CARGO_MANIFEST_DIR")
            );
            if std::env::var_os("OVRCR_TUI_UPDATE_SNAPSHOTS").is_some() {
                std::fs::write(&path, &snapshot).unwrap();
            } else {
                let expected = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("read {path}: {error}"));
                assert_eq!(snapshot, expected, "singleton frame {cols}x{rows}");
            }
            assert_eq!(d.operator_layout(d.outer_area).composer.height, 2);
        }
    }

    #[test]
    fn sidebar_geometry_shrinks_the_operator_section_without_a_close_target() {
        let mut d = dashboard();
        opened(&mut d, busy());
        for (cols, rows, operator_height) in [
            (140, 40, 4),
            (100, 24, 4),
            (80, 24, 0),
            (60, 16, 0),
            (40, 12, 0),
        ] {
            d.install_area(Rect::new(0, 0, cols, rows));
            let rects = d.sidebar_rects(d.outer_area);
            assert_eq!(
                rects.operator.height, operator_height,
                "Operator follows the section shrink order at {cols}x{rows}"
            );
            if operator_height == 0 {
                assert!(
                    rects.operator.is_empty(),
                    "the on-demand sidebar hides at {cols}x{rows}"
                );
                continue;
            }
            assert_eq!(
                rects.operator.y,
                rects.attention.bottom() + u16::from(rects.attention.height > 0),
                "Operator follows attention and its optional blank row at {cols}x{rows}"
            );
            assert!(rects.tree.height >= 3, "three tree lines at {cols}x{rows}");
            assert!(rects.operator.bottom() <= rects.tree.y);
            assert!(rects.tree.bottom() <= rects.quota.y);
            assert_eq!(rects.quota.bottom(), rows - 1);
        }
        d.install_area(Rect::new(0, 0, 140, 24));
        let row = d.sidebar_rects(d.outer_area).operator;
        // The right edge is navigation, never a close hit target.
        click(&mut d, row.right() - 1, row.y);
        assert!(d.palette.is_none(), "no close confirmation");
        assert!(d.operator.visible);
        // The busy row animates and the view is worth the spinner cadence.
        assert!(d.operator_sidebar_busy());
        assert!(d.operator_conversation_busy());
    }

    #[test]
    fn quick_editing_is_unicode_grapheme_and_byte_bounded() {
        let mut d = dashboard();
        d.ctrl('o');
        d.event_action(Event::Paste("界e\u{301}🙂".into()));
        let quick = d.operator.quick.as_ref().unwrap();
        assert_eq!(quick.text, "界e\u{301}🙂");
        d.key(KeyCode::Backspace);
        assert_eq!(d.operator.quick.as_ref().unwrap().text, "界e\u{301}");
        d.key(KeyCode::Backspace);
        assert_eq!(d.operator.quick.as_ref().unwrap().text, "界");
        d.key(KeyCode::Char('a'));
        assert_eq!(d.operator.quick.as_ref().unwrap().text, "界a");
        d.event_action(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::SHIFT,
        )));
        assert_eq!(d.operator.quick.as_ref().unwrap().text, "界a\n");
        d.ctrl('c');
        assert_eq!(d.operator.quick.as_ref().unwrap().text, "");
        let huge = "x".repeat(MAX_OPERATOR_TEXT_BYTES + 10);
        d.event_action(Event::Paste(huge));
        assert!(
            d.operator.quick.as_ref().unwrap().text.len() <= MAX_OPERATOR_TEXT_BYTES,
            "the protocol text bound holds"
        );
        assert!(d.drain_outbox().is_empty(), "paste only edits");
    }

    #[test]
    fn operator_changed_loads_only_the_singleton_and_ignores_legacy_ids() {
        let mut d = dashboard();
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(summary(), Vec::new(), None),
        );
        let out =
            d.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(Id(9))));
        assert!(out.is_empty(), "legacy rows stay hidden");
        let out =
            d.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(Id(1))));
        assert_eq!(out.len(), 1);
        assert!(matches!(
            out[0].request,
            Request::Operator(OperatorRequest::Read {
                conversation: Id(1),
                after: 0,
                ..
            })
        ));
    }

    #[test]
    fn empty_thinking_and_compaction_events_add_no_visible_rows() {
        let mut chat = Chat::new(summary(), OperatorContext::default());
        chat.messages = vec![
            message(
                1,
                OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(String::new())),
            ),
            message(
                2,
                OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionStarted),
            ),
            message(
                3,
                OperatorMessageContent::Worker(OperatorWorkerEvent::CompactionEnded),
            ),
            delta(4, "answer"),
        ];
        let visible = transcript_rows(&chat, 80, 0)
            .iter()
            .map(|row| row.line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!visible.contains("Thinking"), "{visible}");
        assert!(!visible.contains("Compaction"), "{visible}");
        assert!(visible.contains("answer"), "{visible}");
    }

    #[test]
    fn feed_collapses_older_requests_and_opens_the_newest() {
        let mut chat = Chat::new(summary(), OperatorContext::default());
        chat.messages = two_requests();
        let rows = feed(&chat, 60);
        assert_eq!(
            rows,
            [
                "▌ ✓ create the workspace               1 change · 2 commands",
                "",
                "▌ ✓ now remove it",
                "▌   − ovrcr workspace remove issue-1 --force          exit 2",
                "▌",
                "▌   It failed.",
                "",
            ]
        );
        // A finished request never shows its thinking or a command's output.
        chat.expanded.insert(1, true);
        let open = feed(&chat, 60);
        assert_eq!(
            open[..7],
            [
                "▌ ✓ create the workspace",
                "▌   → global",
                "▌     ovrcr workspace list",
                "▌   + ovrcr workspace create ovrcr issue-1",
                "▌",
                "▌   Created it.",
                "",
            ]
        );
        let all = open.join("\n");
        assert!(!all.contains("SECRET_PLAN"), "{all}");
        assert!(!all.contains("CREATED_OUTPUT"), "{all}");
        // The state colours the bar; a changing command is marked in peach.
        let styled = transcript_rows(&chat, 60, 0);
        assert_eq!(
            styled[0].line.spans[0].style.fg,
            Some(crate::theme::GREEN())
        );
        assert_eq!(
            styled[3].line.spans[2].style.fg,
            Some(crate::theme::PEACH())
        );
        assert_eq!(
            styled[2].line.spans[2].style.fg,
            Some(crate::theme::SUBTEXT()),
            "a lookup is dimmed"
        );
    }

    #[test]
    fn running_request_shows_its_pending_command_and_one_escaped_thought() {
        let mut chat = Chat::new(running_turn(), OperatorContext::default());
        chat.messages = vec![
            request(1, "do it"),
            call(2, "a", "ovrcr list"),
            thinking(3, "first line\nPLAN \u{1b}]52;c;INJECTED\u{7}\n\n"),
        ];
        let rows = feed(&chat, 60);
        assert_eq!(
            rows,
            [
                "▌ ● do it",
                "▌   → global",
                "▌     ovrcr list                                     running",
                "▌     PLAN \\u{1b}]52;c;INJECTED\\u{7}",
                "▌   ⠋ Responding",
            ]
        );
        assert!(!rows.join("\n").contains('\u{1b}'));
        // Another tool's expansion shows its arguments as sent, with no `$`.
        chat.messages.insert(
            1,
            message(
                2,
                OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                    id: "r".into(),
                    name: "read".into(),
                    arguments: r#"{"path":"/tmp/notes.md"}"#.into(),
                }),
            ),
        );
        chat.messages[2].ordinal = 3;
        chat.messages[3].ordinal = 4;
        chat.expanded.insert(2, true);
        let rows = feed(&chat, 60);
        assert_eq!(
            rows[2..4],
            [
                "▌     read /tmp/notes.md                             running",
                "▌       {\"path\":\"/tmp/notes.md\"}",
            ]
        );
        chat.messages.remove(1);
        chat.messages[1].ordinal = 2;
        chat.messages[2].ordinal = 3;
        chat.expanded.clear();
        assert_eq!(
            transcript_rows(&chat, 60, 0)[0].line.spans[0].style.fg,
            Some(crate::theme::BLUE())
        );
        // The thought is only the trailing one; an answer replaces it.
        chat.messages.push(delta(4, "answer"));
        let rows = feed(&chat, 60);
        assert!(!rows.join("\n").contains("PLAN"), "{rows:?}");
        assert_eq!(rows[rows.len() - 2], "▌   answer");
        // Empty or newline-only thinking adds no row.
        chat.messages = vec![request(1, "do it"), thinking(2, ""), thinking(3, "\n\n")];
        assert_eq!(
            feed(&chat, 60),
            ["▌ ● do it", "▌   → global", "▌   ⠋ Responding"]
        );
    }

    #[test]
    fn failed_interrupted_and_wake_entries_name_their_outcome() {
        let mut chat = Chat::new(summary(), OperatorContext::default());
        chat.messages = vec![
            request(1, "first"),
            message(
                2,
                OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(
                    "No Pi model selected; run ovrcr operator setup.".into(),
                )),
            ),
            on_turn(2, request(3, "second")),
            on_turn(2, delta(4, "partial")),
            on_turn(2, message(5, OperatorMessageContent::Interrupted)),
            on_turn(
                3,
                message(
                    6,
                    OperatorMessageContent::AsyncWakeup {
                        text: "These watched OVRCR Workspace Agent sessions settled:\n- session 13 (coordinator-alpha) in consigint: response_ready (phase running, activity response_ready, run 1)\n- session 14 (coordinator-beta) in consigint: response_ready (phase running, activity response_ready, run 1)".into(),
                    },
                ),
            ),
            on_turn(3, delta(7, "Both finished.")),
            on_turn(3, completed(8)),
        ];
        chat.expanded.insert(6, false);
        assert_eq!(
            feed(&chat, 100),
            [
                "▌ ✕ first                                            No Pi model selected; run ovrcr operator setup.",
                "▌ ■ second                                                                               interrupted",
                "▌ ◆ Woke · #13 coordinator-alpha, #14 coordinator-beta",
                "",
            ]
        );
        chat.expanded.clear();
        chat.expanded.insert(3, true);
        let rows = feed(&chat, 100);
        assert!(rows.contains(&"▌   partial".to_owned()), "{rows:?}");
        assert!(
            rows.contains(
                &"▌   Interrupted · operations may finish; file changes remain.".to_owned()
            ),
            "{rows:?}"
        );
        assert!(rows.contains(&"▌   Both finished.".to_owned()), "{rows:?}");
        assert!(
            rows.iter()
                .any(|row| row.starts_with("▌   - session 13 (coordinator-alpha)")),
            "the open wake-up keeps its report: {rows:?}"
        );
    }

    #[test]
    fn operator_sidebar_is_its_own_card_not_a_project_heading() {
        let mut d = dashboard();
        d.install_area(Rect::new(0, 0, 140, 40));
        let rect = d.sidebar_rects(d.outer_area).operator;
        let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
        terminal
            .draw(|frame| super::super::draw_dashboard_at(frame, &d, 0))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, rect.y)].symbol(), "┏");
        assert_eq!(
            buffer[(2, rect.y + 1)].bg,
            crate::theme::CRUST(),
            "the closed tile has its own surface"
        );
        let title: String = (0..rect.width)
            .map(|column| buffer[(column, rect.y + 1)].symbol())
            .collect();
        assert!(title.contains("Operator"), "{title:?}");
        assert!(
            !title.contains("OPERATOR") && !title.contains('─'),
            "the tile is not a project heading: {title:?}"
        );

        d = loaded(summary(), Vec::new());
        d.install_area(Rect::new(0, 0, 140, 40));
        terminal
            .draw(|frame| super::super::draw_dashboard_at(frame, &d, 0))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(2, rect.y + 1)].bg,
            crate::theme::SURFACE0(),
            "the selected tile uses the selection surface"
        );
    }

    #[test]
    fn sidebar_never_echoes_the_reply_as_a_session_title() {
        if super::super::pi_available_test::run_with_pi_available(
            "dashboard::operator::tests::sidebar_never_echoes_the_reply_as_a_session_title",
        ) {
            return;
        }
        let mut d = loaded(
            summary(),
            vec![
                request(1, "report"),
                delta(2, "## Status\n\n**Ready** with `cargo test`."),
                completed(3),
            ],
        );
        let screen = frame_lines(&mut d, 140, 40);
        let sidebar: String = screen
            .lines()
            .map(|line| line.chars().take(39).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        for reply_text in ["Status", "Ready", "cargo test"] {
            assert!(
                !sidebar.contains(reply_text),
                "the sidebar repeats reply text {reply_text:?}: {sidebar}"
            );
        }
        assert!(
            !sidebar.contains("o open") && !sidebar.contains("Ctrl-o"),
            "the operator card carries no keyboard shortcut copy: {sidebar}"
        );
    }

    #[test]
    fn markdown_replies_render_as_prose_and_tables_not_source() {
        let mut chat = Chat::new(summary(), OperatorContext::default());
        chat.messages = vec![
            request(1, "report"),
            delta(
                2,
                "## Status\n\n**Ready** and `cargo test` passed.\n\n```text\nall green\n```\n\n| Area | Result |\n| --- | --- |\n| tests | pass |\n",
            ),
            completed(3),
        ];
        let rows = transcript_rows(&chat, 100, 0);
        let screen = rows
            .iter()
            .map(|row| row.line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        for source in ["##", "**", "```", "| ---"] {
            assert!(!screen.contains(source), "raw {source:?}: {screen}");
        }
        for expected in [
            "Status",
            "Ready and cargo test passed.",
            "all green",
            "┌",
            "pass",
        ] {
            assert!(screen.contains(expected), "missing {expected:?}: {screen}");
        }
        let style = |needle: &str| {
            rows.iter()
                .flat_map(|row| &row.line.spans)
                .find(|span| span.content.contains(needle))
                .unwrap_or_else(|| panic!("no span contains {needle:?}"))
                .style
        };
        assert_eq!(style("Status").fg, Some(crate::theme::BLUE()));
        assert!(
            style("Ready")
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        assert_eq!(style("cargo test").fg, Some(crate::theme::TEAL()));
        assert_eq!(style("all green").fg, Some(crate::theme::TEAL()));
        assert_eq!(style("Area").fg, Some(crate::theme::BLUE()));
        let anchors: Vec<_> = rows.iter().filter_map(|row| row.anchor).collect();
        assert!(anchors.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn long_requests_fold_runs_of_finished_lookups() {
        let mut chat = Chat::new(summary(), OperatorContext::default());
        chat.messages = vec![request(1, "find it")];
        for step in 0..9u64 {
            let id = format!("lookup-{step}");
            chat.messages.push(call(
                2 + step * 2,
                &id,
                &format!("ovrcr list --page {step}"),
            ));
            chat.messages.push(output(
                3 + step * 2,
                &id,
                "",
                if step == 8 {
                    OperatorToolStatus::TimedOut
                } else {
                    OperatorToolStatus::Succeeded
                },
            ));
        }
        chat.messages
            .push(call(20, "change", "ovrcr new --project ovrcr"));
        chat.messages
            .push(output(21, "change", "", OperatorToolStatus::Succeeded));
        chat.messages.push(completed(22));
        assert_eq!(
            feed(&chat, 60),
            [
                "▌ ✓ find it",
                "▌   → global",
                "▌     ▸ 8 lookups",
                "▌     ovrcr list --page 8                          timed out",
                "▌   + ovrcr new --project ovrcr",
                "",
            ],
            "a failed lookup stays visible"
        );
        chat.expanded.insert(2 | feed::FOLD_KEY, true);
        let rows = feed(&chat, 60);
        assert_eq!(rows[2], "▌     ▾ 8 lookups");
        assert_eq!(
            rows.iter()
                .filter(|row| row.contains("ovrcr list --page"))
                .count(),
            9
        );
    }

    #[test]
    fn keys_and_clicks_open_entries_and_commands() {
        let mut d = loaded(summary(), two_requests());
        let screen = frame_lines(&mut d, 140, 24);
        assert!(!screen.contains("Created it."), "{screen}");
        // Tab focuses the transcript; Home reaches the oldest entry.
        d.key(KeyCode::Tab);
        d.key(KeyCode::Home);
        assert_eq!(d.operator.chat.as_ref().unwrap().entry_focus, Some(1));
        d.key(KeyCode::Enter);
        let screen = frame_lines(&mut d, 140, 24);
        assert!(screen.contains("▌   Created it."), "{screen}");
        assert!(!screen.contains("CREATED_OUTPUT"), "{screen}");
        // Down passes the context row and the lookup to reach the changing command.
        for _ in 0..3 {
            d.key(KeyCode::Down);
        }
        assert_eq!(d.operator.chat.as_ref().unwrap().entry_focus, Some(5));
        d.key(KeyCode::Enter);
        let screen = frame_lines(&mut d, 140, 24);
        assert!(
            screen.contains("▌       $ \"$OVRCR_EXECUTABLE\" workspace create ovrcr issue-1"),
            "{screen}"
        );
        assert!(screen.contains("▌       CREATED_OUTPUT"), "{screen}");
        d.key(KeyCode::Enter);
        let screen = frame_lines(&mut d, 140, 24);
        assert!(!screen.contains("CREATED_OUTPUT"), "{screen}");
        // Enter on the header collapses the entry again.
        d.key(KeyCode::Home);
        d.key(KeyCode::Enter);
        let screen = frame_lines(&mut d, 140, 24);
        assert!(!screen.contains("Created it."), "{screen}");

        // A click on a command row opens it.
        let content = content_area(&d, d.outer_area);
        let row = screen
            .lines()
            .position(|line| line.contains("− ovrcr workspace remove"))
            .expect("the newest entry lists its command") as u16;
        click(&mut d, content.x + 8, row);
        let screen = frame_lines(&mut d, 140, 24);
        assert!(screen.contains("▌       REMOVE_OUTPUT"), "{screen}");
        // A click above a short feed hits nothing.
        let transcript = d.operator_layout(d.outer_area).transcript;
        click(&mut d, transcript.x + 8, transcript.y);
        assert_eq!(d.operator.chat.as_ref().unwrap().entry_focus, None);
        assert_eq!(frame_lines(&mut d, 140, 24), screen);
    }

    #[test]
    fn a_short_feed_sits_against_the_composer() {
        let mut d = loaded(
            summary(),
            vec![request(1, "hello"), delta(2, "Hello back."), completed(3)],
        );
        let screen = frame_lines(&mut d, 140, 24);
        let lines: Vec<&str> = screen.lines().collect();
        let caption = lines
            .iter()
            .position(|line| line.contains("Managing global"))
            .expect("the target caption is drawn");
        assert!(lines[caption - 2].contains("▌   Hello back."), "{screen}");
        assert!(lines[caption - 4].contains("▌ ✓ hello"), "{screen}");
        let transcript = d.operator_layout(d.outer_area).transcript;
        let top: String = lines[transcript.y as usize]
            .chars()
            .skip(transcript.x as usize)
            .collect();
        assert!(
            top.trim().is_empty(),
            "the space is above the feed: {screen}"
        );
    }

    #[test]
    fn a_scrolled_feed_keeps_its_row_while_the_reply_streams() {
        let mut chat = Chat::new(running_turn(), OperatorContext::default());
        chat.messages = vec![request(1, "list them"), call(2, "a", "ovrcr list")];
        chat.messages
            .push(output(3, "a", "", OperatorToolStatus::Succeeded));
        for line in 0..60u64 {
            chat.messages
                .push(delta(4 + line, &format!("reply line {line}\n")));
        }
        let rows = transcript_rows(&chat, 80, 0);
        let top = rows
            .iter()
            .position(|row| row.line.to_string().contains("reply line 20"))
            .unwrap();
        chat.follow = false;
        chat.anchor = rows[top].anchor;
        assert_eq!(transcript_offset(&chat, &rows, 10), top);
        // More commands and more reply arrive below the viewport.
        chat.messages.push(call(100, "b", "ovrcr list"));
        chat.messages.push(delta(101, "and a later paragraph"));
        let rows = transcript_rows(&chat, 80, 0);
        let offset = transcript_offset(&chat, &rows, 10);
        assert!(
            rows[offset].line.to_string().contains("reply line 20"),
            "{}",
            rows[offset].line
        );
        // Durable rows keep ascending anchors, so the offset search is exact.
        let anchors: Vec<_> = rows.iter().filter_map(|row| row.anchor).collect();
        assert!(anchors.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn styled_wrapping_keeps_spans_and_the_real_cursor_on_wide_graphemes() {
        let style = Style::default().fg(super::super::render::RED());
        let rows = wrapped_lines(
            vec![Line::from(vec![Span::styled("界e\u{301}界", style)])],
            3,
        );
        assert_eq!(
            rows.iter().map(Line::to_string).collect::<Vec<_>>(),
            ["界e\u{301}", "界"]
        );
        assert!(
            rows.iter()
                .all(|line| line.spans.iter().all(|span| span.style.fg == style.fg))
        );
        let mut chat = Chat::new(summary(), OperatorContext::default());
        chat.messages = vec![delta(1, "wide 界 text")];
        assert!(
            transcript_rows(&chat, 80, 0)
                .iter()
                .any(|row| row.line.to_string().contains("wide 界 text"))
        );

        let mut d = dashboard();
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(summary(), Vec::new(), None),
        );
        {
            let chat = d.operator.chat.as_mut().unwrap();
            let text = format!("{}e\u{301}🙂", "界".repeat(160));
            chat.cursor.insert(&mut chat.draft, &text);
        }
        let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
        d.install_area(Rect::new(0, 0, 60, 16));
        d.draw(&mut terminal).unwrap();
        let cursor =
            ratatui::backend::Backend::get_cursor_position(terminal.backend_mut()).unwrap();
        assert!(
            cursor.y >= 10 && cursor.y < 15 && cursor.x < 60,
            "real cursor {cursor:?}"
        );
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .any(|cell| cell.symbol() == "🙂")
        );
    }

    #[test]
    fn multiline_up_down_keeps_graphemes_and_shift_enter_inserts_newlines() {
        let mut d = dashboard();
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(summary(), Vec::new(), None),
        );
        d.event_action(Event::Paste("one\n界e\u{301}🙂".into()));
        assert_eq!(d.operator.chat.as_ref().unwrap().draft, "one\n界e\u{301}🙂");
        d.key(KeyCode::Up);
        d.event_action(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::SHIFT,
        )));
        let draft = d.operator.chat.as_ref().unwrap().draft.clone();
        assert!(draft.starts_with("one\n"), "{draft:?}");
        assert!(draft.contains("界e\u{301}🙂"), "{draft:?}");
        assert_eq!(draft.matches('\n').count(), 2);
        d.ctrl('c');
        assert_eq!(d.operator.chat.as_ref().unwrap().draft, "");
    }

    #[test]
    fn footer_and_keymap_describe_only_singleton_actions() {
        let mut d = dashboard();
        opened(&mut d, summary());
        let groups = super::super::keymap::keymap(&d);
        let labels: Vec<&str> = groups
            .into_iter()
            .flat_map(|group| group.keys)
            .map(|binding| binding.name)
            .collect();
        assert!(!labels.iter().any(|label| label.contains("Conversation")));
        assert!(!labels.iter().any(|label| label.contains("Close")));
        assert!(!labels.iter().any(|label| label.contains("Chats")));
        assert!(labels.contains(&"Send"));
        assert!(labels.contains(&"Toggle sidebar"));
    }

    #[test]
    fn scrolling_an_unchanged_transcript_reuses_wrapped_rows() {
        let mut d = dashboard();
        // A non-default theme: input handlers and draws must share one build.
        d.theme = crate::theme::ThemeTokens::light();
        let out = opened(&mut d, summary());
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(
                summary(),
                (1..=200)
                    .map(|ordinal| delta(ordinal, &format!("line {ordinal}\n")))
                    .collect(),
                None,
            ),
        );
        let area = Rect::new(0, 0, 140, 24);
        let bottom = draw(&mut d, 140, 24);
        let transcript = d.operator_layout(area).transcript;
        let wheel = |kind| MouseEvent {
            kind,
            column: transcript.x + 1,
            row: transcript.y + 1,
            modifiers: KeyModifiers::NONE,
        };
        let builds = render::ROW_BUILDS.get();
        for _ in 0..20 {
            d.handle_mouse(wheel(MouseEventKind::ScrollUp), area);
        }
        let scrolled = draw(&mut d, 140, 24);
        assert!(scrolled.contains("line 130"), "{scrolled}");
        assert!(!scrolled.contains("line 200"), "{scrolled}");
        for _ in 0..20 {
            d.handle_mouse(wheel(MouseEventKind::ScrollDown), area);
            draw(&mut d, 140, 24);
        }
        assert_eq!(draw(&mut d, 140, 24), bottom);
        assert_eq!(
            render::ROW_BUILDS.get(),
            builds,
            "wheel events and redraws over an unchanged journal must not re-wrap it"
        );
        // An appended message is a new journal and is wrapped once.
        let chat = d.operator.chat.as_mut().unwrap();
        chat.messages.push(delta(201, "appended tail"));
        let appended = draw(&mut d, 140, 24);
        assert!(appended.contains("appended tail"), "{appended}");
        draw(&mut d, 140, 24);
        assert_eq!(render::ROW_BUILDS.get(), builds + 1);
    }

    #[test]
    fn copy_notice_clears_on_next_operator_action_and_overlay_keymaps_match_footers() {
        use crossterm::event::Event;
        use ovrcr_protocol::{OperatorSessionTree, OperatorTreeEntry};

        let mut d = dashboard();
        let mut state = summary();
        state.run = SessionRunId(1);
        state.current_turn = Some(turn(1, OperatorTurnState::Complete));
        state.worker_stopped = true;
        state.worker_live = false;
        let out = opened(&mut d, state.clone());
        let read = read_request(&out);
        respond(
            &mut d,
            read.request_id,
            history_of(
                state,
                vec![
                    message(
                        1,
                        OperatorMessageContent::User {
                            skill: None,
                            text: "hello".into(),
                            context: OperatorContext::default(),
                        },
                    ),
                    delta(2, "Durable reply."),
                    message(
                        3,
                        OperatorMessageContent::Worker(OperatorWorkerEvent::Completed {
                            leased: false,
                            pi_first_entry_id: Some("u1".into()),
                            pi_leaf_entry_id: Some("a1".into()),
                        }),
                    ),
                ],
                None,
            ),
        );

        // 1. `/copy` + `finish_copy(Ok(()))` renders outer clipboard confirmation, then clears on next Operator input.
        d.event_action(Event::Paste("/copy".into()));
        assert_eq!(
            d.key(KeyCode::Enter),
            DashboardAction::CopyText("Durable reply.".into())
        );
        d.finish_copy(Ok(()));
        let with_copy_notice = draw(&mut d, 140, 28);
        assert!(
            with_copy_notice.contains("Clipboard request sent; paste to verify"),
            "{with_copy_notice}"
        );
        d.event_action(Event::Paste("/tree".into()));
        let after_paste = draw(&mut d, 140, 28);
        assert!(
            !after_paste.contains("Clipboard request sent; paste to verify"),
            "copy_notice must clear on next Operator paste/key:\n{after_paste}"
        );

        // 2. TreeModal::Loading keymap: Esc -> Close, Enter unbound.
        d.key(KeyCode::Enter);
        let tree_req = d
            .drain_outbox()
            .into_iter()
            .find(|m| matches!(m.request, Request::Operator(OperatorRequest::Tree { .. })))
            .expect("expected Tree request");
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .map(|b| b.name),
            Some("Close")
        );
        assert!(
            d.key_binding_for(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .is_none(),
            "Loading /tree modal must not advertise Enter"
        );

        // 3. TreeModal::Browse keymap: Up/Down, Enter, Tab, Ctrl-f, Ctrl-u, Ctrl-c, Esc.
        respond(
            &mut d,
            tree_req.request_id,
            OperatorResponse::Tree(Box::new(OperatorSessionTree {
                pi_session_id: Some("pi-1".into()),
                current_leaf_id: Some("a2".into()),
                entries: vec![
                    OperatorTreeEntry {
                        id: "u1".into(),
                        parent_id: None,
                        kind: OperatorTreeEntryKind::User,
                        preview: "first".into(),
                    },
                    OperatorTreeEntry {
                        id: "a1".into(),
                        parent_id: Some("u1".into()),
                        kind: OperatorTreeEntryKind::Assistant { tool_only: false },
                        preview: "ans 1".into(),
                    },
                    OperatorTreeEntry {
                        id: "u2".into(),
                        parent_id: Some("a1".into()),
                        kind: OperatorTreeEntryKind::User,
                        preview: "second".into(),
                    },
                    OperatorTreeEntry {
                        id: "a2".into(),
                        parent_id: Some("u2".into()),
                        kind: OperatorTreeEntryKind::Assistant { tool_only: false },
                        preview: "ans 2".into(),
                    },
                ],
            })),
        );
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
                .map(|b| b.name),
            Some("Select")
        );
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
                .map(|b| b.name),
            Some("Select")
        );
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
                .map(|b| b.name),
            Some("Filter mode")
        );
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL))
                .map(|b| b.name),
            Some("Filter mode")
        );
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL))
                .map(|b| b.name),
            Some("User-only")
        );
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
                .map(|b| b.name),
            Some("Clear")
        );
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .map(|b| b.name),
            Some("Close")
        );

        // 4. ConfirmSummarize keymap: Esc -> Back, Enter -> Confirm.
        d.key(KeyCode::Up); // select u2
        d.key(KeyCode::Enter);
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .map(|b| b.name),
            Some("Back")
        );
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .map(|b| b.name),
            Some("Confirm")
        );

        // 5. CustomInstructions keymap: Esc -> Back, Ctrl-c -> Clear, Enter -> Summarize.
        d.key(KeyCode::Down);
        d.key(KeyCode::Down);
        d.key(KeyCode::Enter);
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .map(|b| b.name),
            Some("Back")
        );
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
                .map(|b| b.name),
            Some("Clear")
        );
        assert_eq!(
            d.key_binding_for(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .map(|b| b.name),
            Some("Summarize")
        );
    }
}

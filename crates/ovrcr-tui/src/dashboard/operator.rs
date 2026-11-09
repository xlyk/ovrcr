//! One durable pi conversation is protocol data, never a terminal parser or a
//! PTY input target. The shared sidebar row, the full view and the Quick
//! message overlay all address that same singleton.
mod render;

use super::text_cursor::TextCursor;
use super::{Dashboard, DashboardAction, InputMode};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr_protocol::{
    ClientMessage, MAX_OPERATOR_TEXT_BYTES, OperatorContext, OperatorConversationId as Id,
    OperatorConversationSummary as Summary, OperatorHistory, OperatorMessage,
    OperatorMessageContent, OperatorRequest, OperatorResponse, OperatorToolStatus, OperatorTurnId,
    OperatorTurnIdentity, OperatorTurnState, OperatorWorkerEvent, Request, Response, SessionId,
    SessionRunId,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub(super) struct Hub {
    pub visible: bool,
    editing: bool,
    transcript_focus: bool,
    sidebar_open: bool,
    chat: Option<Chat>,
    /// One Open or history read is in flight for the singleton.
    loading: bool,
    pending: HashMap<u64, Pending>,
    context: OperatorContext,
    error: Option<String>,
    quick: Option<QuickMessage>,
    quick_generation: u64,
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
    Opening,
    Submitting,
}
struct QuickSubmission {
    generation: u64,
    text: String,
    context: OperatorContext,
}
/// Why an Open was sent: the full view, or the first explicit Quick Send.
enum OpenOrigin {
    Manual,
    Quick(QuickSubmission),
}
#[derive(Clone, Copy)]
enum SubmitOrigin {
    Composer,
    Quick { generation: u64 },
}
struct Chat {
    summary: Summary,
    /// The fixed agent working directory; the captured target is per message.
    workdir: Option<std::path::PathBuf>,
    error: Option<String>,
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
    expanded: BTreeSet<u64>,
    entry_focus: Option<u64>,
    follow: bool,
    unread: Option<OperatorTurnId>,
    reviewed: Option<OperatorTurnId>,
}
impl Chat {
    fn new(summary: Summary, context: OperatorContext) -> Self {
        Self {
            summary,
            workdir: None,
            error: None,
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
            expanded: BTreeSet::new(),
            entry_focus: None,
            follow: true,
            unread: None,
            reviewed: None,
        }
    }
    fn can_submit(&self) -> bool {
        !self.submitting
            && self.summary.worker_stopped
            && self
                .summary
                .current_turn
                .as_ref()
                .is_none_or(|turn| turn.state != OperatorTurnState::Running)
    }
}
enum Pending {
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
        origin: SubmitOrigin,
    },
    Stop {
        identity: OperatorTurnIdentity,
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
        right.y,
        width,
        right.height.min(1),
    )
}
#[cfg(test)]
use render::{transcript_lines, wrapped_lines};
use render::{transcript_offset, transcript_rows};

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
        Some(OperatorTurnState::Complete) if !chat.summary.worker_stopped => {
            "finishing · stop unconfirmed"
        }
        Some(OperatorTurnState::Interrupted) if !chat.summary.worker_stopped => {
            "interrupted · stop unconfirmed"
        }
        Some(OperatorTurnState::Error) if !chat.summary.worker_stopped => {
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
            return (!chat.summary.worker_stopped).then(|| ConversationActivity {
                label: "Stop unconfirmed".into(),
                animated: false,
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

fn expandable(message: &OperatorMessage) -> bool {
    matches!(
        message.content,
        OperatorMessageContent::User { .. }
            | OperatorMessageContent::Worker(
                OperatorWorkerEvent::ToolCall { .. } | OperatorWorkerEvent::ToolOutput { .. }
            )
    )
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
        if self
            .operator
            .quick
            .as_ref()
            .is_some_and(|q| q.phase == QuickPhase::Editing && q.error.is_none() && !q.seeded)
        {
            // A cancelled unsent Quick draft creates nothing and leaves no overlay.
            self.operator.quick = None;
        } else if let Some(quick) = &mut self.operator.quick {
            quick.visible = false;
        }
    }
    pub(super) fn operator_quick_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind != KeyEventKind::Press {
            return DashboardAction::None;
        }
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
        self.operator
            .chat
            .as_ref()
            .is_some_and(|chat| chat.can_submit() && !chat.draft.trim().is_empty())
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
        if !chat.can_submit() {
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
        chat.submitting = true;
        chat.context = context.clone();
        chat.error = None;
        self.operator_request(
            Pending::Submit {
                id,
                text: text.clone(),
                session,
                expected_run,
                origin,
            },
            OperatorRequest::Submit {
                conversation: id,
                expected_run,
                text,
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
    pub(super) fn operator_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
        }
        if !self.operator.transcript_focus
            && key.modifiers != KeyModifiers::NONE
            && key.modifiers != KeyModifiers::SHIFT
            && key.modifiers != KeyModifiers::CONTROL
            && key.modifiers != KeyModifiers::ALT
        {
            return DashboardAction::None;
        }
        if super::input::is_browse_key(key) || key.code == KeyCode::Esc {
            // Back to the terminal panes; the conversation keeps its draft.
            self.operator.editing = false;
            self.operator.visible = false;
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
                    }
                }
                return DashboardAction::Redraw;
            }
            // Ctrl-l and Ctrl-x are commands, never draft text.
            if !(key.modifiers == KeyModifiers::CONTROL
                && matches!(key.code, KeyCode::Char('l' | 'x')))
                && chat.cursor.edit_key(&mut chat.draft, key)
            {
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
                if !visible.iter().any(|(_, anchor)| {
                    anchor.is_some_and(|anchor| Some(anchor.0) == chat.entry_focus)
                }) {
                    chat.entry_focus = visible
                        .iter()
                        .find_map(|(_, anchor)| anchor.map(|anchor| anchor.0));
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
                let entries: Vec<_> = chat
                    .messages
                    .iter()
                    .filter(|m| expandable(m))
                    .map(|m| m.ordinal)
                    .collect();
                let index = entries.iter().position(|id| Some(*id) == chat.entry_focus);
                match key.code {
                    KeyCode::Enter => {
                        if let Some(id) = chat.entry_focus
                            && chat
                                .messages
                                .iter()
                                .any(|message| message.ordinal == id && expandable(message))
                            && !chat.expanded.remove(&id)
                        {
                            chat.expanded.insert(id);
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
                ) {
                    let rows = transcript_rows(chat, layout.transcript.width, 0);
                    let offset = transcript_offset(chat, &rows, layout.transcript.height as usize);
                    if let Some(target) = rows.iter().position(|(_, anchor)| {
                        anchor.is_some_and(|anchor| Some(anchor.0) == chat.entry_focus)
                    }) && (target < offset
                        || target >= offset + layout.transcript.height as usize)
                    {
                        chat.follow = false;
                        chat.anchor = rows[target].1;
                        chat.scroll = rows
                            .len()
                            .saturating_sub(layout.transcript.height as usize)
                            .saturating_sub(target);
                    }
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
            KeyCode::Enter => self.operator_submit(),
            KeyCode::Up | KeyCode::Down => {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.cursor.vertical(&chat.draft, key.code == KeyCode::Down);
                }
            }
            KeyCode::PageUp => self.operator_scroll(-10),
            KeyCode::PageDown => self.operator_scroll(10),
            KeyCode::Char(ch) if !ch.is_control() => self.operator_insert(&ch.to_string()),
            code => {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.cursor.key(&mut chat.draft, code);
                }
            }
        }
        DashboardAction::Redraw
    }
    pub(super) fn operator_is_editing(&self) -> bool {
        self.operator.visible && self.operator.editing
    }
    fn operator_insert(&mut self, text: &str) {
        if let Some(chat) = self.operator.chat.as_mut() {
            // Match the protocol's user-text bound without splitting UTF-8.
            if chat.draft.len().saturating_add(text.len()) <= MAX_OPERATOR_TEXT_BYTES {
                chat.cursor.insert(&mut chat.draft, text);
            }
        }
    }
    pub(super) fn operator_paste(&mut self, text: &str) -> DashboardAction {
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
            chat.anchor = rows.get(target).and_then(|(_, start)| *start);
        }
    }
    pub(super) fn operator_mouse(
        &mut self,
        mouse: MouseEvent,
        area: Rect,
    ) -> Option<DashboardAction> {
        if let Some(action) = self.operator_sidebar_mouse(mouse, area) {
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
        let layout = self.operator_layout(area);
        let transcript = layout.transcript;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if !transcript.is_empty() => {
                if stop_rect(layout.header).contains(point) && self.operator_stop_available() {
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
                        chat.entry_focus = rows
                            .get(offset + usize::from(mouse.row - transcript.y))
                            .and_then(|(_, anchor)| anchor.map(|anchor| anchor.0))
                            .filter(|id| {
                                chat.messages
                                    .iter()
                                    .any(|m| m.ordinal == *id && expandable(m))
                            });
                        if let Some(id) = chat.entry_focus
                            && !chat.expanded.remove(&id)
                        {
                            chat.expanded.insert(id);
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
            Pending::Current => {}
            Pending::Open { .. } => self.operator.loading = false,
            Pending::Read { .. } => {
                self.operator.loading = false;
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.reading = false;
                }
            }
            Pending::Submit { .. } => {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.submitting = false;
                }
            }
            Pending::Stop { .. } => {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.stopping = false;
                }
            }
        }
        if let Response::Error { message, .. } = response {
            match &pending {
                Pending::Open {
                    origin: OpenOrigin::Quick(submission),
                } => self.operator_quick_failed(submission.generation, message),
                Pending::Submit {
                    origin: SubmitOrigin::Quick { generation },
                    ..
                } => self.operator_quick_failed(*generation, message),
                _ => {}
            }
            let chat_error = matches!(
                pending,
                Pending::Read { .. } | Pending::Submit { .. } | Pending::Stop { .. }
            );
            if chat_error {
                if let Some(chat) = self.operator.chat.as_mut() {
                    chat.error = Some(display_text(message));
                }
            } else {
                self.operator.error = Some(display_text(message));
            }
            // No automatic retry of a refused mutation or failed read.
            return true;
        }
        match (pending, response) {
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
                    OpenOrigin::Manual => self.operator.context.clone(),
                    OpenOrigin::Quick(submission) => submission.context.clone(),
                };
                if !self.operator_attach(summary.clone(), context) {
                    match origin {
                        OpenOrigin::Manual => self.operator.error = Some(UNAVAILABLE_REASON.into()),
                        OpenOrigin::Quick(submission) => {
                            self.operator_quick_failed(submission.generation, UNAVAILABLE_REASON)
                        }
                    }
                    return true;
                }
                match &origin {
                    OpenOrigin::Manual => self.operator_read(),
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
                    origin,
                },
                Response::Operator(OperatorResponse::Submitted(turn)),
            ) if turn.identity.conversation == id
                && turn.identity.session == session
                && turn.identity.run.0 == expected_run.0.saturating_add(1)
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
            (pending, _) => {
                match pending {
                    Pending::Open {
                        origin: OpenOrigin::Quick(submission),
                    } => self.operator_quick_failed(submission.generation, UNAVAILABLE_REASON),
                    Pending::Submit {
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
        if summary.run.0 < chat.summary.run.0 {
            return false;
        }
        if chat.summary.current_turn.is_some() && summary.current_turn.is_none() {
            return false;
        }
        if let (Some(old), Some(new)) = (&chat.summary.current_turn, &summary.current_turn)
            && (new.identity.run.0 < old.identity.run.0
                || (new.identity.run == old.identity.run && new.identity != old.identity)
                || (new.identity == old.identity
                    && (new.sequence < old.sequence
                        || (chat.summary.worker_stopped && !summary.worker_stopped))))
        {
            return false;
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
                OperatorMessageContent::Worker(OperatorWorkerEvent::ModelSelected(model)) => {
                    chat.summary.model = Some(model.clone());
                }
                OperatorMessageContent::Worker(
                    OperatorWorkerEvent::Completed | OperatorWorkerEvent::Failed(_),
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
        if history.next_after.is_some() {
            chat.dirty = true;
        } else if !chat.dirty {
            chat.confirmed_tail = Some(chat.ordinal);
        }
    }
    pub(super) fn operator_commit_presented(&mut self) {
        if !self.operator.visible || !self.operator.editing || self.operator.transcript_focus {
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
            run: SessionRunId(1),
            worker_stopped: true,
            id: Id(1),
            session: SessionId(11),
            model: None,
            title: "Operator".into(),
            current_turn: None,
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
        OperatorTurn {
            identity: OperatorTurnIdentity {
                conversation: Id(1),
                turn: OperatorTurnId(run),
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
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed),
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
        d.install_area(Rect::new(0, 0, cols, rows));
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).unwrap();
        d.draw(&mut terminal).unwrap();
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
    fn operator_is_one_navigation_row_and_opens_the_singleton_directly() {
        let mut d = dashboard();
        let rects = d.sidebar_rects(d.outer_area);
        let row = rects.operator;
        assert_eq!(row.height, 1, "exactly one fixed navigation row");
        assert_eq!(row.bottom(), 23, "the dock is the sidebar's last line");
        assert_eq!(rects.tree.y, 1, "the first project starts under the title");
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
                            text: "Review this.".into(),
                            context,
                        },
                    ),
                    message(
                        2,
                        OperatorMessageContent::Worker(OperatorWorkerEvent::ModelSelected(
                            OperatorModel {
                                provider: "openai".into(),
                                id: "gpt-6.1-sol".into(),
                            },
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
            screen.contains("Context · proj / work"),
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
        chat.expanded.insert(1);
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
        for failure in [
            "Configured Operator model is unavailable in the dedicated Pi profile.",
            "Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE.",
        ] {
            let chat = d.operator.chat.as_mut().unwrap();
            chat.messages = vec![message(
                1,
                OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(failure.into())),
            )];
            chat.dirty = false;
            chat.confirmed_tail = Some(1);
            let screen = draw(&mut d, 140, 24);
            assert!(screen.contains(failure), "{screen}");
        }
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
    fn busy_or_stop_unconfirmed_send_preserves_the_draft_with_a_reason() {
        for value in [busy(), stop_unconfirmed()] {
            let mut d = dashboard();
            let out = opened(&mut d, value);
            d.drain_outbox();
            assert!(!d.operator_submit_available());
            d.operator.chat.as_mut().unwrap().draft = "keep this".into();
            d.key(KeyCode::Enter);
            assert!(d.drain_outbox().is_empty(), "never queues or steers");
            let chat = d.operator.chat.as_ref().unwrap();
            assert_eq!(chat.draft, "keep this");
            assert_eq!(chat.error.as_deref(), Some(BUSY_REASON));
            assert!(!chat.submitting);

            // Quick shares the same draft and refuses with its own visible reason.
            let _ = out;
            d.ctrl('o');
            d.key(KeyCode::Enter);
            assert!(d.drain_outbox().is_empty());
            let quick = d.operator.quick.as_ref().unwrap();
            assert_eq!(quick.text, "keep this");
            assert_eq!(quick.error.as_deref(), Some(BUSY_REASON));
            assert_eq!(d.operator.chat.as_ref().unwrap().draft, "keep this");
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
                            text: "run it".into(),
                            context: OperatorContext::default(),
                        },
                    ),
                    message(
                        2,
                        OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                            id: "call-1".into(),
                            name: "bash".into(),
                            arguments: "{\"cmd\":\"ls\"}".into(),
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
        assert!(screen.contains("Turn complete"), "{screen}");
        assert!(
            !screen.contains("\u{1b}]52;c;INJECTED\u{7}"),
            "collapsed receipts do not leak tool output"
        );
        // Expanding keeps the source escaped and expands under its own ordinal.
        d.operator.chat.as_mut().unwrap().expanded.insert(3);
        let screen = draw(&mut d, 140, 24);
        assert!(screen.contains("succeeded"), "{screen}");
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
            let frame = frame_lines(&mut d, cols, rows);
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
    fn sidebar_geometry_reserves_one_row_and_has_no_close_target() {
        let mut d = dashboard();
        opened(&mut d, busy());
        for (cols, rows) in [(140, 40), (100, 24), (80, 24), (60, 16), (40, 12)] {
            d.install_area(Rect::new(0, 0, cols, rows));
            let rects = d.sidebar_rects(d.outer_area);
            assert!(rects.operator.height <= 1, "one row at {cols}x{rows}");
            if rects.operator.height == 1 {
                assert!(rects.tree.height >= 3, "three tree lines at {cols}x{rows}");
            }
            assert_eq!(
                rects.operator.bottom(),
                rows - 1,
                "Operator stays just above the footer at {cols}x{rows}"
            );
            assert_eq!(
                rects.tree.y, 1,
                "the tree starts under the title at {cols}x{rows}"
            );
            assert_eq!(rects.tree.bottom(), rects.attention.y);
            assert_eq!(rects.attention.bottom(), rects.quota.y);
            assert_eq!(rects.quota.bottom(), rects.operator.y);
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
    fn thinking_text_escapes_terminal_controls() {
        let mut chat = Chat::new(summary(), OperatorContext::default());
        chat.messages = vec![message(
            1,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(
                "PLAN \u{1b}]52;c;INJECTED\u{7}".into(),
            )),
        )];
        let visible = transcript_lines(&chat)
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains("Thinking"), "{visible}");
        assert!(
            visible.contains("PLAN \\u{1b}]52;c;INJECTED\\u{7}"),
            "{visible}"
        );
        assert!(!visible.contains('\u{1b}'), "{visible}");
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
        let visible = transcript_lines(&chat)
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!visible.contains("Thinking"), "{visible}");
        assert!(!visible.contains("Compaction"), "{visible}");
        assert!(visible.contains("answer"), "{visible}");
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
            transcript_lines(&chat)
                .iter()
                .any(|line| line.to_string().contains("wide 界 text"))
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
}

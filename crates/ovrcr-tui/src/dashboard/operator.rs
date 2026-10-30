//! Native chat is durable protocol data, never a terminal parser or PTY input target.
mod render;

use super::text_cursor::TextCursor;
use super::{Dashboard, DashboardAction, InputMode};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr_protocol::{
    ClientMessage, OperatorContext, OperatorConversationId as Id,
    OperatorConversationSummary as Summary, OperatorHistory, OperatorMessage,
    OperatorMessageContent, OperatorProvider, OperatorRequest, OperatorResponse,
    OperatorToolStatus, OperatorTurnId, OperatorTurnIdentity, OperatorTurnState,
    OperatorWorkerEvent, Request, Response, SessionRunId,
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
    list_focus: bool,
    transcript_focus: bool,
    sidebar_open: bool,
    pub(super) collapsed: bool,
    list_offset: usize,
    hovered: Option<Id>,
    selected: Option<Id>,
    chats: BTreeMap<u64, Chat>,
    pending: HashMap<u64, Pending>,
    listing: bool,
    list_dirty: bool,
    new: Option<NewChat>,
    context: OperatorContext,
    error: Option<String>,
}
struct Chat {
    summary: Summary,
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
    closing: bool,
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
            closing: false,
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
        !self.closing
            && !self.summary.archived
            && !self.submitting
            && self.summary.worker_stopped
            && self
                .summary
                .current_turn
                .as_ref()
                .is_none_or(|turn| turn.state != OperatorTurnState::Running)
    }
}
struct NewChat {
    provider: OperatorProvider,
    model: String,
    cursor: TextCursor,
    provider_focus: bool,
}
enum Pending {
    List,
    Read {
        id: Id,
        after: u64,
    },
    Create {
        context: OperatorContext,
    },
    Close {
        id: Id,
        session: ovrcr_protocol::SessionId,
        expected_run: SessionRunId,
    },
    Submit {
        id: Id,
        text: String,
        session: ovrcr_protocol::SessionId,
        expected_run: SessionRunId,
    },
    Stop {
        identity: OperatorTurnIdentity,
    },
}

/// Display escaped control characters literally. In particular ESC, OSC, CSI and bidi controls
/// can never reach the terminal backend as active controls. Newlines and tabs retain structure.
fn display_text(text: &str) -> String {
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
fn default_model(dashboard: &Dashboard, provider: OperatorProvider) -> String {
    match provider {
        OperatorProvider::ChatGpt => dashboard
            .settings
            .operator
            .chat_gpt_model
            .clone()
            .unwrap_or_else(|| "gpt-6.1-sol".into()),
        OperatorProvider::Grok => dashboard
            .settings
            .operator
            .grok_model
            .clone()
            .unwrap_or_else(|| "grok-4.7".into()),
    }
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
    pub(super) fn operator_clear_hover(&mut self) {
        self.operator.hovered = None;
    }
    pub(super) fn operator_sidebar_requested(&self) -> bool {
        self.operator.visible && self.operator.sidebar_open
    }
    pub(super) fn operator_sidebar_on_demand(&self) -> bool {
        self.operator.visible && self.outer_area.width < 100 && !self.operator.sidebar_open
    }
    pub(super) fn operator_has_form(&self) -> bool {
        self.operator.new.is_some()
    }
    pub(super) fn operator_transcript_focused(&self) -> bool {
        self.operator.visible && self.operator.transcript_focus
    }
    pub(super) fn operator_stop_available(&self) -> bool {
        self.operator.new.is_none()
            && self
                .operator
                .selected
                .and_then(|id| self.operator.chats.get(&id.0))
                .is_some_and(|chat| {
                    !chat.summary.worker_stopped
                        || chat
                            .summary
                            .current_turn
                            .as_ref()
                            .is_some_and(|turn| turn.state == OperatorTurnState::Running)
                })
    }
    pub(super) fn operator_header_rect(&self, area: Rect) -> Rect {
        let sidebar =
            super::sidebar_width_for(area, self.sidebar_preference()).max(area.width.min(35));
        let indent = sidebar.min(7);
        Rect::new(
            area.x.saturating_add(indent),
            area.y,
            sidebar.saturating_sub(indent),
            area.height.min(1),
        )
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
    pub(super) fn open_operator(&mut self) -> DashboardAction {
        if !self.operator.visible {
            self.operator.context = self.capture_operator_context();
            self.retarget(super::state::PaneChange::Focus);
        }
        self.operator.visible = true;
        self.operator.editing = true;
        self.mode = InputMode::Browse;
        if self.operator.chats.is_empty() && self.operator.new.is_none() {
            self.operator_new();
        }
        self.operator_list(0);
        self.operator_catch_up();
        DashboardAction::Redraw
    }
    pub(super) fn operator_start_listing(&mut self) {
        self.operator_list(0);
    }
    fn operator_new(&mut self) {
        let provider = self.settings.operator.provider;
        self.operator.new = Some(NewChat {
            provider,
            model: default_model(self, provider),
            cursor: TextCursor::default(),
            provider_focus: false,
        });
        self.operator.list_focus = false;
        self.operator.transcript_focus = false;
        self.operator.editing = true;
        self.operator.error = None;
    }
    fn operator_request(&mut self, pending: Pending, request: OperatorRequest) {
        let request_id = self.next_request_id();
        self.operator.pending.insert(request_id, pending);
        self.push_request(ClientMessage {
            request_id,
            request: Request::Operator(request),
        });
    }
    fn operator_list(&mut self, after: u64) {
        if self.operator.listing {
            self.operator.list_dirty = true;
            return;
        }
        self.operator.listing = true;
        self.operator_request(Pending::List, OperatorRequest::List { after, limit: 100 });
    }
    fn operator_catch_up(&mut self) {
        let ids: Vec<_> = self
            .operator
            .chats
            .iter()
            .filter_map(|(id, chat)| (chat.dirty && !chat.reading).then_some(Id(*id)))
            .collect();
        for id in ids {
            let chat = self.operator.chats.get_mut(&id.0).unwrap();
            chat.dirty = false;
            chat.reading = true;
            chat.confirmed_tail = None;
            let after = chat.ordinal;
            self.operator_request(
                Pending::Read { id, after },
                OperatorRequest::Read {
                    conversation: id,
                    after,
                    limit: 100,
                },
            );
        }
    }
    pub(super) fn operator_changed(&mut self, id: Id) {
        if let Some(chat) = self.operator.chats.get_mut(&id.0) {
            chat.dirty = true;
            chat.confirmed_tail = None;
        }
        self.operator_list(0);
        self.operator_catch_up();
    }
    fn operator_select(&mut self, id: Id) {
        if self
            .operator
            .chats
            .get(&id.0)
            .is_some_and(|chat| !chat.summary.archived)
        {
            self.operator.selected = Some(id);
            self.operator.new = None;
            self.operator.error = None;
            self.operator.list_focus = false;
            self.operator.transcript_focus = false;
            self.operator.sidebar_open = false;
            self.operator.editing = true;
            self.operator_ensure_visible();
            self.operator_catch_up();
        }
    }
    pub(super) fn operator_close_target(
        &self,
        id: Id,
    ) -> Option<(ovrcr_protocol::SessionId, SessionRunId, String, bool, bool)> {
        let chat = self.operator.chats.get(&id.0)?;
        (!chat.summary.archived && !chat.closing).then(|| {
            (
                chat.summary.session,
                chat.summary.run,
                display_text(&chat.summary.title),
                !chat.summary.worker_stopped,
                !chat.draft.is_empty(),
            )
        })
    }
    pub(super) fn operator_close(&mut self, id: Id, expected_run: SessionRunId) {
        let Some(chat) = self.operator.chats.get_mut(&id.0) else {
            return;
        };
        if chat.summary.archived || chat.closing || chat.summary.run != expected_run {
            return;
        }
        let session = chat.summary.session;
        chat.closing = true;
        self.operator_request(
            Pending::Close {
                id,
                session,
                expected_run,
            },
            OperatorRequest::Close {
                conversation: id,
                expected_run,
            },
        );
    }
    fn operator_submit(&mut self) {
        let Some(id) = self.operator.selected else {
            return;
        };
        let Some(chat) = self.operator.chats.get_mut(&id.0) else {
            return;
        };
        if !chat.can_submit() {
            chat.error = Some(
                "This conversation is busy or its worker stop is unconfirmed; draft retained."
                    .into(),
            );
            return;
        }
        if chat.draft.trim().is_empty() {
            return;
        }
        let text = chat.draft.clone();
        let context = chat.context.clone();
        let expected_run = chat.summary.run;
        let session = chat.summary.session;
        chat.submitting = true;
        chat.error = None;
        self.operator_request(
            Pending::Submit {
                id,
                text: text.clone(),
                session,
                expected_run,
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
        if self.operator.new.is_some() {
            return;
        }
        let Some(chat) = self
            .operator
            .selected
            .and_then(|id| self.operator.chats.get_mut(&id.0))
        else {
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
        if super::input::is_browse_key(key) || key.code == KeyCode::Esc {
            self.operator.editing = false;
            self.mode = InputMode::Browse;
            return DashboardAction::EnterBrowse;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('n') => self.operator_new(),
                KeyCode::Char('x') => self.operator_stop(),
                KeyCode::Char('l') => {
                    self.operator.list_focus = !self.operator.list_focus;
                    self.operator.transcript_focus = false;
                    self.operator.sidebar_open = self.operator.list_focus;
                    if self.operator.selected.is_none() {
                        self.operator.selected = self.operator_ids().first().copied();
                    }
                    if self.operator.list_focus && self.operator.selected.is_some() {
                        self.operator.new = None;
                        self.operator_set_collapsed(false);
                        self.operator_ensure_visible();
                    }
                }
                _ => {}
            }
            return DashboardAction::Redraw;
        }
        if self.operator.new.is_none() && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            let focus = if self.operator.list_focus {
                0
            } else if self.operator.transcript_focus {
                1
            } else {
                2
            };
            let next = (focus + if key.code == KeyCode::BackTab { 2 } else { 1 }) % 3;
            self.operator.list_focus = next == 0;
            self.operator.transcript_focus = next == 1;
            self.operator.sidebar_open = self.operator.list_focus;
            self.operator_clear_hover();
            let layout = self.operator_layout(self.outer_area);
            if next == 1
                && let Some(chat) = self
                    .operator
                    .selected
                    .and_then(|id| self.operator.chats.get_mut(&id.0))
            {
                let rows = transcript_rows(chat, layout.transcript.width);
                let offset = transcript_offset(chat, &rows, layout.transcript.height as usize);
                let visible = &rows[offset.min(rows.len())
                    ..rows.len().min(offset + layout.transcript.height as usize)];
                if !visible
                    .iter()
                    .any(|(_, anchor)| Some(anchor.0) == chat.entry_focus)
                {
                    chat.entry_focus = visible.first().map(|(_, anchor)| anchor.0);
                }
            }
            self.operator_ensure_visible();
            return DashboardAction::Redraw;
        }
        if self.operator.transcript_focus {
            if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
                self.operator_scroll(if key.code == KeyCode::PageUp { -10 } else { 10 });
                return DashboardAction::Redraw;
            }
            let layout = self.operator_layout(self.outer_area);
            if let Some(chat) = self
                .operator
                .selected
                .and_then(|id| self.operator.chats.get_mut(&id.0))
            {
                let entries: Vec<_> = chat
                    .messages
                    .iter()
                    .filter(|m| expandable(m))
                    .map(|m| m.ordinal)
                    .collect();
                let index = entries
                    .iter()
                    .position(|id| Some(*id) == chat.entry_focus)
                    .unwrap_or(0);
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
                    KeyCode::Up => chat.entry_focus = entries.get(index.saturating_sub(1)).copied(),
                    KeyCode::Down => {
                        chat.entry_focus = entries
                            .get((index + 1).min(entries.len().saturating_sub(1)))
                            .copied()
                    }
                    _ => {}
                }
                if matches!(
                    key.code,
                    KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End
                ) {
                    let rows = transcript_rows(chat, layout.transcript.width);
                    let offset = transcript_offset(chat, &rows, layout.transcript.height as usize);
                    if let Some(target) = rows
                        .iter()
                        .position(|(_, anchor)| Some(anchor.0) == chat.entry_focus)
                        && (target < offset || target >= offset + layout.transcript.height as usize)
                    {
                        chat.follow = false;
                        chat.anchor = Some(rows[target].1);
                        chat.scroll = rows
                            .len()
                            .saturating_sub(layout.transcript.height as usize)
                            .saturating_sub(target);
                    }
                }
            }
            return DashboardAction::Redraw;
        }
        if self.operator.list_focus {
            let ids = self.operator_ids();
            let current = ids
                .iter()
                .position(|id| Some(*id) == self.operator.selected)
                .unwrap_or(0);
            let next = match key.code {
                KeyCode::Up | KeyCode::Char('k') => current.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    (current + 1).min(ids.len().saturating_sub(1))
                }
                KeyCode::Char('x') => {
                    if let Some(id) = self.operator.selected {
                        return self.open_close_operator_for(id);
                    }
                    return DashboardAction::None;
                }
                KeyCode::Left => {
                    self.operator_set_collapsed(true);
                    return DashboardAction::Redraw;
                }
                KeyCode::Right => {
                    self.operator_set_collapsed(false);
                    return DashboardAction::Redraw;
                }
                KeyCode::Enter => {
                    if let Some(id) = self.operator.selected.or_else(|| ids.first().copied()) {
                        self.operator_select(id);
                    }
                    return DashboardAction::Redraw;
                }
                _ => return DashboardAction::None,
            };
            if let Some(id) = ids.get(next) {
                self.operator.selected = Some(*id);
                self.operator_set_collapsed(false);
                self.operator.new = None;
                self.operator_ensure_visible();
            }
            return DashboardAction::Redraw;
        }
        if let Some(form) = &mut self.operator.new {
            match key.code {
                KeyCode::Tab | KeyCode::BackTab => form.provider_focus = !form.provider_focus,
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.provider_focus => {
                    let provider = match form.provider {
                        OperatorProvider::ChatGpt => OperatorProvider::Grok,
                        OperatorProvider::Grok => OperatorProvider::ChatGpt,
                    };
                    let model = default_model(self, provider);
                    let form = self.operator.new.as_mut().unwrap();
                    form.provider = provider;
                    form.model = model;
                    form.cursor = TextCursor::default();
                }
                KeyCode::Enter => {
                    let model = form.model.trim().to_owned();
                    if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control)
                    {
                        self.operator.error =
                            Some("Choose a model identifier (1–256 bytes, no controls).".into());
                    } else if !self
                        .operator
                        .pending
                        .values()
                        .any(|pending| matches!(pending, Pending::Create { .. }))
                    {
                        let provider = form.provider;
                        self.operator_request(
                            Pending::Create {
                                context: self.operator.context.clone(),
                            },
                            OperatorRequest::Create { provider, model },
                        );
                    }
                }
                KeyCode::Char(ch) if !form.provider_focus && !ch.is_control() => {
                    form.cursor.insert(&mut form.model, &ch.to_string())
                }
                code if !form.provider_focus => form.cursor.key(&mut form.model, code),
                _ => {}
            }
            return DashboardAction::Redraw;
        }
        if self
            .operator
            .selected
            .and_then(|id| self.operator.chats.get(&id.0))
            .is_some_and(|chat| chat.closing)
        {
            return DashboardAction::None;
        }
        match key.code {
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.operator_insert("\n")
            }
            KeyCode::Enter => self.operator_submit(),
            KeyCode::Up | KeyCode::Down => {
                if let Some(chat) = self
                    .operator
                    .selected
                    .and_then(|id| self.operator.chats.get_mut(&id.0))
                {
                    chat.cursor.vertical(&chat.draft, key.code == KeyCode::Down);
                }
            }
            KeyCode::PageUp => self.operator_scroll(-10),
            KeyCode::PageDown => self.operator_scroll(10),
            KeyCode::Char(ch) if !ch.is_control() => self.operator_insert(&ch.to_string()),
            code => {
                if let Some(chat) = self
                    .operator
                    .selected
                    .and_then(|id| self.operator.chats.get_mut(&id.0))
                {
                    chat.cursor.key(&mut chat.draft, code);
                }
            }
        }
        DashboardAction::Redraw
    }
    pub(super) fn operator_sidebar_focused(&self) -> bool {
        self.operator.visible && self.operator.list_focus
    }
    pub(super) fn operator_is_editing(&self) -> bool {
        self.operator.visible && self.operator.editing
    }
    fn operator_insert(&mut self, text: &str) {
        if let Some(chat) = self
            .operator
            .selected
            .and_then(|id| self.operator.chats.get_mut(&id.0))
        {
            // Match the protocol's user-text bound without splitting UTF-8.
            if !chat.closing && chat.draft.len().saturating_add(text.len()) <= 64 * 1024 {
                chat.cursor.insert(&mut chat.draft, text);
            }
        }
    }
    pub(super) fn operator_paste(&mut self, text: &str) -> DashboardAction {
        if self.operator.list_focus || self.operator.transcript_focus {
            return DashboardAction::None;
        }
        let text = paste_text(text);
        if let Some(form) = &mut self.operator.new {
            if !form.provider_focus && form.model.len().saturating_add(text.len()) <= 256 {
                form.cursor
                    .insert(&mut form.model, &text.replace(['\n', '\t'], ""));
            }
        } else {
            self.operator_insert(&text);
        }
        DashboardAction::Redraw
    }
    fn operator_scroll(&mut self, delta: isize) {
        let layout = self.operator_layout(self.outer_area);
        let height = layout.transcript.height as usize;
        if let Some(chat) = self
            .operator
            .selected
            .and_then(|id| self.operator.chats.get_mut(&id.0))
        {
            let rows = transcript_rows(chat, layout.transcript.width);
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
            chat.anchor = rows.get(target).map(|(_, start)| *start);
        }
    }
    pub(super) fn operator_mouse(
        &mut self,
        mouse: MouseEvent,
        area: Rect,
    ) -> Option<DashboardAction> {
        let point = ratatui::layout::Position::new(mouse.column, mouse.row);
        if let Some(action) = self.operator_sidebar_mouse(mouse, area) {
            return Some(action);
        }
        if self.operator_header_rect(area).contains(point)
            && mouse.kind == MouseEventKind::Down(MouseButton::Left)
        {
            return Some(self.open_operator());
        }
        if !self.operator.visible {
            return None;
        }
        let content = content_area(self, area);
        if !content.contains(point) {
            return None;
        }
        let layout = self.operator_layout(area);
        let transcript = layout.transcript;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if !transcript.is_empty() => {
                if stop_rect(layout.header).contains(point) && self.operator.new.is_none() {
                    self.operator_stop();
                } else {
                    self.operator.editing = true;
                    self.operator.list_focus = false;
                    self.operator.sidebar_open = false;
                    self.operator.transcript_focus = transcript.contains(point);
                    if self.operator.transcript_focus
                        && let Some(chat) = self
                            .operator
                            .selected
                            .and_then(|id| self.operator.chats.get_mut(&id.0))
                    {
                        let rows = transcript_rows(chat, transcript.width);
                        let offset = transcript_offset(chat, &rows, transcript.height as usize);
                        chat.entry_focus = rows
                            .get(offset + usize::from(mouse.row - transcript.y))
                            .map(|(_, anchor)| anchor.0)
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
            Pending::List => self.operator.listing = false,
            Pending::Read { id, .. } => {
                if let Some(chat) = self.operator.chats.get_mut(&id.0) {
                    chat.reading = false;
                }
            }
            Pending::Submit { id, .. } => {
                if let Some(chat) = self.operator.chats.get_mut(&id.0) {
                    chat.submitting = false;
                }
            }
            Pending::Stop { identity } => {
                if let Some(chat) = self.operator.chats.get_mut(&identity.conversation.0) {
                    chat.stopping = false;
                }
            }
            Pending::Close { id, .. } => {
                if let Some(chat) = self.operator.chats.get_mut(&id.0) {
                    chat.closing = false;
                }
            }
            Pending::Create { .. } => {}
        }
        if let Response::Error { message, .. } = response {
            let id = match &pending {
                Pending::Read { id, .. }
                | Pending::Submit { id, .. }
                | Pending::Close { id, .. } => Some(*id),
                Pending::Stop { identity } => Some(identity.conversation),
                Pending::List | Pending::Create { .. } => None,
            };
            if let Some(chat) = id.and_then(|id| self.operator.chats.get_mut(&id.0)) {
                chat.error = Some(display_text(message));
            } else {
                self.operator.error = Some(display_text(message));
            }
            // No automatic retry of a refused mutation or failed read.
            return true;
        }
        match (pending, response) {
            (
                Pending::Close {
                    id,
                    session,
                    expected_run,
                },
                Response::Operator(OperatorResponse::Closed(summary)),
            ) if summary.id == id
                && summary.session == session
                && summary.archived
                && summary.worker_stopped
                && summary.run.0 == expected_run.0.saturating_add(1) =>
            {
                self.operator_summary(summary.clone());
                if let Some(chat) = self.operator.chats.get_mut(&id.0)
                    && chat.summary.archived
                    && chat.summary.run == summary.run
                {
                    chat.draft.clear();
                    chat.cursor = TextCursor::default();
                }
                if self.operator.selected == Some(id) && self.operator.chats[&id.0].summary.archived
                {
                    self.operator.selected = self.operator_ids().first().copied();
                    if self.operator.selected.is_none() {
                        self.operator_new();
                    }
                }
                self.operator_ensure_visible();
            }
            (Pending::List, Response::Operator(OperatorResponse::Conversations(summaries))) => {
                for summary in summaries {
                    self.operator_summary(summary.clone());
                }
                if summaries.len() == 100 {
                    if let Some(last) = summaries.last() {
                        self.operator_list(last.id.0);
                    }
                } else if std::mem::take(&mut self.operator.list_dirty) {
                    self.operator_list(0);
                }
                self.operator_catch_up();
            }
            (
                Pending::Read { id, after },
                Response::Operator(OperatorResponse::History(history)),
            ) if history.conversation.id == id => {
                self.operator_history(id, after, history);
                self.operator_catch_up();
            }
            (
                Pending::Create { context },
                Response::Operator(OperatorResponse::Created(summary)),
            ) => {
                self.operator_summary(summary.clone());
                if let Some(chat) = self.operator.chats.get_mut(&summary.id.0) {
                    chat.context = context;
                }
                if self.operator.visible && self.operator.new.is_some() {
                    self.operator_select(summary.id);
                }
                self.operator_catch_up();
            }
            (
                Pending::Submit {
                    id,
                    text,
                    session,
                    expected_run,
                },
                Response::Operator(OperatorResponse::Submitted(turn)),
            ) if turn.identity.conversation == id
                && turn.identity.session == session
                && turn.identity.run.0 == expected_run.0.saturating_add(1)
                && turn.identity.validate().is_ok() =>
            {
                if let Some(chat) = self.operator.chats.get_mut(&id.0) {
                    if chat.draft == text {
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
                self.operator_list(0);
                self.operator_catch_up();
            }
            (Pending::Stop { identity }, Response::Operator(OperatorResponse::Stopped(turn)))
                if turn.identity == identity =>
            {
                if let Some(chat) = self.operator.chats.get_mut(&identity.conversation.0) {
                    chat.dirty = true;
                }
                // The response retires authority; only a refreshed summary certifies process stop.
                self.operator_list(0);
                self.operator_catch_up();
            }
            _ => {
                self.operator.error =
                    Some("Operator response did not match its requested conversation.".into());
            }
        }
        true
    }
    fn operator_summary(&mut self, summary: Summary) {
        let previous_tree = self.sidebar_rects(self.outer_area).tree;
        let id = summary.id;
        if summary.current_turn.as_ref().is_some_and(|turn| {
            turn.identity.validate().is_err()
                || turn.identity.conversation != id
                || turn.identity.session != summary.session
                || turn.identity.run.0 > summary.run.0
        }) {
            return;
        }
        if let Some(chat) = self.operator.chats.get_mut(&id.0) {
            if chat.summary.session != summary.session
                || chat.summary.provider != summary.provider
                || chat.summary.model != summary.model
            {
                return;
            }
            if summary.run.0 < chat.summary.run.0 {
                return;
            }
            if chat.summary.current_turn.is_some() && summary.current_turn.is_none() {
                return;
            }
            if let (Some(old), Some(new)) = (&chat.summary.current_turn, &summary.current_turn)
                && (new.identity.run.0 < old.identity.run.0
                    || (new.identity.run == old.identity.run && new.identity != old.identity)
                    || (new.identity == old.identity
                        && (new.sequence < old.sequence
                            || (chat.summary.worker_stopped && !summary.worker_stopped))))
            {
                return;
            }
            chat.summary = summary;
        } else {
            self.operator
                .chats
                .insert(id.0, Chat::new(summary, OperatorContext::default()));
        }
        if !self.operator.chats[&id.0].summary.archived
            && self.operator.selected.is_none()
            && self.operator.new.is_none()
        {
            self.operator.selected = Some(id);
        }
        self.operator_reconcile_tree(previous_tree);
    }
    fn operator_history(&mut self, id: Id, after: u64, history: &OperatorHistory) {
        self.operator_summary(history.conversation.clone());
        let Some(chat) = self.operator.chats.get_mut(&id.0) else {
            return;
        };
        if chat.ordinal != after || history.conversation.session != chat.summary.session {
            return;
        }
        let mut previous = after;
        for message in &history.messages {
            if message.ordinal <= previous
                || message.turn.0 == 0
                || history.conversation.session != chat.summary.session
            {
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
            if let OperatorMessageContent::User { context, .. } = &message.content
                && chat.draft.is_empty()
                && !chat.submitting
            {
                chat.context = context.clone();
            }
            if matches!(
                message.content,
                OperatorMessageContent::Worker(
                    OperatorWorkerEvent::Completed(_) | OperatorWorkerEvent::Failed(_)
                ) | OperatorMessageContent::Interrupted
            ) && chat.reviewed != Some(message.turn)
            {
                chat.unread = Some(message.turn);
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
        if !self.operator.visible
            || !self.operator.editing
            || self.operator.new.is_some()
            || self.operator.list_focus
        {
            return;
        }
        let layout = self.operator_layout(self.outer_area);
        if self.outer_area.width < 60
            || self.outer_area.height < 16
            || layout.transcript.height < 3
            || layout.transcript.width < 6
            || (self.outer_area.width < 100 && self.operator.sidebar_open)
        {
            return;
        }
        if let Some(chat) = self
            .operator
            .selected
            .and_then(|id| self.operator.chats.get_mut(&id.0))
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
    pub(super) fn draw_operator_header(&self, frame: &mut Frame<'_>) {
        let unread = self
            .operator
            .chats
            .values()
            .filter(|chat| !chat.summary.archived && chat.unread.is_some())
            .count();
        let label = if unread > 0 {
            format!(" Operator [o] · {unread} unread")
        } else {
            " Operator [o]".into()
        };
        frame.render_widget(
            Paragraph::new(label).style(Style::default().fg(super::render::MAUVE()).bg(
                if self.operator.visible {
                    super::render::SURFACE0()
                } else {
                    super::render::BASE()
                },
            )),
            self.operator_header_rect(frame.area()),
        );
    }
    pub(super) fn draw_operator(&self, frame: &mut Frame<'_>) {
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
        let right = area;
        if frame.area().width < 60 || frame.area().height < 16 {
            frame.render_widget(
                Paragraph::new(format!("Operator needs 60×16 · current {}×{}\nResize to continue · Esc / Ctrl-g Browse", frame.area().width, frame.area().height))
                    .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        if right.is_empty() {
            return;
        }
        if let Some(form) = &self.operator.new {
            let mut model = form.model.clone();
            if self.operator.editing && !form.provider_focus {
                model.insert(form.cursor.position(&model), '▏');
            }
            let model = display_text(&model);
            let text = format!(
                "New Conversation\n\nProvider: {} {}\nModel: {model}\n\nTab: provider/model · ←/→: choose provider\nCtrl-l: conversations\nEnter: create · Ctrl-g: Browse\nSubscription sign-in: ovrcr operator login {}\n\n{}",
                form.provider.as_str(),
                if form.provider_focus { "◀" } else { "" },
                match form.provider {
                    OperatorProvider::ChatGpt => "chatgpt",
                    OperatorProvider::Grok => "grok",
                },
                self.operator.error.as_deref().unwrap_or("")
            );
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().borders(Borders::ALL)),
                right,
            );
            return;
        }
        let Some(chat) = self
            .operator
            .selected
            .and_then(|id| self.operator.chats.get(&id.0))
        else {
            return;
        };
        self.draw_operator_conversation(frame, chat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::Event;
    use ovrcr_protocol::{
        OperatorModelHistory, OperatorTurn, OperatorTurnId, ServerEvent, ServerMessage, SessionId,
        TerminalSize,
    };
    fn summary(id: u64, run: u64, state: OperatorTurnState, stopped: bool) -> Summary {
        Summary {
            run: SessionRunId(run),
            archived: false,
            id: Id(id),
            session: SessionId(id + 10),
            provider: OperatorProvider::ChatGpt,
            model: "fixture".into(),
            title: format!("chat-{id}"),
            worker_stopped: stopped,
            current_turn: Some(OperatorTurn {
                identity: OperatorTurnIdentity {
                    conversation: Id(id),
                    turn: OperatorTurnId(run),
                    session: SessionId(id + 10),
                    run: SessionRunId(run),
                },
                state,
                sequence: 1,
            }),
        }
    }
    fn request_id(requests: &[ClientMessage], id: u64) -> u64 {
        requests
            .iter()
            .find_map(|message| match &message.request {
                Request::Operator(OperatorRequest::Read { conversation, .. })
                    if *conversation == Id(id) =>
                {
                    Some(message.request_id)
                }
                _ => None,
            })
            .unwrap()
    }
    fn setup() -> (Dashboard, Vec<ClientMessage>) {
        let mut d = Dashboard::new(TerminalSize {
            rows: 24,
            cols: 140,
        });
        d.key(KeyCode::Char('o'));
        let requests = d.drain_outbox();
        let list = requests
            .iter()
            .find(|message| {
                matches!(
                    message.request,
                    Request::Operator(OperatorRequest::List { .. })
                )
            })
            .unwrap();
        let reads = d.handle_server_message(ServerMessage::Response {
            request_id: list.request_id,
            response: Response::Operator(OperatorResponse::Conversations(vec![
                summary(1, 1, OperatorTurnState::Running, false),
                summary(2, 1, OperatorTurnState::Running, false),
            ])),
        });
        d.operator_select(Id(1));
        (d, reads)
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
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed(OperatorModelHistory {
                version: 1,
                json: "[]".into(),
            })),
        )
    }
    fn history(
        d: &mut Dashboard,
        request: u64,
        summary: Summary,
        messages: Vec<OperatorMessage>,
        next_after: Option<u64>,
    ) -> Vec<ClientMessage> {
        d.handle_server_message(ServerMessage::Response {
            request_id: request,
            response: Response::Operator(OperatorResponse::History(OperatorHistory {
                conversation: summary,
                messages,
                next_after,
            })),
        })
    }
    fn draw(d: &mut Dashboard, cols: u16, rows: u16) -> String {
        d.install_area(Rect::new(0, 0, cols, rows));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(cols, rows)).unwrap();
        d.draw(&mut terminal).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }
    #[test]
    fn operator_original_typed_event_matrix_and_context_details() {
        let (mut d, _) = setup();
        let context = OperatorContext {
            project: Some("project".repeat(14)),
            workspace: Some("workspace".repeat(14)),
            workspace_id: Some("stable-workspace".into()),
            cwd: Some(std::path::PathBuf::from("/private/a/very/long/path")),
            session: Some(SessionId(11)),
            run: Some(SessionRunId(7)),
        };
        let chat = d.operator.chats.get_mut(&1).unwrap();
        chat.context = context.clone();
        chat.summary.title = "long-title-".repeat(80);
        chat.summary.model = "long-model-".repeat(80);
        chat.messages = vec![
            message(
                1,
                OperatorMessageContent::User {
                    text: "hello\n\n界é🙂".into(),
                    context: context.clone(),
                },
            ),
            message(
                2,
                OperatorMessageContent::User {
                    text: "same context".into(),
                    context: context.clone(),
                },
            ),
            message(
                3,
                OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                    id: "bash".into(),
                    name: "Bash".into(),
                    arguments: "{\"command\":\"pwd\",\"cwd\":\"/private\"}".into(),
                }),
            ),
        ];
        for (i, status) in [
            OperatorToolStatus::Succeeded,
            OperatorToolStatus::Failed { exit_code: Some(9) },
            OperatorToolStatus::TimedOut,
        ]
        .into_iter()
        .enumerate()
        {
            chat.messages.push(message(
                4 + i as u64,
                OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                    id: "bash".into(),
                    output: if i == 0 {
                        String::new()
                    } else {
                        "\u{1b}[31munsafe\u{202e}\n\nlast".into()
                    },
                    truncated: i == 2,
                    status,
                }),
            ));
        }
        chat.messages.push(message(
            7,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed(OperatorModelHistory {
                version: 1,
                json: "[\"PRIVATE_MODEL_HISTORY\"]".into(),
            })),
        ));
        let compact = transcript_lines(chat)
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(compact.matches("Context ·").count(), 1);
        for status in [
            "succeeded",
            "(no output)",
            "failed (exit 9)",
            "timed out",
            "truncated at 64 KiB",
        ] {
            assert!(compact.contains(status), "{compact}");
        }
        assert!(!compact.contains("PRIVATE_MODEL_HISTORY") && !compact.contains("\"command\""));
        chat.expanded.extend([1, 3, 5, 6]);
        let expanded = transcript_lines(chat)
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        for value in [
            "stable-workspace",
            "/private/a/very/long/path",
            "session: 11 · run: 7",
            "\"command\":\"pwd\"",
            "\\u{1b}",
            "\\u{202e}",
            "\n  \n  last",
            "hello\n\n界é🙂",
        ] {
            assert!(expanded.contains(value), "missing {value}: {expanded}");
        }
        assert!(
            !expanded.contains('\u{1b}')
                && !expanded.contains('\u{202e}')
                && !expanded.contains("PRIVATE_MODEL_HISTORY")
        );
        // Rendered failure uses semantic error styling independently of snapshots.
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 40)).unwrap();
        d.install_area(Rect::new(0, 0, 140, 40));
        d.operator.chats.get_mut(&1).unwrap().expanded.clear();
        d.draw(&mut terminal).unwrap();
        let buffer = terminal.backend().buffer();
        let header: String = (40..140).map(|x| buffer[(x, 1)].symbol()).collect();
        assert!(
            header.contains("chat_gpt")
                && header.contains("running")
                && header.contains("Stop Ctrl-x"),
            "long header: {header}"
        );
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| cell.symbol() == "f" && cell.fg == super::super::render::RED())
        );
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| cell.symbol() == "▌" && cell.bg == super::super::render::SURFACE0())
        );
        let caption: String = (0..140)
            .map(|x| buffer[(x, d.operator_layout(d.outer_area).context.y)].symbol())
            .collect();
        assert!(caption.contains("#11 run 7"), "context suffix: {caption}");
    }
    #[test]
    fn operator_original_composer_geometry_focus_and_unread_guards() {
        let (mut d, reads) = setup();
        history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 1, OperatorTurnState::Complete, true),
            vec![delta(1, "answer"), completed(2)],
            None,
        );
        d.install_area(Rect::new(0, 0, 60, 16));
        assert_eq!(d.operator_layout(d.outer_area).composer.height, 2);
        assert_eq!(content_area(&d, d.outer_area).width, 60);
        d.operator_insert(&"line\n".repeat(20));
        assert_eq!(d.operator_layout(d.outer_area).composer.height, 4);
        d.key(KeyCode::Tab);
        assert!(d.operator.list_focus && !d.operator.transcript_focus);
        assert!(content_area(&d, d.outer_area).width < 60);
        draw(&mut d, 60, 16);
        assert!(d.operator.chats[&1].unread.is_some());
        d.key(KeyCode::Tab);
        assert!(d.operator.transcript_focus && !d.operator.list_focus);
        let draft = d.operator.chats[&1].draft.clone();
        d.key(KeyCode::Char('x'));
        d.event_action(Event::Paste("ignored".into()));
        assert_eq!(d.operator.chats[&1].draft, draft);
        d.key(KeyCode::Tab);
        assert!(!d.operator.transcript_focus && !d.operator.list_focus);
        d.key(KeyCode::Char('x'));
        assert!(d.operator.chats[&1].draft.ends_with('x'));
        draw(&mut d, 60, 15);
        assert!(d.operator.chats[&1].unread.is_some());
        draw(&mut d, 60, 16);
        assert!(d.operator.chats[&1].unread.is_none());
        assert!(!draw(&mut d, 100, 24).contains("[Stop Ctrl-x]"));
        assert_eq!(
            d.sidebar_width, None,
            "responsive hiding cannot alter saved width"
        );
        d.operator.hovered = Some(Id(1));
        d.event_action(Event::FocusLost);
        assert_eq!(d.operator.hovered, None);
        d.operator.hovered = Some(Id(1));
        d.toggle_sidebar();
        assert_eq!(d.operator.hovered, None);
        d.ctrl('l');
        assert!(d.operator.list_focus);
        assert!(
            content_area(&d, d.outer_area).width < d.outer_area.width,
            "on-demand sidebar opens even with a saved hidden preference"
        );
        assert!(
            d.sidebar_hidden,
            "opening on demand cannot overwrite hidden preference"
        );
        d.ctrl('l');
        assert_eq!(content_area(&d, d.outer_area).width, d.outer_area.width);
        assert!(d.sidebar_hidden);
    }
    #[test]
    fn operator_original_keyboard_expands_a_visible_focused_receipt() {
        let (mut d, _) = setup();
        let chat = d.operator.chats.get_mut(&1).unwrap();
        chat.messages = vec![message(
            1,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                id: "call".into(),
                name: "Bash".into(),
                arguments: "FULL_COMMAND_CWD".into(),
            }),
        )];
        d.key(KeyCode::Tab);
        d.key(KeyCode::Tab);
        d.key(KeyCode::Home);
        assert!(d.operator.transcript_focus);
        d.key(KeyCode::Enter);
        assert!(draw(&mut d, 140, 24).contains("FULL_COMMAND_CWD"));
        d.key(KeyCode::Enter);
        assert!(!draw(&mut d, 140, 24).contains("FULL_COMMAND_CWD"));
    }
    #[test]
    fn operator_user_context_expansion_keeps_its_own_prose_anchor() {
        let (mut d, _) = setup();
        let chat = d.operator.chats.get_mut(&1).unwrap();
        chat.messages = vec![message(
            1,
            OperatorMessageContent::User {
                text: (0..50).map(|i| format!("PROSE_{i:02}\n")).collect(),
                context: OperatorContext::default(),
            },
        )];
        draw(&mut d, 140, 24);
        d.operator_scroll(-10);
        let before = draw(&mut d, 140, 24);
        let marker = (0..50)
            .find(|i| before.contains(&format!("PROSE_{i:02}")))
            .unwrap();
        d.operator.chats.get_mut(&1).unwrap().expanded.insert(1);
        let after = draw(&mut d, 80, 24);
        assert!(after.contains(&format!("PROSE_{marker:02}")), "{after}");
    }
    #[test]
    fn operator_original_layout_floor_frames() {
        for (cols, rows) in [(140, 40), (100, 24), (80, 24), (60, 24), (60, 16)] {
            let (mut d, _) = setup();
            d.operator_summary(summary(1, 1, OperatorTurnState::Complete, true));
            d.operator_summary(summary(2, 1, OperatorTurnState::Complete, true));
            d.operator.chats.get_mut(&1).unwrap().messages = vec![
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
            ];
            let screen = draw(&mut d, cols, rows);
            let content = content_area(&d, d.outer_area);
            let lines = screen.chars().collect::<Vec<_>>();
            let main: String = lines
                .chunks(cols as usize)
                .skip(content.y as usize)
                .take(content.height as usize)
                .flat_map(|row| row[content.x as usize..].iter())
                .collect();
            assert!(
                !main.contains('┌') && !main.contains('┐'),
                "full frames at {cols}x{rows}: {main}"
            );
            let snapshot = lines
                .chunks(cols as usize)
                .map(|line| line.iter().collect::<String>().trim_end().to_owned())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            let expected = match (cols, rows) {
                (140, 40) => include_str!("operator/snapshots/original-140x40.txt"),
                (100, 24) => include_str!("operator/snapshots/original-100x24.txt"),
                (80, 24) => include_str!("operator/snapshots/original-80x24.txt"),
                (60, 24) => include_str!("operator/snapshots/original-60x24.txt"),
                (60, 16) => include_str!("operator/snapshots/original-60x16.txt"),
                _ => unreachable!(),
            };
            assert_eq!(snapshot, expected, "Original frame {cols}x{rows}");
            assert_eq!(d.operator_layout(d.outer_area).composer.height, 2);
            if cols < 100 {
                assert_eq!(content.x, 0, "conversation wins at {cols}");
            }
        }
        let (mut d, _) = setup();
        d.operator_insert("preserve draft");
        assert!(draw(&mut d, 59, 16).contains("60×16"));
        assert!(draw(&mut d, 60, 15).contains("60×16"));
        assert_eq!(d.operator.chats[&1].draft, "preserve draft");
    }
    #[test]
    fn operator_original_styled_wrapping_retains_spans() {
        let style = Style::default().fg(super::super::render::RED());
        let rows = wrapped_lines(vec![Line::from(vec![Span::styled("界é界", style)])], 3);
        assert_eq!(
            rows.iter().map(Line::to_string).collect::<Vec<_>>(),
            ["界é", "界"]
        );
        assert!(
            rows.iter()
                .all(|line| line.spans.iter().all(|span| span.style.fg == style.fg))
        );
    }
    #[test]
    fn operator_original_cursor_keeps_wide_graphemes_visible() {
        let (mut d, _) = setup();
        d.operator_insert(&format!("{}é🙂", "界".repeat(160)));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 16)).unwrap();
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
    fn operator_receipt_resize_preserves_message_anchor() {
        let (mut d, reads) = setup();
        let mut messages = vec![message(
            1,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                id: "cmd".into(),
                name: "bash".into(),
                arguments: "very long command ".repeat(80),
            }),
        )];
        messages.extend((2..30).map(|n| delta(n, &format!("MESSAGE_{n:02}\n"))));
        messages.push(completed(30));
        history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 1, OperatorTurnState::Complete, true),
            messages,
            None,
        );
        assert!(
            !transcript_lines(&d.operator.chats[&1])
                .iter()
                .any(|line| line.to_string().contains("very long command")),
            "receipt must start compact"
        );
        d.operator_scroll(-12);
        let before = draw(&mut d, 140, 24);
        let marker = (2..30)
            .find(|n| before.contains(&format!("MESSAGE_{n:02}")))
            .unwrap();
        // Expanding a receipt before the viewport must not move its durable anchor.
        d.operator.chats.get_mut(&1).unwrap().expanded.insert(1);
        assert!(
            transcript_lines(&d.operator.chats[&1])
                .iter()
                .any(|line| line.to_string().contains("very long command")),
            "expanded receipt retains full command details"
        );
        let resized = draw(&mut d, 80, 24);
        assert!(
            resized.contains(&format!("MESSAGE_{marker:02}")),
            "anchor lost: {resized}"
        );
        assert!(!d.operator.chats[&1].follow);
        assert_eq!(
            d.operator.chats[&1].unread,
            Some(OperatorTurnId(1)),
            "unseen durable tail must remain Unread"
        );
    }
    #[test]
    fn operator_sidebar_rows_share_render_and_hit_geometry() {
        let (mut d, _) = setup();
        d.operator_summary(summary(3, 1, OperatorTurnState::Complete, true));
        d.operator.visible = false;
        let rendered = draw(&mut d, 120, 24);
        let sidebar: Vec<String> = rendered
            .chars()
            .collect::<Vec<_>>()
            .chunks(120)
            .map(|row| row[..39].iter().collect())
            .collect();
        assert!(sidebar[1].contains("OPERATOR"), "{sidebar:?}");
        for id in 1..=3 {
            assert!(
                sidebar[id + 1].contains(&format!("chat-{id}")),
                "{sidebar:?}"
            );
            assert!(!sidebar[id + 1].contains("fixture"));
        }
    }
    #[test]
    fn operator_sidebar_busy_without_project_activity_ticks() {
        let (mut d, _) = setup();
        d.operator.visible = false;
        assert_eq!(d.redraw_interval(), super::super::status::SPINNER_INTERVAL);
    }
    fn click(d: &mut Dashboard, column: u16, row: u16, kind: MouseEventKind) {
        d.mouse_action(
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            d.outer_area,
        );
    }
    #[test]
    fn operator_sidebar_mouse_keyboard_scroll_and_close_share_ids() {
        let (mut d, _) = setup();
        for id in 3..=30 {
            d.operator_summary(summary(id, 1, OperatorTurnState::Running, false));
        }
        d.install_area(Rect::new(0, 0, 120, 24));
        d.operator_select(Id(30));
        let rect = d.sidebar_rects(d.outer_area).operator;
        assert_eq!(rect.height, 6);
        let offset = d.operator.list_offset;
        click(
            &mut d,
            rect.x + 3,
            rect.y + 1,
            MouseEventKind::Down(MouseButton::Left),
        );
        assert_eq!(d.operator.selected, Some(Id(offset as u64 + 1)));
        d.key(KeyCode::Down);
        assert_eq!(d.operator.selected, Some(Id(offset as u64 + 2)));
        click(&mut d, rect.x + 3, rect.y + 1, MouseEventKind::ScrollUp);
        let new_offset = d.operator.list_offset;
        click(
            &mut d,
            rect.x + 3,
            rect.y + 1,
            MouseEventKind::Down(MouseButton::Left),
        );
        assert_eq!(d.operator.selected, Some(Id(new_offset as u64 + 1)));
        click(&mut d, rect.right() - 1, rect.y + 1, MouseEventKind::Moved);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
        d.draw(&mut terminal).unwrap();
        let last: String = (rect.right() - 3..rect.right())
            .map(|x| terminal.backend().buffer()[(x, rect.y + 1)].symbol())
            .collect();
        assert_eq!(last, "[x]");
        d.key(KeyCode::Char('x'));
        assert!(d.palette.is_some());
        assert!(draw(&mut d, 120, 24).contains("unconfirmed"));
        d.key(KeyCode::Esc);
        assert!(d.palette.is_none());
        assert_eq!(d.operator.selected, Some(Id(new_offset as u64 + 1)));
    }
    #[test]
    fn operator_close_confirmation_preserves_cancel_error_and_another_selection() {
        let (mut d, _) = setup();
        d.operator_insert("discard this draft");
        d.operator.chats.get_mut(&1).unwrap().summary.run = SessionRunId(5);
        d.open_close_operator_for(Id(1));
        let rendered = draw(&mut d, 140, 24);
        assert!(rendered.contains("draft will be discarded"));
        d.key(KeyCode::Esc);
        assert_eq!(d.operator.chats[&1].draft, "discard this draft");
        d.open_close_operator_for(Id(1));
        d.key(KeyCode::Enter);
        let close = d
            .drain_outbox()
            .into_iter()
            .find(|request| {
                matches!(
                    request.request,
                    Request::Operator(OperatorRequest::Close { .. })
                )
            })
            .unwrap();
        assert_eq!(
            close.request,
            Request::Operator(OperatorRequest::Close {
                conversation: Id(1),
                expected_run: SessionRunId(5)
            })
        );
        assert!(d.operator.chats[&1].closing);
        d.operator_insert("blocked");
        assert_eq!(d.operator.chats[&1].draft, "discard this draft");
        d.handle_server_message(ServerMessage::Response {
            request_id: close.request_id,
            response: Response::Error {
                code: ovrcr_protocol::ErrorCode::Conflict,
                message: "stop uncertain".into(),
            },
        });
        assert!(!d.operator.chats[&1].closing);
        assert_eq!(d.operator.chats[&1].draft, "discard this draft");
        d.open_close_operator_for(Id(1));
        d.key(KeyCode::Enter);
        let close = d
            .drain_outbox()
            .into_iter()
            .find(|request| {
                matches!(
                    request.request,
                    Request::Operator(OperatorRequest::Close { .. })
                )
            })
            .unwrap();
        d.open_operator_conversation(Id(2));
        d.operator_insert("keep B");
        let mut closed = d.operator.chats[&1].summary.clone();
        closed.run = SessionRunId(6);
        closed.archived = true;
        closed.worker_stopped = true;
        d.handle_server_message(ServerMessage::Response {
            request_id: close.request_id,
            response: Response::Operator(OperatorResponse::Closed(closed)),
        });
        assert!(d.operator.chats[&1].draft.is_empty());
        assert_eq!(d.operator_ids(), vec![Id(2)]);
        assert_eq!(d.operator.selected, Some(Id(2)));
        assert_eq!(d.operator.chats[&2].draft, "keep B");
        assert!(
            d.operator.chats.contains_key(&1),
            "archive retains the authority tombstone"
        );
    }
    #[test]
    fn operator_sidebar_focus_reveals_a_selected_row_from_new_conversation() {
        let (mut d, _) = setup();
        d.operator_new();
        d.operator.collapsed = true;
        d.ctrl('l');
        assert!(d.operator_sidebar_focused());
        assert!(!d.operator.collapsed);
        assert!(d.operator.new.is_none());
        assert!(d.operator.selected.is_some());
    }
    #[test]
    fn operator_close_rejects_a_receipt_for_another_retained_run() {
        let (mut d, _) = setup();
        d.operator_insert("keep");
        d.open_close_operator_for(Id(1));
        d.key(KeyCode::Enter);
        let close = d
            .drain_outbox()
            .into_iter()
            .find(|request| {
                matches!(
                    request.request,
                    Request::Operator(OperatorRequest::Close { .. })
                )
            })
            .unwrap();
        let mut wrong = summary(1, 99, OperatorTurnState::Interrupted, true);
        wrong.archived = true;
        d.handle_server_message(ServerMessage::Response {
            request_id: close.request_id,
            response: Response::Operator(OperatorResponse::Closed(wrong)),
        });
        assert_eq!(d.operator.chats[&1].draft, "keep");
        assert!(!d.operator.chats[&1].summary.archived);
        assert_eq!(d.operator_ids(), vec![Id(1), Id(2)]);
    }
    #[test]
    fn operator_composer_x_is_text_and_collapsing_or_scrolling_keeps_background_unread() {
        let (mut d, _) = setup();
        d.operator.chats.get_mut(&2).unwrap().unread = Some(OperatorTurnId(1));
        d.key(KeyCode::Char('x'));
        assert_eq!(d.operator.chats[&1].draft, "x");
        assert!(d.palette.is_none());
        let rect = d.sidebar_rects(d.outer_area).operator;
        click(
            &mut d,
            rect.right() - 1,
            rect.y + 1,
            MouseEventKind::Down(MouseButton::Left),
        );
        assert!(
            d.palette.is_none(),
            "undrawn close marks are not hit targets"
        );
        click(
            &mut d,
            rect.x + 1,
            rect.y,
            MouseEventKind::Down(MouseButton::Left),
        );
        assert_eq!(d.sidebar_rects(d.outer_area).operator.height, 1);
        assert!(draw(&mut d, 140, 24).contains("▸ 2"));
        assert!(d.operator.chats[&2].unread.is_some());
        click(
            &mut d,
            rect.x + 1,
            rect.y,
            MouseEventKind::Down(MouseButton::Left),
        );
        click(&mut d, rect.x + 1, rect.y + 1, MouseEventKind::ScrollDown);
        draw(&mut d, 140, 24);
        assert!(d.operator.chats[&2].unread.is_some());
    }
    #[test]
    fn operator_sidebar_geometry_preserves_quota_and_tiny_rectangles() {
        let (mut d, _) = setup();
        for id in 3..=30 {
            d.operator_summary(summary(id, 1, OperatorTurnState::Running, false));
        }
        for width in [0, 1, 6, 60, 80, 120] {
            for height in 0..=25 {
                let area = Rect::new(2, 3, width, height);
                let rects = d.sidebar_rects(area);
                assert!(rects.operator.bottom() <= rects.tree.y);
                assert!(rects.tree.bottom() <= rects.quota.y);
                assert!(rects.quota.bottom() <= area.bottom());
                if height >= 6 {
                    assert!(rects.tree.height >= 3);
                }
                assert!(rects.operator.height <= 6);
                d.install_area(area);
            }
        }
        d.operator.collapsed = true;
        assert!(!d.operator_sidebar_busy());
        d.operator.collapsed = false;
        d.sidebar_hidden = true;
        assert!(!d.operator_sidebar_busy());
    }
    #[test]
    fn concurrent_histories_follow_ids_through_a_terminal_b_a_navigation() {
        let (mut d, reads) = setup();
        d.event_action(Event::Paste("draft A\n界e\u{301}".into()));
        d.operator_scroll(-3);
        let context_a = OperatorContext {
            project: Some("a".into()),
            ..Default::default()
        };
        d.operator.chats.get_mut(&1).unwrap().context = context_a.clone();
        d.operator.visible = false; // Navigate away; protocol dispatch remains live.
        history(
            &mut d,
            request_id(&reads, 2),
            summary(2, 1, OperatorTurnState::Complete, false),
            vec![delta(1, "B token"), completed(2)],
            None,
        );
        history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 1, OperatorTurnState::Running, false),
            vec![delta(1, "A token")],
            None,
        );
        d.open_operator();
        d.operator_select(Id(2));
        d.event_action(Event::Paste("draft B".into()));
        let rendered = draw(&mut d, 140, 24);
        assert!(rendered.contains("B token") && !rendered.contains("A token"));
        assert!(d.operator.chats[&2].unread.is_none());
        d.operator_select(Id(1));
        assert_eq!(d.operator.chats[&1].draft, "draft A\n界e\u{301}");
        assert!(!d.operator.chats[&1].follow);
        assert_eq!(d.operator.chats[&1].scroll, 3);
        assert_eq!(d.operator.chats[&1].context, context_a);
        assert_eq!(d.operator.chats[&2].draft, "draft B");
        assert_eq!(d.operator.chats[&1].messages, vec![delta(1, "A token")]);
    }
    #[test]
    fn paste_unicode_and_shift_enter_only_edit_native_draft_even_while_busy() {
        let (mut d, _) = setup();
        assert!(!matches!(
            d.event_action(Event::Paste("one\r\n界e\u{301}\u{1b}[31m".into())),
            DashboardAction::PtyBytes(_) | DashboardAction::Request(_)
        ));
        d.key_action(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        d.key(KeyCode::Char('z'));
        d.key(KeyCode::Backspace);
        assert_eq!(d.operator.chats[&1].draft, "one\n界e\u{301}[31m\n");
        d.key(KeyCode::Enter);
        assert!(!d.drain_outbox().iter().any(|message| matches!(
            message.request,
            Request::Operator(OperatorRequest::Submit { .. })
        )));
        d.ctrl('g');
        assert_eq!(d.operator.chats[&1].draft, "one\n界e\u{301}[31m\n");
        assert!(!d.operator_is_editing());
    }
    #[test]
    fn complete_turn_does_not_enable_submit_until_certified_stop_and_rejection_retains_draft() {
        let (mut d, _) = setup();
        d.operator_summary(summary(1, 1, OperatorTurnState::Complete, false));
        d.operator_insert("next");
        d.key(KeyCode::Enter);
        assert!(!d.operator.chats[&1].can_submit());
        assert!(d.drain_outbox().is_empty());
        assert!(status(&d.operator.chats[&1]).contains("finishing"));
        d.operator_summary(summary(1, 1, OperatorTurnState::Complete, true));
        d.key(KeyCode::Enter);
        let requests = d.drain_outbox();
        let request = requests
            .iter()
            .find(|message| {
                matches!(
                    message.request,
                    Request::Operator(OperatorRequest::Submit { .. })
                )
            })
            .unwrap();
        d.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Error {
                code: ovrcr_protocol::ErrorCode::Conflict,
                message: "busy".into(),
            },
        });
        assert_eq!(d.operator.chats[&1].draft, "next");
        assert!(!d.operator.chats[&1].submitting);
    }
    #[test]
    fn accepted_submission_clears_only_its_captured_text_and_stop_keeps_exact_identity() {
        let (mut d, _) = setup();
        d.operator_summary(summary(1, 1, OperatorTurnState::Complete, true));
        d.operator_insert("send");
        d.key(KeyCode::Enter);
        let requests = d.drain_outbox();
        let submit = requests
            .iter()
            .find(|message| {
                matches!(
                    message.request,
                    Request::Operator(OperatorRequest::Submit { .. })
                )
            })
            .unwrap();
        d.operator_insert(" plus draft");
        d.handle_server_message(ServerMessage::Response {
            request_id: submit.request_id,
            response: Response::Operator(OperatorResponse::Submitted(
                summary(1, 2, OperatorTurnState::Running, false)
                    .current_turn
                    .unwrap(),
            )),
        });
        assert_eq!(d.operator.chats[&1].draft, "send plus draft");
        d.ctrl('x');
        let stop = d
            .drain_outbox()
            .into_iter()
            .find(|message| {
                matches!(
                    message.request,
                    Request::Operator(OperatorRequest::Stop { .. })
                )
            })
            .unwrap();
        d.operator_select(Id(2));
        assert_eq!(
            stop.request,
            Request::Operator(OperatorRequest::Stop {
                identity: summary(1, 2, OperatorTurnState::Running, false)
                    .current_turn
                    .unwrap()
                    .identity
            })
        );
        assert!(!d.operator.chats[&2].stopping);
    }
    #[test]
    fn dirty_during_pagination_and_inflight_read_is_caught_without_duplicates() {
        let (mut d, reads) = setup();
        d.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(Id(1))));
        let more = history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 1, OperatorTurnState::Running, false),
            vec![delta(1, "one")],
            Some(1),
        );
        let next = request_id(&more, 1);
        d.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(Id(1))));
        let more = history(
            &mut d,
            next,
            summary(1, 1, OperatorTurnState::Complete, true),
            vec![delta(2, "two"), completed(3)],
            None,
        );
        let final_read = request_id(&more, 1);
        history(
            &mut d,
            final_read,
            summary(1, 1, OperatorTurnState::Complete, true),
            vec![],
            None,
        );
        history(
            &mut d,
            next,
            summary(1, 1, OperatorTurnState::Running, false),
            vec![delta(2, "duplicate")],
            None,
        );
        assert_eq!(d.operator.chats[&1].messages.len(), 3);
        assert!(d.operator.chats[&1].summary.worker_stopped);
        assert!(!d.operator.chats[&1].reading);
    }
    #[test]
    fn completion_unread_is_reviewed_only_after_successful_unobscured_tail_draw() {
        let (mut d, reads) = setup();
        history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 1, OperatorTurnState::Complete, true),
            vec![delta(1, "answer"), completed(2)],
            None,
        );
        assert!(d.operator.chats[&1].unread.is_some());
        d.operator_scroll(-1);
        draw(&mut d, 140, 24);
        assert!(d.operator.chats[&1].unread.is_some());
        d.operator_scroll(1);
        d.open_palette();
        draw(&mut d, 140, 24);
        assert!(d.operator.chats[&1].unread.is_some());
        d.palette = None;
        draw(&mut d, 140, 24);
        assert!(d.operator.chats[&1].unread.is_none());
        assert!(d.operator.chats[&1].reviewed.is_some());
    }
    #[test]
    fn foreign_history_and_old_summary_cannot_retarget_current_run() {
        let (mut d, reads) = setup();
        history(
            &mut d,
            request_id(&reads, 1),
            summary(2, 1, OperatorTurnState::Running, false),
            vec![delta(1, "foreign")],
            None,
        );
        assert!(d.operator.chats[&1].messages.is_empty());
        d.operator_summary(summary(1, 2, OperatorTurnState::Running, false));
        d.operator_summary(summary(1, 1, OperatorTurnState::Complete, true));
        assert_eq!(
            d.operator.chats[&1]
                .summary
                .current_turn
                .as_ref()
                .unwrap()
                .identity
                .run,
            SessionRunId(2)
        );
    }
    #[test]
    fn restored_summary_submits_current_retained_run_and_late_receipt_cannot_unarchive() {
        let (mut d, _) = setup();
        let mut restored = summary(1, 1, OperatorTurnState::Complete, true);
        restored.run = SessionRunId(3);
        d.operator_summary(restored.clone());
        d.operator_insert("next");
        d.operator_submit();
        let requests = d.drain_outbox();
        let submit = requests
            .iter()
            .find(|message| {
                matches!(
                    message.request,
                    Request::Operator(OperatorRequest::Submit { .. })
                )
            })
            .unwrap();
        assert!(matches!(
            submit.request,
            Request::Operator(OperatorRequest::Submit {
                expected_run: SessionRunId(3),
                ..
            })
        ));
        let mut archived = restored;
        archived.run = SessionRunId(5);
        archived.archived = true;
        archived.current_turn = Some(OperatorTurn {
            identity: OperatorTurnIdentity {
                run: SessionRunId(4),
                turn: OperatorTurnId(2),
                ..archived.current_turn.as_ref().unwrap().identity
            },
            state: OperatorTurnState::Complete,
            sequence: 2,
        });
        let accepted = OperatorTurn {
            state: OperatorTurnState::Running,
            sequence: 0,
            ..archived.current_turn.clone().unwrap()
        };
        d.operator_summary(archived.clone());
        d.operator_summary(summary(1, 1, OperatorTurnState::Complete, true));
        d.handle_server_message(ServerMessage::Response {
            request_id: submit.request_id,
            response: Response::Operator(OperatorResponse::Submitted(accepted)),
        });
        assert_eq!(d.operator.chats[&1].summary, archived);
        assert!(!d.operator.chats[&1].can_submit());
    }

    #[test]
    fn tool_result_including_empty_success_and_controls_is_native_text() {
        let (mut d, reads) = setup();
        let output = message(
            2,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                id: "bash".into(),
                output: String::new(),
                truncated: false,
                status: OperatorToolStatus::Succeeded,
            }),
        );
        let call = message(
            1,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                id: "bash".into(),
                name: "Bash".into(),
                arguments: "{\"command\":\"pwd\",\"cwd\":\"/tmp\"}".into(),
            }),
        );
        history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 1, OperatorTurnState::Complete, true),
            vec![
                call,
                output,
                delta(3, "\u{1b}]52;c;evil\u{7}\u{9b}31m\u{202e}safe"),
            ],
            None,
        );
        // Original starts compact; expansion preserves the complete command/cwd.
        d.operator.chats.get_mut(&1).unwrap().expanded.insert(1);
        let lines = transcript_lines(&d.operator.chats[&1]);
        let text = lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("succeeded") && text.contains("(no output)") && text.contains("cwd"));
        assert!(
            !text.contains('\u{1b}')
                && !text.contains('\u{7}')
                && !text.contains('\u{9b}')
                && !text.contains('\u{202e}')
        );
        assert!(text.contains("\\u{1b}"));
    }
    #[test]
    fn defaults_are_editable_provider_specific_and_tiny_layouts_do_not_panic() {
        let (mut d, _) = setup();
        d.settings.operator.provider = OperatorProvider::Grok;
        d.settings.operator.grok_model = Some("configured-model".into());
        d.ctrl('n');
        assert_eq!(d.operator.new.as_ref().unwrap().model, "configured-model");
        d.key(KeyCode::Backspace);
        d.key(KeyCode::Char('x'));
        assert_eq!(d.operator.new.as_ref().unwrap().model, "configured-modex");
        for (cols, rows) in [(0, 0), (1, 1), (20, 3), (55, 15), (140, 24)] {
            draw(&mut d, cols, rows);
        }
        d.operator_select(Id(1));
        d.key(KeyCode::Tab);
        let list = draw(&mut d, 55, 15);
        assert!(list.contains("chat-1") && list.contains("chat-2"));
        d.key(KeyCode::Tab);
        let chat = draw(&mut d, 55, 15);
        assert!(
            chat.contains("60×16"),
            "undersized conversation keeps work and draft"
        );
    }
    #[test]
    fn native_keys_and_paste_obey_existing_overlay_priority() {
        let (mut d, _) = setup();
        d.ctrl('g');
        d.key(KeyCode::Char(':'));
        assert!(d.palette.is_some());
        d.event_action(Event::Paste("palette filter".into()));
        assert_eq!(d.operator.chats[&1].draft, "");
        d.key(KeyCode::Esc);
        d.key(KeyCode::Enter);
        assert!(d.operator_is_editing());
        for key in [
            KeyCode::Char('a'),
            KeyCode::Enter,
            KeyCode::Backspace,
            KeyCode::Tab,
        ] {
            assert!(!matches!(d.key(key), DashboardAction::PtyBytes(_)));
        }
    }
    #[test]
    fn narrow_new_form_can_toggle_to_history_and_restore_selected_chat() {
        let (mut d, _) = setup();
        d.ctrl('n');
        d.ctrl('l');
        let rendered = draw(&mut d, 55, 15);
        assert!(
            rendered.contains("chat-1"),
            "history must be reachable from narrow creation: {rendered}"
        );
        d.key(KeyCode::Down);
        d.key(KeyCode::Enter);
        assert_eq!(d.operator.selected, Some(Id(2)));
        assert!(d.operator.new.is_none());
        assert!(d.operator_is_editing());
    }
    #[test]
    fn completion_in_tiny_hidden_transcript_remains_unread() {
        let (mut d, reads) = setup();
        history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 1, OperatorTurnState::Complete, true),
            vec![completed(1)],
            None,
        );
        draw(&mut d, 55, 3);
        assert!(d.operator.chats[&1].unread.is_some());
        draw(&mut d, 140, 24);
        assert!(d.operator.chats[&1].unread.is_none());
    }
    #[test]
    fn large_multiline_transcript_reaches_actual_tail_and_scrolls_in_cells() {
        let (mut d, reads) = setup();
        let mut messages: Vec<_> = (1..=8)
            .map(|ordinal| delta(ordinal, &"line\n".repeat(9000)))
            .collect();
        messages.push(delta(9, "ACTUAL_TAIL"));
        history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 1, OperatorTurnState::Running, false),
            messages,
            None,
        );
        let rendered = draw(&mut d, 140, 24);
        assert!(rendered.contains("ACTUAL_TAIL"));
    }
    #[test]
    fn multiline_up_down_moves_cursor_without_losing_graphemes() {
        let (mut d, _) = setup();
        d.event_action(Event::Paste("abc\n界xy".into()));
        d.key(KeyCode::Up);
        d.key(KeyCode::Char('!'));
        assert_eq!(d.operator.chats[&1].draft, "abc!\n界xy");
        d.key(KeyCode::Down);
        d.key(KeyCode::Char('?'));
        assert_eq!(d.operator.chats[&1].draft, "abc!\n界xy?");
    }

    #[test]
    fn late_submit_receipt_cannot_regress_newer_completed_run_or_stop_proof() {
        let (mut d, _) = setup();
        d.operator_summary(summary(1, 1, OperatorTurnState::Complete, true));
        d.operator_insert("send");
        d.key(KeyCode::Enter);
        let request = d
            .drain_outbox()
            .into_iter()
            .find(|message| {
                matches!(
                    message.request,
                    Request::Operator(OperatorRequest::Submit { .. })
                )
            })
            .unwrap();
        let mut newer = summary(1, 2, OperatorTurnState::Complete, true);
        newer.current_turn.as_mut().unwrap().sequence = 5;
        d.operator_summary(newer);
        d.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Operator(OperatorResponse::Submitted(
                summary(1, 2, OperatorTurnState::Running, false)
                    .current_turn
                    .unwrap(),
            )),
        });
        assert!(d.operator.chats[&1].summary.worker_stopped);
        assert_eq!(
            d.operator.chats[&1]
                .summary
                .current_turn
                .as_ref()
                .unwrap()
                .state,
            OperatorTurnState::Complete
        );
    }

    #[test]
    fn clicking_transcript_title_does_not_stop_without_the_explicit_control() {
        let (mut d, _) = setup();
        let right = content_area(&d, d.outer_area);
        d.operator_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: right.x + 2,
                row: right.y,
                modifiers: KeyModifiers::NONE,
            },
            d.outer_area,
        );
        assert!(!d.drain_outbox().iter().any(|message| matches!(
            message.request,
            Request::Operator(OperatorRequest::Stop { .. })
        )));
    }

    #[test]
    fn wrong_session_submit_receipt_cannot_clear_draft_or_install_turn() {
        let (mut d, _) = setup();
        d.operator_summary(summary(1, 1, OperatorTurnState::Complete, true));
        d.operator_insert("keep");
        d.key(KeyCode::Enter);
        let request = d
            .drain_outbox()
            .into_iter()
            .find(|message| {
                matches!(
                    message.request,
                    Request::Operator(OperatorRequest::Submit { .. })
                )
            })
            .unwrap();
        let mut turn = summary(1, 2, OperatorTurnState::Running, false)
            .current_turn
            .unwrap();
        turn.identity.session = SessionId(999);
        d.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Operator(OperatorResponse::Submitted(turn)),
        });
        assert_eq!(d.operator.chats[&1].draft, "keep");
        assert_eq!(
            d.operator.chats[&1]
                .summary
                .current_turn
                .as_ref()
                .unwrap()
                .identity
                .run,
            SessionRunId(1)
        );
    }
    #[test]
    fn conversation_list_mouse_uses_the_same_scrolled_rows_as_rendering() {
        let (mut d, _) = setup();
        for id in 3..=30 {
            d.operator_summary(summary(id, 1, OperatorTurnState::Running, false));
        }
        d.operator_select(Id(30));
        let area = Rect::new(0, 0, 140, 24);
        d.install_area(area);
        d.operator_ensure_visible();
        let list = d.sidebar_rects(area).operator;
        let offset = d.operator.list_offset;
        d.operator_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: list.x + 2,
                row: list.y + 1,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert_eq!(d.operator.selected, Some(Id(offset as u64 + 1)));
        assert!(draw(&mut d, 140, 24).contains("OPERATOR"));
    }
    #[test]
    fn explicit_mouse_stop_targets_only_selected_turn() {
        let (mut d, _) = setup();
        let right = content_area(&d, d.outer_area);
        let button = stop_rect(right);
        d.operator_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: button.x + 1,
                row: button.y,
                modifiers: KeyModifiers::NONE,
            },
            d.outer_area,
        );
        assert!(d.drain_outbox().iter().any(|message| matches!(message.request, Request::Operator(OperatorRequest::Stop { identity }) if identity.conversation == Id(1))));
        assert!(!d.operator.chats[&2].stopping);
    }
    #[test]
    fn late_mutation_error_stays_with_its_conversation_after_navigation() {
        let (mut d, _) = setup();
        d.operator_summary(summary(1, 1, OperatorTurnState::Complete, true));
        d.operator_insert("send");
        d.key(KeyCode::Enter);
        let request = d
            .drain_outbox()
            .into_iter()
            .find(|message| {
                matches!(
                    message.request,
                    Request::Operator(OperatorRequest::Submit { .. })
                )
            })
            .unwrap();
        d.operator_select(Id(2));
        d.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Error {
                code: ovrcr_protocol::ErrorCode::Conflict,
                message: "A_REFUSAL".into(),
            },
        });
        assert!(!draw(&mut d, 140, 24).contains("A_REFUSAL"));
        d.operator_select(Id(1));
        assert!(draw(&mut d, 140, 24).contains("A_REFUSAL"));
    }
    #[test]
    fn native_footer_and_palette_bindings_describe_the_actual_composer_keys() {
        let (mut d, _) = setup();
        let footer = super::super::hints::footer(&d, 140);
        assert!(
            footer.starts_with("OPERATOR") && footer.contains("Enter") && footer.contains("Ctrl-g"),
            "{footer}"
        );
        assert!(!footer.contains("Space Menu"));
        let binding = d
            .key_binding_for(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL))
            .unwrap();
        d.run(binding.action);
        assert!(d.drain_outbox().iter().any(|message| matches!(
            message.request,
            Request::Operator(OperatorRequest::Stop { .. })
        )));
    }
    #[test]
    fn operator_review_round1_unread_waits_for_the_final_durable_page_to_be_drawn() {
        let (mut d, reads) = setup();
        let later = summary(1, 2, OperatorTurnState::Running, false);
        let more = history(
            &mut d,
            request_id(&reads, 1),
            later.clone(),
            vec![completed(1)],
            Some(1),
        );
        assert!(draw(&mut d, 140, 24).contains("Turn complete"));
        assert_eq!(
            d.operator.chats[&1].unread,
            Some(OperatorTurnId(1)),
            "an older completion is not a caught-up durable tail"
        );
        assert_eq!(d.operator.chats[&1].reviewed, None);
        let mut next_user = message(
            2,
            OperatorMessageContent::User {
                text: "next turn".into(),
                context: OperatorContext::default(),
            },
        );
        next_user.turn = OperatorTurnId(2);
        let mut next_delta = delta(3, "LATER_DURABLE_OUTPUT");
        next_delta.turn = OperatorTurnId(2);
        history(
            &mut d,
            request_id(&more, 1),
            later,
            vec![next_user, next_delta],
            None,
        );
        assert!(draw(&mut d, 140, 24).contains("LATER_DURABLE_OUTPUT"));
        assert_eq!(d.operator.chats[&1].unread, None);
        assert_eq!(d.operator.chats[&1].reviewed, Some(OperatorTurnId(1)));
    }
    #[test]
    fn operator_review_round1_unread_waits_for_dirty_read_confirmation_even_after_failure() {
        let (mut d, reads) = setup();
        let complete = summary(1, 1, OperatorTurnState::Complete, true);
        history(
            &mut d,
            request_id(&reads, 1),
            complete.clone(),
            vec![completed(1)],
            None,
        );
        let catchup =
            d.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(Id(1))));
        let request = request_id(&catchup, 1);
        d.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(Id(1))));
        assert!(d.operator.chats[&1].reading && d.operator.chats[&1].dirty);
        draw(&mut d, 140, 24);
        assert_eq!(
            d.operator.chats[&1].unread,
            Some(OperatorTurnId(1)),
            "a dirty-triggered read must confirm the durable tail before review"
        );
        d.handle_server_message(ServerMessage::Response {
            request_id: request,
            response: Response::Error {
                code: ovrcr_protocol::ErrorCode::Conflict,
                message: "read unavailable".into(),
            },
        });
        draw(&mut d, 140, 24);
        assert_eq!(
            d.operator.chats[&1].unread,
            Some(OperatorTurnId(1)),
            "a failed read is not tail confirmation"
        );
        let retry =
            d.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(Id(1))));
        history(&mut d, request_id(&retry, 1), complete, vec![], None);
        draw(&mut d, 140, 24);
        assert_eq!(d.operator.chats[&1].unread, None);
    }
    #[test]
    fn operator_review_round1_exact_width_multiline_draft_keeps_cursor_visible() {
        for cols in [60, 140] {
            let (mut d, _) = setup();
            d.install_area(Rect::new(0, 0, cols, 24));
            let right = content_area(&d, d.outer_area);
            let width = right.width.saturating_sub(2) as usize;
            let line = "界".repeat(width / 2) + &"x".repeat(width % 2);
            let draft = std::iter::repeat_n(line, 5).collect::<Vec<_>>().join("\n");
            d.event_action(Event::Paste(draft.clone()));
            let rendered = draw(&mut d, cols, 24);
            assert_eq!(d.operator.chats[&1].draft, draft);
            assert!(
                rendered.contains('▏'),
                "cursor must remain visible after five full composer rows at width {cols}"
            );
        }
    }
    #[test]
    fn final_review_success_summary_marks_unread_without_sdk_history_message() {
        let (mut d, reads) = setup();
        let next = history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 1, OperatorTurnState::Complete, true),
            vec![delta(1, "durable answer")],
            Some(1),
        );
        assert_eq!(d.operator.chats[&1].unread, Some(OperatorTurnId(1)));
        draw(&mut d, 140, 24);
        assert_eq!(d.operator.chats[&1].unread, Some(OperatorTurnId(1)));
        history(
            &mut d,
            request_id(&next, 1),
            summary(1, 1, OperatorTurnState::Complete, true),
            vec![],
            None,
        );
        draw(&mut d, 140, 24);
        assert_eq!(d.operator.chats[&1].unread, None);
        assert_eq!(d.operator.chats[&1].reviewed, Some(OperatorTurnId(1)));
    }
    #[test]
    fn final_review_older_interrupted_page_does_not_replace_successful_current_turn_unread() {
        let (mut d, reads) = setup();
        let mut old = message(1, OperatorMessageContent::Interrupted);
        old.turn = OperatorTurnId(1);
        history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 2, OperatorTurnState::Complete, true),
            vec![old],
            Some(1),
        );
        assert_eq!(
            d.operator.chats[&1].unread,
            Some(OperatorTurnId(2)),
            "older journal pages must preserve the exact successful current turn marker"
        );
    }
    #[test]
    fn final_review_scrolled_wrapped_transcript_stays_anchored_across_stream_switch_and_resize() {
        let (mut d, reads) = setup();
        let text = (0..45)
            .map(|n| format!("ROW_{n:02} {}\n", "界".repeat(50)))
            .collect::<String>();
        history(
            &mut d,
            request_id(&reads, 1),
            summary(1, 1, OperatorTurnState::Running, false),
            vec![delta(1, &text)],
            None,
        );
        draw(&mut d, 140, 24);
        d.operator_scroll(-16);
        let before = draw(&mut d, 140, 24);
        let visible_rows = |screen: &str| {
            (0..45)
                .filter(|n| screen.contains(&format!("ROW_{n:02}")))
                .collect::<Vec<_>>()
        };
        let rows = visible_rows(&before);
        assert!(!rows.is_empty());
        d.operator_select(Id(2));
        let requests =
            d.handle_server_message(ServerMessage::Event(ServerEvent::OperatorChanged(Id(1))));
        history(
            &mut d,
            request_id(&requests, 1),
            summary(1, 1, OperatorTurnState::Running, false),
            vec![delta(2, &"STREAMED wrapped continuation ".repeat(80))],
            None,
        );
        d.operator_select(Id(1));
        assert_eq!(
            visible_rows(&draw(&mut d, 140, 24)),
            rows,
            "streaming moved the historical viewport"
        );
        let resized = draw(&mut d, 110, 24);
        assert!(
            resized.contains(&format!("ROW_{:02}", rows[0])),
            "resize lost the top logical text anchor: {resized}"
        );
        assert_eq!(
            visible_rows(&draw(&mut d, 140, 24)),
            rows,
            "resize round trip lost the anchor"
        );
        d.operator_scroll(isize::MAX);
        assert!(d.operator.chats[&1].follow);
        assert!(draw(&mut d, 140, 24).contains("STREAMED wrapped continuation"));
    }
    #[test]
    fn final_review_new_conversation_names_the_selected_provider_login_command() {
        let (mut d, _) = setup();
        d.ctrl('n');
        assert!(draw(&mut d, 180, 32).contains("ovrcr operator login chatgpt"));
        d.operator.new.as_mut().unwrap().provider = OperatorProvider::Grok;
        assert!(draw(&mut d, 180, 32).contains("ovrcr operator login grok"));
    }
}

//! Native chat is durable protocol data, never a terminal parser or PTY input target.
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
use std::collections::{BTreeMap, HashMap};
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub(super) struct Hub {
    pub visible: bool,
    editing: bool,
    list_focus: bool,
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
    scroll: usize,
    anchor: Option<(usize, usize)>,
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
            scroll: 0,
            anchor: None,
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
    Create,
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
fn split(area: Rect, list_focus: bool) -> (Rect, Rect) {
    if area.width < 72 {
        return if list_focus {
            (area, Rect::default())
        } else {
            (Rect::default(), area)
        };
    }
    let width = (area.width / 3).clamp(20, 32);
    (
        Rect::new(area.x, area.y, width, area.height),
        Rect::new(area.x + width, area.y, area.width - width, area.height),
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
fn transcript_lines(chat: &Chat) -> Vec<Line<'static>> {
    let mut text = String::new();
    let mut assistant = false;
    for message in &chat.messages {
        match &message.content {
            OperatorMessageContent::User {
                text: user,
                context,
            } => {
                text.push_str(&format!(
                    "\nYou · {}\ncwd: {}\n{}\n",
                    context_label(context),
                    context.cwd.as_ref().map_or_else(
                        || "operator workdir".into(),
                        |cwd| display_text(&cwd.to_string_lossy())
                    ),
                    display_text(user)
                ));
                assistant = false;
            }
            OperatorMessageContent::Worker(event) => match event {
                OperatorWorkerEvent::AssistantDelta(delta) => {
                    if !assistant {
                        text.push_str("\nOperator\n");
                        assistant = true;
                    }
                    text.push_str(&display_text(delta));
                }
                OperatorWorkerEvent::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    text.push_str(&format!(
                        "\n\nTool {} [{}]\n{}\n",
                        display_text(name),
                        display_text(id),
                        display_text(arguments)
                    ));
                    assistant = false;
                }
                OperatorWorkerEvent::ToolOutput {
                    id,
                    output,
                    truncated,
                    status,
                } => {
                    let status = match status {
                        OperatorToolStatus::Succeeded => "succeeded".into(),
                        OperatorToolStatus::Failed { exit_code } => format!(
                            "failed (exit {})",
                            exit_code.map_or_else(|| "unknown".into(), |code| code.to_string())
                        ),
                        OperatorToolStatus::TimedOut => "timed out".into(),
                    };
                    text.push_str(&format!(
                        "\nOutput [{}] · {status}{}\n{}\n",
                        display_text(id),
                        if *truncated {
                            " · truncated at 64 KiB"
                        } else {
                            ""
                        },
                        if output.is_empty() {
                            "(no output)".into()
                        } else {
                            display_text(output)
                        }
                    ));
                    assistant = false;
                }
                OperatorWorkerEvent::Completed(_) => {
                    text.push_str("\n\nTurn complete\n");
                    assistant = false;
                }
                OperatorWorkerEvent::Failed(error) => {
                    text.push_str(&format!("\n\nError: {}\n", display_text(error)));
                    assistant = false;
                }
            },
            OperatorMessageContent::Interrupted => {
                text.push_str("\n\nInterrupted · accepted operations may finish; no rollback.\n");
                assistant = false;
            }
        }
    }
    text.lines()
        .map(|line| Line::raw(line.to_owned()))
        .collect()
}
fn wrapped_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    wrapped_rows(lines, width)
        .into_iter()
        .map(|(line, _)| line)
        .collect()
}
// Logical line/byte anchors survive appended output and width changes.
fn wrapped_rows(lines: Vec<Line<'static>>, width: u16) -> Vec<(Line<'static>, (usize, usize))> {
    let width = usize::from(width.max(1));
    let mut rows = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        let text = line.to_string();
        let span = Span::raw(text);
        let mut row = String::new();
        let mut used = 0;
        let mut byte = 0;
        let mut start = 0;
        for grapheme in span.styled_graphemes(Style::default()) {
            let cells = grapheme.symbol.width();
            if used + cells > width && !row.is_empty() {
                rows.push((Line::raw(std::mem::take(&mut row)), (index, start)));
                used = 0;
                start = byte;
            }
            row.push_str(grapheme.symbol);
            used += cells;
            byte += grapheme.symbol.len();
        }
        rows.push((Line::raw(row), (index, start)));
    }
    rows
}
fn transcript_offset(chat: &Chat, rows: &[(Line<'_>, (usize, usize))], height: usize) -> usize {
    if !chat.follow
        && let Some(anchor) = chat.anchor
    {
        rows.iter()
            .rposition(|(_, start)| *start <= anchor)
            .unwrap_or(0)
    } else {
        rows.len()
            .saturating_sub(height)
            .saturating_sub(chat.scroll)
    }
}
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

impl Dashboard {
    pub(super) fn operator_header_rect(&self, area: Rect) -> Rect {
        let sidebar = super::sidebar_width_for(area, self.sidebar_preference());
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
            let context = self.capture_operator_context();
            self.retarget(super::state::PaneChange::Focus);
            let has_context = context != OperatorContext::default();
            if has_context {
                self.operator.context = context.clone();
            }
            if has_context
                && let Some(chat) = self
                    .operator
                    .selected
                    .and_then(|id| self.operator.chats.get_mut(&id.0))
            {
                chat.context = context;
            }
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
    fn operator_new(&mut self) {
        let provider = self.settings.operator.provider;
        self.operator.new = Some(NewChat {
            provider,
            model: default_model(self, provider),
            cursor: TextCursor::default(),
            provider_focus: false,
        });
        self.operator.list_focus = false;
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
        if self.operator.chats.contains_key(&id.0) {
            self.operator.selected = Some(id);
            self.operator.new = None;
            self.operator.error = None;
            self.operator.list_focus = false;
            self.operator.editing = true;
            self.operator_catch_up();
        }
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
        let expected_run = chat
            .summary
            .current_turn
            .as_ref()
            .map_or(SessionRunId(0), |turn| turn.identity.run);
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
                    if self.operator.selected.is_none() {
                        self.operator.selected = self.operator.chats.keys().next().copied().map(Id);
                    }
                }
                _ => {}
            }
            return DashboardAction::Redraw;
        }
        if self.operator.list_focus && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.operator.list_focus = false;
            return DashboardAction::Redraw;
        }
        if self.operator.list_focus {
            let ids: Vec<_> = self.operator.chats.keys().copied().map(Id).collect();
            let current = ids
                .iter()
                .position(|id| Some(*id) == self.operator.selected)
                .unwrap_or(0);
            let next = match key.code {
                KeyCode::Up | KeyCode::Char('k') => current.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    (current + 1).min(ids.len().saturating_sub(1))
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
                        .any(|pending| matches!(pending, Pending::Create))
                    {
                        let provider = form.provider;
                        self.operator_request(
                            Pending::Create,
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
        if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
            self.operator.list_focus = !self.operator.list_focus;
            return DashboardAction::Redraw;
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
            if chat.draft.len().saturating_add(text.len()) <= 64 * 1024 {
                chat.cursor.insert(&mut chat.draft, text);
            }
        }
    }
    pub(super) fn operator_paste(&mut self, text: &str) -> DashboardAction {
        if self.operator.list_focus {
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
        let (_, right) = split(
            content_area(self, self.outer_area),
            self.operator.list_focus,
        );
        let height = right
            .height
            .saturating_sub(right.height.min(7))
            .saturating_sub(2) as usize;
        if let Some(chat) = self
            .operator
            .selected
            .and_then(|id| self.operator.chats.get_mut(&id.0))
        {
            let rows = wrapped_rows(transcript_lines(chat), right.width.saturating_sub(2));
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
        if !self.sidebar_hidden
            && self.operator_header_rect(area).contains(point)
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
        let (list, transcript) = split(content, self.operator.list_focus);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if !list.is_empty() && list.contains(point) => {
                let index = usize::from(mouse.row.saturating_sub(list.y + 1))
                    + self.operator_list_offset(list.height);
                if mouse.row == list.y {
                    self.operator_new();
                } else if let Some(id) = self.operator.chats.keys().nth(index).copied().map(Id) {
                    self.operator_select(id);
                }
            }
            MouseEventKind::Down(MouseButton::Left) if !transcript.is_empty() => {
                if stop_rect(transcript).contains(point) && self.operator.new.is_none() {
                    self.operator_stop();
                } else {
                    self.operator.editing = true;
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
            Pending::Create => {}
        }
        if let Response::Error { message, .. } = response {
            let id = match &pending {
                Pending::Read { id, .. } | Pending::Submit { id, .. } => Some(*id),
                Pending::Stop { identity } => Some(identity.conversation),
                Pending::List | Pending::Create => None,
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
            (Pending::Create, Response::Operator(OperatorResponse::Created(summary))) => {
                self.operator_summary(summary.clone());
                self.operator_select(summary.id);
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
                    if current.is_none_or(|current| {
                        current.identity.run.0 < turn.identity.run.0
                            || (current.identity == turn.identity
                                && current.sequence <= turn.sequence
                                && !chat.summary.worker_stopped)
                    }) {
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
        let id = summary.id;
        if summary.current_turn.as_ref().is_some_and(|turn| {
            turn.identity.validate().is_err()
                || turn.identity.conversation != id
                || turn.identity.session != summary.session
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
                .insert(id.0, Chat::new(summary, self.operator.context.clone()));
        }
        if self.operator.selected.is_none() && self.operator.new.is_none() {
            self.operator.selected = Some(id);
        }
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
            || self.operator.new.is_some()
            || self.operator.list_focus && content_area(self, self.outer_area).width < 72
        {
            return;
        }
        let content = content_area(self, self.outer_area);
        if content.height.saturating_sub(content.height.min(7)) < 3 || content.width < 6 {
            return;
        }
        if let Some(chat) = self
            .operator
            .selected
            .and_then(|id| self.operator.chats.get_mut(&id.0))
            && chat.follow
            && !chat.reading
            && !chat.dirty
            && chat.confirmed_tail == Some(chat.ordinal)
        {
            // A drawn page is reviewable only after the durable tail has been confirmed.
            chat.reviewed = chat.unread.take().or(chat.reviewed);
        }
    }
    fn operator_list_offset(&self, height: u16) -> usize {
        let index = self
            .operator
            .selected
            .and_then(|id| {
                self.operator
                    .chats
                    .keys()
                    .position(|candidate| *candidate == id.0)
            })
            .unwrap_or(0);
        index
            .saturating_add(1)
            .saturating_sub(height.saturating_sub(1) as usize)
    }
    pub(super) fn draw_operator_header(&self, frame: &mut Frame<'_>) {
        if self.sidebar_hidden {
            return;
        }
        let unread = self
            .operator
            .chats
            .values()
            .filter(|chat| chat.unread.is_some())
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
        let (list, right) = split(area, self.operator.list_focus);
        if !list.is_empty() {
            frame.render_widget(
                Paragraph::new("New Conversation [Ctrl-n]")
                    .style(Style::default().fg(super::render::MAUVE())),
                Rect::new(list.x, list.y, list.width, list.height.min(1)),
            );
            let offset = self.operator_list_offset(list.height);
            let mut lines = Vec::new();
            for (id, chat) in self.operator.chats.iter().skip(offset) {
                let selected =
                    self.operator.selected == Some(Id(*id)) && self.operator.new.is_none();
                lines.push(Line::styled(
                    format!(
                        "{}{} {}",
                        if selected { "›" } else { " " },
                        if chat.error.is_some() {
                            "!"
                        } else if chat.unread.is_some() {
                            "●"
                        } else {
                            " "
                        },
                        display_text(&chat.summary.title)
                    ),
                    Style::default().add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ));
            }
            frame.render_widget(
                Paragraph::new(lines),
                Rect::new(
                    list.x,
                    list.y.saturating_add(1),
                    list.width,
                    list.height.saturating_sub(1),
                ),
            );
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
        let composer_height = right.height.min(7);
        let transcript = Rect::new(
            right.x,
            right.y,
            right.width,
            right.height.saturating_sub(composer_height),
        );
        let composer = Rect::new(right.x, transcript.bottom(), right.width, composer_height);
        let title = format!(
            "{} · {} · {} / {}",
            status(chat),
            display_text(&chat.summary.title),
            chat.summary.provider.as_str(),
            display_text(&chat.summary.model)
        );
        let rows = wrapped_rows(transcript_lines(chat), transcript.width.saturating_sub(2));
        let height = transcript.height.saturating_sub(2) as usize;
        let offset = transcript_offset(chat, &rows, height);
        let visible = rows
            .into_iter()
            .skip(offset)
            .take(height)
            .map(|(line, _)| line)
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(visible).block(Block::default().borders(Borders::ALL).title(title)),
            transcript,
        );
        frame.render_widget(
            Paragraph::new("[Stop Ctrl-x]").style(
                Style::default()
                    .fg(super::render::MAUVE())
                    .bg(super::render::BASE()),
            ),
            stop_rect(transcript),
        );
        let cursor = chat.cursor.position(&chat.draft);
        let cursor_visible = self.operator.editing && !self.operator.list_focus;
        let mut draft = chat.draft.clone();
        if cursor_visible {
            draft.insert(cursor, '▏');
        }
        let draft = display_text(&draft);
        let title = format!(
            "{} · Enter send · Shift-Enter newline · Tab/Ctrl-l chats · Ctrl-n new · Ctrl-g Browse",
            context_label(&chat.context)
        );
        let footer = chat
            .error
            .as_deref()
            .or(self.operator.error.as_deref())
            .unwrap_or(if chat.can_submit() {
                ""
            } else {
                "Busy / stop unconfirmed · draft retained"
            });
        let text = format!("{draft}\n{footer}");
        let lines = wrapped_lines(
            text.lines()
                .map(|line| Line::raw(line.to_owned()))
                .collect(),
            composer.width.saturating_sub(2),
        );
        let mut through_cursor = chat.draft[..cursor].to_owned();
        if cursor_visible {
            through_cursor.push('▏');
        }
        let through_cursor = display_text(&through_cursor);
        let cursor_row = wrapped_lines(
            through_cursor
                .split('\n')
                .map(|line| Line::raw(line.to_owned()))
                .collect(),
            composer.width.saturating_sub(2),
        )
        .len()
        .saturating_sub(1);
        let height = composer.height.saturating_sub(2) as usize;
        let offset = cursor_row.saturating_add(1).saturating_sub(height);
        let visible = lines
            .into_iter()
            .skip(offset)
            .take(height)
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(visible).block(Block::default().borders(Borders::ALL).title(title)),
            composer,
        );
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
        assert!(chat.contains("running"));
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
        let (_, right) = split(content_area(&d, d.outer_area), false);
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
        let area = Rect::new(0, 0, 140, 12);
        d.install_area(area);
        let (list, _) = split(content_area(&d, area), false);
        let offset = d.operator_list_offset(list.height);
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
        assert!(draw(&mut d, 140, 12).contains("New Conversation"));
    }
    #[test]
    fn explicit_mouse_stop_targets_only_selected_turn() {
        let (mut d, _) = setup();
        let (_, right) = split(content_area(&d, d.outer_area), false);
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
        for cols in [55, 140] {
            let (mut d, _) = setup();
            d.install_area(Rect::new(0, 0, cols, 24));
            let (_, right) = split(content_area(&d, d.outer_area), false);
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

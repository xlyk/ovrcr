//! The shared sidebar is the sole conversation list and hit-test geometry.
use super::*;
use crate::dashboard::render::{
    BASE, BLUE, GREEN, MAUVE, SURFACE0, SURFACE2, TEAL, TEXT, clip_text, compose_row,
};
use ovrcr_protocol::AgentActivity;

fn activity(chat: &Chat) -> AgentActivity {
    if chat.closing || chat.stopping {
        return AgentActivity::Busy;
    }
    match chat.summary.current_turn.as_ref().map(|turn| turn.state) {
        Some(OperatorTurnState::Running) => AgentActivity::Busy,
        Some(_) if !chat.summary.worker_stopped => AgentActivity::Busy,
        Some(OperatorTurnState::Error) => AgentActivity::Error,
        Some(OperatorTurnState::Interrupted) => AgentActivity::WaitingInput,
        Some(OperatorTurnState::Complete) => AgentActivity::ResponseReady,
        None => AgentActivity::Idle,
    }
}
impl Dashboard {
    pub(in crate::dashboard) fn draw_operator_quick_message(&self, frame: &mut Frame<'_>) {
        let Some(quick) = self.operator.quick.as_ref().filter(|quick| quick.visible) else {
            return;
        };
        let outer = frame.area();
        let width = outer.width.min(80);
        let error_rows = u16::from(quick.error.is_some());
        let draft_rows = wrapped_lines(
            display_text(&format!("{} ", quick.text))
                .split('\n')
                .map(|line| Line::raw(line.to_owned()))
                .collect(),
            width.saturating_sub(2),
        )
        .len();
        let height = (draft_rows.clamp(2, 11) as u16 + 5 + error_rows)
            .min(16)
            .min(outer.height);
        let area = Rect::new(
            outer.x + (outer.width - width) / 2,
            outer.y + (outer.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Quick message · Ctrl-o ")
            .style(Style::default().fg(TEXT()).bg(BASE()))
            .border_style(Style::default().fg(MAUVE()));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if !inner.is_empty() {
            frame.set_cursor_position((inner.x, inner.y));
        }
        if outer.width < 60 || outer.height < 16 {
            frame.render_widget(
                Paragraph::new("Resize to 60×16 · Esc dismisses; work retained"),
                inner,
            );
            return;
        }
        let caption = format!(
            "{} / {} · {}",
            quick.provider.as_str(),
            display_text(&quick.model),
            context_label(&quick.context)
        );
        frame.render_widget(
            Paragraph::new(clip_text(&caption, usize::from(inner.width)))
                .style(Style::default().fg(super::super::render::SUBTEXT())),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        let editor = Rect::new(
            inner.x,
            inner.y + 2,
            inner.width,
            inner.height.saturating_sub(3 + error_rows),
        );
        let position = quick.cursor.position(&quick.text);
        let prefix = display_text(&quick.text[..position]);
        let mut text = display_text(&quick.text);
        let editing = quick.phase == QuickPhase::Editing;
        if editing {
            text.insert(prefix.len(), '▏');
        }
        let lines = wrapped_lines(
            text.split('\n')
                .map(|line| Line::raw(line.to_owned()))
                .collect(),
            editor.width,
        );
        let through_cursor = wrapped_lines(
            format!("{prefix}▏")
                .split('\n')
                .map(|line| Line::raw(line.to_owned()))
                .collect(),
            editor.width,
        );
        let row = through_cursor.len().saturating_sub(1);
        let column = through_cursor
            .last()
            .map_or(0, |line| line.width().saturating_sub(1));
        let offset = row
            .saturating_add(1)
            .saturating_sub(usize::from(editor.height));
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(offset)
                    .take(usize::from(editor.height))
                    .collect::<Vec<_>>(),
            ),
            editor,
        );
        if editing && !editor.is_empty() {
            frame.set_cursor_position((
                editor.x + (column as u16).min(editor.width.saturating_sub(1)),
                editor.y + (row - offset) as u16,
            ));
        }
        if let Some(error) = &quick.error {
            frame.render_widget(
                Paragraph::new(clip_text(error, usize::from(inner.width)))
                    .style(Style::default().fg(super::super::render::RED())),
                Rect::new(inner.x, inner.bottom() - 2, inner.width, 1),
            );
        }
        let hint = match quick.phase {
            QuickPhase::Editing => "Enter Send · Shift-Enter Newline · Esc Dismiss",
            QuickPhase::Creating => "Creating conversation… · Esc Dismiss",
            QuickPhase::Submitting => "Sending first message… · Esc Dismiss",
        };
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(MAUVE())),
            Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
        );
    }
    pub(in crate::dashboard) fn operator_ids(&self) -> Vec<Id> {
        self.operator
            .chats
            .iter()
            .filter_map(|(id, chat)| (!chat.summary.archived).then_some(Id(*id)))
            .collect()
    }
    pub(in crate::dashboard) fn open_operator_conversation(&mut self, id: Id) -> DashboardAction {
        if !self.operator_ids().contains(&id) {
            return DashboardAction::None;
        }
        if !self.operator.visible {
            self.retarget(crate::dashboard::state::PaneChange::Focus);
        }
        self.operator.visible = true;
        self.mode = InputMode::Browse;
        self.operator_select(id);
        DashboardAction::Redraw
    }
    pub(super) fn operator_reconcile_tree(&mut self, previous: Rect) {
        if previous != self.sidebar_rects(self.outer_area).tree {
            self.ensure_selection_visible(&self.visible_rows());
        }
    }
    pub(super) fn operator_set_collapsed(&mut self, collapsed: bool) {
        let previous = self.sidebar_rects(self.outer_area).tree;
        self.operator.collapsed = collapsed;
        self.operator_reconcile_tree(previous);
    }
    pub(in crate::dashboard) fn operator_ensure_visible(&mut self) {
        let height = usize::from(
            self.sidebar_rects(self.outer_area)
                .operator
                .height
                .saturating_sub(1),
        );
        let ids = self.operator_ids();
        let max = ids.len().saturating_sub(height);
        self.operator.list_offset = self.operator.list_offset.min(max);
        if let Some(index) = ids
            .iter()
            .position(|id| Some(*id) == self.operator.selected)
        {
            if index < self.operator.list_offset {
                self.operator.list_offset = index;
            }
            if height > 0 && index >= self.operator.list_offset + height {
                self.operator.list_offset = index + 1 - height;
            }
        }
    }
    pub(in crate::dashboard) fn operator_sidebar_busy(&self) -> bool {
        if self.sidebar_preference() == Some(0) || self.operator.collapsed {
            return false;
        }
        let height = self
            .sidebar_rects(self.outer_area)
            .operator
            .height
            .saturating_sub(1) as usize;
        self.operator_ids()
            .into_iter()
            .skip(self.operator.list_offset)
            .take(height)
            .any(|id| activity(&self.operator.chats[&id.0]) == AgentActivity::Busy)
    }
    fn operator_row_close(&self, id: Id, width: u16) -> bool {
        width >= 3
            && (self.operator.hovered == Some(id)
                || self.operator.visible
                    && self.operator.new.is_none()
                    && self.operator.list_focus
                    && self.operator.selected == Some(id))
    }
    pub(in crate::dashboard) fn draw_operator_sidebar(&self, frame: &mut Frame<'_>, now: u64) {
        if self.sidebar_preference() == Some(0) {
            return;
        }
        let area = self.sidebar_rects(frame.area()).operator;
        if area.is_empty() {
            return;
        }
        let width = usize::from(area.width);
        let suffix = if self.operator.collapsed {
            format!(" ▸ {} [+]", self.operator_ids().len())
        } else {
            " [+]".into()
        };
        let heading = compose_row(
            vec![Span::styled(
                clip_text(" OPERATOR ", width.saturating_sub(suffix.len())),
                Style::default().fg(BLUE()).add_modifier(Modifier::BOLD),
            )],
            Span::styled("─", Style::default().fg(SURFACE2())),
            vec![Span::styled(suffix, Style::default().fg(MAUVE()))],
            width,
            false,
        );
        frame.render_widget(
            Paragraph::new(heading),
            Rect::new(area.x, area.y, area.width, 1),
        );
        for (row, id) in self
            .operator_ids()
            .into_iter()
            .skip(self.operator.list_offset)
            .take(area.height.saturating_sub(1) as usize)
            .enumerate()
        {
            let chat = &self.operator.chats[&id.0];
            let selected = self.operator.visible
                && self.operator.new.is_none()
                && self.operator.selected == Some(id);
            let close = self.operator_row_close(id, area.width);
            let (glyph, color) = crate::dashboard::status::sidebar_activity_glyph(
                activity(chat),
                chat.unread.is_some(),
                now,
            );
            let title = clip_text(
                &display_text(&chat.summary.title),
                width.saturating_sub(4 + if close { 3 } else { 0 }),
            );
            let line = compose_row(
                vec![
                    Span::raw(" "),
                    Span::styled(format!("{glyph} "), Style::default().fg(color)),
                    Span::raw(title),
                ],
                Span::raw(" "),
                if close {
                    vec![Span::styled("[x]", Style::default().fg(MAUVE()))]
                } else {
                    vec![]
                },
                width,
                selected,
            );
            frame.render_widget(
                Paragraph::new(line).style(Style::default().fg(TEXT()).bg(if selected {
                    SURFACE0()
                } else {
                    BASE()
                })),
                Rect::new(area.x, area.y + 1 + row as u16, area.width, 1),
            );
        }
    }
    pub(super) fn operator_sidebar_mouse(
        &mut self,
        mouse: MouseEvent,
        area: Rect,
    ) -> Option<DashboardAction> {
        if self.sidebar_preference() == Some(0) {
            return None;
        }
        let rect = self.sidebar_rects(area).operator;
        let point = ratatui::layout::Position::new(mouse.column, mouse.row);
        let inside = rect.contains(point);
        let ids = self.operator_ids();
        let id = inside
            .then(|| usize::from(mouse.row.saturating_sub(rect.y + 1)) + self.operator.list_offset)
            .filter(|_| mouse.row > rect.y)
            .and_then(|index| ids.get(index).copied());
        if mouse.kind == MouseEventKind::Moved {
            let changed = self.operator.hovered != id;
            self.operator.hovered = id;
            if changed || inside {
                return Some(if changed {
                    DashboardAction::Redraw
                } else {
                    DashboardAction::None
                });
            }
            return None;
        }
        if !inside {
            return None;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && mouse.row == rect.y
            && rect.width >= 3
            && mouse.column >= rect.right() - 3
        {
            return Some(self.open_operator_quick_message());
        }
        let close_visible = id.is_some_and(|id| self.operator_row_close(id, rect.width));
        if self.operator.visible {
            self.operator.list_focus = true;
        }
        self.mouse.hovered = None;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if mouse.row == rect.y => {
                self.operator_set_collapsed(!self.operator.collapsed);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(id) = id {
                    if close_visible && mouse.column >= rect.right().saturating_sub(3) {
                        return Some(self.open_close_operator_for(id));
                    }
                    self.open_operator_conversation(id);
                    self.operator.list_focus = area.width >= 100;
                    self.operator.sidebar_open = self.operator.list_focus;
                }
            }
            MouseEventKind::ScrollUp if rect.height > 1 => {
                self.operator.list_offset = self.operator.list_offset.saturating_sub(1)
            }
            MouseEventKind::ScrollDown if rect.height > 1 => {
                self.operator.list_offset = (self.operator.list_offset + 1).min(
                    ids.len()
                        .saturating_sub(rect.height.saturating_sub(1) as usize),
                )
            }
            _ => {}
        }
        Some(DashboardAction::Redraw)
    }
}

/// One source for draw, input, cursor and review geometry.
#[derive(Clone, Copy)]
pub(super) struct OperatorLayout {
    pub header: Rect,
    pub transcript: Rect,
    pub context: Rect,
    pub composer: Rect,
    pub top_rule: Rect,
    pub bottom_rule: Rect,
}

type Anchor = (u64, usize, usize);
struct TranscriptLine {
    line: Line<'static>,
    ordinal: u64,
    part: usize,
    byte: usize,
    prose: bool,
    gutter: &'static str,
}
fn push_lines(
    lines: &mut Vec<TranscriptLine>,
    ordinal: u64,
    part: usize,
    text: String,
    style: Style,
    prose: bool,
) {
    let mut byte = 0;
    for text in text.split('\n') {
        lines.push(TranscriptLine {
            line: Line::styled(text.to_owned(), style),
            ordinal,
            part,
            byte,
            prose,
            gutter: "│ ",
        });
        byte += text.len() + 1;
    }
}
fn logical_lines(chat: &Chat) -> Vec<TranscriptLine> {
    let mut lines = Vec::new();
    let mut previous_context = None;
    let mut index = 0;
    while index < chat.messages.len() {
        let message = &chat.messages[index];
        let first_line = lines.len();
        let mut ordinal = message.ordinal;
        let expanded = chat.expanded.contains(&ordinal);
        let disclosure = if expanded { "▾" } else { "▸" };
        let quiet = Style::default().fg(super::super::render::SUBTEXT());
        let normal = Style::default().fg(TEXT());
        let error = Style::default().fg(super::super::render::RED());
        match &message.content {
            OperatorMessageContent::User { text, context } => {
                push_lines(
                    &mut lines,
                    ordinal,
                    0,
                    "You".into(),
                    Style::default().fg(MAUVE()).add_modifier(Modifier::BOLD),
                    true,
                );
                if previous_context != Some(context) {
                    push_lines(
                        &mut lines,
                        ordinal,
                        1,
                        format!("{disclosure} Context · {}", context_label(context)),
                        quiet,
                        true,
                    );
                    previous_context = Some(context);
                } else if expanded {
                    push_lines(
                        &mut lines,
                        ordinal,
                        1,
                        "▾ Captured context".into(),
                        quiet,
                        true,
                    );
                }
                if expanded {
                    let detail = format!(
                        "  project: {}\n  workspace: {}\n  workspace id: {}\n  cwd: {}\n  session: {} · run: {}",
                        context.project.as_deref().unwrap_or("global"),
                        context.workspace.as_deref().unwrap_or("none"),
                        context.workspace_id.as_deref().unwrap_or("none"),
                        context.cwd.as_ref().map_or_else(
                            || "operator workdir".into(),
                            |cwd| cwd.to_string_lossy().into_owned()
                        ),
                        context
                            .session
                            .map_or_else(|| "none".into(), |id| id.0.to_string()),
                        context
                            .run
                            .map_or_else(|| "none".into(), |id| id.0.to_string())
                    );
                    push_lines(&mut lines, ordinal, 2, display_text(&detail), quiet, false);
                }
                push_lines(&mut lines, ordinal, 3, display_text(text), normal, true);
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(_)) => {
                if index == 0
                    || matches!(
                        chat.messages[index - 1].content,
                        OperatorMessageContent::User { .. }
                    )
                {
                    push_lines(
                        &mut lines,
                        ordinal,
                        0,
                        "Operator".into(),
                        Style::default().fg(BLUE()),
                        true,
                    );
                }
                let mut text = String::new();
                while let Some(OperatorMessage {
                    content:
                        OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(delta)),
                    ..
                }) = chat.messages.get(index)
                {
                    text.push_str(delta);
                    index += 1;
                }
                push_lines(&mut lines, ordinal, 3, display_text(&text), normal, true);
                index -= 1;
                ordinal = chat.messages[index].ordinal;
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                id,
                name,
                arguments,
            }) => {
                push_lines(
                    &mut lines,
                    ordinal,
                    0,
                    format!("{disclosure} {}", display_text(name)),
                    Style::default().fg(TEAL()),
                    false,
                );
                if expanded {
                    push_lines(
                        &mut lines,
                        ordinal,
                        1,
                        format!(
                            "  {} [{}]\n  {}",
                            display_text(name),
                            display_text(id),
                            display_text(arguments).replace('\n', "\n  ")
                        ),
                        normal,
                        false,
                    );
                }
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                id,
                output,
                truncated,
                status,
            }) => {
                let (status, style) = match status {
                    OperatorToolStatus::Succeeded => {
                        ("succeeded".into(), Style::default().fg(GREEN()))
                    }
                    OperatorToolStatus::Failed { exit_code } => (
                        format!(
                            "failed (exit {})",
                            exit_code.map_or_else(|| "unknown".into(), |code| code.to_string())
                        ),
                        error,
                    ),
                    OperatorToolStatus::TimedOut => ("timed out".into(), error),
                };
                push_lines(
                    &mut lines,
                    ordinal,
                    0,
                    format!(
                        "{disclosure} {status}{}{} · Output",
                        if *truncated {
                            " · truncated at 64 KiB"
                        } else {
                            ""
                        },
                        if output.is_empty() {
                            " · (no output)"
                        } else {
                            ""
                        }
                    ),
                    style,
                    false,
                );
                if expanded {
                    push_lines(
                        &mut lines,
                        ordinal,
                        1,
                        format!(
                            "  Output [{}]\n  {}",
                            display_text(id),
                            display_text(output).replace('\n', "\n  ")
                        ),
                        normal,
                        false,
                    );
                }
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed(_)) => {
                push_lines(
                    &mut lines,
                    ordinal,
                    0,
                    "Turn complete".into(),
                    Style::default().fg(TEAL()),
                    true,
                );
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(message)) => {
                push_lines(
                    &mut lines,
                    ordinal,
                    0,
                    format!("Error: {}", display_text(message)),
                    error,
                    true,
                );
            }
            OperatorMessageContent::Interrupted => {
                push_lines(
                    &mut lines,
                    ordinal,
                    0,
                    "Interrupted · accepted operations may finish; no rollback.".into(),
                    error,
                    true,
                );
            }
        }
        if matches!(message.content, OperatorMessageContent::User { .. }) {
            for line in &mut lines[first_line..] {
                line.gutter = if line.part == 0 { "❯ " } else { "  " };
            }
        }
        if chat.messages.get(index + 1).is_none_or(|next| {
            matches!(message.content, OperatorMessageContent::User { .. })
                || matches!(next.content, OperatorMessageContent::User { .. })
        }) {
            push_lines(&mut lines, ordinal, 4, String::new(), normal, true);
        }
        index += 1;
    }
    lines
}
#[cfg(test)]
pub(super) fn transcript_lines(chat: &Chat) -> Vec<Line<'static>> {
    logical_lines(chat)
        .into_iter()
        .map(|line| line.line)
        .collect()
}
// Retain each grapheme's style and logical byte position through wrapping.
fn wrapped_rows(lines: Vec<Line<'static>>, width: u16) -> Vec<(Line<'static>, (usize, usize))> {
    let width = usize::from(width.max(1));
    let mut rows = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        let mut spans = Vec::new();
        let mut used = 0;
        let mut byte = 0;
        let mut start = 0;
        for span in &line.spans {
            for grapheme in span.styled_graphemes(line.style) {
                let cells = grapheme.symbol.width();
                if used + cells > width && !spans.is_empty() {
                    rows.push((Line::from(std::mem::take(&mut spans)), (index, start)));
                    used = 0;
                    start = byte;
                }
                spans.push(Span::styled(grapheme.symbol.to_owned(), grapheme.style));
                used += cells;
                byte += grapheme.symbol.len();
            }
        }
        rows.push((Line::from(spans), (index, start)));
    }
    rows
}
pub(super) fn wrapped_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    wrapped_rows(lines, width)
        .into_iter()
        .map(|(line, _)| line)
        .collect()
}
pub(super) fn transcript_rows(chat: &Chat, width: u16) -> Vec<(Line<'static>, Anchor)> {
    // Deltas share visual paragraphs, but their anchors keep the actual durable source.
    let mut sources: HashMap<u64, Vec<(usize, u64)>> = HashMap::new();
    let mut first = None;
    let mut byte = 0;
    for message in &chat.messages {
        if let OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text)) =
            &message.content
        {
            let start = *first.get_or_insert(message.ordinal);
            sources
                .entry(start)
                .or_default()
                .push((byte, message.ordinal));
            byte += display_text(text).len();
        } else {
            first = None;
            byte = 0;
        }
    }
    logical_lines(chat)
        .into_iter()
        .flat_map(|entry| {
            let width = width.saturating_sub(2).max(1);
            let limit = if entry.prose { width.min(88) } else { width };
            let line = if !entry.prose && entry.part == 0 {
                Line::styled(
                    clip_text(&entry.line.to_string(), usize::from(limit)),
                    entry.line.style,
                )
            } else {
                entry.line
            };
            let source = (entry.part == 3)
                .then(|| sources.get(&entry.ordinal))
                .flatten();
            wrapped_rows(vec![line], limit)
                .into_iter()
                .map(move |(line, (_, byte))| {
                    let byte = entry.byte + byte;
                    let (start, ordinal) = source
                        .and_then(|sources| sources.iter().rev().find(|(start, _)| *start <= byte))
                        .copied()
                        .unwrap_or((0, entry.ordinal));
                    // Decoration follows source-byte calculation, so scroll anchors stay durable.
                    let gutter = if entry.part == 4 { "  " } else { entry.gutter };
                    let color = if gutter == "❯ " {
                        MAUVE()
                    } else {
                        SURFACE2()
                    };
                    let mut spans = vec![Span::styled(gutter, Style::default().fg(color))];
                    spans.extend(line.spans);
                    (Line::from(spans), (ordinal, entry.part, byte - start))
                })
        })
        .collect()
}
pub(super) fn transcript_offset(chat: &Chat, rows: &[(Line<'_>, Anchor)], height: usize) -> usize {
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
fn context_caption(context: &OperatorContext, width: u16) -> String {
    let suffix = match (context.session, context.run) {
        (Some(session), Some(run)) => format!(" / #{} run {}", session.0, run.0),
        _ => String::new(),
    };
    let mut label = context.clone();
    label.session = None;
    label.run = None;
    format!(
        "{}{}",
        clip_text(
            &format!("Managing {}", context_label(&label)),
            usize::from(width).saturating_sub(suffix.width())
        ),
        suffix
    )
}
impl Dashboard {
    pub(super) fn operator_layout(&self, area: Rect) -> OperatorLayout {
        let body = content_area(self, area);
        let draft = self
            .operator
            .selected
            .and_then(|id| self.operator.chats.get(&id.0))
            .map_or("", |chat| chat.draft.as_str());
        // Reserve the cursor cell too, so an exactly full final row remains editable.
        let draft_rows = wrapped_lines(
            display_text(&format!("{draft} "))
                .split('\n')
                .map(|line| Line::raw(line.to_owned()))
                .collect(),
            body.width.saturating_sub(4),
        )
        .len();
        let height = (draft_rows.clamp(2, usize::from((body.height / 3).clamp(2, 6))) as u16)
            .min(body.height.saturating_sub(5));
        let header = Rect::new(body.x, body.y, body.width, body.height.min(1));
        let composer = Rect::new(
            body.x.saturating_add(3),
            body.bottom().saturating_sub(height + 1),
            body.width.saturating_sub(4),
            height,
        );
        let top_rule = Rect::new(
            body.x + 1,
            composer.y.saturating_sub(1),
            body.width.saturating_sub(2),
            1,
        );
        let bottom_rule = Rect::new(
            body.x + 1,
            composer.bottom(),
            body.width.saturating_sub(2),
            1,
        );
        let context = Rect::new(
            body.x + 1,
            top_rule.y.saturating_sub(1),
            body.width.saturating_sub(2),
            1.min(body.height),
        );
        let transcript = Rect::new(
            body.x,
            header.bottom(),
            body.width,
            context.y.saturating_sub(header.bottom()),
        );
        OperatorLayout {
            header,
            transcript,
            context,
            composer,
            top_rule,
            bottom_rule,
        }
    }
    pub(super) fn draw_operator_conversation(
        &self,
        frame: &mut Frame<'_>,
        chat: &Chat,
        now_unix_ms: u64,
    ) {
        let layout = self.operator_layout(frame.area());
        let stop = !chat.summary.worker_stopped
            || chat
                .summary
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.state == OperatorTurnState::Running);
        let title_width = layout
            .header
            .width
            .saturating_sub(if stop { 13 } else { 0 });
        let width = usize::from(title_width);
        let provider = chat.summary.provider.as_str();
        let state = status(chat);
        let sidebar = self.sidebar_rects(frame.area()).operator;
        let selected_visible = self.sidebar_preference() != Some(0)
            && !sidebar.is_empty()
            && self
                .operator_ids()
                .into_iter()
                .skip(self.operator.list_offset)
                .take(sidebar.height.saturating_sub(1) as usize)
                .any(|id| id == chat.summary.id);
        let identity = if selected_visible {
            "Operator".into()
        } else {
            display_text(&chat.summary.title)
        };
        let identity = clip_text(
            &identity,
            width.saturating_sub(provider.width() + state.width() + 9),
        );
        let model = clip_text(
            &display_text(&chat.summary.model),
            width.saturating_sub(identity.width() + provider.width() + state.width() + 9),
        );
        let title = format!("{identity} · {provider} / {model} · {state}");
        frame.render_widget(
            Paragraph::new(clip_text(&title, title_width as usize))
                .style(Style::default().fg(super::super::render::SUBTEXT())),
            layout.header,
        );
        if stop {
            frame.render_widget(
                Paragraph::new("[Stop Ctrl-x]").style(Style::default().fg(MAUVE())),
                stop_rect(layout.header),
            );
        }
        let rows = transcript_rows(chat, layout.transcript.width);
        let height = layout.transcript.height as usize;
        let offset = transcript_offset(chat, &rows, height);
        let lines: Vec<_> = rows
            .into_iter()
            .skip(offset)
            .take(height)
            .map(|(line, anchor)| {
                if self.operator.transcript_focus && chat.entry_focus == Some(anchor.0) {
                    line.style(Style::default().bg(SURFACE0()))
                } else {
                    line
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), layout.transcript);
        let error = chat.error.as_deref().or(self.operator.error.as_deref());
        let mut context = layout.context;
        if let Some(activity) = conversation_activity(chat) {
            let (glyph, color) = super::super::status::sidebar_activity_glyph(
                AgentActivity::Busy,
                false,
                now_unix_ms,
            );
            let label = if activity.animated {
                format!("{glyph} {}", activity.label)
            } else {
                activity.label
            };
            // Keep room for the target or retained error beside even a long tool name.
            let width = label
                .width()
                .min(usize::from(context.width.saturating_sub(12))) as u16;
            let area = Rect::new(
                context.right().saturating_sub(width),
                context.y,
                width,
                context.height,
            );
            context.width = context.width.saturating_sub(width + 1);
            frame.render_widget(
                Paragraph::new(clip_text(&label, width as usize)).style(Style::default().fg(
                    if activity.animated {
                        color
                    } else {
                        super::super::render::SUBTEXT()
                    },
                )),
                area,
            );
        }
        let caption = error.map_or_else(
            || context_caption(&chat.context, context.width),
            display_text,
        );
        frame.render_widget(
            Paragraph::new(caption).style(Style::default().fg(if error.is_some() {
                super::super::render::RED()
            } else {
                BLUE()
            })),
            context,
        );
        for (area, color) in [(layout.top_rule, MAUVE()), (layout.bottom_rule, SURFACE2())] {
            frame.render_widget(
                Paragraph::new("─".repeat(usize::from(area.width)))
                    .style(Style::default().fg(color)),
                area,
            );
        }
        let focused =
            self.operator.editing && !self.operator.list_focus && !self.operator.transcript_focus;
        frame.render_widget(
            Paragraph::new(if focused { ">" } else { "·" }).style(Style::default().fg(MAUVE())),
            Rect::new(layout.composer.x.saturating_sub(2), layout.composer.y, 1, 1),
        );
        let position = chat.cursor.position(&chat.draft);
        let prefix = display_text(&chat.draft[..position]);
        let mut draft = display_text(&chat.draft);
        // A styled visible caret complements the real terminal cursor for headless capture.
        if focused {
            draft.insert(prefix.len(), '▏');
        }
        let lines = wrapped_lines(
            draft
                .split('\n')
                .map(|line| Line::raw(line.to_owned()))
                .collect(),
            layout.composer.width,
        );
        let through_cursor = wrapped_lines(
            format!("{prefix}▏")
                .split('\n')
                .map(|line| Line::raw(line.to_owned()))
                .collect(),
            layout.composer.width,
        );
        let row = through_cursor.len().saturating_sub(1);
        let column = through_cursor
            .last()
            .map_or(0, |line| line.width().saturating_sub(1));
        let offset = row
            .saturating_add(1)
            .saturating_sub(layout.composer.height as usize);
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(offset)
                    .take(layout.composer.height as usize)
                    .collect::<Vec<_>>(),
            ),
            layout.composer,
        );
        if focused && !layout.composer.is_empty() {
            frame.set_cursor_position((
                layout.composer.x + column as u16,
                layout.composer.y + row.saturating_sub(offset) as u16,
            ));
        }
    }
}

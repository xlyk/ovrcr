//! The shared sidebar is the sole conversation list and hit-test geometry.
use super::*;
use crate::dashboard::render::{
    BASE, BLUE, GREEN, MAUVE, PEACH, SUBTEXT, SURFACE0, SURFACE2, TEAL, TEXT, clip_text,
    compose_row,
};
use ovrcr_protocol::AgentActivity;

fn activity(chat: &Chat) -> AgentActivity {
    if chat.stopping {
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
            "{} · {}",
            self.operator
                .chat
                .as_ref()
                .map_or_else(|| "Operator · pi".into(), identity),
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
            QuickPhase::Editing => "Enter Send  Shift-Enter Newline  Ctrl-c Clear  Esc Dismiss",
            QuickPhase::Opening => "Opening Operator… · Esc Dismiss",
            QuickPhase::Submitting => "Sending message… · Esc Dismiss",
        };
        frame.render_widget(
            Paragraph::new(hint).style(Style::default().fg(MAUVE())),
            Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
        );
    }
    pub(in crate::dashboard) fn operator_sidebar_busy(&self) -> bool {
        self.sidebar_preference() != Some(0)
            && self
                .operator
                .chat
                .as_ref()
                .is_some_and(|chat| activity(chat) == AgentActivity::Busy)
    }
    /// One fixed navigation row below the tree: the first project starts at the
    /// top, and the row's right-hand words say whether a conversation can be
    /// opened, holds a draft or unread reply, or needs inspection. It has no
    /// count, collapse arrow, plus button, child row or close target.
    pub(in crate::dashboard) fn draw_operator_sidebar(&self, frame: &mut Frame<'_>, now: u64) {
        if self.sidebar_preference() == Some(0) {
            return;
        }
        let area = self.sidebar_rects(frame.area()).operator;
        if area.is_empty() {
            return;
        }
        let width = usize::from(area.width);
        let selected = self.operator.visible;
        let (glyph, color, clause) = match &self.operator.chat {
            Some(chat) => {
                let (glyph, color) = crate::dashboard::status::sidebar_activity_glyph(
                    activity(chat),
                    chat.unread.is_some(),
                    now,
                );
                let turn = chat.summary.current_turn.as_ref().map(|turn| turn.state);
                // A failure or interruption stays identifiable even while the
                // worker's stop is still unconfirmed; the dock spells both out
                // instead of reusing the full view's longer status sentence.
                let outcome = match (turn, chat.summary.worker_stopped) {
                    (Some(OperatorTurnState::Error), true) => Some("Turn failed"),
                    (Some(OperatorTurnState::Error), false) => Some("Failed · stop unconfirmed"),
                    (Some(OperatorTurnState::Interrupted), true) => Some("Interrupted"),
                    (Some(OperatorTurnState::Interrupted), false) => {
                        Some("Interrupted · stop unconfirmed")
                    }
                    (Some(OperatorTurnState::Complete), false) => Some("Stop unconfirmed"),
                    _ => None,
                };
                let clause = if chat.unread.is_some() {
                    "1 unread".to_string()
                } else if chat.submitting
                    || chat.stopping
                    || turn == Some(OperatorTurnState::Running)
                {
                    conversation_activity(chat)
                        .map_or_else(|| status(chat).to_string(), |activity| activity.label)
                } else if let Some(outcome) = outcome {
                    outcome.to_string()
                } else if !chat.draft.is_empty() {
                    "Draft".to_string()
                } else if selected {
                    String::new()
                } else {
                    "Open chat".to_string()
                };
                (glyph, color, clause)
            }
            None => (
                ' ',
                super::super::render::MUTED(),
                if self.operator.error.is_some() {
                    "Unavailable".to_string()
                } else if self.operator.loading {
                    "Opening…".to_string()
                } else {
                    "Open chat".to_string()
                },
            ),
        };
        let (inset, end) = if width < 24 { (" ", "") } else { ("  ", " ") };
        let title = clip_text("Operator", width.saturating_sub(inset.width()));
        let room = width.saturating_sub(inset.width() + title.width() + 1 + end.width());
        // At the minimum sidebar width the state words outrank the activity glyph.
        let marker = if glyph != ' ' && clause.width() + 2 <= room {
            format!("{glyph} ")
        } else {
            String::new()
        };
        let clause = clip_text(&clause, room.saturating_sub(marker.width()));
        let line = compose_row(
            vec![Span::raw(inset), Span::raw(title)],
            Span::raw(" "),
            vec![
                Span::styled(marker, Style::default().fg(color)),
                Span::styled(clause, Style::default().fg(super::super::render::SUBTEXT())),
                Span::raw(end),
            ],
            width,
            selected,
        );
        frame.render_widget(
            Paragraph::new(line).style(Style::default().fg(TEXT()).bg(if selected {
                SURFACE0()
            } else {
                BASE()
            })),
            area,
        );
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
        if rect.is_empty() || mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return None;
        }
        let point = ratatui::layout::Position::new(mouse.column, mouse.row);
        if !rect.contains(point) {
            return None;
        }
        Some(self.open_operator())
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

pub(super) type Anchor = (u64, usize, usize);

#[derive(Clone, Copy)]
pub(super) struct TranscriptEntry {
    pub(super) ordinal: u64,
    pub(super) expanded: bool,
}

pub(super) struct TranscriptRow {
    pub(super) line: Line<'static>,
    pub(super) anchor: Option<Anchor>,
    pub(super) entry: Option<TranscriptEntry>,
}

const THINKING_PREVIEW_LINES: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ThinkingDisplay {
    Capped,
    Collapsed,
    Expanded,
}

#[derive(Clone, Copy)]
enum ThinkingLine {
    Header(OperatorTurnId),
    Body,
}

struct TranscriptLine {
    line: Line<'static>,
    ordinal: u64,
    part: usize,
    byte: usize,
    prose: bool,
    gutter: &'static str,
    entry: Option<TranscriptEntry>,
    thinking: Option<ThinkingLine>,
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
            entry: None,
            thinking: None,
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
        // The selected model is retained metadata and compaction events are
        // transient phase state; neither becomes a permanent journal row.
        if matches!(
            message.content,
            OperatorMessageContent::Worker(
                OperatorWorkerEvent::ModelSelected(_)
                    | OperatorWorkerEvent::ReasoningSelected(_)
                    | OperatorWorkerEvent::CompactionStarted
                    | OperatorWorkerEvent::CompactionEnded
            )
        ) {
            index += 1;
            continue;
        }
        let first_line = lines.len();
        let owner = message.ordinal;
        let mut ordinal = message.ordinal;
        let expanded = chat.expanded.get(&owner).copied().unwrap_or(false);
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
                    // The captured target is per-message; the agent working
                    // directory is fixed for the conversation's lifetime.
                    let detail = format!(
                        "  project: {}\n  workspace: {}\n  workspace id: {}\n  captured cwd: {}\n  agent workdir: {}\n  session: {} · run: {}",
                        context.project.as_deref().unwrap_or("global"),
                        context.workspace.as_deref().unwrap_or("none"),
                        context.workspace_id.as_deref().unwrap_or("none"),
                        context.cwd.as_ref().map_or_else(
                            || "none".into(),
                            |cwd| cwd.to_string_lossy().into_owned()
                        ),
                        chat.workdir.as_ref().map_or_else(
                            || "fixed operator workdir".into(),
                            |workdir| workdir.to_string_lossy().into_owned()
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
                            | OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(_))
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
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(_)) => {
                let turn = message.turn;
                let mut text = String::new();
                while let Some(OperatorMessage {
                    content:
                        OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(delta)),
                    turn: delta_turn,
                    ..
                }) = chat.messages.get(index)
                {
                    if *delta_turn != turn {
                        break;
                    }
                    text.push_str(delta);
                    index += 1;
                }
                // Only stream terminators are structural; interior blank lines stay.
                let trimmed = text.trim_end_matches('\n').len();
                text.truncate(trimmed);
                if !text.is_empty() {
                    let thinking_style =
                        Style::default().fg(PEACH()).add_modifier(Modifier::ITALIC);
                    push_lines(
                        &mut lines,
                        owner,
                        0,
                        "Thinking".into(),
                        quiet.add_modifier(Modifier::BOLD),
                        true,
                    );
                    lines[first_line].thinking = Some(ThinkingLine::Header(turn));
                    push_lines(
                        &mut lines,
                        owner,
                        3,
                        display_text(&text),
                        thinking_style,
                        true,
                    );
                    for line in &mut lines[first_line + 1..] {
                        line.thinking = Some(ThinkingLine::Body);
                    }
                }
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
            OperatorMessageContent::Worker(
                OperatorWorkerEvent::ModelSelected(_)
                | OperatorWorkerEvent::ReasoningSelected(_)
                | OperatorWorkerEvent::CompactionStarted
                | OperatorWorkerEvent::CompactionEnded,
            ) => {}
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed) => {
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
        if expandable(message) {
            let entry = TranscriptEntry {
                ordinal: owner,
                expanded: chat.expanded.get(&owner).copied().unwrap_or(false),
            };
            for line in &mut lines[first_line..] {
                line.entry = Some(entry);
            }
        }
        if lines.len() > first_line
            && chat.messages.get(index + 1).is_none_or(|next| {
                matches!(message.content, OperatorMessageContent::User { .. })
                    || matches!(next.content, OperatorMessageContent::User { .. })
            })
        {
            push_lines(&mut lines, ordinal, 4, String::new(), normal, true);
        }
        index += 1;
    }
    lines
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

/// A missing override follows the turn; an explicit choice always wins.
fn thinking_display(
    chat: &Chat,
    ordinal: u64,
    turn: OperatorTurnId,
    total: usize,
) -> ThinkingDisplay {
    match chat.expanded.get(&ordinal).copied() {
        Some(true) => ThinkingDisplay::Expanded,
        Some(false) => ThinkingDisplay::Collapsed,
        None => {
            let current_running = chat.summary.current_turn.as_ref().is_some_and(|current| {
                current.state == OperatorTurnState::Running && current.identity.turn == turn
            });
            if !current_running {
                ThinkingDisplay::Collapsed
            } else if total > THINKING_PREVIEW_LINES {
                ThinkingDisplay::Capped
            } else {
                ThinkingDisplay::Expanded
            }
        }
    }
}

fn thinking_header(display: ThinkingDisplay, total: usize) -> Line<'static> {
    let (disclosure, count) = match display {
        ThinkingDisplay::Expanded => ("▾ Thinking", format!(" · {total} lines")),
        ThinkingDisplay::Collapsed => ("▸ Thinking", format!(" · {total} lines")),
        ThinkingDisplay::Capped => (
            "▸ Thinking",
            format!(" · showing last {THINKING_PREVIEW_LINES} of {total} lines"),
        ),
    };
    Line::from(vec![
        Span::styled(
            disclosure,
            Style::default().fg(SUBTEXT()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(count, Style::default().fg(SUBTEXT())),
    ])
}

/// Clip a styled header to one row; headers never wrap.
fn clip_line(line: Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line;
    }
    if width == 0 {
        return Line::default();
    }
    let base = line.style;
    let mut budget = width - 1;
    let mut spans = Vec::new();
    let mut last_style = Style::default();
    for span in line.spans {
        let mut text = String::new();
        let mut used = 0;
        let mut stopped = false;
        for grapheme in span.styled_graphemes(base) {
            let cells = grapheme.symbol.width();
            if used + cells > budget {
                stopped = true;
                break;
            }
            text.push_str(grapheme.symbol);
            used += cells;
        }
        if !text.is_empty() {
            last_style = span.style;
            spans.push(Span::styled(text, span.style));
            budget -= used;
        }
        if stopped || budget == 0 {
            break;
        }
    }
    spans.push(Span::styled("…", last_style));
    Line::from(spans)
}

pub(super) fn transcript_rows(chat: &Chat, width: u16, now_unix_ms: u64) -> Vec<TranscriptRow> {
    // Deltas share visual paragraphs, but their anchors keep the actual durable
    // source. Answer and thinking runs are separate paragraphs; thinking blocks
    // never merge across turns.
    let mut sources: HashMap<u64, Vec<(usize, u64)>> = HashMap::new();
    let mut group: Option<(u64, bool, OperatorTurnId)> = None;
    let mut byte = 0;
    for message in &chat.messages {
        let (thinking, text) = match &message.content {
            OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text)) => {
                (false, text.as_str())
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(text)) => {
                (true, text.as_str())
            }
            _ => {
                group = None;
                byte = 0;
                continue;
            }
        };
        if group
            .is_none_or(|(_, kind, turn)| kind != thinking || (thinking && turn != message.turn))
        {
            group = Some((message.ordinal, thinking, message.turn));
            byte = 0;
        }
        let start = group.map_or(message.ordinal, |(start, _, _)| start);
        sources
            .entry(start)
            .or_default()
            .push((byte, message.ordinal));
        byte += display_text(text).len();
    }
    let limit = width.saturating_sub(2).max(1);
    let body_width = width.saturating_sub(4).max(1);
    let thinking_style = Style::default().fg(PEACH()).add_modifier(Modifier::ITALIC);
    let mut rows = Vec::new();
    let mut lines = logical_lines(chat).into_iter().peekable();
    while let Some(entry) = lines.next() {
        if let Some(ThinkingLine::Header(turn)) = entry.thinking {
            // Consume the block's body before wrapping, so its visible row count
            // follows the one real wrap rather than a second guess.
            let mut body_text = Vec::new();
            let mut body_bytes = Vec::new();
            while lines
                .peek()
                .is_some_and(|next| matches!(next.thinking, Some(ThinkingLine::Body)))
            {
                let body = lines.next().expect("peeked body row");
                body_text.push(body.line);
                body_bytes.push(body.byte);
            }
            let wrapped = wrapped_rows(body_text, body_width);
            let total = wrapped.len();
            let display = thinking_display(chat, entry.ordinal, turn, total);
            let disclosure = display == ThinkingDisplay::Expanded;
            let entry_choice = Some(TranscriptEntry {
                ordinal: entry.ordinal,
                expanded: disclosure,
            });
            let header = clip_line(thinking_header(display, total), usize::from(limit));
            let mut spans = vec![Span::styled("│ ", Style::default().fg(SURFACE2()))];
            spans.extend(header.spans);
            rows.push(TranscriptRow {
                line: Line::from(spans),
                anchor: Some((entry.ordinal, 0, 0)),
                entry: entry_choice,
            });
            let body_source = sources.get(&entry.ordinal);
            let skip = match display {
                ThinkingDisplay::Expanded => 0,
                ThinkingDisplay::Collapsed => total,
                ThinkingDisplay::Capped => total.saturating_sub(THINKING_PREVIEW_LINES),
            };
            for (line, (logical, start)) in wrapped.into_iter().skip(skip) {
                let byte = body_bytes.get(logical).copied().unwrap_or(0) + start;
                // The visible row keeps the delta that actually owns its bytes.
                let (start, ordinal) = body_source
                    .and_then(|sources| sources.iter().rev().find(|(start, _)| *start <= byte))
                    .copied()
                    .unwrap_or((0, entry.ordinal));
                let mut spans = vec![Span::styled("│ ", Style::default().fg(SURFACE2()))];
                spans.push(Span::styled("  ", thinking_style));
                spans.extend(line.spans);
                rows.push(TranscriptRow {
                    line: Line::from(spans),
                    anchor: Some((ordinal, 3, byte - start)),
                    entry: entry_choice,
                });
            }
            continue;
        }
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
        for (line, (_, byte)) in wrapped_rows(vec![line], limit) {
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
            rows.push(TranscriptRow {
                line: Line::from(spans),
                anchor: Some((ordinal, entry.part, byte - start)),
                entry: entry.entry,
            });
        }
    }
    if let Some(activity) = conversation_activity(chat) {
        if rows
            .last()
            .is_some_and(|row| row.anchor.is_some_and(|anchor| anchor.1 == 4))
        {
            rows.pop();
        }
        let (glyph, color) =
            super::super::status::sidebar_activity_glyph(AgentActivity::Busy, false, now_unix_ms);
        let label = if activity.animated {
            format!("{glyph} {}", activity.label)
        } else {
            activity.label
        };
        rows.push(TranscriptRow {
            line: Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    clip_text(&label, usize::from(width.saturating_sub(2))),
                    Style::default().fg(if activity.animated {
                        color
                    } else {
                        super::super::render::SUBTEXT()
                    }),
                ),
            ]),
            anchor: None,
            entry: None,
        });
    }
    rows
}
pub(super) fn transcript_offset(chat: &Chat, rows: &[TranscriptRow], height: usize) -> usize {
    if !chat.follow
        && let Some(anchor) = chat.anchor
    {
        rows.iter()
            .rposition(|row| row.anchor.is_some_and(|start| start <= anchor))
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
            .chat
            .as_ref()
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
        let title_width = self.operator.chat.as_ref().map_or(0, |chat| {
            format!("{} · {}", identity(chat), status(chat)).width()
        });
        let controls_width = self.operator_reasoning_label().width()
            + 13
            + if self.operator_stop_available() {
                13
            } else {
                0
            };
        let header = Rect::new(
            body.x,
            body.y,
            body.width,
            body.height.min(
                if body.width >= 50 && title_width + controls_width > usize::from(body.width) {
                    2
                } else {
                    1
                },
            ),
        );
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
        let reasoning = self.operator_reasoning_rect(layout.header);
        let title_width = if layout.header.height > 1 {
            layout.header.width
        } else {
            reasoning.x.saturating_sub(layout.header.x)
        };
        let title = format!("{} · {}", identity(chat), status(chat));
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
        let suffix = " ▾ Ctrl-t]";
        let label = clip_text(
            &self.operator_reasoning_label(),
            usize::from(reasoning.width).saturating_sub(suffix.width() + 1),
        );
        frame.render_widget(
            Paragraph::new(format!("[{label}{suffix}")).style(Style::default().fg(MAUVE())),
            reasoning,
        );
        let rows = transcript_rows(chat, layout.transcript.width, now_unix_ms);
        let height = layout.transcript.height as usize;
        let offset = transcript_offset(chat, &rows, height);
        let lines: Vec<_> = rows
            .into_iter()
            .skip(offset)
            .take(height)
            .map(|row| {
                if self.operator.transcript_focus
                    && row
                        .entry
                        .is_some_and(|entry| chat.entry_focus == Some(entry.ordinal))
                {
                    row.line.style(Style::default().bg(SURFACE0()))
                } else {
                    row.line
                }
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), layout.transcript);
        let error = chat.error.as_deref().or(self.operator.error.as_deref());
        let context = layout.context;
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
        let focused = self.operator.editing && !self.operator.transcript_focus;
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
        self.draw_operator_reasoning_picker(frame);
    }

    fn draw_operator_reasoning_picker(&self, frame: &mut Frame<'_>) {
        let Some(picker) = &self.operator.reasoning_picker else {
            return;
        };
        let outer = frame.area();
        let chat = self.operator.chat.as_ref();
        let state = chat.and_then(|chat| chat.summary.reasoning.as_ref());
        let model = chat
            .and_then(|chat| chat.summary.model.as_ref())
            .map_or("model unknown", |model| model.id.as_str());
        let width = outer.width.min(56);
        let unsupported = if let Some(state) = state {
            let names: Vec<_> = ThinkingLevel::ALL
                .into_iter()
                .skip(1)
                .filter(|level| !state.available.contains(level))
                .map(ThinkingLevel::as_str)
                .collect();
            if names.is_empty() {
                "All levels offered".into()
            } else {
                format!("not offered: {}", names.join(", "))
            }
        } else {
            "Supported levels available after the first turn".into()
        };
        let mut lines: Vec<Line<'static>> = picker
            .choices
            .iter()
            .enumerate()
            .map(|(index, level)| {
                let tick = if state.is_some_and(|state| state.level == *level) {
                    "✓"
                } else {
                    " "
                };
                let label = if *level == ThinkingLevel::Default {
                    "default (Pi's choice)"
                } else {
                    level.as_str()
                };
                Line::raw(format!("{tick} {label}")).style(if index == picker.selected {
                    Style::default().fg(TEXT()).bg(SURFACE2())
                } else {
                    Style::default().fg(TEXT())
                })
            })
            .collect();
        lines.extend(wrapped_lines(
            vec![Line::raw(unsupported)],
            width.saturating_sub(2),
        ));
        lines.push(Line::raw("↑↓ Enter pick · Esc close").style(Style::default().fg(MAUVE())));
        let height = (lines.len() as u16 + 2).min(outer.height);
        let area = Rect::new(
            outer.x + (outer.width - width) / 2,
            outer.y + (outer.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Reasoning · {} ", display_text(model)))
                    .style(Style::default().fg(TEXT()).bg(BASE()))
                    .border_style(Style::default().fg(MAUVE())),
            ),
            area,
        );
    }
}

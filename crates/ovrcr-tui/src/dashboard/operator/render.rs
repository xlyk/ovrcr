//! The shared sidebar is the sole conversation list and hit-test geometry.
use super::*;
use crate::dashboard::render::{
    BASE, BLUE, CRUST, GREEN, MAUVE, MUTED, PEACH, SUBTEXT, SURFACE0, SURFACE2, TEAL, TEXT,
    clip_text, compose_row,
};
use ovrcr_protocol::AgentActivity;
use ratatui::style::Color;

struct SidebarStatus {
    glyph: char,
    color: Color,
    label: String,
    right: String,
    detail: String,
    detail_color: Color,
}

fn elapsed(timestamp: Option<u64>, now: u64) -> String {
    let Some(seconds) = timestamp
        .and_then(|stamp| now.checked_sub(stamp))
        .map(|millis| millis / 1_000)
    else {
        return String::new();
    };
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3_600, (seconds % 3_600) / 60),
    }
}

fn age(timestamp: Option<u64>, now: u64) -> String {
    let Some(seconds) = timestamp
        .and_then(|stamp| now.checked_sub(stamp))
        .map(|millis| millis / 1_000)
    else {
        return String::new();
    };
    match seconds {
        0..60 => format!("{}s", seconds),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

fn format_age(timestamp: Option<u64>, now: u64) -> String {
    let age = age(timestamp, now);
    if age.is_empty() {
        age
    } else {
        format!("last reply {age}")
    }
}

fn setup_failure(error: Option<&str>) -> bool {
    error.is_some_and(|error| {
        error.starts_with("Pi is unavailable;")
            || error.starts_with("No Pi model selected;")
            || error.starts_with("Configured Operator model is unavailable")
    })
}

fn pi_available() -> bool {
    let executable = std::env::var_os("OVRCR_PI_EXECUTABLE").unwrap_or_else(|| "pi".into());
    let path = std::env::var_os("PATH").unwrap_or_default();
    super::super::agents::executable_available(&executable, &path)
}

fn current_activity(chat: &Chat) -> String {
    if chat.submitting {
        return "sending message".into();
    }
    if chat.stopping {
        return "stopping the worker".into();
    }
    let Some(turn) = chat.summary.current_turn.as_ref() else {
        return "responding".into();
    };
    let mut finished = std::collections::BTreeSet::new();
    for message in chat
        .messages
        .iter()
        .rev()
        .filter(|message| message.turn == turn.identity.turn)
    {
        match &message.content {
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput { id, .. }) => {
                finished.insert(id.as_str());
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                id,
                name,
                arguments,
            }) if !finished.contains(id.as_str()) => {
                let command = (name == "bash")
                    .then(|| serde_json::from_str::<serde_json::Value>(arguments).ok())
                    .flatten()
                    .and_then(|arguments| arguments.get("command")?.as_str().map(str::to_owned));
                return command.map_or_else(
                    || format!("running {}", display_text(name)),
                    |command| format!("running {}", display_text(&command)),
                );
            }
            _ => {}
        }
    }
    "responding".into()
}

fn activity(chat: &Chat) -> AgentActivity {
    if chat.submitting || chat.stopping {
        return AgentActivity::Busy;
    }
    match chat.summary.current_turn.as_ref().map(|turn| turn.state) {
        Some(OperatorTurnState::Running) => AgentActivity::Busy,
        Some(_) if !chat.summary.worker_live && !chat.summary.worker_stopped => AgentActivity::Busy,
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
        self.settings.operator.sidebar
            && self
                .operator
                .chat
                .as_ref()
                .is_some_and(|chat| activity(chat) == AgentActivity::Busy)
    }
    pub(super) fn capture_operator_setup_for_open(&mut self) {
        if self.operator.chat.is_none() || self.operator.sidebar_setup_unavailable {
            self.operator.sidebar_setup_unavailable = !pi_available();
        }
    }
    fn operator_sidebar_status(&self, now: u64) -> SidebarStatus {
        let setup_failed = setup_failure(self.operator.error.as_deref())
            || self
                .operator
                .chat
                .as_ref()
                .is_some_and(|chat| setup_failure(chat.error.as_deref()));
        let should_check_pi =
            self.operator.chat.is_none() || self.operator.sidebar_setup_unavailable;
        if setup_failed || (should_check_pi && !pi_available()) {
            return SidebarStatus {
                glyph: '−',
                color: PEACH(),
                label: "Not set up".into(),
                right: String::new(),
                detail: "run ovrcr operator setup".into(),
                detail_color: SUBTEXT(),
            };
        }
        let Some(chat) = &self.operator.chat else {
            if self.operator.visible || self.operator.loading {
                return SidebarStatus {
                    glyph: '·',
                    color: MUTED(),
                    label: "Idle".into(),
                    right: String::new(),
                    detail: "No replies yet · o open".into(),
                    detail_color: SUBTEXT(),
                };
            }
            return SidebarStatus {
                glyph: '·',
                color: MUTED(),
                label: "Off".into(),
                right: String::new(),
                detail: "o open · Ctrl-o quick message".into(),
                detail_color: SUBTEXT(),
            };
        };
        if activity(chat) == AgentActivity::Busy {
            let label = if chat.stopping {
                "Stopping"
            } else if chat.submitting {
                "Sending"
            } else {
                "Responding"
            };
            return SidebarStatus {
                glyph: crate::dashboard::status::sidebar_activity_glyph(
                    AgentActivity::Busy,
                    false,
                    now,
                )
                .0,
                color: TEAL(),
                label: label.into(),
                right: elapsed(chat.summary.current_turn_started_unix_ms, now),
                detail: format!("↳ {}", current_activity(chat)),
                detail_color: SUBTEXT(),
            };
        }
        if chat.unread.is_some() {
            return SidebarStatus {
                glyph: '✓',
                color: GREEN(),
                label: "Reply ready · 1 unread".into(),
                right: format_age(chat.summary.last_reply_unix_ms, now),
                detail: "o open to review · Ctrl-o message".into(),
                detail_color: SUBTEXT(),
            };
        }
        let detail = if chat.summary.last_reply_unix_ms.is_some() {
            "o open · Ctrl-o quick message".into()
        } else {
            "No replies yet · o open".into()
        };
        SidebarStatus {
            glyph: '·',
            color: MUTED(),
            label: "Idle".into(),
            right: format_age(chat.summary.last_reply_unix_ms, now),
            detail,
            detail_color: MUTED(),
        }
    }
    /// The Operator's own navigation card: state and timing on a tinted surface
    /// that does not read as a project heading or a session row. It deliberately
    /// omits harness and model metadata, and never echoes reply text as a title.
    pub(in crate::dashboard) fn draw_operator_sidebar(&self, frame: &mut Frame<'_>, now: u64) {
        if !self.settings.operator.sidebar {
            return;
        }
        let area = self.sidebar_rects(frame.area()).operator;
        if area.is_empty() {
            return;
        }
        let width = usize::from(area.width);
        let selected = self.operator.visible;
        let status = self.operator_sidebar_status(now);
        let background = if selected { SURFACE0() } else { CRUST() };
        frame.render_widget(
            Block::default().style(Style::default().bg(background)),
            area,
        );
        let rail = Span::styled("┃", Style::default().fg(MAUVE()));
        let identity = Span::styled(
            "󰚩 Operator",
            Style::default().fg(MAUVE()).add_modifier(Modifier::BOLD),
        );
        if area.height == 1 {
            let compact = compose_row(
                vec![rail, Span::raw(" "), identity],
                Span::raw(" "),
                vec![Span::styled(
                    format!("{} {}", status.glyph, status.label),
                    Style::default().fg(status.color),
                )],
                width,
                false,
            );
            frame.render_widget(
                Paragraph::new(compact).style(Style::default().fg(TEXT()).bg(background)),
                area,
            );
            return;
        }
        let title = compose_row(
            vec![rail.clone(), Span::raw(" "), identity],
            Span::raw(" "),
            Vec::new(),
            width,
            false,
        );
        frame.render_widget(
            Paragraph::new(title).style(Style::default().fg(TEXT()).bg(background)),
            Rect::new(area.x, area.y, area.width, 1),
        );

        let right = if status.right.is_empty() {
            Vec::new()
        } else {
            vec![Span::styled(
                format!("{} ", status.right),
                Style::default().fg(SUBTEXT()),
            )]
        };
        let status_line = compose_row(
            vec![
                rail.clone(),
                Span::raw(" "),
                Span::styled(status.glyph.to_string(), Style::default().fg(status.color)),
                Span::raw(" "),
                Span::styled(status.label.clone(), Style::default().fg(status.color)),
            ],
            Span::raw(" "),
            right,
            width,
            false,
        );
        frame.render_widget(
            Paragraph::new(status_line).style(Style::default().fg(TEXT()).bg(background)),
            Rect::new(area.x, area.y + 1, area.width, 1),
        );
        if area.height >= 3 {
            let detail = compose_row(
                vec![
                    rail,
                    Span::raw(" "),
                    Span::styled(
                        clip_text(&status.detail, width.saturating_sub(2)),
                        Style::default().fg(status.detail_color),
                    ),
                ],
                Span::raw(" "),
                Vec::new(),
                width,
                false,
            );
            frame.render_widget(
                Paragraph::new(detail).style(Style::default().fg(TEXT()).bg(background)),
                Rect::new(area.x, area.y + 2, area.width, 1),
            );
        }
    }
    pub(super) fn operator_sidebar_mouse(
        &mut self,
        mouse: MouseEvent,
        area: Rect,
    ) -> Option<DashboardAction> {
        if !self.settings.operator.sidebar {
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

#[derive(Clone)]
pub(super) struct TranscriptRow {
    pub(super) line: Line<'static>,
    pub(super) anchor: Option<Anchor>,
    pub(super) entry: Option<TranscriptEntry>,
}

/// Everything the journal rows are built from. Scrolling, focus moves and
/// spinner frames leave it unchanged, so they reuse the wrapped rows.
#[derive(PartialEq)]
pub(super) struct RowsKey {
    width: u16,
    // The journal is append-only, so its length and tail identify its content.
    messages: usize,
    tail: Option<u64>,
    expanded: BTreeMap<u64, bool>,
    workdir: Option<std::path::PathBuf>,
    current: Option<(OperatorTurnId, OperatorTurnState)>,
    unread: Option<OperatorTurnId>,
    theme: crate::theme::ThemeTokens,
}

#[cfg(test)]
thread_local! {
    pub(super) static ROW_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
                // One span per style run, not per grapheme: rows stay cheap to
                // build, clone and draw.
                match spans.last_mut() {
                    Some(Span { content, style }) if *style == grapheme.style => {
                        content.to_mut().push_str(grapheme.symbol);
                    }
                    _ => spans.push(Span::styled(grapheme.symbol.to_owned(), grapheme.style)),
                }
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

pub(super) fn transcript_rows(chat: &Chat, width: u16, now_unix_ms: u64) -> Vec<TranscriptRow> {
    let key = RowsKey {
        width,
        messages: chat.messages.len(),
        tail: chat.messages.last().map(|message| message.ordinal),
        expanded: chat.expanded.clone(),
        workdir: chat.workdir.clone(),
        current: chat
            .summary
            .current_turn
            .as_ref()
            .map(|turn| (turn.identity.turn, turn.state)),
        unread: chat.unread,
        theme: crate::theme::active(),
    };
    let mut cache = chat.rows.borrow_mut();
    if cache.as_ref().is_none_or(|(cached, _)| *cached != key) {
        #[cfg(test)]
        ROW_BUILDS.with(|builds| builds.set(builds.get() + 1));
        *cache = Some((key, super::feed::feed_rows(chat, width)));
    }
    // ponytail: clones every cached row per call and rebuilds the feed on
    // append. Only open entries are wrapped, so both stay small; cache per
    // entry if a journal ever holds thousands of open ones.
    let mut rows = cache
        .as_ref()
        .map_or_else(Vec::new, |(_, rows)| rows.clone());
    drop(cache);
    if let Some(activity) = conversation_activity(chat) {
        if rows.last().is_some_and(|row| {
            row.anchor
                .is_some_and(|anchor| anchor.1 == super::feed::GAP)
        }) {
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
                Span::styled(
                    "▌   ",
                    Style::default().fg(if activity.animated {
                        BLUE()
                    } else {
                        SURFACE2()
                    }),
                ),
                Span::styled(
                    clip_text(&label, usize::from(width.saturating_sub(4))),
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
/// Blank rows above a feed shorter than its viewport.
pub(super) fn transcript_pad(rows: usize, height: usize) -> usize {
    height.saturating_sub(rows)
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
        let stop = (!chat.summary.worker_live && !chat.summary.worker_stopped)
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
        // A short feed sits against the composer, not the header.
        let pad = transcript_pad(rows.len(), height);
        let lines: Vec<_> = std::iter::repeat_n(Line::default(), pad)
            .chain(rows.into_iter().skip(offset).take(height).map(|row| {
                if self.operator.transcript_focus
                    && row
                        .entry
                        .is_some_and(|entry| chat.entry_focus == Some(entry.ordinal))
                {
                    row.line.style(Style::default().bg(SURFACE0()))
                } else {
                    row.line
                }
            }))
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

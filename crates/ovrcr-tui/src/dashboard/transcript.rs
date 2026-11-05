//! The Transcript view: the conversation of an agent drawn on the alternate screen, where
//! terminal History has nothing to show.
//!
//! History's alternate-screen refusal (`Dashboard::alternate_screen_history`) asks the
//! Server for the newest page of the session's own transcript. A page opens this
//! read-only popup (`Details::Transcript`); no transcript (another provider, no
//! conversation yet) leaves PageUp's muted footer line and keeps a wheel tick quiet.
//! Scrolling to the top asks for the page before; End reloads the newest page.

use super::render::{BASE, MAUVE, MUTED, PEACH, SUBTEXT, TEAL, TEXT};
use super::{Dashboard, DashboardAction};
use crate::protocol::{
    ClientMessage, ConversationReference, Request, Response, SessionId, SessionRunId,
    TranscriptEntry, TranscriptPage,
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

const MAX_VIEW_BYTES: usize = 1024 * 1024;
const MAX_VIEW_ENTRIES: usize = 16_384;
/// A rendered logical line must fit Ratatui's u16 scroll offset even at one cell wide.
const MAX_LOGICAL_LINE_CHARS: usize = 60_000;

#[derive(Default)]
pub(super) struct View {
    pub(super) session: Option<SessionId>,
    pub(super) run: Option<SessionRunId>,
    title: String,
    /// The request in flight and the `before` it asked for.
    request: Option<(u64, Option<u64>)>,
    /// Still waiting for the first page; `true` when a wheel tick asked.
    pub(super) opening: Option<bool>,
    entries: Vec<TranscriptEntry>,
    /// The provider reference returned by the first page; continuation cannot mix runs.
    identity: Option<ConversationReference>,
    /// The loaded pages start here in the provider's file; zero is the beginning.
    start: u64,
    scroll: usize,
    /// Keep the newest line in view until the user scrolls up.
    stick: bool,
    /// The bounded window has evicted entries; older pages can still be loaded.
    windowed: bool,
    error: Option<String>,
}

fn popup_area(outer: Rect) -> Rect {
    let width = outer.width.saturating_sub(4);
    let height = outer.height.saturating_sub(2);
    Rect::new(
        outer.x + (outer.width - width) / 2,
        outer.y + (outer.height - height) / 2,
        width,
        height,
    )
}

fn body_rect(outer: Rect) -> Rect {
    Block::bordered().inner(popup_area(outer))
}

fn lines(view: &View) -> Vec<Line<'static>> {
    let text = |text: &str| super::operator::display_text(text);
    let mut lines = Vec::new();
    if view.windowed {
        let note = if view.start > 0 {
            "Bounded window · scroll up for older messages · End for newest."
        } else {
            "Bounded window · End reloads the newest page."
        };
        lines.push(Line::styled(note, Style::default().fg(MUTED())));
    } else if view.start > 0 {
        let note = match view.request {
            Some((_, Some(_))) => "Loading earlier messages…",
            Some((_, None)) => "Loading newest messages…",
            None => "Earlier messages load as you scroll up.",
        };
        lines.push(Line::styled(note, Style::default().fg(MUTED())));
    } else if view.opening.is_some() {
        lines.push(Line::styled(
            "Loading transcript…",
            Style::default().fg(MUTED()),
        ));
    } else if view.entries.is_empty() {
        lines.push(Line::styled(
            "No messages yet.",
            Style::default().fg(MUTED()),
        ));
    }
    if let Some(error) = &view.error {
        lines.push(Line::styled(text(error), Style::default().fg(PEACH())));
    }
    for entry in &view.entries {
        match entry {
            TranscriptEntry::User(prompt) => {
                if !lines.is_empty() {
                    lines.push(Line::default());
                }
                for (index, row) in text(prompt).split('\n').enumerate() {
                    let gutter = if index == 0 { "❯ " } else { "  " };
                    lines.push(Line::from(vec![
                        Span::styled(gutter, Style::default().fg(MAUVE())),
                        Span::styled(
                            bounded_line(row),
                            Style::default().fg(MAUVE()).add_modifier(Modifier::BOLD),
                        ),
                    ]));
                }
            }
            TranscriptEntry::Assistant(reply) => {
                for row in text(reply).split('\n') {
                    lines.push(Line::styled(
                        format!("  {}", bounded_line(row)),
                        Style::default().fg(TEXT()),
                    ));
                }
            }
            TranscriptEntry::Tool { name, input } => {
                let name = bounded_tool_text(&text(name), 512);
                let input = bounded_tool_text(&text(input).replace('\n', " "), 2048);
                lines.push(Line::from(vec![
                    Span::styled(format!("  ● {name}"), Style::default().fg(TEAL())),
                    Span::styled(format!("  {input}"), Style::default().fg(SUBTEXT())),
                ]));
            }
        }
    }
    lines
}

fn row_count(view: &View, width: u16) -> usize {
    Paragraph::new(lines(view))
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
}

fn entry_bytes(entry: &TranscriptEntry) -> usize {
    match entry {
        TranscriptEntry::User(text) | TranscriptEntry::Assistant(text) => text.len(),
        TranscriptEntry::Tool { name, input } => name.len().saturating_add(input.len()),
    }
}

fn bounded_line(text: &str) -> String {
    bounded_tool_text(text, MAX_LOGICAL_LINE_CHARS)
}

fn bounded_tool_text(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut clipped: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        clipped.push('…');
    }
    clipped
}

/// Drops the newest tail only while paging older; the opening page instead keeps its tail.
fn trim_view(view: &mut View, keep_newest: bool) -> bool {
    let mut bytes = view.entries.iter().fold(0_usize, |total, entry| {
        total.saturating_add(entry_bytes(entry))
    });
    if keep_newest {
        let mut drop = 0;
        while (bytes > MAX_VIEW_BYTES || view.entries.len() - drop > MAX_VIEW_ENTRIES)
            && drop < view.entries.len()
        {
            bytes = bytes.saturating_sub(entry_bytes(&view.entries[drop]));
            drop += 1;
        }
        if drop > 0 {
            view.entries.drain(..drop);
        }
        drop > 0
    } else {
        let mut dropped = false;
        while bytes > MAX_VIEW_BYTES || view.entries.len() > MAX_VIEW_ENTRIES {
            let Some(entry) = view.entries.pop() else {
                break;
            };
            bytes = bytes.saturating_sub(entry_bytes(&entry));
            dropped = true;
        }
        dropped
    }
}

/// Paragraph stores its scroll offset as u16. Discard whole logical lines at the top until
/// the remaining visual-row offset fits, so long conversations still render their real tail.
fn scrollable_lines(
    mut lines: Vec<Line<'static>>,
    width: u16,
    scroll: usize,
) -> (Vec<Line<'static>>, u16) {
    let width = width.max(1);
    let mut remaining = scroll;
    let mut skip = 0;
    while remaining > usize::from(u16::MAX) && skip < lines.len() {
        let rows = Paragraph::new(vec![lines[skip].clone()])
            .wrap(Wrap { trim: false })
            .line_count(width);
        if rows > remaining {
            break;
        }
        remaining -= rows.max(1);
        skip += 1;
    }
    if skip > 0 {
        lines.drain(..skip);
    }
    (lines, remaining.min(usize::from(u16::MAX)) as u16)
}

impl Dashboard {
    /// Asks for the newest page of `session`'s transcript; the answer opens the view.
    pub(super) fn open_transcript(&mut self, session: SessionId, at_tail: bool) {
        if self.transcript.opening.is_some() && self.transcript.session == Some(session) {
            return;
        }
        if self.details.is_some()
            || self.palette.is_some()
            || self.whichkey.is_some()
            || self.tasks.is_some()
            || self.copy.is_some()
            || self.mode != super::InputMode::Browse
            || self.action_session() != Some(session)
        {
            return;
        }
        self.close_transcript();
        let Some(summary) = super::state::find_session(self, session) else {
            return;
        };
        let run = summary.run;
        let title = self.session_display_name(summary);
        self.transcript = View {
            session: Some(session),
            run: Some(run),
            title,
            opening: Some(at_tail),
            stick: true,
            ..View::default()
        };
        self.open_details(super::quota::Details::Transcript);
        self.request_transcript_page(None);
    }

    fn request_transcript_page(&mut self, before: Option<u64>) {
        let (Some(session), Some(run)) = (self.transcript.session, self.transcript.run) else {
            return;
        };
        let identity = before.and_then(|_| self.transcript.identity.clone());
        if before.is_some() && identity.is_none() {
            return;
        }
        if let Some((request_id, pending_before)) = self.transcript.request {
            if before.is_some() || pending_before.is_none() {
                return;
            }
            self.transcript.request = None;
            self.ignored_responses.insert(request_id);
        }
        let request_id = self.next_request_id();
        self.transcript.request = Some((request_id, before));
        self.enqueue_request(ClientMessage {
            request_id,
            request: Request::Transcript {
                session,
                run,
                identity,
                before,
            },
        });
    }

    pub(super) fn close_transcript(&mut self) {
        if let Some((request_id, _)) = self.transcript.request.take() {
            self.ignored_responses.insert(request_id);
        }
        self.transcript = View::default();
        if self
            .details
            .is_some_and(|(details, _)| details == super::quota::Details::Transcript)
        {
            self.details = None;
        }
    }

    /// Claims the page this view asked for.
    pub(super) fn transcript_response(&mut self, request_id: u64, response: &Response) -> bool {
        let Some((_, before)) = self
            .transcript
            .request
            .take_if(|(pending, _)| *pending == request_id)
        else {
            return false;
        };
        if let Some(at_tail) = self.transcript.opening.take() {
            let session = self.transcript.session;
            // Something else took the screen meanwhile: the open is no longer wanted.
            let wanted = self.mode == super::InputMode::Browse
                && self.copy.is_none()
                && self.history.is_none()
                && !self.operator.visible
                && !self.operator_quick_visible()
                && self
                    .details
                    .is_some_and(|(details, _)| details == super::quota::Details::Transcript)
                && self.palette.is_none()
                && self.whichkey.is_none()
                && self.tasks.is_none()
                && self.action_session() == session
                && session.is_some_and(|id| {
                    super::state::find_session(self, id)
                        .is_some_and(|summary| Some(summary.run) == self.transcript.run)
                });
            match response {
                Response::Transcript(page) if wanted => {
                    self.transcript_page(page, before);
                }
                Response::Transcript(_) => self.close_transcript(),
                _ => {
                    // No transcript to show: PageUp explains, a wheel tick stays quiet.
                    let title = std::mem::take(&mut self.transcript.title);
                    self.close_transcript();
                    if wanted && !at_tail {
                        self.set_limit(format!(
                            "History isn't available while {title} is full-screen"
                        ));
                    }
                }
            }
            return true;
        }
        match response {
            Response::Transcript(page) => {
                self.transcript.error = None;
                if before.is_some() && self.transcript.identity.as_ref() != Some(&page.identity) {
                    self.transcript.error = Some(
                        "This conversation changed. Press End to load the current transcript."
                            .into(),
                    );
                    return true;
                }
                self.transcript_page(page, before);
            }
            Response::Error { message, .. } => self.transcript.error = Some(message.clone()),
            _ => self.transcript.error = Some("Unexpected transcript response.".into()),
        }
        true
    }

    fn transcript_page(&mut self, page: &TranscriptPage, before: Option<u64>) {
        let width = body_rect(self.outer_area).width;
        let view = &mut self.transcript;
        if before.is_none() {
            view.entries = page.entries.clone();
            view.start = page.start;
            view.identity = Some(page.identity.clone());
            view.stick = true;
            view.windowed = trim_view(view, true);
            return;
        }
        // Prepend, keeping the rows on screen where they were.
        let rows = row_count(view, width);
        let mut entries = page.entries.clone();
        entries.append(&mut view.entries);
        let mut combined = View {
            start: page.start,
            entries,
            ..View::default()
        };
        let inserted_rows = row_count(&combined, width).saturating_sub(rows);
        let truncated = trim_view(&mut combined, false);
        view.entries = combined.entries;
        view.start = page.start;
        view.windowed |= truncated;
        if !view.stick {
            view.scroll = view.scroll.saturating_add(inserted_rows);
        }
    }

    pub(super) fn transcript_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
        }
        let page = usize::from(body_rect(self.outer_area).height.max(2) - 1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_transcript(),
            _ if super::input::is_browse_key(key) => self.close_transcript(),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_transcript(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_transcript(1),
            KeyCode::PageUp => self.scroll_transcript(-(page as isize)),
            KeyCode::PageDown => self.scroll_transcript(page as isize),
            KeyCode::Home => self.scroll_transcript(isize::MIN / 2),
            KeyCode::End => {
                self.transcript.stick = true;
                self.transcript.windowed = false;
                self.transcript.error = None;
                self.request_transcript_page(None);
            }
            _ => {}
        }
        DashboardAction::Redraw
    }

    pub(super) fn transcript_mouse(&mut self, mouse: MouseEvent) -> DashboardAction {
        match mouse.kind {
            MouseEventKind::ScrollDown => self.scroll_transcript(3),
            MouseEventKind::ScrollUp => self.scroll_transcript(-3),
            _ => return DashboardAction::None,
        }
        DashboardAction::Redraw
    }

    fn scroll_transcript(&mut self, delta: isize) {
        let body = body_rect(self.outer_area);
        let max = row_count(&self.transcript, body.width).saturating_sub(usize::from(body.height));
        let view = &mut self.transcript;
        let current = if view.stick {
            max
        } else {
            view.scroll.min(max)
        };
        let next = current.saturating_add_signed(delta).min(max);
        view.stick = next >= max;
        view.scroll = next;
        if next == 0 && view.start > 0 {
            self.request_transcript_page(Some(self.transcript.start));
        }
    }

    pub(super) fn draw_transcript(&self, frame: &mut Frame<'_>) {
        let area = popup_area(frame.area());
        let title = format!(
            "Transcript · {} · ↑↓ scroll · Home/End · Esc close",
            super::operator::display_text(&self.transcript.title)
        );
        let block = Block::bordered()
            .title(super::render::clip_text(
                &title,
                usize::from(area.width.saturating_sub(2)),
            ))
            .style(Style::default().bg(BASE()).fg(TEXT()));
        let inner = block.inner(area);
        let paragraph = Paragraph::new(lines(&self.transcript)).wrap(Wrap { trim: false });
        let max = paragraph
            .line_count(inner.width.max(1))
            .saturating_sub(usize::from(inner.height));
        let scroll = if self.transcript.stick {
            max
        } else {
            self.transcript.scroll.min(max)
        };
        let (lines, scroll) = scrollable_lines(lines(&self.transcript), inner.width, scroll);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0)),
            inner,
        );
    }
}

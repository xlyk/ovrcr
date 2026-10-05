//! The Events popup: the Server's ring, newest last.
//!
//! Opened from the menu and the palette (`Details::Events`). The Dashboard
//! asks for the ring when the popup opens and appends `ServerEvent::Recorded`
//! while it stays open. It does not read or write `events.jsonl`.

use super::quota::local_time;
use super::render::{BASE, MAUVE, MUTED, PEACH, SUBTEXT, TEXT};
use super::text_cursor::TextCursor;
use super::{Dashboard, DashboardAction};
use crate::protocol::Event as Recorded;
use crate::protocol::{ClientMessage, EventComponent, Request, Response};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Line,
    widgets::{Block, Clear, Paragraph, Wrap},
};

pub(super) const TITLE: &str = "Events · / filter · Tab component · ↑↓ scroll · Esc close";

const COMPONENTS: [Option<EventComponent>; 4] = [
    None,
    Some(EventComponent::Titles),
    Some(EventComponent::Settings),
    Some(EventComponent::Quota),
];

#[derive(Default)]
pub(super) struct View {
    request: Option<u64>,
    loaded: bool,
    events: Vec<Recorded>,
    /// Recorded while the snapshot request is still in flight.
    pending: Vec<Recorded>,
    filter: String,
    filtering: bool,
    cursor: TextCursor,
    /// `None` is every component.
    component: Option<EventComponent>,
    scroll: usize,
    /// Keep the newest line in view until the user scrolls up.
    stick: bool,
    error: Option<String>,
}

impl View {
    fn loading(request: u64) -> Self {
        Self {
            request: Some(request),
            stick: true,
            ..Self::default()
        }
    }
}

pub(super) fn component_name(component: EventComponent) -> &'static str {
    match component {
        EventComponent::Titles => "titles",
        EventComponent::Settings => "settings",
        EventComponent::Quota => "quota",
    }
}

fn component_label(component: Option<EventComponent>) -> &'static str {
    component.map(component_name).unwrap_or("all")
}

/// Every word is a case-insensitive substring of the component, subject, and
/// message. The timestamp is not part of the filter.
fn matches_words(event: &Recorded, filter: &str) -> bool {
    let subject = event.subject.as_deref().unwrap_or("");
    let haystack = format!(
        "{} {subject} {}",
        component_name(event.component),
        event.message
    )
    .to_lowercase();
    filter
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

fn visible<'a>(
    events: &'a [Recorded],
    filter: &str,
    component: Option<EventComponent>,
) -> Vec<&'a Recorded> {
    events
        .iter()
        .filter(|event| component.is_none_or(|component| event.component == component))
        .filter(|event| matches_words(event, filter))
        .collect()
}

/// Drop the prefix of `buffered` that is already the tail of `snapshot`.
fn merge(snapshot: &[Recorded], buffered: &[Recorded]) -> Vec<Recorded> {
    let mut skip = 0;
    for len in 1..=buffered.len().min(snapshot.len()) {
        if snapshot[snapshot.len() - len..] == buffered[..len] {
            skip = len;
        }
    }
    let mut events = snapshot.to_vec();
    events.extend(buffered[skip..].iter().cloned());
    events
}

fn event_line(event: &Recorded) -> String {
    let subject = event
        .subject
        .as_ref()
        .map(|subject| format!("  {subject}"))
        .unwrap_or_default();
    format!(
        "{}  {}{subject}  {}",
        local_time(event.time_unix_ms),
        component_name(event.component),
        event.message
    )
}

fn popup_area(outer: Rect) -> Rect {
    let width = outer.width.saturating_sub(4).min(82);
    let height = outer.height.saturating_sub(2).min(30);
    Rect::new(
        outer.x + (outer.width - width) / 2,
        outer.y + (outer.height - height) / 2,
        width,
        height,
    )
}

impl Dashboard {
    pub(super) fn open_events(&mut self) -> DashboardAction {
        if let Some(request_id) = self.events.request.take() {
            self.ignored_responses.insert(request_id);
        }
        self.open_details(super::quota::Details::Events);
        let request_id = self.next_request_id();
        self.events = View::loading(request_id);
        DashboardAction::Request(ClientMessage {
            request_id,
            request: Request::Events { follow: false },
        })
    }

    pub(super) fn close_events(&mut self) {
        if let Some(request_id) = self.events.request.take() {
            self.ignored_responses.insert(request_id);
        }
        self.events = View::default();
        self.details = None;
    }

    pub(super) fn events_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
        }
        if super::input::is_browse_key(key) {
            self.close_events();
            return DashboardAction::Redraw;
        }
        if self.events.filtering {
            let view = &mut self.events;
            match key.code {
                KeyCode::Esc => {
                    view.filter.clear();
                    view.filtering = false;
                    view.stick = true;
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Up => {
                    view.filtering = false;
                    view.stick = true;
                }
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    view.cursor.insert(&mut view.filter, &ch.to_string());
                    view.stick = true;
                }
                code => view.cursor.key(&mut view.filter, code),
            }
            return DashboardAction::Redraw;
        }
        match key.code {
            KeyCode::Esc if !self.events.filter.is_empty() => {
                self.events.filter.clear();
                self.events.stick = true;
            }
            KeyCode::Esc => self.close_events(),
            KeyCode::Char('/') => {
                let view = &mut self.events;
                view.filtering = true;
                view.filter.clear();
                view.cursor = TextCursor::default();
                view.stick = true;
            }
            KeyCode::Tab => self.cycle_component(1),
            KeyCode::BackTab => self.cycle_component(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_events(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_events(-1),
            KeyCode::PageDown => self.scroll_events(10),
            KeyCode::PageUp => self.scroll_events(-10),
            KeyCode::Home => {
                self.events.stick = false;
                self.events.scroll = 0;
            }
            KeyCode::End => self.events.stick = true,
            _ => {}
        }
        DashboardAction::Redraw
    }

    pub(super) fn events_paste(&mut self, text: &str) -> DashboardAction {
        if !self.events.filtering {
            return DashboardAction::None;
        }
        let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
        self.events.cursor.insert(&mut self.events.filter, &text);
        self.events.stick = true;
        DashboardAction::Redraw
    }

    pub(super) fn events_mouse(&mut self, mouse: MouseEvent) -> DashboardAction {
        match mouse.kind {
            MouseEventKind::ScrollDown => self.scroll_events(1),
            MouseEventKind::ScrollUp => self.scroll_events(-1),
            _ => {}
        }
        DashboardAction::Redraw
    }

    /// Claims the snapshot this popup asked for.
    pub(super) fn events_response(&mut self, request_id: u64, response: &Response) -> bool {
        if self.events.request != Some(request_id) {
            return false;
        }
        self.events.request = None;
        let pending = std::mem::take(&mut self.events.pending);
        match response {
            Response::Events(events) => {
                self.events.events = merge(events, &pending);
                self.events.error = None;
                self.events.loaded = true;
            }
            Response::Error { message, .. } => {
                self.events.events = pending;
                self.events.error = Some(message.clone());
                self.events.loaded = true;
            }
            _ => {
                self.events.events = pending;
                self.events.error = Some("Unexpected events response.".into());
                self.events.loaded = true;
            }
        }
        true
    }

    pub(super) fn events_recorded(&mut self, event: Recorded) {
        let open = self
            .details
            .is_some_and(|(details, _)| details == super::quota::Details::Events);
        if !open {
            return;
        }
        if self.events.request.is_some() {
            self.events.pending.push(event);
        } else if self.events.loaded {
            self.events.events.push(event);
        }
    }

    fn cycle_component(&mut self, delta: isize) {
        let index = COMPONENTS
            .iter()
            .position(|component| *component == self.events.component)
            .unwrap_or(0);
        let next = (index as isize + delta).rem_euclid(COMPONENTS.len() as isize) as usize;
        self.events.component = COMPONENTS[next];
        self.events.stick = true;
    }

    fn scroll_events(&mut self, delta: isize) {
        let max = self.events_max_scroll(self.outer_area);
        if delta < 0 {
            let step = delta.unsigned_abs();
            // The first scroll up leaves the newest line, so start from the bottom.
            let start = if self.events.stick {
                max
            } else {
                self.events.scroll.min(max)
            };
            self.events.stick = false;
            self.events.scroll = start.saturating_sub(step);
        } else if !self.events.stick {
            let next = self.events.scroll.min(max).saturating_add(delta as usize);
            if next >= max {
                self.events.stick = true;
                self.events.scroll = max;
            } else {
                self.events.scroll = next;
            }
        }
    }

    fn events_max_scroll(&self, outer: Rect) -> usize {
        let header = self.events_header();
        let lines = self.events_body();
        let body = events_body_rect(outer, header.len() as u16);
        if body.width == 0 || body.height == 0 {
            return 0;
        }
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .line_count(body.width)
            .saturating_sub(usize::from(body.height))
    }

    fn events_header(&self) -> Vec<Line<'static>> {
        let style = |color| Style::default().fg(color);
        let mut lines = vec![Line::styled(
            format!("Component: {}", component_label(self.events.component)),
            style(SUBTEXT),
        )];
        if self.events.filtering || !self.events.filter.is_empty() {
            let text = if self.events.filtering {
                self.events.cursor.display(&self.events.filter, 60).0
            } else {
                self.events.filter.clone()
            };
            lines.push(Line::styled(format!("Filter: {text}"), style(MAUVE)));
        }
        if let Some(error) = &self.events.error {
            lines.push(Line::styled(error.clone(), style(PEACH)));
        }
        lines
    }

    fn events_body(&self) -> Vec<Line<'static>> {
        let style = |color| Style::default().fg(color);
        if !self.events.loaded {
            return vec![Line::styled(
                "Waiting for the Server's events.",
                style(MUTED),
            )];
        }
        let rows = visible(
            &self.events.events,
            &self.events.filter,
            self.events.component,
        );
        if rows.is_empty() {
            let text = if self.events.events.is_empty() {
                "No events."
            } else {
                "No events match."
            };
            return vec![Line::styled(text, style(MUTED))];
        }
        rows.into_iter()
            .map(|event| Line::styled(event_line(event), style(TEXT)))
            .collect()
    }

    pub(super) fn draw_events(&self, frame: &mut Frame<'_>) {
        let area = popup_area(frame.area());
        let block = Block::bordered()
            .title(TITLE)
            .style(Style::default().bg(BASE).fg(TEXT));
        let inner = block.inner(area);
        let header = self.events_header();
        let header_height = (header.len() as u16).min(inner.height);
        let regions = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(header_height), Constraint::Min(0)])
            .split(inner);
        let lines = self.events_body();
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        let max = paragraph
            .line_count(regions[1].width)
            .saturating_sub(usize::from(regions[1].height));
        let scroll = if self.events.stick {
            max
        } else {
            self.events.scroll.min(max)
        };
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(Paragraph::new(header), regions[0]);
        frame.render_widget(
            paragraph.scroll((scroll.min(u16::MAX as usize) as u16, 0)),
            regions[1],
        );
    }
}

fn events_body_rect(outer: Rect, header_rows: u16) -> Rect {
    let inner = Block::bordered().inner(popup_area(outer));
    let header_height = header_rows.min(inner.height);
    Rect::new(
        inner.x,
        inner.y.saturating_add(header_height),
        inner.width,
        inner.height.saturating_sub(header_height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(component: EventComponent, message: &str) -> Recorded {
        Recorded {
            time_unix_ms: 0,
            component,
            subject: None,
            message: message.into(),
        }
    }

    #[test]
    fn merge_drops_a_buffered_tail_already_in_the_snapshot() {
        let snapshot = vec![
            event(EventComponent::Titles, "a"),
            event(EventComponent::Quota, "b"),
        ];
        let buffered = vec![
            event(EventComponent::Quota, "b"),
            event(EventComponent::Settings, "c"),
        ];
        let merged = merge(&snapshot, &buffered);
        let messages: Vec<_> = merged.iter().map(|event| event.message.as_str()).collect();
        assert_eq!(messages, ["a", "b", "c"]);
    }

    #[test]
    fn word_filter_requires_every_word_in_component_subject_or_message() {
        let event = Recorded {
            time_unix_ms: 1_700_000_000_000,
            component: EventComponent::Titles,
            subject: Some("12".into()),
            message: "call sent".into(),
        };
        assert!(matches_words(&event, "CALL sent"));
        assert!(matches_words(&event, "titles 12"));
        assert!(!matches_words(&event, "call missing"));
        // The raw timestamp is not a word the filter can see.
        assert!(!matches_words(&event, "1700"));
    }
}

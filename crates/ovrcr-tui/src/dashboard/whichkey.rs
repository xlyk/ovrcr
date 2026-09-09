use super::hints::{HintAction, HintGroup, KeyHint, key_hints};
use super::input::is_browse_key;
use super::render::{CRUST, MAUVE, MUTED, TEXT};
use super::{Dashboard, DashboardAction, InputMode};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

pub(super) struct WhichKey {
    pub pending_leader: bool,
    pub browsing: Option<usize>,
}

struct PopupRow {
    index: Option<usize>,
    area: Rect,
    text: String,
}

struct PopupLayout {
    area: Rect,
    rows: Vec<PopupRow>,
    detail: Rect,
}

fn paragraph(text: &str) -> Paragraph<'_> {
    Paragraph::new(text).wrap(Wrap { trim: false })
}

fn row_text(hint: &KeyHint, narrow: bool) -> String {
    if narrow {
        format!("{}  {}", hint.key, hint.name)
    } else {
        format!("{}  {} — {}", hint.key, hint.name, hint.description)
    }
}

// Rendering and mouse hit testing use exactly the same wrapped row geometry.
fn popup_layout(outer: Rect, groups: &[HintGroup], selected: usize) -> PopupLayout {
    let area = Rect::new(
        outer.x + outer.width.min(4) / 2,
        outer.y + outer.height.min(2) / 2,
        outer.width.saturating_sub(4),
        outer.height.saturating_sub(2),
    );
    let inner = Block::bordered().inner(area);
    let narrow = area.width < 100;
    let columns = !groups.is_empty() && usize::from(inner.width) >= groups.len() * 60;
    let hints: Vec<_> = groups.iter().flat_map(|g| &g.hints).collect();
    let description = hints.get(selected).map_or("", |h| h.description.as_str());
    let detail_height = (paragraph(description).line_count(inner.width) as u16)
        .max(1)
        .min(inner.height / 2);
    let detail = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(detail_height),
        inner.width,
        detail_height,
    );
    let body_height = inner.height.saturating_sub(detail_height).saturating_sub(1);
    let column_width = if columns {
        inner.width / groups.len() as u16
    } else {
        inner.width
    };
    let mut logical: Vec<Vec<(Option<usize>, String, u16)>> =
        vec![Vec::new(); if columns { groups.len() } else { 1 }];
    let mut index = 0;
    for (group_index, group) in groups.iter().enumerate() {
        let column = if columns { group_index } else { 0 };
        logical[column].push((None, group.title.into(), 1));
        for hint in &group.hints {
            let text = row_text(hint, narrow);
            let height = paragraph(&text)
                .line_count(column_width)
                .max(1)
                .min(u16::MAX as usize) as u16;
            logical[column].push((Some(index), text, height));
            index += 1;
        }
    }
    let mut rows = Vec::new();
    for (column, entries) in logical.into_iter().enumerate() {
        let mut end = 0usize;
        let mut selected_end = 0usize;
        for (index, _, height) in &entries {
            end += usize::from(*height);
            if *index == Some(selected) {
                selected_end = end;
            }
        }
        let start = selected_end.saturating_sub(usize::from(body_height));
        let mut offset = 0usize;
        for (index, text, height) in entries {
            let row_start = offset;
            offset += usize::from(height);
            if row_start < start || row_start >= start + usize::from(body_height) {
                continue;
            }
            rows.push(PopupRow {
                index,
                text,
                area: Rect::new(
                    inner.x + column as u16 * column_width,
                    inner.y + (row_start - start) as u16,
                    column_width,
                    height.min(body_height.saturating_sub((row_start - start) as u16)),
                ),
            });
        }
    }
    PopupLayout { area, rows, detail }
}

impl Dashboard {
    pub(super) fn whichkey_key(&mut self, key: KeyEvent) -> Option<DashboardAction> {
        if self.whichkey.is_none() {
            if self.mode == InputMode::Terminal
                || key.kind != KeyEventKind::Press
                || key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
                || !matches!(key.code, KeyCode::Char(' ' | '?'))
            {
                return None;
            }
            self.whichkey = Some(WhichKey {
                pending_leader: key.code == KeyCode::Char(' '),
                browsing: Some(0),
            });
            return Some(DashboardAction::Redraw);
        }
        if key.kind == KeyEventKind::Release {
            return Some(DashboardAction::None);
        }
        if key.code == KeyCode::Esc || (is_browse_key(key) && self.mode == InputMode::Browse) {
            self.whichkey = None;
            return Some(DashboardAction::Redraw);
        }
        let groups = key_hints(self);
        let hints: Vec<_> = groups.iter().flat_map(|g| &g.hints).collect();
        let popup = self.whichkey.as_mut().unwrap();
        if !popup.pending_leader
            && matches!(
                key.code,
                KeyCode::Down | KeyCode::Up | KeyCode::Left | KeyCode::Right
            )
        {
            let index = popup.browsing.unwrap_or(0);
            popup.browsing = Some(if matches!(key.code, KeyCode::Up | KeyCode::Left) {
                index.saturating_sub(1)
            } else {
                (index + 1).min(hints.len().saturating_sub(1))
            });
            return Some(DashboardAction::Redraw);
        }
        if key.kind != KeyEventKind::Press {
            return Some(DashboardAction::None);
        }
        let chosen = if !popup.pending_leader && key.code == KeyCode::Enter {
            hints.get(popup.browsing.unwrap_or(0)).copied()
        } else {
            hints.iter().copied().find(|h| h.matches(key))
        };
        let action = chosen.filter(|h| h.enabled).map(|h| h.action);
        if popup.pending_leader || action.is_some() {
            self.whichkey = None;
        }
        Some(action.map_or(DashboardAction::Redraw, |action| self.run_hint(action)))
    }

    pub(super) fn run_hint(&mut self, action: HintAction) -> DashboardAction {
        self.key_action(action.event())
    }

    pub(super) fn whichkey_mouse(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        let groups = key_hints(self);
        let hints: Vec<_> = groups.iter().flat_map(|g| &g.hints).collect();
        let selected = self.whichkey.as_ref().and_then(|p| p.browsing).unwrap_or(0);
        let layout = popup_layout(area, &groups, selected);
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
        ) {
            let popup = self.whichkey.as_mut().unwrap();
            popup.pending_leader = false;
            popup.browsing = Some(if mouse.kind == MouseEventKind::ScrollUp {
                selected.saturating_sub(1)
            } else {
                (selected + 1).min(hints.len().saturating_sub(1))
            });
            return DashboardAction::Redraw;
        }
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return DashboardAction::None;
        }
        if let Some(index) = layout
            .rows
            .iter()
            .find(|r| r.area.contains(Position::new(mouse.column, mouse.row)))
            .and_then(|r| r.index)
        {
            let hint = hints[index];
            if hint.enabled {
                self.whichkey = None;
                return self.run_hint(hint.action);
            }
            let popup = self.whichkey.as_mut().unwrap();
            popup.pending_leader = false;
            popup.browsing = Some(index);
        }
        DashboardAction::Redraw
    }

    pub(super) fn draw_whichkey(&self, frame: &mut Frame<'_>) {
        let Some(popup) = &self.whichkey else {
            return;
        };
        let groups = key_hints(self);
        let hints: Vec<_> = groups.iter().flat_map(|g| &g.hints).collect();
        let selected = popup
            .browsing
            .unwrap_or(0)
            .min(hints.len().saturating_sub(1));
        let layout = popup_layout(frame.area(), &groups, selected);
        frame.render_widget(Clear, layout.area);
        frame.render_widget(
            Block::bordered()
                .title(if popup.pending_leader {
                    " Which key · next key runs · Esc closes "
                } else {
                    " Which key · arrows browse · Enter/click runs · Esc closes "
                })
                .style(Style::default().bg(CRUST).fg(TEXT))
                .border_style(Style::default().fg(MAUVE)),
            layout.area,
        );
        for row in layout.rows {
            if row.index == Some(selected) && !row.area.is_empty() {
                frame.set_cursor_position((row.area.x, row.area.y));
            }
            let style = match row.index {
                None => Style::default().fg(MAUVE).add_modifier(Modifier::BOLD),
                Some(index) => {
                    let mut style =
                        Style::default().fg(if hints[index].enabled { TEXT } else { MUTED });
                    if !hints[index].enabled {
                        style = style.add_modifier(Modifier::DIM);
                    }
                    if index == selected {
                        style = style.bg(MAUVE).fg(CRUST);
                    }
                    style
                }
            };
            frame.render_widget(paragraph(&row.text).style(style), row.area);
        }
        if let Some(hint) = hints.get(selected) {
            frame.render_widget(
                paragraph(&hint.description).style(Style::default().fg(if hint.enabled {
                    TEXT
                } else {
                    MUTED
                })),
                layout.detail,
            );
        }
    }
}

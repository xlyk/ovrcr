use super::input::is_browse_key;
use super::keymap::{Action, KeyBinding, KeyGroup, key_binding, keymap};
use super::render::{CRUST, MAUVE, MUTED, SKY, TEXT};
use super::{Dashboard, DashboardAction, InputMode};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

pub(super) struct WhichKey {
    pub pending_leader: bool,
    pub browsing: Option<usize>,
    pub group: Option<char>,
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

fn navigation_text(area: Rect) -> &'static str {
    if area.width < 18 {
        " [X] [<] "
    } else {
        " [Close] [Back] "
    }
}

fn paragraph(text: &str) -> Paragraph<'_> {
    Paragraph::new(text).wrap(Wrap { trim: false })
}

fn row_text(hint: &KeyBinding) -> String {
    format!("{}  {}", hint.key, hint.name)
}

// Rendering and mouse hit testing share the compact, wrapped row geometry.
fn popup_layout(outer: Rect, groups: &[KeyGroup], selected: usize) -> PopupLayout {
    let hints: Vec<_> = groups.iter().flat_map(|g| &g.keys).collect();
    let width = hints
        .iter()
        .map(|hint| Line::from(row_text(hint)).width())
        .max()
        .unwrap_or(0)
        .clamp(36, 46) as u16
        + 2;
    let width = width.min(outer.width.saturating_sub(4));
    let inner_width = width.saturating_sub(2);
    let max_height = outer.height.saturating_sub(4).min(30);
    let description = hints.get(selected).map_or("", |h| h.description.as_str());
    let detail_height = (paragraph(description).line_count(inner_width) as u16)
        .max(1)
        .min(max_height.saturating_sub(2) / 2)
        .min(max_height.saturating_sub(4));
    let mut entries = Vec::new();
    let mut index = 0;
    for group in groups {
        entries.push((
            None,
            group.title.to_owned(),
            paragraph(&group.title).line_count(inner_width).max(1) as u16,
        ));
        for hint in &group.keys {
            let text = row_text(hint);
            let height = paragraph(&text).line_count(inner_width).max(1) as u16;
            entries.push((Some(index), text, height));
            index += 1;
        }
    }
    let content_height: u16 = entries.iter().map(|(_, _, height)| height).sum();
    let height = content_height
        .saturating_add(detail_height)
        .saturating_add(3)
        .min(max_height);
    let area = Rect::new(
        outer
            .right()
            .saturating_sub(2)
            .saturating_sub(width)
            .max(outer.x),
        outer
            .bottom()
            .saturating_sub(2)
            .saturating_sub(height)
            .max(outer.y),
        width,
        height,
    );
    let inner = Block::bordered().inner(area);
    let detail = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(detail_height),
        inner.width,
        detail_height,
    );
    let body_height = inner.height.saturating_sub(detail_height).saturating_sub(1);
    let mut end = 0u16;
    let mut start = 0u16;
    for (index, _, height) in &entries {
        if *index == Some(selected) {
            // Keep the start of even a taller-than-viewport row visible.
            start = end
                .saturating_add(*height)
                .saturating_sub(body_height)
                .min(end);
        }
        end = end.saturating_add(*height);
    }
    let mut offset = 0u16;
    let mut rows = Vec::new();
    for (index, text, height) in entries {
        let row_start = offset;
        offset = offset.saturating_add(height);
        if row_start < start || row_start >= start.saturating_add(body_height) {
            continue;
        }
        rows.push(PopupRow {
            index,
            text,
            area: Rect::new(
                inner.x,
                inner.y + row_start - start,
                inner.width,
                height.min(body_height.saturating_sub(row_start - start)),
            ),
        });
    }
    PopupLayout { area, rows, detail }
}

impl Dashboard {
    /// The popup's rows: every binding the table places in the open group, under
    /// the key that group gives it, plus the group list at the top level.
    fn whichkey_hints(&self) -> Vec<KeyGroup> {
        let groups = keymap(self);
        if self.mode != InputMode::Browse {
            return groups;
        }
        let (project, workspace) = self.creation_context();
        let session = self
            .action_session()
            .and_then(|id| super::state::find_session(self, id));
        let group = self.whichkey.as_ref().and_then(|popup| popup.group);
        let title = match group {
            Some('t') => session.map(|s| format!("Terminal: {} (#{})", s.display_name(), s.id.0)),
            Some('w') if !workspace.is_empty() => {
                Some(format!("Workspace: {project} / {workspace}"))
            }
            Some('p') if !project.is_empty() => Some(format!("Project: {project}")),
            Some('v') => Some("View".into()),
            _ => None,
        };
        let mut hints = Vec::new();
        if let Some(title) = title {
            let group = group.expect("a titled popup names its group");
            for binding in groups.into_iter().flat_map(|g| g.keys) {
                let Some(slot) = binding.group.filter(|slot| slot.group == group) else {
                    continue;
                };
                if !binding.shown_in_group() {
                    continue;
                }
                hints.push(binding.with_group_key(slot.key));
            }
            let removal = match group {
                'w' => Some((
                    "Remove workspace",
                    Action::RemoveWorkspace,
                    format!(
                        "Remove {project} / {workspace}; asks for confirmation. Requires no terminals and a clean worktree; keeps the branch."
                    ),
                )),
                'p' => Some((
                    "Remove project",
                    Action::RemoveProject,
                    format!("Unregister {project}; asks for confirmation. Keeps the repository."),
                )),
                _ => None,
            };
            if let Some((name, action, description)) = removal {
                hints.push(key_binding(
                    "x",
                    name,
                    description,
                    KeyCode::Char('x'),
                    action,
                ));
            }
            return vec![KeyGroup { title, keys: hints }];
        }
        for (key, name, target, available) in [
            (
                "t",
                "Terminal",
                session
                    .map(|s| s.display_name().to_string())
                    .unwrap_or_default(),
                session.is_some(),
            ),
            (
                "w",
                "Workspace",
                format!("{project} / {workspace}"),
                !workspace.is_empty(),
            ),
            ("p", "Project", project.clone(), !project.is_empty()),
            ("v", "View", "dashboard".into(), true),
        ] {
            if available {
                let group = key.chars().next().unwrap();
                hints.push(key_binding(
                    key,
                    name,
                    format!("Show actions for {target}"),
                    KeyCode::Char(group),
                    Action::Group(group),
                ));
            }
        }
        // Registration remains reachable even before a project exists.
        hints.extend(groups.into_iter().flat_map(|g| g.keys).filter(|binding| {
            matches!(
                binding.action,
                Action::RegisterProject
                    | Action::Detach
                    | Action::ToggleNotifications
                    | Action::ToggleSound
            )
        }));
        vec![KeyGroup {
            title: "Groups".into(),
            keys: hints,
        }]
    }

    /// Open the popup's group `group`, leaving the popup itself open.
    pub(super) fn open_group(&mut self, group: char) -> DashboardAction {
        if let Some(popup) = &mut self.whichkey {
            popup.group = Some(group);
            popup.browsing = Some(0);
        }
        DashboardAction::Redraw
    }

    /// Open the popup: `leader` waits for the next key, otherwise it starts browsable.
    pub(super) fn open_whichkey(&mut self, leader: bool) -> DashboardAction {
        self.cancel_mouse_gesture();
        self.whichkey = Some(WhichKey {
            pending_leader: leader,
            browsing: Some(0),
            group: None,
        });
        DashboardAction::Redraw
    }

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
            return Some(self.open_whichkey(key.code == KeyCode::Char(' ')));
        }
        if key.kind == KeyEventKind::Release {
            return Some(DashboardAction::None);
        }
        if key.code == KeyCode::Esc || (is_browse_key(key) && self.mode == InputMode::Browse) {
            self.whichkey = None;
            return Some(DashboardAction::Redraw);
        }
        if key.code == KeyCode::Backspace && key.kind == KeyEventKind::Press {
            let popup = self.whichkey.as_mut().unwrap();
            popup.group = None;
            popup.browsing = Some(0);
            return Some(DashboardAction::Redraw);
        }
        let groups = self.whichkey_hints();
        let hints: Vec<_> = groups.iter().flat_map(|g| &g.keys).collect();
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
        let action = chosen.filter(|h| h.enabled()).map(|h| h.action);
        Some(action.map_or(DashboardAction::Redraw, |action| self.run(action)))
    }

    pub(super) fn whichkey_mouse(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        let groups = self.whichkey_hints();
        let hints: Vec<_> = groups.iter().flat_map(|g| &g.keys).collect();
        let selected = self.whichkey.as_ref().and_then(|p| p.browsing).unwrap_or(0);
        let layout = popup_layout(area, &groups, selected);
        if layout.area.contains(Position::new(mouse.column, mouse.row))
            && matches!(
                mouse.kind,
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
            )
        {
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
        let point = Position::new(mouse.column, mouse.row);
        if mouse.row == layout.area.bottom().saturating_sub(1) && layout.area.contains(point) {
            let x = mouse.column.saturating_sub(layout.area.x + 1);
            let text = navigation_text(layout.area);
            let close = text.find('[').unwrap() as u16;
            let close_end = text.find(']').unwrap() as u16 + 1;
            let back = text.rfind('[').unwrap() as u16;
            let back_end = text.rfind(']').unwrap() as u16 + 1;
            let key = if (close..close_end).contains(&x) {
                Some(KeyCode::Esc)
            } else if (back..back_end).contains(&x) {
                Some(KeyCode::Backspace)
            } else {
                None
            };
            if let Some(code) = key {
                return self
                    .whichkey_key(KeyEvent::new(code, KeyModifiers::NONE))
                    .unwrap_or(DashboardAction::Redraw);
            }
        }
        if let Some(index) = layout
            .rows
            .iter()
            .find(|r| r.area.contains(Position::new(mouse.column, mouse.row)))
            .and_then(|r| r.index)
        {
            let hint = hints[index];
            if hint.enabled() {
                return self.run(hint.action);
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
        let groups = self.whichkey_hints();
        let hints: Vec<_> = groups.iter().flat_map(|g| &g.keys).collect();
        let selected = popup
            .browsing
            .unwrap_or(0)
            .min(hints.len().saturating_sub(1));
        let layout = popup_layout(frame.area(), &groups, selected);
        frame.render_widget(Clear, layout.area);
        frame.render_widget(
            Block::bordered()
                .title(match popup.group {
                    Some(group) => format!(" Which key · Space {group} "),
                    None => " Which key · Space ".into(),
                })
                .title_bottom(navigation_text(layout.area))
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
                        Style::default().fg(if hints[index].enabled() { TEXT } else { MUTED });
                    if !hints[index].enabled() {
                        style = style.add_modifier(Modifier::DIM);
                    }
                    if index == selected {
                        style = style.bg(MAUVE).fg(CRUST);
                    }
                    style
                }
            };
            let text = if let Some(index) = row.index {
                let (key, label) = row.text.split_at(hints[index].key.len());
                Line::from(vec![
                    Span::styled(
                        key,
                        if index == selected || !hints[index].enabled() {
                            style
                        } else {
                            style.fg(SKY)
                        },
                    ),
                    Span::raw(label),
                ])
            } else {
                Line::from(row.text)
            };
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: false }).style(style),
                row.area,
            );
        }
        if let Some(hint) = hints.get(selected) {
            frame.render_widget(
                paragraph(&hint.description).style(Style::default().fg(if hint.enabled() {
                    TEXT
                } else {
                    MUTED
                })),
                layout.detail,
            );
        }
    }
}

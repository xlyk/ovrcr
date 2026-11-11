//! The small create menu anchored to a project or workspace row.

use super::keymap::{Action, keymap};
use super::render::{CRUST, MAUVE, MUTED, SURFACE0, TEXT};
use super::{Dashboard, DashboardAction, TreeRow};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};

pub(super) struct SidebarCreateMenu {
    target: TreeRow,
    anchor: Position,
    selected: usize,
}

#[derive(Clone, Copy)]
struct Item {
    action: Action,
    label: &'static str,
    reason: Option<&'static str>,
}

impl Item {
    fn text(self) -> String {
        match self.reason {
            Some(reason) => format!("{} · {reason}", self.label),
            None => self.label.into(),
        }
    }
}

#[derive(Clone, Copy)]
struct EntryRow {
    index: usize,
    area: Rect,
}

struct Layout {
    area: Rect,
    rows: Vec<EntryRow>,
}

fn specs(target: &TreeRow) -> &'static [(Action, &'static str)] {
    const PROJECT: &[(Action, &str)] = &[
        (Action::CreateWorkspace, "New workspace"),
        (Action::NewAgentSession, "New agent session"),
    ];
    const WORKSPACE: &[(Action, &str)] = &[
        (Action::NewAgentSession, "New agent session"),
        (Action::NewTerminal, "New terminal"),
    ];
    match target {
        TreeRow::Project { .. } => PROJECT,
        TreeRow::Workspace { .. } => WORKSPACE,
        _ => &[],
    }
}

fn items(dashboard: &Dashboard, target: &TreeRow) -> Vec<Item> {
    let bindings = keymap(dashboard);
    specs(target)
        .iter()
        .map(|(action, label)| {
            let reason = bindings
                .iter()
                .flat_map(|group| &group.keys)
                .find(|binding| binding.action == *action)
                .map(|binding| binding.reason)
                .unwrap_or(Some("unavailable"));
            Item {
                action: *action,
                label,
                reason,
            }
        })
        .collect()
}

fn layout(outer: Rect, menu: &SidebarCreateMenu, items: &[Item]) -> Layout {
    if outer.is_empty() {
        return Layout {
            area: Rect::default(),
            rows: Vec::new(),
        };
    }
    let content_width = items
        .iter()
        .map(|item| Line::from(item.text()).width())
        .max()
        .unwrap_or(0);
    let width = (content_width as u16).saturating_add(2).min(outer.width);
    let height = (items.len() as u16).saturating_add(2).min(outer.height);
    let max_x = outer.right().saturating_sub(width).max(outer.x);
    let x = menu.anchor.x.min(max_x).max(outer.x);
    let below = menu.anchor.y.saturating_add(1);
    let y = if below.saturating_add(height) <= outer.bottom() {
        below
    } else {
        menu.anchor.y.saturating_sub(height).max(outer.y)
    };
    let area = Rect::new(x, y, width, height);
    let inner = Block::bordered().inner(area);
    let body_height = usize::from(inner.height);
    let start = menu.selected.saturating_add(1).saturating_sub(body_height);
    let rows = (start..items.len().min(start.saturating_add(body_height)))
        .enumerate()
        .map(|(offset, index)| EntryRow {
            index,
            area: Rect::new(
                inner.x,
                inner.y.saturating_add(offset as u16),
                inner.width,
                1,
            ),
        })
        .collect();
    Layout { area, rows }
}

impl Dashboard {
    pub(super) fn open_sidebar_create_menu(
        &mut self,
        target: TreeRow,
        anchor: Position,
    ) -> DashboardAction {
        self.cancel_mouse_gesture();
        self.whichkey = None;
        self.select_container(target.clone());
        self.sidebar_create_menu = Some(SidebarCreateMenu {
            target,
            anchor,
            selected: 0,
        });
        DashboardAction::Redraw
    }

    pub(super) fn sidebar_create_menu_key(&mut self, key: KeyEvent) -> Option<DashboardAction> {
        let menu = self.sidebar_create_menu.as_ref()?;
        if key.kind == KeyEventKind::Release {
            return Some(DashboardAction::None);
        }
        let choices = items(self, &menu.target);
        match key.code {
            KeyCode::Esc => {
                self.sidebar_create_menu = None;
                Some(DashboardAction::Redraw)
            }
            KeyCode::Up | KeyCode::Down => {
                let menu = self.sidebar_create_menu.as_mut().unwrap();
                menu.selected = if key.code == KeyCode::Up {
                    menu.selected.saturating_sub(1)
                } else {
                    (menu.selected + 1).min(choices.len().saturating_sub(1))
                };
                Some(DashboardAction::Redraw)
            }
            KeyCode::Enter => {
                let item = choices.get(menu.selected).copied();
                if let Some(item) = item.filter(|item| item.reason.is_none()) {
                    self.sidebar_create_menu = None;
                    Some(self.run(item.action))
                } else {
                    Some(DashboardAction::None)
                }
            }
            _ => Some(DashboardAction::None),
        }
    }

    pub(super) fn sidebar_create_menu_mouse(
        &mut self,
        mouse: MouseEvent,
        outer: Rect,
    ) -> DashboardAction {
        let Some(menu) = self.sidebar_create_menu.as_ref() else {
            return DashboardAction::None;
        };
        let choices = items(self, &menu.target);
        let popup = layout(outer, menu, &choices);
        let point = Position::new(mouse.column, mouse.row);
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
        ) && popup.area.contains(point)
        {
            let menu = self.sidebar_create_menu.as_mut().unwrap();
            menu.selected = if mouse.kind == MouseEventKind::ScrollUp {
                menu.selected.saturating_sub(1)
            } else {
                (menu.selected + 1).min(choices.len().saturating_sub(1))
            };
            return DashboardAction::Redraw;
        }
        if mouse.kind == MouseEventKind::Moved {
            if let Some(index) = popup
                .rows
                .iter()
                .find(|row| row.area.contains(point))
                .map(|row| row.index)
                && self.sidebar_create_menu.as_ref().unwrap().selected != index
            {
                self.sidebar_create_menu.as_mut().unwrap().selected = index;
                return DashboardAction::Redraw;
            }
            return DashboardAction::None;
        }
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return DashboardAction::None;
        }
        if let Some(index) = popup
            .rows
            .iter()
            .find(|row| row.area.contains(point))
            .map(|row| row.index)
        {
            let item = choices[index];
            if item.reason.is_none() {
                self.sidebar_create_menu = None;
                return self.run(item.action);
            }
            self.sidebar_create_menu.as_mut().unwrap().selected = index;
            return DashboardAction::None;
        }
        if popup.area.contains(point) {
            return DashboardAction::None;
        }
        self.sidebar_create_menu = None;
        DashboardAction::Redraw
    }

    pub(super) fn draw_sidebar_create_menu(&self, frame: &mut Frame<'_>) {
        let Some(menu) = &self.sidebar_create_menu else {
            return;
        };
        let choices = items(self, &menu.target);
        let popup = layout(frame.area(), menu, &choices);
        if popup.area.is_empty() {
            return;
        }
        frame.render_widget(Clear, popup.area);
        frame.render_widget(
            Block::bordered()
                .title(" Create ")
                .style(Style::default().bg(CRUST()).fg(TEXT()))
                .border_style(Style::default().fg(MAUVE())),
            popup.area,
        );
        for row in popup.rows {
            let item = choices[row.index];
            let selected = row.index == menu.selected;
            let mut style = if item.reason.is_some() {
                Style::default().fg(MUTED()).add_modifier(Modifier::DIM)
            } else {
                Style::default().fg(TEXT())
            };
            if selected {
                style = if item.reason.is_some() {
                    style.bg(SURFACE0())
                } else {
                    style.bg(MAUVE()).fg(CRUST())
                };
            }
            frame.render_widget(Paragraph::new(item.text()).style(style), row.area);
        }
    }
}

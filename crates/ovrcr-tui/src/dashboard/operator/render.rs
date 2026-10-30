//! The shared sidebar is the sole conversation list and hit-test geometry.
use super::*;
use crate::dashboard::render::{BASE, MAUVE, SURFACE0, SURFACE2, TEXT, clip_text, compose_row};
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
        if self.sidebar_hidden || self.operator.collapsed {
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
        if self.sidebar_hidden {
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
                Style::default().fg(MAUVE()).add_modifier(Modifier::BOLD),
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
        if self.sidebar_hidden {
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
        let close_visible = id.is_some_and(|id| self.operator_row_close(id, rect.width));
        if self.operator.visible {
            self.operator.list_focus = true;
        }
        self.mouse.hovered = None;
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if mouse.row == rect.y => {
                if rect.width >= 3 && mouse.column >= rect.right() - 3 {
                    self.open_operator();
                    self.operator_new();
                } else {
                    self.operator.collapsed = !self.operator.collapsed;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(id) = id {
                    if close_visible && mouse.column >= rect.right().saturating_sub(3) {
                        return Some(self.open_close_operator_for(id));
                    }
                    self.open_operator_conversation(id);
                    self.operator.list_focus = true;
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

//! Shipped skill discovery edits the shared draft; only Send submits work.
use super::*;
use ovrcr_protocol::operator::skills::{BUNDLED_SKILLS, BundledSkill};

#[derive(Default)]
pub(super) struct SkillPicker {
    pub query: String,
    pub cursor: TextCursor,
    pub selected: usize,
}

impl SkillPicker {
    fn choices(&self) -> Vec<&'static BundledSkill> {
        let query = self.query.trim().to_lowercase();
        BUNDLED_SKILLS
            .iter()
            .filter(|skill| {
                skill.description().is_some_and(|purpose| {
                    skill.name.contains(&query) || purpose.to_lowercase().contains(&query)
                })
            })
            .collect()
    }

    // One geometry for drawing, scrolling and mouse hits. Each name/purpose pair
    // occupies two rows; the selected purpose wraps below the list.
    fn layout(&self, outer: Rect) -> (Rect, usize, usize) {
        let width = outer.width.min(96);
        let height = outer.height.min(18);
        let rows = usize::from(height.saturating_sub(8)) / 2;
        let start = self.selected.saturating_add(1).saturating_sub(rows.max(1));
        (
            Rect::new(
                outer.x + (outer.width - width) / 2,
                outer.y + (outer.height - height) / 2,
                width,
                height,
            ),
            start,
            rows,
        )
    }
}

impl Dashboard {
    pub(super) fn operator_run_skills(&mut self, from_quick: bool) -> DashboardAction {
        if from_quick {
            let Some(quick) = &mut self.operator.quick else {
                return DashboardAction::None;
            };
            let text = quick.text.clone();
            let context = quick.context.clone();
            quick.visible = false;
            // Open normally, then restore the draft's capture, never the later selection.
            self.open_operator();
            self.operator.context = context.clone();
            if self.operator.chat.is_some() {
                self.operator_install_skills(text, context);
            } else {
                self.operator.pending_overlay =
                    Some(PendingOperatorOverlay::Skills { text, context });
            }
        } else {
            self.operator
                .set_overlay(OperatorOverlay::Skills(SkillPicker::default()));
        }
        DashboardAction::Redraw
    }

    pub(super) fn operator_install_skills(&mut self, text: String, context: OperatorContext) {
        if let Some(chat) = &mut self.operator.chat {
            chat.draft = text;
            chat.context = context;
            chat.cursor = TextCursor::default();
            if let Some(quick) = &mut self.operator.quick {
                quick.seeded = true;
            }
            self.operator
                .set_overlay(OperatorOverlay::Skills(SkillPicker::default()));
        }
    }

    fn operator_prepare_skill(&mut self, skill: &'static BundledSkill) {
        let Some(chat) = &mut self.operator.chat else {
            return;
        };
        let request = match slash::parse_draft(&chat.draft) {
            // Also consume the partial command when opened from the slash menu.
            slash::ParsedDraft::Slash { args, .. } => args,
            slash::ParsedDraft::Skill { request, .. } => request,
            _ => &chat.draft,
        };
        chat.draft = if request.is_empty() {
            format!("/skill:{}", skill.name)
        } else {
            format!("/skill:{} {request}", skill.name)
        };
        chat.cursor = TextCursor::default();
        chat.notice = Some("Skill prepared; Send to invoke.".into());
        self.operator.clear_overlays();
        self.operator.slash_dismissed_for = None;
    }

    pub(super) fn operator_skills_key(&mut self, key: KeyEvent) -> DashboardAction {
        let Some(OperatorOverlay::Skills(picker)) = &mut self.operator.overlay else {
            return DashboardAction::None;
        };
        match key.code {
            KeyCode::Esc => self.operator.clear_overlays(),
            KeyCode::Up if key.modifiers.is_empty() => {
                picker.selected = picker.selected.saturating_sub(1)
            }
            KeyCode::Down if key.modifiers.is_empty() => {
                picker.selected =
                    (picker.selected + 1).min(picker.choices().len().saturating_sub(1))
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                if let Some(skill) = picker.choices().get(picker.selected) {
                    self.operator_prepare_skill(skill);
                }
            }
            _ => {
                if edit_modal_line(
                    &mut picker.query,
                    &mut picker.cursor,
                    key,
                    MAX_MODEL_FILTER_BYTES,
                ) {
                    picker.selected = 0;
                }
            }
        }
        DashboardAction::Redraw
    }

    pub(super) fn operator_skills_mouse(
        &mut self,
        mouse: MouseEvent,
        outer: Rect,
    ) -> DashboardAction {
        let Some(OperatorOverlay::Skills(picker)) = &mut self.operator.overlay else {
            return DashboardAction::None;
        };
        let (area, start, rows) = picker.layout(outer);
        let point = ratatui::layout::Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if !area.contains(point) => {
                self.operator.clear_overlays()
            }
            MouseEventKind::Down(MouseButton::Left)
                if outer.width >= 60
                    && outer.height >= 16
                    && mouse.row >= area.y + 2
                    && mouse.row < area.y + 2 + rows as u16 * 2 =>
            {
                let index = start + usize::from(mouse.row - area.y - 2) / 2;
                if let Some(skill) = picker.choices().get(index) {
                    self.operator_prepare_skill(skill);
                }
            }
            MouseEventKind::ScrollUp => picker.selected = picker.selected.saturating_sub(1),
            MouseEventKind::ScrollDown => {
                picker.selected =
                    (picker.selected + 1).min(picker.choices().len().saturating_sub(1))
            }
            _ => {}
        }
        DashboardAction::Redraw
    }

    pub(super) fn draw_operator_skills(&self, frame: &mut Frame<'_>) {
        let Some(OperatorOverlay::Skills(picker)) = &self.operator.overlay else {
            return;
        };
        let (area, start, rows) = picker.layout(frame.area());
        let width = area.width.saturating_sub(2);
        let choices = picker.choices();
        let mut lines = vec![Line::raw(format!(
            "Filter: {}",
            display_text(&picker.query)
        ))];
        if choices.is_empty() {
            lines.push(Line::raw("No shipped skills match."));
        }
        for (index, skill) in choices.iter().enumerate().skip(start).take(rows) {
            let style = if index == picker.selected {
                Style::default().bg(super::super::render::SURFACE2())
            } else {
                Style::default()
            };
            lines.push(Line::styled(
                super::super::render::clip_text(skill.name, width as usize),
                style,
            ));
            lines.push(Line::styled(
                super::super::render::clip_text(skill.description().unwrap(), width as usize),
                style,
            ));
        }
        if let Some(skill) = choices.get(picker.selected) {
            lines.extend(
                render::wrapped_lines(vec![Line::raw(skill.description().unwrap())], width)
                    .into_iter()
                    .take(3),
            );
        }
        for line in super::super::keymap::operator_overlay_footer_lines(self, width as usize) {
            lines.push(Line::raw(line));
        }
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Operator skills ")
                    .style(
                        Style::default()
                            .fg(super::super::render::TEXT())
                            .bg(super::super::render::BASE()),
                    ),
            ),
            area,
        );
        let pos = picker.cursor.position(&picker.query);
        let column =
            (8 + display_text(&picker.query[..pos]).width()).min(width.saturating_sub(1) as usize);
        frame.set_cursor_position((area.x + 1 + column as u16, area.y + 1));
    }
}

use super::copy::{CopyPoint, CopySelection};
use super::state::{
    find_session, find_workspace, history_page_covers, workspace_disambiguator, workspace_heading,
};
use super::status::SessionStatus;
use super::{Dashboard, HistoryView, InputMode, PaneRects, PaneState, TreeRow, history_view_size};
use crate::session::{SessionPhase, TerminalSize};
use crate::task_tui::draw_tasks;
use ovrcr_protocol::{CostKind, HistoryColor, SessionSummary, UsageCoverage, UsageScope};
use ovrcr_terminal::vt100;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn action_controls(area: Rect) -> (Rect, Rect) {
    let title = dashboard_layout(area, None).title;
    let x = title.right().saturating_sub(16).max(title.x);
    let actions = Rect::new(
        x,
        title.y,
        title.right().saturating_sub(x).min(9),
        title.height,
    );
    let menu_x = x.saturating_add(10).min(title.right());
    let menu = Rect::new(
        menu_x,
        title.y,
        title.right().saturating_sub(menu_x).min(6),
        title.height,
    );
    (actions, menu)
}

pub(super) fn capture_controls(area: Rect) -> (Rect, Rect) {
    let title = dashboard_layout(area, None).title;
    let x = title.right().saturating_sub(16).max(title.x);
    let copy = if title.width >= 14 {
        Rect::new(x, title.y, 6, title.height)
    } else {
        Rect::default()
    };
    let close = if title.width >= 7 {
        Rect::new(title.right() - 7, title.y, 7, title.height)
    } else {
        Rect::default()
    };
    (copy, close)
}

pub(super) fn history_content_rect(area: Rect) -> Rect {
    Rect::new(area.x, area.y, area.width.min(256), area.height.min(64))
}

#[derive(Clone, Copy)]
struct DashboardLayout {
    title: Rect,
    footer: Rect,
    sidebar: Rect,
    sidebar_content: Rect,
    terminal: Rect,
}

pub(super) const METADATA_HEIGHT: u16 = 2;
pub(super) const BASE: Color = Color::Rgb(30, 30, 46);
pub(super) const CRUST: Color = Color::Rgb(17, 17, 27);
pub(super) const TEXT: Color = Color::Rgb(205, 214, 244);
pub(super) const SUBTEXT: Color = Color::Rgb(166, 173, 200);
pub(super) const MUTED: Color = Color::Rgb(108, 112, 134);
pub(super) const MAUVE: Color = Color::Rgb(203, 166, 247);
pub(super) const PEACH: Color = Color::Rgb(250, 179, 135);
pub(super) const GREEN: Color = Color::Rgb(166, 227, 161);
pub(super) const TEAL: Color = Color::Rgb(148, 226, 213);
pub(super) const BLUE: Color = Color::Rgb(137, 180, 250);
pub(super) const SKY: Color = Color::Rgb(137, 220, 235);
pub(super) const YELLOW: Color = Color::Rgb(249, 226, 175);
pub(super) const RED: Color = Color::Rgb(243, 139, 168);
pub(super) const SURFACE0: Color = Color::Rgb(49, 50, 68);
pub(super) const SURFACE2: Color = Color::Rgb(88, 91, 112);

pub fn render_terminal(frame: &mut Frame<'_>, area: Rect, screen: &vt100::Screen, focused: bool) {
    let (rows, cols) = screen.size();
    let rows = rows.min(area.height);
    let cols = cols.min(area.width);
    let buffer = frame.buffer_mut();
    for row in 0..rows {
        for col in 0..cols {
            let x = area.x + col;
            let y = area.y + row;
            let cell = buffer.cell_mut((x, y)).expect("terminal area is in frame");
            cell.reset();
            let Some(vt_cell) = screen.cell(row, col) else {
                continue;
            };
            if vt_cell.is_wide_continuation() {
                continue;
            }
            let (fg, bg) = (
                color(vt_cell.fgcolor(), TEXT),
                color(vt_cell.bgcolor(), BASE),
            );
            let contents = vt_cell.contents();
            let symbol =
                if (vt_cell.is_wide() && col.saturating_add(1) >= cols) || contents.is_empty() {
                    " "
                } else {
                    contents
                };
            cell.set_symbol(symbol).set_fg(fg).set_bg(bg);
            let mut modifier = Modifier::empty();
            if vt_cell.bold() {
                modifier.insert(Modifier::BOLD);
            }
            if vt_cell.dim() {
                modifier.insert(Modifier::DIM);
            }
            if vt_cell.italic() {
                modifier.insert(Modifier::ITALIC);
            }
            if vt_cell.underline() {
                modifier.insert(Modifier::UNDERLINED);
            }
            if vt_cell.inverse() {
                modifier.insert(Modifier::REVERSED);
            }
            cell.modifier = modifier;
        }
    }
    if focused && !screen.hide_cursor() {
        let (row, col) = screen.cursor_position();
        if row < rows && col < cols {
            frame.set_cursor_position((area.x + col, area.y + row));
        }
    }
}

fn pane_is_running(dashboard: &Dashboard, pane: &PaneState) -> bool {
    pane.session
        .and_then(|session| find_session(dashboard, session))
        .is_some_and(|session| matches!(session.phase, SessionPhase::Running))
}

pub fn render_copy(frame: &mut Frame<'_>, area: Rect, selection: &CopySelection) {
    render_terminal(frame, area, &selection.screen, false);
    for row in 0..selection.screen.size().0.min(area.height) {
        for col in 0..selection.screen.size().1.min(area.width) {
            if selection.contains(CopyPoint { row, col }) {
                let cell = &mut frame.buffer_mut()[(area.x + col, area.y + row)];
                cell.set_bg(TEAL).set_fg(BASE);
            }
        }
    }
    if selection.cursor.row < area.height && selection.cursor.col < area.width {
        frame.set_cursor_position((area.x + selection.cursor.col, area.y + selection.cursor.row));
    }
}

pub fn render_history(frame: &mut Frame<'_>, area: Rect, view: &HistoryView) {
    let bounded = history_content_rect(area);
    for row in 0..area.height {
        for col in 0..area.width {
            let cell = frame
                .buffer_mut()
                .cell_mut((area.x + col, area.y + row))
                .expect("history area is in frame");
            cell.reset();
            cell.set_bg(BASE).set_fg(TEXT).set_symbol(" ");
        }
    }
    for screen_row in 0..bounded.height {
        let absolute_row = view.top.saturating_add(u32::from(screen_row));
        for screen_col in 0..bounded.width {
            let absolute_col = u32::from(view.left).saturating_add(u32::from(screen_col));
            if absolute_col > u32::from(u16::MAX) {
                continue;
            }
            let absolute_col = absolute_col as u16;
            let Some((page_start_col, history_row, history_cell)) =
                history_cell_at(view, absolute_row, absolute_col)
            else {
                continue;
            };
            if absolute_col < page_start_col || absolute_col >= history_row.width {
                continue;
            }
            if history_cell.width == 0 {
                continue;
            }
            if history_cell.width == 2
                && (screen_col.saturating_add(1) >= bounded.width
                    || absolute_col.saturating_add(1) >= history_row.width)
            {
                continue;
            }
            let cell = frame
                .buffer_mut()
                .cell_mut((bounded.x + screen_col, bounded.y + screen_row))
                .expect("history cell is in frame");
            cell.set_symbol(if history_cell.text.is_empty() {
                " "
            } else {
                &history_cell.text
            });
            cell.set_fg(history_color(history_cell.fg.clone(), TEXT));
            cell.set_bg(history_color(history_cell.bg.clone(), BASE));
            let mut modifier = Modifier::empty();
            if history_cell.attributes & 1 != 0 {
                modifier.insert(Modifier::BOLD);
            }
            if history_cell.attributes & 2 != 0 {
                modifier.insert(Modifier::DIM);
            }
            if history_cell.attributes & 4 != 0 {
                modifier.insert(Modifier::ITALIC);
            }
            if history_cell.attributes & 8 != 0 {
                modifier.insert(Modifier::UNDERLINED);
            }
            if history_cell.attributes & 16 != 0 {
                modifier.insert(Modifier::REVERSED);
            }
            cell.modifier = modifier;
            if history_selected(view, absolute_row, absolute_col) {
                cell.set_fg(BASE).set_bg(TEAL);
                if history_cell.width == 2
                    && screen_col.saturating_add(1) < bounded.width
                    && absolute_col.saturating_add(1) < history_row.width
                {
                    frame
                        .buffer_mut()
                        .cell_mut((
                            bounded.x + screen_col.saturating_add(1),
                            bounded.y + screen_row,
                        ))
                        .expect("wide history cell is in frame")
                        .set_fg(BASE)
                        .set_bg(TEAL);
                }
            }
        }
    }
    if let Some(cursor) = view.cursor.filter(|_| view.cursor_target.is_none())
        && cursor.row_width > 0
        && cursor.point.row >= view.top
        && cursor.point.row < view.top.saturating_add(bounded.height as u32)
        && cursor.point.col >= view.left
    {
        let screen_row = cursor.point.row.saturating_sub(view.top) as u16;
        let screen_col = cursor.point.col.saturating_sub(view.left);
        if screen_row < bounded.height
            && screen_col < bounded.width
            && (cursor.cell_width != 2 || screen_col.saturating_add(1) < bounded.width)
            && history_cell_at(view, cursor.point.row, cursor.point.col)
                .is_some_and(|(_, _, cell)| cell.width != 0)
        {
            frame.set_cursor_position((
                bounded.x.saturating_add(screen_col),
                bounded.y.saturating_add(screen_row),
            ));
        }
    }
}

pub(super) fn history_cell_at(
    view: &HistoryView,
    row: u32,
    col: u16,
) -> Option<(
    u16,
    &crate::protocol::HistoryRow,
    &crate::protocol::HistoryCell,
)> {
    view.pages
        .iter()
        .filter_map(|page| {
            let history_row = page
                .rows
                .get(usize::try_from(row.checked_sub(page.start_row)?).ok()?)?;
            let end = page
                .start_col
                .saturating_add(history_row.cells.len() as u16);
            if col < page.start_col || col >= end {
                return None;
            }
            let cell = history_row.cells.get(usize::from(col - page.start_col))?;
            Some((page.start_col, history_row, cell))
        })
        .max_by_key(|(start_col, _, _)| *start_col)
}

fn history_selected(view: &HistoryView, row: u32, col: u16) -> bool {
    let Some(anchor) = view.anchor else {
        return false;
    };
    let Some(cursor) = view.cursor else {
        return false;
    };
    let cursor = cursor.point;
    let (start, end) = if anchor <= cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };
    let point = super::copy::HistoryCopyPoint { row, col };
    start <= point && point <= end
}

fn history_hint(view: &HistoryView, pane_size: TerminalSize) -> String {
    let viewport = history_view_size(pane_size);
    let row_end = view
        .top
        .saturating_add(u32::from(viewport.rows))
        .min(view.opened.total_rows);
    let col_end = u32::from(view.left)
        .saturating_add(u32::from(viewport.cols))
        .min(u32::from(u16::MAX).saturating_add(1));
    let mut missing = false;
    let mut has_content = false;
    let mut row_start = (view.top / 16) * 16;
    while row_start < row_end {
        let mut col_start = (u32::from(view.left) / 128) * 128;
        while col_start < col_end {
            let Some(page_start_col) = u16::try_from(col_start).ok() else {
                break;
            };
            let rows = view.opened.total_rows.saturating_sub(row_start).min(16) as u16;
            let cols =
                u16::try_from(u32::from(u16::MAX).saturating_sub(col_start).min(128)).unwrap_or(0);
            let cached = view.pages.iter().find(|page| {
                history_page_covers(&view.opened, page, row_start, page_start_col, rows, cols)
            });
            let Some(page) = cached else {
                missing = true;
                col_start = col_start.saturating_add(128);
                continue;
            };
            has_content |= page.rows.iter().any(|row| {
                row.cells
                    .iter()
                    .any(|cell| cell.width != 0 && !cell.text.is_empty())
            });
            col_start = col_start.saturating_add(128);
        }
        row_start = row_start.saturating_add(16);
    }
    let status = if missing {
        "loading"
    } else if has_content {
        "loaded"
    } else {
        "empty"
    };
    let activity = if view.copy_job.is_some() || view.copy_completion.is_some() {
        " · copying"
    } else if view.cursor_target.is_some() {
        " · cursor loading"
    } else {
        ""
    };
    let row_end = row_end.max(view.top);
    let col_end = col_end.max(u32::from(view.left));
    let output = if view.new_output {
        "HISTORY · frozen · new output"
    } else {
        "HISTORY · frozen"
    };
    let limit = if pane_size.rows > viewport.rows || pane_size.cols > viewport.cols {
        " · viewport limit"
    } else {
        ""
    };
    format!(
        "{output} · {status}{activity} · row {}-{} · col {}-{}{limit}",
        view.top.saturating_add(1),
        row_end,
        u32::from(view.left).saturating_add(1),
        col_end,
    )
}

fn history_color(value: HistoryColor, default: Color) -> Color {
    match value {
        HistoryColor::Default => default,
        HistoryColor::Indexed(index) => Color::Indexed(index),
        HistoryColor::Rgb(red, green, blue) => Color::Rgb(red, green, blue),
    }
}

fn color(value: vt100::Color, default: Color) -> Color {
    match value {
        vt100::Color::Default => default,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(red, green, blue) => Color::Rgb(red, green, blue),
    }
}

impl Dashboard {
    /// Draw the dashboard and commit its review target only after the backend succeeds.
    pub fn draw<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut ratatui::Terminal<B>,
    ) -> Result<(), B::Error> {
        let area = terminal.draw(|frame| draw_dashboard(frame, self))?.area;
        // Overlays may obscure the response; keep the last unobscured Presented Unread.
        if self.tasks.is_none() && self.palette.is_none() && self.whichkey.is_none() {
            self.unread
                .commit_presented(self.action_session().and_then(|session| {
                    let visible = self.pane_rects(area).iter().any(|rect| {
                        rect.pane_index == self.focused_pane
                            && rect.metadata.width >= 6
                            && rect.metadata.height > 0
                    });
                    visible.then_some(session).and_then(|session| {
                        find_session(self, session)
                            .and_then(|summary| summary.unread.clone())
                            .map(|unread| (session, unread))
                    })
                }));
        }
        Ok(())
    }
}

pub fn draw_dashboard(frame: &mut Frame<'_>, dashboard: &Dashboard) {
    let now_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    draw_dashboard_at(frame, dashboard, now_unix_ms);
}

pub fn draw_dashboard_at(frame: &mut Frame<'_>, dashboard: &Dashboard, now_unix_ms: u64) {
    if let Some(tasks) = &dashboard.tasks {
        draw_tasks(frame, tasks);
        return;
    }
    let layout = dashboard_layout(frame.area(), dashboard.sidebar_preference());
    frame.render_widget(
        Block::default().style(Style::default().bg(BASE)),
        frame.area(),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("󰚩 ", Style::default().fg(CRUST)),
            Span::styled(
                "OVRCR",
                Style::default().fg(CRUST).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  agent runtime",
                Style::default().fg(CRUST).add_modifier(Modifier::DIM),
            ),
        ]))
        .style(Style::default().bg(MAUVE)),
        layout.title,
    );

    if !dashboard.sidebar_hidden {
        frame.render_widget(
            Block::default()
                .borders(Borders::RIGHT)
                .border_style(Style::default().fg(MUTED))
                .style(Style::default().bg(BASE)),
            layout.sidebar,
        );
    }
    let rows = dashboard.visible_rows();
    let width = usize::from(layout.sidebar_content.width);
    let heights = tree_row_heights(dashboard, &rows, width);
    let stacked = heights.contains(&2);
    let viewport_height = if dashboard.sidebar_hidden {
        0
    } else {
        usize::from(layout.sidebar_content.height)
    };
    let start = dashboard.tree_offset.min(tree_line_count(&rows, &heights));
    for screen_line in 0..viewport_height {
        let Some((row, row_line)) =
            tree_line_at(&rows, &heights, start.saturating_add(screen_line))
        else {
            continue;
        };
        let y = layout.sidebar_content.y.saturating_add(screen_line as u16);
        let (line, style) = tree_line_text(dashboard, row, row_line, stacked, width, now_unix_ms);
        let line_area = Rect::new(layout.sidebar_content.x, y, layout.sidebar_content.width, 1);
        frame.render_widget(Paragraph::new(line).style(style), line_area);
    }

    let rects = dashboard.pane_rects(frame.area());
    let split_hidden = dashboard.panes.len() == 2 && rects.len() == 1;
    let split_separator = dashboard.panes.len() == 2 && rects.len() == 2;
    if split_separator {
        let separator_x = rects[0].terminal.right();
        for row in 0..layout.sidebar.height {
            let cell = frame
                .buffer_mut()
                .cell_mut((separator_x, layout.sidebar.y + row))
                .expect("split separator is in frame");
            cell.reset();
            cell.set_symbol("│").set_fg(MUTED).set_bg(BASE);
        }
    }

    if dashboard.panes.len() == 2 {
        for rect in &rects {
            let Some(pane) = dashboard.panes.get(rect.pane_index) else {
                continue;
            };
            render_split_metadata(
                frame,
                *rect,
                pane,
                dashboard,
                rect.pane_index == dashboard.focused_pane,
                now_unix_ms,
            );
            render_terminal(
                frame,
                rect.terminal,
                pane.parser.screen(),
                rect.pane_index == dashboard.focused_pane
                    && dashboard.mode == InputMode::Terminal
                    && dashboard.pane_ready(pane)
                    && pane_is_running(dashboard, pane),
            );
        }
    } else if let Some(rect) = rects.first().copied() {
        let selected = dashboard
            .focused_session()
            .and_then(|id| find_session(dashboard, id));
        let metadata = selected.map_or_else(
            || {
                Line::from(Span::styled(
                    "no session selected",
                    Style::default().fg(MUTED),
                ))
            },
            |session| {
                let status = SessionStatus::of(session, now_unix_ms);
                let width = usize::from(rect.metadata.width);
                if let Some(unread) = &status.unread {
                    return Line::from(Span::styled(
                        clip_text(unread, width),
                        Style::default().fg(TEAL),
                    ));
                }
                if status.ready {
                    let paused = if status.paused { " paused" } else { "" };
                    let mut text = clip_text(
                        &format!("pid: {}{paused}{}", status.pid, status.activity),
                        width,
                    );
                    if !status.elapsed.is_empty() {
                        append_metadata_field(
                            &mut text,
                            &format!("elapsed: {}", status.elapsed),
                            rect.metadata.width,
                        );
                    }
                    append_metadata_field(
                        &mut text,
                        &dashboard.session_display_name(session),
                        rect.metadata.width,
                    );
                    return Line::from(Span::styled(text, Style::default().fg(TEAL)));
                }
                let activity = if status.exited && !status.recovery_diagnostic {
                    Span::raw("")
                } else if !status.live {
                    Span::styled(format!(" {}", status.activity), Style::default().fg(TEAL))
                } else {
                    Span::styled(
                        format!("  agent{}", status.activity),
                        Style::default().fg(TEAL),
                    )
                };
                Line::from(vec![
                    Span::styled("pid: ", Style::default().fg(MUTED)),
                    Span::styled(status.pid, Style::default().fg(TEAL)),
                    Span::styled(
                        if status.elapsed.is_empty() {
                            ""
                        } else {
                            "  elapsed: "
                        },
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(status.elapsed, Style::default().fg(TEAL)),
                    activity,
                    if status.paused {
                        Span::styled("  paused", Style::default().fg(PEACH))
                    } else {
                        Span::raw("")
                    },
                    Span::styled(
                        format!("  {}", dashboard.session_display_name(session)),
                        Style::default().fg(TEXT),
                    ),
                ])
            },
        );
        frame.render_widget(
            Paragraph::new(metadata).style(Style::default().bg(BASE)),
            Rect::new(rect.metadata.x, rect.metadata.y, rect.metadata.width, 1),
        );
        if rect.metadata.height > 1 {
            let metadata_hint = dashboard.history.as_ref().map_or_else(
                || {
                    selected
                        .map(|session| {
                            let width = usize::from(rect.metadata.width);
                            if provider_metrics(session, now_unix_ms, width).is_some() {
                                let title = clipped_session_title(
                                    dashboard,
                                    session,
                                    (width / 3).clamp(1, 24),
                                );
                                let metrics_width =
                                    width.saturating_sub(Line::raw(&title).width() + 3);
                                let metrics = provider_metrics(session, now_unix_ms, metrics_width)
                                    .unwrap_or_default();
                                format!("{title} · {metrics}")
                            } else {
                                clipped_session_title(dashboard, session, width)
                            }
                        })
                        .unwrap_or_else(|| "─".repeat(usize::from(rect.metadata.width)))
                },
                |view| history_hint(view, dashboard.focused_size()),
            );
            frame.render_widget(
                Paragraph::new(metadata_hint).style(Style::default().fg(MUTED).bg(BASE)),
                Rect::new(
                    rect.metadata.x,
                    rect.metadata.y.saturating_add(1),
                    rect.metadata.width,
                    1,
                ),
            );
        }
        if let Some(pane) = dashboard.focused_pane() {
            render_terminal(
                frame,
                rect.terminal,
                pane.parser.screen(),
                dashboard.mode == InputMode::Terminal
                    && dashboard.pane_ready(pane)
                    && pane_is_running(dashboard, pane),
            );
        }
    }

    if let Some(copy) = dashboard.copy.as_ref() {
        if let Some(rect) = rects
            .iter()
            .find(|rect| rect.pane_index == dashboard.focused_pane)
        {
            render_copy(frame, rect.terminal, copy);
        }
    } else if let Some(view) = dashboard
        .history
        .as_ref()
        .filter(|_| dashboard.mode == InputMode::History)
        && let Some(rect) = rects
            .iter()
            .find(|rect| rect.pane_index == dashboard.focused_pane)
    {
        render_history(frame, rect.terminal, view);
    }
    let footer = dashboard.error.as_deref().map_or_else(
        || {
            if let Some(notice) = dashboard
                .copy_notice
                .as_deref()
                .or(dashboard.desktop.notice.as_deref())
            {
                if split_hidden {
                    return Line::from(Span::styled(
                        format!("{notice}  split hidden: terminal too small"),
                        Style::default().fg(TEXT),
                    ));
                }
                return Line::from(Span::styled(notice, Style::default().fg(TEXT)));
            }
            if dashboard.mode != InputMode::Terminal {
                let prefix = if split_hidden { "split hidden  " } else { "" };
                let width = layout.footer.width.saturating_sub(prefix.len() as u16);
                let text = format!("{prefix}{}", super::hints::footer(dashboard, width));
                return Line::from(Span::styled(text, Style::default().fg(TEXT)));
            }
            let mut text = super::hints::footer(dashboard, layout.footer.width);
            if split_hidden {
                text.push_str("  split hidden: terminal too small");
            }
            Line::from(Span::styled(text, Style::default().fg(TEXT)))
        },
        |error| {
            Line::from(vec![
                Span::styled("ERROR: ", Style::default().fg(Color::Rgb(243, 139, 168))),
                Span::styled(error, Style::default().fg(TEXT)),
            ])
        },
    );
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().bg(CRUST)),
        layout.footer,
    );
    if matches!(dashboard.mode, InputMode::Browse | InputMode::Terminal) {
        let (actions, menu) = action_controls(frame.area());
        frame.render_widget(
            Paragraph::new("[Actions]").style(Style::default().bg(CRUST).fg(MAUVE)),
            actions,
        );
        frame.render_widget(
            Paragraph::new("[Menu]").style(Style::default().bg(CRUST).fg(MAUVE)),
            menu,
        );
    }
    if matches!(dashboard.mode, InputMode::Copy | InputMode::History) {
        let (copy, close) = capture_controls(frame.area());
        frame.render_widget(
            Paragraph::new("[Copy]").style(Style::default().bg(CRUST).fg(MAUVE)),
            copy,
        );
        frame.render_widget(
            Paragraph::new("[Close]").style(Style::default().bg(CRUST).fg(MAUVE)),
            close,
        );
    }
    dashboard.draw_start_screen(frame);
    dashboard.draw_palette(frame);
    dashboard.draw_whichkey(frame);
}

impl Dashboard {
    fn draw_start_screen(&self, frame: &mut Frame<'_>) {
        if self.mode != InputMode::Browse || self.focused_session().is_some() {
            return;
        }
        let Some(rect) = self
            .pane_rects(frame.area())
            .into_iter()
            .find(|r| r.pane_index == self.focused_pane)
        else {
            return;
        };
        let groups = super::keymap::keymap(self);
        let hints: Vec<_> = groups.iter().flat_map(|g| &g.keys).collect();
        let mut lines = Vec::new();
        if self.hierarchy.projects.is_empty() {
            lines.push(Line::from("Welcome to OVRCR"));
            lines.push(Line::from(""));
            for key in ["a", ":", "?"] {
                if let Some(hint) = hints.iter().find(|h| h.key == key) {
                    lines.push(Line::from(format!("{}  {}", hint.key, hint.name)));
                    lines.push(Line::from(hint.description.clone()));
                }
            }
            lines.push(Line::from(""));
            let (config, socket) = self
                .configuration_paths
                .as_ref()
                .map(|(c, s)| (c.display().to_string(), s.display().to_string()))
                .unwrap_or_else(|| ("not supplied".into(), "not supplied".into()));
            lines.push(Line::from(format!("Config: {config}")));
            lines.push(Line::from(format!("Socket: {socket}")));
        } else if let Some(super::TreeRow::Workspace { project, id }) = &self.selected_container
            && let Some(workspace) = find_workspace(self, project, id)
        {
            if let Some(warning) = &workspace.warning {
                lines.push(Line::from(workspace_heading(self, workspace)));
                lines.push(Line::from(warning.clone()));
            } else if workspace.sessions.is_empty()
                && let Some(hint) = hints.iter().find(|h| h.key == "n")
            {
                lines.push(Line::from(format!(
                    "{}: press {} to start a terminal here",
                    workspace_heading(self, workspace),
                    hint.key
                )));
                lines.push(Line::from(hint.description.clone()));
            }
        }
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .style(Style::default().fg(TEXT)),
            rect.terminal,
        );
    }
}

fn clipped_session_title(
    dashboard: &Dashboard,
    session: &crate::session::SessionSummary,
    width: usize,
) -> String {
    let display = dashboard.session_display_name(session);
    if display != session.display_name() {
        let suffix = format!(" (#{})", session.id.0);
        if width >= suffix.len() {
            format!(
                "{}{}",
                clip_text(session.display_name(), width - suffix.len()),
                suffix
            )
        } else {
            clip_text(&format!("#{}", session.id.0), width)
        }
    } else {
        clip_text(&display, width)
    }
}

fn render_split_metadata(
    frame: &mut Frame<'_>,
    rect: PaneRects,
    pane: &PaneState,
    dashboard: &Dashboard,
    focused: bool,
    now_unix_ms: u64,
) {
    if rect.metadata.is_empty() {
        return;
    }
    let session = pane.session.and_then(|id| find_session(dashboard, id));
    let name =
        session.map_or_else(
            || "no session".into(),
            |session| {
                clipped_session_title(
                    dashboard,
                    session,
                    usize::from(rect.metadata.width)
                        .saturating_sub(if dashboard.pane_ready(pane) { 2 } else { 10 }),
                )
            },
        );
    let prefix = if focused { "> " } else { "  " };
    let mut text = if dashboard.pane_ready(pane) {
        format!(
            "{prefix}{name} {}x{}",
            rect.terminal.width, rect.terminal.height
        )
    } else {
        format!("{prefix}loading {name}")
    };
    let status = session.map(|session| SessionStatus::of(session, now_unix_ms));
    if let Some(status) = &status
        && (dashboard.pane_ready(pane) || status.ready)
    {
        if status.ready {
            // Reserve process and health status before clipping activity quality.
            // At intermediate widths, identity/geometry yields to these statuses.
            let field = format!("pid: {}{}", status.pid, status.activity);
            let with_identity = format!("{text}  {field}");
            text = if Line::raw(&with_identity).width() <= usize::from(rect.metadata.width) {
                with_identity
            } else {
                clip_text(&field, usize::from(rect.metadata.width))
            };
        } else {
            append_metadata_field(
                &mut text,
                &format!("pid: {}", status.pid),
                rect.metadata.width,
            );
        }
        if !status.elapsed.is_empty() {
            append_metadata_field(
                &mut text,
                &format!("elapsed: {}", status.elapsed),
                rect.metadata.width,
            );
        }
    }
    if let Some(unread) = status.as_ref().and_then(|status| status.unread.as_deref()) {
        // Keep both unread and reporting availability ahead of names, PID and geometry.
        text = clip_text(unread, usize::from(rect.metadata.width));
    }
    let style = if focused {
        Style::default().fg(CRUST).bg(MAUVE)
    } else {
        Style::default().fg(TEXT).bg(BASE)
    };
    frame.render_widget(
        Paragraph::new(clip_text(&text, usize::from(rect.metadata.width))).style(style),
        Rect::new(rect.metadata.x, rect.metadata.y, rect.metadata.width, 1),
    );
    if rect.metadata.height > 1 {
        frame.render_widget(
            Paragraph::new(
                session
                    .and_then(|session| {
                        provider_metrics(session, now_unix_ms, usize::from(rect.metadata.width))
                    })
                    .unwrap_or_else(|| "─".repeat(usize::from(rect.metadata.width))),
            )
            .style(Style::default().fg(MUTED).bg(BASE)),
            Rect::new(
                rect.metadata.x,
                rect.metadata.y.saturating_add(1),
                rect.metadata.width,
                1,
            ),
        );
    }
}

fn append_metadata_field(text: &mut String, field: &str, width: u16) {
    let candidate = format!("{text}  {field}");
    if Line::raw(&candidate).width() <= usize::from(width) {
        *text = candidate;
    }
}

fn dashboard_layout(area: Rect, sidebar_width: Option<u16>) -> DashboardLayout {
    let [title, body, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(area);
    let sidebar_width = super::sidebar_width_for(body, sidebar_width);
    let [sidebar, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(sidebar_width), Constraint::Min(0)])
        .areas(body);
    let metadata_height = right.height.min(METADATA_HEIGHT);
    let [_metadata, terminal] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(metadata_height), Constraint::Min(0)])
        .areas(right);
    DashboardLayout {
        title,
        footer,
        sidebar,
        sidebar_content: Rect::new(
            sidebar.x,
            sidebar.y,
            sidebar.width.saturating_sub(1),
            sidebar.height,
        ),
        terminal,
    }
}

pub(super) fn sidebar_area(area: Rect, sidebar_width: Option<u16>) -> Rect {
    dashboard_layout(area, sidebar_width).sidebar_content
}

/// Blank lines drawn before a row: one before every project except the first.
pub(super) fn tree_row_gap(rows: &[TreeRow], index: usize) -> usize {
    usize::from(index > 0 && matches!(rows[index], TreeRow::Project { .. }))
}

/// Lines each row takes. Stacking is all or nothing: when any visible agent label
/// does not fit beside its name in a `width`-cell sidebar, every session with an
/// agent label takes two lines, so the column reads the same way top to bottom.
/// Local shells and sessions without a label always take one.
pub(super) fn tree_row_heights(
    dashboard: &Dashboard,
    rows: &[TreeRow],
    width: usize,
) -> Vec<usize> {
    let session = |row: &TreeRow| match row {
        TreeRow::Session { id } => find_session(dashboard, *id),
        TreeRow::Project { .. } | TreeRow::Workspace { .. } => None,
    };
    let stacked = rows
        .iter()
        .filter_map(session)
        .any(|session| agent_label_needs_own_line(session, width));
    rows.iter()
        .map(|row| {
            let labelled =
                session(row).is_some_and(|session| sidebar_agent_label(session).is_some());
            1 + usize::from(stacked && labelled)
        })
        .collect()
}

/// First sidebar line occupied by `rows[index]`, whose `heights[index]` lines follow.
pub(super) fn tree_row_start(rows: &[TreeRow], heights: &[usize], index: usize) -> usize {
    heights[..index].iter().sum::<usize>()
        + (0..=index)
            .map(|earlier| tree_row_gap(rows, earlier))
            .sum::<usize>()
}

pub(super) fn tree_line_count(rows: &[TreeRow], heights: &[usize]) -> usize {
    heights.iter().sum::<usize>()
        + (0..rows.len())
            .map(|index| tree_row_gap(rows, index))
            .sum::<usize>()
}

/// The row drawn on sidebar `line` and which of that row's lines it is, or `None` for
/// a gap line or a line past the tree.
pub(super) fn tree_line_at<'a>(
    rows: &'a [TreeRow],
    heights: &[usize],
    line: usize,
) -> Option<(&'a TreeRow, usize)> {
    let mut start: usize = 0;
    for (index, row) in rows.iter().enumerate() {
        start += tree_row_gap(rows, index);
        if line < start {
            return None;
        }
        if line < start + heights[index] {
            return Some((row, line - start));
        }
        start += heights[index];
    }
    None
}

const WORKSPACE_INDENT: &str = "  ";
const SESSION_INDENT: &str = "     ";
/// Indent, status glyph and separating space before a session name.
const SESSION_NAME_COLUMN: usize = SESSION_INDENT.len() + 2;
/// Minimum cells kept for a session name before its agent label is dropped.
const SESSION_NAME_MIN_WIDTH: usize = 12;
/// Indent of a stacked label line: the connector sits under the start of the name.
const LABEL_LINE_INDENT: &str = "       ";
/// Indent, connector and separating space before a stacked label.
const LABEL_LINE_COLUMN: usize = LABEL_LINE_INDENT.len() + 2;

fn tree_line_text(
    dashboard: &Dashboard,
    row: &TreeRow,
    row_line: usize,
    stacked: bool,
    width: usize,
    now_unix_ms: u64,
) -> (Line<'static>, Style) {
    let selected = match row {
        TreeRow::Session { id } => dashboard
            .close_confirm_session()
            .map(|closing| closing == *id)
            .unwrap_or_else(|| dashboard.action_session() == Some(*id)),
        TreeRow::Project { .. } | TreeRow::Workspace { .. } => {
            dashboard.selected_container.as_ref() == Some(row)
        }
    };
    let style = Style::default()
        .fg(TEXT)
        .bg(if selected { SURFACE0 } else { BASE });
    let muted = Style::default().fg(MUTED);
    // History and Copy already use the title-bar Close for leaving the capture.
    // The mark replaces the agent label; it does not reflow the title.
    let close_mark = dashboard.row_shows_close_mark(row)
        && width >= 3
        && !matches!(dashboard.mode, InputMode::History | InputMode::Copy);
    let mark = Span::styled("[x]", Style::default().fg(MAUVE));
    let (left, fill, right) = match row {
        TreeRow::Project { name } => {
            let full_title = format!(" {} ", name.to_uppercase());
            let title = clip_text(&full_title, width);
            // A clipped title has no room for a rule; a wide glyph can leave one stray cell.
            let rule = if title == full_title { "─" } else { " " };
            let right = if dashboard.collapsed_projects.contains(name) {
                let count = dashboard
                    .hierarchy
                    .projects
                    .iter()
                    .find(|project| project.name == *name)
                    .map_or(0, |project| project.workspaces.len());
                vec![Span::styled(format!(" ▸ {count} ws "), muted)]
            } else {
                Vec::new()
            };
            (
                vec![Span::styled(
                    title,
                    Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
                )],
                Span::styled(rule, Style::default().fg(SURFACE2)),
                right,
            )
        }
        TreeRow::Workspace { project, id } => {
            let workspace = find_workspace(dashboard, project, id);
            let collapsed = dashboard.collapsed_workspaces.iter().any(
                |(collapsed_project, collapsed_workspace)| {
                    collapsed_project == project && collapsed_workspace == id
                },
            );
            let mut right = Vec::new();
            if let Some(workspace) = workspace {
                if workspace.warning.is_some() {
                    right.push(Span::styled("! ", Style::default().fg(PEACH)));
                }
                if workspace.root {
                    right.push(Span::styled("root ", muted));
                }
            }
            if collapsed {
                let count = workspace.map_or(0, |workspace| workspace.sessions.len());
                right.push(Span::styled(format!("▸ {count} "), muted));
            }
            if close_mark {
                right = vec![mark.clone()];
            }
            let available =
                width.saturating_sub(WORKSPACE_INDENT.len() + 1 + usize::from(close_mark) * 3);
            let label = match workspace {
                Some(workspace) => fit_workspace_sidebar_label(
                    &workspace.name,
                    workspace_disambiguator(dashboard, workspace).as_deref(),
                    available,
                ),
                None => clip_text(&format!(" {id}"), available),
            };
            (
                vec![
                    Span::raw(WORKSPACE_INDENT),
                    Span::styled("󰘬", muted),
                    Span::styled(
                        label,
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                ],
                Span::raw(" "),
                right,
            )
        }
        TreeRow::Session { id } => {
            let Some(session) = find_session(dashboard, *id) else {
                return (
                    Line::from(clip_text(&format!("     session {}", id.0), width)),
                    style,
                );
            };
            let status = SessionStatus::of(session, now_unix_ms);
            if row_line == 1 {
                // The agent and model under a name they could not share a line with,
                // hung from the name by a corner connector two cells in from the glyph.
                let (agent, model) = sidebar_agent_label(session).unwrap_or_default();
                let color = label_color(&session.label);
                let agent_style = Style::default().fg(if status.exited {
                    faded(color, 65)
                } else {
                    color
                });
                let label = clip_text(
                    &format!("{agent}{model}"),
                    width.saturating_sub(LABEL_LINE_COLUMN + 1),
                );
                // A label clipped inside the agent name is all agent colour.
                let split = if label.starts_with(agent.as_str()) {
                    agent.len()
                } else {
                    label.len()
                };
                let (head, tail) = label.split_at(split);
                return (
                    compose_row(
                        vec![
                            Span::raw(LABEL_LINE_INDENT),
                            Span::styled("└ ", muted),
                            Span::styled(head.to_string(), agent_style),
                            Span::styled(tail.to_string(), muted),
                        ],
                        Span::raw(" "),
                        Vec::new(),
                        width,
                        selected,
                    ),
                    style,
                );
            }
            if session.name == "local" && session.display_name() == "local" {
                // A shell is quiet unless a hook reports real activity inside it.
                let (glyph, glyph_color) = match (status.glyph, status.color) {
                    ('-' | ' ', _) => ('$', SUBTEXT),
                    reported => reported,
                };
                let subtext = Style::default().fg(SUBTEXT);
                (
                    vec![
                        Span::raw(SESSION_INDENT),
                        Span::styled(glyph.to_string(), Style::default().fg(glyph_color)),
                        Span::styled(
                            clip_text(" local", width.saturating_sub(SESSION_INDENT.len() + 1)),
                            subtext,
                        ),
                    ],
                    Span::raw(" "),
                    if close_mark {
                        vec![mark.clone()]
                    } else {
                        Vec::new()
                    },
                )
            } else {
                let (exited, glyph, glyph_color) = (status.exited, status.glyph, status.color);
                let mut name_style = Style::default().fg(TEXT);
                if exited {
                    name_style = name_style.add_modifier(Modifier::DIM);
                }
                // With the column stacked, every label is on its own line, even one that fits.
                let fitted = if close_mark || stacked {
                    None
                } else {
                    sidebar_agent_label(session)
                };
                let name_width = match &fitted {
                    Some((agent, model)) => {
                        let cells = Line::raw(agent).width() + Line::raw(model).width();
                        width.saturating_sub(SESSION_NAME_COLUMN + cells + 3)
                    }
                    None => width.saturating_sub(SESSION_NAME_COLUMN + usize::from(close_mark) * 3),
                };
                let right = if close_mark {
                    vec![mark.clone()]
                } else if let Some((agent, model)) = fitted {
                    let color = label_color(&session.label);
                    let mut spans = vec![Span::styled(
                        agent,
                        Style::default().fg(if exited { faded(color, 65) } else { color }),
                    )];
                    if !model.is_empty() {
                        spans.push(Span::styled(model, Style::default().fg(MUTED)));
                    }
                    spans.push(Span::raw(" "));
                    spans
                } else {
                    Vec::new()
                };
                (
                    vec![
                        Span::raw(SESSION_INDENT),
                        Span::styled(glyph.to_string(), Style::default().fg(glyph_color)),
                        Span::raw(" "),
                        Span::styled(
                            clipped_session_title(dashboard, session, name_width),
                            name_style,
                        ),
                    ],
                    Span::raw(" "),
                    right,
                )
            }
        }
    };
    (compose_row(left, fill, right, width, selected), style)
}

/// Compose one `width`-cell sidebar row: `left`, the `fill` glyph across the gap,
/// then `right`. A selected row replaces its first cell with a mauve bar.
fn compose_row(
    mut left: Vec<Span<'static>>,
    fill: Span<'static>,
    right: Vec<Span<'static>>,
    width: usize,
    selected: bool,
) -> Line<'static> {
    let cells = |spans: &[Span<'static>]| spans.iter().map(Span::width).sum::<usize>();
    let right = if cells(&left) + cells(&right) > width {
        Vec::new()
    } else {
        right
    };
    let gap = width.saturating_sub(cells(&left) + cells(&right));
    if selected && let Some(first) = left.first_mut() {
        let mut rest = first.content.to_string();
        if !rest.is_empty() {
            rest.remove(0);
        }
        first.content = rest.into();
        left.insert(0, Span::styled("▌", Style::default().fg(MAUVE)));
    }
    let mut spans = left;
    spans.push(Span::styled(fill.content.repeat(gap), fill.style));
    spans.extend(right);
    Line::from(spans)
}

fn provider_metrics(session: &SessionSummary, now: u64, width: usize) -> Option<String> {
    let agent = session.agent.as_ref()?;
    let Some(metrics) = &agent.metrics else {
        return Some("tokens —  cost —".into());
    };
    let scope = |scope| match scope {
        UsageScope::Conversation => "conv",
        UsageScope::Invocation => "inv",
    };
    let exited = matches!(session.phase, SessionPhase::Exited { .. });
    let stale_label = |received| {
        if ovrcr_protocol::freshness::is_stale(received, now, exited) {
            " stale"
        } else {
            ""
        }
    };
    let usage = &metrics.sample.usage;
    // Input already includes cache subsets; reasoning is a subset of output.
    let tokens = usage
        .value
        .input_tokens
        .zip(usage.value.output_tokens)
        .map(|(input, output)| (u128::from(input) + u128::from(output)).to_string())
        .unwrap_or_else(|| "—".into());
    let partial = if usage.value.coverage == UsageCoverage::Partial {
        " partial"
    } else {
        ""
    };
    let cost = &metrics.sample.cost;
    let amount = cost.value.as_ref().map_or_else(
        || "—".into(),
        |cost| {
            let estimate = if cost.kind == CostKind::Estimated {
                "estimate "
            } else {
                ""
            };
            let cents = (u128::from(cost.usd_ticks) + 50_000_000) / 100_000_000;
            format!(
                "{} {estimate}${}.{:02}",
                scope(cost.scope),
                cents / 100,
                cents % 100
            )
        },
    );
    let full = format!(
        "tokens {}{partial} {tokens}{}  cost {amount}{}",
        scope(usage.value.scope),
        stale_label(metrics.usage_received_unix_ms),
        stale_label(metrics.cost_received_unix_ms)
    );
    if Line::raw(&full).width() <= width {
        return Some(full);
    }
    let compact = full.replacen(" estimate ", " est ", 1);
    if Line::raw(&compact).width() <= width {
        Some(compact)
    } else {
        Some(
            compact
                .replacen("tokens ", "tok ", 1)
                .replacen("  cost ", " cost ", 1),
        )
    }
}

/// Whether a session's agent label takes its own sidebar line: it has one, and the
/// whole `agent:model` cannot sit beside a twelve-cell name in `width` cells.
fn agent_label_needs_own_line(session: &SessionSummary, width: usize) -> bool {
    sidebar_agent_label(session).is_some_and(|(agent, model)| {
        Line::raw(agent).width() + Line::raw(model).width()
            > width.saturating_sub(SESSION_NAME_COLUMN + SESSION_NAME_MIN_WIDTH + 3)
    })
}

/// The agent name and its `:model` suffix (empty when no model is known), or `None`
/// for a quiet local shell or a session without an agent label.
fn sidebar_agent_label(session: &SessionSummary) -> Option<(String, String)> {
    if session.name == "local" && session.display_name() == "local" {
        return None;
    }
    let agent = session
        .label
        .split_once('/')
        .map_or(session.label.as_str(), |(agent, _)| agent)
        .trim();
    if agent.is_empty() {
        return None;
    }
    let reported = session
        .agent
        .as_ref()
        .and_then(|agent| agent.metrics.as_ref())
        .and_then(|metrics| metrics.sample.model.as_deref())
        .map(str::trim)
        .filter(|model| !model.is_empty());
    let stored = session
        .label
        .split_once('/')
        .map(|(_, model)| model.trim())
        .filter(|model| !model.is_empty());
    let Some(model) = reported.or(stored) else {
        return Some((agent.to_string(), String::new()));
    };
    let model = model_without_agent(agent, model);
    let suffix = if model.is_empty() {
        String::new()
    } else {
        format!(":{model}")
    };
    Some((agent.to_string(), suffix))
}

fn model_without_agent(agent: &str, model: &str) -> String {
    let chars: Vec<char> = model.chars().collect();
    let agent_len = agent.chars().count();
    if chars.len() > agent_len {
        let head: String = chars[..agent_len].iter().collect();
        if head.eq_ignore_ascii_case(agent) && matches!(chars[agent_len], '-' | '/' | ':' | ' ') {
            return chars[agent_len + 1..].iter().collect();
        }
    }
    model.to_string()
}

fn label_color(label: &str) -> Color {
    let explicit_label = label.split('/').next().unwrap_or(label).trim();
    if explicit_label.eq_ignore_ascii_case("claude") {
        PEACH
    } else if explicit_label.eq_ignore_ascii_case("codex") {
        GREEN
    } else if explicit_label.eq_ignore_ascii_case("pi") {
        MAUVE
    } else if explicit_label.eq_ignore_ascii_case("omp") {
        SKY
    } else if explicit_label.eq_ignore_ascii_case("grok") {
        BLUE
    } else if explicit_label.eq_ignore_ascii_case("terminal") {
        SUBTEXT
    } else {
        TEXT
    }
}

// Match the mockup's label/metadata opacity against its solid terminal background.
fn faded(color: Color, percent: u16) -> Color {
    match (color, BASE) {
        (Color::Rgb(r, g, b), Color::Rgb(br, bg, bb)) => {
            let blend = |channel, background| {
                ((u16::from(channel) * percent + u16::from(background) * (100 - percent) + 50)
                    / 100) as u8
            };
            Color::Rgb(blend(r, br), blend(g, bg), blend(b, bb))
        }
        _ => color,
    }
}

pub(super) fn clip_text(text: &str, width: usize) -> String {
    if Line::raw(text).width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut clipped = String::new();
    for character in text.chars() {
        let next = format!("{clipped}{character}");
        if Line::raw(&next).width() > width - 1 {
            break;
        }
        clipped.push(character);
    }
    clipped.push('…');
    clipped
}

fn fit_workspace_sidebar_label(name: &str, disambiguator: Option<&str>, width: usize) -> String {
    let text = match disambiguator {
        Some(suffix) => format!(" {name} ({suffix})"),
        None => format!(" {name}"),
    };
    if Line::raw(&text).width() <= width {
        return text;
    }
    let Some(suffix) = disambiguator else {
        return clip_text(&text, width);
    };
    let tail = format!(" ({suffix})");
    let tail_width = Line::raw(&tail).width();
    if tail_width + 1 >= width {
        return clip_text(&text, width);
    }
    let name_width = width.saturating_sub(tail_width + 1);
    format!(" {}{tail}", clip_text(name, name_width))
}

pub fn actual_drawn_inner_rect(area: Rect) -> Rect {
    dashboard_layout(area, None).terminal
}

pub(super) fn pane_size(size: TerminalSize) -> TerminalSize {
    let inner = actual_drawn_inner_rect(Rect::new(0, 0, size.cols, size.rows));
    TerminalSize {
        rows: inner.height.max(1),
        cols: inner.width.max(1),
    }
}

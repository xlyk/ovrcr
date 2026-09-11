use super::copy::{CopyPoint, CopySelection};
use super::state::{find_session, history_page_covers};
use super::{Dashboard, HistoryView, InputMode, PaneRects, PaneState, TreeRow, history_view_size};
use crate::context::{ContextSource, ContextUsageReport, ContextUsageSnapshot, format_context};
use crate::session::{AgentActivity, SessionPhase, TerminalSize};
use crate::task_tui::draw_tasks;
use ovrcr_protocol::{
    CostKind, HistoryColor, MeasurementFreshness, ReporterHealth, SampleQuality, SessionSummary,
    UsageCoverage, UsageScope,
};
use ovrcr_terminal::vt100;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn action_controls(area: Rect) -> (Rect, Rect) {
    let title = dashboard_layout(area).title;
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
    let title = dashboard_layout(area).title;
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
const DEFAULT_SIDEBAR_WIDTH: u16 = 40;
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
pub(super) const SPINNER_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

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
    let layout = dashboard_layout(frame.area());
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

    frame.render_widget(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(MUTED))
            .style(Style::default().bg(BASE)),
        layout.sidebar,
    );
    let rows = dashboard.visible_rows();
    let viewport_height = usize::from(layout.sidebar_content.height);
    let start = dashboard.tree_offset.min(tree_line_count(&rows));
    for screen_line in 0..viewport_height {
        let Some((row, row_line)) = tree_line_at(&rows, start.saturating_add(screen_line)) else {
            continue;
        };
        let y = layout.sidebar_content.y.saturating_add(screen_line as u16);
        let (text, style) = tree_line_text(
            dashboard,
            row,
            row_line,
            usize::from(layout.sidebar_content.width),
            now_unix_ms,
        );
        let line_area = Rect::new(layout.sidebar_content.x, y, layout.sidebar_content.width, 1);
        frame.render_widget(Block::default().style(style), line_area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::raw(text))).style(style),
            line_area,
        );
        let accents: &[(u16, Color)] = match row {
            TreeRow::Project { name } => &[(
                2,
                if dashboard
                    .hierarchy
                    .projects
                    .iter()
                    .filter(|project| project.name < *name)
                    .count()
                    % 2
                    == 0
                {
                    MAUVE
                } else {
                    SKY
                },
            )],
            TreeRow::Workspace { .. } => &[(2, MAUVE)],
            TreeRow::Session { id } if row_line == 0 && dashboard.action_session() != Some(*id) => {
                &[(
                    2,
                    if dashboard.session_is_busy(*id) {
                        GREEN
                    } else {
                        MUTED
                    },
                )]
            }
            _ => &[],
        };
        for &(column, color) in accents {
            if column < line_area.width {
                frame.buffer_mut()[(line_area.x + column, y)].set_fg(color);
            }
        }
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
                    && pane.ready
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
                let pid = if matches!(session.phase, SessionPhase::Exited { .. }) {
                    "closed".to_string()
                } else {
                    session
                        .pid
                        .map_or_else(|| "—".to_string(), |pid| pid.to_string())
                };
                if session
                    .agent
                    .as_ref()
                    .and_then(|agent| agent.activity.as_ref())
                    .is_some_and(|activity| activity.state == AgentActivity::ResponseReady)
                {
                    let paused = if matches!(session.phase, SessionPhase::Paused) {
                        " paused"
                    } else {
                        ""
                    };
                    let mut text = clip_text(
                        &format!("pid: {pid}{paused}{}", provider_activity(session)),
                        usize::from(rect.metadata.width),
                    );
                    append_metadata_field(
                        &mut text,
                        &format!(
                            "elapsed: {}",
                            format_elapsed_at(session.started_unix_ms, now_unix_ms)
                        ),
                        rect.metadata.width,
                    );
                    return Line::from(Span::styled(text, Style::default().fg(TEAL)));
                }
                let activity = if matches!(session.phase, SessionPhase::Exited { .. })
                    && !session
                        .agent
                        .as_ref()
                        .and_then(|agent| agent.activity.as_ref())
                        .is_some_and(|activity| activity.state == AgentActivity::ResponseReady)
                {
                    Span::raw("")
                } else {
                    let label = if session.agent.is_some() {
                        format!("agent{}", provider_activity(session))
                    } else {
                        match session.activity {
                            AgentActivity::Unknown => "agent unknown",
                            AgentActivity::Idle => "agent idle",
                            AgentActivity::Busy => "agent busy",
                            AgentActivity::WaitingInput => "agent waiting input",
                            AgentActivity::Error => "agent error",
                            AgentActivity::ResponseReady => "agent response ready",
                        }
                        .to_owned()
                    };
                    Span::styled(format!("  {label}"), Style::default().fg(TEAL))
                };
                Line::from(vec![
                    Span::styled("pid: ", Style::default().fg(MUTED)),
                    Span::styled(pid, Style::default().fg(TEAL)),
                    Span::styled("  elapsed: ", Style::default().fg(MUTED)),
                    Span::styled(
                        format_elapsed_at(session.started_unix_ms, now_unix_ms),
                        Style::default().fg(TEAL),
                    ),
                    activity,
                    if matches!(session.phase, SessionPhase::Paused) {
                        Span::styled("  paused", Style::default().fg(PEACH))
                    } else {
                        Span::raw("")
                    },
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
                        .and_then(|session| {
                            provider_metrics(session, now_unix_ms, usize::from(rect.metadata.width))
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
                    && pane.ready
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
            if let Some(notice) = dashboard.copy_notice.as_deref() {
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
        let groups = super::hints::key_hints(self);
        let hints: Vec<_> = groups.iter().flat_map(|g| &g.hints).collect();
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
        } else if let Some(super::TreeRow::Workspace { project, name }) = &self.selected_container {
            let empty = self
                .hierarchy
                .projects
                .iter()
                .find(|p| p.name == *project)
                .and_then(|p| p.workspaces.iter().find(|w| w.name == *name))
                .is_some_and(|w| w.sessions.is_empty());
            if empty && let Some(hint) = hints.iter().find(|h| h.key == "n") {
                lines.push(Line::from(format!(
                    "{project} / {name}: press {} to start a terminal here",
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
    let name = session.map_or("no session", |session| session.name.as_str());
    let prefix = if focused { "> " } else { "  " };
    let mut text = if pane.ready {
        format!(
            "{prefix}{name} {}x{}",
            rect.terminal.width, rect.terminal.height
        )
    } else {
        format!("{prefix}loading {name}")
    };
    let ready_agent = session.and_then(|session| {
        session.agent.as_ref().filter(|agent| {
            agent
                .activity
                .as_ref()
                .is_some_and(|activity| activity.state == AgentActivity::ResponseReady)
        })
    });
    if let Some(session) = session
        && (pane.ready || ready_agent.is_some())
    {
        let pid = if matches!(session.phase, SessionPhase::Exited { .. }) {
            "closed".to_string()
        } else {
            session
                .pid
                .map_or_else(|| "—".to_string(), |pid| pid.to_string())
        };
        if ready_agent.is_some() {
            // Reserve process and health status before clipping activity quality.
            // At intermediate widths, identity/geometry yields to these statuses.
            let status = format!("pid: {pid}{}", provider_activity(session));
            let with_identity = format!("{text}  {status}");
            text = if Line::raw(&with_identity).width() <= usize::from(rect.metadata.width) {
                with_identity
            } else {
                clip_text(&status, usize::from(rect.metadata.width))
            };
        } else {
            append_metadata_field(&mut text, &format!("pid: {pid}"), rect.metadata.width);
        }
        append_metadata_field(
            &mut text,
            &format!(
                "elapsed: {}",
                format_elapsed_at(session.started_unix_ms, now_unix_ms)
            ),
            rect.metadata.width,
        );
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

fn dashboard_layout(area: Rect) -> DashboardLayout {
    let [title, body, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(area);
    let sidebar_width = body.width.min(DEFAULT_SIDEBAR_WIDTH).min(body.width / 2);
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

pub(super) fn sidebar_area(area: Rect) -> Rect {
    dashboard_layout(area).sidebar_content
}

pub(super) fn tree_row_height(row: &TreeRow) -> usize {
    match row {
        TreeRow::Session { .. } => 3,
        TreeRow::Project { .. } | TreeRow::Workspace { .. } => 1,
    }
}

pub(super) fn tree_line_count(rows: &[TreeRow]) -> usize {
    rows.iter()
        .enumerate()
        .map(|(index, row)| tree_row_gap(rows, index) + tree_row_height(row))
        .sum()
}

pub(super) fn tree_row_gap(rows: &[TreeRow], index: usize) -> usize {
    usize::from(
        index > 0
            && match &rows[index] {
                TreeRow::Project { .. } => true,
                TreeRow::Workspace { .. } => !matches!(rows[index - 1], TreeRow::Project { .. }),
                TreeRow::Session { .. } => false,
            },
    )
}

pub(super) fn tree_line_at(rows: &[TreeRow], line: usize) -> Option<(&TreeRow, usize)> {
    let mut start: usize = 0;
    for (index, row) in rows.iter().enumerate() {
        start += tree_row_gap(rows, index);
        if line < start {
            return None;
        }
        let height = tree_row_height(row);
        if line < start.saturating_add(height) {
            return Some((row, line - start));
        }
        start += height;
    }
    None
}

fn tree_line_text(
    dashboard: &Dashboard,
    row: &TreeRow,
    line: usize,
    width: usize,
    now_unix_ms: u64,
) -> (String, Style) {
    let base_style =
        Style::default()
            .fg(TEXT)
            .bg(if dashboard.selected_container.as_ref() == Some(row) {
                CRUST
            } else {
                BASE
            });
    match row {
        TreeRow::Project { name } => {
            let disclosure = if dashboard.collapsed_projects.contains(name) {
                '▶'
            } else {
                '▼'
            };
            (
                clip_text(&format!("{disclosure} 󰉋 {name}"), width),
                base_style.fg(BLUE).add_modifier(Modifier::BOLD),
            )
        }
        TreeRow::Workspace { project, name } => {
            let disclosure = if dashboard.collapsed_workspaces.iter().any(
                |(collapsed_project, collapsed_workspace)| {
                    collapsed_project == project && collapsed_workspace == name
                },
            ) {
                '▶'
            } else {
                '▼'
            };
            (
                clip_text(&format!("  {disclosure} {name}"), width),
                base_style.fg(TEXT).add_modifier(Modifier::BOLD),
            )
        }
        TreeRow::Session { id } => {
            let Some(session) = find_session(dashboard, *id) else {
                return (
                    clip_text(&format!("    session {}", id.0), width),
                    base_style,
                );
            };
            let selected = dashboard.action_session() == Some(*id);
            let label = if session.name == "local" {
                "terminal"
            } else {
                session.label.as_str()
            };
            let status = match (&session.phase, session.activity) {
                (SessionPhase::Exited { .. }, _) => ' ',
                (SessionPhase::Paused, _) => 'P',
                (_, AgentActivity::Unknown) => '-',
                (_, AgentActivity::Idle) => ' ',
                (_, AgentActivity::WaitingInput) => '?',
                (_, AgentActivity::Error) => '!',
                (_, AgentActivity::ResponseReady) => '✓',
                (SessionPhase::Running, AgentActivity::Busy) => {
                    SPINNER_FRAMES[((now_unix_ms / 100) % SPINNER_FRAMES.len() as u64) as usize]
                }
            };
            let text = match line {
                0 => format!("  {status} {}", session.name),
                1 if session.agent.as_ref().is_some_and(|agent| {
                    agent.health.state == ReporterHealth::Unavailable
                        && agent
                            .activity
                            .as_ref()
                            .is_some_and(|activity| activity.state == AgentActivity::ResponseReady)
                }) =>
                {
                    format!("     ├{} {label}", provider_activity(session))
                }
                1 => format!("     ├ {label}{}", provider_activity(session)),
                _ => format!(
                    "     └ run {}  ctx {}",
                    format_elapsed_at(session.started_unix_ms, now_unix_ms),
                    session_context(session, now_unix_ms)
                ),
            };
            let mut style = if selected {
                Style::default().fg(CRUST).bg(MAUVE)
            } else if line == 1 {
                Style::default()
                    .fg(if matches!(session.phase, SessionPhase::Exited { .. }) {
                        faded(label_color(label), 65)
                    } else {
                        label_color(label)
                    })
                    .bg(BASE)
            } else if line == 2 {
                Style::default()
                    .fg(if matches!(session.phase, SessionPhase::Exited { .. }) {
                        faded(MUTED, 80)
                    } else {
                        SUBTEXT
                    })
                    .bg(BASE)
            } else {
                base_style
            };
            if line == 0 {
                style = style.add_modifier(Modifier::BOLD);
                if !selected && matches!(session.phase, SessionPhase::Exited { .. }) {
                    style = style.add_modifier(Modifier::DIM);
                }
            }
            (clip_text(&text, width), style)
        }
    }
}

// Bound snapshots are authoritative even when a component is unknown after clear.
fn session_context(session: &SessionSummary, now: u64) -> String {
    let exited = matches!(session.phase, SessionPhase::Exited { .. });
    let Some(agent) = &session.agent else {
        return format_context(session.context_usage.as_ref(), now, exited);
    };
    let Some(metrics) = &agent.metrics else {
        return "—".into();
    };
    let context = &metrics.sample.context;
    let sample = ContextUsageSnapshot {
        report: ContextUsageReport {
            source: ContextSource::Generic,
            model: None,
            conversation: None,
            used_tokens: context.value.used_tokens,
            capacity_tokens: context.value.capacity_tokens,
        },
        received_unix_ms: metrics.context_received_unix_ms,
    };
    let mut text = format_context(Some(&sample), now, exited);
    if context.freshness == MeasurementFreshness::Uncertain {
        text.push_str(" uncertain");
    }
    if context.value.quality == SampleQuality::Estimated {
        text.push_str(" estimate");
    }
    text
}

fn provider_activity(session: &SessionSummary) -> String {
    let Some(agent) = &session.agent else {
        return String::new();
    };
    if let Some(activity) = &agent.activity
        && activity.state == AgentActivity::ResponseReady
    {
        let quality = match activity.quality {
            SampleQuality::Confirmed => "confirmed",
            SampleQuality::Observed => "observed",
            SampleQuality::Estimated => "estimated",
        };
        let health = if agent.health.state == ReporterHealth::Unavailable {
            " unavailable"
        } else {
            ""
        };
        return format!("{health} response ready · {quality}");
    }
    if agent.health.state == ReporterHealth::Unavailable {
        return " unavailable".into();
    }
    let Some(activity) = &agent.activity else {
        return " unknown".into();
    };
    let state = match activity.state {
        AgentActivity::Unknown => "unknown",
        AgentActivity::Idle => "idle",
        AgentActivity::Busy => "busy",
        AgentActivity::WaitingInput => "waiting",
        AgentActivity::Error => "error",
        AgentActivity::ResponseReady => "response ready",
    };
    let quality = match activity.quality {
        SampleQuality::Confirmed => "confirmed",
        SampleQuality::Observed => "observed",
        SampleQuality::Estimated => "estimated",
    };
    format!(" {state} {quality}")
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
    let age = |received, freshness| {
        if matches!(session.phase, SessionPhase::Exited { .. })
            || now
                .checked_sub(received)
                .is_none_or(|age| age >= crate::context::CONTEXT_STALE_AFTER_MS)
        {
            " stale"
        } else if freshness == MeasurementFreshness::Uncertain {
            " uncertain"
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
        age(metrics.usage_received_unix_ms, usage.freshness),
        age(metrics.cost_received_unix_ms, cost.freshness)
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

fn label_color(label: &str) -> Color {
    let explicit_label = label.split('/').next().unwrap_or(label).trim();
    if explicit_label.eq_ignore_ascii_case("claude") {
        PEACH
    } else if explicit_label.eq_ignore_ascii_case("codex") {
        GREEN
    } else if explicit_label.eq_ignore_ascii_case("pi") {
        MAUVE
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

fn format_elapsed_at(started_unix_ms: u64, now_unix_ms: u64) -> String {
    let minutes = now_unix_ms.saturating_sub(started_unix_ms) / 60_000;
    let days = minutes / (24 * 60);
    let hours = minutes / 60 % 24;
    let minutes = minutes % 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h{minutes:02}")
    } else {
        format!("{minutes}m")
    }
}

pub fn actual_drawn_inner_rect(area: Rect) -> Rect {
    dashboard_layout(area).terminal
}

pub(super) fn pane_size(size: TerminalSize) -> TerminalSize {
    let inner = actual_drawn_inner_rect(Rect::new(0, 0, size.cols, size.rows));
    TerminalSize {
        rows: inner.height.max(1),
        cols: inner.width.max(1),
    }
}

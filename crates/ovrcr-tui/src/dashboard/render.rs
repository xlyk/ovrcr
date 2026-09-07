use super::copy::{CopyPoint, CopySelection};
use super::state::{find_session, history_page_covers};
use super::{Dashboard, HistoryView, InputMode, TreeRow, history_view_size};
use crate::context::format_context;
use crate::session::{AgentActivity, SessionPhase, TerminalSize};
use ovrcr_protocol::HistoryColor;
use ovrcr_terminal::vt100;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
struct DashboardLayout {
    title: Rect,
    footer: Rect,
    sidebar: Rect,
    sidebar_content: Rect,
    metadata: Rect,
    terminal: Rect,
}

pub(super) const METADATA_HEIGHT: u16 = 2;
const DEFAULT_SIDEBAR_WIDTH: u16 = 40;
const BASE: Color = Color::Rgb(30, 30, 46);
const CRUST: Color = Color::Rgb(17, 17, 27);
const TEXT: Color = Color::Rgb(205, 214, 244);
const SUBTEXT: Color = Color::Rgb(166, 173, 200);
const MUTED: Color = Color::Rgb(108, 112, 134);
const MAUVE: Color = Color::Rgb(203, 166, 247);
const PEACH: Color = Color::Rgb(250, 179, 135);
const GREEN: Color = Color::Rgb(166, 227, 161);
const TEAL: Color = Color::Rgb(148, 226, 213);
const BLUE: Color = Color::Rgb(137, 180, 250);
const SKY: Color = Color::Rgb(137, 220, 235);
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
            let symbol = if contents.is_empty() { " " } else { contents };
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
    let bounded = Rect::new(area.x, area.y, area.width.min(256), area.height.min(64));
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
    if let Some(cursor) = view.cursor.filter(|_| view.cursor_target.is_none()) {
        if cursor.row_width > 0
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
}

fn history_cell_at<'a>(
    view: &'a HistoryView,
    row: u32,
    col: u16,
) -> Option<(
    u16,
    &'a crate::protocol::HistoryRow,
    &'a crate::protocol::HistoryCell,
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
    let status = if missing || view.pending.is_some() {
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

fn history_footer(dashboard: &Dashboard) -> Line<'static> {
    let text = dashboard.history.as_ref().map_or_else(
        || "HISTORY  arrows/hjkl scroll  PgUp/PgDn page  Home/End bounds  Space/v anchor  Esc/q exit".to_owned(),
        |view| {
            if view.copy_job.is_some() || view.copy_completion.is_some() {
                "HISTORY COPY  Copying selection  Esc cancel  q exit".to_owned()
            } else if view.cursor_target.is_some() {
                "HISTORY  Waiting for history cell  Esc/q exit".to_owned()
            } else if view.anchor.is_some() {
                view.cursor.map_or_else(
                    || "HISTORY  Nothing to select on this row  Esc/q exit".to_owned(),
                    |cursor| {
                        format!(
                            "HISTORY SELECT  row {}:{}  arrows/hjkl move  Space/v anchor  y copy  Esc/q exit",
                            cursor.point.row.saturating_add(1),
                            u32::from(cursor.point.col).saturating_add(1),
                        )
                    },
                )
            } else {
                "HISTORY  arrows/hjkl scroll  PgUp/PgDn page  Home/End bounds  Space/v anchor  Esc/q exit".to_owned()
            }
        },
    );
    Line::from(Span::styled(text, Style::default().fg(TEXT)))
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
            TreeRow::Session { id } if row_line == 0 && dashboard.selected != Some(*id) => &[(
                2,
                if dashboard.session_is_busy(*id) {
                    GREEN
                } else {
                    MUTED
                },
            )],
            _ => &[],
        };
        for &(column, color) in accents {
            if column < line_area.width {
                frame.buffer_mut()[(line_area.x + column, y)].set_fg(color);
            }
        }
    }

    let selected = dashboard
        .selected
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
            let activity = if matches!(session.phase, SessionPhase::Exited { .. }) {
                Span::raw("")
            } else {
                let label = match session.activity {
                    AgentActivity::Unknown => "agent unknown",
                    AgentActivity::Idle => "agent idle",
                    AgentActivity::Busy => "agent busy",
                    AgentActivity::WaitingInput => "agent waiting input",
                    AgentActivity::Error => "agent error",
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
    if !layout.metadata.is_empty() {
        frame.render_widget(
            Paragraph::new(metadata).style(Style::default().bg(BASE)),
            Rect::new(
                layout.metadata.x,
                layout.metadata.y,
                layout.metadata.width,
                1,
            ),
        );
    }
    if layout.metadata.height > 1 {
        let metadata_hint = dashboard.history.as_ref().map_or_else(
            || "─".repeat(usize::from(layout.metadata.width)),
            |view| history_hint(view, dashboard.pane_size),
        );
        frame.render_widget(
            Paragraph::new(metadata_hint).style(Style::default().fg(MUTED).bg(BASE)),
            Rect::new(
                layout.metadata.x,
                layout.metadata.y.saturating_add(1),
                layout.metadata.width,
                1,
            ),
        );
    }
    if let Some(copy) = dashboard.copy.as_ref() {
        render_copy(frame, layout.terminal, copy);
    } else if let Some(view) = dashboard
        .history
        .as_ref()
        .filter(|_| dashboard.mode == InputMode::History)
    {
        render_history(frame, layout.terminal, view);
    } else {
        render_terminal(
            frame,
            layout.terminal,
            dashboard.parser.screen(),
            dashboard.mode == InputMode::Terminal,
        );
    }
    let footer = dashboard.error.as_deref().map_or_else(
        || {
            if let Some(notice) = dashboard.copy_notice.as_deref() {
                return Line::from(Span::styled(notice, Style::default().fg(TEXT)));
            }
            if dashboard.mode == InputMode::Copy {
                return Line::from(vec![
                    Span::styled("COPY  ", Style::default().fg(TEAL)),
                    Span::styled("h/j/k/l", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" move  ", Style::default().fg(MUTED)),
                    Span::styled("Space", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" anchor  ", Style::default().fg(MUTED)),
                    Span::styled("y", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" copy  ", Style::default().fg(MUTED)),
                    Span::styled("Esc", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" cancel", Style::default().fg(MUTED)),
                ]);
            }
            let paused = dashboard.selected_phase() == Some(&SessionPhase::Paused);
            let narrow = layout.footer.width < 60;
            let mut footer = if dashboard.mode == InputMode::Terminal {
                vec![
                    Span::styled("Terminal mode  ", Style::default().fg(MUTED)),
                    Span::styled("Ctrl-g", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" browse  ", Style::default().fg(MUTED)),
                ]
            } else if dashboard.mode == InputMode::History {
                return history_footer(dashboard);
            } else if paused && narrow {
                vec![
                    Span::styled("r", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" resume  ", Style::default().fg(MUTED)),
                    Span::styled("p", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" pause  ", Style::default().fg(MUTED)),
                ]
            } else {
                vec![
                    Span::styled("j/k/↑/↓", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" select  ", Style::default().fg(MUTED)),
                    Span::styled("Enter", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" focus  ", Style::default().fg(MUTED)),
                    Span::styled("p", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" pause  ", Style::default().fg(MUTED)),
                    Span::styled("r", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" resume  ", Style::default().fg(MUTED)),
                ]
            };
            if dashboard.mode != InputMode::Terminal
                && dashboard.mode != InputMode::History
                && !(paused && narrow)
            {
                footer.extend([
                    Span::styled("Ctrl-g", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" browse  ", Style::default().fg(MUTED)),
                ]);
            }
            if dashboard.mode == InputMode::Browse {
                footer.extend([
                    Span::styled("q", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" detach", Style::default().fg(MUTED)),
                ]);
            }
            Line::from(footer)
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
    let [metadata, terminal] = Layout::default()
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
        metadata,
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
    let base_style = Style::default().fg(TEXT).bg(BASE);
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
            let selected = dashboard.selected == Some(*id);
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
                (SessionPhase::Running, AgentActivity::Busy) => {
                    SPINNER_FRAMES[((now_unix_ms / 100) % SPINNER_FRAMES.len() as u64) as usize]
                }
            };
            let text = match line {
                0 => format!("  {status} {}", session.name),
                1 => format!("     ├ {label}"),
                _ => format!(
                    "     └ run {}  ctx {}",
                    format_elapsed_at(session.started_unix_ms, now_unix_ms),
                    format_context(
                        session.context_usage.as_ref(),
                        now_unix_ms,
                        matches!(session.phase, SessionPhase::Exited { .. }),
                    )
                ),
            };
            let mut style = if selected {
                Style::default().fg(CRUST).bg(MAUVE)
            } else if line == 1 {
                Style::default().fg(faded(label_color(label), 65)).bg(BASE)
            } else if line == 2 {
                Style::default().fg(faded(MUTED, 80)).bg(BASE)
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

fn clip_text(text: &str, width: usize) -> String {
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

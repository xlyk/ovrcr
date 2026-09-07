use super::state::find_session;
use super::{Dashboard, InputMode, TreeRow};
use crate::context::format_context;
use crate::session::{AgentActivity, SessionPhase, TerminalSize};
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
        frame.render_widget(
            Paragraph::new("─".repeat(usize::from(layout.metadata.width)))
                .style(Style::default().fg(MUTED).bg(BASE)),
            Rect::new(
                layout.metadata.x,
                layout.metadata.y.saturating_add(1),
                layout.metadata.width,
                1,
            ),
        );
    }
    render_terminal(
        frame,
        layout.terminal,
        dashboard.parser.screen(),
        dashboard.mode == InputMode::Terminal,
    );
    let footer = dashboard.error.as_deref().map_or_else(
        || {
            let paused = dashboard.selected_phase() == Some(&SessionPhase::Paused);
            let narrow = layout.footer.width < 60;
            let mut footer = if dashboard.mode == InputMode::Terminal {
                vec![
                    Span::styled("Terminal mode  ", Style::default().fg(MUTED)),
                    Span::styled("Ctrl-g", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" browse  ", Style::default().fg(MUTED)),
                ]
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
            if dashboard.mode != InputMode::Terminal && !(paused && narrow) {
                footer.extend([
                    Span::styled("Ctrl-g", Style::default().fg(Color::Rgb(249, 226, 175))),
                    Span::styled(" browse  ", Style::default().fg(MUTED)),
                ]);
            }
            if dashboard.mode != InputMode::Terminal {
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

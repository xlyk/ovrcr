use crate::context::format_context;
use crate::protocol::{
    ClientMessage, HierarchySnapshot, Request, Response, ServerEvent, ServerMessage,
};
use crate::session::{AgentActivity, SessionId, SessionPhase, TerminalSize};
use anyhow::{Context, Result, bail};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::{cursor, execute, terminal as crossterm_terminal};
pub use ovrcr_terminal::encode_paste;
use ovrcr_terminal::vt100;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use std::cell::Cell;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::panic;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

thread_local! {
    static PANIC_TERMINAL_RESTORED: Cell<bool> = const { Cell::new(false) };
}

pub const DASHBOARD_READER_QUEUE_CAPACITY: usize = 64;
const DASHBOARD_INPUT_BATCH_LIMIT: usize = 32;
const DASHBOARD_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const DASHBOARD_EVENT_PROBE: Duration = Duration::from_micros(100);
const DASHBOARD_IDLE_REDRAW_INTERVAL: Duration = Duration::from_secs(1);
const SPINNER_INTERVAL: Duration = Duration::from_millis(100);
const SPINNER_FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

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
const METADATA_HEIGHT: u16 = 2;
const DEFAULT_SIDEBAR_WIDTH: u16 = 40;

#[derive(Clone, Copy)]
struct DashboardLayout {
    title: Rect,
    footer: Rect,
    sidebar: Rect,
    sidebar_content: Rect,
    metadata: Rect,
    terminal: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Browse,
    Terminal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeRow {
    Project { name: String },
    Workspace { project: String, name: String },
    Session { id: SessionId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DashboardAction {
    None,
    Redraw,
    Detach,
    EnterBrowse,
    PtyBytes(Vec<u8>),
    Request(ClientMessage),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyEncoding {
    Browse,
    Bytes(Vec<u8>),
    Ignore,
}

pub struct Dashboard {
    pub hierarchy: HierarchySnapshot,
    pub selected: Option<SessionId>,
    pub mode: InputMode,
    pub parser: vt100::Parser,
    pub pane_size: TerminalSize,
    pub collapsed_projects: HashSet<String>,
    pub collapsed_workspaces: HashSet<(String, String)>,
    pub error: Option<String>,
    tree_offset: usize,
    next_request_id: u64,
}

impl Dashboard {
    pub fn new(size: TerminalSize) -> Self {
        Self {
            hierarchy: HierarchySnapshot {
                projects: Vec::new(),
            },
            selected: None,
            mode: InputMode::Browse,
            parser: vt100::Parser::new(size.rows, size.cols, 0),
            pane_size: size,
            collapsed_projects: HashSet::new(),
            collapsed_workspaces: HashSet::new(),
            error: None,
            tree_offset: 0,
            next_request_id: 1,
        }
    }

    fn session_is_busy(&self, id: SessionId) -> bool {
        find_session(self, id).is_some_and(|session| {
            matches!(session.phase, crate::session::SessionPhase::Running)
                && session.activity == crate::session::AgentActivity::Busy
        })
    }

    fn redraw_interval(&self) -> Duration {
        if self.hierarchy.projects.iter().any(|project| {
            project.workspaces.iter().any(|workspace| {
                workspace
                    .sessions
                    .iter()
                    .any(|session| self.session_is_busy(session.id))
            })
        }) {
            SPINNER_INTERVAL
        } else {
            DASHBOARD_IDLE_REDRAW_INTERVAL
        }
    }

    pub fn visible_rows(&self) -> Vec<TreeRow> {
        let mut projects = self.hierarchy.projects.iter().collect::<Vec<_>>();
        projects.sort_by(|left, right| left.name.cmp(&right.name));
        let mut rows = Vec::new();
        for project in projects {
            rows.push(TreeRow::Project {
                name: project.name.clone(),
            });
            if self.collapsed_projects.contains(&project.name) {
                continue;
            }
            let mut workspaces = project.workspaces.iter().collect::<Vec<_>>();
            workspaces.sort_by(|left, right| left.name.cmp(&right.name));
            for workspace in workspaces {
                rows.push(TreeRow::Workspace {
                    project: project.name.clone(),
                    name: workspace.name.clone(),
                });
                if self
                    .collapsed_workspaces
                    .contains(&(project.name.clone(), workspace.name.clone()))
                {
                    continue;
                }
                let mut sessions = workspace.sessions.iter().collect::<Vec<_>>();
                sessions.sort_by_key(|session| session.name != "local");
                rows.extend(
                    sessions
                        .into_iter()
                        .map(|session| TreeRow::Session { id: session.id }),
                );
            }
        }
        rows
    }

    pub fn move_selection(&mut self, delta: isize) {
        let rows = self.visible_rows();
        let ids = rows
            .iter()
            .filter_map(|row| match row {
                TreeRow::Session { id } => Some(*id),
                TreeRow::Project { .. } | TreeRow::Workspace { .. } => None,
            })
            .collect::<Vec<_>>();
        if ids.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .and_then(|selected| ids.iter().position(|id| *id == selected));
        let index = match current {
            Some(index) => (index as isize + delta).clamp(0, ids.len() as isize - 1) as usize,
            None if delta < 0 => ids.len() - 1,
            None => 0,
        };
        self.selected = Some(ids[index]);
        self.ensure_selection_visible(&rows);
    }

    pub fn toggle_selected_group(&mut self) {
        let Some(selected) = self.selected else {
            return;
        };
        for project in &self.hierarchy.projects {
            for workspace in &project.workspaces {
                if workspace
                    .sessions
                    .iter()
                    .any(|session| session.id == selected)
                {
                    let key = (project.name.clone(), workspace.name.clone());
                    if !self.collapsed_workspaces.remove(&key) {
                        self.collapsed_workspaces.insert(key);
                    }
                    return;
                }
            }
        }
    }

    pub fn select_session(&mut self, id: SessionId) {
        self.selected = Some(id);
        let rows = self.visible_rows();
        self.ensure_selection_visible(&rows);
    }

    fn tree_viewport_height(&self) -> usize {
        usize::from(self.pane_size.rows)
            .saturating_add(usize::from(METADATA_HEIGHT))
            .max(1)
    }

    fn ensure_selection_visible(&mut self, rows: &[TreeRow]) {
        let Some(selected) = self.selected else {
            return;
        };
        let Some(index) = rows
            .iter()
            .position(|row| *row == TreeRow::Session { id: selected })
        else {
            return;
        };
        let height = self.tree_viewport_height();
        let selected_start = rows
            .iter()
            .enumerate()
            .take(index)
            .map(|(index, row)| tree_row_gap(rows, index) + tree_row_height(row))
            .sum::<usize>()
            + tree_row_gap(rows, index);
        let selected_end = selected_start.saturating_add(tree_row_height(&rows[index]));
        if selected_start < self.tree_offset {
            self.tree_offset = selected_start;
        } else if selected_end > self.tree_offset.saturating_add(height) {
            self.tree_offset = selected_end.saturating_sub(height);
        }
        let max_offset = tree_line_count(rows).saturating_sub(height);
        self.tree_offset = self.tree_offset.min(max_offset);
    }

    pub fn key_action(&mut self, key: KeyEvent) -> DashboardAction {
        match self.mode {
            InputMode::Browse => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return DashboardAction::None;
                }
                match key.code {
                    KeyCode::Char('q') => DashboardAction::Detach,
                    KeyCode::Char('p') => self.pause_request(true),
                    KeyCode::Char('r') => self.pause_request(false),
                    KeyCode::Enter => {
                        if self.selected_phase() == Some(&SessionPhase::Paused) {
                            self.error = Some("Session paused; press r to resume".into());
                            DashboardAction::Redraw
                        } else {
                            self.mode = InputMode::Terminal;
                            DashboardAction::Redraw
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.move_selection(1);
                        self.request_selected()
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.move_selection(-1);
                        self.request_selected()
                    }
                    _ => DashboardAction::None,
                }
            }
            InputMode::Terminal => {
                if is_browse_key(key) {
                    self.mode = InputMode::Browse;
                    DashboardAction::EnterBrowse
                } else if !self.input_is_allowed() {
                    self.refuse_input()
                } else {
                    match encode_key(key, self.parser.screen().application_cursor()) {
                        KeyEncoding::Bytes(bytes) => DashboardAction::PtyBytes(bytes),
                        KeyEncoding::Browse | KeyEncoding::Ignore => DashboardAction::None,
                    }
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DashboardAction {
        self.key_action(key)
    }

    pub fn key(&mut self, code: KeyCode) -> DashboardAction {
        self.key_action(KeyEvent::new(code, KeyModifiers::NONE))
    }

    pub fn ctrl(&mut self, code: char) -> DashboardAction {
        self.key_action(KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL))
    }

    pub fn event_action(&mut self, event: Event) -> DashboardAction {
        match event {
            Event::Key(key) => self.key_action(key),
            Event::Paste(text) if self.mode == InputMode::Terminal => {
                if !self.input_is_allowed() {
                    self.refuse_input()
                } else {
                    DashboardAction::PtyBytes(encode_paste(
                        &text,
                        self.parser.screen().bracketed_paste(),
                    ))
                }
            }
            Event::Mouse(mouse) => self.mouse_action(mouse, Rect::new(0, 0, 0, 0)),
            Event::Resize(_, _) | Event::FocusGained | Event::FocusLost => DashboardAction::Redraw,
            Event::Paste(_) => DashboardAction::None,
        }
    }

    pub fn mouse_action(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        if self.mode == InputMode::Terminal || mouse.kind != MouseEventKind::Down(MouseButton::Left)
        {
            return DashboardAction::None;
        }
        let sidebar = sidebar_area(area);
        if mouse.column < sidebar.x
            || mouse.column >= sidebar.x.saturating_add(sidebar.width)
            || mouse.row < sidebar.y
            || mouse.row >= sidebar.y.saturating_add(sidebar.height)
        {
            return DashboardAction::None;
        }
        let row_index = self.tree_offset + usize::from(mouse.row.saturating_sub(sidebar.y));
        let rows = self.visible_rows();
        let Some((row, _)) = tree_line_at(&rows, row_index) else {
            return DashboardAction::None;
        };
        match row.clone() {
            TreeRow::Session { id } => {
                self.select_session(id);
                self.request_selected()
            }
            TreeRow::Project { name } => {
                if !self.collapsed_projects.remove(&name) {
                    self.collapsed_projects.insert(name);
                }
                DashboardAction::Redraw
            }
            TreeRow::Workspace { project, name } => {
                let key = (project, name);
                if !self.collapsed_workspaces.remove(&key) {
                    self.collapsed_workspaces.insert(key);
                }
                DashboardAction::Redraw
            }
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        self.mouse_action(mouse, area)
    }

    fn request_selected(&mut self) -> DashboardAction {
        let Some(session) = self.selected else {
            return DashboardAction::Redraw;
        };
        let request_id = self.next_request_id();
        DashboardAction::Request(self.select_request(session, request_id))
    }

    fn pause_request(&mut self, paused: bool) -> DashboardAction {
        let Some(session) = self.selected.and_then(|id| find_session(self, id)) else {
            self.error = Some("No session selected".into());
            return DashboardAction::Redraw;
        };
        if matches!(session.phase, SessionPhase::Exited { .. }) {
            self.error = Some("Session exited".into());
            return DashboardAction::Redraw;
        }
        let session = session.id;
        let request_id = self.next_request_id();
        DashboardAction::Request(ClientMessage {
            request_id,
            request: if paused {
                Request::PauseSession { session }
            } else {
                Request::ResumeSession { session }
            },
        })
    }

    pub fn handle_server_message(&mut self, message: ServerMessage) -> Vec<ClientMessage> {
        match message {
            ServerMessage::Response { response, .. } => match response {
                Response::Hierarchy(hierarchy) => {
                    self.hierarchy = hierarchy;
                    self.update_mode_for_selected_phase();
                }
                Response::Screen {
                    session,
                    size,
                    bytes,
                } => {
                    if self.selected == Some(session) {
                        self.pane_size = size;
                        self.parser = vt100::Parser::new(size.rows, size.cols, 0);
                        self.parser.process(&bytes);
                    }
                }
                Response::Ok => self.error = None,
                Response::CreatedSession(_)
                | Response::Inventory { .. }
                | Response::TerminalText { .. } => self.error = None,
                Response::Error { code, message } => {
                    self.error = Some(format!("{code:?}: {message}"));
                }
            },
            ServerMessage::Event(event) => match event {
                ServerEvent::HierarchyChanged(hierarchy) => {
                    self.hierarchy = hierarchy;
                    self.update_mode_for_selected_phase();
                }
                ServerEvent::Output { session, bytes } if self.selected == Some(session) => {
                    self.parser.process(&bytes);
                }
                ServerEvent::ScreenDirty { session } if self.selected == Some(session) => {
                    let request_id = self.next_request_id();
                    return vec![self.select_request(session, request_id)];
                }
                ServerEvent::SessionChanged(summary) => {
                    for session in self
                        .hierarchy
                        .projects
                        .iter_mut()
                        .flat_map(|project| project.workspaces.iter_mut())
                        .flat_map(|workspace| workspace.sessions.iter_mut())
                    {
                        if session.id == summary.id {
                            *session = summary;
                            break;
                        }
                    }
                    self.update_mode_for_selected_phase();
                }
                ServerEvent::Output { .. } | ServerEvent::ScreenDirty { .. } => {}
            },
        }
        Vec::new()
    }

    pub fn select_request(&mut self, id: SessionId, request_id: u64) -> ClientMessage {
        self.selected = Some(id);
        ClientMessage {
            request_id,
            request: Request::Select {
                session: id,
                size: self.pane_size,
            },
        }
    }

    pub fn resize_request(&mut self, size: TerminalSize, request_id: u64) -> Option<ClientMessage> {
        if size == self.pane_size {
            return None;
        }
        self.pane_size = size;
        self.parser.screen_mut().set_size(size.rows, size.cols);
        let rows = self.visible_rows();
        self.ensure_selection_visible(&rows);
        self.selected.map(|session| ClientMessage {
            request_id,
            request: Request::Resize { session, size },
        })
    }

    pub fn input_request(&self, bytes: Vec<u8>, request_id: u64) -> Option<ClientMessage> {
        if !self.input_is_allowed() {
            return None;
        }
        self.selected.map(|session| ClientMessage {
            request_id,
            request: Request::Input { session, bytes },
        })
    }

    fn selected_phase(&self) -> Option<&SessionPhase> {
        self.selected
            .and_then(|id| find_session(self, id))
            .map(|session| &session.phase)
    }

    fn input_is_allowed(&self) -> bool {
        matches!(self.selected_phase(), Some(SessionPhase::Running))
    }

    fn refuse_input(&mut self) -> DashboardAction {
        self.error = Some(match self.selected_phase() {
            Some(SessionPhase::Paused) => "Session paused; press r to resume".into(),
            Some(SessionPhase::Exited { .. }) => "Session exited".into(),
            None => "No session selected".into(),
            Some(SessionPhase::Running) => return DashboardAction::None,
        });
        DashboardAction::Redraw
    }

    fn update_mode_for_selected_phase(&mut self) {
        if self.selected_phase() == Some(&SessionPhase::Paused) {
            self.mode = InputMode::Browse;
        }
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        id
    }
}

pub fn encode_key(event: KeyEvent, application_cursor: bool) -> KeyEncoding {
    if is_browse_key(event) {
        return KeyEncoding::Browse;
    }

    let bytes = match event.code {
        KeyCode::Char(ch) if event.modifiers.contains(KeyModifiers::CONTROL) => {
            let ch = ch.to_ascii_lowercase();
            if ch.is_ascii_lowercase() {
                vec![ch as u8 - b'a' + 1]
            } else {
                return KeyEncoding::Ignore;
            }
        }
        KeyCode::Char(ch) => {
            let mut bytes = Vec::new();
            if event.modifiers.contains(KeyModifiers::ALT) {
                bytes.push(0x1b);
            }
            let mut encoded = [0; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            bytes
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Left => cursor_sequence(application_cursor, b'D'),
        KeyCode::Right => cursor_sequence(application_cursor, b'C'),
        KeyCode::Up => cursor_sequence(application_cursor, b'A'),
        KeyCode::Down => cursor_sequence(application_cursor, b'B'),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::F(number) => {
            let Some(bytes) = function_key(number) else {
                return KeyEncoding::Ignore;
            };
            bytes
        }
        _ => return KeyEncoding::Ignore,
    };
    KeyEncoding::Bytes(bytes)
}

fn is_browse_key(key: KeyEvent) -> bool {
    (key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('g' | 'G')))
        || matches!(key.code, KeyCode::Char('\u{7}'))
}

fn cursor_sequence(application: bool, suffix: u8) -> Vec<u8> {
    if application {
        vec![0x1b, b'O', suffix]
    } else {
        vec![0x1b, b'[', suffix]
    }
}

fn function_key(number: u8) -> Option<Vec<u8>> {
    Some(match number {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => return None,
    })
}

pub fn event_to_request(
    dashboard: &mut Dashboard,
    event: Event,
    request_id: u64,
) -> Option<ClientMessage> {
    if dashboard.mode != InputMode::Terminal {
        return None;
    }
    match event {
        Event::Paste(text) => dashboard.input_request(
            encode_paste(&text, dashboard.parser.screen().bracketed_paste()),
            request_id,
        ),
        Event::Key(key) => match encode_key(key, dashboard.parser.screen().application_cursor()) {
            KeyEncoding::Browse => {
                dashboard.mode = InputMode::Browse;
                None
            }
            KeyEncoding::Bytes(bytes) => dashboard.input_request(bytes, request_id),
            KeyEncoding::Ignore => None,
        },
        _ => None,
    }
}

pub fn dashboard_message_channel() -> (
    mpsc::SyncSender<ServerMessage>,
    mpsc::Receiver<ServerMessage>,
) {
    mpsc::sync_channel(DASHBOARD_READER_QUEUE_CAPACITY)
}

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

pub struct TerminalGuard<W: Write = io::Stdout> {
    writer: W,
    raw: bool,
    alternate: bool,
    mouse: bool,
    cursor_hidden: bool,
    bracketed_paste: bool,
}

impl TerminalGuard<io::Stdout> {
    pub fn enter() -> Result<Self> {
        let mut guard = Self {
            writer: io::stdout(),
            raw: false,
            alternate: false,
            mouse: false,
            cursor_hidden: false,
            bracketed_paste: false,
        };
        crossterm_terminal::enable_raw_mode().context("enable raw terminal mode")?;
        guard.raw = true;
        guard.enter_modes()?;
        Ok(guard)
    }
}

impl<W: Write> TerminalGuard<W> {
    pub fn enter_with_writer(writer: W) -> Result<Self> {
        let mut guard = Self::with_writer(writer);
        guard.enter_modes()?;
        Ok(guard)
    }

    pub fn with_writer(writer: W) -> Self {
        Self {
            writer,
            raw: false,
            alternate: false,
            mouse: false,
            cursor_hidden: false,
            bracketed_paste: false,
        }
    }

    pub fn writer_mut(&mut self) -> &mut W {
        &mut self.writer
    }

    fn enter_modes(&mut self) -> Result<()> {
        execute!(self.writer, crossterm_terminal::EnterAlternateScreen)
            .context("enter alternate screen")?;
        self.alternate = true;
        execute!(self.writer, EnableBracketedPaste).context("enable bracketed paste")?;
        self.bracketed_paste = true;
        execute!(self.writer, EnableMouseCapture).context("enable dashboard mouse")?;
        self.mouse = true;
        execute!(self.writer, cursor::Hide).context("hide dashboard cursor")?;
        self.cursor_hidden = true;
        Ok(())
    }

    pub fn restore_before<F: FnOnce()>(&mut self, prior: F) {
        self.restore();
        prior();
    }

    fn mark_mouse(&mut self, enabled: bool) {
        self.mouse = enabled;
    }

    fn restore(&mut self) {
        if self.mouse {
            let _ = execute!(self.writer, DisableMouseCapture);
            self.mouse = false;
        }
        if self.bracketed_paste {
            let _ = execute!(self.writer, DisableBracketedPaste);
            self.bracketed_paste = false;
        }
        if self.cursor_hidden {
            let _ = execute!(self.writer, cursor::Show);
            self.cursor_hidden = false;
        }
        if self.alternate {
            let _ = execute!(self.writer, crossterm_terminal::LeaveAlternateScreen);
            self.alternate = false;
        }
        if self.raw {
            let _ = crossterm_terminal::disable_raw_mode();
            self.raw = false;
        }
    }
}

impl<W: Write> Drop for TerminalGuard<W> {
    fn drop(&mut self) {
        let restored_by_panic_hook = PANIC_TERMINAL_RESTORED.with(|restored| {
            let was_restored = restored.get();
            restored.set(false);
            was_restored
        });
        if !restored_by_panic_hook {
            self.restore();
        }
    }
}

pub fn run_dashboard(mut stream: UnixStream) -> Result<()> {
    let size = terminal_size()?;
    let pane_size = pane_size(size);
    write_client(&mut stream, 1, Request::DashboardHello)?;
    let initial = read_server(&mut stream)?;
    dashboard_hello_result(&initial)?;
    let mut dashboard = Dashboard::new(pane_size);
    dashboard.handle_server_message(initial);
    write_client(
        &mut stream,
        2,
        Request::DashboardGeometry { size: pane_size },
    )?;
    let geometry_ack = read_server(&mut stream)?;
    dashboard.handle_server_message(geometry_ack);
    let first_session = dashboard
        .visible_rows()
        .into_iter()
        .find_map(|row| match row {
            TreeRow::Session { id } => Some(id),
            TreeRow::Project { .. } | TreeRow::Workspace { .. } => None,
        });
    if let Some(id) = first_session {
        write_client(&mut stream, 3, dashboard.select_request(id, 3).request)?;
        if let Ok(message) = read_server(&mut stream) {
            dashboard.handle_server_message(message);
        }
    }

    let mut guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(guard.writer_mut());
    let mut terminal = Terminal::new(backend).context("create dashboard terminal")?;
    // Initialize crossterm's singleton event source before adding the
    // dashboard's SIGWINCH self-pipe handler.
    let _ = event::poll(DASHBOARD_EVENT_PROBE)?;
    let reader_stream = stream.try_clone().context("clone dashboard socket")?;
    let input = dashboard_input()?.context("dashboard input terminal is unavailable")?;
    let (wake, reader_wake) = DashboardWake::new()?;
    let (received, messages) = dashboard_message_channel();
    thread::Builder::new()
        .name("ovrcr-dashboard-reader".into())
        .spawn(move || read_messages(reader_stream, received, reader_wake))?;
    let prior_hook = Arc::new(Mutex::new(Some(panic::take_hook())));
    let hook_prior = Arc::clone(&prior_hook);
    panic::set_hook(Box::new(move |panic_info| {
        let mut cleanup = TerminalGuard::with_writer(io::stdout());
        cleanup.raw = true;
        cleanup.alternate = true;
        cleanup.mouse = true;
        cleanup.cursor_hidden = true;
        cleanup.bracketed_paste = true;
        cleanup.restore_before(|| {
            PANIC_TERMINAL_RESTORED.with(|restored| restored.set(true));
            if let Some(prior) = hook_prior.lock().ok().and_then(|mut hooks| hooks.take()) {
                prior(panic_info);
            }
        });
    }));
    let result = dashboard_loop(
        &mut terminal,
        &mut stream,
        &mut dashboard,
        &messages,
        input,
        wake,
    );
    drop(terminal);
    guard.mark_mouse(dashboard.mode == InputMode::Browse);
    let _ = panic::take_hook();
    if let Some(prior) = prior_hook.lock().ok().and_then(|mut hooks| hooks.take()) {
        panic::set_hook(prior);
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
    result
}

fn dashboard_loop<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    messages: &mpsc::Receiver<ServerMessage>,
    input: File,
    mut wake: DashboardWake,
) -> Result<()> {
    let mut mouse_enabled = dashboard.mode == InputMode::Browse;
    let input_fd = input.as_raw_fd();
    let mut next_idle_redraw = Instant::now() + dashboard.redraw_interval();
    let mut next_frame_redraw = Instant::now();
    let mut pending_redraw = false;
    let mut first_frame = true;
    loop {
        let outer = terminal.size()?;
        let next_size = pane_size(TerminalSize {
            rows: outer.height,
            cols: outer.width,
        });
        let request_id = dashboard.next_request_id();
        if let Some(request) = dashboard.resize_request(next_size, request_id) {
            write_frame(stream, &request)?;
        }
        wake.clear()?;
        pending_redraw |= next_dashboard_messages(messages, dashboard, stream)?;
        pending_redraw |= first_frame;
        first_frame = false;

        // Crossterm may already have parsed an event while reading an escape
        // sequence. Check its buffered event queue before polling the raw fd.
        if event::poll(DASHBOARD_EVENT_PROBE)? {
            if process_dashboard_input(terminal, stream, dashboard, &mut mouse_enabled)? {
                break;
            }
            pending_redraw = true;
            pending_redraw |= next_dashboard_messages(messages, dashboard, stream)?;
        }
        if pending_redraw && Instant::now() >= next_frame_redraw {
            update_mouse_capture(terminal, dashboard, &mut mouse_enabled)?;
            terminal.draw(|frame| draw_dashboard(frame, dashboard))?;
            pending_redraw = false;
            next_frame_redraw = Instant::now() + DASHBOARD_FRAME_INTERVAL;
            next_idle_redraw = Instant::now() + dashboard.redraw_interval();
            continue;
        }

        {
            let now = Instant::now();
            let idle_wait = next_idle_redraw.saturating_duration_since(now);
            let frame_wait = next_frame_redraw.saturating_duration_since(now);
            let wait = if pending_redraw && frame_wait < idle_wait {
                wait_for_dashboard_activity(input_fd, None, frame_wait)?
            } else {
                wait_for_dashboard_activity(input_fd, Some(wake.receiver()), idle_wait)?
            };
            // A terminal resize and keyboard input can become ready together.
            // Send the new PTY geometry before forwarding that input.
            let outer = terminal.size()?;
            let next_size = pane_size(TerminalSize {
                rows: outer.height,
                cols: outer.width,
            });
            let request_id = dashboard.next_request_id();
            if let Some(request) = dashboard.resize_request(next_size, request_id) {
                write_frame(stream, &request)?;
            }
            // If both are ready, process keyboard input first so output floods
            // cannot delay a user's command.
            // Raw descriptor readiness can represent an incomplete escape
            // sequence. Let crossterm confirm a complete event before read().
            let input_ready = if wait.input_ready {
                // The raw descriptor can wake for the first byte of an
                // escape sequence. Give crossterm a short bounded window to
                // finish parsing it before deciding whether read() is safe.
                event::poll(DASHBOARD_EVENT_PROBE)?
            } else if wait.server_ready || wait.timed_out {
                event::poll(DASHBOARD_EVENT_PROBE)?
            } else {
                false
            };
            if input_ready
                && process_dashboard_input(terminal, stream, dashboard, &mut mouse_enabled)?
            {
                break;
            }
            if input_ready {
                let outer = terminal.size()?;
                let next_size = pane_size(TerminalSize {
                    rows: outer.height,
                    cols: outer.width,
                });
                let request_id = dashboard.next_request_id();
                if let Some(request) = dashboard.resize_request(next_size, request_id) {
                    write_frame(stream, &request)?;
                }
            }
            if input_ready {
                pending_redraw = true;
                for _ in 1..DASHBOARD_INPUT_BATCH_LIMIT {
                    if !event::poll(DASHBOARD_EVENT_PROBE)? {
                        break;
                    }
                    if process_dashboard_input(terminal, stream, dashboard, &mut mouse_enabled)? {
                        return Ok(());
                    }
                }
            }
            if wait.server_ready {
                wake.clear()?;
                pending_redraw |= next_dashboard_messages(messages, dashboard, stream)?;
            }
            if wait.timed_out {
                pending_redraw = true;
            }
        }
        update_mouse_capture(terminal, dashboard, &mut mouse_enabled)?;
        if pending_redraw && Instant::now() >= next_frame_redraw {
            // Output may have arrived while the frame wait ignored the server
            // wake. Clear before draining so a producer racing this drain
            // leaves a wake for the next iteration.
            wake.clear()?;
            next_dashboard_messages(messages, dashboard, stream)?;
            terminal.draw(|frame| draw_dashboard(frame, dashboard))?;
            pending_redraw = false;
            next_frame_redraw = Instant::now() + DASHBOARD_FRAME_INTERVAL;
            next_idle_redraw = Instant::now() + dashboard.redraw_interval();
        }
    }
    Ok(())
}

fn update_mouse_capture<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    dashboard: &Dashboard,
    mouse_enabled: &mut bool,
) -> Result<()> {
    if dashboard.mode == InputMode::Terminal {
        if *mouse_enabled {
            execute!(terminal.backend_mut(), DisableMouseCapture)
                .context("disable dashboard mouse")?;
            *mouse_enabled = false;
        }
    } else if !*mouse_enabled {
        execute!(terminal.backend_mut(), EnableMouseCapture).context("enable dashboard mouse")?;
        *mouse_enabled = true;
    }
    Ok(())
}

fn dashboard_hello_result(message: &ServerMessage) -> Result<()> {
    if let ServerMessage::Response {
        response: Response::Error { code, message },
        ..
    } = message
    {
        bail!("dashboard hello failed ({code:?}): {message}");
    }
    Ok(())
}

fn next_dashboard_message(
    messages: &mpsc::Receiver<ServerMessage>,
) -> Result<Option<ServerMessage>> {
    match messages.try_recv() {
        Ok(message) => Ok(Some(message)),
        Err(mpsc::TryRecvError::Empty) => Ok(None),
        Err(mpsc::TryRecvError::Disconnected) => Err(anyhow::anyhow!("dashboard connection lost")),
    }
}

fn next_dashboard_messages(
    messages: &mpsc::Receiver<ServerMessage>,
    dashboard: &mut Dashboard,
    stream: &mut UnixStream,
) -> Result<bool> {
    let mut redraw = false;
    for _ in 0..DASHBOARD_READER_QUEUE_CAPACITY {
        let Some(message) = next_dashboard_message(messages)? else {
            break;
        };
        redraw = true;
        for request in dashboard.handle_server_message(message) {
            write_frame(stream, &request)?;
        }
    }
    Ok(redraw)
}

fn process_dashboard_input<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    mouse_enabled: &mut bool,
) -> Result<bool> {
    let event = event::read()?;
    let action = match event {
        Event::Mouse(mouse) => {
            let area = terminal.size()?.into();
            dashboard.mouse_action(mouse, area)
        }
        event @ (Event::Key(_) | Event::Paste(_)) => dashboard.event_action(event),
        Event::Resize(_, _) => DashboardAction::Redraw,
        Event::FocusGained | Event::FocusLost => DashboardAction::None,
    };
    match action {
        DashboardAction::None | DashboardAction::Redraw => Ok(false),
        DashboardAction::Detach => Ok(true),
        DashboardAction::EnterBrowse => {
            if !*mouse_enabled {
                execute!(terminal.backend_mut(), EnableMouseCapture)
                    .context("enable dashboard mouse")?;
                *mouse_enabled = true;
            }
            Ok(false)
        }
        DashboardAction::PtyBytes(bytes) => {
            let request_id = dashboard.next_request_id();
            if let Some(request) = dashboard.input_request(bytes, request_id) {
                write_frame(stream, &request)?;
            }
            Ok(false)
        }
        DashboardAction::Request(request) => {
            write_frame(stream, &request)?;
            Ok(false)
        }
    }
}

struct DashboardActivity {
    input_ready: bool,
    server_ready: bool,
    timed_out: bool,
}

struct DashboardWake {
    receiver: UnixStream,
    sigwinch: signal_hook::SigId,
}

impl DashboardWake {
    fn new() -> Result<(Self, UnixStream)> {
        let (receiver, sender) = UnixStream::pair().context("create dashboard wake pair")?;
        receiver
            .set_nonblocking(true)
            .context("set dashboard wake receiver nonblocking")?;
        sender
            .set_nonblocking(true)
            .context("set dashboard wake sender nonblocking")?;
        let sigwinch = signal_hook::low_level::pipe::register(libc::SIGWINCH, sender.try_clone()?)
            .context("register dashboard resize wake")?;
        Ok((Self { receiver, sigwinch }, sender))
    }

    fn receiver(&self) -> &UnixStream {
        &self.receiver
    }

    fn clear(&mut self) -> Result<()> {
        let mut bytes = [0_u8; 128];
        loop {
            match (&self.receiver).read(&mut bytes) {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error).context("drain dashboard wake")?,
            }
        }
    }
}

impl Drop for DashboardWake {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.sigwinch);
    }
}

fn notify_dashboard_wake(wake: &mut UnixStream) {
    match wake.write(&[1]) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        Err(error) if error.kind() == io::ErrorKind::Interrupted => notify_dashboard_wake(wake),
        Err(_) => {}
    }
}

fn wait_for_dashboard_activity(
    input_fd: RawFd,
    wake: Option<&UnixStream>,
    timeout: Duration,
) -> Result<DashboardActivity> {
    let timeout_ms = timeout
        .as_millis()
        .saturating_add(u128::from(!timeout.is_zero()))
        .min(i32::MAX as u128) as libc::c_int;
    let wake_fd = wake.map_or(-1, UnixStream::as_raw_fd);
    let mut fds = [
        libc::pollfd {
            fd: input_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let result = {
        // SAFETY: `fds` points to two valid pollfd values for the duration of
        // the call. A negative input fd is intentionally ignored by poll.
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            return Ok(DashboardActivity {
                input_ready: false,
                server_ready: false,
                timed_out: true,
            });
        }
        result
    };
    if result < 0 {
        return Err(io::Error::last_os_error()).context("poll dashboard input and output");
    }
    if fds.iter().any(|fd| fd.revents & libc::POLLNVAL != 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "dashboard poll descriptor is invalid",
        ))
        .context("poll dashboard input and output");
    }
    Ok(DashboardActivity {
        input_ready: fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0,
        server_ready: fds[1].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0,
        timed_out: result == 0,
    })
}

fn dashboard_input() -> Result<Option<File>> {
    let input = if unsafe { libc::isatty(libc::STDIN_FILENO) == 1 } {
        File::open("/dev/stdin").context("open dashboard stdin")?
    } else {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("open dashboard controlling terminal")?
    };
    Ok(Some(input))
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

fn sidebar_area(area: Rect) -> Rect {
    dashboard_layout(area).sidebar_content
}

fn tree_row_height(row: &TreeRow) -> usize {
    match row {
        TreeRow::Session { .. } => 3,
        TreeRow::Project { .. } | TreeRow::Workspace { .. } => 1,
    }
}

fn tree_line_count(rows: &[TreeRow]) -> usize {
    rows.iter()
        .enumerate()
        .map(|(index, row)| tree_row_gap(rows, index) + tree_row_height(row))
        .sum()
}

fn tree_row_gap(rows: &[TreeRow], index: usize) -> usize {
    usize::from(
        index > 0
            && match &rows[index] {
                TreeRow::Project { .. } => true,
                TreeRow::Workspace { .. } => !matches!(rows[index - 1], TreeRow::Project { .. }),
                TreeRow::Session { .. } => false,
            },
    )
}

fn tree_line_at(rows: &[TreeRow], line: usize) -> Option<(&TreeRow, usize)> {
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

fn find_session(dashboard: &Dashboard, id: SessionId) -> Option<&crate::session::SessionSummary> {
    dashboard
        .hierarchy
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .find(|session| session.id == id)
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

fn read_messages(
    mut stream: UnixStream,
    sender: mpsc::SyncSender<ServerMessage>,
    mut wake: UnixStream,
) {
    while let Ok(message) = read_server(&mut stream) {
        if sender.send(message).is_err() {
            break;
        }
        notify_dashboard_wake(&mut wake);
    }
    drop(sender);
    notify_dashboard_wake(&mut wake);
}

fn read_server(stream: &mut UnixStream) -> Result<ServerMessage> {
    crate::protocol::read_frame(stream)
}

fn write_client(stream: &mut UnixStream, request_id: u64, request: Request) -> Result<()> {
    crate::protocol::write_frame(
        stream,
        &ClientMessage {
            request_id,
            request,
        },
    )
}

fn write_frame(stream: &mut UnixStream, message: &ClientMessage) -> Result<()> {
    crate::protocol::write_frame(stream, message)
}

fn terminal_size() -> Result<TerminalSize> {
    let size = crossterm_terminal::size().context("read terminal size")?;
    Ok(TerminalSize {
        rows: size.1.max(1),
        cols: size.0.max(1),
    })
}

pub fn actual_drawn_inner_rect(area: Rect) -> Rect {
    dashboard_layout(area).terminal
}

fn pane_size(size: TerminalSize) -> TerminalSize {
    let inner = actual_drawn_inner_rect(Rect::new(0, 0, size.cols, size.rows));
    TerminalSize {
        rows: inner.height.max(1),
        cols: inner.width.max(1),
    }
}

#[cfg(test)]
mod tests {
    use super::{dashboard_hello_result, dashboard_message_channel, next_dashboard_message};
    use crate::protocol::{ErrorCode, Response, ServerMessage};
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn dashboard_surfaces_hello_refusal_and_reader_disconnect() {
        let refusal = ServerMessage::Response {
            request_id: 1,
            response: Response::Error {
                code: ErrorCode::Conflict,
                message: "another dashboard is already connected".into(),
            },
        };
        let error = dashboard_hello_result(&refusal).unwrap_err().to_string();
        assert!(error.contains("dashboard hello failed"));
        assert!(error.contains("another dashboard is already connected"));

        let (sender, receiver) = dashboard_message_channel();
        drop(sender);
        let error = next_dashboard_message(&receiver).unwrap_err().to_string();
        assert_eq!(error, "dashboard connection lost");
    }

    #[test]
    fn dashboard_output_wake_interrupts_idle_wait() {
        let (wake_receiver, mut wake_sender) = UnixStream::pair().unwrap();
        wake_receiver.set_nonblocking(true).unwrap();
        wake_sender.set_nonblocking(true).unwrap();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            wake_sender.write_all(&[1]).unwrap();
        });

        let started = Instant::now();
        let activity = super::wait_for_dashboard_activity(
            -1,
            Some(&wake_receiver),
            Duration::from_millis(500),
        )
        .unwrap();
        assert!(activity.server_ready);
        assert!(!activity.input_ready);
        assert!(started.elapsed() < Duration::from_millis(250));
    }

    #[test]
    fn dashboard_wait_reports_input_and_output_together() {
        let (input_receiver, mut input_sender) = UnixStream::pair().unwrap();
        let (wake_receiver, mut wake_sender) = UnixStream::pair().unwrap();
        input_receiver.set_nonblocking(true).unwrap();
        input_sender.set_nonblocking(true).unwrap();
        wake_receiver.set_nonblocking(true).unwrap();
        wake_sender.set_nonblocking(true).unwrap();
        input_sender.write_all(&[1]).unwrap();
        wake_sender.write_all(&[1]).unwrap();

        let activity = super::wait_for_dashboard_activity(
            input_receiver.as_raw_fd(),
            Some(&wake_receiver),
            Duration::from_millis(500),
        )
        .unwrap();
        assert!(activity.input_ready);
        assert!(activity.server_ready);
    }
}

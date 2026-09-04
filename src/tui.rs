use crate::protocol::{
    ClientMessage, HierarchySnapshot, Request, Response, ServerEvent, ServerMessage,
};
use crate::session::{SessionId, TerminalSize};
use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::{execute, terminal as crossterm_terminal};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use std::collections::HashSet;
use std::io;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub const DASHBOARD_READER_QUEUE_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Browse,
    Terminal,
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
            next_request_id: 1,
        }
    }

    pub fn handle_server_message(&mut self, message: ServerMessage) -> Vec<ClientMessage> {
        match message {
            ServerMessage::Response { response, .. } => match response {
                Response::Hierarchy(hierarchy) => self.hierarchy = hierarchy,
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
                Response::Ok | Response::CreatedSession(_) | Response::Error { .. } => {}
            },
            ServerMessage::Event(event) => match event {
                ServerEvent::HierarchyChanged(hierarchy) => self.hierarchy = hierarchy,
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
        self.selected.map(|session| ClientMessage {
            request_id,
            request: Request::Resize { session, size },
        })
    }

    pub fn input_request(&self, bytes: Vec<u8>, request_id: u64) -> Option<ClientMessage> {
        self.selected.map(|session| ClientMessage {
            request_id,
            request: Request::Input { session, bytes },
        })
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        id
    }
}

pub fn encode_key(event: KeyEvent, application_cursor: bool) -> KeyEncoding {
    if event.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(event.code, KeyCode::Char('g' | 'G'))
    {
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

pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let mut bytes = b"\x1b[200~".to_vec();
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(b"\x1b[201~");
        bytes
    } else {
        text.as_bytes().to_vec()
    }
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
            let (fg, bg) = (color(vt_cell.fgcolor()), color(vt_cell.bgcolor()));
            cell.set_symbol(vt_cell.contents()).set_fg(fg).set_bg(bg);
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

fn color(value: vt100::Color) -> Color {
    match value {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(index) => Color::Indexed(index),
        vt100::Color::Rgb(red, green, blue) => Color::Rgb(red, green, blue),
    }
}

pub fn run_dashboard(mut stream: UnixStream) -> Result<()> {
    write_client(&mut stream, 1, Request::DashboardHello)?;
    let initial = read_server(&mut stream)?;
    let size = terminal_size()?;
    let mut dashboard = Dashboard::new(pane_size(size));
    dashboard.handle_server_message(initial);
    let first_session = sessions(&dashboard.hierarchy).next();
    if let Some(id) = first_session {
        write_client(&mut stream, 2, dashboard.select_request(id, 2).request)?;
        if let Ok(message) = read_server(&mut stream) {
            dashboard.handle_server_message(message);
        }
    }

    let reader_stream = stream.try_clone().context("clone dashboard socket")?;
    let (received, messages) = dashboard_message_channel();
    thread::Builder::new()
        .name("ovrcr-dashboard-reader".into())
        .spawn(move || read_messages(reader_stream, received))?;

    crossterm_terminal::enable_raw_mode().context("enable raw terminal mode")?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        crossterm_terminal::EnterAlternateScreen,
        EnableBracketedPaste
    )
    .context("enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create dashboard terminal")?;
    let result = dashboard_loop(&mut terminal, &mut stream, &mut dashboard, &messages);
    let _ = stream.shutdown(std::net::Shutdown::Both);
    crossterm_terminal::disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        crossterm_terminal::LeaveAlternateScreen
    )
    .ok();
    result
}

fn dashboard_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    messages: &mpsc::Receiver<ServerMessage>,
) -> Result<()> {
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
        terminal.draw(|frame| draw_dashboard(frame, dashboard))?;
        while let Ok(message) = messages.try_recv() {
            for request in dashboard.handle_server_message(message) {
                write_frame(stream, &request)?;
            }
        }
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if dashboard.mode == InputMode::Browse => match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Enter => dashboard.mode = InputMode::Terminal,
                    KeyCode::Down | KeyCode::Char('j') => select_relative(dashboard, 1, stream)?,
                    KeyCode::Up | KeyCode::Char('k') => select_relative(dashboard, -1, stream)?,
                    _ => {}
                },
                event @ (Event::Key(_) | Event::Paste(_))
                    if dashboard.mode == InputMode::Terminal =>
                {
                    let request_id = dashboard.next_request_id();
                    if let Some(request) = event_to_request(dashboard, event, request_id) {
                        write_frame(stream, &request)?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn select_relative(dashboard: &mut Dashboard, delta: isize, stream: &mut UnixStream) -> Result<()> {
    let ids = sessions(&dashboard.hierarchy).collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let current = dashboard
        .selected
        .and_then(|selected| ids.iter().position(|id| *id == selected))
        .unwrap_or(0);
    let next = (current as isize + delta).clamp(0, ids.len() as isize - 1) as usize;
    let request_id = dashboard.next_request_id();
    write_frame(stream, &dashboard.select_request(ids[next], request_id))?;
    Ok(())
}

fn draw_dashboard(frame: &mut Frame<'_>, dashboard: &Dashboard) {
    let areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(1)])
        .split(frame.area());
    let sidebar = sessions(&dashboard.hierarchy)
        .map(|id| format!("session {}", id.0))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(sidebar.join("\n"))
            .block(Block::default().title("OVRCR").borders(Borders::ALL)),
        areas[0],
    );
    let title = dashboard
        .selected
        .map_or_else(|| "terminal".to_string(), |id| format!("session {}", id.0));
    frame.render_widget(
        Block::default()
            .title(Line::from(title))
            .borders(Borders::ALL),
        areas[1],
    );
    let terminal_area = Rect {
        x: areas[1].x + 1,
        y: areas[1].y + 1,
        width: areas[1].width.saturating_sub(2),
        height: areas[1].height.saturating_sub(2),
    };
    render_terminal(
        frame,
        terminal_area,
        dashboard.parser.screen(),
        dashboard.mode == InputMode::Terminal,
    );
}

fn sessions(hierarchy: &HierarchySnapshot) -> impl Iterator<Item = SessionId> + '_ {
    hierarchy
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .map(|session| session.id)
}

fn read_messages(mut stream: UnixStream, sender: mpsc::SyncSender<ServerMessage>) {
    while let Ok(message) = read_server(&mut stream) {
        if sender.send(message).is_err() {
            break;
        }
    }
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

fn pane_size(size: TerminalSize) -> TerminalSize {
    TerminalSize {
        rows: size.rows.saturating_sub(2).max(1),
        cols: size.cols.saturating_sub(32).max(1),
    }
}

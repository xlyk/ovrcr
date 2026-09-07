use super::event_loop::DASHBOARD_IDLE_REDRAW_INTERVAL;
use super::input::{encode_key, is_browse_key};
use super::render::{
    METADATA_HEIGHT, SPINNER_INTERVAL, sidebar_area, tree_line_at, tree_line_count, tree_row_gap,
    tree_row_height,
};
use super::{Dashboard, DashboardAction, InputMode, KeyEncoding, TreeRow};
use crate::protocol::{
    ClientMessage, HierarchySnapshot, Request, Response, ServerEvent, ServerMessage,
};
use crate::session::{SessionId, SessionPhase, TerminalSize};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr_terminal::encode_paste;
use ovrcr_terminal::vt100;
use ratatui::layout::Rect;
use std::collections::HashSet;
use std::time::Duration;

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

    pub(super) fn session_is_busy(&self, id: SessionId) -> bool {
        find_session(self, id).is_some_and(|session| {
            matches!(session.phase, crate::session::SessionPhase::Running)
                && session.activity == crate::session::AgentActivity::Busy
        })
    }

    pub(super) fn redraw_interval(&self) -> Duration {
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
                | Response::TerminalText { .. }
                | Response::HistoryOpened(_)
                | Response::HistoryRows(_) => self.error = None,
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

    pub(super) fn selected_phase(&self) -> Option<&SessionPhase> {
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

    pub(super) fn next_request_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        id
    }
}
pub(super) fn find_session(
    dashboard: &Dashboard,
    id: SessionId,
) -> Option<&crate::session::SessionSummary> {
    dashboard
        .hierarchy
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .find(|session| session.id == id)
}

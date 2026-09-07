use super::event_loop::DASHBOARD_IDLE_REDRAW_INTERVAL;
use super::input::{encode_key, is_browse_key};
use super::render::{
    METADATA_HEIGHT, SPINNER_INTERVAL, sidebar_area, tree_line_at, tree_line_count, tree_row_gap,
    tree_row_height,
};
use super::{Dashboard, DashboardAction, InputMode, KeyEncoding, TreeRow, history_view_size};
use crate::protocol::{
    ClientMessage, HierarchySnapshot, HistoryOpened, HistoryRows, HistorySnapshotId, Request,
    Response, ServerEvent, ServerMessage,
};
use crate::session::{SessionId, SessionPhase, TerminalSize};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr_terminal::encode_paste;
use ovrcr_terminal::vt100;
use ratatui::layout::Rect;
use std::collections::{HashSet, VecDeque};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingHistoryBegin {
    pub request_id: u64,
    pub session: SessionId,
    pub cancelled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingHistoryPage {
    pub request_id: u64,
    pub session: SessionId,
    pub snapshot: HistorySnapshotId,
    pub start_row: u32,
    pub rows: u16,
    pub start_col: u16,
    pub cols: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryView {
    pub opened: HistoryOpened,
    pub top: u32,
    pub left: u16,
    pub new_output: bool,
    pub pages: VecDeque<HistoryRows>,
    pub pending: Option<PendingHistoryPage>,
}

impl HistoryView {
    pub fn new(opened: HistoryOpened, top: u32) -> Self {
        Self {
            opened,
            top,
            left: 0,
            new_output: false,
            pages: VecDeque::new(),
            pending: None,
        }
    }

    pub fn accept_page(&mut self, request_id: u64, page: HistoryRows) -> bool {
        let Some(pending) = self.pending.as_ref() else {
            return false;
        };
        if pending.request_id != request_id
            || page.session != pending.session
            || page.snapshot != pending.snapshot
            || page.session != self.opened.session
            || page.snapshot != self.opened.snapshot
            || page.start_row != pending.start_row
            || page.start_col != pending.start_col
            || page.start_row > self.opened.total_rows
            || page.rows.len() > usize::from(pending.rows)
            || page.start_row.saturating_add(page.rows.len() as u32) > self.opened.total_rows
            || page.rows.iter().any(|row| {
                row.cells.len() > usize::from(pending.cols)
                    || (!row.cells.is_empty()
                        && (row.width < page.start_col
                            || row.width.saturating_sub(page.start_col)
                                < u16::try_from(row.cells.len()).unwrap_or(u16::MAX)))
            })
        {
            return false;
        }
        self.pending = None;
        if let Some(existing) = self.pages.iter_mut().find(|cached| {
            cached.snapshot == page.snapshot
                && cached.start_row == page.start_row
                && cached.start_col == page.start_col
        }) {
            *existing = page;
        } else {
            self.pages.push_back(page);
        }
        while self.pages.len() > 16 {
            self.pages.pop_front();
        }
        true
    }
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
            history: None,
            history_begin_request: None,
            tree_offset: 0,
            next_request_id: 1,
            history_page_error: false,
            history_end_after_selection: None,
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
        if self.selected != Some(ids[index]) {
            self.release_for_selection_change();
        }
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
        if self.selected != Some(id) {
            self.release_for_selection_change();
        }
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
                    KeyCode::Esc if self.history_begin_request.is_some() => {
                        if let Some(begin) = self.history_begin_request.as_mut() {
                            begin.cancelled = true;
                        }
                        DashboardAction::EnterBrowse
                    }
                    KeyCode::Char('q') => DashboardAction::Detach,
                    KeyCode::PageUp => self.begin_history_request(),
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
            InputMode::History => self.history_key_action(key),
        }
    }

    fn begin_history_request(&mut self) -> DashboardAction {
        let Some(session) = self.selected else {
            return DashboardAction::None;
        };
        if self.history.is_some() || self.history_begin_request.is_some() {
            return DashboardAction::Redraw;
        }
        self.history_page_error = false;
        self.error = None;
        let request_id = self.next_request_id();
        self.history_begin_request = Some(PendingHistoryBegin {
            request_id,
            session,
            cancelled: false,
        });
        DashboardAction::Request(ClientMessage {
            request_id,
            request: Request::HistoryBegin { session },
        })
    }

    fn history_key_action(&mut self, key: KeyEvent) -> DashboardAction {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('g' | 'G'))
            || matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        {
            return self.leave_history();
        }
        let Some(view) = self.history.as_mut() else {
            return DashboardAction::None;
        };
        let size = history_view_size(self.pane_size);
        let max_top = view.opened.total_rows.saturating_sub(u32::from(size.rows));
        let max_left = u32::from(u16::MAX)
            .saturating_sub(u32::from(size.cols))
            .min(u32::from(u16::MAX));
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                view.top = view.top.saturating_sub(1).min(max_top);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                view.top = view.top.saturating_add(1).min(max_top);
            }
            KeyCode::PageUp => {
                view.top = view.top.saturating_sub(u32::from(size.rows)).min(max_top);
            }
            KeyCode::PageDown => {
                view.top = view.top.saturating_add(u32::from(size.rows)).min(max_top);
            }
            KeyCode::Home => view.top = 0,
            KeyCode::End => view.top = max_top,
            KeyCode::Left | KeyCode::Char('h') => view.left = view.left.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l') => {
                view.left = view
                    .left
                    .saturating_add(1)
                    .min(u16::try_from(max_left).unwrap_or(u16::MAX));
            }
            KeyCode::Enter => return DashboardAction::None,
            _ => return DashboardAction::None,
        }
        self.history_page_error = false;
        self.error = None;
        self.history_request_if_needed()
            .map_or(DashboardAction::Redraw, DashboardAction::Request)
    }

    fn leave_history(&mut self) -> DashboardAction {
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        let end = self.history.take().map(|view| {
            let request_id = self.next_request_id();
            ClientMessage {
                request_id,
                request: Request::HistoryEnd {
                    session: view.opened.session,
                    snapshot: view.opened.snapshot,
                },
            }
        });
        self.mode = InputMode::Browse;
        self.history_end_after_selection = end;
        DashboardAction::EnterBrowse
    }

    fn release_for_selection_change(&mut self) {
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        if let Some(view) = self.history.take() {
            self.mode = InputMode::Browse;
            let request_id = self.next_request_id();
            self.history_end_after_selection = Some(ClientMessage {
                request_id,
                request: Request::HistoryEnd {
                    session: view.opened.session,
                    snapshot: view.opened.snapshot,
                },
            });
        }
    }

    pub(super) fn take_pending_history_end(&mut self) -> Option<ClientMessage> {
        self.history_end_after_selection.take()
    }

    pub(super) fn history_request_if_needed(&mut self) -> Option<ClientMessage> {
        if self.history_page_error {
            return None;
        }
        let (start_row, start_col, rows, cols, session, snapshot) = {
            let view = self.history.as_mut()?;
            if view.pending.is_some() {
                return None;
            }
            let size = history_view_size(self.pane_size);
            let row_end = view
                .top
                .saturating_add(u32::from(size.rows))
                .min(view.opened.total_rows);
            let col_end = u32::from(view.left).saturating_add(u32::from(size.cols));
            let first_row = (view.top / 16) * 16;
            let first_col = (u32::from(view.left) / 128) * 128;
            let mut candidate = None;
            let mut row_start = first_row;
            while row_start < row_end {
                let mut col_start = first_col;
                while col_start < col_end {
                    let start_col = u16::try_from(col_start).unwrap_or(u16::MAX);
                    let cached = view.pages.iter().any(|page| {
                        page.snapshot == view.opened.snapshot
                            && page.start_row == row_start
                            && page.start_col == start_col
                    });
                    if !cached {
                        let rows = view.opened.total_rows.saturating_sub(row_start).min(16);
                        let cols = u32::from(u16::MAX).saturating_sub(col_start).min(128);
                        if rows > 0 && cols > 0 {
                            candidate = Some((row_start, start_col, rows as u16, cols as u16));
                            break;
                        }
                    }
                    col_start = col_start.saturating_add(128);
                }
                if candidate.is_some() {
                    break;
                }
                let Some(next_row) = row_start.checked_add(16) else {
                    break;
                };
                row_start = next_row;
            }
            let (start_row, start_col, rows, cols) = candidate?;
            (
                start_row,
                start_col,
                rows,
                cols,
                view.opened.session,
                view.opened.snapshot,
            )
        };
        let request_id = self.next_request_id();
        let pending = PendingHistoryPage {
            request_id,
            session,
            snapshot,
            start_row,
            rows,
            start_col,
            cols,
        };
        let view = self.history.as_mut()?;
        view.pending = Some(pending.clone());
        Some(ClientMessage {
            request_id,
            request: Request::HistoryPage {
                session: pending.session,
                snapshot: pending.snapshot,
                start_row,
                rows,
                start_col,
                cols,
            },
        })
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
        let select = self.select_request(session, request_id);
        if let Some(end) = self.history_end_after_selection.take() {
            DashboardAction::RequestBatch(vec![end, select])
        } else {
            DashboardAction::Request(select)
        }
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
        let mut outgoing = Vec::new();
        match message {
            ServerMessage::Response {
                request_id,
                response,
            } => match response {
                Response::Hierarchy(hierarchy) => {
                    self.hierarchy = hierarchy;
                    self.update_mode_for_selected_phase();
                    if self
                        .selected
                        .is_some_and(|selected| find_session(self, selected).is_none())
                    {
                        self.release_for_selection_change();
                        if let Some(request) = self.take_pending_history_end() {
                            outgoing.push(request);
                        }
                    }
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
                        if let Some(view) = self.history.as_mut() {
                            if view.opened.session == session {
                                view.new_output = true;
                            }
                        }
                    }
                }
                Response::Ok if !self.history_page_error => self.error = None,
                Response::Ok => {}
                Response::CreatedSession(_)
                | Response::Inventory { .. }
                | Response::TerminalText { .. } => self.error = None,
                Response::HistoryOpened(opened) => {
                    self.accept_history_opened(request_id, opened, &mut outgoing);
                }
                Response::HistoryRows(page) => {
                    let accepted = self
                        .history
                        .as_mut()
                        .is_some_and(|view| view.accept_page(request_id, page));
                    if accepted {
                        if let Some(request) = self.history_request_if_needed() {
                            outgoing.push(request);
                        }
                    }
                }
                Response::Error { code, message } => {
                    let mut matched_history = false;
                    if self
                        .history_begin_request
                        .as_ref()
                        .is_some_and(|pending| pending.request_id == request_id)
                    {
                        self.history_begin_request = None;
                        matched_history = true;
                    }
                    if self
                        .history
                        .as_ref()
                        .and_then(|view| view.pending.as_ref())
                        .is_some_and(|pending| pending.request_id == request_id)
                    {
                        if let Some(view) = self.history.as_mut() {
                            view.pending = None;
                        }
                        self.history_page_error = true;
                        matched_history = true;
                    }
                    self.error = Some(format!("{code:?}: {message}"));
                    if matched_history {
                        self.mode = if self.history.is_some() {
                            InputMode::History
                        } else {
                            InputMode::Browse
                        };
                    }
                }
            },
            ServerMessage::Event(event) => match event {
                ServerEvent::HierarchyChanged(hierarchy) => {
                    self.hierarchy = hierarchy;
                    self.update_mode_for_selected_phase();
                    if self
                        .selected
                        .is_some_and(|selected| find_session(self, selected).is_none())
                    {
                        self.release_for_selection_change();
                        if let Some(request) = self.take_pending_history_end() {
                            outgoing.push(request);
                        }
                    }
                }
                ServerEvent::Output { session, bytes } if self.selected == Some(session) => {
                    self.parser.process(&bytes);
                    if let Some(view) = self.history.as_mut() {
                        if view.opened.session == session {
                            view.new_output = true;
                        }
                    }
                }
                ServerEvent::ScreenDirty { session } if self.selected == Some(session) => {
                    if let Some(view) = self.history.as_mut() {
                        view.new_output = true;
                    }
                    let request_id = self.next_request_id();
                    outgoing.push(self.select_request(session, request_id));
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
        outgoing
    }

    fn accept_history_opened(
        &mut self,
        request_id: u64,
        opened: crate::protocol::HistoryOpened,
        outgoing: &mut Vec<ClientMessage>,
    ) {
        let Some(pending) = self.history_begin_request.as_ref() else {
            return;
        };
        if pending.request_id != request_id || pending.session != opened.session {
            return;
        }
        let pending = self
            .history_begin_request
            .take()
            .expect("history begin request checked above");
        if pending.cancelled || self.selected != Some(opened.session) {
            let end_request_id = self.next_request_id();
            outgoing.push(ClientMessage {
                request_id: end_request_id,
                request: Request::HistoryEnd {
                    session: opened.session,
                    snapshot: opened.snapshot,
                },
            });
            return;
        }
        let size = history_view_size(self.pane_size);
        let max_top = opened.total_rows.saturating_sub(u32::from(size.rows));
        let top = opened
            .total_rows
            .saturating_sub(u32::from(size.rows).saturating_mul(2))
            .min(max_top);
        self.history = Some(HistoryView::new(opened, top));
        self.mode = InputMode::History;
        if let Some(request) = self.history_request_if_needed() {
            outgoing.push(request);
        }
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
        if self.mode != InputMode::Terminal || !self.input_is_allowed() {
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
        if self.mode != InputMode::History && self.selected_phase() == Some(&SessionPhase::Paused) {
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

use super::copy::{
    CopyMotion, CopySelection, HistoryCopyCompletion, HistoryCopyJob, HistoryCopyPoint,
    HistoryCopyRange, MAX_COPY_BYTES,
};
use super::event_loop::DASHBOARD_IDLE_REDRAW_INTERVAL;
use super::input::{encode_key, is_browse_key};
use super::render::{
    METADATA_HEIGHT, SPINNER_INTERVAL, sidebar_area, tree_line_at, tree_line_count, tree_row_gap,
    tree_row_height,
};
use super::{Dashboard, DashboardAction, InputMode, KeyEncoding, TreeRow, history_view_size};
use crate::protocol::{
    ClientMessage, ErrorCode, HierarchySnapshot, HistoryOpened, HistoryRows, HistorySnapshotId,
    Request, Response, ServerEvent, ServerMessage,
};
use crate::session::{SessionId, SessionPhase, TerminalSize};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr_terminal::encode_paste;
use ovrcr_terminal::vt100;
use ratatui::layout::Rect;
use std::collections::{HashSet, VecDeque};
use std::io;
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
    pub purpose: HistoryPagePurpose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryPagePurpose {
    Viewport,
    Cursor,
    Copy(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryCursor {
    pub point: HistoryCopyPoint,
    pub row_width: u16,
    pub cell_width: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryCursorTarget {
    At(HistoryCopyPoint),
    RowEnd(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryView {
    pub opened: HistoryOpened,
    pub top: u32,
    pub left: u16,
    pub new_output: bool,
    pub pages: VecDeque<HistoryRows>,
    pub pending: Option<PendingHistoryPage>,
    pub cursor: Option<HistoryCursor>,
    pub cursor_target: Option<HistoryCursorTarget>,
    cursor_reveal: bool,
    pub anchor: Option<HistoryCopyPoint>,
    pub copy_job: Option<HistoryCopyJob>,
    pub copy_completion: Option<HistoryCopyCompletion>,
}

impl HistoryView {
    pub fn new(opened: HistoryOpened, top: u32) -> Self {
        let has_rows = opened.total_rows > 0;
        Self {
            opened,
            top,
            left: 0,
            new_output: false,
            pages: VecDeque::new(),
            pending: None,
            cursor: None,
            cursor_target: has_rows.then_some(HistoryCursorTarget::At(HistoryCopyPoint {
                row: top,
                col: 0,
            })),
            cursor_reveal: false,
            anchor: None,
            copy_job: None,
            copy_completion: None,
        }
    }

    pub fn accept_page(
        &mut self,
        request_id: u64,
        page: HistoryRows,
    ) -> io::Result<Option<HistoryRows>> {
        let Some(pending) = self.pending.as_ref() else {
            return Ok(None);
        };
        if pending.request_id != request_id
            || page.session != pending.session
            || page.snapshot != pending.snapshot
            || page.session != self.opened.session
            || page.snapshot != self.opened.snapshot
            || page.start_row != pending.start_row
            || page.start_col != pending.start_col
        {
            return Ok(None);
        }
        let pending = pending.clone();
        if pending.rows == 0
            || pending.rows > 16
            || pending.cols == 0
            || pending.cols > 128
            || pending.start_row > self.opened.total_rows
            || pending.start_col == u16::MAX
            || u32::from(pending.start_col) + u32::from(pending.cols) > u32::from(u16::MAX)
        {
            self.pending = None;
            return Err(invalid_history_data(
                "history page request bounds are invalid",
            ));
        }
        let expected_rows = self
            .opened
            .total_rows
            .saturating_sub(pending.start_row)
            .min(u32::from(pending.rows));
        if page.rows.len() != usize::try_from(expected_rows).unwrap_or(usize::MAX) {
            self.pending = None;
            return Err(invalid_history_data("history page row count is incomplete"));
        }
        for row in &page.rows {
            let expected_cells = usize::from(
                pending
                    .cols
                    .min(row.width.saturating_sub(pending.start_col)),
            );
            if row.cells.len() != expected_cells {
                self.pending = None;
                return Err(invalid_history_data(
                    "history page cell count is incomplete",
                ));
            }
            for (index, cell) in row.cells.iter().enumerate() {
                if cell.width > 2 {
                    self.pending = None;
                    return Err(invalid_history_data("history cell width is invalid"));
                }
                let col = u32::from(pending.start_col) + u32::try_from(index).unwrap_or(u32::MAX);
                if cell.width == 2 {
                    if col + 1 >= u32::from(row.width) {
                        self.pending = None;
                        return Err(invalid_history_data(
                            "wide history cell exceeds the physical row",
                        ));
                    }
                    if index + 1 < row.cells.len() && row.cells[index + 1].width != 0 {
                        self.pending = None;
                        return Err(invalid_history_data(
                            "wide history cell lacks a continuation",
                        ));
                    }
                } else if cell.width == 0 && index > 0 && row.cells[index - 1].width != 2 {
                    self.pending = None;
                    return Err(invalid_history_data(
                        "history continuation lacks a wide leader",
                    ));
                }
            }
        }
        self.pending = None;
        Ok(Some(page))
    }

    pub fn copy_range(&self) -> Option<HistoryCopyRange> {
        let anchor = self.anchor?;
        let cursor = self.cursor?;
        if self.cursor_target.is_some() {
            return None;
        }
        Some(HistoryCopyRange {
            session: self.opened.session,
            snapshot: self.opened.snapshot,
            anchor,
            cursor: cursor.point,
        })
    }

    pub fn cursor_page_needed(&self) -> Option<(u32, u16, u16, u16)> {
        let target = self.cursor_target?;
        let (row, col) = match target {
            HistoryCursorTarget::At(point) => (point.row, point.col),
            HistoryCursorTarget::RowEnd(row) => {
                let row_width = self
                    .pages
                    .iter()
                    .find_map(|page| row_in_page(page, row).map(|row| row.width));
                let Some(width) = row_width else {
                    return Some(canonical_page_bounds(&self.opened, row, 0)?);
                };
                if width == 0 {
                    return None;
                }
                (row, width - 1)
            }
        };
        Some(canonical_page_bounds(&self.opened, row, col)?)
    }

    pub fn resolve_cursor(&mut self, viewport: TerminalSize) -> io::Result<bool> {
        let Some(target) = self.cursor_target else {
            return Ok(true);
        };
        let (row_number, requested_col, row_end) = match target {
            HistoryCursorTarget::At(point) => (point.row, point.col, false),
            HistoryCursorTarget::RowEnd(row) => (row, 0, true),
        };
        if row_number >= self.opened.total_rows {
            self.cursor = None;
            self.cursor_target = None;
            self.cursor_reveal = false;
            return Ok(true);
        }
        let Some(row) = self
            .pages
            .iter()
            .find_map(|page| row_in_page(page, row_number))
            .cloned()
        else {
            return Ok(false);
        };
        if row.width == 0 {
            if row_end || self.anchor.is_some() {
                self.cursor = Some(HistoryCursor {
                    point: HistoryCopyPoint {
                        row: row_number,
                        col: 0,
                    },
                    row_width: 0,
                    cell_width: 1,
                });
                self.cursor_target = None;
                if self.anchor.is_some() {
                    self.reveal_cursor(viewport);
                }
                return Ok(true);
            }
            self.cursor = None;
            self.cursor_target = None;
            self.cursor_reveal = false;
            return Ok(true);
        }
        let requested_col = if row_end {
            row.width - 1
        } else {
            requested_col
        };
        let requested_col = if requested_col >= row.width {
            if self.anchor.is_some() {
                row.width - 1
            } else {
                self.cursor = None;
                self.cursor_target = None;
                return Ok(true);
            }
        } else {
            requested_col
        };
        let Some((_page_start_col, cell)) = self
            .pages
            .iter()
            .filter_map(|page| {
                let row = row_in_page(page, row_number)?;
                let end = page.start_col.saturating_add(row.cells.len() as u16);
                if requested_col >= page.start_col && requested_col < end {
                    row.cells
                        .get(usize::from(requested_col - page.start_col))
                        .map(|cell| (page.start_col, cell))
                } else {
                    None
                }
            })
            .next()
        else {
            self.cursor_target = Some(HistoryCursorTarget::At(HistoryCopyPoint {
                row: row_number,
                col: requested_col,
            }));
            return Ok(false);
        };
        let mut point_col = requested_col;
        let mut normalized_from_continuation = false;
        let cell_width = if cell.width == 0 {
            let leader_col = point_col.checked_sub(1).ok_or_else(|| {
                invalid_history_data("history cursor continuation lacks a leader")
            })?;
            let leader = self.pages.iter().find_map(|page| {
                let row = row_in_page(page, row_number)?;
                if leader_col < page.start_col {
                    return None;
                }
                row.cells
                    .get(usize::from(leader_col - page.start_col))
                    .map(|cell| (page.start_col, cell))
            });
            let Some((_, leader)) = leader else {
                self.cursor_target = Some(HistoryCursorTarget::At(HistoryCopyPoint {
                    row: row_number,
                    col: leader_col,
                }));
                self.cursor_reveal = true;
                return Ok(false);
            };
            if leader.width != 2 {
                return Err(invalid_history_data(
                    "history cursor continuation lacks a wide leader",
                ));
            }
            point_col = leader_col;
            normalized_from_continuation = true;
            2
        } else if cell.width == 1 || cell.width == 2 {
            cell.width
        } else {
            return Err(invalid_history_data("history cursor cell width is invalid"));
        };
        self.cursor = Some(HistoryCursor {
            point: HistoryCopyPoint {
                row: row_number,
                col: point_col,
            },
            row_width: row.width,
            cell_width,
        });
        self.cursor_target = None;
        if self.anchor.is_some() || normalized_from_continuation || self.cursor_reveal {
            self.reveal_cursor(viewport);
        }
        self.cursor_reveal = false;
        Ok(true)
    }

    fn reveal_cursor(&mut self, viewport: TerminalSize) {
        let Some(cursor) = self.cursor else {
            return;
        };
        let rows = u32::from(viewport.rows.max(1));
        let cols = u32::from(viewport.cols.max(1));
        let max_top = self.opened.total_rows.saturating_sub(rows);
        if cursor.point.row < self.top {
            self.top = cursor.point.row;
        } else if cursor.point.row >= self.top.saturating_add(rows) {
            self.top = cursor.point.row.saturating_add(1).saturating_sub(rows);
        }
        self.top = self.top.min(max_top);

        let cursor_col = u32::from(cursor.point.col);
        if cursor_col < u32::from(self.left) {
            self.left = cursor.point.col;
        } else {
            let end = cursor_col.saturating_add(u32::from(cursor.cell_width.max(1)));
            let visible_end = u32::from(self.left).saturating_add(cols);
            if end > visible_end {
                self.left = u16::try_from(end.saturating_sub(cols)).unwrap_or(u16::MAX);
            }
        }
        let max_left = u32::from(u16::MAX).saturating_sub(cols.saturating_sub(1));
        self.left = self.left.min(u16::try_from(max_left).unwrap_or(u16::MAX));
    }

    fn cache_page(&mut self, page: HistoryRows) {
        if !is_canonical_page(&self.opened, &page) {
            return;
        }
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
    }
}

fn invalid_history_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn canonical_page_bounds(
    opened: &HistoryOpened,
    row: u32,
    col: u16,
) -> Option<(u32, u16, u16, u16)> {
    if row >= opened.total_rows {
        return None;
    }
    let start_row = (row / 16) * 16;
    let start_col = (u32::from(col) / 128) * 128;
    let start_col = u16::try_from(start_col).ok()?;
    let rows = opened.total_rows.saturating_sub(start_row).min(16) as u16;
    let cols = u16::try_from(
        u32::from(u16::MAX)
            .saturating_sub(u32::from(start_col))
            .min(128),
    )
    .ok()?;
    (rows > 0 && cols > 0).then_some((start_row, start_col, rows, cols))
}

fn row_in_page(page: &HistoryRows, row: u32) -> Option<&crate::protocol::HistoryRow> {
    let offset = row.checked_sub(page.start_row)?;
    page.rows.get(usize::try_from(offset).ok()?)
}

pub(super) fn history_page_covers(
    opened: &HistoryOpened,
    page: &HistoryRows,
    start_row: u32,
    start_col: u16,
    rows: u16,
    cols: u16,
) -> bool {
    if !is_canonical_page(opened, page)
        || page.start_row != start_row
        || page.start_col != start_col
    {
        return false;
    }
    let expected_rows = opened
        .total_rows
        .saturating_sub(start_row)
        .min(u32::from(rows));
    if page.rows.len() != usize::try_from(expected_rows).unwrap_or(usize::MAX) {
        return false;
    }
    page.rows
        .iter()
        .all(|row| usize::from(cols.min(row.width.saturating_sub(start_col))) == row.cells.len())
}

fn is_canonical_page(opened: &HistoryOpened, page: &HistoryRows) -> bool {
    page.session == opened.session
        && page.snapshot == opened.snapshot
        && page.start_row % 16 == 0
        && u32::from(page.start_col) % 128 == 0
}

fn history_retry_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Char('0' | '$' | 'g' | 'G' | 'h' | 'j' | 'k' | 'l' | ' ' | 'v' | 'y')
    )
}

fn history_motion_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Char('0' | '$' | 'g' | 'G' | 'h' | 'j' | 'k' | 'l')
    )
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
            copy: None,
            copy_notice: None,
            history: None,
            history_begin_request: None,
            tree_offset: 0,
            next_request_id: 1,
            history_page_error: false,
            history_end_after_selection: None,
            screen_session: None,
            pending_screen: None,
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
                if is_browse_key(key) && self.history_begin_request.is_some() {
                    if let Some(begin) = self.history_begin_request.as_mut() {
                        begin.cancelled = true;
                    }
                    return DashboardAction::EnterBrowse;
                }
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
                    KeyCode::Char('[') if key.kind == KeyEventKind::Press => self.begin_copy(),
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
                    if let Some(begin) = self.history_begin_request.as_mut() {
                        begin.cancelled = true;
                    }
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
            InputMode::Copy => self.copy_key_action(key),
        }
    }

    fn begin_copy(&mut self) -> DashboardAction {
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        let Some(session) = self.selected else {
            self.error = Some("Waiting for terminal screen".into());
            return DashboardAction::Redraw;
        };
        if self.screen_session != Some(session)
            || self.pending_screen.is_some()
            || find_session(self, session).is_none()
        {
            self.error = Some("Waiting for terminal screen".into());
            return DashboardAction::Redraw;
        }
        self.error = None;
        self.copy_notice = None;
        self.copy = Some(CopySelection::capture(session, self.parser.screen()));
        self.mode = InputMode::Copy;
        DashboardAction::Redraw
    }

    fn copy_key_action(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
        }
        if key.kind == KeyEventKind::Press && is_browse_key(key) {
            self.cancel_copy(None);
            return DashboardAction::Redraw;
        }
        if key.kind != KeyEventKind::Press && !matches!(key.kind, KeyEventKind::Repeat) {
            return DashboardAction::None;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return DashboardAction::None;
        }
        if key.kind != KeyEventKind::Press {
            return match key.code {
                KeyCode::Left
                | KeyCode::Right
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Char('h' | 'j' | 'k' | 'l')
                | KeyCode::Home
                | KeyCode::Char('0')
                | KeyCode::End
                | KeyCode::Char('$')
                | KeyCode::Char('g' | 'G') => self.copy_move(key.code),
                _ => DashboardAction::None,
            };
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.cancel_copy(None);
                DashboardAction::Redraw
            }
            KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Char('h' | 'j' | 'k' | 'l')
            | KeyCode::Home
            | KeyCode::Char('0')
            | KeyCode::End
            | KeyCode::Char('$')
            | KeyCode::Char('g' | 'G') => self.copy_move(key.code),
            KeyCode::Char(' ' | 'v') => {
                if let Some(copy) = self.copy.as_mut() {
                    copy.set_anchor();
                    self.copy_notice = None;
                    DashboardAction::Redraw
                } else {
                    DashboardAction::None
                }
            }
            KeyCode::Char('y') | KeyCode::Enter => {
                let Some(text) = self.copy.as_ref().and_then(CopySelection::selected_text) else {
                    self.copy_notice = Some("Set an anchor with Space".into());
                    return DashboardAction::Redraw;
                };
                if text.is_empty() {
                    self.copy_notice = Some("Nothing to copy".into());
                    return DashboardAction::Redraw;
                }
                if text.len() > MAX_COPY_BYTES {
                    self.copy_notice = Some("Selection exceeds 64 KiB".into());
                    return DashboardAction::Redraw;
                }
                self.copy_notice = None;
                DashboardAction::CopyText(text)
            }
            _ => DashboardAction::None,
        }
    }

    fn copy_move(&mut self, code: KeyCode) -> DashboardAction {
        let Some(copy) = self.copy.as_mut() else {
            self.mode = InputMode::Browse;
            return DashboardAction::None;
        };
        let motion = match code {
            KeyCode::Left | KeyCode::Char('h') => CopyMotion::Left,
            KeyCode::Right | KeyCode::Char('l') => CopyMotion::Right,
            KeyCode::Up | KeyCode::Char('k') => CopyMotion::Up,
            KeyCode::Down | KeyCode::Char('j') => CopyMotion::Down,
            KeyCode::Home | KeyCode::Char('0') => CopyMotion::RowStart,
            KeyCode::End | KeyCode::Char('$') => CopyMotion::RowEnd,
            KeyCode::Char('g') => CopyMotion::First,
            KeyCode::Char('G') => CopyMotion::Last,
            _ => return DashboardAction::None,
        };
        copy.move_cursor(motion);
        self.copy_notice = None;
        DashboardAction::Redraw
    }

    pub fn cancel_copy(&mut self, notice: Option<&str>) {
        self.copy = None;
        self.mode = InputMode::Browse;
        self.copy_notice = notice.map(str::to_owned);
    }

    pub fn finish_copy(&mut self, result: std::io::Result<()>) {
        self.copy_notice = Some(match result {
            Ok(()) => "Clipboard request sent; paste to verify".to_owned(),
            Err(error) => error.to_string(),
        });
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
        let exit_key = (key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('g' | 'G')))
            || matches!(key.code, KeyCode::Esc | KeyCode::Char('q'));
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
        }
        if exit_key && key.kind != KeyEventKind::Press {
            return DashboardAction::None;
        }
        if exit_key {
            if matches!(key.code, KeyCode::Esc)
                && self
                    .history
                    .as_ref()
                    .is_some_and(|view| view.copy_job.is_some() || view.copy_completion.is_some())
            {
                if let Some(view) = self.history.as_mut() {
                    view.copy_job = None;
                    view.copy_completion = None;
                }
                self.copy_notice = Some("Copy cancelled".into());
                return DashboardAction::Redraw;
            }
            return self.leave_history();
        }
        if key.kind == KeyEventKind::Repeat && !history_motion_key(key.code) {
            return DashboardAction::None;
        }
        let Some(view) = self.history.as_ref() else {
            return DashboardAction::None;
        };
        if view.copy_job.is_some() || view.copy_completion.is_some() {
            return DashboardAction::None;
        }
        let anchored = self.history.as_ref().and_then(|view| view.anchor).is_some();
        let unresolved_target = view.cursor_target.is_some();
        if !anchored
            && unresolved_target
            && key.kind == KeyEventKind::Press
            && history_retry_key(key.code)
        {
            self.history_page_error = false;
            self.error = None;
        }
        if anchored && view.cursor_target.is_some() {
            if key.kind == KeyEventKind::Press && history_retry_key(key.code) {
                self.history_page_error = false;
                self.error = None;
                self.copy_notice = Some("Waiting for history cell".into());
                return self
                    .history_request_if_needed()
                    .map_or(DashboardAction::Redraw, DashboardAction::Request);
            }
            return DashboardAction::None;
        }
        let size = history_view_size(self.pane_size);
        if !anchored {
            let Some(view) = self.history.as_mut() else {
                return DashboardAction::None;
            };
            let max_top = view.opened.total_rows.saturating_sub(u32::from(size.rows));
            let max_left = u32::from(u16::MAX)
                .saturating_sub(u32::from(size.cols))
                .min(u32::from(u16::MAX));
            let changed = match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    view.top = view.top.saturating_sub(1).min(max_top);
                    true
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    view.top = view.top.saturating_add(1).min(max_top);
                    true
                }
                KeyCode::PageUp => {
                    view.top = view.top.saturating_sub(u32::from(size.rows)).min(max_top);
                    true
                }
                KeyCode::PageDown => {
                    view.top = view.top.saturating_add(u32::from(size.rows)).min(max_top);
                    true
                }
                KeyCode::Home => {
                    view.top = 0;
                    true
                }
                KeyCode::End => {
                    view.top = max_top;
                    true
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    view.left = view.left.saturating_sub(1);
                    true
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    view.left = view
                        .left
                        .saturating_add(1)
                        .min(u16::try_from(max_left).unwrap_or(u16::MAX));
                    true
                }
                KeyCode::Char(' ' | 'v') => return self.anchor_history_cursor(),
                KeyCode::Char('y') => return self.start_history_copy(),
                KeyCode::Enter => false,
                _ => false,
            };
            if !changed {
                return DashboardAction::None;
            }
            view.cursor_target =
                (view.opened.total_rows > 0).then_some(HistoryCursorTarget::At(HistoryCopyPoint {
                    row: view.top,
                    col: view.left,
                }));
            view.cursor_reveal = false;
            self.history_page_error = false;
            self.error = None;
            self.copy_notice = None;
            return self
                .history_request_if_needed()
                .map_or(DashboardAction::Redraw, DashboardAction::Request);
        }
        match key.code {
            KeyCode::Char(' ' | 'v') => self.anchor_history_cursor(),
            KeyCode::Char('y') => self.start_history_copy(),
            KeyCode::Enter => DashboardAction::None,
            _ => self.move_history_cursor(key.code, size),
        }
    }

    fn anchor_history_cursor(&mut self) -> DashboardAction {
        if self
            .history
            .as_ref()
            .is_some_and(|view| view.cursor_target.is_some())
        {
            self.copy_notice = Some("Waiting for history cell".into());
            return self
                .history_request_if_needed()
                .map_or(DashboardAction::Redraw, DashboardAction::Request);
        }
        let Some(cursor) = self.history.as_ref().and_then(|view| view.cursor) else {
            self.copy_notice = Some("Nothing to select on this row".into());
            return DashboardAction::Redraw;
        };
        if let Some(view) = self.history.as_mut() {
            view.anchor = Some(cursor.point);
        }
        self.history_page_error = false;
        self.error = None;
        self.copy_notice = None;
        DashboardAction::Redraw
    }

    fn start_history_copy(&mut self) -> DashboardAction {
        let Some(view) = self.history.as_ref() else {
            return DashboardAction::None;
        };
        if view.cursor_target.is_some() {
            self.copy_notice = Some("Waiting for history cell".into());
            return self
                .history_request_if_needed()
                .map_or(DashboardAction::Redraw, DashboardAction::Request);
        }
        let Some(range) = view.copy_range() else {
            self.copy_notice = Some("Set an anchor with Space".into());
            return DashboardAction::Redraw;
        };
        let job_id = self.next_request_id();
        if let Some(view) = self.history.as_mut() {
            view.copy_job = Some(HistoryCopyJob::new(job_id, range));
        }
        self.history_page_error = false;
        self.error = None;
        self.copy_notice = None;
        self.history_request_if_needed()
            .map_or(DashboardAction::Redraw, DashboardAction::Request)
    }

    fn move_history_cursor(&mut self, code: KeyCode, size: TerminalSize) -> DashboardAction {
        let Some(view) = self.history.as_ref() else {
            return DashboardAction::None;
        };
        let Some(cursor) = view.cursor else {
            return DashboardAction::None;
        };
        let last_row = view.opened.total_rows.saturating_sub(1);
        let target = match code {
            KeyCode::Left | KeyCode::Char('h') => HistoryCursorTarget::At(HistoryCopyPoint {
                row: cursor.point.row,
                col: cursor.point.col.saturating_sub(1),
            }),
            KeyCode::Right | KeyCode::Char('l') => HistoryCursorTarget::At(HistoryCopyPoint {
                row: cursor.point.row,
                col: cursor
                    .point
                    .col
                    .saturating_add(u16::from(cursor.cell_width)),
            }),
            KeyCode::Up | KeyCode::Char('k') => HistoryCursorTarget::At(HistoryCopyPoint {
                row: cursor.point.row.saturating_sub(1),
                col: cursor.point.col,
            }),
            KeyCode::Down | KeyCode::Char('j') => HistoryCursorTarget::At(HistoryCopyPoint {
                row: cursor.point.row.saturating_add(1).min(last_row),
                col: cursor.point.col,
            }),
            KeyCode::PageUp => HistoryCursorTarget::At(HistoryCopyPoint {
                row: cursor.point.row.saturating_sub(u32::from(size.rows)),
                col: cursor.point.col,
            }),
            KeyCode::PageDown => HistoryCursorTarget::At(HistoryCopyPoint {
                row: cursor
                    .point
                    .row
                    .saturating_add(u32::from(size.rows))
                    .min(last_row),
                col: cursor.point.col,
            }),
            KeyCode::Home | KeyCode::Char('0') => HistoryCursorTarget::At(HistoryCopyPoint {
                row: cursor.point.row,
                col: 0,
            }),
            KeyCode::End | KeyCode::Char('$') => HistoryCursorTarget::RowEnd(cursor.point.row),
            KeyCode::Char('g') => HistoryCursorTarget::At(HistoryCopyPoint { row: 0, col: 0 }),
            KeyCode::Char('G') => HistoryCursorTarget::RowEnd(last_row),
            _ => return DashboardAction::None,
        };
        if let Some(view) = self.history.as_mut() {
            view.cursor_target = Some(target);
            view.cursor_reveal = false;
        }
        self.history_page_error = false;
        self.error = None;
        self.copy_notice = None;
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
        self.copy_notice = None;
        self.history_end_after_selection = end;
        DashboardAction::EnterBrowse
    }

    fn release_for_selection_change(&mut self) {
        self.cancel_copy(None);
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

    pub fn history_request_if_needed(&mut self) -> Option<ClientMessage> {
        if self.history_page_error {
            return None;
        }
        let mut resolve_error = None;
        let request = {
            let view = self.history.as_mut()?;
            if view.pending.is_some() || view.copy_completion.is_some() {
                return None;
            }
            let (purpose, bounds) = if let Some(job) = view.copy_job.as_ref() {
                (HistoryPagePurpose::Copy(job.id), job.page_needed())
            } else {
                if view.cursor_target.is_some() {
                    if let Err(error) = view.resolve_cursor(history_view_size(self.pane_size)) {
                        resolve_error = Some(error);
                    }
                }
                if resolve_error.is_some() {
                    self.history_page_error = true;
                    self.error = resolve_error.as_ref().map(ToString::to_string);
                    return None;
                } else if view.cursor_target.is_some() {
                    (HistoryPagePurpose::Cursor, view.cursor_page_needed()?)
                } else {
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
                            let rows = view.opened.total_rows.saturating_sub(row_start).min(16);
                            let cols = u32::from(u16::MAX).saturating_sub(col_start).min(128);
                            if rows > 0
                                && cols > 0
                                && !view.pages.iter().any(|page| {
                                    history_page_covers(
                                        &view.opened,
                                        page,
                                        row_start,
                                        start_col,
                                        rows as u16,
                                        cols as u16,
                                    )
                                })
                            {
                                candidate = Some((row_start, start_col, rows as u16, cols as u16));
                                break;
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
                    (HistoryPagePurpose::Viewport, candidate?)
                }
            };
            let (start_row, start_col, rows, cols) =
                if matches!(purpose, HistoryPagePurpose::Copy(_)) {
                    (bounds.0, bounds.2, bounds.1, bounds.3)
                } else {
                    (bounds.0, bounds.1, bounds.2, bounds.3)
                };
            Some((
                purpose,
                start_row,
                start_col,
                rows,
                cols,
                view.opened.session,
                view.opened.snapshot,
            ))
        };
        let Some(request) = request else {
            if let Some(error) = resolve_error {
                self.history_page_error = true;
                self.error = Some(error.to_string());
            }
            return None;
        };
        if let Some(error) = resolve_error {
            self.history_page_error = true;
            self.error = Some(error.to_string());
            return None;
        }
        let (purpose, start_row, start_col, rows, cols, session, snapshot) = request;
        let request_id = self.next_request_id();
        let pending = PendingHistoryPage {
            request_id,
            session,
            snapshot,
            start_row,
            rows,
            start_col,
            cols,
            purpose,
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
        if self.mode != InputMode::Browse || mouse.kind != MouseEventKind::Down(MouseButton::Left) {
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
                    self.cancel_copy_if_session_missing();
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
                    let matched_screen = self.pending_screen == Some((session, request_id));
                    if matched_screen {
                        self.pending_screen = None;
                        self.screen_session = Some(session);
                    }
                    if matched_screen && self.selected == Some(session) {
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
                    let purpose = self
                        .history
                        .as_ref()
                        .and_then(|view| view.pending.as_ref())
                        .filter(|pending| pending.request_id == request_id)
                        .map(|pending| pending.purpose);
                    let accepted = self.history.as_mut().and_then(|view| {
                        match view.accept_page(request_id, page) {
                            Ok(page) => Some(Ok((view.opened.clone(), page))),
                            Err(error) => Some(Err(error)),
                        }
                    });
                    match (purpose, accepted) {
                        (Some(purpose), Some(Ok((opened, Some(page))))) => {
                            self.history_page_error = false;
                            self.handle_history_page(purpose, opened, page);
                            if let Some(request) = self.history_request_if_needed() {
                                outgoing.push(request);
                            }
                        }
                        (Some(_), Some(Ok((_opened, None)))) => {}
                        (Some(_), Some(Err(error))) => {
                            self.history_page_error = true;
                            self.error = Some(error.to_string());
                            if let Some(view) = self.history.as_mut() {
                                view.copy_job = None;
                                view.copy_completion = None;
                            }
                        }
                        _ => {}
                    }
                }
                Response::Error { code, message } => {
                    let mut matched_history = false;
                    let matched_page = self
                        .history
                        .as_ref()
                        .and_then(|view| view.pending.as_ref())
                        .filter(|pending| pending.request_id == request_id)
                        .cloned();
                    let matched_screen = self
                        .pending_screen
                        .is_some_and(|(_, pending_id)| pending_id == request_id);
                    if matched_screen {
                        self.pending_screen = None;
                    }
                    if self
                        .history_begin_request
                        .as_ref()
                        .is_some_and(|pending| pending.request_id == request_id)
                    {
                        self.history_begin_request = None;
                        matched_history = true;
                    }
                    if matched_page.is_some() {
                        self.history_page_error = true;
                        matched_history = true;
                        if matches!(code, ErrorCode::Conflict | ErrorCode::NotFound) {
                            if let Some(view) = self.history.take() {
                                self.mode = InputMode::Browse;
                                let end_request_id = self.next_request_id();
                                outgoing.push(ClientMessage {
                                    request_id: end_request_id,
                                    request: Request::HistoryEnd {
                                        session: view.opened.session,
                                        snapshot: view.opened.snapshot,
                                    },
                                });
                            }
                        } else if let Some(view) = self.history.as_mut() {
                            view.pending = None;
                            view.copy_job = None;
                            view.copy_completion = None;
                        }
                    }
                    self.error = Some(format!("{code:?}: {message}"));
                    if matched_history && self.copy.is_none() {
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
                    self.cancel_copy_if_session_missing();
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

    fn handle_history_page(
        &mut self,
        purpose: HistoryPagePurpose,
        opened: HistoryOpened,
        page: HistoryRows,
    ) {
        match purpose {
            HistoryPagePurpose::Viewport | HistoryPagePurpose::Cursor => {
                if let Some(view) = self.history.as_mut() {
                    view.cache_page(page);
                    if matches!(purpose, HistoryPagePurpose::Cursor) {
                        if let Err(error) = view.resolve_cursor(history_view_size(self.pane_size)) {
                            self.history_page_error = true;
                            self.error = Some(error.to_string());
                        }
                    }
                }
            }
            HistoryPagePurpose::Copy(job_id) => {
                let Some(view) = self.history.as_mut() else {
                    return;
                };
                let Some(job) = view.copy_job.as_mut() else {
                    return;
                };
                if job.id != job_id
                    || job.range.session != opened.session
                    || job.range.snapshot != opened.snapshot
                {
                    return;
                }
                match job.consume_page(&page) {
                    Ok(true) => {
                        let job = view.copy_job.take().expect("copy job exists");
                        let completion = job.into_completion();
                        if completion.text.is_empty() {
                            self.copy_notice = Some("Nothing to copy".into());
                        } else {
                            view.copy_completion = Some(completion);
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        view.copy_job = None;
                        self.history_page_error = error.kind() != io::ErrorKind::InvalidInput;
                        self.copy_notice = Some(error.to_string());
                    }
                }
            }
        }
    }

    pub fn take_pending_history_copy(&mut self) -> Option<String> {
        let Some(view) = self.history.as_mut() else {
            return None;
        };
        let Some(completion) = view.copy_completion.take() else {
            return None;
        };
        let valid = self.mode == InputMode::History
            && self.selected == Some(completion.range.session)
            && view.opened.session == completion.range.session
            && view.opened.snapshot == completion.range.snapshot
            && view.copy_range() == Some(completion.range);
        if valid { Some(completion.text) } else { None }
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
        let replaced = self.history.take();
        if let Some(view) = replaced {
            let end_request_id = self.next_request_id();
            outgoing.push(ClientMessage {
                request_id: end_request_id,
                request: Request::HistoryEnd {
                    session: view.opened.session,
                    snapshot: view.opened.snapshot,
                },
            });
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
        if self.screen_session != Some(id) {
            if self.copy.is_some() {
                self.cancel_copy(None);
            }
            self.screen_session = None;
        }
        self.selected = Some(id);
        self.pending_screen = Some((id, request_id));
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
        if self.copy.is_some() {
            self.cancel_copy(Some("Copy cancelled: terminal resized"));
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
        if self.mode != InputMode::History
            && self.mode != InputMode::Copy
            && self.selected_phase() == Some(&SessionPhase::Paused)
        {
            self.mode = InputMode::Browse;
        }
    }

    fn cancel_copy_if_session_missing(&mut self) {
        let Some(session) = self.copy.as_ref().map(|copy| copy.session) else {
            return;
        };
        if find_session(self, session).is_none() {
            self.cancel_copy(None);
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

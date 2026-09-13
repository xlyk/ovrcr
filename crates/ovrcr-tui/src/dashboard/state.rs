use super::copy::{
    CopyMotion, CopySelection, HistoryCopyCompletion, HistoryCopyJob, HistoryCopyPoint,
    HistoryCopyRange, MAX_COPY_BYTES,
};
use super::event_loop::DASHBOARD_IDLE_REDRAW_INTERVAL;
use super::input::{encode_key, encode_mouse, is_browse_key};
use super::render::{
    METADATA_HEIGHT, SPINNER_INTERVAL, sidebar_area, tree_line_at, tree_line_count, tree_row_start,
};
use super::settings::DashboardSettings;
use super::{
    Dashboard, DashboardAction, InputMode, KeyEncoding, RequestedView, TreeRow, history_view_size,
};
use crate::protocol::{
    ClientMessage, DashboardView, ErrorCode, HierarchySnapshot, HistoryOpened, HistoryRows,
    HistorySnapshotId, PaneTarget, Request, Response, ServerEvent, ServerMessage,
};
use crate::session::{SessionId, SessionPhase, TerminalSize};
use crate::task_tui::TasksView;
use anyhow::anyhow;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr_terminal::encode_paste;
use ovrcr_terminal::vt100;
use ratatui::layout::Rect;
use std::collections::{HashSet, VecDeque};
use std::io;
use std::time::{Duration, Instant};

/// How long a refused view waits before the same view is sent again.
const VIEW_RETRY_BACKOFF: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingHistoryBegin {
    pub request_id: u64,
    pub session: SessionId,
    pub cancelled: bool,
    pub at_tail: bool,
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
    dragging: bool,
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
            dragging: false,
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
                    return canonical_page_bounds(&self.opened, row, 0);
                };
                if width == 0 {
                    return None;
                }
                (row, width - 1)
            }
        };
        canonical_page_bounds(&self.opened, row, col)
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
        && page.start_row.is_multiple_of(16)
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

fn is_wheel(kind: MouseEventKind) -> bool {
    matches!(
        kind,
        MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight
    )
}

fn point_in_rect(mouse: MouseEvent, rect: Rect) -> bool {
    mouse.column >= rect.x
        && mouse.column < rect.right()
        && mouse.row >= rect.y
        && mouse.row < rect.bottom()
}

fn relative_mouse(mouse: MouseEvent, inner: Rect) -> Option<MouseEvent> {
    if !point_in_rect(mouse, inner) {
        return None;
    }
    Some(MouseEvent {
        column: mouse.column - inner.x,
        row: mouse.row - inner.y,
        ..mouse
    })
}

fn held_index(button: MouseButton) -> usize {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

impl Dashboard {
    pub fn new(size: TerminalSize) -> Self {
        Self {
            tasks: None,
            desktop: super::desktop::DesktopNotifications::default(),
            unread: Default::default(),
            hierarchy: HierarchySnapshot {
                projects: Vec::new(),
            },
            mode: InputMode::Browse,
            panes: vec![super::PaneState::new(size)],
            focused_pane: 0,
            view_revision: 0,
            outer_area: Rect::new(0, 0, size.cols, size.rows),
            collapsed_projects: HashSet::new(),
            collapsed_workspaces: HashSet::new(),
            error: None,
            error_owned_by_view: false,
            copy: None,
            copy_notice: None,
            history: None,
            history_begin_request: None,
            mouse: super::MouseForwarding::default(),
            split_preference: None,
            sidebar_width: None,
            mouse_focused: true,
            deferred_history_at_tail: None,
            tree_offset: 0,
            next_request_id: 1,
            palette: None,
            whichkey: None,
            selected_container: None,
            configuration_paths: None,
            history_page_error: false,
            outbox: super::outbox::Outbox::default(),
            ignored_responses: HashSet::new(),
            settings: DashboardSettings::default(),
            config_dir: std::path::PathBuf::new(),
            last_view_request_id: None,
            pending_view: None,
            requested_view: None,
            failed_view: None,
            force_view_refresh: false,
            view_request_ids: HashSet::new(),
            error_owning_requests: HashSet::new(),
            pending_user_view_change: false,
            pending_snapshot_sessions: HashSet::new(),
        }
    }

    pub fn install_hierarchy(&mut self, hierarchy: HierarchySnapshot) {
        let _ = self.update_hierarchy(hierarchy);
    }

    pub fn install_settings(&mut self, settings: DashboardSettings) {
        self.settings = settings;
    }

    pub fn install_config_dir(&mut self, dir: std::path::PathBuf) {
        self.config_dir = dir;
    }

    pub fn install_area(&mut self, area: Rect) {
        self.outer_area = area;
        for rect in self.pane_rects(area) {
            if let Some(pane) = self.panes.get_mut(rect.pane_index) {
                pane.desired_size = TerminalSize {
                    rows: rect.terminal.height.max(1),
                    cols: rect.terminal.width.max(1),
                };
            }
        }
        let rows = self.visible_rows();
        self.ensure_selection_visible(&rows);
    }

    pub fn install_focus(&mut self, id: SessionId) {
        self.select_session(id);
    }

    pub fn install_unready(&mut self, session: SessionId) {
        if let Some(pane) = self
            .panes
            .iter_mut()
            .find(|pane| pane.session == Some(session))
        {
            pane.ready = false;
            pane.snapshot_installed = false;
        }
    }

    pub fn install_screen(&mut self, session: SessionId, bytes: &[u8]) {
        if !self.panes.iter().any(|pane| pane.session == Some(session)) {
            let index = self
                .panes
                .iter()
                .position(|pane| pane.session.is_none())
                .unwrap_or(self.focused_pane);
            if let Some(pane) = self.panes.get_mut(index) {
                pane.session = Some(session);
            }
        }
        let size = self
            .pane_rects(self.outer_area)
            .into_iter()
            .find(|rect| {
                self.panes
                    .get(rect.pane_index)
                    .and_then(|pane| pane.session)
                    == Some(session)
            })
            .map(|rect| TerminalSize {
                rows: rect.terminal.height.max(1),
                cols: rect.terminal.width.max(1),
            })
            .unwrap_or_else(|| self.focused_size());
        let Some(pane) = self
            .panes
            .iter_mut()
            .find(|pane| pane.session == Some(session))
        else {
            return;
        };
        pane.size = size;
        pane.desired_size = size;
        pane.parser = vt100::Parser::new(size.rows, size.cols, 0);
        pane.parser.process(bytes);
        pane.snapshot_installed = true;
        pane.ready = true;
        pane.error = None;
        let rows = self.visible_rows();
        self.ensure_selection_visible(&rows);
        if self.focused_session() == Some(session) {
            self.reconcile_mouse_protocol();
        }
        self.pending_user_view_change = false;
        self.requested_view = Some(self.desired_view());
    }

    /// Shows `message` in the error banner. A refused view is the only writer whose banner a
    /// later view completion clears, so every other writer comes through here and releases it.
    pub(super) fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
        self.error_owned_by_view = false;
    }

    pub fn view_revision(&self) -> u64 {
        self.view_revision
    }

    /// Queue a request for the next drain. A synthetic mouse release must precede the
    /// `SetView` that moves focus; every retarget calls `cancel_mouse_gesture` before
    /// `view_request`, so push order is send order.
    pub(super) fn push_request(&mut self, message: ClientMessage) {
        self.outbox.push(message);
    }

    /// Everything queued since the last drain, then the next history page if one is due.
    pub fn drain_outbox(&mut self) -> Vec<ClientMessage> {
        let mut batch = self.outbox.drain();
        if let Some(request) = self.history_request_if_needed() {
            batch.push(request);
        }
        batch
    }

    /// The drained batch as the action a key or mouse path returns.
    pub(super) fn drained_action(&mut self) -> DashboardAction {
        let mut batch = self.drain_outbox();
        match batch.len() {
            0 => DashboardAction::Redraw,
            1 => DashboardAction::Request(batch.remove(0)),
            _ => DashboardAction::RequestBatch(batch),
        }
    }

    pub fn request_view_at(&mut self, area: Rect) -> Option<ClientMessage> {
        let id = self.next_request_id();
        let request = self.view_request(area, id).ok().flatten();
        if let Some(request) = &request {
            self.push_request(request.clone());
        }
        request
    }

    pub fn focused_bracketed_paste(&self) -> bool {
        self.focused_pane()
            .is_some_and(|pane| pane.parser.screen().bracketed_paste())
    }

    pub fn install_terminal_input(&mut self) {
        self.mode = InputMode::Terminal;
    }
    pub fn focused_session(&self) -> Option<SessionId> {
        self.panes
            .get(self.focused_pane)
            .and_then(|pane| pane.session)
    }

    pub(super) fn action_session(&self) -> Option<SessionId> {
        if self.selected_container.is_some() {
            None
        } else {
            self.focused_session()
        }
    }

    pub(super) fn focused_pane(&self) -> Option<&super::PaneState> {
        self.panes.get(self.focused_pane)
    }

    pub(super) fn focused_pane_mut(&mut self) -> Option<&mut super::PaneState> {
        self.panes.get_mut(self.focused_pane)
    }

    pub(super) fn focused_size(&self) -> TerminalSize {
        self.focused_pane()
            .map_or(TerminalSize { rows: 1, cols: 1 }, |pane| pane.size)
    }

    pub fn split_pane(&mut self) -> bool {
        if self.panes.len() >= 2 {
            return false;
        }
        let current = self.focused_session();
        let sessions = self
            .visible_rows()
            .into_iter()
            .filter_map(|row| match row {
                TreeRow::Session { id } => Some(id),
                TreeRow::Project { .. } | TreeRow::Workspace { .. } => None,
            })
            .collect::<Vec<_>>();
        let Some(current_index) =
            current.and_then(|id| sessions.iter().position(|candidate| *candidate == id))
        else {
            self.set_error("No other visible session to split");
            return false;
        };
        let Some(session) = (1..sessions.len())
            .map(|offset| sessions[(current_index + offset) % sessions.len()])
            .next()
        else {
            self.set_error("No other visible session to split");
            return false;
        };
        self.mark_pending_parser_discarded();
        self.release_for_selection_change();
        let size = self.focused_size();
        let mut pane = super::PaneState::new(size);
        pane.session = Some(session);
        self.panes.push(pane);
        self.focused_pane = 1;
        self.mode = InputMode::Browse;
        self.pending_user_view_change = true;
        self.invalidate_view_readiness();
        true
    }

    pub(crate) fn focus_pane(&mut self, index: usize) -> bool {
        if index >= self.panes.len() || index == self.focused_pane {
            return false;
        }
        self.release_for_selection_change();
        self.focused_pane = index;
        self.selected_container = None;
        self.mode = InputMode::Browse;
        self.pending_user_view_change = true;
        self.invalidate_view_readiness();
        true
    }

    pub(crate) fn close_focused_pane(&mut self) -> bool {
        if self.panes.len() < 2 {
            return false;
        }
        self.mark_pending_parser_discarded();
        self.release_for_selection_change();
        self.panes.remove(self.focused_pane);
        self.selected_container = None;
        self.focused_pane = self.focused_pane.min(self.panes.len().saturating_sub(1));
        self.mode = InputMode::Browse;
        self.pending_user_view_change = true;
        self.invalidate_view_readiness();
        true
    }

    fn desired_view(&self) -> RequestedView {
        let rects = self.pane_rects(self.outer_area);
        let mut targets = Vec::new();
        for rect in rects {
            let Some(pane) = self.panes.get(rect.pane_index) else {
                continue;
            };
            let Some(session) = pane.session else {
                continue;
            };
            if rect.terminal.width == 0 || rect.terminal.height == 0 {
                continue;
            }
            targets.push((
                session,
                TerminalSize {
                    rows: rect.terminal.height,
                    cols: rect.terminal.width,
                },
            ));
        }
        let focused = self
            .focused_session()
            .filter(|session| targets.iter().any(|(id, _)| id == session));
        RequestedView {
            revision: self.view_revision,
            targets,
            focused,
        }
    }

    fn same_view(left: &RequestedView, right: &RequestedView) -> bool {
        left.targets == right.targets && left.focused == right.focused
    }

    /// The instant a refused view may be sent again, so the event loop can wake for the retry.
    pub(super) fn view_retry_deadline(&self) -> Option<Instant> {
        self.failed_view
            .as_ref()
            .map(|(_, refused_at)| *refused_at + VIEW_RETRY_BACKOFF)
    }

    /// Whether the desired view is the one the server just refused and is still inside its
    /// backoff. A repeating refusal would otherwise re-send `SetView` on every loop pass.
    fn view_retry_is_waiting(&mut self, desired: &RequestedView) -> bool {
        let Some((refused, refused_at)) = self.failed_view.as_ref() else {
            return false;
        };
        if Self::same_view(refused, desired) && refused_at.elapsed() < VIEW_RETRY_BACKOFF {
            return true;
        }
        self.failed_view = None;
        false
    }

    pub(crate) fn view_request(
        &mut self,
        area: Rect,
        request_id: u64,
    ) -> anyhow::Result<Option<ClientMessage>> {
        let geometry_changed = self.outer_area != area;
        if geometry_changed {
            self.cancel_mouse_gesture();
        }
        if geometry_changed && self.copy.is_some() {
            self.cancel_copy(Some("Copy cancelled: terminal resized"));
        }
        self.outer_area = area;
        let desired = self.desired_view();
        if self.view_retry_is_waiting(&desired) {
            // A true no-op for the waiting view: readiness, pane errors, and the recorded
            // failure all survive until the backoff deadline the event loop wakes for.
            return Ok(None);
        }
        let pending_same = self
            .pending_view
            .as_ref()
            .is_some_and(|pending| Self::same_view(&pending.view, &desired));
        let unchanged = self.pending_view.is_none()
            && !self.force_view_refresh
            && self
                .requested_view
                .as_ref()
                .is_some_and(|requested| Self::same_view(requested, &desired));
        let rects = self.pane_rects(area);
        let desired_sessions = desired
            .targets
            .iter()
            .map(|(session, _)| *session)
            .collect::<HashSet<_>>();
        if !unchanged && (self.pending_view.is_none() || !pending_same) {
            for pane in &mut self.panes {
                if pane
                    .session
                    .is_none_or(|session| !desired_sessions.contains(&session))
                {
                    pane.ready = false;
                    pane.snapshot_installed = false;
                }
            }
        }
        for rect in &rects {
            if let Some(pane) = self.panes.get_mut(rect.pane_index) {
                pane.desired_size = TerminalSize {
                    rows: rect.terminal.height,
                    cols: rect.terminal.width,
                };
                if unchanged {
                    continue;
                } else if self.pending_view.is_some() {
                    if !pending_same {
                        pane.ready = false;
                        pane.snapshot_installed = false;
                    }
                } else if !pending_same {
                    pane.ready = false;
                    pane.snapshot_installed = false;
                    pane.error = None;
                }
            }
        }
        if self.pending_view.is_some() {
            // The change the user asked for is coalesced into the request this pending view's
            // completion sends, so its banner ownership waits here rather than being dropped.
            return Ok(None);
        }
        if unchanged {
            // A true no-op completes nothing, so it must not hand a pending user change's banner
            // ownership to whichever request the server asks for next.
            self.pending_user_view_change = false;
            return Ok(None);
        }
        let revision = self
            .view_revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("dashboard view revision exhausted"))?;
        self.view_revision = revision;
        let mut view = desired;
        view.revision = revision;
        for (session, size) in &view.targets {
            if let Some(pane) = self
                .panes
                .iter_mut()
                .find(|pane| pane.session == Some(*session))
            {
                pane.desired_size = *size;
                pane.ready = false;
                pane.snapshot_installed = false;
                pane.error = None;
            }
        }
        self.force_view_refresh = false;
        self.last_view_request_id = Some(request_id);
        self.view_request_ids.insert(request_id);
        if std::mem::take(&mut self.pending_user_view_change) {
            self.error_owning_requests.insert(request_id);
        }
        self.pending_snapshot_sessions.clear();
        self.pending_view = Some(super::PendingView {
            request_id,
            view: view.clone(),
            parser_discarded: false,
        });
        Ok(Some(ClientMessage {
            request_id,
            request: Request::SetView {
                view: DashboardView {
                    revision,
                    panes: view
                        .targets
                        .iter()
                        .map(|(session, size)| PaneTarget {
                            session: *session,
                            size: *size,
                        })
                        .collect(),
                    focused: view.focused,
                },
            },
        }))
    }

    pub(crate) fn apply_screen(
        &mut self,
        revision: u64,
        session: SessionId,
        size: TerminalSize,
        bytes: &[u8],
    ) {
        if revision != self.view_revision
            || !self.pending_view.as_ref().is_some_and(|pending| {
                pending.view.revision == revision
                    && pending.view.targets.iter().any(|(id, _)| *id == session)
            })
        {
            return;
        }
        let Some(pane) = self
            .panes
            .iter_mut()
            .find(|pane| pane.session == Some(session))
        else {
            self.pending_snapshot_sessions.insert(session);
            return;
        };
        pane.size = size;
        pane.parser = vt100::Parser::new(size.rows, size.cols, 0);
        pane.parser.process(bytes);
        pane.snapshot_installed = true;
        pane.error = None;
        self.pending_snapshot_sessions.insert(session);
        let rows = self.visible_rows();
        self.ensure_selection_visible(&rows);
        if self.focused_session() == Some(session) {
            self.reconcile_mouse_protocol();
        }
    }

    pub(super) fn session_is_busy(&self, id: SessionId) -> bool {
        find_session(self, id).is_some_and(|session| {
            matches!(session.phase, crate::session::SessionPhase::Running)
                && session.activity == crate::session::AgentActivity::Busy
        })
    }

    pub(super) fn redraw_interval(&self) -> Duration {
        if self.tasks.is_some() {
            return Duration::from_millis(50);
        }
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

    pub(crate) fn visible_rows(&self) -> Vec<TreeRow> {
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

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let rows = self.visible_rows();
        let ids = rows
            .iter()
            .filter_map(|row| match row {
                TreeRow::Session { id } => Some(*id),
                TreeRow::Project { .. } | TreeRow::Workspace { .. } => None,
            })
            .collect::<Vec<_>>();
        if ids.is_empty() {
            self.mark_pending_parser_discarded();
            if let Some(pane) = self.focused_pane_mut() {
                pane.session = None;
                pane.ready = false;
                pane.snapshot_installed = false;
            }
            return;
        }
        let current = self
            .focused_session()
            .and_then(|selected| ids.iter().position(|id| *id == selected));
        let index = match current {
            Some(index) => (index as isize + delta).clamp(0, ids.len() as isize - 1) as usize,
            None if delta < 0 => ids.len() - 1,
            None => 0,
        };
        self.select_session(ids[index]);
        self.ensure_selection_visible(&rows);
    }

    pub(crate) fn select_session(&mut self, id: SessionId) {
        self.selected_container = None;
        if let Some(index) = self.panes.iter().position(|pane| pane.session == Some(id)) {
            self.focus_pane(index);
        } else if self.focused_session() != Some(id) {
            self.mark_pending_parser_discarded();
            self.release_for_selection_change();
            let size = self.focused_size();
            if let Some(pane) = self.focused_pane_mut() {
                pane.session = Some(id);
                pane.parser = vt100::Parser::new(size.rows, size.cols, 0);
                pane.size = size;
                pane.desired_size = size;
                pane.snapshot_installed = false;
                pane.ready = false;
                pane.error = None;
            }
            self.pending_user_view_change = true;
            self.invalidate_view_readiness();
            self.mode = InputMode::Browse;
        }
        let rows = self.visible_rows();
        self.ensure_selection_visible(&rows);
    }

    fn tree_viewport_height(&self) -> usize {
        usize::from(self.focused_size().rows)
            .saturating_add(usize::from(METADATA_HEIGHT))
            .max(1)
    }

    fn ensure_selection_visible(&mut self, rows: &[TreeRow]) {
        let Some(selected) = self.focused_session() else {
            return;
        };
        let Some(index) = rows
            .iter()
            .position(|row| *row == TreeRow::Session { id: selected })
        else {
            return;
        };
        let height = self.tree_viewport_height();
        let selected_start = tree_row_start(rows, index);
        let selected_end = selected_start.saturating_add(1);
        if selected_start < self.tree_offset {
            self.tree_offset = selected_start;
        } else if selected_end > self.tree_offset.saturating_add(height) {
            self.tree_offset = selected_end.saturating_sub(height);
        }
        let max_offset = tree_line_count(rows).saturating_sub(height);
        self.tree_offset = self.tree_offset.min(max_offset);
    }

    pub fn key_action(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Press {
            self.desktop.notice = None;
        }
        if let Some(tasks) = &mut self.tasks {
            if tasks.event(Event::Key(key)) {
                self.tasks = None;
            }
            return DashboardAction::Redraw;
        }
        if self.palette.is_some() {
            return self.palette_key(key);
        }
        if let Some(action) = self.whichkey_key(key) {
            return action;
        }
        match self.mode {
            InputMode::Browse => {
                if is_browse_key(key) && self.history_begin_request.is_some() {
                    if let Some(begin) = self.history_begin_request.as_mut() {
                        begin.cancelled = true;
                    }
                    return DashboardAction::EnterBrowse;
                }
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    if key.code == KeyCode::Char('t') {
                        if let Some(begin) = self.history_begin_request.as_mut() {
                            begin.cancelled = true;
                        }
                        self.cancel_mouse_gesture();
                        let (project, _) = self.creation_context();
                        self.tasks = Some(TasksView::with_projects(&self.hierarchy, &project));
                        return DashboardAction::Redraw;
                    }
                    return DashboardAction::None;
                }
                match key.code {
                    KeyCode::Char(':') => self.open_palette(),
                    KeyCode::Char('n') => self.open_create_terminal(),
                    KeyCode::Char('N') if key.kind == KeyEventKind::Press => {
                        self.toggle_desktop_notifications()
                    }
                    KeyCode::Char('S') if key.kind == KeyEventKind::Press => {
                        self.toggle_ready_sound()
                    }
                    KeyCode::Char('w') => self.open_create_workspace(),
                    KeyCode::Char('a') => self.open_register_project(),
                    KeyCode::Char('X') => self.open_close_terminal(),
                    KeyCode::Char('t') => self.ctrl('t'),
                    KeyCode::Esc if self.history_begin_request.is_some() => {
                        if let Some(begin) = self.history_begin_request.as_mut() {
                            begin.cancelled = true;
                        }
                        DashboardAction::EnterBrowse
                    }
                    KeyCode::Char('q') => DashboardAction::Detach,
                    KeyCode::Char('[') if key.kind == KeyEventKind::Press => self.begin_copy(),
                    KeyCode::PageUp => self.begin_history_request(false),
                    KeyCode::Char('p') => self.pause_request(true),
                    KeyCode::Char('r') => self.pause_request(false),
                    KeyCode::Char('R') if key.kind == KeyEventKind::Press => {
                        self.mark_reviewed_request()
                    }
                    KeyCode::Char('v') => {
                        self.split_pane();
                        DashboardAction::Redraw
                    }
                    KeyCode::Char('x') => {
                        self.close_focused_pane();
                        DashboardAction::Redraw
                    }
                    KeyCode::Tab => {
                        if self.panes.len() == 2 {
                            self.focus_pane((self.focused_pane + 1) % 2);
                        }
                        DashboardAction::Redraw
                    }
                    KeyCode::BackTab => {
                        if self.panes.len() == 2 {
                            self.focus_pane((self.focused_pane + 1) % 2);
                        }
                        DashboardAction::Redraw
                    }
                    KeyCode::Enter => {
                        if self.input_is_allowed() {
                            self.mode = InputMode::Terminal;
                            DashboardAction::Redraw
                        } else {
                            self.refuse_input()
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
                    self.cancel_mouse_gesture();
                    if let Some(begin) = self.history_begin_request.as_mut() {
                        begin.cancelled = true;
                    }
                    self.mode = InputMode::Browse;
                    DashboardAction::EnterBrowse
                } else if !self.input_is_allowed() {
                    self.refuse_input()
                } else {
                    let application_cursor = self
                        .focused_pane()
                        .is_some_and(|pane| pane.parser.screen().application_cursor());
                    match encode_key(key, application_cursor) {
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
        let Some(session) = self.action_session() else {
            self.set_error("Waiting for terminal screen");
            return DashboardAction::Redraw;
        };
        if !self.focused_pane().is_some_and(|pane| pane.ready)
            || find_session(self, session).is_none()
        {
            self.set_error("Waiting for terminal screen");
            return DashboardAction::Redraw;
        }
        self.error = None;
        self.copy_notice = None;
        let screen = self
            .focused_pane()
            .map(|pane| pane.parser.screen())
            .expect("focused session has a pane");
        self.copy = Some(CopySelection::capture(session, screen));
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
            KeyCode::Char('v') => {
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
                    self.copy_notice = Some("Set an anchor with v".into());
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

    pub(crate) fn cancel_copy(&mut self, notice: Option<&str>) {
        self.whichkey = None;
        self.copy = None;
        self.mode = InputMode::Browse;
        self.copy_notice = notice.map(str::to_owned);
    }

    pub(crate) fn finish_copy(&mut self, result: std::io::Result<()>) {
        self.copy_notice = Some(match result {
            Ok(()) => "Clipboard request sent; paste to verify".to_owned(),
            Err(error) => error.to_string(),
        });
    }

    fn begin_history_request(&mut self, at_tail: bool) -> DashboardAction {
        let Some(session) = self.action_session() else {
            return DashboardAction::None;
        };
        if self.history.is_some() || self.history_begin_request.is_some() {
            return DashboardAction::Redraw;
        }
        // A coalesced focus change would invalidate a capture opened before SetView.
        if !self.focused_pane().is_some_and(|pane| pane.ready) {
            self.set_error("Pane is loading; retry history");
            return DashboardAction::Redraw;
        }
        self.history_page_error = false;
        self.error = None;
        let request_id = self.error_owning_request_id();
        self.history_begin_request = Some(PendingHistoryBegin {
            request_id,
            session,
            cancelled: false,
            at_tail,
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
        let size = history_view_size(self.focused_size());
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
                KeyCode::Char('v') => return self.anchor_history_cursor(),
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
            KeyCode::Char('v') => self.anchor_history_cursor(),
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
            self.copy_notice = Some("Set an anchor with v".into());
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
        self.whichkey = None;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        let end = self.history.take().map(|view| {
            let request_id = self.error_owning_request_id();
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
        if let Some(end) = end {
            self.push_request(end);
        }
        DashboardAction::EnterBrowse
    }

    fn release_for_selection_change(&mut self) {
        self.whichkey = None;
        self.cancel_mouse_gesture();
        self.cancel_copy(None);
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        if let Some(view) = self.history.take() {
            self.mode = InputMode::Browse;
            let request_id = self.error_owning_request_id();
            self.push_request(ClientMessage {
                request_id,
                request: Request::HistoryEnd {
                    session: view.opened.session,
                    snapshot: view.opened.snapshot,
                },
            });
        }
    }

    fn invalidate_view_readiness(&mut self) {
        // Every caller changes selection, assignment, or geometry, so a wheel tick parked
        // against the previous selection is no longer the one the user asked for.
        self.deferred_history_at_tail = None;
        self.requested_view = None;
        for pane in &mut self.panes {
            pane.ready = false;
            pane.snapshot_installed = false;
        }
    }

    fn mark_pending_parser_discarded(&mut self) {
        if let Some(pending) = self.pending_view.as_mut() {
            pending.parser_discarded = true;
        }
    }

    pub(crate) fn history_request_if_needed(&mut self) -> Option<ClientMessage> {
        if self.history_page_error {
            return None;
        }
        let mut resolve_error = None;
        let size = history_view_size(self.focused_size());
        let request = {
            let view = self.history.as_mut()?;
            if view.pending.is_some() || view.copy_completion.is_some() {
                return None;
            }
            let (purpose, bounds) = if let Some(job) = view.copy_job.as_ref() {
                (HistoryPagePurpose::Copy(job.id), job.page_needed())
            } else {
                if view.cursor_target.is_some()
                    && let Err(error) = view.resolve_cursor(size)
                {
                    resolve_error = Some(error);
                }
                if let Some(error) = resolve_error.as_ref().map(ToString::to_string) {
                    self.history_page_error = true;
                    self.set_error(error);
                    return None;
                } else if view.cursor_target.is_some() {
                    (HistoryPagePurpose::Cursor, view.cursor_page_needed()?)
                } else {
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
                self.set_error(error.to_string());
            }
            return None;
        };
        if let Some(error) = resolve_error {
            self.history_page_error = true;
            self.set_error(error.to_string());
            return None;
        }
        let (purpose, start_row, start_col, rows, cols, session, snapshot) = request;
        let request_id = self.error_owning_request_id();
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
        if let Some(tasks) = &mut self.tasks {
            if tasks.event(event) {
                self.tasks = None;
            }
            return DashboardAction::Redraw;
        }
        match event {
            Event::Paste(_) if self.whichkey.is_some() => DashboardAction::None,
            Event::Paste(text) if self.palette.is_some() => self.palette_paste(&text),
            Event::Key(key) => self.key_action(key),
            Event::Paste(text) if self.mode == InputMode::Terminal => {
                if !self.input_is_allowed() {
                    self.refuse_input()
                } else {
                    DashboardAction::PtyBytes(encode_paste(
                        &text,
                        self.focused_pane()
                            .is_some_and(|pane| pane.parser.screen().bracketed_paste()),
                    ))
                }
            }
            Event::Mouse(mouse) => self.mouse_action(mouse, self.outer_area),
            Event::FocusLost => {
                self.cancel_mouse_gesture();
                self.mouse_focused = false;
                DashboardAction::Redraw
            }
            Event::FocusGained => {
                self.mouse_focused = true;
                DashboardAction::Redraw
            }
            Event::Resize(_, _) => DashboardAction::Redraw,
            Event::Paste(_) => DashboardAction::None,
        }
    }

    pub fn mouse_action(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        if self.palette.is_some() {
            return self.palette_mouse(mouse, area);
        }
        if let Some(tasks) = &mut self.tasks {
            if tasks.mouse(mouse, area) {
                self.tasks = None;
            }
            return DashboardAction::Redraw;
        }
        if self.whichkey.is_some() {
            return self.whichkey_mouse(mouse, area);
        }
        if !self.mouse_focused {
            return DashboardAction::None;
        }
        if matches!(self.mode, InputMode::Browse | InputMode::Terminal)
            && mouse.kind == MouseEventKind::Down(MouseButton::Left)
        {
            let (actions, menu) = super::render::action_controls(area);
            let point = ratatui::layout::Position::new(mouse.column, mouse.row);
            if actions.contains(point) {
                self.cancel_mouse_gesture();
                return self.open_palette();
            }
            if menu.contains(point) {
                self.cancel_mouse_gesture();
                self.mode = InputMode::Browse;
                return self
                    .whichkey_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE))
                    .unwrap_or(DashboardAction::Redraw);
            }
        }
        if matches!(self.mode, InputMode::Copy | InputMode::History) {
            return self.capture_mouse_action(mouse, area);
        }
        if let Some(action) = self.pane_mouse_action(mouse, area) {
            return action;
        }
        if let Some(action) = self.sidebar_mouse_action(mouse, area) {
            return action;
        }
        match self.mode {
            InputMode::Copy => DashboardAction::None,
            InputMode::History => self.history_wheel_action(mouse, area),
            InputMode::Browse => {
                if is_wheel(mouse.kind)
                    && let Some(action) = self.pane_wheel_history(mouse, area)
                {
                    return action;
                }
                DashboardAction::None
            }
            InputMode::Terminal => self.terminal_mouse_action(mouse, area),
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        self.mouse_action(mouse, area)
    }

    pub(crate) fn mouse_capture_required(&self) -> bool {
        if self.palette.is_some() {
            return true;
        }
        if self.tasks.is_some() {
            return true;
        }
        if self.whichkey.is_some() {
            return true;
        }
        match self.mode {
            InputMode::Browse | InputMode::History => true,
            InputMode::Copy => true,
            InputMode::Terminal => self.mouse_focused,
        }
    }

    pub(crate) fn cancel_mouse_gesture(&mut self) {
        self.queue_held_releases();
        self.mouse.split_dragging = false;
        self.mouse.sidebar_dragging = false;
        if let Some(copy) = &mut self.copy {
            copy.dragging = false;
        }
        if let Some(history) = &mut self.history {
            history.dragging = false;
        }
    }

    pub fn pane_rects(&self, area: Rect) -> Vec<super::PaneRects> {
        super::pane_rects_with_preference(
            area,
            self.panes.len(),
            self.focused_pane,
            self.split_preference,
            self.sidebar_width,
        )
    }

    fn sidebar_mouse_action(&mut self, mouse: MouseEvent, area: Rect) -> Option<DashboardAction> {
        if !matches!(self.mode, InputMode::Browse | InputMode::Terminal) {
            return None;
        }
        let sidebar = sidebar_area(area, self.sidebar_width);
        if !point_in_rect(mouse, sidebar) {
            return None;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let rows = self.visible_rows();
                let max_offset = tree_line_count(&rows).saturating_sub(usize::from(sidebar.height));
                let next = if mouse.kind == MouseEventKind::ScrollUp {
                    self.tree_offset.saturating_sub(1)
                } else {
                    self.tree_offset.saturating_add(1).min(max_offset)
                };
                if next == self.tree_offset {
                    Some(DashboardAction::None)
                } else {
                    self.tree_offset = next;
                    Some(DashboardAction::Redraw)
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let row_index = self.tree_offset + usize::from(mouse.row.saturating_sub(sidebar.y));
                let rows = self.visible_rows();
                let row = tree_line_at(&rows, row_index)?;
                let row = row.clone();
                let column = mouse.column.saturating_sub(sidebar.x);
                match row {
                    TreeRow::Session { id } => {
                        self.select_session(id);
                        Some(self.request_selected())
                    }
                    TreeRow::Project { name } if column == 0 => {
                        if !self.collapsed_projects.remove(&name) {
                            self.collapsed_projects.insert(name);
                        }
                        self.clamp_tree_offset(usize::from(sidebar.height));
                        Some(DashboardAction::Redraw)
                    }
                    TreeRow::Workspace { project, name } if column == 2 => {
                        let key = (project, name);
                        if !self.collapsed_workspaces.remove(&key) {
                            self.collapsed_workspaces.insert(key);
                        }
                        self.clamp_tree_offset(usize::from(sidebar.height));
                        Some(DashboardAction::Redraw)
                    }
                    row @ (TreeRow::Project { .. } | TreeRow::Workspace { .. }) => {
                        self.select_container(row);
                        Some(DashboardAction::Redraw)
                    }
                }
            }
            _ => None,
        }
    }

    fn clamp_tree_offset(&mut self, viewport_height: usize) {
        let max_offset = tree_line_count(&self.visible_rows()).saturating_sub(viewport_height);
        self.tree_offset = self.tree_offset.min(max_offset);
    }

    fn pane_mouse_action(&mut self, mouse: MouseEvent, area: Rect) -> Option<DashboardAction> {
        if !matches!(self.mode, InputMode::Browse | InputMode::Terminal) {
            return None;
        }
        if self.mouse.split_dragging {
            return match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    Some(if self.resize_split(area, mouse.column) {
                        DashboardAction::Redraw
                    } else {
                        DashboardAction::None
                    })
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.mouse.split_dragging = false;
                    Some(DashboardAction::Redraw)
                }
                _ => Some(DashboardAction::None),
            };
        }
        if self.mouse.sidebar_dragging {
            return match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    Some(if self.resize_sidebar(area, mouse.column) {
                        DashboardAction::Redraw
                    } else {
                        DashboardAction::None
                    })
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.mouse.sidebar_dragging = false;
                    Some(DashboardAction::Redraw)
                }
                _ => Some(DashboardAction::None),
            };
        }

        let sidebar = sidebar_area(area, self.sidebar_width);
        let rects = self.pane_rects(area);
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && mouse.row >= sidebar.y
            && mouse.row < sidebar.bottom()
        {
            // The sidebar's border column sits just past its content.
            if mouse.column == sidebar.right() {
                self.queue_held_releases();
                self.mouse.sidebar_dragging = true;
                return Some(DashboardAction::Redraw);
            }
            if rects.len() == 2 && mouse.column == rects[0].terminal.right() {
                self.queue_held_releases();
                self.mouse.split_dragging = true;
                return Some(DashboardAction::Redraw);
            }
        }

        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(pane) = rects
                .iter()
                .find(|pane| point_in_rect(mouse, pane.terminal))
        {
            if self.panes[pane.pane_index].session.is_none() {
                return Some(DashboardAction::Redraw);
            }
            if self.mode == InputMode::Browse {
                if pane.pane_index == self.focused_pane {
                    self.selected_container = None;
                } else {
                    self.focus_pane(pane.pane_index);
                }
                if matches!(self.selected_phase(), Some(SessionPhase::Running)) {
                    self.mode = InputMode::Terminal;
                }
                return Some(DashboardAction::Redraw);
            }
            if pane.pane_index != self.focused_pane {
                self.focus_pane(pane.pane_index);
                self.mode = InputMode::Terminal;
                return Some(DashboardAction::Redraw);
            }
        }
        None
    }

    /// Drag the sidebar border to `column`; the panes take the remaining width.
    fn resize_sidebar(&mut self, area: Rect, column: u16) -> bool {
        let body_width = area.width;
        let width = column
            .saturating_add(1)
            .saturating_sub(area.x)
            .clamp(super::MIN_SIDEBAR_WIDTH, body_width / 2);
        if width == super::sidebar_width_for(area, self.sidebar_width) {
            return false;
        }
        self.sidebar_width = Some(width);
        self.apply_pane_sizes(area);
        true
    }

    /// Copy the drawn pane sizes into each pane's desired size and revoke readiness.
    fn apply_pane_sizes(&mut self, area: Rect) {
        for rect in self.pane_rects(area) {
            if let Some(pane) = self.panes.get_mut(rect.pane_index) {
                pane.desired_size = TerminalSize {
                    rows: rect.terminal.height,
                    cols: rect.terminal.width,
                };
            }
        }
        self.queue_held_releases();
        self.pending_user_view_change = true;
        self.invalidate_view_readiness();
    }

    fn resize_split(&mut self, area: Rect, column: u16) -> bool {
        let rects = self.pane_rects(area);
        if rects.len() != 2 {
            self.mouse.split_dragging = false;
            return false;
        }
        let available = rects[0]
            .terminal
            .width
            .saturating_add(rects[1].terminal.width);
        let left = column.saturating_sub(rects[0].terminal.x).clamp(
            super::MIN_SPLIT_PANE_WIDTH,
            available - super::MIN_SPLIT_PANE_WIDTH,
        );
        if left == rects[0].terminal.width {
            return false;
        }
        self.split_preference = Some(super::SplitPreference { left, available });
        self.apply_pane_sizes(area);
        true
    }

    fn select_container(&mut self, row: TreeRow) {
        if self.selected_container.as_ref() == Some(&row) {
            return;
        }
        self.mark_pending_parser_discarded();
        self.release_for_selection_change();
        let size = self.focused_size();
        if let Some(pane) = self.focused_pane_mut() {
            *pane = super::PaneState::new(size);
        }
        if let Some(index) = self.panes.iter().position(|pane| pane.session.is_some()) {
            self.focused_pane = index;
        }
        self.selected_container = Some(row);
        self.mode = InputMode::Browse;
        self.pending_user_view_change = true;
        self.invalidate_view_readiness();
    }

    fn terminal_mouse_action(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        if !self.mouse_focused {
            return DashboardAction::None;
        }
        if self.terminal_mouse_enabled() {
            return self.forward_mouse(mouse, area);
        }
        if is_wheel(mouse.kind) {
            return self
                .pane_wheel_history(mouse, area)
                .unwrap_or(DashboardAction::None);
        }
        DashboardAction::None
    }

    fn terminal_mouse_enabled(&self) -> bool {
        self.mode == InputMode::Terminal
            && self.mouse_focused
            && self.input_is_allowed()
            && self.focused_mouse_mode() != vt100::MouseProtocolMode::None
    }

    fn focused_mouse_mode(&self) -> vt100::MouseProtocolMode {
        self.focused_pane()
            .map(|pane| pane.parser.screen().mouse_protocol_mode())
            .unwrap_or(vt100::MouseProtocolMode::None)
    }

    fn focused_mouse_encoding(&self) -> vt100::MouseProtocolEncoding {
        self.focused_pane()
            .map(|pane| pane.parser.screen().mouse_protocol_encoding())
            .unwrap_or_default()
    }

    fn forward_mouse(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        let Some(inner) = self
            .pane_rects(area)
            .into_iter()
            .find(|pane| pane.pane_index == self.focused_pane)
            .map(|pane| pane.terminal)
        else {
            return DashboardAction::None;
        };
        let parser_size = self.focused_size();
        let relative = relative_mouse(mouse, inner);
        let inside = relative
            .is_some_and(|event| event.column < parser_size.cols && event.row < parser_size.rows);
        if !inside {
            return self.release_held_leaving_rectangle(mouse.kind);
        }
        let relative = relative.expect("inside cell is relative");
        if matches!(mouse.kind, MouseEventKind::Moved | MouseEventKind::Drag(_))
            && self.mouse.last_motion.as_ref().is_some_and(|last| {
                last.kind == relative.kind
                    && last.column == relative.column
                    && last.row == relative.row
                    && last.modifiers == relative.modifiers
            })
        {
            return DashboardAction::None;
        }
        if let MouseEventKind::Drag(button) = mouse.kind
            && self.mouse.held[held_index(button)].is_none()
        {
            return DashboardAction::None;
        }
        if matches!(mouse.kind, MouseEventKind::Moved)
            && self.mouse.held.iter().any(Option::is_some)
        {
            return DashboardAction::None;
        }
        if let MouseEventKind::Up(button) = mouse.kind
            && self.mouse.held[held_index(button)].is_none()
        {
            return DashboardAction::None;
        }
        let mode = self.focused_mouse_mode();
        let encoding = self.focused_mouse_encoding();
        let Some(bytes) = encode_mouse(relative, mode, encoding) else {
            return DashboardAction::None;
        };
        let session = self.focused_session();
        match mouse.kind {
            MouseEventKind::Down(button) => {
                if let Some(session) = session {
                    self.mouse.held[held_index(button)] = Some(super::HeldMouse {
                        session,
                        event: relative,
                        mode,
                        encoding,
                    });
                }
            }
            MouseEventKind::Up(button) => {
                self.mouse.held[held_index(button)] = None;
            }
            MouseEventKind::Drag(button) => {
                if let Some(held) = &mut self.mouse.held[held_index(button)] {
                    held.event = relative;
                }
            }
            _ => {}
        }
        if matches!(mouse.kind, MouseEventKind::Moved | MouseEventKind::Drag(_)) {
            self.mouse.last_motion = Some(relative);
        }
        DashboardAction::PtyBytes(bytes)
    }

    fn release_held_leaving_rectangle(&mut self, kind: MouseEventKind) -> DashboardAction {
        let button = match kind {
            MouseEventKind::Drag(button) | MouseEventKind::Up(button) => button,
            _ => return DashboardAction::None,
        };
        let Some(held) = self.mouse.held[held_index(button)].take() else {
            return DashboardAction::None;
        };
        let release = MouseEvent {
            kind: MouseEventKind::Up(button),
            ..held.event
        };
        self.mouse.last_motion = None;
        encode_mouse(release, held.mode, held.encoding)
            .map_or(DashboardAction::None, DashboardAction::PtyBytes)
    }

    fn pane_wheel_history(&mut self, mouse: MouseEvent, area: Rect) -> Option<DashboardAction> {
        if mouse.kind != MouseEventKind::ScrollUp {
            return None;
        }
        let pane = self
            .pane_rects(area)
            .into_iter()
            .find(|pane| point_in_rect(mouse, pane.terminal))?;
        if pane.pane_index != self.focused_pane && self.focus_pane(pane.pane_index) {
            // Focusing revoked readiness, so the open has to wait for the replacement view.
            self.deferred_history_at_tail = Some(pane.pane_index);
            return Some(DashboardAction::Redraw);
        }
        Some(self.begin_history_request(true))
    }

    /// Opens the history a wheel tick asked for once its pane became ready and stayed focused.
    pub(super) fn take_deferred_history_request(&mut self) -> Option<ClientMessage> {
        let index = self.deferred_history_at_tail.take()?;
        if index != self.focused_pane || !self.focused_pane().is_some_and(|pane| pane.ready) {
            return None;
        }
        match self.begin_history_request(true) {
            DashboardAction::Request(request) => Some(request),
            _ => None,
        }
    }

    fn capture_mouse_action(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            let (copy, close) = super::render::capture_controls(area);
            if point_in_rect(mouse, close) {
                if self.mode == InputMode::History {
                    return self.leave_history();
                }
                self.cancel_copy(None);
                return DashboardAction::Redraw;
            }
            if point_in_rect(mouse, copy) {
                let key = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
                return if self.mode == InputMode::History {
                    self.history_key_action(key)
                } else {
                    self.copy_key_action(key)
                };
            }
        }
        let Some(mut inner) = self
            .pane_rects(area)
            .into_iter()
            .find(|pane| pane.pane_index == self.focused_pane)
            .map(|pane| pane.terminal)
        else {
            return DashboardAction::None;
        };
        if self.mode == InputMode::Copy {
            let Some(copy) = &mut self.copy else {
                return DashboardAction::None;
            };
            let (rows, cols) = copy.screen.size();
            inner.width = inner.width.min(cols);
            inner.height = inner.height.min(rows);
            let dragging = copy.dragging;
            if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
                copy.dragging = false;
            }
            if !point_in_rect(mouse, inner) {
                return DashboardAction::None;
            }
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    copy.point_at(super::copy::CopyPoint {
                        row: mouse.row - inner.y,
                        col: mouse.column - inner.x,
                    });
                    copy.anchor = Some(copy.cursor);
                    copy.dragging = true;
                }
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
                    if dragging =>
                {
                    copy.point_at(super::copy::CopyPoint {
                        row: mouse.row - inner.y,
                        col: mouse.column - inner.x,
                    });
                }
                _ => return DashboardAction::None,
            }
            self.copy_notice = None;
            return DashboardAction::Redraw;
        }
        inner = super::render::history_content_rect(inner);
        let dragging = self.history.as_ref().is_some_and(|view| view.dragging);
        if let Some(view) = &mut self.history
            && mouse.kind == MouseEventKind::Up(MouseButton::Left)
        {
            view.dragging = false;
        }
        if !point_in_rect(mouse, inner) {
            return DashboardAction::None;
        }
        if is_wheel(mouse.kind) {
            return self.history_wheel_action(mouse, area);
        }
        let size = history_view_size(self.focused_size());
        let Some(view) = &mut self.history else {
            return DashboardAction::None;
        };
        let starting = mouse.kind == MouseEventKind::Down(MouseButton::Left);
        if !starting
            && !(matches!(
                mouse.kind,
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
            ) && dragging)
        {
            return DashboardAction::None;
        }
        let point = HistoryCopyPoint {
            row: view.top.saturating_add(u32::from(mouse.row - inner.y)),
            col: view.left.saturating_add(mouse.column - inner.x),
        };
        // Only painted, loaded cells can start a gesture. This keeps page arrival
        // from creating an anchor after a release or a click on an empty row.
        let painted = super::render::history_cell_at(view, point.row, point.col).is_some_and(
            |(_, row, cell)| {
                point.col < row.width
                    && (cell.width != 2 || mouse.column + 1 < inner.right())
                    && (cell.width != 0 || mouse.column > inner.x)
            },
        );
        if !painted {
            return DashboardAction::None;
        }
        view.copy_job = None;
        view.copy_completion = None;
        if starting {
            view.anchor = None;
            view.dragging = false;
        }
        view.cursor_target = Some(HistoryCursorTarget::At(point));
        view.cursor_reveal = false;
        match view.resolve_cursor(size) {
            Ok(true) => {
                if starting {
                    view.anchor = view.cursor.map(|cursor| cursor.point);
                    view.dragging = view.anchor.is_some();
                }
            }
            Ok(false) => {}
            Err(error) => {
                self.copy_notice = Some(error.to_string());
                return DashboardAction::Redraw;
            }
        }
        self.history_page_error = false;
        self.error = None;
        self.copy_notice = None;
        self.history_request_if_needed()
            .map_or(DashboardAction::Redraw, DashboardAction::Request)
    }

    fn history_wheel_action(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        let Some(inner) = self
            .pane_rects(area)
            .into_iter()
            .find(|pane| pane.pane_index == self.focused_pane)
            .map(|pane| pane.terminal)
        else {
            return DashboardAction::None;
        };
        if !point_in_rect(mouse, inner) {
            return DashboardAction::None;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp => self.history_scroll_rows(-1),
            MouseEventKind::ScrollDown => {
                let size = history_view_size(self.focused_size());
                let at_tail = self.history.as_ref().is_some_and(|view| {
                    let max_top = view.opened.total_rows.saturating_sub(u32::from(size.rows));
                    view.top >= max_top
                });
                if at_tail {
                    self.leave_history()
                } else {
                    self.history_scroll_rows(1)
                }
            }
            _ => DashboardAction::None,
        }
    }

    fn history_scroll_rows(&mut self, delta: i32) -> DashboardAction {
        let size = history_view_size(self.focused_size());
        let Some(view) = self.history.as_mut() else {
            return DashboardAction::None;
        };
        let max_top = view.opened.total_rows.saturating_sub(u32::from(size.rows));
        let next = if delta < 0 {
            view.top.saturating_sub(delta.unsigned_abs())
        } else {
            view.top.saturating_add(delta.unsigned_abs())
        }
        .min(max_top);
        if next == view.top {
            return DashboardAction::Redraw;
        }
        view.top = next;
        view.cursor_target =
            (view.opened.total_rows > 0).then_some(HistoryCursorTarget::At(HistoryCopyPoint {
                row: view.top,
                col: view.left,
            }));
        view.cursor_reveal = false;
        self.history_page_error = false;
        self.error = None;
        self.copy_notice = None;
        self.history_request_if_needed()
            .map_or(DashboardAction::Redraw, DashboardAction::Request)
    }

    fn queue_held_releases(&mut self) {
        let mut bytes = Vec::new();
        let mut session = None;
        for held in self.mouse.held.iter_mut() {
            let Some(gesture) = held.take() else {
                continue;
            };
            session = Some(gesture.session);
            let button = match gesture.event.kind {
                MouseEventKind::Down(button)
                | MouseEventKind::Up(button)
                | MouseEventKind::Drag(button) => button,
                _ => continue,
            };
            let release = MouseEvent {
                kind: MouseEventKind::Up(button),
                ..gesture.event
            };
            if let Some(encoded) = encode_mouse(release, gesture.mode, gesture.encoding) {
                bytes.extend(encoded);
            }
        }
        self.mouse.last_motion = None;
        if bytes.is_empty() {
            return;
        }
        let Some(session) = session else {
            return;
        };
        let request_id = self.next_request_id();
        self.push_request(ClientMessage {
            request_id,
            request: Request::Input { session, bytes },
        });
    }

    fn clear_held_mouse(&mut self) {
        self.mouse.held = Default::default();
        self.mouse.last_motion = None;
    }

    fn reconcile_mouse_protocol(&mut self) {
        let mode = self.focused_mouse_mode();
        let encoding = self.focused_mouse_encoding();
        let stale = self
            .mouse
            .held
            .iter()
            .flatten()
            .any(|held| held.mode != mode || held.encoding != encoding);
        if stale {
            self.clear_held_mouse();
        }
    }

    pub(super) fn request_selected(&mut self) -> DashboardAction {
        let _ = self.request_view_at(self.outer_area);
        self.drained_action()
    }

    fn pause_request(&mut self, paused: bool) -> DashboardAction {
        let Some(session) = self.action_session().and_then(|id| find_session(self, id)) else {
            self.set_error("No session selected");
            return DashboardAction::Redraw;
        };
        if matches!(session.phase, SessionPhase::Exited { .. }) {
            self.set_error("Session exited");
            return DashboardAction::Redraw;
        }
        let session = session.id;
        let request_id = self.error_owning_request_id();
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
        if let ServerMessage::Response { request_id, .. } = &message
            && self.ignored_responses.remove(request_id)
        {
            return Vec::new();
        }
        if let ServerMessage::Response {
            request_id,
            response,
        } = &message
            && let Some(requests) = self.palette_response(*request_id, response)
        {
            for request in requests {
                self.push_request(request);
            }
            return self.drain_outbox();
        }
        let (was_view_request, owns_error) = match &message {
            ServerMessage::Response {
                request_id,
                response,
            } => self.retire_request_id(*request_id, response),
            ServerMessage::Event(_) => (false, false),
        };
        match message {
            ServerMessage::Response {
                request_id,
                response,
            } => match response {
                Response::AgentOperation(_) => {}
                Response::Hierarchy(hierarchy) => {
                    for request in self.update_hierarchy(hierarchy) {
                        self.push_request(request);
                    }
                }
                Response::Screen {
                    session,
                    revision,
                    size,
                    bytes,
                } => {
                    let matched_screen = self.pending_view.as_ref().is_some_and(|pending| {
                        pending.request_id == request_id
                            && pending.view.revision == revision
                            && pending.view.targets.iter().any(|(id, _)| *id == session)
                    });
                    if matched_screen {
                        self.apply_screen(revision, session, size, &bytes);
                        if let Some(view) = self.history.as_mut()
                            && view.opened.session == session
                        {
                            view.new_output = true;
                        }
                    }
                }
                Response::Ok
                    if self
                        .pending_view
                        .as_ref()
                        .is_some_and(|pending| pending.request_id == request_id) =>
                {
                    let pending = self.pending_view.take().expect("matching view request");
                    let desired_now = self.desired_view();
                    let desired_matches =
                        !self.force_view_refresh && Self::same_view(&pending.view, &desired_now);
                    let complete = pending
                        .view
                        .targets
                        .iter()
                        .all(|(session, _)| self.pending_snapshot_sessions.contains(session));
                    self.pending_snapshot_sessions.clear();
                    if !complete {
                        // `Ok` is final and every snapshot precedes it, so a missing one is a
                        // failed view rather than one still arriving.
                        for (session, _) in &pending.view.targets {
                            if let Some(pane) = self
                                .panes
                                .iter_mut()
                                .find(|pane| pane.session == Some(*session))
                            {
                                pane.ready = false;
                                pane.snapshot_installed = false;
                            }
                        }
                        self.force_view_refresh = true;
                        let next_id = self.next_request_id();
                        if let Ok(Some(request)) = self.view_request(self.outer_area, next_id) {
                            self.push_request(request);
                        }
                        // Recorded after the refresh so the first retry is immediate and a
                        // server that keeps answering without snapshots is throttled.
                        self.failed_view = Some((pending.view, Instant::now()));
                    } else if desired_matches && !pending.parser_discarded {
                        for (session, _) in &pending.view.targets {
                            if let Some(pane) = self
                                .panes
                                .iter_mut()
                                .find(|pane| pane.session == Some(*session))
                            {
                                pane.snapshot_installed = true;
                                pane.ready = true;
                            }
                        }
                        self.requested_view = Some(pending.view);
                        self.failed_view = None;
                        // A server-driven refresh completes the same way a user's selection does,
                        // so the banner is cleared only when a refused view wrote it or when this
                        // view is the one the user's own selection, split, focus, or pane close
                        // asked for.
                        if self.error_owned_by_view || owns_error {
                            self.error = None;
                            self.error_owned_by_view = false;
                        }
                        if let Some(request) = self.take_deferred_history_request() {
                            self.push_request(request);
                        }
                    } else {
                        self.requested_view = Some(pending.view);
                        self.force_view_refresh |= pending.parser_discarded;
                        let next_id = self.next_request_id();
                        if let Ok(Some(request)) = self.view_request(self.outer_area, next_id) {
                            self.push_request(request);
                        }
                    }
                }
                Response::Ok => {
                    // Only a request the user asked for owns the banner; a view ack, the geometry
                    // handshake, and a synthetic mouse release leave a fresh error in place.
                    if owns_error && !self.history_page_error {
                        self.error = None;
                        self.error_owned_by_view = false;
                    }
                }
                Response::CreatedSession(_)
                | Response::Inventory { .. }
                | Response::TerminalText { .. }
                | Response::Task(_) => self.error = None,
                Response::HistoryOpened(opened) => {
                    self.accept_history_opened(request_id, opened);
                }
                Response::HistoryRows(page) => {
                    let purpose = self
                        .history
                        .as_ref()
                        .and_then(|view| view.pending.as_ref())
                        .filter(|pending| pending.request_id == request_id)
                        .map(|pending| pending.purpose);
                    let accepted = self.history.as_mut().map(|view| {
                        view.accept_page(request_id, page)
                            .map(|page| (view.opened.clone(), page))
                    });
                    match (purpose, accepted) {
                        (Some(purpose), Some(Ok((opened, Some(page))))) => {
                            self.history_page_error = false;
                            self.handle_history_page(purpose, opened, page);
                            if let Some(request) = self.history_request_if_needed() {
                                self.push_request(request);
                            }
                        }
                        (Some(_), Some(Ok((_opened, None)))) => {}
                        (Some(_), Some(Err(error))) => {
                            self.history_page_error = true;
                            self.set_error(error.to_string());
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
                    let matched_view = self
                        .pending_view
                        .as_ref()
                        .is_some_and(|pending| pending.request_id == request_id);
                    if matched_view {
                        let pending = self.pending_view.take().expect("matching view request");
                        self.pending_snapshot_sessions.clear();
                        for (session, _) in &pending.view.targets {
                            if let Some(pane) = self
                                .panes
                                .iter_mut()
                                .find(|pane| pane.session == Some(*session))
                            {
                                pane.ready = false;
                                pane.snapshot_installed = false;
                                pane.error = Some(message.clone());
                            }
                        }
                        // No view is acknowledged any more, so input stays revoked and the
                        // refused view is recorded for one backoff-delayed retry.
                        self.requested_view = None;
                        self.failed_view = Some((pending.view, Instant::now()));
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
                                let end_request_id = self.error_owning_request_id();
                                self.push_request(ClientMessage {
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
                    if matched_view || !was_view_request {
                        self.set_error(format!("{code:?}: {message}"));
                        // Only a refused view hands the banner to the next view completion.
                        self.error_owned_by_view = matched_view;
                    }
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
                    for request in self.update_hierarchy(hierarchy) {
                        self.push_request(request);
                    }
                }
                ServerEvent::Output {
                    session,
                    revision,
                    bytes,
                } => {
                    if revision == self.view_revision
                        && self
                            .panes
                            .iter()
                            .any(|pane| pane.session == Some(session) && pane.ready)
                    {
                        if let Some(pane) = self
                            .panes
                            .iter_mut()
                            .find(|pane| pane.session == Some(session) && pane.ready)
                        {
                            pane.parser.process(&bytes);
                        }
                        if self.focused_session() == Some(session) {
                            self.reconcile_mouse_protocol();
                            if let Some(view) = self.history.as_mut()
                                && view.opened.session == session
                            {
                                view.new_output = true;
                            }
                        }
                    }
                }
                ServerEvent::ScreenDirty { session, revision } => {
                    if revision == self.view_revision
                        && let Some(pane) = self
                            .panes
                            .iter_mut()
                            .find(|pane| pane.session == Some(session))
                    {
                        pane.ready = false;
                        pane.snapshot_installed = false;
                        self.force_view_refresh = true;
                        if let Some(view) = self.history.as_mut()
                            && view.opened.session == session
                        {
                            view.new_output = true;
                        }
                        if self.pending_view.is_none() {
                            let request_id = self.next_request_id();
                            if let Ok(Some(request)) =
                                self.view_request(self.outer_area, request_id)
                            {
                                self.push_request(request);
                            }
                        }
                    }
                }
                ServerEvent::SessionChanged(summary) => {
                    self.observe_desktop_update(&summary);
                    for session in self
                        .hierarchy
                        .projects
                        .iter_mut()
                        .flat_map(|project| project.workspaces.iter_mut())
                        .flat_map(|workspace| workspace.sessions.iter_mut())
                    {
                        if session.id == summary.id {
                            *session = *summary;
                            break;
                        }
                    }
                    self.update_mode_for_selected_phase();
                }
            },
        }
        self.drain_outbox()
    }

    fn handle_history_page(
        &mut self,
        purpose: HistoryPagePurpose,
        opened: HistoryOpened,
        page: HistoryRows,
    ) {
        match purpose {
            HistoryPagePurpose::Viewport | HistoryPagePurpose::Cursor => {
                let size = history_view_size(self.focused_size());
                if let Some(view) = self.history.as_mut() {
                    view.cache_page(page);
                    if matches!(purpose, HistoryPagePurpose::Cursor)
                        && let Err(error) = view.resolve_cursor(size)
                    {
                        self.history_page_error = true;
                        self.set_error(error.to_string());
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

    pub(crate) fn take_pending_history_copy(&mut self) -> Option<String> {
        let focused_session = self.focused_session();
        let view = self.history.as_mut()?;
        let completion = view.copy_completion.take()?;
        let valid = self.mode == InputMode::History
            && focused_session == Some(completion.range.session)
            && view.opened.session == completion.range.session
            && view.opened.snapshot == completion.range.snapshot
            && view.copy_range() == Some(completion.range);
        if valid { Some(completion.text) } else { None }
    }

    fn accept_history_opened(&mut self, request_id: u64, opened: crate::protocol::HistoryOpened) {
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
        if pending.cancelled || self.focused_session() != Some(opened.session) {
            let end_request_id = self.error_owning_request_id();
            self.push_request(ClientMessage {
                request_id: end_request_id,
                request: Request::HistoryEnd {
                    session: opened.session,
                    snapshot: opened.snapshot,
                },
            });
            return;
        }
        self.copy_notice = None;
        let replaced = self.history.take();
        if let Some(view) = replaced {
            let end_request_id = self.error_owning_request_id();
            self.push_request(ClientMessage {
                request_id: end_request_id,
                request: Request::HistoryEnd {
                    session: view.opened.session,
                    snapshot: view.opened.snapshot,
                },
            });
        }
        let size = history_view_size(self.focused_size());
        let max_top = opened.total_rows.saturating_sub(u32::from(size.rows));
        let top = if pending.at_tail {
            max_top
        } else {
            opened
                .total_rows
                .saturating_sub(u32::from(size.rows).saturating_mul(2))
                .min(max_top)
        };
        self.history = Some(HistoryView::new(opened, top));
        self.mode = InputMode::History;
        if let Some(request) = self.history_request_if_needed() {
            self.push_request(request);
        }
    }

    fn update_hierarchy(&mut self, hierarchy: HierarchySnapshot) -> Vec<ClientMessage> {
        let focused_before = self.focused_session();
        self.observe_desktop_responses(&hierarchy);
        self.hierarchy = hierarchy;
        self.cancel_copy_if_session_missing();
        self.update_mode_for_selected_phase();
        let mut outgoing = Vec::new();
        let existing = self
            .hierarchy
            .projects
            .iter()
            .flat_map(|project| project.workspaces.iter())
            .flat_map(|workspace| workspace.sessions.iter())
            .map(|session| session.id)
            .collect::<HashSet<_>>();
        let removed = self
            .panes
            .iter()
            .any(|pane| pane.session.is_some_and(|id| !existing.contains(&id)));
        if removed {
            self.mark_pending_parser_discarded();
            // `retain` below shifts pane indices and `focused_pane` follows them, so a parked
            // wheel deferral no longer names the pane the user scrolled. A surviving focused
            // session skips `invalidate_view_readiness`, so clear it here rather than leaning on
            // the downstream focus guard.
            self.deferred_history_at_tail = None;
            let focused_survives = focused_before.is_some_and(|id| existing.contains(&id));
            self.panes
                .retain(|pane| pane.session.is_some_and(|id| existing.contains(&id)));
            if !focused_survives {
                self.release_for_selection_change();
                self.invalidate_view_readiness();
                self.mode = InputMode::Browse;
            }
            if self.panes.is_empty() {
                self.panes.push(super::PaneState::new(self.focused_size()));
            }
            self.focused_pane = self.focused_pane.min(self.panes.len() - 1);
            if self.panes.iter().all(|pane| pane.session.is_none())
                && let Some(id) = self.visible_rows().iter().find_map(|row| match row {
                    TreeRow::Session { id } => Some(*id),
                    TreeRow::Project { .. } | TreeRow::Workspace { .. } => None,
                })
            {
                self.select_session(id);
            }
            let request_id = self.next_request_id();
            if let Ok(Some(request)) = self.view_request(self.outer_area, request_id) {
                outgoing.push(request);
            }
        }
        self.update_mode_for_selected_phase();
        outgoing.extend(self.attach_created_workspace());
        outgoing
    }

    fn mark_reviewed_request(&mut self) -> DashboardAction {
        let Some(session) = self.action_session() else {
            return DashboardAction::None;
        };
        let Some(expected) = self.unread.review_target(session) else {
            return DashboardAction::None;
        };
        DashboardAction::Request(ClientMessage {
            request_id: self.error_owning_request_id(),
            request: Request::MarkReviewed { session, expected },
        })
    }

    pub fn input_request(&mut self, bytes: Vec<u8>, request_id: u64) -> Option<ClientMessage> {
        if self.whichkey.is_some()
            || self.palette.is_some()
            || self.tasks.is_some()
            || self.mode != InputMode::Terminal
            || !self.input_is_allowed()
        {
            return None;
        }
        let session = self.focused_session()?;
        self.error_owning_requests.insert(request_id);
        Some(ClientMessage {
            request_id,
            request: Request::Input { session, bytes },
        })
    }

    pub(super) fn selected_phase(&self) -> Option<&SessionPhase> {
        self.action_session()
            .and_then(|id| find_session(self, id))
            .map(|session| &session.phase)
    }

    pub(super) fn input_is_allowed(&self) -> bool {
        let Some(session) = self.action_session() else {
            return false;
        };
        matches!(self.selected_phase(), Some(SessionPhase::Running))
            && self.focused_pane().is_some_and(|pane| pane.ready)
            && self.requested_view.as_ref().is_some_and(|view| {
                view.focused == Some(session)
                    && view.targets.iter().any(|(target, _)| *target == session)
            })
    }

    fn refuse_input(&mut self) -> DashboardAction {
        let refusal: String = match self.selected_phase() {
            Some(SessionPhase::Paused) => "Session paused; press r to resume".into(),
            Some(SessionPhase::Exited { .. }) => "Session exited".into(),
            None => "No session selected".into(),
            Some(SessionPhase::Running) if !self.focused_pane().is_some_and(|pane| pane.ready) => {
                "Pane is loading; retry input".into()
            }
            Some(SessionPhase::Running) => return DashboardAction::None,
        };
        self.set_error(refusal);
        DashboardAction::Redraw
    }

    fn update_mode_for_selected_phase(&mut self) {
        if self.mode != InputMode::History
            && self.mode != InputMode::Copy
            && self.selected_phase() == Some(&SessionPhase::Paused)
        {
            let leaving_terminal = self.mode != InputMode::Browse;
            self.mode = InputMode::Browse;
            if leaving_terminal {
                // A held button would otherwise release against a paused session.
                self.cancel_mouse_gesture();
            }
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

    /// An id for a request the user asked for, whose failure shows in the error banner and whose
    /// plain `Ok` may therefore clear it. The set is released by every final response.
    fn error_owning_request_id(&mut self) -> u64 {
        let id = self.next_request_id();
        self.error_owning_requests.insert(id);
        id
    }

    /// Retires a request id once its final response arrives and reports what that id owned: a
    /// `SetView` that was still in flight, and the error banner. Only a view snapshot is not
    /// final, so every other response releases the id; both sets would otherwise grow for the
    /// life of the dashboard.
    fn retire_request_id(&mut self, request_id: u64, response: &Response) -> (bool, bool) {
        let was_view_request = self.view_request_ids.contains(&request_id);
        let owns_error = self.error_owning_requests.contains(&request_id);
        if !matches!(response, Response::Screen { .. }) {
            self.view_request_ids.remove(&request_id);
            self.error_owning_requests.remove(&request_id);
        }
        (was_view_request, owns_error)
    }
}
pub(super) fn find_workspace<'a>(
    dashboard: &'a Dashboard,
    project: &str,
    name: &str,
) -> Option<&'a ovrcr_protocol::WorkspaceSummary> {
    dashboard
        .hierarchy
        .projects
        .iter()
        .find(|candidate| candidate.name == project)
        .and_then(|candidate| {
            candidate
                .workspaces
                .iter()
                .find(|workspace| workspace.name == name)
        })
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

use super::copy::{
    CopyMotion, CopySelection, HistoryCopyCompletion, HistoryCopyJob, HistoryCopyPoint,
    HistoryCopyRange, MAX_COPY_BYTES,
};
use super::event_loop::DASHBOARD_IDLE_REDRAW_INTERVAL;
use super::input::{encode_key, encode_mouse, is_browse_key};
use super::render::{tree_line_at, tree_line_count, tree_row_heights, tree_row_start};
use super::settings::Settings;
use super::status::SPINNER_INTERVAL;
use super::view_handshake::{Acknowledged, Desire, RequestedView};
use super::{Dashboard, DashboardAction, InputMode, KeyEncoding, TreeRow, history_view_size};
use crate::protocol::{
    ClientMessage, ErrorCode, HierarchySnapshot, HistoryOpened, HistoryRows, HistorySnapshotId,
    QuotaSnapshot, Request, Response, ServerEvent, ServerMessage, SessionRunId,
};
use crate::session::{SessionId, SessionPhase, TerminalSize};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ovrcr_terminal::encode_paste;
use ovrcr_terminal::vt100;
use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const HISTORY_WHEEL_ROWS: i32 = 3;

fn unknown_prompt_snapshot(
    id: crate::protocol::InitialPromptIntentId,
) -> crate::protocol::InitialPromptIntentSnapshot {
    crate::protocol::InitialPromptIntentSnapshot {
        id,
        state: crate::protocol::InitialPromptIntentState::Unknown,
        project: String::new(),
        workspace: String::new(),
        name: String::new(),
        label: None,
        session: None,
        run: None,
        recoverable_prompt: None,
        recoverable_launch: None,
        message: Some("Launch status is unknown. Check for the agent before trying again.".into()),
        delivery: None,
    }
}

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

fn pane_bounds(pane: super::PaneRects) -> Rect {
    Rect::new(
        pane.metadata.x,
        pane.metadata.y,
        pane.metadata.width,
        pane.metadata.height.saturating_add(pane.terminal.height),
    )
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

/// Every way a pane change can move: which release rules apply is decided by
/// the variant, not the caller. See `Dashboard::retarget`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaneChange {
    Split,
    Focus,
    Close,
    Select,
    Container,
    /// The tree has no sessions; the focused pane is being emptied.
    Cleared,
    /// Drawn pane geometry changed (split drag, sidebar drag). History keeps its frozen cells.
    Resize,
    /// The server removed a session shown in a pane.
    ServerRemoved {
        focused_survives: bool,
    },
}

pub(super) struct AgentTyping {
    session: SessionId,
    run: SessionRunId,
    revision: u64,
    /// The production loop must discard already-pending input before acknowledgement cutover.
    defer_until_input_boundary: bool,
    pub(super) rejected: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SidebarJumpTarget {
    pub(super) session: SessionId,
    pub(super) run: SessionRunId,
    phase: SidebarJumpPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SidebarJumpPhase {
    Running,
    Retained,
}

impl SidebarJumpTarget {
    fn capture(session: &crate::session::SessionSummary) -> Option<Self> {
        if session.scope != crate::protocol::SessionScope::Workspace
            || session.archived
            || !matches!(session.kind, crate::protocol::SessionKind::Agent { .. })
        {
            return None;
        }
        let phase = match &session.phase {
            SessionPhase::Running => SidebarJumpPhase::Running,
            SessionPhase::Paused => return None,
            SessionPhase::Exited { .. } | SessionPhase::Stopped | SessionPhase::Interrupted => {
                SidebarJumpPhase::Retained
            }
        };
        Some(Self {
            session: session.id,
            run: session.run,
            phase,
        })
    }

    fn still_matches(self, session: &crate::session::SessionSummary) -> bool {
        session.scope == crate::protocol::SessionScope::Workspace
            && !session.archived
            && session.run == self.run
            && matches!(session.kind, crate::protocol::SessionKind::Agent { .. })
            && matches!(
                (&self.phase, &session.phase),
                (SidebarJumpPhase::Running, SessionPhase::Running)
                    | (
                        SidebarJumpPhase::Retained,
                        SessionPhase::Exited { .. }
                            | SessionPhase::Stopped
                            | SessionPhase::Interrupted
                    )
            )
    }
}

pub(super) struct SidebarJump {
    targets: Vec<SidebarJumpTarget>,
    hint: Option<String>,
}

impl Dashboard {
    pub fn new(size: TerminalSize) -> Self {
        Self {
            operator: Default::default(),
            tasks: None,
            desktop: super::desktop::DesktopNotifications::default(),
            unread: Default::default(),
            quotas: Some(QuotaSnapshot::default()),
            details: None,
            transcript: Default::default(),
            settings_report: None,
            settings_editor: Default::default(),
            events: Default::default(),
            hierarchy: HierarchySnapshot {
                projects: Vec::new(),
            },
            mode: InputMode::Browse,
            panes: vec![super::PaneState::new(size)],
            focused_pane: 0,
            pane_layout: super::PaneLayout::Pane(0),
            handshake: Default::default(),
            navigation: Default::default(),
            agent_typing: None,
            sidebar_jump: None,
            recovery_requests: Default::default(),
            outer_area: Rect::new(0, 0, size.cols, size.rows),
            collapsed_projects: HashSet::new(),
            collapsed_workspaces: HashSet::new(),
            error: None,
            notice_queue: VecDeque::new(),
            error_owned_by_view: false,
            error_is_limit: false,
            error_since: None,
            copy: None,
            copy_notice: None,
            history: None,
            history_begin_request: None,
            mouse: super::MouseForwarding::default(),
            sidebar_width: None,
            sidebar_hidden: false,
            mouse_focused: true,
            deferred_history_at_tail: None,
            tree_offset: 0,
            next_request_id: 1,
            palette: None,
            model_query: None,
            #[cfg(test)]
            model_agent_entries: None,
            whichkey: None,
            sidebar_create_menu: None,
            selected_container: None,
            configuration_paths: None,
            connect_status: None,
            history_page_error: false,
            outbox: super::outbox::Outbox::default(),
            wip_prompts: VecDeque::new(),
            ignored_responses: HashSet::new(),
            lifecycle_pending: None,
            provisional: None,
            lifecycle_strip: None,
            lifecycle_preference: None,
            settings: Settings::default(),
            theme: crate::theme::ThemeTokens::dark(),
            config_dir: std::path::PathBuf::new(),
            error_owning_requests: HashSet::new(),
            input_requests: HashSet::new(),
            pending_initial_prompt_allocation: None,
            initial_prompt_intent: None,
            initial_prompt_intent_id: None,
            initial_prompt_focus_pending: None,
            deferred_initial_prompt_recovery: None,
            initial_prompt_reconcile_request: None,
        }
    }

    pub(super) fn session_display_name(&self, session: &crate::session::SessionSummary) -> String {
        let duplicate = session.title.is_some()
            && self
                .hierarchy
                .projects
                .iter()
                .flat_map(|p| &p.workspaces)
                .flat_map(|w| &w.sessions)
                .any(|s| s.id != session.id && s.display_name() == session.display_name());
        if duplicate {
            format!("{} (#{})", session.display_name(), session.id.0)
        } else {
            session.display_name().into()
        }
    }

    pub fn install_hierarchy(&mut self, hierarchy: HierarchySnapshot) {
        let _ = self.update_hierarchy(hierarchy);
    }

    pub fn install_settings(&mut self, settings: Settings) {
        let sidebar_view_changed = self.settings.sidebar_view != settings.sidebar_view;
        self.theme = crate::theme::ThemeTokens::for_id(settings.theme);
        self.settings = settings;
        if sidebar_view_changed {
            self.ensure_selection_visible(&self.visible_rows());
        }
    }

    /// Active Design tokens for the current settings theme.
    pub(crate) fn theme_tokens(&self) -> crate::theme::ThemeTokens {
        if self
            .details
            .is_some_and(|(details, _)| details == super::quota::Details::Settings)
            && let Some(theme) = self.settings_editor.preview_theme
        {
            return crate::theme::ThemeTokens::for_id(theme);
        }
        self.theme
    }

    /// Adopt the Server's reading: every setting, including the ones a
    /// `SetSetting` request asked it to change. The footer names a changed
    /// alert or automatic-terminal setting, and the findings once at attach and
    /// again whenever their number changes.
    fn install_settings_report(&mut self, report: crate::protocol::SettingsReport) {
        let previous = self
            .settings_report
            .as_ref()
            .map(|report| report.findings.len());
        let count = report.findings.len();
        let before = std::mem::replace(&mut self.settings, report.settings.clone());
        self.theme = crate::theme::ThemeTokens::for_id(self.settings.theme);
        if before.sidebar_view != self.settings.sidebar_view {
            self.ensure_selection_visible(&self.visible_rows());
        }
        self.settings_editor.report_published(&report);
        if self.settings_editor.preview_request.is_some()
            && self.settings_editor.preview_theme == Some(self.settings.theme)
        {
            // The persisted reading confirms the highlighted preview was
            // saved. The cached persisted palette now supplies the same colors.
            self.settings_editor.preview_theme = None;
            self.settings_editor.preview_request = None;
        }
        if previous.is_some() {
            self.settings_changed_notice(&before);
        }
        if previous.map_or(count > 0, |previous| previous != count) {
            self.show_notice(super::settings::findings_notice(&self.settings, count));
        }
        self.apply_alert_channels(
            #[cfg(target_os = "macos")]
            &before,
        );
        self.settings_report = Some(Box::new(report));
    }

    pub fn install_config_dir(&mut self, dir: std::path::PathBuf) {
        self.config_dir = dir;
    }

    pub fn install_area(&mut self, area: Rect) {
        if self.outer_area != area && self.sidebar_jump.is_some() {
            self.cancel_sidebar_jump();
        }
        self.outer_area = area;
        self.settings_editor.resize(area);
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

    /// Fixture seam: puts a session in the state a `ScreenDirty` leaves it in, by marking it
    /// stale and forcing the next `desire` to refresh. Stays `pub` for the root crate's
    /// `tests/tui` suites, which use it alongside this crate's own tests.
    pub fn install_unready(&mut self, session: SessionId) {
        self.handshake.mark_stale(session);
    }

    /// Fixture seam: seeds a pane's parser with `bytes` and counts the resulting desired view
    /// as acknowledged, discarding any in-flight request — the shortest path to a ready pane
    /// without a server. Stays `pub` for the root crate's `tests/tui` suites, which use it
    /// alongside this crate's own tests.
    pub fn install_screen(&mut self, session: SessionId, bytes: &[u8]) {
        let run = find_session(self, session).map(|summary| summary.run);
        if !self.panes.iter().any(|pane| pane.session == Some(session)) {
            let index = self
                .panes
                .iter()
                .position(|pane| pane.session.is_none())
                .unwrap_or(self.focused_pane);
            if let Some(pane) = self.panes.get_mut(index) {
                pane.session = Some(session);
                pane.run = run;
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
        pane.run = pane.run.or(run);
        pane.parser = vt100::Parser::new(size.rows, size.cols, 0);
        pane.parser.process(bytes);
        pane.error = None;
        let rows = self.visible_rows();
        self.ensure_selection_visible(&rows);
        if self.focused_session() == Some(session) {
            self.reconcile_mouse_protocol();
        }
        let desired = self.desired_view();
        self.handshake.install_acknowledged(desired);
    }

    /// Shows `message` in the error banner as a failure, behind a red `ERROR:` marker. A
    /// refused view is the only writer whose banner a later view completion clears, so every
    /// other writer comes through here and releases it. A notice already in the footer waits
    /// behind the banner instead of being covered and lost.
    pub(super) fn set_error(&mut self, message: impl Into<String>) {
        self.raise_banner(message.into(), false);
    }

    /// An expected limit (`Pane is loading; retry history`): the error banner's slot and
    /// lifetime, read muted and without the `ERROR:` marker.
    pub(super) fn set_limit(&mut self, message: impl Into<String>) {
        self.raise_banner(message.into(), true);
    }

    /// A Server refusal in plain words (see [`Self::plain_error`]). Only the typed refusals
    /// that say what blocks the action (sessions or workspaces remain, a session running,
    /// uncommitted changes, a name taken) read as limits. `Conflict` is the Server's
    /// catch-all for failed lifecycle work, so it stays a failure here; a caller that knows
    /// its refusal is a limit (a refused view) uses [`Self::set_limit`] itself.
    pub(super) fn set_server_error(&mut self, code: ErrorCode, message: &str) {
        let text = self.plain_error(message);
        match code {
            ErrorCode::AlreadyExists
            | ErrorCode::DirtyWorktree
            | ErrorCode::SessionRunning
            | ErrorCode::SessionsRemain
            | ErrorCode::WorkspacesRemain => self.set_limit(text),
            ErrorCode::Conflict
            | ErrorCode::InvalidRequest
            | ErrorCode::NotFound
            | ErrorCode::PartialFailure
            | ErrorCode::Internal
            | ErrorCode::OwnershipUncertain => self.set_error(text),
        }
    }

    /// Server and internal error text in plain words, for the footer and for any form that
    /// shows a refusal inline: no `Conflict:`-style code in front, `project/<workspace id>`
    /// read as `project / branch` and a bare workspace id as its branch, and `#N` as that
    /// session's title. An id that continues a path stays, since a worktree's real folder
    /// may be named after it, and an id nothing names stays as written. Inline form
    /// errors add their own cleanups on top in `plain_reason`.
    pub(super) fn plain_error(&self, message: &str) -> String {
        let mut text = message.to_string();
        for project in &self.hierarchy.projects {
            for workspace in &project.workspaces {
                text = replace_outside_paths(
                    &text,
                    &format!("{}/{}", project.name, workspace.id),
                    &workspace_heading(self, workspace),
                );
                if internal_id(&workspace.id) {
                    text = replace_outside_paths(
                        &text,
                        &workspace.id,
                        &workspace_label(self, workspace),
                    );
                }
            }
        }
        let mut titled = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(at) = rest.find('#') {
            titled.push_str(&rest[..at]);
            let digits = rest[at + 1..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
            match rest[at + 1..at + 1 + digits]
                .parse()
                .ok()
                .and_then(|id| find_session(self, SessionId(id)))
            {
                Some(session) => titled.push_str(&self.session_title(session)),
                None => titled.push_str(&rest[at..at + 1 + digits]),
            }
            rest = &rest[at + 1 + digits..];
        }
        titled.push_str(rest);
        titled
    }

    fn raise_banner(&mut self, message: String, limit: bool) {
        if let Some(notice) = self.desktop.notice.take() {
            self.notice_queue.push_front(notice);
        }
        self.error = Some(message);
        self.error_is_limit = limit;
        // A limit answers what the user just did; a failure may land unseen, so its clock
        // starts at the first key press after it appears.
        self.error_since = limit.then(Instant::now);
        self.error_owned_by_view = false;
        // Requests and coalesced selections predating this error cannot dismiss it.
        self.error_owning_requests.clear();
        self.handshake.note_error();
    }

    /// Drops a banner that has stood for [`BANNER_LIFETIME`] since its clock started (at once
    /// for a limit, at the first key press for a failure), so an error never lingers; the
    /// notice waiting behind it shows next. The production draw path calls this every frame.
    pub(super) fn expire_banner(&mut self, now: Instant) {
        if self.error.is_some()
            && self
                .error_since
                .is_some_and(|since| now.saturating_duration_since(since) >= BANNER_LIFETIME)
        {
            self.dismiss_error_banner();
            self.error_owned_by_view = false;
        }
    }

    /// Footer notice. The error banner owns the footer line, so a notice posted while it is
    /// showing waits until the banner is dismissed rather than being written where the next key
    /// would drop it unseen.
    pub(super) fn show_notice(&mut self, notice: impl Into<String>) {
        let notice = notice.into();
        if self.error.is_some() {
            self.notice_queue.push_back(notice);
        } else {
            self.desktop.notice = Some(notice);
        }
    }

    /// Drops the error banner and shows the notice that was waiting behind it, if the footer
    /// is not already saying something else.
    pub(super) fn dismiss_error_banner(&mut self) {
        if self.error.take().is_some() {
            self.reveal_queued_notice();
        }
    }

    pub(super) fn reveal_queued_notice(&mut self) {
        if self.error.is_none()
            && self.desktop.notice.is_none()
            && let Some(notice) = self.notice_queue.pop_front()
        {
            self.desktop.notice = Some(notice);
        }
    }

    pub fn view_revision(&self) -> u64 {
        self.handshake.revision()
    }

    /// A pane is ready when its session is in the acknowledged view with no request in
    /// flight and no `ScreenDirty` since. Never written, only derived.
    pub(super) fn pane_ready(&self, pane: &super::PaneState) -> bool {
        pane.session
            .is_some_and(|session| self.handshake.is_ready(session))
    }

    pub(super) fn sidebar_jump_modal_priority_open(&self) -> bool {
        self.tasks.is_some()
            || self.details.is_some()
            || self.palette.is_some()
            || self.whichkey.is_some()
            || self.copy.is_some()
            || self.history.is_some()
            || self.operator_quick_visible()
    }

    fn sidebar_jump_can_start(&self) -> bool {
        super::keymap::sidebar_jump_binding(self).is_some_and(|binding| binding.enabled())
    }

    /// Uses the rendered tree's exact heights, viewport and top-to-bottom row order.
    fn visible_sidebar_jump_targets(&self) -> Vec<SidebarJumpTarget> {
        if self.sidebar_preference() == Some(0) {
            return Vec::new();
        }
        let tree = self.sidebar_rects(self.outer_area).tree;
        let width = usize::from(tree.width);
        let viewport = usize::from(tree.height);
        // Five indentation cells hold the badge, then the status glyph, separator and title.
        if width < 8 || viewport == 0 {
            return Vec::new();
        }
        let rows = self.visible_rows();
        let heights = tree_row_heights(self, &rows, width);
        let end = self.tree_offset.saturating_add(viewport);
        rows.iter()
            .enumerate()
            .filter_map(|(index, row)| {
                let title_line = tree_row_start(&rows, &heights, index);
                if title_line < self.tree_offset || title_line >= end {
                    return None;
                }
                let TreeRow::Session { id } = row else {
                    return None;
                };
                if self.provisional_overlay_for(row).is_some() {
                    return None;
                }
                let session = find_session(self, *id)?;
                SidebarJumpTarget::capture(session)
            })
            .take(9)
            .collect()
    }

    fn sidebar_jump_target_available(&self, target: SidebarJumpTarget) -> bool {
        let row = TreeRow::Session { id: target.session };
        self.provisional_overlay_for(&row).is_none()
            && find_session(self, target.session)
                .is_some_and(|session| target.still_matches(session))
    }

    pub(super) fn sidebar_jump_label(&self, session: SessionId) -> Option<usize> {
        let jump = self.sidebar_jump.as_ref()?;
        let index = jump.targets.iter().position(|target| {
            target.session == session && self.sidebar_jump_target_available(*target)
        })?;
        Some(index + 1)
    }

    pub(super) fn sidebar_jump_footer_hint(&self) -> Option<String> {
        let jump = self.sidebar_jump.as_ref()?;
        if let Some(hint) = &jump.hint {
            return Some(format!("{hint}  Esc cancel"));
        }
        if jump.targets.is_empty() {
            return Some("No visible Agents  s Agents search".into());
        }
        Some("1–9 jump  Ctrl-g/Esc cancel".into())
    }

    fn cancel_sidebar_jump(&mut self) {
        self.sidebar_jump = None;
    }

    /// Keep the picker open for a digit and close it for everything else. A
    /// closing key then keeps its ordinary Dashboard, Operator, or program
    /// handling instead of being replayed into a child as a shortcut.
    fn dispatch_sidebar_jump_key(&mut self, key: KeyEvent) -> Option<DashboardAction> {
        self.sidebar_jump.as_ref()?;
        if key.kind != KeyEventKind::Press {
            return Some(DashboardAction::None);
        }
        if key.code == KeyCode::Esc {
            self.cancel_sidebar_jump();
            return Some(DashboardAction::Redraw);
        }
        if is_browse_key(key) {
            let terminal = self.mode == InputMode::Terminal;
            self.cancel_sidebar_jump();
            // Let the Terminal branch perform its ordinary Browse return,
            // including mouse and history cleanup; Browse just closes.
            return (!terminal).then_some(DashboardAction::Redraw);
        }
        if let KeyCode::Char(digit @ '0'..='9') = key.code
            && matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::CONTROL)
        {
            return Some(self.sidebar_jump_key(digit));
        }
        self.cancel_sidebar_jump();
        None
    }

    pub(super) fn begin_sidebar_jump(&mut self) -> DashboardAction {
        if !self.sidebar_jump_can_start() {
            return DashboardAction::None;
        }
        let targets = self.visible_sidebar_jump_targets();
        self.sidebar_jump = Some(SidebarJump {
            targets,
            hint: None,
        });
        DashboardAction::Redraw
    }

    fn sidebar_jump_key(&mut self, digit: char) -> DashboardAction {
        let target = digit
            .to_digit(10)
            .and_then(|value| value.checked_sub(1))
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| self.sidebar_jump.as_ref()?.targets.get(index).copied());
        let Some(target) = target else {
            if let Some(jump) = &mut self.sidebar_jump {
                jump.hint = Some("No visible Agent for that number".into());
            }
            return DashboardAction::Redraw;
        };
        if !self.sidebar_jump_target_available(target) {
            if let Some(jump) = &mut self.sidebar_jump {
                jump.hint = Some("Agent unavailable; label kept".into());
            }
            return DashboardAction::Redraw;
        }
        self.sidebar_jump = None;
        self.mode = InputMode::Browse;
        // Retained inspection only changes the view. Running destinations use the
        // existing acknowledged-view and input-drain boundary before typing.
        match target.phase {
            SidebarJumpPhase::Running => self.confirm_agent_switch(target.session, target.run),
            SidebarJumpPhase::Retained => {
                self.agent_typing = None;
                self.select_session(target.session);
                // Showing a retained Interrupted run is inspection, not recovery.
                // Match notification navigation's exact-run suppression before its
                // view request reaches the ordinary recovery hook.
                self.recovery_requests
                    .entry((target.session, target.run))
                    .or_insert(None);
                self.request_selected()
            }
        }
    }

    pub(super) fn confirm_agent_switch(
        &mut self,
        session: SessionId,
        run: SessionRunId,
    ) -> DashboardAction {
        self.select_session(session);
        let coalesced = self.handshake.in_flight();
        if coalesced {
            // Even a focus-only round trip must not inherit the older acknowledgement.
            self.mark_pending_parser_discarded();
        }
        let previous_revision = self.handshake.revision();
        let action = self.request_selected();
        // A coalesced choice owns the next revision, never the older request still in flight.
        let revision = if coalesced {
            previous_revision.checked_add(1)
        } else {
            Some(self.handshake.revision())
        };
        self.agent_typing = revision.map(|revision| AgentTyping {
            session,
            run,
            revision,
            defer_until_input_boundary: false,
            rejected: None,
        });
        self.finish_agent_switch();
        action
    }

    fn finish_agent_switch(&mut self) {
        let Some(intent) = &self.agent_typing else {
            return;
        };
        if self.handshake.revision() > intent.revision {
            self.agent_typing = None;
            self.set_error("Agent switch cancelled: view changed");
            return;
        }
        if self.mode != InputMode::Browse
            || self.palette.is_some()
            || self.whichkey.is_some()
            || self.tasks.is_some()
            || self.copy.is_some()
            || self.history.is_some()
            || self
                .history_begin_request
                .as_ref()
                .is_some_and(|begin| !begin.cancelled)
            || self.deferred_history_at_tail.is_some()
        {
            self.agent_typing = None;
            return;
        }
        if self.focused_session() != Some(intent.session)
            || !find_session(self, intent.session).is_some_and(|s| {
                s.run == intent.run
                    && s.phase == SessionPhase::Running
                    && !s.archived
                    && matches!(s.kind, crate::protocol::SessionKind::Agent { .. })
            })
        {
            self.agent_typing = None;
            self.set_error("Agent switch cancelled: destination changed or is no longer running");
            return;
        }
        let ready = self.handshake.acknowledged().is_some_and(|view| {
            view.revision == intent.revision
                && view.focused == Some(intent.session)
                && view
                    .targets
                    .iter()
                    .all(|(id, _, _)| self.handshake.is_ready(*id))
        });
        if ready && !intent.defer_until_input_boundary && self.input_is_allowed() {
            self.agent_typing = None;
            self.mode = InputMode::Terminal;
        }
    }

    pub(super) fn defer_agent_typing_until_input_boundary(&mut self) {
        if let Some(intent) = &mut self.agent_typing {
            intent.defer_until_input_boundary = true;
        }
    }

    pub(super) fn finish_agent_input_boundary(&mut self) -> bool {
        let previous_mode = self.mode;
        if let Some(intent) = &mut self.agent_typing {
            intent.defer_until_input_boundary = false;
        }
        self.finish_agent_switch();
        self.mode != previous_mode
    }

    /// How many panes the in-flight view expects snapshots for.
    pub fn pending_targets(&self) -> usize {
        self.handshake.pending_targets()
    }

    /// Queue a request for the next drain. A synthetic mouse release must precede the
    /// `SetView` that moves focus; every retarget calls `cancel_mouse_gesture` before
    /// `view_request`, so push order is send order.
    pub(super) fn push_request(&mut self, message: ClientMessage) {
        self.outbox.push(message);
    }

    /// Everything queued since the last drain, then the next history page if one is due.
    /// Every drain calls `history_request_if_needed`, which may resolve the history cursor and
    /// set a history page error as a side effect; a pending page suppresses it, so repeating
    /// the call is idempotent.
    pub fn drain_outbox(&mut self) -> Vec<ClientMessage> {
        self.queue_history_request();
        let drained = self.outbox.drain();
        self.input_requests.extend(
            drained
                .iter()
                .filter(|message| is_input(&message.request))
                .map(|message| message.request_id),
        );
        drained
    }

    pub(super) fn queue_history_request(&mut self) {
        if let Some(request) = self.history_request_if_needed() {
            self.push_request(request);
        }
    }

    /// The writer used to drain due History before sending action-carried requests.
    pub(super) fn enqueue_request(&mut self, message: ClientMessage) {
        self.queue_history_request();
        self.push_request(message);
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
        if self.operator.visible || self.selected_container.is_some() {
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
        self.split_pane_in(super::SplitDirection::Right)
    }

    pub(super) fn split_pane_in(&mut self, direction: super::SplitDirection) -> bool {
        if let Some(reason) = self.split_refusal(direction) {
            if matches!(reason, "no session selected" | "no other visible session") {
                self.set_error("No other visible session to split");
            } else {
                self.set_limit(format!("split refused: {reason}"));
            }
            return false;
        }
        let Some(session) = self.split_session_candidate() else {
            self.set_error("No other visible session to split");
            return false;
        };
        self.retarget(PaneChange::Split);
        let size = self.focused_size();
        let mut pane = super::PaneState::new(size);
        pane.session = Some(session);
        pane.run = find_session(self, session).map(|summary| summary.run);
        let new_index = self.focused_pane + 1;
        let mut layout = self.pane_layout.clone();
        if !layout.insert_after(self.focused_pane, direction, new_index) {
            self.set_error("Pane layout could not be split");
            return false;
        }
        self.panes.insert(new_index, pane);
        self.pane_layout = layout;
        self.focused_pane = new_index;
        if let Some(rect) = self
            .pane_rects(self.outer_area)
            .into_iter()
            .find(|rect| rect.pane_index == new_index)
        {
            let size = TerminalSize {
                rows: rect.terminal.height.max(1),
                cols: rect.terminal.width.max(1),
            };
            if let Some(pane) = self.panes.get_mut(new_index) {
                pane.size = size;
                pane.desired_size = size;
            }
        }
        true
    }

    pub(super) fn split_refusal(&self, direction: super::SplitDirection) -> Option<&'static str> {
        if self.panes.len() >= super::MAX_PANE_COUNT {
            return Some("four panes is the most");
        }
        if self.action_session().is_none() {
            return Some("no session selected");
        }
        if self.split_session_candidate().is_none() {
            return Some("no other visible session");
        }
        let Some(rect) = self
            .pane_rects(self.outer_area)
            .into_iter()
            .find(|rect| rect.pane_index == self.focused_pane)
        else {
            return Some("panes would be too small");
        };
        let width = rect.metadata.width;
        let height = rect.metadata.height.saturating_add(rect.terminal.height);
        match direction {
            super::SplitDirection::Right if width < 41 => {
                Some("panes would be narrower than 20 columns")
            }
            super::SplitDirection::Right if height < super::MIN_PANE_HEIGHT => {
                Some("panes would be shorter than 8 rows")
            }
            super::SplitDirection::Down if height < 17 => {
                Some("panes would be shorter than 8 rows")
            }
            super::SplitDirection::Down if width < super::MIN_PANE_WIDTH => {
                Some("panes would be narrower than 20 columns")
            }
            _ => None,
        }
    }

    fn split_session_candidate(&self) -> Option<SessionId> {
        let sessions = self
            .visible_rows()
            .into_iter()
            .filter_map(|row| match row {
                TreeRow::Session { id } => Some(id),
                TreeRow::Project { .. }
                | TreeRow::Workspace { .. }
                | TreeRow::ProvisionalWorkspace { .. }
                | TreeRow::ProvisionalSession { .. } => None,
            })
            .collect::<Vec<_>>();
        if sessions.is_empty() {
            return None;
        }
        let current = self.action_session()?;
        let already_shown = self
            .panes
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != self.focused_pane)
            .filter_map(|(_, pane)| pane.session)
            .collect::<HashSet<_>>();
        let start = sessions
            .iter()
            .position(|candidate| *candidate == current)?;
        (1..=sessions.len())
            .map(|offset| sessions[(start + offset) % sessions.len()])
            .find(|session| *session != current && !already_shown.contains(session))
    }

    pub(crate) fn focus_pane(&mut self, index: usize) -> bool {
        if index >= self.panes.len() || index == self.focused_pane {
            return false;
        }
        self.retarget(PaneChange::Focus);
        self.focused_pane = index;
        self.selected_container = None;
        true
    }

    pub(crate) fn close_focused_pane(&mut self) -> bool {
        if self.panes.len() < 2 {
            return false;
        }
        let removed = self.focused_pane;
        let sibling = self.pane_layout.sibling_first(removed).unwrap_or(removed);
        self.retarget(PaneChange::Close);
        let layout = std::mem::replace(&mut self.pane_layout, super::PaneLayout::Pane(0));
        self.pane_layout = layout.remove(removed).unwrap_or(super::PaneLayout::Pane(0));
        self.panes.remove(removed);
        self.selected_container = None;
        self.focused_pane = sibling
            .saturating_sub(usize::from(sibling > removed))
            .min(self.panes.len().saturating_sub(1));
        true
    }

    fn desired_view(&self) -> RequestedView {
        if self.operator.visible {
            return RequestedView {
                revision: self.handshake.revision(),
                targets: Vec::new(),
                focused: None,
            };
        }
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
            let Some(run) = pane
                .run
                .or_else(|| find_session(self, session).map(|summary| summary.run))
            else {
                continue;
            };
            targets.push((
                session,
                run,
                TerminalSize {
                    rows: rect.terminal.height,
                    cols: rect.terminal.width,
                },
            ));
        }
        let focused = self
            .focused_session()
            .filter(|session| targets.iter().any(|(id, _, _)| id == session));
        RequestedView {
            revision: self.handshake.revision(),
            targets,
            focused,
        }
    }

    /// The instant a refused view may be sent again, so the event loop can wake for the retry.
    pub(super) fn view_retry_deadline(&self) -> Option<Instant> {
        self.handshake.retry_deadline()
    }

    pub(crate) fn view_request(
        &mut self,
        area: Rect,
        request_id: u64,
    ) -> anyhow::Result<Option<ClientMessage>> {
        if self.navigation.discard_input {
            return Ok(None);
        }
        let geometry_changed = self.outer_area != area;
        if geometry_changed {
            self.cancel_sidebar_jump();
            self.cancel_mouse_gesture();
        }
        if geometry_changed && self.copy.is_some() {
            self.cancel_copy(Some("Copy cancelled: terminal resized"));
        }
        self.outer_area = area;
        let desired = self.desired_view();
        let current_runs: HashSet<_> = self
            .hierarchy
            .projects
            .iter()
            .flat_map(|project| &project.workspaces)
            .flat_map(|workspace| &workspace.sessions)
            .map(|session| (session.id, session.run))
            .collect();
        self.recovery_requests
            .retain(|target, pending| pending.is_some() || current_runs.contains(target));
        for (session, run, _) in &desired.targets {
            let target = (*session, *run);
            if !self.recovery_requests.contains_key(&target)
                && find_session(self, *session)
                    .is_some_and(|summary| summary.run == *run && summary.can_auto_recover())
            {
                let request_id = self.next_request_id();
                self.recovery_requests.insert(target, Some(request_id));
                self.push_request(ClientMessage {
                    request_id,
                    request: Request::RecoverSession {
                        session: *session,
                        expected_run: *run,
                    },
                });
            }
        }
        for rect in self.pane_rects(area) {
            if let Some(pane) = self.panes.get_mut(rect.pane_index) {
                pane.desired_size = TerminalSize {
                    rows: rect.terminal.height,
                    cols: rect.terminal.width,
                };
            }
        }
        match self.handshake.desire(desired, request_id, Instant::now())? {
            Desire::Send {
                request,
                owns_error,
            } => {
                let sessions = match &request.request {
                    Request::SetView { view } => view
                        .panes
                        .iter()
                        .map(|pane| pane.session)
                        .collect::<HashSet<_>>(),
                    _ => HashSet::new(),
                };
                for pane in &mut self.panes {
                    if pane
                        .session
                        .is_some_and(|session| sessions.contains(&session))
                    {
                        pane.error = None;
                    }
                }
                if owns_error {
                    self.error_owning_requests.insert(request_id);
                }
                Ok(Some(*request))
            }
            // Coalesced into the pending view, unchanged, or waiting out a refusal: readiness
            // and pane errors are the handshake's to keep.
            Desire::Coalesced | Desire::Unchanged | Desire::Waiting => Ok(None),
        }
    }

    pub(crate) fn apply_screen(
        &mut self,
        request_id: u64,
        revision: u64,
        session: SessionId,
        run: SessionRunId,
        size: TerminalSize,
        bytes: &[u8],
    ) {
        if !self
            .handshake
            .snapshot_matches(request_id, revision, session, run)
        {
            return;
        }
        let Some(pane) = self
            .panes
            .iter_mut()
            .find(|pane| pane.session == Some(session) && pane.run.unwrap_or(run) == run)
        else {
            self.handshake.record_snapshot(session);
            return;
        };
        pane.size = size;
        pane.run = Some(run);
        pane.parser = vt100::Parser::new(size.rows, size.cols, 0);
        pane.parser.process(bytes);
        pane.error = None;
        self.handshake.record_snapshot(session);
        let rows = self.visible_rows();
        self.ensure_selection_visible(&rows);
        if self.focused_session() == Some(session) {
            self.reconcile_mouse_protocol();
        }
    }

    /// A session whose glyph animates, so the Dashboard owes it the spinner cadence. It and
    /// the drawn glyph come out of one decision in `status`, so they cannot disagree.
    pub(super) fn session_is_busy(&self, id: SessionId) -> bool {
        find_session(self, id).is_some_and(super::status::busy)
    }

    pub(super) fn redraw_interval(&self) -> Duration {
        if self.tasks.is_some() {
            return Duration::from_millis(50);
        }
        if self.operator_sidebar_busy()
            || self.operator_conversation_busy()
            || self.hierarchy.projects.iter().any(|project| {
                project.workspaces.iter().any(|workspace| {
                    workspace
                        .sessions
                        .iter()
                        .any(|session| self.session_is_busy(session.id))
                })
            })
        {
            SPINNER_INTERVAL
        } else {
            DASHBOARD_IDLE_REDRAW_INTERVAL
        }
    }

    pub(crate) fn visible_rows(&self) -> Vec<TreeRow> {
        match self.settings.sidebar_view {
            crate::protocol::SidebarView::Tree => self.hierarchy_rows(false),
            crate::protocol::SidebarView::Recent => self.recent_activity_rows(),
        }
    }

    fn recent_activity_rows(&self) -> Vec<TreeRow> {
        let mut projects = self.hierarchy.projects.iter().collect::<Vec<_>>();
        projects.sort_by(|left, right| left.name.cmp(&right.name));
        let mut rows = Vec::new();
        for project in projects {
            let mut sessions = project
                .workspaces
                .iter()
                .flat_map(|workspace| &workspace.sessions)
                .filter(|session| {
                    matches!(session.kind, crate::protocol::SessionKind::Agent { .. })
                })
                .collect::<Vec<_>>();
            if sessions.is_empty() {
                continue;
            }
            sessions.sort_by(|left, right| {
                right
                    .last_activity_unix_ms
                    .or(right.started_unix_ms)
                    .unwrap_or(0)
                    .cmp(
                        &left
                            .last_activity_unix_ms
                            .or(left.started_unix_ms)
                            .unwrap_or(0),
                    )
            });
            rows.push(TreeRow::Project {
                name: project.name.clone(),
            });
            if self.collapsed_projects.contains(&project.name) {
                continue;
            }
            rows.extend(
                sessions
                    .into_iter()
                    .map(|session| TreeRow::Session { id: session.id }),
            );
        }
        rows
    }

    pub(super) fn hierarchy_rows(&self, include_collapsed: bool) -> Vec<TreeRow> {
        let mut projects = self.hierarchy.projects.iter().collect::<Vec<_>>();
        projects.sort_by(|left, right| left.name.cmp(&right.name));
        let mut rows = Vec::new();
        let provisionals = self.provisional_tree_rows();
        for project in projects {
            rows.push(TreeRow::Project {
                name: project.name.clone(),
            });
            if !include_collapsed && self.collapsed_projects.contains(&project.name) {
                continue;
            }
            let mut workspaces = project.workspaces.iter().collect::<Vec<_>>();
            workspaces
                .sort_by(|left, right| (!left.root, &left.name).cmp(&(!right.root, &right.name)));
            let mut seen_workspace_ids = std::collections::HashSet::new();
            for workspace in workspaces {
                seen_workspace_ids.insert(workspace.id.clone());
                rows.push(TreeRow::Workspace {
                    project: project.name.clone(),
                    id: workspace.id.clone(),
                });
                if !include_collapsed
                    && self
                        .collapsed_workspaces
                        .contains(&(project.name.clone(), workspace.id.clone()))
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
                for (tree, _) in &provisionals {
                    if let TreeRow::ProvisionalSession {
                        project: p,
                        workspace: w,
                        ..
                    } = tree
                        && p == &project.name
                        && w == &workspace.id
                    {
                        rows.push(tree.clone());
                    }
                }
            }
            for (tree, _) in &provisionals {
                if let TreeRow::ProvisionalWorkspace { project: p, id, .. } = tree
                    && p == &project.name
                    && !seen_workspace_ids.contains(id)
                {
                    rows.push(tree.clone());
                }
            }
        }
        rows
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let rows = self.visible_rows();
        if rows.is_empty() {
            self.selected_container = None;
            self.retarget(PaneChange::Cleared);
            if let Some(pane) = self.focused_pane_mut() {
                pane.session = None;
                pane.run = None;
            }
            return;
        }
        let current = self.visible_selection_index(&rows);
        let index = match current {
            Some(index) => (index as isize + delta).clamp(0, rows.len() as isize - 1) as usize,
            None if delta < 0 => rows.len() - 1,
            None => 0,
        };
        match rows[index].clone() {
            TreeRow::Session { id } => self.select_session(id),
            row @ (TreeRow::Project { .. }
            | TreeRow::Workspace { .. }
            | TreeRow::ProvisionalWorkspace { .. }
            | TreeRow::ProvisionalSession { .. }) => {
                self.select_container(row);
                self.ensure_selection_visible(&rows);
            }
        }
    }

    // Container selection wipes the focused pane. Session actions stay off until
    // the user selects a session again; create/remove still follow this header.
    pub(super) fn creation_context(&self) -> (String, String) {
        match &self.selected_container {
            Some(TreeRow::Project { name }) => (name.clone(), String::new()),
            Some(TreeRow::Workspace { project, id })
            | Some(TreeRow::ProvisionalWorkspace { project, id }) => (project.clone(), id.clone()),
            Some(TreeRow::ProvisionalSession {
                project, workspace, ..
            }) => (project.clone(), workspace.clone()),
            _ => self
                .focused_session()
                .and_then(|id| find_session(self, id))
                .map(|s| (s.project.clone(), s.workspace.clone()))
                .unwrap_or_default(),
        }
    }

    pub(super) fn current_workspace(&self) -> Option<&ovrcr_protocol::WorkspaceSummary> {
        let (project, id) = self.creation_context();
        if id.is_empty() {
            None
        } else {
            find_workspace(self, &project, &id)
        }
    }

    pub(crate) fn select_session(&mut self, id: SessionId) {
        if self.initial_prompt_focus_pending.is_some() {
            let matches_target = self.initial_prompt_intent.as_ref().is_some_and(|snapshot| {
                snapshot.session == Some(id)
                    && snapshot.run.is_some_and(|run| {
                        find_session(self, id).is_some_and(|session| session.run == run)
                    })
            });
            if !matches_target {
                self.initial_prompt_focus_pending = None;
            }
        }
        self.operator.visible = false;
        self.selected_container = None;
        if let Some(index) = self.panes.iter().position(|pane| pane.session == Some(id)) {
            self.focus_pane(index);
        } else if self.focused_session() != Some(id) {
            self.retarget(PaneChange::Select);
            let size = self.focused_size();
            let run = find_session(self, id).map(|summary| summary.run);
            if let Some(pane) = self.focused_pane_mut() {
                pane.session = Some(id);
                pane.run = run;
                pane.parser = vt100::Parser::new(size.rows, size.cols, 0);
                pane.size = size;
                pane.desired_size = size;
                pane.error = None;
            }
        }
        let rows = self.visible_rows();
        self.ensure_selection_visible(&rows);
    }

    pub(super) fn tree_viewport_height(&self) -> usize {
        usize::from(self.sidebar_rects(self.outer_area).tree.height)
    }

    fn visible_selection_index(&self, rows: &[TreeRow]) -> Option<usize> {
        if let Some(selected) = &self.selected_container {
            if let Some(index) = rows.iter().position(|row| row == selected) {
                return Some(index);
            }
            if let TreeRow::Workspace { project, .. } = selected
                && let Some(index) = rows
                    .iter()
                    .position(|row| matches!(row, TreeRow::Project { name } if name == project))
            {
                return Some(index);
            }
        }
        let id = self.focused_session()?;
        if let Some(index) = rows.iter().position(|row| *row == TreeRow::Session { id }) {
            return Some(index);
        }
        let session = find_session(self, id)?;
        let workspace = TreeRow::Workspace {
            project: session.project.clone(),
            id: session.workspace.clone(),
        };
        if let Some(index) = rows.iter().position(|row| *row == workspace) {
            return Some(index);
        }
        rows.iter()
            .position(|row| matches!(row, TreeRow::Project { name } if name == &session.project))
    }

    /// Lines each visible row takes in the sidebar drawn for `self.outer_area`.
    fn tree_heights(&self, rows: &[TreeRow]) -> Vec<usize> {
        let width = self.sidebar_rects(self.outer_area).tree.width;
        tree_row_heights(self, rows, usize::from(width))
    }

    pub(super) fn ensure_selection_visible(&mut self, rows: &[TreeRow]) {
        let heights = self.tree_heights(rows);
        let height = self.tree_viewport_height();
        let max_offset = tree_line_count(rows, &heights).saturating_sub(height);
        self.tree_offset = self.tree_offset.min(max_offset);
        let Some(index) = self.visible_selection_index(rows) else {
            return;
        };
        let selected_start = tree_row_start(rows, &heights, index);
        let selected_end = selected_start.saturating_add(heights[index]);
        if selected_start < self.tree_offset {
            self.tree_offset = selected_start;
        } else if selected_end > self.tree_offset.saturating_add(height) {
            self.tree_offset = selected_end.saturating_sub(height);
        }
        self.tree_offset = self.tree_offset.min(max_offset);
    }

    /// Toggle collapse for a project or workspace row. Returns true when the row
    /// is foldable, whether the toggle collapsed or expanded it.
    pub(super) fn toggle_row_collapse(&mut self, row: &TreeRow) -> bool {
        match row {
            TreeRow::Project { name } => {
                if !self.collapsed_projects.remove(name) {
                    self.collapsed_projects.insert(name.clone());
                }
                true
            }
            TreeRow::Workspace { project, id } => {
                let key = (project.clone(), id.clone());
                if !self.collapsed_workspaces.remove(&key) {
                    self.collapsed_workspaces.insert(key);
                }
                true
            }
            TreeRow::Session { .. }
            | TreeRow::ProvisionalSession { .. }
            | TreeRow::ProvisionalWorkspace { .. } => false,
        }
    }

    pub fn key_action(&mut self, key: KeyEvent) -> DashboardAction {
        // A failure has been seen once a key is pressed while it shows.
        if key.kind == KeyEventKind::Press && self.error.is_some() && self.error_since.is_none() {
            self.error_since = Some(Instant::now());
        }
        let action = self.dispatch_key(key);
        // A key clears the visible notice. One that was only queued behind the banner
        // stays queued until the banner is gone, then the next key reveals it.
        self.reveal_queued_notice();
        action
    }

    /// Ctrl-o opens Quick message, except where keys go to the terminal program
    /// (Terminal mode with nothing over the pane): Oh My Pi expands output and Claude
    /// Code opens its transcript with it. Ctrl-g then Ctrl-o still opens Quick message.
    fn opens_operator_quick(&self, key: KeyEvent) -> bool {
        super::keymap::is_operator_quick_key(key)
            && !(self.mode == InputMode::Terminal
                && !self.operator_quick_visible()
                && self.agent_typing.is_none()
                && self.tasks.is_none()
                && self.details.is_none()
                && self.palette.is_none()
                && self.whichkey.is_none()
                && !self.operator_is_editing())
    }

    fn dispatch_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Release {
            // Enhanced keyboard mode reports releases for ordinary keys too; they are not
            // Keystrokes and must never be encoded into a child PTY.
            return DashboardAction::None;
        }
        if self.sidebar_create_menu.is_some() {
            return self
                .sidebar_create_menu_key(key)
                .unwrap_or(DashboardAction::None);
        }
        if self.sidebar_jump.is_some()
            && (self.sidebar_jump_modal_priority_open() || self.opens_operator_quick(key))
        {
            self.cancel_sidebar_jump();
        }
        // Transcript is a read-only modal, including while its first page is loading.
        // Route it before the global Operator quick-message shortcut.
        if self
            .details
            .is_some_and(|(details, _)| details == super::quota::Details::Transcript)
        {
            return self.details_key(key);
        }
        if self.opens_operator_quick(key) {
            return if key.kind == KeyEventKind::Press {
                self.open_operator_quick_message()
            } else {
                DashboardAction::None
            };
        }
        if self.operator_quick_visible() {
            return self.operator_quick_key(key);
        }
        if key.kind == KeyEventKind::Press {
            self.desktop.notice = None;
        }
        if let Some(tasks) = &mut self.tasks {
            if tasks.event(Event::Key(key)) {
                self.tasks = None;
            }
            return DashboardAction::Redraw;
        }
        if self.details.is_some() {
            return self.details_key(key);
        }
        if self.palette.is_some() {
            return self.palette_key(key);
        }
        if (self.whichkey.is_some() || !self.operator_is_editing())
            && let Some(action) = self.whichkey_key(key)
        {
            return action;
        }
        if let Some(action) = self.dispatch_sidebar_jump_key(key) {
            return action;
        }
        if self.operator_is_editing() {
            return self.operator_key(key);
        }
        // Esc dismisses the Lifecycle status strip only while a job runs.
        // After a failure that could not return to its form, one Esc dismisses
        // the strip and the failed Provisional row together.
        if key.kind == KeyEventKind::Press
            && key.code == KeyCode::Esc
            && self.palette.is_none()
            && self.whichkey.is_none()
            && self.details.is_none()
        {
            if self.lifecycle_strip.is_some() {
                self.dismiss_lifecycle_strip();
                return DashboardAction::Redraw;
            }
            if self.dismiss_failed_provisional() {
                return DashboardAction::Redraw;
            }
        }
        if let Some(intent) = &mut self.agent_typing {
            if key.kind == KeyEventKind::Release {
                return DashboardAction::None;
            }
            if is_browse_key(key) || key.code == KeyCode::Esc {
                self.agent_typing = None;
                self.cancel_mouse_gesture();
                self.mode = InputMode::Browse;
                return DashboardAction::EnterBrowse;
            }
            intent.rejected = Some("input");
            return DashboardAction::Redraw;
        }
        match self.mode {
            InputMode::Browse => {
                // Esc and Ctrl-g cancel a history request that has not opened
                // yet; neither is a binding of its own. Ctrl reaches Browse only
                // as Ctrl-t, so it blocks the Esc half.
                let cancels_history = is_browse_key(key)
                    || (key.code == KeyCode::Esc && !key.modifiers.contains(KeyModifiers::CONTROL));
                if cancels_history && self.history_begin_request.is_some() {
                    if let Some(begin) = self.history_begin_request.as_mut() {
                        begin.cancelled = true;
                    }
                    return DashboardAction::EnterBrowse;
                }
                match self.key_binding_for(key) {
                    Some(binding) => self.run(binding.action),
                    None => DashboardAction::None,
                }
            }
            InputMode::Terminal => {
                if is_browse_key(key) {
                    self.cancel_mouse_gesture();
                    if let Some(begin) = self.history_begin_request.as_mut() {
                        begin.cancelled = true;
                    }
                    self.mode = InputMode::Browse;
                    // Ctrl-g is the navigation key: arriving in Browse also opens
                    // the label picker in the same press.
                    self.begin_sidebar_jump();
                    DashboardAction::EnterBrowse
                } else if !self.input_is_allowed() {
                    self.refuse_input()
                } else {
                    let application_cursor = self
                        .focused_pane()
                        .is_some_and(|pane| pane.parser.screen().application_cursor());
                    match encode_key(key, application_cursor) {
                        KeyEncoding::Bytes(bytes) => {
                            self.review_on_reply();
                            DashboardAction::PtyBytes(bytes)
                        }
                        KeyEncoding::Browse | KeyEncoding::Ignore => DashboardAction::None,
                    }
                }
            }
            InputMode::History => self.history_key_action(key),
            InputMode::Copy => self.copy_key_action(key),
        }
    }

    pub(super) fn begin_copy(&mut self) -> DashboardAction {
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        let Some(session) = self.action_session() else {
            self.set_limit("Waiting for terminal screen");
            return DashboardAction::Redraw;
        };
        if !self
            .focused_pane()
            .is_some_and(|pane| self.pane_ready(pane))
            || find_session(self, session).is_none()
        {
            self.set_limit("Waiting for terminal screen");
            return DashboardAction::Redraw;
        }
        self.dismiss_error_banner();
        self.copy_notice = None;
        let screen = self
            .focused_pane()
            .map(|pane| pane.parser.screen())
            .expect("focused session has a pane");
        self.copy = Some(CopySelection::capture(session, screen));
        self.mode = InputMode::Copy;
        DashboardAction::Redraw
    }

    pub(super) fn copy_key_action(&mut self, key: KeyEvent) -> DashboardAction {
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

    pub(super) fn begin_history_request(&mut self, at_tail: bool) -> DashboardAction {
        let Some(session) = self.action_session() else {
            return DashboardAction::None;
        };
        if self.history.is_some() || self.history_begin_request.is_some() {
            return DashboardAction::Redraw;
        }
        // A coalesced focus change would invalidate a capture opened before SetView.
        if !self
            .focused_pane()
            .is_some_and(|pane| self.pane_ready(pane))
        {
            self.set_limit("Pane is loading; retry history");
            return DashboardAction::Redraw;
        }
        self.history_page_error = false;
        // A wheel tick may land on a full-screen program and then do nothing at all, so it
        // leaves a standing banner to its own lifetime; PageUp is a deliberate new action.
        if !at_tail {
            self.dismiss_error_banner();
        }
        let request_id = self.error_owning_request_id();
        self.history_begin_request = Some(PendingHistoryBegin {
            request_id,
            session,
            cancelled: false,
            at_tail,
        });
        self.enqueue_request(ClientMessage {
            request_id,
            request: Request::HistoryBegin { session },
        });
        DashboardAction::Redraw
    }

    /// History cannot freeze a program on the alternate screen, such as a full-screen agent.
    /// The read-only Transcript view can show an agent's provider-owned conversation instead.
    fn alternate_screen_history(&mut self, begin: &PendingHistoryBegin) {
        if begin.cancelled {
            return;
        }
        self.open_transcript(begin.session, begin.at_tail);
    }

    pub(super) fn history_key_action(&mut self, key: KeyEvent) -> DashboardAction {
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
        let (busy, anchored, unresolved_target) = {
            let Some(view) = self.history.as_ref() else {
                return DashboardAction::None;
            };
            (
                view.copy_job.is_some() || view.copy_completion.is_some(),
                view.anchor.is_some(),
                view.cursor_target.is_some(),
            )
        };
        if busy {
            return DashboardAction::None;
        }
        if !anchored
            && unresolved_target
            && key.kind == KeyEventKind::Press
            && history_retry_key(key.code)
        {
            self.history_page_error = false;
            self.dismiss_error_banner();
        }
        if anchored && unresolved_target {
            if key.kind == KeyEventKind::Press && history_retry_key(key.code) {
                self.history_page_error = false;
                self.dismiss_error_banner();
                self.copy_notice = Some("Waiting for history cell".into());
                self.queue_history_request();
                return DashboardAction::Redraw;
            }
            return DashboardAction::None;
        }
        let size = history_view_size(self.focused_size());
        if !anchored {
            let changed = {
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
                view.cursor_target = (view.opened.total_rows > 0).then_some(
                    HistoryCursorTarget::At(HistoryCopyPoint {
                        row: view.top,
                        col: view.left,
                    }),
                );
                view.cursor_reveal = false;
                changed
            };
            if !changed {
                return DashboardAction::None;
            }
            self.history_page_error = false;
            self.dismiss_error_banner();
            self.copy_notice = None;
            self.queue_history_request();
            return DashboardAction::Redraw;
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
            self.queue_history_request();
            return DashboardAction::Redraw;
        }
        let Some(cursor) = self.history.as_ref().and_then(|view| view.cursor) else {
            self.copy_notice = Some("Nothing to select on this row".into());
            return DashboardAction::Redraw;
        };
        if let Some(view) = self.history.as_mut() {
            view.anchor = Some(cursor.point);
        }
        self.history_page_error = false;
        self.dismiss_error_banner();
        self.copy_notice = None;
        DashboardAction::Redraw
    }

    fn start_history_copy(&mut self) -> DashboardAction {
        let Some(view) = self.history.as_ref() else {
            return DashboardAction::None;
        };
        if view.cursor_target.is_some() {
            self.copy_notice = Some("Waiting for history cell".into());
            self.queue_history_request();
            return DashboardAction::Redraw;
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
        self.dismiss_error_banner();
        self.copy_notice = None;
        self.queue_history_request();
        DashboardAction::Redraw
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
        self.dismiss_error_banner();
        self.copy_notice = None;
        self.queue_history_request();
        DashboardAction::Redraw
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

    /// Every path that changes what a pane shows comes through here. The variant decides
    /// which releases apply, so a reader can tell "deliberate" from "forgotten" in one
    /// place instead of eight.
    pub(super) fn retarget(&mut self, change: PaneChange) {
        if self.agent_typing.take().is_some() {
            self.set_error("Agent switch cancelled: view changed");
        }
        use PaneChange::*;
        if matches!(change, Select | Container | Split | Close | Cleared) {
            self.operator.visible = false;
        }
        let discard_pending_parser = !matches!(change, Focus | Resize);
        let release_capture = !matches!(
            change,
            Resize
                | ServerRemoved {
                    focused_survives: true
                }
        );
        let user_change = !matches!(change, ServerRemoved { .. } | Cleared);
        let invalidate = !matches!(
            change,
            ServerRemoved {
                focused_survives: true
            }
        );
        let browse = !matches!(
            change,
            Resize
                | ServerRemoved {
                    focused_survives: true
                }
        );
        if discard_pending_parser {
            self.mark_pending_parser_discarded();
        }
        if matches!(change, ServerRemoved { .. }) {
            // `retain` shifts pane indices, so a parked wheel deferral no longer names the
            // pane the user scrolled, even when the focused session survives.
            self.deferred_history_at_tail = None;
        }
        if release_capture {
            self.release_for_selection_change();
        }
        if user_change {
            self.handshake.note_user_change();
        }
        if invalidate {
            self.invalidate_view_readiness();
        }
        if browse {
            self.mode = InputMode::Browse;
        }
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
        self.handshake.invalidate();
    }

    fn mark_pending_parser_discarded(&mut self) {
        self.handshake.mark_parser_discarded();
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
        if self.navigation.discard_input {
            return DashboardAction::None;
        }
        if self.sidebar_jump.is_some() && self.sidebar_jump_modal_priority_open() {
            self.cancel_sidebar_jump();
        }
        if let Event::Key(key) = event
            && (self.opens_operator_quick(key) || self.operator_quick_visible())
        {
            return self.key_action(key);
        }
        if self.operator_quick_visible() {
            match &event {
                Event::Paste(text) => return self.operator_quick_paste(text),
                Event::Mouse(mouse) => {
                    return self.operator_quick_mouse(*mouse, self.outer_area).unwrap();
                }
                Event::Resize(_, _) => return DashboardAction::Redraw,
                _ => {}
            }
        }
        if let Some(tasks) = &mut self.tasks {
            if tasks.event(event) {
                self.tasks = None;
            }
            return DashboardAction::Redraw;
        }
        match event {
            Event::Paste(text)
                if self
                    .details
                    .is_some_and(|(d, _)| d == super::quota::Details::Settings) =>
            {
                self.settings_editor_paste(&text)
            }
            Event::Paste(text)
                if self
                    .details
                    .is_some_and(|(d, _)| d == super::quota::Details::Events) =>
            {
                self.events_paste(&text)
            }
            Event::Paste(_) if self.details.is_some() => DashboardAction::None,
            Event::Paste(_) if self.sidebar_create_menu.is_some() => DashboardAction::None,
            Event::Paste(_) if self.whichkey.is_some() => DashboardAction::None,
            Event::Paste(text) if self.palette.is_some() => self.palette_paste(&text),
            Event::Key(key) => self.key_action(key),
            Event::Paste(_) if self.sidebar_jump.is_some() => {
                self.cancel_sidebar_jump();
                DashboardAction::Redraw
            }
            Event::Paste(_) if self.agent_typing.is_some() => {
                self.agent_typing.as_mut().unwrap().rejected = Some("paste");
                DashboardAction::Redraw
            }
            Event::Paste(text) if self.operator_is_editing() => self.operator_paste(&text),
            Event::Paste(_) if self.operator.visible => DashboardAction::None,
            Event::Paste(text) if self.mode == InputMode::Terminal => {
                if !self.input_is_allowed() {
                    self.refuse_input()
                } else {
                    self.review_on_reply();
                    DashboardAction::PtyBytes(encode_paste(
                        &text,
                        self.focused_pane()
                            .is_some_and(|pane| pane.parser.screen().bracketed_paste()),
                    ))
                }
            }
            Event::Mouse(mouse) => {
                let action = self.mouse_action(mouse, self.outer_area);
                self.finish_agent_switch();
                action
            }
            Event::FocusLost => {
                self.cancel_sidebar_jump();
                self.sidebar_create_menu = None;
                self.agent_typing = None;
                self.cancel_mouse_gesture();
                self.mouse.hovered = None;
                self.mouse_focused = false;
                DashboardAction::Redraw
            }
            Event::FocusGained => {
                self.mouse_focused = true;
                DashboardAction::Redraw
            }
            Event::Resize(cols, rows) => {
                self.cancel_sidebar_jump();
                self.agent_typing = None;
                let area = Rect::new(0, 0, cols, rows);
                self.outer_area = area;
                self.settings_editor.resize(area);
                DashboardAction::Redraw
            }
            Event::Paste(_) => DashboardAction::None,
        }
    }

    pub fn mouse_action(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        if self.sidebar_jump.is_some() && mouse.kind != MouseEventKind::Moved {
            self.cancel_sidebar_jump();
        }
        if self.sidebar_create_menu.is_some() {
            return self.sidebar_create_menu_mouse(mouse, area);
        }
        if let Some(action) = self.operator_quick_mouse(mouse, area) {
            return action;
        }
        if self.details.is_some() {
            return self.details_mouse(mouse);
        }
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
        if let Some(action) = self.operator_mouse(mouse, area) {
            return action;
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
        if self.operator.visible || self.operator_quick_visible() {
            return true;
        }
        if self.sidebar_create_menu.is_some() {
            return true;
        }
        if self.details.is_some() {
            return true;
        }
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
        self.mouse.split_dragging = None;
        self.mouse.sidebar_dragging = false;
        if let Some(copy) = &mut self.copy {
            copy.dragging = false;
        }
        if let Some(history) = &mut self.history {
            history.dragging = false;
        }
    }

    /// The sidebar width every layout reads: zero while hidden, else the
    /// dragged preference.
    pub(super) fn sidebar_preference(&self) -> Option<u16> {
        if !self.operator_sidebar_requested()
            && (self.sidebar_hidden || self.operator_sidebar_on_demand())
        {
            Some(0)
        } else {
            self.sidebar_width
        }
    }

    /// Hide or show the sidebar; the panes take or give back its width.
    pub(super) fn toggle_sidebar(&mut self) -> DashboardAction {
        self.sidebar_hidden = !self.sidebar_hidden;
        self.mouse.hovered = None;
        self.apply_pane_sizes(self.outer_area);
        DashboardAction::Redraw
    }

    pub fn pane_rects(&self, area: Rect) -> Vec<super::PaneRects> {
        self.pane_layout_rects(area).panes
    }

    pub(super) fn pane_layout_rects(&self, area: Rect) -> super::PaneLayoutRects {
        super::pane_layout_rects(
            area,
            &self.pane_layout,
            self.panes.len(),
            self.focused_pane,
            self.sidebar_preference(),
        )
    }

    pub(super) fn row_shows_close_mark(&self, row: &TreeRow) -> bool {
        match (&self.mouse.hovered, row) {
            (Some(super::HoveredRow::Session(id)), TreeRow::Session { id: row_id }) => id == row_id,
            (
                Some(super::HoveredRow::Workspace { project, id }),
                TreeRow::Workspace {
                    project: row_project,
                    id: row_id,
                },
            ) => project == row_project && id == row_id,
            _ => false,
        }
    }

    pub(super) fn row_shows_create_mark(&self, row: &TreeRow) -> bool {
        if matches!(self.mode, InputMode::History | InputMode::Copy) {
            return false;
        }
        match (&self.mouse.hovered, row) {
            (Some(super::HoveredRow::Project(name)), TreeRow::Project { name: row_name }) => {
                name == row_name
            }
            (
                Some(super::HoveredRow::Workspace { project, id }),
                TreeRow::Workspace {
                    project: row_project,
                    id: row_id,
                },
            ) => project == row_project && id == row_id,
            _ => false,
        }
    }

    fn hovered_row_at(&self, sidebar: Rect, row: u16) -> Option<super::HoveredRow> {
        let row_index = self.tree_offset + usize::from(row.saturating_sub(sidebar.y));
        let rows = self.visible_rows();
        let heights = tree_row_heights(self, &rows, usize::from(sidebar.width));
        match tree_line_at(&rows, &heights, row_index)?.0 {
            TreeRow::Session { id } => Some(super::HoveredRow::Session(*id)),
            TreeRow::Project { name } => Some(super::HoveredRow::Project(name.clone())),
            TreeRow::Workspace { project, id } | TreeRow::ProvisionalWorkspace { project, id } => {
                Some(super::HoveredRow::Workspace {
                    project: project.clone(),
                    id: id.clone(),
                })
            }
            TreeRow::ProvisionalSession { .. } => None,
        }
    }

    fn close_mark_hit(&self, column: u16, width: u16) -> bool {
        width >= 3 && column >= width - 3
    }

    fn create_mark_hit(&self, column: u16, width: u16, row: &TreeRow) -> bool {
        if !self.row_shows_create_mark(row) {
            return false;
        }
        match row {
            TreeRow::Project { .. } => width >= 3 && column >= width - 3,
            TreeRow::Workspace { .. } => width >= 6 && (width - 6..width - 3).contains(&column),
            _ => false,
        }
    }

    fn sidebar_mouse_action(&mut self, mouse: MouseEvent, area: Rect) -> Option<DashboardAction> {
        if !matches!(self.mode, InputMode::Browse | InputMode::Terminal)
            || self.sidebar_preference() == Some(0)
        {
            return None;
        }
        let rects = self.sidebar_rects(area);
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if point_in_rect(mouse, rects.attention) {
                return Some(self.jump_to_attention());
            }
            if point_in_rect(mouse, rects.quota) {
                return Some(self.open_quota_details());
            }
        }
        let sidebar = rects.tree;
        if mouse.kind == MouseEventKind::Moved {
            let inside = point_in_rect(mouse, sidebar);
            let next = inside
                .then(|| self.hovered_row_at(sidebar, mouse.row))
                .flatten();
            let changed = self.mouse.hovered != next;
            self.mouse.hovered = next;
            // Motion outside the sidebar still belongs to the pane.
            if !inside {
                return None;
            }
            return Some(if changed {
                DashboardAction::Redraw
            } else {
                DashboardAction::None
            });
        }
        if !point_in_rect(mouse, sidebar) {
            return None;
        }
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let rows = self.visible_rows();
                let heights = tree_row_heights(self, &rows, usize::from(sidebar.width));
                let max_offset =
                    tree_line_count(&rows, &heights).saturating_sub(usize::from(sidebar.height));
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
                let heights = tree_row_heights(self, &rows, usize::from(sidebar.width));
                let (row, row_line) = tree_line_at(&rows, &heights, row_index)?;
                let row = row.clone();
                let column = mouse.column.saturating_sub(sidebar.x);
                match row {
                    TreeRow::Session { id } => {
                        // The mark is drawn on the first line only; the label line selects.
                        if row_line == 0
                            && self.close_mark_hit(column, sidebar.width)
                            && self.mouse.hovered == Some(super::HoveredRow::Session(id))
                        {
                            return Some(self.open_close_terminal_for(id));
                        }
                        self.select_session(id);
                        Some(self.request_selected())
                    }
                    TreeRow::Project { name }
                        if self.create_mark_hit(
                            column,
                            sidebar.width,
                            &TreeRow::Project { name: name.clone() },
                        ) =>
                    {
                        Some(self.open_sidebar_create_menu(
                            TreeRow::Project { name },
                            ratatui::layout::Position::new(
                                sidebar.x + sidebar.width.saturating_sub(3),
                                mouse.row,
                            ),
                        ))
                    }
                    TreeRow::Project { name } if column == 0 => {
                        self.toggle_row_collapse(&TreeRow::Project { name });
                        self.clamp_tree_offset(usize::from(sidebar.height));
                        Some(DashboardAction::Redraw)
                    }
                    TreeRow::Workspace { project, id }
                        if self.create_mark_hit(
                            column,
                            sidebar.width,
                            &TreeRow::Workspace {
                                project: project.clone(),
                                id: id.clone(),
                            },
                        ) =>
                    {
                        Some(self.open_sidebar_create_menu(
                            TreeRow::Workspace { project, id },
                            ratatui::layout::Position::new(
                                sidebar.x + sidebar.width.saturating_sub(6),
                                mouse.row,
                            ),
                        ))
                    }
                    TreeRow::Workspace { project, id }
                        if self.close_mark_hit(column, sidebar.width)
                            && self.mouse.hovered
                                == Some(super::HoveredRow::Workspace {
                                    project: project.clone(),
                                    id: id.clone(),
                                }) =>
                    {
                        Some(self.open_remove_workspace(project, id))
                    }
                    TreeRow::Workspace { project, id } if column == 2 => {
                        self.toggle_row_collapse(&TreeRow::Workspace { project, id });
                        self.clamp_tree_offset(usize::from(sidebar.height));
                        Some(DashboardAction::Redraw)
                    }
                    row @ (TreeRow::Project { .. }
                    | TreeRow::Workspace { .. }
                    | TreeRow::ProvisionalWorkspace { .. }
                    | TreeRow::ProvisionalSession { .. }) => {
                        self.select_container(row);
                        Some(DashboardAction::Redraw)
                    }
                }
            }
            _ => None,
        }
    }

    pub(super) fn clamp_tree_offset(&mut self, viewport_height: usize) {
        let rows = self.visible_rows();
        let heights = self.tree_heights(&rows);
        let max_offset = tree_line_count(&rows, &heights).saturating_sub(viewport_height);
        self.tree_offset = self.tree_offset.min(max_offset);
    }

    fn pane_mouse_action(&mut self, mouse: MouseEvent, area: Rect) -> Option<DashboardAction> {
        if !matches!(self.mode, InputMode::Browse | InputMode::Terminal) {
            return None;
        }
        if let Some(path) = self.mouse.split_dragging.clone() {
            return match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    Some(if self.resize_split(area, &path, mouse.column, mouse.row) {
                        DashboardAction::Redraw
                    } else {
                        DashboardAction::None
                    })
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.mouse.split_dragging = None;
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

        let sidebar = super::render::sidebar_area(area, self.sidebar_preference());
        let layout = self.pane_layout_rects(area);
        let rects = &layout.panes;
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && mouse.row >= sidebar.y
            && mouse.row < sidebar.bottom()
        {
            // The sidebar's border column sits just past its content; a hidden
            // sidebar has no border.
            if self.sidebar_preference() != Some(0) && mouse.column == sidebar.right() {
                self.queue_held_releases();
                self.mouse.sidebar_dragging = true;
                return Some(DashboardAction::Redraw);
            }
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(boundary) = layout
                .boundaries
                .iter()
                .find(|boundary| point_in_rect(mouse, boundary.divider))
        {
            self.queue_held_releases();
            self.mouse.split_dragging = Some(boundary.path.clone());
            return Some(DashboardAction::Redraw);
        }

        if mouse.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(pane) = rects
                .iter()
                .find(|pane| point_in_rect(mouse, pane_bounds(**pane)))
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
    pub(super) fn apply_pane_sizes(&mut self, area: Rect) {
        for rect in self.pane_rects(area) {
            if let Some(pane) = self.panes.get_mut(rect.pane_index) {
                pane.desired_size = TerminalSize {
                    rows: rect.terminal.height,
                    cols: rect.terminal.width,
                };
            }
        }
        self.queue_held_releases();
        self.retarget(PaneChange::Resize);
    }

    fn resize_split(&mut self, area: Rect, path: &[bool], column: u16, row: u16) -> bool {
        let Some(boundary) = self
            .pane_layout_rects(area)
            .boundaries
            .into_iter()
            .find(|boundary| boundary.path == path)
        else {
            self.mouse.split_dragging = None;
            return false;
        };
        let (position, origin, dimension, minimum) = match boundary.direction {
            super::SplitDirection::Right => (
                column,
                boundary.area.x,
                boundary.area.width.saturating_sub(1),
                super::MIN_PANE_WIDTH,
            ),
            super::SplitDirection::Down => (
                row,
                boundary.area.y,
                boundary.area.height.saturating_sub(1),
                super::MIN_PANE_HEIGHT,
            ),
        };
        let first = position
            .saturating_sub(origin)
            .clamp(minimum, dimension.saturating_sub(minimum));
        let old_first = match boundary.direction {
            super::SplitDirection::Right => boundary.divider.x.saturating_sub(boundary.area.x),
            super::SplitDirection::Down => boundary.divider.y.saturating_sub(boundary.area.y),
        };
        if first == old_first {
            return false;
        }
        if !self.pane_layout.set_ratio(
            path,
            super::SplitRatio {
                first,
                available: dimension,
            },
        ) {
            self.mouse.split_dragging = None;
            return false;
        }
        self.apply_pane_sizes(area);
        true
    }

    pub(super) fn select_container(&mut self, row: TreeRow) {
        self.operator.visible = false;
        if self.selected_container.as_ref() == Some(&row) {
            return;
        }
        self.retarget(PaneChange::Container);
        let size = self.focused_size();
        if let Some(pane) = self.focused_pane_mut() {
            *pane = super::PaneState::new(size);
        }
        if let Some(index) = self.panes.iter().position(|pane| pane.session.is_some()) {
            self.focused_pane = index;
        }
        self.selected_container = Some(row);
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
                    let Some(run) = self.input_run(session) else {
                        return DashboardAction::None;
                    };
                    self.mouse.held[held_index(button)] = Some(super::HeldMouse {
                        session,
                        run,
                        control_epoch: find_session(self, session)
                            .and_then(|summary| summary.assignment_control.as_ref())
                            .map(|view| view.control.epoch),
                        view_revision: self.view_revision(),
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
        let Some(bytes) = encode_mouse(release, held.mode, held.encoding) else {
            return DashboardAction::None;
        };
        match held.control_epoch {
            Some(control_epoch) => {
                let request_id = self.next_request_id();
                self.enqueue_request(ClientMessage {
                    request_id,
                    request: Request::AssignedHumanInput {
                        session: held.session,
                        run: held.run,
                        control_epoch,
                        view_revision: held.view_revision,
                        bytes,
                    },
                });
                DashboardAction::Redraw
            }
            None => DashboardAction::PtyBytes(bytes),
        }
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
    pub(super) fn queue_deferred_history_request(&mut self) {
        let Some(index) = self.deferred_history_at_tail.take() else {
            return;
        };
        if index != self.focused_pane
            || !self
                .focused_pane()
                .is_some_and(|pane| self.pane_ready(pane))
        {
            return;
        }
        self.begin_history_request(true);
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
        if !(starting
            || (matches!(
                mouse.kind,
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
            ) && dragging))
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
        self.dismiss_error_banner();
        self.copy_notice = None;
        self.queue_history_request();
        DashboardAction::Redraw
    }

    /// A wheel tick moves History three rows, as terminals scroll; keys move one.
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
            MouseEventKind::ScrollUp => self.history_scroll_rows(-HISTORY_WHEEL_ROWS),
            MouseEventKind::ScrollDown => {
                let size = history_view_size(self.focused_size());
                let at_tail = self.history.as_ref().is_some_and(|view| {
                    let max_top = view.opened.total_rows.saturating_sub(u32::from(size.rows));
                    view.top >= max_top
                });
                if at_tail {
                    self.leave_history()
                } else {
                    self.history_scroll_rows(HISTORY_WHEEL_ROWS)
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
        self.dismiss_error_banner();
        self.copy_notice = None;
        self.queue_history_request();
        DashboardAction::Redraw
    }

    fn queue_held_releases(&mut self) {
        let mut bytes = Vec::new();
        let mut target = None;
        for held in self.mouse.held.iter_mut() {
            let Some(gesture) = held.take() else {
                continue;
            };
            let captured = (
                gesture.session,
                gesture.run,
                gesture.control_epoch,
                gesture.view_revision,
            );
            if target.is_some_and(|target| target != captured) {
                continue;
            }
            target = Some(captured);
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
        let Some((session, run, control_epoch, view_revision)) = target else {
            return;
        };
        let request_id = self.next_request_id();
        let request = match control_epoch {
            Some(control_epoch) => Request::AssignedHumanInput {
                session,
                run,
                control_epoch,
                view_revision,
                bytes,
            },
            None => Request::Input {
                session,
                run,
                bytes,
            },
        };
        self.push_request(ClientMessage {
            request_id,
            request,
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
        self.queue_history_request();
        DashboardAction::Redraw
    }

    fn initial_prompt_reconciliation_response(
        &mut self,
        request_id: u64,
        response: &Response,
    ) -> bool {
        if self.initial_prompt_reconcile_request != Some(request_id) {
            return false;
        }
        self.initial_prompt_reconcile_request = None;
        match response {
            Response::InitialPromptIntents(snapshots) => {
                if let Some(current) = self.initial_prompt_intent_id.clone() {
                    if let Some(snapshot) = snapshots.iter().find(|s| s.id == current) {
                        self.install_initial_prompt_snapshot(snapshot.clone());
                    } else {
                        self.install_initial_prompt_snapshot(unknown_prompt_snapshot(current));
                    }
                } else {
                    self.install_initial_prompt_snapshots(snapshots);
                }
                self.abandon_unbound_initial_prompt_allocation();
            }
            Response::Error { code, message } => self.set_server_error(code.clone(), message),
            _ => self.set_error("Could not reconcile the previous agent start"),
        }
        true
    }

    pub(super) fn install_initial_prompt_snapshots(
        &mut self,
        snapshots: &[crate::protocol::InitialPromptIntentSnapshot],
    ) {
        let Some(snapshot) = snapshots.iter().max_by_key(|snapshot| snapshot.id.sequence) else {
            return;
        };
        self.install_initial_prompt_snapshot(snapshot.clone());
    }

    pub(super) fn install_initial_prompt_snapshot(
        &mut self,
        snapshot: crate::protocol::InitialPromptIntentSnapshot,
    ) {
        if self
            .initial_prompt_intent_id
            .as_ref()
            .is_some_and(|current| {
                current.server_lifetime == snapshot.id.server_lifetime
                    && current.sequence > snapshot.id.sequence
            })
            || self.initial_prompt_intent.as_ref().is_some_and(|current| {
                current.id.server_lifetime == snapshot.id.server_lifetime
                    && current.id.sequence > snapshot.id.sequence
            })
        {
            return;
        }
        if self
            .initial_prompt_focus_pending
            .as_ref()
            .is_some_and(|id| id.server_lifetime != snapshot.id.server_lifetime)
        {
            self.initial_prompt_focus_pending = None;
        }
        self.initial_prompt_intent_id = Some(snapshot.id.clone());
        self.initial_prompt_intent = Some(snapshot.clone());
        self.initial_prompt_snapshot_updated(&snapshot);
    }

    pub(super) fn attach_initial_prompt_session(&mut self) -> bool {
        let Some(snapshot) = self.initial_prompt_intent.as_ref() else {
            return false;
        };
        if snapshot.state != crate::protocol::InitialPromptIntentState::LaunchAccepted {
            return false;
        }
        let (Some(session), Some(run)) = (snapshot.session, snapshot.run) else {
            return false;
        };
        let id = snapshot.id.clone();
        let focused = self.initial_prompt_focus_pending.as_ref() == Some(&id);
        let Some(summary) = find_session(self, session)
            .filter(|summary| summary.run == run)
            .cloned()
        else {
            return false;
        };

        if let Some(token) = self
            .lifecycle_pending
            .as_ref()
            .filter(|row| row.initial_prompt_intent.as_ref() == Some(&id))
            .map(|row| row.token)
        {
            self.accept_lifecycle_job(token);
        }
        if self
            .provisional
            .as_ref()
            .is_some_and(|row| row.initial_prompt_intent.as_ref() == Some(&id))
        {
            self.provisional = None;
        }
        self.lifecycle_strip = None;
        if focused {
            self.initial_prompt_focus_pending = None;
            self.select_created_session(summary);
        } else {
            self.lifecycle_preference = None;
        }
        true
    }

    pub(super) fn pause_request(&mut self, paused: bool) -> DashboardAction {
        let Some(session) = self.action_session().and_then(|id| find_session(self, id)) else {
            self.set_limit("No session selected");
            return DashboardAction::Redraw;
        };
        if !session.phase.is_live() {
            self.set_limit("Session is not running");
            return DashboardAction::Redraw;
        }
        let session = session.id;
        let request_id = self.error_owning_request_id();
        self.enqueue_request(ClientMessage {
            request_id,
            request: if paused {
                Request::PauseSession { session }
            } else {
                Request::ResumeSession { session }
            },
        });
        DashboardAction::Redraw
    }

    pub fn handle_server_message(&mut self, message: ServerMessage) -> Vec<ClientMessage> {
        #[cfg(target_os = "macos")]
        if self.handle_iterm_setup_message(&message) {
            return self.drain_outbox();
        }
        if self.handle_notification_navigation(&message) {
            self.cancel_sidebar_jump();
            return self.drain_outbox();
        }
        if let ServerMessage::Response {
            request_id,
            response,
        } = &message
            && self.settings_editor_response(*request_id, response)
        {
            return self.drain_outbox();
        }
        if let ServerMessage::Response {
            request_id,
            response,
        } = &message
            && self.operator_response(*request_id, response)
        {
            return self.drain_outbox();
        }
        if let ServerMessage::Response {
            request_id,
            response,
        } = &message
            && self.events_response(*request_id, response)
        {
            return self.drain_outbox();
        }
        if let ServerMessage::Response {
            request_id,
            response,
        } = &message
            && self.transcript_response(*request_id, response)
        {
            return self.drain_outbox();
        }
        if let ServerMessage::Response {
            request_id,
            response,
        } = &message
            && self.initial_prompt_reconciliation_response(*request_id, response)
        {
            return self.drain_outbox();
        }
        if let ServerMessage::Response { request_id, .. } = &message
            && self.ignored_responses.remove(request_id)
        {
            self.pending_initial_prompt_allocation
                .take_if(|pending| pending.request_id == *request_id);
            return Vec::new();
        }
        if let ServerMessage::Response {
            request_id,
            response,
        } = &message
            && self.initial_prompt_allocation_response(*request_id, response)
        {
            return self.drain_outbox();
        }
        if let ServerMessage::Response {
            request_id,
            response,
        } = &message
            && let Some(&(session, run)) = self
                .recovery_requests
                .iter()
                .find_map(|(target, id)| (*id == Some(*request_id)).then_some(target))
        {
            self.recovery_requests.insert((session, run), None);
            // Hierarchy events, not launch receipts, install replacement runs. A late
            // receipt cannot select a hidden pane or overwrite a newer run's error.
            if let Response::Error { code, message } = response
                && self
                    .desired_view()
                    .targets
                    .iter()
                    .any(|(id, current, _)| *id == session && *current == run)
            {
                self.set_server_error(code.clone(), message);
            }
            return self.drain_outbox();
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
        let (was_view_request, owns_error, was_input) = match &message {
            ServerMessage::Response {
                request_id,
                response,
            } => self.retire_request_id(*request_id, response),
            ServerMessage::Event(_) => (false, false, false),
        };
        match message {
            ServerMessage::Response {
                request_id,
                response,
            } => match response {
                Response::AgentOperation(_)
                | Response::NotificationNavigation(_)
                | Response::NotificationNavigationConfirmed(_)
                | Response::ITermFocusPrepared(_)
                | Response::BridgeOwner(_)
                | Response::ScreenWindow(_) => {}
                Response::InitialPromptIntent(id) => {
                    self.initial_prompt_intent_id = Some(id);
                }
                Response::InitialPromptIntents(snapshots) => {
                    self.install_initial_prompt_snapshots(&snapshots);
                }
                Response::QuotaCooldown { remaining_ms } => self.set_limit(format!(
                    "Quota refresh cooling down: {}s left",
                    remaining_ms.div_ceil(1_000)
                )),
                Response::Hierarchy(hierarchy) => {
                    // Reconnect drops client Provisional fiction; Server hierarchy is truth.
                    self.drop_provisional_fiction();
                    for request in self.update_hierarchy(hierarchy) {
                        self.push_request(request);
                    }
                    self.attach_initial_prompt_session();
                }
                Response::Screen {
                    session,
                    run,
                    revision,
                    size,
                    bytes,
                } => {
                    let matched_screen = self
                        .handshake
                        .snapshot_matches(request_id, revision, session, run);
                    self.apply_screen(request_id, revision, session, run, size, &bytes);
                    if matched_screen
                        && let Some(view) = self.history.as_mut()
                        && view.opened.session == session
                    {
                        view.new_output = true;
                    }
                }
                Response::Ok => {
                    let desired_now = self.desired_view();
                    match self.handshake.acknowledge(request_id, &desired_now) {
                        Some(Acknowledged::Granted) => {
                            // A server-driven refresh completes the same way a user's selection
                            // does, so the banner is cleared only when a refused view wrote it or
                            // when this view is the one the user's own selection, split, focus, or
                            // pane close asked for.
                            if self.error_owned_by_view || owns_error {
                                self.dismiss_error_banner();
                                self.error_owned_by_view = false;
                            }
                            self.queue_deferred_history_request();
                        }
                        Some(Acknowledged::Incomplete { view }) => {
                            if self
                                .agent_typing
                                .as_ref()
                                .is_some_and(|intent| intent.revision == view.revision)
                            {
                                self.agent_typing = None;
                                self.set_error(
                                    "Agent switch cancelled: incomplete view acknowledgement",
                                );
                                self.error_owned_by_view = true;
                            }
                            // The first retry is immediate, because no failure is recorded while
                            // a request is in flight; recording it after that re-request makes
                            // the next snapshot-less `Ok` wait out the backoff instead of
                            // spinning one `SetView` per acknowledgement.
                            let _ = self.request_view_at(self.outer_area);
                            self.handshake.record_failure(view);
                        }
                        Some(Acknowledged::Refresh) => {
                            let _ = self.request_view_at(self.outer_area);
                        }
                        None => {
                            // Only a request the user asked for owns the banner; a view ack, the
                            // geometry handshake, and a synthetic mouse release leave a fresh
                            // error in place.
                            if owns_error && !self.history_page_error {
                                self.dismiss_error_banner();
                                self.error_owned_by_view = false;
                            }
                        }
                    }
                }
                Response::CreatedSession(session) => {
                    if !self.stale_created_session(&session) {
                        self.dismiss_error_banner();
                    }
                }
                Response::Inventory { .. } | Response::TerminalText { .. } | Response::Task(_) => {
                    self.dismiss_error_banner()
                }
                // A snapshot this popup did not ask for. The one it did ask
                // for was claimed above.
                Response::Events(_)
                | Response::Workflows(_)
                | Response::WorkflowCandidates(_)
                | Response::WorkflowParents(_)
                | Response::WorkflowRecovery(_)
                | Response::Delegation(_)
                | Response::DelegationObservation(_)
                | Response::DelegationCapabilities(_)
                | Response::AssignedRead(_)
                | Response::Operator(_)
                | Response::OperatorExchange(_)
                | Response::OperatorWorker(_)
                | Response::Transcript(_) => {}
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
                    // No view is acknowledged any more, so input stays revoked and the refused
                    // view is recorded for one backoff-delayed retry.
                    let refused = self.handshake.refuse(request_id, Instant::now());
                    if let Some(view) = &refused {
                        if self
                            .agent_typing
                            .as_ref()
                            .is_some_and(|intent| intent.revision == view.revision)
                        {
                            self.agent_typing = None;
                        }
                        for (session, _, _) in &view.targets {
                            if let Some(pane) = self
                                .panes
                                .iter_mut()
                                .find(|pane| pane.session == Some(*session))
                            {
                                pane.error = Some(message.clone());
                            }
                        }
                    }
                    let matched_view = refused.is_some();
                    let begin = self
                        .history_begin_request
                        .take_if(|pending| pending.request_id == request_id);
                    matched_history |= begin.is_some();
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
                    // History cannot freeze a program on the alternate screen; the Server says
                    // so with Conflict, a limit of that program rather than a failure. This
                    // relies on `dispatch_history_begin` answering Conflict only for the
                    // alternate screen (its other Conflict, a disconnected dashboard, never
                    // reaches a live one).
                    if let Some(begin) = begin.filter(|_| code == ErrorCode::Conflict) {
                        self.alternate_screen_history(&begin);
                    } else if matched_view {
                        // A refused view is a limit of the moment: the retry follows.
                        let text = self.plain_error(&message);
                        self.set_limit(text);
                        // Only a refused view hands the banner to the next view completion.
                        self.error_owned_by_view = true;
                    } else if was_input
                        && matches!(code, ErrorCode::Conflict | ErrorCode::InvalidRequest)
                    {
                        // Input the Server did not admit (assigned, paused, no longer focused)
                        // is a limit; a failed write stays `Internal`, a failure.
                        let text = self.plain_error(&message);
                        self.set_limit(text);
                    } else if !was_view_request {
                        self.set_server_error(code, &message);
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
                ServerEvent::BridgeContext(_)
                | ServerEvent::NotificationNavigation(_)
                | ServerEvent::ITermFocus(_) => {}
                ServerEvent::OperatorChanged(id) => self.operator_changed(id),
                ServerEvent::AgentModels {
                    request_id,
                    capabilities,
                } => self.agent_models_event(request_id, capabilities),
                ServerEvent::HierarchyChanged(hierarchy) => {
                    for request in self.update_hierarchy(hierarchy) {
                        self.push_request(request);
                    }
                    self.try_attach_lifecycle_workspace_from_hierarchy();
                    self.attach_initial_prompt_session();
                }
                ServerEvent::LifecycleCompleted {
                    client_token,
                    op,
                    outcome,
                    target_workspace,
                } => {
                    self.on_lifecycle_completed_with_target(
                        client_token,
                        op,
                        outcome,
                        target_workspace,
                    );
                }
                ServerEvent::InitialPromptIntentChanged(snapshot) => {
                    self.install_initial_prompt_snapshot(*snapshot);
                }
                ServerEvent::Output {
                    session,
                    run,
                    revision,
                    bytes,
                } => {
                    if revision == self.handshake.revision()
                        && self.handshake.is_ready(session)
                        && self.handshake.current_run(session) == Some(run)
                    {
                        if let Some(pane) = self
                            .panes
                            .iter_mut()
                            .find(|pane| pane.session == Some(session) && pane.run == Some(run))
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
                ServerEvent::ScreenDirty {
                    session,
                    run,
                    revision,
                } => {
                    if revision == self.handshake.revision()
                        && self.handshake.current_run(session) == Some(run)
                        && self
                            .panes
                            .iter()
                            .any(|pane| pane.session == Some(session) && pane.run == Some(run))
                    {
                        self.handshake.mark_stale(session);
                        if let Some(view) = self.history.as_mut()
                            && view.opened.session == session
                        {
                            view.new_output = true;
                        }
                        if !self.handshake.in_flight() {
                            let _ = self.request_view_at(self.outer_area);
                        }
                    }
                }
                ServerEvent::QuotaChanged(snapshot) => {
                    self.quotas = Some(*snapshot);
                    self.ensure_selection_visible(&self.visible_rows());
                }
                ServerEvent::SettingsChanged(report) => self.install_settings_report(*report),
                ServerEvent::Recorded(event) => self.events_recorded(event),
                ServerEvent::WipSavePrompt {
                    project,
                    workspace,
                    branch,
                } => self.queue_wip_prompt(project, workspace, branch),
                ServerEvent::SessionChanged(summary) => {
                    if find_session(self, summary.id).is_some_and(|current| {
                        current.run.0 > summary.run.0
                            || (current.run == summary.run
                                && super::assignment_control::older_control(&summary, current))
                    }) {
                        return self.drain_outbox();
                    }
                    self.assignment_control_changed(&summary);
                    self.observe_desktop_update(&summary);
                    let summary = *summary;
                    let previous_live =
                        find_session(self, summary.id).map(|session| session.phase.is_live());
                    let tree = self.sidebar_rects(self.outer_area).tree;
                    for session in self
                        .hierarchy
                        .projects
                        .iter_mut()
                        .flat_map(|project| project.workspaces.iter_mut())
                        .flat_map(|workspace| workspace.sessions.iter_mut())
                    {
                        if session.id == summary.id {
                            *session = summary.clone();
                            break;
                        }
                    }
                    if self.transcript.session == Some(summary.id)
                        && self.transcript.run != Some(summary.run)
                    {
                        self.close_transcript();
                    }
                    // The attention strip comes and goes with sessions; keep the selection shown.
                    if tree != self.sidebar_rects(self.outer_area).tree
                        || self.settings.sidebar_view == crate::protocol::SidebarView::Recent
                    {
                        self.ensure_selection_visible(&self.visible_rows());
                    }
                    if self.resync_session_run(&summary, previous_live) {
                        let _ = self.request_view_at(self.outer_area);
                    }
                    self.update_mode_for_selected_phase();
                    self.attach_initial_prompt_session();
                }
            },
        }
        if self.sidebar_jump.is_some()
            && (!matches!(self.mode, InputMode::Browse | InputMode::Terminal)
                || self.sidebar_preference() == Some(0)
                || self.sidebar_jump_modal_priority_open()
                || !self.mouse_focused)
        {
            self.cancel_sidebar_jump();
        }
        self.refresh_agent_search();
        self.finish_agent_switch();
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

    pub(super) fn update_hierarchy(
        &mut self,
        mut hierarchy: HierarchySnapshot,
    ) -> Vec<ClientMessage> {
        let focused_before = self.focused_session();
        let previous_live: Vec<_> = self
            .panes
            .iter()
            .filter_map(|pane| {
                let id = pane.session?;
                find_session(self, id).map(|session| (id, session.phase.is_live()))
            })
            .collect();
        let previous_summaries: HashMap<_, _> = self
            .hierarchy
            .projects
            .iter()
            .flat_map(|project| project.workspaces.iter())
            .flat_map(|workspace| workspace.sessions.iter())
            .map(|session| (session.id, session))
            .collect();
        for incoming in hierarchy
            .projects
            .iter_mut()
            .flat_map(|project| project.workspaces.iter_mut())
            .flat_map(|workspace| workspace.sessions.iter_mut())
        {
            if let Some(previous) = previous_summaries.get(&incoming.id)
                && (previous.run.0 > incoming.run.0
                    || (previous.run == incoming.run
                        && super::assignment_control::older_control(incoming, previous)))
            {
                *incoming = (**previous).clone();
            }
        }
        let control_target = focused_before
            .and_then(|id| {
                hierarchy
                    .projects
                    .iter()
                    .flat_map(|project| project.workspaces.iter())
                    .flat_map(|workspace| workspace.sessions.iter())
                    .find(|summary| summary.id == id)
            })
            .cloned();
        if let Some(summary) = control_target {
            self.assignment_control_changed(&summary);
        }
        self.observe_desktop_responses(&hierarchy);
        let tree = self.sidebar_rects(self.outer_area).tree;
        self.hierarchy = hierarchy;
        if self.transcript.session.is_some_and(|id| {
            !find_session(self, id).is_some_and(|summary| Some(summary.run) == self.transcript.run)
        }) {
            self.close_transcript();
        }
        if tree != self.sidebar_rects(self.outer_area).tree
            || self.settings.sidebar_view == crate::protocol::SidebarView::Recent
        {
            self.ensure_selection_visible(&self.visible_rows());
        }
        self.sync_palette_workspace_identity();

        self.selected_container = self.selected_container.take().filter(|row| match row {
            TreeRow::Project { name } => self
                .hierarchy
                .projects
                .iter()
                .any(|project| &project.name == name),
            TreeRow::Workspace { project, id } => find_workspace(self, project, id).is_some(),
            TreeRow::Session { .. }
            | TreeRow::ProvisionalSession { .. }
            | TreeRow::ProvisionalWorkspace { .. } => false,
        });
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
            let focused_survives = focused_before.is_some_and(|id| existing.contains(&id));
            self.retarget(PaneChange::ServerRemoved { focused_survives });
            let remove = self
                .panes
                .iter()
                .enumerate()
                .filter_map(|(index, pane)| {
                    (!pane.session.is_some_and(|id| existing.contains(&id))).then_some(index)
                })
                .collect::<Vec<_>>();
            for index in remove.into_iter().rev() {
                let layout = std::mem::replace(&mut self.pane_layout, super::PaneLayout::Pane(0));
                self.pane_layout = layout.remove(index).unwrap_or(super::PaneLayout::Pane(0));
                self.panes.remove(index);
            }
            if self.panes.is_empty() {
                self.panes.push(super::PaneState::new(self.focused_size()));
                self.pane_layout = super::PaneLayout::Pane(0);
                self.focused_pane = 0;
            } else {
                self.focused_pane = focused_before
                    .and_then(|session| {
                        self.panes
                            .iter()
                            .position(|pane| pane.session == Some(session))
                    })
                    .unwrap_or_else(|| self.focused_pane.min(self.panes.len() - 1));
            }
            if !self.operator.visible
                && self.panes.iter().all(|pane| pane.session.is_none())
                && let Some(id) = self.visible_rows().iter().find_map(|row| match row {
                    TreeRow::Session { id } => Some(*id),
                    TreeRow::Project { .. }
                    | TreeRow::Workspace { .. }
                    | TreeRow::ProvisionalWorkspace { .. }
                    | TreeRow::ProvisionalSession { .. } => None,
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
        let mut run_changed = false;
        let summaries: Vec<_> = self
            .panes
            .iter()
            .filter_map(|pane| pane.session)
            .filter_map(|id| find_session(self, id))
            .cloned()
            .collect();
        for summary in &summaries {
            run_changed |= self.resync_session_run(
                summary,
                previous_live
                    .iter()
                    .find(|(id, _)| *id == summary.id)
                    .map(|(_, live)| *live),
            );
        }
        if run_changed {
            let request_id = self.next_request_id();
            if let Ok(Some(request)) = self.view_request(self.outer_area, request_id) {
                outgoing.push(request);
            }
        }
        outgoing.extend(self.attach_created_workspace());
        outgoing
    }

    pub(super) fn mark_reviewed_request(&mut self) -> DashboardAction {
        let Some(session) = self.action_session() else {
            return DashboardAction::None;
        };
        let Some(expected) = self.unread.review_target(session) else {
            return DashboardAction::None;
        };
        let request_id = self.error_owning_request_id();
        self.enqueue_request(ClientMessage {
            request_id,
            request: Request::MarkReviewed { session, expected },
        });
        DashboardAction::Redraw
    }

    /// A keystroke or paste into a session reviews the Unread it displays, with the request
    /// `R` sends, queued ahead of the input. The response is ignored: a stale refusal means a
    /// newer Ready arrived, and that one stays Unread.
    fn review_on_reply(&mut self) {
        let Some(session) = self.action_session() else {
            return;
        };
        let Some(expected) = self.unread.reply_target(session) else {
            return;
        };
        let request_id = self.next_request_id();
        self.ignored_responses.insert(request_id);
        self.enqueue_request(ClientMessage {
            request_id,
            request: Request::MarkReviewed { session, expected },
        });
    }

    pub fn input_request(&mut self, bytes: Vec<u8>, request_id: u64) -> Option<ClientMessage> {
        if self.navigation.discard_input
            || self.whichkey.is_some()
            || self.palette.is_some()
            || self.tasks.is_some()
            || self.mode != InputMode::Terminal
            || !self.input_is_allowed()
        {
            return None;
        }
        let session = self.action_session()?;
        let run = self.input_run(session)?;
        self.error_owning_requests.insert(request_id);
        self.input_requests.insert(request_id);
        Some(ClientMessage {
            request_id,
            request: self.terminal_input_request(session, run, bytes),
        })
    }

    fn terminal_input_request(
        &self,
        session: SessionId,
        run: SessionRunId,
        bytes: Vec<u8>,
    ) -> Request {
        match find_session(self, session).and_then(|summary| summary.assignment_control.as_ref()) {
            Some(view) => Request::AssignedHumanInput {
                session,
                run,
                control_epoch: view.control.epoch,
                view_revision: self.view_revision(),
                bytes,
            },
            None => Request::Input {
                session,
                run,
                bytes,
            },
        }
    }

    fn input_run(&self, session: SessionId) -> Option<SessionRunId> {
        self.panes
            .iter()
            .find(|pane| pane.session == Some(session))
            .and_then(|pane| pane.run)
            .or_else(|| find_session(self, session).map(|summary| summary.run))
    }

    pub(super) fn stale_created_session(&self, created: &crate::session::SessionSummary) -> bool {
        find_session(self, created.id).is_some_and(|current| current.run != created.run)
    }

    fn resync_session_run(
        &mut self,
        summary: &crate::session::SessionSummary,
        previous_live: Option<bool>,
    ) -> bool {
        let became_live = summary.phase.is_live() && previous_live == Some(false);
        let mut changed = false;
        for pane in &mut self.panes {
            if pane.session != Some(summary.id) {
                continue;
            }
            if pane.run != Some(summary.run) || became_live {
                let size = pane.size;
                pane.run = Some(summary.run);
                pane.parser = vt100::Parser::new(size.rows.max(1), size.cols.max(1), 0);
                pane.error = None;
                changed = true;
            }
        }
        if !changed {
            return false;
        }
        if self.agent_typing.take().is_some() {
            self.set_error("Agent switch cancelled: pane run changed");
        }
        self.dismiss_close_confirm_for(summary.id);
        self.unread.forget_session(summary.id);
        if self
            .copy
            .as_ref()
            .is_some_and(|copy| copy.session == summary.id)
        {
            self.cancel_copy(None);
        }
        if self
            .history
            .as_ref()
            .is_some_and(|view| view.opened.session == summary.id)
            || self
                .history_begin_request
                .as_ref()
                .is_some_and(|pending| pending.session == summary.id)
        {
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
        self.mark_pending_parser_discarded();
        self.invalidate_view_readiness();
        true
    }

    pub(super) fn selected_phase(&self) -> Option<&SessionPhase> {
        self.action_session()
            .and_then(|id| find_session(self, id))
            .map(|session| &session.phase)
    }

    pub(super) fn input_is_allowed(&self) -> bool {
        if self.operator.visible {
            return false;
        }
        let Some(session) = self.action_session() else {
            return false;
        };
        matches!(self.selected_phase(), Some(SessionPhase::Running))
            && find_session(self, session)
                .and_then(super::assignment_control::session_input_denial)
                .is_none()
            && self.handshake.is_ready(session)
            && self
                .handshake
                .acknowledged()
                .is_some_and(|view| view.focused == Some(session))
    }

    pub(super) fn refuse_input(&mut self) -> DashboardAction {
        let refusal: String = match self.selected_phase() {
            Some(SessionPhase::Paused) => "Session paused; press r to resume".into(),
            Some(SessionPhase::Exited { .. }) => "Session exited".into(),
            Some(SessionPhase::Stopped) => "Session stopped".into(),
            Some(SessionPhase::Interrupted) => "Session interrupted".into(),
            None => "No session selected".into(),
            Some(SessionPhase::Running)
                if !self
                    .focused_pane()
                    .is_some_and(|pane| self.pane_ready(pane)) =>
            {
                "Pane is loading; retry input".into()
            }
            Some(SessionPhase::Running) => {
                let denial = self
                    .action_session()
                    .and_then(|id| find_session(self, id))
                    .and_then(super::assignment_control::session_input_denial);
                match denial {
                    Some(reason) => reason.into(),
                    None => return DashboardAction::None,
                }
            }
        };
        self.set_limit(refusal);
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
    pub(super) fn error_owning_request_id(&mut self) -> u64 {
        let id = self.next_request_id();
        self.error_owning_requests.insert(id);
        id
    }

    /// Retires a request id once its final response arrives and reports what that id owned: a
    /// `SetView` that was still in flight, and the error banner. Only a view snapshot is not
    /// final, so every other response releases the id; both sets would otherwise grow for the
    /// life of the dashboard.
    fn retire_request_id(&mut self, request_id: u64, response: &Response) -> (bool, bool, bool) {
        let was_view_request = self.handshake.was_view_request(request_id);
        let owns_error = self.error_owning_requests.contains(&request_id);
        let was_input = self.input_requests.contains(&request_id);
        if !matches!(response, Response::Screen { .. }) {
            self.handshake.retire(request_id);
            self.error_owning_requests.remove(&request_id);
            self.input_requests.remove(&request_id);
        }
        (was_view_request, owns_error, was_input)
    }
}
/// How long a footer banner stands before it clears itself: long enough to read, never
/// minutes. A later success or a newer banner can end it sooner.
pub(super) const BANNER_LIFETIME: Duration = Duration::from_secs(10);

/// An id the Server generated for a workspace: 32 hexadecimal digits.
fn internal_id(word: &str) -> bool {
    word.len() == 32 && word.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_input(request: &Request) -> bool {
    matches!(
        request,
        Request::Input { .. } | Request::AssignedHumanInput { .. }
    )
}

/// `text` with each stand-alone `from` replaced by `to`. An occurrence inside a path (a `/`
/// on either side) or inside a longer word stays: a worktree's real directory may be named
/// after its internal id.
pub(super) fn replace_outside_paths(text: &str, from: &str, to: &str) -> String {
    let joined = |c: char| c == '/' || c.is_alphanumeric() || matches!(c, '-' | '_' | '.');
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        let before = rest[..at].chars().next_back();
        let after = &rest[at + from.len()..];
        // A trailing `.` or `-` after the match ends a sentence or clause; a `/` continues a path.
        let inside = before.is_some_and(joined)
            || after.starts_with('/')
            || after.chars().next().is_some_and(char::is_alphanumeric);
        out.push_str(&rest[..at]);
        out.push_str(if inside { from } else { to });
        rest = after;
    }
    out.push_str(rest);
    out
}

pub(super) fn find_workspace<'a>(
    dashboard: &'a Dashboard,
    project: &str,
    id: &str,
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
                .find(|workspace| workspace.id == id)
        })
}

fn workspace_name_duplicated(
    dashboard: &Dashboard,
    workspace: &ovrcr_protocol::WorkspaceSummary,
) -> bool {
    dashboard
        .hierarchy
        .projects
        .iter()
        .find(|project| project.name == workspace.project)
        .is_some_and(|project| {
            project
                .workspaces
                .iter()
                .filter(|candidate| candidate.name == workspace.name)
                .count()
                > 1
        })
}

pub(super) fn workspace_label(
    dashboard: &Dashboard,
    workspace: &ovrcr_protocol::WorkspaceSummary,
) -> String {
    match workspace_disambiguator(dashboard, workspace) {
        Some(suffix) => format!("{} ({})", workspace.name, suffix),
        None => workspace.name.clone(),
    }
}

pub(super) fn workspace_disambiguator(
    dashboard: &Dashboard,
    workspace: &ovrcr_protocol::WorkspaceSummary,
) -> Option<String> {
    if !workspace_name_duplicated(dashboard, workspace) {
        return None;
    }
    let others: Vec<&Path> = dashboard
        .hierarchy
        .projects
        .iter()
        .find(|project| project.name == workspace.project)
        .map(|project| {
            project
                .workspaces
                .iter()
                .filter(|candidate| {
                    candidate.name == workspace.name && candidate.id != workspace.id
                })
                .map(|candidate| candidate.path.as_path())
                .collect()
        })
        .unwrap_or_default();
    Some(distinguishing_path_suffix(&workspace.path, &others))
}

fn distinguishing_path_suffix(path: &Path, others: &[&Path]) -> String {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    if let Some(file_name) = file_name.as_ref()
        && !file_name.is_empty()
        && others.iter().all(|other| {
            other
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .as_deref()
                != Some(file_name.as_str())
        })
    {
        return file_name.clone();
    }
    let comps: Vec<_> = path.iter().collect();
    for n in 1..=comps.len() {
        let suffix = PathBuf::from_iter(&comps[comps.len() - n..]);
        let unique = others.iter().all(|other| {
            let other_comps: Vec<_> = other.iter().collect();
            other_comps.len() < n
                || other_comps[other_comps.len() - n..] != comps[comps.len() - n..]
        });
        if unique {
            return suffix.display().to_string();
        }
    }
    path.display().to_string()
}

pub(super) fn workspace_heading(
    dashboard: &Dashboard,
    workspace: &ovrcr_protocol::WorkspaceSummary,
) -> String {
    format!(
        "{} / {}",
        workspace.project,
        workspace_label(dashboard, workspace)
    )
}

pub(super) fn session_workspace_heading(
    dashboard: &Dashboard,
    session: &crate::session::SessionSummary,
) -> String {
    find_workspace(dashboard, &session.project, &session.workspace)
        .map(|workspace| workspace_heading(dashboard, workspace))
        .unwrap_or_else(|| "unavailable".into())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        AgentActivity, ProjectSummary, SessionKind, SessionPhase, SessionSummary, WorkspaceSummary,
    };

    fn hierarchy_with_run(id: u64, run: u64) -> HierarchySnapshot {
        HierarchySnapshot {
            projects: vec![ProjectSummary {
                name: "demo".into(),
                workspaces: vec![WorkspaceSummary {
                    project: "demo".into(),
                    name: "root".into(),
                    id: "root".into(),
                    root: false,
                    warning: None,
                    path: "/tmp/unused".into(),
                    sessions: vec![SessionSummary {
                        scope: ovrcr_protocol::SessionScope::Workspace,
                        archived: false,
                        cwd: "/work".into(),
                        id: crate::session::SessionId(id),
                        run: SessionRunId(run),
                        kind: SessionKind::Terminal,
                        recovery: None,
                        project: "demo".into(),
                        workspace: "root".into(),
                        name: format!("root-{id}"),
                        title: None,
                        manual_title: None,
                        assignment_control: None,
                        label: "shell".into(),
                        pid: None,
                        started_unix_ms: Some(0),
                        last_activity_unix_ms: None,
                        phase: SessionPhase::Running,
                        activity: AgentActivity::Unknown,
                        context_usage: None,
                        agent: None,
                        agent_epoch: 0,
                        unread: None,
                    }],
                }],
            }],
        }
    }

    fn prompt_snapshot(
        id: crate::protocol::InitialPromptIntentId,
        session: u64,
        run: u64,
    ) -> crate::protocol::InitialPromptIntentSnapshot {
        crate::protocol::InitialPromptIntentSnapshot {
            id,
            state: crate::protocol::InitialPromptIntentState::LaunchAccepted,
            project: "demo".into(),
            workspace: "root".into(),
            name: "agent".into(),
            label: Some("claude".into()),
            session: Some(crate::session::SessionId(session)),
            run: Some(SessionRunId(run)),
            recoverable_prompt: None,
            recoverable_launch: None,
            message: None,
            delivery: Some(crate::protocol::InitialPromptDelivery::Unknown),
        }
    }

    fn intent_id(sequence: u64) -> crate::protocol::InitialPromptIntentId {
        crate::protocol::InitialPromptIntentId {
            server_lifetime: "123e4567-e89b-12d3-a456-426614174000".into(),
            sequence,
        }
    }

    #[test]
    fn prompt_focus_waits_for_exact_run_and_reconciliation_is_idempotent() {
        let mut dashboard = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
        let intent = intent_id(1);
        dashboard.initial_prompt_intent_id = Some(intent.clone());
        dashboard.initial_prompt_focus_pending = Some(intent.clone());
        let mut mismatched = hierarchy_with_run(8, 1);
        let mut stale_same_id = hierarchy_with_run(7, 3);
        let other = mismatched.projects[0].workspaces[0].sessions.remove(0);
        stale_same_id.projects[0].workspaces[0].sessions.push(other);
        dashboard.hierarchy = stale_same_id;

        let snapshot = prompt_snapshot(intent.clone(), 7, 2);
        dashboard.install_initial_prompt_snapshot(snapshot.clone());
        assert!(dashboard.initial_prompt_focus_pending.is_some());
        assert_eq!(dashboard.focused_session(), None);

        dashboard.hierarchy = hierarchy_with_run(7, 2);
        let mut matching = dashboard.hierarchy.projects[0].workspaces[0].sessions[0].clone();
        matching.kind = SessionKind::Agent {
            name: "claude".into(),
        };
        matching.label = "claude".into();
        dashboard.hierarchy.projects[0].workspaces[0].sessions[0] = matching;
        assert!(dashboard.attach_initial_prompt_session());
        assert_eq!(
            dashboard.focused_session(),
            Some(crate::session::SessionId(7))
        );
        assert!(dashboard.initial_prompt_focus_pending.is_none());

        let second = hierarchy_with_run(8, 1).projects[0].workspaces[0].sessions[0].clone();
        dashboard.hierarchy.projects[0].workspaces[0]
            .sessions
            .push(second);
        dashboard.select_session(crate::session::SessionId(8));
        dashboard.install_initial_prompt_snapshot(snapshot);
        assert_eq!(
            dashboard.focused_session(),
            Some(crate::session::SessionId(8))
        );
    }

    #[test]
    fn pending_or_unknown_bound_prompt_intents_never_focus_the_session() {
        for intent_state in [
            crate::protocol::InitialPromptIntentState::Pending,
            crate::protocol::InitialPromptIntentState::Unknown,
        ] {
            let mut dashboard = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
            let intent = intent_id(1);
            dashboard.initial_prompt_intent_id = Some(intent.clone());
            dashboard.initial_prompt_focus_pending = Some(intent.clone());
            let mut hierarchy = hierarchy_with_run(7, 2);
            let session = &mut hierarchy.projects[0].workspaces[0].sessions[0];
            session.kind = SessionKind::Agent {
                name: "claude".into(),
            };
            session.label = "claude".into();
            dashboard.hierarchy = hierarchy;

            let mut snapshot = prompt_snapshot(intent, 7, 2);
            snapshot.state = intent_state;
            snapshot.delivery = None;
            dashboard.install_initial_prompt_snapshot(snapshot);

            assert!(!dashboard.attach_initial_prompt_session());
            assert_eq!(dashboard.focused_session(), None);
        }
    }

    #[test]
    fn delayed_older_prompt_receipt_cannot_replace_newer_allocated_intent() {
        let mut dashboard = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
        let newer = intent_id(2);
        dashboard.initial_prompt_intent_id = Some(newer.clone());

        dashboard.install_initial_prompt_snapshot(prompt_snapshot(intent_id(1), 7, 2));

        assert_eq!(dashboard.initial_prompt_intent_id.as_ref(), Some(&newer));
        assert!(dashboard.initial_prompt_intent.is_none());
    }

    #[test]
    fn old_generic_lifecycle_token_cannot_complete_prompt_row() {
        let mut dashboard = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
        let intent = intent_id(2);
        let request =
            Request::CreateAgentSessionWithPrompt(crate::protocol::InitialPromptLaunchRequest {
                intent,
                launch: crate::protocol::CreateAgentSessionRequest {
                    session: crate::protocol::CreateSessionRequest {
                        project: "demo".into(),
                        workspace: "root".into(),
                        name: "agent".into(),
                        label: Some("claude".into()),
                        argv: vec!["claude".into()],
                        kind: SessionKind::Agent {
                            name: "claude".into(),
                        },
                    },
                    model_policy: crate::protocol::AgentModelPolicy::HarnessDefault,
                    model_binding: None,
                    expected_model: None,
                },
                prompt: String::new(),
            });
        dashboard.begin_lifecycle_pending(3, &request);
        dashboard.accept_lifecycle_job(3);
        dashboard.on_lifecycle_completed(
            3,
            crate::protocol::LifecycleOp::CreateSession,
            crate::protocol::LifecycleOutcome::Succeeded,
        );
        assert!(dashboard.provisional.is_some());
    }

    #[test]
    fn run_replacement_leaves_history_mode() {
        let mut dashboard = Dashboard::new(TerminalSize { rows: 24, cols: 80 });
        dashboard.hierarchy = hierarchy_with_run(1, 1);
        dashboard.select_session(crate::session::SessionId(1));
        dashboard.mode = InputMode::History;
        dashboard.history = Some(HistoryView::new(
            HistoryOpened {
                session: crate::session::SessionId(1),
                snapshot: HistorySnapshotId(7),
                revision: 1,
                size: TerminalSize { rows: 24, cols: 80 },
                history_rows: 0,
                total_rows: 1,
            },
            0,
        ));
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
            hierarchy_with_run(1, 2),
        )));
        assert_eq!(dashboard.mode, InputMode::Browse);
        assert!(
            dashboard.history.is_none(),
            "history capture must be gone after run replacement"
        );
    }
}

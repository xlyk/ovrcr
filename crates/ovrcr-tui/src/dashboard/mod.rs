mod agents;
mod copy;
mod event_loop;
mod input;
mod palette;
mod picker;
mod render;
mod settings;
mod state;
mod terminal_guard;
#[cfg(test)]
mod tests;

pub use agents::detect_agents;
pub use copy::{
    CopyMotion, CopyPoint, CopySelection, HistoryCopyCompletion, HistoryCopyJob, HistoryCopyPoint,
    HistoryCopyRange, append_history_selection, write_clipboard,
};
pub use event_loop::{dashboard_message_channel, run_dashboard};
pub use input::{encode_key, encode_mouse, event_to_request};
pub use render::{
    actual_drawn_inner_rect, draw_dashboard, draw_dashboard_at, render_copy, render_history,
    render_terminal,
};
pub use state::{
    HistoryCursor, HistoryCursorTarget, HistoryPagePurpose, HistoryView, PendingHistoryBegin,
    PendingHistoryPage,
};
pub use terminal_guard::TerminalGuard;

use crate::task_tui::TasksView;
use crossterm::event::MouseEvent;
use ovrcr_protocol::{ClientMessage, HierarchySnapshot, SessionId, TerminalSize};
use ovrcr_terminal::vt100;
use ratatui::layout::Rect;
use std::collections::HashSet;

pub const DASHBOARD_READER_QUEUE_CAPACITY: usize = 64;

pub fn history_view_size(pane: TerminalSize) -> TerminalSize {
    TerminalSize {
        rows: pane.rows.clamp(1, 64),
        cols: pane.cols.clamp(1, 256),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Browse,
    Terminal,
    History,
    Copy,
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
    CopyText(String),
    Request(ClientMessage),
    RequestBatch(Vec<ClientMessage>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyEncoding {
    Browse,
    Bytes(Vec<u8>),
    Ignore,
}

pub struct PaneState {
    pub session: Option<SessionId>,
    pub parser: vt100::Parser,
    pub size: TerminalSize,
    pub desired_size: TerminalSize,
    pub snapshot_installed: bool,
    pub ready: bool,
    pub error: Option<String>,
}

impl PaneState {
    pub fn new(size: TerminalSize) -> Self {
        Self {
            session: None,
            parser: vt100::Parser::new(size.rows, size.cols, 0),
            size,
            desired_size: size,
            snapshot_installed: false,
            ready: false,
            error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneRects {
    pub pane_index: usize,
    pub metadata: Rect,
    pub terminal: Rect,
}

pub fn pane_rects(area: Rect, pane_count: usize, focused: usize) -> Vec<PaneRects> {
    let pane_count = pane_count.min(2);
    if pane_count == 0 {
        return Vec::new();
    }
    let sidebar = area.width.min(40).min(area.width / 2);
    let right = Rect::new(
        area.x.saturating_add(sidebar),
        area.y,
        area.width.saturating_sub(sidebar),
        area.height,
    );
    let terminal_y = right.y.saturating_add(3);
    let terminal_height = right.height.saturating_sub(4);
    if terminal_height == 0 || right.width == 0 {
        return Vec::new();
    }
    if pane_count == 1 || right.width < 41 {
        let index = focused.min(pane_count - 1);
        return vec![PaneRects {
            pane_index: index,
            metadata: Rect::new(right.x, right.y.saturating_add(1), right.width, 2),
            terminal: Rect::new(right.x, terminal_y, right.width, terminal_height),
        }];
    }
    let available = right.width - 1;
    let left_width = available / 2;
    let right_width = available - left_width;
    vec![
        PaneRects {
            pane_index: 0,
            metadata: Rect::new(right.x, right.y.saturating_add(1), left_width, 2),
            terminal: Rect::new(right.x, terminal_y, left_width, terminal_height),
        },
        PaneRects {
            pane_index: 1,
            metadata: Rect::new(
                right.x.saturating_add(left_width).saturating_add(1),
                right.y.saturating_add(1),
                right_width,
                2,
            ),
            terminal: Rect::new(
                right.x.saturating_add(left_width).saturating_add(1),
                terminal_y,
                right_width,
                terminal_height,
            ),
        },
    ]
}

pub(super) struct HeldMouse {
    pub(super) session: SessionId,
    pub(super) event: MouseEvent,
    pub(super) mode: vt100::MouseProtocolMode,
    pub(super) encoding: vt100::MouseProtocolEncoding,
}

#[derive(Default)]
pub(super) struct MouseForwarding {
    pub(super) held: [Option<HeldMouse>; 3],
    pub(super) last_motion: Option<MouseEvent>,
    pub(super) pending_cleanup: Option<ClientMessage>,
}

pub struct Dashboard {
    pub tasks: Option<TasksView>,
    pub hierarchy: HierarchySnapshot,
    pub mode: InputMode,
    pub panes: Vec<PaneState>,
    pub focused_pane: usize,
    pub view_revision: u64,
    pub outer_area: Rect,
    pub collapsed_projects: HashSet<String>,
    pub collapsed_workspaces: HashSet<(String, String)>,
    pub error: Option<String>,
    pub copy: Option<CopySelection>,
    pub copy_notice: Option<String>,
    pub history: Option<HistoryView>,
    pub history_begin_request: Option<PendingHistoryBegin>,
    pub(super) mouse: MouseForwarding,
    pub(super) mouse_focused: bool,
    tree_offset: usize,
    next_request_id: u64,
    palette: Option<palette::Palette>,
    history_page_error: bool,
    history_end_after_selection: Option<ClientMessage>,
    ignored_responses: HashSet<u64>,
    pub(super) settings: settings::DashboardSettings,
    pub(super) last_view_request_id: Option<u64>,
    pub(super) pending_view: Option<PendingView>,
    pub(super) requested_view: Option<RequestedView>,
    pub(super) force_view_refresh: bool,
    pub(super) view_request_ids: HashSet<u64>,
    pub(super) pending_snapshot_sessions: HashSet<SessionId>,
}

#[derive(Clone)]
pub(super) struct RequestedView {
    pub revision: u64,
    pub targets: Vec<(SessionId, TerminalSize)>,
    pub focused: Option<SessionId>,
}

pub(super) struct PendingView {
    pub request_id: u64,
    pub view: RequestedView,
    pub parser_discarded: bool,
}

thread_local! {
    pub(super) static PANIC_TERMINAL_RESTORED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

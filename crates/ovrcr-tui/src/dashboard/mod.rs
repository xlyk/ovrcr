mod agents;
mod copy;
mod desktop;
mod event_loop;
mod git_hints;
mod hints;
mod input;
mod palette;
pub(crate) mod picker;
mod render;
mod settings;
mod state;
mod terminal_guard;
#[cfg(test)]
mod tests;
pub(crate) mod text_cursor;
mod whichkey;

pub use agents::detect_agents;
pub(crate) use copy::CopySelection;
pub use copy::write_clipboard;
pub use event_loop::{dashboard_message_channel, run_dashboard};
pub use input::{encode_key, encode_mouse};
pub use render::{actual_drawn_inner_rect, draw_dashboard, draw_dashboard_at, render_terminal};
pub use settings::DashboardSettings;
pub(crate) use state::{HistoryView, PendingHistoryBegin};

use crate::task_tui::TasksView;
use crossterm::event::MouseEvent;
use ovrcr_protocol::{ClientMessage, HierarchySnapshot, SessionId, TerminalSize};
use ovrcr_terminal::vt100;
use ratatui::layout::Rect;
use std::collections::HashSet;

pub const DASHBOARD_READER_QUEUE_CAPACITY: usize = 64;

pub(crate) fn history_view_size(pane: TerminalSize) -> TerminalSize {
    TerminalSize {
        rows: pane.rows.clamp(1, 64),
        cols: pane.cols.clamp(1, 256),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputMode {
    Browse,
    Terminal,
    History,
    Copy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TreeRow {
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

pub(crate) struct PaneState {
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
    pane_rects_with_preference(area, pane_count, focused, None, None)
}

pub(super) const DEFAULT_SIDEBAR_WIDTH: u16 = 40;
pub(super) const MIN_SIDEBAR_WIDTH: u16 = 20;

/// Sidebar width for `area`: the dragged preference or the default, never more
/// than half the window.
pub(super) fn sidebar_width_for(area: Rect, preferred: Option<u16>) -> u16 {
    area.width
        .min(preferred.unwrap_or(DEFAULT_SIDEBAR_WIDTH))
        .min(area.width / 2)
}

#[derive(Clone, Copy)]
pub(super) struct SplitPreference {
    left: u16,
    available: u16,
}

const MIN_SPLIT_PANE_WIDTH: u16 = 20;

fn pane_rects_with_preference(
    area: Rect,
    pane_count: usize,
    focused: usize,
    preference: Option<SplitPreference>,
    sidebar_width: Option<u16>,
) -> Vec<PaneRects> {
    let pane_count = pane_count.min(2);
    if pane_count == 0 {
        return Vec::new();
    }
    let sidebar = sidebar_width_for(area, sidebar_width);
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
    let left_width = preference
        .filter(|preference| preference.available > 0)
        .map(|preference| {
            let scaled = (u32::from(available) * u32::from(preference.left)
                + u32::from(preference.available) / 2)
                / u32::from(preference.available);
            u16::try_from(scaled).unwrap_or(available)
        })
        .unwrap_or(available / 2)
        .clamp(MIN_SPLIT_PANE_WIDTH, available - MIN_SPLIT_PANE_WIDTH);
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
    pub(super) split_dragging: bool,
    pub(super) sidebar_dragging: bool,
}

pub struct Dashboard {
    desktop: desktop::DesktopNotifications,
    tasks: Option<TasksView>,
    hierarchy: HierarchySnapshot,
    // Review actions name only the observation committed by the last successful draw.
    presented_unread: Option<(SessionId, ovrcr_protocol::ReadyObservation)>,
    mode: InputMode,
    panes: Vec<PaneState>,
    focused_pane: usize,
    view_revision: u64,
    outer_area: Rect,
    collapsed_projects: HashSet<String>,
    collapsed_workspaces: HashSet<(String, String)>,
    error: Option<String>,
    /// Whether the banner in `error` was written by a refused view, which is the only writer a
    /// successful view completion is allowed to clear. `set_error` releases it.
    error_owned_by_view: bool,
    copy: Option<CopySelection>,
    copy_notice: Option<String>,
    history: Option<HistoryView>,
    history_begin_request: Option<PendingHistoryBegin>,
    mouse: MouseForwarding,
    split_preference: Option<SplitPreference>,
    /// Sidebar width chosen by dragging its border; `None` means the default.
    sidebar_width: Option<u16>,
    mouse_focused: bool,
    /// Pane index whose history a wheel tick asked for while that pane was still loading.
    deferred_history_at_tail: Option<usize>,
    tree_offset: usize,
    next_request_id: u64,
    palette: Option<palette::Palette>,
    whichkey: Option<whichkey::WhichKey>,
    selected_container: Option<TreeRow>,
    configuration_paths: Option<(std::path::PathBuf, std::path::PathBuf)>,
    history_page_error: bool,
    history_end_after_selection: Option<ClientMessage>,
    ignored_responses: HashSet<u64>,
    settings: settings::DashboardSettings,
    config_dir: std::path::PathBuf,
    last_view_request_id: Option<u64>,
    pending_view: Option<PendingView>,
    requested_view: Option<RequestedView>,
    /// The view the server refused, with the instant of the refusal. The same view waits out
    /// `VIEW_RETRY_BACKOFF` before it is re-sent so a repeating failure cannot spin the loop.
    failed_view: Option<(RequestedView, std::time::Instant)>,
    force_view_refresh: bool,
    /// Ids of `SetView` requests still waiting for their final `Ok` or `Error`.
    view_request_ids: HashSet<u64>,
    /// Ids of requests that own the error banner, so their plain `Ok` may clear it. Requests the
    /// dashboard sends on its own behalf, such as a synthetic mouse release, are absent.
    error_owning_requests: HashSet<u64>,
    /// Set when the user changes selection, splits, focuses, or closes a pane, and consumed by the
    /// next `SetView` those changes produce, which then owns the error banner: a banner written
    /// before the change describes the state the user just left. A server-driven refresh mints its
    /// request with this clear, so its success leaves a fresh banner alone.
    pending_user_view_change: bool,
    pending_snapshot_sessions: HashSet<SessionId>,
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

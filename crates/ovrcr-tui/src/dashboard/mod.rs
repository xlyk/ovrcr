mod agents;
mod copy;
mod desktop;
mod event_loop;
mod git_hints;
mod hints;
mod input;
mod keymap;
mod outbox;
mod palette;
pub(crate) mod picker;
mod quota;
mod ready;
mod render;
mod settings;
mod state;
mod status;
mod terminal_guard;
#[cfg(test)]
mod tests;
pub(crate) mod text_cursor;
mod unread;
mod view_handshake;
mod whichkey;

pub use agents::detect_agents;
pub(crate) use copy::CopySelection;
pub use copy::write_clipboard;
pub use event_loop::{dashboard_message_channel, run_dashboard};
pub use input::{encode_key, encode_mouse};
pub use render::{actual_drawn_inner_rect, draw_dashboard, draw_dashboard_at, render_terminal};
pub use settings::{LaunchChoice, Settings};
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
    Workspace { project: String, id: String },
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
    pub run: Option<ovrcr_protocol::SessionRunId>,
    pub parser: vt100::Parser,
    pub size: TerminalSize,
    pub desired_size: TerminalSize,
    pub error: Option<String>,
}

impl PaneState {
    pub fn new(size: TerminalSize) -> Self {
        Self {
            session: None,
            run: None,
            parser: vt100::Parser::new(size.rows, size.cols, 0),
            size,
            desired_size: size,
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
    pub(super) split_dragging: bool,
    pub(super) sidebar_dragging: bool,
    /// Row under the pointer. Drawing reads it; motion that stays on the same row does not redraw.
    pub(super) hovered: Option<HoveredRow>,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum HoveredRow {
    Session(SessionId),
    Workspace { project: String, id: String },
}

pub struct Dashboard {
    desktop: desktop::DesktopNotifications,
    tasks: Option<TasksView>,
    hierarchy: HierarchySnapshot,
    quotas: Option<crate::protocol::QuotaSnapshot>,
    /// The read-only details popup on screen, and its scroll offset.
    details: Option<(quota::Details, u16)>,
    /// The Server's latest settings reading; `settings` is its typed half.
    settings_report: Option<Box<crate::protocol::SettingsReport>>,
    // Review names Presented, not a newer Unread that arrived before the next draw.
    unread: unread::Unread,
    mode: InputMode,
    panes: Vec<PaneState>,
    focused_pane: usize,
    /// The whole view state machine: the revision, the one in-flight `SetView`, the
    /// acknowledged view, and the marks pane readiness is derived from.
    handshake: view_handshake::ViewHandshake,
    agent_typing: Option<state::AgentTyping>,
    /// One display request per retained run; the server persists launch failures across reconnects.
    recovery_requests:
        std::collections::HashMap<(SessionId, crate::protocol::SessionRunId), Option<u64>>,
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
    /// `b` hides the sidebar for this attachment; `sidebar_width` survives so
    /// showing it again restores the dragged width.
    sidebar_hidden: bool,
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
    /// Requests waiting for the event loop's next drain.
    outbox: outbox::Outbox,
    ignored_responses: HashSet<u64>,
    settings: settings::Settings,
    config_dir: std::path::PathBuf,
    /// Ids of requests that own the error banner, so their plain `Ok` may clear it. Requests the
    /// dashboard sends on its own behalf, such as a synthetic mouse release, are absent.
    error_owning_requests: HashSet<u64>,
}

thread_local! {
    pub(super) static PANIC_TERMINAL_RESTORED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

mod agents;
mod assignment_control;
mod attention;
mod cards;
mod copy;
mod desktop;
mod event_loop;
mod events;
mod git_hints;
mod hints;
mod input;
#[cfg(target_os = "macos")]
mod iterm;
mod keymap;
mod lifecycle_ui;
mod navigation;
mod operator;
mod outbox;
mod palette;
pub(crate) mod picker;
mod quota;
mod ready;
mod render;
mod row_menu;
mod settings;
mod settings_editor;
mod state;
mod status;
mod terminal_guard;
#[cfg(test)]
mod tests;
pub(crate) mod text_cursor;
mod transcript;
mod unread;
mod view_handshake;
mod whichkey;

pub use agents::{AgentEntry, AgentSource, apply_overrides, detect_agents, takes_task, with_task};
pub(crate) use copy::CopySelection;
pub use copy::write_clipboard;
pub use event_loop::{dashboard_message_channel, run_dashboard};
pub use input::{encode_key, encode_mouse};
pub use render::{actual_drawn_inner_rect, draw_dashboard, draw_dashboard_at, render_terminal};
pub use settings::{LaunchChoice, Settings};
pub(crate) use state::{HistoryView, PendingHistoryBegin};

use crate::task_tui::TasksView;
use crossterm::event::MouseEvent;
use ovrcr_protocol::{HierarchySnapshot, SessionId, TerminalSize};
use ovrcr_terminal::vt100;
use ratatui::layout::Rect;
use std::collections::{HashSet, VecDeque};

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
    Project {
        name: String,
    },
    Workspace {
        project: String,
        id: String,
    },
    Session {
        id: SessionId,
    },
    /// Client fiction for a workspace create before the Server publishes it.
    ProvisionalWorkspace {
        project: String,
        id: String,
    },
    /// Client fiction for a session create; never carries a SessionId.
    ProvisionalSession {
        project: String,
        workspace: String,
        token: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DashboardAction {
    None,
    Redraw,
    Detach,
    EnterBrowse,
    PtyBytes(Vec<u8>),
    CopyText(String),
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
pub(super) struct SidebarRects {
    pub attention: Rect,
    pub operator: Rect,
    pub tree: Rect,
    pub quota: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneRects {
    pub pane_index: usize,
    pub metadata: Rect,
    pub terminal: Rect,
}

const MAX_PANE_COUNT: usize = 4;
const MIN_PANE_WIDTH: u16 = 20;
const MIN_PANE_HEIGHT: u16 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SplitDirection {
    Right,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaneDirection {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SplitRatio {
    pub(super) first: u16,
    pub(super) available: u16,
}

impl Default for SplitRatio {
    fn default() -> Self {
        Self {
            first: 1,
            available: 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PaneLayout {
    Pane(usize),
    Split {
        direction: SplitDirection,
        ratio: SplitRatio,
        first: Box<PaneLayout>,
        second: Box<PaneLayout>,
    },
}

impl PaneLayout {
    pub(super) fn insert_after(
        &mut self,
        index: usize,
        direction: SplitDirection,
        new_index: usize,
    ) -> bool {
        match self {
            Self::Pane(existing) if *existing == index => {
                *self = Self::Split {
                    direction,
                    ratio: SplitRatio::default(),
                    first: Box::new(Self::Pane(index)),
                    second: Box::new(Self::Pane(new_index)),
                };
                true
            }
            Self::Pane(existing) => {
                if *existing >= new_index {
                    *existing += 1;
                }
                false
            }
            Self::Split { first, second, .. } => {
                if first.insert_after(index, direction, new_index) {
                    second.shift_indices(new_index, 1);
                    true
                } else {
                    second.insert_after(index, direction, new_index)
                }
            }
        }
    }

    pub(super) fn remove(self, index: usize) -> Option<Self> {
        match self {
            Self::Pane(existing) if existing == index => None,
            Self::Pane(existing) => Some(Self::Pane(if existing > index {
                existing - 1
            } else {
                existing
            })),
            Self::Split {
                direction,
                ratio,
                first,
                second,
            } => match (first.remove(index), second.remove(index)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    direction,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(sibling), None) | (None, Some(sibling)) => Some(sibling),
                (None, None) => None,
            },
        }
    }

    fn shift_indices(&mut self, start: usize, delta: usize) {
        match self {
            Self::Pane(index) if *index >= start => *index += delta,
            Self::Pane(_) => {}
            Self::Split { first, second, .. } => {
                first.shift_indices(start, delta);
                second.shift_indices(start, delta);
            }
        }
    }

    pub(super) fn sibling_first(&self, index: usize) -> Option<usize> {
        match self {
            Self::Pane(_) => None,
            Self::Split { first, second, .. } => {
                if first.contains(index) {
                    first.sibling_first(index).or_else(|| Some(second.first()))
                } else if second.contains(index) {
                    second.sibling_first(index).or_else(|| Some(first.first()))
                } else {
                    None
                }
            }
        }
    }

    fn contains(&self, index: usize) -> bool {
        match self {
            Self::Pane(existing) => *existing == index,
            Self::Split { first, second, .. } => first.contains(index) || second.contains(index),
        }
    }

    fn first(&self) -> usize {
        match self {
            Self::Pane(index) => *index,
            Self::Split { first, .. } => first.first(),
        }
    }

    pub(super) fn swap_indices(&mut self, left: usize, right: usize) -> bool {
        if left == right || !self.contains(left) || !self.contains(right) {
            return false;
        }
        fn swap(layout: &mut PaneLayout, left: usize, right: usize) {
            match layout {
                PaneLayout::Pane(index) if *index == left => *index = right,
                PaneLayout::Pane(index) if *index == right => *index = left,
                PaneLayout::Pane(_) => {}
                PaneLayout::Split { first, second, .. } => {
                    swap(first, left, right);
                    swap(second, left, right);
                }
            }
        }
        swap(self, left, right);
        true
    }

    pub(super) fn set_ratio(&mut self, path: &[bool], ratio: SplitRatio) -> bool {
        match self {
            Self::Pane(_) => false,
            Self::Split {
                ratio: current,
                first,
                second,
                ..
            } if path.is_empty() => {
                *current = ratio;
                true
            }
            Self::Split { first, second, .. } => {
                if path[0] {
                    second.set_ratio(&path[1..], ratio)
                } else {
                    first.set_ratio(&path[1..], ratio)
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SplitBoundary {
    pub path: Vec<bool>,
    pub direction: SplitDirection,
    pub area: Rect,
    pub divider: Rect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PaneLayoutRects {
    pub panes: Vec<PaneRects>,
    pub boundaries: Vec<SplitBoundary>,
    pub hidden: bool,
}

pub fn pane_rects(area: Rect, pane_count: usize, focused: usize) -> Vec<PaneRects> {
    let count = pane_count.min(MAX_PANE_COUNT);
    let layout = default_pane_layout(count);
    pane_layout_rects(area, &layout, count, focused, None).panes
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

fn default_pane_layout(count: usize) -> PaneLayout {
    fn build(start: usize, count: usize) -> PaneLayout {
        if count == 1 {
            PaneLayout::Pane(start)
        } else {
            PaneLayout::Split {
                direction: SplitDirection::Right,
                ratio: SplitRatio::default(),
                first: Box::new(PaneLayout::Pane(start)),
                second: Box::new(build(start + 1, count - 1)),
            }
        }
    }
    if count == 0 {
        PaneLayout::Pane(0)
    } else {
        build(0, count)
    }
}

pub(super) fn pane_layout_rects(
    area: Rect,
    layout: &PaneLayout,
    pane_count: usize,
    focused: usize,
    sidebar_width: Option<u16>,
) -> PaneLayoutRects {
    let pane_count = pane_count.min(MAX_PANE_COUNT);
    if pane_count == 0 {
        return PaneLayoutRects {
            panes: Vec::new(),
            boundaries: Vec::new(),
            hidden: false,
        };
    }
    let sidebar = sidebar_width_for(area, sidebar_width);
    let right = Rect::new(
        area.x.saturating_add(sidebar),
        area.y,
        area.width.saturating_sub(sidebar),
        area.height,
    );
    let content = Rect::new(
        right.x,
        right.y.saturating_add(1),
        right.width,
        right.height.saturating_sub(2),
    );
    if content.width == 0 || content.height < 3 {
        return PaneLayoutRects {
            panes: Vec::new(),
            boundaries: Vec::new(),
            hidden: false,
        };
    }
    let mut panes = Vec::new();
    let mut boundaries = Vec::new();
    if append_layout_rects(
        layout,
        content,
        &mut Vec::new(),
        &mut panes,
        &mut boundaries,
    ) {
        PaneLayoutRects {
            panes,
            boundaries,
            hidden: false,
        }
    } else {
        PaneLayoutRects {
            panes: vec![pane_rect(focused.min(pane_count - 1), content)],
            boundaries: Vec::new(),
            hidden: pane_count > 1,
        }
    }
}

fn pane_rect(index: usize, area: Rect) -> PaneRects {
    let metadata_height = area.height.min(2);
    PaneRects {
        pane_index: index,
        metadata: Rect::new(area.x, area.y, area.width, metadata_height),
        terminal: Rect::new(
            area.x,
            area.y.saturating_add(metadata_height),
            area.width,
            area.height.saturating_sub(metadata_height),
        ),
    }
}

fn append_layout_rects(
    layout: &PaneLayout,
    area: Rect,
    path: &mut Vec<bool>,
    panes: &mut Vec<PaneRects>,
    boundaries: &mut Vec<SplitBoundary>,
) -> bool {
    match layout {
        PaneLayout::Pane(index) => {
            panes.push(pane_rect(*index, area));
            true
        }
        PaneLayout::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let (dimension, minimum) = match direction {
                SplitDirection::Right => (area.width, MIN_PANE_WIDTH),
                SplitDirection::Down => (area.height, MIN_PANE_HEIGHT),
            };
            let available = dimension.saturating_sub(1);
            if available < minimum.saturating_mul(2)
                || match direction {
                    SplitDirection::Right => area.height < MIN_PANE_HEIGHT,
                    SplitDirection::Down => area.width < MIN_PANE_WIDTH,
                }
            {
                return false;
            }
            let first_size = ((u32::from(available) * u32::from(ratio.first))
                / u32::from(ratio.available.max(1)))
            .try_into()
            .unwrap_or(available)
            .clamp(minimum, available - minimum);
            let second_size = available - first_size;
            let (first_area, divider, second_area) = match direction {
                SplitDirection::Right => (
                    Rect::new(area.x, area.y, first_size, area.height),
                    Rect::new(area.x.saturating_add(first_size), area.y, 1, area.height),
                    Rect::new(
                        area.x.saturating_add(first_size).saturating_add(1),
                        area.y,
                        second_size,
                        area.height,
                    ),
                ),
                SplitDirection::Down => (
                    Rect::new(area.x, area.y, area.width, first_size),
                    Rect::new(area.x, area.y.saturating_add(first_size), area.width, 1),
                    Rect::new(
                        area.x,
                        area.y.saturating_add(first_size).saturating_add(1),
                        area.width,
                        second_size,
                    ),
                ),
            };
            boundaries.push(SplitBoundary {
                path: path.clone(),
                direction: *direction,
                area,
                divider,
            });
            path.push(false);
            let fits = append_layout_rects(first, first_area, path, panes, boundaries);
            path.pop();
            if !fits {
                return false;
            }
            path.push(true);
            let fits = append_layout_rects(second, second_area, path, panes, boundaries);
            path.pop();
            fits
        }
    }
}

pub(super) struct HeldMouse {
    pub(super) session: SessionId,
    pub(super) run: ovrcr_protocol::SessionRunId,
    pub(super) control_epoch: Option<u64>,
    pub(super) view_revision: u64,
    pub(super) event: MouseEvent,
    pub(super) mode: vt100::MouseProtocolMode,
    pub(super) encoding: vt100::MouseProtocolEncoding,
}

#[derive(Default)]
pub(super) struct MouseForwarding {
    pub(super) held: [Option<HeldMouse>; 3],
    pub(super) last_motion: Option<MouseEvent>,
    pub(super) split_dragging: Option<Vec<bool>>,
    pub(super) sidebar_dragging: bool,
    /// Row under the pointer. Drawing reads it; motion that stays on the same row does not redraw.
    pub(super) hovered: Option<HoveredRow>,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum HoveredRow {
    Session(SessionId),
    Project(String),
    Workspace { project: String, id: String },
}

pub struct Dashboard {
    operator: operator::Hub,
    desktop: desktop::DesktopNotifications,
    tasks: Option<TasksView>,
    hierarchy: HierarchySnapshot,
    quotas: Option<crate::protocol::QuotaSnapshot>,
    /// The details popup on screen, and its scroll offset.
    details: Option<(quota::Details, u16)>,
    transcript: transcript::View,
    /// The Server's latest settings reading; `settings` is its typed half.
    settings_report: Option<Box<crate::protocol::SettingsReport>>,
    settings_editor: settings_editor::Editor,
    events: events::View,
    // Review names Presented, not a newer Unread that arrived before the next draw.
    unread: unread::Unread,
    mode: InputMode,
    panes: Vec<PaneState>,
    focused_pane: usize,
    pane_layout: PaneLayout,
    pub(super) zoomed: bool,
    /// The whole view state machine: the revision, the one in-flight `SetView`, the
    /// acknowledged view, and the marks pane readiness is derived from.
    handshake: view_handshake::ViewHandshake,
    navigation: navigation::Navigation,
    agent_typing: Option<state::AgentTyping>,
    /// Session targets frozen while the Ctrl-g label picker is open.
    sidebar_jump: Option<state::SidebarJump>,
    /// One display request per retained run; the server persists launch failures across reconnects.
    recovery_requests:
        std::collections::HashMap<(SessionId, crate::protocol::SessionRunId), Option<u64>>,
    outer_area: Rect,
    collapsed_projects: HashSet<String>,
    collapsed_workspaces: HashSet<(String, String)>,
    error: Option<String>,
    /// Notices waiting out the error banner. The footer has one line and the banner owns it, so a
    /// notice that is already showing when the banner appears, or one posted while it is up, waits
    /// here instead of being overwritten. Dismissing the banner shows the next one.
    notice_queue: VecDeque<String>,
    /// Whether the banner in `error` was written by a refused view, which is the only writer a
    /// successful view completion is allowed to clear. `set_error` releases it.
    error_owned_by_view: bool,
    /// The banner in `error` is an expected limit (`Pane is loading; retry history`), not a
    /// failure: it reads muted, without the `ERROR:` marker.
    error_is_limit: bool,
    /// When the banner's clock started: when a limit went up, or the first key press after a
    /// failure did. It clears itself `state::BANNER_LIFETIME` later.
    error_since: Option<std::time::Instant>,
    copy: Option<CopySelection>,
    copy_notice: Option<String>,
    history: Option<HistoryView>,
    history_begin_request: Option<PendingHistoryBegin>,
    mouse: MouseForwarding,
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
    /// One native model lookup may outlive its palette or owning socket. Keep
    /// its identity here so a reopened form can wait and coalesce one latest
    /// target after the result arrives.
    model_query: Option<(u64, String)>,
    #[cfg(test)]
    model_agent_entries: Option<Vec<agents::AgentEntry>>,
    whichkey: Option<whichkey::WhichKey>,
    sidebar_create_menu: Option<row_menu::SidebarCreateMenu>,
    selected_container: Option<TreeRow>,
    configuration_paths: Option<(std::path::PathBuf, std::path::PathBuf)>,
    /// Set while startup left agent status off: the palette offers Connect agent status,
    /// and the startup note it retires once connecting succeeds.
    connect_status: Option<(crate::ConnectStatusFn, String)>,
    history_page_error: bool,
    /// Requests waiting for the event loop's next drain.
    outbox: outbox::Outbox,
    /// Shutdown questions still to ask, oldest first: project, workspace id, branch.
    wip_prompts: VecDeque<(String, String, String)>,
    ignored_responses: HashSet<u64>,
    settings: settings::Settings,
    /// Cached palette for the current settings theme (hot-switched on SettingsChanged).
    theme: crate::theme::ThemeTokens,
    config_dir: std::path::PathBuf,
    /// Ids of requests that own the error banner, so their plain `Ok` may clear it. Requests the
    /// dashboard sends on its own behalf, such as a synthetic mouse release, are absent.
    error_owning_requests: HashSet<u64>,
    /// Ids of terminal input requests in flight: their refusal (input not admitted) is a limit.
    input_requests: HashSet<u64>,
    /// One Start is between intent allocation and launch admission. Prompt is bounded by the
    /// protocol's 64 KiB limit and is dropped as soon as the launch request is sent.
    pending_initial_prompt_allocation: Option<InitialPromptAllocation>,
    /// Latest Server-owned launch intent observed for this Dashboard.
    initial_prompt_intent: Option<crate::protocol::InitialPromptIntentSnapshot>,
    initial_prompt_intent_id: Option<crate::protocol::InitialPromptIntentId>,
    initial_prompt_focus_pending: Option<crate::protocol::InitialPromptIntentId>,
    deferred_initial_prompt_recovery: Option<crate::protocol::InitialPromptIntentId>,
    initial_prompt_reconcile_request: Option<u64>,
    /// Lifecycle job accepted but not yet shown as a Provisional row (awaiting Response::Ok).
    lifecycle_pending: Option<lifecycle_ui::Provisional>,
    /// In-flight or failed Provisional row.
    provisional: Option<lifecycle_ui::Provisional>,
    /// Bottom status strip for Lifecycle progress / soft refuse; Esc dismisses it,
    /// with a failed Provisional row when there is one.
    lifecycle_strip: Option<String>,
    /// Remembered launch choice held across Provisional wait until CreatedSession completes.
    lifecycle_preference: Option<(String, settings::LaunchChoice)>,
}

struct InitialPromptAllocation {
    request_id: u64,
    launch: crate::protocol::CreateAgentSessionRequest,
    prompt: String,
    form: Option<Box<palette::Palette>>,
    then: Vec<crate::protocol::Request>,
}

thread_local! {
    pub(super) static PANIC_TERMINAL_RESTORED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

mod copy;
mod event_loop;
mod input;
mod render;
mod state;
mod terminal_guard;
#[cfg(test)]
mod tests;

pub use copy::{CopyMotion, CopyPoint, CopySelection, write_clipboard};
pub use event_loop::{dashboard_message_channel, run_dashboard};
pub use input::{encode_key, event_to_request};
pub use render::{
    actual_drawn_inner_rect, draw_dashboard, draw_dashboard_at, render_copy, render_history,
    render_terminal,
};
pub use state::{HistoryView, PendingHistoryBegin, PendingHistoryPage};
pub use terminal_guard::TerminalGuard;

use ovrcr_protocol::{ClientMessage, HierarchySnapshot, SessionId, TerminalSize};
use ovrcr_terminal::vt100;
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

pub struct Dashboard {
    pub hierarchy: HierarchySnapshot,
    pub selected: Option<SessionId>,
    pub mode: InputMode,
    pub parser: vt100::Parser,
    pub pane_size: TerminalSize,
    pub collapsed_projects: HashSet<String>,
    pub collapsed_workspaces: HashSet<(String, String)>,
    pub error: Option<String>,
    pub copy: Option<CopySelection>,
    pub copy_notice: Option<String>,
    pub history: Option<HistoryView>,
    pub history_begin_request: Option<PendingHistoryBegin>,
    tree_offset: usize,
    next_request_id: u64,
    history_page_error: bool,
    history_end_after_selection: Option<ClientMessage>,
    screen_session: Option<SessionId>,
    pending_screen: Option<(SessionId, u64)>,
}

thread_local! {
    pub(super) static PANIC_TERMINAL_RESTORED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

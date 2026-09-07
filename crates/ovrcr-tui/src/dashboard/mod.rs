mod event_loop;
mod input;
mod palette;
mod render;
mod state;
mod terminal_guard;
#[cfg(test)]
mod tests;

pub use event_loop::{dashboard_message_channel, run_dashboard};
pub use input::{encode_key, event_to_request};
pub use render::{actual_drawn_inner_rect, draw_dashboard, draw_dashboard_at, render_terminal};
pub use terminal_guard::TerminalGuard;

use crate::task_tui::TasksView;
use ovrcr_protocol::{ClientMessage, HierarchySnapshot, SessionId, TerminalSize};
use ovrcr_terminal::vt100;
use std::collections::HashSet;

pub const DASHBOARD_READER_QUEUE_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    Browse,
    Terminal,
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
    Request(ClientMessage),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyEncoding {
    Browse,
    Bytes(Vec<u8>),
    Ignore,
}

pub struct Dashboard {
    pub tasks: Option<TasksView>,
    pub hierarchy: HierarchySnapshot,
    pub selected: Option<SessionId>,
    pub mode: InputMode,
    pub parser: vt100::Parser,
    pub pane_size: TerminalSize,
    pub collapsed_projects: HashSet<String>,
    pub collapsed_workspaces: HashSet<(String, String)>,
    pub error: Option<String>,
    tree_offset: usize,
    next_request_id: u64,
    palette: Option<palette::Palette>,
}

thread_local! {
    pub(super) static PANIC_TERMINAL_RESTORED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

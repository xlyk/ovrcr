mod dashboard;
pub mod task_tui;
pub mod theme;

pub(crate) use ovrcr_protocol as protocol;
pub(crate) use ovrcr_protocol as session;

pub use ovrcr_protocol::task::{TaskRequest, TaskResponse};

pub type TaskRequestFn = fn(TaskRequest) -> anyhow::Result<TaskResponse>;

/// What the palette's Connect agent status entry shows next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectStatus {
    /// The startup screen's text: what connecting does and the files it changes.
    /// Nothing is written until the user confirms.
    Confirm(String),
    /// The result line: `Ok` says what changed, `Err` what is still off and why.
    Done(Result<String, String>),
}

/// OVRCR's reporting hooks in the installed agents' native settings, for the palette's
/// Connect agent status entry; the binary owns those writers. `apply == false` only
/// reads (a `Confirm` preview, or `Done` when there is nothing to confirm);
/// `apply == true` writes and always returns `Done`.
pub type ConnectStatusFn = fn(apply: bool) -> ConnectStatus;

pub use task_tui::{event_text, parse_duration};

pub use dashboard::{
    AgentEntry, AgentSource, CpuCounters, DASHBOARD_READER_QUEUE_CAPACITY, Dashboard,
    DashboardAction, KeyEncoding, LaunchChoice, MachineReadings, MachineResourceSource,
    MemoryCounters, PaneRects, Settings, actual_drawn_inner_rect, apply_overrides,
    dashboard_message_channel, detect_agents, draw_dashboard, draw_dashboard_at, encode_key,
    encode_mouse, pane_rects, render_terminal, run_dashboard, takes_task, with_task,
    write_clipboard,
};
pub use ovrcr_terminal::encode_paste;

mod dashboard;
pub mod task_tui;
pub mod theme;

pub(crate) use ovrcr_protocol as protocol;
pub(crate) use ovrcr_protocol as session;

pub use ovrcr_protocol::task::{TaskRequest, TaskResponse};

pub type TaskRequestFn = fn(TaskRequest) -> anyhow::Result<TaskResponse>;

/// Installs OVRCR's reporting hooks in the installed agents' native settings, for the
/// palette's Connect agent status entry; the binary owns those writers. `Ok` says
/// what changed, `Err` what is still off and why.
pub type ConnectStatusFn = fn() -> Result<String, String>;

pub use task_tui::{event_text, parse_duration};

pub use dashboard::{
    DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, DashboardAction, KeyEncoding, LaunchChoice,
    PaneRects, Settings, actual_drawn_inner_rect, dashboard_message_channel, detect_agents,
    draw_dashboard, draw_dashboard_at, encode_key, encode_mouse, pane_rects, render_terminal,
    run_dashboard, write_clipboard,
};
pub use ovrcr_terminal::encode_paste;

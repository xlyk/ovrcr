mod dashboard;
pub mod task_tui;

pub(crate) use ovrcr_protocol as protocol;
pub(crate) use ovrcr_protocol as session;

pub use ovrcr_protocol::task::{TaskRequest, TaskResponse};

pub type TaskRequestFn = fn(TaskRequest) -> anyhow::Result<TaskResponse>;

pub use task_tui::{event_text, parse_duration};

pub use dashboard::{
    DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, DashboardAction, KeyEncoding, LaunchChoice,
    PaneRects, Settings, actual_drawn_inner_rect, dashboard_message_channel, detect_agents,
    draw_dashboard, draw_dashboard_at, encode_key, encode_mouse, pane_rects, render_terminal,
    run_dashboard, write_clipboard,
};
pub use ovrcr_terminal::encode_paste;

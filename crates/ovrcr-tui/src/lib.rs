mod dashboard;
pub mod task_tui;

pub(crate) use ovrcr_protocol as protocol;
pub(crate) use ovrcr_protocol as session;
pub(crate) use ovrcr_protocol::context;

pub use ovrcr_protocol::task::{TaskRequest, TaskResponse};

pub type TaskRequestFn = fn(TaskRequest) -> anyhow::Result<TaskResponse>;

pub use task_tui::{event_text, parse_duration};

pub use dashboard::{
    DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, DashboardAction, InputMode, KeyEncoding,
    TerminalGuard, TreeRow, actual_drawn_inner_rect, dashboard_message_channel, draw_dashboard,
    draw_dashboard_at, encode_key, event_to_request, render_terminal, run_dashboard,
};
pub use ovrcr_terminal::encode_paste;

mod dashboard;
pub mod task_tui;

pub(crate) use ovrcr_protocol as protocol;
pub(crate) use ovrcr_protocol as session;
pub(crate) use ovrcr_protocol::context;

pub use ovrcr_protocol::task::{TaskRequest, TaskResponse};

pub type TaskRequestFn = fn(TaskRequest) -> anyhow::Result<TaskResponse>;

pub use task_tui::{event_text, parse_duration};

pub use dashboard::{
    CopyMotion, CopyPoint, CopySelection, DASHBOARD_READER_QUEUE_CAPACITY, Dashboard,
    DashboardAction, DashboardSettings, HistoryCopyCompletion, HistoryCopyJob, HistoryCopyPoint,
    HistoryCopyRange, HistoryCursor, HistoryCursorTarget, HistoryPagePurpose, HistoryView,
    InputMode, KeyEncoding, PaneRects, PaneState, PendingHistoryBegin, PendingHistoryPage,
    TerminalGuard, TreeRow, actual_drawn_inner_rect, append_history_selection,
    dashboard_message_channel, detect_agents, draw_dashboard, draw_dashboard_at, encode_key,
    encode_mouse, event_to_request, history_view_size, pane_rects, render_copy, render_history,
    render_terminal, run_dashboard, write_clipboard,
};
pub use ovrcr_terminal::encode_paste;

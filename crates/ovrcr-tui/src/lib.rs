mod dashboard;

pub(crate) use ovrcr_protocol as protocol;
pub(crate) use ovrcr_protocol as session;
pub(crate) use ovrcr_protocol::context;

pub use dashboard::{
    DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, DashboardAction, HistoryView, InputMode,
    KeyEncoding, PendingHistoryBegin, PendingHistoryPage, TerminalGuard, TreeRow,
    actual_drawn_inner_rect, dashboard_message_channel, draw_dashboard, draw_dashboard_at,
    encode_key, event_to_request, history_view_size, render_history, render_terminal,
    run_dashboard,
};
pub use ovrcr_terminal::encode_paste;

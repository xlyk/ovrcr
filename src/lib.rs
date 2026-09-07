pub mod client;
pub use ovrcr_protocol::context;
#[cfg(feature = "gui")]
pub mod gui;
pub use ovrcr_protocol as protocol;
pub use ovrcr_runtime::{config, git, session};
pub mod report;
pub mod server {
    pub use crate::client::{connect_if_running, connect_or_start};
    pub use ovrcr_runtime::server::*;
}
pub mod service;
pub mod task_cli;
pub mod task_manager {
    pub use ovrcr_runtime::task_manager::*;
}
pub mod task_paging {
    pub use ovrcr_runtime::task_paging::*;
}
pub mod task_runner {
    pub use ovrcr_runtime::task_runner::*;
}
pub mod tasks {
    pub use ovrcr_runtime::tasks::*;
}
pub mod task_tui {
    pub use ovrcr_tui::task_tui::*;
}
pub mod tui {
    pub use ovrcr_tui::{
        DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, DashboardAction, HistoryView, InputMode,
        KeyEncoding, PendingHistoryBegin, PendingHistoryPage, TerminalGuard, TreeRow,
        actual_drawn_inner_rect, dashboard_message_channel, draw_dashboard, draw_dashboard_at,
        encode_key, encode_paste, event_to_request, history_view_size, render_history,
        render_terminal,
    };

    pub fn run_dashboard(stream: std::os::unix::net::UnixStream) -> anyhow::Result<()> {
        ovrcr_tui::run_dashboard(stream, crate::task_cli::request)
    }
}

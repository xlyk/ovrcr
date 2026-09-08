pub mod client;
pub use ovrcr_protocol::context;
#[cfg(feature = "gui")]
pub mod gui;
pub use ovrcr_protocol as protocol;
pub use ovrcr_runtime::{config, git, session};
pub mod report;
pub mod server {
    pub use crate::client::{
        connect_if_running, connect_or_start, connect_raw_if_running, handshake,
    };
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
        CopyMotion, CopyPoint, CopySelection, DASHBOARD_READER_QUEUE_CAPACITY, Dashboard,
        DashboardAction, DashboardSettings, HistoryCopyCompletion, HistoryCopyJob,
        HistoryCopyPoint, HistoryCopyRange, HistoryCursor, HistoryCursorTarget, HistoryPagePurpose,
        HistoryView, InputMode, KeyEncoding, PaneRects, PaneState, PendingHistoryBegin,
        PendingHistoryPage, TerminalGuard, TreeRow, actual_drawn_inner_rect,
        append_history_selection, dashboard_message_channel, detect_agents, draw_dashboard,
        draw_dashboard_at, encode_key, encode_mouse, encode_paste, event_to_request,
        history_view_size, pane_rects, render_copy, render_history, render_terminal,
        write_clipboard,
    };

    pub fn run_dashboard(
        stream: std::os::unix::net::UnixStream,
        settings_path: std::path::PathBuf,
    ) -> anyhow::Result<()> {
        ovrcr_tui::run_dashboard(stream, crate::task_cli::request, settings_path)
    }
}

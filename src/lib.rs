pub use ovrcr_runtime::agent_runner;
pub mod client;
pub use ovrcr_protocol::context;
pub use ovrcr_protocol::freshness;
#[cfg(feature = "gui")]
pub mod gui;
pub use ovrcr_protocol as protocol;
pub use ovrcr_runtime::{config, git, retained, session};
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
        DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, DashboardAction, DashboardSettings,
        KeyEncoding, PaneRects, actual_drawn_inner_rect, dashboard_message_channel, detect_agents,
        draw_dashboard, draw_dashboard_at, encode_key, encode_mouse, encode_paste, pane_rects,
        render_terminal, write_clipboard,
    };

    pub fn run_dashboard(
        stream: std::os::unix::net::UnixStream,
        settings_path: std::path::PathBuf,
        startup_warning: Option<String>,
    ) -> anyhow::Result<()> {
        let paths = (
            crate::config::RegistryPath::resolve()?.0,
            crate::server::ServerPaths::resolve()?.socket,
        );
        ovrcr_tui::run_dashboard(
            stream,
            crate::task_cli::request,
            settings_path,
            paths,
            startup_warning,
        )
    }
}

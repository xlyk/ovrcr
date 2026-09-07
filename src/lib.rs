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
pub use ovrcr_tui as tui;

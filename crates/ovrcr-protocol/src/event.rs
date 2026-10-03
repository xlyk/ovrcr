//! One dated line about a decision the Server made.
//!
//! A message is OVRCR's own sentence. It never carries a prompt, a transcript,
//! a credential, an account identifier, or a native body.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventComponent {
    Titles,
    Settings,
    Quota,
}

/// Oldest first when a list is returned. `subject` is a session id or a
/// provider name, never an account identifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub time_unix_ms: u64,
    pub component: EventComponent,
    pub subject: Option<String>,
    pub message: String,
}

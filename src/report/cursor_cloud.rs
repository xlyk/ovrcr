//! Admission for the preview adapter. No conversation or readiness is invented.
use super::{
    InvocationLease,
    reporter::{self, Frames, Reporter},
};
use ovrcr_protocol::AgentProvider;
use ovrcr_runtime::agent_runner::HookHandler;
use std::time::Instant;

pub fn receiver(lease: InvocationLease) -> HookHandler {
    Reporter::new(AgentProvider::CursorCloud, Some(lease), None).handler(Preview)
}
struct Preview;
impl Frames for Preview {
    fn frame(
        &mut self,
        reporter: &mut Reporter,
        input: &[u8],
        native_root: bool,
        _: Instant,
    ) -> Vec<u8> {
        if !reporter.closed() && native_root && serde_json::from_slice::<serde_json::Value>(input).ok().is_some_and(|value| value == serde_json::json!({"provider":"cursor-cloud","origin":"preview-admission","payload":{}})) {
            reporter::ACCEPTED.to_vec()
        } else { b"admission-unavailable\n".to_vec() }
    }
}

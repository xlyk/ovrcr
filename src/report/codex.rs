//! Authenticated synchronous Codex hooks. No transcript or completion inference.
use super::InvocationLease;
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, ProviderReport, SampleQuality,
};
use ovrcr_runtime::agent_runner::{HookEvent, HookHandler};
use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    path::Path,
    time::Instant,
};

pub const PINNED_VERSION: &str = "0.153.0";
const MAX_IDENTITIES: usize = 65_536;
const MAX_IDENTITY_BYTES: usize = 16 * 1024 * 1024;

/// Fresh interactive grammar only. Provider arguments are never rewritten.
pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("codex")) {
        return false;
    }
    let mut args = argv[1..].iter();
    let mut prompt = false;
    while let Some(arg) = args.next() {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        match arg {
            "--no-alt-screen" | "--full-auto" => {}
            "--model" | "-m" | "--profile" | "-p" | "--sandbox" | "-s" | "--ask-for-approval"
            | "-a" | "--cd" | "-C" => {
                if args
                    .next()
                    .is_none_or(|v| v.is_empty() || v.to_string_lossy().starts_with('-'))
                {
                    return false;
                }
            }
            "--" => return !prompt && args.count() == 1,
            value
                if !value.starts_with('-')
                    && !prompt
                    && !matches!(
                        value,
                        "exec"
                            | "e"
                            | "review"
                            | "login"
                            | "logout"
                            | "mcp"
                            | "mcp-server"
                            | "app-server"
                            | "app"
                            | "completion"
                            | "sandbox"
                            | "debug"
                            | "apply"
                            | "resume"
                            | "fork"
                            | "cloud"
                            | "features"
                            | "help"
                    ) =>
            {
                prompt = true
            }
            _ => return false,
        }
    }
    true
}
pub fn supported_version(executable: &OsStr) -> bool {
    super::admission::probe_version(executable).as_deref() == Some(b"codex-cli 0.153.0\n")
}
pub fn receiver(lease: Option<InvocationLease>, argv: &[OsString]) -> HookHandler {
    let unavailable = if lease.is_none() {
        Some("no managed reservation")
    } else if !eligible_argv(argv) {
        Some("unsupported launch arguments")
    } else if unsafe { libc::isatty(0) != 1 || libc::isatty(1) != 1 } {
        Some("interactive terminal required")
    } else if !supported_version(&argv[0]) {
        Some("version probe unsupported or unavailable")
    } else {
        None
    };
    let supported = unavailable.is_none();
    let mut receiver = Receiver {
        lease,
        disabled: !supported,
        active: None,
        seen: HashSet::new(),
        charged_bytes: 0,
        revision: 0,
    };
    if let Some(reason) = unavailable {
        receiver.disable();
        eprintln!("Codex reporting unavailable ({reason}); running native command");
    }
    Box::new(move |event| match event {
        HookEvent::Request {
            input,
            native_root,
            deadline,
        } => receiver.handle(input, native_root, deadline),
        HookEvent::NativeCompleted { .. } => {
            receiver.disable();
            Vec::new()
        }
        HookEvent::Poll { .. } => Vec::new(),
    })
}
struct Receiver {
    lease: Option<InvocationLease>,
    disabled: bool,
    active: Option<(String, String)>,
    seen: HashSet<(String, String)>,
    charged_bytes: usize,
    revision: u64,
}
impl Receiver {
    fn disable(&mut self) {
        self.disabled = true;
        self.active = None;
        if let Some(lease) = self.lease.take() {
            // Close the watch immediately; Drop cannot create an extra blocking grace period.
            let _ = lease.stream.shutdown(std::net::Shutdown::Both);
            drop(lease);
        }
    }
    fn handle(&mut self, input: &[u8], native_root: bool, deadline: Instant) -> Vec<u8> {
        const IGNORED: &[u8] = b"admission-ignored\n";
        const UNAVAILABLE: &[u8] = b"admission-unavailable\n";
        if self.disabled {
            return UNAVAILABLE.to_vec();
        }
        if !native_root || input.len() > super::HOOK_INPUT_LIMIT {
            return IGNORED.to_vec();
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(input) else {
            return IGNORED.to_vec();
        };
        if value["provider"] != "codex" || value["origin"] != "codex-hook" {
            return IGNORED.to_vec();
        }
        let payload = &value["payload"];
        let Some(event) = payload["hook_event_name"].as_str() else {
            return IGNORED.to_vec();
        };
        if event.starts_with("Subagent")
            || payload
                .get("agent_id")
                .is_some_and(|id| id.as_str() != Some(""))
        {
            return IGNORED.to_vec();
        }
        if !matches!(
            event,
            "UserPromptSubmit" | "Stop" | "Interrupt" | "SessionEnd"
        ) {
            return IGNORED.to_vec();
        }
        let Some(session) = payload["session_id"]
            .as_str()
            .filter(|s| ovrcr_protocol::validate_agent_id(s).is_ok())
        else {
            return IGNORED.to_vec();
        };
        let Some(turn) = payload["turn_id"]
            .as_str()
            .filter(|s| ovrcr_protocol::validate_agent_id(s).is_ok())
        else {
            return IGNORED.to_vec();
        };
        let identity = (session.to_owned(), turn.to_owned());
        if event == "SessionEnd" {
            if self.active.as_ref() == Some(&identity)
                || (self.active.is_none()
                    && self.seen.contains(&identity)
                    && self
                        .lease
                        .as_ref()
                        .and_then(|lease| lease.binding.as_ref())
                        .is_some_and(|binding| binding.conversation == session))
            {
                self.disable();
            }
            return IGNORED.to_vec();
        }
        let state = if event == "UserPromptSubmit" {
            // Deduplicate *before* any revisions, binding changes, or freshness updates.
            if self.seen.contains(&identity) {
                return IGNORED.to_vec();
            }
            let charge = session.len() + turn.len() + std::mem::size_of::<(String, String)>();
            if self.active.is_some()
                || self.seen.len() >= MAX_IDENTITIES
                || self.charged_bytes + charge > MAX_IDENTITY_BYTES
            {
                self.disable();
                return UNAVAILABLE.to_vec();
            }
            self.seen.insert(identity.clone());
            self.charged_bytes += charge;
            if !self.bind(session, deadline) {
                self.disable();
                return UNAVAILABLE.to_vec();
            }
            self.active = Some(identity.clone());
            AgentActivity::Busy
        } else {
            if self.active.as_ref() != Some(&identity) {
                return IGNORED.to_vec();
            }
            self.active = None;
            if event == "Stop" {
                AgentActivity::ResponseReady
            } else {
                AgentActivity::Idle
            }
        };
        let Some(lease) = &self.lease else {
            self.disable();
            return UNAVAILABLE.to_vec();
        };
        let Some(binding) = lease.binding.clone() else {
            self.disable();
            return UNAVAILABLE.to_vec();
        };
        self.revision += 1;
        let report = ProviderReport {
            binding,
            revision: self.revision,
            observation: AgentObservation::Activity(ActivitySample {
                state,
                quality: SampleQuality::Observed,
                turn: Some(turn.to_owned()),
            }),
        };
        if lease.publish_observation(report, deadline).is_err() {
            self.disable();
            return UNAVAILABLE.to_vec();
        }
        b"admission-accepted\n".to_vec()
    }
    fn bind(&mut self, conversation: &str, deadline: Instant) -> bool {
        let Some(lease) = &mut self.lease else {
            return false;
        };
        match lease.bind(ovrcr_protocol::AgentProvider::Codex, conversation, deadline) {
            Some(true) => {
                self.revision = 0;
                true
            }
            Some(false) => true,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn unbound() -> Receiver {
        Receiver {
            lease: None,
            disabled: false,
            active: None,
            seen: HashSet::new(),
            charged_bytes: 0,
            revision: 0,
        }
    }
    fn prompt(session: &str, turn: &str) -> Vec<u8> {
        serde_json::to_vec(
            &serde_json::json!({"provider":"codex","origin":"codex-hook","payload":{
                "hook_event_name":"UserPromptSubmit","session_id":session,"turn_id":turn
            }}),
        )
        .unwrap()
    }
    #[test]
    fn codex_identity_count_exhaustion_disables_instead_of_evicting() {
        let mut receiver = unbound();
        for i in 0..MAX_IDENTITIES {
            receiver.seen.insert(("root".into(), i.to_string()));
        }
        let before = receiver.seen.clone();
        assert_eq!(
            receiver.handle(&prompt("root", "new"), true, Instant::now()),
            b"admission-unavailable\n"
        );
        assert!(receiver.disabled);
        assert_eq!(receiver.seen, before);
        assert_eq!(receiver.revision, 0);
    }
    #[test]
    fn codex_identity_byte_exhaustion_disables_instead_of_evicting() {
        let mut receiver = unbound();
        let session = "s".repeat(256);
        let mut i = 0;
        // Charge actual retained string sizes and identity storage, independently
        // calculating the boundary instead of merely setting the counter to its limit.
        loop {
            let turn = format!("{i:0256}");
            let charge = session.len() + turn.len() + std::mem::size_of::<(String, String)>();
            if receiver.charged_bytes + charge > MAX_IDENTITY_BYTES {
                break;
            }
            receiver.charged_bytes += charge;
            receiver.seen.insert((session.clone(), turn));
            i += 1;
        }
        assert!(receiver.seen.len() < MAX_IDENTITIES);
        let count = receiver.seen.len();
        assert_eq!(
            receiver.handle(
                &prompt(&session, &format!("{i:0256}")),
                true,
                Instant::now()
            ),
            b"admission-unavailable\n"
        );
        assert!(receiver.disabled);
        assert_eq!(receiver.seen.len(), count);
        assert!(receiver.charged_bytes <= MAX_IDENTITY_BYTES);
    }
    #[test]
    fn codex_closed_turn_duplicate_is_checked_before_capacity() {
        let mut receiver = unbound();
        receiver.seen.insert(("root".into(), "old".into()));
        receiver.charged_bytes = MAX_IDENTITY_BYTES;
        assert_eq!(
            receiver.handle(&prompt("root", "old"), true, Instant::now()),
            b"admission-ignored\n"
        );
        assert!(!receiver.disabled);
        assert!(receiver.active.is_none());
        assert_eq!(receiver.revision, 0);
    }
}

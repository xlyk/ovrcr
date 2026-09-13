//! Managed interactive Pi: argv eligibility and the authenticated extension receiver.
use super::{HOOK_INPUT_LIMIT, InvocationLease};
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, AgentProvider, ProviderReport, SampleQuality,
    validate_agent_id,
};
use ovrcr_runtime::agent_runner::{HookEvent, HookHandler, private_identifier};
use std::{
    collections::HashSet,
    ffi::{OsStr, OsString},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

pub const EXTENSION_SOURCE: &str = include_str!("../pi-reporting-extension.mjs");
/// Bounded delivery shared by every provider extension; materialized as its sibling.
pub const TRANSPORT_SOURCE: &str = include_str!("../ovrcr-reporting-transport.mjs");
const TRANSPORT_FILE: &str = "ovrcr-reporting-transport.mjs";
const EXTENSION_FILE: &str = "ovrcr-pi-reporting.mjs";
const BINARY_TOKEN: &str = "__OVRCR_BINARY__";
const MAX_IDENTITIES: usize = 65_536;
const MAX_IDENTITY_BYTES: usize = 16 * 1024 * 1024;
const ACCEPTED: &[u8] = b"admission-accepted\n";
const IGNORED: &[u8] = b"admission-ignored\n";
const UNAVAILABLE: &[u8] = b"admission-unavailable\n";

/// Interactive terminal launches only. Provider arguments are never rewritten here.
pub fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|s| Path::new(s).file_name()) != Some(OsStr::new("pi")) {
        return false;
    }
    let Some(rest) = argv.get(1..) else {
        return false;
    };
    if let Some(first) = rest.first().and_then(|s| s.to_str())
        && !first.starts_with('-')
        && matches!(
            first,
            "auth"
                | "install"
                | "uninstall"
                | "remove"
                | "list"
                | "update"
                | "upgrade"
                | "config"
                | "help"
        )
    {
        return false;
    }
    rest.iter().all(|arg| {
        let Some(arg) = arg.to_str() else {
            return false;
        };
        !matches!(
            arg,
            "--mode"
                | "-p"
                | "--print"
                | "-h"
                | "--help"
                | "-v"
                | "--version"
                | "--export"
                | "--list-models"
        )
    })
}

/// The extension text for one invocation: the helper path is embedded as a JSON string.
pub fn extension_source(binary: &Path) -> Option<String> {
    let encoded = serde_json::to_string(binary.to_str()?).ok()?;
    Some(EXTENSION_SOURCE.replacen(BINARY_TOKEN, &encoded, 1))
}

/// A task-owned 0700 directory holding the materialized extension and the transport
/// module it imports; both 0600, both removed with the directory on disable.
fn materialize_extension() -> std::io::Result<(PathBuf, PathBuf)> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let binary = std::env::current_exe()?;
    let source = extension_source(&binary)
        .ok_or_else(|| std::io::Error::other("ovrcr path is not valid UTF-8"))?;
    let dir = std::env::temp_dir().join(format!("ovrcr-pi-{}", private_identifier()?));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    let write = |name: &str, text: &str| -> std::io::Result<PathBuf> {
        let path = dir.join(name);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(text.as_bytes())?;
        Ok(path)
    };
    write(TRANSPORT_FILE, TRANSPORT_SOURCE)?;
    let path = write(EXTENSION_FILE, &source)?;
    Ok((dir, path))
}

pub fn receiver(lease: Option<InvocationLease>, argv: &mut Vec<OsString>) -> HookHandler {
    let unavailable = if lease.is_none() {
        Some("no managed reservation".to_owned())
    } else if !eligible_argv(argv) {
        Some("unsupported launch arguments".to_owned())
    } else if unsafe { libc::isatty(0) != 1 || libc::isatty(1) != 1 } {
        Some("interactive terminal required".to_owned())
    } else if super::admission::probe_version(&argv[0]).is_none() {
        Some("version probe unavailable".to_owned())
    } else {
        None
    };
    let mut receiver = Receiver::new(lease);
    match unavailable {
        Some(reason) => {
            receiver.disable();
            eprintln!("Pi reporting unavailable ({reason}); running native command");
        }
        None => match materialize_extension() {
            Ok((dir, path)) => {
                receiver.extension_dir = Some(dir);
                argv.insert(1, "-e".into());
                argv.insert(2, path.into_os_string());
            }
            Err(error) => {
                receiver.disable();
                eprintln!(
                    "Pi reporting unavailable (extension not materialized: {error}); running native command"
                );
            }
        },
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
    extension_dir: Option<PathBuf>,
    /// The admitted extension instance and its last accepted source sequence.
    producer: Option<(String, u64)>,
    /// The open response cycle: Pi run counter and the cycle identity published as `turn`.
    current: Option<(u64, String)>,
    seen: HashSet<String>,
    charged_bytes: usize,
    revision: u64,
}

impl Receiver {
    fn new(lease: Option<InvocationLease>) -> Self {
        Receiver {
            lease,
            disabled: false,
            extension_dir: None,
            producer: None,
            current: None,
            seen: HashSet::new(),
            charged_bytes: 0,
            revision: 0,
        }
    }
    fn disable(&mut self) {
        self.disabled = true;
        self.current = None;
        if let Some(lease) = self.lease.take() {
            let _ = lease.stream.shutdown(std::net::Shutdown::Both);
            drop(lease);
        }
        if let Some(dir) = self.extension_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    fn handle(&mut self, input: &[u8], native_root: bool, deadline: Instant) -> Vec<u8> {
        if self.disabled {
            return UNAVAILABLE.to_vec();
        }
        if !native_root || input.len() > HOOK_INPUT_LIMIT {
            return IGNORED.to_vec();
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(input) else {
            return IGNORED.to_vec();
        };
        if value["provider"] != "pi" || value["origin"] != "pi-extension" {
            return IGNORED.to_vec();
        }
        let payload = &value["payload"];
        if payload["schema"] != 1 || payload["mode"] != "tui" {
            return IGNORED.to_vec();
        }
        let (Some(event), Some(instance), Some(sequence), Some(session)) = (
            payload["event"].as_str(),
            payload["instance"]
                .as_str()
                .filter(|s| validate_agent_id(s).is_ok()),
            payload["sequence"].as_u64().filter(|s| *s > 0),
            payload["session_id"]
                .as_str()
                .filter(|s| validate_agent_id(s).is_ok()),
        ) else {
            return IGNORED.to_vec();
        };
        // Producer fencing: a new instance is admitted only by its session_start; a stale,
        // replayed or foreign sequence never mutates state, freshness, or Unread.
        match &mut self.producer {
            Some((current, last)) if current.as_str() == instance => {
                if sequence <= *last {
                    return IGNORED.to_vec();
                }
                *last = sequence;
            }
            _ if event == "session_start" => {
                self.producer = Some((instance.to_owned(), sequence));
                self.current = None;
            }
            _ => return IGNORED.to_vec(),
        }
        let run = payload["run"].as_u64();
        let outcome = payload["outcome"].as_str().unwrap_or("none");
        let (state, quality, turn) = match event {
            "session_start" => {
                if !self.bind(session, deadline) {
                    self.disable();
                    return UNAVAILABLE.to_vec();
                }
                (AgentActivity::Idle, SampleQuality::Observed, None)
            }
            "agent_start" => {
                let Some(run) = run else {
                    return IGNORED.to_vec();
                };
                let turn = format!("{instance}:{run}");
                // A repeated start for the same run is the continuation Pi emits for a retry,
                // automatic compaction or a queued follow-up: the cycle is already open and
                // already published, so there is nothing to add. Duplicates are checked before
                // any capacity, binding, or revision change.
                if self.seen.contains(&turn) {
                    return IGNORED.to_vec();
                }
                // A different run while a cycle is open replaces it and publishes the new
                // identity. Permanent disablement is reserved for unresolvable identity or
                // ordering ambiguity (#88); recovering a lost close is #91's job.
                let charge = turn.len() + std::mem::size_of::<String>();
                if self.seen.len() >= MAX_IDENTITIES
                    || self.charged_bytes + charge > MAX_IDENTITY_BYTES
                {
                    self.disable();
                    return UNAVAILABLE.to_vec();
                }
                if !self.bind(session, deadline) {
                    self.disable();
                    return UNAVAILABLE.to_vec();
                }
                self.seen.insert(turn.clone());
                self.charged_bytes += charge;
                self.current = Some((run, turn.clone()));
                (AgentActivity::Busy, SampleQuality::Observed, Some(turn))
            }
            "agent_end" => {
                // Records the outcome on the producer side; Ready waits for the settled boundary.
                return if self.current.as_ref().is_some_and(|(r, _)| Some(*r) == run) {
                    ACCEPTED.to_vec()
                } else {
                    IGNORED.to_vec()
                };
            }
            "agent_settled" => {
                let Some((current_run, turn)) = self.current.clone() else {
                    return IGNORED.to_vec();
                };
                if Some(current_run) != run {
                    return IGNORED.to_vec();
                }
                self.current = None;
                match outcome {
                    "ok" => (
                        AgentActivity::ResponseReady,
                        SampleQuality::Confirmed,
                        Some(turn),
                    ),
                    "error" => (AgentActivity::Error, SampleQuality::Observed, Some(turn)),
                    _ => (AgentActivity::Idle, SampleQuality::Observed, Some(turn)),
                }
            }
            "session_shutdown" => {
                // `reason` is a bounded discriminant: compared against the known set and never
                // retained. Anything else is unknown and retires the producer rather than
                // disabling it.
                let reason = payload["reason"].as_str().filter(|value| {
                    matches!(
                        *value,
                        "startup" | "reload" | "new" | "resume" | "fork" | "quit" | "overflow"
                    )
                });
                if reason == Some("quit") {
                    self.disable();
                } else {
                    // Replacement or reload: Pi re-runs factories; the next session_start admits them.
                    self.producer = None;
                    self.current = None;
                }
                return IGNORED.to_vec();
            }
            "unavailable" => {
                self.disable();
                return UNAVAILABLE.to_vec();
            }
            _ => return IGNORED.to_vec(),
        };
        self.publish(state, quality, turn, deadline)
    }
    fn bind(&mut self, conversation: &str, deadline: Instant) -> bool {
        let Some(lease) = &mut self.lease else {
            return false;
        };
        match lease.bind(AgentProvider::Pi, conversation, deadline) {
            Some(true) => {
                self.revision = 0;
                true
            }
            Some(false) => true,
            None => false,
        }
    }
    fn publish(
        &mut self,
        state: AgentActivity,
        quality: SampleQuality,
        turn: Option<String>,
        deadline: Instant,
    ) -> Vec<u8> {
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
                quality,
                turn,
            }),
        };
        if lease.publish_observation(report, deadline).is_err() {
            self.disable();
            return UNAVAILABLE.to_vec();
        }
        ACCEPTED.to_vec()
    }
}

/// The accept loop can exit, or its thread be abandoned, without a disabling callback:
/// the materialized directory is still this receiver's to remove.
impl Drop for Receiver {
    fn drop(&mut self) {
        if let Some(dir) = self.extension_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn unbound() -> Receiver {
        Receiver::new(None)
    }
    fn event(
        instance: &str,
        sequence: u64,
        event: &str,
        run: Option<u64>,
        outcome: &str,
    ) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"provider":"pi","origin":"pi-extension","payload":{
            "schema":1,"event":event,"mode":"tui","owner_pid":1,"instance":instance,"sequence":sequence,
            "session_id":"sess-a","run":run,"outcome":outcome}}))
        .unwrap()
    }
    #[test]
    fn unknown_producer_is_admitted_only_by_session_start() {
        let mut receiver = unbound();
        assert_eq!(
            receiver.handle(
                &event("a", 1, "agent_start", Some(1), "none"),
                true,
                Instant::now()
            ),
            b"admission-ignored\n"
        );
        assert!(receiver.producer.is_none());
        // session_start with no lease cannot bind: the receiver disables itself honestly.
        assert_eq!(
            receiver.handle(
                &event("a", 2, "session_start", None, "none"),
                true,
                Instant::now()
            ),
            b"admission-unavailable\n"
        );
        assert!(receiver.disabled);
    }
    #[test]
    fn stale_and_replayed_sequences_are_ignored_before_any_mutation() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 5));
        for sequence in [5, 4, 1] {
            assert_eq!(
                receiver.handle(
                    &event("a", sequence, "agent_end", Some(1), "ok"),
                    true,
                    Instant::now()
                ),
                b"admission-ignored\n"
            );
        }
        assert_eq!(receiver.producer, Some(("a".into(), 5)));
        assert!(!receiver.disabled);
        assert_eq!(receiver.revision, 0);
    }
    #[test]
    fn foreign_provider_non_tui_and_grandchild_payloads_are_ignored() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        let mut rpc: serde_json::Value =
            serde_json::from_slice(&event("a", 2, "agent_start", Some(1), "none")).unwrap();
        rpc["payload"]["mode"] = "rpc".into();
        assert_eq!(
            receiver.handle(&serde_json::to_vec(&rpc).unwrap(), true, Instant::now()),
            b"admission-ignored\n"
        );
        let codex =
            br#"{"provider":"codex","origin":"codex-hook","payload":{"hook_event_name":"Stop"}}"#;
        assert_eq!(
            receiver.handle(codex, true, Instant::now()),
            b"admission-ignored\n"
        );
        assert_eq!(
            receiver.handle(
                &event("a", 3, "agent_start", Some(1), "none"),
                false,
                Instant::now()
            ),
            b"admission-ignored\n"
        );
        assert_eq!(
            receiver.producer,
            Some(("a".into(), 1)),
            "ignored payloads never advance the sequence"
        );
    }
    #[test]
    fn settled_for_a_run_that_is_not_current_is_ignored() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        receiver.current = Some((2, "a:2".into()));
        assert_eq!(
            receiver.handle(
                &event("a", 2, "agent_settled", Some(1), "ok"),
                true,
                Instant::now()
            ),
            b"admission-ignored\n"
        );
        assert_eq!(receiver.current, Some((2, "a:2".into())));
    }
    #[test]
    fn duplicate_agent_start_is_checked_before_capacity() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        receiver.seen.insert("a:1".into());
        receiver.charged_bytes = MAX_IDENTITY_BYTES;
        assert_eq!(
            receiver.handle(
                &event("a", 2, "agent_start", Some(1), "none"),
                true,
                Instant::now()
            ),
            b"admission-ignored\n"
        );
        assert!(!receiver.disabled);
    }
    #[test]
    fn shutdown_for_replacement_retires_the_producer_without_disabling() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        receiver.current = Some((1, "a:1".into()));
        let mut shutdown: serde_json::Value =
            serde_json::from_slice(&event("a", 2, "session_shutdown", Some(1), "ok")).unwrap();
        shutdown["payload"]["reason"] = "reload".into();
        assert_eq!(
            receiver.handle(
                &serde_json::to_vec(&shutdown).unwrap(),
                true,
                Instant::now()
            ),
            b"admission-ignored\n"
        );
        assert!(receiver.producer.is_none() && receiver.current.is_none() && !receiver.disabled);
        shutdown["payload"]["reason"] = "quit".into();
        shutdown["payload"]["sequence"] = 3.into();
        receiver.producer = Some(("a".into(), 2));
        receiver.handle(
            &serde_json::to_vec(&shutdown).unwrap(),
            true,
            Instant::now(),
        );
        assert!(receiver.disabled);
    }
}

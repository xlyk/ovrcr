//! One authenticated receiver for every provider whose managed launch loads an OVRCR
//! reporting extension. The vocabulary is Pi's — `session_start`, `agent_start`,
//! `agent_end`, `agent_settled`, `session_shutdown` — and each harness says only who it
//! is and how good the Ready at its settled boundary is. Oh My Pi has no settled event of
//! its own: its extension synthesizes one and this receiver publishes it as Observed,
//! because a stop hook may still continue after a clean end.
use super::{HOOK_INPUT_LIMIT, InvocationLease};
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, AgentProvider, InputKind, InputRequest,
    MAX_INPUT_REQUESTS, ProviderReport, SampleQuality, validate_agent_id,
};
use ovrcr_runtime::agent_runner::{HookEvent, HookHandler, private_identifier};
use std::{
    collections::HashSet,
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

/// Bounded delivery shared by every provider extension; materialized as its sibling.
pub const TRANSPORT_SOURCE: &str = include_str!("../ovrcr-reporting-transport.mjs");
const TRANSPORT_FILE: &str = "ovrcr-reporting-transport.mjs";
const BINARY_TOKEN: &str = "__OVRCR_BINARY__";
const MAX_IDENTITIES: usize = 65_536;
const MAX_IDENTITY_BYTES: usize = 16 * 1024 * 1024;
const ACCEPTED: &[u8] = b"admission-accepted\n";
const IGNORED: &[u8] = b"admission-ignored\n";
const UNAVAILABLE: &[u8] = b"admission-unavailable\n";

/// Everything that differs between one extension-reporting provider and the next.
pub struct Harness {
    pub provider: AgentProvider,
    pub name: &'static str,
    pub display: &'static str,
    pub origin: &'static str,
    pub extension_file: &'static str,
    pub extension_source: &'static str,
    pub eligible_argv: fn(&[OsString]) -> bool,
    /// Quality of the Ready published at the extension's settled event.
    pub settled_quality: SampleQuality,
}

/// The extension text for one invocation: the helper path is embedded as a JSON string.
pub fn extension_source(harness: &Harness, binary: &Path) -> Option<String> {
    let encoded = serde_json::to_string(binary.to_str()?).ok()?;
    Some(harness.extension_source.replacen(BINARY_TOKEN, &encoded, 1))
}

/// A task-owned 0700 directory holding the materialized extension and the transport
/// module it imports; both 0600, both removed with the directory on disable.
fn materialize_extension(harness: &Harness) -> std::io::Result<(PathBuf, PathBuf)> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let binary = std::env::current_exe()?;
    let source = extension_source(harness, &binary)
        .ok_or_else(|| std::io::Error::other("ovrcr path is not valid UTF-8"))?;
    let dir =
        std::env::temp_dir().join(format!("ovrcr-{}-{}", harness.name, private_identifier()?));
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
    let path = write(harness.extension_file, &source)?;
    Ok((dir, path))
}

pub fn receiver(
    harness: &'static Harness,
    lease: Option<InvocationLease>,
    argv: &mut Vec<OsString>,
) -> HookHandler {
    let unavailable = if lease.is_none() {
        Some("no managed reservation".to_owned())
    } else if !(harness.eligible_argv)(argv) {
        Some("unsupported launch arguments".to_owned())
    } else if unsafe { libc::isatty(0) != 1 || libc::isatty(1) != 1 } {
        Some("interactive terminal required".to_owned())
    } else if super::admission::probe_version(&argv[0]).is_none() {
        Some("version probe unavailable".to_owned())
    } else {
        None
    };
    let mut receiver = Receiver::new(harness, lease);
    match unavailable {
        Some(reason) => {
            receiver.disable();
            eprintln!(
                "{} reporting unavailable ({reason}); running native command",
                harness.display
            );
        }
        None => match materialize_extension(harness) {
            Ok((dir, path)) => {
                receiver.extension_dir = Some(dir);
                argv.insert(1, "-e".into());
                argv.insert(2, path.into_os_string());
            }
            Err(error) => {
                receiver.disable();
                eprintln!(
                    "{} reporting unavailable (extension not materialized: {error}); running native command",
                    harness.display
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
    harness: &'static Harness,
    lease: Option<InvocationLease>,
    disabled: bool,
    extension_dir: Option<PathBuf>,
    /// The admitted extension instance and its last accepted source sequence.
    producer: Option<(String, u64)>,
    /// The open response cycle: the harness run counter and the identity published as `turn`.
    current: Option<(u64, String)>,
    /// The open Input requests, oldest first, bounded by `MAX_INPUT_REQUESTS`. Every
    /// mutation republishes the whole set, so the runtime never merges deltas. The kind
    /// travels with the id because only the `prompt` namespace's kind comes from the
    /// payload; `approval` and `question` are fixed by their namespace.
    open_requests: Vec<(String, InputKind)>,
    seen: HashSet<String>,
    charged_bytes: usize,
    revision: u64,
}

impl Receiver {
    fn new(harness: &'static Harness, lease: Option<InvocationLease>) -> Self {
        Receiver {
            harness,
            lease,
            disabled: false,
            extension_dir: None,
            producer: None,
            current: None,
            open_requests: Vec::new(),
            seen: HashSet::new(),
            charged_bytes: 0,
            revision: 0,
        }
    }
    fn disable(&mut self) {
        self.disabled = true;
        self.current = None;
        self.open_requests.clear();
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
        if value["provider"] != self.harness.name || value["origin"] != self.harness.origin {
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
                // A conversation switch happens in place on the same instance (Oh My Pi
                // re-announces with session_start): whatever cycle was open belongs to the
                // conversation being left, so it is invalidated before the new binding.
                self.current = None;
                // The runtime must see the dialogs close against the binding that owned
                // them, so the set is published empty while that binding is still current.
                if let Some(closed) = self.retire_requests(deadline) {
                    return closed;
                }
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
                // A repeated start for the same run is the continuation the harness emits for
                // a retry, automatic compaction or a queued follow-up: the cycle is already
                // open and already published, so there is nothing to add. Duplicates are
                // checked before any capacity, binding, or revision change.
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
                        self.harness.settled_quality,
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
                    // Replacement or reload: the harness re-runs factories; the next
                    // session_start admits them. The lease and binding are still live here, so
                    // a dialog left open is published closed before the producer retires:
                    // otherwise a re-admitted producer that binds the same conversation reuses
                    // the same snapshot and keeps a stale WaitingInput with nothing on screen.
                    if let Some(closed) = self.retire_requests(deadline) {
                        return closed;
                    }
                    self.producer = None;
                    self.current = None;
                }
                return IGNORED.to_vec();
            }
            "input_open" => {
                let (Some(id), Some(namespace)) = (
                    payload["request_id"]
                        .as_str()
                        .filter(|s| validate_agent_id(s).is_ok()),
                    namespace_of(
                        payload["namespace"].as_str(),
                        payload["request_id"].as_str(),
                    ),
                ) else {
                    return IGNORED.to_vec();
                };
                // The kind of an approval or a question follows from its namespace; only a
                // prompt carries its own, and it must still be one this protocol knows.
                let Some(kind) = namespace.kind(payload["kind"].as_str()) else {
                    return IGNORED.to_vec();
                };
                return self.open_request(id, kind, deadline);
            }
            "input_close" => {
                let Some(id) = payload["request_id"].as_str() else {
                    return IGNORED.to_vec();
                };
                return self.close_request(id, deadline);
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
        let provider = self.harness.provider;
        let Some(lease) = &mut self.lease else {
            return false;
        };
        match lease.bind(provider, conversation, deadline) {
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
        self.publish_observation(
            AgentObservation::Activity(ActivitySample {
                state,
                quality,
                turn,
            }),
            deadline,
        )
    }
    /// Insert-if-absent, then publish the whole set. A repeated open of a known id and the
    /// opening past the bound are both IGNORED before any mutation: overflow drops the new
    /// request, it never disables reporting.
    fn open_request(&mut self, id: &str, kind: InputKind, deadline: Instant) -> Vec<u8> {
        if self.open_requests.iter().any(|(open, _)| open == id)
            || self.open_requests.len() >= MAX_INPUT_REQUESTS
        {
            return IGNORED.to_vec();
        }
        self.open_requests.push((id.to_owned(), kind));
        self.publish_requests(deadline)
    }
    /// Remove-if-present, then publish the whole set. A close for an id this receiver never
    /// admitted is IGNORED: a late subscriber's unmatched close changes nothing.
    fn close_request(&mut self, id: &str, deadline: Instant) -> Vec<u8> {
        let before = self.open_requests.len();
        self.open_requests.retain(|(open, _)| open != id);
        if self.open_requests.len() == before {
            return IGNORED.to_vec();
        }
        self.publish_requests(deadline)
    }
    fn publish_requests(&mut self, deadline: Instant) -> Vec<u8> {
        let requests = self
            .open_requests
            .iter()
            .map(|(id, kind)| InputRequest {
                id: id.clone(),
                kind: *kind,
            })
            .collect();
        self.publish_observation(AgentObservation::Input(requests), deadline)
    }
    /// Publishes the empty set for a producer that is about to retire or rebind, while the
    /// binding that owned the requests is still current. `Some` is the caller's early
    /// return: the publish failed and the receiver disabled itself.
    fn retire_requests(&mut self, deadline: Instant) -> Option<Vec<u8>> {
        if self.open_requests.is_empty() {
            return None;
        }
        self.open_requests.clear();
        let closed = self.publish_requests(deadline);
        (closed.as_slice() == UNAVAILABLE).then_some(closed)
    }
    fn publish_observation(&mut self, observation: AgentObservation, deadline: Instant) -> Vec<u8> {
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
            observation,
        };
        if lease.publish_observation(report, deadline).is_err() {
            self.disable();
            return UNAVAILABLE.to_vec();
        }
        ACCEPTED.to_vec()
    }
}

fn input_kind(value: Option<&str>) -> Option<InputKind> {
    Some(match value? {
        "select" => InputKind::Select,
        "confirm" => InputKind::Confirm,
        "input" => InputKind::Input,
        "editor" => InputKind::Editor,
        "custom" => InputKind::Custom,
        "approval" => InputKind::Approval,
        _ => return None,
    })
}

/// Which surface issued a request. The id carries the namespace so the runtime, the
/// Dashboard and a later close cannot confuse one surface's request identity with
/// another's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Namespace {
    Prompt,
    Approval,
    Question,
}

impl Namespace {
    /// An approval and a question are what their namespace says they are; only a prompt
    /// reports its own kind, and it may not claim to be an approval.
    fn kind(self, payload: Option<&str>) -> Option<InputKind> {
        match self {
            Namespace::Approval => Some(InputKind::Approval),
            Namespace::Question => Some(InputKind::Select),
            Namespace::Prompt => match input_kind(payload)? {
                InputKind::Approval => None,
                kind => Some(kind),
            },
        }
    }
}

/// The namespace a frame declares, checked against the id that carries it. `approval` and
/// `question` ids are namespace-prefixed; a prompt's id is the producer's own (Pi's
/// `<instance>:p<n>`), so a missing namespace means `prompt` and no prefix is required.
fn namespace_of(declared: Option<&str>, id: Option<&str>) -> Option<Namespace> {
    let (namespace, prefix) = match declared {
        None | Some("prompt") => return Some(Namespace::Prompt),
        Some("approval") => (Namespace::Approval, "approval:"),
        Some("question") => (Namespace::Question, "question:"),
        Some(_) => return None,
    };
    id?.starts_with(prefix).then_some(namespace)
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
    use crate::report::{omp, pi};
    fn unbound() -> Receiver {
        Receiver::new(&pi::HARNESS, None)
    }
    fn harness_event(
        harness: &Harness,
        instance: &str,
        sequence: u64,
        event: &str,
        run: Option<u64>,
        outcome: &str,
    ) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"provider":harness.name,"origin":harness.origin,"payload":{
            "schema":1,"event":event,"mode":"tui","owner_pid":1,"instance":instance,"sequence":sequence,
            "session_id":"sess-a","run":run,"outcome":outcome}}))
        .unwrap()
    }
    fn event(
        instance: &str,
        sequence: u64,
        event: &str,
        run: Option<u64>,
        outcome: &str,
    ) -> Vec<u8> {
        harness_event(&pi::HARNESS, instance, sequence, event, run, outcome)
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
    fn each_harness_ignores_the_other_harness_envelope() {
        // Oh My Pi cannot be labelled Pi to bypass the provider check, and the reverse.
        for (harness, foreign) in [(&pi::HARNESS, &omp::HARNESS), (&omp::HARNESS, &pi::HARNESS)] {
            let mut receiver = Receiver::new(harness, None);
            receiver.producer = Some(("a".into(), 1));
            assert_eq!(
                receiver.handle(
                    &harness_event(foreign, "a", 2, "agent_start", Some(1), "none"),
                    true,
                    Instant::now()
                ),
                b"admission-ignored\n",
                "{} accepted {}",
                harness.name,
                foreign.name
            );
            assert_eq!(receiver.producer, Some(("a".into(), 1)));
            // Its own envelope reaches the state machine.
            assert_eq!(
                receiver.handle(
                    &harness_event(harness, "a", 2, "agent_end", Some(1), "ok"),
                    true,
                    Instant::now()
                ),
                b"admission-ignored\n"
            );
            assert_eq!(receiver.producer, Some(("a".into(), 2)));
        }
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
    fn input_event(
        instance: &str,
        sequence: u64,
        name: &str,
        request_id: &str,
        kind: Option<&str>,
    ) -> Vec<u8> {
        let mut value: serde_json::Value =
            serde_json::from_slice(&event(instance, sequence, name, None, "none")).unwrap();
        value["payload"]["request_id"] = request_id.into();
        if let Some(kind) = kind {
            value["payload"]["kind"] = kind.into();
        }
        serde_json::to_vec(&value).unwrap()
    }
    fn namespaced_event(
        instance: &str,
        sequence: u64,
        name: &str,
        namespace: &str,
        request_id: &str,
        kind: Option<&str>,
    ) -> Vec<u8> {
        let mut value: serde_json::Value =
            serde_json::from_slice(&input_event(instance, sequence, name, request_id, kind))
                .unwrap();
        value["payload"]["namespace"] = namespace.into();
        serde_json::to_vec(&value).unwrap()
    }
    fn ids(receiver: &Receiver) -> Vec<&str> {
        receiver
            .open_requests
            .iter()
            .map(|(id, _)| id.as_str())
            .collect()
    }
    #[test]
    fn the_request_set_keeps_its_order_and_removes_only_the_closed_member() {
        // A lease-less receiver cannot publish, so this drives the state through the
        // helpers directly: the publication path is covered by the lifecycle tests.
        let mut receiver = unbound();
        assert_eq!(
            receiver.open_request("approval:c1", InputKind::Approval, Instant::now()),
            b"admission-unavailable\n",
            "a lease-less publish is UNAVAILABLE exactly as before"
        );
        assert_eq!(
            ids(&receiver),
            Vec::<&str>::new(),
            "disabling clears the set"
        );

        let mut receiver = unbound();
        receiver.open_requests = vec![
            ("approval:c1".into(), InputKind::Approval),
            ("question:q1".into(), InputKind::Select),
        ];
        assert_eq!(
            receiver.open_request("approval:c1", InputKind::Approval, Instant::now()),
            b"admission-ignored\n",
            "a repeated open of a known id never mutates the set"
        );
        assert_eq!(ids(&receiver), ["approval:c1", "question:q1"]);
        assert_eq!(
            receiver.close_request("question:zz", Instant::now()),
            b"admission-ignored\n",
            "an unknown close never mutates the set"
        );
        assert_eq!(ids(&receiver), ["approval:c1", "question:q1"]);
        assert!(!receiver.disabled && receiver.revision == 0);
    }
    #[test]
    fn the_thirty_third_open_is_ignored_without_disabling() {
        let mut receiver = unbound();
        receiver.open_requests = (0..MAX_INPUT_REQUESTS)
            .map(|n| (format!("approval:c{n}"), InputKind::Approval))
            .collect();
        assert_eq!(
            receiver.open_request("approval:overflow", InputKind::Approval, Instant::now()),
            b"admission-ignored\n"
        );
        assert_eq!(receiver.open_requests.len(), MAX_INPUT_REQUESTS);
        assert!(
            !receiver.disabled,
            "the bound drops the new request, it never disables reporting"
        );
        assert_eq!(receiver.revision, 0);
    }
    #[test]
    fn an_open_whose_namespace_and_id_disagree_is_ignored() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        for (sequence, namespace, id, kind) in [
            // The prefix must match the declared namespace.
            (2, "approval", "question:c1", Some("approval")),
            (3, "question", "approval:q1", Some("select")),
            (4, "approval", "c1", Some("approval")),
            // An unknown namespace is not a namespace.
            (5, "dialog", "dialog:c1", Some("select")),
            // A prompt may not claim to be an approval.
            (6, "prompt", "a:p1", Some("approval")),
        ] {
            assert_eq!(
                receiver.handle(
                    &namespaced_event("a", sequence, "input_open", namespace, id, kind),
                    true,
                    Instant::now()
                ),
                b"admission-ignored\n",
                "{namespace} / {id}"
            );
        }
        assert!(receiver.open_requests.is_empty());
        assert_eq!(receiver.revision, 0);
        assert!(!receiver.disabled);
    }
    #[test]
    fn an_approval_or_question_namespace_fixes_the_kind_it_publishes() {
        // The kind follows the namespace, so a mislabelled payload cannot make an approval
        // look like a select in the Dashboard.
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        assert_eq!(
            receiver.handle(
                &namespaced_event(
                    "a",
                    2,
                    "input_open",
                    "approval",
                    "approval:c1",
                    Some("select")
                ),
                true,
                Instant::now()
            ),
            b"admission-unavailable\n",
            "no lease: the publish fails after the set is admitted"
        );
        assert_eq!(
            namespace_of(Some("approval"), Some("approval:c1"))
                .unwrap()
                .kind(Some("select")),
            Some(InputKind::Approval)
        );
        assert_eq!(
            namespace_of(Some("question"), Some("question:q1"))
                .unwrap()
                .kind(Some("confirm")),
            Some(InputKind::Select)
        );
        assert_eq!(
            namespace_of(None, Some("a:p1"))
                .unwrap()
                .kind(Some("editor")),
            Some(InputKind::Editor),
            "a missing namespace is a prompt and keeps its own kind"
        );
    }
    #[test]
    fn input_close_for_an_unknown_or_stale_request_is_ignored() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        assert_eq!(
            receiver.handle(
                &input_event("a", 2, "input_close", "a:p1", None),
                true,
                Instant::now()
            ),
            b"admission-ignored\n",
            "no request is open"
        );
        assert!(receiver.open_requests.is_empty());
        receiver.open_requests = vec![("a:p1".into(), InputKind::Select)];
        assert_eq!(
            receiver.handle(
                &input_event("a", 3, "input_close", "a:p2", None),
                true,
                Instant::now()
            ),
            b"admission-ignored\n",
            "a close only closes its own request"
        );
        assert_eq!(ids(&receiver), ["a:p1"]);
        assert_eq!(receiver.revision, 0);
        assert!(!receiver.disabled);
    }
    #[test]
    fn input_open_requires_a_known_kind_and_valid_identity() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        for (sequence, id, kind) in [
            (2, "a:p1", Some("dialog")),
            (3, "a:p1", None),
            (4, "a:\u{1b}p1", Some("select")),
            (5, "", Some("select")),
        ] {
            assert_eq!(
                receiver.handle(
                    &input_event("a", sequence, "input_open", id, kind),
                    true,
                    Instant::now()
                ),
                b"admission-ignored\n",
                "{id:?} {kind:?}"
            );
        }
        assert!(receiver.open_requests.is_empty());
        assert_eq!(receiver.revision, 0);
        assert!(!receiver.disabled);
    }
    #[test]
    fn a_repeated_open_for_the_same_request_is_ignored_before_any_mutation() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        receiver.open_requests = vec![("a:p1".into(), InputKind::Select)];
        assert_eq!(
            receiver.handle(
                &input_event("a", 2, "input_open", "a:p1", Some("select")),
                true,
                Instant::now()
            ),
            b"admission-ignored\n"
        );
        assert_eq!(ids(&receiver), ["a:p1"]);
        assert_eq!(receiver.revision, 0);
        assert!(!receiver.disabled);
    }
    #[test]
    fn session_start_and_shutdown_publish_the_close_before_forgetting_the_request() {
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        let mut shutdown: serde_json::Value =
            serde_json::from_slice(&event("a", 2, "session_shutdown", None, "none")).unwrap();
        shutdown["payload"]["reason"] = "reload".into();
        // Nothing open: a reload retires the producer and publishes nothing.
        assert_eq!(
            receiver.handle(
                &serde_json::to_vec(&shutdown).unwrap(),
                true,
                Instant::now()
            ),
            b"admission-ignored\n"
        );
        assert!(receiver.producer.is_none() && receiver.open_requests.is_empty());
        assert!(!receiver.disabled);
        // An open request is published closed first; with no lease that publish cannot
        // happen, so the receiver disables itself rather than retiring behind a stale dialog.
        receiver.producer = Some(("a".into(), 2));
        receiver.open_requests = vec![("a:p1".into(), InputKind::Select)];
        shutdown["payload"]["sequence"] = 3.into();
        assert_eq!(
            receiver.handle(
                &serde_json::to_vec(&shutdown).unwrap(),
                true,
                Instant::now()
            ),
            b"admission-unavailable\n"
        );
        assert!(receiver.open_requests.is_empty() && receiver.disabled);
        // The same ordering on the conversation-switch path.
        let mut receiver = unbound();
        receiver.producer = Some(("a".into(), 1));
        receiver.open_requests = vec![("a:p1".into(), InputKind::Select)];
        assert_eq!(
            receiver.handle(
                &event("a", 2, "session_start", None, "none"),
                true,
                Instant::now()
            ),
            b"admission-unavailable\n"
        );
        assert!(receiver.open_requests.is_empty() && receiver.disabled);
    }
    #[test]
    fn a_re_announced_conversation_invalidates_the_open_cycle() {
        // Oh My Pi switches conversations in place on the same instance: the settled event
        // of the cycle left behind must not publish against the new binding.
        let mut receiver = Receiver::new(&omp::HARNESS, None);
        receiver.producer = Some(("a".into(), 1));
        receiver.current = Some((1, "a:1".into()));
        assert_eq!(
            receiver.handle(
                &harness_event(&omp::HARNESS, "a", 2, "session_start", None, "none"),
                true,
                Instant::now()
            ),
            b"admission-unavailable\n",
            "no lease: binding fails after the cycle is cleared"
        );
        assert!(receiver.current.is_none());
    }
}

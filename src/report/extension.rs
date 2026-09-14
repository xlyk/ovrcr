//! One authenticated receiver for every provider whose managed launch loads an OVRCR
//! reporting extension. The vocabulary is Pi's — `session_start`, `agent_start`,
//! `agent_end`, `agent_settled`, `session_shutdown` — and each harness says only who it
//! is and how good the Ready at its settled boundary is. Oh My Pi has no settled event of
//! its own: its extension synthesizes one and this receiver publishes it as Observed,
//! because a stop hook may still continue after a clean end. Everything that is not this
//! vocabulary — the lease, the binding, the revisions, the fences, the pause — belongs to
//! the [`Reporter`](super::reporter::Reporter) each call is handed.
use super::InvocationLease;
use super::reporter::{self, Admission, Cycle, Reporter};
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, AgentProvider, InputKind, InputRequest,
    MAX_INPUT_REQUESTS, SampleQuality, validate_agent_id,
};
use ovrcr_runtime::agent_runner::{HookHandler, private_identifier};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

/// Bounded delivery shared by every provider extension; materialized as its sibling.
pub const TRANSPORT_SOURCE: &str = include_str!("../ovrcr-reporting-transport.mjs");
const TRANSPORT_FILE: &str = "ovrcr-reporting-transport.mjs";
const BINARY_TOKEN: &str = "__OVRCR_BINARY__";

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

/// Admission is a reservation, eligible argv and an interactive terminal — nothing about
/// the executable itself. The launch does not probe `--version`: a non-runnable executable
/// already fails visibly in the native launch, and a bounded probe on a loaded machine
/// times out and disables reporting for an executable that was perfectly fine. Version
/// stays a doctor diagnostic.
pub fn receiver(
    harness: &'static Harness,
    lease: Option<InvocationLease>,
    argv: &mut Vec<OsString>,
) -> HookHandler {
    let mut scratch = None;
    let unavailable = reporter::preflight(&lease, argv, harness.eligible_argv)
        .map(str::to_owned)
        .or_else(|| match materialize_extension(harness) {
            Ok((dir, path)) => {
                scratch = Some(dir);
                argv.insert(1, "-e".into());
                argv.insert(2, path.into_os_string());
                None
            }
            Err(error) => Some(format!("extension not materialized: {error}")),
        });
    let mut reporter = Reporter::new(harness.provider, lease, scratch);
    if let Some(reason) = unavailable {
        reporter.unavailable(harness.display, &reason);
    }
    reporter.handler(Events {
        harness,
        current: None,
        open_requests: Vec::new(),
    })
}

/// One extension's frames. Only the response cycle they open and the Input requests they
/// leave on screen are this adapter's to remember.
struct Events {
    harness: &'static Harness,
    /// The open response cycle: the harness run counter and the identity published as `turn`.
    current: Option<(u64, String)>,
    /// The open Input requests, oldest first, bounded by `MAX_INPUT_REQUESTS`. Every
    /// mutation republishes the whole set, so the runtime never merges deltas. The kind
    /// travels with the id because only the `prompt` namespace's kind comes from the
    /// payload; `approval` and `question` are fixed by their namespace.
    open_requests: Vec<(String, InputKind)>,
}

impl reporter::Frames for Events {
    fn frame(
        &mut self,
        reporter: &mut Reporter,
        input: &[u8],
        native_root: bool,
        deadline: Instant,
    ) -> Vec<u8> {
        if reporter.closed() {
            return reporter::UNAVAILABLE.to_vec();
        }
        if !reporter::own_frame(input, native_root) {
            return reporter::IGNORED.to_vec();
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(input) else {
            return reporter::IGNORED.to_vec();
        };
        if value["provider"] != self.harness.name || value["origin"] != self.harness.origin {
            return reporter::IGNORED.to_vec();
        }
        let payload = &value["payload"];
        if payload["schema"] != 1 || payload["mode"] != "tui" {
            return reporter::IGNORED.to_vec();
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
            return reporter::IGNORED.to_vec();
        };
        // Producer fencing: a new instance is admitted only by its session_start; a stale,
        // replayed, retired or foreign sequence never mutates state, freshness, or Unread.
        let admission = reporter.admit_producer(instance, sequence, event == "session_start");
        if admission == Admission::Ignored {
            return reporter::IGNORED.to_vec();
        }
        // The reattach command is a recovery whatever this receiver's health was: it
        // restarts the producer's own counters, so the cycles that follow it reuse
        // identities this generation has already published and would be dropped as
        // duplicates. A fresh generation is what makes them new responses again.
        let reattach = event == "session_start" && payload["reason"].as_str() == Some("reattach");
        if reporter.paused() || reattach {
            // Nothing is applied while reporting is uncertain, and nothing is inferred from
            // the silence: the next boundary this producer reaches recovers it, once. A
            // boundary that arrives with a hole of its own still recovers — the reattach
            // command drops whatever was queued, and that hole is the pause it is ending.
            if !matches!(event, "session_start" | "agent_start") {
                return reporter::IGNORED.to_vec();
            }
            // One forced rebind of the conversation this boundary names: the supervisor
            // admits a fresh reporting generation while the lease and the session identity
            // hold, and it starts blank — no cycle, no requests, Unread untouched.
            if !reporter.bind(session, deadline, true) {
                reporter.disable();
                return reporter::UNAVAILABLE.to_vec();
            }
            // A fresh generation's snapshot is blank at the server, so the local set goes
            // with it: a request the paused or reattached producer left open is no longer
            // published.
            self.open_requests.clear();
        } else if admission == Admission::Gap {
            return self.pause(reporter, "source_gap", deadline);
        } else if admission == Admission::LostClose {
            return self.pause(reporter, "producer_replaced", deadline);
        } else if event == "session_start"
            && transition_mismatch(payload["previous"].as_str(), reporter.binding())
        {
            // The conversation this transition says it left is not the one this receiver is
            // bound to: a transition happened that it never saw.
            return self.pause(reporter, "transition_mismatch", deadline);
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
                if let Some(closed) = self.retire_requests(reporter, deadline) {
                    return closed;
                }
                if !reporter.bind(session, deadline, false) {
                    reporter.disable();
                    return reporter::UNAVAILABLE.to_vec();
                }
                // A reattach reports what the harness's own API says about the session: a
                // session that is not idle is doing something this reporter did not see, and
                // uncertain is Unknown until a fresh authoritative event. An ordinary
                // announcement carries no `idle` and is Idle by construction.
                let state = if payload["idle"].as_bool().unwrap_or(true) {
                    AgentActivity::Idle
                } else {
                    AgentActivity::Unknown
                };
                (state, SampleQuality::Observed, None)
            }
            "agent_start" => {
                let Some(run) = run else {
                    return reporter::IGNORED.to_vec();
                };
                let turn = format!("{instance}:{run}");
                // A repeated start for the same run is the continuation the harness emits for
                // a retry, automatic compaction or a queued follow-up: the cycle is already
                // open and already published, so there is nothing to add. Duplicates are
                // checked before any capacity, binding, or revision change.
                //
                // A different run while a cycle is open replaces it and publishes the new
                // identity. Permanent disablement is reserved for unresolvable identity or
                // ordering ambiguity (#88); recovering a lost close is #91's job.
                match reporter.admit_cycle(&turn) {
                    Cycle::Known => return reporter::IGNORED.to_vec(),
                    Cycle::Exhausted => return reporter::UNAVAILABLE.to_vec(),
                    Cycle::Fresh => {}
                }
                if !reporter.bind(session, deadline, false) {
                    reporter.disable();
                    return reporter::UNAVAILABLE.to_vec();
                }
                self.current = Some((run, turn.clone()));
                (AgentActivity::Busy, SampleQuality::Observed, Some(turn))
            }
            "agent_end" => {
                // Records the outcome on the producer side; Ready waits for the settled boundary.
                return if self.current.as_ref().is_some_and(|(r, _)| Some(*r) == run) {
                    reporter::ACCEPTED.to_vec()
                } else {
                    reporter::IGNORED.to_vec()
                };
            }
            "agent_settled" => {
                let Some((current_run, turn)) = self.current.clone() else {
                    return reporter::IGNORED.to_vec();
                };
                if Some(current_run) != run {
                    return reporter::IGNORED.to_vec();
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
                    reporter.disable();
                } else {
                    // Replacement or reload: the harness re-runs factories; the next
                    // session_start admits them. The lease and binding are still live here, so
                    // a dialog left open is published closed before the producer retires:
                    // otherwise a re-admitted producer that binds the same conversation reuses
                    // the same snapshot and keeps a stale WaitingInput with nothing on screen.
                    if let Some(closed) = self.retire_requests(reporter, deadline) {
                        return closed;
                    }
                    reporter.retire(instance);
                    self.current = None;
                }
                return reporter::IGNORED.to_vec();
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
                    return reporter::IGNORED.to_vec();
                };
                // The kind of an approval or a question follows from its namespace; only a
                // prompt carries its own, and it must still be one this protocol knows.
                let Some(kind) = namespace.kind(payload["kind"].as_str()) else {
                    return reporter::IGNORED.to_vec();
                };
                return self.open_request(reporter, id, kind, deadline);
            }
            "input_close" => {
                let Some(id) = payload["request_id"].as_str() else {
                    return reporter::IGNORED.to_vec();
                };
                return self.close_request(reporter, id, deadline);
            }
            "cycle_invalidated" => {
                // Tree navigation moved the conversation somewhere this producer cannot
                // vouch for: the cycle it left behind is gone, and the only activity that
                // survives is what the extension can read from its own public API.
                self.current = None;
                let idle = payload["idle"].as_bool().unwrap_or(false);
                let state = if idle {
                    AgentActivity::Idle
                } else {
                    AgentActivity::Unknown
                };
                (state, SampleQuality::Observed, None)
            }
            "unavailable" => {
                // The producer dropped frames of its own: what it reported is no longer
                // complete, but the native session and this lease are both still alive.
                return self.pause(reporter, "source_overflow", deadline);
            }
            _ => return reporter::IGNORED.to_vec(),
        };
        reporter.publish(
            AgentObservation::Activity(ActivitySample {
                state,
                quality,
                turn,
            }),
            deadline,
        )
    }
}

impl Events {
    /// Stops applying events and says so once, keeping the lease, the binding and the
    /// native session. The local cycle and request set go with it: what is on screen is
    /// no longer certain, and the Unavailable health clears the published set too.
    fn pause(
        &mut self,
        reporter: &mut Reporter,
        reason: &'static str,
        deadline: Instant,
    ) -> Vec<u8> {
        self.current = None;
        self.open_requests.clear();
        if reporter.health(Some(reason), deadline) {
            reporter::ACCEPTED.to_vec()
        } else {
            reporter::UNAVAILABLE.to_vec()
        }
    }
    /// Insert-if-absent, then publish the whole set. A repeated open of a known id and the
    /// opening past the bound are both IGNORED before any mutation: overflow drops the new
    /// request, it never disables reporting.
    fn open_request(
        &mut self,
        reporter: &mut Reporter,
        id: &str,
        kind: InputKind,
        deadline: Instant,
    ) -> Vec<u8> {
        if self.open_requests.iter().any(|(open, _)| open == id)
            || self.open_requests.len() >= MAX_INPUT_REQUESTS
        {
            return reporter::IGNORED.to_vec();
        }
        self.open_requests.push((id.to_owned(), kind));
        self.publish_requests(reporter, deadline)
    }
    /// Remove-if-present, then publish the whole set. A close for an id this receiver never
    /// admitted is IGNORED: a late subscriber's unmatched close changes nothing.
    fn close_request(&mut self, reporter: &mut Reporter, id: &str, deadline: Instant) -> Vec<u8> {
        let before = self.open_requests.len();
        self.open_requests.retain(|(open, _)| open != id);
        if self.open_requests.len() == before {
            return reporter::IGNORED.to_vec();
        }
        self.publish_requests(reporter, deadline)
    }
    fn publish_requests(&mut self, reporter: &mut Reporter, deadline: Instant) -> Vec<u8> {
        let requests = self
            .open_requests
            .iter()
            .map(|(id, kind)| InputRequest {
                id: id.clone(),
                kind: *kind,
            })
            .collect();
        reporter.publish(AgentObservation::Input(requests), deadline)
    }
    /// Publishes the empty set for a producer that is about to retire or rebind, while the
    /// binding that owned the requests is still current. `Some` is the caller's early
    /// return: the publish failed and the receiver disabled itself.
    fn retire_requests(&mut self, reporter: &mut Reporter, deadline: Instant) -> Option<Vec<u8>> {
        if self.open_requests.is_empty() {
            return None;
        }
        self.open_requests.clear();
        let closed = self.publish_requests(reporter, deadline);
        (closed.as_slice() == reporter::UNAVAILABLE).then_some(closed)
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

/// Whether a transition names a conversation this receiver is not bound to. A frame with
/// no `previous` carries no expectation (a startup announcement, or a harness with no
/// previous file), and a
/// receiver with no binding has none to contradict.
fn transition_mismatch(
    previous: Option<&str>,
    binding: Option<&ovrcr_protocol::AgentBinding>,
) -> bool {
    matches!((previous, binding), (Some(previous), Some(binding)) if binding.conversation != previous)
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

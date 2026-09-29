//! One authenticated receiver for every provider whose managed launch loads an OVRCR
//! reporting extension. The vocabulary is Pi's — `session_start`, `agent_start`,
//! `agent_end`, `agent_settled`, `model_select`, `session_shutdown` — and each harness
//! says only who it is and how good the Ready at its settled boundary is. Oh My Pi has no
//! settled event of its own: its extension synthesizes one and this receiver publishes it
//! as Observed, because a stop hook may still continue after a clean end. The current
//! model arrives only from the session's own report (`session_start` or `model_select`),
//! never from a launch flag or terminal text, and is published as a Metrics observation
//! for the sidebar. Everything that is not this vocabulary — the lease, the binding, the
//! revisions, the fences, the pause — belongs to the [`Reporter`](super::reporter::Reporter)
//! each call is handed.
use super::InvocationLease;
use super::reporter::{self, Fence, Reporter};
use ovrcr_protocol::{
    ActivitySample, AgentActivity, AgentObservation, AgentProvider, ContextSample, InputKind,
    InputRequest, MAX_INPUT_REQUESTS, Measurement, MetricsSample, SampleQuality, UsageCoverage,
    UsageScope, UsageTotals, validate_agent_id,
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
    // Capture only non-secret launch references, before adding our temporary extension.
    let recovery = (|| {
        let executable = argv.first().map(PathBuf::from)?;
        let executable = if executable.is_absolute() {
            executable
        } else {
            std::env::split_paths(&std::env::var_os("PATH")?)
                .map(|directory| directory.join(&executable))
                .find(|path| path.is_absolute() && path.is_file())?
        };
        Some(ovrcr_protocol::ExtensionConversation {
            conversation: String::new(),
            executable,
            history: None,
            config_dir: ovrcr_runtime::extension_recovery::config_dir(harness.provider).ok()?,
            options: ovrcr_runtime::extension_recovery::launch_options(harness.provider, argv)
                .ok()?,
        })
    })();
    let mut scratch = None;
    let unavailable = reporter::preflight(
        lease.is_some(),
        argv,
        harness.eligible_argv,
        reporter::interactive(),
    )
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
        recovery,
        current: None,
        open_requests: Vec::new(),
        model: None,
    })
}

/// One extension's frames. Only the response cycle they open, the Input requests they
/// leave on screen, and the model they last published are this receiver's to remember.
struct Events {
    harness: &'static Harness,
    recovery: Option<ovrcr_protocol::ExtensionConversation>,
    /// The open response cycle: the harness run counter and the identity published as `turn`.
    current: Option<(u64, String)>,
    /// The open Input requests, oldest first, bounded by `MAX_INPUT_REQUESTS`. Every
    /// mutation republishes the whole set, so the runtime never merges deltas. The kind
    /// travels with the id because only the `prompt` namespace's kind comes from the
    /// payload; `approval` and `question` are fixed by their namespace.
    open_requests: Vec<(String, InputKind)>,
    /// The model last accepted for this binding. Cleared when a bind starts a fresh
    /// generation or reporting pauses, so a later identical report is published again.
    model: Option<String>,
}

impl reporter::Frames for Events {
    fn frame(
        &mut self,
        reporter: &mut Reporter,
        input: &[u8],
        native_root: bool,
        deadline: Instant,
    ) -> Vec<u8> {
        if let Some(answer) = reporter.own_frame(input, native_root) {
            return answer;
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
        let fence = reporter.admit_producer(instance, sequence, event == "session_start");
        if fence == Fence::Ignored {
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
            if let Some(unavailable) = reporter.bind_or_disable(session, deadline, true) {
                return unavailable;
            }
            // A fresh generation's snapshot is blank at the server, so the local set goes
            // with it: a request the paused or reattached producer left open is no longer
            // published, and any previously published model must be reported again.
            self.open_requests.clear();
            self.model = None;
        } else if fence == Fence::Gap {
            return self.pause(reporter, "source_gap", deadline);
        } else if fence == Fence::LostClose {
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
                if let Some(unavailable) = reporter.bind_or_disable(session, deadline, false) {
                    return unavailable;
                }
                // A fresh binding starts without a published model: inventing one from a
                // previous generation or from the launch argv is not this receiver's job.
                self.model = None;
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
                if let Some(answer) = reporter.admit_cycle(&turn) {
                    return answer;
                }
                if let Some(unavailable) = reporter.bind_or_disable(session, deadline, false) {
                    return unavailable;
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
            "model_select" => {
                // Mid-session switch: replace the sidebar model on this same binding. No
                // activity change. An unbound, empty, or identical report invents nothing.
                let model = payload["model"]
                    .as_str()
                    .filter(|value| validate_agent_id(value).is_ok());
                if reporter.binding().is_none() || model.is_none() {
                    return reporter::IGNORED.to_vec();
                }
                return match self.publish_model(reporter, model, deadline) {
                    None => reporter::ACCEPTED.to_vec(),
                    Some(failed) => failed,
                };
            }
            "unavailable" => {
                // The producer dropped frames of its own: what it reported is no longer
                // complete, but the native session and this lease are both still alive.
                return self.pause(reporter, "source_overflow", deadline);
            }
            _ => return reporter::IGNORED.to_vec(),
        };
        // Recovery metadata must not suppress the provider's activity when the
        // durable store fails. The helper still receives no durable acknowledgment.
        let activity = reporter.publish(
            AgentObservation::Activity(ActivitySample {
                state,
                quality,
                turn,
            }),
            deadline,
        );
        if activity != reporter::ACCEPTED {
            return activity;
        }
        if matches!(event, "session_start" | "agent_start")
            && let Some(mut reference) = self.recovery.clone()
        {
            reference.conversation = session.to_owned();
            reference.history = payload["session_file"].as_str().map(PathBuf::from);
            let reference = match self.harness.provider {
                AgentProvider::Pi => ovrcr_protocol::ConversationReference::Pi(reference),
                AgentProvider::Omp => ovrcr_protocol::ConversationReference::Omp(reference),
                _ => unreachable!("only Pi and OMP use the extension receiver"),
            };
            if !reporter.retain_conversation(reference, deadline) {
                return reporter::UNAVAILABLE.to_vec();
            }
        }
        // The session's own model report on announce (startup, switch, reattach). Absent
        // when the harness has not selected one yet: the sidebar stays the agent name.
        if event == "session_start"
            && let Some(failed) = self.publish_model(reporter, payload["model"].as_str(), deadline)
        {
            return failed;
        }
        activity
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
        self.model = None;
        if reporter.health(Some(reason), deadline) {
            reporter::ACCEPTED.to_vec()
        } else {
            reporter::UNAVAILABLE.to_vec()
        }
    }
    /// Publish the session-reported model as a Metrics observation. `None` means nothing
    /// changed (missing, invalid, or identical); `Some` is a failed publish the caller
    /// must return. Never invents a model when the session has not reported one.
    fn publish_model(
        &mut self,
        reporter: &mut Reporter,
        model: Option<&str>,
        deadline: Instant,
    ) -> Option<Vec<u8>> {
        let model = model.filter(|value| validate_agent_id(value).is_ok())?;
        if self.model.as_deref() == Some(model) {
            return None;
        }
        let source = self.harness.origin.to_owned();
        let sample = MetricsSample {
            model: Some(model.to_owned()),
            context: Measurement {
                value: ContextSample {
                    used_tokens: None,
                    capacity_tokens: None,
                    quality: SampleQuality::Observed,
                },
                source: source.clone(),
            },
            usage: Measurement {
                value: UsageTotals {
                    scope: UsageScope::Conversation,
                    coverage: UsageCoverage::Partial,
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    reasoning_output_tokens: None,
                },
                source: source.clone(),
            },
            cost: Measurement {
                value: None,
                source,
            },
        };
        let published = reporter.publish(AgentObservation::Metrics(Box::new(sample)), deadline);
        if published == reporter::ACCEPTED {
            self.model = Some(model.to_owned());
            None
        } else {
            Some(published)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::reporter::Frames;
    use crate::report::reporter::scripted::{Observed, Supervisor};
    use crate::report::{omp, pi};

    fn start(harness: &'static Harness) -> (Events, Reporter, Supervisor) {
        let (reporter, supervisor) = Supervisor::reporter(harness.provider);
        (
            Events {
                harness,
                recovery: None,
                current: None,
                open_requests: Vec::new(),
                model: None,
            },
            reporter,
            supervisor,
        )
    }
    fn managed() -> (Events, Reporter, Supervisor) {
        start(&pi::HARNESS)
    }
    fn deadline() -> Instant {
        Instant::now() + std::time::Duration::from_secs(10)
    }
    fn drive(events: &mut Events, reporter: &mut Reporter, input: &[u8]) -> Vec<u8> {
        events.frame(reporter, input, true, deadline())
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
    fn with(bytes: &[u8], field: &str, value: serde_json::Value) -> Vec<u8> {
        let mut payload: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        payload["payload"][field] = value;
        serde_json::to_vec(&payload).unwrap()
    }
    fn states(observed: &[Observed]) -> Vec<(u64, AgentActivity, Option<String>, SampleQuality)> {
        Supervisor::activity(observed)
            .into_iter()
            .map(|(generation, _, state, turn, quality)| (generation, state, turn, quality))
            .collect()
    }
    fn ids(events: &Events) -> Vec<&str> {
        events
            .open_requests
            .iter()
            .map(|(id, _)| id.as_str())
            .collect()
    }

    #[test]
    fn unknown_producer_is_admitted_only_by_session_start() {
        let (mut events, mut reporter, supervisor) = managed();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 1, "agent_start", Some(1), "none")
            ),
            reporter::IGNORED
        );
        assert!(
            reporter.binding().is_none(),
            "an unannounced instance never reaches the binding"
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 2, "session_start", None, "none")
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            states(&supervisor.observed()),
            vec![(1, AgentActivity::Idle, None, SampleQuality::Observed)]
        );
    }

    #[test]
    fn stale_and_replayed_sequences_are_ignored_before_any_mutation() {
        let (mut events, mut reporter, supervisor) = managed();
        drive(
            &mut events,
            &mut reporter,
            &event("a", 5, "session_start", None, "none"),
        );
        for sequence in [5, 4, 1] {
            assert_eq!(
                drive(
                    &mut events,
                    &mut reporter,
                    &event("a", sequence, "agent_end", Some(1), "ok")
                ),
                reporter::IGNORED
            );
        }
        // The next sequence in order still applies: nothing advanced the fence.
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 6, "agent_start", Some(1), "none")
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            states(&supervisor.observed()),
            vec![
                (1, AgentActivity::Idle, None, SampleQuality::Observed),
                (
                    1,
                    AgentActivity::Busy,
                    Some("a:1".into()),
                    SampleQuality::Observed
                ),
            ]
        );
    }

    #[test]
    fn foreign_provider_non_tui_and_grandchild_payloads_are_ignored() {
        let (mut events, mut reporter, supervisor) = managed();
        drive(
            &mut events,
            &mut reporter,
            &event("a", 1, "session_start", None, "none"),
        );
        let rpc = with(
            &event("a", 2, "agent_start", Some(1), "none"),
            "mode",
            "rpc".into(),
        );
        assert_eq!(drive(&mut events, &mut reporter, &rpc), reporter::IGNORED);
        let codex =
            br#"{"provider":"codex","origin":"codex-hook","payload":{"hook_event_name":"Stop"}}"#;
        assert_eq!(drive(&mut events, &mut reporter, codex), reporter::IGNORED);
        assert_eq!(
            events.frame(
                &mut reporter,
                &event("a", 3, "agent_start", Some(1), "none"),
                false,
                deadline()
            ),
            reporter::IGNORED,
            "a frame that is not the native root's is never this receiver's"
        );
        // Sequence 2 still applies, so no ignored payload advanced the fence.
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 2, "agent_start", Some(1), "none")
            ),
            reporter::ACCEPTED
        );
        assert_eq!(states(&supervisor.observed()).len(), 2);
    }

    #[test]
    fn each_harness_ignores_the_other_harness_envelope() {
        // Oh My Pi cannot be labelled Pi to bypass the provider check, and the reverse.
        for (harness, foreign) in [(&pi::HARNESS, &omp::HARNESS), (&omp::HARNESS, &pi::HARNESS)] {
            let (mut events, mut reporter, supervisor) = start(harness);
            drive(
                &mut events,
                &mut reporter,
                &harness_event(harness, "a", 1, "session_start", None, "none"),
            );
            assert_eq!(
                drive(
                    &mut events,
                    &mut reporter,
                    &harness_event(foreign, "a", 2, "agent_start", Some(1), "none")
                ),
                reporter::IGNORED,
                "{} accepted {}",
                harness.name,
                foreign.name
            );
            // Its own envelope at the same sequence still reaches the state machine.
            assert_eq!(
                drive(
                    &mut events,
                    &mut reporter,
                    &harness_event(harness, "a", 2, "agent_start", Some(1), "none")
                ),
                reporter::ACCEPTED
            );
            assert_eq!(states(&supervisor.observed()).len(), 2);
        }
    }

    #[test]
    fn a_settled_or_repeated_run_that_is_not_the_open_cycle_publishes_nothing() {
        let (mut events, mut reporter, supervisor) = managed();
        drive(
            &mut events,
            &mut reporter,
            &event("a", 1, "session_start", None, "none"),
        );
        drive(
            &mut events,
            &mut reporter,
            &event("a", 2, "agent_start", Some(2), "none"),
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 3, "agent_settled", Some(1), "ok")
            ),
            reporter::IGNORED
        );
        assert_eq!(events.current, Some((2, "a:2".into())));
        // A repeated start for the open run is the continuation a retry, an automatic
        // compaction or a queued follow-up emits: already open, already published.
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 4, "agent_start", Some(2), "none")
            ),
            reporter::IGNORED
        );
        assert_eq!(events.current, Some((2, "a:2".into())));
        assert_eq!(
            states(&supervisor.observed()),
            vec![
                (1, AgentActivity::Idle, None, SampleQuality::Observed),
                (
                    1,
                    AgentActivity::Busy,
                    Some("a:2".into()),
                    SampleQuality::Observed
                ),
            ]
        );
    }

    #[test]
    fn a_reload_retires_the_producer_it_observed_shutting_down() {
        let (mut events, mut reporter, supervisor) = managed();
        drive(
            &mut events,
            &mut reporter,
            &event("a", 1, "session_start", None, "none"),
        );
        drive(
            &mut events,
            &mut reporter,
            &event("a", 2, "agent_start", Some(1), "none"),
        );
        let reload = with(
            &event("a", 3, "session_shutdown", None, "none"),
            "reason",
            "reload".into(),
        );
        assert_eq!(
            drive(&mut events, &mut reporter, &reload),
            reporter::IGNORED
        );
        assert!(events.current.is_none() && !reporter.closed());
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 4, "session_start", None, "none")
            ),
            reporter::IGNORED,
            "the retired producer cannot re-admit itself"
        );
        // The successor drives the quit path, which ends reporting for good.
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("b", 1, "session_start", None, "none")
            ),
            reporter::ACCEPTED
        );
        let quit = with(
            &event("b", 2, "session_shutdown", None, "none"),
            "reason",
            "quit".into(),
        );
        assert_eq!(drive(&mut events, &mut reporter, &quit), reporter::IGNORED);
        assert!(reporter.closed());
        assert_eq!(states(&supervisor.observed()).len(), 3);
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
    /// A receiver whose producer is admitted and whose binding is live.
    fn announced() -> (Events, Reporter, Supervisor) {
        let (mut events, mut reporter, supervisor) = managed();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 1, "session_start", None, "none")
            ),
            reporter::ACCEPTED
        );
        (events, reporter, supervisor)
    }

    #[test]
    fn the_request_set_keeps_its_order_and_removes_only_the_closed_member() {
        let (mut events, mut reporter, supervisor) = announced();
        for (sequence, namespace, id, kind) in [
            (2, "approval", "approval:c1", Some("approval")),
            (3, "question", "question:q1", Some("select")),
        ] {
            assert_eq!(
                drive(
                    &mut events,
                    &mut reporter,
                    &namespaced_event("a", sequence, "input_open", namespace, id, kind)
                ),
                reporter::ACCEPTED
            );
        }
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &namespaced_event(
                    "a",
                    4,
                    "input_open",
                    "approval",
                    "approval:c1",
                    Some("approval")
                )
            ),
            reporter::IGNORED,
            "a repeated open of a known id never mutates the set"
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &input_event("a", 5, "input_close", "question:zz", None)
            ),
            reporter::IGNORED,
            "an unknown close never mutates the set"
        );
        assert_eq!(ids(&events), ["approval:c1", "question:q1"]);
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &input_event("a", 6, "input_close", "approval:c1", None)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(ids(&events), ["question:q1"]);
        assert!(!reporter.closed());
        // The set is published whole every time, so the runtime never merges deltas.
        assert_eq!(
            Supervisor::inputs(&supervisor.observed()),
            vec![
                vec![("approval:c1".to_owned(), InputKind::Approval)],
                vec![
                    ("approval:c1".to_owned(), InputKind::Approval),
                    ("question:q1".to_owned(), InputKind::Select)
                ],
                vec![("question:q1".to_owned(), InputKind::Select)],
            ]
        );
    }

    #[test]
    fn the_thirty_third_open_is_ignored_without_disabling() {
        let (mut events, mut reporter, supervisor) = announced();
        events.open_requests = (0..MAX_INPUT_REQUESTS)
            .map(|n| (format!("approval:c{n}"), InputKind::Approval))
            .collect();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &namespaced_event(
                    "a",
                    2,
                    "input_open",
                    "approval",
                    "approval:overflow",
                    Some("approval")
                )
            ),
            reporter::IGNORED
        );
        assert_eq!(events.open_requests.len(), MAX_INPUT_REQUESTS);
        assert!(
            !reporter.closed(),
            "the bound drops the new request, it never disables reporting"
        );
        assert!(Supervisor::inputs(&supervisor.observed()).is_empty());
    }

    #[test]
    fn an_open_whose_namespace_kind_or_identity_is_wrong_is_ignored() {
        let (mut events, mut reporter, supervisor) = announced();
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
                drive(
                    &mut events,
                    &mut reporter,
                    &namespaced_event("a", sequence, "input_open", namespace, id, kind)
                ),
                reporter::IGNORED,
                "{namespace} / {id}"
            );
        }
        for (sequence, id, kind) in [
            (7, "a:p1", Some("dialog")),
            (8, "a:p1", None),
            (9, "a:\u{1b}p1", Some("select")),
            (10, "", Some("select")),
        ] {
            assert_eq!(
                drive(
                    &mut events,
                    &mut reporter,
                    &input_event("a", sequence, "input_open", id, kind)
                ),
                reporter::IGNORED,
                "{id:?} {kind:?}"
            );
        }
        assert!(events.open_requests.is_empty() && !reporter.closed());
        assert!(Supervisor::inputs(&supervisor.observed()).is_empty());
    }

    #[test]
    fn an_approval_or_question_namespace_fixes_the_kind_it_publishes() {
        // The kind follows the namespace, so a mislabelled payload cannot make an approval
        // look like a select in the Dashboard.
        let (mut events, mut reporter, supervisor) = announced();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &namespaced_event(
                    "a",
                    2,
                    "input_open",
                    "approval",
                    "approval:c1",
                    Some("select")
                )
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            Supervisor::inputs(&supervisor.observed()),
            vec![vec![("approval:c1".to_owned(), InputKind::Approval)]]
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
        let (mut events, mut reporter, supervisor) = announced();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &input_event("a", 2, "input_close", "a:p1", None)
            ),
            reporter::IGNORED,
            "no request is open"
        );
        assert!(events.open_requests.is_empty());
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &input_event("a", 3, "input_open", "a:p1", Some("select"))
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &input_event("a", 4, "input_close", "a:p2", None)
            ),
            reporter::IGNORED,
            "a close only closes its own request"
        );
        assert_eq!(ids(&events), ["a:p1"]);
        assert!(!reporter.closed());
        assert_eq!(
            Supervisor::inputs(&supervisor.observed()),
            vec![vec![("a:p1".to_owned(), InputKind::Select)]]
        );
    }

    #[test]
    fn session_start_and_shutdown_publish_the_close_before_forgetting_the_request() {
        let (mut events, mut reporter, supervisor) = announced();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &input_event("a", 2, "input_open", "a:p1", Some("select"))
            ),
            reporter::ACCEPTED
        );
        // The conversation-switch path publishes the close against the binding that
        // owned it, before the rebind.
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 3, "session_start", None, "none")
            ),
            reporter::ACCEPTED
        );
        assert!(events.open_requests.is_empty());
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &input_event("a", 4, "input_open", "a:p2", Some("select"))
            ),
            reporter::ACCEPTED
        );
        // A reload retires the producer, and the dialog it left is published closed
        // first: otherwise a re-admitted producer keeps a stale WaitingInput on screen.
        let reload = with(
            &event("a", 5, "session_shutdown", None, "none"),
            "reason",
            "reload".into(),
        );
        assert_eq!(
            drive(&mut events, &mut reporter, &reload),
            reporter::IGNORED
        );
        assert!(events.open_requests.is_empty() && !reporter.closed());
        assert_eq!(
            Supervisor::inputs(&supervisor.observed()),
            vec![
                vec![("a:p1".to_owned(), InputKind::Select)],
                Vec::new(),
                vec![("a:p2".to_owned(), InputKind::Select)],
                Vec::new(),
            ]
        );
    }

    #[test]
    fn a_gap_pauses_before_the_event_is_applied_and_only_a_boundary_recovers_it() {
        let (mut events, mut reporter, supervisor) = announced();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &input_event("a", 2, "input_open", "a:p1", Some("select"))
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 4, "agent_start", Some(1), "none")
            ),
            reporter::ACCEPTED,
            "the hole is reported, the event it carried is not applied"
        );
        assert!(reporter.paused());
        assert!(
            events.current.is_none() && events.open_requests.is_empty(),
            "the gapped event never opened a cycle, and what was on screen is no longer certain"
        );
        // Nothing else applies while reporting is uncertain, and nothing is inferred
        // from the silence.
        for (sequence, name, run) in [
            (5, "agent_settled", Some(1)),
            (6, "agent_end", Some(1)),
            (7, "session_shutdown", None),
        ] {
            assert_eq!(
                drive(
                    &mut events,
                    &mut reporter,
                    &event("a", sequence, name, run, "ok")
                ),
                reporter::IGNORED,
                "{name} while paused"
            );
        }
        assert!(reporter.paused() && !reporter.closed());
        // A trustworthy boundary recovers, once, as a fresh Reporting generation.
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 8, "agent_start", Some(1), "none")
            ),
            reporter::ACCEPTED
        );
        assert!(!reporter.paused());
        assert_eq!(events.current, Some((1, "a:1".into())));
        let observed = supervisor.observed();
        assert_eq!(
            Supervisor::health(&observed),
            vec![(
                ovrcr_protocol::ReporterHealth::Unavailable,
                "source_gap".to_owned()
            )]
        );
        assert_eq!(
            states(&observed),
            vec![
                (1, AgentActivity::Idle, None, SampleQuality::Observed),
                (
                    2,
                    AgentActivity::Busy,
                    Some("a:1".into()),
                    SampleQuality::Observed
                ),
            ]
        );
    }

    #[test]
    fn an_overflow_notice_pauses_instead_of_disabling() {
        let (mut events, mut reporter, supervisor) = announced();
        let notice = with(
            &event("a", 2, "unavailable", None, "none"),
            "reason",
            "overflow".into(),
        );
        assert_eq!(
            drive(&mut events, &mut reporter, &notice),
            reporter::ACCEPTED
        );
        assert!(reporter.paused() && !reporter.closed());
        assert_eq!(
            Supervisor::health(&supervisor.observed()),
            vec![(
                ovrcr_protocol::ReporterHealth::Unavailable,
                "source_overflow".to_owned()
            )]
        );
    }

    #[test]
    fn a_transition_this_receiver_never_saw_pauses_instead_of_rebinding() {
        let (mut events, mut reporter, supervisor) = announced();
        let elsewhere = with(
            &event("a", 2, "session_start", None, "none"),
            "previous",
            "sess-b".into(),
        );
        assert_eq!(
            drive(&mut events, &mut reporter, &elsewhere),
            reporter::ACCEPTED
        );
        assert!(reporter.paused());
        assert_eq!(
            Supervisor::health(&supervisor.observed()),
            vec![(
                ovrcr_protocol::ReporterHealth::Unavailable,
                "transition_mismatch".to_owned()
            )]
        );
    }

    #[test]
    fn an_unobserved_producer_replacement_pauses_and_retires_what_it_replaced() {
        let (mut events, mut reporter, supervisor) = announced();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("b", 1, "session_start", None, "none")
            ),
            reporter::ACCEPTED,
            "the replacement is admitted; what it replaced was never observed closing"
        );
        assert!(reporter.paused());
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 9, "agent_start", Some(1), "none")
            ),
            reporter::IGNORED,
            "every delayed frame of the retired instance is ignored"
        );
        assert_eq!(
            Supervisor::health(&supervisor.observed()),
            vec![(
                ovrcr_protocol::ReporterHealth::Unavailable,
                "producer_replaced".to_owned()
            )]
        );
    }

    #[test]
    fn cycle_invalidated_forgets_the_response_cycle() {
        let (mut events, mut reporter, supervisor) = announced();
        drive(
            &mut events,
            &mut reporter,
            &event("a", 2, "agent_start", Some(1), "none"),
        );
        let tree = with(
            &event("a", 3, "cycle_invalidated", None, "none"),
            "idle",
            true.into(),
        );
        assert_eq!(drive(&mut events, &mut reporter, &tree), reporter::ACCEPTED);
        assert!(
            events.current.is_none(),
            "tree navigation invalidates the cycle it left behind"
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 4, "agent_settled", Some(1), "ok")
            ),
            reporter::IGNORED,
            "the settled boundary of a forgotten cycle is not a Ready"
        );
        assert_eq!(
            states(&supervisor.observed()),
            vec![
                (1, AgentActivity::Idle, None, SampleQuality::Observed),
                (
                    1,
                    AgentActivity::Busy,
                    Some("a:1".into()),
                    SampleQuality::Observed
                ),
                (1, AgentActivity::Idle, None, SampleQuality::Observed),
            ]
        );
    }

    #[test]
    fn a_re_announced_conversation_invalidates_the_open_cycle() {
        // Oh My Pi switches conversations in place on the same instance: the settled event
        // of the cycle left behind must not publish against the new binding.
        let (mut events, mut reporter, supervisor) = start(&omp::HARNESS);
        let frame =
            |sequence, name, run| harness_event(&omp::HARNESS, "a", sequence, name, run, "ok");
        drive(&mut events, &mut reporter, &frame(1, "session_start", None));
        drive(
            &mut events,
            &mut reporter,
            &frame(2, "agent_start", Some(1)),
        );
        assert_eq!(
            drive(&mut events, &mut reporter, &frame(3, "session_start", None)),
            reporter::ACCEPTED
        );
        assert!(events.current.is_none());
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &frame(4, "agent_settled", Some(1))
            ),
            reporter::IGNORED
        );
        assert_eq!(states(&supervisor.observed()).len(), 3);
    }

    #[test]
    fn a_recovering_continuation_publishes_its_cycle_on_the_fresh_generation() {
        // Pi emits `agent_start` again for a retry, an automatic compaction or a queued
        // follow-up, with the same run: the cycle identity of the generation that paused
        // must not silence the frame that recovered it, or the Dashboard would sit at
        // Unknown while Pi works and the response would never become Ready.
        let (mut events, mut reporter, supervisor) = announced();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 2, "agent_start", Some(1), "none")
            ),
            reporter::ACCEPTED
        );
        assert_eq!(events.current, Some((1, "a:1".into())));
        // A hole in the source sequence pauses; the cycle is forgotten locally.
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 5, "agent_end", Some(1), "ok")
            ),
            reporter::ACCEPTED
        );
        assert!(reporter.paused() && events.current.is_none());
        for (sequence, name, outcome) in [
            (6, "agent_start", "none"),
            (7, "agent_end", "ok"),
            (8, "agent_settled", "ok"),
        ] {
            assert_eq!(
                drive(
                    &mut events,
                    &mut reporter,
                    &event("a", sequence, name, Some(1), outcome)
                ),
                reporter::ACCEPTED,
                "{name} after the recovery"
            );
        }
        let observed = supervisor.observed();
        assert_eq!(
            Supervisor::activity(&observed)
                .into_iter()
                .map(|(generation, revision, state, turn, _)| (generation, revision, state, turn))
                .collect::<Vec<_>>(),
            vec![
                (1, 1, AgentActivity::Idle, None),
                (1, 2, AgentActivity::Busy, Some("a:1".into())),
                (2, 1, AgentActivity::Busy, Some("a:1".into())),
                (2, 2, AgentActivity::ResponseReady, Some("a:1".into())),
            ]
        );
        // The recovery names the binding it is replacing, so a generation the supervisor
        // granted to someone else is never overwritten by a reporter that never saw it.
        assert_eq!(
            observed
                .iter()
                .filter(|entry| matches!(entry, Observed::Bind { .. }))
                .collect::<Vec<_>>(),
            vec![
                &Observed::Bind {
                    conversation: "sess-a".to_owned(),
                    generation: 1,
                    expected: None,
                },
                &Observed::Bind {
                    conversation: "sess-a".to_owned(),
                    generation: 2,
                    expected: Some(ovrcr_protocol::AgentBinding {
                        provider: AgentProvider::Pi,
                        invocation: "inv".into(),
                        conversation: "sess-a".into(),
                        generation: 1,
                    }),
                },
            ]
        );
    }

    #[test]
    fn a_reattach_on_a_healthy_receiver_is_a_fresh_generation_that_keeps_its_fences() {
        // The reattach command restarts the producer's run counter, so without a fresh
        // generation the cycles that follow reuse identities this receiver has already
        // published and are dropped. The command is a recovery whatever the health was.
        let (mut events, mut reporter, supervisor) = managed();
        // An earlier instance shuts down where this receiver can see it, earning a fence
        // the recovery must keep, and its successor takes over.
        drive(
            &mut events,
            &mut reporter,
            &event("z", 1, "session_start", None, "none"),
        );
        let retired = with(
            &event("z", 2, "session_shutdown", None, "none"),
            "reason",
            "reload".into(),
        );
        assert_eq!(
            drive(&mut events, &mut reporter, &retired),
            reporter::IGNORED
        );
        drive(
            &mut events,
            &mut reporter,
            &event("a", 1, "session_start", None, "none"),
        );
        drive(
            &mut events,
            &mut reporter,
            &event("a", 2, "agent_start", Some(1), "none"),
        );
        let mut reattach = with(
            &event("a", 3, "session_start", None, "none"),
            "reason",
            "reattach".into(),
        );
        reattach = with(&reattach, "idle", false.into());
        assert_eq!(
            drive(&mut events, &mut reporter, &reattach),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("z", 3, "session_start", None, "none")
            ),
            reporter::IGNORED,
            "a retired producer stays retired across a recovery"
        );
        // The cycle numbering starts over with the producer's counters.
        for (sequence, name, outcome) in [
            (4, "agent_start", "none"),
            (5, "agent_end", "ok"),
            (6, "agent_settled", "ok"),
        ] {
            assert_eq!(
                drive(
                    &mut events,
                    &mut reporter,
                    &event("a", sequence, name, Some(1), outcome)
                ),
                reporter::ACCEPTED,
                "{name} after a reattach"
            );
        }
        assert_eq!(
            states(&supervisor.observed()),
            vec![
                (1, AgentActivity::Idle, None, SampleQuality::Observed),
                (1, AgentActivity::Idle, None, SampleQuality::Observed),
                (
                    1,
                    AgentActivity::Busy,
                    Some("a:1".into()),
                    SampleQuality::Observed
                ),
                // The reattach frame says Pi is not idle: what it is doing is unknown until
                // a fresh authoritative event, never Idle and never the response before it.
                (2, AgentActivity::Unknown, None, SampleQuality::Observed),
                (
                    2,
                    AgentActivity::Busy,
                    Some("a:1".into()),
                    SampleQuality::Observed
                ),
                (
                    2,
                    AgentActivity::ResponseReady,
                    Some("a:1".into()),
                    SampleQuality::Confirmed
                ),
            ]
        );
    }

    #[test]
    fn the_settled_quality_is_the_harness_claim_about_its_own_boundary() {
        // Pi's `agent_settled` fires once the whole prompt is finished; Oh My Pi has no
        // settled event of its own, so a stop hook may still continue after a clean end.
        for (harness, expected) in [
            (&pi::HARNESS, SampleQuality::Confirmed),
            (&omp::HARNESS, SampleQuality::Observed),
        ] {
            let (mut events, mut reporter, supervisor) = start(harness);
            for (sequence, name, run) in [
                (1, "session_start", None),
                (2, "agent_start", Some(1)),
                (3, "agent_settled", Some(1)),
            ] {
                drive(
                    &mut events,
                    &mut reporter,
                    &harness_event(harness, "a", sequence, name, run, "ok"),
                );
            }
            let settled = states(&supervisor.observed());
            assert_eq!(
                settled.last(),
                Some(&(
                    1,
                    AgentActivity::ResponseReady,
                    Some("a:1".into()),
                    expected
                )),
                "{}",
                harness.name
            );
        }
    }

    #[test]
    fn session_start_and_model_select_publish_the_reported_model_for_both_harnesses() {
        for harness in [&pi::HARNESS, &omp::HARNESS] {
            let (mut events, mut reporter, supervisor) = start(harness);
            let start = with(
                &harness_event(harness, "a", 1, "session_start", None, "none"),
                "model",
                "grok-4.7".into(),
            );
            assert_eq!(
                drive(&mut events, &mut reporter, &start),
                reporter::ACCEPTED,
                "{} session_start",
                harness.name
            );
            let switched = with(
                &harness_event(harness, "a", 2, "model_select", None, "none"),
                "model",
                "claude-sonnet-4".into(),
            );
            assert_eq!(
                drive(&mut events, &mut reporter, &switched),
                reporter::ACCEPTED,
                "{} model_select",
                harness.name
            );
            // An identical model on a fresh sequence is accepted but does not republish.
            let same = with(
                &harness_event(harness, "a", 3, "model_select", None, "none"),
                "model",
                "claude-sonnet-4".into(),
            );
            assert_eq!(drive(&mut events, &mut reporter, &same), reporter::ACCEPTED);
            assert_eq!(
                Supervisor::models(&supervisor.observed()),
                vec![Some("grok-4.7".into()), Some("claude-sonnet-4".into())],
                "{} mid-session switch replaces the previous model; identical is not republished",
                harness.name
            );
        }
    }

    #[test]
    fn no_model_is_invented_when_the_session_has_not_reported_one() {
        let (mut events, mut reporter, supervisor) = managed();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 1, "session_start", None, "none")
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &event("a", 2, "model_select", None, "none")
            ),
            reporter::IGNORED,
            "model_select without a model invents nothing"
        );
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &with(
                    &event("a", 3, "model_select", None, "none"),
                    "model",
                    "".into()
                )
            ),
            reporter::IGNORED,
            "empty model invents nothing"
        );
        assert!(
            Supervisor::models(&supervisor.observed()).is_empty(),
            "session_start without a model leaves Metrics unpublished"
        );
    }

    #[test]
    fn model_select_before_a_binding_is_ignored() {
        let (mut events, mut reporter, supervisor) = managed();
        assert_eq!(
            drive(
                &mut events,
                &mut reporter,
                &with(
                    &event("a", 1, "model_select", None, "none"),
                    "model",
                    "grok-4.7".into()
                )
            ),
            reporter::IGNORED,
            "an unannounced producer never reaches the binding"
        );
        assert!(reporter.binding().is_none());
        assert!(Supervisor::models(&supervisor.observed()).is_empty());
    }

    #[test]
    fn a_transition_names_the_conversation_it_left() {
        let binding = |conversation: &str| ovrcr_protocol::AgentBinding {
            provider: AgentProvider::Pi,
            invocation: "inv".into(),
            conversation: conversation.into(),
            generation: 1,
        };
        assert!(
            !transition_mismatch(None, Some(&binding("sess-a"))),
            "no previous is no expectation"
        );
        assert!(
            !transition_mismatch(Some("sess-a"), None),
            "nothing is bound yet"
        );
        assert!(!transition_mismatch(
            Some("sess-a"),
            Some(&binding("sess-a"))
        ));
        assert!(
            transition_mismatch(Some("sess-b"), Some(&binding("sess-a"))),
            "the conversation being left is not the one this receiver is bound to"
        );
    }
}

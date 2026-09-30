//! One scripted supervisor for every reporter-lifecycle test: the paired supervisor
//! connection and a real socket for publications and receipt re-reads, answering in the
//! order a synchronous reporter drives them.
//!
//! It mirrors the checks `ovrcr_runtime::session::reporting` makes on the same requests,
//! so a test cannot pass against a fixture the real server would have rejected:
//!
//! - a Bind whose `expected_binding` is not the binding in force is refused, and a
//!   granted one takes the next Reporting generation and starts every revision at zero;
//! - a publication, a health command and a finalize are all rejected when their binding
//!   is not the one in force or their revision does not advance that observation kind;
//! - a Release checks `expected_binding` too, and afterwards only the retained receipt
//!   is still answerable;
//! - exactly one operation receipt is retained, as `latest` is, so an `AgentStatus` for
//!   any earlier operation is stale rather than replayed.
//!
//! It does not model the reservation epoch, the capability, the measurement watermarks
//! or Ready and Unread; those belong to the runtime's own tests.
//!
//! `Answer::Withhold` performs the request and never replies, which is what a lost
//! receipt is; `Answer::Refuse` rejects it, changing nothing and leaving no receipt. The
//! script is consumed in the order requests arrive, counting every request including the
//! `SupervisorHello` that opens a receipt re-read.
use super::Reporter;
use crate::report::InvocationLease;
use ovrcr_protocol::{
    AgentBinding, AgentCommand, AgentOperationResult, AgentProvider, AgentSecret, AgentUpdate,
    ClientMessage, ErrorCode, ProviderReport, Request, Response, ServerMessage, SessionId,
    SupervisorAuth, exchange_preamble, read_frame, write_frame,
};
use std::{
    os::fd::AsRawFd,
    os::unix::net::{UnixListener, UnixStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    /// Answer the way the server would.
    Auto,
    /// Perform the request and never reply: the receipt is lost in transit.
    Withhold,
    /// Reject the request, and reject its receipt re-read the same way.
    Refuse,
}

/// What the supervisor was asked to do, in order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Observed {
    Hello,
    Bind {
        conversation: String,
        generation: u64,
        expected: Option<AgentBinding>,
    },
    Status(String),
    Report(ProviderReport),
    Health(ProviderReport),
    Finalize(ProviderReport),
    Release,
    Retain {
        binding: AgentBinding,
        reference: ovrcr_protocol::ConversationReference,
    },
}

pub(crate) struct Supervisor {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Vec<Observed>>>,
    _root: tempfile::TempDir,
}

impl Supervisor {
    /// A reporter whose supervisor answers everything the way the server would.
    pub(crate) fn reporter(provider: AgentProvider) -> (Reporter, Supervisor) {
        Self::scripted(provider, Vec::new())
    }

    pub(crate) fn scripted(provider: AgentProvider, script: Vec<Answer>) -> (Reporter, Supervisor) {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("agent.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (client, server) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let auth = SupervisorAuth {
            session: SessionId(1),
            lease: AgentSecret([7; 32]),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread =
            std::thread::spawn(move || serve(listener, server, provider, script, thread_stop));
        let lease = InvocationLease {
            stream: client,
            auth,
            socket,
            binding: None,
            next_request: 1,
            capability: [0; 32],
        };
        (
            Reporter::new(provider, Some(lease), None),
            Supervisor {
                stop,
                thread: Some(thread),
                _root: root,
            },
        )
    }

    /// Everything the supervisor was asked to do, in order.
    pub(crate) fn observed(mut self) -> Vec<Observed> {
        self.stop.store(true, Ordering::Release);
        self.thread.take().unwrap().join().unwrap()
    }

    /// The activity samples that reached the server: generation, revision, state, turn.
    pub(crate) fn activity(
        observed: &[Observed],
    ) -> Vec<(
        u64,
        u64,
        ovrcr_protocol::AgentActivity,
        Option<String>,
        ovrcr_protocol::SampleQuality,
    )> {
        observed
            .iter()
            .filter_map(|entry| match entry {
                Observed::Report(report) => match &report.observation {
                    ovrcr_protocol::AgentObservation::Activity(sample) => Some((
                        report.binding.generation,
                        report.revision,
                        sample.state,
                        sample.turn.clone(),
                        sample.quality,
                    )),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The published Input request sets, in order.
    pub(crate) fn inputs(observed: &[Observed]) -> Vec<Vec<(String, ovrcr_protocol::InputKind)>> {
        observed
            .iter()
            .filter_map(|entry| match entry {
                Observed::Report(report) => match &report.observation {
                    ovrcr_protocol::AgentObservation::Input(requests) => Some(
                        requests
                            .iter()
                            .map(|request| (request.id.clone(), request.kind))
                            .collect(),
                    ),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The model field of each published Metrics sample, in order.
    pub(crate) fn models(observed: &[Observed]) -> Vec<Option<String>> {
        observed
            .iter()
            .filter_map(|entry| match entry {
                Observed::Report(report) => match &report.observation {
                    ovrcr_protocol::AgentObservation::Metrics(sample) => Some(sample.model.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The reporter-health reasons that reached the supervisor, in order.
    pub(crate) fn health(observed: &[Observed]) -> Vec<(ovrcr_protocol::ReporterHealth, String)> {
        observed
            .iter()
            .filter_map(|entry| match entry {
                Observed::Health(report) => match &report.observation {
                    ovrcr_protocol::AgentObservation::Health(sample) => {
                        Some((sample.state, sample.reason.clone().unwrap_or_default()))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn readable(stream: &UnixStream) -> bool {
    let mut poll = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut poll, 1, 0) > 0 }
}

/// What the supervisor believes, checked the way the runtime checks it.
struct State {
    provider: AgentProvider,
    binding: Option<AgentBinding>,
    generation: u64,
    /// Activity, metrics, health, input, and quota have independent revision watermarks.
    revisions: [u64; 5],
    /// The one retained operation receipt, as the runtime's `latest` is.
    latest: Option<(String, Response)>,
    released: bool,
    observed: Vec<Observed>,
}

fn serve(
    listener: UnixListener,
    supervisor: UnixStream,
    provider: AgentProvider,
    script: Vec<Answer>,
    stop: Arc<AtomicBool>,
) -> Vec<Observed> {
    let mut live = vec![supervisor];
    let mut state = State {
        provider,
        binding: None,
        generation: 0,
        revisions: [0; 5],
        latest: None,
        released: false,
        observed: Vec::new(),
    };
    let mut served = 0;
    while !stop.load(Ordering::Acquire) {
        if let Ok((mut connection, _)) = listener.accept() {
            // A caller whose deadline expired mid-connect leaves a socket here that is
            // already gone; it is not this fixture's business to complain about it.
            let usable = connection.set_nonblocking(false).is_ok()
                && connection
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .is_ok();
            if usable && exchange_preamble(&mut connection).is_ok() {
                live.push(connection);
            }
            continue;
        }
        let mut worked = false;
        let mut index = 0;
        while index < live.len() {
            if !readable(&live[index]) {
                index += 1;
                continue;
            }
            let Ok(message) = read_frame::<ClientMessage>(&mut live[index]) else {
                live.remove(index);
                continue;
            };
            worked = true;
            let answer = script.get(served).copied().unwrap_or(Answer::Auto);
            served += 1;
            let response = respond(message.request, answer, &mut state);
            if answer != Answer::Withhold {
                let _ = write_frame(
                    &mut live[index],
                    &ServerMessage::Response {
                        request_id: message.request_id,
                        response,
                    },
                );
            }
            index += 1;
        }
        if !worked {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    state.observed
}

/// Index of the revision watermark one observation kind advances.
fn kind(observation: &ovrcr_protocol::AgentObservation) -> usize {
    use ovrcr_protocol::AgentObservation::*;
    match observation {
        Activity(_) => 0,
        Metrics(_) => 1,
        Health(_) => 2,
        Input(_) => 3,
        Quota(_) => 4,
    }
}

/// The runtime's `apply`: the report must name the binding in force and advance that
/// observation kind's revision.
fn apply(state: &mut State, report: &ProviderReport) -> Result<(), Response> {
    if state.released || state.binding.as_ref() != Some(&report.binding) {
        return Err(reject("agent binding changed"));
    }
    let watermark = &mut state.revisions[kind(&report.observation)];
    if report.revision <= *watermark {
        return Err(reject("stale revision"));
    }
    *watermark = report.revision;
    Ok(())
}

fn reject(message: &str) -> Response {
    Response::Error {
        code: ErrorCode::Conflict,
        message: message.into(),
    }
}

fn refusal() -> Response {
    reject("scripted supervisor refused the request")
}

fn respond(request: Request, answer: Answer, state: &mut State) -> Response {
    match request {
        Request::SupervisorHello(_) => {
            state.observed.push(Observed::Hello);
            if answer == Answer::Refuse || state.released {
                refusal()
            } else {
                Response::Ok
            }
        }
        // The retained receipt is checked before the lease is, so the final operation
        // stays answerable after a release.
        Request::AgentStatus { operation, .. } => {
            state.observed.push(Observed::Status(operation.clone()));
            match &state.latest {
                Some((retained, response)) if *retained == operation => response.clone(),
                _ => reject("operation receipt is unavailable or stale"),
            }
        }
        Request::AgentReport(report) => {
            let AgentUpdate::Provider(report) = report.update else {
                return refusal();
            };
            state.observed.push(Observed::Report(report.clone()));
            if answer == Answer::Refuse {
                return refusal();
            }
            match apply(state, &report) {
                Ok(()) => Response::Ok,
                Err(rejection) => rejection,
            }
        }
        Request::Supervisor(supervisor) => {
            let receipt = match supervisor.command {
                AgentCommand::RetainConversation { binding, reference } => {
                    state.observed.push(Observed::Retain {
                        binding: binding.clone(),
                        reference: *reference.clone(),
                    });
                    if answer == Answer::Refuse {
                        refusal()
                    } else if state.released
                        || state.binding.as_ref() != Some(&binding)
                        || !reference.matches_binding(&binding)
                    {
                        reject("agent binding changed")
                    } else {
                        Response::AgentOperation(AgentOperationResult::ConversationRetained)
                    }
                }
                AgentCommand::InvalidateConversation => {
                    Response::AgentOperation(AgentOperationResult::ConversationInvalidated)
                }
                AgentCommand::Bind {
                    expected_binding,
                    conversation,
                } => {
                    state.observed.push(Observed::Bind {
                        conversation: conversation.clone(),
                        generation: state.generation + 1,
                        expected: expected_binding.clone(),
                    });
                    if answer == Answer::Refuse {
                        refusal()
                    } else if state.released || expected_binding != state.binding {
                        reject("agent binding changed")
                    } else {
                        state.generation += 1;
                        let binding = AgentBinding {
                            provider: state.provider,
                            invocation: "inv".into(),
                            conversation,
                            generation: state.generation,
                        };
                        state.binding = Some(binding.clone());
                        state.revisions = [0; 5];
                        Response::AgentOperation(AgentOperationResult::Bound(binding))
                    }
                }
                AgentCommand::Health(report) => {
                    state.observed.push(Observed::Health(report.clone()));
                    if answer == Answer::Refuse {
                        refusal()
                    } else {
                        match apply(state, &report) {
                            Ok(()) => Response::AgentOperation(AgentOperationResult::HealthUpdated),
                            Err(rejection) => rejection,
                        }
                    }
                }
                AgentCommand::Finalize {
                    binding,
                    revision,
                    final_metrics,
                } => {
                    let report = ProviderReport {
                        binding,
                        revision,
                        observation: ovrcr_protocol::AgentObservation::Metrics(final_metrics),
                    };
                    state.observed.push(Observed::Finalize(report.clone()));
                    if answer == Answer::Refuse {
                        refusal()
                    } else {
                        match apply(state, &report) {
                            Ok(()) => {
                                state.released = true;
                                Response::AgentOperation(AgentOperationResult::Released)
                            }
                            Err(rejection) => rejection,
                        }
                    }
                }
                AgentCommand::Release { expected_binding } => {
                    state.observed.push(Observed::Release);
                    if state.released || expected_binding != state.binding {
                        reject("agent binding changed")
                    } else {
                        state.released = true;
                        Response::AgentOperation(AgentOperationResult::Released)
                    }
                }
            };
            // Only a successful operation leaves a receipt, and only the latest is kept.
            if !matches!(receipt, Response::Error { .. }) {
                state.latest = Some((supervisor.operation, receipt.clone()));
            }
            receipt
        }
        _ => refusal(),
    }
}

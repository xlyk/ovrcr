//! One scripted supervisor for every reporter-lifecycle test: the paired supervisor
//! connection and a real socket for publications and receipt re-reads, answering in the
//! order a synchronous reporter drives them.
//!
//! By default it answers the way the server does — a Bind is granted the next Reporting
//! generation, a publication is accepted, health is acknowledged, a finalize releases —
//! and it remembers each operation's receipt, so an `AgentStatus` re-read replays exactly
//! what the operation produced. `Answer::Withhold` performs the request and never replies,
//! which is what a lost receipt is; `Answer::Refuse` rejects it. The script is consumed in
//! the order requests arrive, counting every request including the `SupervisorHello` that
//! opens a receipt re-read.
use super::Reporter;
use crate::report::InvocationLease;
use ovrcr_protocol::{
    AgentBinding, AgentCommand, AgentOperationResult, AgentProvider, AgentSecret, AgentUpdate,
    ClientMessage, ErrorCode, ProviderReport, Request, Response, ServerMessage, SessionId,
    SupervisorAuth, exchange_preamble, read_frame, write_frame,
};
use std::{
    collections::HashMap,
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

fn serve(
    listener: UnixListener,
    supervisor: UnixStream,
    provider: AgentProvider,
    script: Vec<Answer>,
    stop: Arc<AtomicBool>,
) -> Vec<Observed> {
    let mut live = vec![supervisor];
    let mut observed = Vec::new();
    let mut receipts: HashMap<String, Response> = HashMap::new();
    let mut generation = 0;
    let mut served = 0;
    while !stop.load(Ordering::Acquire) {
        if let Ok((mut connection, _)) = listener.accept() {
            connection.set_nonblocking(false).unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            if exchange_preamble(&mut connection).is_ok() {
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
            let response = respond(
                message.request,
                provider,
                answer,
                &mut generation,
                &mut receipts,
                &mut observed,
            );
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
    observed
}

fn refusal() -> Response {
    Response::Error {
        code: ErrorCode::Conflict,
        message: "scripted supervisor refused the request".into(),
    }
}

fn respond(
    request: Request,
    provider: AgentProvider,
    answer: Answer,
    generation: &mut u64,
    receipts: &mut HashMap<String, Response>,
    observed: &mut Vec<Observed>,
) -> Response {
    match request {
        Request::SupervisorHello(_) => {
            observed.push(Observed::Hello);
            if answer == Answer::Refuse {
                refusal()
            } else {
                Response::Ok
            }
        }
        Request::AgentStatus { operation, .. } => {
            observed.push(Observed::Status(operation.clone()));
            receipts.get(&operation).cloned().unwrap_or_else(refusal)
        }
        Request::AgentReport(report) => {
            if let AgentUpdate::Provider(report) = report.update {
                observed.push(Observed::Report(report));
            }
            if answer == Answer::Refuse {
                refusal()
            } else {
                Response::Ok
            }
        }
        Request::Supervisor(supervisor) => {
            let receipt = match supervisor.command {
                AgentCommand::Bind {
                    expected_binding,
                    conversation,
                } => {
                    *generation += 1;
                    observed.push(Observed::Bind {
                        conversation: conversation.clone(),
                        generation: *generation,
                        expected: expected_binding,
                    });
                    if answer == Answer::Refuse {
                        refusal()
                    } else {
                        Response::AgentOperation(AgentOperationResult::Bound(AgentBinding {
                            provider,
                            invocation: "inv".into(),
                            conversation,
                            generation: *generation,
                        }))
                    }
                }
                AgentCommand::Health(report) => {
                    observed.push(Observed::Health(report));
                    if answer == Answer::Refuse {
                        refusal()
                    } else {
                        Response::AgentOperation(AgentOperationResult::HealthUpdated)
                    }
                }
                AgentCommand::Finalize {
                    binding,
                    revision,
                    final_metrics,
                } => {
                    observed.push(Observed::Finalize(ProviderReport {
                        binding,
                        revision,
                        observation: ovrcr_protocol::AgentObservation::Metrics(final_metrics),
                    }));
                    if answer == Answer::Refuse {
                        refusal()
                    } else {
                        Response::AgentOperation(AgentOperationResult::Released)
                    }
                }
                AgentCommand::Release { .. } => {
                    observed.push(Observed::Release);
                    Response::AgentOperation(AgentOperationResult::Released)
                }
            };
            receipts.insert(supervisor.operation, receipt.clone());
            receipt
        }
        _ => refusal(),
    }
}

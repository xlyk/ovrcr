//! One reporter lifecycle behind every provider's frame receiver.
//!
//! A `Reporter` owns everything about a managed invocation that is not provider
//! vocabulary: the lease and the binding it carries, the bind receipts that survive a
//! lost reply, the one revision set every observation is numbered from, the publish-or-
//! disable decision, the Producer fence and the identity budget it is charged against,
//! the pause a Reporting generation recovers from, and the teardown. A receiver decides
//! only what its provider's frames mean; it says so by handing observations here.
//!
//! What every provider shares is this mechanism, not a uniform use of it. Each receiver
//! still chooses which parts it needs, and the choices differ:
//!
//! - Only Pi and Oh My Pi pause: they stop applying frames where the Producer fence
//!   finds a hole, and recover with a forced rebind. Claude reports health for something
//!   else — whether its transcript reader is working — and Codex reports none at all.
//! - Claude settles its accounting at exit through [`Reporter::finalize`]; every other
//!   receiver takes the default [`Frames::finish`], which disables and lets the closed
//!   connection release the reservation.
//! - A bind whose receipt is lost is remembered for the next attempt, but only Claude
//!   ever makes one: the extension and Codex receivers have no second chance at a
//!   boundary, so they call [`Reporter::bind_or_disable`] and end reporting instead.
//! - Only the receivers that speak their own transport ask [`Reporter::own_frame`];
//!   Claude's hook helper runs inside the native command, so its frames are certified by
//!   conversation identity rather than by which process connected.
use super::{HOOK_INPUT_LIMIT, InvocationLease};
use ovrcr_protocol::{
    AgentBinding, AgentCommand, AgentObservation, AgentOperationResult, AgentProvider,
    HealthSample, MetricsSample, ProviderReport, ReporterHealth, Response,
};
use ovrcr_runtime::agent_runner::{HookEvent, HookHandler, private_identifier};
use std::{
    collections::HashSet,
    ffi::OsString,
    path::PathBuf,
    time::{Duration, Instant},
};

pub const ACCEPTED: &[u8] = b"admission-accepted\n";
pub const IGNORED: &[u8] = b"admission-ignored\n";
pub const UNAVAILABLE: &[u8] = b"admission-unavailable\n";

/// Identity capacity one reporter retains, shared by its Producer fences and the
/// response cycles it has published. Both are unbounded provider-supplied strings, so
/// they are charged against one budget and exhausting it is identity ambiguity.
pub const MAX_IDENTITIES: usize = 65_536;
pub const MAX_IDENTITY_BYTES: usize = 16 * 1024 * 1024;

/// What a provider's frames mean. Everything else — the lease, the binding, the
/// revisions, the fences, the teardown — is the `Reporter` each call is handed.
pub trait Frames: Send + 'static {
    /// One authenticated frame from this provider's own transport.
    fn frame(
        &mut self,
        reporter: &mut Reporter,
        input: &[u8],
        native_root: bool,
        deadline: Instant,
    ) -> Vec<u8>;
    /// Roughly ten times a second while the native command runs.
    fn poll(&mut self, _reporter: &mut Reporter, _deadline: Instant) {}
    /// The native command exited and the lease is about to go.
    fn finish(&mut self, reporter: &mut Reporter, _deadline: Instant) {
        reporter.disable();
    }
}

/// The launch checks every provider makes before its receiver can report anything, in
/// the order they are answered: a managed reservation, an eligible argv, and an
/// interactive terminal. `Some` names the reason this launch cannot be reported on.
pub fn preflight(
    reserved: bool,
    argv: &[OsString],
    eligible: fn(&[OsString]) -> bool,
    interactive: bool,
) -> Option<&'static str> {
    if !reserved {
        Some("no managed reservation")
    } else if !eligible(argv) {
        Some("unsupported launch arguments")
    } else if !interactive {
        Some("interactive terminal required")
    } else {
        None
    }
}

/// Whether both standard streams are a terminal, which is what a managed launch needs.
pub fn interactive() -> bool {
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

/// What the Producer fence makes of one frame, before any state changes. Named for the
/// fence rather than for admission, which in this crate is Claude's launch gate and the
/// `admission-*` reply bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fence {
    Accepted,
    Ignored,
    /// A source sequence is missing: whatever it carried was never applied.
    Gap,
    /// A new instance announced itself while the previous one was still live: the
    /// replacement is admitted, but what it replaced was never observed closing.
    LostClose,
}

enum ClaudeInvalidation {
    Pending(String),
    Committed,
}

pub struct Reporter {
    provider: AgentProvider,
    lease: Option<InvocationLease>,
    closed: bool,
    claude_invalidation: Option<ClaudeInvalidation>,
    claude_retention: Option<(String, ovrcr_protocol::ClaudeConversation)>,
    /// One revision set for every observation this reporter publishes. The server keeps
    /// a watermark per observation kind, so one monotonic counter satisfies all of them
    /// and no two kinds can disagree about which sample is newer.
    revision: u64,
    /// A bind whose receipt was lost. The next attempt asks for this operation's receipt
    /// instead of issuing a second Bind, which would claim a generation it never saw.
    pending: Option<String>,
    /// Why live reporting is uncertain, mirroring the health last published.
    paused: Option<String>,
    /// The admitted extension instance and its last accepted source sequence.
    producer: Option<(String, u64)>,
    /// Instances fenced for the life of this reporter: a retired Producer is gone for good.
    fences: HashSet<String>,
    /// Response-cycle identities this reporter has published under the current generation.
    cycles: HashSet<String>,
    charged: usize,
    /// A task-owned directory removed with this reporter, however the invocation ends.
    scratch: Option<PathBuf>,
}

impl Reporter {
    pub fn new(
        provider: AgentProvider,
        lease: Option<InvocationLease>,
        scratch: Option<PathBuf>,
    ) -> Self {
        Self {
            provider,
            lease,
            closed: false,
            claude_invalidation: None,
            claude_retention: None,
            revision: 0,
            pending: None,
            paused: None,
            producer: None,
            fences: HashSet::new(),
            cycles: HashSet::new(),
            charged: 0,
            scratch,
        }
    }

    /// The `HookHandler` `src/cli/agent.rs` installs and `ovrcr_runtime::agent_runner`
    /// drives: this lifecycle in front of one provider's `Frames`.
    pub fn handler(mut self, mut frames: impl Frames) -> HookHandler {
        Box::new(move |event| match event {
            HookEvent::Request {
                input,
                native_root,
                deadline,
            } => frames.frame(&mut self, input, native_root, deadline),
            HookEvent::Poll { deadline } => {
                if matches!(
                    self.claude_invalidation,
                    Some(ClaudeInvalidation::Pending(_))
                ) {
                    self.invalidate_claude(deadline);
                }
                if !self.closed
                    && let Some((_, reference)) = self.claude_retention.clone()
                {
                    self.retain_claude(reference, deadline);
                }
                frames.poll(&mut self, deadline);
                Vec::new()
            }
            HookEvent::NativeCompleted { deadline } => {
                if matches!(
                    self.claude_invalidation,
                    Some(ClaudeInvalidation::Pending(_))
                ) {
                    self.invalidate_claude(deadline);
                }
                if !self.closed
                    && let Some((_, reference)) = self.claude_retention.clone()
                {
                    self.retain_claude(reference, deadline);
                }
                frames.finish(&mut self, deadline);
                Vec::new()
            }
        })
    }

    /// This launch will not be reported on. The native command still runs.
    pub fn unavailable(&mut self, display: &str, reason: &str) {
        self.disable();
        eprintln!("{display} reporting unavailable ({reason}); running native command");
    }

    pub fn closed(&self) -> bool {
        self.closed
    }

    pub fn binding(&self) -> Option<&AgentBinding> {
        self.lease.as_ref()?.binding.as_ref()
    }

    /// Whether a reason is currently reported as this reporter's health. The extension
    /// receivers read it as their own gate — they apply nothing until a trustworthy
    /// source boundary rebinds — which is why they are the only ones that ask.
    pub fn paused(&self) -> bool {
        self.paused.is_some()
    }

    /// Stop reporting, keeping the lease so the reservation is released the ordinary way
    /// when the invocation ends.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Stop reporting and drop the watch now. Closing the connection releases this exact
    /// reservation generation even if the release reply is lost, and Drop cannot then
    /// open an extra blocking grace period.
    pub fn disable(&mut self) {
        self.closed = true;
        if let Some(lease) = self.lease.take() {
            let _ = lease.stream.shutdown(std::net::Shutdown::Both);
            drop(lease);
        }
        self.remove_scratch();
    }

    /// Bind this reporter's reservation to `conversation`, answering whether it now holds
    /// a binding for it. The whole callback budget goes to the command, because a caller
    /// that uses this rather than [`Reporter::bind_or_disable`] will be asked again; a
    /// lost receipt is remembered, and the next call asks for that same operation's
    /// receipt instead of issuing a second Bind. A refusal closes the reporter — this
    /// reservation will not bind this conversation.
    ///
    /// `force` asks for a fresh Reporting generation on the conversation already bound —
    /// the recovery a paused reporter makes at a trustworthy source boundary — instead of
    /// short-circuiting as already bound. That generation starts blank: the pause is over
    /// and the response-cycle identities start again, because the server reads the same
    /// turn under a new generation as a new response. Producer fences are not identities
    /// of a generation and survive it.
    pub fn bind(&mut self, conversation: &str, deadline: Instant, force: bool) -> bool {
        self.bind_within(conversation, deadline, force, false)
    }

    /// Persist provider-owned identity only after the certified binding is accepted.
    pub fn retain_claude(
        &mut self,
        reference: ovrcr_protocol::ClaudeConversation,
        deadline: Instant,
    ) -> bool {
        if self.closed {
            return false;
        }
        if self.claude_retention.is_none() {
            let Ok(operation) = private_identifier() else {
                return false;
            };
            self.claude_retention = Some((operation, reference));
        }
        let (operation, reference) = self.claude_retention.as_ref().unwrap();
        let Some(lease) = self.lease.as_mut() else {
            return false;
        };
        let Some(binding) = lease.binding.clone() else {
            return false;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        let response = lease.command(
            operation.clone(),
            AgentCommand::RetainClaude {
                binding,
                reference: Box::new(reference.clone()),
            },
            Instant::now() + remaining / 2,
        );
        let response = match response {
            Err(_) if Instant::now() < deadline => {
                lease.operation_status(operation.clone(), deadline)
            }
            response => response,
        };
        if matches!(
            response,
            Ok(Response::AgentOperation(
                AgentOperationResult::ConversationRetained
            ))
        ) {
            self.claude_retention = None;
            true
        } else {
            false
        }
    }

    pub fn invalidate_claude(&mut self, deadline: Instant) -> bool {
        self.closed = true;
        self.claude_retention = None;
        if matches!(
            self.claude_invalidation,
            Some(ClaudeInvalidation::Committed)
        ) {
            return true;
        }
        if self.claude_invalidation.is_none() {
            let Ok(operation) = private_identifier() else {
                return false;
            };
            self.claude_invalidation = Some(ClaudeInvalidation::Pending(operation));
        }
        // The server retains one operation receipt. Recover a lost Bind before
        // invalidation replaces that receipt, preserving its observed generation.
        // Invalidation still proceeds if that old receipt cannot be recovered.
        let unresolved_binding = if let Some(operation) = self.pending.clone() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if self.settle(Instant::now() + remaining / 2) {
                false
            } else {
                self.pending = Some(operation);
                true
            }
        } else {
            false
        };
        let Some(ClaudeInvalidation::Pending(operation)) = &self.claude_invalidation else {
            unreachable!()
        };
        let Some(lease) = self.lease.as_mut() else {
            return false;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        let response = lease.command(
            operation.clone(),
            AgentCommand::InvalidateClaude,
            Instant::now() + remaining / 2,
        );
        let response = match response {
            Err(_) if Instant::now() < deadline => {
                lease.operation_status(operation.clone(), deadline)
            }
            response => response,
        };
        if matches!(
            response,
            Ok(Response::AgentOperation(
                AgentOperationResult::ConversationInvalidated
            ))
        ) {
            self.claude_invalidation = Some(ClaudeInvalidation::Committed);
            if unresolved_binding {
                self.disable();
            } else {
                self.health(Some("identity_transition_unavailable"), deadline);
            }
            true
        } else {
            // Retain the obligation and lease. The handler retries on poll and
            // native completion, while closed prevents any further admission.
            false
        }
    }

    /// `reserve_receipt` keeps half the budget back so a lost reply can still be re-read
    /// inside this same call. A receiver that will be asked again spends the whole budget
    /// on the command instead and asks for the receipt next time; re-reading early only
    /// when the command failed with time to spare costs it nothing.
    fn bind_within(
        &mut self,
        conversation: &str,
        deadline: Instant,
        force: bool,
        reserve_receipt: bool,
    ) -> bool {
        if self.closed {
            return false;
        }
        let provider = self.provider;
        let Some(lease) = self.lease.as_mut() else {
            self.closed = true;
            return false;
        };
        if !force
            && self.pending.is_none()
            && lease
                .binding
                .as_ref()
                .is_some_and(|binding| binding.conversation == conversation)
        {
            return true;
        }
        let (operation, response) = match self.pending.take() {
            Some(operation) => {
                let response = lease.operation_status(operation.clone(), deadline);
                (operation, response)
            }
            None => {
                let Ok(operation) = private_identifier() else {
                    self.closed = true;
                    return false;
                };
                let expected_binding = lease.binding.clone();
                let remaining = deadline.saturating_duration_since(Instant::now());
                let granted = if reserve_receipt {
                    Instant::now() + remaining / 2
                } else {
                    deadline
                };
                let response = lease.command(
                    operation.clone(),
                    AgentCommand::Bind {
                        expected_binding,
                        conversation: conversation.to_owned(),
                    },
                    granted,
                );
                let response = match response {
                    Ok(response) => Ok(response),
                    // Worth asking for the receipt only while budget remains. Spending
                    // none of it on a re-read that cannot finish is what leaves the
                    // operation to be asked about on the next attempt instead.
                    Err(_) if Instant::now() < deadline => {
                        lease.operation_status(operation.clone(), deadline)
                    }
                    Err(error) => Err(error),
                };
                (operation, response)
            }
        };
        match response {
            Ok(Response::AgentOperation(AgentOperationResult::Bound(binding)))
                if binding.conversation == conversation && binding.provider == provider =>
            {
                self.bound(binding, force);
                true
            }
            Ok(_) => {
                self.closed = true;
                false
            }
            Err(_) => {
                self.pending = Some(operation);
                false
            }
        }
    }

    /// Publish one observation against the current binding under the next revision.
    /// `UNAVAILABLE` means it could not be delivered and the reporter disabled itself:
    /// delivery uncertainty cannot leave a connected reporter claiming continuity.
    pub fn publish(&mut self, observation: AgentObservation, deadline: Instant) -> Vec<u8> {
        let Some(binding) = self.binding().cloned() else {
            self.disable();
            return UNAVAILABLE.to_vec();
        };
        self.revision += 1;
        let report = ProviderReport {
            binding,
            revision: self.revision,
            observation,
        };
        let published = self
            .lease
            .as_ref()
            .is_some_and(|lease| lease.publish_observation(report, deadline).is_ok());
        if published {
            ACCEPTED.to_vec()
        } else {
            self.disable();
            UNAVAILABLE.to_vec()
        }
    }

    /// Bind, or end reporting. The extension and Codex receivers reach a binding only at
    /// a source boundary and have no second chance at one, so a bind they cannot complete
    /// disables them rather than leaving a receipt to ask about — and, for the same
    /// reason, half the budget is kept back to re-read a lost reply inside this same
    /// call. `Some` is the caller's early return. Claude admission does not use this: it
    /// waits for the announcement to repeat and asks for the original receipt then.
    pub fn bind_or_disable(
        &mut self,
        conversation: &str,
        deadline: Instant,
        force: bool,
    ) -> Option<Vec<u8>> {
        if self.bind_within(conversation, deadline, force, true) {
            return None;
        }
        self.disable();
        Some(UNAVAILABLE.to_vec())
    }

    /// The gate a receiver that speaks its own transport applies before reading a frame.
    /// `Some` is the caller's early return: a reporter that has ended answers UNAVAILABLE,
    /// and a frame that did not come from the native root, or is larger than one hook
    /// input, is not this receiver's to read.
    pub fn own_frame(&self, input: &[u8], native_root: bool) -> Option<Vec<u8>> {
        if self.closed {
            return Some(UNAVAILABLE.to_vec());
        }
        if !native_root || input.len() > HOOK_INPUT_LIMIT {
            return Some(IGNORED.to_vec());
        }
        None
    }

    /// Publish this reporter's own health over the supervisor connection, and set the
    /// reason [`Reporter::paused`] reports. `Some` names what is uncertain while the
    /// lease, the binding and the native session all hold — for the extension receivers
    /// a hole in the frames they stop applying, for Claude a transcript reader that is
    /// not working; `None` reports it healthy again. `false` means the report could not be
    /// delivered and the reporter disabled itself. A reporter that never bound has no
    /// health to report and says so without sending anything.
    pub fn health(&mut self, reason: Option<&str>, deadline: Instant) -> bool {
        if self.paused.as_deref() == reason {
            return true;
        }
        // A bind whose receipt was lost must settle before its binding can carry health.
        if self.pending.is_some() && !self.settle(deadline) {
            self.disable();
            return false;
        }
        self.paused = reason.map(str::to_owned);
        let Some(binding) = self.binding().cloned() else {
            return true;
        };
        self.revision += 1;
        let report = ProviderReport {
            binding,
            revision: self.revision,
            observation: AgentObservation::Health(HealthSample {
                state: if reason.is_some() {
                    ReporterHealth::Unavailable
                } else {
                    ReporterHealth::Connected
                },
                reason: reason.map(str::to_owned),
            }),
        };
        let acknowledged =
            private_identifier()
                .map_err(anyhow::Error::from)
                .and_then(|operation| {
                    let lease = self
                        .lease
                        .as_mut()
                        .ok_or_else(|| anyhow::anyhow!("no supervisor connection"))?;
                    lease.command(operation, AgentCommand::Health(report), deadline)
                });
        if matches!(
            acknowledged,
            Ok(Response::AgentOperation(
                AgentOperationResult::HealthUpdated
            ))
        ) {
            true
        } else {
            self.disable();
            false
        }
    }

    /// Settle the accounting for an invocation whose native command has exited and stop.
    /// A bound reporter finalizes, recovering the original operation's receipt when the
    /// acknowledgement is lost; an unbound one only releases its reservation.
    pub fn finalize(&mut self, final_metrics: Box<MetricsSample>, deadline: Instant) {
        let Some(mut lease) = self.lease.take() else {
            return;
        };
        let binding = (!self.closed).then(|| lease.binding.clone()).flatten();
        if let Some(binding) = binding {
            self.revision += 1;
            if let Ok(operation) = private_identifier() {
                let first = (Instant::now() + Duration::from_millis(300)).min(deadline);
                let result = lease.command(
                    operation.clone(),
                    AgentCommand::Finalize {
                        binding,
                        revision: self.revision,
                        final_metrics,
                    },
                    first,
                );
                if !matches!(
                    result,
                    Ok(Response::AgentOperation(AgentOperationResult::Released))
                ) && Instant::now() < deadline
                {
                    let _ = lease.final_status(operation, deadline);
                }
            }
        } else if let Ok(operation) = private_identifier() {
            let expected_binding = lease.binding.clone();
            let _ = lease.command(
                operation,
                AgentCommand::Release { expected_binding },
                deadline,
            );
        }
        self.closed = true;
        // Ack uncertainty falls back to watched disconnection, never a new grace period.
        let _ = lease.stream.shutdown(std::net::Shutdown::Both);
        drop(lease);
        self.remove_scratch();
    }

    /// The Producer fence. A retired instance is gone for good, a stale or replayed
    /// sequence changes nothing, and only a frame that announces an instance admits one
    /// this reporter has not seen — replacing a live Producer as an unobserved loss.
    /// Admitting an instance is the one answer that mutates: it installs the new Producer
    /// and retires the instance it replaced.
    pub fn admit_producer(&mut self, instance: &str, sequence: u64, announces: bool) -> Fence {
        if self.fences.contains(instance) {
            return Fence::Ignored;
        }
        match &mut self.producer {
            Some((current, last)) if current.as_str() == instance => {
                // The hole is reported once: the sequence advances before the pause so the
                // frames that follow it are ordinary frames of a paused reporter.
                if sequence <= *last {
                    return Fence::Ignored;
                }
                let gap = sequence > *last + 1;
                *last = sequence;
                if gap { Fence::Gap } else { Fence::Accepted }
            }
            live if announces => {
                let replaced = live.take();
                self.producer = Some((instance.to_owned(), sequence));
                match replaced {
                    Some((old, _)) => {
                        self.fence(&old);
                        Fence::LostClose
                    }
                    None => Fence::Accepted,
                }
            }
            _ => Fence::Ignored,
        }
    }

    /// Retire the Producer this reporter observed shutting down: fenced for good, and
    /// this reporter has no Producer until a successor announces itself.
    pub fn retire(&mut self, instance: &str) {
        self.fence(instance);
        self.producer = None;
    }

    /// Charge one response-cycle identity against the identity budget. `Some` is the
    /// caller's early return: a `Known` identity is a duplicate and is IGNORED before any
    /// capacity is considered, so a repeat can never exhaust the budget; an identity there
    /// is no room to retain could not be told from a new one later, which is identity
    /// ambiguity — the reporter disables itself and answers UNAVAILABLE.
    pub fn admit_cycle(&mut self, identity: &str) -> Option<Vec<u8>> {
        if self.cycles.contains(identity) {
            return Some(IGNORED.to_vec());
        }
        if !self.charge(identity) {
            return Some(UNAVAILABLE.to_vec());
        }
        self.cycles.insert(identity.to_owned());
        None
    }

    /// Whether this reporter has published `identity` under the current generation.
    pub fn published(&self, identity: &str) -> bool {
        self.cycles.contains(identity)
    }

    fn fence(&mut self, instance: &str) {
        if self.fences.contains(instance) {
            return;
        }
        if !self.charge(instance) {
            // Without room to fence it, a delayed frame from this instance could not be
            // told from a live one: that is identity ambiguity, and it disables.
            return;
        }
        self.fences.insert(instance.to_owned());
    }

    /// Reserve room for one retained identity, disabling when there is none.
    fn charge(&mut self, identity: &str) -> bool {
        let charge = identity.len() + std::mem::size_of::<String>();
        if self.fences.len() + self.cycles.len() >= MAX_IDENTITIES
            || self.charged + charge > MAX_IDENTITY_BYTES
        {
            self.disable();
            return false;
        }
        self.charged += charge;
        true
    }

    fn recharge(&mut self) {
        self.charged = self
            .fences
            .iter()
            .chain(self.cycles.iter())
            .map(|identity| identity.len() + std::mem::size_of::<String>())
            .sum();
    }

    /// Install a Reporting generation this reporter just obtained a receipt for, however
    /// it obtained it. Every generation starts its revision set over and ends the pause;
    /// a forced one starts its response-cycle identities over too, because the server
    /// reads the same turn under a new generation as a new response.
    fn bound(&mut self, binding: AgentBinding, force: bool) {
        if let Some(lease) = self.lease.as_mut() {
            lease.binding = Some(binding);
        }
        self.revision = 0;
        self.paused = None;
        if force {
            self.cycles.clear();
            self.recharge();
        }
    }

    /// Re-read the receipt of a bind this reporter is still waiting on.
    fn settle(&mut self, deadline: Instant) -> bool {
        let Some(operation) = self.pending.take() else {
            return true;
        };
        let Some(lease) = self.lease.as_mut() else {
            return false;
        };
        match lease.operation_status(operation, deadline) {
            Ok(Response::AgentOperation(AgentOperationResult::Bound(binding))) => {
                self.bound(binding, false);
                true
            }
            _ => false,
        }
    }

    fn remove_scratch(&mut self) {
        if let Some(dir) = self.scratch.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// The accept loop can exit, or its thread be abandoned, without a disabling callback:
/// the materialized directory is still this reporter's to remove.
impl Drop for Reporter {
    fn drop(&mut self) {
        self.remove_scratch();
    }
}

#[cfg(test)]
pub(crate) mod scripted;

#[cfg(test)]
mod tests;

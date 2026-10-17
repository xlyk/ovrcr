//! Bounded, content-free invalidations. Reporter/session state remains authoritative.
use super::{ServerState, delegation::Record, delegation_files::Directory};
use anyhow::{Result, bail};
use ovrcr_protocol::{
    AgentBinding, AssignmentIdentity, DelegationAuth, DelegationObservation,
    DelegationObservationBatch, ObservationCursor, ObservationStatus, PublicationIssue,
    PublicationState, ReporterHealth, SessionId, SessionPhase, SessionRunId, SessionSummary,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Condvar, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(super) const HINT_CAPACITY: usize = 256;
pub(super) const OBSERVATION_BATCH: usize = 64;
pub(super) const MAX_WAIT_MS: u64 = 20_000;
// Reconcile files even when the producer's post-publication invalidation was lost.
pub(super) const RECONCILE_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    run: SessionRunId,
    binding: Option<AgentBinding>,
    activity: u64,
    input: u64,
    native_control: u64,
    health: Option<ReporterHealth>,
    phase: SessionPhase,
    control: Option<ovrcr_protocol::AssignmentControl>,
}
impl Source {
    fn of(summary: &SessionSummary) -> Self {
        let agent = summary.agent.as_ref();
        Self {
            run: summary.run,
            binding: agent.map(|a| a.binding.clone()),
            activity: agent.map_or(0, |a| a.activity_revision),
            input: agent.map_or(0, |a| a.input_revision),
            native_control: agent.map_or(0, |a| a.native_control_revision),
            health: agent.map(|a| a.health.state),
            phase: summary.phase.clone(),
            control: summary
                .assignment_control
                .as_ref()
                .map(|view| view.control.clone()),
        }
    }
}

#[derive(Default)]
struct Ring {
    sequence: u64,
    hints: VecDeque<(u64, SessionId)>,
    // Only live sessions are cached, bounded by the Server's existing live-slot limit.
    sources: HashMap<SessionId, (Source, Option<u64>)>,
    publications: HashMap<String, Fingerprint>,
}

#[derive(Default)]
pub(super) struct Observations {
    ring: Mutex<Ring>,
    changed: Condvar,
    checkpoints: Mutex<()>,
}

pub(super) struct Delta {
    pub sequence: u64,
    pub reconcile: bool,
    pub changed: bool,
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    result_version: u32,
    publication: Option<PublicationState>,
    issue: Option<PublicationIssue>,
    current: bool,
    stalled: bool,
    unresolved: bool,
    control: ovrcr_protocol::AssignmentControl,
    control_receipt_digest: String,
    workflow_cancelled: bool,
    questions: Vec<ovrcr_protocol::QuestionPublicationState>,
    native_question_gap: Option<ovrcr_protocol::NativeQuestionGap>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    identity: AssignmentIdentity,
    #[serde(default = "checkpoint_initial_version")]
    result_version: u32,
    epoch: String,
    binding: Option<AgentBinding>,
    activity: u64,
    input: u64,
    progress_unix_ms: u64,
    unresolved: bool,
}
fn checkpoint_initial_version() -> u32 {
    1
}

impl Checkpoint {
    fn expire_future(&mut self, now: u64) {
        if self.progress_unix_ms > now {
            self.progress_unix_ms = 0;
            self.unresolved = true;
        }
    }
    fn advance(&mut self, epoch: &str, source: &Source, accepted: Option<u64>) {
        if self.epoch != epoch {
            // Cache seeding and an epoch change are baseline, never new progress.
            self.epoch = epoch.into();
            self.unresolved = true;
        } else if self.activity != source.activity || self.input != source.input {
            if let Some(stamp) = accepted {
                self.progress_unix_ms = if self.progress_unix_ms > now_ms() {
                    stamp
                } else {
                    self.progress_unix_ms.max(stamp)
                };
                self.unresolved = false;
            } else {
                self.unresolved = true;
            }
        }
        self.binding = source.binding.clone();
        self.activity = source.activity;
        self.input = source.input;
    }
}

impl Observations {
    pub fn session_changed(&self, summary: &SessionSummary) {
        self.source_changed(summary.id, Source::of(summary));
    }

    fn source_changed(&self, id: SessionId, source: Source) {
        let mut ring = self.ring.lock().unwrap();
        if ring.sources.get(&id).is_some_and(|(old, _)| *old == source) {
            return;
        }
        let progress = ring.sources.get(&id).and_then(|(old, stamp)| {
            if old.run != source.run
                || old.binding != source.binding
                || old.activity != source.activity
                || old.input != source.input
            {
                Some(now_ms())
            } else {
                *stamp
            }
        });
        if source.phase.is_live() {
            ring.sources.insert(id, (source, progress));
        } else {
            ring.sources.remove(&id);
        }
        Self::push(&mut ring, id);
        self.changed.notify_all();
    }

    pub fn invalidate(&self, session: SessionId) {
        let mut ring = self.ring.lock().unwrap();
        Self::push(&mut ring, session);
        self.changed.notify_all();
    }

    fn push(ring: &mut Ring, session: SessionId) {
        ring.sequence = ring
            .sequence
            .checked_add(1)
            .expect("observation sequence exhausted");
        if ring.hints.len() == HINT_CAPACITY {
            ring.hints.pop_front();
        }
        ring.hints.push_back((ring.sequence, session));
    }

    pub fn delta(&self, after: Option<u64>, handles: &[SessionId]) -> Delta {
        let ring = self.ring.lock().unwrap();
        let reconcile = after.is_none_or(|after| {
            after > ring.sequence
                || ring
                    .hints
                    .front()
                    .is_some_and(|(first, _)| after < first.saturating_sub(1))
        });
        let changed = reconcile
            || ring
                .hints
                .iter()
                .any(|(seq, id)| after.is_none_or(|after| *seq > after) && handles.contains(id));
        Delta {
            sequence: ring.sequence,
            reconcile,
            changed,
        }
    }

    #[cfg(test)]
    fn progress_at(&self, session: SessionId) -> Option<u64> {
        self.ring
            .lock()
            .unwrap()
            .sources
            .get(&session)
            .and_then(|(_, stamp)| *stamp)
    }

    fn publication_changed(
        &self,
        handle: &str,
        session: SessionId,
        fingerprint: Fingerprint,
    ) -> Result<()> {
        let mut ring = self.ring.lock().unwrap();
        if ring.publications.get(handle) == Some(&fingerprint) {
            return Ok(());
        }
        if !ring.publications.contains_key(handle) && ring.publications.len() == 4096 {
            bail!("observation tracking capacity exhausted; reconcile existing assignments");
        }
        ring.publications.insert(handle.into(), fingerprint);
        Self::push(&mut ring, session);
        self.changed.notify_all();
        Ok(())
    }

    // The caller releases all session, persistence and lifecycle locks before waiting.
    // Recheck the sequence while holding this lock to avoid a lost wake before sleep.
    pub fn wait(&self, sequence: u64, timeout: Duration) {
        let ring = self.ring.lock().unwrap();
        if ring.sequence == sequence {
            let _ = self.changed.wait_timeout(ring, timeout).unwrap();
        }
    }
}

impl ServerState {
    fn delegation_progress_deadline_ms(&self) -> u64 {
        self.settings
            .lock()
            .unwrap()
            .report
            .settings
            .delegation
            .progress_deadline_ms
    }

    /// Read-only control observation plus this parent's private observation checkpoint.
    /// This never acquires the lifecycle lock and never claims the Dashboard seat.
    pub(crate) fn observe_delegations(
        &self,
        auth: &DelegationAuth,
        handles: &[String],
        after: Option<ObservationCursor>,
        wait_ms: u64,
    ) -> Result<DelegationObservationBatch> {
        if handles.is_empty()
            || handles.len() > OBSERVATION_BATCH
            || wait_ms > MAX_WAIT_MS
            || handles
                .iter()
                .enumerate()
                .any(|(i, handle)| handles[..i].contains(handle))
        {
            bail!("observe requires 1..64 distinct handles and wait_ms <= 20000");
        }
        let sequence = after
            .as_ref()
            .filter(|c| c.server_epoch == self.server_lifetime)
            .map(|c| c.sequence);
        let until = std::time::Instant::now() + Duration::from_millis(wait_ms);
        loop {
            if self.stopping.load(std::sync::atomic::Ordering::Acquire)
                || self.shutdown.load(std::sync::atomic::Ordering::Acquire)
            {
                bail!("observation disconnected: Server is stopping; reconnect and reconcile");
            }
            let deadline = self.delegation_progress_deadline_ms();
            let mut observations = Vec::with_capacity(handles.len());
            let mut sessions = Vec::with_capacity(handles.len());
            for handle in handles {
                let record = self.owned_assignment(auth, handle)?;
                let observed = self.delegation_observation(auth, &record, deadline)?;
                let session = record.identity.session.unwrap_or(auth.session);
                self.delegation_observations.publication_changed(
                    handle,
                    session,
                    Fingerprint {
                        result_version: record.result_version,
                        publication: observed.publication.clone(),
                        issue: observed.publication_issue.clone(),
                        current: observed.current,
                        stalled: observed.stalled,
                        unresolved: observed.progress_unresolved,
                        control: observed.control.clone(),
                        control_receipt_digest: observed.control_receipt_digest.clone(),
                        workflow_cancelled: observed.workflow_cancelled,
                        questions: observed.questions.clone(),
                        native_question_gap: observed.native_question_gap,
                    },
                )?;
                sessions.push(session);
                observations.push(observed);
            }
            let delta = self.delegation_observations.delta(sequence, &sessions);
            let remaining = until.saturating_duration_since(std::time::Instant::now());
            if delta.changed || remaining.is_zero() {
                let status = if delta.reconcile
                    || observations.iter().any(|observed| {
                        observed.control.barrier.is_some()
                            || observed.control.owner == ovrcr_protocol::ControlOwner::Uncertain
                    }) {
                    ObservationStatus::ReconcileRequired
                } else if observations.iter().any(|o| o.stalled) {
                    ObservationStatus::Stalled
                } else if delta.changed {
                    ObservationStatus::Changed
                } else {
                    ObservationStatus::Deadline
                };
                return Ok(DelegationObservationBatch {
                    cursor: ObservationCursor {
                        server_epoch: self.server_lifetime.clone(),
                        sequence: delta.sequence,
                    },
                    status,
                    progress_deadline_ms: deadline,
                    observations,
                });
            }
            self.delegation_observations
                .wait(delta.sequence, remaining.min(RECONCILE_INTERVAL));
        }
    }

    fn delegation_observation(
        &self,
        auth: &DelegationAuth,
        record: &Record,
        deadline: u64,
    ) -> Result<DelegationObservation> {
        let summary = record
            .identity
            .session
            .and_then(|id| self.session_summary(id));
        let agent = summary.as_ref().and_then(|s| s.agent.as_ref());
        let current = summary.as_ref().is_some_and(|s| {
            Some(s.run) == record.identity.run
                && (!s.phase.is_live() || agent.map(|a| &a.binding) == record.binding.as_ref())
        });
        let (publication, publication_issue) = match self.assignment_publication(record) {
            Ok(publication) => (
                publication.map(|identity| PublicationState {
                    handled: record.handled_publication.as_ref() == Some(&identity),
                    identity,
                }),
                None,
            ),
            Err(issue) => (None, Some(issue)),
        };
        let parent = self.delegation_auth(auth)?;
        let _checkpoint = self.delegation_observations.checkpoints.lock().unwrap();
        let store = self.retained.lock();
        let authority = self.require_workflow(&store, auth, record)?;
        let progress =
            parent.with_delegation_binding(&auth.capability, &authority.binding, |_| {
                let directory = Directory::open(&record.root, false)?;
                let previous = directory.read("observation-v1.json");
                let mut checkpoint: Checkpoint = match previous {
                    Ok(ref bytes) => serde_json::from_slice(bytes)?,
                    Err(ref error)
                        if error.chain().any(|e| {
                            e.downcast_ref::<std::io::Error>()
                                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                        }) =>
                    {
                        Checkpoint {
                            identity: record.identity.clone(),
                            result_version: record.result_version,
                            epoch: self.server_lifetime.clone(),
                            binding: record.binding.clone(),
                            activity: agent.map_or(0, |a| a.activity_revision),
                            input: agent.map_or(0, |a| a.input_revision),
                            progress_unix_ms: now_ms(),
                            unresolved: false,
                        }
                    }
                    Err(error) => return Err(error),
                };
                checkpoint.expire_future(now_ms());
                if checkpoint.identity != record.identity
                    || checkpoint.result_version != record.result_version
                {
                    if !record.previous_versions.iter().any(|version| {
                        version.identity == checkpoint.identity
                            && version.result_version == checkpoint.result_version
                    }) {
                        bail!("observation checkpoint belongs to another assignment or result");
                    }
                    // A known follow-up changes the selected result, never inventing progress.
                    checkpoint.identity = record.identity.clone();
                    checkpoint.result_version = record.result_version;
                }
                if current && let Some(summary) = &summary {
                    let source = Source::of(summary);
                    let accepted = {
                        let ring = self.delegation_observations.ring.lock().unwrap();
                        ring.sources
                            .get(&summary.id)
                            .filter(|(cached, _)| *cached == source)
                            .and_then(|(_, stamp)| *stamp)
                    };
                    checkpoint.advance(&self.server_lifetime, &source, accepted);
                }
                let bytes = serde_json::to_vec(&checkpoint)?;
                if previous.as_ref().ok() != Some(&bytes) {
                    directory.replace("observation-v1.json", &bytes)?;
                } else {
                    // Recover a prior replace whose final directory sync failed.
                    directory.sync()?;
                }
                Ok(checkpoint)
            })?;
        let now = now_ms();
        let future = progress.progress_unix_ms > now;
        let stalled = publication.is_none()
            && (future || now.saturating_sub(progress.progress_unix_ms) >= deadline);
        let questions = self.question_states(record);
        let native_question_gap = ovrcr_protocol::NativeQuestionGap::from_observation(
            current,
            agent.is_some(),
            agent.is_some_and(|a| !a.input_requests.is_empty()),
            stalled,
        );
        Ok(DelegationObservation {
            questions,
            native_question_gap,
            handle: record.handle.clone(),
            identity: record.identity.clone(),
            binding: agent.map(|a| a.binding.clone()),
            activity: agent.and_then(|a| a.activity.clone()),
            activity_revision: agent.map_or(0, |a| a.activity_revision),
            input_requests: agent.map_or_else(Vec::new, |a| a.input_requests.clone()),
            input_revision: agent.map_or(0, |a| a.input_revision),
            health: agent.map(|a| a.health.state),
            current,
            publication,
            publication_issue,
            last_progress_unix_ms: progress.progress_unix_ms,
            stalled,
            progress_unresolved: progress.unresolved || future,
            control: record.control.clone(),
            control_receipt_digest: super::delegation_recovery::hash(&serde_json::to_vec(
                &record.receipts,
            )?),
            native_control_revision: agent.map_or(0, |a| a.native_control_revision),
            workflow_cancelled: store
                .workflow(&record.identity.workflow)?
                .is_some_and(|row| row.cancellation.is_some()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overflow_and_foreign_cursors_require_reconciliation() {
        let observations = Observations::default();
        for _ in 0..HINT_CAPACITY + 2 {
            observations.invalidate(SessionId(1));
        }
        assert!(observations.delta(Some(0), &[SessionId(1)]).reconcile);
        assert!(
            observations
                .delta(Some(u64::MAX), &[SessionId(1)])
                .reconcile
        );
        let last = observations.delta(None, &[]).sequence;
        assert!(!observations.delta(Some(last), &[SessionId(1)]).changed);
        assert!(!observations.delta(Some(last - 1), &[SessionId(2)]).changed);
        assert!(observations.delta(Some(last - 1), &[SessionId(1)]).changed);
    }
    #[test]
    fn seeding_and_health_changes_do_not_certify_fresh_progress() {
        let observations = Observations::default();
        let mut source = Source {
            run: SessionRunId(1),
            binding: None,
            activity: 8,
            input: 3,
            health: Some(ReporterHealth::Connected),
            phase: SessionPhase::Running,
            control: None,
            native_control: 0,
        };
        observations.source_changed(SessionId(1), source.clone());
        assert_eq!(observations.progress_at(SessionId(1)), None);
        source.health = Some(ReporterHealth::Unavailable);
        observations.source_changed(SessionId(1), source.clone());
        assert_eq!(observations.progress_at(SessionId(1)), None);
        source.control = Some(ovrcr_protocol::AssignmentControl {
            epoch: 2,
            owner: ovrcr_protocol::ControlOwner::Human,
            ..ovrcr_protocol::AssignmentControl::default()
        });
        observations.source_changed(SessionId(1), source.clone());
        assert_eq!(
            observations.progress_at(SessionId(1)),
            None,
            "control ownership does not certify child progress"
        );
        source.activity += 1;
        observations.source_changed(SessionId(1), source.clone());
        let progress = observations
            .progress_at(SessionId(1))
            .expect("new accepted activity");
        observations.source_changed(SessionId(1), source);
        assert_eq!(observations.progress_at(SessionId(1)), Some(progress));
    }
    #[test]
    fn epoch_changes_gaps_and_repeated_snapshots_do_not_reset_deadline() {
        let identity: AssignmentIdentity = serde_json::from_value(serde_json::json!({"instance":"i","workflow":"w","controller":"c","controller_epoch":1,"task":"t","attempt":1,"contract":1,"request":"r","session":1,"run":1})).unwrap();
        let mut checkpoint = Checkpoint {
            identity,
            result_version: 1,
            epoch: "old".into(),
            binding: None,
            activity: 1,
            input: 0,
            progress_unix_ms: 100,
            unresolved: false,
        };
        let mut source = Source {
            run: SessionRunId(1),
            binding: None,
            activity: 9,
            input: 0,
            health: Some(ReporterHealth::Connected),
            phase: SessionPhase::Running,
            control: None,
            native_control: 0,
        };
        checkpoint.advance("new", &source, Some(500));
        assert_eq!(
            checkpoint.progress_unix_ms, 100,
            "new epoch seeds a baseline"
        );
        assert!(checkpoint.unresolved);
        source.health = Some(ReporterHealth::Unavailable);
        checkpoint.advance("new", &source, None);
        assert_eq!(checkpoint.progress_unix_ms, 100);
        source.activity += 1;
        checkpoint.advance("new", &source, Some(600));
        assert_eq!(checkpoint.progress_unix_ms, 600);
        assert!(!checkpoint.unresolved);
        checkpoint.advance("new", &source, Some(700));
        assert_eq!(
            checkpoint.progress_unix_ms, 600,
            "polling does not create progress"
        );
        source.input += 1;
        checkpoint.advance("new", &source, None);
        assert_eq!(
            checkpoint.progress_unix_ms, 600,
            "gapped evidence is not fresh progress"
        );
        assert!(checkpoint.unresolved);
    }
    #[test]
    fn future_checkpoint_expires_instead_of_extending_its_deadline() {
        let identity: AssignmentIdentity = serde_json::from_value(serde_json::json!({"instance":"i","workflow":"w","controller":"c","controller_epoch":1,"task":"t","attempt":1,"contract":1,"request":"r","session":1,"run":1})).unwrap();
        let mut checkpoint = Checkpoint {
            identity,
            result_version: 1,
            epoch: "server".into(),
            binding: None,
            activity: 1,
            input: 0,
            progress_unix_ms: u64::MAX,
            unresolved: false,
        };
        checkpoint.expire_future(100);
        assert_eq!(checkpoint.progress_unix_ms, 0);
        assert!(checkpoint.unresolved);
        checkpoint.expire_future(200);
        assert_eq!(
            checkpoint.progress_unix_ms, 0,
            "repeated polls cannot reset deadline"
        );
    }
    #[test]
    fn notification_before_wait_is_not_lost() {
        let observations = Observations::default();
        observations.invalidate(SessionId(1));
        let start = std::time::Instant::now();
        observations.wait(0, Duration::from_secs(1));
        assert!(start.elapsed() < Duration::from_millis(100));
    }
}

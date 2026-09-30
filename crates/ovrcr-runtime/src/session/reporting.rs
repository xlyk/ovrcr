use super::*;
use ovrcr_protocol::*;

#[derive(Default)]
pub(super) struct ReportingState {
    pub epoch: u64,
    generation: u64,
    lease: Option<AgentSecret>,
    reservation: Option<(ReserveAgent, AgentReservation)>,
    latest: Option<(SupervisorRequest, AgentOperationResult)>,
    owner: Option<Arc<()>>,
    pub snapshot: Option<AgentSnapshot>,
    measurements: MeasurementWatermarks,
    quota: Option<(AgentBinding, ProviderQuota, u64)>,
    // One turn only, retained across reporting resets for the server lifetime.
    ready: Option<(ReadyObservation, bool)>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl ReportingState {
    pub fn unread(&self) -> Option<ReadyObservation> {
        self.ready
            .as_ref()
            .filter(|(_, reviewed)| !reviewed)
            .map(|(ready, _)| ready.clone())
    }

    fn mark_reviewed(&mut self, expected: &ReadyObservation) -> Result<()> {
        expected.validate()?;
        let Some((ready, reviewed)) = &mut self.ready else {
            bail!("Ready observation is no longer available");
        };
        if ready != expected {
            bail!("Ready observation changed; inspect the unread response before reviewing it");
        }
        *reviewed = true;
        Ok(())
    }

    pub fn active(&self) -> bool {
        self.lease.is_some()
    }
    fn binding(&self) -> Option<&AgentBinding> {
        self.snapshot.as_ref().map(|s| &s.binding)
    }
    fn authenticate(&self, auth: &SupervisorAuth) -> Result<()> {
        if self.lease.as_ref() != Some(&auth.lease) {
            bail!("supervisor lease is stale or invalid");
        }
        Ok(())
    }
    fn expected(&self, expected: Option<&AgentBinding>) -> Result<()> {
        if self.binding() != expected {
            bail!("agent binding changed");
        }
        Ok(())
    }
    pub fn unavailable(&mut self, reason: &str) {
        if let Some(snapshot) = &mut self.snapshot {
            snapshot.health = HealthSample {
                state: ReporterHealth::Unavailable,
                reason: Some(reason.into()),
            };
            if let Some(metrics) = &mut snapshot.metrics {
                metrics.sample.usage.value.coverage = UsageCoverage::Partial;
            }
            // A reporter that is gone cannot vouch for a visible dialog; the request is
            // unknown until a fresh authoritative event.
            snapshot.input_requests.clear();
        }
    }
    pub fn lost(&mut self, reason: &str) {
        if self.active() {
            self.unavailable(reason);
            self.lease = None;
            self.owner = None;
        }
    }
    pub fn apply(&mut self, report: &ProviderReport, health_authorized: bool) -> Result<bool> {
        report.validate()?;
        if !self.active() {
            bail!("no active supervisor");
        }
        self.expected(Some(&report.binding))?;
        let snapshot = self.snapshot.as_mut().context("agent is not bound")?;
        match &report.observation {
            AgentObservation::Activity(activity) => {
                if report.revision <= snapshot.activity_revision {
                    bail!("stale activity revision");
                }
                if report.binding.provider.supports_readiness()
                    && activity.state == AgentActivity::ResponseReady
                    && matches!(
                        activity.quality,
                        SampleQuality::Observed | SampleQuality::Confirmed
                    )
                    && activity.turn.is_some()
                    && snapshot.health.state == ReporterHealth::Connected
                    && self.ready.as_ref().is_none_or(|(ready, _)| {
                        ready.binding != report.binding || ready.turn != activity.turn
                    })
                {
                    self.ready = Some((
                        ReadyObservation {
                            binding: report.binding.clone(),
                            turn: activity.turn.clone(),
                            activity_revision: report.revision,
                        },
                        false,
                    ));
                }
                snapshot.activity = Some(activity.clone());
                snapshot.activity_revision = report.revision;
            }
            AgentObservation::Metrics(metrics) => {
                if report.revision <= snapshot.metrics_revision {
                    bail!("stale metrics revision");
                }
                let mut measurements = self.measurements.clone();
                let mut next = measurements.replace(metrics, now_ms())?;
                if snapshot.health.state == ReporterHealth::Unavailable {
                    next.sample.usage.value.coverage = UsageCoverage::Partial;
                }
                self.measurements = measurements;
                snapshot.metrics = Some(next);
                snapshot.metrics_revision = report.revision;
            }
            AgentObservation::Health(health) => {
                if !health_authorized {
                    bail!("health reports require the supervisor lease");
                }
                if report.revision <= snapshot.health_revision {
                    bail!("stale health revision");
                }
                snapshot.health = health.clone();
                snapshot.health_revision = report.revision;
                // A reporter that is gone cannot vouch for a visible dialog.
                if health.state == ReporterHealth::Unavailable {
                    snapshot.input_requests.clear();
                }
            }
            AgentObservation::Input(requests) => {
                if report.revision <= snapshot.input_revision {
                    bail!("stale input revision");
                }
                // The set is published whole: the snapshot is replaced, never merged.
                snapshot.input_requests = requests.clone();
                snapshot.input_revision = report.revision;
            }
            AgentObservation::Quota(incoming) => {
                if self.quota.as_ref().is_some_and(|(binding, _, revision)| {
                    binding == &report.binding && report.revision <= *revision
                }) {
                    bail!("stale quota revision");
                }
                let mut quota = self
                    .quota
                    .as_ref()
                    .filter(|(binding, _, _)| binding == &report.binding)
                    .map(|(_, quota, _)| quota.clone())
                    .unwrap_or_else(|| {
                        ProviderQuota::unknown(QuotaProvider::Claude, QuotaState::Waiting)
                    });
                if let Some(windows) = &incoming.windows {
                    if quota.observed_unix_ms.is_none() || quota.windows != *windows {
                        quota.observed_unix_ms = Some(now_ms());
                    }
                    quota.windows = windows.clone();
                }
                quota.state = incoming.state;
                self.quota = Some((report.binding.clone(), quota, report.revision));
            }
        }
        Ok(true)
    }
}

#[derive(Clone, Default)]
struct MeasurementWatermarks {
    context: Option<(Measurement<ContextSample>, u64)>,
    usage: Option<(Measurement<UsageTotals>, u64)>,
    cost: Option<(Measurement<Option<UsageCost>>, u64)>,
    usage_totals: [Option<UsageTotals>; 2],
    costs: [Option<u64>; 2],
}
impl MeasurementWatermarks {
    fn replace(&mut self, new: &MetricsSample, now: u64) -> Result<MetricsSnapshot> {
        new.validate()?;
        let scope_index = |scope| match scope {
            UsageScope::Conversation => 0,
            UsageScope::Invocation => 1,
        };
        let incoming = &new.usage.value;
        let mut totals = self.usage_totals[scope_index(incoming.scope)]
            .clone()
            .unwrap_or_else(|| incoming.clone());
        for (old, next) in [
            (&mut totals.input_tokens, incoming.input_tokens),
            (&mut totals.output_tokens, incoming.output_tokens),
            (&mut totals.cache_read_tokens, incoming.cache_read_tokens),
            (&mut totals.cache_write_tokens, incoming.cache_write_tokens),
            (
                &mut totals.reasoning_output_tokens,
                incoming.reasoning_output_tokens,
            ),
        ] {
            if let Some(next) = next {
                if old.is_some_and(|old| next < old) {
                    bail!(
                        "cumulative usage decrease requires a new binding or certified correction"
                    );
                }
                *old = Some(next);
            }
        }
        self.usage_totals[scope_index(incoming.scope)] = Some(totals);
        if let Some(cost) = &new.cost.value {
            let old = &mut self.costs[scope_index(cost.scope)];
            if old.is_some_and(|old| cost.usd_ticks < old) {
                bail!("cumulative cost decreased");
            }
            *old = Some(cost.usd_ticks);
        }
        Ok(MetricsSnapshot {
            sample: new.clone(),
            received_unix_ms: now,
            context_received_unix_ms: freshness::received_at(&mut self.context, &new.context, now),
            usage_received_unix_ms: freshness::received_at(&mut self.usage, &new.usage, now),
            cost_received_unix_ms: freshness::received_at(&mut self.cost, &new.cost, now),
        })
    }
}

impl Session {
    pub(crate) fn quota_snapshot(&self) -> Option<ProviderQuota> {
        let state = self.state.lock().unwrap();
        let snapshot = state.reporting.snapshot.as_ref()?;
        let (binding, quota, _) = state.reporting.quota.as_ref()?;
        if binding != &snapshot.binding {
            return None;
        }
        let mut quota = quota.clone();
        quota.source = Some(QuotaSource::Session {
            session: self.summary.id,
            run: self.run(),
            binding: binding.clone(),
        });
        if !state.phase.is_live() || snapshot.health.state != ReporterHealth::Connected {
            quota.state = QuotaState::Unavailable;
        }
        Some(quota)
    }

    pub(crate) fn agent_command(
        &self,
        request: &Request,
        owner: Option<&Arc<()>>,
        persist: impl FnOnce(&AgentCommand) -> Result<()>,
    ) -> Result<Response> {
        let mut state = self.state.lock().unwrap();
        if let Request::MarkReviewed { expected, .. } = request {
            state.reporting.mark_reviewed(expected)?;
            return Ok(Response::Ok);
        }
        if matches!(state.phase, SessionPhase::Exited { .. }) || state.hook_capability.is_none() {
            bail!("agent session is unavailable");
        }
        if let Request::ReserveAgent(reserve) = request {
            if reserve.session != self.summary.id
                || state.hook_capability != Some(reserve.capability.0)
            {
                bail!("agent reservation rejected");
            }
            validate_agent_id(&reserve.operation)?;
            validate_agent_id(&reserve.invocation)?;
            let reporting = &mut state.reporting;
            // Recovery requires the same unguessable operation ID AND original arguments.
            if let Some((original, receipt)) = &reporting.reservation
                && original.operation == reserve.operation
            {
                if original != reserve {
                    bail!("reservation operation reused with different arguments");
                }
                if !reporting.active() {
                    return Ok(Response::AgentOperation(AgentOperationResult::Released));
                }
                reporting.owner = Some(
                    owner
                        .context("reservation requires a watched connection")?
                        .clone(),
                );
                return Ok(Response::AgentOperation(AgentOperationResult::Reserved(
                    receipt.clone(),
                )));
            }
            if reporting.active() || reserve.expected_epoch != reporting.epoch {
                bail!("reporting epoch changed or lease is active");
            }
            let owner = owner.context("reservation requires a watched connection")?;
            let epoch = reporting
                .epoch
                .checked_add(1)
                .context("reporting epoch exhausted")?;
            let mut secret = [0; 32];
            use std::io::Read;
            std::fs::File::open("/dev/urandom")?.read_exact(&mut secret)?;
            let receipt = AgentReservation {
                epoch,
                lease: AgentSecret(secret),
            };
            reporting.epoch = epoch;
            reporting.lease = Some(receipt.lease.clone());
            reporting.reservation = Some((reserve.clone(), receipt.clone()));
            reporting.latest = None;
            reporting.owner = Some(owner.clone());
            reporting.snapshot = None;
            reporting.measurements = MeasurementWatermarks::default();
            return Ok(Response::AgentOperation(AgentOperationResult::Reserved(
                receipt,
            )));
        }
        let (auth, operation) = match request {
            Request::Supervisor(r) => (&r.auth, Some(r.operation.as_str())),
            Request::AgentStatus { auth, operation } => (auth, Some(operation.as_str())),
            Request::SupervisorHello(auth) => (auth, None),
            _ => bail!("not a supervisor request"),
        };
        if auth.session != self.summary.id {
            bail!("wrong supervisor session");
        }
        if let Some(operation) = operation {
            validate_agent_id(operation)?;
        }
        let reporting = &mut state.reporting;
        // Retain the final operation receipt after release so an acknowledgement loss is recoverable.
        if let Some((original, result)) = &reporting.latest
            && Some(original.operation.as_str()) == operation
            && original.auth == *auth
        {
            match request {
                Request::AgentStatus { .. } => {
                    return Ok(Response::AgentOperation(result.clone()));
                }
                Request::Supervisor(retry) if retry == original => {
                    return Ok(Response::AgentOperation(result.clone()));
                }
                _ => bail!("operation ID was reused with different arguments"),
            }
        }
        reporting.authenticate(auth)?;
        if matches!(request, Request::SupervisorHello(_)) {
            let owner = owner.context("supervisor requires a dedicated connection")?;
            reporting.owner = Some(owner.clone());
            return Ok(Response::Ok);
        }
        let Request::Supervisor(r) = request else {
            bail!("operation receipt is unavailable or stale");
        };
        let result = match &r.command {
            AgentCommand::InvalidateConversation => {
                // Invalidation concerns the entire authenticated invocation, even
                // when a successful Bind receipt was lost before this callback.
                if reporting
                    .reservation
                    .as_ref()
                    .is_none_or(|(reserve, _)| !crate::recovery::supported(reserve.provider))
                {
                    bail!("managed invocation does not support conversation recovery");
                }
                persist(&r.command)?;
                AgentOperationResult::ConversationInvalidated
            }
            AgentCommand::RetainConversation { binding, reference } => {
                reporting.expected(Some(binding))?;
                if !reference.matches_binding(binding) {
                    bail!("Recovery must match the certified provider binding");
                }
                crate::recovery::validate(reference)?;
                persist(&r.command)?;
                AgentOperationResult::ConversationRetained
            }
            AgentCommand::Bind {
                expected_binding,
                conversation,
            } => {
                validate_agent_id(conversation)?;
                reporting.expected(expected_binding.as_ref())?;
                let generation = reporting
                    .generation
                    .checked_add(1)
                    .context("binding generation exhausted")?;
                let reservation = &reporting
                    .reservation
                    .as_ref()
                    .context("missing reservation")?
                    .0;
                let binding = AgentBinding {
                    provider: reservation.provider,
                    invocation: reservation.invocation.clone(),
                    conversation: conversation.clone(),
                    generation,
                };
                reporting.generation = generation;
                reporting.measurements = MeasurementWatermarks::default();
                reporting.snapshot = Some(AgentSnapshot {
                    binding: binding.clone(),
                    activity: None,
                    metrics: None,
                    health: HealthSample {
                        state: ReporterHealth::Connected,
                        reason: None,
                    },
                    activity_revision: 0,
                    metrics_revision: 0,
                    health_revision: 0,
                    input_requests: Vec::new(),
                    input_revision: 0,
                });
                AgentOperationResult::Bound(binding)
            }
            AgentCommand::Release { expected_binding } => {
                reporting.expected(expected_binding.as_ref())?;
                reporting.lost("unfinalized_release");
                AgentOperationResult::Released
            }
            AgentCommand::Finalize {
                binding,
                revision,
                final_metrics,
            } => {
                reporting.apply(
                    &ProviderReport {
                        binding: binding.clone(),
                        revision: *revision,
                        observation: AgentObservation::Metrics(final_metrics.clone()),
                    },
                    false,
                )?;
                if final_metrics.usage.value.coverage == UsageCoverage::Partial {
                    reporting.unavailable("incomplete_final_accounting");
                }
                reporting.lease = None;
                reporting.owner = None;
                AgentOperationResult::Released
            }
            AgentCommand::Health(report) => {
                if !matches!(report.observation, AgentObservation::Health(_)) {
                    bail!("expected supervisor health observation");
                }
                reporting.apply(report, true)?;
                if let AgentObservation::Health(health) = &report.observation
                    && health.state == ReporterHealth::Unavailable
                    && let Some(metrics) =
                        reporting.snapshot.as_mut().and_then(|s| s.metrics.as_mut())
                {
                    metrics.sample.usage.value.coverage = UsageCoverage::Partial;
                }
                AgentOperationResult::HealthUpdated
            }
        };
        reporting.latest = Some((r.clone(), result.clone()));
        Ok(Response::AgentOperation(result))
    }

    pub(crate) fn agent_supervisor_disconnected(&self, owner: &Arc<()>) -> bool {
        let mut state = self.state.lock().unwrap();
        if !state
            .reporting
            .owner
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, owner))
        {
            return false;
        }
        state.reporting.lost("supervisor_disconnected");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unread_requires_supported_provider_ready_with_identified_turn() {
        for (provider, quality, turn, health, expected) in [
            (
                AgentProvider::Codex,
                SampleQuality::Observed,
                Some("one"),
                ReporterHealth::Connected,
                true,
            ),
            (
                AgentProvider::Claude,
                SampleQuality::Observed,
                Some("one"),
                ReporterHealth::Connected,
                true,
            ),
            (
                AgentProvider::Codex,
                SampleQuality::Confirmed,
                Some("one"),
                ReporterHealth::Connected,
                true,
            ),
            (
                AgentProvider::Codex,
                SampleQuality::Estimated,
                Some("one"),
                ReporterHealth::Connected,
                false,
            ),
            (
                AgentProvider::Codex,
                SampleQuality::Observed,
                None,
                ReporterHealth::Connected,
                false,
            ),
            (
                AgentProvider::Codex,
                SampleQuality::Observed,
                Some("one"),
                ReporterHealth::Unavailable,
                false,
            ),
            (
                AgentProvider::Pi,
                SampleQuality::Confirmed,
                Some("one"),
                ReporterHealth::Connected,
                true,
            ),
            (
                AgentProvider::Pi,
                SampleQuality::Observed,
                Some("one"),
                ReporterHealth::Connected,
                true,
            ),
            (
                AgentProvider::Omp,
                SampleQuality::Observed,
                Some("one"),
                ReporterHealth::Connected,
                true,
            ),
            (
                AgentProvider::Grok,
                SampleQuality::Observed,
                Some("one"),
                ReporterHealth::Connected,
                false,
            ),
            (
                AgentProvider::Hermes,
                SampleQuality::Confirmed,
                Some("one"),
                ReporterHealth::Connected,
                false,
            ),
        ] {
            let binding = AgentBinding {
                provider,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            };
            let mut reporting = ReportingState {
                lease: Some(AgentSecret([1; 32])),
                snapshot: Some(AgentSnapshot {
                    binding: binding.clone(),
                    activity: None,
                    metrics: None,
                    health: HealthSample {
                        state: health,
                        reason: None,
                    },
                    activity_revision: 0,
                    metrics_revision: 0,
                    health_revision: 0,
                    input_requests: Vec::new(),
                    input_revision: 0,
                }),
                ..Default::default()
            };
            reporting
                .apply(
                    &ProviderReport {
                        binding,
                        revision: 1,
                        observation: AgentObservation::Activity(ActivitySample {
                            state: AgentActivity::ResponseReady,
                            quality,
                            turn: turn.map(str::to_owned),
                        }),
                    },
                    false,
                )
                .unwrap();
            assert_eq!(
                reporting.unread().is_some(),
                expected,
                "{provider:?} {quality:?} {turn:?} {health:?}"
            );
        }
    }

    fn pi_state() -> (ReportingState, AgentBinding) {
        let binding = AgentBinding {
            provider: AgentProvider::Pi,
            invocation: "inv".into(),
            conversation: "conv".into(),
            generation: 1,
        };
        let reporting = ReportingState {
            lease: Some(AgentSecret([1; 32])),
            snapshot: Some(AgentSnapshot {
                binding: binding.clone(),
                activity: None,
                metrics: None,
                health: HealthSample {
                    state: ReporterHealth::Connected,
                    reason: None,
                },
                activity_revision: 0,
                metrics_revision: 0,
                health_revision: 0,
                input_requests: Vec::new(),
                input_revision: 0,
            }),
            ..Default::default()
        };
        (reporting, binding)
    }
    fn activity(
        binding: &AgentBinding,
        revision: u64,
        state: AgentActivity,
        quality: SampleQuality,
    ) -> ProviderReport {
        ProviderReport {
            binding: binding.clone(),
            revision,
            observation: AgentObservation::Activity(ActivitySample {
                state,
                quality,
                turn: Some("one".into()),
            }),
        }
    }
    fn input(
        binding: &AgentBinding,
        revision: u64,
        requests: &[(&str, InputKind)],
    ) -> ProviderReport {
        ProviderReport {
            binding: binding.clone(),
            revision,
            observation: AgentObservation::Input(
                requests
                    .iter()
                    .map(|(id, kind)| InputRequest {
                        id: (*id).into(),
                        kind: *kind,
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn input_request_opens_and_closes_atomically_without_touching_activity_or_unread() {
        let (mut reporting, binding) = pi_state();
        reporting
            .apply(
                &activity(&binding, 1, AgentActivity::Busy, SampleQuality::Observed),
                false,
            )
            .unwrap();
        reporting
            .apply(
                &activity(
                    &binding,
                    2,
                    AgentActivity::ResponseReady,
                    SampleQuality::Confirmed,
                ),
                false,
            )
            .unwrap();
        let unread = reporting.unread().expect("Confirmed Ready is unread");

        reporting
            .apply(&input(&binding, 3, &[("a:p1", InputKind::Select)]), false)
            .unwrap();
        let snapshot = reporting.snapshot.as_ref().unwrap();
        assert_eq!(
            snapshot.input_requests,
            vec![InputRequest {
                id: "a:p1".into(),
                kind: InputKind::Select,
            }]
        );
        assert_eq!(
            snapshot.activity.as_ref().unwrap().state,
            AgentActivity::ResponseReady,
            "a wait never overwrites the underlying activity sample"
        );
        assert_eq!(snapshot.activity_revision, 2);
        assert_eq!(snapshot.input_revision, 3);
        assert_eq!(snapshot.effective_activity(), AgentActivity::WaitingInput);
        assert_eq!(reporting.unread(), Some(unread.clone()));

        let before = reporting.snapshot.clone();
        let stale = reporting
            .apply(&input(&binding, 3, &[("a:p1", InputKind::Select)]), false)
            .unwrap_err();
        assert_eq!(stale.to_string(), "stale input revision");
        assert_eq!(reporting.snapshot, before, "a stale open changes nothing");

        reporting.apply(&input(&binding, 4, &[]), false).unwrap();
        let snapshot = reporting.snapshot.as_ref().unwrap();
        assert!(snapshot.input_requests.is_empty());
        assert_eq!(
            snapshot.effective_activity(),
            AgentActivity::ResponseReady,
            "closing the last request restores what was underneath"
        );
        assert_eq!(reporting.unread(), Some(unread.clone()));

        reporting
            .apply(&input(&binding, 5, &[("a:p2", InputKind::Editor)]), false)
            .unwrap();
        reporting
            .apply(
                &activity(&binding, 3, AgentActivity::Busy, SampleQuality::Observed),
                false,
            )
            .unwrap();
        let snapshot = reporting.snapshot.as_ref().unwrap();
        assert_eq!(
            snapshot.activity.as_ref().unwrap().state,
            AgentActivity::Busy
        );
        assert_eq!(
            snapshot.effective_activity(),
            AgentActivity::WaitingInput,
            "activity under an open request is recorded, not effective"
        );
        assert_eq!(reporting.unread(), Some(unread));
    }

    #[test]
    fn a_published_request_set_replaces_the_snapshot_and_waits_for_its_last_member() {
        let (mut reporting, binding) = pi_state();
        reporting
            .apply(
                &activity(&binding, 1, AgentActivity::Busy, SampleQuality::Observed),
                false,
            )
            .unwrap();
        reporting
            .apply(
                &input(
                    &binding,
                    2,
                    &[
                        ("approval:c1", InputKind::Approval),
                        ("question:q1", InputKind::Select),
                    ],
                ),
                false,
            )
            .unwrap();
        let snapshot = reporting.snapshot.as_ref().unwrap();
        assert_eq!(
            snapshot
                .input_requests
                .iter()
                .map(|request| request.id.as_str())
                .collect::<Vec<_>>(),
            ["approval:c1", "question:q1"],
            "the set is stored in the published order, oldest first"
        );
        assert_eq!(snapshot.effective_activity(), AgentActivity::WaitingInput);

        // One member closing publishes the remainder whole: still waiting.
        reporting
            .apply(
                &input(&binding, 3, &[("question:q1", InputKind::Select)]),
                false,
            )
            .unwrap();
        let snapshot = reporting.snapshot.as_ref().unwrap();
        assert_eq!(
            snapshot.input_requests,
            vec![InputRequest {
                id: "question:q1".into(),
                kind: InputKind::Select,
            }],
            "the snapshot is replaced by the new set, never merged with the old one"
        );
        assert_eq!(snapshot.effective_activity(), AgentActivity::WaitingInput);

        reporting.apply(&input(&binding, 4, &[]), false).unwrap();
        let snapshot = reporting.snapshot.as_ref().unwrap();
        assert!(snapshot.input_requests.is_empty());
        assert_eq!(
            snapshot.effective_activity(),
            AgentActivity::Busy,
            "the last close restores what was underneath"
        );
    }

    #[test]
    fn an_unavailable_health_report_clears_the_open_input_request() {
        let (mut reporting, binding) = pi_state();
        reporting
            .apply(
                &activity(&binding, 1, AgentActivity::Busy, SampleQuality::Observed),
                false,
            )
            .unwrap();
        reporting
            .apply(&input(&binding, 2, &[("a:p1", InputKind::Select)]), false)
            .unwrap();
        reporting
            .apply(
                &ProviderReport {
                    binding: binding.clone(),
                    revision: 3,
                    observation: AgentObservation::Health(HealthSample {
                        state: ReporterHealth::Unavailable,
                        reason: Some("collector_lost".into()),
                    }),
                },
                true,
            )
            .unwrap();
        let snapshot = reporting.snapshot.as_ref().unwrap();
        assert_eq!(snapshot.health.state, ReporterHealth::Unavailable);
        assert!(snapshot.input_requests.is_empty());
        assert_eq!(
            snapshot.effective_activity(),
            AgentActivity::Busy,
            "the underlying sample is untouched"
        );
        // Health coming back is not a fresh authoritative dialog event.
        reporting
            .apply(
                &ProviderReport {
                    binding,
                    revision: 4,
                    observation: AgentObservation::Health(HealthSample {
                        state: ReporterHealth::Connected,
                        reason: None,
                    }),
                },
                true,
            )
            .unwrap();
        assert!(
            reporting
                .snapshot
                .as_ref()
                .unwrap()
                .input_requests
                .is_empty()
        );
    }

    #[test]
    fn reporter_loss_clears_the_open_input_request() {
        let (mut reporting, binding) = pi_state();
        reporting
            .apply(
                &activity(
                    &binding,
                    1,
                    AgentActivity::ResponseReady,
                    SampleQuality::Confirmed,
                ),
                false,
            )
            .unwrap();
        let unread = reporting.unread().expect("Confirmed Ready is unread");
        reporting
            .apply(&input(&binding, 2, &[("a:p1", InputKind::Confirm)]), false)
            .unwrap();
        reporting.lost("supervisor_disconnected");
        let snapshot = reporting.snapshot.as_ref().unwrap();
        assert_eq!(snapshot.health.state, ReporterHealth::Unavailable);
        assert_eq!(
            snapshot.health.reason.as_deref(),
            Some("supervisor_disconnected")
        );
        assert_eq!(
            snapshot.input_requests,
            Vec::new(),
            "a reporter that is gone cannot vouch for a visible dialog"
        );
        assert_eq!(
            snapshot.effective_activity(),
            AgentActivity::ResponseReady,
            "loss does not rewrite the activity sample"
        );
        assert_eq!(
            reporting.unread(),
            Some(unread),
            "reporter loss keeps Unread"
        );
    }

    #[test]
    fn changed_measurement_advances_its_receipt_stamp_while_a_replay_keeps_it() {
        const START: u64 = 1_000;
        const SIX_MINUTES: u64 = 360_000;
        fn measurement<T>(value: T) -> Measurement<T> {
            Measurement {
                value,
                source: "fixture".into(),
            }
        }
        let sample = |input_tokens| MetricsSample {
            model: None,
            context: measurement(ContextSample {
                used_tokens: Some(10),
                capacity_tokens: Some(100),
                quality: SampleQuality::Observed,
            }),
            usage: measurement(UsageTotals {
                scope: UsageScope::Conversation,
                coverage: UsageCoverage::Complete,
                input_tokens: Some(input_tokens),
                output_tokens: Some(1),
                cache_read_tokens: None,
                cache_write_tokens: None,
                reasoning_output_tokens: None,
            }),
            cost: measurement(None),
        };
        let mut watermarks = MeasurementWatermarks::default();
        let first = watermarks.replace(&sample(10), START).unwrap();
        assert_eq!(first.usage_received_unix_ms, START);
        assert_eq!(first.context_received_unix_ms, START);

        // A timer replay of an unchanged sample keeps every stamp, so six
        // minutes of replaying one sample really is a stale reading.
        let later = START + SIX_MINUTES;
        let replay = watermarks.replace(&sample(10), later).unwrap();
        assert_eq!(
            replay.usage_received_unix_ms, START,
            "replay keeps its stamp"
        );
        assert!(freshness::is_stale(
            replay.usage_received_unix_ms,
            later,
            false
        ));

        // A sample that actually differs advances its own stamp and nothing else.
        let changed = watermarks.replace(&sample(20), later).unwrap();
        assert_eq!(
            changed.usage_received_unix_ms, later,
            "a changed usage sample advances its receipt stamp"
        );
        assert_eq!(
            changed.context_received_unix_ms, START,
            "an unchanged context sample keeps its own stamp"
        );
        assert!(
            !freshness::is_stale(changed.usage_received_unix_ms, later, false),
            "a session whose totals keep moving is never stale"
        );
        assert!(freshness::is_stale(
            changed.context_received_unix_ms,
            later,
            false
        ));
        assert!(
            freshness::is_stale(changed.usage_received_unix_ms, later, true),
            "an exited session is stale whatever its stamps say"
        );

        // `received_at` compares whole samples, so the reporter label counts:
        // the same number arriving under a different source is a new reading,
        // not a replay. Production runs one source per component per binding,
        // so this only decides the ambiguous case, and it decides it the safe
        // way -- a fresh reading is never reported as old.
        let relabelled_at = later + 1_000;
        let mut relabelled = sample(20);
        relabelled.usage.source = "second_fixture".into();
        let relabelled = watermarks.replace(&relabelled, relabelled_at).unwrap();
        assert_eq!(
            relabelled.usage_received_unix_ms, relabelled_at,
            "a source change with an unchanged value takes a new stamp"
        );
        assert_eq!(
            relabelled.context_received_unix_ms, START,
            "and it moves nothing else"
        );
    }
}

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
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl ReportingState {
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
                snapshot.activity = Some(activity.clone());
                snapshot.activity_revision = report.revision;
            }
            AgentObservation::Metrics(metrics) => {
                if report.revision <= snapshot.metrics_revision {
                    bail!("stale metrics revision");
                }
                let mut measurements = self.measurements.clone();
                let mut next =
                    measurements.replace(snapshot.metrics.as_ref(), metrics, now_ms())?;
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
fn measurement_age<T: Clone + PartialEq>(
    watermark: &mut Option<(Measurement<T>, u64)>,
    new: &Measurement<T>,
    previous_age: Option<u64>,
    now: u64,
) -> Result<u64> {
    if new.freshness == MeasurementFreshness::Uncertain {
        // Unknown/uncertain replacement does not erase the certified ordering watermark.
        return Ok(previous_age.unwrap_or(now));
    }
    if let Some((old, age)) = watermark {
        if old.source != new.source {
            bail!("identified component source changed within binding");
        }
        if new.source_sequence < old.source_sequence {
            bail!("stale component source sequence");
        }
        if new.source_sequence == old.source_sequence {
            if old != new {
                bail!("conflicting component reuses source sequence");
            }
            return Ok(*age);
        }
        if new.source_revision == old.source_revision {
            bail!("source identity reused with a new sequence");
        }
    }
    *watermark = Some((new.clone(), now));
    Ok(now)
}
impl MeasurementWatermarks {
    fn replace(
        &mut self,
        old: Option<&MetricsSnapshot>,
        new: &MetricsSample,
        now: u64,
    ) -> Result<MetricsSnapshot> {
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
        let context_age = measurement_age(
            &mut self.context,
            &new.context,
            old.map(|o| o.context_received_unix_ms),
            now,
        )?;
        let usage_age = measurement_age(
            &mut self.usage,
            &new.usage,
            old.map(|o| o.usage_received_unix_ms),
            now,
        )?;
        let cost_age = measurement_age(
            &mut self.cost,
            &new.cost,
            old.map(|o| o.cost_received_unix_ms),
            now,
        )?;
        Ok(MetricsSnapshot {
            sample: new.clone(),
            received_unix_ms: now,
            context_received_unix_ms: context_age,
            usage_received_unix_ms: usage_age,
            cost_received_unix_ms: cost_age,
        })
    }
}

impl Session {
    pub(crate) fn agent_command(
        &self,
        request: &Request,
        owner: Option<&Arc<()>>,
    ) -> Result<Response> {
        let mut state = self.state.lock().unwrap();
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
    fn agent_report_component_receipt_age_uses_source_order_not_transport_time() {
        let first = Measurement {
            value: Some(5_u64),
            source: "fixture".into(),
            source_revision: Some("one".into()),
            source_sequence: Some(1),
            freshness: MeasurementFreshness::SourceIdentified,
        };
        let mut watermark = None;
        assert_eq!(
            measurement_age(&mut watermark, &first, None, 100).unwrap(),
            100
        );
        assert_eq!(
            measurement_age(&mut watermark, &first, Some(100), 999).unwrap(),
            100
        );
        let uncertain = Measurement {
            value: None,
            source: "fixture".into(),
            source_revision: None,
            source_sequence: None,
            freshness: MeasurementFreshness::Uncertain,
        };
        assert_eq!(
            measurement_age(&mut watermark, &uncertain, Some(100), 1000).unwrap(),
            100
        );
        let second = Measurement {
            source_revision: Some("two".into()),
            source_sequence: Some(2),
            ..first.clone()
        };
        assert_eq!(
            measurement_age(&mut watermark, &second, Some(100), 1200).unwrap(),
            1200
        );
        assert!(measurement_age(&mut watermark, &first, Some(1200), 1500).is_err());
        assert_eq!(watermark.unwrap().1, 1200);
    }
}

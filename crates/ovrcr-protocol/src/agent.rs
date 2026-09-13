//! Provider-independent reporting and supervisor ownership wire contracts.
use crate::{AgentActivity, SessionId};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentProvider {
    Claude,
    Codex,
    Grok,
    Pi,
    Hermes,
    /// Oh My Pi. Appended last: bincode numbers variants by declaration order.
    Omp,
}

impl AgentProvider {
    /// The providers whose root response cycles may become Ready, Unread, a review
    /// target, and an alert. One owner for what used to be four Codex-pinned checks.
    pub const fn supports_readiness(self) -> bool {
        matches!(
            self,
            AgentProvider::Codex | AgentProvider::Pi | AgentProvider::Omp
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleQuality {
    Confirmed,
    Observed,
    Estimated,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageScope {
    Conversation,
    Invocation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageCoverage {
    Complete,
    Partial,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CostKind {
    Reported,
    Estimated,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageTotals {
    pub scope: UsageScope,
    pub coverage: UsageCoverage,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageCost {
    pub usd_ticks: u64,
    pub kind: CostKind,
    pub scope: UsageScope,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentBinding {
    pub provider: AgentProvider,
    pub invocation: String,
    pub conversation: String,
    pub generation: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivitySample {
    pub state: AgentActivity,
    pub quality: SampleQuality,
    pub turn: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSample {
    pub used_tokens: Option<u64>,
    pub capacity_tokens: Option<u64>,
    pub quality: SampleQuality,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeasurementFreshness {
    SourceIdentified,
    Uncertain,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measurement<T> {
    pub value: T,
    pub source: String,
    pub source_revision: Option<String>,
    pub source_sequence: Option<u64>,
    pub freshness: MeasurementFreshness,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsSample {
    pub model: Option<String>,
    pub context: Measurement<ContextSample>,
    pub usage: Measurement<UsageTotals>,
    pub cost: Measurement<Option<UsageCost>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReporterHealth {
    Connected,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthSample {
    pub state: ReporterHealth,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentObservation {
    Activity(ActivitySample),
    Metrics(Box<MetricsSample>),
    Health(HealthSample),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderReport {
    pub binding: AgentBinding,
    pub revision: u64,
    pub observation: AgentObservation,
}

/// The first accepted Ready observation for a response cycle of a supported readiness
/// provider. Used as an exact acknowledgement target; a later activity revision does
/// not change this identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadyObservation {
    pub binding: AgentBinding,
    pub turn: Option<String>,
    pub activity_revision: u64,
}

impl ReadyObservation {
    pub fn validate(&self) -> Result<()> {
        self.binding.validate()?;
        optional_id(&self.turn)?;
        if !self.binding.provider.supports_readiness()
            || self.activity_revision == 0
            || self.turn.is_none()
        {
            bail!(
                "review requires a Ready observation from a supported readiness provider with a turn and positive activity revision"
            );
        }
        Ok(())
    }
}

/// Receipt age is per component; transport activity never refreshes a replay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub sample: MetricsSample,
    pub received_unix_ms: u64,
    pub context_received_unix_ms: u64,
    pub usage_received_unix_ms: u64,
    pub cost_received_unix_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub binding: AgentBinding,
    pub activity: Option<ActivitySample>,
    pub metrics: Option<MetricsSnapshot>,
    pub health: HealthSample,
    pub activity_revision: u64,
    pub metrics_revision: u64,
    pub health_revision: u64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSecret(pub [u8; 32]);
impl std::fmt::Debug for AgentSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReserveAgent {
    pub session: SessionId,
    pub capability: AgentSecret,
    pub operation: String,
    pub expected_epoch: u64,
    pub invocation: String,
    pub provider: AgentProvider,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReservation {
    pub epoch: u64,
    pub lease: AgentSecret,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorAuth {
    pub session: SessionId,
    pub lease: AgentSecret,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentCommand {
    Bind {
        expected_binding: Option<AgentBinding>,
        conversation: String,
    },
    Finalize {
        binding: AgentBinding,
        revision: u64,
        final_metrics: Box<MetricsSample>,
    },
    Release {
        expected_binding: Option<AgentBinding>,
    },
    Health(ProviderReport),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorRequest {
    pub auth: SupervisorAuth,
    pub operation: String,
    pub command: AgentCommand,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentOperationResult {
    Reserved(AgentReservation),
    Bound(AgentBinding),
    Released,
    HealthUpdated,
}

pub fn validate_agent_id(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        bail!("agent identifier must contain 1..=256 UTF-8 bytes without control characters");
    }
    Ok(())
}
fn optional_id(value: &Option<String>) -> Result<()> {
    value.as_deref().map(validate_agent_id).transpose()?;
    Ok(())
}
impl AgentBinding {
    pub fn validate(&self) -> Result<()> {
        validate_agent_id(&self.invocation)?;
        validate_agent_id(&self.conversation)?;
        if self.generation == 0 {
            bail!("binding generation must be positive");
        }
        Ok(())
    }
}
impl<T> Measurement<T> {
    fn validate(&self) -> Result<()> {
        validate_agent_id(&self.source)?;
        optional_id(&self.source_revision)?;
        if (self.freshness == MeasurementFreshness::SourceIdentified)
            != (self.source_revision.is_some() && self.source_sequence.is_some_and(|s| s > 0))
        {
            bail!(
                "identified measurements require a source revision; uncertain measurements omit it"
            );
        }
        if self.freshness == MeasurementFreshness::Uncertain
            && (self.source_revision.is_some() || self.source_sequence.is_some())
        {
            bail!("uncertain measurements omit source identity and sequence");
        }
        Ok(())
    }
}
impl MetricsSample {
    pub fn validate(&self) -> Result<()> {
        optional_id(&self.model)?;
        self.context.validate()?;
        self.usage.validate()?;
        self.cost.validate()?;
        if self.context.value.capacity_tokens == Some(0) {
            bail!("context capacity must be positive");
        }
        let u = &self.usage.value;
        if let Some(input) = u.input_tokens {
            let cache = u
                .cache_read_tokens
                .unwrap_or(0)
                .checked_add(u.cache_write_tokens.unwrap_or(0));
            if cache.is_none_or(|cache| cache > input) {
                bail!("cache subsets exceed input tokens");
            }
        }
        if let (Some(reasoning), Some(output)) = (u.reasoning_output_tokens, u.output_tokens)
            && reasoning > output
        {
            bail!("reasoning subset exceeds output tokens");
        }
        Ok(())
    }
}
impl ProviderReport {
    pub fn validate(&self) -> Result<()> {
        self.binding.validate()?;
        if self.revision == 0 {
            bail!("report revision must be positive");
        }
        match &self.observation {
            AgentObservation::Activity(a) => optional_id(&a.turn),
            AgentObservation::Metrics(m) => m.validate(),
            AgentObservation::Health(h) => optional_id(&h.reason),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{Request, Response, read_frame, write_frame};
    fn metrics() -> MetricsSample {
        fn m<T>(value: T) -> Measurement<T> {
            Measurement {
                value,
                source: "fixture".into(),
                source_revision: Some("one".into()),
                source_sequence: Some(1),
                freshness: MeasurementFreshness::SourceIdentified,
            }
        }
        MetricsSample {
            model: None,
            context: m(ContextSample {
                used_tokens: Some(10),
                capacity_tokens: Some(100),
                quality: SampleQuality::Confirmed,
            }),
            usage: m(UsageTotals {
                scope: UsageScope::Conversation,
                coverage: UsageCoverage::Complete,
                input_tokens: Some(10),
                output_tokens: Some(5),
                cache_read_tokens: Some(3),
                cache_write_tokens: Some(2),
                reasoning_output_tokens: Some(1),
            }),
            cost: m(Some(UsageCost {
                usd_ticks: 0,
                kind: CostKind::Estimated,
                scope: UsageScope::Invocation,
            })),
        }
    }
    #[test]
    fn reviewed_observation_validates_identity_and_round_trips_request() {
        let ready = ReadyObservation {
            binding: AgentBinding {
                provider: AgentProvider::Codex,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
            turn: Some("turn".into()),
            activity_revision: 2,
        };
        ready.validate().unwrap();
        let request = Request::MarkReviewed {
            session: SessionId(1),
            expected: ready.clone(),
        };
        let mut frame = Vec::new();
        write_frame(&mut frame, &request).unwrap();
        assert_eq!(
            read_frame::<Request>(&mut frame.as_slice()).unwrap(),
            request
        );
        for invalid in [
            ReadyObservation {
                turn: None,
                ..ready.clone()
            },
            ReadyObservation {
                turn: Some(String::new()),
                ..ready.clone()
            },
            ReadyObservation {
                activity_revision: 0,
                ..ready.clone()
            },
            ReadyObservation {
                binding: AgentBinding {
                    generation: 0,
                    ..ready.binding.clone()
                },
                ..ready.clone()
            },
            ReadyObservation {
                binding: AgentBinding {
                    provider: AgentProvider::Claude,
                    ..ready.binding.clone()
                },
                ..ready
            },
        ] {
            assert!(invalid.validate().is_err());
        }
    }

    #[test]
    fn readiness_providers_are_codex_pi_and_omp_only() {
        for (provider, expected) in [
            (AgentProvider::Codex, true),
            (AgentProvider::Pi, true),
            (AgentProvider::Omp, true),
            (AgentProvider::Claude, false),
            (AgentProvider::Grok, false),
            (AgentProvider::Hermes, false),
        ] {
            assert_eq!(provider.supports_readiness(), expected, "{provider:?}");
        }
    }

    #[test]
    fn ready_observation_validates_for_every_readiness_provider() {
        let base = ReadyObservation {
            binding: AgentBinding {
                provider: AgentProvider::Codex,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
            turn: Some("turn".into()),
            activity_revision: 2,
        };
        for provider in [AgentProvider::Codex, AgentProvider::Pi, AgentProvider::Omp] {
            let ready = ReadyObservation {
                binding: AgentBinding {
                    provider,
                    ..base.binding.clone()
                },
                ..base.clone()
            };
            ready.validate().unwrap();
        }
        for provider in [
            AgentProvider::Claude,
            AgentProvider::Grok,
            AgentProvider::Hermes,
        ] {
            let ready = ReadyObservation {
                binding: AgentBinding {
                    provider,
                    ..base.binding.clone()
                },
                ..base.clone()
            };
            let error = ready.validate().unwrap_err().to_string();
            assert!(
                error.contains("supported readiness provider"),
                "{provider:?}: {error}"
            );
        }
    }

    #[test]
    fn agent_report_validation_and_integer_boundaries() {
        assert!(metrics().validate().is_ok());
        for invalid in ["".into(), "x".repeat(257), "id\n".into()] {
            assert!(validate_agent_id(&invalid).is_err());
        }
        assert!(validate_agent_id(&"x".repeat(256)).is_ok());
        let base = serde_json::to_value(metrics()).unwrap();
        for value in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::from_str("18446744073709551616").unwrap(),
        ] {
            let mut invalid = base.clone();
            invalid["usage"]["value"]["input_tokens"] = value.clone();
            assert!(serde_json::from_value::<MetricsSample>(invalid).is_err());
            let mut invalid = base.clone();
            invalid["cost"]["value"]["usd_ticks"] = value;
            assert!(serde_json::from_value::<MetricsSample>(invalid).is_err());
        }
        for (path, value) in [
            ("cache_read_tokens", 11),
            ("cache_write_tokens", 8),
            ("reasoning_output_tokens", 6),
        ] {
            let mut invalid = base.clone();
            invalid["usage"]["value"][path] = serde_json::json!(value);
            assert!(
                serde_json::from_value::<MetricsSample>(invalid)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut invalid = metrics();
        invalid.context.value.capacity_tokens = Some(0);
        assert!(invalid.validate().is_err());
        invalid = metrics();
        invalid.context.source_sequence = None;
        assert!(invalid.validate().is_err());
        invalid = metrics();
        invalid.context.freshness = MeasurementFreshness::Uncertain;
        assert!(invalid.validate().is_err());
        assert!(serde_json::from_str::<AgentProvider>("\"UnknownProvider\"").is_err());
        assert!(serde_json::from_str::<SampleQuality>("\"Unknown\"").is_err());
        assert!(serde_json::from_str::<UsageScope>("\"Unknown\"").is_err());
        assert!(serde_json::from_str::<UsageCoverage>("\"Unknown\"").is_err());
        assert!(serde_json::from_str::<ReporterHealth>("\"Unknown\"").is_err());
    }
    pub(crate) fn request_fixtures() -> Vec<Request> {
        let auth = SupervisorAuth {
            session: SessionId(1),
            lease: AgentSecret([165; 32]),
        };
        let binding = AgentBinding {
            provider: AgentProvider::Claude,
            invocation: "inv".into(),
            conversation: "conv".into(),
            generation: 1,
        };
        let health = ProviderReport {
            binding: binding.clone(),
            revision: 1,
            observation: AgentObservation::Health(HealthSample {
                state: ReporterHealth::Unavailable,
                reason: Some("collector_lost".into()),
            }),
        };
        let mut requests = vec![
            Request::ReserveAgent(ReserveAgent {
                session: SessionId(1),
                capability: AgentSecret([165; 32]),
                operation: "reserve".into(),
                expected_epoch: 0,
                invocation: "inv".into(),
                provider: AgentProvider::Claude,
            }),
            Request::SupervisorHello(auth.clone()),
            Request::AgentStatus {
                auth: auth.clone(),
                operation: "op".into(),
            },
        ];
        for command in [
            AgentCommand::Bind {
                expected_binding: None,
                conversation: "conv".into(),
            },
            AgentCommand::Finalize {
                binding: binding.clone(),
                revision: 3,
                final_metrics: metrics().into(),
            },
            AgentCommand::Release {
                expected_binding: Some(binding.clone()),
            },
            AgentCommand::Health(health.clone()),
        ] {
            requests.push(Request::Supervisor(SupervisorRequest {
                auth: auth.clone(),
                operation: "op".into(),
                command,
            }));
        }
        for observation in [
            AgentObservation::Activity(ActivitySample {
                state: AgentActivity::Busy,
                quality: SampleQuality::Observed,
                turn: Some("turn".into()),
            }),
            AgentObservation::Metrics(metrics().into()),
            health.observation,
        ] {
            requests.push(Request::AgentReport(crate::AgentReport {
                session: SessionId(1),
                capability: [165; 32],
                sequence: None,
                update: crate::AgentUpdate::Provider(ProviderReport {
                    binding: binding.clone(),
                    revision: 1,
                    observation,
                }),
            }));
        }
        requests
    }
    pub(crate) fn response_fixtures() -> Vec<Response> {
        let auth = SupervisorAuth {
            session: SessionId(1),
            lease: AgentSecret([165; 32]),
        };
        let binding = AgentBinding {
            provider: AgentProvider::Claude,
            invocation: "inv".into(),
            conversation: "conv".into(),
            generation: 1,
        };
        [
            AgentOperationResult::Reserved(AgentReservation {
                epoch: 1,
                lease: auth.lease,
            }),
            AgentOperationResult::Bound(binding),
            AgentOperationResult::Released,
            AgentOperationResult::HealthUpdated,
        ]
        .into_iter()
        .map(Response::AgentOperation)
        .collect()
    }
    #[test]
    fn agent_report_all_variants_roundtrip_and_secret_redaction() {
        for request in request_fixtures() {
            let mut bytes = Vec::new();
            write_frame(&mut bytes, &request).unwrap();
            assert_eq!(
                read_frame::<Request>(&mut bytes.as_slice()).unwrap(),
                request
            );
            assert!(!format!("{request:?}").contains("165"));
        }
        for response in response_fixtures() {
            let mut bytes = Vec::new();
            write_frame(&mut bytes, &response).unwrap();
            assert_eq!(
                read_frame::<Response>(&mut bytes.as_slice()).unwrap(),
                response
            );
            assert!(!format!("{response:?}").contains("165"));
        }
    }
}

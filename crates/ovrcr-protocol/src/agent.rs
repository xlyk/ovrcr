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
    /// Canonical executable/preset name for this provider.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Grok => "grok",
            Self::Pi => "pi",
            Self::Hermes => "hermes",
            Self::Omp => "omp",
        }
    }

    /// Inverse of [`Self::name`]. Unknown labels are not providers.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "claude" => Self::Claude,
            "codex" => Self::Codex,
            "grok" => Self::Grok,
            "pi" => Self::Pi,
            "hermes" => Self::Hermes,
            "omp" => Self::Omp,
            _ => return None,
        })
    }

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
/// A component value and the reporter that produced it. No provider certifies
/// ordering, so how fresh the value is comes from its receipt stamp in
/// [`MetricsSnapshot`], never from the provider (`crate::freshness`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measurement<T> {
    pub value: T,
    pub source: String,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputKind {
    Select,
    Confirm,
    Input,
    Editor,
    Custom,
    /// A native tool-approval prompt. Appended last: bincode numbers variants by
    /// declaration order.
    Approval,
}

/// An open request for a human answer (CONTEXT.md: Input request). Identified within its
/// binding by `id`, whose prefix names the namespace that issued it (`prompt:`,
/// `approval:`, `question:`); carries no content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputRequest {
    pub id: String,
    pub kind: InputKind,
}

impl InputRequest {
    pub fn validate(&self) -> Result<()> {
        validate_agent_id(&self.id)
    }
}

/// The most open Input requests one binding may carry. A producer that would exceed it
/// ignores the new opening; the bound never disables reporting.
pub const MAX_INPUT_REQUESTS: usize = 32;

/// The trust-boundary check for a whole published request set: bounded, each id valid,
/// no id twice.
pub fn validate_input_requests(requests: &[InputRequest]) -> Result<()> {
    if requests.len() > MAX_INPUT_REQUESTS {
        bail!("at most {MAX_INPUT_REQUESTS} open Input requests per binding");
    }
    for (index, request) in requests.iter().enumerate() {
        request.validate()?;
        if requests[..index]
            .iter()
            .any(|earlier| earlier.id == request.id)
        {
            bail!("Input request ids are unique within a binding");
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentObservation {
    Activity(ActivitySample),
    Metrics(Box<MetricsSample>),
    Health(HealthSample),
    /// The complete set of open Input requests for the binding, oldest first; empty closes
    /// all. Published whole so a snapshot is never assembled from deltas. Appended last:
    /// bincode numbers variants by declaration order.
    Input(Vec<InputRequest>),
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
    /// Open Input requests, oldest first (CONTEXT.md: Input request).
    pub input_requests: Vec<InputRequest>,
    pub input_revision: u64,
}

impl AgentSnapshot {
    /// WaitingInput while any Input request is open; otherwise the underlying activity
    /// sample. The one rule the server summary and the Dashboard both use.
    pub fn effective_activity(&self) -> AgentActivity {
        if !self.input_requests.is_empty() {
            AgentActivity::WaitingInput
        } else {
            self.activity
                .as_ref()
                .map_or(AgentActivity::Unknown, |sample| sample.state)
        }
    }
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
        validate_agent_id(&self.source)
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
            AgentObservation::Input(requests) => validate_input_requests(requests),
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
    fn effective_activity_is_waiting_input_while_any_request_is_open() {
        let mut snapshot = AgentSnapshot {
            binding: AgentBinding {
                provider: AgentProvider::Pi,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
            activity: Some(ActivitySample {
                state: AgentActivity::ResponseReady,
                quality: SampleQuality::Confirmed,
                turn: Some("t".into()),
            }),
            metrics: None,
            health: HealthSample {
                state: ReporterHealth::Connected,
                reason: None,
            },
            activity_revision: 3,
            metrics_revision: 0,
            health_revision: 0,
            input_requests: vec![
                InputRequest {
                    id: "approval:c1".into(),
                    kind: InputKind::Approval,
                },
                InputRequest {
                    id: "question:q1".into(),
                    kind: InputKind::Select,
                },
            ],
            input_revision: 4,
        };
        assert_eq!(snapshot.effective_activity(), AgentActivity::WaitingInput);
        snapshot.input_requests.remove(0);
        assert_eq!(
            snapshot.effective_activity(),
            AgentActivity::WaitingInput,
            "the wait holds until the last request closes"
        );
        snapshot.input_requests.clear();
        assert_eq!(
            snapshot.effective_activity(),
            AgentActivity::ResponseReady,
            "closing the last one restores the underlying sample"
        );
        snapshot.activity = None;
        assert_eq!(snapshot.effective_activity(), AgentActivity::Unknown);
    }

    #[test]
    fn input_observation_validates_its_identity() {
        let binding = AgentBinding {
            provider: AgentProvider::Pi,
            invocation: "inv".into(),
            conversation: "conv".into(),
            generation: 1,
        };
        let ok = ProviderReport {
            binding: binding.clone(),
            revision: 1,
            observation: AgentObservation::Input(vec![InputRequest {
                id: "prompt:i:p1".into(),
                kind: InputKind::Editor,
            }]),
        };
        ok.validate().unwrap();
        let close = ProviderReport {
            binding: binding.clone(),
            revision: 2,
            observation: AgentObservation::Input(Vec::new()),
        };
        close.validate().unwrap();
        let bad = ProviderReport {
            binding,
            revision: 3,
            observation: AgentObservation::Input(vec![InputRequest {
                id: "bad\nid".into(),
                kind: InputKind::Custom,
            }]),
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn input_observation_rejects_overflow_and_duplicate_ids() {
        let binding = AgentBinding {
            provider: AgentProvider::Omp,
            invocation: "inv".into(),
            conversation: "conv".into(),
            generation: 1,
        };
        let set = |count: usize| -> Vec<InputRequest> {
            (0..count)
                .map(|n| InputRequest {
                    id: format!("approval:c{n}"),
                    kind: InputKind::Approval,
                })
                .collect()
        };
        let report = |requests: Vec<InputRequest>| ProviderReport {
            binding: binding.clone(),
            revision: 1,
            observation: AgentObservation::Input(requests),
        };
        report(set(MAX_INPUT_REQUESTS)).validate().unwrap();
        let overflow = report(set(MAX_INPUT_REQUESTS + 1))
            .validate()
            .unwrap_err()
            .to_string();
        assert!(overflow.contains("at most 32"), "{overflow}");
        let mut duplicate = set(2);
        duplicate[1].id = duplicate[0].id.clone();
        let error = report(duplicate).validate().unwrap_err().to_string();
        assert!(error.contains("unique"), "{error}");
        let mut invalid = set(2);
        invalid[1].id = "bad\nid".into();
        assert!(report(invalid).validate().is_err());
    }

    #[test]
    fn provider_names_round_trip_the_six_known_executables() {
        for (provider, name) in [
            (AgentProvider::Claude, "claude"),
            (AgentProvider::Codex, "codex"),
            (AgentProvider::Grok, "grok"),
            (AgentProvider::Pi, "pi"),
            (AgentProvider::Hermes, "hermes"),
            (AgentProvider::Omp, "omp"),
        ] {
            assert_eq!(provider.name(), name);
            assert_eq!(AgentProvider::from_name(name), Some(provider));
        }
        assert_eq!(AgentProvider::from_name("aider"), None);
        assert_eq!(AgentProvider::from_name("Claude"), None);
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
        invalid.context.source = String::new();
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
            AgentObservation::Input(vec![InputRequest {
                id: "question:req".into(),
                kind: InputKind::Select,
            }]),
            AgentObservation::Input(Vec::new()),
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

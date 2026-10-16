//! Parent delivery assertions and per-resource archive receipts are separate facts.
use crate::{
    AcceptanceReference, AgentBinding, AssignmentIdentity, PublicationIdentity, SessionRunId,
    WorkflowAuthority,
};
use serde::{Deserialize, Serialize};

pub const MAX_DELIVERIES: usize = 64;
pub const MAX_DELIVERY_RESOURCES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryRef {
    pub request: String,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetentionChoice {
    Keep { reason: String },
    ArchiveEligible,
}

/// Opaque parent-authored indices. These never authorize reads or verify content.
/// Server publications and acceptance use the separate typed fields below.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryEvidenceKind {
    Recovery,
    Review,
    Check,
    Continuation,
    OtherAuthorizedEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryEvidence {
    pub kind: DeliveryEvidenceKind,
    pub reference: String,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryResource {
    pub handle: String,
    pub identity: AssignmentIdentity,
    pub result_version: u32,
    pub obligation_revision: u64,
    pub publication: Option<PublicationIdentity>,
    pub acceptance: Option<AcceptanceReference>,
    pub evidence: Vec<DeliveryEvidence>,
    pub completed_obligations: Vec<String>,
    pub retention: RetentionChoice,
    pub previous: Option<DeliveryRef>,
}

/// An explicit parent assertion, never proof that a person received a message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryDeclaration {
    pub request: String,
    pub workflow: String,
    pub scope: String,
    pub provenance: String,
    pub resources: Vec<DeliveryResource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedDelivery {
    pub reference: DeliveryRef,
    pub authority: WorkflowAuthority,
    pub declaration: DeliveryDeclaration,
}

/// Auth is excluded so replay preserves the original intent after parent transfer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveRequest {
    pub request: String,
    pub handle: String,
    pub expected_identity: AssignmentIdentity,
    pub expected_result_version: u32,
    pub expected_obligation_revision: u64,
    pub expected_control_epoch: u64,
    pub delivery: DeliveryRef,
    pub continuation_of: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveIntent {
    pub request: ArchiveRequest,
    pub digest: String,
    pub authority: WorkflowAuthority,
    pub control_epoch_admitted: Option<u64>,
    pub child_binding: Option<AgentBinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveRefusal {
    RequiredRetention,
    DeliverySuperseded,
    ObligationChanged,
    StaleTarget,
    HumanOwned,
    OwnershipUncertain,
    UnresolvedControl,
    AttributionChanged,
    Busy,
    NoOwnedSession,
    Capacity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveOutcome {
    Refused {
        reason: ArchiveRefusal,
    },
    /// The admitted owned stop may have had an effect. Never replay termination.
    StopUnknown,
    /// Certified stopped original run; final archive filing has not committed.
    StoppedArchivePending,
    Archived {
        previous_run: SessionRunId,
        archived_run: SessionRunId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveReceipt {
    pub intent: ArchiveIntent,
    pub outcome: ArchiveOutcome,
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn digest_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn text_valid(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.contains('\0')
}

impl DeliveryRef {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !identifier(&self.request) || !digest_valid(&self.digest) {
            return Err("invalid delivery reference");
        }
        Ok(())
    }
}
impl DeliveryDeclaration {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !identifier(&self.request)
            || !identifier(&self.workflow)
            || !text_valid(&self.scope, 1024)
            || !text_valid(&self.provenance, 1024)
            || self.resources.is_empty()
            || self.resources.len() > MAX_DELIVERY_RESOURCES
        {
            return Err("delivery declaration exceeds bounds");
        }
        let mut handles = std::collections::HashSet::new();
        for resource in &self.resources {
            if !identifier(&resource.handle)
                || !handles.insert(&resource.handle)
                || resource.identity.workflow != self.workflow
                || resource.identity.request != resource.handle
                || [
                    &resource.identity.instance,
                    &resource.identity.controller,
                    &resource.identity.task,
                ]
                .iter()
                .any(|value| !identifier(value))
                || resource.identity.controller_epoch == 0
                || resource.identity.attempt == 0
                || resource.identity.contract == 0
                || resource.identity.session.is_some() != resource.identity.run.is_some()
                || resource.identity.session.is_some_and(|id| id.0 == 0)
                || resource.identity.run.is_some_and(|run| run.0 == 0)
                || resource.result_version == 0
                || resource.obligation_revision == 0
                || resource.evidence.len() > 16
                || resource.completed_obligations.len() > 16
                || resource
                    .evidence
                    .iter()
                    .any(|e| !text_valid(&e.reference, 1024) || !digest_valid(&e.digest))
                || resource
                    .completed_obligations
                    .iter()
                    .any(|o| !text_valid(o, 128))
            {
                return Err("invalid delivery resource");
            }
            if resource.publication.as_ref().is_some_and(|publication| {
                publication.assignment != resource.identity
                    || publication.version != resource.result_version
                    || !digest_valid(&publication.digest)
            }) || resource.acceptance.as_ref().is_some_and(|acceptance| {
                acceptance.identity != resource.identity
                    || acceptance.result_version != resource.result_version
                    || !digest_valid(&acceptance.digest)
                    || !digest_valid(&acceptance.result_digest)
            }) {
                return Err("delivery artifact identity or digest changed");
            }
            if let RetentionChoice::Keep { reason } = &resource.retention
                && !text_valid(reason, 1024)
            {
                return Err("retention reason exceeds bounds");
            }
            if let Some(previous) = &resource.previous {
                previous.validate()?;
            }
        }
        Ok(())
    }
}
impl ArchiveRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        let identity = &self.expected_identity;
        if [
            &self.request,
            &self.handle,
            &identity.instance,
            &identity.workflow,
            &identity.controller,
            &identity.task,
            &identity.request,
        ]
        .iter()
        .any(|s| !identifier(s))
            || identity.request != self.handle
            || identity.controller_epoch == 0
            || identity.attempt == 0
            || identity.contract == 0
            || identity.session.is_none_or(|id| id.0 == 0)
            || identity.run.is_none_or(|run| run.0 == 0)
            || self.expected_result_version == 0
            || self.expected_obligation_revision == 0
            || self.expected_control_epoch == 0
            || self
                .continuation_of
                .as_ref()
                .is_some_and(|r| !identifier(r) || r == &self.request)
        {
            return Err("invalid archive target or request");
        }
        self.delivery.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_delivery_references_cannot_authorize_archive() {
        for (request, digest) in [
            ("", "a".repeat(64)),
            ("valid", "A".repeat(64)),
            ("../path", "a".repeat(64)),
        ] {
            assert!(
                DeliveryRef {
                    request: request.into(),
                    digest
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            DeliveryRef {
                request: "delivery-1".into(),
                digest: "a".repeat(64)
            }
            .validate()
            .is_ok()
        );
    }

    fn roundtrip<T>(value: T)
    where
        T: Serialize + for<'de> Deserialize<'de> + std::fmt::Debug + PartialEq,
    {
        let bytes = bincode::serde::encode_to_vec(&value, bincode::config::standard()).unwrap();
        let (decoded, used): (T, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(decoded, value);
        let json = serde_json::to_vec(&value).unwrap();
        assert_eq!(serde_json::from_slice::<T>(&json).unwrap(), value);
    }

    #[test]
    fn delivery_archive_requests_and_all_outcomes_roundtrip_through_public_wire_types() {
        let identity = AssignmentIdentity {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            controller_epoch: 1,
            task: "t".into(),
            attempt: 1,
            contract: 1,
            request: "assignment".into(),
            session: Some(crate::SessionId(2)),
            run: Some(SessionRunId(1)),
        };
        let authority = WorkflowAuthority {
            instance: "i".into(),
            workflow: "w".into(),
            controller: "c".into(),
            epoch: 1,
            parent: crate::SessionId(1),
            parent_run: SessionRunId(1),
            binding: AgentBinding {
                provider: crate::AgentProvider::Pi,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
            producer: "producer".into(),
            server_lifetime: "server".into(),
        };
        let auth = crate::DelegationAuth {
            session: crate::SessionId(1),
            capability: "test-only-capability".into(),
            source: None,
            controller: Some(authority.fence()),
        };
        let reference = DeliveryRef {
            request: "delivery".into(),
            digest: "a".repeat(64),
        };
        let declaration = DeliveryDeclaration {
            request: "delivery".into(),
            workflow: "w".into(),
            scope: "report delivered".into(),
            provenance: "explicit decision".into(),
            resources: vec![DeliveryResource {
                handle: "assignment".into(),
                identity: identity.clone(),
                result_version: 1,
                obligation_revision: 1,
                publication: None,
                acceptance: None,
                evidence: vec![],
                completed_obligations: vec![],
                retention: RetentionChoice::Keep {
                    reason: "requested review".into(),
                },
                previous: None,
            }],
        };
        roundtrip(crate::Request::Workflow(Box::new(
            crate::WorkflowRequest::DeclareDelivery {
                auth: auth.clone(),
                declaration: declaration.clone(),
            },
        )));
        roundtrip(RecordedDelivery {
            reference: reference.clone(),
            authority: authority.clone(),
            declaration,
        });
        let archive = ArchiveRequest {
            request: "archive".into(),
            handle: "assignment".into(),
            expected_identity: identity,
            expected_result_version: 1,
            expected_obligation_revision: 1,
            expected_control_epoch: 1,
            delivery: reference,
            continuation_of: None,
        };
        roundtrip(crate::Request::Delegation(Box::new(
            crate::DelegationRequest::Archive {
                auth,
                archive: archive.clone(),
            },
        )));
        roundtrip(crate::ControlBarrier::Archive {
            request: "archive".into(),
        });
        let mut outcomes = vec![
            ArchiveOutcome::StopUnknown,
            ArchiveOutcome::StoppedArchivePending,
            ArchiveOutcome::Archived {
                previous_run: SessionRunId(1),
                archived_run: SessionRunId(2),
            },
        ];
        for reason in [
            ArchiveRefusal::RequiredRetention,
            ArchiveRefusal::DeliverySuperseded,
            ArchiveRefusal::ObligationChanged,
            ArchiveRefusal::StaleTarget,
            ArchiveRefusal::HumanOwned,
            ArchiveRefusal::OwnershipUncertain,
            ArchiveRefusal::UnresolvedControl,
            ArchiveRefusal::AttributionChanged,
            ArchiveRefusal::Busy,
            ArchiveRefusal::NoOwnedSession,
            ArchiveRefusal::Capacity,
        ] {
            outcomes.push(ArchiveOutcome::Refused { reason });
        }
        for outcome in outcomes {
            roundtrip(ArchiveReceipt {
                intent: ArchiveIntent {
                    request: archive.clone(),
                    digest: "b".repeat(64),
                    authority: authority.clone(),
                    control_epoch_admitted: Some(2),
                    child_binding: Some(authority.binding.clone()),
                },
                outcome,
            });
        }
    }
}

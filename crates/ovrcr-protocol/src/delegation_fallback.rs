//! Active-screen evidence and parent recoveries, never child publications or acceptance.
use crate::{
    AgentBinding, AssignmentControl, AssignmentControlEvidence, AssignmentIdentity, TerminalSize,
    WorkflowAuthority,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackContext {
    pub identity: AssignmentIdentity,
    pub result_version: u32,
    pub contract_digest: String,
    pub brief_digest: String,
    /// Random value issued in the current result version's immutable brief.
    pub freshness_nonce: String,
    pub binding: AgentBinding,
    pub control: AssignmentControl,
}

/// A bounded active screen. It has no complete-response or scrollback claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackCapture {
    pub schema: u32,
    pub id: String,
    pub server_lifetime: String,
    /// Public capture provenance. Source credentials remain in-memory authentication.
    pub authority: WorkflowAuthority,
    pub context: FallbackContext,
    /// Server-produced reporting/publication revisions, never caller-supplied authority.
    pub evidence: AssignmentControlEvidence,
    pub captured_unix_ms: u64,
    pub size: TerminalSize,
    pub screen: String,
    pub truncated: bool,
    pub original: FallbackOriginalCondition,
    pub original_bytes: Option<Vec<u8>>,
}

/// Child-visible immutable correlation only. Attribution is owned by the trusted
/// Server capture and final current-source/control fences, never by child fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackArtifactContext {
    pub identity: AssignmentIdentity,
    pub result_version: u32,
    pub contract_digest: String,
    pub brief_digest: String,
    pub freshness_nonce: String,
}
impl From<&FallbackContext> for FallbackArtifactContext {
    fn from(context: &FallbackContext) -> Self {
        Self {
            identity: context.identity.clone(),
            result_version: context.result_version,
            contract_digest: context.contract_digest.clone(),
            brief_digest: context.brief_digest.clone(),
            freshness_nonce: context.freshness_nonce.clone(),
        }
    }
}

/// Optional child evidence when the normal result could not be published.
/// Its digest is printed as a complete `OVRCR_FALLBACK:<digest>` screen line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackArtifact {
    pub schema: u32,
    pub context: FallbackArtifactContext,
    pub deliverable: String,
    pub unresolved: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackFileReference {
    /// One file in the assigned private directory; paths are not accepted.
    pub name: String,
    pub digest: String,
}

/// Parent judgment must be supported by independently inspected bytes.
/// An excerpt proves a reference's integrity, not its semantic sufficiency.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackEvidence {
    pub requirement: String,
    pub file: FallbackFileReference,
    pub excerpt: String,
    pub assessment: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FallbackOriginalCondition {
    Missing,
    Invalid { digest: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackRecoveryDraft {
    pub capture_id: String,
    pub version: u32,
    pub artifact: FallbackFileReference,
    pub original: FallbackOriginalCondition,
    pub evidence: Vec<FallbackEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenFallbackFile {
    pub reference: FallbackFileReference,
    pub bytes: Vec<u8>,
}

/// Parent-owned immutable evidence. This type deliberately has no acceptance field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackRecovery {
    pub schema: u32,
    pub version: u32,
    /// Immutable public provenance, never permission for a later effect.
    pub author: WorkflowAuthority,
    pub capture: FallbackCapture,
    pub artifact: FrozenFallbackFile,
    pub contract: FrozenFallbackFile,
    pub brief: FrozenFallbackFile,
    pub evidence: Vec<FallbackEvidence>,
    pub evidence_files: Vec<FrozenFallbackFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureReference {
    pub id: String,
    pub digest: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryReference {
    pub version: u32,
    pub digest: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPublication {
    pub path: std::path::PathBuf,
    pub digest: String,
    pub recovery: FallbackRecovery,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AcceptanceReference, AcceptanceTarget, SessionId, SessionRunId};

    fn identity() -> AssignmentIdentity {
        AssignmentIdentity {
            instance: "instance".into(),
            workflow: "workflow".into(),
            controller: "parent".into(),
            controller_epoch: 1,
            task: "task".into(),
            attempt: 1,
            contract: 1,
            request: "request".into(),
            session: Some(SessionId(2)),
            run: Some(SessionRunId(3)),
        }
    }

    #[test]
    fn legacy_child_acceptance_json_preserves_exact_field_order_and_nested_bytes() {
        let legacy = format!(
            "{{\"identity\":{},\"result_version\":1,\"result_digest\":\"child-digest\",\"path\":\"/private/acceptance-v1.json\",\"digest\":\"decision-digest\"}}",
            serde_json::to_string(&identity()).unwrap()
        );
        let reference: AcceptanceReference = serde_json::from_str(&legacy).unwrap();
        assert_eq!(
            reference.target,
            AcceptanceTarget::ChildSubmission {
                digest: "child-digest".into()
            }
        );
        assert_eq!(serde_json::to_string(&reference).unwrap(), legacy);
        let nested = format!("{{\"acceptance\":{legacy}}}");
        #[derive(Serialize, Deserialize)]
        struct Nested {
            acceptance: AcceptanceReference,
        }
        let decoded: Nested = serde_json::from_str(&nested).unwrap();
        assert_eq!(
            serde_json::to_string(&decoded).unwrap(),
            nested,
            "old declaration digest input remains exact"
        );
    }

    #[test]
    fn legacy_complete_delivery_declaration_preserves_authenticated_bytes() {
        // Legacy declaration golden, including the nested flat child reference.
        let original = r#"{"request":"legacy-delivery","workflow":"workflow","scope":"Deliver the original reviewed result","provenance":"Original parent decision","resources":[{"handle":"request","identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":3},"result_version":1,"obligation_revision":1,"publication":{"assignment":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":3},"kind":"Submission","version":1,"digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"acceptance":{"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":3},"result_version":1,"result_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","path":"/private/acceptance-v1.json","digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},"evidence":[{"kind":"Review","reference":"original-review","digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}],"completed_obligations":["Original review complete"],"retention":{"Keep":{"reason":"Explicitly retained for review"}},"previous":null}]}"#;
        let declaration: crate::DeliveryDeclaration = serde_json::from_str(original).unwrap();
        declaration.validate().unwrap();
        assert_eq!(declaration.resources.len(), 1);
        assert_eq!(
            declaration.resources[0].acceptance.as_ref().unwrap().target,
            AcceptanceTarget::ChildSubmission {
                digest: "a".repeat(64)
            }
        );
        assert_eq!(
            serde_json::to_vec(&declaration).unwrap(),
            original.as_bytes(),
            "the complete authenticated declaration must retain its original digest input"
        );
        let encoded =
            bincode::serde::encode_to_vec(&declaration, bincode::config::standard()).unwrap();
        let (decoded, used): (crate::DeliveryDeclaration, usize) =
            bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(decoded, declaration);
        assert_eq!(used, encoded.len());
        assert_eq!(serde_json::to_string(&decoded).unwrap(), original);
    }

    #[test]
    fn recovery_reference_rejects_ambiguous_unknown_and_mixed_json_targets() {
        let reference = AcceptanceReference {
            identity: identity(),
            result_version: 1,
            target: AcceptanceTarget::ParentRecovery {
                recovery_version: 2,
                digest: "recovery-digest".into(),
            },
            path: "/private/acceptance-v1.json".into(),
            digest: "decision-digest".into(),
        };
        let value = serde_json::to_value(&reference).unwrap();
        assert_eq!(
            serde_json::from_value::<AcceptanceReference>(value.clone()).unwrap(),
            reference
        );
        for (field, bad) in [
            ("result_digest", serde_json::json!("mixed")),
            ("target", serde_json::json!("Unknown")),
            ("recovery_version", serde_json::json!(0)),
        ] {
            let mut mixed = value.clone();
            mixed[field] = bad;
            assert!(serde_json::from_value::<AcceptanceReference>(mixed).is_err());
        }
        let bytes = bincode::serde::encode_to_vec(&reference, bincode::config::standard()).unwrap();
        let (decoded, used): (AcceptanceReference, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(decoded, reference);
        assert_eq!(used, bytes.len());
    }
    #[test]
    fn delivery_validates_the_exact_recovery_target_without_a_child_publication() {
        let mut declaration = crate::DeliveryDeclaration {
            request: "delivery".into(),
            workflow: "workflow".into(),
            scope: "Deliver the independently verified README".into(),
            provenance: "Explicit parent decision".into(),
            resources: vec![crate::DeliveryResource {
                handle: "request".into(),
                identity: identity(),
                result_version: 1,
                obligation_revision: 3,
                publication: None,
                acceptance: Some(AcceptanceReference {
                    identity: identity(),
                    result_version: 1,
                    target: AcceptanceTarget::ParentRecovery {
                        recovery_version: 1,
                        digest: "a".repeat(64),
                    },
                    path: "/private/acceptance-v1.json".into(),
                    digest: "b".repeat(64),
                }),
                evidence: vec![],
                completed_obligations: vec!["Independent review".into()],
                retention: crate::RetentionChoice::ArchiveEligible,
                previous: None,
            }],
        };
        declaration.validate().unwrap();
        let bytes = serde_json::to_vec(&declaration).unwrap();
        let decoded: crate::DeliveryDeclaration = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, declaration);
        assert!(decoded.resources[0].publication.is_none());
        for (version, digest) in [
            (0, "a".repeat(64)),
            (65, "a".repeat(64)),
            (1, "partial".into()),
        ] {
            declaration.resources[0].acceptance.as_mut().unwrap().target =
                AcceptanceTarget::ParentRecovery {
                    recovery_version: version,
                    digest,
                };
            assert!(
                declaration.validate().is_err(),
                "invalid recovery target must not become delivery evidence"
            );
        }
    }

    #[test]
    fn fallback_requests_round_trip_through_production_framing() {
        let auth = crate::DelegationAuth {
            session: SessionId(1),
            capability: "fixture".into(),
            controller: None,
            source: None,
        };
        for operation in [
            crate::DelegationRequest::CaptureFallback {
                auth: auth.clone(),
                handle: "request".into(),
                expected_identity: identity(),
                expected_result_version: 1,
                expected_control_epoch: 1,
            },
            crate::DelegationRequest::RecoverFallback {
                auth,
                handle: "request".into(),
                draft: Box::new(FallbackRecoveryDraft {
                    capture_id: "capture-id".into(),
                    version: 1,
                    artifact: FallbackFileReference {
                        name: "artifact.json".into(),
                        digest: "a".repeat(64),
                    },
                    original: FallbackOriginalCondition::Missing,
                    evidence: vec![],
                }),
            },
        ] {
            let request = crate::Request::Delegation(Box::new(operation));
            let mut frame = vec![];
            crate::write_frame(&mut frame, &request).unwrap();
            let decoded: crate::Request = crate::read_frame(&mut frame.as_slice()).unwrap();
            assert_eq!(decoded, request);
        }
    }
}

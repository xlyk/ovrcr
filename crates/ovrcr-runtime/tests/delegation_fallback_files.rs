//! Supplemental file-only tests. No Server, PTY, provider or transport is exercised.
pub use ovrcr_protocol::*;
pub use ovrcr_runtime::agent_runner;
#[allow(dead_code)] // Unchanged shared Directory also has non-fallback callers.
#[path = "../src/server/delegation_files.rs"]
mod delegation_files;
#[path = "../src/server/delegation_fallback.rs"]
mod fallback;
use delegation_files::Directory;
use fallback::{FallbackFiles, digest, original_condition, prepare_recovery, publish_recovery};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

struct Fixture {
    root: tempfile::TempDir,
    directory: Directory,
    context: FallbackContext,
    author: WorkflowAuthority,
    evidence: AssignmentControlEvidence,
    capture: FallbackCapture,
    artifact: FallbackArtifact,
    draft: FallbackRecoveryDraft,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let directory = Directory::open(&root.path().canonicalize().unwrap(), false).unwrap();
        let contract = ResearchContract {
            schema: 1,
            workflow: "wf".into(),
            controller: "parent".into(),
            task: "readme".into(),
            request: "initial".into(),
            project: "project".into(),
            workspace: "workspace".into(),
            scope: "Explain the README setup steps".into(),
            permitted_actions: vec!["read".into()],
            provider: "pi".into(),
            model_provider: "fixture".into(),
            model: "fixture".into(),
            acceptance_evidence: vec![
                "Cite setup instructions".into(),
                "Verify command spelling".into(),
            ],
            mandatory_capabilities: vec!["read".into()],
            limits: DelegationLimits::default(),
            previous_request: None,
            coding: None,
        };
        // Deliberate whitespace proves exact original bytes, not reserialization.
        let contract_bytes = serde_json::to_vec_pretty(&contract).unwrap();
        directory
            .publish("contract-v1.json", &contract_bytes)
            .unwrap();
        let identity = AssignmentIdentity {
            instance: "instance".into(),
            workflow: "wf".into(),
            controller: "parent".into(),
            controller_epoch: 1,
            task: "readme".into(),
            attempt: 1,
            contract: 1,
            request: "initial".into(),
            session: Some(SessionId(7)),
            run: Some(SessionRunId(8)),
        };
        let nonce = "a".repeat(64);
        let brief_bytes = serde_json::to_vec(&serde_json::json!({
            "schema": 1, "identity": identity, "result_version": 1,
            "fallback_nonce": nonce,
            "instructions": "Leave complete fallback evidence if result publication fails"
        }))
        .unwrap();
        directory.publish("brief-v1.json", &brief_bytes).unwrap();
        let context = FallbackContext {
            identity,
            result_version: 1,
            contract_digest: digest(&contract_bytes),
            brief_digest: digest(&brief_bytes),
            freshness_nonce: nonce,
            binding: AgentBinding {
                provider: AgentProvider::Pi,
                invocation: "child-invocation".into(),
                conversation: "child-conversation".into(),
                generation: 1,
            },
            control: AssignmentControl::default(),
        };
        let author = WorkflowAuthority {
            instance: context.identity.instance.clone(),
            workflow: context.identity.workflow.clone(),
            controller: context.identity.controller.clone(),
            epoch: 1,
            parent: SessionId(1),
            parent_run: SessionRunId(2),
            producer: "parent-source-1".into(),
            server_lifetime: "server-1".into(),
            binding: AgentBinding {
                provider: AgentProvider::Pi,
                invocation: "current-parent-invocation".into(),
                conversation: "current-parent-conversation".into(),
                generation: 5,
            },
        };
        let evidence = AssignmentControlEvidence {
            server_lifetime: author.server_lifetime.clone(),
            identity: context.identity.clone(),
            binding: context.binding.clone(),
            result_version: context.result_version,
            activity_revision: 1,
            input_revision: 0,
            publication: None,
            // Canonical control_evidence preserves Ok(None) as no issue.
            publication_issue: None,
            human_input_revision: context.control.human_input_revision,
        };
        let source = b"README setup: run cargo build. Then run ovrcr.\n";
        directory
            .publish("independently-read-source.txt", source)
            .unwrap();
        let check = b"Inspected README against CLI help: cargo build; ovrcr.\n";
        directory.publish("independent-check.txt", check).unwrap();
        let artifact = FallbackArtifact {
            schema: 1, context: (&context).into(),
            deliverable: "Build with cargo build, then start with ovrcr. These are the complete README setup steps.".into(),
            unresolved: vec![],
        };
        let bytes = serde_json::to_vec(&artifact).unwrap();
        directory.publish("fallback-evidence.json", &bytes).unwrap();
        let capture = FallbackCapture {
            schema: 1,
            id: "capture-1".into(),
            server_lifetime: "server-1".into(),
            authority: author.clone(),
            context: context.clone(),
            evidence: evidence.clone(),
            captured_unix_ms: 100,
            size: TerminalSize {
                rows: 24,
                cols: 100,
            },
            screen: format!(
                "Report write failed; complete evidence saved.\nOVRCR_FALLBACK:{}",
                digest(&bytes)
            ),
            truncated: false,
            original: FallbackOriginalCondition::Missing,
            original_bytes: None,
        };
        let draft = FallbackRecoveryDraft {
            capture_id: capture.id.clone(),
            version: 1,
            artifact: FallbackFileReference {
                name: "fallback-evidence.json".into(),
                digest: digest(&bytes),
            },
            original: FallbackOriginalCondition::Missing,
            evidence: vec![
                FallbackEvidence {
                    requirement: contract.acceptance_evidence[0].clone(),
                    file: FallbackFileReference {
                        name: "independently-read-source.txt".into(),
                        digest: digest(source),
                    },
                    excerpt: "README setup: run cargo build. Then run ovrcr.".into(),
                    assessment: "The complete setup section names both required commands.".into(),
                },
                FallbackEvidence {
                    requirement: contract.acceptance_evidence[1].clone(),
                    file: FallbackFileReference {
                        name: "independent-check.txt".into(),
                        digest: digest(check),
                    },
                    excerpt: "cargo build; ovrcr".into(),
                    assessment:
                        "Compared both command spellings with the independently inspected help."
                            .into(),
                },
            ],
        };
        Self {
            root,
            directory,
            context,
            author,
            evidence,
            capture,
            artifact,
            draft,
        }
    }

    fn files(&self) -> FallbackFiles<'_> {
        FallbackFiles {
            directory: &self.directory,
            contract_name: "contract-v1.json",
            brief_name: "brief-v1.json",
            original_name: "result-v1.json",
        }
    }

    fn prepare(&self) -> anyhow::Result<FallbackRecovery> {
        prepare_recovery(
            &self.files(),
            &self.capture,
            &self.context,
            &self.author,
            &self.evidence,
            &self.draft,
        )
    }

    fn publish(&self, recovery: &FallbackRecovery) -> anyhow::Result<String> {
        publish_recovery(
            &self.files(),
            &self.context,
            &self.author,
            &self.evidence,
            recovery,
        )
    }

    fn rewrite_artifact(&mut self) {
        let bytes = serde_json::to_vec(&self.artifact).unwrap();
        self.directory
            .replace("fallback-evidence.json", &bytes)
            .unwrap();
        self.draft.artifact.digest = digest(&bytes);
        self.capture.screen = format!("OVRCR_FALLBACK:{}", digest(&bytes));
    }

    fn current_result(&mut self, version: u32) {
        self.context.result_version = version;
        self.context.freshness_nonce = digest(format!("issued-for-result-{version}").as_bytes());
        let brief = serde_json::to_vec(&serde_json::json!({
            "schema": 1, "identity": self.context.identity, "result_version": version,
            "fallback_nonce": self.context.freshness_nonce,
        }))
        .unwrap();
        self.directory.replace("brief-v1.json", &brief).unwrap();
        self.context.brief_digest = digest(&brief);
        self.capture.context = self.context.clone();
        self.evidence.result_version = version;
        self.capture.evidence = self.evidence.clone();
        self.artifact.context = (&self.context).into();
        self.rewrite_artifact();
    }

    fn freeze_original(&mut self, bytes: &[u8]) {
        self.directory.publish("result-v1.json", bytes).unwrap();
        let (condition, bytes) =
            original_condition(&self.directory, "result-v1.json", &self.context).unwrap();
        // An uncommitted file yields canonical Unattributed independently of
        // whether the separately preserved original bytes are structurally invalid.
        self.evidence.publication_issue = Some(ControlPublicationIssue::Unattributed);
        self.capture.evidence = self.evidence.clone();
        self.capture.original = condition.clone();
        self.capture.original_bytes = bytes;
        self.draft.original = condition;
    }
}

#[test]
fn missing_result_recovery_freezes_deliverable_and_all_evidence_without_forging_result() {
    let f = Fixture::new();
    let recovery = f.prepare().unwrap();
    let name = f.publish(&recovery).unwrap();
    assert_eq!(
        f.publish(&recovery).unwrap(),
        name,
        "identical publication retry"
    );
    let saved = f.directory.read(&name).unwrap();
    let saved: FallbackRecovery = serde_json::from_slice(&saved).unwrap();
    assert_eq!(saved, recovery);
    assert_eq!(
        saved.author, f.author,
        "current parent provenance is preserved"
    );
    assert_eq!(
        saved.artifact.bytes,
        f.directory.read("fallback-evidence.json").unwrap()
    );
    assert_eq!(saved.evidence_files.len(), 2);
    assert_eq!(saved.capture.evidence, f.evidence);
    assert!(!f.root.path().join("result-v1.json").exists());
    let json = serde_json::to_value(saved).unwrap();
    for forbidden in ["acceptance", "receipt", "dispatch", "submission"] {
        assert!(
            json.get(forbidden).is_none(),
            "recovery cannot invent {forbidden}"
        );
    }
}

#[test]
fn invalid_original_bytes_and_evidence_survive_later_source_changes() {
    let mut f = Fixture::new();
    let invalid = b"{broken report\0\xff";
    f.freeze_original(invalid);
    let recovery = f.prepare().unwrap();
    let name = f.publish(&recovery).unwrap();
    f.directory
        .replace("result-v1.json", b"later repair")
        .unwrap();
    f.directory
        .replace("independently-read-source.txt", b"later source")
        .unwrap();
    let saved: FallbackRecovery =
        serde_json::from_slice(&f.directory.read(&name).unwrap()).unwrap();
    assert_eq!(
        saved.capture.original_bytes.as_deref(),
        Some(invalid.as_slice())
    );
    assert_eq!(
        saved.capture.original,
        FallbackOriginalCondition::Invalid {
            digest: digest(invalid)
        }
    );
    assert_eq!(
        saved.evidence_files[0].bytes,
        recovery.evidence_files[0].bytes
    );
    assert_eq!(f.directory.read("result-v1.json").unwrap(), b"later repair");
}

#[test]
fn plausible_success_lines_wrapped_or_truncated_markers_are_insufficient() {
    let f = Fixture::new();
    for screen in [
        "Success; tests passed".into(),
        format!("OVRCR_FALLBACK:{}", &f.draft.artifact.digest[..63]),
        format!(
            "OVRCR_FALLBACK:{}\n{}",
            &f.draft.artifact.digest[..32],
            &f.draft.artifact.digest[32..]
        ),
        format!("echo OVRCR_FALLBACK:{}", f.draft.artifact.digest),
    ] {
        let mut capture = f.capture.clone();
        capture.screen = screen;
        assert!(
            prepare_recovery(
                &f.files(),
                &capture,
                &f.context,
                &f.author,
                &f.evidence,
                &f.draft
            )
            .is_err()
        );
    }
    let mut capture = f.capture.clone();
    capture.truncated = true;
    assert!(
        prepare_recovery(
            &f.files(),
            &capture,
            &f.context,
            &f.author,
            &f.evidence,
            &f.draft
        )
        .is_err()
    );
}

#[test]
fn same_run_old_result_and_prepared_future_artifacts_cannot_satisfy_current_result() {
    for version in [1, 3] {
        let mut f = Fixture::new();
        f.current_result(2);
        // Capture is current; its visible marker references older/prepared work in the same PTY.
        f.artifact.context.result_version = version;
        f.artifact.context.freshness_nonce = "b".repeat(64);
        f.rewrite_artifact();
        assert_eq!(
            f.artifact.context.identity.session,
            f.context.identity.session
        );
        assert_eq!(f.artifact.context.identity.run, f.context.identity.run);
        assert!(f.prepare().unwrap_err().to_string().contains("old work"));
    }
}

#[test]
fn changed_current_artifact_cannot_reuse_an_old_same_run_screen_marker() {
    let mut f = Fixture::new();
    let old_screen = f.capture.screen.clone();
    f.artifact.deliverable.push_str(" Updated finding.");
    f.rewrite_artifact();
    f.capture.screen = old_screen;
    assert!(f.prepare().unwrap_err().to_string().contains("marker"));
}

#[test]
fn authoritative_sparse_result_numbers_are_supported_without_reusing_reserved_versions() {
    let mut f = Fixture::new();
    // The Server validates reservation history. File helpers use its current version,
    // not a history length or a dense sequence invented from filenames.
    f.current_result(7);
    let recovery = f.prepare().unwrap();
    assert_eq!(f.publish(&recovery).unwrap(), "parent-fallback-r7-v1.json");
    for stale_or_prepared in [2, 8] {
        f.artifact.context.result_version = stale_or_prepared;
        f.artifact.context.freshness_nonce = digest(b"not-current");
        f.rewrite_artifact();
        assert!(f.prepare().unwrap_err().to_string().contains("old work"));
    }
}

#[test]
fn nondefault_limits_and_retry_provenance_remain_part_of_exact_contract_bytes() {
    let mut f = Fixture::new();
    let bytes = f.directory.read("contract-v1.json").unwrap();
    let mut contract: ResearchContract = serde_json::from_slice(&bytes).unwrap();
    contract.limits.max_execution_attempts = Some(1);
    contract.limits.max_live_children = Some(4);
    contract.limits.min_free_slots_after_launch = 2;
    contract.previous_request = Some("prior-zero-effect-request".into());
    let bytes = serde_json::to_vec_pretty(&contract).unwrap();
    f.directory.replace("contract-v1.json", &bytes).unwrap();
    f.context.contract_digest = digest(&bytes);
    f.current_result(1);
    let recovery = f.prepare().unwrap();
    f.publish(&recovery).unwrap();
    contract.limits = DelegationLimits::default();
    contract.previous_request = None;
    f.directory
        .replace(
            "contract-v1.json",
            &serde_json::to_vec_pretty(&contract).unwrap(),
        )
        .unwrap();
    assert!(f.prepare().unwrap_err().to_string().contains("changed"));
    assert!(f.publish(&recovery).is_err());
}

#[test]
fn legacy_brief_without_nonce_and_substituted_exact_bytes_fail_closed() {
    for bytes in [b"{}".to_vec(), b"{\"fallback_nonce\":\"old\"}".to_vec()] {
        let mut f = Fixture::new();
        f.directory.replace("brief-v1.json", &bytes).unwrap();
        f.context.brief_digest = digest(&bytes);
        f.capture.context = f.context.clone();
        assert!(
            f.prepare()
                .unwrap_err()
                .to_string()
                .contains("freshness anchor")
        );
    }
    let f = Fixture::new();
    let bytes = f.directory.read("contract-v1.json").unwrap();
    let contract: ResearchContract = serde_json::from_slice(&bytes).unwrap();
    f.directory
        .replace("contract-v1.json", &serde_json::to_vec(&contract).unwrap())
        .unwrap();
    assert!(
        f.prepare().unwrap_err().to_string().contains("changed"),
        "semantically equivalent contract has different immutable bytes"
    );
}

#[test]
fn any_assignment_reporting_or_control_change_invalidates_captured_attribution() {
    let f = Fixture::new();
    let mut changed = Vec::new();
    macro_rules! context {
        ($field:ident.$sub:ident = $value:expr) => {{
            let mut c = f.context.clone();
            c.$field.$sub = $value;
            changed.push(c);
        }};
        ($field:ident = $value:expr) => {{
            let mut c = f.context.clone();
            c.$field = $value;
            changed.push(c);
        }};
    }
    context!(identity.instance = "replacement".into());
    context!(identity.attempt = 2);
    context!(identity.contract = 2);
    context!(identity.run = Some(SessionRunId(99)));
    context!(identity.session = Some(SessionId(99)));
    context!(identity.controller_epoch = 2);
    context!(result_version = 2);
    context!(binding.provider = AgentProvider::Codex);
    context!(binding.invocation = "replacement".into());
    context!(binding.conversation = "replacement".into());
    context!(binding.generation = 2);
    context!(control.epoch = 2);
    context!(control.human_input_revision = 1);
    context!(control.owner = ControlOwner::Human);
    context!(control.owner = ControlOwner::Uncertain);
    context!(control.attribution_invalidated = true);
    context!(control.barrier = Some(ControlBarrier::AttributionChanged));
    context!(
        control.barrier = Some(ControlBarrier::Interrupt {
            request: "interrupt".into()
        })
    );
    for current in changed {
        assert!(
            fallback::validate_capture(&f.capture, &current, &f.author, &f.evidence).is_err(),
            "accepted changed context: {current:?}"
        );
    }
    let mut replacement = f.author.clone();
    replacement.server_lifetime = "new-server".into();
    assert!(fallback::validate_capture(&f.capture, &f.context, &replacement, &f.evidence).is_err());
}

#[test]
fn captured_workflow_provenance_cannot_authorize_a_replacement_parent_or_source() {
    let f = Fixture::new();
    for changed in 0..4 {
        let mut authority = f.author.clone();
        match changed {
            0 => authority.epoch += 1,
            1 => authority.producer = "replacement-source".into(),
            2 => authority.parent = SessionId(99),
            3 => authority.parent_run = SessionRunId(99),
            _ => unreachable!(),
        }
        assert!(
            fallback::validate_capture(&f.capture, &f.context, &authority, &f.evidence).is_err()
        );
    }
}

#[test]
fn canonical_reporting_and_publication_revisions_must_still_match_the_capture() {
    let f = Fixture::new();
    for change in 0..6 {
        let mut current = f.evidence.clone();
        match change {
            0 => current.activity_revision += 1,
            1 => current.input_revision += 1,
            2 => current.publication_issue = Some(ControlPublicationIssue::Missing),
            3 => current.publication_issue = Some(ControlPublicationIssue::Invalid),
            4 => current.publication_issue = Some(ControlPublicationIssue::Unattributed),
            5 => {
                current.publication = Some(PublicationIdentity {
                    assignment: f.context.identity.clone(),
                    kind: PublicationKind::Submission,
                    version: f.context.result_version,
                    digest: digest(b"new child publication"),
                })
            }
            _ => unreachable!(),
        }
        assert!(fallback::validate_capture(&f.capture, &f.context, &f.author, &current).is_err());
        let recovery = f.prepare().unwrap();
        assert!(publish_recovery(&f.files(), &f.context, &f.author, &current, &recovery).is_err());
        assert!(!f.root.path().join("parent-fallback-r1-v1.json").exists());
    }
}

#[test]
fn matching_canonical_evidence_cannot_describe_a_different_context_or_valid_publication() {
    let f = Fixture::new();
    for change in 0..6 {
        let mut current = f.evidence.clone();
        match change {
            0 => current.identity.run = Some(SessionRunId(99)),
            1 => current.binding.generation += 1,
            2 => current.result_version += 1,
            3 => current.server_lifetime = "different-server".into(),
            4 => current.human_input_revision += 1,
            5 => {
                current.publication = Some(PublicationIdentity {
                    assignment: f.context.identity.clone(),
                    kind: PublicationKind::Submission,
                    version: f.context.result_version,
                    digest: digest(b"valid child publication"),
                })
            }
            _ => unreachable!(),
        }
        let mut capture = f.capture.clone();
        capture.evidence = current.clone();
        assert!(fallback::validate_capture(&capture, &f.context, &f.author, &current).is_err());
    }
}

#[test]
fn artifact_from_unrelated_assignment_or_unresolved_deliverable_is_rejected() {
    let mut f = Fixture::new();
    f.artifact.context.identity.task = "unrelated".into();
    f.rewrite_artifact();
    assert!(f.prepare().is_err());
    f.artifact.context = (&f.context).into();
    f.artifact.unresolved.push("missing setup steps".into());
    f.rewrite_artifact();
    assert!(f.prepare().is_err());
    f.artifact.unresolved.clear();
    f.artifact.deliverable = "  ".into();
    f.rewrite_artifact();
    assert!(f.prepare().is_err());
}

#[test]
fn success_assertions_cannot_replace_complete_independently_read_evidence() {
    let mut f = Fixture::new();
    f.draft.evidence.clear();
    assert!(f.prepare().is_err());
    let mut f = Fixture::new();
    f.draft.evidence[0].excerpt = "Tests passed".into();
    assert!(f.prepare().unwrap_err().to_string().contains("excerpt"));
    f.draft.evidence[0] = f.draft.evidence[1].clone();
    assert!(
        f.prepare().is_err(),
        "duplicate coverage cannot stand in for a missing requirement"
    );
    let mut f = Fixture::new();
    f.draft.evidence[0].assessment.clear();
    assert!(f.prepare().is_err());
    f.draft.evidence[0].assessment = "Done".into();
    f.draft.evidence[0].file = f.draft.artifact.clone();
    assert!(
        f.prepare().is_err(),
        "child artifact cannot serve as independent evidence for itself"
    );
}

#[test]
fn changed_missing_or_invalid_original_is_rejected_at_capture_and_publication() {
    let f = Fixture::new();
    let recovery = f.prepare().unwrap();
    f.directory
        .publish("result-v1.json", b"new invalid result")
        .unwrap();
    assert!(
        fallback::validate_capture_files(
            &f.files(),
            &f.capture,
            &f.context,
            &f.author,
            &f.evidence
        )
        .is_err()
    );
    assert!(f.publish(&recovery).is_err());
    let mut f = Fixture::new();
    f.freeze_original(b"first invalid");
    let recovery = f.prepare().unwrap();
    f.directory
        .replace("result-v1.json", b"changed invalid")
        .unwrap();
    assert!(f.publish(&recovery).is_err());
    fs::remove_file(f.root.path().join("result-v1.json")).unwrap();
    assert!(f.publish(&recovery).is_err());
}

#[test]
fn valid_child_publication_is_collected_normally_and_never_replaced_by_fallback() {
    let f = Fixture::new();
    let submission = ResearchSubmission {
        schema: 1,
        identity: f.context.identity.clone(),
        version: 1,
        outcome: "success".into(),
        coding: None,
        summary: "The original valid result".into(),
        evidence: vec!["README".into()],
        unresolved: vec![],
    };
    let bytes = serde_json::to_vec(&submission).unwrap();
    f.directory.publish("result-v1.json", &bytes).unwrap();
    assert!(
        f.prepare()
            .unwrap_err()
            .to_string()
            .contains("ordinary collection")
    );
    assert_eq!(f.directory.read("result-v1.json").unwrap(), bytes);
}

#[test]
fn final_publication_rechecks_author_binding_control_artifact_and_independent_files() {
    for change in 0..5 {
        let mut f = Fixture::new();
        let recovery = f.prepare().unwrap();
        match change {
            0 => f.author.binding.generation += 1,
            1 => f.context.control.epoch += 1,
            2 => f.context.binding.generation += 1,
            3 => f
                .directory
                .replace("fallback-evidence.json", b"substituted")
                .unwrap(),
            4 => f
                .directory
                .replace("independent-check.txt", b"substituted")
                .unwrap(),
            _ => unreachable!(),
        }
        assert!(f.publish(&recovery).is_err());
        assert!(!f.root.path().join("parent-fallback-r1-v1.json").exists());
    }
}

#[test]
fn recovery_versions_are_immutable_and_new_versions_preserve_prior_evidence() {
    let mut f = Fixture::new();
    let first = f.prepare().unwrap();
    let first_name = f.publish(&first).unwrap();
    let first_bytes = f.directory.read(&first_name).unwrap();
    f.draft.evidence[0]
        .assessment
        .push_str(" Parent completed an additional inspection.");
    let changed = f.prepare().unwrap();
    assert!(f.publish(&changed).is_err());
    f.draft.version = 2;
    let second = f.prepare().unwrap();
    assert_ne!(f.publish(&second).unwrap(), first_name);
    assert_eq!(f.directory.read(&first_name).unwrap(), first_bytes);
}

#[test]
fn path_escape_symlink_and_held_directory_replacement_cannot_substitute_evidence() {
    let mut f = Fixture::new();
    f.draft.evidence[0].file.name = "../outside.txt".into();
    assert!(f.prepare().is_err());
    let mut f = Fixture::new();
    symlink("independent-check.txt", f.root.path().join("linked.txt")).unwrap();
    f.draft.evidence[0].file.name = "linked.txt".into();
    assert!(f.prepare().is_err());
    let parent = tempfile::tempdir().unwrap();
    let original = parent.path().canonicalize().unwrap().join("held");
    fs::create_dir(&original).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&original, fs::Permissions::from_mode(0o700)).unwrap();
    let held = Directory::open(&original, false).unwrap();
    held.publish("evidence.txt", b"original bytes").unwrap();
    let moved = parent.path().join("moved");
    fs::rename(&original, &moved).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("evidence.txt"), b"substituted bytes").unwrap();
    assert_eq!(held.read("evidence.txt").unwrap(), b"original bytes");
    held.publish("parent-recovery.json", b"immutable bytes")
        .unwrap();
    assert!(moved.join("parent-recovery.json").exists());
    assert!(!original.join("parent-recovery.json").exists());
}

#[test]
fn oversized_screens_deliverables_originals_and_aggregate_evidence_fail_explicitly() {
    let mut f = Fixture::new();
    f.capture.screen = "x".repeat(64 * 1024 + 1);
    assert!(f.prepare().is_err());
    let mut f = Fixture::new();
    f.artifact.deliverable = "x".repeat(32 * 1024 + 1);
    f.rewrite_artifact();
    assert!(f.prepare().is_err());
    let f = Fixture::new();
    f.directory
        .publish("result-v1.json", &vec![0; 64 * 1024 + 1])
        .unwrap();
    assert!(
        f.prepare()
            .unwrap_err()
            .to_string()
            .contains("preservation bound")
    );
    let mut f = Fixture::new();
    for i in 0..2 {
        let mut bytes = f.draft.evidence[i].excerpt.as_bytes().to_vec();
        bytes.resize(70 * 1024, b' ');
        f.directory
            .replace(&f.draft.evidence[i].file.name, &bytes)
            .unwrap();
        f.draft.evidence[i].file.digest = digest(&bytes);
    }
    assert!(f.prepare().unwrap_err().to_string().contains("aggregate"));
}

#[test]
fn frozen_recovery_remains_verifiable_after_mutable_sources_change() {
    let f = Fixture::new();
    let recovery = f.prepare().unwrap();
    fallback::verify_frozen(&recovery).unwrap();
    fs::write(
        f.root.path().join("independently-read-source.txt"),
        b"changed later",
    )
    .unwrap();
    fs::write(f.root.path().join("contract-v1.json"), b"changed later").unwrap();
    fallback::verify_frozen(&recovery).unwrap();
    assert!(
        f.prepare().is_err(),
        "a new effect still requires current unchanged sources"
    );
    let mut substituted = recovery.clone();
    substituted.evidence_files[0].bytes = b"substituted".to_vec();
    assert!(fallback::verify_frozen(&substituted).is_err());
    let mut substituted = recovery;
    substituted.capture.original = FallbackOriginalCondition::Invalid {
        digest: digest(b"absent bytes"),
    };
    assert!(fallback::verify_frozen(&substituted).is_err());
}

#[test]
fn child_artifact_correlation_does_not_claim_current_binding_or_control() {
    let f = Fixture::new();
    let value = serde_json::to_value(&f.artifact.context).unwrap();
    assert!(value.get("binding").is_none());
    assert!(value.get("control").is_none());
    let mut stale = f.capture.clone();
    stale.context.control.epoch += 1;
    assert!(fallback::validate_capture(&stale, &f.context, &f.author, &f.evidence).is_err());
}

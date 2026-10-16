//! Fallback effects share the existing Server, retained registry and Session fences.
use super::super::delegation_fallback as files;
use super::*;
use files::{FallbackFiles, validate_capture_files, verify_frozen};

const MAX_FALLBACKS: usize = 64;
pub(super) const FALLBACK_INSTRUCTIONS: &str = "If normal ovrcr_publish fails, preserve its original bytes or absence. Read the exact current brief and contract bytes and form fallback-evidence.json (schema1) with context {identity, result_version, contract_digest: SHA256 exact contract bytes, brief_digest: SHA256 exact brief bytes, freshness_nonce: this brief fallback_nonce}, deliverable: the complete requested deliverable, unresolved: every remaining gap. Do not include binding/control claims: only the Server can attribute a capture. Publish complete artifact bytes in the assigned private directory, mode0600, and print one complete active-screen line OVRCR_FALLBACK:<lowercase SHA256 of exact artifact bytes>. A marker or child success cannot establish completion: the parent independently inspects authorized evidence, captures current attribution and explicitly decides acceptance. Never create parent recoveries, acceptance files or dispatch receipts; do not overwrite a normal child submission.";

fn names(record: &Record) -> (String, String, String) {
    (
        format!("contract-v{}.json", record.identity.contract),
        format!("brief-v{}.json", record.result_version),
        format!("submission-v{}.json", record.result_version),
    )
}

pub(super) fn context(record: &Record, directory: &Directory) -> Result<FallbackContext> {
    let (contract, brief, _) = names(record);
    let contract_bytes = directory.read(&contract)?;
    let brief_bytes = directory.read(&brief)?;
    if digest(&contract_bytes) != record.contract_digest
        || record.brief_digest.as_deref() != Some(digest(&brief_bytes).as_str())
    {
        bail!("fallback immutable contract or brief changed");
    }
    let value: serde_json::Value = serde_json::from_slice(&brief_bytes)?;
    let context = FallbackContext {
        identity: record.identity.clone(),
        result_version: record.result_version,
        contract_digest: record.contract_digest.clone(),
        brief_digest: record
            .brief_digest
            .clone()
            .context("fallback has no current brief")?,
        freshness_nonce: value["fallback_nonce"]
            .as_str()
            .context("fallback freshness gap: legacy brief has no nonce; use a scoped followup")?
            .into(),
        binding: record
            .binding
            .clone()
            .context("fallback child is not bound")?,
        control: record.control.clone(),
    };
    files::validate_context(&context)?;
    Ok(context)
}

impl ServerState {
    /// Caller holds mutation serialization. Reload after taking retained storage:
    /// explicit human input also updates control without the mutation lock.
    pub(super) fn with_fallback_fence<T>(
        &self,
        auth: &DelegationAuth,
        handle: &str,
        expected: Option<(&AssignmentIdentity, u32, u64)>,
        action: impl FnOnce(
            &mut Record,
            &Directory,
            &FallbackContext,
            &WorkflowAuthority,
            &AssignmentControlEvidence,
            &super::super::SessionStore,
        ) -> Result<T>,
    ) -> Result<T> {
        let initial = self.owned_assignment(auth, handle)?;
        let parent = self.session_for_control(auth.session)?;
        let child = self.assignment_session(&initial)?;
        let store = self.retained.lock();
        let mut record = store
            .assignment(handle)?
            .context("assignment disappeared")?;
        record.upgrade_legacy();
        if record.identity != initial.identity || record.binding != initial.binding {
            bail!("fallback child identity changed");
        }
        if let Some((identity, version, epoch)) = expected {
            if &record.identity != identity || record.result_version != version {
                bail!("fallback expected result is stale");
            }
            control::require_controller(&record.control, epoch)?;
        }
        let directory = Directory::open(&record.root, false)?;
        let current = context(&record, &directory)?;
        let authority = self.require_workflow(&store, auth, &record)?;
        let model = read_contract(&record)?.model;
        let parent_binding = authority.binding.clone();
        let child_binding = current.binding.clone();
        let captured_control = current.control.clone();
        child
            .with_delegated_context(
                &crate::session::DelegatedInput {
                    parent_producer: Some(&authority.producer),
                    parent_source: auth.source.as_ref(),
                    request: handle,
                    control: &captured_control,
                    control_epoch: captured_control.epoch,
                    authority: crate::session::DelegatedAuthority::Controller,
                    parent: &parent,
                    parent_binding: &parent_binding,
                    capability: &auth.capability,
                    child_binding: &child_binding,
                    model: &model,
                    text: "",
                },
                |agent| {
                    let evidence = self.control_evidence(&record, agent)?;
                    action(
                        &mut record,
                        &directory,
                        &current,
                        &authority,
                        &evidence,
                        &store,
                    )
                },
            )?
            .context("fallback current source, child or control binding changed")
    }

    pub(super) fn capture_fallback(
        &self,
        auth: &DelegationAuth,
        handle: &str,
        identity: &AssignmentIdentity,
        result_version: u32,
        epoch: u64,
    ) -> Result<AssignmentSnapshot> {
        let capture = {
            let _mutation = self.mutation_lock.lock().unwrap();
            self.reject_if_stopping()?;
            self.with_fallback_fence(
                auth,
                handle,
                Some((identity, result_version, epoch)),
                |record, directory, current, authority, evidence, _| {
                    if record.captures.len() >= MAX_FALLBACKS {
                        bail!("fallback capture storage is full; history is retained");
                    }
                    let (_, _, original_name) = names(record);
                    let (original, original_bytes) =
                        files::original_condition(directory, &original_name, current)?;
                    // terminal_text takes only the parser lock; no Session state reentry.
                    let child = self
                        .sessions
                        .lock()
                        .unwrap()
                        .get(&identity.session.context("missing child")?)
                        .cloned()
                        .context("missing child")?;
                    hold_before_effect("fallback-after-original", handle)?;
                    let (size, screen) = child.terminal_text();
                    let capture = FallbackCapture {
                        schema: DELEGATION_SCHEMA,
                        id: crate::agent_runner::private_identifier()?,
                        server_lifetime: self.server_lifetime.clone(),
                        authority: authority.clone(),
                        context: current.clone(),
                        evidence: evidence.clone(),
                        captured_unix_ms: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)?
                            .as_millis()
                            .try_into()?,
                        size,
                        truncated: screen.len() > 64 * 1024,
                        screen,
                        original,
                        original_bytes,
                    };
                    let (contract, brief, original) = names(record);
                    validate_capture_files(
                        &FallbackFiles {
                            directory,
                            contract_name: &contract,
                            brief_name: &brief,
                            original_name: &original,
                        },
                        &capture,
                        current,
                        authority,
                        evidence,
                    )?;
                    Ok(capture)
                },
            )?
        };
        // Deliberately outside mutation, retained and Session locks: takeover and
        // source retirement must be able to commit before final revalidation.
        hold_before_effect("fallback-after-screen", handle)?;
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let record = self.with_fallback_fence(
            auth,
            handle,
            Some((identity, result_version, epoch)),
            |record, directory, current, authority, evidence, store| {
                if record.captures.len() >= MAX_FALLBACKS {
                    bail!("fallback capture storage is full");
                }
                let (contract, brief, original) = names(record);
                validate_capture_files(
                    &FallbackFiles {
                        directory,
                        contract_name: &contract,
                        brief_name: &brief,
                        original_name: &original,
                    },
                    &capture,
                    current,
                    authority,
                    evidence,
                )?;
                let next_obligation_revision = record
                    .obligation_revision
                    .checked_add(1)
                    .context("obligation revision exhausted")?;
                let bytes = serde_json::to_vec(&capture)?;
                publish_matching(
                    directory,
                    &format!(
                        "fallback-capture-r{}-v{}.json",
                        current.result_version,
                        record.captures.len() + 1
                    ),
                    &bytes,
                )?;
                if take_fault("fallback-capture-before-save", handle).is_some() {
                    bail!("fallback capture publication exposed but registry commit unconfirmed");
                }
                record.captures.push(CaptureReference {
                    id: capture.id.clone(),
                    digest: digest(&bytes),
                });
                record.obligation_revision = next_obligation_revision;
                store.save_assignment(record)?;
                Ok(record.clone())
            },
        )?;
        let mut snapshot = self.reconciliation_snapshot(record)?;
        snapshot.fallback_capture = Some(capture);
        Ok(snapshot)
    }

    pub(super) fn recover_fallback(
        &self,
        auth: &DelegationAuth,
        handle: &str,
        draft: &FallbackRecoveryDraft,
    ) -> Result<AssignmentSnapshot> {
        hold_before_effect("fallback-before-publication", handle)?;
        let (record, publication) = self.with_fallback_fence(
            auth,
            handle,
            None,
            |record, directory, current, authority, evidence, store| {
                if draft.version == 0 || draft.version > 64 {
                    bail!("invalid recovery version");
                }
                let (slot, reference) = record
                    .captures
                    .iter()
                    .enumerate()
                    .find(|(_, r)| r.id == draft.capture_id)
                    .context("unregistered fallback capture")?;
                let bytes = directory.read(&format!(
                    "fallback-capture-r{}-v{}.json",
                    record.result_version,
                    slot + 1
                ))?;
                if digest(&bytes) != reference.digest {
                    bail!("trusted fallback capture changed");
                }
                let capture: FallbackCapture = serde_json::from_slice(&bytes)?;
                let (contract, brief, original) = names(record);
                let sources = FallbackFiles {
                    directory,
                    contract_name: &contract,
                    brief_name: &brief,
                    original_name: &original,
                };
                let recovery = files::prepare_recovery(
                    &sources, &capture, current, authority, evidence, draft,
                )?;
                let bytes = serde_json::to_vec(&recovery)?;
                let reference = RecoveryReference {
                    version: draft.version,
                    digest: digest(&bytes),
                };
                let existing = record
                    .recoveries
                    .iter()
                    .find(|r| r.version == draft.version);
                if existing.is_some_and(|r| r != &reference) {
                    bail!("immutable recovery reference changed");
                }
                if existing.is_none() && record.recoveries.len() >= MAX_FALLBACKS {
                    bail!("fallback recovery storage is full; history is retained");
                }
                let next_obligation_revision = if existing.is_none() {
                    Some(
                        record
                            .obligation_revision
                            .checked_add(1)
                            .context("obligation revision exhausted")?,
                    )
                } else {
                    None
                };
                let name =
                    files::publish_recovery(&sources, current, authority, evidence, &recovery)?;
                if existing.is_none() {
                    if take_fault("fallback-recovery-before-save", handle).is_some() {
                        bail!("fallback recovery file exposed but registry commit unconfirmed");
                    }
                    record.recoveries.push(reference.clone());
                    record.obligation_revision = next_obligation_revision.unwrap();
                    store.save_assignment(record)?;
                }
                Ok((
                    record.clone(),
                    RecoveryPublication {
                        path: record.root.join(name),
                        digest: reference.digest,
                        recovery,
                    },
                ))
            },
        )?;
        if let Some(session) = record.identity.session {
            self.delegation_observations.invalidate(session);
        }
        let mut snapshot = self.reconciliation_snapshot(record)?;
        snapshot.recovered = Some(publication);
        Ok(snapshot)
    }
}

pub(super) fn read_recovery(
    record: &Record,
    version: &VersionState,
    recovery_version: u32,
    expected: &str,
) -> Result<FallbackRecovery> {
    let reference = version
        .recoveries
        .iter()
        .find(|r| r.version == recovery_version)
        .ok_or_else(|| {
            lifecycle_error(
                ErrorCode::Conflict,
                "recovery is not registered with this result",
            )
        })?;
    if reference.digest != expected {
        return Err(lifecycle_error(
            ErrorCode::Conflict,
            "recovery digest does not match registration",
        ));
    }
    let directory = Directory::open(&record.root, false)?;
    let bytes = directory.read(&format!(
        "parent-fallback-r{}-v{}.json",
        version.result_version, recovery_version
    ))?;
    if digest(&bytes) != expected {
        return Err(lifecycle_error(
            ErrorCode::Conflict,
            "registered recovery bytes changed",
        ));
    }
    let recovery: FallbackRecovery = serde_json::from_slice(&bytes)?;
    let capture = &recovery.capture;
    let capture_digest = digest(&serde_json::to_vec(capture)?);
    if recovery.version != recovery_version
        || capture.context.identity != version.identity
        || capture.context.result_version != version.result_version
        || capture.context.contract_digest != version.contract_digest
        || Some(&capture.context.brief_digest) != version.brief_digest.as_ref()
        || !version
            .captures
            .iter()
            .any(|r| r.id == capture.id && r.digest == capture_digest)
    {
        bail!("registered recovery provenance changed");
    }
    verify_frozen(&recovery)?;
    Ok(recovery)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_metadata_capacity_refusal_preserves_the_last_committed_record() {
        let directory = tempfile::tempdir().unwrap();
        let record = super::super::tests::record(
            directory.path().join("assignment"),
            DispatchStatus::Applied,
        );
        let store = crate::retained::SessionStore::in_memory();
        store.save_assignment(&record).unwrap();
        let original =
            serde_json::to_vec(&store.assignment(&record.handle).unwrap().unwrap()).unwrap();
        for overflow in ["captures", "recoveries", "metadata"] {
            let mut changed = record.clone();
            match overflow {
                "captures" => {
                    changed.captures = (0..65)
                        .map(|n| CaptureReference {
                            id: format!("capture-{n}"),
                            digest: "a".repeat(64),
                        })
                        .collect()
                }
                "recoveries" => {
                    changed.recoveries = (1..=65)
                        .map(|version| RecoveryReference {
                            version,
                            digest: "b".repeat(64),
                        })
                        .collect()
                }
                "metadata" => changed.reason = Some("x".repeat(2 * 1024 * 1024)),
                _ => unreachable!(),
            }
            assert!(
                store.save_assignment(&changed).is_err(),
                "{overflow} must refuse before replacing the durable row"
            );
            assert_eq!(
                serde_json::to_vec(&store.assignment(&record.handle).unwrap().unwrap()).unwrap(),
                original
            );
        }
    }
}

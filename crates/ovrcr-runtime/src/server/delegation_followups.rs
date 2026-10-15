//! Same-assignment continuations; no controller transfer or replacement attempts.
use super::*;

pub(super) const MAX_FOLLOWUPS: usize = 64;

pub(super) fn initial_result_version() -> u32 {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct VersionState {
    pub identity: AssignmentIdentity,
    pub result_version: u32,
    pub contract_digest: String,
    pub brief_digest: Option<String>,
    pub result_digest: Option<String>,
    pub acceptance: Option<AcceptanceReference>,
    pub publication_intent: Option<PublicationIntent>,
    pub handled_publication: Option<PublicationIdentity>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FollowupIntent {
    pub request: String,
    pub expected: AssignmentIdentity,
    pub expected_result_version: u32,
    #[serde(default)]
    pub expected_control_epoch: u64,
    #[serde(default)]
    pub expected_parent_epoch: Option<u64>,
    pub target: AssignmentIdentity,
    pub result_version: u32,
    pub digest: String,
    pub contract_digest: String,
    pub brief_digest: String,
}

impl Record {
    pub(super) fn version(&self) -> VersionState {
        VersionState {
            identity: self.identity.clone(),
            result_version: self.result_version,
            contract_digest: self.contract_digest.clone(),
            brief_digest: self.brief_digest.clone(),
            result_digest: self.result_digest.clone(),
            acceptance: self.acceptance.clone(),
            publication_intent: self.publication_intent.clone(),
            handled_publication: self.handled_publication.clone(),
        }
    }

    pub(super) fn request_receipt(
        &self,
        request: &str,
        operation: DispatchOperation,
    ) -> Option<&DispatchReceipt> {
        self.receipts
            .iter()
            .find(|r| r.request == request && r.operation == operation)
    }

    fn followup_outcome(
        &mut self,
        request: &str,
        outcome: DispatchOutcome,
        input: Option<InputDelivery>,
        reason: Option<&str>,
    ) {
        let receipt = self
            .receipts
            .iter_mut()
            .find(|r| r.request == request && r.operation == DispatchOperation::Followup)
            .expect("durable followup intent");
        receipt.outcome = outcome;
        receipt.input = input;
        receipt.reason = reason.map(str::to_owned);
        self.reason = reason.map(str::to_owned);
    }

    pub(super) fn unresolved_input(&self) -> bool {
        self.receipts.iter().any(|r| {
            matches!(
                r.operation,
                DispatchOperation::InitialInput | DispatchOperation::Followup
            ) && matches!(
                r.outcome,
                DispatchOutcome::Unknown | DispatchOutcome::Partial
            )
        })
    }

    pub(super) fn version_reference(&self, version: &VersionState) -> AssignmentVersion {
        AssignmentVersion {
            identity: version.identity.clone(),
            result_version: version.result_version,
            contract: self
                .root
                .join(format!("contract-v{}.json", version.identity.contract)),
            brief: self
                .root
                .join(format!("brief-v{}.json", version.result_version)),
            publication: version.result_digest.as_ref().map(|hash| {
                let identity = PublicationIdentity {
                    assignment: version.identity.clone(),
                    kind: PublicationKind::Submission,
                    version: version.result_version,
                    digest: hash.clone(),
                };
                PublicationState {
                    handled: version.handled_publication.as_ref() == Some(&identity),
                    identity,
                }
            }),
            acceptance: version.acceptance.clone(),
        }
    }
}

fn publish_matching(directory: &Directory, name: &str, bytes: &[u8]) -> Result<()> {
    if let Err(error) = directory.publish(name, bytes)
        && directory.read(name).ok().as_deref() != Some(bytes)
    {
        return Err(error);
    }
    directory.sync()
}

fn valid_request(request: &str) -> bool {
    !request.is_empty()
        && request.len() <= 128
        && request
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn revision_contract(
    original: &ResearchContract,
    revised: Option<ResearchContract>,
) -> Result<ResearchContract> {
    let Some(revised) = revised else {
        return Ok(original.clone());
    };
    revised
        .validate()
        .map_err(|r| lifecycle_error(ErrorCode::InvalidRequest, r))?;
    let mut allowed = original.clone();
    allowed.scope = revised.scope.clone();
    allowed.acceptance_evidence = revised.acceptance_evidence.clone();
    if allowed != revised {
        return Err(lifecycle_error(
            ErrorCode::Conflict,
            "a same-child revision may change only scope and acceptance evidence; other selections require a separate assignment",
        ));
    }
    Ok(revised)
}

fn matching_legacy_brief(
    bytes: Vec<u8>,
    contract: &ResearchContract,
    expected: &str,
) -> Result<Vec<u8>> {
    if digest(&bytes) != expected && legacy_contract_bytes(contract)?.is_some() {
        let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
        if let Some(nested) = value
            .get_mut("contract")
            .and_then(serde_json::Value::as_object_mut)
        {
            nested.remove("limits");
            nested.remove("previous_request");
            let legacy = serde_json::to_vec_pretty(&value)?;
            if digest(&legacy) == expected {
                return Ok(legacy);
            }
        }
    }
    Ok(bytes)
}

fn followup_payload(
    expected: &AssignmentIdentity,
    version: u32,
    control_epoch: u64,
    text: &str,
    revised: &Option<ResearchContract>,
    saved_digest: Option<&str>,
) -> Result<Vec<u8>> {
    let mut payload = serde_json::to_vec(&(expected, version, control_epoch, text, revised))?;
    // Reconstruct only the old canonical nested shape, and only under its exact
    // durable digest. Changed semantics cannot inherit a prior request identity.
    if let Some(saved) = saved_digest
        && digest(&payload) != saved
        && let Some(revised) = revised.as_ref()
        && let Some(legacy) = legacy_contract_bytes(revised)?
    {
        let mut candidate = serde_json::to_vec(&(expected, version, control_epoch, text))?;
        candidate.pop();
        candidate.push(b',');
        candidate.extend_from_slice(&legacy);
        candidate.push(b']');
        if digest(&candidate) == saved {
            payload = candidate;
        }
    }
    Ok(payload)
}

impl ServerState {
    pub(super) fn followup(
        &self,
        auth: DelegationAuth,
        handle: String,
        request: String,
        expected: (AssignmentIdentity, u32, u64),
        text: String,
        revised: Option<ResearchContract>,
    ) -> Result<AssignmentSnapshot> {
        let (expected, expected_result_version, expected_control_epoch) = expected;
        if !valid_request(&request) || text.trim().is_empty() || text.len() > 32_768 {
            return Err(lifecycle_error(
                ErrorCode::InvalidRequest,
                "followup requires a bounded request identity and 1..32768 bytes of text",
            ));
        }
        let mut record = self.owned_assignment(&auth, &handle)?;
        let parent_authority = self.current_workflow_authority(&auth, &record.identity.workflow)?;
        let payload = followup_payload(
            &expected,
            expected_result_version,
            expected_control_epoch,
            &text,
            &revised,
            record
                .followups
                .iter()
                .find(|f| f.request == request)
                .map(|f| f.digest.as_str()),
        )?;
        let payload_digest = digest(&payload);
        let intent = if let Some(intent) = record
            .followups
            .iter()
            .find(|f| f.request == request)
            .cloned()
        {
            if intent.digest != payload_digest
                || intent.expected != expected
                || intent.expected_result_version != expected_result_version
                || intent.expected_control_epoch != expected_control_epoch
            {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "followup request already belongs to a different intent",
                ));
            }
            let receipt = record
                .request_receipt(&request, DispatchOperation::Followup)
                .context("missing followup receipt")?;
            if receipt.outcome != DispatchOutcome::Accepted {
                return self.reconciliation_snapshot(record);
            }
            intent
        } else {
            control::require_controller(&record.control, expected_control_epoch)?;
            if expected != record.identity || expected_result_version != record.result_version {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "followup identity is stale",
                ));
            }
            if record.receipts.iter().any(|r| r.request == request) {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "request already identifies another operation",
                ));
            }
            if record.unresolved_input()
                || record.followups.iter().any(|f| {
                    record
                        .request_receipt(&f.request, DispatchOperation::Followup)
                        .is_none_or(|r| r.outcome == DispatchOutcome::Accepted)
                })
            {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "original input remains unresolved; reconcile its request before any new followup",
                ));
            }
            if !record
                .receipt(DispatchOperation::InitialInput)
                .is_some_and(|r| r.outcome == DispatchOutcome::Applied)
            {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "initial input is not confirmed applied",
                ));
            }
            if record.followups.len() >= MAX_FOLLOWUPS {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "followup receipt storage is full (64 followups); history and unresolved receipts are retained",
                ));
            }
            if record.publication_intent.as_ref().is_some_and(|intent| {
                record.result_digest.as_ref() != Some(&intent.identity.digest)
            }) {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "original publication is unresolved; recover it before changing the expected result",
                ));
            }
            self.assignment_session(&record)?;
            let original = read_contract(&record)?;
            let changed = revised.is_some();
            let contract = revision_contract(&original, revised.clone())?;
            let mut target = expected.clone();
            if changed {
                target.contract = record
                    .followups
                    .iter()
                    .map(|intent| intent.target.contract)
                    .fold(target.contract, u32::max)
                    .checked_add(1)
                    .context("contract version exhausted")?;
            }
            // Rejected old-epoch intents keep their immutable reserved files.
            let result_version = record
                .followups
                .iter()
                .map(|intent| intent.result_version)
                .fold(record.result_version, u32::max)
                .checked_add(1)
                .context("result version exhausted")?;
            let bytes = contract_bytes(
                &contract,
                (!changed).then_some(record.contract_digest.as_str()),
            )?;
            let brief =
                followup_brief(&record, &target, result_version, &contract, &request, &text)?;
            let intent = FollowupIntent {
                request: request.clone(),
                expected: expected.clone(),
                expected_result_version,
                expected_control_epoch,
                expected_parent_epoch: Some(parent_authority.epoch),
                target: target.clone(),
                result_version,
                digest: payload_digest.clone(),
                contract_digest: digest(&bytes),
                brief_digest: digest(&brief),
            };
            record.followups.push(intent.clone());
            record.receipts.push(DispatchReceipt {
                request: request.clone(),
                operation: DispatchOperation::Followup,
                result_version,
                identity: target,
                outcome: DispatchOutcome::Accepted,
                input: None,
                reason: None,
            });
            self.retained.lock().save_assignment(&record)?;
            intent
        };
        // Completed receipts remain readable. Accepted work never adopts a replacement parent.
        if intent.expected_parent_epoch.unwrap_or(1) != parent_authority.epoch {
            return self.reject_followup(record, &request);
        }
        // Accepted requests keep their captured authority after interrupted publication.
        if control::require_controller(&record.control, intent.expected_control_epoch).is_err() {
            return self.reject_followup(record, &request);
        }
        if self.assignment_session(&record).is_err() {
            return self.reject_followup(record, &request);
        }
        if record.identity != intent.expected
            || record.result_version != intent.expected_result_version
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "followup publication no longer targets the expected version",
            ));
        }
        let original = read_contract(&record)?;
        let contract = revision_contract(&original, revised)?;
        let brief = followup_brief(
            &record,
            &intent.target,
            intent.result_version,
            &contract,
            &request,
            &text,
        )?;
        let brief = matching_legacy_brief(brief, &contract, &intent.brief_digest)?;
        let bytes = contract_bytes(&contract, Some(&intent.contract_digest))?;
        if digest(&bytes) != intent.contract_digest || digest(&brief) != intent.brief_digest {
            bail!("followup publication does not match durable intent");
        }
        let publication_guard = self.retained.lock();
        let authority = self.require_workflow(&publication_guard, &auth, &record)?;
        if intent.expected_parent_epoch.unwrap_or(1) != authority.epoch { bail!("followup publication parent authority changed"); }
        let directory = Directory::open(&record.root, false)?;
        publish_matching(&directory, &format!("followup-{request}.json"), &payload)?;
        if take_fault("followup-before-files", &request).is_some() {
            drop(publication_guard);
            return self.reconciliation_snapshot(record);
        }
        publish_matching(
            &directory,
            &format!("contract-v{}.json", intent.target.contract),
            &bytes,
        )?;
        publish_matching(
            &directory,
            &format!("brief-v{}.json", intent.result_version),
            &brief,
        )?;
        if take_fault("followup-after-files", &request).is_some() {
            drop(publication_guard);
            return self.reconciliation_snapshot(record);
        }
        drop(publication_guard);
        hold_before_input(&request)?;
        let session = match self.assignment_session(&record) {
            Ok(session) => session,
            Err(_) => return self.reject_followup(record, &request),
        };
        let parent = self.session_for_control(auth.session)?;
        if record.identity != intent.expected
            || record.result_version != intent.expected_result_version
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "followup expected version changed before advance",
            ));
        }
        control::require_controller(&record.control, intent.expected_control_epoch)?;
        record.previous_versions.push(record.version());
        record.control.attribution_invalidated = false;
        record.identity = intent.target;
        record.result_version = intent.result_version;
        record.contract_digest = intent.contract_digest;
        record.brief_digest = Some(intent.brief_digest);
        record.result_digest = None;
        record.publication_intent = None;
        record.handled_publication = None;
        record.acceptance = None;
        record.dispatch = DispatchStatus::InputUnknown;
        record.followup_outcome(
            &request,
            DispatchOutcome::Unknown,
            None,
            Some("followup input effect is unknown; do not replay"),
        );
        let store = self.retained.lock();
        let authority = self.require_workflow(&store, &auth, &record)?;
        if intent.expected_parent_epoch.unwrap_or(1) != authority.epoch { bail!("captured followup parent authority changed"); }
        store.save_assignment(&record)?;
        let prompt = format!(
            "Perform the read-only research assignment in {}. Read that immutable brief with your read tool; this is a scoped followup in the same assignment. Publish the exact requested result version with ovrcr_publish using handle {} and the identity in that brief. Do not modify task or parent records.",
            record
                .root
                .join(format!("brief-v{}.json", record.result_version))
                .display(),
            record.handle
        );
        match session.send_delegated_text(crate::session::DelegatedInput {
                    parent_producer: Some(&authority.producer),
                    parent_source: auth.source.as_ref(),
            request: &request,
            control: &record.control,
            control_epoch: intent.expected_control_epoch,
            authority: crate::session::DelegatedAuthority::Controller,
            parent: &parent,
            parent_binding: &authority.binding,
            capability: &auth.capability,
            child_binding: record
                .binding
                .as_ref()
                .context("child assignment not bound")?,
            model: &contract.model,
            text: &prompt,
        }) {
            Ok(crate::session::DelegatedDelivery::Applied(input)) => {
                if take_fault("input-after-effect", &request).is_some() {
                    drop(store);
                    self.delegation_observations.invalidate(session.id());
                    return self.reconciliation_snapshot(record);
                }
                record.dispatch = DispatchStatus::Applied;
                record.followup_outcome(&request, DispatchOutcome::Applied, Some(input), None);
            }
            Ok(crate::session::DelegatedDelivery::Rejected(input)) => {
                record.dispatch = DispatchStatus::Refused;
                record.followup_outcome(
                    &request,
                    DispatchOutcome::Rejected,
                    Some(input),
                    Some("followup rejected without writing bytes"),
                );
            }
            Ok(crate::session::DelegatedDelivery::Partial(input)) => record.followup_outcome(
                &request,
                DispatchOutcome::Partial,
                Some(input),
                Some("only a prefix reached the PTY; do not replay"),
            ),
            Ok(crate::session::DelegatedDelivery::Unknown(input)) => record.followup_outcome(
                &request,
                DispatchOutcome::Unknown,
                Some(input),
                Some("input bytes reached the PTY but completion is uncertain; do not replay"),
            ),
            Err(_) => (),
        }
        store.save_assignment(&record)?;
        drop(store);
        if let Some(session) = record.identity.session {
            self.delegation_observations.invalidate(session);
        }
        self.reconciliation_snapshot(record)
    }

    fn reject_followup(&self, mut record: Record, request: &str) -> Result<AssignmentSnapshot> {
        record.dispatch = DispatchStatus::Refused;
        record.followup_outcome(
            request,
            DispatchOutcome::Rejected,
            None,
            Some("child run or conversation changed before followup input; no bytes written"),
        );
        self.retained.lock().save_assignment(&record)?;
        self.reconciliation_snapshot(record)
    }

    pub(super) fn collect_version(
        &self,
        auth: DelegationAuth,
        handle: String,
        version: u32,
    ) -> Result<AssignmentSnapshot> {
        let record = self.owned_assignment(&auth, &handle)?;
        let selected = if record.result_version == version {
            record.version()
        } else {
            record
                .previous_versions
                .iter()
                .find(|v| v.result_version == version)
                .cloned()
                .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "result version not found"))?
        };
        let submission = read_version_submission(&record, &selected)?;
        verify_acceptance(&record, &selected)?;
        let mut snapshot = self.reconciliation_snapshot(record)?;
        snapshot.historical_submission = submission;
        snapshot.selected_version = Some(version);
        Ok(snapshot)
    }

    pub(super) fn record_acceptance(
        &self,
        auth: DelegationAuth,
        handle: String,
        identity: AssignmentIdentity,
        result_version: u32,
        result_digest: String,
        expected_control_epoch: u64,
    ) -> Result<AssignmentSnapshot> {
        let mut record = self.owned_assignment(&auth, &handle)?;
        control::require_controller(&record.control, expected_control_epoch)?;
        if record.control.attribution_invalidated {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "human changes require a fresh result version before acceptance",
            ));
        }
        if record.identity != identity
            || record.result_version != result_version
            || record.result_digest.as_ref() != Some(&result_digest)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "acceptance must identify the exact current result",
            ));
        }
        self.assignment_session(&record)?;
        read_version_submission(&record, &record.version())?
            .context("no published result to accept")?;
        let bytes = serde_json::to_vec(
            &serde_json::json!({ "schema": 1, "identity": identity, "result_version": result_version, "result_digest": result_digest, "parent": record.parent, "parent_run": record.parent_run, "parent_binding": record.parent_binding }),
        )?;
        let name = format!("acceptance-v{result_version}.json");
        let reference = AcceptanceReference {
            identity,
            result_version,
            result_digest,
            path: record.root.join(&name),
            digest: digest(&bytes),
        };
        if let Some(previous) = &record.acceptance
            && previous != &reference
        {
            bail!("immutable acceptance reference changed");
        }
        let parent = self.session_for_control(auth.session)?;
        let session = self.assignment_session(&record)?;
        let child_binding = record
            .binding
            .clone()
            .context("child assignment not bound")?;
        let model = read_contract(&record)?.model;
        // Same retained -> ordered Session -> file/metadata boundary as scoped input.
        let store = self.retained.lock();
        let authority = self.require_workflow(&store, &auth, &record)?;
        let parent_binding = authority.binding.clone();
        let captured_control = record.control.clone();
        session
            .with_delegated_context(
                &crate::session::DelegatedInput {
                    parent_producer: Some(&authority.producer),
                    parent_source: auth.source.as_ref(),
                    request: &handle,
                    control: &captured_control,
                    control_epoch: expected_control_epoch,
                    authority: crate::session::DelegatedAuthority::Controller,
                    parent: &parent,
                    parent_binding: &parent_binding,
                    capability: &auth.capability,
                    child_binding: &child_binding,
                    model: &model,
                    text: "",
                },
                |_| {
                    publish_matching(&Directory::open(&record.root, false)?, &name, &bytes)?;
                    record.acceptance = Some(reference);
                    store.save_assignment(&record)
                },
            )?
            .ok_or_else(|| {
                lifecycle_error(
                    ErrorCode::Conflict,
                    "acceptance parent or child binding changed",
                )
            })?;
        drop(store);
        self.assignment_snapshot(record)
    }
}

fn followup_brief(
    record: &Record,
    identity: &AssignmentIdentity,
    version: u32,
    contract: &ResearchContract,
    request: &str,
    text: &str,
) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(
        &serde_json::json!({ "schema": 1, "handle": record.handle, "identity": identity, "contract": contract, "request": request, "followup": text, "result_version": version, "output": record.root.join(format!("submission-v{version}.json")), "publication": "Call ovrcr_publish with the exact identity and result_version as submission.version. Only this child can publish; it cannot accept the result. Research tools remain read-only." }),
    )?)
}

pub(super) fn verify_acceptance(record: &Record, version: &VersionState) -> Result<()> {
    let Some(reference) = &version.acceptance else {
        return Ok(());
    };
    let name = format!("acceptance-v{}.json", version.result_version);
    if reference.identity != version.identity
        || reference.result_version != version.result_version
        || version.result_digest.as_ref() != Some(&reference.result_digest)
        || reference.path != record.root.join(&name)
    {
        bail!("acceptance reference does not match the selected result");
    }
    if digest(&Directory::open(&record.root, false)?.read(&name)?) != reference.digest {
        bail!("immutable acceptance publication changed");
    }
    Ok(())
}

pub(super) fn read_version_submission(
    record: &Record,
    version: &VersionState,
) -> Result<Option<ResearchSubmission>> {
    let Some(expected) = &version.result_digest else {
        return Ok(None);
    };
    let directory = Directory::open(&record.root, false)?;
    if digest(&directory.read(&format!("contract-v{}.json", version.identity.contract))?)
        != version.contract_digest
    {
        bail!("immutable contract changed");
    }
    if let Some(expected) = &version.brief_digest
        && &digest(&directory.read(&format!("brief-v{}.json", version.result_version))?) != expected
    {
        bail!("immutable brief changed");
    }
    let bytes = directory.read(&format!("submission-v{}.json", version.result_version))?;
    if &digest(&bytes) != expected {
        bail!("submission publication changed or is partial");
    }
    let result: ResearchSubmission = serde_json::from_slice(&bytes)?;
    result
        .validate(&version.identity)
        .map_err(anyhow::Error::msg)?;
    if result.version != version.result_version {
        bail!("submission version does not match the selected publication");
    }
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_nested_revision_requires_exact_saved_payload_digest() {
        let dir = tempfile::tempdir().unwrap();
        let record =
            super::super::tests::record(dir.path().join("assignment"), DispatchStatus::Applied);
        let contract = read_contract(&record).unwrap();
        let text = "scoped clarification";
        let mut old = serde_json::to_vec(&(&record.identity, 1_u32, 1_u64, text)).unwrap();
        old.pop();
        old.push(b',');
        old.extend_from_slice(&legacy_contract_bytes(&contract).unwrap().unwrap());
        old.push(b']');
        let hash = digest(&old);
        assert_eq!(
            followup_payload(
                &record.identity,
                1,
                1,
                text,
                &Some(contract.clone()),
                Some(&hash)
            )
            .unwrap(),
            old
        );
        let mut changed = contract.clone();
        changed.limits.max_execution_attempts = Some(2);
        for revised in [changed, {
            let mut c = contract.clone();
            c.scope.push_str(" changed");
            c
        }] {
            assert_ne!(
                digest(
                    &followup_payload(&record.identity, 1, 1, text, &Some(revised), Some(&hash))
                        .unwrap()
                ),
                hash
            );
        }
        assert_ne!(
            digest(
                &followup_payload(
                    &record.identity,
                    1,
                    1,
                    "changed text",
                    &Some(contract),
                    Some(&hash)
                )
                .unwrap()
            ),
            hash
        );
    }

    #[test]
    fn contract_revision_changes_only_scope_and_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let record =
            super::super::tests::record(dir.path().join("assignment"), DispatchStatus::Applied);
        let original = read_contract(&record).unwrap();
        let mut changed = original.clone();
        changed.scope = "Clarified read-only requirements".into();
        changed.acceptance_evidence = vec!["new citation".into()];
        assert_eq!(
            revision_contract(&original, Some(changed.clone())).unwrap(),
            changed
        );
        for field in [
            "task",
            "workflow",
            "controller",
            "request",
            "workspace",
            "project",
            "model",
            "model_provider",
        ] {
            let mut value = serde_json::to_value(&changed).unwrap();
            value[field] = "replacement".into();
            assert!(
                revision_contract(&original, Some(serde_json::from_value(value).unwrap())).is_err(),
                "must preserve {field}"
            );
        }
        assert!(!valid_request("../escape"));
        assert!(!valid_request(&"a".repeat(129)));
    }

    #[test]
    fn schema_eight_record_defaults_preserve_exact_original_receipts_and_identity() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = super::super::tests::record(
            dir.path().join("assignment"),
            DispatchStatus::InputUnknown,
        );
        record.upgrade_legacy();
        let mut value = serde_json::to_value(&record).unwrap();
        for field in [
            "result_version",
            "publication_intent",
            "handled_publication",
            "acceptance",
            "previous_versions",
            "followups",
            "control",
            "takeover_evidence",
            "control_intents",
            "initial_control_epoch",
        ] {
            value.as_object_mut().unwrap().remove(field);
        }
        for receipt in value["receipts"].as_array_mut().unwrap() {
            receipt.as_object_mut().unwrap().remove("result_version");
        }
        let mut restored: Record = serde_json::from_value(value).unwrap();
        restored.upgrade_legacy();
        assert_eq!(restored.identity, record.identity);
        assert_eq!(restored.receipts, record.receipts);
        assert_eq!(restored.request_digests, record.request_digests);
        assert_eq!(restored.contract_digest, record.contract_digest);
        assert_eq!(restored.result_version, 1);
        assert!(restored.previous_versions.is_empty());
        assert!(restored.acceptance.is_none());
        assert!(restored.publication_intent.is_none());
        assert!(restored.handled_publication.is_none());
        assert_eq!(restored.control, AssignmentControl::default());
        assert!(
            restored.initial_control_epoch.is_none(),
            "legacy initial intent never adopts a current write epoch"
        );
        assert!(restored.control_intents.is_empty());
    }

    #[test]
    fn legacy_followup_keeps_original_digest_without_gaining_current_authority() {
        let dir = tempfile::tempdir().unwrap();
        let record =
            super::super::tests::record(dir.path().join("assignment"), DispatchStatus::Applied);
        let original = serde_json::json!({
            "request":"legacy-followup", "expected":record.identity, "expected_result_version":1,
            "target":record.identity, "result_version":2, "digest":"original-payload-digest",
            "contract_digest":"original-contract", "brief_digest":"original-brief"
        });
        let restored: FollowupIntent = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(restored.expected_control_epoch, 0);
        assert!(
            control::require_controller(&record.control, restored.expected_control_epoch).is_err()
        );
        let mut saved = serde_json::to_value(restored).unwrap();
        saved
            .as_object_mut()
            .unwrap()
            .remove("expected_control_epoch");
        assert_eq!(
            saved, original,
            "migration preserves every immutable intent field and original digest"
        );
    }

    #[test]
    fn historical_collection_rejects_changed_contract_and_brief() {
        let dir = tempfile::tempdir().unwrap();
        let mut record =
            super::super::tests::record(dir.path().join("assignment"), DispatchStatus::Applied);
        let directory = Directory::open(&record.root, false).unwrap();
        let result = ResearchSubmission {
            schema: 1,
            identity: record.identity.clone(),
            version: 1,
            outcome: "success".into(),
            summary: "original result".into(),
            evidence: vec!["citation".into()],
            unresolved: vec![],
        };
        let bytes = serde_json::to_vec(&result).unwrap();
        directory.publish("submission-v1.json", &bytes).unwrap();
        record.result_digest = Some(digest(&bytes));
        let brief = b"immutable original brief";
        directory.publish("brief-v1.json", brief).unwrap();
        record.brief_digest = Some(digest(brief));
        assert_eq!(
            read_version_submission(&record, &record.version()).unwrap(),
            Some(result)
        );
        let contract = directory.read("contract-v1.json").unwrap();
        std::fs::write(record.root.join("contract-v1.json"), "changed contract").unwrap();
        assert!(
            read_version_submission(&record, &record.version()).is_err(),
            "old result bytes alone cannot authenticate a changed contract"
        );
        std::fs::write(record.root.join("contract-v1.json"), contract).unwrap();
        std::fs::write(record.root.join("brief-v1.json"), "changed brief").unwrap();
        assert!(
            read_version_submission(&record, &record.version()).is_err(),
            "old result bytes alone cannot authenticate a changed brief"
        );
    }

    #[test]
    fn unresolved_input_is_an_assignment_barrier_not_a_request_filter() {
        let dir = tempfile::tempdir().unwrap();
        let mut record =
            super::super::tests::record(dir.path().join("assignment"), DispatchStatus::Applied);
        record.upgrade_legacy();
        assert!(!record.unresolved_input());
        record.receipts.push(DispatchReceipt {
            request: "original-followup".into(),
            operation: DispatchOperation::Followup,
            result_version: 2,
            identity: record.identity.clone(),
            outcome: DispatchOutcome::Partial,
            input: Some(InputDelivery {
                written_bytes: 7,
                total_bytes: 100,
                submit_written: false,
            }),
            reason: None,
        });
        assert!(record.unresolved_input());
        assert!(
            record
                .request_receipt("fresh-request", DispatchOperation::Followup)
                .is_none()
        );
        record.followup_outcome("original-followup", DispatchOutcome::Unknown, None, None);
        assert!(record.unresolved_input());
        record.followup_outcome("original-followup", DispatchOutcome::Rejected, None, None);
        assert!(!record.unresolved_input());
    }
}

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
    #[serde(default)]
    pub captures: Vec<CaptureReference>,
    #[serde(default)]
    pub recoveries: Vec<RecoveryReference>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_parent_epoch: Option<u64>,
    pub target: AssignmentIdentity,
    pub result_version: u32,
    pub digest: String,
    pub contract_digest: String,
    pub brief_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_nonce: Option<String>,
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
            captures: self.captures.clone(),
            recoveries: self.recoveries.clone(),
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
                DispatchOperation::InitialInput
                    | DispatchOperation::Followup
                    | DispatchOperation::AnswerQuestion
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
            recoveries: version.recoveries.clone(),
        }
    }
}

pub(super) fn valid_request(request: &str) -> bool {
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
    if digest(&bytes) == expected {
        return Ok(bytes);
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    for old_coding in [false, true] {
        if old_coding && contract.coding.is_some() {
            continue;
        }
        for old_questions in [false, true] {
            for old_contract in [false, true] {
                if old_contract && legacy_contract_bytes(contract)?.is_none() {
                    continue;
                }
                let mut candidate = value.clone();
                if old_coding
                    && let Some(nested) = candidate
                        .get_mut("contract")
                        .and_then(serde_json::Value::as_object_mut)
                {
                    nested.remove("coding");
                }
                if old_questions {
                    candidate
                        .as_object_mut()
                        .context("brief is not an object")?
                        .remove("questions");
                }
                if old_contract
                    && let Some(nested) = candidate
                        .get_mut("contract")
                        .and_then(serde_json::Value::as_object_mut)
                {
                    nested.remove("limits");
                    nested.remove("previous_request");
                }
                let candidate = serde_json::to_vec_pretty(&candidate)?;
                if digest(&candidate) == expected {
                    return Ok(candidate);
                }
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
    if let (Some(saved), Some(contract)) = (saved_digest, revised.as_ref())
        && contract.coding.is_none()
        && digest(&payload) != saved
    {
        let mut candidate = serde_json::to_vec(&(expected, version, control_epoch, text))?;
        candidate.pop();
        candidate.push(b',');
        candidate.extend_from_slice(&research_contract_bytes(contract)?);
        candidate.push(b']');
        if digest(&candidate) == saved {
            payload = candidate;
        }
    }
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
        self.require_dispatch_open(&self.retained.lock(), &record.identity.workflow)?;
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
            if record.delivery_request_used(&request) {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "request already identifies another operation",
                ));
            }
            if record.unresolved_input()
                || record.pending_answer()
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
            }) && read_accepted_recovery(&record, &record.version())?.is_none()
            {
                return Err(lifecycle_error(
                    ErrorCode::Conflict,
                    "original publication is unresolved; recover it before changing the expected result",
                ));
            }
            self.assignment_session(&record)?;
            self.require_dependency_fresh(&record)?;
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
            let fallback_nonce = crate::agent_runner::private_identifier()?;
            let brief = followup_brief(
                &record,
                &target,
                result_version,
                &contract,
                &request,
                &text,
                Some(&fallback_nonce),
            )?;
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
                fallback_nonce: Some(fallback_nonce),
            };
            record.obligation_revision = record
                .obligation_revision
                .checked_add(1)
                .context("obligation revision exhausted")?;
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
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "followup belongs to a retired workflow controller; original Accepted intent remains suspended without replay",
            ));
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
        if original.coding.is_some() {
            self.coding_workspace(&record)?;
        }
        let contract = revision_contract(&original, revised)?;
        let brief = followup_brief(
            &record,
            &intent.target,
            intent.result_version,
            &contract,
            &request,
            &text,
            intent.fallback_nonce.as_deref(),
        )?;
        let brief = matching_legacy_brief(brief, &contract, &intent.brief_digest)?;
        let bytes = contract_bytes(&contract, Some(&intent.contract_digest))?;
        if digest(&bytes) != intent.contract_digest || digest(&brief) != intent.brief_digest {
            bail!("followup publication does not match durable intent");
        }
        let publication_guard = self.retained.lock();
        let authority = self.require_workflow(&publication_guard, &auth, &record)?;
        self.require_dispatch_open(&publication_guard, &record.identity.workflow)?;
        if intent.expected_parent_epoch.unwrap_or(1) != authority.epoch {
            bail!("followup publication parent authority changed");
        }
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
        self.require_dependency_fresh(&record)?;
        hold_before_input(&request)?;
        self.require_dependency_fresh(&record)?;
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
        record.captures.clear();
        record.recoveries.clear();
        record.dispatch = DispatchStatus::InputUnknown;
        record.followup_outcome(
            &request,
            DispatchOutcome::Unknown,
            None,
            Some("followup input effect is unknown; do not replay"),
        );
        let store = self.retained.lock();
        let authority = self.require_workflow(&store, &auth, &record)?;
        self.require_dispatch_open(&store, &record.identity.workflow)?;
        if intent.expected_parent_epoch.unwrap_or(1) != authority.epoch {
            bail!("captured followup parent authority changed");
        }
        store.save_assignment(&record)?;
        let prompt = assignment_prompt(
            &contract,
            &record
                .root
                .join(format!("brief-v{}.json", record.result_version)),
            &record.handle,
            true,
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
        let recovery = read_accepted_recovery(&record, &selected)?;
        let (submission, issue) = match read_version_submission(&record, &selected) {
            Ok(submission) => (submission, None),
            Err(error) if recovery.is_some() => (None, Some(error.to_string())),
            Err(error) => return Err(error),
        };
        let mut snapshot = self.reconciliation_snapshot(record)?;
        snapshot.historical_submission = submission;
        snapshot.accepted_recovery = recovery;
        snapshot.submission_issue = issue;
        snapshot.selected_version = Some(version);
        snapshot.selected_acceptance_ref = selected.acceptance;
        snapshot.selected_recoveries = selected.recoveries;
        Ok(snapshot)
    }

    pub(super) fn record_acceptance(
        &self,
        auth: DelegationAuth,
        handle: String,
        identity: AssignmentIdentity,
        result_version: u32,
        target: AcceptanceTarget,
        expected_control_epoch: u64,
    ) -> Result<AssignmentSnapshot> {
        hold_before_effect("fallback-before-acceptance", &handle)?;
        let initial = self.owned_assignment(&auth, &handle)?;
        self.require_dependency_fresh(&initial)?;
        let exact_replay = initial.identity == identity
            && initial.result_version == result_version
            && !initial.control.attribution_invalidated
            && control::require_controller(&initial.control, expected_control_epoch).is_ok()
            && initial.acceptance.as_ref().is_some_and(|receipt| {
                receipt.identity == identity
                    && receipt.result_version == result_version
                    && receipt.target == target
            });
        // Existing receipt verification below still requires every current authority fence.
        if !exact_replay {
            self.require_review_acceptance(&auth, &initial)?;
        }
        #[cfg(feature = "acceptance-diagnostics")]
        hold_before_effect("review-final-auth", &handle)?;
        let parent = self.session_for_control(auth.session)?;
        let session = self.assignment_session(&initial)?;
        let store = self.retained.lock();
        let mut record = store
            .assignment(&handle)?
            .ok_or_else(|| lifecycle_error(ErrorCode::NotFound, "assignment disappeared"))?;
        record.upgrade_legacy();
        control::require_controller(&record.control, expected_control_epoch)?;
        if record.control.attribution_invalidated
            || record.identity != identity
            || record.result_version != result_version
            || record.identity != initial.identity
            || record.binding != initial.binding
            || record.review != initial.review
            || record.obligation_revision != initial.obligation_revision
            || (exact_replay && record.acceptance != initial.acceptance)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "acceptance must identify the exact current attributable result",
            ));
        }
        let authority = self.require_workflow(&store, &auth, &record)?;
        let parent_binding = authority.binding.clone();
        let child_binding = record
            .binding
            .clone()
            .ok_or_else(|| lifecycle_error(ErrorCode::Conflict, "child assignment not bound"))?;
        let contract = read_contract(&record)?;
        if matches!(&target, AcceptanceTarget::ParentRecovery { .. }) && contract.coding.is_some() {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "coding acceptance requires an authenticated child candidate; parent recovery cannot replace it",
            ));
        }
        let model = contract.model;
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
                |agent| {
                    if let Some(previous) = &record.acceptance {
                        if previous.target != target {
                            return Err(lifecycle_error(
                                ErrorCode::Conflict,
                                "this result already has a different immutable acceptance target",
                            ));
                        }
                        return verify_acceptance(&record, &record.version());
                    }
                    self.require_review_acceptance_at_commit(&store, &record)?;
                    match &target {
                        AcceptanceTarget::ChildSubmission { digest: expected } => {
                            if record.result_digest.as_ref() != Some(expected) {
                                return Err(lifecycle_error(
                                    ErrorCode::Conflict,
                                    "acceptance child digest is not current",
                                ));
                            }
                            read_version_submission(&record, &record.version())?.ok_or_else(
                                || {
                                    lifecycle_error(
                                        ErrorCode::Conflict,
                                        "no published child result to accept",
                                    )
                                },
                            )?;
                        }
                        AcceptanceTarget::ParentRecovery {
                            recovery_version,
                            digest: expected,
                        } => {
                            let recovery = fallback::read_recovery(
                                &record,
                                &record.version(),
                                *recovery_version,
                                expected,
                            )?;
                            let directory = Directory::open(&record.root, false)?;
                            let current = fallback::context(&record, &directory)?;
                            let evidence = self.control_evidence(&record, agent)?;
                            super::super::delegation_fallback::validate_capture(
                                &recovery.capture,
                                &current,
                                &authority,
                                &evidence,
                            )
                            .map_err(|error| {
                                lifecycle_error(ErrorCode::Conflict, error.to_string())
                            })?;
                            let contract = format!("contract-v{}.json", identity.contract);
                            let brief = format!("brief-v{result_version}.json");
                            let original = format!("submission-v{result_version}.json");
                            let checked = super::super::delegation_fallback::prepare_recovery(
                                &super::super::delegation_fallback::FallbackFiles {
                                    directory: &directory,
                                    contract_name: &contract,
                                    brief_name: &brief,
                                    original_name: &original,
                                },
                                &recovery.capture,
                                &current,
                                &authority,
                                &evidence,
                                &FallbackRecoveryDraft {
                                    capture_id: recovery.capture.id.clone(),
                                    version: *recovery_version,
                                    artifact: recovery.artifact.reference.clone(),
                                    original: recovery.capture.original.clone(),
                                    evidence: recovery.evidence.clone(),
                                },
                            )?;
                            if checked != recovery {
                                return Err(lifecycle_error(
                                    ErrorCode::Conflict,
                                    "recovery changed before explicit acceptance",
                                ));
                            }
                        }
                    }
                    let next_obligation_revision =
                        record.obligation_revision.checked_add(1).ok_or_else(|| {
                            lifecycle_error(ErrorCode::Conflict, "obligation revision exhausted")
                        })?;
                    if let Some(reference) = record.preserved_review_acceptance(&target)? {
                        record.acceptance = Some(reference);
                        record.obligation_revision = next_obligation_revision;
                        return store.save_assignment(&record);
                    }
                    let name = format!("acceptance-v{result_version}.json");
                    let bytes = serde_json::to_vec(&serde_json::json!({
                        "schema": 2, "identity": identity, "result_version": result_version,
                        "target": target, "author": authority,
                    }))?;
                    let reference = AcceptanceReference {
                        identity: identity.clone(),
                        result_version,
                        target: target.clone(),
                        path: record.root.join(&name),
                        digest: digest(&bytes),
                    };
                    // A published file followed by a failed save remains an uncommitted gap.
                    // A fresh authorized exact retry can commit it; conflicting bytes refuse.
                    publish_matching(&Directory::open(&record.root, false)?, &name, &bytes)?;
                    if take_fault("fallback-acceptance-before-save", &handle).is_some() {
                        bail!("acceptance file exposed but registry commit unconfirmed");
                    }
                    record.acceptance = Some(reference);
                    record.obligation_revision = next_obligation_revision;
                    store.save_assignment(&record)
                },
            )?
            .ok_or_else(|| {
                lifecycle_error(
                    ErrorCode::Conflict,
                    "acceptance current source, parent or child binding changed",
                )
            })?;
        drop(store);
        if let Some(session) = record.identity.session {
            self.delegation_observations.invalidate(session);
        }
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
    fallback_nonce: Option<&str>,
) -> Result<Vec<u8>> {
    let publication = if contract.coding.is_some() {
        "Publish exact identity/result_version and actual named check outcomes. The same workspace/base/permission envelope applies. The Server captures a new complete immutable authorized delta; prior candidates stay unchanged. Do not commit, push, merge, deploy, access credentials or modify task/parent records. Only the parent can accept the result."
    } else {
        "Call ovrcr_publish with the exact identity and result_version as submission.version. Only this child can publish; it cannot accept the result. Research tools remain read-only."
    };
    let publication = publication.replace(
        "ovrcr_publish",
        &child_tool(&contract.provider, "ovrcr_publish"),
    );
    let mut value = serde_json::json!({"schema":1,"handle":record.handle,"identity":identity,"contract":contract_document(contract)?,
        "request":request,"followup":text,"result_version":version,"output":record.root.join(format!("submission-v{version}.json")),"publication":publication,"questions":child_questions(&contract.provider)});
    if let Some(resource) = &record.coding {
        value["coding_workspace"] = serde_json::to_value(resource)?;
    }
    if let Some(nonce) = fallback_nonce {
        value["fallback_nonce"] = nonce.into();
        value["fallback"] = fallback::FALLBACK_INSTRUCTIONS.into();
    }
    Ok(serde_json::to_vec_pretty(&value)?)
}

pub(super) fn verify_acceptance(record: &Record, version: &VersionState) -> Result<()> {
    let Some(reference) = &version.acceptance else {
        return Ok(());
    };
    let name = format!("acceptance-v{}.json", version.result_version);
    if reference.identity != version.identity
        || reference.result_version != version.result_version
        || reference.path != record.root.join(&name)
    {
        bail!("acceptance reference does not match selected result");
    }
    let bytes = Directory::open(&record.root, false)?.read(&name)?;
    if digest(&bytes) != reference.digest {
        bail!("immutable acceptance publication changed");
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if serde_json::from_value::<AssignmentIdentity>(value["identity"].clone())? != version.identity
        || value["result_version"].as_u64() != Some(u64::from(version.result_version))
    {
        bail!("acceptance artifact identity changed");
    }
    match value["schema"].as_u64() {
        Some(1) => match &reference.target {
            AcceptanceTarget::ChildSubmission { digest }
                if value["result_digest"].as_str() == Some(digest.as_str()) => {}
            _ => bail!("legacy acceptance target changed"),
        },
        Some(2) => {
            if serde_json::from_value::<AcceptanceTarget>(value["target"].clone())?
                != reference.target
            {
                bail!("acceptance artifact target changed");
            }
            let author: WorkflowAuthority = serde_json::from_value(value["author"].clone())?;
            if author.instance != version.identity.instance
                || author.workflow != version.identity.workflow
                || author.controller != version.identity.controller
                || author.epoch == 0
                || author.server_lifetime.is_empty()
                || author.producer.is_empty()
            {
                bail!("acceptance historical author is incomplete");
            }
        }
        _ => bail!("unsupported acceptance artifact schema"),
    }
    match &reference.target {
        AcceptanceTarget::ChildSubmission { digest } => {
            if version.result_digest.as_ref() != Some(digest) {
                bail!("accepted child digest changed");
            }
            read_version_submission(record, version)?
                .context("accepted child submission is absent")?;
        }
        AcceptanceTarget::ParentRecovery {
            recovery_version,
            digest,
        } => {
            let recovery = fallback::read_recovery(record, version, *recovery_version, digest)?;
            let contract: ResearchContract = serde_json::from_slice(&recovery.contract.bytes)?;
            if contract.coding.is_some() {
                bail!("coding acceptance requires an authenticated selected child candidate");
            }
        }
    }
    Ok(())
}

pub(super) fn read_accepted_recovery(
    record: &Record,
    version: &VersionState,
) -> Result<Option<FallbackRecovery>> {
    verify_acceptance(record, version)?;
    match version
        .acceptance
        .as_ref()
        .map(|reference| &reference.target)
    {
        Some(AcceptanceTarget::ParentRecovery {
            recovery_version,
            digest,
        }) => fallback::read_recovery(record, version, *recovery_version, digest).map(Some),
        _ => Ok(None),
    }
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
    let contract: ResearchContract = serde_json::from_slice(
        &directory.read(&format!("contract-v{}.json", version.identity.contract))?,
    )?;
    let authenticated = PublicationIdentity {
        assignment: version.identity.clone(),
        kind: PublicationKind::Submission,
        version: version.result_version,
        digest: expected.clone(),
    };
    let selected = super::super::delegation_coding::SelectedCodingVersion {
        identity: &version.identity,
        result_version: version.result_version,
        contract: &contract,
        contract_digest: &version.contract_digest,
        publication: Some(&authenticated),
    };
    ServerState::verify_coding_candidate(record, &selected, &result)?;
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
            "captures",
            "recoveries",
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
        assert!(restored.captures.is_empty());
        assert!(restored.recoveries.is_empty());
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
            review: None,
            coding: None,
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
    #[test]
    fn new_default_coding_fields_preserve_previous_contract_brief_and_followup_digests() {
        let dir = tempfile::tempdir().unwrap();
        let record =
            super::super::tests::record(dir.path().join("assignment"), DispatchStatus::Applied);
        let contract = read_contract(&record).unwrap();
        let current = serde_json::to_vec(&contract).unwrap();
        let suffix = b",\"coding\":null}";
        assert!(current.ends_with(suffix));
        let mut old = current[..current.len() - suffix.len()].to_vec();
        old.push(b'}');
        assert_eq!(contract_bytes(&contract, Some(&digest(&old))).unwrap(), old);
        let mut changed = contract.clone();
        changed.scope.push_str(" different scope");
        assert_ne!(
            digest(&contract_bytes(&changed, Some(&digest(&old))).unwrap()),
            digest(&old)
        );
        let text = "same scoped followup";
        let mut old_payload = serde_json::to_vec(&(&record.identity, 1_u32, 1_u64, text)).unwrap();
        old_payload.pop();
        old_payload.push(b',');
        old_payload.extend_from_slice(&old);
        old_payload.push(b']');
        assert_eq!(
            followup_payload(
                &record.identity,
                1,
                1,
                text,
                &Some(contract.clone()),
                Some(&digest(&old_payload))
            )
            .unwrap(),
            old_payload
        );
        let brief = followup_brief(
            &record,
            &record.identity,
            2,
            &contract,
            "followup",
            text,
            None,
        )
        .unwrap();
        let mut previous: serde_json::Value = serde_json::from_slice(&brief).unwrap();
        previous["contract"]
            .as_object_mut()
            .unwrap()
            .remove("coding");
        let previous = serde_json::to_vec_pretty(&previous).unwrap();
        assert_eq!(
            matching_legacy_brief(brief, &contract, &digest(&previous)).unwrap(),
            previous
        );
    }
}

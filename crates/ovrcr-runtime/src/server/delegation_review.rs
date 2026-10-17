//! Parent review policy over existing assignments and scoped same-child input.
//! This module never launches, waits for, archives, commits or publishes code.
use super::*;

const MAX_REVIEW_CANDIDATES: usize = 33;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReviewRecord {
    declarations: Vec<DeclarationReference>,
    rounds: Vec<ReviewRound>,
    limit: RepairLimit,
    repairs: Vec<RepairIntent>,
    stopped: Option<ReviewStop>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct DeclarationReference {
    request: String,
    digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ReviewRound {
    declaration: String,
    target: CodingTargetReference,
    requirements: Vec<ReviewRequirement>,
    seats: Vec<ReviewSeat>,
    evidence: Vec<ReviewReference>,
    outcomes: Vec<(ReviewAxis, ReviewOutcome)>,
    superseded_acceptance: Option<AcceptanceReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ReviewSeat {
    axis: ReviewAxis,
    handle: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RepairIntent {
    request: String,
    target: CodingTargetReference,
    control_epoch: u64,
    text_digest: String,
    blockers_digest: String,
}

impl ReviewRecord {
    pub(super) fn status(&self, record: &Record) -> Option<ReviewStatus> {
        let current = self.rounds.last()?;
        Some(ReviewStatus {
            target: current.target.clone(),
            requirements: current.requirements.clone(),
            evidence: current.evidence.clone(),
            limit: self.limit.clone(),
            consumed_rounds: self.repairs.len() as u32,
            pending_repair: self
                .repairs
                .last()
                .filter(|repair| {
                    record
                        .request_receipt(&repair.request, DispatchOperation::Followup)
                        .is_none_or(|receipt| {
                            matches!(
                                receipt.outcome,
                                DispatchOutcome::Accepted
                                    | DispatchOutcome::Unknown
                                    | DispatchOutcome::Partial
                            )
                        })
                })
                .map(|repair| repair.request.clone()),
            stopped: self.stopped.clone(),
            independent_review_satisfied: self.stopped.is_none()
                && record.identity == current.target.assignment
                && record.result_version == current.target.publication.version
                && record.result_digest.as_ref() == Some(&current.target.publication.digest)
                && record.contract_digest == current.target.source_contract_digest
                && current.requirements.iter().all(|requirement| {
                    current.evidence.iter().any(|evidence| {
                        evidence.axis == requirement.axis
                            && evidence.target.coding == current.target
                            && evidence.target.sources == requirement.sources
                    }) && current.outcomes.iter().any(|(axis, outcome)| {
                        *axis == requirement.axis && *outcome == ReviewOutcome::Passed
                    })
                }),
        })
    }
}

impl Record {
    /// Re-register only the original exact decision after fresh current gates; never rewrite it.
    pub(super) fn preserved_review_acceptance(
        &self,
        target: &AcceptanceTarget,
    ) -> Result<Option<AcceptanceReference>> {
        let reference = self.review.as_ref().and_then(|state| {
            state
                .rounds
                .iter()
                .rev()
                .filter(|round| {
                    round.target.assignment == self.identity
                        && round.target.publication.version == self.result_version
                })
                .find_map(|round| round.superseded_acceptance.clone())
        });
        if let Some(reference) = &reference {
            if &reference.target != target {
                return Err(review_error(
                    "preserved immutable acceptance has a different target",
                ));
            }
            let mut version = self.version();
            version.acceptance = Some(reference.clone());
            followups::verify_acceptance(self, &version)?;
        }
        Ok(reference)
    }
    /// Fail closed on damaged retained policy without moving private task payloads into SQLite.
    pub(super) fn validate_review_storage(&self) -> Result<()> {
        let Some(state) = &self.review else {
            return Ok(());
        };
        state.limit.validate().map_err(anyhow::Error::msg)?;
        if state.rounds.is_empty()
            || state.rounds.len() > MAX_REVIEW_CANDIDATES
            || state.declarations.len() != state.rounds.len()
            || state.repairs.len() > state.limit.rounds as usize
        {
            bail!("invalid bounded independent review history");
        }
        for (index, round) in state.rounds.iter().enumerate() {
            let declared = declaration(self, &round.declaration)?;
            if state.rounds[..index]
                .iter()
                .any(|old| old.declaration == round.declaration)
                || declared.target != round.target
                || declared.requirements != round.requirements
                || match &declared.repair_limit {
                    Some(limit) => *limit != state.limit,
                    None if index == 0 => RepairLimit::default() != state.limit,
                    None => false,
                }
                || round.seats.len() > 2
                || round.evidence.len() > 2
                || round.outcomes.len() != round.evidence.len()
            {
                bail!("independent review metadata differs from its immutable declaration");
            }
            if let Some(acceptance) = &round.superseded_acceptance {
                let expected = self.root.join(format!(
                    "acceptance-v{}.json",
                    round.target.publication.version
                ));
                if acceptance.identity != round.target.assignment
                    || acceptance.result_version != round.target.publication.version
                    || acceptance.target
                        != (AcceptanceTarget::ChildSubmission {
                            digest: round.target.publication.digest.clone(),
                        })
                    || acceptance.path != expected
                    || digest(
                        &Directory::open(&self.root, false)?
                            .read(&format!("acceptance-v{}.json", acceptance.result_version))?,
                    ) != acceptance.digest
                {
                    bail!("preserved immutable acceptance reference changed");
                }
            }
            for source in &declared.sources {
                if digest(source.content.as_bytes()) != source.source.digest {
                    bail!("immutable independent review source bytes changed");
                }
            }
            for (seat_index, seat) in round.seats.iter().enumerate() {
                if !followups::valid_request(&seat.handle)
                    || !round
                        .requirements
                        .iter()
                        .any(|required| required.axis == seat.axis)
                    || round.seats[..seat_index]
                        .iter()
                        .any(|old| old.axis == seat.axis || old.handle == seat.handle)
                {
                    bail!("invalid independent reviewer seat");
                }
            }
            for (evidence_index, evidence) in round.evidence.iter().enumerate() {
                evidence
                    .target
                    .validate(&evidence.axis)
                    .map_err(anyhow::Error::msg)?;
                if evidence.target.coding != round.target
                    || evidence.reviewer != evidence.publication.assignment
                    || evidence.publication.kind != PublicationKind::Submission
                    || evidence.publication.version == 0
                    || !round.requirements.iter().any(|required| {
                        required.axis == evidence.axis
                            && required.sources == evidence.target.sources
                    })
                    || !round.seats.iter().any(|seat| {
                        seat.axis == evidence.axis && seat.handle == evidence.reviewer.request
                    })
                    || !round
                        .outcomes
                        .iter()
                        .any(|(axis, _)| *axis == evidence.axis)
                    || round.evidence[..evidence_index]
                        .iter()
                        .any(|old| old.axis == evidence.axis)
                {
                    bail!("invalid independent review evidence metadata");
                }
            }
        }
        for (index, repair) in state.repairs.iter().enumerate() {
            if !followups::valid_request(&repair.request)
                || repair.control_epoch == 0
                || !state
                    .rounds
                    .iter()
                    .any(|round| round.target == repair.target)
                || state.repairs[..index]
                    .iter()
                    .any(|old| old.request == repair.request)
            {
                bail!("invalid original corrective round identity");
            }
            let bytes = Directory::open(&self.root, false)?
                .read(&format!("repair-{}.txt", repair.request))?;
            if digest(&bytes) != repair.text_digest {
                bail!("immutable corrective brief changed");
            }
        }
        Ok(())
    }
}

fn review_error(message: impl Into<String>) -> anyhow::Error {
    lifecycle_error(ErrorCode::Conflict, message)
}

fn current_round(record: &Record) -> Result<&ReviewRound> {
    record
        .review
        .as_ref()
        .and_then(|review| review.rounds.last())
        .ok_or_else(|| review_error("independent review has not been declared"))
}

fn declaration(record: &Record, request: &str) -> Result<ReviewDeclaration> {
    let state = record
        .review
        .as_ref()
        .context("review declaration missing")?;
    let reference = state
        .declarations
        .iter()
        .find(|value| value.request == request)
        .context("original review declaration missing")?;
    let bytes = Directory::open(&record.root, false)?.read(&format!("review-{request}.json"))?;
    if digest(&bytes) != reference.digest {
        bail!("immutable review declaration changed");
    }
    let declaration: ReviewDeclaration = serde_json::from_slice(&bytes)?;
    declaration.validate().map_err(anyhow::Error::msg)?;
    if declaration.request != request {
        bail!("foreign review declaration");
    }
    Ok(declaration)
}

fn required_check_outcomes(
    record: &Record,
    coding: &CodingSubmission,
) -> Result<Vec<CheckOutcome>> {
    let contract = read_contract(record)?;
    let required = contract.coding.context("coding check contract missing")?;
    Ok(required
        .required_checks
        .iter()
        .map(|name| {
            coding
                .checks
                .iter()
                .find(|check| check.name == *name)
                .map(|check| check.outcome.clone())
                .unwrap_or(CheckOutcome::Missing)
        })
        .collect())
}

/// Fixed, axis-specific context. Sources are in contract.review.source_documents.
/// The candidate file contains all tracked/untracked delta bytes, modes and deletions.
pub(super) fn review_scope(assignment: &ReviewAssignment) -> String {
    let axis = match assignment.axis {
        ReviewAxis::Standards => {
            "Apply the supplied repository standards and installed method's Fowler smell baseline. Repository rules override that baseline; smells are judgment calls. Skip tooling-enforced rules."
        }
        ReviewAxis::Spec => {
            "Use only the supplied originating specification/direct task brief to check missing, incorrect or unrequested behavior."
        }
    };
    format!(
        "Independently review the {:?} axis only. Read the exact documents in contract.review.source_documents and verify their SHA256 digests. Read the complete immutable coding publication at {} (SHA256 {}). Compare its complete snapshot, including untracked files, deletions, modes and safe symlinks, against pinned Git base {}. Read unchanged base context from the snapshot repository with git show <base>:<path>; refuse missing or corrupt base objects or candidate bytes. Never substitute HEAD or the current implementation checkout. {} Publish a typed review report for the exact contract.review target and axis. Keep blocking findings identifiable across correction rounds. Do not implement fixes, reuse the implementation conversation, consult the other axis, or launch another outer review team.",
        assignment.axis,
        assignment.target.coding.candidate.path.display(),
        assignment.target.coding.candidate.digest,
        assignment.target.coding.candidate.base,
        axis
    )
}

fn coding_target_from_result(record: &Record) -> Result<CodingTargetReference> {
    let version = record.version();
    let submission = read_version_submission(record, &version)?.context("coding result missing")?;
    let candidate = submission
        .coding
        .as_ref()
        .and_then(|coding| coding.candidate.clone())
        .context("complete immutable coding candidate missing")?;
    let publication = version
        .publication_intent
        .as_ref()
        .map(|intent| intent.identity.clone())
        .context("authenticated coding publication missing")?;
    if version.result_digest.as_ref() != Some(&publication.digest)
        || publication.assignment != version.identity
        || publication.version != version.result_version
        || publication.kind != PublicationKind::Submission
    {
        bail!("coding target publication differs from original result");
    }
    Ok(CodingTargetReference {
        assignment: version.identity,
        publication,
        candidate,
        source_contract_digest: version.contract_digest,
    })
}

fn require_passed_worker_checks(record: &Record) -> Result<()> {
    let submission =
        read_version_submission(record, &record.version())?.context("worker result missing")?;
    let coding = submission
        .coding
        .as_ref()
        .context("coding check evidence missing")?;
    if required_check_outcomes(record, coding)?
        .iter()
        .any(|outcome| *outcome != CheckOutcome::Passed)
    {
        return Err(review_error(
            "named required worker checks remain failed, missing or unrun; independent review cannot replace them",
        ));
    }
    Ok(())
}

/// File-only authentication, also safe inside the final held-store/Session boundary.
fn review_publication(
    record: &Record,
    contract: &ResearchContract,
    axis: &ReviewAxis,
) -> Result<(ReviewReference, ReviewOutcome)> {
    let assignment = contract
        .review
        .as_ref()
        .context("typed review contract missing")?;
    let version = record.version();
    let submission = read_version_submission(&record, &version)?
        .context("independent review publication missing")?;
    let report = submission
        .review
        .as_ref()
        .context("typed independent review report missing")?;
    report.validate().map_err(anyhow::Error::msg)?;
    if report.axis != *axis || report.target != assignment.target {
        return Err(review_error(
            "review publication does not match its original axis and exact target/source contract",
        ));
    }
    let publication = version
        .publication_intent
        .as_ref()
        .context("review publication authentication missing")?
        .identity
        .clone();
    if version.result_digest.as_ref() != Some(&publication.digest)
        || publication.assignment != record.identity
        || publication.version != version.result_version
        || publication.kind != PublicationKind::Submission
    {
        return Err(review_error(
            "independent review publication identity changed",
        ));
    }
    Ok((
        ReviewReference {
            axis: axis.clone(),
            target: report.target.clone(),
            reviewer: record.identity.clone(),
            publication,
        },
        report.outcome.clone(),
    ))
}

impl ServerState {
    /// Uses the authenticated immutable result owner, never the mutable checkout.
    pub(super) fn review_coding_target(&self, record: &Record) -> Result<CodingTargetReference> {
        let target = coding_target_from_result(record)?;
        let resource = record
            .coding
            .as_ref()
            .context("original coding workspace provenance missing")?;
        if crate::git::resolve_commit(&resource.repository, &target.candidate.base)?
            != target.candidate.base
        {
            return Err(review_error(
                "exact immutable review base context is unavailable",
            ));
        }
        Ok(target)
    }

    fn require_review_target(&self, record: &Record, target: &CodingTargetReference) -> Result<()> {
        if self
            .review_coding_target(record)
            .map_err(|error| review_error(error.to_string()))?
            != *target
        {
            return Err(review_error(
                "review target is stale or belongs to another candidate/source contract",
            ));
        }
        Ok(())
    }

    /// Hooked into ordinary Prepare/Launch. No independent scheduler or child owner.
    pub(super) fn validate_review_assignment(
        &self,
        auth: &DelegationAuth,
        contract: &ResearchContract,
    ) -> Result<()> {
        let Some(assignment) = &contract.review else {
            return Ok(());
        };
        require_typed_review_provider(&contract.provider)
            .map_err(|reason| lifecycle_error(ErrorCode::InvalidRequest, reason))?;
        self.validate_review_contract(contract)?;
        if contract.coding.is_some() || contract.permitted_actions != ["read"] {
            return Err(review_error(
                "independent review requires a fresh read-only assignment and exact separate axis context",
            ));
        }
        let implementation = self.owned_assignment(auth, &assignment.implementation)?;
        implementation.validate_review_storage()?;
        self.require_review_target(&implementation, &assignment.target.coding)?;
        if implementation.identity.request == contract.request
            || implementation.identity.workflow != contract.workflow
        {
            return Err(review_error(
                "implementation cannot review itself or borrow a foreign workflow",
            ));
        }
        // Accepted direct review contracts have no coordinator ledger dependency.
        // A present policy is always validated and governed; errors never fall back.
        if implementation.review.is_none() {
            return Ok(());
        }
        if contract.scope != review_scope(assignment) {
            return Err(review_error(
                "independent review requires the exact separate axis context",
            ));
        }
        if implementation
            .review
            .as_ref()
            .is_some_and(|state| state.stopped.is_some())
        {
            return Err(review_error(
                "review policy is stopped; no new reviewer input is authorized",
            ));
        }
        let round = current_round(&implementation)?;
        if !round.requirements.iter().any(|requirement| {
            requirement.axis == assignment.axis && requirement.sources == assignment.target.sources
        }) {
            return Err(review_error(
                "review axis/source contract was not declared by the parent",
            ));
        }
        let supplied = declaration(&implementation, &round.declaration)?;
        let expected_documents: Vec<_> = assignment
            .target
            .sources
            .iter()
            .map(|reference| {
                supplied
                    .sources
                    .iter()
                    .find(|source| source.source == *reference)
                    .cloned()
                    .context("actual review source document missing")
            })
            .collect::<Result<_>>()?;
        if assignment.source_documents.len() != expected_documents.len()
            || expected_documents
                .iter()
                .any(|expected| !assignment.source_documents.contains(expected))
        {
            return Err(review_error(
                "review source documents differ from the immutable declared context",
            ));
        }
        for document in &assignment.source_documents {
            if digest(document.content.as_bytes()) != document.source.digest {
                return Err(review_error("review source content digest changed"));
            }
        }
        let retry = self.verify_review_retry(auth, contract)?;
        for seat in &round.seats {
            if seat.axis == assignment.axis
                && seat.handle != contract.request
                && retry.as_ref() != Some(&seat.handle)
            {
                return Err(review_error(
                    "this candidate/axis already has an original reviewer assignment; reconcile it",
                ));
            }
            if seat.axis != assignment.axis && seat.handle == contract.request {
                return Err(review_error(
                    "Standards and Spec require distinct fresh reviewer contexts",
                ));
            }
        }
        Ok(())
    }

    /// A refused zero-effect launch has no native context to contaminate a retry.
    fn verify_review_retry(
        &self,
        auth: &DelegationAuth,
        contract: &ResearchContract,
    ) -> Result<Option<String>> {
        let original = contract.previous_request.clone();
        let mut previous = original.clone();
        let mut count = 0;
        while let Some(handle) = previous {
            count += 1;
            if count > 64 {
                return Err(review_error("review retry lineage exceeds its bound"));
            }
            let record = self.owned_assignment(auth, &handle)?;
            let prior = read_contract(&record)?;
            if record.dispatch != DispatchStatus::Refused
                || !record.retryable_capacity_refusal
                || record.identity.session.is_some()
                || record.identity.run.is_some()
                || record.binding.is_some()
                || record.execution_attempts != Some(0)
                || prior.review != contract.review
                || prior.scope != contract.scope
                || prior.workflow != contract.workflow
            {
                return Err(review_error(
                    "only a proven zero-effect capacity refusal preserves a fresh independent reviewer context",
                ));
            }
            previous = prior.previous_request;
        }
        Ok(original)
    }

    pub(super) fn validate_review_submission(
        &self,
        record: &Record,
        submission: &ResearchSubmission,
    ) -> Result<()> {
        match (read_contract(record)?.review, &submission.review) {
            (None, None) => Ok(()),
            (Some(assignment), Some(report)) => {
                report.validate().map_err(anyhow::Error::msg)?;
                if report.axis != assignment.axis || report.target != assignment.target {
                    return Err(review_error(
                        "review publication must match its original exact axis and target/source contract",
                    ));
                }
                Ok(())
            }
            (None, Some(_)) => Err(review_error(
                "worker/native-helper evidence is not a declared independent review assignment",
            )),
            (Some(_), None) => Err(review_error(
                "independent reviewer must publish its typed exact-axis report",
            )),
        }
    }

    /// Called after common Prepare saves its original contract, and again on recovery.
    pub(super) fn reserve_review_seat(
        &self,
        auth: &DelegationAuth,
        contract: &ResearchContract,
    ) -> Result<()> {
        let Some(assignment) = &contract.review else {
            return Ok(());
        };
        self.validate_review_assignment(auth, contract)?;
        let mut implementation = self.owned_assignment(auth, &assignment.implementation)?;
        if implementation.review.is_none() {
            return Ok(());
        }
        let round = implementation
            .review
            .as_mut()
            .context("review policy missing")?
            .rounds
            .last_mut()
            .context("review target missing")?;
        if !round
            .seats
            .iter()
            .any(|seat| seat.axis == assignment.axis && seat.handle == contract.request)
        {
            round.seats.retain(|seat| seat.axis != assignment.axis);
            round.seats.push(ReviewSeat {
                axis: assignment.axis.clone(),
                handle: contract.request.clone(),
            });
            let store = self.retained.lock();
            self.require_workflow(&store, auth, &implementation)?;
            store.save_assignment(&implementation)?;
        }
        Ok(())
    }

    /// The dispatcher holds the existing mutation lock. Source bytes stay private.
    pub(super) fn coordinate_review(
        &self,
        auth: DelegationAuth,
        handle: String,
        operation: ReviewOperation,
    ) -> Result<AssignmentSnapshot> {
        let mut record = self.owned_assignment(&auth, &handle)?;
        record.validate_review_storage()?;
        match operation {
            ReviewOperation::Declare(declared) => {
                declared.validate().map_err(anyhow::Error::msg)?;
                self.require_review_target(&record, &declared.target)?;
                for source in &declared.sources {
                    if digest(source.content.as_bytes()) != source.source.digest {
                        return Err(review_error("actual review source digest mismatch"));
                    }
                }
                let bytes = serde_json::to_vec(&declared)?;
                let reference = DeclarationReference {
                    request: declared.request.clone(),
                    digest: digest(&bytes),
                };
                let limit = declared
                    .repair_limit
                    .clone()
                    .or_else(|| record.review.as_ref().map(|review| review.limit.clone()))
                    .unwrap_or_default();
                let review = record.review.get_or_insert_with(|| ReviewRecord {
                    declarations: Vec::new(),
                    rounds: Vec::new(),
                    limit: limit.clone(),
                    repairs: Vec::new(),
                    stopped: None,
                });
                if let Some(old) = review
                    .declarations
                    .iter()
                    .find(|value| value.request == declared.request)
                {
                    if old.digest != reference.digest {
                        return Err(review_error(
                            "review request already belongs to a different declaration",
                        ));
                    }
                    declaration(&record, &declared.request)?;
                    return self.reconciliation_snapshot(record);
                }
                if review.limit != limit {
                    return Err(review_error(
                        "repair limits cannot be reset by a new candidate declaration",
                    ));
                }
                if review.rounds.len() >= MAX_REVIEW_CANDIDATES {
                    return Err(review_error(
                        "bounded review history is full; retained evidence remains available",
                    ));
                }
                if review.rounds.last().is_some_and(|round| {
                    round.target == declared.target && round.requirements == declared.requirements
                }) {
                    return Err(review_error(
                        "unchanged review target already has a declaration; reconcile its original request",
                    ));
                }
                // A stopped policy needs a genuine separate parent decision, never an automatic reset.
                if review.stopped.is_some() {
                    return Err(review_error(
                        "review/repair policy is stopped; a new declaration cannot invent approval",
                    ));
                }
                // Keep only evidence whose entire axis target/source contract is unchanged.
                let mut seats = Vec::new();
                let mut evidence = Vec::new();
                let mut outcomes = Vec::new();
                if let Some(previous) = review
                    .rounds
                    .last()
                    .filter(|round| round.target == declared.target)
                {
                    for requirement in &declared.requirements {
                        if previous.requirements.contains(requirement) {
                            seats.extend(
                                previous
                                    .seats
                                    .iter()
                                    .filter(|seat| seat.axis == requirement.axis)
                                    .cloned(),
                            );
                            evidence.extend(
                                previous
                                    .evidence
                                    .iter()
                                    .filter(|value| value.axis == requirement.axis)
                                    .cloned(),
                            );
                            outcomes.extend(
                                previous
                                    .outcomes
                                    .iter()
                                    .filter(|(axis, _)| *axis == requirement.axis)
                                    .cloned(),
                            );
                        }
                    }
                }
                let superseded_acceptance = record.acceptance.take();
                record.obligation_revision = record
                    .obligation_revision
                    .checked_add(1)
                    .context("review obligation revision exhausted")?;
                review.declarations.push(reference);
                review.rounds.push(ReviewRound {
                    declaration: declared.request.clone(),
                    target: declared.target.clone(),
                    requirements: declared.requirements.clone(),
                    seats,
                    evidence,
                    outcomes,
                    superseded_acceptance,
                });
                let store = self.retained.lock();
                self.require_workflow(&store, &auth, &record)?;
                let directory = Directory::open(&record.root, false)?;
                publish_matching(
                    &directory,
                    &format!("review-{}.json", declared.request),
                    &bytes,
                )?;
                store.save_assignment(&record)?;
            }
            ReviewOperation::Assignment { axis } => {
                if record
                    .review
                    .as_ref()
                    .is_some_and(|state| state.stopped.is_some())
                {
                    return Err(review_error(
                        "review policy is stopped; no new reviewer assignment is authorized",
                    ));
                }
                let round = current_round(&record)?;
                self.require_review_target(&record, &round.target)?;
                let requirement = round
                    .requirements
                    .iter()
                    .find(|requirement| requirement.axis == axis)
                    .ok_or_else(|| review_error("requested axis is not required"))?;
                let supplied = declaration(&record, &round.declaration)?;
                let assignment = ReviewAssignment {
                    implementation: handle,
                    axis,
                    target: ReviewTarget {
                        coding: round.target.clone(),
                        sources: requirement.sources.clone(),
                    },
                    source_documents: requirement
                        .sources
                        .iter()
                        .map(|reference| {
                            supplied
                                .sources
                                .iter()
                                .find(|source| source.source == *reference)
                                .cloned()
                                .context("review source missing")
                        })
                        .collect::<Result<_>>()?,
                };
                let mut snapshot = self.reconciliation_snapshot(record)?;
                snapshot.review_scope = Some(review_scope(&assignment));
                snapshot.review_assignment = Some(assignment);
                return Ok(snapshot);
            }
            ReviewOperation::Collect { axis, reviewer } => {
                let (evidence, outcome) = self
                    .authenticated_review(&auth, &record, &axis, &reviewer)
                    .map_err(|error| review_error(error.to_string()))?;
                let review = record.review.as_mut().context("review policy missing")?;
                let round = review.rounds.last_mut().context("review target missing")?;
                if let Some(old) = round.evidence.iter().find(|old| old.axis == axis) {
                    if old != &evidence {
                        return Err(review_error(
                            "review evidence already recorded for this exact axis",
                        ));
                    }
                } else {
                    round.outcomes.push((axis, outcome));
                    round.evidence.push(evidence);
                    let store = self.retained.lock();
                    self.require_workflow(&store, &auth, &record)?;
                    store.save_assignment(&record)?;
                }
            }
            ReviewOperation::Stop { reason } => {
                let state = record.review.as_mut().context("review policy missing")?;
                if state.stopped.as_ref() == Some(&reason) {
                    return self.reconciliation_snapshot(record);
                }
                if let Some(round) = state.rounds.last_mut() {
                    if let Some(acceptance) = record.acceptance.take() {
                        round.superseded_acceptance = Some(acceptance);
                    }
                }
                record.obligation_revision = record
                    .obligation_revision
                    .checked_add(1)
                    .context("review obligation revision exhausted")?;
                record
                    .review
                    .as_mut()
                    .context("review policy missing")?
                    .stopped = Some(reason);
                let store = self.retained.lock();
                self.require_workflow(&store, &auth, &record)?;
                store.save_assignment(&record)?;
            }
            ReviewOperation::Repair {
                request,
                expected_control_epoch,
            } => {
                return self.repair_review(auth, record, request, expected_control_epoch);
            }
        }
        self.reconciliation_snapshot(record)
    }

    fn authenticated_review(
        &self,
        auth: &DelegationAuth,
        implementation: &Record,
        axis: &ReviewAxis,
        reviewer: &str,
    ) -> Result<(ReviewReference, ReviewOutcome)> {
        let round = current_round(implementation)?;
        self.require_review_target(implementation, &round.target)?;
        let seat = round
            .seats
            .iter()
            .find(|seat| &seat.axis == axis && seat.handle == reviewer)
            .context("review must come from the original parent-assigned independent seat")?;
        let record = self.owned_assignment(auth, &seat.handle)?;
        let contract = read_contract(&record)?;
        let assignment = contract
            .review
            .as_ref()
            .context("worker/self-review is not an independent review assignment")?;
        self.validate_review_assignment(auth, &contract)?;
        if &assignment.axis != axis
            || assignment.target.coding != round.target
            || record.identity.session == implementation.identity.session
            || record.control.attribution_invalidated
            || record.unresolved_input()
        {
            return Err(review_error(
                "stale, foreign, unsafe or self-review evidence",
            ));
        }
        let binding = record
            .binding
            .as_ref()
            .context("reviewer conversation missing")?;
        let worker_binding = implementation
            .binding
            .as_ref()
            .context("implementation conversation missing")?;
        if binding.invocation == worker_binding.invocation
            || binding.conversation == worker_binding.conversation
        {
            return Err(review_error(
                "reviewer must have an independent fresh native context",
            ));
        }
        for other in round.seats.iter().filter(|other| other.axis != *axis) {
            if let Some(other) = self.retained.lock().assignment(&other.handle)?
                && let Some(other_binding) = other.binding
                && (binding.invocation == other_binding.invocation
                    || binding.conversation == other_binding.conversation)
            {
                return Err(review_error("Standards and Spec share a native context"));
            }
        }
        review_publication(&record, &contract, axis)
    }

    /// Re-authenticate the publication on every use; a stored reference is no proof.
    fn current_review_reports(
        &self,
        auth: &DelegationAuth,
        record: &Record,
    ) -> Result<Vec<ReviewReport>> {
        let round = current_round(record)?;
        self.require_review_target(record, &round.target)?;
        let mut reports = Vec::new();
        for requirement in &round.requirements {
            let reference = round
                .evidence
                .iter()
                .find(|evidence| evidence.axis == requirement.axis)
                .context("required independent review evidence missing")?;
            let (actual, _) = self.authenticated_review(
                auth,
                record,
                &requirement.axis,
                &reference.reviewer.request,
            )?;
            if actual != *reference {
                bail!("recorded independent review reference changed");
            }
            let reviewer = self.owned_assignment(auth, &actual.reviewer.request)?;
            reports.push(
                read_version_submission(&reviewer, &reviewer.version())?
                    .and_then(|submission| submission.review)
                    .context("review report missing")?,
            );
        }
        Ok(reports)
    }

    /// Caller owns the current store and final parent/worker Session context.
    /// Never call ownership helpers here: they would reenter those held locks.
    pub(super) fn require_review_acceptance_at_commit(
        &self,
        store: &crate::retained::SessionStore,
        record: &Record,
    ) -> Result<()> {
        if record.review.is_none() {
            return Ok(());
        }
        record.validate_review_storage()?;
        let round = current_round(record)?;
        // Preauthentication checked the Git base; final checks read immutable files only.
        if coding_target_from_result(record)? != round.target {
            return Err(review_error("coding target changed at final acceptance"));
        }
        for requirement in &round.requirements {
            let reference = round
                .evidence
                .iter()
                .find(|reference| reference.axis == requirement.axis)
                .context("current required review reference missing")?;
            let mut reviewer = store
                .assignment(&reference.reviewer.request)?
                .context("current independent reviewer missing")?;
            reviewer.upgrade_legacy();
            if reviewer.identity != reference.reviewer
                || reviewer.control.attribution_invalidated
                || reviewer.unresolved_input()
            {
                return Err(review_error(
                    "independent reviewer changed at final acceptance",
                ));
            }
            let contract = read_contract(&reviewer)?;
            let assignment = contract
                .review
                .as_ref()
                .context("independent review contract missing")?;
            if assignment.implementation != record.handle
                || assignment.target.coding != round.target
                || assignment.target.sources != requirement.sources
                || assignment.axis != requirement.axis
            {
                return Err(review_error(
                    "independent review target changed at final acceptance",
                ));
            }
            for document in &assignment.source_documents {
                if digest(document.content.as_bytes()) != document.source.digest {
                    return Err(review_error(
                        "independent review source changed at final acceptance",
                    ));
                }
            }
            let (actual, outcome) = review_publication(&reviewer, &contract, &requirement.axis)?;
            if actual != *reference || outcome != ReviewOutcome::Passed {
                return Err(review_error(
                    "independent report changed at final acceptance",
                ));
            }
        }
        require_passed_worker_checks(record)
    }

    pub(super) fn require_review_acceptance(
        &self,
        auth: &DelegationAuth,
        record: &Record,
    ) -> Result<()> {
        if record.review.is_none() {
            return Ok(());
        }
        record.validate_review_storage()?;
        if record
            .review
            .as_ref()
            .is_some_and(|review| review.stopped.is_some())
        {
            return Err(review_error(
                "review policy stopped; acceptance requirements remain unresolved",
            ));
        }
        let reports = self
            .current_review_reports(auth, record)
            .map_err(|error| review_error(error.to_string()))?;
        if reports
            .iter()
            .any(|report| report.outcome != ReviewOutcome::Passed)
        {
            return Err(review_error(
                "required independent review has blocking or unresolved findings",
            ));
        }
        require_passed_worker_checks(record)
    }

    fn repair_review(
        &self,
        auth: DelegationAuth,
        mut record: Record,
        request: String,
        expected_control_epoch: u64,
    ) -> Result<AssignmentSnapshot> {
        if !followups::valid_request(&request) {
            return Err(review_error("invalid original repair request"));
        }
        let saved = record
            .review
            .as_ref()
            .context("review policy missing")?
            .repairs
            .iter()
            .find(|repair| repair.request == request)
            .cloned();
        let intent = if let Some(saved) = saved {
            if record
                .review
                .as_ref()
                .is_some_and(|state| state.stopped.is_some())
                && record
                    .request_receipt(&request, DispatchOperation::Followup)
                    .is_none_or(|receipt| receipt.outcome == DispatchOutcome::Accepted)
            {
                return Err(review_error(
                    "review policy stopped before original corrective input; inspect its retained intent without resuming",
                ));
            }
            if saved.control_epoch != expected_control_epoch {
                return Err(review_error(
                    "repair recovery must retain its original control epoch",
                ));
            }
            saved
        } else {
            if record.delivery_request_used(&request)
                || record.review.as_ref().is_some_and(|state| {
                    state
                        .declarations
                        .iter()
                        .any(|declaration| declaration.request == request)
                })
            {
                return Err(review_error(
                    "repair request already identifies another original operation",
                ));
            }
            control::require_controller(&record.control, expected_control_epoch)?;
            self.assignment_session(&record)
                .map_err(|error| review_error(error.to_string()))?;
            if record.unresolved_input() || record.control.attribution_invalidated {
                return Err(review_error(
                    "unsafe child attribution or unresolved input; no corrective input sent",
                ));
            }
            let reports = self
                .current_review_reports(&auth, &record)
                .map_err(|error| review_error(error.to_string()))?;
            let submission = read_version_submission(&record, &record.version())?
                .context("worker result missing")?;
            let checks = submission
                .coding
                .as_ref()
                .context("coding check evidence missing")?;
            let required_checks = required_check_outcomes(&record, checks)?;
            let state = record.review.as_ref().context("review policy missing")?;
            if state.stopped.is_some() {
                return Err(review_error("repair policy is stopped"));
            }
            let mut blockers: Vec<_> = reports
                .iter()
                .flat_map(|report| {
                    report
                        .findings
                        .iter()
                        .filter(|finding| finding.blocking)
                        .map(|finding| (&report.axis, finding))
                })
                .collect();
            blockers.sort_by_key(|(axis, finding)| (format!("{axis:?}"), finding.id.clone()));
            let blockers_digest = digest(&serde_json::to_vec(&blockers)?);
            let stop = if reports
                .iter()
                .any(|report| report.outcome == ReviewOutcome::Blocked)
            {
                Some(ReviewStop::MissingDecision)
            } else if required_checks
                .iter()
                .any(|outcome| matches!(outcome, CheckOutcome::Missing | CheckOutcome::Unrun))
            {
                Some(ReviewStop::MissingChecks)
            } else if state.repairs.len() >= state.limit.rounds as usize {
                Some(ReviewStop::LimitExhausted)
            } else if state
                .repairs
                .last()
                .is_some_and(|repair| repair.blockers_digest == blockers_digest)
            {
                Some(ReviewStop::NoProgress)
            } else {
                None
            };
            if let Some(stop) = stop {
                record.review.as_mut().unwrap().stopped = Some(stop);
                let store = self.retained.lock();
                self.require_workflow(&store, &auth, &record)?;
                store.save_assignment(&record)?;
                drop(store);
                return self.reconciliation_snapshot(record);
            }
            if blockers.is_empty() {
                return Err(review_error(
                    "no independent blocking findings require a corrective round",
                ));
            }
            let text = format!(
                "Correct these exact independent findings in this same assigned conversation and original authorized workspace/base. Run required checks and publish a new immutable coding result. The parent will assign fresh independent review. A new decision must be published as a blocking question; this correction grants no commit/push/merge authority.\n{}",
                serde_json::to_string_pretty(&blockers)?
            );
            if text.len() > 32768 {
                return Err(review_error(
                    "bounded repair brief is full; narrow the findings before input",
                ));
            }
            let intent = RepairIntent {
                request: request.clone(),
                target: self.review_coding_target(&record)?,
                control_epoch: expected_control_epoch,
                text_digest: digest(text.as_bytes()),
                blockers_digest,
            };
            record.review.as_mut().unwrap().repairs.push(intent.clone());
            let store = self.retained.lock();
            self.require_workflow(&store, &auth, &record)?;
            let directory = Directory::open(&record.root, false)?;
            publish_matching(
                &directory,
                &format!("repair-{request}.txt"),
                text.as_bytes(),
            )?;
            store.save_assignment(&record)?;
            intent
        };
        let text = Directory::open(&record.root, false)?.read(&format!("repair-{request}.txt"))?;
        if digest(&text) != intent.text_digest {
            bail!("original corrective brief changed");
        }
        // Common Followup owns all control/conversation/delivery fences and original receipt recovery.
        self.followup(
            auth,
            record.handle,
            request,
            (
                intent.target.assignment,
                intent.target.publication.version,
                intent.control_epoch,
            ),
            String::from_utf8(text)?,
            None,
        )
    }
}

//! Exact independent review evidence and finite parent-owned corrective policy.
use crate::{AssignmentIdentity, CodingTargetReference, PublicationIdentity, PublicationKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewAxis {
    Standards,
    Spec,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewSourceKind {
    Standards,
    Spec,
    Method,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewSourceReference {
    pub kind: ReviewSourceKind,
    pub reference: String,
    pub digest: String,
}

/// Contents are private request/brief data, never retained session metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewSource {
    pub source: ReviewSourceReference,
    pub content: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewTarget {
    pub coding: CodingTargetReference,
    pub sources: Vec<ReviewSourceReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewAssignment {
    pub implementation: String,
    pub axis: ReviewAxis,
    pub target: ReviewTarget,
    pub source_documents: Vec<ReviewSource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewOutcome {
    Passed,
    ChangesRequired,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFinding {
    /// Stable across a corrective round so unchanged blockers remain observable.
    pub id: String,
    pub blocking: bool,
    pub location: String,
    pub finding: String,
    pub source: ReviewSourceReference,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewReport {
    pub target: ReviewTarget,
    pub axis: ReviewAxis,
    pub outcome: ReviewOutcome,
    pub findings: Vec<ReviewFinding>,
}

/// A reference is useful only after authenticating the reviewer's original contract
/// and immutable publication; a worker-supplied claim never satisfies review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewReference {
    pub axis: ReviewAxis,
    pub target: ReviewTarget,
    pub reviewer: AssignmentIdentity,
    pub publication: PublicationIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRequirement {
    pub axis: ReviewAxis,
    pub sources: Vec<ReviewSourceReference>,
}

fn bounded(text: &str, maximum: usize) -> bool {
    !text.trim().is_empty() && text.len() <= maximum && !text.contains('\0')
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl ReviewSourceReference {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !bounded(&self.reference, 4096) || !digest(&self.digest) {
            return Err("review source needs an exact bounded reference and digest");
        }
        Ok(())
    }
}

impl ReviewTarget {
    pub fn validate(&self, axis: &ReviewAxis) -> Result<(), &'static str> {
        if self.coding.assignment != self.coding.publication.assignment
            || self.coding.publication.kind != PublicationKind::Submission
            || self.coding.publication.version == 0
            || !digest(&self.coding.publication.digest)
            || !digest(&self.coding.source_contract_digest)
            || !digest(&self.coding.candidate.digest)
            || self.sources.is_empty()
            || self.sources.len() > 32
        {
            return Err("review requires an exact authenticated coding target and sources");
        }
        for (index, source) in self.sources.iter().enumerate() {
            source.validate()?;
            if self.sources[..index].contains(source) {
                return Err("duplicate review source");
            }
            match (axis, &source.kind) {
                (ReviewAxis::Standards, ReviewSourceKind::Spec)
                | (ReviewAxis::Spec, ReviewSourceKind::Standards | ReviewSourceKind::Method) => {
                    return Err("review contexts must keep Standards and Spec sources separate");
                }
                _ => {}
            }
        }
        let required = match axis {
            ReviewAxis::Standards => ReviewSourceKind::Method,
            ReviewAxis::Spec => ReviewSourceKind::Spec,
        };
        if !self.sources.iter().any(|source| source.kind == required) {
            return Err("Standards requires its installed method; Spec requires its actual spec");
        }
        Ok(())
    }
}

impl ReviewAssignment {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !bounded(&self.implementation, 128)
            || !self
                .implementation
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            || self.implementation != self.target.coding.assignment.request
        {
            return Err("review implementation handle must match the original target");
        }
        self.target.validate(&self.axis)?;
        if self.source_documents.len() != self.target.sources.len()
            || self
                .source_documents
                .iter()
                .map(|source| source.content.len())
                .sum::<usize>()
                > 131072
            || self
                .source_documents
                .iter()
                .any(|source| !bounded(&source.content, 65536))
            || self.target.sources.iter().any(|reference| {
                self.source_documents
                    .iter()
                    .filter(|source| &source.source == reference)
                    .count()
                    != 1
            })
        {
            return Err("review contract must contain each exact actual source document");
        }
        Ok(())
    }
}

impl ReviewReport {
    pub fn validate(&self) -> Result<(), &'static str> {
        self.target.validate(&self.axis)?;
        if self.findings.len() > 64 {
            return Err("too many independent review findings");
        }
        for (index, finding) in self.findings.iter().enumerate() {
            if !bounded(&finding.id, 128)
                || !bounded(&finding.location, 4096)
                || !bounded(&finding.finding, 4096)
                || !self.target.sources.contains(&finding.source)
                || self.findings[..index]
                    .iter()
                    .any(|old| old.id == finding.id)
            {
                return Err(
                    "review finding needs distinct identity, location and exact axis source",
                );
            }
        }
        if (self.outcome == ReviewOutcome::Passed
            && self.findings.iter().any(|finding| finding.blocking))
            || (self.outcome == ReviewOutcome::ChangesRequired
                && !self.findings.iter().any(|finding| finding.blocking))
        {
            return Err("review outcome disagrees with its blocking findings");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDeclaration {
    pub request: String,
    pub target: CodingTargetReference,
    pub requirements: Vec<ReviewRequirement>,
    pub sources: Vec<ReviewSource>,
    pub repair_limit: Option<RepairLimit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepairLimitSource {
    ExplicitTask,
    RepositoryPolicy,
    CoordinatorDefault,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairLimit {
    pub rounds: u32,
    pub source: RepairLimitSource,
    pub reference: String,
}

impl Default for RepairLimit {
    fn default() -> Self {
        Self {
            rounds: 3,
            source: RepairLimitSource::CoordinatorDefault,
            reference: "coordinator default: three corrective rounds; not user approval".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewStop {
    LimitExhausted,
    NoProgress,
    MissingChecks,
    MissingDecision,
    UnsafeChild,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ReviewOperation {
    Declare(Box<ReviewDeclaration>),
    Assignment {
        axis: ReviewAxis,
    },
    Collect {
        axis: ReviewAxis,
        reviewer: String,
    },
    Repair {
        request: String,
        expected_control_epoch: u64,
    },
    Stop {
        reason: ReviewStop,
    },
}

impl RepairLimit {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.rounds == 0 || self.rounds > 32 || !bounded(&self.reference, 4096) {
            return Err("repair policy requires 1..32 finite rounds and declared provenance");
        }
        Ok(())
    }
}

impl ReviewDeclaration {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !bounded(&self.request, 128)
            || !self
                .request
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            || self.requirements.len() > 2
            || self.sources.len() > 64
        {
            return Err("invalid review declaration bounds");
        }
        if let Some(limit) = &self.repair_limit {
            limit.validate()?;
        }
        for (index, requirement) in self.requirements.iter().enumerate() {
            if self.requirements[..index]
                .iter()
                .any(|old| old.axis == requirement.axis)
            {
                return Err("duplicate required review axis");
            }
            ReviewTarget {
                coding: self.target.clone(),
                sources: requirement.sources.clone(),
            }
            .validate(&requirement.axis)?;
            if requirement.sources.iter().any(|reference| {
                !self
                    .sources
                    .iter()
                    .any(|source| &source.source == reference)
            }) {
                return Err("review declaration must supply every actual source document");
            }
        }
        if self
            .sources
            .iter()
            .any(|source| !bounded(&source.content, 65536))
            || self
                .sources
                .iter()
                .map(|source| source.content.len())
                .sum::<usize>()
                > 131072
        {
            return Err("actual review sources exceed private brief bounds");
        }
        Ok(())
    }
}

/// Worker checks, independent review and integration remain separate evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewStatus {
    pub target: CodingTargetReference,
    pub requirements: Vec<ReviewRequirement>,
    pub evidence: Vec<ReviewReference>,
    pub limit: RepairLimit,
    pub consumed_rounds: u32,
    pub pending_repair: Option<String>,
    pub stopped: Option<ReviewStop>,
    pub independent_review_satisfied: bool,
}

/// Existing native research-only publishers do not authenticate typed review reports.
pub fn require_typed_review_provider(provider: &str) -> Result<(), &'static str> {
    if provider != "pi" {
        return Err(
            "requested provider lacks authenticated typed independent-review publication; read/result_file alone is insufficient",
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "delegation_review_tests.rs"]
mod tests;

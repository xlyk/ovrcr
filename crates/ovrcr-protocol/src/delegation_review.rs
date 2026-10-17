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

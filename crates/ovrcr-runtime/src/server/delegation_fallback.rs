//! File-only fallback validation. The authenticated Server caller must hold its
//! authority/binding fences through capture and through final publication.
use super::delegation_files::Directory;
use anyhow::{Context, Result, bail};
use ovrcr_protocol::{
    AssignmentControlEvidence, ControlOwner, DELEGATION_SCHEMA, ResearchContract,
    ResearchSubmission, WorkflowAuthority,
};
use ovrcr_protocol::{
    FallbackArtifact, FallbackCapture, FallbackContext, FallbackFileReference,
    FallbackOriginalCondition, FallbackRecovery, FallbackRecoveryDraft, FrozenFallbackFile,
};
use sha2::{Digest, Sha256};

const MAX_SCREEN_BYTES: usize = 64 * 1024;
const MAX_ARTIFACT_BYTES: usize = 64 * 1024;
const MAX_EVIDENCE_BYTES: usize = 128 * 1024;
const MAX_ORIGINAL_BYTES: usize = 64 * 1024;

/// Known filenames supplied by the authoritative assignment/version reader.
pub(super) struct FallbackFiles<'a> {
    pub directory: &'a Directory,
    pub contract_name: &'a str,
    pub brief_name: &'a str,
    pub original_name: &'a str,
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

pub(super) fn validate_context(context: &FallbackContext) -> Result<()> {
    if context.identity.session.is_none()
        || context.identity.run.is_none()
        || context.result_version == 0
        || !valid_digest(&context.contract_digest)
        || !valid_digest(&context.brief_digest)
        || !valid_digest(&context.freshness_nonce)
        || context.binding.invocation.is_empty()
        || context.binding.conversation.is_empty()
        || context.binding.generation == 0
        || context.control.epoch == 0
        || context.control.owner != ControlOwner::Controller
        || context.control.barrier.is_some()
        || context.control.attribution_invalidated
    {
        bail!("fallback attribution is incomplete or unresolved");
    }
    Ok(())
}

/// The caller derives current evidence under the held Server fences, never from
/// request payloads. Equality cannot substitute for those fences or authenticate a source.
pub(super) fn validate_capture(
    capture: &FallbackCapture,
    current: &FallbackContext,
    authority: &WorkflowAuthority,
    current_evidence: &AssignmentControlEvidence,
) -> Result<()> {
    validate_context(current)?;
    if capture.schema != DELEGATION_SCHEMA
        || capture.context != *current
        || capture.authority != *authority
        || capture.evidence != *current_evidence
        || current_evidence.server_lifetime != authority.server_lifetime
        || current_evidence.identity != current.identity
        || current_evidence.binding != current.binding
        || current_evidence.result_version != current.result_version
        || current_evidence.human_input_revision != current.control.human_input_revision
        || current_evidence.publication.is_some()
        || capture.server_lifetime != authority.server_lifetime
        || authority.server_lifetime.is_empty()
        || authority.instance != current.identity.instance
        || authority.workflow != current.identity.workflow
        || authority.controller != current.identity.controller
        || !valid_name(&capture.id)
        || capture.captured_unix_ms == 0
        || capture.size.rows == 0
        || capture.size.cols == 0
        || capture.truncated
        || capture.screen.len() > MAX_SCREEN_BYTES
    {
        bail!("fallback capture is stale, truncated or unattributed");
    }
    Ok(())
}

fn read_reference(
    directory: &Directory,
    reference: &FallbackFileReference,
    bound: usize,
) -> Result<FrozenFallbackFile> {
    if !valid_name(&reference.name) || !valid_digest(&reference.digest) {
        bail!("invalid fallback evidence reference");
    }
    let bytes = directory.read(&reference.name)?;
    if bytes.len() > bound || digest(&bytes) != reference.digest {
        bail!("fallback evidence is oversized or changed");
    }
    Ok(FrozenFallbackFile {
        reference: reference.clone(),
        bytes,
    })
}

/// Preserve missing versus invalid without replacing the normal child publication.
/// A valid child result requires ordinary collection, never fallback replacement.
pub(super) fn original_condition(
    directory: &Directory,
    name: &str,
    current: &FallbackContext,
) -> Result<(FallbackOriginalCondition, Option<Vec<u8>>)> {
    if !valid_name(name) {
        bail!("invalid original publication name");
    }
    let bytes = match directory.read(name) {
        Ok(bytes) => bytes,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok((FallbackOriginalCondition::Missing, None));
        }
        Err(error) => return Err(error).context("original publication cannot be preserved"),
    };
    if bytes.len() > MAX_ORIGINAL_BYTES {
        bail!("original publication exceeds fallback preservation bound");
    }
    if serde_json::from_slice::<ResearchSubmission>(&bytes)
        .ok()
        .is_some_and(|submission| {
            submission.version == current.result_version
                && submission.validate(&current.identity).is_ok()
        })
    {
        bail!("valid child publication requires ordinary collection");
    }
    Ok((
        FallbackOriginalCondition::Invalid {
            digest: digest(&bytes),
        },
        Some(bytes),
    ))
}

/// `current` and the filenames come from the Server's authoritative record,
/// never from the recovery author. Hash the original bytes, not a reserialization.
pub(super) fn validate_capture_files(
    files: &FallbackFiles<'_>,
    capture: &FallbackCapture,
    current: &FallbackContext,
    authority: &WorkflowAuthority,
    current_evidence: &AssignmentControlEvidence,
) -> Result<ResearchContract> {
    let FallbackFiles {
        directory,
        contract_name,
        brief_name,
        original_name,
    } = files;
    validate_capture(capture, current, authority, current_evidence)?;
    let contract = read_reference(
        directory,
        &FallbackFileReference {
            name: (*contract_name).into(),
            digest: current.contract_digest.clone(),
        },
        MAX_ARTIFACT_BYTES,
    )?;
    let contract: ResearchContract = serde_json::from_slice(&contract.bytes)?;
    contract.validate().map_err(anyhow::Error::msg)?;
    if contract.workflow != current.identity.workflow
        || contract.controller != current.identity.controller
        || contract.task != current.identity.task
    {
        bail!("fallback contract does not identify the current assignment");
    }
    let brief = read_reference(
        directory,
        &FallbackFileReference {
            name: (*brief_name).into(),
            digest: current.brief_digest.clone(),
        },
        MAX_ARTIFACT_BYTES,
    )?;
    let brief: serde_json::Value = serde_json::from_slice(&brief.bytes)?;
    let brief_identity = brief
        .get("identity")
        .cloned()
        .and_then(|value| serde_json::from_value::<ovrcr_protocol::AssignmentIdentity>(value).ok());
    if brief.get("schema").and_then(|value| value.as_u64()) != Some(1)
        || brief_identity.as_ref() != Some(&current.identity)
        || brief.get("result_version").and_then(|value| value.as_u64())
            != Some(u64::from(current.result_version))
        || brief.get("fallback_nonce").and_then(|value| value.as_str())
            != Some(current.freshness_nonce.as_str())
    {
        bail!("fallback freshness anchor is absent from the current immutable brief");
    }
    let (original, bytes) = original_condition(directory, original_name, current)?;
    if original != capture.original || bytes != capture.original_bytes {
        bail!("original submission changed since capture");
    }
    Ok(contract)
}

/// The Server supplies a trusted capture and current immutable contract. A caller
/// cannot turn an arbitrary screen or its own success boolean into evidence.
pub(super) fn prepare_recovery(
    files: &FallbackFiles<'_>,
    capture: &FallbackCapture,
    current: &FallbackContext,
    author: &WorkflowAuthority,
    current_evidence: &AssignmentControlEvidence,
    draft: &FallbackRecoveryDraft,
) -> Result<FallbackRecovery> {
    let contract = validate_capture_files(files, capture, current, author, current_evidence)?;
    let FallbackFiles {
        directory,
        original_name,
        ..
    } = files;
    if draft.version == 0 || draft.version > 64 || draft.capture_id != capture.id {
        bail!("invalid fallback recovery version or capture reference");
    }
    if capture.original != draft.original {
        bail!("original submission condition changed; collect again");
    }
    if draft.artifact.name == *original_name {
        bail!("fallback evidence cannot replace a child submission");
    }
    let artifact = read_reference(directory, &draft.artifact, MAX_ARTIFACT_BYTES)?;
    let content: FallbackArtifact =
        serde_json::from_slice(&artifact.bytes).context("invalid complete fallback artifact")?;
    if content.schema != DELEGATION_SCHEMA
        || content.context != current.into()
        || content.deliverable.trim().is_empty()
        || content.deliverable.len() > 32 * 1024
        || !content.unresolved.is_empty()
    {
        bail!("fallback artifact is incomplete or belongs to old work");
    }
    let marker = format!("OVRCR_FALLBACK:{}", draft.artifact.digest);
    if !capture.screen.lines().any(|line| line.trim_end() == marker) {
        bail!("active screen lacks the complete current artifact marker");
    }
    if draft.evidence.len() != contract.acceptance_evidence.len() {
        bail!("fallback does not cover the complete acceptance requirements");
    }
    let mut evidence_files = Vec::new();
    let mut total = 0;
    for (required, evidence) in contract.acceptance_evidence.iter().zip(&draft.evidence) {
        if evidence.requirement != *required
            || evidence.excerpt.trim().is_empty()
            || evidence.excerpt.len() > 4096
            || evidence.assessment.trim().is_empty()
            || evidence.assessment.len() > 4096
            || evidence.file.name == draft.artifact.name
            || evidence.file.name == *original_name
        {
            bail!("fallback requires independent evidence for each contract requirement");
        }
        let file = read_reference(directory, &evidence.file, MAX_EVIDENCE_BYTES)?;
        let text = std::str::from_utf8(&file.bytes).context("fallback evidence must be text")?;
        if !text.contains(&evidence.excerpt) {
            bail!("fallback evidence excerpt is not present in the inspected bytes");
        }
        total += file.bytes.len();
        if total > MAX_EVIDENCE_BYTES {
            bail!("fallback evidence exceeds aggregate preservation bound");
        }
        evidence_files.push(file);
    }
    Ok(FallbackRecovery {
        schema: DELEGATION_SCHEMA,
        version: draft.version,
        author: author.clone(),
        capture: capture.clone(),
        artifact,
        contract: read_reference(
            directory,
            &FallbackFileReference {
                name: files.contract_name.into(),
                digest: current.contract_digest.clone(),
            },
            MAX_ARTIFACT_BYTES,
        )?,
        brief: read_reference(
            directory,
            &FallbackFileReference {
                name: files.brief_name.into(),
                digest: current.brief_digest.clone(),
            },
            MAX_ARTIFACT_BYTES,
        )?,
        evidence: draft.evidence.clone(),
        evidence_files,
    })
}

/// Recheck files and all live provenance while the Server still holds its fences.
/// The immutable file is discovered only after its complete bytes are published.
pub(super) fn publish_recovery(
    files: &FallbackFiles<'_>,
    current: &FallbackContext,
    author: &WorkflowAuthority,
    current_evidence: &AssignmentControlEvidence,
    recovery: &FallbackRecovery,
) -> Result<String> {
    let draft = FallbackRecoveryDraft {
        capture_id: recovery.capture.id.clone(),
        version: recovery.version,
        artifact: recovery.artifact.reference.clone(),
        original: recovery.capture.original.clone(),
        evidence: recovery.evidence.clone(),
    };
    let checked = prepare_recovery(
        files,
        &recovery.capture,
        current,
        author,
        current_evidence,
        &draft,
    )?;
    if checked != *recovery {
        bail!("fallback evidence changed before immutable publication");
    }
    let name = format!(
        "parent-fallback-r{}-v{}.json",
        current.result_version, recovery.version
    );
    let bytes = serde_json::to_vec(recovery)?;
    let directory = files.directory;
    // Identical retries are harmless; a version can never be reused for new bytes.
    match directory.read(&name) {
        Ok(existing) if existing == bytes => return Ok(name),
        Ok(_) => bail!("fallback recovery version already contains different evidence"),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
        Err(error) => return Err(error),
    }
    directory.publish(&name, &bytes)?;
    Ok(name)
}

/// Historical verification uses only frozen bytes and stored identity/digests.
/// Mutable originals and today's source capability are never historical authority.
pub(super) fn verify_frozen(recovery: &FallbackRecovery) -> Result<()> {
    let capture = &recovery.capture;
    let context = &capture.context;
    validate_capture(capture, context, &capture.authority, &capture.evidence)?;
    if recovery.schema != DELEGATION_SCHEMA
        || recovery.version == 0
        || recovery.version > 64
        || recovery.author != capture.authority
        || recovery.contract.reference.name
            != format!("contract-v{}.json", context.identity.contract)
        || recovery.brief.reference.name != format!("brief-v{}.json", context.result_version)
        || recovery.contract.reference.digest != context.contract_digest
        || recovery.brief.reference.digest != context.brief_digest
    {
        bail!("frozen recovery identity is inconsistent");
    }
    for file in std::iter::once(&recovery.contract)
        .chain(std::iter::once(&recovery.brief))
        .chain(std::iter::once(&recovery.artifact))
        .chain(recovery.evidence_files.iter())
    {
        if !valid_name(&file.reference.name) || digest(&file.bytes) != file.reference.digest {
            bail!("frozen fallback bytes changed");
        }
    }
    if recovery.contract.bytes.len() > MAX_ARTIFACT_BYTES
        || recovery.brief.bytes.len() > MAX_ARTIFACT_BYTES
        || recovery.artifact.bytes.len() > MAX_ARTIFACT_BYTES
        || capture
            .original_bytes
            .as_ref()
            .is_some_and(|b| b.len() > MAX_ORIGINAL_BYTES)
    {
        bail!("frozen fallback exceeds bounds");
    }
    match (&capture.original, &capture.original_bytes) {
        (FallbackOriginalCondition::Missing, None) => (),
        (FallbackOriginalCondition::Invalid { digest: expected }, Some(bytes))
            if digest(bytes) == *expected =>
        {
            if serde_json::from_slice::<ResearchSubmission>(bytes)
                .ok()
                .is_some_and(|s| {
                    s.version == context.result_version && s.validate(&context.identity).is_ok()
                })
            {
                bail!("frozen original was a valid child submission");
            }
        }
        _ => bail!("frozen original condition changed"),
    }
    let contract: ResearchContract = serde_json::from_slice(&recovery.contract.bytes)?;
    contract.validate().map_err(anyhow::Error::msg)?;
    if contract.workflow != context.identity.workflow
        || contract.controller != context.identity.controller
        || contract.task != context.identity.task
    {
        bail!("frozen contract identity changed");
    }
    let brief: serde_json::Value = serde_json::from_slice(&recovery.brief.bytes)?;
    if brief["schema"].as_u64() != Some(1)
        || serde_json::from_value::<ovrcr_protocol::AssignmentIdentity>(brief["identity"].clone())?
            != context.identity
        || brief["result_version"].as_u64() != Some(u64::from(context.result_version))
        || brief["fallback_nonce"].as_str() != Some(context.freshness_nonce.as_str())
    {
        bail!("frozen brief freshness changed");
    }
    let artifact: FallbackArtifact = serde_json::from_slice(&recovery.artifact.bytes)?;
    if artifact.schema != DELEGATION_SCHEMA
        || artifact.context != context.into()
        || artifact.deliverable.trim().is_empty()
        || artifact.deliverable.len() > 32 * 1024
        || !artifact.unresolved.is_empty()
        || !capture.screen.lines().any(|line| {
            line.trim_end() == format!("OVRCR_FALLBACK:{}", recovery.artifact.reference.digest)
        })
        || recovery.evidence.len() != contract.acceptance_evidence.len()
        || recovery.evidence_files.len() != recovery.evidence.len()
    {
        bail!("frozen fallback is incomplete");
    }
    let original_name = format!("submission-v{}.json", context.result_version);
    if recovery.artifact.reference.name == original_name {
        bail!("fallback replaces child evidence");
    }
    let mut total = 0usize;
    for ((required, evidence), file) in contract
        .acceptance_evidence
        .iter()
        .zip(&recovery.evidence)
        .zip(&recovery.evidence_files)
    {
        total = total
            .checked_add(file.bytes.len())
            .context("evidence size overflow")?;
        if evidence.requirement != *required
            || evidence.file != file.reference
            || evidence.file.name == recovery.artifact.reference.name
            || evidence.file.name == original_name
            || evidence.excerpt.trim().is_empty()
            || evidence.excerpt.len() > 4096
            || evidence.assessment.trim().is_empty()
            || evidence.assessment.len() > 4096
            || total > MAX_EVIDENCE_BYTES
            || !std::str::from_utf8(&file.bytes)?.contains(&evidence.excerpt)
        {
            bail!("frozen independent evidence is incomplete");
        }
    }
    Ok(())
}

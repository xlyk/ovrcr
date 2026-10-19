//! Readable review files use the existing immutable private assignment publisher.
use super::*;
use crate::git::candidate::{
    CandidateFile,
    review::{readable, readable_image},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    target: ReviewTarget,
    axis: ReviewAxis,
    round: String,
    context_paths: Vec<String>,
    files: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    kind: String,
    path: String,
    before_mode: u32,
    after_mode: u32,
    before: Option<Artifact>,
    after: Option<Artifact>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    path: std::path::PathBuf,
    digest: String,
}

fn axis_name(axis: &ReviewAxis) -> &'static str {
    match axis {
        ReviewAxis::Standards => "standards",
        ReviewAxis::Spec => "spec",
    }
}

pub(super) fn publish(
    record: &Record,
    declared: &ReviewDeclaration,
) -> Result<Vec<(ReviewAxis, ReviewViewReference)>> {
    use crate::server::delegation_coding::publication::CodingPublication;
    let directory = Directory::open(&record.root, false)?;
    let name = format!("candidate-v{}.json", declared.target.publication.version);
    if declared.target.candidate.path != record.root.join(&name) {
        bail!("review candidate escaped its original assignment");
    }
    let bytes = directory.read(&name)?;
    if digest(&bytes) != declared.target.candidate.digest {
        bail!("review candidate bytes changed");
    }
    // read_version_submission already authenticates this exact publication/provenance.
    let publication: CodingPublication = serde_json::from_slice(&bytes)?;
    let snapshot = &publication.snapshot;
    let baseline = ServerState::read_input_baseline(record)?;
    let context = crate::git::candidate::review::context(
        &snapshot.repository,
        &snapshot.base,
        &declared.context_paths,
    )?;
    let mut views = Vec::new();
    let mut publications = Vec::new();
    for requirement in &declared.requirements {
        let prefix = format!(
            "review-view-{}-{}",
            declared.request,
            axis_name(&requirement.axis)
        );
        let target = ReviewTarget {
            coding: declared.target.clone(),
            sources: requirement.sources.clone(),
            view: None,
        };
        let mut manifest = Manifest {
            schema: 1,
            target,
            axis: requirement.axis.clone(),
            round: declared.request.clone(),
            context_paths: declared.context_paths.clone(),
            files: Vec::new(),
        };
        for (kind, files) in [
            ("candidate", snapshot.files.as_slice()),
            (
                "inherited",
                baseline
                    .as_ref()
                    .map(|b| b.inputs.as_slice())
                    .unwrap_or_default(),
            ),
            ("context", context.as_slice()),
        ] {
            for file in files {
                add(
                    record,
                    &prefix,
                    kind,
                    file,
                    &mut manifest,
                    &mut publications,
                )?;
            }
        }
        let bytes = serde_json::to_vec_pretty(&manifest)?;
        if bytes.len() > 512 * 1024 {
            bail!("complete readable review manifest exceeds publication bound");
        }
        let name = format!("{prefix}.json");
        views.push((
            requirement.axis.clone(),
            ReviewViewReference {
                path: record.root.join(&name),
                digest: digest(&bytes),
                round: declared.request.clone(),
                context_paths: declared.context_paths.clone(),
            },
        ));
        publications.push((name, bytes));
    }
    // Validate every byte/bound first: never launch with an omitted or truncated image.
    for (name, bytes) in publications {
        publish_matching(&directory, &name, &bytes)?;
    }
    Ok(views)
}

fn add(
    record: &Record,
    prefix: &str,
    kind: &str,
    file: &CandidateFile,
    manifest: &mut Manifest,
    publications: &mut Vec<(String, Vec<u8>)>,
) -> Result<()> {
    let mut artifact =
        |side: &str, mode: u32, bytes: Option<Vec<u8>>| -> Result<Option<Artifact>> {
            let Some(bytes) = bytes else { return Ok(None) };
            readable_image(&file.path, mode, &bytes)?;
            let name = format!("{prefix}-{}-{side}.txt", manifest.files.len());
            let reference = Artifact {
                path: record.root.join(&name),
                digest: digest(&bytes),
            };
            publications.push((name, bytes));
            Ok(Some(reference))
        };
    let before = artifact("before", file.base_mode, file.base_bytes()?)?;
    let after = artifact("after", file.mode, file.bytes()?)?;
    manifest.files.push(Entry {
        kind: kind.into(),
        path: file.path.clone(),
        before_mode: file.base_mode,
        after_mode: file.mode,
        before,
        after,
    });
    Ok(())
}

pub(in crate::server) fn verify(
    record: &Record,
    target: &ReviewTarget,
    axis: &ReviewAxis,
) -> Result<()> {
    let reference = target
        .view
        .as_ref()
        .context("complete readable review view missing; Blocked before launch")?;
    reference.validate().map_err(anyhow::Error::msg)?;
    let prefix = format!("review-view-{}-{}", reference.round, axis_name(axis));
    if reference.path != record.root.join(format!("{prefix}.json")) {
        bail!("review view escaped its original private assignment");
    }
    let directory = Directory::open(&record.root, false)?;
    let bytes = directory.read(&format!("{prefix}.json"))?;
    if digest(&bytes) != reference.digest {
        bail!("immutable readable review view changed");
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let mut expected = target.clone();
    expected.view = None;
    if manifest.schema != 1
        || manifest.target != expected
        || manifest.axis != *axis
        || manifest.round != reference.round
        || manifest.context_paths != reference.context_paths
    {
        bail!("readable review view has stale/swapped candidate, context, sources, axis or round");
    }
    for (index, entry) in manifest.files.iter().enumerate() {
        for (side, artifact) in [("before", &entry.before), ("after", &entry.after)] {
            if let Some(artifact) = artifact {
                let name = format!("{prefix}-{index}-{side}.txt");
                if artifact.path != record.root.join(&name) {
                    bail!("readable review artifact escaped its private assignment");
                }
                let bytes = directory.read(&name)?;
                if digest(&bytes) != artifact.digest {
                    bail!("immutable readable review artifact changed");
                }
                readable(&bytes)?;
            }
        }
    }
    Ok(())
}

pub(in crate::server) fn verify_current(
    record: &Record,
    target: &ReviewTarget,
    axis: &ReviewAxis,
) -> Result<()> {
    verify(record, target, axis)
        .map_err(|error| review_error(format!("review Blocked: {error}")))?;
    if let Some(state) = &record.review {
        let round = state
            .rounds
            .last()
            .context("current review round missing")?;
        if round.target != target.coding
            || !round.views.iter().any(|(selected, reference)| {
                selected == axis && target.view.as_ref() == Some(reference)
            })
        {
            return Err(review_error(
                "review view is not the current declared axis/round",
            ));
        }
    }
    Ok(())
}

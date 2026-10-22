//! A replay resolves one stored candidate; it never selects a newer checkout state.
use super::{Directory, digest};
use anyhow::{Context, Result, bail};
use ovrcr_protocol::{
    CandidateReference, CheckOutcome, CodingCheck, PublicationIdentity, PublicationKind,
    ResearchSubmission,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::server) struct CodingPublication {
    pub schema: u32,
    pub report: ResearchSubmission,
    pub snapshot: crate::git::candidate::CandidateSnapshot,
}

pub(super) fn prepare(
    directory: &Directory,
    name: &str,
    report: &ResearchSubmission,
    expected_digest: Option<&str>,
    capture_allowed: bool,
    capture: impl FnOnce() -> Result<crate::git::candidate::CandidateSnapshot>,
) -> Result<(CodingPublication, String, Vec<u8>)> {
    if report
        .coding
        .as_ref()
        .context("coding check evidence missing")?
        .candidate
        .is_some()
    {
        bail!("only the Server may attach a coding candidate");
    }
    let bytes = match directory.read(name) {
        Ok(_) if capture_allowed && expected_digest.is_none() => {
            bail!("candidate file has no authenticated publication intent; refuse adoption");
        }
        Ok(bytes) => bytes,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|cause| cause.kind() == std::io::ErrorKind::NotFound) =>
        {
            if !capture_allowed || expected_digest.is_some() {
                bail!("authenticated candidate is missing; refuse recapture");
            }
            let publication = CodingPublication {
                schema: 1,
                report: report.clone(),
                snapshot: capture()?,
            };
            serde_json::to_vec(&publication)?
        }
        Err(error) => return Err(error),
    };
    let hash = digest(&bytes);
    if expected_digest.is_some_and(|expected| expected != hash) {
        bail!("immutable candidate digest changed");
    }
    let publication: CodingPublication = serde_json::from_slice(&bytes)?;
    if publication.schema != 1 || publication.report != *report {
        bail!("candidate version belongs to a different immutable child report");
    }
    Ok((publication, hash, bytes))
}

/// Called only after the ordinary child-authenticated result intent is durable.
pub(super) fn commit(
    directory: &Directory,
    name: &str,
    bytes: &[u8],
    expected_digest: &str,
) -> Result<()> {
    if digest(bytes) != expected_digest {
        bail!("prepared candidate bytes differ from authenticated result reference");
    }
    if let Err(error) = directory.publish(name, bytes) {
        if directory.read(name).ok().as_deref() != Some(bytes) {
            return Err(error);
        }
        directory.sync()?;
    }
    Ok(())
}

impl CodingPublication {
    pub(super) fn submission(
        &self,
        required: &[String],
        path: PathBuf,
        digest: String,
        authenticated: Option<&PublicationIdentity>,
    ) -> Result<ResearchSubmission> {
        let mut submission = self.report.clone();
        let result = submission
            .coding
            .as_mut()
            .context("coding check evidence missing")?;
        for name in required {
            if !result.checks.iter().any(|check| &check.name == name) {
                result.checks.push(CodingCheck {
                    name: name.clone(),
                    command: None,
                    outcome: CheckOutcome::Missing,
                    exit_code: None,
                    evidence: Vec::new(),
                });
            }
        }
        result.candidate = Some(CandidateReference {
            input_baseline: self.snapshot.input_baseline.clone(),
            path,
            digest,
            workspace: self.snapshot.workspace.clone(),
            base: self.snapshot.base.clone(),
            git_identity: self.snapshot.git_identity.clone(),
            changed_artifacts: self
                .snapshot
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect(),
        });
        submission
            .validate(&self.report.identity)
            .map_err(anyhow::Error::msg)?;
        if let Some(expected) = authenticated
            && (expected.kind != PublicationKind::Submission
                || expected.assignment != submission.identity
                || expected.version != submission.version
                || expected.digest
                    != super::digest(&crate::server::delegation::submission_bytes(&submission)?))
        {
            bail!("candidate result does not match its authenticated publication intent");
        }
        Ok(submission)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::candidate::{CandidateFile, CandidateSnapshot};
    use base64::{Engine, engine::general_purpose::STANDARD};

    fn report() -> ResearchSubmission {
        serde_json::from_value(serde_json::json!({"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"failed","summary":"Actual failed check.","evidence":["check.log"],"unresolved":[],"coding":{"checks":[{"name":"failure","command":"false","outcome":"Failed","exit_code":1,"evidence":["check.log"]},{"name":"unrun","command":null,"outcome":"Unrun","exit_code":null,"evidence":[]}],"candidate":null}})).unwrap()
    }
    fn prepare_and_commit(
        directory: &Directory,
        name: &str,
        report: &ResearchSubmission,
        expected: Option<&str>,
        capture_allowed: bool,
        capture: impl FnOnce() -> Result<CandidateSnapshot>,
    ) -> Result<(CodingPublication, String)> {
        let (publication, hash, bytes) =
            prepare(directory, name, report, expected, capture_allowed, capture)?;
        commit(directory, name, &bytes, &hash)?;
        Ok((publication, hash))
    }
    fn snapshot(bytes: &[u8]) -> CandidateSnapshot {
        CandidateSnapshot {
            input_baseline: None,
            schema: 1,
            project: "project".into(),
            repository: "/repo".into(),
            workspace: "workspace".into(),
            git_identity: "generation".into(),
            base: "1".repeat(40),
            head: "2".repeat(40),
            files: vec![CandidateFile {
                path: "file".into(),
                base_mode: 0,
                base_content: None,
                base_digest: None,
                mode: 0o100644,
                content: Some(STANDARD.encode(bytes)),
                digest: Some(digest(bytes)),
            }],
        }
    }
    #[test]
    fn replay_keeps_original_candidate_after_checkout_changes_and_removal() {
        let root = tempfile::tempdir().unwrap();
        let directory = Directory::open(
            &std::fs::canonicalize(root.path())
                .unwrap()
                .join("assignment"),
            true,
        )
        .unwrap();
        let checkout = root.path().join("checkout");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(checkout.join("file"), b"original bytes").unwrap();
        let report = report();
        let (stored, hash) =
            prepare_and_commit(&directory, "candidate-v1.json", &report, None, true, || {
                Ok(snapshot(&std::fs::read(checkout.join("file"))?))
            })
            .unwrap();
        std::fs::write(checkout.join("file"), b"later mutable bytes").unwrap();
        std::fs::remove_dir_all(checkout).unwrap();
        let (replayed, replayed_hash, _) = prepare(
            &directory,
            "candidate-v1.json",
            &report,
            Some(&hash),
            false,
            || panic!("replay must never inspect a checkout"),
        )
        .unwrap();
        assert_eq!(stored, replayed);
        assert_eq!(hash, replayed_hash);
        assert_eq!(
            replayed.snapshot.files[0].bytes().unwrap().unwrap(),
            b"original bytes"
        );
        let mut changed = report;
        changed.summary = "changed report".into();
        assert!(
            prepare(
                &directory,
                "candidate-v1.json",
                &changed,
                Some(&hash),
                false,
                || panic!("changed report cannot capture")
            )
            .is_err()
        );
    }
    #[test]
    fn missing_corrupt_or_digest_mismatched_publications_never_recapture() {
        let root = tempfile::tempdir().unwrap();
        let directory = Directory::open(
            &std::fs::canonicalize(root.path())
                .unwrap()
                .join("assignment"),
            true,
        )
        .unwrap();
        let report = report();
        assert!(
            prepare(
                &directory,
                "missing.json",
                &report,
                Some("expected"),
                false,
                || panic!("committed candidate cannot recapture")
            )
            .is_err()
        );
        directory.publish("corrupt.json", b"{partial").unwrap();
        assert!(
            prepare_and_commit(&directory, "corrupt.json", &report, None, true, || panic!(
                "corrupt publication cannot recapture"
            ))
            .is_err()
        );
        let (_, hash) =
            prepare_and_commit(&directory, "candidate.json", &report, None, true, || {
                Ok(snapshot(b"original"))
            })
            .unwrap();
        assert!(
            prepare(
                &directory,
                "candidate.json",
                &report,
                Some(&format!("{hash}wrong")),
                false,
                || panic!("digest mismatch cannot recapture")
            )
            .is_err()
        );
    }
    #[test]
    fn server_enrichment_preserves_actual_checks_and_adds_missing_without_acceptance() {
        let publication = CodingPublication {
            schema: 1,
            report: report(),
            snapshot: snapshot(b"bytes"),
        };
        let result = publication
            .submission(
                &["failure".into(), "unrun".into(), "missing".into()],
                "/private/candidate.json".into(),
                "a".repeat(64),
                None,
            )
            .unwrap();
        let coding = result.coding.unwrap();
        assert_eq!(coding.checks.len(), 3);
        assert_eq!(coding.checks[0].outcome, CheckOutcome::Failed);
        assert_eq!(coding.checks[1].outcome, CheckOutcome::Unrun);
        assert_eq!(coding.checks[2].outcome, CheckOutcome::Missing);
        assert_eq!(coding.candidate.unwrap().changed_artifacts, ["file"]);
        assert_eq!(result.outcome, "failed");
    }
    #[test]
    fn intent_without_final_result_recovers_only_exact_stored_candidate() {
        let root = tempfile::tempdir().unwrap();
        let directory = Directory::open(
            &std::fs::canonicalize(root.path())
                .unwrap()
                .join("assignment"),
            true,
        )
        .unwrap();
        let report = report();
        let required = vec!["failure".into(), "unrun".into(), "missing".into()];
        let path = std::fs::canonicalize(root.path())
            .unwrap()
            .join("assignment/candidate-v1.json");
        let (original, hash) =
            prepare_and_commit(&directory, "candidate-v1.json", &report, None, true, || {
                Ok(snapshot(b"immutable original"))
            })
            .unwrap();
        let result = original
            .submission(&required, path.clone(), hash.clone(), None)
            .unwrap();
        // This models a durable authenticated intent before submission-v1.json or
        // final result_digest exists. Only its exact digest may be reconstructed.
        let intent = PublicationIdentity {
            assignment: report.identity.clone(),
            kind: PublicationKind::Submission,
            version: 1,
            digest: digest(&serde_json::to_vec(&result).unwrap()),
        };
        let (recovered, recovered_hash, _) = prepare(
            &directory,
            "candidate-v1.json",
            &report,
            None,
            false,
            || panic!("authenticated intent must never capture mutable content"),
        )
        .unwrap();
        assert_eq!(
            recovered
                .submission(&required, path.clone(), recovered_hash, Some(&intent))
                .unwrap(),
            result
        );
        assert!(
            prepare(
                &directory,
                "missing-candidate.json",
                &report,
                None,
                false,
                || panic!("missing candidate cannot be recaptured for an intent")
            )
            .is_err()
        );
        let changed = CodingPublication {
            schema: 1,
            report: report.clone(),
            snapshot: snapshot(b"different valid snapshot"),
        };
        let changed_bytes = serde_json::to_vec(&changed).unwrap();
        assert!(
            changed
                .submission(
                    &required,
                    path.clone(),
                    digest(&changed_bytes),
                    Some(&intent)
                )
                .is_err()
        );
        let mut wrong_version = intent.clone();
        wrong_version.version += 1;
        assert!(
            original
                .submission(&required, path.clone(), hash.clone(), Some(&wrong_version))
                .is_err()
        );
        let mut wrong_identity = intent;
        wrong_identity.assignment.controller_epoch += 1;
        assert!(
            original
                .submission(&required, path, hash, Some(&wrong_identity))
                .is_err()
        );
    }

    #[test]
    fn intent_recovery_rejects_missing_corrupt_and_replaced_candidate_without_capture() {
        let root = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(root.path())
            .unwrap()
            .join("assignment");
        let directory = Directory::open(&path, true).unwrap();
        let report = report();
        let (original, hash) =
            prepare_and_commit(&directory, "candidate-v1.json", &report, None, true, || {
                Ok(snapshot(b"authenticated original"))
            })
            .unwrap();
        let required = vec!["original required check".into()];
        let result = original
            .submission(&required, path.join("candidate-v1.json"), hash, None)
            .unwrap();
        let intent = PublicationIdentity {
            assignment: report.identity.clone(),
            kind: PublicationKind::Submission,
            version: report.version,
            digest: digest(&serde_json::to_vec(&result).unwrap()),
        };
        let bytes = directory.read("candidate-v1.json").unwrap();
        for corrupt in [None, Some(b"{partial".as_slice())] {
            std::fs::remove_file(path.join("candidate-v1.json")).unwrap();
            if let Some(bytes) = corrupt {
                directory.publish("candidate-v1.json", bytes).unwrap();
            }
            assert!(
                prepare(
                    &directory,
                    "candidate-v1.json",
                    &report,
                    None,
                    false,
                    || panic!("authenticated intent cannot recapture a missing/corrupt candidate")
                )
                .is_err()
            );
            if corrupt.is_none() {
                assert!(!path.join("candidate-v1.json").exists());
            } else {
                std::fs::remove_file(path.join("candidate-v1.json")).unwrap();
            }
            directory.publish("candidate-v1.json", &bytes).unwrap();
        }
        let (recovered, hash, _) = prepare(
            &directory,
            "candidate-v1.json",
            &report,
            None,
            false,
            || panic!("recovery uses only original stored candidate bytes"),
        )
        .unwrap();
        assert_eq!(
            recovered
                .submission(
                    &required,
                    path.join("candidate-v1.json"),
                    hash.clone(),
                    Some(&intent)
                )
                .unwrap(),
            result
        );
        assert!(
            recovered
                .submission(
                    &["current replacement check".into()],
                    path.join("candidate-v1.json"),
                    hash,
                    Some(&intent)
                )
                .is_err(),
            "enrichment with a different contract must not satisfy the original intent digest"
        );
        assert_eq!(directory.read("candidate-v1.json").unwrap(), bytes);
        assert!(
            directory.read("submission-v1.json").is_err(),
            "the helper cannot promote a candidate to an authenticated result"
        );
    }
    #[test]
    fn preparation_does_not_reserve_candidate_before_authenticated_intent() {
        let root = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(root.path())
            .unwrap()
            .join("assignment");
        let directory = Directory::open(&path, true).unwrap();
        let report = report();
        let (rejected, rejected_hash, rejected_bytes) =
            prepare(&directory, "candidate-v1.json", &report, None, true, || {
                Ok(snapshot(b"rejected preparation"))
            })
            .unwrap();
        assert!(!path.join("candidate-v1.json").exists());
        let mut authorized_report = report.clone();
        authorized_report.summary = "different valid report after rejected preparation".into();
        let (authorized, authorized_hash, authorized_bytes) = prepare(
            &directory,
            "candidate-v1.json",
            &authorized_report,
            None,
            true,
            || Ok(snapshot(b"authorized current content")),
        )
        .unwrap();
        assert_ne!(rejected, authorized);
        commit(
            &directory,
            "candidate-v1.json",
            &authorized_bytes,
            &authorized_hash,
        )
        .unwrap();
        assert!(
            commit(
                &directory,
                "candidate-v1.json",
                &rejected_bytes,
                &rejected_hash
            )
            .is_err()
        );
        assert!(
            prepare(
                &directory,
                "candidate-v1.json",
                &report,
                None,
                false,
                || panic!("losing report cannot recapture")
            )
            .is_err()
        );
        let (replay, _, _) = prepare(
            &directory,
            "candidate-v1.json",
            &authorized_report,
            None,
            false,
            || panic!("committed candidate cannot recapture"),
        )
        .unwrap();
        assert_eq!(replay, authorized);
    }

    #[test]
    fn stored_publication_replay_and_commit_preserve_exact_original_encoded_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(root.path())
            .unwrap()
            .join("assignment");
        let directory = Directory::open(&path, true).unwrap();
        let report = report();
        let publication = CodingPublication {
            schema: 1,
            report: report.clone(),
            snapshot: snapshot(b"original"),
        };
        let original = serde_json::to_vec_pretty(&publication).unwrap();
        let hash = digest(&original);
        directory.publish("candidate-v1.json", &original).unwrap();
        let (replay, replay_hash, bytes) = prepare(
            &directory,
            "candidate-v1.json",
            &report,
            Some(&hash),
            false,
            || panic!("original publication must not recapture"),
        )
        .unwrap();
        assert_eq!(replay, publication);
        assert_eq!(replay_hash, hash);
        assert_eq!(bytes, original);
        commit(&directory, "candidate-v1.json", &bytes, &hash).unwrap();
        assert_eq!(directory.read("candidate-v1.json").unwrap(), original);
        assert!(commit(&directory, "other-candidate.json", &bytes, &"0".repeat(64)).is_err());
        assert!(!path.join("other-candidate.json").exists());
    }
}

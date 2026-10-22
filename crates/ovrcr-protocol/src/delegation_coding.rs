//! Bounded coding authority and immutable candidate evidence.
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodingContract {
    pub requested_base: String,
    pub branch: String,
    pub authorized_paths: Vec<String>,
    pub required_checks: Vec<String>,
}

pub fn validate_coding_paths(paths: &[String]) -> Result<(), &'static str> {
    if paths.is_empty() || paths.len() > 64 {
        return Err("coding requires 1..64 authorized repository paths");
    }
    for path in paths {
        if path == "." {
            continue;
        }
        if path.is_empty()
            || path.len() > 4096
            || path.chars().any(char::is_control)
            || !Path::new(path)
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err("authorized paths must be normalized repository-relative paths");
        }
    }
    Ok(())
}

impl CodingContract {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_coding_paths(&self.authorized_paths)?;
        for text in [&self.requested_base, &self.branch] {
            if text.is_empty() || text.len() > 256 || text.chars().any(char::is_control) {
                return Err("coding requires bounded base and branch selections");
            }
        }
        if self.branch.starts_with('-') {
            return Err("invalid coding branch");
        }
        if self.required_checks.is_empty()
            || self.required_checks.len() > 32
            || self.required_checks.iter().any(|name| {
                name.trim().is_empty() || name.len() > 256 || name.chars().any(char::is_control)
            })
            || self
                .required_checks
                .iter()
                .enumerate()
                .any(|(index, name)| self.required_checks[..index].contains(name))
        {
            return Err("coding requires 1..32 distinct named checks");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkspaceCreation {
    Prepared,
    Creating,
    CreatedUnregistered,
    Registered,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodingWorkspace {
    pub path: PathBuf,
    pub repository: PathBuf,
    pub base: String,
    pub git_identity: Option<String>,
    pub creation: WorkspaceCreation,
    /// Native launch proof only. A created Git workspace may still remain.
    pub no_process_started: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckOutcome {
    Passed,
    Failed,
    Missing,
    Unrun,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodingCheck {
    pub name: String,
    pub command: Option<String>,
    pub outcome: CheckOutcome,
    pub exit_code: Option<i32>,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateReference {
    #[serde(default)]
    pub input_baseline: Option<crate::InputBaselineReference>,
    pub path: PathBuf,
    pub digest: String,
    pub workspace: String,
    pub base: String,
    pub git_identity: String,
    pub changed_artifacts: Vec<String>,
}

// JSON omits only the new absent extension. Binary framing always carries it.
impl Serialize for CandidateReference {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let include = !serializer.is_human_readable() || self.input_baseline.is_some();
        let mut state =
            serializer.serialize_struct("CandidateReference", if include { 7 } else { 6 })?;
        if include {
            state.serialize_field("input_baseline", &self.input_baseline)?;
        }
        state.serialize_field("path", &self.path)?;
        state.serialize_field("digest", &self.digest)?;
        state.serialize_field("workspace", &self.workspace)?;
        state.serialize_field("base", &self.base)?;
        state.serialize_field("git_identity", &self.git_identity)?;
        state.serialize_field("changed_artifacts", &self.changed_artifacts)?;
        state.end()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodingSubmission {
    pub checks: Vec<CodingCheck>,
    #[serde(default)]
    pub candidate: Option<CandidateReference>,
}

impl CodingSubmission {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.checks.len() > 64 {
            return Err("too many coding checks");
        }
        for (index, check) in self.checks.iter().enumerate() {
            if check.name.trim().is_empty()
                || check.name.len() > 256
                || check.name.chars().any(char::is_control)
                || self.checks[..index]
                    .iter()
                    .any(|old| old.name == check.name)
                || check
                    .command
                    .as_ref()
                    .is_some_and(|command| command.trim().is_empty() || command.len() > 4096)
                || check.evidence.len() > 64
                || check.evidence.iter().any(|item| item.len() > 4096)
            {
                return Err("invalid or duplicate coding check evidence");
            }
            match check.outcome {
                CheckOutcome::Passed if check.exit_code != Some(0) || check.command.is_none() => {
                    return Err("passed check requires its actual command and zero exit code");
                }
                CheckOutcome::Failed
                    if check.exit_code.is_none_or(|code| code == 0) || check.command.is_none() =>
                {
                    return Err("failed check requires its actual command and nonzero exit code");
                }
                CheckOutcome::Missing | CheckOutcome::Unrun if check.exit_code.is_some() => {
                    return Err("missing or unrun check cannot claim an exit code");
                }
                _ => {}
            }
        }
        if let Some(candidate) = &self.candidate
            && (!candidate.path.is_absolute()
                || candidate.digest.len() != 64
                || !candidate
                    .digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || !matches!(candidate.base.len(), 40 | 64)
                || !candidate.base.bytes().all(|byte| byte.is_ascii_hexdigit())
                || candidate.workspace.is_empty()
                || candidate.git_identity.is_empty()
                || candidate.changed_artifacts.len() > 1024)
        {
            return Err("invalid immutable candidate reference");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coding_checks_preserve_missing_failed_and_unrun_outcomes() {
        let mut result = CodingSubmission {
            candidate: None,
            checks: vec![CodingCheck {
                name: "test".into(),
                command: Some("cargo test".into()),
                outcome: CheckOutcome::Failed,
                exit_code: Some(7),
                evidence: vec!["run.log".into()],
            }],
        };
        assert!(result.validate().is_ok());
        result.checks[0].outcome = CheckOutcome::Passed;
        assert!(result.validate().is_err());
        result.checks[0].exit_code = Some(0);
        assert!(result.validate().is_ok());
        result.checks[0].outcome = CheckOutcome::Unrun;
        assert!(result.validate().is_err());
        result.checks[0].exit_code = None;
        assert!(result.validate().is_ok());
        result.checks.push(result.checks[0].clone());
        assert!(result.validate().is_err());
    }
    #[test]
    fn research_and_coding_contract_submission_and_resources_roundtrip_on_wire() {
        use crate::{AssignmentResources, ResearchContract, ResearchSubmission};
        for coding in [false, true] {
            let mut contract = serde_json::json!({"schema":1,"workflow":"workflow","controller":"parent","task":"task","request":"request","project":"project","workspace":"workspace","scope":"bounded work","permitted_actions":["read"],"provider":"pi","model_provider":"test","model":"model","acceptance_evidence":["evidence"],"mandatory_capabilities":["read","result_file"]});
            if coding {
                contract["coding"] = serde_json::json!({"requested_base":"main","branch":"coding","authorized_paths":["src"],"required_checks":["test"]});
                contract["permitted_actions"] =
                    serde_json::json!(["read", "write", "edit", "check"]);
            }
            let contract: ResearchContract = serde_json::from_value(contract).unwrap();
            contract.validate().unwrap();
            let bytes =
                bincode::serde::encode_to_vec(&contract, bincode::config::standard()).unwrap();
            let (decoded, used): (ResearchContract, usize) =
                bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
            assert_eq!(decoded, contract);
            assert_eq!(used, bytes.len());
            let mut submission = serde_json::json!({"schema":1,"identity":{"instance":"instance","workflow":"workflow","controller":"parent","controller_epoch":1,"task":"task","attempt":1,"contract":1,"request":"request","session":2,"run":1},"version":1,"outcome":"failed","summary":"actual failed check","evidence":["run.log"],"unresolved":[]});
            if coding {
                submission["coding"] = serde_json::json!({"checks":[{"name":"test","command":"false","outcome":"Failed","exit_code":1,"evidence":["run.log"]}],"candidate":null});
            }
            let submission: ResearchSubmission = serde_json::from_value(submission).unwrap();
            submission.validate(&submission.identity).unwrap();
            let bytes =
                bincode::serde::encode_to_vec(&submission, bincode::config::standard()).unwrap();
            let (decoded, used): (ResearchSubmission, usize) =
                bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
            assert_eq!(decoded, submission);
            assert_eq!(used, bytes.len());
            let resources = AssignmentResources {
                project: Some("project".into()),
                workspace: Some("workspace".into()),
                workspace_present: coding,
                current_session: None,
                coding: coding.then(|| CodingWorkspace {
                    path: "/workspaces/workspace".into(),
                    repository: "/repo".into(),
                    base: "1".repeat(40),
                    git_identity: Some("generation".into()),
                    creation: WorkspaceCreation::Registered,
                    no_process_started: true,
                }),
            };
            let bytes =
                bincode::serde::encode_to_vec(&resources, bincode::config::standard()).unwrap();
            let (decoded, used): (AssignmentResources, usize) =
                bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
            assert_eq!(decoded, resources);
            assert_eq!(used, bytes.len());
        }
    }
}

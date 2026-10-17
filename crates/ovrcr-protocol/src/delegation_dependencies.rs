//! Parent-authored bounded task edges. This describes inputs, never schedules work.
use crate::{
    AcceptanceReference, AssignmentIdentity, CandidateReference, PublicationIdentity,
    PublicationKind, ReviewReference, ReviewRequirement,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputBaselineReference {
    pub path: PathBuf,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodingTargetReference {
    pub assignment: AssignmentIdentity,
    pub publication: PublicationIdentity,
    pub candidate: CandidateReference,
    pub source_contract_digest: String,
}

impl CodingTargetReference {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.assignment != self.publication.assignment
            || self.publication.kind != PublicationKind::Submission
            || self.publication.version == 0
            || !digest(&self.publication.digest)
            || !digest(&self.candidate.digest)
            || !digest(&self.source_contract_digest)
            || !self.candidate.path.is_absolute()
            || !matches!(self.candidate.base.len(), 40 | 64)
            || !self
                .candidate
                .base
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.assignment.session.is_none()
            || self.assignment.run.is_none()
            || self
                .candidate
                .input_baseline
                .as_ref()
                .is_some_and(|baseline| !baseline.path.is_absolute() || !digest(&baseline.digest))
        {
            return Err("invalid immutable coding target");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyTask {
    pub task: String,
    /// Canonical tracker/document reference; never confused with an internal task id.
    pub canonical_task: String,
    pub request: String,
    pub project: String,
    pub workspace: String,
    pub revision: u64,
    /// Explicit parent authority for compatible snapshot input materialization.
    pub integrate_snapshots: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedDependency {
    pub target: CodingTargetReference,
    pub acceptance: AcceptanceReference,
    pub reviews: Vec<ReviewReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyEdge {
    pub predecessor: String,
    pub successor: String,
    pub output: String,
    /// Exact required output paths. Inherited input is retained separately.
    pub artifacts: Vec<String>,
    pub required_reviews: Vec<ReviewRequirement>,
    pub selected: Option<AcceptedDependency>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyGraph {
    pub tasks: Vec<DependencyTask>,
    pub edges: Vec<DependencyEdge>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyState {
    pub version: u64,
    pub graph: DependencyGraph,
    pub invalidated_tasks: Vec<String>,
}

fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl DependencyGraph {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.tasks.len() > 64 || self.edges.len() > 256 {
            return Err("dependency graph exceeds 64 tasks or 256 edges");
        }
        for (index, task) in self.tasks.iter().enumerate() {
            if !id(&task.task)
                || !id(&task.request)
                || task.revision == 0
                || task.canonical_task.trim().is_empty()
                || task.canonical_task.len() > 4096
                || task.canonical_task.chars().any(char::is_control)
                || crate::validate_name(&task.project, "project").is_err()
                || crate::validate_name(&task.workspace, "workspace").is_err()
                || self.tasks[..index].iter().any(|old| {
                    old.task == task.task
                        || old.request == task.request
                        || old.canonical_task == task.canonical_task
                        || (old.project == task.project && old.workspace == task.workspace)
                })
            {
                return Err(
                    "dependency task has missing/duplicate identity or incompatible workspace ownership",
                );
            }
        }
        for (index, edge) in self.edges.iter().enumerate() {
            if edge.predecessor == edge.successor
                || !self.tasks.iter().any(|task| task.task == edge.predecessor)
                || !self.tasks.iter().any(|task| task.task == edge.successor)
                || edge.output.trim().is_empty()
                || edge.output.len() > 1024
                || edge.output.chars().any(char::is_control)
                || edge.required_reviews.len() > 2
                || self.edges[..index].iter().any(|old| {
                    old.predecessor == edge.predecessor
                        && old.successor == edge.successor
                        && old.output == edge.output
                })
                || crate::validate_coding_paths(&edge.artifacts).is_err()
                || edge.artifacts.iter().any(|path| path == ".")
            {
                return Err(
                    "dependency edge needs distinct existing tasks and named required outputs",
                );
            }
            let source = self
                .tasks
                .iter()
                .find(|task| task.task == edge.predecessor)
                .unwrap();
            let destination = self
                .tasks
                .iter()
                .find(|task| task.task == edge.successor)
                .unwrap();
            if source.project != destination.project {
                return Err("dependency workspaces must share their registered repository owner");
            }
            for (index, requirement) in edge.required_reviews.iter().enumerate() {
                if requirement.sources.is_empty()
                    || requirement.sources.len() > 32
                    || requirement
                        .sources
                        .iter()
                        .any(|source| source.validate().is_err())
                    || edge.required_reviews[..index]
                        .iter()
                        .any(|old| old.axis == requirement.axis)
                {
                    return Err("invalid required dependency review sources/axes");
                }
            }
            if let Some(selected) = &edge.selected {
                selected.target.validate()?;
                if selected.reviews.len() > 2 {
                    return Err("too many selected dependency reviews");
                }
            }
        }
        for task in &self.tasks {
            let mut pending = vec![task.task.as_str()];
            let mut visited = BTreeSet::new();
            while let Some(next) = pending.pop() {
                for edge in self.edges.iter().filter(|edge| edge.predecessor == next) {
                    if edge.successor == task.task {
                        return Err("dependency graph contains a cycle");
                    }
                    if visited.insert(edge.successor.as_str()) {
                        pending.push(&edge.successor);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn ancestors(&self, task: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::from([task.to_owned()]);
        let mut pending = vec![task.to_owned()];
        while let Some(next) = pending.pop() {
            for edge in self.edges.iter().filter(|edge| edge.successor == next) {
                if found.insert(edge.predecessor.clone()) {
                    pending.push(edge.predecessor.clone());
                }
            }
        }
        found
    }

    /// Only the changed task and descendants lose their input/review validity.
    pub fn affected(&self, previous: &Self) -> Vec<String> {
        let mut affected: BTreeSet<String> = self
            .tasks
            .iter()
            .filter(|task| !previous.tasks.contains(task))
            .chain(
                previous
                    .tasks
                    .iter()
                    .filter(|task| !self.tasks.contains(task)),
            )
            .map(|task| task.task.clone())
            .collect();
        for edge in self
            .edges
            .iter()
            .filter(|edge| !previous.edges.contains(edge))
            .chain(
                previous
                    .edges
                    .iter()
                    .filter(|edge| !self.edges.contains(edge)),
            )
        {
            affected.insert(edge.successor.clone());
        }
        let mut pending: Vec<_> = affected.iter().cloned().collect();
        while let Some(next) = pending.pop() {
            for edge in self
                .edges
                .iter()
                .chain(&previous.edges)
                .filter(|edge| edge.predecessor == next)
            {
                if affected.insert(edge.successor.clone()) {
                    pending.push(edge.successor.clone());
                }
            }
        }
        affected.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(name: &str) -> DependencyTask {
        DependencyTask {
            task: name.into(),
            canonical_task: format!("issue:{name}"),
            request: format!("request-{name}"),
            project: "project".into(),
            workspace: name.into(),
            revision: 1,
            integrate_snapshots: false,
        }
    }
    fn edge(from: &str, to: &str) -> DependencyEdge {
        DependencyEdge {
            predecessor: from.into(),
            successor: to.into(),
            output: "code".into(),
            artifacts: vec!["source".into()],
            required_reviews: vec![],
            selected: None,
        }
    }
    #[test]
    fn graph_rejects_cycles_missing_nodes_and_shared_workspaces() {
        let mut graph = DependencyGraph {
            tasks: vec![task("a"), task("b")],
            edges: vec![edge("a", "b")],
        };
        assert!(graph.validate().is_ok());
        graph.edges.push(edge("b", "a"));
        assert!(graph.validate().is_err());
        graph.edges.pop();
        graph.edges.push(edge("missing", "b"));
        assert!(graph.validate().is_err());
        graph.edges.pop();
        graph.tasks[1].workspace = "a".into();
        assert!(graph.validate().is_err());
    }
    #[test]
    fn removed_tasks_invalidate_previous_descendants_and_preserve_independent_branch() {
        let original = DependencyGraph {
            tasks: vec![task("a"), task("b"), task("c"), task("x"), task("y")],
            edges: vec![edge("a", "b"), edge("b", "c")],
        };
        let revised = DependencyGraph {
            tasks: vec![task("b"), task("c"), task("y")],
            edges: vec![edge("b", "c")],
        };
        assert!(original.validate().is_ok());
        assert!(revised.validate().is_ok());
        assert_eq!(revised.affected(&original), ["a", "b", "c", "x"]);
        assert_eq!(original.affected(&original), Vec::<String>::new());
    }
    #[test]
    fn contract_change_invalidates_descendants_and_preserves_independent_branch() {
        let original = DependencyGraph {
            tasks: vec![task("a"), task("b"), task("c"), task("x")],
            edges: vec![edge("a", "b"), edge("b", "c")],
        };
        let mut revised = original.clone();
        revised.tasks[0].revision += 1;
        assert_eq!(revised.affected(&original), ["a", "b", "c"]);
        assert_eq!(
            revised.ancestors("c"),
            BTreeSet::from(["a".into(), "b".into(), "c".into()])
        );
    }
}

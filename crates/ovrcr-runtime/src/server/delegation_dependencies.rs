//! Task edges under the existing workflow/assignment authority. No scheduler.
use super::{
    Record, ServerState, digest,
    followups::{read_version_submission, verify_acceptance},
    read_contract,
};
use crate::git::candidate::{
    CandidateFile,
    baseline::{self, InputBaseline},
};
use crate::server::{delegation_files::Directory, delegation_recovery::hash, lifecycle_error};
use anyhow::{Context, Result, bail};
use ovrcr_protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DependencyWitness {
    pub input_digest: String,
    pub baseline: Option<InputBaselineReference>,
}

fn input_digest(graph: &DependencyGraph, task: &str) -> Result<String> {
    let ancestors = graph.ancestors(task);
    let mut tasks: Vec<_> = graph
        .tasks
        .iter()
        .filter(|node| ancestors.contains(&node.task))
        .collect();
    tasks.sort_by_key(|node| &node.task);
    let mut edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| ancestors.contains(&edge.successor))
        .collect();
    edges.sort_by_key(|edge| (&edge.predecessor, &edge.successor, &edge.output));
    Ok(hash(&serde_json::to_vec(&(tasks, edges))?))
}

impl ServerState {
    pub(crate) fn set_dependencies(
        &self,
        auth: DelegationAuth,
        workflow: String,
        expected_version: u64,
        controller: String,
        graph: DependencyGraph,
    ) -> Result<WorkflowSnapshot> {
        graph.validate().map_err(anyhow::Error::msg)?;
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_assigned_input(auth.session)?;
        if !crate::server::delegation_recovery::identifier(&workflow)
            || !crate::server::delegation_recovery::identifier(&controller)
        {
            bail!("invalid dependency workflow identity");
        }
        let parent = self.delegation_auth(&auth)?;
        let store = self.retained.lock();
        let (binding, producer) = parent
            .authenticated_workflow_source(&auth)
            .context("dependency source is retired")?;
        let mut row = match store.workflow(&workflow)? {
            Some(row) => row,
            None => {
                if expected_version != 0
                    || store.workflow_controller(&workflow)?.is_some()
                    || store.workflows()?.total >= crate::server::delegation_recovery::MAX_WORKFLOWS
                {
                    bail!("workflow cannot acquire initial dependency authority");
                }
                let authority = WorkflowAuthority {
                    instance: store.delegation_instance()?,
                    workflow: workflow.clone(),
                    controller: controller.clone(),
                    epoch: 1,
                    parent: auth.session,
                    parent_run: parent.run(),
                    binding: binding.clone(),
                    producer: producer.clone(),
                    server_lifetime: self.server_lifetime.clone(),
                };
                if auth.controller.as_ref() != Some(&authority.fence()) {
                    bail!("new dependency workflow source fence changed");
                }
                crate::server::delegation_recovery::WorkflowRecord {
                    dependencies: None,
                    deliveries: vec![],
                    schema: 1,
                    authority,
                    departure: None,
                    admission: None,
                    reopen_run: None,
                    pointer: None,
                    transfers: vec![],
                    cancellation: None,
                }
            }
        };
        let authority = row.authority.clone();
        if authority.controller != controller {
            bail!("dependency logical controller changed");
        }
        row.require(
            &auth,
            parent.run(),
            &binding,
            &producer,
            &self.server_lifetime,
        )?;
        if row.authority != authority {
            bail!("dependency controller changed");
        }
        self.require_dispatch_open(&store, &workflow)?;
        if row.dependencies.as_ref().is_some_and(|saved| {
            saved.version == expected_version.saturating_add(1) && saved.graph == graph
        }) {
            drop(store);
            return self.workflow_snapshot(row, None);
        }
        if row.dependencies.as_ref().map_or(0, |saved| saved.version) != expected_version {
            bail!("dependency version changed; recover the original graph");
        }
        for task in &graph.tasks {
            if let Some(record) = store.assignment(&task.request)? {
                if record.identity.workflow != workflow
                    || record.identity.controller != authority.controller
                    || record.identity.task != task.task
                    || record.project != task.project
                    || record.workspace != task.workspace
                {
                    bail!("task request belongs to a foreign assignment/workspace");
                }
            }
        }
        let old = row.dependencies.as_ref().map(|saved| &saved.graph);
        if let Some(old) = old {
            for task in &graph.tasks {
                if let Some(prior) = old.tasks.iter().find(|prior| prior.task == task.task) {
                    if task != prior && task.revision <= prior.revision {
                        bail!("changed task contract requires a new task revision");
                    }
                }
            }
        }
        let affected = graph.affected(old.unwrap_or(&DependencyGraph::default()));
        let (members, gaps) = store.workflow_members(&workflow)?;
        if !affected.is_empty() && !gaps.is_empty() {
            bail!(
                "dependency invalidation has unverifiable workflow members; retain original graph"
            );
        }
        // Check every increment before opening a transaction or changing a durable row.
        let mut invalidated = Vec::new();
        for mut record in members {
            if affected.contains(&record.identity.task) {
                record.obligation_revision = record
                    .obligation_revision
                    .checked_add(1)
                    .context("dependency obligation revision exhausted")?;
                invalidated.push(record);
            }
        }
        row.dependencies = Some(DependencyState {
            version: expected_version
                .checked_add(1)
                .context("dependency version exhausted")?,
            graph,
            invalidated_tasks: affected,
        });
        parent.with_workflow_context(&auth, &row.authority, || {
            let transaction = store.connection.unchecked_transaction()?;
            for record in &invalidated {
                store.save_assignment(record)?;
            }
            store.save_workflow(&row)?;
            transaction.commit()?;
            Ok(())
        })?;
        drop(store);
        self.workflow_snapshot(row, None)
    }

    /// Read only original, authenticated files; a reference never grants read authority.
    pub(crate) fn authenticated_coding_target(
        &self,
        auth: &DelegationAuth,
        target: &CodingTargetReference,
    ) -> Result<(Record, ResearchContract, ResearchSubmission)> {
        target.validate().map_err(anyhow::Error::msg)?;
        let record = self.owned_assignment(auth, &target.assignment.request)?;
        let version = record.version();
        if version.identity != target.assignment
            || version.result_version != target.publication.version
            || version.contract_digest != target.source_contract_digest
            || version.result_digest.as_ref() != Some(&target.publication.digest)
        {
            bail!("accepted target is foreign or stale");
        }
        let result = read_version_submission(&record, &version)?
            .context("target has no authenticated result")?;
        if result
            .coding
            .as_ref()
            .and_then(|coding| coding.candidate.as_ref())
            != Some(&target.candidate)
        {
            bail!("target candidate reference changed");
        }
        let contract = read_contract(&record)?;
        Ok((record, contract, result))
    }

    fn verify_dependency_review(
        &self,
        target: &CodingTargetReference,
        required: &ReviewRequirement,
        reference: &ReviewReference,
    ) -> Result<()> {
        let expected = ReviewTarget {
            coding: target.clone(),
            sources: required.sources.clone(),
        };
        expected
            .validate(&required.axis)
            .map_err(anyhow::Error::msg)?;
        if reference.axis != required.axis
            || reference.target != expected
            || reference.reviewer != reference.publication.assignment
            || reference.publication.kind != PublicationKind::Submission
            || reference.reviewer.request == target.assignment.request
            || reference.reviewer.session == target.assignment.session
        {
            bail!("review is foreign, wrong-axis or not independent");
        }
        let reviewer = self
            .retained
            .lock()
            .assignment(&reference.reviewer.request)?
            .context("required reviewer assignment missing")?;
        if reviewer.identity.instance != target.assignment.instance
            || reviewer.identity.workflow != target.assignment.workflow
            || reviewer.identity.controller != target.assignment.controller
        {
            bail!("reviewer belongs to another workflow owner");
        }
        let version = reviewer.version();
        if version.identity != reference.reviewer
            || version.result_version != reference.publication.version
            || version.result_digest.as_ref() != Some(&reference.publication.digest)
        {
            bail!("required review publication is absent or stale");
        }
        let contract = read_contract(&reviewer)?;
        let assignment = contract
            .review
            .as_ref()
            .context("reviewer lacks an immutable review contract")?;
        if contract.coding.is_some()
            || contract.permitted_actions != ["read"]
            || assignment.axis != required.axis
            || assignment.target != expected
            || assignment.implementation != target.assignment.request
        {
            bail!(
                "reviewer's original contract does not authorize this exact independent axis/target"
            );
        }
        self.validate_review_contract(&contract)?;
        let submission = read_version_submission(&reviewer, &version)?
            .context("required review publication missing")?;
        let report = submission
            .review
            .context("authenticated reviewer did not publish typed review evidence")?;
        if report.target != expected
            || report.axis != required.axis
            || report.outcome != ReviewOutcome::Passed
            || report.findings.iter().any(|finding| finding.blocking)
            || submission.outcome != "success"
            || !submission.unresolved.is_empty()
        {
            bail!("required independent review has not passed");
        }
        report.validate().map_err(anyhow::Error::msg)
    }

    pub(in crate::server) fn validate_review_contract(
        &self,
        contract: &ResearchContract,
    ) -> Result<()> {
        if let Some(review) = &contract.review {
            review.validate().map_err(anyhow::Error::msg)?;
            for source in &review.source_documents {
                if digest(source.content.as_bytes()) != source.source.digest {
                    bail!("actual immutable review source digest changed");
                }
            }
        }
        Ok(())
    }

    fn dependency_graph(&self, workflow: &str) -> Result<Option<DependencyGraph>> {
        Ok(self
            .retained
            .lock()
            .workflow(workflow)?
            .and_then(|row| row.dependencies.map(|state| state.graph)))
    }

    /// Recognize only the ordinary retry owner's durable no-effect lineage.
    fn dependency_task_owns_contract(
        &self,
        task: &DependencyTask,
        contract: &ResearchContract,
        mut recorded: bool,
    ) -> Result<()> {
        if task.task != contract.task
            || task.project != contract.project
            || task.workspace != contract.workspace
        {
            bail!("assignment no longer owns its declared task/workspace");
        }
        let mut current = contract.clone();
        let mut visited = std::collections::BTreeSet::new();
        loop {
            if current.request == task.request {
                return Ok(());
            }
            if !visited.insert(current.request.clone())
                || visited.len() as u64 > super::MAX_DELEGATION_RECORDS
            {
                bail!("dependency retry lineage is cyclic or exceeds assignment capacity");
            }
            let previous = current
                .previous_request
                .as_ref()
                .context("assignment is not the declared request or its proven retry")?;
            let previous = self
                .retained
                .lock()
                .assignment(previous)?
                .context("dependency retry predecessor missing")?;
            let original = read_contract(&previous)?;
            let mut comparable = current.clone();
            comparable.request = original.request.clone();
            comparable.previous_request = original.previous_request.clone();
            if comparable != original
                || previous.identity.workflow != contract.workflow
                || previous.identity.controller != contract.controller
                || previous.identity.task != task.task
                || previous.project != task.project
                || previous.workspace != task.workspace
                || (recorded && previous.successor_request.as_ref() != Some(&current.request))
                || previous
                    .successor_request
                    .as_ref()
                    .is_some_and(|successor| successor != &current.request)
                || previous.dispatch != DispatchStatus::Refused
                || !(previous.retryable_capacity_refusal
                    || previous
                        .coding
                        .as_ref()
                        .is_some_and(|coding| coding.no_process_started))
                || previous.binding.is_some()
                || previous.result_version != 1
                || !previous.previous_versions.is_empty()
                || !previous.followups.is_empty()
                || !previous.control_intents.is_empty()
                || previous.control != AssignmentControl::default()
                || previous.publication_intent.is_some()
                || previous.result_digest.is_some()
                || previous.unresolved_input()
                || (previous.coding.is_none()
                    && (previous.identity.session.is_some() || previous.identity.run.is_some()))
                || !previous
                    .receipt(DispatchOperation::Launch)
                    .is_some_and(|receipt| receipt.outcome == DispatchOutcome::Rejected)
                || previous.spent_execution_attempts().is_none()
            {
                bail!(
                    "dependency retry lacks the original exact contract and authoritative no-effect receipt chain"
                );
            }
            current = original;
            recorded = true;
        }
    }

    fn dependency_task_owns_record(&self, task: &DependencyTask, record: &Record) -> Result<()> {
        if task.task != record.identity.task
            || task.project != record.project
            || task.workspace != record.workspace
        {
            bail!("dependency task identity/workspace changed");
        }
        if task.request == record.handle {
            return Ok(());
        }
        let contract = read_contract(record)?;
        if contract.request != record.handle {
            bail!("retry record and immutable contract request differ");
        }
        self.dependency_task_owns_contract(task, &contract, true)
    }

    /// Admission checks every selected predecessor and its applicable review contract.
    fn dependency_inputs(
        &self,
        auth: &DelegationAuth,
        contract: &ResearchContract,
    ) -> Result<Option<(String, Vec<CodingTargetReference>, Vec<CandidateFile>, bool)>> {
        let Some(graph) = self.dependency_graph(&contract.workflow)? else {
            return Ok(None);
        };
        graph.validate().map_err(anyhow::Error::msg)?;
        self.current_workflow_authority(auth, &contract.workflow)?;
        let Some(task) = graph.tasks.iter().find(|task| task.task == contract.task) else {
            if graph.tasks.iter().any(|task| {
                task.request == contract.request
                    || (task.project == contract.project && task.workspace == contract.workspace)
            }) {
                bail!("assignment conflicts with a declared dependency task owner");
            }
            return Ok(None);
        };
        self.dependency_task_owns_contract(task, contract, false)?;
        if let Some(previous) = &contract.previous_request {
            let previous = self
                .retained
                .lock()
                .assignment(previous)?
                .context("retry predecessor disappeared")?;
            self.require_dependency_fresh(&previous)?;
        }
        let mut lineage = Vec::new();
        let mut files: BTreeMap<String, CandidateFile> = BTreeMap::new();
        for edge in graph
            .edges
            .iter()
            .filter(|edge| edge.successor == task.task)
        {
            let selected = edge
                .selected
                .as_ref()
                .context("required predecessor output is not selected/accepted")?;
            let source = graph
                .tasks
                .iter()
                .find(|task| task.task == edge.predecessor)
                .context("predecessor node missing")?;
            let (record, source_contract, result) =
                self.authenticated_coding_target(auth, &selected.target)?;
            self.dependency_task_owns_record(source, &record)?;
            self.require_dependency_fresh(&record)?;
            let version = record.version();
            if version.acceptance.as_ref() != Some(&selected.acceptance)
                || selected.acceptance.target
                    != (AcceptanceTarget::ChildSubmission {
                        digest: selected.target.publication.digest.clone(),
                    })
            {
                bail!("exact predecessor acceptance is absent or changed");
            }
            verify_acceptance(&record, &version)?;
            let coding = source_contract
                .coding
                .as_ref()
                .context("required predecessor is not coding work")?;
            if result.outcome != "success"
                || !result.unresolved.is_empty()
                || coding.required_checks.iter().any(|name| {
                    !result
                        .coding
                        .as_ref()
                        .unwrap()
                        .checks
                        .iter()
                        .any(|check| &check.name == name && check.outcome == CheckOutcome::Passed)
                })
            {
                bail!("predecessor has missing/failed/unrun required acceptance evidence");
            }
            if selected.reviews.len() != edge.required_reviews.len() {
                bail!("required predecessor review evidence is missing");
            }
            for requirement in &edge.required_reviews {
                let reference = selected
                    .reviews
                    .iter()
                    .find(|review| review.axis == requirement.axis)
                    .context("required review axis is missing")?;
                self.verify_dependency_review(&selected.target, requirement, reference)?;
            }
            let inherited = Self::read_input_baseline(&record)?;
            let mut source_files: BTreeMap<String, CandidateFile> = BTreeMap::new();
            if let Some(inherited) = &inherited {
                for target in &inherited.selected {
                    if !lineage.contains(target) {
                        lineage.push(target.clone());
                    }
                }
                for file in &inherited.inputs {
                    source_files.insert(file.path.clone(), file.clone());
                }
            }
            let bytes = Directory::open(&record.root, false)?.read(&format!(
                "candidate-v{}.json",
                selected.target.publication.version
            ))?;
            let publication: super::super::delegation_coding::publication::CodingPublication =
                serde_json::from_slice(&bytes)?;
            for artifact in &edge.artifacts {
                let mut file = if let Some(file) = publication
                    .snapshot
                    .files
                    .iter()
                    .find(|file| &file.path == artifact)
                {
                    file.clone()
                } else if let Some(file) = inherited
                    .as_ref()
                    .and_then(|baseline| baseline.files.iter().find(|file| &file.path == artifact))
                {
                    CandidateFile {
                        path: file.path.clone(),
                        base_mode: file.base_mode,
                        base_content: file.base_content.clone(),
                        base_digest: file.base_digest.clone(),
                        mode: file.base_mode,
                        content: file.base_content.clone(),
                        digest: file.base_digest.clone(),
                    }
                } else if let Some(file) = inherited
                    .as_ref()
                    .and_then(|baseline| baseline.inputs.iter().find(|file| &file.path == artifact))
                {
                    file.clone()
                } else {
                    bail!("named required predecessor output is absent from complete candidate");
                };
                if let Some(original) = source_files.get(&file.path) {
                    file.base_mode = original.base_mode;
                    file.base_content = original.base_content.clone();
                    file.base_digest = original.base_digest.clone();
                }
                source_files.insert(file.path.clone(), file);
            }
            if !lineage.contains(&selected.target) {
                lineage.push(selected.target.clone());
            }
            for (path, file) in source_files {
                if files
                    .get(&path)
                    .is_some_and(|old| !baseline::same_postimage(old, &file))
                {
                    bail!("selected predecessor outputs conflict: {path}");
                }
                files.insert(path, file);
            }
        }
        Ok(Some((
            input_digest(&graph, &contract.task)?,
            lineage,
            files.into_values().collect(),
            task.integrate_snapshots,
        )))
    }

    pub(super) fn prepare_dependency_inputs(
        &self,
        auth: &DelegationAuth,
        contract: &ResearchContract,
        resource: Option<&CodingWorkspace>,
        root: &std::path::Path,
    ) -> Result<Option<DependencyWitness>> {
        self.validate_review_contract(contract)
            .map_err(|error| lifecycle_error(ErrorCode::InvalidRequest, format!("{error:#}")))?;
        let Some((input_digest, lineage, files, permitted)) = self
            .dependency_inputs(auth, contract)
            .map_err(|error| lifecycle_error(ErrorCode::Conflict, format!("{error:#}")))?
        else {
            return Ok(None);
        };
        let baseline = if lineage.is_empty() {
            None
        } else {
            let resource =
                resource.context("dependent inputs require an isolated coding workspace")?;
            let project = self
                .registry
                .lock()
                .unwrap()
                .project(&contract.project)?
                .clone();
            let baseline = baseline::plan(&project, &resource.base, lineage, files, permitted)
                .map_err(|error| lifecycle_error(ErrorCode::Conflict, format!("{error:#}")))?;
            let bytes = serde_json::to_vec(&baseline)?;
            let directory = Directory::open(root, true)?;
            let name = "input-baseline-v1.json";
            if let Err(error) = directory.publish(name, &bytes)
                && directory.read(name).ok().as_deref() != Some(bytes.as_slice())
            {
                return Err(error);
            }
            directory.sync()?;
            Some(InputBaselineReference {
                path: root.join(name),
                digest: digest(&bytes),
            })
        };
        Ok(Some(DependencyWitness {
            input_digest,
            baseline,
        }))
    }

    pub(in crate::server) fn require_dependency_fresh(&self, record: &Record) -> Result<()> {
        self.check_dependency_fresh(record)
            .map_err(|error| lifecycle_error(ErrorCode::Conflict, format!("{error:#}")))
    }

    fn check_dependency_fresh(&self, record: &Record) -> Result<()> {
        let Some(graph) = self.dependency_graph(&record.identity.workflow)? else {
            if record.dependencies.is_some() {
                bail!("dependency graph was removed; task inputs are stale");
            }
            return Ok(());
        };
        if let Some(task) = graph
            .tasks
            .iter()
            .find(|task| task.task == record.identity.task)
        {
            self.dependency_task_owns_record(task, record)?;
            let current = input_digest(&graph, &task.task)?;
            match &record.dependencies {
                Some(saved) if saved.input_digest == current => {}
                None if task.revision == 1
                    && !graph.edges.iter().any(|edge| edge.successor == task.task) => {}
                _ => bail!(
                    "dependency contract changed; affected input/base/review evidence is stale"
                ),
            }
        } else if record.dependencies.is_some() {
            bail!("dependency task was removed; inputs are stale");
        }
        let ancestors = graph.ancestors(&record.identity.task);
        for edge in graph
            .edges
            .iter()
            .filter(|edge| ancestors.contains(&edge.successor))
        {
            let selected = edge
                .selected
                .as_ref()
                .context("ancestor accepted input is not selected")?;
            let source = self
                .retained
                .lock()
                .assignment(&selected.target.assignment.request)?
                .context("selected predecessor disappeared")?;
            let node = graph
                .tasks
                .iter()
                .find(|node| node.task == edge.predecessor)
                .context("predecessor node disappeared")?;
            if source.identity != selected.target.assignment
                || source.result_version != selected.target.publication.version
                || source.result_digest.as_ref() != Some(&selected.target.publication.digest)
                || source.contract_digest != selected.target.source_contract_digest
                || source.acceptance.as_ref() != Some(&selected.acceptance)
                || selected.acceptance.target
                    != (AcceptanceTarget::ChildSubmission {
                        digest: selected.target.publication.digest.clone(),
                    })
            {
                bail!(
                    "ancestor candidate/contract/acceptance changed; descendant inputs and reviews are stale"
                );
            }
            self.dependency_task_owns_record(node, &source)?;
            let version = source.version();
            read_version_submission(&source, &version)?
                .context("selected predecessor result missing")?;
            verify_acceptance(&source, &version)?;
            for requirement in &edge.required_reviews {
                let reference = selected
                    .reviews
                    .iter()
                    .find(|reference| reference.axis == requirement.axis)
                    .context("required review missing")?;
                self.verify_dependency_review(&selected.target, requirement, reference)?;
            }
        }
        Ok(())
    }

    pub(in crate::server) fn read_input_baseline(record: &Record) -> Result<Option<InputBaseline>> {
        let Some(reference) = record
            .dependencies
            .as_ref()
            .and_then(|witness| witness.baseline.as_ref())
        else {
            return Ok(None);
        };
        if reference.path != record.root.join("input-baseline-v1.json") {
            bail!("input baseline escaped its original assignment");
        }
        let bytes = Directory::open(&record.root, false)?.read("input-baseline-v1.json")?;
        if digest(&bytes) != reference.digest {
            bail!("authenticated input baseline changed");
        }
        let baseline: InputBaseline = serde_json::from_slice(&bytes)?;
        baseline.validate()?;
        let resource = record
            .coding
            .as_ref()
            .context("baseline lacks original workspace provenance")?;
        if baseline.project != record.project
            || baseline.repository != resource.repository
            || baseline.anchor != resource.base
        {
            bail!("input baseline original provenance changed");
        }
        Ok(Some(baseline))
    }

    pub(in crate::server) fn materialize_dependency_inputs(&self, handle: &str) -> Result<()> {
        let record = self
            .retained
            .lock()
            .assignment(handle)?
            .context("assignment missing")?;
        self.require_dependency_fresh(&record)?;
        let Some(baseline) = Self::read_input_baseline(&record)? else {
            return Ok(());
        };
        let (project, workspace) = self.coding_workspace(&record)?;
        super::hold_before_effect("dependency-before-materialization", handle)?;
        baseline::materialize(&project, &workspace, &baseline)?;
        self.require_dependency_fresh(&record)
    }

    pub(super) fn verify_dependency_workspace(&self, record: &Record) -> Result<()> {
        self.require_dependency_fresh(record)?;
        if let Some(baseline) = Self::read_input_baseline(record)? {
            let (project, workspace) = self.coding_workspace(record)?;
            baseline::verify_workspace(&project, &workspace, &baseline)?;
        }
        Ok(())
    }
}

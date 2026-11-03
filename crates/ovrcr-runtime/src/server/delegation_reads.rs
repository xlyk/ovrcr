//! Assigned native research reads. The existing Server owns authority and files;
//! native hooks cannot expand the roots by rewriting MCP arguments.
use super::*;
use crate::server::delegation_files::ReadTree;
use std::path::Path;

impl ServerState {
    pub(crate) fn read_assignment(
        &self,
        auth: &DelegationAuth,
        read: &AssignedRead,
    ) -> Result<AssignedReadResult> {
        let record = self
            .retained
            .lock()
            .assignment(&read.handle)?
            .context("assignment not found")?;
        let child = self.assignment_publisher(auth, &record)?;
        let contract = read_contract(&record)?;
        if read.handle != record.handle
            || read.identity != record.identity
            || read.result_version != record.result_version
            || !matches!(
                record.dispatch,
                DispatchStatus::Applied | DispatchStatus::InputUnknown
            )
            || contract.provider != "claude"
            || contract.model_provider != "native"
            || contract.coding.is_some()
            || contract.review.is_some()
            || contract.permitted_actions != ["read"]
        {
            bail!("read requires the current assigned native Claude research contract");
        }
        control::require_controller(&record.control, read.control_epoch)?;
        let binding = record
            .binding
            .as_ref()
            .context("read assignment is not bound")?;
        let native_model = expected_native_model(&record)?;
        let workflow = {
            let store = self.retained.lock();
            self.require_dispatch_open(&store, &record.identity.workflow)?;
            let row = store
                .workflow(&record.identity.workflow)?
                .context("workflow authority missing")?;
            row.validate()?;
            if row.departure.is_some() || row.authority.server_lifetime != self.server_lifetime {
                bail!("read workflow authority is unavailable");
            }
            row.authority
        };
        let workspace = if matches!(&read.target, AssignedReadTarget::Workspace { .. }) {
            let registry = self.registry.lock().unwrap();
            let project = registry.project(&record.project)?.clone();
            let workspace = registry
                .workspace(&record.project, &record.workspace)?
                .clone();
            drop(registry);
            if workspace.git_identity.is_none() {
                bail!("read workspace has no recorded generation");
            }
            crate::git::verify_read_checkout(&project, &workspace)?;
            Some((project, workspace))
        } else {
            None
        };
        let mut tree = None;
        let bytes = match &read.target {
            AssignedReadTarget::Workspace { path } => {
                let (_, workspace) = workspace.as_ref().unwrap();
                let root = ReadTree::open(&workspace.path)?;
                let bytes = root.read(Path::new(path))?;
                tree = Some(root);
                bytes
            }
            target => {
                let (name, expected) = match target {
                    AssignedReadTarget::Brief => (
                        format!("brief-v{}.json", record.result_version),
                        record.brief_digest.as_ref(),
                    ),
                    AssignedReadTarget::Contract => (
                        format!("contract-v{}.json", record.identity.contract),
                        Some(&record.contract_digest),
                    ),
                    AssignedReadTarget::Submission => (
                        format!("submission-v{}.json", record.result_version),
                        record.result_digest.as_ref(),
                    ),
                    AssignedReadTarget::Answer { request } => {
                        let answer = record
                            .answers
                            .iter()
                            .find(|answer| &answer.request == request)
                            .context("scoped answer is unavailable")?;
                        if answer.control_epoch != read.control_epoch
                            || answer.expected_parent_epoch != workflow.epoch
                            || record.current_question().is_none_or(|question| {
                                question.reference != answer.question
                                    || Some(&question.binding) != record.binding.as_ref()
                            })
                        {
                            bail!("scoped answer belongs to stale question or authority");
                        }
                        (
                            format!("answer-{}.json", answer.request),
                            Some(&answer.digest),
                        )
                    }
                    AssignedReadTarget::Workspace { .. } => unreachable!(),
                };
                let expected = expected.context("current task artifact is unavailable")?;
                let bytes = Directory::open(&record.root, false)?.read(&name)?;
                if &digest(&bytes) != expected {
                    bail!("current task artifact changed");
                }
                bytes
            }
        };
        let result = AssignedReadResult::from_bytes(read, &bytes).map_err(anyhow::Error::msg)?;
        if let Some((project, workspace)) = &workspace {
            crate::git::verify_read_checkout(project, workspace)?;
            let registry = self.registry.lock().unwrap();
            let current = registry.workspace(&record.project, &record.workspace)?;
            if current.path != workspace.path
                || current.git_identity != workspace.git_identity
                || !tree.as_ref().unwrap().still_at(&workspace.path)?
            {
                bail!("read workspace root or generation changed");
            }
        }
        let store = self.retained.lock();
        let latest = store
            .assignment(&read.handle)?
            .context("read assignment disappeared")?;
        self.require_dispatch_open(&store, &record.identity.workflow)?;
        let authority = store
            .workflow(&record.identity.workflow)?
            .context("workflow authority disappeared")?;
        if authority.authority != workflow
            || authority.departure.is_some()
            || latest.identity != record.identity
            || latest.result_version != record.result_version
            || latest.control != record.control
            || latest.binding != record.binding
            || latest.contract_digest != record.contract_digest
            || latest.brief_digest != record.brief_digest
            || latest.result_digest != record.result_digest
            || latest.questions != record.questions
            || latest.answers != record.answers
            || latest.root != record.root
            || latest.project != record.project
            || latest.workspace != record.workspace
            || latest.review != record.review
            || latest.dispatch != record.dispatch
            || latest.native_model != record.native_model
            || latest.native_policy != record.native_policy
        {
            bail!("read authority or task version changed");
        }
        if expected_native_model(&latest)? != native_model {
            bail!("native read model or sealed policy changed during I/O");
        }
        child.with_delegation_binding(&auth.capability, binding, |agent| {
            if agent.health.state != ReporterHealth::Connected
                || agent
                    .metrics
                    .as_ref()
                    .and_then(|metrics| metrics.sample.model.as_deref())
                    != Some(native_model.as_str())
            {
                bail!("read requires the current native model and healthy binding");
            }
            Ok(result)
        })
    }
}

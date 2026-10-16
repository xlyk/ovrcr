//! Coding-specific evidence, under the existing assignment and workspace owners.
use super::delegation::{Record, digest};
use super::delegation_files::Directory;
use super::{ServerState, lifecycle_error};
use anyhow::{Context, Result, bail};
use ovrcr_protocol::*;
#[path = "delegation_coding/publication.rs"]
mod publication;
use publication::CodingPublication;

/// Borrowed selection from the existing validated VersionState/publication owner.
/// It carries historical provenance, never current parent authority.
pub(super) struct SelectedCodingVersion<'a> {
    pub identity: &'a AssignmentIdentity,
    pub result_version: u32,
    pub contract: &'a ResearchContract,
    pub contract_digest: &'a str,
    pub publication: Option<&'a PublicationIdentity>,
}

impl SelectedCodingVersion<'_> {
    fn validate(&self, record: &Record, directory: &Directory) -> Result<()> {
        let mut original = record.identity.clone();
        original.contract = self.identity.contract;
        if original != *self.identity || self.result_version == 0 {
            bail!("selected candidate version belongs to a different assignment");
        }
        let bytes = directory.read(&format!("contract-v{}.json", self.identity.contract))?;
        if digest(&bytes) != self.contract_digest
            || serde_json::from_slice::<ResearchContract>(&bytes)? != *self.contract
        {
            bail!("selected immutable coding contract changed");
        }
        if self.contract.project != record.project
            || self.contract.workspace != record.workspace
            || self.contract.workflow != self.identity.workflow
            || self.contract.task != self.identity.task
            || self.contract.controller != self.identity.controller
            || self.contract.request != self.identity.request
        {
            bail!("selected coding contract provenance changed");
        }
        self.contract.validate().map_err(anyhow::Error::msg)?;
        if let Some(publication) = self.publication
            && (publication.assignment != *self.identity
                || publication.version != self.result_version
                || publication.kind != PublicationKind::Submission)
        {
            bail!("selected authenticated publication belongs to a different result");
        }
        Ok(())
    }
    fn validate_submission(&self, submission: &ResearchSubmission) -> Result<()> {
        submission
            .validate(self.identity)
            .map_err(anyhow::Error::msg)?;
        if submission.version != self.result_version {
            bail!("candidate belongs to a different selected result version");
        }
        Ok(())
    }
}

impl ServerState {
    /// A matching path/base does not authorize a replacement registered repository.
    fn verify_coding_repository(project: &ProjectRecord) -> Result<()> {
        let root = project
            .workspaces
            .iter()
            .find(|workspace| super::is_root_workspace(project, workspace))
            .ok_or_else(|| {
                lifecycle_error(ErrorCode::Conflict, "registered repository root is missing")
            })?;
        let expected = root.git_identity.as_deref().ok_or_else(|| {
            lifecycle_error(
                ErrorCode::Conflict,
                "registered repository root Git identity is missing",
            )
        })?;
        let actual = crate::git::worktree_identity(&project.repo)
            .context("read registered repository root Git identity")?;
        if actual != expected {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "registered repository root Git identity changed; refuse coding effects",
            ));
        }
        Ok(())
    }

    /// Resolve the display ref once, before saving intent and before Git effects.
    pub(super) fn coding_workspace_intent(
        &self,
        contract: &ResearchContract,
    ) -> Result<Option<CodingWorkspace>> {
        let Some(coding) = &contract.coding else {
            return Ok(None);
        };
        crate::git::candidate::validate_scope(&coding.authorized_paths)
            .map_err(|error| lifecycle_error(ErrorCode::InvalidRequest, error.to_string()))?;
        ovrcr_protocol::validate_name(&contract.workspace, "workspace")
            .map_err(|error| lifecycle_error(ErrorCode::InvalidRequest, error.to_string()))?;
        let project = self
            .registry
            .lock()
            .unwrap()
            .project(&contract.project)?
            .clone();
        if project
            .workspaces
            .iter()
            .any(|workspace| workspace.id == contract.workspace)
        {
            return Err(lifecycle_error(
                ErrorCode::AlreadyExists,
                "coding requires a new isolated workspace ID; existing and root workspaces cannot be adopted",
            ));
        }
        let (repository, root) =
            crate::git::validate_project(&project.repo, &project.workspace_root)?;
        Self::verify_coding_repository(&project)?;
        let path = root.join(&contract.workspace);
        if std::fs::symlink_metadata(&path).is_ok() {
            return Err(lifecycle_error(
                ErrorCode::AlreadyExists,
                "coding workspace destination already exists; refuse adoption",
            ));
        }
        let base = crate::git::resolve_commit(&project.repo, &coding.requested_base)?;
        Ok(Some(CodingWorkspace {
            path,
            repository,
            base,
            git_identity: None,
            creation: WorkspaceCreation::Prepared,
            no_process_started: true,
        }))
    }

    /// An opaque requested ID never proves attribution: path and generation must match.
    pub(super) fn coding_workspace(
        &self,
        record: &Record,
    ) -> Result<(ProjectRecord, WorkspaceRecord)> {
        let coding = record
            .coding
            .as_ref()
            .context("coding workspace intent missing")?;
        if coding.creation != WorkspaceCreation::Registered {
            bail!("coding workspace creation is unresolved");
        }
        let registry = self.registry.lock().unwrap();
        let project = registry.project(&record.project)?.clone();
        let workspace = registry
            .workspace(&record.project, &record.workspace)?
            .clone();
        drop(registry);
        Self::verify_coding_repository(&project)?;
        if std::fs::canonicalize(&project.repo)? != coding.repository
            || workspace.path != coding.path
            || workspace.git_identity != coding.git_identity
        {
            bail!("coding workspace path or generation was replaced");
        }
        crate::git::inspect_worktree(&project, &workspace)?;
        Ok((project, workspace))
    }

    /// Recover only the exact previously saved generation, never an ID or branch match.
    pub(super) fn coding_retained_workspace(
        &self,
        handle: &str,
    ) -> Result<Option<WorkspaceRecord>> {
        let record = self
            .retained
            .lock()
            .assignment(handle)?
            .context("coding assignment missing")?;
        let coding = record.coding.as_ref().context("coding resource missing")?;
        let project = self
            .registry
            .lock()
            .unwrap()
            .project(&record.project)?
            .clone();
        let (repository, root) =
            crate::git::validate_project(&project.repo, &project.workspace_root)?;
        Self::verify_coding_repository(&project)?;
        if repository != coding.repository || root.join(&record.workspace) != coding.path {
            bail!(
                "coding repository or workspace root changed before effect; refuse creation or adoption"
            );
        }
        match coding.creation {
            WorkspaceCreation::Prepared | WorkspaceCreation::Creating => return Ok(None),
            WorkspaceCreation::CreatedUnregistered | WorkspaceCreation::Registered => {}
            WorkspaceCreation::Unresolved => bail!(
                "coding workspace effect is unresolved; refuse adoption or duplicate creation"
            ),
        }
        let workspace = WorkspaceRecord {
            id: record.workspace.clone(),
            path: coding.path.clone(),
            branch: crate::git::checkout_name(&coding.path)?,
            git_identity: coding.git_identity.clone(),
            setup_pending: false,
        };
        if workspace.git_identity.is_none() {
            bail!("coding generation was never captured");
        }
        crate::git::inspect_worktree(&project, &workspace)?;
        if crate::git::resolve_commit(&workspace.path, "HEAD")? != coding.base {
            bail!("prelaunch coding workspace no longer has its exact original base");
        }
        if coding.creation == WorkspaceCreation::Registered {
            let actual = self
                .registry
                .lock()
                .unwrap()
                .workspace(&record.project, &record.workspace)?
                .clone();
            if actual.path != workspace.path || actual.git_identity != workspace.git_identity {
                bail!("coding registration was replaced");
            }
            return Ok(Some(actual));
        }
        Ok(Some(workspace))
    }

    pub(super) fn record_coding_workspace(
        &self,
        handle: &str,
        workspace: &WorkspaceRecord,
        creation: WorkspaceCreation,
    ) -> Result<()> {
        let mut record = self
            .retained
            .lock()
            .assignment(handle)?
            .context("coding assignment missing")?;
        let resource = record.coding.as_mut().context("coding resource missing")?;
        if workspace.id != record.workspace
            || workspace.path != resource.path
            || resource
                .git_identity
                .as_ref()
                .is_some_and(|identity| Some(identity) != workspace.git_identity.as_ref())
            || crate::git::resolve_commit(&workspace.path, "HEAD")? != resource.base
        {
            bail!("created workspace differs from durable coding intent");
        }
        resource.git_identity = workspace.git_identity.clone();
        resource.creation = creation;
        resource.no_process_started = true;
        self.retained.lock().save_assignment(&record)
    }

    pub(super) fn coding_require_creation_open(&self, handle: &str) -> Result<()> {
        let store = self.retained.lock();
        let record = store
            .assignment(handle)?
            .context("coding assignment missing")?;
        self.require_dispatch_open(&store, &record.identity.workflow)?;
        let workflow = store
            .workflow(&record.identity.workflow)?
            .context("coding workflow missing")?;
        let parent = self.session_for_control(workflow.authority.parent)?;
        if workflow.departure.is_some()
            || workflow.authority.epoch != record.identity.controller_epoch
            || workflow.authority.server_lifetime != self.server_lifetime
            || workflow.authority.parent_run != parent.run()
            || parent.workflow_source()
                != Some((
                    workflow.authority.binding.clone(),
                    workflow.authority.producer.clone(),
                ))
        {
            bail!(
                "captured coding creation authority retired; retain resources without further effects"
            );
        }
        drop(store);
        // Existing lifecycle callers use this gate before Git, registration and spawn.
        let project = self
            .registry
            .lock()
            .unwrap()
            .project(&record.project)?
            .clone();
        Self::verify_coding_repository(&project)
    }

    pub(super) fn record_coding_spawn_possible(&self, handle: &str) -> Result<()> {
        let mut record = self
            .retained
            .lock()
            .assignment(handle)?
            .context("coding assignment missing")?;
        record
            .coding
            .as_mut()
            .context("coding resource missing")?
            .no_process_started = false;
        self.retained.lock().save_assignment(&record)
    }

    pub(super) fn coding_retry_resource(
        &self,
        previous: &Record,
    ) -> Result<Option<CodingWorkspace>> {
        let Some(resource) = &previous.coding else {
            return Ok(None);
        };
        if !resource.no_process_started
            || previous.binding.is_some()
            || previous
                .receipts
                .iter()
                .any(|receipt| receipt.operation == DispatchOperation::InitialInput)
        {
            bail!("coding retry requires authoritative no-process and no-input proof");
        }
        if resource.creation == WorkspaceCreation::Prepared && previous.retryable_capacity_refusal {
            if std::fs::symlink_metadata(&resource.path).is_ok() {
                bail!("unused coding destination appeared; refuse adoption");
            }
            return Ok(Some(resource.clone()));
        }
        let workspace = self
            .coding_retained_workspace(&previous.handle)?
            .context("retry has no proven created workspace")?;
        let project = self
            .registry
            .lock()
            .unwrap()
            .project(&previous.project)?
            .clone();
        if crate::git::inspect_worktree(&project, &workspace)?.dirty {
            bail!("coding retry workspace changed before launch");
        }
        Ok(Some(resource.clone()))
    }

    /// Caller authenticates the current child at the ordinary publication boundary.
    /// Git capture stays outside retained/Session locks; selected intent forbids recapture.
    pub(super) fn capture_coding_submission(
        &self,
        record: &Record,
        selected: &SelectedCodingVersion<'_>,
        submission: &mut ResearchSubmission,
    ) -> Result<Option<(String, Vec<u8>)>> {
        if selected.contract.coding.is_none() {
            if submission.coding.is_some() {
                bail!("research submissions cannot claim a coding candidate");
            }
            return Ok(None);
        }
        let directory = Directory::open(&record.root, false)?;
        selected.validate(record, &directory)?;
        selected.validate_submission(submission)?;
        if record.identity != *selected.identity || record.result_version != selected.result_version
        {
            bail!("historical candidate cannot be captured or rebound to current content");
        }
        let Some(coding) = &selected.contract.coding else {
            if submission.coding.is_some() {
                bail!("research submissions cannot claim a coding candidate");
            }
            return Ok(None);
        };
        if let Some(intent) = &record.publication_intent
            && (selected.publication != Some(&intent.identity)
                || record.binding.as_ref() != Some(&intent.binding))
        {
            bail!("coding recovery omitted or changed its authenticated publication intent");
        }
        if let Some(committed) = &record.result_digest
            && selected
                .publication
                .is_none_or(|publication| &publication.digest != committed)
        {
            bail!("coding recovery omitted or changed its committed result digest");
        }
        let file = format!("candidate-v{}.json", selected.result_version);
        let expected_candidate = if let Some(authenticated) = selected.publication {
            match directory.read(&format!("submission-v{}.json", selected.result_version)) {
                Ok(bytes) => {
                    if digest(&bytes) != authenticated.digest {
                        bail!("authenticated coding submission changed");
                    }
                    let saved: ResearchSubmission = serde_json::from_slice(&bytes)?;
                    selected.validate_submission(&saved)?;
                    Some(
                        saved
                            .coding
                            .context("authenticated coding evidence missing")?
                            .candidate
                            .context("authenticated candidate missing")?
                            .digest,
                    )
                }
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|cause| cause.kind() == std::io::ErrorKind::NotFound) =>
                {
                    None
                }
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        let (publication, hash, original_bytes) = publication::prepare(
            &directory,
            &file,
            submission,
            expected_candidate.as_deref(),
            selected.publication.is_none(),
            || {
                let (project, workspace) = self.coding_workspace(record)?;
                let resource = record.coding.as_ref().context("coding intent missing")?;
                let captured = crate::git::candidate::capture(
                    &project,
                    &workspace,
                    &resource.base,
                    &coding.authorized_paths,
                )?;
                Self::verify_coding_repository(&project)?;
                serde_json::from_slice(&captured).map_err(Into::into)
            },
        )?;
        Self::verify_coding_provenance(record, selected, &publication)?;
        // With an intent but no submission file, reconstruct only the original bytes
        // whose final result digest was authenticated; never read the checkout again.
        *submission = publication.submission(
            &coding.required_checks,
            record.root.join(&file),
            hash,
            selected.publication,
        )?;
        Ok(Some((file, original_bytes)))
    }

    pub(super) fn commit_coding_candidate(
        record: &Record,
        prepared: &(String, Vec<u8>),
        submission: &ResearchSubmission,
    ) -> Result<()> {
        let candidate = submission
            .coding
            .as_ref()
            .and_then(|coding| coding.candidate.as_ref())
            .context("prepared coding candidate reference missing")?;
        if candidate.path != record.root.join(&prepared.0) {
            bail!("prepared candidate path changed");
        }
        publication::commit(
            &Directory::open(&record.root, false)?,
            &prepared.0,
            &prepared.1,
            &candidate.digest,
        )
    }

    /// A durable original child intent permits exact-byte recovery, never capture.
    pub(super) fn recover_coding_result(
        record: &Record,
        authenticated: &PublicationIdentity,
    ) -> Result<Vec<u8>> {
        let directory = Directory::open(&record.root, false)?;
        let contract: ResearchContract = serde_json::from_slice(
            &directory.read(&format!("contract-v{}.json", record.identity.contract))?,
        )?;
        let selected = SelectedCodingVersion {
            identity: &record.identity,
            result_version: record.result_version,
            contract: &contract,
            contract_digest: &record.contract_digest,
            publication: Some(authenticated),
        };
        selected.validate(record, &directory)?;
        let file = format!("candidate-v{}.json", record.result_version);
        let bytes = directory.read(&file)?;
        let publication: CodingPublication = serde_json::from_slice(&bytes)?;
        Self::verify_coding_provenance(record, &selected, &publication)?;
        let coding = contract
            .coding
            .as_ref()
            .context("coding contract missing")?;
        let submission = publication.submission(
            &coding.required_checks,
            record.root.join(file),
            digest(&bytes),
            Some(authenticated),
        )?;
        Ok(serde_json::to_vec(&submission)?)
    }

    fn verify_coding_provenance(
        record: &Record,
        selected: &SelectedCodingVersion<'_>,
        publication: &CodingPublication,
    ) -> Result<()> {
        let coding = selected
            .contract
            .coding
            .as_ref()
            .context("selected coding contract missing")?;
        selected.validate_submission(&publication.report)?;
        let resource = record
            .coding
            .as_ref()
            .context("missing original coding workspace provenance")?;
        let snapshot = &publication.snapshot;
        snapshot.validate(&coding.authorized_paths)?;
        if publication.schema != 1
            || snapshot.project != record.project
            || snapshot.repository != resource.repository
            || snapshot.workspace != record.workspace
            || snapshot.base != resource.base
            || Some(&snapshot.git_identity) != resource.git_identity.as_ref()
        {
            bail!("immutable coding candidate provenance changed");
        }
        Ok(())
    }

    /// Selected historical contract/publication come from the existing version owner.
    /// This never consults the current contract, live checkout or replacement parent.
    pub(super) fn verify_coding_candidate(
        record: &Record,
        selected: &SelectedCodingVersion<'_>,
        submission: &ResearchSubmission,
    ) -> Result<()> {
        if selected.contract.coding.is_none() {
            if submission.coding.is_some() {
                bail!("research result cannot claim a coding candidate");
            }
            return Ok(());
        }
        let directory = Directory::open(&record.root, false)?;
        selected.validate(record, &directory)?;
        selected.validate_submission(submission)?;
        let Some(coding) = &selected.contract.coding else {
            if submission.coding.is_some() {
                bail!("unexpected coding evidence");
            }
            return Ok(());
        };
        let authenticated = selected
            .publication
            .context("selected coding result is not authenticated")?;
        let result_bytes =
            directory.read(&format!("submission-v{}.json", selected.result_version))?;
        if digest(&result_bytes) != authenticated.digest
            || serde_json::from_slice::<ResearchSubmission>(&result_bytes)? != *submission
        {
            bail!("selected authenticated coding submission changed");
        }
        let candidate = submission
            .coding
            .as_ref()
            .context("missing coding evidence")?
            .candidate
            .as_ref()
            .context("missing immutable coding candidate")?;
        let file = format!("candidate-v{}.json", selected.result_version);
        if candidate.path != record.root.join(&file) {
            bail!("selected coding candidate path changed");
        }
        let bytes = directory.read(&file)?;
        if digest(&bytes) != candidate.digest {
            bail!("immutable candidate changed or is partial");
        }
        let publication: CodingPublication = serde_json::from_slice(&bytes)?;
        Self::verify_coding_provenance(record, selected, &publication)?;
        if publication.submission(
            &coding.required_checks,
            candidate.path.clone(),
            candidate.digest.clone(),
            Some(authenticated),
        )? != *submission
        {
            bail!(
                "coding submission no longer matches its immutable original report and candidate"
            );
        }
        Ok(())
    }
}

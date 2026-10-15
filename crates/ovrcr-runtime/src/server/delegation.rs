//! Server-owned assignment control. Task payloads live only in private publications.
use super::delegation_files::Directory;
use super::{ServerState, lifecycle_error};
use anyhow::{Context, Result, bail};
use ovrcr_protocol::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub handle: String,
    pub identity: AssignmentIdentity,
    pub binding: Option<AgentBinding>,
    pub parent_binding: AgentBinding,
    pub parent: SessionId,
    pub parent_run: SessionRunId,
    pub root: PathBuf,
    pub contract_digest: String,
    pub brief_digest: Option<String>,
    pub result_digest: Option<String>,
    pub dispatch: DispatchStatus,
    pub reason: Option<String>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl ServerState {
    fn delegation_auth(
        &self,
        auth: &DelegationAuth,
    ) -> Result<std::sync::Arc<crate::session::Session>> {
        let session = self.session_for_control(auth.session)?;
        if !matches!(session.summary().kind, SessionKind::Agent { ref name } if name == "pi")
            || !session.authorizes_delegation(&auth.capability)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "delegation requires the current managed Pi capability",
            ));
        }
        Ok(session)
    }

    fn owned_assignment(&self, auth: &DelegationAuth, handle: &str) -> Result<Record> {
        let parent = self.delegation_auth(auth)?;
        let record = self
            .retained
            .lock()
            .assignment(handle)?
            .context("assignment not found")?;
        if record.parent != auth.session
            || record.parent_run != parent.run()
            || parent.summary().agent.as_ref().map(|agent| &agent.binding)
                != Some(&record.parent_binding)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "assignment controller changed",
            ));
        }
        Ok(record)
    }

    pub(crate) fn reject_assigned_input(&self, id: SessionId) -> Result<()> {
        if self.retained.lock().assigned(id)? {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "Pi owns this assignment; generic terminal input is disabled",
            ));
        }
        Ok(())
    }

    pub(crate) fn delegation(&self, request: DelegationRequest) -> Result<AssignmentSnapshot> {
        // Capability discovery is outside lifecycle serialization. A concurrent launch
        // still rechecks its durable status under the lock before creating anything.
        let availability = if let DelegationRequest::Launch { auth, handle } = &request {
            let record = self.owned_assignment(auth, handle)?;
            if record.dispatch == DispatchStatus::Prepared {
                Some(available_model(&read_contract(&record)?).is_ok())
            } else {
                None
            }
        } else {
            None
        };
        // Same serialization as session admission: validation, receipts and effects cannot
        // race another lifecycle mutation or a competing assignment input.
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        let record = match request {
            DelegationRequest::Prepare { auth, contract } => {
                contract
                    .validate()
                    .map_err(|reason| lifecycle_error(ErrorCode::InvalidRequest, reason))?;
                let parent = self.delegation_auth(&auth)?;
                self.reject_assigned_input(auth.session)?;
                let parent_binding = parent
                    .summary()
                    .agent
                    .filter(|agent| agent.health.state == ReporterHealth::Connected)
                    .map(|agent| agent.binding)
                    .ok_or_else(|| {
                        lifecycle_error(ErrorCode::Conflict, "parent Pi conversation is not bound")
                    })?;
                let handle = contract.request.clone();
                let bytes = serde_json::to_vec(&contract)?;
                let contract_digest = digest(&bytes);
                let store = self.retained.lock();
                if let Some(record) = store.assignment(&handle)? {
                    if record.parent != auth.session
                        || record.parent_run != parent.run()
                        || record.parent_binding != parent_binding
                        || record.contract_digest != contract_digest
                    {
                        return Err(lifecycle_error(
                            ErrorCode::Conflict,
                            "request identity already belongs to another contract or controller",
                        ));
                    }
                    record
                } else {
                    if let Some(previous) = store.workflow_controller(&contract.workflow)?
                        && (previous.identity.controller != contract.controller
                            || previous.parent != auth.session
                            || previous.parent_run != parent.run()
                            || previous.parent_binding != parent_binding)
                    {
                        return Err(lifecycle_error(
                            ErrorCode::Conflict,
                            "workflow already has a different controller",
                        ));
                    }
                    let root = std::fs::canonicalize(&self.registry_path)?
                        .join("workflows")
                        .join(&handle);
                    let directory = Directory::open(&root, true)?;
                    let record = Record {
                        handle,
                        identity: AssignmentIdentity {
                            instance: store.delegation_instance()?,
                            workflow: contract.workflow.clone(),
                            controller: contract.controller.clone(),
                            controller_epoch: 1,
                            task: contract.task.clone(),
                            attempt: 1,
                            contract: 1,
                            request: contract.request.clone(),
                            session: None,
                            run: None,
                        },
                        binding: None,
                        parent_binding,
                        parent: auth.session,
                        parent_run: parent.run(),
                        root,
                        contract_digest,
                        brief_digest: None,
                        result_digest: None,
                        dispatch: DispatchStatus::Prepared,
                        reason: None,
                    };
                    directory.publish("contract-v1.json", &bytes)?;
                    store.save_assignment(&record)?;
                    record
                }
            }
            DelegationRequest::Launch { auth, handle } => {
                let mut record = self.owned_assignment(&auth, &handle)?;
                if record.dispatch == DispatchStatus::Prepared {
                    let contract = read_contract(&record)?;
                    // The real provider inventory is checked before creating a session.
                    if availability != Some(true) {
                        record.dispatch = DispatchStatus::Refused;
                        record.reason =
                            Some("requested Pi/model unavailable; no substitute launched".into());
                        self.retained.lock().save_assignment(&record)?;
                    } else {
                        self.check_live_capacity()?;
                        record.dispatch = DispatchStatus::LaunchUnknown;
                        self.retained.lock().save_assignment(&record)?;
                        let argv = vec![
                            std::env::current_exe()?.into_os_string(),
                            "agent".into(),
                            "run".into(),
                            "pi".into(),
                            "--".into(),
                            "pi".into(),
                            "--no-approve".into(),
                            "--no-extensions".into(),
                            "--no-skills".into(),
                            "--tools".into(),
                            "read,grep,find,ls".into(),
                            "--provider".into(),
                            contract.model_provider.into(),
                            "--model".into(),
                            contract.model.into(),
                        ];
                        let result = self.create_session_locked(
                            CreateSessionRequest {
                                project: contract.project,
                                workspace: contract.workspace,
                                name: String::new(),
                                label: Some("Pi research".into()),
                                argv,
                                kind: SessionKind::Agent { name: "pi".into() },
                            },
                            None,
                            Some(&handle),
                        );
                        record = self
                            .retained
                            .lock()
                            .assignment(&handle)?
                            .context("assignment disappeared")?;
                        match result {
                            Ok(_) => record.dispatch = DispatchStatus::Launched,
                            Err(error) => {
                                record.reason = Some("launch did not complete; inspect retained session before taking further action".into());
                                if error.is::<crate::session::NoProcessStarted>()
                                    || record.identity.session.is_none()
                                {
                                    record.dispatch = DispatchStatus::Refused;
                                }
                            }
                        }
                        self.retained.lock().save_assignment(&record)?;
                    }
                }
                record
            }
            DelegationRequest::SendInitial {
                auth,
                handle,
                identity,
            } => {
                let mut record = self.owned_assignment(&auth, &handle)?;
                if record.identity != identity {
                    return Err(lifecycle_error(
                        ErrorCode::Conflict,
                        "assignment binding changed",
                    ));
                }
                if record.dispatch == DispatchStatus::Launched {
                    let session = self.assignment_session(&record)?;
                    let summary = session.summary();
                    let Some(agent) = summary.agent.as_ref().filter(|agent| {
                        agent.binding.provider == AgentProvider::Pi
                            && agent.health.state == ReporterHealth::Connected
                    }) else {
                        return self.assignment_snapshot(record);
                    };
                    let contract = read_contract(&record)?;
                    if agent
                        .metrics
                        .as_ref()
                        .and_then(|m| m.sample.model.as_deref())
                        != Some(contract.model.as_str())
                    {
                        return self.assignment_snapshot(record);
                    }
                    record.binding = Some(agent.binding.clone());
                    let bytes = Directory::open(&record.root, false)?.read("brief-v1.json")?;
                    if Some(digest(&bytes)) != record.brief_digest {
                        bail!("immutable brief changed");
                    }
                    record.dispatch = DispatchStatus::InputUnknown;
                    let parent = self.session_for_control(auth.session)?;
                    let store = self.retained.lock();
                    store.save_assignment(&record)?;
                    let text = format!(
                        "Perform the read-only research assignment in {}. Read that immutable brief with your read tool; publish the complete result with ovrcr_publish using handle {} and the exact identity in the brief. Do not modify task or parent records. A terminal success line is not a submission.",
                        record.root.join("brief-v1.json").display(),
                        record.handle
                    );
                    match session.send_delegated_text(crate::session::DelegatedInput {
                        parent: &parent,
                        parent_binding: &record.parent_binding,
                        capability: &auth.capability,
                        child_binding: record.binding.as_ref().unwrap(),
                        model: &contract.model,
                        text: &text,
                    }) {
                        Ok(true) => record.dispatch = DispatchStatus::Applied,
                        Ok(false) => {
                            record.dispatch = DispatchStatus::Refused;
                            record.reason = Some(
                                "controller or child binding changed before initial input".into(),
                            );
                        }
                        Err(_) => {
                            record.reason =
                                Some("initial input effect is unknown; do not replay".into())
                        }
                    }
                    store.save_assignment(&record)?;
                }
                record
            }
            DelegationRequest::Inspect { auth, handle }
            | DelegationRequest::Collect { auth, handle } => {
                self.owned_assignment(&auth, &handle)?
            }
            DelegationRequest::Publish {
                auth,
                handle,
                submission,
            } => {
                let mut record = self
                    .retained
                    .lock()
                    .assignment(&handle)?
                    .context("assignment not found")?;
                let session = self.delegation_auth(&auth)?;
                if Some(auth.session) != record.identity.session
                    || Some(session.run()) != record.identity.run
                {
                    return Err(lifecycle_error(
                        ErrorCode::Conflict,
                        "only the assigned child may publish a submission",
                    ));
                }
                self.assignment_session(&record)?;
                submission
                    .validate(&record.identity)
                    .map_err(anyhow::Error::msg)?;
                if record.dispatch != DispatchStatus::Applied
                    && record.dispatch != DispatchStatus::InputUnknown
                {
                    bail!("initial assignment has not been delivered");
                }
                let bytes = serde_json::to_vec(&submission)?;
                let hash = digest(&bytes);
                if record
                    .result_digest
                    .as_ref()
                    .is_some_and(|old| old != &hash)
                {
                    bail!("submission version is already published");
                }
                let binding = record
                    .binding
                    .clone()
                    .context("child assignment is not bound")?;
                let store = self.retained.lock();
                session.with_delegation_binding(&auth.capability, &binding, || {
                    let directory = Directory::open(&record.root, false)?;
                    if let Err(error) = directory.publish("submission-v1.json", &bytes)
                        && directory.read("submission-v1.json").ok().as_ref() != Some(&bytes)
                    {
                        return Err(error);
                    }
                    record.result_digest = Some(hash);
                    store.save_assignment(&record)
                })?;
                drop(store);
                record
            }
        };
        self.assignment_snapshot(record)
    }

    fn assignment_session(
        &self,
        record: &Record,
    ) -> Result<std::sync::Arc<crate::session::Session>> {
        let id = record.identity.session.context("assignment has no child")?;
        let session = self.session_for_control(id)?;
        if Some(session.run()) != record.identity.run {
            bail!("assigned run changed");
        }
        let summary = session.summary();
        if let Some(binding) = &record.binding
            && summary.agent.as_ref().map(|agent| &agent.binding) != Some(binding)
        {
            bail!("child conversation binding changed");
        }
        Ok(session)
    }

    // Called after row allocation and before spawn. The durable run identity here is
    // exactly the next begin_run identity; the shared admission lock fences both.
    pub(super) fn bind_assignment(
        &self,
        handle: &str,
        row: &crate::retained::RetainedSession,
    ) -> Result<()> {
        let mut record = self
            .retained
            .lock()
            .assignment(handle)?
            .context("assignment not found")?;
        record.identity.session = Some(row.id);
        record.identity.run = Some(SessionRunId(
            row.run.0.checked_add(1).context("run exhausted")?,
        ));
        self.retained.lock().save_assignment(&record)?;
        let contract = read_contract(&record)?;
        let brief = serde_json::to_vec_pretty(
            &serde_json::json!({"schema": 1, "handle": handle, "identity": record.identity, "contract": contract, "output": record.root.join("submission-v1.json"), "publication": "Call ovrcr_publish. Only this child can publish; it cannot accept the result. Research tools are read-only."}),
        )?;
        Directory::open(&record.root, false)?.publish("brief-v1.json", &brief)?;
        record.brief_digest = Some(digest(&brief));
        self.retained.lock().save_assignment(&record)?;
        Ok(())
    }

    fn assignment_snapshot(&self, record: Record) -> Result<AssignmentSnapshot> {
        let summary = record
            .identity
            .session
            .and_then(|id| self.session_summary(id));
        let submission = match &record.result_digest {
            Some(expected) => {
                if summary
                    .as_ref()
                    .is_none_or(|current| Some(current.run) != record.identity.run)
                {
                    bail!("submission belongs to a stale run");
                }
                if let Some(current) = summary
                    .as_ref()
                    .filter(|s| s.phase.is_live())
                    .and_then(|s| s.agent.as_ref())
                    && record.binding.as_ref() != Some(&current.binding)
                {
                    bail!("submission belongs to a changed conversation");
                }
                let bytes = Directory::open(&record.root, false)?.read("submission-v1.json")?;
                if &digest(&bytes) != expected {
                    bail!("submission publication changed or is partial");
                }
                let result: ResearchSubmission = serde_json::from_slice(&bytes)?;
                result
                    .validate(&record.identity)
                    .map_err(anyhow::Error::msg)?;
                Some(result)
            }
            None => None,
        };
        Ok(AssignmentSnapshot {
            handle: record.handle,
            identity: record.identity,
            dispatch: record.dispatch,
            reason: record.reason,
            activity: summary
                .as_ref()
                .map(|s| s.activity)
                .unwrap_or(AgentActivity::Unknown),
            conversation: summary
                .as_ref()
                .and_then(|s| s.agent.as_ref())
                .map(|a| a.binding.conversation.clone()),
            brief: record.root.join("brief-v1.json"),
            result_path: record.root.join("submission-v1.json"),
            submission,
            acceptance: "pending".into(),
        })
    }
}

fn read_contract(record: &Record) -> Result<ResearchContract> {
    let bytes = Directory::open(&record.root, false)?.read("contract-v1.json")?;
    if digest(&bytes) != record.contract_digest {
        bail!("immutable contract changed");
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn available_model(contract: &ResearchContract) -> Result<()> {
    // Provider inventory is local to the same inherited profile as the eventual child.
    // File-backed bounded output avoids a full pipe blocking the probe at exit.
    let mut output = tempfile::tempfile()?;
    let mut child = Command::new("pi")
        .args(["--no-extensions", "--no-skills", "--list-models"])
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("provider inventory failed");
            }
            break;
        }
        if Instant::now() >= deadline || output.metadata()?.len() > 512 * 1024 {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            bail!("provider inventory unavailable");
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    output.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    output.take(512 * 1024).read_to_string(&mut text)?;
    if !text.lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next() == Some(contract.model_provider.as_str())
            && fields.next() == Some(contract.model.as_str())
    }) {
        bail!("requested exact model is unavailable");
    }
    Ok(())
}

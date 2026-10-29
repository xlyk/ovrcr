//! Creation authority shares SessionStore's connection and retained -> Session fence.
use super::*;
use crate::server::delegation_files::Directory;
use ovrcr_protocol::{CloudCreationCommand, CloudCreationIntent, CloudCreationReceipt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SendRecord {
    pub key: String,
    pub digest: String,
    pub claimed: bool,
    pub run_id: Option<String>,
    pub terminal: Option<ovrcr_protocol::CloudRunStatus>,
    #[serde(default)]
    progress: Option<Progress>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Progress {
    origin: String,
    send: u64,
    observe: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub intent: CloudCreationIntent,
    pub run: SessionRunId,
    pub lifetime: String,
    pub invocation: String,
    pub claimed: bool,
    pub closed: bool,
    pub discarded: bool,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(default)]
    pub cancel_claimed: bool,
    pub receipt: Option<CloudCreationReceipt>,
    #[serde(default)]
    pub sends: Vec<SendRecord>,
    #[serde(default)]
    pub recovery_owner: Option<(SessionRunId, String, String)>,
    #[serde(default)]
    pub recovery_cancel_claimed: bool,
    #[serde(default)]
    pub controller_conflict: Option<SessionId>,
}
impl Record {
    pub fn validate(&self) -> Result<()> {
        self.intent.validate()?;
        if let Some(receipt) = &self.receipt {
            receipt.validate()?;
            if receipt.model != self.intent.account.model || !self.claimed {
                bail!("invalid retained creation receipt");
            }
        }
        let mut keys = std::collections::HashSet::new();
        let mut runs = std::collections::HashSet::new();
        if self.sends.len() > 128
            || (!self.sends.is_empty() && self.receipt.is_none())
            || self.sends.iter().any(|send| {
                send.key == self.intent.key
                    || !keys.insert(&send.key)
                    || ![&send.key, &send.digest].iter().all(|v| {
                        v.len() == 64
                            && v.bytes()
                                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                    })
                    || send.run_id.as_ref().is_some_and(|id| {
                        !send.claimed
                            || !runs.insert(id)
                            || id.is_empty()
                            || id.len() > 256
                            || !id
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
                    })
                    || (send.terminal.is_some() && send.run_id.is_none())
                    || send.progress.as_ref().is_some_and(|p| {
                        send.run_id.is_none()
                            || p.origin.len() != 64
                            || !p
                                .origin
                                .bytes()
                                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                            || (p.send == 0 && p.observe == 0)
                    })
            })
        {
            bail!("invalid retained Send authority");
        }
        if self.controller_conflict.is_some() && (!self.closed || self.receipt.is_none())
            || self.recovery_cancel_claimed
                && (self.recovery_owner.is_none() || !self.cancel_requested)
            || self
                .recovery_owner
                .as_ref()
                .is_some_and(|(run, invocation, lifetime)| {
                    run.0 <= self.run.0
                        || [invocation, lifetime].iter().any(|v| {
                            v.is_empty() || v.len() > 256 || v.chars().any(char::is_control)
                        })
                })
        {
            bail!("invalid retained recovery authority");
        }
        if (self.discarded || self.cancel_requested) && !self.closed
            || self.cancel_claimed && !self.cancel_requested
        {
            bail!("invalid creation closure");
        }
        Ok(())
    }
    pub fn recovery_target(&self) -> Option<ovrcr_protocol::CloudRecovery> {
        if self.discarded || self.controller_conflict.is_some() {
            return None;
        }
        let send = self.sends.last()?;
        Some(ovrcr_protocol::CloudRecovery {
            intent: self.intent.clone(),
            receipt: self.receipt.clone()?,
            key: send.key.clone(),
            run_id: send.run_id.clone()?,
            cancel_requested: self.cancel_requested,
        })
    }
    pub fn description(&self) -> String {
        if let Some(owner) = self.controller_conflict {
            return format!(
                "Cloud conversation already controlled by Session {}; second attachment rejected. Original request/receipt retained; explicit Discard available.",
                owner.0
            );
        }
        if self.cancel_requested && !self.discarded {
            return format!(
                "Cloud cancellation intent retained; delivery closed. {} Cancel acknowledgement means requested, not stopped. Remote work/charges may continue; after detach explicit Retry Cancel requires supported recovery. Saved unresolved content remains until cloud-discard.",
                if let Some(status) = self.sends.last().and_then(|s| s.terminal) {
                    format!("Authoritative exact-run outcome: {status:?}.")
                } else if self.sends.last().is_some_and(|s| s.run_id.is_some()) {
                    "Exact run retained; remote cancellation unconfirmed.".into()
                } else if self.sends.last().is_some_and(|s| s.claimed)
                    || self.claimed && self.receipt.is_none()
                {
                    "Original attempt may have escaped; remote cancellation unconfirmed.".into()
                } else {
                    "Local delivery stopped before dispatch; no remote-run cancellation confirmed."
                        .into()
                }
            );
        }
        if !self.discarded
            && let Some(send) = self.sends.last()
        {
            return format!(
                "Cloud Send {}. Remote work may continue; Retry and active reattachment unavailable. Explicit local Discard: ovrcr terminal cloud-discard <id>.",
                if send.terminal.is_some() {
                    "terminal receipt retained"
                } else if send.run_id.is_some() {
                    "exact run retained; observation unknown after disconnect"
                } else if send.claimed {
                    "unresolved, possibly escaped"
                } else {
                    "prepared, never dispatched"
                }
            );
        }
        let status = if self.discarded {
            "discarded locally; any escaped effect remains unknown"
        } else if self.receipt.is_some() {
            "handle acknowledged; handle restoration unavailable after bridge exit"
        } else if self.claimed {
            "unresolved, possibly escaped; automatic creation and Retry unavailable"
        } else {
            "prepared, never dispatched; automatic creation unavailable"
        };
        format!(
            "Cloud creation {status}. Explicit local Discard: ovrcr terminal cloud-discard <id>."
        )
    }
    fn requires_discard(&self) -> bool {
        !self.discarded
    }
}
pub(crate) fn create_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS cloud_creations (session INTEGER PRIMARY KEY REFERENCES retained_sessions(id) ON DELETE CASCADE, metadata TEXT NOT NULL); CREATE TABLE IF NOT EXISTS cloud_creation_fences(session INTEGER PRIMARY KEY REFERENCES retained_sessions(id) ON DELETE CASCADE, run INTEGER NOT NULL);")?;
    Ok(())
}
fn creation_directory(config: &Path, create: bool) -> Result<Directory> {
    // Resolve the selected instance once (macOS /var aliases /private/var), then
    // open every private publication component without following links.
    let root = config.canonicalize()?.join("cloud-creations");
    let directory = Directory::open(&root, create)?;
    Ok(directory)
}
fn cleanup_creation_aliases(config: &Path, directory: &Directory) -> Result<()> {
    let root = config.canonicalize()?.join("cloud-creations");
    for entry in std::fs::read_dir(&root)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.starts_with('.')
            && name.len() == 65
            && name[1..].bytes().all(|c| c.is_ascii_hexdigit())
        {
            directory.remove(&name)?;
        }
    }
    Ok(())
}
impl SessionStore {
    pub(crate) fn cleanup_creation_payloads(&self, config: &Path) -> Result<()> {
        if !config.join("cloud-creations").try_exists()? {
            return Ok(());
        }
        let directory = creation_directory(config, false)?;
        cleanup_creation_aliases(config, &directory)?;
        for creation in self
            .records
            .values()
            .filter_map(|r| r.cloud_creation.as_ref())
        {
            for send in &creation.sends {
                if send.run_id.is_some() || creation.discarded {
                    directory.remove(&send.key)?;
                }
            }
            if creation.receipt.is_some() || creation.discarded {
                directory.remove(&creation.intent.key)?;
            }
        }
        Ok(())
    }

    fn save_creation(&mut self, id: SessionId, record: Record) -> Result<()> {
        record.validate()?;
        self.connection.execute("INSERT INTO cloud_creations(session,metadata) VALUES(?1,?2) ON CONFLICT(session) DO UPDATE SET metadata=excluded.metadata", params![sql_integer(id.0)?,serde_json::to_string(&record)?])?;
        self.records
            .get_mut(&id)
            .context("original creation session missing")?
            .cloud_creation = Some(record);
        Ok(())
    }
    pub(crate) fn creation_command(
        &mut self,
        id: SessionId,
        run: SessionRunId,
        invocation: &str,
        lifetime: &str,
        config: &Path,
        command: &CloudCreationCommand,
    ) -> Result<()> {
        let session = self
            .records
            .get(&id)
            .context("original creation session missing")?;
        if session.run != run || session.disposition == Disposition::Archived {
            bail!("creation session ownership changed");
        }
        let existing = session.cloud_creation.clone();
        if let CloudCreationCommand::Recover(target)
        | CloudCreationCommand::Reconcile { target, .. }
        | CloudCreationCommand::RetryCancel(target) = command
        {
            let mut record = existing.context("original recovery operation missing")?;
            let mut expected = record.recovery_target().context("recovery unavailable")?;
            // Intent may gain cancellation after target admission; identity never changes.
            expected.cancel_requested = target.cancel_requested;
            if &expected != target {
                bail!("recovery account/source/exact-run binding changed");
            }
            let owner = (run, invocation.to_owned(), lifetime.to_owned());
            if matches!(command, CloudCreationCommand::Recover(_)) {
                if record.run == run
                    || record
                        .recovery_owner
                        .as_ref()
                        .is_some_and(|o| o.0 == run && o != &owner)
                {
                    bail!("cloud recovery already controlled by this Session");
                }
                if record.recovery_owner.as_ref() != Some(&owner) {
                    record.recovery_cancel_claimed = false;
                }
                record.recovery_owner = Some(owner);
            } else {
                if record.recovery_owner.as_ref() != Some(&owner) {
                    bail!("stale recovery controller");
                }
                if matches!(command, CloudCreationCommand::RetryCancel(_)) {
                    let closed: bool = self.connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM cloud_creation_fences WHERE session=?1 AND run=?2)",
                        params![sql_integer(id.0)?, sql_integer(run.0)?], |row| row.get(0))?;
                    if closed {
                        bail!("recovery attachment closed; no cancellation dispatch");
                    }
                    if record.recovery_cancel_claimed
                        || record.sends.last().is_none_or(|s| s.terminal.is_some())
                    {
                        bail!(
                            "exact recovery cancellation already claimed or terminal; no automatic retry"
                        );
                    }
                    record.closed = true;
                    record.cancel_requested = true;
                    record.recovery_cancel_claimed = true;
                }
                if let CloudCreationCommand::Reconcile { status, .. } = command {
                    let send = record.sends.last_mut().context("exact run missing")?;
                    if send.terminal.is_some_and(|old| old != *status) {
                        bail!("authoritative terminal outcome changed");
                    }
                    send.terminal = Some(*status);
                }
            }
            return self.save_creation(id, record);
        }
        let fenced: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_creation_fences WHERE session=?1 AND run=?2)",
            params![sql_integer(id.0)?, sql_integer(run.0)?],
            |row| row.get(0),
        )?;
        if fenced
            && !matches!(
                command,
                CloudCreationCommand::Acknowledge { .. }
                    | CloudCreationCommand::AcknowledgeSend { .. }
                    | CloudCreationCommand::Cancel
                    | CloudCreationCommand::ClaimCancel { .. }
                    | CloudCreationCommand::CheckCancel { .. }
                    | CloudCreationCommand::ObserveSend { .. }
                    | CloudCreationCommand::ObservationLost { .. }
                    | CloudCreationCommand::CompleteSend { .. }
                    | CloudCreationCommand::ObservationProgress { .. }
            )
        {
            bail!("cloud creation delivery is closed");
        }

        if matches!(
            command,
            CloudCreationCommand::ClaimCancel { .. } | CloudCreationCommand::CheckCancel { .. }
        ) {
            match command {
                CloudCreationCommand::ClaimCancel {
                    key,
                    agent_id,
                    run_id,
                }
                | CloudCreationCommand::CheckCancel {
                    key,
                    agent_id,
                    run_id,
                } => {
                    let mut record = existing.clone().context("no retained cloud operation")?;
                    if record.run != run
                        || record.invocation != invocation
                        || record.lifetime != lifetime
                        || !record.cancel_requested
                        || record.discarded
                        || record.cancel_claimed
                        || record
                            .receipt
                            .as_ref()
                            .is_none_or(|r| &r.agent_id != agent_id)
                        || !record.sends.last().is_some_and(|s| {
                            &s.key == key
                                && s.run_id.as_ref() == Some(run_id)
                                && s.terminal.is_none()
                        })
                    {
                        bail!("exact live cancellation unavailable; no automatic retry");
                    }
                    if matches!(command, CloudCreationCommand::ClaimCancel { .. }) {
                        record.cancel_claimed = true;
                        self.save_creation(id, record)?;
                    }
                }
                _ => unreachable!(),
            }
            return Ok(());
        }
        let directory = creation_directory(config, true)?;
        match command {
            CloudCreationCommand::Recover(_)
            | CloudCreationCommand::Reconcile { .. }
            | CloudCreationCommand::RetryCancel(_) => unreachable!(),
            CloudCreationCommand::Cancel => {
                let record = existing.as_ref().context("no retained cloud operation")?;
                if record.run != run
                    || record.invocation != invocation
                    || record.lifetime != lifetime
                {
                    bail!("stale cancellation owner");
                }
                self.cancel_creation(id, run)?;
            }
            CloudCreationCommand::ClaimCancel { .. } | CloudCreationCommand::CheckCancel { .. } => {
                unreachable!()
            }
            CloudCreationCommand::Prepare(intent) => {
                intent.validate()?;
                if existing.is_some() {
                    bail!("original cloud creation already retained; no replacement or retry");
                }
                let record = Record {
                    intent: intent.clone(),
                    run,
                    invocation: invocation.into(),
                    lifetime: lifetime.into(),
                    claimed: false,
                    closed: false,
                    discarded: false,
                    cancel_requested: false,
                    cancel_claimed: false,
                    receipt: None,
                    sends: Vec::new(),
                    recovery_owner: None,
                    recovery_cancel_claimed: false,
                    controller_conflict: None,
                };
                self.save_creation(id, record)?;
                directory.publish(&intent.key, &serde_json::to_vec(&intent.request())?)?;
            }
            CloudCreationCommand::PrepareSend(intent) => {
                intent.validate()?;
                let mut record =
                    existing.context("durable creation receipt required before Send")?;
                let receipt = record
                    .receipt
                    .as_ref()
                    .context("durable creation receipt required before Send")?;
                if record.closed
                    || record.run != run
                    || record.invocation != invocation
                    || record.lifetime != lifetime
                    || intent.creation_key != record.intent.key
                    || intent.agent_id != receipt.agent_id
                    || intent.credential_generation != record.intent.credential_generation
                    || intent.model != record.intent.account.model
                    || record.sends.len() >= 128
                    || record.sends.iter().any(|s| s.key == intent.key)
                    || record.sends.last().is_some_and(|s| s.terminal.is_none())
                {
                    bail!(
                        "Send busy, closed or original account/options/identity changed; retain prompt for manual resubmission"
                    );
                }
                let bytes = serde_json::to_vec(&intent.request())?;
                record.sends.push(SendRecord {
                    key: intent.key.clone(),
                    digest: format!("{:x}", Sha256::digest(&bytes)),
                    claimed: false,
                    run_id: None,
                    terminal: None,
                    progress: None,
                });
                self.save_creation(id, record)?;
                directory.publish(&intent.key, &bytes)?;
            }
            CloudCreationCommand::ClaimSend { key, .. }
            | CloudCreationCommand::AcknowledgeSend { key, .. }
            | CloudCreationCommand::CompleteSend { key, .. }
            | CloudCreationCommand::ObservationLost { key, .. }
            | CloudCreationCommand::ObserveSend { key, .. }
            | CloudCreationCommand::ObservationProgress { key, .. } => {
                let mut record = existing.context("original Send unavailable")?;
                if record.run != run
                    || record.invocation != invocation
                    || record.lifetime != lifetime
                {
                    bail!("stale Send owner");
                }
                let agent = &record
                    .receipt
                    .as_ref()
                    .context("creation receipt missing")?
                    .agent_id;
                let index = record
                    .sends
                    .iter()
                    .position(|s| &s.key == key)
                    .context("original Send unavailable")?;
                match command {
                    CloudCreationCommand::ClaimSend {
                        credential_generation,
                        ..
                    } => {
                        if record.closed
                            || record.sends[index].claimed
                            || credential_generation != &record.intent.credential_generation
                        {
                            bail!("Send closed, already claimed or credential changed; no retry");
                        }
                        if format!("{:x}", Sha256::digest(directory.read(key)?))
                            != record.sends[index].digest
                        {
                            bail!("immutable Send payload corrupt; no dispatch");
                        }
                        record.sends[index].claimed = true;
                    }
                    CloudCreationCommand::AcknowledgeSend {
                        agent_id, run_id, ..
                    } => {
                        CloudCreationReceipt {
                            agent_id: run_id.clone(),
                            model: record.intent.account.model.clone(),
                        }
                        .validate()?;
                        if agent_id != agent
                            || !record.sends[index].claimed
                            || record.sends[index]
                                .run_id
                                .as_ref()
                                .is_some_and(|r| r != run_id)
                            || record
                                .sends
                                .iter()
                                .enumerate()
                                .any(|(i, s)| i != index && s.run_id.as_ref() == Some(run_id))
                        {
                            bail!("exact Send receipt identity mismatch");
                        }
                        record.sends[index].run_id = Some(run_id.clone());
                    }
                    CloudCreationCommand::ObservationLost {
                        agent_id, run_id, ..
                    }
                    | CloudCreationCommand::ObserveSend {
                        agent_id, run_id, ..
                    } => {
                        if (record.closed && (!record.cancel_requested || record.discarded))
                            || agent_id != agent
                            || record.sends[index].run_id.as_ref() != Some(run_id)
                        {
                            bail!("observation identity mismatch or closed");
                        }
                    }
                    CloudCreationCommand::ObservationProgress {
                        agent_id,
                        run_id,
                        origin,
                        observe,
                        offset,
                        ..
                    } => {
                        if (record.closed && (!record.cancel_requested || record.discarded))
                            || agent_id != agent
                            || record.sends[index].run_id.as_ref() != Some(run_id)
                            || record.sends[index].terminal.is_some()
                            || *offset == 0
                        {
                            bail!("observation progress identity mismatch or closed");
                        }
                        let progress =
                            record.sends[index]
                                .progress
                                .get_or_insert_with(|| Progress {
                                    origin: origin.clone(),
                                    send: 0,
                                    observe: 0,
                                });
                        if &progress.origin != origin {
                            bail!("observation stream origin changed");
                        }
                        let previous = if *observe {
                            &mut progress.observe
                        } else {
                            &mut progress.send
                        };
                        if *offset <= *previous || (*observe && *offset != *previous + 1) {
                            bail!(
                                "observation progress must advance exclusively without Observe gaps"
                            );
                        }
                        *previous = *offset;
                    }
                    CloudCreationCommand::CompleteSend {
                        agent_id,
                        run_id,
                        status,
                        ..
                    } => {
                        if (record.closed && (!record.cancel_requested || record.discarded))
                            || agent_id != agent
                            || record.sends[index].run_id.as_ref() != Some(run_id)
                            || record.sends[index].terminal.is_some_and(|s| s != *status)
                        {
                            bail!("terminal Send identity mismatch or closed");
                        }
                        record.sends[index].terminal = Some(*status);
                    }
                    _ => unreachable!(),
                }
                let retire = record.sends[index].run_id.is_some();
                self.save_creation(id, record)?;
                if retire {
                    directory.remove(key)?;
                }
            }
            CloudCreationCommand::Claim { key } | CloudCreationCommand::Acknowledge { key, .. } => {
                let mut record = existing.context("original cloud creation unavailable")?;
                if record.run != run
                    || record.invocation != invocation
                    || record.lifetime != lifetime
                    || &record.intent.key != key
                {
                    bail!("stale cloud creation owner");
                }
                match command {
                    CloudCreationCommand::Claim { .. } => {
                        if record.closed || record.claimed {
                            bail!("cloud creation closed or already claimed; do not retry");
                        }
                        let bytes = directory.read(key)?;
                        if bytes != serde_json::to_vec(&record.intent.request())? {
                            bail!("original creation payload corrupt; no dispatch");
                        }
                        record.claimed = true;
                        self.save_creation(id, record)?;
                    }
                    CloudCreationCommand::Acknowledge { receipt, .. } => {
                        receipt.validate()?;
                        if !record.claimed
                            || receipt.model != record.intent.account.model
                            || record.receipt.as_ref().is_some_and(|r| r != receipt)
                        {
                            bail!("creation receipt identity mismatch");
                        }
                        if let Some(owner) = self
                            .records
                            .values()
                            .find(|row| {
                                row.id != id
                                    && row
                                        .cloud_creation
                                        .as_ref()
                                        .and_then(|r| r.receipt.as_ref())
                                        .is_some_and(|r| r.agent_id == receipt.agent_id)
                            })
                            .map(|r| r.id)
                        {
                            record.receipt = Some(receipt.clone());
                            record.controller_conflict = Some(owner);
                            record.closed = true;
                            self.save_creation(id, record)?;
                            directory.remove(key)?;
                            bail!(
                                "cloud conversation already controlled by Session {}",
                                owner.0
                            );
                        }
                        record.receipt = Some(receipt.clone());
                        self.save_creation(id, record)?;
                        directory.remove(key)?;
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok(())
    }
    pub(crate) fn cancel_creation(&mut self, id: SessionId, run: SessionRunId) -> Result<()> {
        let session = self.records.get(&id).context("session missing")?;
        if session.run != run {
            bail!("cancellation owner changed");
        }
        let mut record = session
            .cloud_creation
            .clone()
            .context("no retained cloud operation")?;
        if record.discarded {
            bail!("operation already discarded");
        }
        record.closed = true;
        record.cancel_requested = true;
        // One SQLite statement commits both intent and closure before acknowledgement/RPC.
        self.save_creation(id, record)
    }
    pub(crate) fn close_creation(&mut self, id: SessionId, run: SessionRunId) -> Result<()> {
        let session = self.records.get(&id).context("session missing")?;
        if session.run != run {
            bail!("creation closure owner changed");
        }
        let creation = session.cloud_creation.clone();
        self.connection.execute("INSERT INTO cloud_creation_fences(session,run) VALUES(?1,?2) ON CONFLICT(session) DO UPDATE SET run=excluded.run", params![sql_integer(id.0)?,sql_integer(run.0)?])?;
        if let Some(mut record) = creation
            && !record.closed
        {
            record.closed = true;
            self.save_creation(id, record)?;
        }
        Ok(())
    }
    pub(crate) fn discard_creation(
        &mut self,
        id: SessionId,
        run: SessionRunId,
        config: &Path,
    ) -> Result<()> {
        self.close_creation(id, run)?;
        let mut record = self
            .records
            .get(&id)
            .and_then(|r| r.cloud_creation.clone())
            .context("no retained cloud creation")?;
        record.discarded = true;
        let key = record.intent.key.clone();
        let sends = record
            .sends
            .iter()
            .map(|s| s.key.clone())
            .collect::<Vec<_>>();
        self.save_creation(id, record)?;
        let directory = creation_directory(config, false)?;
        cleanup_creation_aliases(config, &directory)?;
        for send in sends {
            directory.remove(&send)?;
        }
        directory.remove(&key)?;
        Ok(())
    }
    pub(crate) fn refuse_creation_deletion(&self, id: SessionId) -> Result<()> {
        if self
            .records
            .get(&id)
            .and_then(|r| r.cloud_creation.as_ref())
            .is_some_and(Record::requires_discard)
        {
            bail!(
                "cloud creation retained; explicit cloud-discard required before local record deletion"
            );
        }
        Ok(())
    }
}

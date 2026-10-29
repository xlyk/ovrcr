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
#[serde(untagged)]
enum StreamProgress {
    /// Numeric offsets written by schema 25 and earlier are readable history,
    /// never a cursor or revision that a new writer may continue.
    Legacy(u64),
    Current {
        token: String,
        revision: u64,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Progress {
    origin: String,
    #[serde(default)]
    send: Option<StreamProgress>,
    #[serde(default)]
    observe: Option<StreamProgress>,
}
impl Progress {
    fn valid(&self) -> bool {
        let historical = match (&self.send, &self.observe) {
            (Some(StreamProgress::Legacy(send)), Some(StreamProgress::Legacy(observe))) => {
                *send != 0 || *observe != 0
            }
            _ => false,
        };
        let valid_current = |slot: &Option<StreamProgress>| match slot {
            None => true,
            Some(StreamProgress::Current { token, revision }) => {
                !token.is_empty() && token.len() <= ovrcr_protocol::MAX_FRAME_BYTES && *revision > 0
            }
            Some(StreamProgress::Legacy(_)) => false,
        };
        let current = valid_current(&self.send)
            && valid_current(&self.observe)
            && (matches!(&self.send, Some(StreamProgress::Current { .. }))
                || matches!(&self.observe, Some(StreamProgress::Current { .. })));
        (historical || current)
            && self.origin.len() == 64
            && self
                .origin
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    }
}

fn advance_stream_progress(
    slot: &mut Option<StreamProgress>,
    token: &str,
    revision: u64,
) -> Result<()> {
    if token.is_empty() || token.len() > ovrcr_protocol::MAX_FRAME_BYTES || revision == 0 {
        bail!("invalid opaque observation cursor or local revision");
    }
    let expected = match slot {
        None => 1,
        Some(StreamProgress::Legacy(_)) => {
            bail!("legacy observation cursor is historical-only")
        }
        Some(StreamProgress::Current { revision, .. }) => revision
            .checked_add(1)
            .context("observation revision exhausted")?,
    };
    if revision != expected {
        bail!("observation progress revision is stale or out of order");
    }
    *slot = Some(StreamProgress::Current {
        token: token.to_owned(),
        revision,
    });
    Ok(())
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
                    || send
                        .progress
                        .as_ref()
                        .is_some_and(|p| send.run_id.is_none() || !p.valid())
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
                        revision,
                        ..
                    } => {
                        if (record.closed && (!record.cancel_requested || record.discarded))
                            || agent_id != agent
                            || record.sends[index].run_id.as_ref() != Some(run_id)
                            || record.sends[index].terminal.is_some()
                            || offset.is_empty()
                            || offset.len() > ovrcr_protocol::MAX_FRAME_BYTES
                            || *revision == 0
                        {
                            bail!("observation progress identity mismatch or closed");
                        }
                        let progress =
                            record.sends[index]
                                .progress
                                .get_or_insert_with(|| Progress {
                                    origin: origin.clone(),
                                    send: None,
                                    observe: None,
                                });
                        if &progress.origin != origin {
                            bail!("observation stream origin changed");
                        }
                        let slot = if *observe {
                            &mut progress.observe
                        } else {
                            &mut progress.send
                        };
                        advance_stream_progress(slot, offset, *revision)?;
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

#[cfg(test)]
mod progress_tests {
    use super::{Progress, Record, SendRecord, StreamProgress, advance_stream_progress};
    use serde_json::json;

    fn record(progress: Progress) -> Record {
        Record {
            intent: ovrcr_protocol::CloudCreationIntent {
                key: "a".repeat(64),
                account: ovrcr_protocol::CursorCloudAccount {
                    name: "work".into(),
                    keychain_service: "cursor".into(),
                    keychain_account: "work".into(),
                    model: "composer-2.5".into(),
                    repositories: vec!["https://github.com/acme/app".into()],
                    owners: vec![],
                    bridge_archive: "/sdk.tar.gz".into(),
                },
                credential_generation: "b".repeat(64),
                repository: "https://github.com/acme/app".into(),
                source_sha: "c".repeat(40),
            },
            run: ovrcr_protocol::SessionRunId(7),
            lifetime: "lifetime-1".into(),
            invocation: "invocation-1".into(),
            claimed: true,
            closed: true,
            discarded: false,
            cancel_requested: false,
            cancel_claimed: false,
            receipt: Some(ovrcr_protocol::CloudCreationReceipt {
                agent_id: "agent-1".into(),
                model: "composer-2.5".into(),
            }),
            sends: vec![SendRecord {
                key: "d".repeat(64),
                digest: "e".repeat(64),
                claimed: true,
                run_id: Some("run-1".into()),
                terminal: None,
                progress: Some(progress),
            }],
            recovery_owner: None,
            recovery_cancel_claimed: false,
            controller_conflict: None,
        }
    }

    #[test]
    fn legacy_numeric_progress_is_historical_and_zero_string_is_an_opaque_token() {
        let origin = "f".repeat(64);
        let old: Progress = serde_json::from_value(json!({
            "origin": origin,
            "send": 0,
            "observe": 17
        }))
        .unwrap();
        assert!(matches!(&old.send, Some(StreamProgress::Legacy(0))));
        assert!(matches!(&old.observe, Some(StreamProgress::Legacy(17))));
        let old_json = serde_json::to_value(&old).unwrap();
        assert_eq!(old_json["send"], json!(0));
        assert_eq!(old_json["observe"], json!(17));

        let mut legacy_zero = old.send;
        assert!(advance_stream_progress(&mut legacy_zero, "next", 1).is_err());
        let mut legacy_offset = old.observe;
        assert!(advance_stream_progress(&mut legacy_offset, "next", 1).is_err());

        let mut current = None;
        advance_stream_progress(&mut current, "cursor-z", 1).unwrap();
        // Provider tokens are opaque; a lower-looking token still follows the
        // next local acceptance revision.
        advance_stream_progress(&mut current, "0", 2).unwrap();
        assert!(matches!(
            current,
            Some(StreamProgress::Current { token, revision: 2 }) if token == "0"
        ));
    }

    #[test]
    fn progress_requires_nonempty_bounded_token_and_exact_local_successor() {
        let mut current = None;
        assert!(advance_stream_progress(&mut current, "", 1).is_err());
        assert!(advance_stream_progress(&mut current, "cursor", 2).is_err());
        advance_stream_progress(&mut current, "cursor", 1).unwrap();
        assert!(advance_stream_progress(&mut current, "next", 1).is_err());
        assert!(advance_stream_progress(&mut current, "next", 3).is_err());
        assert!(advance_stream_progress(&mut current, "next", 0).is_err());
        let oversized = "x".repeat(ovrcr_protocol::MAX_FRAME_BYTES + 1);
        assert!(advance_stream_progress(&mut current, &oversized, 2).is_err());
    }

    #[test]
    fn record_validation_preserves_only_complete_legacy_or_current_progress() {
        let origin = "f".repeat(64);
        let legacy = |send, observe| Progress {
            origin: origin.clone(),
            send: Some(StreamProgress::Legacy(send)),
            observe: Some(StreamProgress::Legacy(observe)),
        };
        assert!(record(legacy(17, 0)).validate().is_ok());
        assert!(record(legacy(0, 0)).validate().is_err());

        let partial_legacy = Progress {
            origin: origin.clone(),
            send: Some(StreamProgress::Legacy(17)),
            observe: None,
        };
        assert!(record(partial_legacy).validate().is_err());

        let mixed = Progress {
            origin: origin.clone(),
            send: Some(StreamProgress::Legacy(17)),
            observe: Some(StreamProgress::Current {
                token: "cursor".into(),
                revision: 1,
            }),
        };
        assert!(record(mixed).validate().is_err());

        let current = Progress {
            origin,
            send: Some(StreamProgress::Current {
                token: "0".into(),
                revision: 1,
            }),
            observe: None,
        };
        assert!(record(current).validate().is_ok());
    }

    #[test]
    fn persisted_invalid_current_progress_fails_before_payload_cleanup() {
        let cases = [
            ("zero revision", "cursor".to_owned(), 0),
            ("empty token", String::new(), 1),
            (
                "oversized token",
                "x".repeat(ovrcr_protocol::MAX_FRAME_BYTES + 1),
                1,
            ),
        ];
        for (case, token, revision) in cases {
            let root = tempfile::tempdir().unwrap();
            let config = root.path().join("instance");
            crate::config::initialize_registry(&config).unwrap();
            let metadata = serde_json::to_string(&record(Progress {
                origin: "f".repeat(64),
                send: Some(StreamProgress::Current { token, revision }),
                observe: None,
            }))
            .unwrap();

            let database = crate::config::database_path(&config);
            let connection = rusqlite::Connection::open(&database).unwrap();
            connection
                .execute(
                    "INSERT INTO retained_sessions(id,run,project,workspace,name,label,cwd,kind,title_revision,stopped) VALUES(42,7,'demo','root','cloud','cloud',X'2f746d70','\"Terminal\"',0,1)",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO cloud_creations(session,metadata) VALUES(42,?1)",
                    [&metadata],
                )
                .unwrap();
            drop(connection);

            let payload_directory = config.join("cloud-creations");
            std::fs::create_dir_all(&payload_directory).unwrap();
            let creation_payload = payload_directory.join("a".repeat(64));
            let send_payload = payload_directory.join("d".repeat(64));
            std::fs::write(&creation_payload, b"creation payload sentinel").unwrap();
            std::fs::write(&send_payload, b"send payload sentinel").unwrap();

            let error = match crate::retained::SessionStore::open(&config) {
                Ok(_) => panic!("{case} persisted observation progress was accepted"),
                Err(error) => error,
            };
            assert!(
                error
                    .to_string()
                    .contains("invalid retained Send authority"),
                "{case}: {error:#}"
            );

            let connection = rusqlite::Connection::open(&database).unwrap();
            let stored: String = connection
                .query_row(
                    "SELECT metadata FROM cloud_creations WHERE session=42",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(stored, metadata, "{case} metadata changed");
            assert_eq!(
                std::fs::read(&creation_payload).unwrap(),
                b"creation payload sentinel",
                "{case} creation payload changed"
            );
            assert_eq!(
                std::fs::read(&send_payload).unwrap(),
                b"send payload sentinel",
                "{case} Send payload changed"
            );
        }
    }
}

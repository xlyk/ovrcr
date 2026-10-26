//! Creation authority shares SessionStore's connection and retained -> Session fence.
use super::*;
use crate::server::delegation_files::Directory;
use ovrcr_protocol::{CloudCreationCommand, CloudCreationIntent, CloudCreationReceipt};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Record {
    pub intent: CloudCreationIntent,
    pub run: SessionRunId,
    pub lifetime: String,
    pub invocation: String,
    pub claimed: bool,
    pub closed: bool,
    pub discarded: bool,
    pub receipt: Option<CloudCreationReceipt>,
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
        if self.discarded && !self.closed {
            bail!("invalid creation closure");
        }
        Ok(())
    }
    pub fn description(&self) -> String {
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
            "Cloud creation {status}. Send unavailable. Explicit local Discard: ovrcr terminal cloud-discard <id>."
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
    for entry in std::fs::read_dir(&root)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.starts_with('.')
            && name.len() == 65
            && name[1..].bytes().all(|c| c.is_ascii_hexdigit())
        {
            directory.remove(&name)?;
        }
    }
    Ok(directory)
}
impl SessionStore {
    pub(crate) fn cleanup_creation_payloads(&self, config: &Path) -> Result<()> {
        if !config.join("cloud-creations").try_exists()? {
            return Ok(());
        }
        let directory = creation_directory(config, false)?;
        for creation in self
            .records
            .values()
            .filter_map(|r| r.cloud_creation.as_ref())
        {
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
        let fenced: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_creation_fences WHERE session=?1 AND run=?2)",
            params![sql_integer(id.0)?, sql_integer(run.0)?],
            |row| row.get(0),
        )?;
        if fenced && !matches!(command, CloudCreationCommand::Acknowledge { .. }) {
            bail!("cloud creation delivery is closed");
        }

        let directory = creation_directory(config, true)?;
        match command {
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
                    receipt: None,
                };
                self.save_creation(id, record)?;
                directory.publish(&intent.key, &serde_json::to_vec(&intent.request())?)?;
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
        self.save_creation(id, record)?;
        Directory::open(&config.canonicalize()?.join("cloud-creations"), false)?.remove(&key)?;
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

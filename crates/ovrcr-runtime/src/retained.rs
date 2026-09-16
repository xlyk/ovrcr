//! Durable row identity and recovery metadata. Process handles and output stay in Session.
use anyhow::{Context, Result, bail};
use ovrcr_protocol::{
    AgentActivity, SessionId, SessionKind, SessionPhase, SessionRecovery, SessionRunId,
    SessionSummary,
};
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

pub(crate) fn create_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE retained_sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT CHECK (id > 0),
            run INTEGER NOT NULL CHECK (run >= 0),
            project TEXT NOT NULL,
            workspace TEXT NOT NULL,
            name TEXT NOT NULL,
            label TEXT NOT NULL,
            cwd BLOB NOT NULL,
            kind TEXT NOT NULL,
            pinned_title TEXT,
            application_title TEXT,
            title_revision INTEGER NOT NULL CHECK (title_revision >= 0),
            boot_id TEXT,
            stopped INTEGER NOT NULL CHECK (stopped IN (0, 1)),
            failure TEXT,
            UNIQUE (project, workspace, name)
        );",
    )?;
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct SessionMetadata {
    pub project: String,
    pub workspace: String,
    pub name: String,
    pub label: String,
    pub cwd: PathBuf,
    pub kind: SessionKind,
    pub pinned_title: Option<String>,
    pub application_title: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct RetainedSession {
    pub id: SessionId,
    pub run: SessionRunId,
    pub metadata: SessionMetadata,
    pub title_revision: u64,
    pub boot_id: Option<String>,
    pub stopped: bool,
    pub failure: Option<String>,
}

impl RetainedSession {
    pub fn requires_ack(&self, current_boot: Option<&str>) -> bool {
        !self.stopped && !different_boot(self.boot_id.as_deref(), current_boot)
    }

    pub fn recovery(&self, current_boot: Option<&str>) -> SessionRecovery {
        SessionRecovery {
            requires_ack: self.requires_ack(current_boot),
            unavailable: match &self.metadata.kind {
                SessionKind::Terminal => None,
                SessionKind::Agent { name } => {
                    Some(format!("Native resume is not available for {name}"))
                }
            },
            failure: self.failure.clone(),
        }
    }

    pub fn summary(&self, current_boot: Option<&str>) -> SessionSummary {
        SessionSummary {
            id: self.id,
            run: self.run,
            kind: self.metadata.kind.clone(),
            recovery: Some(self.recovery(current_boot)),
            project: self.metadata.project.clone(),
            workspace: self.metadata.workspace.clone(),
            name: self.metadata.name.clone(),
            label: self.metadata.label.clone(),
            pid: None,
            started_unix_ms: None,
            phase: if self.stopped {
                SessionPhase::Stopped
            } else {
                SessionPhase::Interrupted
            },
            activity: AgentActivity::Unknown,
            agent: None,
            agent_epoch: 0,
            unread: None,
            context_usage: None,
            title: self
                .metadata
                .pinned_title
                .clone()
                .or_else(|| self.metadata.application_title.clone()),
        }
    }
}

pub(crate) struct SessionStore {
    connection: Connection,
    records: HashMap<SessionId, RetainedSession>,
    boot_id: Option<String>,
}

impl SessionStore {
    pub fn open(config: &Path) -> Result<Self> {
        Self::from_connection(crate::config::open_writable_registry(config)?)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        let records = read_records(&connection)?;
        Ok(Self {
            connection,
            records,
            boot_id: current_boot_id(),
        })
    }

    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        let connection = Connection::open_in_memory().unwrap();
        create_schema(&connection).unwrap();
        Self::from_connection(connection).unwrap()
    }

    #[cfg(test)]
    pub(crate) fn register_fixture(&mut self, session: &crate::session::Session) -> Result<()> {
        let summary = session.summary();
        let titles = session.title_snapshot();
        let record = RetainedSession {
            id: summary.id,
            run: summary.run,
            metadata: SessionMetadata {
                project: summary.project,
                workspace: summary.workspace,
                name: summary.name,
                label: summary.label,
                cwd: session.initial_cwd.clone(),
                kind: summary.kind,
                pinned_title: titles.pinned,
                application_title: titles.application,
            },
            title_revision: titles.revision,
            boot_id: self.boot_id.clone(),
            stopped: !session.is_live(),
            failure: None,
        };
        let metadata = &record.metadata;
        self.connection.execute(
            "INSERT INTO retained_sessions
             (id, run, project, workspace, name, label, cwd, kind, pinned_title,
              application_title, title_revision, boot_id, stopped, failure)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL)
             ON CONFLICT(id) DO UPDATE SET
             run=excluded.run, project=excluded.project, workspace=excluded.workspace,
             name=excluded.name, label=excluded.label, cwd=excluded.cwd, kind=excluded.kind,
             pinned_title=excluded.pinned_title, application_title=excluded.application_title,
             title_revision=excluded.title_revision, boot_id=excluded.boot_id,
             stopped=excluded.stopped, failure=NULL",
            params![
                sql_integer(record.id.0)?,
                sql_integer(record.run.0)?,
                metadata.project,
                metadata.workspace,
                metadata.name,
                metadata.label,
                metadata.cwd.as_os_str().as_bytes(),
                serde_json::to_string(&metadata.kind)?,
                metadata.pinned_title,
                metadata.application_title,
                sql_integer(record.title_revision)?,
                record.boot_id,
                record.stopped,
            ],
        )?;
        self.records.insert(record.id, record);
        Ok(())
    }

    pub fn boot_id(&self) -> Option<&str> {
        self.boot_id.as_deref()
    }

    pub fn get(&self, id: SessionId) -> Option<&RetainedSession> {
        self.records.get(&id)
    }

    pub fn records(&self) -> impl Iterator<Item = &RetainedSession> {
        self.records.values()
    }

    pub fn create(&mut self, metadata: SessionMetadata) -> Result<RetainedSession> {
        self.connection
            .execute(
                "INSERT INTO retained_sessions
             (run, project, workspace, name, label, cwd, kind, pinned_title,
              application_title, title_revision, stopped)
             VALUES (0, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 1)",
                params![
                    metadata.project,
                    metadata.workspace,
                    metadata.name,
                    metadata.label,
                    metadata.cwd.as_os_str().as_bytes(),
                    serde_json::to_string(&metadata.kind)?,
                    metadata.pinned_title,
                    metadata.application_title,
                ],
            )
            .context("retain new session")?;
        let id = SessionId(self.connection.last_insert_rowid().try_into()?);
        let record = RetainedSession {
            id,
            run: SessionRunId(0),
            metadata,
            title_revision: 0,
            boot_id: None,
            stopped: true,
            failure: None,
        };
        self.records.insert(id, record.clone());
        Ok(record)
    }

    /// Commit launch intent before any process exists. The caller holds admission ownership.
    pub fn begin_run(&mut self, id: SessionId, expected: SessionRunId) -> Result<RetainedSession> {
        let record = self.records.get(&id).context("session not found")?;
        if record.run != expected {
            bail!("session run changed");
        }
        let next = expected
            .0
            .checked_add(1)
            .context("session run identity exhausted")?;
        let changed = self
            .connection
            .execute(
                "UPDATE retained_sessions SET run = ?1, boot_id = ?2, stopped = 0,
             failure = NULL, title_revision = 0 WHERE id = ?3 AND run = ?4",
                params![
                    sql_integer(next)?,
                    self.boot_id,
                    sql_integer(id.0)?,
                    sql_integer(expected.0)?
                ],
            )
            .context("retain session launch intent")?;
        if changed != 1 {
            bail!("session run changed");
        }
        let record = self.records.get_mut(&id).unwrap();
        record.run = SessionRunId(next);
        record.boot_id = self.boot_id.clone();
        record.stopped = false;
        record.failure = None;
        record.title_revision = 0;
        Ok(record.clone())
    }

    pub fn mark_stopped(&mut self, id: SessionId, run: SessionRunId) -> Result<bool> {
        let Some(record) = self.records.get(&id).filter(|record| record.run == run) else {
            return Ok(false);
        };
        if record.stopped {
            return Ok(true);
        }
        let changed = self
            .connection
            .execute(
                "UPDATE retained_sessions SET stopped = 1 WHERE id = ?1 AND run = ?2",
                params![sql_integer(id.0)?, sql_integer(run.0)?],
            )
            .context("retain stopped session")?;
        if changed != 1 {
            return Ok(false);
        }
        self.records.get_mut(&id).unwrap().stopped = true;
        Ok(true)
    }

    pub fn record_failure(
        &mut self,
        id: SessionId,
        run: SessionRunId,
        message: String,
    ) -> Result<bool> {
        if self.records.get(&id).is_none_or(|record| record.run != run) {
            return Ok(false);
        }
        let changed = self
            .connection
            .execute(
                "UPDATE retained_sessions SET failure = ?1 WHERE id = ?2 AND run = ?3",
                params![message, sql_integer(id.0)?, sql_integer(run.0)?],
            )
            .context("retain session failure")?;
        if changed != 1 {
            return Ok(false);
        }
        self.records.get_mut(&id).unwrap().failure = Some(message);
        Ok(true)
    }

    pub fn update_titles(
        &mut self,
        id: SessionId,
        run: SessionRunId,
        revision: u64,
        pinned: Option<String>,
        application: Option<String>,
    ) -> Result<bool> {
        let Some(record) = self.records.get(&id) else {
            return Ok(false);
        };
        if record.run != run || revision < record.title_revision {
            return Ok(false);
        }
        if record.metadata.pinned_title == pinned
            && record.metadata.application_title == application
            && record.title_revision == revision
        {
            return Ok(false);
        }
        let changed = self
            .connection
            .execute(
                "UPDATE retained_sessions SET pinned_title = ?1, application_title = ?2,
             title_revision = ?3 WHERE id = ?4 AND run = ?5",
                params![
                    pinned,
                    application,
                    sql_integer(revision)?,
                    sql_integer(id.0)?,
                    sql_integer(run.0)?
                ],
            )
            .context("retain session title")?;
        if changed != 1 {
            return Ok(false);
        }
        let record = self.records.get_mut(&id).unwrap();
        record.metadata.pinned_title = pinned;
        record.metadata.application_title = application;
        record.title_revision = revision;
        Ok(true)
    }

    pub fn remove(&mut self, id: SessionId, run: SessionRunId) -> Result<bool> {
        if self.records.get(&id).is_none_or(|record| record.run != run) {
            return Ok(false);
        }
        let changed = self
            .connection
            .execute(
                "DELETE FROM retained_sessions WHERE id = ?1 AND run = ?2",
                params![sql_integer(id.0)?, sql_integer(run.0)?],
            )
            .context("remove retained session")?;
        if changed != 1 {
            return Ok(false);
        }
        self.records.remove(&id);
        Ok(true)
    }
}

/// Offline inventory is read-only, including before the session schema has been installed.
pub fn load_session_summaries(config: &Path) -> Result<Vec<SessionSummary>> {
    let Some(mut connection) = crate::config::open_readonly_registry(config)? else {
        return Ok(Vec::new());
    };
    let transaction = connection.transaction()?;
    crate::config::check_schema(&transaction)?;
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 1 {
        return Ok(Vec::new());
    }
    let boot_id = current_boot_id();
    Ok(read_records(&transaction)?
        .values()
        .map(|record| record.summary(boot_id.as_deref()))
        .collect())
}

fn sql_integer(value: u64) -> Result<i64> {
    i64::try_from(value).context("session counter exceeds SQLite integer range")
}

fn read_integer(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    value
        .try_into()
        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn read_records(connection: &Connection) -> Result<HashMap<SessionId, RetainedSession>> {
    let mut query = connection.prepare(
        "SELECT id, run, project, workspace, name, label, cwd, kind, pinned_title,
         application_title, title_revision, boot_id, stopped, failure FROM retained_sessions",
    )?;
    let rows = query.query_map([], |row| {
        let kind: String = row.get(7)?;
        let kind = serde_json::from_str(&kind).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                7,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        let stopped = match row.get::<_, i64>(12)? {
            0 => false,
            1 => true,
            value => return Err(rusqlite::Error::IntegralValueOutOfRange(12, value)),
        };
        Ok(RetainedSession {
            id: SessionId(read_integer(row, 0)?),
            run: SessionRunId(read_integer(row, 1)?),
            metadata: SessionMetadata {
                project: row.get(2)?,
                workspace: row.get(3)?,
                name: row.get(4)?,
                label: row.get(5)?,
                cwd: PathBuf::from(OsString::from_vec(row.get(6)?)),
                kind,
                pinned_title: row.get(8)?,
                application_title: row.get(9)?,
            },
            title_revision: read_integer(row, 10)?,
            boot_id: row.get(11)?,
            stopped,
            failure: row.get(13)?,
        })
    })?;
    let mut records = HashMap::new();
    for row in rows {
        let record = row.context("read retained session")?;
        if record.id.0 == 0 || record.metadata.name.is_empty() {
            bail!("invalid retained session identity");
        }
        ovrcr_protocol::validate_name(&record.metadata.project, "project")?;
        ovrcr_protocol::validate_name(&record.metadata.workspace, "workspace")?;
        records.insert(record.id, record);
    }
    Ok(records)
}

fn valid_boot_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
        && id.bytes().any(|byte| byte != b'0' && byte != b'-')
}

pub(crate) fn different_boot(saved: Option<&str>, current: Option<&str>) -> bool {
    saved
        .filter(|id| valid_boot_id(id))
        .zip(current.filter(|id| valid_boot_id(id)))
        .is_some_and(|(saved, current)| !saved.eq_ignore_ascii_case(current))
}

/// Read the kernel's boot UUID, never a PID, wall-clock estimate, or operator override.
pub fn current_boot_id() -> Option<String> {
    #[cfg(target_os = "linux")]
    let value = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    #[cfg(target_os = "macos")]
    let value = {
        let mut buffer = [0u8; 64];
        let mut len = buffer.len();
        let result = unsafe {
            libc::sysctlbyname(
                c"kern.bootsessionuuid".as_ptr(),
                buffer.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if result != 0 || len > buffer.len() {
            return None;
        }
        std::str::from_utf8(&buffer[..len])
            .ok()?
            .trim_end_matches('\0')
            .to_owned()
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    return None;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let value = value.trim();
        valid_boot_id(value).then(|| value.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const BOOT_A: &str = "11111111-2222-3333-4444-555555555555";
    const BOOT_B: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

    #[test]
    fn only_two_verified_different_boot_ids_prove_prior_processes_gone() {
        assert!(different_boot(Some(BOOT_A), Some(BOOT_B)));
        assert!(!different_boot(Some(BOOT_A), Some(BOOT_A)));
        assert!(!different_boot(Some(BOOT_B), Some(&BOOT_B.to_uppercase())));
        assert!(!different_boot(Some(BOOT_A), None));
        assert!(!different_boot(None, Some(BOOT_B)));
        assert!(!different_boot(Some("unverifiable"), Some(BOOT_B)));
        assert!(!different_boot(
            Some(BOOT_A),
            Some("00000000-0000-0000-0000-000000000000")
        ));
    }
}

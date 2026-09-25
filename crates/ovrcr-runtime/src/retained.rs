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
            disposition INTEGER NOT NULL DEFAULT 0 CHECK (disposition IN (0, 1, 2)),
            UNIQUE (project, workspace, name)
        );",
    )?;
    Ok(())
}

pub(crate) fn has_table(connection: &Connection, name: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = ?1)",
        [name],
        |row| row.get(0),
    )?)
}

pub(crate) fn has_disposition(connection: &Connection) -> Result<bool> {
    Ok(connection.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('retained_sessions') WHERE name = 'disposition')", [], |row| row.get(0))?)
}

pub(crate) fn create_conversation_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE agent_conversations (session INTEGER PRIMARY KEY REFERENCES retained_sessions(id) ON DELETE CASCADE, reference TEXT, invalid INTEGER NOT NULL CHECK(invalid IN (0, 1)));")?;
    Ok(())
}

pub(crate) fn create_conversation_subject_schema(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS conversation_subjects (
            session INTEGER NOT NULL REFERENCES retained_sessions(id) ON DELETE CASCADE,
            conversation TEXT NOT NULL,
            topic TEXT,
            dismissed INTEGER NOT NULL DEFAULT 0 CHECK(dismissed IN (0, 1)),
            accepted_count INTEGER NOT NULL DEFAULT 0 CHECK(accepted_count >= 0),
            attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count >= 0),
            PRIMARY KEY(session, conversation)
        );",
    )?;
    Ok(())
}

/// Called inside the registry migration transaction. Retain the exact old
/// reference and invalidation marker; never infer a replacement identity.
pub(crate) fn migrate_claude_conversations(connection: &Connection) -> Result<()> {
    create_conversation_schema(connection)?;
    let mut query =
        connection.prepare("SELECT session, reference, invalid FROM claude_conversations")?;
    let rows = query.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, bool>(2)?,
        ))
    })?;
    for row in rows {
        let (id, reference, invalid) = row?;
        let reference = reference
            .map(|value| -> Result<String> {
                let reference: ovrcr_protocol::ClaudeConversation = serde_json::from_str(&value)?;
                Ok(serde_json::to_string(
                    &ovrcr_protocol::ConversationReference::Claude(reference),
                )?)
            })
            .transpose()?;
        connection.execute(
            "INSERT INTO agent_conversations (session, reference, invalid) VALUES (?1, ?2, ?3)",
            params![id, reference, invalid],
        )?;
    }
    drop(query);
    connection.execute_batch("DROP TABLE claude_conversations")?;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Disposition {
    Active = 0,
    Archived = 1,
    Returned = 2,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ConversationSubject {
    pub topic: Option<String>,
    pub dismissed: bool,
    pub accepted_count: u64,
    pub attempt_count: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct RetainedSession {
    pub id: SessionId,
    pub run: SessionRunId,
    pub metadata: SessionMetadata,
    pub title_revision: u64,
    pub boot_id: Option<String>,
    pub stopped: bool,
    pub disposition: Disposition,
    pub failure: Option<String>,
    pub conversation: Option<ovrcr_protocol::ConversationReference>,
    pub identity_invalid: bool,
    pub subjects: HashMap<String, ConversationSubject>,
}

impl RetainedSession {
    pub fn requires_ack(&self, current_boot: Option<&str>) -> bool {
        !self.stopped && !different_boot(self.boot_id.as_deref(), current_boot)
    }

    pub fn recovery(&self, current_boot: Option<&str>) -> SessionRecovery {
        SessionRecovery {
            conversation: self
                .conversation
                .as_ref()
                .map(|reference| reference.identity().to_owned()),
            attached: false,
            requires_ack: self.requires_ack(current_boot),
            unavailable: match &self.metadata.kind {
                SessionKind::Terminal => None,
                SessionKind::Agent { name } => crate::recovery::unavailable(
                    name,
                    self.conversation.as_ref(),
                    self.identity_invalid,
                ),
            },
            failure: self.failure.clone(),
        }
    }

    pub fn effective_title(&self) -> Option<String> {
        if self.metadata.pinned_title.is_some() {
            return self.metadata.pinned_title.clone();
        }
        if matches!(self.metadata.kind, SessionKind::Terminal) {
            return None;
        }
        let conversation = self.conversation.as_ref()?.identity();
        self.subjects
            .get(conversation)
            .filter(|subject| !subject.dismissed)
            .and_then(|subject| subject.topic.clone())
    }

    pub fn summary(&self, current_boot: Option<&str>) -> SessionSummary {
        SessionSummary {
            id: self.id,
            cwd: self.metadata.cwd.clone(),
            archived: self.disposition == Disposition::Archived,
            run: self.run,
            kind: self.metadata.kind.clone(),
            recovery: Some(self.recovery(current_boot)),
            project: self.metadata.project.clone(),
            workspace: self.metadata.workspace.clone(),
            name: self.metadata.name.clone(),
            label: self.metadata.label.clone(),
            pid: None,
            started_unix_ms: None,
            phase: if self.stopped || self.disposition != Disposition::Active {
                SessionPhase::Stopped
            } else {
                SessionPhase::Interrupted
            },
            activity: AgentActivity::Unknown,
            agent: None,
            agent_epoch: 0,
            unread: None,
            context_usage: None,
            title: self.effective_title(),
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
        let records = read_records(&connection, true)?;
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
        create_conversation_schema(&connection).unwrap();
        create_conversation_subject_schema(&connection).unwrap();
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
            conversation: None,
            identity_invalid: false,
            disposition: Disposition::Active,
            subjects: HashMap::new(),
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

    pub fn retain_conversation(
        &mut self,
        id: SessionId,
        run: SessionRunId,
        reference: Option<&ovrcr_protocol::ConversationReference>,
    ) -> Result<()> {
        let record = self.records.get(&id).context("session not found")?;
        if record.run != run {
            bail!("session run changed");
        }
        if record.identity_invalid && reference.is_some() {
            bail!("Recovery identity was invalidated");
        }
        if reference.is_none() {
            // Fail closed in the running owner even if the durable write fails.
            // The caller receives failure and must retry until it is committed.
            self.records.get_mut(&id).unwrap().identity_invalid = true;
        }
        let encoded = reference.map(serde_json::to_string).transpose()?;
        self.connection.execute(
            "INSERT INTO agent_conversations (session, reference, invalid) VALUES (?1, ?2, ?3)
             ON CONFLICT(session) DO UPDATE SET reference = COALESCE(excluded.reference, reference), invalid = excluded.invalid",
            params![sql_integer(id.0)?, encoded, reference.is_none()],
        )?;
        let record = self.records.get_mut(&id).unwrap();
        if let Some(reference) = reference {
            record.conversation = Some(reference.clone());
        }
        record.identity_invalid = reference.is_none();
        Ok(())
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
            conversation: None,
            identity_invalid: false,
            disposition: Disposition::Active,
            subjects: HashMap::new(),
        };
        self.records.insert(id, record.clone());
        Ok(record)
    }

    /// Commit launch intent before any process exists. The caller holds admission ownership.
    pub fn begin_run(&mut self, id: SessionId, expected: SessionRunId) -> Result<RetainedSession> {
        let record = self.records.get(&id).context("session not found")?;
        if record.run != expected || record.disposition == Disposition::Archived {
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
             failure = NULL, title_revision = 0, disposition = 0 WHERE id = ?3 AND run = ?4",
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
        record.disposition = Disposition::Active;
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

    pub fn record_subject_attempt(
        &mut self,
        id: SessionId,
        run: SessionRunId,
        conversation: &str,
    ) -> Result<bool> {
        let Some(record) = self.records.get(&id) else {
            return Ok(false);
        };
        if record.run != run || record.disposition == Disposition::Archived {
            return Ok(false);
        }
        self.connection.execute(
            "INSERT INTO conversation_subjects
             (session, conversation, attempt_count)
             VALUES (?1, ?2, 1)
             ON CONFLICT(session, conversation) DO UPDATE SET
             attempt_count = attempt_count + 1",
            params![sql_integer(id.0)?, conversation],
        )?;
        let subject = self
            .records
            .get_mut(&id)
            .unwrap()
            .subjects
            .entry(conversation.to_owned())
            .or_default();
        subject.attempt_count = subject.attempt_count.saturating_add(1);
        Ok(true)
    }

    pub fn save_conversation_subject(
        &mut self,
        id: SessionId,
        run: SessionRunId,
        conversation: &str,
        topic: String,
    ) -> Result<bool> {
        let Some(record) = self.records.get(&id) else {
            return Ok(false);
        };
        if record.run != run
            || record.disposition == Disposition::Archived
            || record.metadata.pinned_title.is_some()
        {
            return Ok(false);
        }
        let changed = self.connection.execute(
            "INSERT INTO conversation_subjects
             (session, conversation, topic, accepted_count, attempt_count)
             VALUES (?1, ?2, ?3, 1, 1)
             ON CONFLICT(session, conversation) DO UPDATE SET
             topic = CASE WHEN accepted_count = 0 AND dismissed = 0 THEN excluded.topic ELSE topic END,
             accepted_count = CASE WHEN accepted_count = 0 AND dismissed = 0 THEN accepted_count + 1 ELSE accepted_count END,
             attempt_count = attempt_count + 1",
            params![sql_integer(id.0)?, conversation, topic],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        let subject = self
            .records
            .get_mut(&id)
            .unwrap()
            .subjects
            .entry(conversation.to_owned())
            .or_default();
        subject.attempt_count = subject.attempt_count.saturating_add(1);
        if subject.accepted_count == 0 && !subject.dismissed {
            subject.topic = Some(topic);
            subject.accepted_count = 1;
            return Ok(true);
        }
        Ok(false)
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
        if record.run != run
            || record.disposition == Disposition::Archived
            || revision < record.title_revision
        {
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

    /// Invalidate the old run on both transitions. Delayed reopen, close and
    /// process callbacks must not cross a close/unarchive boundary.
    /// Ownership proof (`stopped`) is independent of filing the record.
    pub fn set_archived(
        &mut self,
        id: SessionId,
        expected: SessionRunId,
        archived: bool,
    ) -> Result<()> {
        let record = self.records.get(&id).context("session not found")?;
        if record.run != expected || (record.disposition == Disposition::Archived) == archived {
            bail!("session archive state changed");
        }
        let next = expected
            .0
            .checked_add(1)
            .context("session run identity exhausted")?;
        let disposition = if archived {
            Disposition::Archived
        } else {
            Disposition::Returned
        };
        self.connection.execute(
            "UPDATE retained_sessions SET disposition = ?1, run = ?2 WHERE id = ?3 AND run = ?4",
            params![disposition as u8, sql_integer(next)?, sql_integer(id.0)?, sql_integer(expected.0)?],
        ).context("persist session archive transition")?;
        let record = self.records.get_mut(&id).unwrap();
        record.disposition = disposition;
        record.run = SessionRunId(next);
        Ok(())
    }

    /// Prepare metadata changes before touching Git. Git failure rolls them back;
    /// commit failure after Git removal leaves the original rows available for recovery.
    pub fn archive_workspace(
        &mut self,
        registry: &ovrcr_protocol::Registry,
        project: &str,
        workspace: &str,
        remove: impl FnOnce() -> Result<()>,
    ) -> Result<Vec<SessionId>> {
        registry.validate()?;
        let changes = self
            .records
            .values()
            .filter(|record| {
                record.metadata.project == project
                    && record.metadata.workspace == workspace
                    && record.disposition != Disposition::Archived
            })
            .map(|record| {
                let next = record
                    .run
                    .0
                    .checked_add(1)
                    .context("session run identity exhausted")?;
                Ok((record.id, sql_integer(next)?))
            })
            .collect::<Result<Vec<_>>>()?;
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        crate::config::write_registry(&transaction, registry)?;
        for (id, next) in &changes {
            transaction
                .execute(
                    "UPDATE retained_sessions SET disposition = 1, run = ?1 WHERE id = ?2",
                    params![next, sql_integer(id.0)?],
                )
                .context("archive workspace sessions")?;
        }
        remove()?;
        transaction
            .commit()
            .context("commit workspace removal and session archive")?;
        for (id, next) in &changes {
            let record = self.records.get_mut(id).unwrap();
            record.disposition = Disposition::Archived;
            record.run = SessionRunId(*next as u64);
        }
        Ok(changes.into_iter().map(|(id, _)| id).collect())
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
    Ok(read_records(&transaction, has_disposition(&transaction)?)?
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

fn read_records(
    connection: &Connection,
    has_disposition: bool,
) -> Result<HashMap<SessionId, RetainedSession>> {
    let disposition = if has_disposition { "disposition" } else { "0" };
    let mut query = connection.prepare(&format!(
        "SELECT id, run, project, workspace, name, label, cwd, kind, pinned_title,
         application_title, title_revision, boot_id, stopped, failure, {disposition} FROM retained_sessions",
    ))?;
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
            conversation: None,
            identity_invalid: false,
            disposition: match row.get::<_, i64>(14)? {
                0 => Disposition::Active,
                1 => Disposition::Archived,
                2 => Disposition::Returned,
                value => return Err(rusqlite::Error::IntegralValueOutOfRange(14, value)),
            },
            subjects: HashMap::new(),
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
    let legacy = has_table(connection, "claude_conversations")?;
    let table = if legacy {
        "claude_conversations"
    } else {
        "agent_conversations"
    };
    let table_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = ?1)",
        [table],
        |row| row.get(0),
    )?;
    if table_exists {
        let mut query =
            connection.prepare(&format!("SELECT session, reference, invalid FROM {table}"))?;
        let rows = query.query_map([], |row| {
            Ok((
                read_integer(row, 0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, bool>(2)?,
            ))
        })?;
        for row in rows {
            let (id, reference, invalid) = row?;
            if let Some(record) = records.get_mut(&SessionId(id)) {
                record.conversation = reference
                    .map(|value| {
                        if legacy {
                            serde_json::from_str(&value)
                                .map(ovrcr_protocol::ConversationReference::Claude)
                        } else {
                            serde_json::from_str(&value)
                        }
                    })
                    .transpose()?;
                record.identity_invalid = invalid;
            }
        }
    }
    if has_table(connection, "conversation_subjects")? {
        let mut query = connection.prepare(
            "SELECT session, conversation, topic, dismissed, accepted_count, attempt_count
             FROM conversation_subjects",
        )?;
        let rows = query.query_map([], |row| {
            Ok((
                read_integer(row, 0)?,
                row.get::<_, String>(1)?,
                ConversationSubject {
                    topic: row.get(2)?,
                    dismissed: row.get(3)?,
                    accepted_count: read_integer(row, 4)?,
                    attempt_count: read_integer(row, 5)?,
                },
            ))
        })?;
        for row in rows {
            let (id, conversation, subject) = row?;
            if let Some(record) = records.get_mut(&SessionId(id)) {
                record.subjects.insert(conversation, subject);
            }
        }
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

    #[test]
    fn workspace_archive_rolls_back_on_external_failure_and_fences_every_changed_row() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        let mut store = SessionStore::open(&config).unwrap();
        let mut records = Vec::new();
        for (workspace, name) in [
            ("w", "first"),
            ("w", "second"),
            ("w", "archived"),
            ("other", "unrelated"),
        ] {
            records.push(
                store
                    .create(SessionMetadata {
                        project: "p".into(),
                        workspace: workspace.into(),
                        name: name.into(),
                        label: "sh".into(),
                        cwd: root.path().join(workspace),
                        kind: SessionKind::Terminal,
                        pinned_title: Some(name.into()),
                        application_title: None,
                    })
                    .unwrap(),
            );
        }
        store
            .set_archived(records[2].id, records[2].run, true)
            .unwrap();
        let prior_archive_run = store.get(records[2].id).unwrap().run;
        let registry = crate::config::load_registry(&config).unwrap();
        assert!(
            store
                .archive_workspace(&registry, "p", "w", || bail!("Git refused removal"))
                .is_err()
        );
        for record in &records[..2] {
            assert_eq!(store.get(record.id).unwrap().run, record.run);
            assert_eq!(
                store.get(record.id).unwrap().disposition,
                Disposition::Active
            );
        }
        let changed = store
            .archive_workspace(&registry, "p", "w", || Ok(()))
            .unwrap();
        assert_eq!(changed.len(), 2);
        for record in &records[..2] {
            assert!(changed.contains(&record.id));
            assert_eq!(
                store.get(record.id).unwrap().disposition,
                Disposition::Archived
            );
            assert_eq!(
                store.get(record.id).unwrap().metadata.cwd,
                record.metadata.cwd
            );
            assert!(
                !store
                    .update_titles(record.id, record.run, 999, None, Some("late".into()))
                    .unwrap()
            );
            assert!(store.begin_run(record.id, record.run).is_err());
        }
        assert_eq!(store.get(records[2].id).unwrap().run, prior_archive_run);
        assert_eq!(
            store.get(records[3].id).unwrap().disposition,
            Disposition::Active
        );
        drop(store);
        let rows = load_session_summaries(&config).unwrap();
        assert_eq!(rows.iter().filter(|row| row.archived).count(), 3);
        assert_eq!(rows.len(), 4);
    }

    #[test]
    fn archive_schema_three_migrates_without_reviving_archived_rows() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        let mut store = SessionStore::open(&config).unwrap();
        let record = store
            .create(SessionMetadata {
                project: "p".into(),
                workspace: "w".into(),
                name: "saved".into(),
                label: "sh".into(),
                cwd: root.path().to_path_buf(),
                kind: SessionKind::Terminal,
                pinned_title: Some("Archived title".into()),
                application_title: None,
            })
            .unwrap();
        store.set_archived(record.id, record.run, true).unwrap();
        let expected = store.get(record.id).unwrap().summary(store.boot_id());
        drop(store);
        let connection = Connection::open(crate::config::database_path(&config)).unwrap();
        connection
            .execute_batch("DROP TABLE agent_conversations; PRAGMA user_version = 3;")
            .unwrap();
        drop(connection);
        assert_eq!(
            load_session_summaries(&config).unwrap(),
            vec![expected.clone()]
        );
        let mut reopened = SessionStore::open(&config).unwrap();
        assert_eq!(
            reopened.get(record.id).unwrap().summary(reopened.boot_id()),
            expected
        );
        assert!(reopened.begin_run(record.id, expected.run).is_err());
    }

    #[test]
    fn schema_two_migrates_without_losing_rows_and_archive_fences_old_callbacks() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        crate::config::initialize_registry(&config).unwrap();
        let mut store = SessionStore::open(&config).unwrap();
        let record = store
            .create(SessionMetadata {
                project: "p".into(),
                workspace: "w".into(),
                name: "saved".into(),
                label: "sh".into(),
                cwd: root.path().to_path_buf(),
                kind: SessionKind::Terminal,
                pinned_title: Some("Saved title".into()),
                application_title: None,
            })
            .unwrap();
        drop(store);
        let connection = Connection::open(crate::config::database_path(&config)).unwrap();
        connection
            .execute_batch(
                "ALTER TABLE retained_sessions DROP COLUMN disposition; PRAGMA user_version = 2;",
            )
            .unwrap();
        drop(connection);
        assert!(!load_session_summaries(&config).unwrap()[0].archived);
        let mut store = SessionStore::open(&config).unwrap();
        assert_eq!(
            store
                .get(record.id)
                .unwrap()
                .metadata
                .pinned_title
                .as_deref(),
            Some("Saved title")
        );
        store.set_archived(record.id, record.run, true).unwrap();
        assert!(store.begin_run(record.id, record.run).is_err());
        assert!(
            !store
                .update_titles(record.id, record.run, 999, None, Some("late title".into()))
                .unwrap()
        );
        assert!(!store.mark_stopped(record.id, record.run).unwrap());
        assert!(
            !store
                .record_failure(record.id, record.run, "late failure".into())
                .unwrap()
        );
        let archived_run = store.get(record.id).unwrap().run;
        assert!(store.begin_run(record.id, archived_run).is_err());
        store.set_archived(record.id, archived_run, false).unwrap();
        assert!(store.begin_run(record.id, record.run).is_err());
        drop(store);
        let reopened = SessionStore::open(&config).unwrap();
        let summary = reopened.get(record.id).unwrap().summary(reopened.boot_id());
        assert_eq!(summary.phase, SessionPhase::Stopped);
        assert!(!summary.archived);
        assert_eq!(summary.title.as_deref(), Some("Saved title"));
    }
}

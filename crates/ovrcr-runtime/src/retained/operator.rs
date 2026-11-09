//! Durable Operator journal, owned by the existing SessionStore connection.
use super::*;
use ovrcr_protocol::operator::*;
use rusqlite::OptionalExtension;

pub(crate) fn create_schema(connection: &Connection) -> Result<()> {
    let has_scope: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('retained_sessions') WHERE name='scope')",
        [],
        |r| r.get(0),
    )?;
    if !has_scope {
        connection.execute_batch(
            "ALTER TABLE retained_sessions ADD COLUMN scope TEXT NOT NULL DEFAULT '\"Workspace\"';",
        )?;
    }
    connection.execute_batch("CREATE TABLE IF NOT EXISTS operator_conversations (
        id INTEGER PRIMARY KEY AUTOINCREMENT CHECK(id>0),
        session INTEGER NOT NULL UNIQUE REFERENCES retained_sessions(id) ON DELETE RESTRICT,
        provider TEXT NOT NULL, model TEXT NOT NULL, title TEXT NOT NULL,
        singleton INTEGER NOT NULL DEFAULT 0 CHECK(singleton IN (0,1)),
        pi_checkpoint_turn INTEGER);
        CREATE TABLE IF NOT EXISTS operator_turns (
        id INTEGER PRIMARY KEY AUTOINCREMENT CHECK(id>0),
        conversation INTEGER NOT NULL REFERENCES operator_conversations(id) ON DELETE CASCADE,
        run INTEGER NOT NULL CHECK(run>0), state TEXT NOT NULL, sequence INTEGER NOT NULL DEFAULT 0,
        model_override TEXT,
        UNIQUE(conversation,run));
        CREATE TABLE IF NOT EXISTS operator_messages (
        ordinal INTEGER PRIMARY KEY AUTOINCREMENT, conversation INTEGER NOT NULL REFERENCES operator_conversations(id) ON DELETE CASCADE,
        turn INTEGER NOT NULL REFERENCES operator_turns(id), content TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS operator_model_history (
        conversation INTEGER PRIMARY KEY REFERENCES operator_conversations(id) ON DELETE CASCADE,
        turn INTEGER NOT NULL REFERENCES operator_turns(id), history TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS operator_batches (
        turn INTEGER NOT NULL REFERENCES operator_turns(id) ON DELETE CASCADE,
        sequence INTEGER NOT NULL, digest TEXT NOT NULL, PRIMARY KEY(turn,sequence));")?;
    // `CREATE TABLE IF NOT EXISTS` leaves a schema-26/27/28 Operator table exactly
    // as the old writer shaped it, so the current columns are added explicitly.
    ensure_singleton_columns(connection)?;
    create_operator_indexes(connection)
}

/// Columns the singleton/checkpoint contract needs. Both the fresh path and the
/// 28 -> 29 step run this one helper so they cannot drift apart.
fn ensure_singleton_columns(connection: &Connection) -> Result<()> {
    if !has_column(connection, "operator_conversations", "singleton")? {
        connection.execute_batch(
            "ALTER TABLE operator_conversations
             ADD COLUMN singleton INTEGER NOT NULL DEFAULT 0 CHECK(singleton IN (0,1));",
        )?;
    }
    if !has_column(connection, "operator_conversations", "pi_checkpoint_turn")? {
        connection.execute_batch(
            "ALTER TABLE operator_conversations ADD COLUMN pi_checkpoint_turn INTEGER;",
        )?;
    }
    if has_table(connection, "operator_turns")?
        && !has_column(connection, "operator_turns", "model_override")?
    {
        connection.execute_batch("ALTER TABLE operator_turns ADD COLUMN model_override TEXT;")?;
    }
    Ok(())
}

/// Indexes that depend on the columns above, so they are created only after
/// `ensure_singleton_columns` has widened an existing table.
fn create_operator_indexes(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS operator_single_conversation
         ON operator_conversations(singleton) WHERE singleton=1;
         CREATE UNIQUE INDEX IF NOT EXISTS operator_one_running_turn
         ON operator_turns(conversation) WHERE state='Running';
         CREATE INDEX IF NOT EXISTS operator_history
         ON operator_messages(conversation,ordinal);",
    )?;
    Ok(())
}

/// Idempotent schema 28 -> 29 step: one singleton conversation plus per-turn model
/// capture. Legacy rows keep `singleton=0`; no conversation, archive state, stopped
/// proof or legacy journal byte is rewritten here.
pub(crate) fn migrate_singleton_conversation(connection: &Connection) -> Result<()> {
    if !has_table(connection, "operator_conversations")? {
        return Ok(());
    }
    ensure_singleton_columns(connection)?;
    create_operator_indexes(connection)
}

fn has_column(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
        params![table, column],
        |row| row.get(0),
    )?)
}

/// Restart retires turn authority without certifying process-group exit.
pub(crate) fn interrupt_unfinished(connection: &Connection) -> Result<()> {
    if !has_table(connection, "operator_turns")? {
        return Ok(());
    }
    let transaction = connection.unchecked_transaction()?;
    let rows: Vec<(i64, i64)> = transaction
        .prepare("SELECT id,conversation FROM operator_turns WHERE state='Running'")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (turn, conversation) in rows {
        append_message(
            &transaction,
            conversation,
            turn,
            &OperatorMessageContent::Interrupted,
        )?;
    }
    transaction.execute(
        "UPDATE operator_turns SET state='Interrupted' WHERE state='Running'",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}
fn append_message(
    connection: &Connection,
    conversation: i64,
    turn: i64,
    content: &OperatorMessageContent,
) -> Result<()> {
    connection.execute(
        "INSERT INTO operator_messages(conversation,turn,content) VALUES(?1,?2,?3)",
        params![conversation, turn, serde_json::to_string(content)?],
    )?;
    Ok(())
}
fn read_turn(
    connection: &Connection,
    conversation: OperatorConversationId,
) -> Result<Option<OperatorTurn>> {
    let row: Option<(u64,u64,u64,String,u64)> = connection.query_row("SELECT t.id,c.session,t.run,t.state,t.sequence FROM operator_turns t JOIN operator_conversations c ON c.id=t.conversation WHERE t.conversation=?1 ORDER BY t.id DESC LIMIT 1", [sql_integer(conversation.0)?], |r| Ok((read_integer(r,0)?,read_integer(r,1)?,read_integer(r,2)?,r.get(3)?,read_integer(r,4)?))).optional()?;
    row.map(|(turn, session, run, state, sequence)| {
        Ok(OperatorTurn {
            identity: OperatorTurnIdentity {
                conversation,
                turn: OperatorTurnId(turn),
                session: SessionId(session),
                run: SessionRunId(run),
            },
            state: serde_json::from_str(&format!("\"{state}\""))?,
            sequence,
        })
    })
    .transpose()
}
impl SessionStore {
    /// The sole conversation, if it exists. Never creates a record or a process.
    pub(crate) fn current_operator_conversation(
        &self,
    ) -> Result<Option<OperatorConversationSummary>> {
        let id: Option<u64> = self
            .connection
            .query_row(
                "SELECT id FROM operator_conversations WHERE singleton=1",
                [],
                |r| read_integer(r, 0),
            )
            .optional()?;
        id.map(|id| self.operator_conversation(OperatorConversationId(id)))
            .transpose()
    }

    /// Atomically gets or creates the sole conversation. Repeats return the same
    /// IDs without resetting state; this never starts a worker.
    pub(crate) fn open_operator_conversation(
        &mut self,
        cwd: &Path,
    ) -> Result<OperatorConversationSummary> {
        if !cwd.is_absolute() {
            bail!("operator working directory must be absolute");
        }
        // Immediate takes the write lock before the read, so two owners cannot both
        // decide the singleton is missing.
        let transaction = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: Option<(u64, u64)> = transaction
            .query_row(
                "SELECT id,session FROM operator_conversations WHERE singleton=1",
                [],
                |r| Ok((read_integer(r, 0)?, read_integer(r, 1)?)),
            )
            .optional()?;
        let Some((conversation, session)) = existing else {
            let conversation: u64 = transaction.query_row("SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='operator_conversations'),0)+1", [], |r| read_integer(r,0))?;
            let scope = ovrcr_protocol::SessionScope::OperatorConversation {
                conversation: OperatorConversationId(conversation),
            };
            let metadata = SessionMetadata {
                scope,
                project: String::new(),
                workspace: String::new(),
                name: format!("operator-{conversation}"),
                label: "Operator".into(),
                cwd: cwd.into(),
                kind: SessionKind::Agent {
                    name: "operator".into(),
                },
                pinned_title: None,
                application_title: None,
            };
            transaction.execute("INSERT INTO retained_sessions(run,project,workspace,name,label,cwd,kind,title_revision,stopped,scope) VALUES(0,'','',?1,?2,?3,?4,0,1,?5)", params![metadata.name,metadata.label,cwd.as_os_str().as_bytes(),serde_json::to_string(&metadata.kind)?,serde_json::to_string(&scope)?])?;
            let session = SessionId(transaction.last_insert_rowid().try_into()?);
            // Blank provider/model stay `model: None` until the first ModelSelected.
            let inserted = transaction.execute("INSERT INTO operator_conversations(id,session,provider,model,title,singleton) VALUES(?1,?2,'','','Operator',1)",params![sql_integer(conversation)?,sql_integer(session.0)?]);
            if let Err(error) = inserted {
                // A foreign owner may have created the singleton between our read
                // and this write. Re-read it and hydrate the retained row it
                // committed after this store loaded its snapshot.
                drop(transaction);
                let existing = self.connection.query_row(
                    "SELECT id,session FROM operator_conversations WHERE singleton=1",
                    [],
                    |r| Ok((read_integer(r, 0)?, read_integer(r, 1)?)),
                );
                match existing.optional() {
                    Ok(Some((foreign, foreign_session))) => {
                        self.hydrate_operator_session(SessionId(foreign_session))?;
                        return self.operator_conversation(OperatorConversationId(foreign));
                    }
                    // The competing row is not committed or visible yet; return the
                    // insert error so the caller can retry Open.
                    _ => return Err(error.into()),
                }
            }
            transaction.commit()?;
            self.hydrate_operator_session(session)?;
            return self.operator_conversation(OperatorConversationId(conversation));
        };
        transaction.commit()?;
        self.hydrate_operator_session(SessionId(session))?;
        self.operator_conversation(OperatorConversationId(conversation))
    }

    /// This store loaded its retained snapshot before another owner committed the
    /// singleton. Read that row through the ordinary loader instead of hand-building
    /// a record, so later Current/Read/Submit see a complete session.
    fn hydrate_operator_session(&mut self, session: SessionId) -> Result<()> {
        if self.records.contains_key(&session) {
            return Ok(());
        }
        let records =
            super::read_records(&self.connection, super::has_disposition(&self.connection)?)?;
        let record = records
            .get(&session)
            .context("retained operator session missing")?
            .clone();
        self.records.insert(session, record);
        Ok(())
    }

    pub(crate) fn operator_conversation(
        &self,
        id: OperatorConversationId,
    ) -> Result<OperatorConversationSummary> {
        let (session, provider, model, title, singleton): (u64, String, String, String, i64) = self
            .connection
            .query_row(
                "SELECT session,provider,model,title,singleton FROM operator_conversations WHERE id=?1",
                [sql_integer(id.0)?],
                |r| {
                    Ok((
                        read_integer(r, 0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                    ))
                },
            )
            .optional()?
            .context("operator conversation not found")?;
        let record = self
            .records
            .get(&SessionId(session))
            .context("retained operator session missing")?;
        // Legacy rows store an old enum identifier; never decode it as a Pi model.
        let model = (singleton == 1 && !(provider.is_empty() && model.is_empty())).then_some(
            OperatorModel {
                provider,
                id: model,
            },
        );
        Ok(OperatorConversationSummary {
            run: record.run,
            worker_stopped: record.stopped,
            id,
            session: SessionId(session),
            model,
            title,
            current_turn: read_turn(&self.connection, id)?,
        })
    }

    /// Ordinary Operator operations belong to the singleton only; legacy rows
    /// stay readable for recovery but never accept new work.
    pub(crate) fn require_current_operator(&self, id: OperatorConversationId) -> Result<()> {
        let singleton: Option<i64> = self
            .connection
            .query_row(
                "SELECT singleton FROM operator_conversations WHERE id=?1",
                [sql_integer(id.0)?],
                |r| r.get(0),
            )
            .optional()?;
        if singleton != Some(1) {
            bail!("operator conversation is not the current conversation");
        }
        Ok(())
    }

    pub(crate) fn read_operator_history(
        &self,
        id: OperatorConversationId,
        after: u64,
        limit: u16,
    ) -> Result<OperatorHistory> {
        OperatorRequest::Read {
            conversation: id,
            after,
            limit,
        }
        .validate()
        .map_err(anyhow::Error::msg)?;
        self.require_current_operator(id)?;
        let conversation = self.operator_conversation(id)?;
        let mut query = self.connection.prepare("SELECT ordinal,turn,content FROM operator_messages WHERE conversation=?1 AND ordinal>?2 ORDER BY ordinal LIMIT ?3")?;
        let rows = query.query_map(
            params![
                sql_integer(id.0)?,
                sql_integer(after)?,
                u32::from(limit) + 1
            ],
            |r| {
                Ok((
                    read_integer(r, 0)?,
                    read_integer(r, 1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )?;
        let mut messages = Vec::new();
        let mut bytes = 0;
        let mut more = false;
        for row in rows {
            let (ordinal, turn, raw) = row?;
            if messages.len() == usize::from(limit) || bytes + raw.len() > 512 * 1024 {
                more = true;
                break;
            }
            bytes += raw.len();
            messages.push(OperatorMessage {
                ordinal,
                turn: OperatorTurnId(turn),
                content: serde_json::from_str(&raw)?,
            });
        }
        let next_after = if more {
            messages.last().map(|m| m.ordinal)
        } else {
            None
        };
        Ok(OperatorHistory {
            conversation,
            messages,
            next_after,
        })
    }
    /// Caller holds mutation_lock and has admitted one of the fifty live slots.
    /// Persists launch intent and the user message atomically, without spawning.
    pub(crate) fn submit_operator_turn(
        &mut self,
        id: OperatorConversationId,
        expected_run: SessionRunId,
        text: &str,
        context: OperatorContext,
        model: Option<String>,
    ) -> Result<OperatorTurn> {
        OperatorRequest::Submit {
            conversation: id,
            expected_run,
            text: text.into(),
            context: context.clone(),
        }
        .validate()
        .map_err(anyhow::Error::msg)?;
        self.require_current_operator(id)?;
        if let Some(model) = &model {
            validate_operator_model(model).map_err(anyhow::Error::msg)?;
        }
        let conversation = self.operator_conversation(id)?;
        if conversation
            .current_turn
            .as_ref()
            .is_some_and(|t| t.state == OperatorTurnState::Running)
        {
            bail!("operator conversation is busy");
        }
        let row = self
            .records
            .get(&conversation.session)
            .context("retained operator session missing")?;
        if row.run != expected_run
            || !matches!(row.disposition, Disposition::Active | Disposition::Returned)
            || !row.stopped
        {
            bail!("operator session run changed or ownership remains uncertain");
        }
        let run = SessionRunId(
            expected_run
                .0
                .checked_add(1)
                .context("session run exhausted")?,
        );
        let transaction = self.connection.transaction()?;
        let changed=transaction.execute("UPDATE retained_sessions SET run=?1,boot_id=?2,stopped=0,failure=NULL,title_revision=0,disposition=0 WHERE id=?3 AND run=?4 AND stopped=1 AND disposition IN (0,2)",params![sql_integer(run.0)?,self.boot_id,sql_integer(conversation.session.0)?,sql_integer(expected_run.0)?])?;
        if changed != 1 {
            bail!("operator session run changed");
        }
        transaction.execute(
            "INSERT INTO operator_turns(conversation,run,state,model_override) VALUES(?1,?2,'Running',?3)",
            params![sql_integer(id.0)?, sql_integer(run.0)?, model],
        )?;
        let turn = OperatorTurnId(transaction.last_insert_rowid().try_into()?);
        append_message(
            &transaction,
            sql_integer(id.0)?,
            sql_integer(turn.0)?,
            &OperatorMessageContent::User {
                text: text.into(),
                context,
            },
        )?;
        transaction.commit()?;
        let row = self.records.get_mut(&conversation.session).unwrap();
        row.run = run;
        row.disposition = Disposition::Active;
        row.stopped = false;
        row.boot_id = self.boot_id.clone();
        row.failure = None;
        row.title_revision = 0;
        Ok(OperatorTurn {
            identity: OperatorTurnIdentity {
                conversation: id,
                turn,
                session: conversation.session,
                run,
            },
            state: OperatorTurnState::Running,
            sequence: 0,
        })
    }
    fn validate_operator_identity(&self, identity: OperatorTurnIdentity) -> Result<OperatorTurn> {
        identity.validate().map_err(anyhow::Error::msg)?;
        let turn =
            read_turn(&self.connection, identity.conversation)?.context("operator turn missing")?;
        let row = self
            .records
            .get(&identity.session)
            .context("operator session missing")?;
        if turn.identity != identity
            || row.run != identity.run
            || row.metadata.scope
                != (ovrcr_protocol::SessionScope::OperatorConversation {
                    conversation: identity.conversation,
                })
        {
            bail!("stale operator turn identity");
        }
        Ok(turn)
    }
    /// Authenticated caller persists before acknowledging or notifying the Dashboard.
    pub(crate) fn apply_operator_batch(
        &mut self,
        batch: &OperatorEventBatch,
    ) -> Result<OperatorExchangeResult> {
        use sha2::{Digest, Sha256};
        batch.validate().map_err(anyhow::Error::msg)?;
        self.require_current_operator(batch.identity.conversation)?;
        let turn = self.validate_operator_identity(batch.identity)?;
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(batch)?));
        let prior: Option<String> = self
            .connection
            .query_row(
                "SELECT digest FROM operator_batches WHERE turn=?1 AND sequence=?2",
                params![
                    sql_integer(batch.identity.turn.0)?,
                    sql_integer(batch.sequence)?
                ],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(prior) = prior {
            if prior != digest {
                bail!("conflicting operator batch replay");
            }
            return Ok(OperatorExchangeResult {
                identity: batch.identity,
                sequence: batch.sequence,
                duplicate: true,
            });
        }
        if turn.state != OperatorTurnState::Running
            || turn.sequence.checked_add(1) != Some(batch.sequence)
        {
            bail!("stale or out-of-order operator batch");
        }
        let transaction = self.connection.transaction()?;
        let mut state = "Running";
        for event in &batch.events {
            // Metadata events update the retained conversation without entering
            // the visible transcript.
            match event {
                OperatorWorkerEvent::ModelSelected(model) => {
                    transaction.execute(
                        "UPDATE operator_conversations SET provider=?1,model=?2 WHERE id=?3 AND singleton=1",
                        params![model.provider, model.id, sql_integer(batch.identity.conversation.0)?],
                    )?;
                    continue;
                }
                OperatorWorkerEvent::Completed => {
                    state = "Complete";
                    continue;
                }
                OperatorWorkerEvent::Failed(_) => state = "Error",
                _ => {}
            }
            append_message(
                &transaction,
                sql_integer(batch.identity.conversation.0)?,
                sql_integer(batch.identity.turn.0)?,
                &OperatorMessageContent::Worker(event.clone()),
            )?;
        }
        transaction.execute(
            "UPDATE operator_turns SET state=?1,sequence=?2 WHERE id=?3",
            params![
                state,
                sql_integer(batch.sequence)?,
                sql_integer(batch.identity.turn.0)?
            ],
        )?;
        if state == "Complete" {
            // Completed promotes this batch's native session as the next checkpoint
            // in the same transaction as the turn state and its receipt.
            transaction.execute(
                "UPDATE operator_conversations SET pi_checkpoint_turn=?1 WHERE id=?2",
                params![
                    sql_integer(batch.identity.turn.0)?,
                    sql_integer(batch.identity.conversation.0)?
                ],
            )?;
        }
        transaction.execute(
            "INSERT INTO operator_batches(turn,sequence,digest) VALUES(?1,?2,?3)",
            params![
                sql_integer(batch.identity.turn.0)?,
                sql_integer(batch.sequence)?,
                digest
            ],
        )?;
        transaction.commit()?;
        Ok(OperatorExchangeResult {
            identity: batch.identity,
            sequence: batch.sequence,
            duplicate: false,
        })
    }
    /// Does not certify controlled process stop. Repeated Stop preserves final state.
    pub(crate) fn interrupt_operator_turn(
        &mut self,
        identity: OperatorTurnIdentity,
    ) -> Result<OperatorTurn> {
        let mut turn = self.validate_operator_identity(identity)?;
        if turn.state != OperatorTurnState::Running {
            return Ok(turn);
        }
        let transaction = self.connection.transaction()?;
        append_message(
            &transaction,
            sql_integer(identity.conversation.0)?,
            sql_integer(identity.turn.0)?,
            &OperatorMessageContent::Interrupted,
        )?;
        transaction.execute(
            "UPDATE operator_turns SET state='Interrupted' WHERE id=?1",
            [sql_integer(identity.turn.0)?],
        )?;
        transaction.commit()?;
        turn.state = OperatorTurnState::Interrupted;
        Ok(turn)
    }
    pub(crate) fn operator_worker_start(
        &self,
        identity: OperatorTurnIdentity,
    ) -> Result<OperatorWorkerStart> {
        self.require_current_operator(identity.conversation)?;
        let turn = self.validate_operator_identity(identity)?;
        if turn.state != OperatorTurnState::Running || turn.sequence != 0 {
            bail!("operator worker bootstrap is no longer current");
        }
        let model: Option<String> = self.connection.query_row(
            "SELECT model_override FROM operator_turns WHERE id=?1",
            [sql_integer(identity.turn.0)?],
            |r| r.get(0),
        )?;
        let checkpoint_turn: Option<i64> = self.connection.query_row(
            "SELECT pi_checkpoint_turn FROM operator_conversations WHERE id=?1 AND singleton=1",
            [sql_integer(identity.conversation.0)?],
            |r| r.get(0),
        )?;
        let raw: String = self.connection.query_row(
            "SELECT content FROM operator_messages WHERE turn=?1 ORDER BY ordinal LIMIT 1",
            [sql_integer(identity.turn.0)?],
            |r| r.get(0),
        )?;
        let OperatorMessageContent::User { text, context } = serde_json::from_str(&raw)? else {
            bail!("operator submission missing")
        };
        let checkpoint_turn = checkpoint_turn
            .map(|turn| -> Result<OperatorTurnId> {
                let turn: u64 = turn
                    .try_into()
                    .context("saved operator checkpoint turn is invalid")?;
                if turn == 0 {
                    bail!("saved operator checkpoint turn is invalid");
                }
                Ok(OperatorTurnId(turn))
            })
            .transpose()?;
        Ok(OperatorWorkerStart {
            identity,
            model,
            text,
            context,
            checkpoint_turn,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn open(store: &mut SessionStore) -> OperatorConversationSummary {
        store
            .open_operator_conversation(Path::new("/tmp/operator"))
            .unwrap()
    }
    fn submit(
        store: &mut SessionStore,
        conversation: &OperatorConversationSummary,
        model: Option<&str>,
    ) -> OperatorTurn {
        let run = store.get(conversation.session).unwrap().run;
        store
            .submit_operator_turn(
                conversation.id,
                run,
                "hello",
                OperatorContext::default(),
                model.map(str::to_owned),
            )
            .unwrap()
    }
    fn batch(turn: &OperatorTurn, sequence: u64, text: &str) -> OperatorEventBatch {
        OperatorEventBatch {
            identity: turn.identity,
            sequence,
            events: vec![OperatorWorkerEvent::AssistantDelta(text.into())],
        }
    }
    fn completed(turn: &OperatorTurn, sequence: u64) -> OperatorEventBatch {
        OperatorEventBatch {
            identity: turn.identity,
            sequence,
            events: vec![OperatorWorkerEvent::Completed],
        }
    }
    fn checkpoint(store: &SessionStore, id: OperatorConversationId) -> Option<i64> {
        store
            .connection
            .query_row(
                "SELECT pi_checkpoint_turn FROM operator_conversations WHERE id=?1",
                [sql_integer(id.0).unwrap()],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn column_specs(
        store: &SessionStore,
        table: &str,
    ) -> Vec<(String, String, i64, Option<String>, i64)> {
        column_specs_on(&store.connection, table)
    }
    fn column_specs_on(
        connection: &Connection,
        table: &str,
    ) -> Vec<(String, String, i64, Option<String>, i64)> {
        connection
            .prepare(
                "SELECT name,type,\"notnull\",dflt_value,pk FROM pragma_table_info(?1) ORDER BY cid",
            )
            .unwrap()
            .query_map([table], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }
    fn index_specs(
        store: &SessionStore,
        table: &str,
    ) -> Vec<(String, i64, i64, Vec<String>, String)> {
        index_specs_on(&store.connection, table)
    }
    fn index_specs_on(
        connection: &Connection,
        table: &str,
    ) -> Vec<(String, i64, i64, Vec<String>, String)> {
        let names: Vec<String> = connection
            .prepare(
                "SELECT name FROM pragma_index_list(?1) WHERE name NOT GLOB 'sqlite_*' ORDER BY name",
            )
            .unwrap()
            .query_map([table], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        names
            .into_iter()
            .map(|name| {
                let (unique, partial): (i64, i64) = connection
                    .query_row(
                        "SELECT \"unique\",partial FROM pragma_index_list(?1) WHERE name=?2",
                        params![table, name],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                let indexed: Vec<String> = connection
                    .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
                    .unwrap()
                    .query_map([name.as_str()], |r| r.get(0))
                    .unwrap()
                    .collect::<rusqlite::Result<_>>()
                    .unwrap();
                let sql: String = connection
                    .query_row(
                        "SELECT sql FROM sqlite_schema WHERE type='index' AND name=?1",
                        [name.as_str()],
                        |r| r.get(0),
                    )
                    .unwrap();
                (name, unique, partial, indexed, sql)
            })
            .collect()
    }
    fn table_sql(store: &SessionStore, table: &str) -> String {
        table_sql_on(&store.connection, table)
    }
    fn table_sql_on(connection: &Connection, table: &str) -> String {
        connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn raw_messages(store: &SessionStore, conversation: OperatorConversationId) -> Vec<String> {
        raw_messages_on(&store.connection, conversation)
    }
    fn raw_messages_on(
        connection: &Connection,
        conversation: OperatorConversationId,
    ) -> Vec<String> {
        connection
            .prepare("SELECT content FROM operator_messages WHERE conversation=?1 ORDER BY ordinal")
            .unwrap()
            .query_map([sql_integer(conversation.0).unwrap()], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// Rebuild the two Operator tables into their schema-28 shape, dropping the
    /// singleton/checkpoint/model-capture columns a real version-28 owner has.
    fn downgrade_to_schema_28(connection: &Connection) {
        downgrade_operator_tables(connection, 28);
    }

    fn downgrade_operator_tables(connection: &Connection, user_version: i64) {
        connection
            .pragma_update(None, "foreign_keys", false)
            .unwrap();
        connection
            .execute_batch(
                "DROP INDEX IF EXISTS operator_single_conversation;
                 DROP INDEX IF EXISTS operator_one_running_turn;
                 CREATE TABLE conversations28 (
                    id INTEGER PRIMARY KEY AUTOINCREMENT CHECK(id>0),
                    session INTEGER NOT NULL UNIQUE REFERENCES retained_sessions(id) ON DELETE RESTRICT,
                    provider TEXT NOT NULL, model TEXT NOT NULL, title TEXT NOT NULL);
                 INSERT INTO conversations28 SELECT id,session,provider,model,title FROM operator_conversations;
                 DROP TABLE operator_conversations;
                 ALTER TABLE conversations28 RENAME TO operator_conversations;
                 CREATE TABLE turns28 (
                    id INTEGER PRIMARY KEY AUTOINCREMENT CHECK(id>0),
                    conversation INTEGER NOT NULL REFERENCES operator_conversations(id) ON DELETE CASCADE,
                    run INTEGER NOT NULL CHECK(run>0), state TEXT NOT NULL,
                    sequence INTEGER NOT NULL DEFAULT 0, UNIQUE(conversation,run));
                 INSERT INTO turns28 SELECT id,conversation,run,state,sequence FROM operator_turns;
                 DROP TABLE operator_turns;
                 ALTER TABLE turns28 RENAME TO operator_turns;",
            )
            .unwrap();
        connection
            .pragma_update(None, "user_version", user_version)
            .unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
    }

    #[test]
    fn open_is_idempotent_and_current_never_creates_anything() {
        let mut store = SessionStore::in_memory();
        assert!(store.current_operator_conversation().unwrap().is_none());
        assert!(store.records().next().is_none());
        let first = open(&mut store);
        assert_eq!(first.title, "Operator");
        assert!(first.worker_stopped);
        assert_eq!(first.run, SessionRunId(0));
        assert!(first.current_turn.is_none());
        assert!(first.model.is_none());
        assert_eq!(first.session, store.get(first.session).unwrap().id);
        assert!(
            store
                .get(first.session)
                .unwrap()
                .summary(None)
                .pid
                .is_none()
        );
        assert!(
            store
                .read_operator_history(first.id, 0, 10)
                .unwrap()
                .messages
                .is_empty()
        );
        assert_eq!(
            store.current_operator_conversation().unwrap(),
            Some(first.clone())
        );
        for _ in 0..3 {
            assert_eq!(open(&mut store), first);
        }
        let singleton: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM operator_conversations WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(singleton, 1);
        assert_eq!(store.records().count(), 1);
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("operator-workdir");
        assert_eq!(store.open_operator_conversation(&cwd).unwrap(), first);
        assert!(
            !cwd.exists(),
            "opening the durable record never creates the agent working directory"
        );
    }

    #[test]
    fn the_database_refuses_a_second_singleton_row() {
        let mut store = SessionStore::in_memory();
        let first = open(&mut store);
        // Any other retained session proves the index still refuses a second marker.
        let other = store
            .create(SessionMetadata {
                scope: ovrcr_protocol::SessionScope::Workspace,
                project: "p".into(),
                workspace: "w".into(),
                name: "other".into(),
                label: "other".into(),
                cwd: "/tmp/operator".into(),
                kind: SessionKind::Terminal,
                pinned_title: None,
                application_title: None,
            })
            .unwrap();
        assert!(
            store
                .connection
                .execute(
                    "INSERT INTO operator_conversations(session,provider,model,title,singleton) VALUES(?1,'','','Operator',1)",
                    [sql_integer(other.id.0).unwrap()]
                )
                .is_err(),
            "the partial unique index is the last line of defence for the singleton"
        );
        assert_eq!(
            store.current_operator_conversation().unwrap().unwrap().id,
            first.id
        );
        // An unmarked row is still allowed, as every legacy row is.
        store
            .connection
            .execute(
                "INSERT INTO operator_conversations(session,provider,model,title,singleton) VALUES(?1,'','','Operator',0)",
                [sql_integer(other.id.0).unwrap()]
            )
            .unwrap();
        assert_eq!(
            store.current_operator_conversation().unwrap().unwrap().id,
            first.id
        );
    }

    #[test]
    fn a_store_that_missed_the_singleton_hydrates_the_foreign_row() {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("operator-workdir");
        let mut winner = SessionStore::open(root.path()).unwrap();
        // The loser loaded its retained snapshot before the singleton existed.
        let mut loser = SessionStore::open(root.path()).unwrap();
        assert!(loser.records().next().is_none());
        let created = winner.open_operator_conversation(&cwd).unwrap();
        let opened = loser.open_operator_conversation(&cwd).unwrap();
        assert_eq!(opened, created);
        assert_eq!(
            loser.current_operator_conversation().unwrap(),
            Some(created.clone())
        );
        assert_eq!(loser.get(created.session).unwrap().id, created.session);
        assert!(loser.read_operator_history(created.id, 0, 10).is_ok());
        // A store opened after the commit already loaded the row normally.
        let late = SessionStore::open(root.path()).unwrap();
        assert_eq!(late.current_operator_conversation().unwrap(), Some(created));
    }

    #[test]
    fn open_conflict_without_a_visible_singleton_returns_the_insert_error() {
        let mut store = SessionStore::in_memory();
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER refuse_operator_conversation
                 BEFORE INSERT ON operator_conversations
                 BEGIN SELECT RAISE(ABORT,'fixture refusal'); END;",
            )
            .unwrap();
        let error = store
            .open_operator_conversation(Path::new("/tmp/operator"))
            .unwrap_err();
        assert!(
            error.to_string().contains("fixture refusal"),
            "an uncommitted competitor keeps the insert error, got {error}"
        );
        assert!(store.current_operator_conversation().unwrap().is_none());
        assert!(
            store.records().next().is_none(),
            "the transaction rolled back"
        );
        store
            .connection
            .execute_batch("DROP TRIGGER refuse_operator_conversation")
            .unwrap();
        assert!(
            store
                .open_operator_conversation(Path::new("/tmp/operator"))
                .is_ok()
        );
    }

    #[test]
    fn batches_store_model_selection_without_copying_metadata_into_history() {
        let mut store = SessionStore::in_memory();
        let c = open(&mut store);
        let t = submit(&mut store, &c, None);
        assert!(
            store
                .operator_worker_start(t.identity)
                .unwrap()
                .model
                .is_none()
        );
        let selection = OperatorEventBatch {
            identity: t.identity,
            sequence: 1,
            events: vec![
                OperatorWorkerEvent::ModelSelected(OperatorModel {
                    provider: "openai".into(),
                    id: "gpt-5.1".into(),
                }),
                OperatorWorkerEvent::AssistantDelta("partial".into()),
            ],
        };
        assert!(!store.apply_operator_batch(&selection).unwrap().duplicate);
        assert!(store.apply_operator_batch(&selection).unwrap().duplicate);
        let stored = store.operator_conversation(c.id).unwrap();
        assert_eq!(
            stored.model,
            Some(OperatorModel {
                provider: "openai".into(),
                id: "gpt-5.1".into()
            })
        );
        assert_eq!(
            raw_messages(&store, c.id).len(),
            2,
            "only the user message and assistant text are in the transcript"
        );
        assert!(
            store
                .apply_operator_batch(&batch(&t, 1, "conflict"))
                .is_err(),
            "the same sequence with a different digest is a conflicting replay"
        );
        assert!(store.apply_operator_batch(&batch(&t, 3, "gap")).is_err());
        let mut stale = batch(&t, 2, "stale");
        stale.identity.run.0 += 1;
        assert!(store.apply_operator_batch(&stale).is_err());
        assert!(
            !store
                .apply_operator_batch(&completed(&t, 2))
                .unwrap()
                .duplicate
        );
        assert_eq!(
            store
                .operator_conversation(c.id)
                .unwrap()
                .current_turn
                .unwrap()
                .state,
            OperatorTurnState::Complete
        );
        assert_eq!(checkpoint(&store, c.id), Some(t.identity.turn.0 as i64));
        assert!(
            !store.get(c.session).unwrap().stopped,
            "model completion is not process-stop proof"
        );
        assert!(store.apply_operator_batch(&batch(&t, 3, "late")).is_err());
    }

    #[test]
    fn only_completed_promotes_the_checkpoint_and_does_so_with_its_receipt() {
        let mut store = SessionStore::in_memory();
        let c = open(&mut store);
        let t = submit(&mut store, &c, Some("openai/gpt-5.1"));
        let mut failed_batch = completed(&t, 1);
        failed_batch.events.insert(
            0,
            OperatorWorkerEvent::ModelSelected(OperatorModel {
                provider: "openai".into(),
                id: "gpt-5.1".into(),
            }),
        );
        store
            .connection
            .execute_batch("CREATE TRIGGER reject_operator_batch BEFORE INSERT ON operator_batches BEGIN SELECT RAISE(ABORT,'fixture failure'); END;")
            .unwrap();
        assert!(store.apply_operator_batch(&failed_batch).is_err());
        assert_eq!(checkpoint(&store, c.id), None);
        assert_eq!(
            store
                .operator_conversation(c.id)
                .unwrap()
                .current_turn
                .unwrap()
                .state,
            OperatorTurnState::Running
        );
        assert_eq!(
            store
                .operator_conversation(c.id)
                .unwrap()
                .current_turn
                .unwrap()
                .sequence,
            0
        );
        assert!(store.operator_conversation(c.id).unwrap().model.is_none());
        assert_eq!(raw_messages(&store, c.id).len(), 1);
        store
            .connection
            .execute_batch("DROP TRIGGER reject_operator_batch")
            .unwrap();
        assert!(!store.apply_operator_batch(&failed_batch).unwrap().duplicate);
        assert_eq!(checkpoint(&store, c.id), Some(t.identity.turn.0 as i64));
        store.mark_stopped(c.session, t.identity.run).unwrap();
        let next = submit(&mut store, &c, None);
        let start = store.operator_worker_start(next.identity).unwrap();
        assert_eq!(start.checkpoint_turn, Some(t.identity.turn));
        assert!(start.model.is_none());
        assert_eq!(checkpoint(&store, c.id), Some(t.identity.turn.0 as i64));
        store
            .apply_operator_batch(&OperatorEventBatch {
                identity: next.identity,
                sequence: 1,
                events: vec![OperatorWorkerEvent::Failed("unsupported".into())],
            })
            .unwrap();
        assert_eq!(
            checkpoint(&store, c.id),
            Some(t.identity.turn.0 as i64),
            "a failed batch never promotes its own attempt"
        );
    }

    #[test]
    fn submit_captures_the_model_override_for_that_turn_only() {
        let mut store = SessionStore::in_memory();
        let c = open(&mut store);
        let run = store.get(c.session).unwrap().run;
        // Refuse these on an idle conversation so an error can only come from model
        // validation, not from a busy or stale turn.
        for invalid in [" ", "model-without-provider", "/model"] {
            let error = store
                .submit_operator_turn(
                    c.id,
                    run,
                    "hello",
                    OperatorContext::default(),
                    Some(invalid.into()),
                )
                .unwrap_err();
            assert!(
                error.to_string().contains("invalid operator model"),
                "{invalid} must be refused by model validation, got {error}"
            );
        }
        assert_eq!(store.get(c.session).unwrap().run, run);
        assert!(
            store
                .operator_conversation(c.id)
                .unwrap()
                .current_turn
                .is_none()
        );
        let first = submit(&mut store, &c, Some("openai/gpt-5.1-codex/mini"));
        assert_eq!(
            store
                .operator_worker_start(first.identity)
                .unwrap()
                .model
                .as_deref(),
            Some("openai/gpt-5.1-codex/mini")
        );
        assert!(store.operator_worker_start(first.identity).is_ok());
        store.interrupt_operator_turn(first.identity).unwrap();
        store.mark_stopped(c.session, first.identity.run).unwrap();
        let second = submit(&mut store, &c, None);
        assert!(
            store
                .operator_worker_start(second.identity)
                .unwrap()
                .model
                .is_none()
        );
        assert!(store.operator_worker_start(first.identity).is_err());
    }

    #[test]
    fn corrupt_saved_checkpoint_turn_fails_worker_start_instead_of_resetting_context() {
        let mut store = SessionStore::in_memory();
        let c = open(&mut store);
        let t = submit(&mut store, &c, None);
        let set_checkpoint = |store: &SessionStore, turn: i64| {
            store
                .connection
                .execute(
                    "UPDATE operator_conversations SET pi_checkpoint_turn=?1 WHERE id=?2",
                    params![turn, sql_integer(c.id.0).unwrap()],
                )
                .unwrap();
        };
        set_checkpoint(&store, t.identity.turn.0 as i64);
        assert_eq!(
            store
                .operator_worker_start(t.identity)
                .unwrap()
                .checkpoint_turn,
            Some(t.identity.turn)
        );
        for invalid in [0i64, -1, i64::MIN] {
            set_checkpoint(&store, invalid);
            let error = store.operator_worker_start(t.identity).unwrap_err();
            assert!(
                error.to_string().contains("checkpoint"),
                "checkpoint {invalid} must fail closed, got {error}"
            );
        }
        // A corrupt saved identity stays on disk; nothing silently clears it.
        let raw: Option<i64> = store
            .connection
            .query_row(
                "SELECT pi_checkpoint_turn FROM operator_conversations WHERE id=?1",
                [sql_integer(c.id.0).unwrap()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, Some(i64::MIN));
    }

    #[test]
    fn interrupt_and_restart_preserve_the_visible_journal_without_replay() {
        let root = tempfile::tempdir().unwrap();
        let (c, t) = {
            let mut store = SessionStore::open(root.path()).unwrap();
            let c = open(&mut store);
            let t = submit(&mut store, &c, None);
            store
                .apply_operator_batch(&batch(&t, 1, "partial"))
                .unwrap();
            (c, t)
        };
        let mut store = SessionStore::open(root.path()).unwrap();
        let history = store.read_operator_history(c.id, 0, 100).unwrap();
        assert_eq!(
            history.conversation.current_turn.unwrap().state,
            OperatorTurnState::Interrupted
        );
        assert_eq!(history.messages.len(), 3);
        assert!(store.apply_operator_batch(&batch(&t, 2, "late")).is_err());
        assert!(store.get(c.session).unwrap().requires_ack(store.boot_id()));
        assert!(
            store
                .submit_operator_turn(
                    c.id,
                    t.identity.run,
                    "unsafe",
                    OperatorContext::default(),
                    None
                )
                .is_err()
        );
        store.mark_stopped(c.session, t.identity.run).unwrap();
        let fresh = store
            .submit_operator_turn(
                c.id,
                store.get(c.session).unwrap().run,
                "fresh",
                OperatorContext::default(),
                None,
            )
            .unwrap();
        assert_eq!(fresh.identity.run.0, t.identity.run.0 + 1);
        assert_eq!(
            store.operator_worker_start(fresh.identity).unwrap().text,
            "fresh"
        );
        assert_eq!(
            store
                .read_operator_history(c.id, 0, 100)
                .unwrap()
                .messages
                .len(),
            4
        );
    }

    #[test]
    fn restart_interrupts_an_unacknowledged_turn_but_never_claims_stop() {
        let root = tempfile::tempdir().unwrap();
        let c = {
            let mut store = SessionStore::open(root.path()).unwrap();
            let c = open(&mut store);
            let t = submit(&mut store, &c, None);
            store
                .apply_operator_batch(&batch(&t, 1, "partial"))
                .unwrap();
            c
        };
        let store = SessionStore::open(root.path()).unwrap();
        let conversation = store.operator_conversation(c.id).unwrap();
        assert!(!conversation.worker_stopped);
        assert_eq!(
            conversation.current_turn.unwrap().state,
            OperatorTurnState::Interrupted
        );
        assert_eq!(
            store
                .read_operator_history(c.id, 0, 100)
                .unwrap()
                .messages
                .len(),
            3
        );
    }

    #[test]
    fn history_pages_fit_a_frame_and_catch_up_without_loss() {
        let mut store = SessionStore::in_memory();
        let c = open(&mut store);
        let t = submit(&mut store, &c, None);
        for sequence in 1..=20 {
            store
                .apply_operator_batch(&batch(&t, sequence, &"x".repeat(MAX_OPERATOR_TEXT_BYTES)))
                .unwrap();
        }
        let mut after = 0;
        let mut total = 0;
        loop {
            let page = store.read_operator_history(c.id, after, 100).unwrap();
            let mut frame = Vec::new();
            ovrcr_protocol::write_frame(
                &mut frame,
                &ovrcr_protocol::Response::Operator(OperatorResponse::History(page.clone())),
            )
            .unwrap();
            total += page.messages.len();
            if let Some(next) = page.next_after {
                assert!(next > after);
                after = next;
            } else {
                break;
            }
        }
        assert_eq!(total, 21);
    }

    #[test]
    fn failed_batch_transaction_preserves_sequence_and_checkpoint() {
        let mut store = SessionStore::in_memory();
        let c = open(&mut store);
        let t = submit(&mut store, &c, None);
        store
            .connection
            .execute_batch("CREATE TRIGGER reject_operator_batch BEFORE INSERT ON operator_batches BEGIN SELECT RAISE(ABORT,'fixture failure'); END;")
            .unwrap();
        assert!(store.apply_operator_batch(&completed(&t, 1)).is_err());
        assert_eq!(raw_messages(&store, c.id).len(), 1);
        assert_eq!(
            store
                .operator_conversation(c.id)
                .unwrap()
                .current_turn
                .unwrap()
                .sequence,
            0
        );
        assert_eq!(checkpoint(&store, c.id), None);
        store
            .connection
            .execute_batch("DROP TRIGGER reject_operator_batch")
            .unwrap();
        assert!(
            !store
                .apply_operator_batch(&completed(&t, 1))
                .unwrap()
                .duplicate
        );
        assert_eq!(checkpoint(&store, c.id), Some(t.identity.turn.0 as i64));
    }

    #[test]
    fn legacy_rows_refuse_new_work_but_recovery_and_diagnostics_still_resolve() {
        let mut store = SessionStore::in_memory();
        let legacy = open(&mut store);
        let turn = submit(&mut store, &legacy, None);
        store
            .apply_operator_batch(&batch(&turn, 1, "old partial"))
            .unwrap();
        // Pre-singleton shape: the marker disappears, the rows stay.
        store
            .connection
            .execute("UPDATE operator_conversations SET singleton=0", [])
            .unwrap();
        assert!(store.current_operator_conversation().unwrap().is_none());
        let summary = store.operator_conversation(legacy.id).unwrap();
        assert!(
            summary.model.is_none(),
            "legacy provider text is never decoded"
        );
        assert_eq!(summary.current_turn.unwrap().identity, turn.identity);
        assert!(store.read_operator_history(legacy.id, 0, 100).is_err());
        assert!(
            store
                .submit_operator_turn(
                    legacy.id,
                    turn.identity.run,
                    "revive",
                    OperatorContext::default(),
                    None
                )
                .is_err()
        );
        assert!(
            store
                .apply_operator_batch(&batch(&turn, 2, "late"))
                .is_err()
        );
        assert!(store.operator_worker_start(turn.identity).is_err());
        // Exact-run acknowledgement interrupts the old unfinished turn.
        assert_eq!(
            store.interrupt_operator_turn(turn.identity).unwrap().state,
            OperatorTurnState::Interrupted
        );
        assert_eq!(
            raw_messages(&store, legacy.id),
            vec![
                "{\"User\":{\"text\":\"hello\",\"context\":{\"project\":null,\"workspace\":null,\"workspace_id\":null,\"cwd\":null,\"session\":null,\"run\":null}}}".to_string(),
                "{\"Worker\":{\"AssistantDelta\":\"old partial\"}}".to_string(),
                "\"Interrupted\"".to_string(),
            ]
        );
        let fresh = open(&mut store);
        assert_ne!(fresh.id, legacy.id);
        assert_eq!(
            store.current_operator_conversation().unwrap().unwrap().id,
            fresh.id
        );
        assert_eq!(
            store
                .operator_conversation(legacy.id)
                .unwrap()
                .current_turn
                .unwrap()
                .state,
            OperatorTurnState::Interrupted
        );
    }

    #[test]
    fn schema_28_upgrade_preserves_legacy_rows_and_evidence_exactly() {
        let root = tempfile::tempdir().unwrap();
        let legacy_user_1 = serde_json::to_string(&OperatorMessageContent::User {
            text: "legacy hello".into(),
            context: OperatorContext::default(),
        })
        .unwrap();
        let legacy_user_2 = serde_json::to_string(&OperatorMessageContent::User {
            text: "uncertain".into(),
            context: OperatorContext::default(),
        })
        .unwrap();
        let legacy_delta_2 = serde_json::to_string(&OperatorMessageContent::Worker(
            OperatorWorkerEvent::AssistantDelta("old partial".into()),
        ))
        .unwrap();
        let legacy_history_1 = r#"[{"role":"user","content":"legacy hello"}]"#.to_string();
        let legacy_history_2 = r#"[{"role":"user","content":"uncertain"}]"#.to_string();
        {
            let store = SessionStore::open(root.path()).unwrap();
            downgrade_to_schema_28(&store.connection);
            store
                .connection
                .execute_batch(
                    "DELETE FROM operator_model_history;
                     DELETE FROM operator_messages;
                     DELETE FROM operator_turns;
                     DELETE FROM operator_conversations;
                     DELETE FROM retained_sessions;
                     DELETE FROM sqlite_sequence;
                     INSERT INTO retained_sessions
                        (id,run,project,workspace,name,label,cwd,kind,title_revision,stopped,disposition,scope)
                     VALUES
                        (1,3,'','','operator-1','Operator',X'2f746d702f6f6c64','{\"Agent\":{\"name\":\"operator\"}}',0,1,0,'{\"OperatorConversation\":{\"conversation\":1}}'),
                        (2,1,'','','operator-2','Operator',X'2f746d702f6f6c64','{\"Agent\":{\"name\":\"operator\"}}',0,0,1,'{\"OperatorConversation\":{\"conversation\":2}}');
                     INSERT INTO operator_conversations(id,session,provider,model,title)
                     VALUES (1,1,'\"chat_gpt\"','gpt-x','Old chat'),
                            (2,2,'\"grok\"','grok-x','Old archived');
                     INSERT INTO operator_turns(id,conversation,run,state,sequence)
                     VALUES (1,1,1,'Complete',2),(2,2,1,'Running',0);",
                )
                .unwrap();
            store
                .connection
                .execute(
                    "INSERT INTO operator_messages(conversation,turn,content) VALUES(1,1,?1)",
                    [legacy_user_1.as_str()],
                )
                .unwrap();
            store
                .connection
                .execute(
                    "INSERT INTO operator_messages(conversation,turn,content) VALUES(2,2,?1)",
                    [legacy_user_2.as_str()],
                )
                .unwrap();
            store
                .connection
                .execute(
                    "INSERT INTO operator_messages(conversation,turn,content) VALUES(2,2,?1)",
                    [legacy_delta_2.as_str()],
                )
                .unwrap();
            store
                .connection
                .execute(
                    "INSERT INTO operator_model_history(conversation,turn,history) VALUES(1,1,?1)",
                    [legacy_history_1.as_str()],
                )
                .unwrap();
            store
                .connection
                .execute(
                    "INSERT INTO operator_model_history(conversation,turn,history) VALUES(2,2,?1)",
                    [legacy_history_2.as_str()],
                )
                .unwrap();
            let version: i64 = store
                .connection
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(version, 28);
            let names: Vec<String> = column_specs(&store, "operator_conversations")
                .into_iter()
                .map(|(name, ..)| name)
                .collect();
            assert_eq!(
                names,
                vec![
                    "id".to_string(),
                    "session".to_string(),
                    "provider".to_string(),
                    "model".to_string(),
                    "title".to_string(),
                ],
                "the fixture is a genuine pre-29 Operator table"
            );
        }
        // The real migration entry point, without the restart interruption path, so
        // the assertions below compare exactly what the upgrade wrote.
        let connection = crate::config::open_writable_registry(root.path()).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 29);
        let singleton = column_specs_on(&connection, "operator_conversations")
            .into_iter()
            .find(|(name, ..)| name == "singleton")
            .unwrap();
        assert_eq!(
            singleton,
            (
                "singleton".to_string(),
                "INTEGER".to_string(),
                1,
                Some("0".into()),
                0
            )
        );
        let checkpoint = column_specs_on(&connection, "operator_conversations")
            .into_iter()
            .find(|(name, ..)| name == "pi_checkpoint_turn")
            .unwrap();
        assert_eq!(
            checkpoint,
            (
                "pi_checkpoint_turn".to_string(),
                "INTEGER".to_string(),
                0,
                None,
                0
            )
        );
        let capture = column_specs_on(&connection, "operator_turns")
            .into_iter()
            .find(|(name, ..)| name == "model_override")
            .unwrap();
        assert_eq!(
            capture,
            ("model_override".to_string(), "TEXT".to_string(), 0, None, 0)
        );
        assert!(
            table_sql_on(&connection, "operator_conversations")
                .contains("CHECK(singleton IN (0,1))")
        );
        let single = index_specs_on(&connection, "operator_conversations")
            .into_iter()
            .find(|(name, ..)| name == "operator_single_conversation")
            .unwrap();
        assert_eq!((single.1, single.2), (1, 1), "unique and partial");
        assert_eq!(single.3, vec!["singleton".to_string()]);
        assert!(single.4.contains("WHERE singleton=1"));
        let conversations: Vec<(i64, i64, String, String, String, Option<i64>)> = connection
            .prepare(
                "SELECT id,singleton,provider,model,title,pi_checkpoint_turn
                 FROM operator_conversations ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            conversations,
            vec![
                (
                    1,
                    0,
                    "\"chat_gpt\"".into(),
                    "gpt-x".into(),
                    "Old chat".into(),
                    None
                ),
                (
                    2,
                    0,
                    "\"grok\"".into(),
                    "grok-x".into(),
                    "Old archived".into(),
                    None
                ),
            ],
            "no conversation is inserted and legacy provider text is untouched"
        );
        let turns: Vec<(i64, i64, i64, String, i64, Option<String>)> = connection
            .prepare(
                "SELECT id,conversation,run,state,sequence,model_override
                 FROM operator_turns ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            turns,
            vec![
                (1, 1, 1, "Complete".into(), 2, None),
                (2, 2, 1, "Running".into(), 0, None),
            ]
        );
        let sessions: Vec<(i64, i64, i64, i64)> = connection
            .prepare("SELECT id,run,stopped,disposition FROM retained_sessions ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            sessions,
            vec![(1, 3, 1, 0), (2, 1, 0, 1)],
            "run, stopped proof and disposition are preserved"
        );
        assert_eq!(
            raw_messages_on(&connection, OperatorConversationId(1)),
            vec![legacy_user_1.clone()]
        );
        assert_eq!(
            raw_messages_on(&connection, OperatorConversationId(2)),
            vec![legacy_user_2.clone(), legacy_delta_2.clone()]
        );
        let histories: Vec<(i64, i64, String)> = connection
            .prepare("SELECT conversation,turn,history FROM operator_model_history ORDER BY conversation")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            histories,
            vec![
                (1, 1, legacy_history_1.clone()),
                (2, 2, legacy_history_2.clone()),
            ],
            "legacy model-history bytes are preserved"
        );
        assert!(
            !connection
                .prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap(),
            "the upgrade leaves valid foreign keys"
        );
        drop(connection);
        // The ordinary restart path additionally interrupts only the unfinished turn.
        let mut store = SessionStore::open(root.path()).unwrap();
        let version: i64 = store
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 29);
        migrate_singleton_conversation(&store.connection).unwrap();
        let completed = store
            .operator_conversation(OperatorConversationId(1))
            .unwrap();
        assert!(completed.worker_stopped);
        assert_eq!(completed.run, SessionRunId(3));
        assert_eq!(
            completed.current_turn.unwrap().state,
            OperatorTurnState::Complete
        );
        let uncertain = store
            .operator_conversation(OperatorConversationId(2))
            .unwrap();
        assert!(
            !uncertain.worker_stopped,
            "restart never infers stopped proof"
        );
        assert_eq!(uncertain.run, SessionRunId(1));
        assert_eq!(
            uncertain.current_turn.unwrap().state,
            OperatorTurnState::Interrupted
        );
        assert!(
            store
                .get(uncertain.session)
                .unwrap()
                .requires_ack(store.boot_id())
        );
        assert_eq!(
            raw_messages(&store, OperatorConversationId(1)),
            vec![legacy_user_1],
            "a completed legacy journal is not rewritten"
        );
        assert_eq!(
            raw_messages(&store, OperatorConversationId(2)),
            vec![legacy_user_2, legacy_delta_2, "\"Interrupted\"".to_string()],
            "the uncertain journal keeps its bytes and gains only the restart marker"
        );
        let histories: Vec<(i64, i64, String)> = store
            .connection
            .prepare("SELECT conversation,turn,history FROM operator_model_history ORDER BY conversation")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            histories,
            vec![(1, 1, legacy_history_1), (2, 2, legacy_history_2)]
        );
        assert!(store.current_operator_conversation().unwrap().is_none());
        assert!(
            store
                .read_operator_history(OperatorConversationId(1), 0, 10)
                .is_err()
        );
        assert!(
            store
                .submit_operator_turn(
                    OperatorConversationId(1),
                    SessionRunId(3),
                    "revive",
                    OperatorContext::default(),
                    None
                )
                .is_err()
        );
        assert!(
            !store
                .connection
                .prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap()
        );
        let singleton = open(&mut store);
        assert_eq!(singleton.id, OperatorConversationId(3));
        assert_eq!(singleton.title, "Operator");
        assert!(singleton.model.is_none());
        assert_eq!(open(&mut store), singleton);
        assert_eq!(
            store.current_operator_conversation().unwrap().unwrap().id,
            singleton.id
        );
    }

    #[test]
    fn schema_26_upgrade_widens_a_pre_singleton_operator_table() {
        let root = tempfile::tempdir().unwrap();
        let legacy_history = r#"[{"role":"user","content":"legacy hello"}]"#.to_string();
        let (conversation_id, messages) = {
            let mut store = SessionStore::open(root.path()).unwrap();
            let c = open(&mut store);
            let t = submit(&mut store, &c, Some("old/model"));
            store
                .apply_operator_batch(&batch(&t, 1, "legacy partial"))
                .unwrap();
            store
                .connection
                .execute(
                    "UPDATE operator_conversations
                     SET provider='\"chat_gpt\"',model='gpt-x',title='Old chat' WHERE id=?1",
                    [sql_integer(c.id.0).unwrap()],
                )
                .unwrap();
            store
                .connection
                .execute(
                    "INSERT INTO operator_model_history(conversation,turn,history) VALUES(?1,?2,?3)",
                    params![
                        sql_integer(c.id.0).unwrap(),
                        sql_integer(t.identity.turn.0).unwrap(),
                        legacy_history.as_str()
                    ],
                )
                .unwrap();
            // Rebuild the tables exactly as a schema-26 owner left them: no
            // singleton/checkpoint/model-capture columns and no unique indexes.
            downgrade_operator_tables(&store.connection, 26);
            let version: i64 = store
                .connection
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(version, 26);
            let names: Vec<String> = column_specs(&store, "operator_conversations")
                .into_iter()
                .map(|(name, ..)| name)
                .collect();
            assert_eq!(
                names,
                vec![
                    "id".to_string(),
                    "session".to_string(),
                    "provider".to_string(),
                    "model".to_string(),
                    "title".to_string(),
                ],
                "the fixture is a genuine pre-29 operator table"
            );
            (c.id, raw_messages(&store, c.id))
        };
        // Before the fix this entry point died creating the singleton index against
        // a table that had no `singleton` column.
        let connection = crate::config::open_writable_registry(root.path()).unwrap();
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 29);
        let singleton = column_specs_on(&connection, "operator_conversations")
            .into_iter()
            .find(|(name, ..)| name == "singleton")
            .unwrap();
        assert_eq!(
            singleton,
            (
                "singleton".to_string(),
                "INTEGER".to_string(),
                1,
                Some("0".into()),
                0
            )
        );
        let checkpoint = column_specs_on(&connection, "operator_conversations")
            .into_iter()
            .find(|(name, ..)| name == "pi_checkpoint_turn")
            .unwrap();
        assert_eq!(
            checkpoint,
            (
                "pi_checkpoint_turn".to_string(),
                "INTEGER".to_string(),
                0,
                None,
                0
            )
        );
        let capture = column_specs_on(&connection, "operator_turns")
            .into_iter()
            .find(|(name, ..)| name == "model_override")
            .unwrap();
        assert_eq!(
            capture,
            ("model_override".to_string(), "TEXT".to_string(), 0, None, 0)
        );
        let single = index_specs_on(&connection, "operator_conversations")
            .into_iter()
            .find(|(name, ..)| name == "operator_single_conversation")
            .unwrap();
        assert_eq!((single.1, single.2), (1, 1), "unique and partial");
        assert_eq!(single.3, vec!["singleton".to_string()]);
        assert!(single.4.contains("WHERE singleton=1"));
        let conversations: Vec<(i64, i64, String, String, String, Option<i64>)> = connection
            .prepare(
                "SELECT id,singleton,provider,model,title,pi_checkpoint_turn
                 FROM operator_conversations ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            conversations,
            vec![(
                conversation_id.0 as i64,
                0,
                "\"chat_gpt\"".into(),
                "gpt-x".into(),
                "Old chat".into(),
                None
            )],
            "the old conversation is not migrated into a singleton or rewritten"
        );
        let turns: Vec<(i64, i64, String, i64, Option<String>)> = connection
            .prepare(
                "SELECT conversation,run,state,sequence,model_override
                 FROM operator_turns ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            turns,
            vec![(conversation_id.0 as i64, 1, "Running".into(), 1, None)]
        );
        let sessions: Vec<(i64, i64, i64)> = connection
            .prepare("SELECT run,stopped,disposition FROM retained_sessions ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(sessions, vec![(1, 0, 0)]);
        assert_eq!(
            raw_messages_on(&connection, conversation_id),
            messages,
            "legacy journal bytes are preserved"
        );
        let histories: Vec<String> = connection
            .prepare("SELECT history FROM operator_model_history ORDER BY conversation")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(histories, vec![legacy_history]);
        assert!(
            !connection
                .prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap()
        );
        // Running the 28 -> 29 helper against the now-current schema is a no-op.
        let before = column_specs_on(&connection, "operator_conversations");
        migrate_singleton_conversation(&connection).unwrap();
        assert_eq!(
            column_specs_on(&connection, "operator_conversations"),
            before,
            "the upgraded structure is stable"
        );
        drop(connection);
        let mut store = SessionStore::open(root.path()).unwrap();
        assert!(store.current_operator_conversation().unwrap().is_none());
        let singleton = open(&mut store);
        assert_eq!(singleton.id, OperatorConversationId(conversation_id.0 + 1));
        assert_eq!(singleton.title, "Operator");
    }

    #[test]
    fn fresh_and_migrated_schemas_share_the_singleton_structure() {
        let fresh_root = tempfile::tempdir().unwrap();
        let fresh = SessionStore::open(fresh_root.path()).unwrap();
        let migrated_root = tempfile::tempdir().unwrap();
        {
            let mut store = SessionStore::open(migrated_root.path()).unwrap();
            open(&mut store);
            downgrade_to_schema_28(&store.connection);
        }
        let migrated = SessionStore::open(migrated_root.path()).unwrap();
        let version: i64 = migrated
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 29, "the comparison uses a migrated database");
        for table in [
            "operator_conversations",
            "operator_turns",
            "operator_messages",
            "operator_model_history",
            "operator_batches",
        ] {
            assert_eq!(
                column_specs(&fresh, table),
                column_specs(&migrated, table),
                "columns differ for {table}"
            );
            assert_eq!(
                index_specs(&fresh, table),
                index_specs(&migrated, table),
                "indexes differ for {table}"
            );
        }
        let singleton = column_specs(&migrated, "operator_conversations")
            .into_iter()
            .find(|(name, ..)| name == "singleton")
            .unwrap();
        assert_eq!(
            singleton,
            (
                "singleton".to_string(),
                "INTEGER".to_string(),
                1,
                Some("0".into()),
                0
            )
        );
        let checkpoint = column_specs(&migrated, "operator_conversations")
            .into_iter()
            .find(|(name, ..)| name == "pi_checkpoint_turn")
            .unwrap();
        assert_eq!(
            checkpoint,
            (
                "pi_checkpoint_turn".to_string(),
                "INTEGER".to_string(),
                0,
                None,
                0
            )
        );
        let capture = column_specs(&migrated, "operator_turns")
            .into_iter()
            .find(|(name, ..)| name == "model_override")
            .unwrap();
        assert_eq!(
            capture,
            ("model_override".to_string(), "TEXT".to_string(), 0, None, 0)
        );
        let conversation_sql = table_sql(&migrated, "operator_conversations");
        assert!(conversation_sql.contains("CHECK(singleton IN (0,1))"));
        assert!(conversation_sql.contains("CHECK(id>0)"));
        let turn_sql = table_sql(&migrated, "operator_turns");
        assert!(turn_sql.contains("CHECK(run>0)"));
        assert!(turn_sql.contains("UNIQUE(conversation,run)"));
        let single = index_specs(&migrated, "operator_conversations")
            .into_iter()
            .find(|(name, ..)| name == "operator_single_conversation")
            .unwrap();
        assert_eq!((single.1, single.2), (1, 1), "unique and partial");
        assert_eq!(single.3, vec!["singleton".to_string()]);
        assert!(single.4.contains("WHERE singleton=1"));
        let running = index_specs(&migrated, "operator_turns")
            .into_iter()
            .find(|(name, ..)| name == "operator_one_running_turn")
            .unwrap();
        assert_eq!((running.1, running.2), (1, 1), "unique and partial");
        assert_eq!(running.3, vec!["conversation".to_string()]);
        assert!(running.4.contains("WHERE state='Running'"));
    }

    #[test]
    fn invalid_inputs_do_not_create_turns_or_change_the_retained_run() {
        let mut store = SessionStore::in_memory();
        let c = open(&mut store);
        assert!(
            store
                .submit_operator_turn(
                    c.id,
                    SessionRunId(0),
                    &"x".repeat(MAX_OPERATOR_TEXT_BYTES + 1),
                    OperatorContext::default(),
                    None
                )
                .is_err()
        );
        assert!(
            store
                .submit_operator_turn(
                    c.id,
                    SessionRunId(0),
                    "   ",
                    OperatorContext::default(),
                    None
                )
                .is_err()
        );
        assert_eq!(store.get(c.session).unwrap().run, SessionRunId(0));
        assert!(store.read_operator_history(c.id, 0, 0).is_err());
        assert!(store.read_operator_history(c.id, 0, 101).is_err());
        assert!(store.current_operator_conversation().unwrap().is_some());
    }
}

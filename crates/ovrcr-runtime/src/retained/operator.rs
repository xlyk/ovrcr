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
        provider TEXT NOT NULL, model TEXT NOT NULL, title TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS operator_turns (
        id INTEGER PRIMARY KEY AUTOINCREMENT CHECK(id>0),
        conversation INTEGER NOT NULL REFERENCES operator_conversations(id) ON DELETE CASCADE,
        run INTEGER NOT NULL CHECK(run>0), state TEXT NOT NULL, sequence INTEGER NOT NULL DEFAULT 0,
        UNIQUE(conversation,run));
        CREATE UNIQUE INDEX IF NOT EXISTS operator_one_running_turn ON operator_turns(conversation) WHERE state='Running';
        CREATE TABLE IF NOT EXISTS operator_messages (
        ordinal INTEGER PRIMARY KEY AUTOINCREMENT, conversation INTEGER NOT NULL REFERENCES operator_conversations(id) ON DELETE CASCADE,
        turn INTEGER NOT NULL REFERENCES operator_turns(id), content TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS operator_history ON operator_messages(conversation,ordinal);
        CREATE TABLE IF NOT EXISTS operator_model_history (
        conversation INTEGER PRIMARY KEY REFERENCES operator_conversations(id) ON DELETE CASCADE,
        turn INTEGER NOT NULL REFERENCES operator_turns(id), history TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS operator_batches (
        turn INTEGER NOT NULL REFERENCES operator_turns(id) ON DELETE CASCADE,
        sequence INTEGER NOT NULL, digest TEXT NOT NULL, PRIMARY KEY(turn,sequence));")?;
    Ok(())
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
    pub(crate) fn create_operator_conversation(
        &mut self,
        provider: OperatorProvider,
        model: &str,
        cwd: &Path,
    ) -> Result<OperatorConversationSummary> {
        validate_operator_model(model).map_err(anyhow::Error::msg)?;
        if !cwd.is_absolute() {
            bail!("operator working directory must be absolute");
        }
        let transaction = self.connection.transaction()?;
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
        transaction.execute("INSERT INTO operator_conversations(id,session,provider,model,title) VALUES(?1,?2,?3,?4,'New Conversation')",params![sql_integer(conversation)?,sql_integer(session.0)?,serde_json::to_string(&provider)?,model])?;
        transaction.commit()?;
        self.records.insert(
            session,
            RetainedSession {
                id: session,
                run: SessionRunId(0),
                metadata,
                title_revision: 0,
                boot_id: None,
                stopped: true,
                disposition: Disposition::Active,
                failure: None,
                conversation: None,
                identity_invalid: false,
                cloud_creation: None,
                subjects: HashMap::new(),
            },
        );
        self.operator_conversation(OperatorConversationId(conversation))
    }
    pub(crate) fn operator_conversation(
        &self,
        id: OperatorConversationId,
    ) -> Result<OperatorConversationSummary> {
        let (session, provider, model, title): (u64, String, String, String) = self
            .connection
            .query_row(
                "SELECT session,provider,model,title FROM operator_conversations WHERE id=?1",
                [sql_integer(id.0)?],
                |r| Ok((read_integer(r, 0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
            .context("operator conversation not found")?;
        let record = self
            .records
            .get(&SessionId(session))
            .context("retained operator session missing")?;
        Ok(OperatorConversationSummary {
            run: record.run,
            archived: record.disposition == Disposition::Archived,
            id,
            session: SessionId(session),
            worker_stopped: record.stopped,
            provider: serde_json::from_str(&provider)?,
            model,
            title,
            current_turn: read_turn(&self.connection, id)?,
        })
    }
    pub(crate) fn list_operator_conversations(
        &self,
        after: u64,
        limit: u16,
    ) -> Result<Vec<OperatorConversationSummary>> {
        OperatorRequest::List { after, limit }
            .validate()
            .map_err(anyhow::Error::msg)?;
        let ids: Vec<u64> = self
            .connection
            .prepare("SELECT c.id FROM operator_conversations c JOIN retained_sessions s ON s.id=c.session WHERE c.id>?1 AND s.disposition!=1 ORDER BY c.id LIMIT ?2")?
            .query_map(params![sql_integer(after)?, limit], |r| read_integer(r, 0))?
            .collect::<rusqlite::Result<_>>()?;
        ids.into_iter()
            .map(|id| self.operator_conversation(OperatorConversationId(id)))
            .collect()
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
    ) -> Result<OperatorTurn> {
        OperatorRequest::Submit {
            conversation: id,
            expected_run,
            text: text.into(),
            context: context.clone(),
        }
        .validate()
        .map_err(anyhow::Error::msg)?;
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
            "INSERT INTO operator_turns(conversation,run,state) VALUES(?1,?2,'Running')",
            params![sql_integer(id.0)?, sql_integer(run.0)?],
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
        if conversation.current_turn.is_none() {
            let title: String = text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(80)
                .collect();
            transaction.execute(
                "UPDATE operator_conversations SET title=?1 WHERE id=?2",
                params![title, sql_integer(id.0)?],
            )?;
        }
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
            // Model history is durable but is not duplicated into visible messages.
            match event {
                OperatorWorkerEvent::Completed(history) => {
                    transaction.execute("INSERT INTO operator_model_history(conversation,turn,history) VALUES(?1,?2,?3) ON CONFLICT(conversation) DO UPDATE SET turn=excluded.turn,history=excluded.history",params![sql_integer(batch.identity.conversation.0)?,sql_integer(batch.identity.turn.0)?,serde_json::to_string(history)?])?;
                    state = "Complete";
                }
                OperatorWorkerEvent::Failed(_) => state = "Error",
                _ => {}
            }
            if !matches!(event, OperatorWorkerEvent::Completed(_)) {
                append_message(
                    &transaction,
                    sql_integer(batch.identity.conversation.0)?,
                    sql_integer(batch.identity.turn.0)?,
                    &OperatorMessageContent::Worker(event.clone()),
                )?;
            }
        }
        transaction.execute(
            "UPDATE operator_turns SET state=?1,sequence=?2 WHERE id=?3",
            params![
                state,
                sql_integer(batch.sequence)?,
                sql_integer(batch.identity.turn.0)?
            ],
        )?;
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
        let turn = self.validate_operator_identity(identity)?;
        if turn.state != OperatorTurnState::Running || turn.sequence != 0 {
            bail!("operator worker bootstrap is no longer current");
        }
        let conversation = self.operator_conversation(identity.conversation)?;
        let raw: String = self.connection.query_row(
            "SELECT content FROM operator_messages WHERE turn=?1 ORDER BY ordinal LIMIT 1",
            [sql_integer(identity.turn.0)?],
            |r| r.get(0),
        )?;
        let OperatorMessageContent::User { text, context } = serde_json::from_str(&raw)? else {
            bail!("operator submission missing")
        };
        Ok(OperatorWorkerStart {
            identity,
            provider: conversation.provider,
            model: conversation.model,
            text,
            context,
            history: self.operator_model_history(identity.conversation)?,
        })
    }
    pub(crate) fn operator_model_history(
        &self,
        id: OperatorConversationId,
    ) -> Result<Option<OperatorModelHistory>> {
        self.operator_conversation(id)?;
        let raw: Option<String> = self
            .connection
            .query_row(
                "SELECT history FROM operator_model_history WHERE conversation=?1",
                [sql_integer(id.0)?],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|raw| {
            let history: OperatorModelHistory = serde_json::from_str(&raw)?;
            history.validate().map_err(anyhow::Error::msg)?;
            Ok(history)
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn create(store: &mut SessionStore) -> OperatorConversationSummary {
        store
            .create_operator_conversation(
                OperatorProvider::ChatGpt,
                "model",
                Path::new("/tmp/operator"),
            )
            .unwrap()
    }
    fn submit(
        store: &mut SessionStore,
        conversation: &OperatorConversationSummary,
    ) -> OperatorTurn {
        let run = store.get(conversation.session).unwrap().run;
        store
            .submit_operator_turn(conversation.id, run, "hello", OperatorContext::default())
            .unwrap()
    }
    fn batch(turn: &OperatorTurn, sequence: u64, text: &str) -> OperatorEventBatch {
        OperatorEventBatch {
            identity: turn.identity,
            sequence,
            events: vec![OperatorWorkerEvent::AssistantDelta(text.into())],
        }
    }
    #[test]
    fn operator_restore_uses_current_retained_run() {
        let mut store = SessionStore::in_memory();
        let c = create(&mut store);
        let turn = submit(&mut store, &c);
        store.interrupt_operator_turn(turn.identity).unwrap();
        store.mark_stopped(c.session, turn.identity.run).unwrap();
        let messages = store.read_operator_history(c.id, 0, 100).unwrap().messages;
        store
            .set_archived(c.session, turn.identity.run, true)
            .unwrap();
        let archived_run = store.get(c.session).unwrap().run;
        assert_eq!(
            store.read_operator_history(c.id, 0, 100).unwrap().messages,
            messages
        );
        store.set_archived(c.session, archived_run, false).unwrap();
        let restored_run = store.get(c.session).unwrap().run;
        assert!(
            store
                .submit_operator_turn(c.id, turn.identity.run, "stale", OperatorContext::default())
                .is_err()
        );
        let fresh = store
            .submit_operator_turn(c.id, restored_run, "fresh", OperatorContext::default())
            .unwrap();
        assert_eq!(fresh.identity.run.0, restored_run.0 + 1);
        assert!(
            store
                .apply_operator_batch(&batch(&turn, 1, "late"))
                .is_err()
        );
    }

    #[test]
    fn operator_active_list_filters_archives_before_pagination() {
        let mut store = SessionStore::in_memory();
        let a = create(&mut store);
        let b = create(&mut store);
        let c = create(&mut store);
        store
            .set_archived(a.session, SessionRunId(0), true)
            .unwrap();
        assert_eq!(
            store.list_operator_conversations(0, 1).unwrap(),
            vec![b.clone()]
        );
        assert_eq!(
            store.list_operator_conversations(b.id.0, 1).unwrap(),
            vec![c]
        );
    }

    #[test]
    fn idle_conversation_retains_one_stopped_global_session_and_no_process() {
        let mut store = SessionStore::in_memory();
        let c = create(&mut store);
        let row = store.get(c.session).unwrap();
        assert!(row.stopped);
        assert_eq!(row.run, SessionRunId(0));
        assert_eq!(
            row.metadata.scope,
            ovrcr_protocol::SessionScope::OperatorConversation { conversation: c.id }
        );
        assert!(row.metadata.project.is_empty() && row.metadata.workspace.is_empty());
        assert!(row.summary(None).pid.is_none());
        assert!(
            store
                .read_operator_history(c.id, 0, 10)
                .unwrap()
                .messages
                .is_empty()
        );
    }
    #[test]
    fn ordered_batches_deduplicate_exact_replay_and_reject_conflicts_gaps_and_stale_runs() {
        let mut store = SessionStore::in_memory();
        let c = create(&mut store);
        let t = submit(&mut store, &c);
        let first = batch(&t, 1, "partial");
        assert!(!store.apply_operator_batch(&first).unwrap().duplicate);
        assert!(store.apply_operator_batch(&first).unwrap().duplicate);
        assert!(
            store
                .apply_operator_batch(&batch(&t, 1, "conflict"))
                .is_err()
        );
        assert!(store.apply_operator_batch(&batch(&t, 3, "gap")).is_err());
        let mut stale = batch(&t, 2, "stale");
        stale.identity.run.0 += 1;
        assert!(store.apply_operator_batch(&stale).is_err());
        assert_eq!(
            store
                .read_operator_history(c.id, 0, 100)
                .unwrap()
                .messages
                .len(),
            2
        );
        let final_batch = OperatorEventBatch {
            identity: t.identity,
            sequence: 2,
            events: vec![OperatorWorkerEvent::Completed(OperatorModelHistory {
                version: 1,
                json: "[]".into(),
            })],
        };
        store.apply_operator_batch(&final_batch).unwrap();
        assert!(store.apply_operator_batch(&final_batch).unwrap().duplicate);
        assert!(store.apply_operator_batch(&batch(&t, 3, "late")).is_err());
        assert_eq!(
            store.operator_model_history(c.id).unwrap().unwrap().json,
            "[]"
        );
        assert!(
            !store.get(c.session).unwrap().stopped,
            "model completion is not process-stop proof"
        );
    }
    #[test]
    fn interleaved_conversations_preserve_context_partial_output_and_paged_history() {
        let mut store = SessionStore::in_memory();
        let a = create(&mut store);
        let b = create(&mut store);
        let context = OperatorContext {
            project: Some("p".into()),
            workspace: Some("w".into()),
            workspace_id: Some("stable-w".into()),
            cwd: Some("/tmp/w".into()),
            session: Some(SessionId(100)),
            run: Some(SessionRunId(3)),
        };
        let ta = store
            .submit_operator_turn(a.id, SessionRunId(0), "first title", context.clone())
            .unwrap();
        let tb = submit(&mut store, &b);
        store.apply_operator_batch(&batch(&ta, 1, "A")).unwrap();
        store.apply_operator_batch(&batch(&tb, 1, "B")).unwrap();
        assert!(
            store
                .submit_operator_turn(a.id, ta.identity.run, "busy", OperatorContext::default())
                .is_err()
        );
        let page = store.read_operator_history(a.id, 0, 1).unwrap();
        assert_eq!(page.conversation.title, "first title");
        assert_eq!(
            page.messages[0].content,
            OperatorMessageContent::User {
                text: "first title".into(),
                context
            }
        );
        let next = store
            .read_operator_history(a.id, page.next_after.unwrap(), 1)
            .unwrap();
        assert_eq!(
            next.messages[0].content,
            OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta("A".into()))
        );
        store.interrupt_operator_turn(ta.identity).unwrap();
        assert!(store.apply_operator_batch(&batch(&ta, 2, "late")).is_err());
        assert_eq!(
            store
                .read_operator_history(b.id, 0, 100)
                .unwrap()
                .conversation
                .current_turn
                .unwrap()
                .state,
            OperatorTurnState::Running
        );
    }
    #[test]
    fn restart_interrupts_unfinished_turns_without_replay_or_stop_claim() {
        let root = tempfile::tempdir().unwrap();
        let (c, t) = {
            let mut store = SessionStore::open(root.path()).unwrap();
            let c = create(&mut store);
            let t = submit(&mut store, &c);
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
                .submit_operator_turn(c.id, t.identity.run, "unsafe", OperatorContext::default())
                .is_err()
        );
        drop(store);
        let store = SessionStore::open(root.path()).unwrap();
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
    fn bootstrap_is_immutable_and_prior_completed_history_is_separate() {
        let mut store = SessionStore::in_memory();
        let c = create(&mut store);
        let t = submit(&mut store, &c);
        let start = store.operator_worker_start(t.identity).unwrap();
        assert_eq!(start.text, "hello");
        assert!(start.history.is_none());
        assert_eq!(store.operator_worker_start(t.identity).unwrap(), start);
        let final_batch = OperatorEventBatch {
            identity: t.identity,
            sequence: 1,
            events: vec![OperatorWorkerEvent::Completed(OperatorModelHistory {
                version: 1,
                json: "[]".into(),
            })],
        };
        store.apply_operator_batch(&final_batch).unwrap();
        assert!(store.operator_worker_start(t.identity).is_err());
        assert_eq!(
            store.interrupt_operator_turn(t.identity).unwrap().state,
            OperatorTurnState::Complete
        );
        store.mark_stopped(c.session, t.identity.run).unwrap();
        let next = submit(&mut store, &c);
        assert_eq!(
            store
                .operator_worker_start(next.identity)
                .unwrap()
                .history
                .unwrap()
                .json,
            "[]"
        );
        assert_eq!(
            store
                .read_operator_history(c.id, 0, 100)
                .unwrap()
                .messages
                .len(),
            2
        );
        assert!(store.apply_operator_batch(&final_batch).is_err());
    }
    #[test]
    fn history_pages_fit_a_frame_and_catch_up_without_loss() {
        let mut store = SessionStore::in_memory();
        let c = create(&mut store);
        let t = submit(&mut store, &c);
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
    fn failed_batch_transaction_preserves_message_sequence_and_model_history() {
        let mut store = SessionStore::in_memory();
        let c = create(&mut store);
        let t = submit(&mut store, &c);
        store.connection.execute_batch("CREATE TRIGGER reject_operator_batch BEFORE INSERT ON operator_batches BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(
            store
                .apply_operator_batch(&batch(&t, 1, "must roll back"))
                .is_err()
        );
        assert_eq!(
            store
                .read_operator_history(c.id, 0, 100)
                .unwrap()
                .messages
                .len(),
            1
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
        store
            .connection
            .execute_batch("DROP TRIGGER reject_operator_batch")
            .unwrap();
        assert!(
            !store
                .apply_operator_batch(&batch(&t, 1, "retry"))
                .unwrap()
                .duplicate
        );
    }
    #[test]
    fn cloud_only_versions25_and26_upgrade_preserve_all_ordinary_session_metadata() {
        for version in [25, 26] {
            for kind in [
                SessionKind::Terminal,
                SessionKind::Agent {
                    name: "cursor".into(),
                },
            ] {
                let root = tempfile::tempdir().unwrap();
                let before = {
                    let mut store = SessionStore::open(root.path()).unwrap();
                    let row = store
                        .create(SessionMetadata {
                            scope: ovrcr_protocol::SessionScope::Workspace,
                            project: "p".into(),
                            workspace: "stable-w".into(),
                            name: "term".into(),
                            label: "label".into(),
                            cwd: "/tmp/w".into(),
                            kind,
                            pinned_title: Some("title".into()),
                            application_title: None,
                        })
                        .unwrap();
                    let row = store.begin_run(row.id, row.run).unwrap();
                    store.mark_stopped(row.id, row.run).unwrap();
                    let row = store.get(row.id).unwrap().clone();
                    store.connection.execute_batch("DROP TABLE operator_batches;DROP TABLE operator_model_history;DROP TABLE operator_messages;DROP TABLE operator_turns;DROP TABLE operator_conversations;ALTER TABLE retained_sessions DROP COLUMN scope;").unwrap();
                    store
                        .connection
                        .pragma_update(None, "user_version", version)
                        .unwrap();
                    row.summary(None)
                };
                let store = SessionStore::open(root.path()).unwrap();
                assert_eq!(store.get(before.id).unwrap().summary(None), before);
                assert_eq!(store.records().count(), 1);
                assert!(
                    store
                        .list_operator_conversations(0, 100)
                        .unwrap()
                        .is_empty()
                );
                let mut store = store;
                let version: i64 = store
                    .connection
                    .pragma_query_value(None, "user_version", |row| row.get(0))
                    .unwrap();
                assert_eq!(version, 27);
                let conversation = create(&mut store);
                assert_eq!(
                    store.get(conversation.session).unwrap().metadata.scope,
                    ovrcr_protocol::SessionScope::OperatorConversation {
                        conversation: conversation.id
                    }
                );
            }
        }
    }
    #[test]
    fn version26_operator_upgrade_preserves_completed_journal_and_model_context() {
        for archived in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (conversation, before, history, model_history) = {
                let mut store = SessionStore::open(root.path()).unwrap();
                let conversation = create(&mut store);
                let turn = submit(&mut store, &conversation);
                store
                    .apply_operator_batch(&batch(&turn, 1, "saved reply"))
                    .unwrap();
                store
                    .apply_operator_batch(&OperatorEventBatch {
                        identity: turn.identity,
                        sequence: 2,
                        events: vec![OperatorWorkerEvent::Completed(OperatorModelHistory {
                            version: 1,
                            json: "[{\"role\":\"assistant\",\"content\":\"saved reply\"}]".into(),
                        })],
                    })
                    .unwrap();
                store
                    .mark_stopped(conversation.session, turn.identity.run)
                    .unwrap();
                if archived {
                    store
                        .set_archived(conversation.session, turn.identity.run, true)
                        .unwrap();
                }
                store
                    .connection
                    .pragma_update(None, "user_version", 26)
                    .unwrap();
                (
                    conversation.clone(),
                    store.get(conversation.session).unwrap().summary(None),
                    store
                        .read_operator_history(conversation.id, 0, 100)
                        .unwrap(),
                    store.operator_model_history(conversation.id).unwrap(),
                )
            };
            let mut store = SessionStore::open(root.path()).unwrap();
            let version: i64 = store
                .connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(version, 27);
            assert_eq!(
                store.get(conversation.session).unwrap().summary(None),
                before
            );
            assert_eq!(
                store
                    .read_operator_history(conversation.id, 0, 100)
                    .unwrap(),
                history
            );
            assert_eq!(
                store.operator_model_history(conversation.id).unwrap(),
                model_history
            );
            let retained_run = store.get(conversation.session).unwrap().run;
            let migrated = store.operator_conversation(conversation.id).unwrap();
            assert_eq!(migrated.run, retained_run);
            assert_eq!(migrated.archived, archived);
            assert_eq!(
                migrated.current_turn.as_ref().unwrap().identity.run,
                SessionRunId(1)
            );
            if archived {
                assert!(
                    store
                        .submit_operator_turn(
                            conversation.id,
                            retained_run,
                            "closed",
                            OperatorContext::default()
                        )
                        .is_err()
                );
                store
                    .set_archived(conversation.session, retained_run, false)
                    .unwrap();
            }
            let next = submit(&mut store, &conversation);
            assert!(next.identity.run.0 > retained_run.0);
            assert_eq!(
                store.operator_worker_start(next.identity).unwrap().history,
                model_history
            );
        }
    }
    #[test]
    fn invalid_inputs_do_not_create_turns_or_change_retained_run() {
        let mut store = SessionStore::in_memory();
        let c = create(&mut store);
        assert!(
            store
                .submit_operator_turn(
                    c.id,
                    SessionRunId(0),
                    &"x".repeat(MAX_OPERATOR_TEXT_BYTES + 1),
                    OperatorContext::default()
                )
                .is_err()
        );
        assert_eq!(store.get(c.session).unwrap().run, SessionRunId(0));
        assert!(store.read_operator_history(c.id, 0, 0).is_err());
        assert!(store.list_operator_conversations(0, 101).is_err());
    }
}

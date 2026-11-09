//! Drive the real Server's Operator protocol from an owned Dashboard connection
//! and inspect the durable state it produces.
//!
//! Everything here is an observation of the application path: protocol frames
//! over the fixture socket, the retained SQLite registry, the fixture's private
//! Pi profile, and owned process groups. Nothing reaches into Server internals.

#![allow(dead_code)]

use ovrcr::protocol::{
    self, ClientMessage, OperatorConversationSummary, OperatorHistory, OperatorRequest,
    OperatorResponse, OperatorTurn, OperatorTurnIdentity, OperatorTurnState, Request, Response,
    ServerEvent, ServerMessage, SessionId, SessionRunId, SessionSummary,
};
use serde_json::Value;
use std::{
    collections::VecDeque,
    io::{ErrorKind, Read},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::{Duration, Instant},
};

pub const BUDGET: Duration = Duration::from_secs(20);

/// How long one `pump` read waits before treating the socket as drained.
const POLL: Duration = Duration::from_millis(2);

/// One Dashboard-owned protocol connection plus every frame it has seen.
pub struct OperatorClient {
    stream: UnixStream,
    next_id: u64,
    pub messages: Vec<ServerMessage>,
    pending: VecDeque<ClientMessage>,
    /// Bytes received but not yet decoded into a whole frame.
    incoming: Vec<u8>,
}

impl OperatorClient {
    pub fn connect(live: &super::live::Live) -> Self {
        let mut stream = protocol::connect_server(&live.socket).unwrap();
        stream.set_read_timeout(Some(BUDGET)).unwrap();
        stream.set_write_timeout(Some(BUDGET)).unwrap();
        let Response::Hierarchy(_) =
            protocol::client::request(&mut stream, 900_000, Request::DashboardHello).unwrap()
        else {
            panic!("Dashboard handshake")
        };
        Self {
            stream,
            next_id: 900_001,
            messages: Vec::new(),
            pending: VecDeque::new(),
            incoming: Vec::new(),
        }
    }

    /// Read the bytes the socket already holds into the persistent buffer.
    fn receive(&mut self, timeout: Duration) -> std::io::Result<usize> {
        self.stream.set_read_timeout(Some(timeout)).unwrap();
        let mut bytes = [0_u8; 64 * 1024];
        let read = self.stream.read(&mut bytes)?;
        self.incoming.extend_from_slice(&bytes[..read]);
        Ok(read)
    }

    /// Decode the oldest whole frame from the persistent buffer, if complete.
    ///
    /// Incomplete head bytes stay buffered for the next call; nothing is
    /// discarded until a full frame has been decoded from it.
    fn take_frame(&mut self) -> Option<ServerMessage> {
        let header = self.incoming.get(..4)?;
        let length = u32::from_be_bytes(header.try_into().unwrap()) as usize;
        assert!(
            length <= protocol::MAX_FRAME_BYTES,
            "protocol frame too large: {length} bytes"
        );
        let total = 4 + length;
        if self.incoming.len() < total {
            return None;
        }
        let frame = self.incoming[..total].to_vec();
        let message = protocol::read_frame::<ServerMessage>(&mut frame.as_slice())
            .unwrap_or_else(|error| panic!("decode protocol frame: {error:#}"));
        self.incoming.drain(..total);
        Some(message)
    }

    /// The next whole frame, blocking up to the full budget for each read.
    fn next_frame(&mut self) -> ServerMessage {
        loop {
            if let Some(message) = self.take_frame() {
                return message;
            }
            match self.receive(BUDGET) {
                Ok(0) => panic!("Operator connection closed before the next frame"),
                Ok(_) => {}
                Err(error) => panic!("read Operator frame: {error}"),
            }
        }
    }

    /// Send one request and wait for its response, collecting events and any
    /// deferred receipts that arrive first.
    pub fn exchange(&mut self, request: Request) -> Response {
        self.next_id += 1;
        let id = self.next_id;
        protocol::write_frame(
            &mut self.stream,
            &ClientMessage {
                request_id: id,
                request,
            },
        )
        .unwrap();
        loop {
            let message: ServerMessage = self.next_frame();
            let response = match &message {
                ServerMessage::Response {
                    request_id,
                    response,
                } if *request_id == id => Some(response.clone()),
                _ => None,
            };
            self.messages.push(message);
            if let Some(response) = response {
                return response;
            }
        }
    }

    /// Drain whatever the socket has right now without blocking a full budget.
    ///
    /// Reception goes through the persistent buffer, so a frame split across
    /// reads (or across `pump` calls) is decoded exactly once when its
    /// remainder arrives; an expiring poll never drops already-read bytes.
    pub fn pump(&mut self) {
        loop {
            while let Some(message) = self.take_frame() {
                self.messages.push(message);
            }
            match self.receive(POLL) {
                Ok(0) => {
                    assert!(
                        self.incoming.is_empty(),
                        "Operator connection closed mid-frame: {} buffered bytes",
                        self.incoming.len()
                    );
                    break;
                }
                Ok(_) => {}
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    break;
                }
                Err(error) => panic!("read Operator frame: {error}"),
            }
        }
        self.stream.set_read_timeout(Some(BUDGET)).unwrap();
    }

    pub fn current(&mut self) -> Option<OperatorConversationSummary> {
        match self.exchange(Request::Operator(OperatorRequest::Current)) {
            Response::Operator(OperatorResponse::Current(current)) => current,
            other => panic!("Current response: {other:?}"),
        }
    }

    pub fn open(&mut self) -> OperatorConversationSummary {
        match self.exchange(Request::Operator(OperatorRequest::Open)) {
            Response::Operator(OperatorResponse::Opened(summary)) => summary,
            other => panic!("Open response: {other:?}"),
        }
    }

    pub fn read(
        &mut self,
        conversation: impl Into<u64>,
        after: u64,
        limit: u16,
    ) -> OperatorHistory {
        let request = Request::Operator(OperatorRequest::Read {
            conversation: ovrcr::protocol::OperatorConversationId(conversation.into()),
            after,
            limit,
        });
        match self.exchange(request) {
            Response::Operator(OperatorResponse::History(history)) => history,
            other => panic!("Read response: {other:?}"),
        }
    }

    pub fn submit(&mut self, summary: &OperatorConversationSummary, text: &str) -> Response {
        // The captured context is a real feature-workspace selection.
        self.exchange(Request::Operator(OperatorRequest::Submit {
            conversation: summary.id,
            expected_run: summary.run,
            text: text.into(),
            context: ovrcr::protocol::OperatorContext::default(),
        }))
    }

    pub fn submit_turn(
        &mut self,
        summary: &OperatorConversationSummary,
        text: &str,
    ) -> OperatorTurn {
        match self.submit(summary, text) {
            Response::Operator(OperatorResponse::Submitted(turn)) => turn,
            other => panic!("Submit response: {other:?}"),
        }
    }

    pub fn stop(&mut self, identity: OperatorTurnIdentity) -> Response {
        self.exchange(Request::Operator(OperatorRequest::Stop { identity }))
    }

    pub fn stop_turn(&mut self, identity: OperatorTurnIdentity) -> OperatorTurn {
        match self.stop(identity) {
            Response::Operator(OperatorResponse::Stopped(turn)) => turn,
            other => panic!("Stop response: {other:?}"),
        }
    }

    /// Poll durable history until `predicate` holds, pumping events meanwhile.
    pub fn wait_history(
        &mut self,
        conversation: ovrcr::protocol::OperatorConversationId,
        predicate: impl Fn(&OperatorHistory) -> bool,
    ) -> OperatorHistory {
        let deadline = Instant::now() + BUDGET;
        loop {
            self.pump();
            let history = self.read(conversation.0, 0, 100);
            if predicate(&history) {
                return history;
            }
            assert!(
                Instant::now() < deadline,
                "operator history deadline: {history:#?}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Wait for a turn to reach a terminal state, returning the history.
    pub fn wait_terminal(
        &mut self,
        conversation: ovrcr::protocol::OperatorConversationId,
    ) -> OperatorHistory {
        self.wait_history(conversation, |history| {
            history
                .conversation
                .current_turn
                .as_ref()
                .is_some_and(|turn| turn.state != OperatorTurnState::Running)
        })
    }

    /// Wait for the controlled-stop proof on the retained summary.
    pub fn wait_stopped(
        &mut self,
        conversation: ovrcr::protocol::OperatorConversationId,
    ) -> OperatorHistory {
        self.wait_history(conversation, |history| history.conversation.worker_stopped)
    }

    pub fn inspect(&mut self) -> Vec<SessionSummary> {
        match self.exchange(Request::Inspect) {
            Response::Inventory { sessions, .. } => sessions,
            other => panic!("Inspect response: {other:?}"),
        }
    }

    pub fn session(&mut self, id: SessionId) -> SessionSummary {
        self.inspect()
            .into_iter()
            .find(|session| session.id == id)
            .unwrap_or_else(|| panic!("session {id:?} not listed"))
    }

    /// Every received frame serialized, for credential-sentinel scans.
    pub fn serialized_messages(&self) -> String {
        self.messages
            .iter()
            .map(|message| serde_json::to_string(message).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn operator_events(&self) -> impl Iterator<Item = &ServerEvent> {
        self.messages.iter().filter_map(|message| match message {
            ServerMessage::Event(event) => Some(event),
            _ => None,
        })
    }

    pub fn wait_operator_changed(&mut self, conversation: ovrcr::protocol::OperatorConversationId) {
        let deadline = Instant::now() + BUDGET;
        loop {
            self.pump();
            if self.operator_events().any(
                |event| matches!(event, ServerEvent::OperatorChanged(id) if *id == conversation),
            ) {
                return;
            }
            assert!(Instant::now() < deadline, "no OperatorChanged event");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Read-only SQLite queries against the fixture's registry database.
pub struct Registry {
    pub path: PathBuf,
}

impl Registry {
    pub fn of(live: &super::live::Live) -> Self {
        Self {
            path: ovrcr::config::database_path(&live.config),
        }
    }

    fn connection(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(&self.path).unwrap()
    }

    pub fn checkpoint(&self, conversation: u64) -> Option<i64> {
        self.connection()
            .query_row(
                "SELECT pi_checkpoint_turn FROM operator_conversations WHERE id=?1",
                [conversation as i64],
                |row| row.get::<_, i64>(0),
            )
            .ok()
    }

    pub fn turn_state(&self, turn: u64) -> Option<String> {
        self.connection()
            .query_row(
                "SELECT state FROM operator_turns WHERE id=?1",
                [turn as i64],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn model_override(&self, turn: u64) -> Option<String> {
        self.connection()
            .query_row(
                "SELECT model_override FROM operator_turns WHERE id=?1",
                [turn as i64],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn batch_digests(&self, turn: u64) -> Vec<(i64, String)> {
        let connection = self.connection();
        let mut statement = connection
            .prepare("SELECT sequence,digest FROM operator_batches WHERE turn=?1 ORDER BY sequence")
            .unwrap();
        statement
            .query_map([turn as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    pub fn conversation_ids(&self) -> Vec<u64> {
        let connection = self.connection();
        let mut statement = connection
            .prepare("SELECT id FROM operator_conversations ORDER BY id")
            .unwrap();
        statement
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<i64>>>()
            .unwrap()
            .into_iter()
            .map(|id| id as u64)
            .collect()
    }

    /// Every cell of every user table as searchable text.
    ///
    /// `rusqlite::types::ValueRef`'s derived `Debug` prints text and BLOB
    /// payloads as numeric bytes, so formatting values would hide a credential
    /// sentinel that really is stored in the registry. Decode text and BLOB
    /// cells instead (lossy UTF-8 keeps any ASCII sentinel literal) and scan
    /// every table, including legacy rows, so a leak cannot pass unnoticed.
    pub fn operator_payload(&self) -> String {
        let connection = self.connection();
        let mut text = String::new();
        let mut statement = connection
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        let tables = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for table in tables {
            let mut statement = connection
                .prepare(&format!("SELECT * FROM \"{}\"", table.replace('"', "\"\"")))
                .unwrap();
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    let mut values = String::new();
                    for index in 0..columns {
                        match row.get_ref(index)? {
                            rusqlite::types::ValueRef::Null => values.push_str("NULL"),
                            rusqlite::types::ValueRef::Integer(value) => {
                                values.push_str(&value.to_string());
                            }
                            rusqlite::types::ValueRef::Real(value) => {
                                values.push_str(&value.to_string());
                            }
                            rusqlite::types::ValueRef::Text(bytes)
                            | rusqlite::types::ValueRef::Blob(bytes) => {
                                values.push_str(&String::from_utf8_lossy(bytes));
                            }
                        }
                        values.push('|');
                    }
                    Ok(values)
                })
                .unwrap();
            for row in rows {
                text.push_str(&row.unwrap());
                text.push('\n');
            }
        }
        text
    }
}

/// History as one searchable string (visible journal only).
pub fn history_text(history: &OperatorHistory) -> String {
    serde_json::to_string(&history.messages).unwrap()
}

pub fn has_assistant(history: &OperatorHistory, needle: &str) -> bool {
    history.messages.iter().any(|message| {
        matches!(
            &message.content,
            ovrcr::protocol::OperatorMessageContent::Worker(
                ovrcr::protocol::OperatorWorkerEvent::AssistantDelta(text)
            ) if text.contains(needle)
        )
    })
}

pub fn tool_calls(history: &OperatorHistory) -> Vec<(String, String, String)> {
    history
        .messages
        .iter()
        .filter_map(|message| match &message.content {
            ovrcr::protocol::OperatorMessageContent::Worker(
                ovrcr::protocol::OperatorWorkerEvent::ToolCall {
                    id,
                    name,
                    arguments,
                },
            ) => Some((id.clone(), name.clone(), arguments.clone())),
            _ => None,
        })
        .collect()
}

pub fn tool_outputs(history: &OperatorHistory) -> Vec<(String, String, bool)> {
    history
        .messages
        .iter()
        .filter_map(|message| match &message.content {
            ovrcr::protocol::OperatorMessageContent::Worker(
                ovrcr::protocol::OperatorWorkerEvent::ToolOutput {
                    id,
                    output,
                    truncated,
                    ..
                },
            ) => Some((id.clone(), output.clone(), *truncated)),
            _ => None,
        })
        .collect()
}

pub fn models_selected(history: &OperatorHistory) -> Vec<ovrcr::protocol::OperatorModel> {
    history
        .messages
        .iter()
        .filter_map(|message| match &message.content {
            ovrcr::protocol::OperatorMessageContent::Worker(
                ovrcr::protocol::OperatorWorkerEvent::ModelSelected(model),
            ) => Some(model.clone()),
            _ => None,
        })
        .collect()
}

pub fn interrupted(history: &OperatorHistory) -> bool {
    history.messages.iter().any(|message| {
        matches!(
            message.content,
            ovrcr::protocol::OperatorMessageContent::Interrupted
        )
    })
}

/// Own the live process group behind a retained session and return it.
pub fn own_worker_group(
    live: &super::live::Live,
    client: &mut OperatorClient,
    id: SessionId,
) -> libc::pid_t {
    let deadline = Instant::now() + BUDGET;
    loop {
        let session = client.session(id);
        if let Some(pid) = session.pid {
            let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
            assert!(pgid > 1, "worker has no process group: {pgid}");
            live.own_group(pgid);
            return pgid;
        }
        assert!(
            Instant::now() < deadline,
            "worker process did not appear: {session:#?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn assert_group_absent(live: &super::live::Live, pgid: libc::pid_t) {
    assert!(
        super::live::wait_group_absent(pgid, BUDGET),
        "owned operator group still exists: {pgid}"
    );
    live.forget_group(pgid);
}

pub fn settings_text(live: &super::live::Live) -> String {
    std::fs::read_to_string(live.config.join("dashboard.toml")).unwrap_or_default()
}

/// Read a JSON field from a protocol value (tests seed legacy rows directly).
pub fn sqlite_exec(live: &super::live::Live, sql: &str) {
    rusqlite::Connection::open(ovrcr::config::database_path(&live.config))
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}

pub fn write_frame(stream: &mut UnixStream, message: &ClientMessage) {
    protocol::write_frame(stream, message).unwrap();
}

pub fn read_frame(stream: &mut UnixStream) -> ServerMessage {
    protocol::read_frame(stream).unwrap()
}

/// The retained run for a session, read offline from the registry.
pub fn offline_run(live: &super::live::Live, id: SessionId) -> SessionRunId {
    let summaries = ovrcr::retained::load_session_summaries(&live.config).unwrap();
    summaries
        .into_iter()
        .find(|summary| summary.id == id)
        .map(|summary| summary.run)
        .unwrap_or(SessionRunId(0))
}

/// A JSON value from the registry database for direct assertions.
pub fn query_value(live: &super::live::Live, sql: &str) -> Option<i64> {
    rusqlite::Connection::open(ovrcr::config::database_path(&live.config))
        .unwrap()
        .query_row(sql, [], |row| row.get(0))
        .ok()
}

pub fn json_scalar(value: &Value) -> Option<i64> {
    value.as_i64()
}

pub fn assert_nonzero(count: usize, what: &str) {
    assert!(count > 0, "{what} must be nonzero");
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr::protocol::OperatorConversationId;
    use std::io::Write;

    const SENTINEL: &str = "sk-FIXTURE-CREDENTIAL-SENTINEL-7f3a";

    fn client(stream: UnixStream) -> OperatorClient {
        OperatorClient {
            stream,
            next_id: 900_001,
            messages: Vec::new(),
            pending: VecDeque::new(),
            incoming: Vec::new(),
        }
    }

    fn changed(conversation: u64) -> ServerMessage {
        ServerMessage::Event(ServerEvent::OperatorChanged(OperatorConversationId(
            conversation,
        )))
    }

    /// A short poll that expires mid-frame must not discard what it read: the
    /// remainder completes the frame exactly once, before the next one.
    #[test]
    fn pump_keeps_a_frame_split_between_polls() {
        let (mut peer, stream) = UnixStream::pair().unwrap();
        let mut client = client(stream);
        let first = changed(7);
        let second = changed(8);

        let mut encoded = Vec::new();
        protocol::write_frame(&mut encoded, &first).unwrap();
        // Split inside the four-byte length header, the worst case for a
        // blocking read that stores its progress in local buffers.
        peer.write_all(&encoded[..2]).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        client.pump();
        assert!(
            client.messages.is_empty(),
            "a partial frame is not a message: {:#?}",
            client.messages
        );

        peer.write_all(&encoded[2..]).unwrap();
        let mut next = Vec::new();
        protocol::write_frame(&mut next, &second).unwrap();
        peer.write_all(&next).unwrap();
        peer.flush().unwrap();
        client.pump();
        assert_eq!(client.messages, vec![first, second]);
    }

    fn scan(seed: impl FnOnce(&rusqlite::Connection)) -> String {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("registry.sqlite3");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE operator_conversations (id INTEGER PRIMARY KEY, provider TEXT);
                 CREATE TABLE operator_turns (id INTEGER PRIMARY KEY, note TEXT);
                 CREATE TABLE operator_messages (id INTEGER PRIMARY KEY, body BLOB);
                 CREATE TABLE operator_batches (id INTEGER PRIMARY KEY, digest TEXT);
                 CREATE TABLE legacy_model_history (id INTEGER PRIMARY KEY, payload BLOB);",
            )
            .unwrap();
        seed(&connection);
        drop(connection);
        Registry { path }.operator_payload()
    }

    /// Positive control: a sentinel stored as TEXT and as an opaque BLOB in a
    /// table outside the four Operator tables is visible to the scan.
    #[test]
    fn registry_scan_finds_text_and_blob_credential_sentinels() {
        let payload = scan(|connection| {
            connection
                .execute(
                    "INSERT INTO operator_conversations (id, provider) VALUES (1, ?1)",
                    [SENTINEL],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO legacy_model_history (id, payload) VALUES (1, ?1)",
                    [SENTINEL.as_bytes()],
                )
                .unwrap();
        });
        assert_eq!(
            payload.matches(SENTINEL).count(),
            2,
            "both stored cells must be decoded, not printed as bytes: {payload}"
        );
    }

    /// Negative control: a clean registry reports no sentinel while the scan
    /// still proves it read the rows (including a BLOB cell).
    #[test]
    fn registry_scan_ignores_a_clean_database() {
        let payload = scan(|connection| {
            connection
                .execute(
                    "INSERT INTO operator_conversations (id, provider) VALUES (1, 'pi')",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO operator_messages (id, body) VALUES (1, ?1)",
                    ["ordinary tool output"],
                )
                .unwrap();
        });
        assert!(
            !payload.contains(SENTINEL),
            "a clean registry must not report a sentinel: {payload}"
        );
        assert!(
            payload.contains("ordinary tool output"),
            "the scan must read BLOB cells: {payload}"
        );
    }
}

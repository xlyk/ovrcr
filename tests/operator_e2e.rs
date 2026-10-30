//! Production worker acceptance; every network endpoint and process is fixture-owned.
#[path = "support/live.rs"]
mod live;

#[test]
fn operator_cli_is_a_real_entry_point() {
    let live = live::Live::idle();
    let output = live
        .command()
        .args(["operator", "--help"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "operator CLI missing: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("login"));
}

#[cfg(feature = "acceptance-diagnostics")]
#[path = "support/operator_provider.rs"]
mod operator_provider;

#[cfg(feature = "acceptance-diagnostics")]
mod compiled_worker {
    use super::{
        live,
        operator_provider::{Provider, cli_array},
    };
    use crossterm::event::{Event, KeyCode};
    use ovrcr::{
        protocol::{
            self, ClientMessage, OperatorConversationId, OperatorConversationSummary,
            OperatorHistory, OperatorMessageContent, OperatorProvider, OperatorRequest,
            OperatorResponse, OperatorToolStatus, OperatorTurn, OperatorTurnState,
            OperatorWorkerEvent, Request, Response, ServerEvent, ServerMessage, SessionPhase,
            SessionSummary, TerminalSize,
        },
        tui::{Dashboard, DashboardAction, Settings},
    };
    use std::{
        collections::{HashSet, VecDeque},
        ffi::OsStr,
        os::unix::net::UnixStream,
        thread,
        time::{Duration, Instant},
    };

    const BUDGET: Duration = Duration::from_secs(15);

    /// Actual Dashboard model/outbox, production frames/socket, actual Server responses/events.
    struct DashboardClient {
        stream: UnixStream,
        dashboard: Dashboard,
        pending: VecDeque<ClientMessage>,
        manual_id: u64,
        changed: HashSet<OperatorConversationId>,
    }
    impl DashboardClient {
        fn connect(live: &live::Live) -> Self {
            let mut stream = protocol::connect_server(&live.socket).unwrap();
            stream.set_read_timeout(Some(BUDGET)).unwrap();
            stream.set_write_timeout(Some(BUDGET)).unwrap();
            let Response::Hierarchy(hierarchy) =
                protocol::client::request(&mut stream, 900_000, Request::DashboardHello).unwrap()
            else {
                panic!("Dashboard handshake")
            };
            let mut dashboard = Dashboard::new(TerminalSize {
                rows: 50,
                cols: 160,
            });
            dashboard.install_hierarchy(hierarchy);
            dashboard.install_area(ratatui::layout::Rect::new(0, 0, 160, 50));
            Self {
                stream,
                dashboard,
                pending: VecDeque::new(),
                manual_id: 900_001,
                changed: HashSet::new(),
            }
        }
        fn receive(&mut self, message: ServerMessage) {
            if let ServerMessage::Event(ServerEvent::OperatorChanged(id)) = &message {
                self.changed.insert(*id);
            }
            self.pending
                .extend(self.dashboard.handle_server_message(message));
            self.pending.extend(self.dashboard.drain_outbox());
        }
        fn exchange(&mut self, message: ClientMessage) -> Response {
            let id = message.request_id;
            protocol::write_frame(&mut self.stream, &message).unwrap();
            loop {
                let message: ServerMessage = protocol::read_frame(&mut self.stream).unwrap();
                let response = match &message {
                    ServerMessage::Response {
                        request_id,
                        response,
                    } if *request_id == id => Some(response.clone()),
                    _ => None,
                };
                self.receive(message);
                if let Some(response) = response {
                    return response;
                }
            }
        }
        /// Hold a real mutation receipt outside the Dashboard until the test releases it.
        fn hold(&mut self, request: ClientMessage) -> ServerMessage {
            protocol::write_frame(&mut self.stream, &request).unwrap();
            loop {
                let message: ServerMessage = protocol::read_frame(&mut self.stream).unwrap();
                if matches!(&message, ServerMessage::Response { request_id, .. } if *request_id == request.request_id)
                {
                    return message;
                }
                self.receive(message);
            }
        }
        fn take_operator_request(&mut self) -> ClientMessage {
            self.pending.extend(self.dashboard.drain_outbox());
            let index = self
                .pending
                .iter()
                .position(|message| {
                    matches!(
                        message.request,
                        Request::Operator(
                            OperatorRequest::Create { .. } | OperatorRequest::Submit { .. }
                        )
                    )
                })
                .expect("public input emitted mutation");
            self.pending.remove(index).unwrap()
        }
        fn flush(&mut self) -> Vec<Response> {
            self.pending.extend(self.dashboard.drain_outbox());
            let mut replies = Vec::new();
            while let Some(message) = self.pending.pop_front() {
                assert!(replies.len() < 500, "Dashboard catch-up did not converge");
                let response = self.exchange(message);
                assert!(
                    !matches!(response, Response::Error { .. }),
                    "Dashboard request failed: {response:?}"
                );
                replies.push(response);
            }
            replies
        }
        fn request(&mut self, request: Request) -> Response {
            self.manual_id += 1;
            let response = self.exchange(ClientMessage {
                request_id: self.manual_id,
                request,
            });
            self.flush();
            response
        }
        fn key(&mut self, code: KeyCode) {
            let action = self.dashboard.key(code);
            assert!(
                !matches!(action, DashboardAction::PtyBytes(_)),
                "native chat emitted terminal input"
            );
            match action {
                DashboardAction::Request(message) => self.pending.push_back(message),
                DashboardAction::RequestBatch(messages) => self.pending.extend(messages),
                _ => {}
            }
        }
        fn ctrl(&mut self, code: char) {
            let action = self.dashboard.ctrl(code);
            assert!(!matches!(action, DashboardAction::PtyBytes(_)));
        }
        fn paste(&mut self, text: &str) {
            assert!(
                !matches!(
                    self.dashboard.event_action(Event::Paste(text.into())),
                    DashboardAction::PtyBytes(_)
                        | DashboardAction::Request(_)
                        | DashboardAction::RequestBatch(_)
                ),
                "paste must only edit the native draft"
            );
            assert!(
                self.dashboard.drain_outbox().is_empty(),
                "paste submitted a request"
            );
        }
        fn create(&mut self, model: &str, first: bool) -> OperatorConversationSummary {
            self.create_provider(OperatorProvider::Grok, model, first)
        }
        fn create_provider(
            &mut self,
            provider: OperatorProvider,
            model: &str,
            first: bool,
        ) -> OperatorConversationSummary {
            let mut settings = Settings::default();
            settings.operator.provider = provider;
            settings.operator.grok_model = Some(model.into());
            settings.operator.chat_gpt_model = Some(model.into());
            self.dashboard.install_settings(settings);
            if first {
                self.key(KeyCode::Char('o'));
            } else {
                self.ctrl('n');
            }
            self.flush();
            self.key(KeyCode::Enter);
            self.flush()
                .into_iter()
                .find_map(|response| match response {
                    Response::Operator(OperatorResponse::Created(summary)) => Some(summary),
                    _ => None,
                })
                .expect("native New Conversation reached real Server create")
        }
        fn submit(&mut self, text: &str) -> OperatorTurn {
            self.paste(text);
            self.key(KeyCode::Enter);
            self.flush()
                .into_iter()
                .find_map(|response| match response {
                    Response::Operator(OperatorResponse::Submitted(turn)) => Some(turn),
                    _ => None,
                })
                .expect("native Enter reached real Server submit")
        }
        fn history(&mut self, conversation: OperatorConversationId) -> OperatorHistory {
            let response = self.request(Request::Operator(OperatorRequest::Read {
                conversation,
                after: 0,
                limit: 100,
            }));
            let Response::Operator(OperatorResponse::History(history)) = response else {
                panic!("durable history: {response:?}")
            };
            history
        }
        fn wait_history(
            &mut self,
            provider: &Provider,
            conversation: OperatorConversationId,
            predicate: impl Fn(&OperatorHistory) -> bool,
        ) -> OperatorHistory {
            let deadline = Instant::now() + BUDGET;
            loop {
                provider.assert_healthy();
                let history = self.history(conversation);
                if predicate(&history) {
                    return history;
                }
                assert!(
                    Instant::now() < deadline,
                    "operator history deadline: {history:?}"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }
        fn rendered(&mut self) -> String {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 50)).unwrap();
            self.dashboard.draw(&mut terminal).unwrap();
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect()
        }
        fn select_previous(&mut self) {
            self.ctrl('l');
            self.key(KeyCode::Up);
            self.key(KeyCode::Enter);
            self.flush();
        }
        fn select_next(&mut self) {
            self.ctrl('l');
            self.key(KeyCode::Down);
            self.key(KeyCode::Enter);
            self.flush();
        }
    }

    fn start(live: &live::Live, provider: &Provider) {
        start_with_log(live, provider, None);
    }
    fn start_with_log(live: &live::Live, provider: &Provider, log: Option<&std::path::Path>) {
        // Empty every inherited provider credential/endpoint and proxy; HOME/profile paths are
        // private in Live. The only inference endpoint is the feature-gated loopback seam.
        let environment = [
            ("OVRCR_OPERATOR_TEST_API_BASE", OsStr::new(&provider.base)),
            ("OPENAI_API_KEY", OsStr::new("")),
            ("XAI_API_KEY", OsStr::new("")),
            ("GROK_API_KEY", OsStr::new("")),
            ("ANTHROPIC_API_KEY", OsStr::new("")),
            ("OPENAI_ACCESS_TOKEN", OsStr::new("")),
            ("OPENAI_REFRESH_TOKEN", OsStr::new("")),
            ("OPENAI_AUTH_TOKEN", OsStr::new("")),
            ("CHATGPT_ACCESS_TOKEN", OsStr::new("")),
            ("CHATGPT_REFRESH_TOKEN", OsStr::new("")),
            ("CHATGPT_TOKEN", OsStr::new("")),
            ("XAI_ACCESS_TOKEN", OsStr::new("")),
            ("XAI_REFRESH_TOKEN", OsStr::new("")),
            ("GROK_ACCESS_TOKEN", OsStr::new("")),
            ("GROK_REFRESH_TOKEN", OsStr::new("")),
            ("GROK_TOKEN", OsStr::new("")),
            ("OPENAI_BASE_URL", OsStr::new("")),
            ("XAI_BASE_URL", OsStr::new("")),
            ("HTTP_PROXY", OsStr::new("")),
            ("HTTPS_PROXY", OsStr::new("")),
            ("ALL_PROXY", OsStr::new("")),
            ("http_proxy", OsStr::new("")),
            ("https_proxy", OsStr::new("")),
            ("all_proxy", OsStr::new("")),
            ("NO_PROXY", OsStr::new("*")),
            ("no_proxy", OsStr::new("*")),
        ];
        if let Some(log) = log {
            live.start_binary_logged(&environment, log);
        } else {
            live.start_binary_env(&environment);
        }
    }
    fn has_text(history: &OperatorHistory, needle: &str) -> bool {
        history.messages.iter().any(|message| matches!(&message.content,
            OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text)) if text.contains(needle)))
    }
    fn assert_tool(history: &OperatorHistory, label: &str) {
        assert!(history.messages.iter().any(|message| matches!(&message.content,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall { name, arguments, .. }) if name == "bash" && arguments.contains("OVRCR_EXECUTABLE"))));
        assert!(history.messages.iter().any(|message| matches!(&message.content,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput { output, truncated: false, status: OperatorToolStatus::Succeeded, .. }) if output.contains(&format!("E2E_{label}_TOOL")) && cli_array(output).is_ok_and(|value| value == serde_json::json!([])))));
    }
    fn inventory(live: &live::Live, summary: &OperatorConversationSummary) -> SessionSummary {
        let Response::Inventory { sessions, .. } = live.request(Request::Inspect) else {
            panic!("inventory")
        };
        sessions
            .into_iter()
            .find(|session| session.id == summary.session)
            .unwrap()
    }
    fn own_worker(live: &live::Live, summary: &OperatorConversationSummary) -> libc::pid_t {
        let session = inventory(live, summary);
        assert!(session.phase.is_live());
        let pid = session.pid.expect("real compiled worker PID");
        let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
        live.own_group(pgid);
        pgid
    }
    fn assert_retired(live: &live::Live, history: &OperatorHistory, pgid: libc::pid_t) {
        let summary = &history.conversation;
        assert!(
            summary.worker_stopped,
            "retained exact-run controlled stop proof is required"
        );
        assert!(
            live::wait_group_absent(pgid, BUDGET),
            "owned operator group still exists: {pgid}"
        );
        live.forget_group(pgid);
        let session = inventory(live, summary);
        assert!(
            !session.phase.is_live(),
            "controlled worker remains live: {session:?}"
        );
    }

    #[test]
    #[ignore = "exact subprocess fixture; the E2E test supplies its private loopback endpoints"]
    fn seed_grok_auth() {
        let base = std::env::var("OVRCR_OPERATOR_E2E_AUTH_BASE").unwrap();
        let instance = std::env::var_os("OVRCR_OPERATOR_E2E_AUTH_INSTANCE").unwrap();
        super::operator_provider::sign_in_local(&base, std::path::Path::new(&instance));
    }

    fn demo_instance(
        parent: &std::path::Path,
        candidate: &std::path::Path,
    ) -> std::io::Result<std::path::PathBuf> {
        let parent = parent.canonicalize()?;
        let candidate = candidate.canonicalize()?;
        let registry = std::fs::symlink_metadata(candidate.join("registry.sqlite3"))?;
        if candidate.parent() != Some(parent.as_path())
            || !candidate
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("ovrcr-gui-")
            || !registry.is_file()
            || registry.file_type().is_symlink()
        {
            return Err(std::io::Error::other(
                "Demo must be an immediate private TMPDIR child with its real registry",
            ));
        }
        Ok(candidate)
    }

    #[test]
    fn native_holder_accepts_only_the_supplied_demo_parent_and_real_registry() {
        let parent = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        for (root, name, registry, accepted) in [
            (parent.path(), "ovrcr-gui-good", true, true),
            (other.path(), "ovrcr-gui-foreign", true, false),
            (parent.path(), "unrelated", true, false),
            (parent.path(), "ovrcr-gui-incomplete", false, false),
        ] {
            let demo = root.join(name);
            std::fs::create_dir(&demo).unwrap();
            if registry {
                std::fs::write(demo.join("registry.sqlite3"), b"fixture").unwrap();
            }
            assert_eq!(demo_instance(parent.path(), &demo).is_ok(), accepted);
        }
    }

    /// Provider only: root owns the actual GUI, Demo Server, launcher and all their cleanup.
    #[test]
    #[ignore = "manual native CUA provider holder; requires explicit private rendezvous/TMPDIR"]
    fn native_provider_holder() {
        use std::{
            fs,
            os::unix::fs::PermissionsExt,
            path::{Path, PathBuf},
        };
        let control = PathBuf::from(std::env::var_os("OVRCR_OPERATOR_E2E_HOLDER").unwrap())
            .canonicalize()
            .unwrap();
        assert_eq!(
            fs::metadata(&control).unwrap().permissions().mode() & 0o077,
            0,
            "private rendezvous required"
        );
        let parent = PathBuf::from(std::env::var_os("OVRCR_OPERATOR_E2E_DEMO_PARENT").unwrap())
            .canonicalize()
            .unwrap();
        let duration = Duration::from_secs(600);
        let deadline = Instant::now() + duration;
        let provider = Provider::new(&control, duration);
        fs::write(control.join("base.json"), serde_json::json!({"api_base":provider.base,"holder_pid":std::process::id(),"deadline_seconds":duration.as_secs()}).to_string()).unwrap();
        let mut seeded = false;
        while !control.join("finish").exists() {
            assert!(
                Instant::now() < deadline,
                "native provider holder exceeded ten minutes"
            );
            if !seeded && control.join("demo-instance").exists() {
                let path = fs::read_to_string(control.join("demo-instance")).unwrap();
                let instance = demo_instance(&parent, Path::new(path.trim()))
                    .expect("refuse foreign or incomplete Demo");
                provider.sign_in(&instance);
                fs::write(
                    control.join("auth-ready.json"),
                    serde_json::json!({"instance":instance,"provider":"grok","signed_in":true})
                        .to_string(),
                )
                .unwrap();
                seeded = true;
            }
            for (marker, model) in [
                ("release-a", "fixture-a"),
                ("release-b", "fixture-b"),
                ("release-b-followup", "fixture-b-followup"),
                ("release-quick", "fixture-quick"),
            ] {
                if control.join(marker).exists() {
                    provider.release(model);
                }
            }
            provider.assert_healthy();
            thread::sleep(Duration::from_millis(25));
        }
        fs::write(control.join("finished.json"), serde_json::json!({"seeded":seeded,"a_requests":provider.requests("fixture-a").len(),"b_requests":provider.requests("fixture-b").len(),"quick_requests":provider.requests("fixture-quick").len()}).to_string()).unwrap();
    }

    #[test]
    fn siwc_missing_content_type_preserves_completed_success_and_validation_failures() {
        use sha2::{Digest, Sha256};
        use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config);
        let mut credential = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(
                live.config
                    .join("operator-auth")
                    .join(format!("chatgpt-{:x}.json", Sha256::digest(b"default"))),
            )
            .unwrap();
        write!(credential, "{}", serde_json::json!({"client_id":"fixture","subject":"fixture","issuer":"https://auth.openai.com","access_token":"operator-e2e-oauth-only","expires_at":4102444800u64,"scopes":["chatgpt.tokens.use.direct"]})).unwrap();
        drop(credential);
        start(&live, &provider);
        let mut ui = DashboardClient::connect(&live);
        for (index, (provider_kind, model, success)) in [
            (OperatorProvider::ChatGpt, "fixture-siwc-missing", true),
            (OperatorProvider::ChatGpt, "fixture-siwc-wrong", false),
            (OperatorProvider::ChatGpt, "fixture-siwc-malformed", false),
            (OperatorProvider::ChatGpt, "fixture-siwc-incomplete", false),
            (OperatorProvider::ChatGpt, "fixture-siwc-error", false),
            (OperatorProvider::Grok, "fixture-siwc-grok-missing", false),
        ]
        .into_iter()
        .enumerate()
        {
            let conversation = ui.create_provider(provider_kind, model, index == 0);
            ui.submit("Complete the isolated header fixture once");
            let group = own_worker(&live, &conversation);
            provider.release(model);
            let history = ui.wait_history(&provider, conversation.id, |history| {
                history.conversation.worker_stopped
            });
            assert_retired(&live, &history, group);
            assert_eq!(
                history.conversation.current_turn.as_ref().unwrap().state,
                if success {
                    OperatorTurnState::Complete
                } else {
                    OperatorTurnState::Error
                },
                "headerless subscription completion must work while counterexamples remain failures"
            );
            let database = rusqlite::Connection::open_with_flags(
                ovrcr::config::database_path(&live.config),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let saved: i64 = database
                .query_row(
                    "SELECT count(*) FROM operator_model_history WHERE conversation=?1",
                    [i64::try_from(conversation.id.0).unwrap()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(saved, i64::from(success));
            assert!(!history.messages.iter().any(|message| matches!(
                message.content,
                OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall { .. })
            )));
            if success {
                assert!(
                    has_text(&history, "HEADERLESS_E2E_OK")
                        && ui.rendered().contains("HEADERLESS_E2E_OK")
                );
            } else {
                assert!(history.messages.iter().any(|message| matches!(
                    message.content,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(_))
                )));
            }
            if model == "fixture-siwc-error" {
                assert!(history.messages.iter().any(|message| matches!(&message.content, OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(text)) if text.contains("HTTP 400"))));
            }
            assert_eq!(provider.requests(model).len(), 1);
        }
        provider.assert_healthy();
        assert_eq!(ui.request(Request::Shutdown { kill: false }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
    }

    #[test]
    fn diagnostic_real_worker_classifies_provider_failures_without_retaining_remote_secrets() {
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config);
        let log = live.root.path().join("provider-errors.log");
        start_with_log(&live, &provider, Some(&log));
        let mut ui = DashboardClient::connect(&live);
        for (index, (model, kind, status, guidance)) in [
            (
                "fixture-error-http-400",
                "provider_response",
                Some(400),
                "request",
            ),
            (
                "fixture-error-http-401",
                "provider_response",
                Some(401),
                "authorization",
            ),
            (
                "fixture-error-http-403",
                "provider_response",
                Some(403),
                "permission",
            ),
            (
                "fixture-error-http-429",
                "provider_response",
                Some(429),
                "usage",
            ),
            (
                "fixture-error-http-503",
                "provider_response",
                Some(503),
                "temporarily",
            ),
            (
                "fixture-error-failed",
                "provider_response",
                None,
                "response",
            ),
            ("fixture-error-incomplete", "cancelled", None, "complete"),
            ("fixture-error-parse", "json", None, "response"),
            ("fixture-error-transport", "http", None, "connection"),
        ]
        .into_iter()
        .enumerate()
        {
            let conversation = ui.create(model, index == 0);
            ui.submit("Diagnose the isolated provider fixture once");
            let group = own_worker(&live, &conversation);
            provider.release(model);
            let history = ui.wait_history(&provider, conversation.id, |history| {
                history.conversation.worker_stopped
            });
            assert_retired(&live, &history, group);
            assert_eq!(
                history.conversation.current_turn.as_ref().unwrap().state,
                OperatorTurnState::Error
            );
            let failure = history
                .messages
                .iter()
                .find_map(|message| match &message.content {
                    OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(message)) => {
                        Some(message)
                    }
                    _ => None,
                })
                .expect("actual worker persisted its failure");
            assert!(
                failure.contains(&format!("kind={kind}")),
                "missing fixed classification for {model}"
            );
            if let Some(status) = status {
                assert!(
                    failure.contains(&format!("HTTP {status}")),
                    "missing actual HTTP status for {model}"
                );
            }
            assert!(
                failure.to_ascii_lowercase().contains(guidance),
                "missing actionable fixed guidance for {model}"
            );
            assert!(
                !failure.contains("New Conversation"),
                "provider errors must not universally ask for a fresh conversation"
            );
            for message in &history.messages {
                assert!(
                    !serde_json::to_string(message)
                        .unwrap()
                        .contains("FAKE_BEARER_PRIVATE_TOKEN_AND_PROMPT"),
                    "remote secret reached durable history"
                );
            }
            let rendered = ui.rendered();
            assert!(
                !rendered.contains("starts fresh context"),
                "native error surfaces must retain the failure-specific guidance"
            );
            assert!(
                !rendered.contains("FAKE_BEARER_PRIVATE_TOKEN_AND_PROMPT"),
                "remote secret reached native output"
            );
            if model == "fixture-error-http-400" {
                assert!(
                    failure.contains("code=subscription_sharing_unsupported_capability")
                        && failure.contains("param=tools")
                );
            }
            if model == "fixture-error-http-401" {
                assert!(
                    failure.contains("body=detail"),
                    "admission detail bodies need a safe structural label"
                );
            }
            let Response::TerminalText { session, text, .. } =
                live.request(Request::ReadTerminal {
                    session: conversation.session,
                    max_lines: None,
                })
            else {
                panic!("worker diagnostic PTY must remain readable");
            };
            assert_eq!(session, conversation.session);
            assert!(
                !text.contains("FAKE_BEARER_PRIVATE_TOKEN_AND_PROMPT"),
                "remote secret reached worker diagnostic PTY"
            );
            assert!(
                !std::fs::read_to_string(&log)
                    .unwrap()
                    .contains("FAKE_BEARER_PRIVATE_TOKEN_AND_PROMPT"),
                "remote secret reached server logs"
            );
            assert_eq!(
                provider.requests(model).len(),
                1,
                "failure must never retry inference"
            );
            assert!(!history.messages.iter().any(|message| matches!(
                message.content,
                OperatorMessageContent::Worker(
                    OperatorWorkerEvent::ToolCall { .. } | OperatorWorkerEvent::Completed(_)
                )
            )));
        }
        let database = rusqlite::Connection::open_with_flags(
            ovrcr::config::database_path(&live.config),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let saved: i64 = database
            .query_row("SELECT count(*) FROM operator_model_history", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            saved, 0,
            "failed requests must not save completed model context"
        );
        provider.assert_healthy();
        assert_eq!(ui.request(Request::Shutdown { kill: false }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
    }

    #[test]
    fn final_review_public_terminal_lifecycle_refuses_operator_then_native_stop_allows_next_turn() {
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config);
        start(&live, &provider);
        let mut ui = DashboardClient::connect(&live);
        let conversation = ui.create("fixture-lifecycle", true);
        let turn = ui.submit("A exercise public lifecycle refusal");
        let group = own_worker(&live, &conversation);
        ui.wait_history(&provider, conversation.id, |history| {
            has_text(history, "LIFECYCLE waiting")
        });
        for action in ["kill", "close"] {
            let output = live
                .command()
                .args([
                    "--json",
                    "terminal",
                    action,
                    &conversation.session.0.to_string(),
                ])
                .output()
                .unwrap();
            assert!(
                !output.status.success(),
                "generic terminal {action} must refuse an Operator conversation"
            );
            let error = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                error.contains("Operator") && error.contains("Stop"),
                "{error}"
            );
            let history = ui.history(conversation.id);
            assert_eq!(
                history.conversation.current_turn.as_ref().unwrap().state,
                OperatorTurnState::Running
            );
            assert!(!history.conversation.worker_stopped && live::group_exists(group));
            assert!(!inventory(&live, &conversation).archived);
        }
        ui.ctrl('x');
        ui.flush();
        let stopped = ui.wait_history(&provider, conversation.id, |history| {
            history.conversation.worker_stopped
        });
        assert_eq!(
            stopped.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Interrupted
        );
        assert_retired(&live, &stopped, group);
        let next = ui.submit("A supported next turn");
        assert_eq!(next.identity.run.0, turn.identity.run.0 + 1);
        let next_group = own_worker(&live, &conversation);
        ui.wait_history(&provider, conversation.id, |history| {
            history.messages.iter().any(|message| message.turn == next.identity.turn && matches!(&message.content, OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text)) if text.contains("LIFECYCLE waiting")))
        });
        ui.ctrl('x');
        ui.flush();
        let stopped = ui.wait_history(&provider, conversation.id, |history| {
            history.conversation.worker_stopped
        });
        assert_retired(&live, &stopped, next_group);
        provider.assert_healthy();
        assert_eq!(ui.request(Request::Shutdown { kill: false }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
    }

    #[test]
    fn final_review_acknowledged_unexpected_worker_exit_interrupts_exact_turn_and_allows_next() {
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config);
        start(&live, &provider);
        let mut ui = DashboardClient::connect(&live);
        let conversation = ui.create("fixture-lifecycle", true);
        let turn = ui.submit("Independently confirm fixture worker exit");
        let group = own_worker(&live, &conversation);
        ui.wait_history(&provider, conversation.id, |history| {
            has_text(history, "LIFECYCLE waiting")
        });
        assert!(
            matches!(
                ui.request(Request::AcknowledgeSessionStopped {
                    session: conversation.session,
                    expected_run: turn.identity.run
                }),
                Response::Error {
                    code: protocol::ErrorCode::SessionRunning,
                    ..
                }
            ),
            "live worker cannot be acknowledged stopped"
        );
        assert_eq!(unsafe { libc::kill(-group, libc::SIGKILL) }, 0);
        assert!(
            live::wait_group_absent(group, BUDGET),
            "owned fixture group is not independently confirmed absent"
        );
        live.forget_group(group);
        let deadline = Instant::now() + BUDGET;
        while inventory(&live, &conversation).phase.is_live() {
            assert!(
                Instant::now() < deadline,
                "Server did not observe its worker exit"
            );
            thread::sleep(Duration::from_millis(5));
        }
        let before = ui.history(conversation.id);
        assert!(
            !before.conversation.worker_stopped,
            "unexpected exit must retain ownership uncertainty before explicit acknowledgement"
        );
        assert_eq!(
            before.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Running
        );
        assert!(
            matches!(
                ui.request(Request::AcknowledgeSessionStopped {
                    session: conversation.session,
                    expected_run: protocol::SessionRunId(turn.identity.run.0 + 1)
                }),
                Response::Error {
                    code: protocol::ErrorCode::Conflict,
                    ..
                }
            ),
            "stale run cannot reconcile or acknowledge the current turn"
        );
        let output = live
            .command()
            .args([
                "--json",
                "terminal",
                "acknowledge-stopped",
                &conversation.session.0.to_string(),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let acknowledged = ui.history(conversation.id);
        assert!(acknowledged.conversation.worker_stopped);
        let closed = acknowledged.conversation.current_turn.as_ref().unwrap();
        assert_eq!(closed.identity, turn.identity);
        assert_eq!(
            closed.state,
            OperatorTurnState::Interrupted,
            "acknowledged dead worker must not wedge the journal in Running"
        );
        assert_eq!(
            acknowledged
                .messages
                .iter()
                .filter(|message| message.turn == turn.identity.turn
                    && matches!(message.content, OperatorMessageContent::Interrupted))
                .count(),
            1
        );
        let next = ui.submit("Explicit next instructions after independent stop confirmation");
        assert_eq!(next.identity.run.0, turn.identity.run.0 + 1);
        let group = own_worker(&live, &conversation);
        ui.wait_history(&provider, conversation.id, |history| history.messages.iter().any(|message| message.turn == next.identity.turn && matches!(&message.content, OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(text)) if text.contains("LIFECYCLE waiting"))));
        ui.ctrl('x');
        ui.flush();
        let stopped = ui.wait_history(&provider, conversation.id, |history| {
            history.conversation.worker_stopped
        });
        assert_retired(&live, &stopped, group);
        provider.assert_healthy();
        assert_eq!(ui.request(Request::Shutdown { kill: false }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
    }

    #[test]
    fn final_review_real_journal_success_is_unread_until_paginated_tail_is_presented() {
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config);
        start(&live, &provider);
        let mut ui = DashboardClient::connect(&live);
        let conversation = ui.create("fixture-a", true);
        ui.submit("A durable success pagination");
        let group = own_worker(&live, &conversation);
        ui.wait_history(&provider, conversation.id, |history| {
            has_text(history, "A partial waiting")
        });
        ui.create("fixture-b", false);
        provider.release("fixture-a");
        let done = ui.wait_history(&provider, conversation.id, |history| {
            history.conversation.worker_stopped
        });
        assert_eq!(
            done.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Complete
        );
        assert!(!done.messages.iter().any(|message| matches!(
            message.content,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed(_))
        )));
        assert_retired(&live, &done, group);
        assert!(
            ui.rendered().contains("✓ A durable success pagination"),
            "background successful journal reply has no Unread marker"
        );
        drop(ui);
        let mut ui = DashboardClient::connect(&live);
        ui.key(KeyCode::Char('o'));
        ui.pending.extend(ui.dashboard.drain_outbox());
        let mut pages = 0;
        while let Some(mut request) = ui.pending.pop_front() {
            if let Request::Operator(OperatorRequest::Read { limit, .. }) = &mut request.request {
                *limit = 1;
            }
            let response = ui.exchange(request);
            if matches!(
                &response,
                Response::Operator(OperatorResponse::Conversations(_))
            ) {
                ui.ctrl('l');
                ui.key(KeyCode::Enter);
            }
            if let Response::Operator(OperatorResponse::History(history)) = response
                && history.conversation.id == conversation.id
            {
                pages += 1;
                let screen = ui.rendered();
                if history.next_after.is_some() {
                    assert!(
                        screen.contains("✓ A durable success pagination"),
                        "intermediate durable page cleared Unread"
                    );
                }
            }
        }
        assert!(pages > 2, "real journal pagination must run");
        assert!(
            !ui.rendered().contains("✓ A durable success pagination"),
            "presented confirmed tail retained Unread"
        );
        provider.assert_healthy();
        assert_eq!(ui.request(Request::Shutdown { kill: false }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
    }

    #[test]
    fn compiled_incomplete_response_preserves_partial_but_never_saves_completed_context() {
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config);
        start(&live, &provider);
        let mut ui = DashboardClient::connect(&live);
        let conversation = ui.create("fixture-incomplete", true);
        let turn = ui.submit("Show the incomplete response once");
        let group = own_worker(&live, &conversation);
        ui.wait_history(&provider, conversation.id, |history| {
            has_text(history, "INCOMPLETE partial visible")
        });
        assert!(ui.rendered().contains("INCOMPLETE partial visible"));
        provider.release("fixture-incomplete");
        let history = ui.wait_history(&provider, conversation.id, |history| {
            history.conversation.worker_stopped
        });
        assert_retired(&live, &history, group);
        let database = rusqlite::Connection::open_with_flags(
            ovrcr::config::database_path(&live.config),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let saved: i64 = database
            .query_row(
                "SELECT count(*) FROM operator_model_history WHERE conversation=?1",
                [i64::try_from(conversation.id.0).unwrap()],
                |row| row.get(0),
            )
            .unwrap();
        let terminal = history.conversation.current_turn.as_ref().unwrap();
        eprintln!(
            "INCOMPLETE_OUTCOME state={:?} completed_history_rows={saved}",
            terminal.state
        );
        assert_eq!(terminal.identity, turn.identity);
        assert_eq!(
            terminal.state,
            OperatorTurnState::Error,
            "incomplete provider termination must not become Complete"
        );
        assert!(has_text(&history, "INCOMPLETE partial visible"));
        assert!(history.messages.iter().any(|message| matches!(&message.content, OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(text)) if !text.is_empty())));
        assert!(!history.messages.iter().any(|message| matches!(
            message.content,
            OperatorMessageContent::Worker(
                OperatorWorkerEvent::ToolCall { .. } | OperatorWorkerEvent::Completed(_)
            )
        )));
        assert_eq!(
            saved, 0,
            "incomplete response must not become reusable completed model context"
        );
        assert_eq!(provider.requests("fixture-incomplete").len(), 1);
        assert_eq!(provider.inference_attempts(), 1);
        provider.assert_healthy();
        assert_eq!(ui.request(Request::Shutdown { kill: false }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
    }

    #[test]
    fn compiled_missing_auth_is_visible_and_controlled_retired_without_provider_http() {
        use sha2::{Digest, Sha256};
        use std::{
            fs::OpenOptions,
            os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
        };
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config); // Grok only; ChatGPT remains signed out.
        // Existing credential serialization holds the worker alive for owned-PGID capture.
        let path = live
            .config
            .join("operator-auth")
            .join(format!("chatgpt-{:x}.lock", Sha256::digest(b"default")));
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        start(&live, &provider);
        let mut ui = DashboardClient::connect(&live);
        let conversation = ui.create_provider(OperatorProvider::ChatGpt, "fixture-a", true);
        let turn = ui.submit("Show the signed-out setup failure");
        let group = own_worker(&live, &conversation);
        drop(lock);
        let history = ui.wait_history(&provider, conversation.id, |history| {
            history.conversation.worker_stopped
        });
        assert_eq!(
            history.conversation.current_turn.as_ref().unwrap().identity,
            turn.identity
        );
        assert_eq!(
            history.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Error
        );
        assert!(history.messages.iter().any(|message| matches!(&message.content, OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(text)) if text.contains("sign in"))));
        assert!(!history.messages.iter().any(|message| matches!(
            message.content,
            OperatorMessageContent::Worker(
                OperatorWorkerEvent::ToolCall { .. } | OperatorWorkerEvent::Completed(_)
            )
        )));
        assert!(
            ui.rendered().contains("sign in"),
            "signed-out failure must be visible in the native transcript"
        );
        assert_retired(&live, &history, group);
        assert_eq!(
            provider.inference_attempts(),
            0,
            "missing auth must fail before any model HTTP request"
        );
        provider.assert_healthy();
        assert_eq!(ui.request(Request::Shutdown { kill: false }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
    }

    #[test]
    fn operator_quick_message_records_one_captured_first_message() {
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config);
        start(&live, &provider);
        live.ready("feature/quick");
        let mut ui = DashboardClient::connect(&live);
        let background = ui.create("fixture-b", true);
        ui.submit("Background completion while quick receipts wait");
        let background_group = own_worker(&live, &background);
        ui.wait_history(&provider, background.id, |history| {
            has_text(history, "B partial waiting")
        });
        ui.key(KeyCode::Esc);
        let Response::Hierarchy(hierarchy) = live.request(Request::List) else {
            panic!("hierarchy")
        };
        let terminal = hierarchy
            .projects
            .iter()
            .flat_map(|p| &p.workspaces)
            .flat_map(|w| &w.sessions)
            .find(|s| s.name == "local")
            .unwrap()
            .clone();
        ui.dashboard.install_hierarchy(hierarchy);
        ui.dashboard.install_focus(terminal.id);
        ui.flush();
        let mut settings = Settings::default();
        settings.operator.provider = OperatorProvider::Grok;
        settings.operator.grok_model = Some("fixture-quick".into());
        ui.dashboard.install_settings(settings);
        ui.ctrl('o');
        ui.paste("Frozen first\nmessage 界🙂");
        ui.key(KeyCode::Enter);
        let create = ui.take_operator_request();
        assert!(matches!(
            create.request,
            Request::Operator(OperatorRequest::Create { .. })
        ));
        let created = ui.hold(create);
        let ServerMessage::Response {
            response: Response::Operator(OperatorResponse::Created(summary)),
            ..
        } = &created
        else {
            panic!("create receipt")
        };
        let conversation = summary.clone();
        ui.key(KeyCode::Esc);
        assert!(!ui.rendered().contains("Quick message · Ctrl-o"));
        provider.release("fixture-b");
        let completed =
            ui.wait_history(&provider, background.id, |h| h.conversation.worker_stopped);
        assert_retired(&live, &completed, background_group);
        assert!(
            ui.history(conversation.id).messages.is_empty(),
            "held Create must not submit"
        );
        ui.receive(created);
        let submit = ui.take_operator_request();
        let Request::Operator(OperatorRequest::Submit { text, context, .. }) = &submit.request
        else {
            panic!("quick submit")
        };
        assert_eq!(text, "Frozen first\nmessage 界🙂");
        let frozen = context.clone();
        assert_eq!(frozen.session, Some(terminal.id));
        assert_eq!(frozen.run, Some(terminal.run));
        assert!(frozen.workspace_id.is_some() && frozen.cwd.is_some());
        let submitted = ui.hold(submit);
        let group = own_worker(&live, &conversation);
        ui.ctrl('o');
        ui.key(KeyCode::Enter);
        ui.key(KeyCode::Esc);
        assert!(
            ui.dashboard.drain_outbox().is_empty(),
            "pending Enter must not create/submit again"
        );
        provider.release("fixture-quick");
        let done = ui.wait_history(&provider, conversation.id, |h| {
            h.conversation.worker_stopped
        });
        assert_retired(&live, &done, group);
        ui.receive(submitted);
        ui.flush();
        let users: Vec<_> = done
            .messages
            .iter()
            .filter_map(|m| {
                if let OperatorMessageContent::User { text, context } = &m.content {
                    Some((text.as_str(), context))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(users, vec![("Frozen first\nmessage 界🙂", &frozen)]);
        assert_eq!(provider.requests("fixture-quick").len(), 1);
        assert_eq!(
            provider.inference_attempts(),
            3,
            "two B tool requests plus one quick inference"
        );
        let Response::TerminalText { text, .. } = live.request(Request::ReadTerminal {
            session: terminal.id,
            max_lines: None,
        }) else {
            panic!("terminal read")
        };
        assert!(!text.contains("Frozen first") && !text.contains("message 界🙂"));
        assert_eq!(
            ui.dashboard.focused_session(),
            Some(terminal.id),
            "late receipts stole terminal focus"
        );
        // Real create failure is delivered after dismissal without retry or focus theft.
        ui.ctrl('o');
        ui.paste("failed create");
        ui.key(KeyCode::Enter);
        let mut rejected = ui.take_operator_request();
        let Request::Operator(OperatorRequest::Create { model, .. }) = &mut rejected.request else {
            panic!("create")
        };
        *model = " ".into();
        let failure = ui.hold(rejected);
        assert!(matches!(
            &failure,
            ServerMessage::Response {
                response: Response::Error { .. },
                ..
            }
        ));
        ui.key(KeyCode::Esc);
        ui.receive(failure);
        ui.flush();
        assert_eq!(provider.requests("fixture-quick").len(), 1);
        provider.assert_healthy();
        assert_eq!(ui.request(Request::Shutdown { kill: true }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
    }

    #[test]
    fn operator_close_archive_restores_history_without_replay() {
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config);
        start(&live, &provider);
        live.ready("feature/close");
        let mut ui = DashboardClient::connect(&live);
        let a = ui.create("fixture-close-a", true);
        let first = ui.submit("A hold a real Bash child");
        let a_group = own_worker(&live, &a);
        let b = ui.create("fixture-close-b", false);
        ui.submit("B hold a separate real Bash child");
        let b_group = own_worker(&live, &b);
        let children: Vec<libc::pid_t> =
            [("fixture-close-a", a_group), ("fixture-close-b", b_group)]
                .into_iter()
                .map(|(model, group)| {
                    let deadline = Instant::now() + BUDGET;
                    loop {
                        if let Ok(text) =
                            std::fs::read_to_string(live.root.path().join(format!("{model}.pid")))
                            && let Ok(pid) = text.trim().parse::<libc::pid_t>()
                        {
                            assert!(pid > 1 && pid != group);
                            assert_eq!(unsafe { libc::getpgid(pid) }, group);
                            eprintln!(
                                "OPERATOR_CLOSE_OWNED_CHILD model={model} pid={pid} pgid={group}"
                            );
                            break pid;
                        }
                        provider.assert_healthy();
                        assert!(
                            Instant::now() < deadline,
                            "real Bash child did not publish PID"
                        );
                        thread::yield_now();
                    }
                })
                .collect();
        let closed = ui.request(Request::Operator(OperatorRequest::Close {
            conversation: a.id,
            expected_run: first.identity.run,
        }));
        let Response::Operator(OperatorResponse::Closed(closed)) = closed else {
            panic!("close receipt")
        };
        assert!(closed.archived && closed.worker_stopped);
        let archived = ui.history(a.id);
        assert_retired(&live, &archived, a_group);
        assert_eq!(unsafe { libc::kill(children[0], 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(
            unsafe { libc::kill(children[1], 0) },
            0,
            "close A killed B's Bash child"
        );
        assert!(live::group_exists(b_group));
        assert!(!ui.history(b.id).conversation.worker_stopped);
        let Response::Hierarchy(hierarchy) = live.request(Request::List) else {
            panic!("hierarchy")
        };
        let terminal = hierarchy
            .projects
            .iter()
            .flat_map(|p| &p.workspaces)
            .flat_map(|w| &w.sessions)
            .find(|s| s.name == "local")
            .unwrap()
            .id;
        let output = live
            .command()
            .args([
                "terminal",
                "send",
                &terminal.0.to_string(),
                "--text",
                "printf 'CLOSE_%s\n' TERMINAL_OK\n",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "unrelated terminal input refused: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let deadline = Instant::now() + BUDGET;
        loop {
            let Response::TerminalText { text, .. } = live.request(Request::ReadTerminal {
                session: terminal,
                max_lines: None,
            }) else {
                panic!("terminal read")
            };
            if text.lines().any(|line| line.trim() == "CLOSE_TERMINAL_OK") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "unrelated terminal did not answer: {text}"
            );
            thread::yield_now();
        }
        std::fs::write(
            live.root.path().join("fixture-close-b.bash-release"),
            b"release",
        )
        .unwrap();
        provider.release("fixture-close-b");
        let completed_b = ui.wait_history(&provider, b.id, |h| h.conversation.worker_stopped);
        assert_eq!(
            completed_b
                .conversation
                .current_turn
                .as_ref()
                .unwrap()
                .state,
            OperatorTurnState::Complete,
            "B did not continue after A close: {completed_b:?}"
        );
        assert!(completed_b.messages.iter().any(|m| matches!(&m.content, OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput { output, status: OperatorToolStatus::Succeeded, .. }) if output.contains("CLOSE_BASH_OK"))), "actual B Bash result was not committed");
        assert_eq!(provider.requests("fixture-close-b").len(), 2);
        assert_retired(&live, &completed_b, b_group);
        assert_eq!(unsafe { libc::kill(children[1], 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(ui.request(Request::Shutdown { kill: true }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
        start(&live, &provider);
        let mut ui = DashboardClient::connect(&live);
        assert_eq!(ui.history(a.id), archived);
        assert_eq!(provider.requests("fixture-close-a").len(), 1);
        assert_eq!(
            ui.request(Request::UnarchiveSession {
                session: a.session,
                expected_run: closed.run
            }),
            Response::Ok
        );
        let restored = ui.history(a.id);
        assert!(!restored.conversation.archived && restored.conversation.worker_stopped);
        assert_eq!(restored.messages, archived.messages);
        assert_eq!(
            provider.requests("fixture-close-a").len(),
            1,
            "restore replayed inference"
        );
        let Response::Operator(OperatorResponse::Submitted(next)) =
            ui.request(Request::Operator(OperatorRequest::Submit {
                conversation: a.id,
                expected_run: restored.conversation.run,
                text: "Explicit retained follow-up".into(),
                context: Default::default(),
            }))
        else {
            panic!("explicit submit")
        };
        assert_eq!(next.identity.session, a.session);
        assert!(next.identity.run.0 > first.identity.run.0);
        let group = own_worker(&live, &a);
        provider.release("fixture-close-a");
        let done = ui.wait_history(&provider, a.id, |h| h.conversation.worker_stopped);
        assert_retired(&live, &done, group);
        assert_eq!(done.messages.iter().filter(|m| matches!(&m.content, OperatorMessageContent::User { text, .. } if text == "Explicit retained follow-up")).count(), 1);
        assert_eq!(
            provider.requests("fixture-close-a").len(),
            2,
            "restored result: {done:?}"
        );
        assert_eq!(
            std::fs::read_to_string(live.root.path().join("fixture-close-a.executions")).unwrap(),
            "started\n",
            "restoration replayed Bash"
        );
        provider.assert_healthy();
        assert_eq!(ui.request(Request::Shutdown { kill: false }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
    }

    #[test]
    fn compiled_worker_timeout_stops_the_actual_bash_descendant_without_retry() {
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config);
        start(&live, &provider);
        let mut ui = DashboardClient::connect(&live);
        let conversation = ui.create("fixture-timeout", true);
        let turn = ui.submit("Run the bounded timeout fixture once");
        let group = own_worker(&live, &conversation);
        let pid_path = live.root.path().join("timeout.pid");
        let deadline = Instant::now() + BUDGET;
        let child: libc::pid_t = loop {
            if let Ok(text) = std::fs::read_to_string(&pid_path)
                && let Ok(pid) = text.trim().parse()
            {
                break pid;
            }
            provider.assert_healthy();
            assert!(
                Instant::now() < deadline,
                "Bash never published its real child PID"
            );
            thread::sleep(Duration::from_millis(5));
        };
        assert!(child > 1);
        assert_ne!(child, group);
        assert_eq!(
            unsafe { libc::getpgid(child) },
            group,
            "Bash must remain in its managed worker group"
        );
        assert_eq!(
            unsafe { libc::kill(child, 0) },
            0,
            "capture the actual child while alive"
        );
        eprintln!("OPERATOR_TIMEOUT_OWNED_CHILD pid={child} pgid={group}");
        let history = ui.wait_history(&provider, conversation.id, |history| {
            history.conversation.worker_stopped
        });
        assert_eq!(
            history.conversation.current_turn.as_ref().unwrap().identity,
            turn.identity
        );
        assert_eq!(
            history.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Interrupted
        );
        assert_eq!(
            history
                .messages
                .iter()
                .filter(|message| matches!(
                    message.content,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall { .. })
                ))
                .count(),
            1
        );
        assert_eq!(
            history
                .messages
                .iter()
                .filter(|message| matches!(
                    message.content,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                        status: OperatorToolStatus::TimedOut,
                        ..
                    })
                ))
                .count(),
            1,
            "actual timeout result must be durable before worker Stop"
        );
        assert!(!history.messages.iter().any(|message| matches!(
            message.content,
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed(_))
        )));
        assert_retired(&live, &history, group);
        assert_eq!(unsafe { libc::kill(child, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH),
            "timeout child remains or absence is unproven"
        );
        assert_eq!(
            provider.requests("fixture-timeout").len(),
            1,
            "timeout retried a model/tool request"
        );
        assert_eq!(
            std::fs::read_to_string(live.root.path().join("timeout.executions")).unwrap(),
            "started\n"
        );
        provider.assert_healthy();
        assert_eq!(ui.request(Request::Shutdown { kill: false }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
    }

    #[test]
    fn compiled_workers_stream_tools_stop_independently_and_restart_without_replay() {
        let live = live::Live::idle().bounded();
        let provider = Provider::new(live.root.path(), BUDGET);
        provider.sign_in(&live.config);
        start(&live, &provider);
        let mut ui = DashboardClient::connect(&live);
        let a = ui.create("fixture-a", true);
        assert!(a.worker_stopped && a.current_turn.is_none());
        assert!(provider.requests("fixture-a").is_empty());
        let turn_a = ui.submit("A run the isolated CLI");
        let a_group = own_worker(&live, &a);
        let b = ui.create("fixture-b", false);
        let turn_b = ui.submit("B run the isolated CLI");
        let b_group = own_worker(&live, &b);
        assert_ne!(turn_a.identity.session, turn_b.identity.session);
        let a_running = ui.wait_history(&provider, a.id, |history| {
            has_text(history, "A partial waiting")
        });
        let b_running = ui.wait_history(&provider, b.id, |history| {
            has_text(history, "B partial waiting")
        });
        for (history, label) in [(&a_running, "A"), (&b_running, "B")] {
            assert_eq!(
                history.conversation.current_turn.as_ref().unwrap().state,
                OperatorTurnState::Running
            );
            assert!(!history.conversation.worker_stopped);
            assert_tool(history, label);
            assert!(!history.messages.iter().any(|message| matches!(
                &message.content,
                OperatorMessageContent::Worker(OperatorWorkerEvent::Completed(_))
            )));
        }
        assert!(!has_text(&a_running, "B partial") && !has_text(&b_running, "A partial"));
        assert_ne!(a_group, b_group);
        assert!(
            ui.changed.contains(&a.id) && ui.changed.contains(&b.id),
            "real Server dirty events must drive Dashboard catch-up"
        );
        ui.paste("B retained draft");
        ui.key(KeyCode::Enter);
        assert!(
            ui.flush()
                .iter()
                .all(|reply| !matches!(reply, Response::Operator(OperatorResponse::Submitted(_)))),
            "busy chat accepted another turn"
        );
        ui.select_previous();
        ui.paste("A retained draft");
        let a_view = ui.rendered();
        assert!(
            a_view.contains("A partial waiting") && a_view.contains("A retained draft"),
            "A transcript/draft lost: {a_view}"
        );
        assert!(!a_view.contains("B partial waiting"));
        ui.ctrl('x');
        let stopped = ui.flush();
        assert!(stopped.iter().any(|reply| matches!(reply,
            Response::Operator(OperatorResponse::Stopped(turn)) if turn.identity == turn_a.identity)));
        let a_stopped = ui.wait_history(&provider, a.id, |history| {
            history.conversation.worker_stopped
        });
        assert_eq!(
            a_stopped.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Interrupted
        );
        assert_eq!(
            a_stopped
                .messages
                .iter()
                .filter(|message| matches!(message.content, OperatorMessageContent::Interrupted))
                .count(),
            1
        );
        assert_retired(&live, &a_stopped, a_group);
        let still_b = ui.history(b.id);
        assert_eq!(
            still_b.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Running
        );
        assert!(!still_b.conversation.worker_stopped && live::group_exists(b_group));
        provider.release("fixture-b");
        let b_done = ui.wait_history(&provider, b.id, |history| {
            history.conversation.worker_stopped
        });
        assert_eq!(
            b_done.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Complete
        );
        assert!(has_text(&b_done, "B final"));
        assert_retired(&live, &b_done, b_group);
        ui.select_next();
        let b_view = ui.rendered();
        assert!(
            b_view.contains("B final")
                && b_view.contains("B retained draft")
                && b_view.contains("complete"),
            "B transcript/draft lost: {b_view}"
        );
        assert!(!b_view.contains("A partial waiting"));
        for _ in "B retained draft".chars() {
            ui.key(KeyCode::Backspace);
        }
        let followup = ui.submit("B continue without another command");
        assert_eq!(followup.identity.conversation, b.id);
        assert_eq!(followup.identity.session, turn_b.identity.session);
        assert_eq!(followup.identity.run.0, turn_b.identity.run.0 + 1);
        ui.wait_history(&provider, b.id, |history| {
            has_text(history, "B follow-up waiting")
        });
        let followup_group = own_worker(&live, &b);
        let requests = provider.requests("fixture-b");
        assert_eq!(requests.len(), 3);
        let input = requests[2]["input"].as_array().unwrap();
        let serialized = requests[2]["input"].to_string();
        for text in [
            "B run the isolated CLI",
            "B before tool",
            "B partial waiting B final",
            "B continue without another command",
        ] {
            assert_eq!(
                serialized.matches(text).count(),
                1,
                "saved/new model message missing or duplicated: {text}: {serialized}"
            );
        }
        let calls: Vec<_> = input
            .iter()
            .filter(|item| item["type"] == "function_call")
            .collect();
        let outputs: Vec<_> = input
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .collect();
        assert_eq!(
            calls.len(),
            1,
            "saved tool call missing or duplicated: {serialized}"
        );
        assert_eq!(
            outputs.len(),
            1,
            "saved tool result missing or duplicated: {serialized}"
        );
        assert_eq!(calls[0]["name"], "bash");
        assert_eq!(calls[0]["call_id"], outputs[0]["call_id"]);
        assert!(outputs[0]["output"].to_string().contains("E2E_B_TOOL"));
        assert!(outputs[0]["output"].to_string().contains("[]"));
        provider.release("fixture-b-followup");
        let b_done = ui.wait_history(&provider, b.id, |history| {
            history.conversation.worker_stopped
        });
        assert_eq!(
            b_done.conversation.current_turn.as_ref().unwrap().identity,
            followup.identity
        );
        assert_eq!(
            b_done.conversation.current_turn.as_ref().unwrap().state,
            OperatorTurnState::Complete
        );
        assert!(has_text(&b_done, "B follow-up final"));
        assert_eq!(
            b_done
                .messages
                .iter()
                .filter(|message| matches!(
                    message.content,
                    OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall { .. })
                ))
                .count(),
            1,
            "follow-up repeated Bash"
        );
        assert_retired(&live, &b_done, followup_group);
        for model in ["fixture-a", "fixture-b"] {
            assert_eq!(
                provider.requests(model).len(),
                if model == "fixture-b" { 3 } else { 2 }
            );
            assert_eq!(
                std::fs::read_to_string(live.root.path().join(format!("{model}.executions")))
                    .unwrap(),
                "ran\n"
            );
        }
        assert_eq!(ui.request(Request::Shutdown { kill: false }), Response::Ok);
        drop(ui);
        assert!(live.join_within(BUDGET).is_finished());
        start(&live, &provider);
        let mut restored = DashboardClient::connect(&live);
        let a_after = restored.history(a.id);
        let b_after = restored.history(b.id);
        assert_eq!(
            a_after, a_stopped,
            "restart changed interrupted transcript or replayed work"
        );
        assert_eq!(
            b_after, b_done,
            "restart changed completed transcript or model history framing"
        );
        for summary in [&a, &b] {
            assert_eq!(inventory(&live, summary).phase, SessionPhase::Stopped);
        }
        for model in ["fixture-a", "fixture-b"] {
            assert_eq!(
                provider.requests(model).len(),
                if model == "fixture-b" { 3 } else { 2 },
                "history/restart launched another model request"
            );
            assert_eq!(
                std::fs::read_to_string(live.root.path().join(format!("{model}.executions")))
                    .unwrap(),
                "ran\n",
                "interrupted Bash replayed"
            );
        }
        provider.assert_healthy();
        assert_eq!(
            restored.request(Request::Shutdown { kill: false }),
            Response::Ok
        );
        drop(restored);
        assert!(live.join_within(BUDGET).is_finished());
    }
}

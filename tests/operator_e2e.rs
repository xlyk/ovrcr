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
        // Empty every inherited provider credential/endpoint and proxy; HOME/profile paths are
        // private in Live. The only inference endpoint is the feature-gated loopback seam.
        live.start_binary_env(&[
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
        ]);
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
            ] {
                if control.join(marker).exists() {
                    provider.release(model);
                }
            }
            provider.assert_healthy();
            thread::sleep(Duration::from_millis(25));
        }
        fs::write(control.join("finished.json"), serde_json::json!({"seeded":seeded,"a_requests":provider.requests("fixture-a").len(),"b_requests":provider.requests("fixture-b").len()}).to_string()).unwrap();
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

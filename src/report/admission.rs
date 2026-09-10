//! Initial-only Claude admission. Provider interpretation stays outside runtime.
use super::{
    InvocationLease,
    claude::{ClaudeEventKind, parse_claude_hook},
};
use ovrcr_protocol::{
    AgentCommand, AgentObservation, AgentOperationResult, HealthSample, ProviderReport,
    ReporterHealth, Response,
};
use std::{
    ffi::{OsStr, OsString},
    io::Read,
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn receiver(
    lease: Option<InvocationLease>,
    argv: &mut Vec<OsString>,
) -> ovrcr_runtime::agent_runner::HookHandler {
    let expected = if lease.is_some() && eligible(argv) && pinned_version(&argv[0]) {
        fresh_uuid().ok()
    } else {
        None
    };
    if let Some(expected) = &expected {
        argv.splice(
            1..1,
            [OsString::from("--session-id"), OsString::from(expected)],
        );
        eprintln!("agent awaiting certified startup");
    } else if lease.is_some() {
        eprintln!("agent admission unavailable; running native command");
    } else {
        eprintln!("agent reporting unavailable; running native command");
    }
    let mut receiver = Receiver {
        lease,
        expected,
        phase: Phase::Waiting,
        prompt: None,
        activity_revision: 0,
        metrics: None,
        metrics_revision: 0,
        health_revision: 0,
        source_health: None,
        transcript_path: None,
        collector: None,
        cost_watermark: None,
        collector_caught_up: false,
    };
    Box::new(move |event| {
        use ovrcr_runtime::agent_runner::HookEvent;
        match event {
            HookEvent::Request { input, deadline } => receiver.handle(input, deadline),
            HookEvent::Poll { deadline } => {
                receiver.poll(deadline);
                Vec::new()
            }
            HookEvent::NativeCompleted { deadline } => {
                receiver.native_completed(deadline);
                Vec::new()
            }
        }
    })
}

fn eligible(argv: &[OsString]) -> bool {
    if unsafe { libc::isatty(0) } != 1 || unsafe { libc::isatty(1) } != 1 {
        return false;
    }
    eligible_argv(argv)
}
fn eligible_argv(argv: &[OsString]) -> bool {
    if argv.first().and_then(|arg| Path::new(arg).file_name()) != Some(OsStr::new("claude")) {
        return false;
    }
    let values = [
        "--model",
        "--permission-mode",
        "--agent",
        "--agents",
        "--settings",
        "--setting-sources",
        "--system-prompt",
        "--append-system-prompt",
        "--name",
        "-n",
    ];
    let flags = [
        "--strict-mcp-config",
        "--verbose",
        "--dangerously-skip-permissions",
        "--allow-dangerously-skip-permissions",
    ];
    let mut index = 1;
    let mut prompt = false;
    while index < argv.len() {
        let Some(arg) = argv[index].to_str() else {
            return false;
        };
        if arg == "--" {
            return !prompt && argv.len() == index + 2;
        }
        if arg.starts_with('-') {
            let (name, value) = arg
                .split_once('=')
                .map_or((arg, None), |(name, value)| (name, Some(value)));
            if values.contains(&name) {
                if value.is_none() {
                    index += 1;
                    if index >= argv.len() || argv[index].as_encoded_bytes().starts_with(b"-") {
                        return false;
                    }
                }
            } else if !flags.contains(&name) || value.is_some() {
                return false;
            }
        } else {
            // Explicit -- permits a prompt that otherwise resembles a management command.
            const COMMANDS: &[&str] = &[
                "agents",
                "attach",
                "auth",
                "auto-mode",
                "doctor",
                "gateway",
                "import",
                "install",
                "logs",
                "mcp",
                "plugin",
                "plugins",
                "project",
                "respawn",
                "rm",
                "setup-token",
                "stop",
                "kill",
                "ultrareview",
                "update",
                "upgrade",
            ];
            if prompt || COMMANDS.contains(&arg) {
                return false;
            }
            prompt = true;
        }
        index += 1;
    }
    true
}
pub fn pinned_version(executable: &OsStr) -> bool {
    let mut command = Command::new(executable);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    for name in [
        "OVRCR_HOOK_SOCKET",
        "OVRCR_SESSION_ID",
        "OVRCR_HOOK_TOKEN",
        "OVRCR_AGENT_SOCKET",
        "OVRCR_AGENT_TOKEN",
    ] {
        command.env_remove(name);
    }
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let mut stdout = child.stdout.take().expect("piped version stdout");
    let fd = stdout.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut bytes = Vec::new();
    if flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0 {
        loop {
            let mut buffer = [0u8; 128];
            match stdout.read(&mut buffer) {
                Ok(count) => bytes.extend_from_slice(&buffer[..count]),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => break,
            }
            if bytes.len() > 128 {
                break;
            }
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let waited = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if waited != 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            if unsafe { info.si_pid() } != 0 {
                // Keep the zombie leader as the ownership anchor until group cleanup.
                unsafe {
                    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                }
                let status = child.wait();
                let _ = stdout
                    .take(129 - bytes.len() as u64)
                    .read_to_end(&mut bytes);
                return status.is_ok_and(|status| status.success())
                    && bytes == b"2.1.267 (Claude Code)\n";
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    // This unreaped direct child still anchors the process group on timeout/error.
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.wait();
    false
}
fn fresh_uuid() -> std::io::Result<String> {
    let identifier = ovrcr_runtime::agent_runner::private_identifier()?;
    let mut bytes = identifier.as_bytes()[..32].to_vec();
    bytes[12] = b'4';
    let variant = (bytes[16] as char).to_digit(16).unwrap() & 3 | 8;
    bytes[16] = char::from_digit(variant, 16).unwrap() as u8;
    let hex = String::from_utf8(bytes).unwrap();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

enum Phase {
    Waiting,
    Binding { operation: String },
    Bound,
    Closed,
}
struct Receiver {
    lease: Option<InvocationLease>,
    expected: Option<String>,
    phase: Phase,
    prompt: Option<String>,
    activity_revision: u64,
    metrics: Option<ovrcr_protocol::MetricsSample>,
    metrics_revision: u64,
    health_revision: u64,
    source_health: Option<String>,
    transcript_path: Option<String>,
    collector: Option<super::collector::CollectorController>,
    cost_watermark: Option<u64>,
    collector_caught_up: bool,
}
impl Receiver {
    fn handle(&mut self, input: &[u8], deadline: Instant) -> Vec<u8> {
        let Some(expected) = self.expected.clone() else {
            return b"admission-unavailable\n".to_vec();
        };
        let Ok(request) = serde_json::from_slice::<serde_json::Value>(input) else {
            return b"admission-ignored\n".to_vec();
        };
        if request.get("provider").and_then(|v| v.as_str()) == Some("claude")
            && request.get("origin").and_then(|v| v.as_str()) == Some("claude-statusline")
        {
            #[derive(serde::Deserialize)]
            struct RawEnvelope<'a> {
                #[serde(borrow)]
                payload: &'a serde_json::value::RawValue,
            }
            if let Ok(raw) = serde_json::from_slice::<RawEnvelope<'_>>(input) {
                self.statusline(raw.payload.get().as_bytes(), deadline);
            }
            return b"admission-ignored\n".to_vec();
        }
        if request.get("provider").and_then(|v| v.as_str()) != Some("claude")
            || request.get("origin").and_then(|v| v.as_str()) != Some("claude-hook")
        {
            return b"admission-ignored\n".to_vec();
        }
        let Some(payload) = request.get("payload") else {
            return b"admission-ignored\n".to_vec();
        };
        let Ok(Some(event)) = parse_claude_hook(&serde_json::to_vec(payload).unwrap_or_default())
        else {
            return b"admission-ignored\n".to_vec();
        };
        let transition = matches!(&event.kind, ClaudeEventKind::SessionStart { source } if matches!(source.as_str(), "clear" | "resume" | "fork"));
        if event.session != expected && !transition {
            return b"admission-ignored\n".to_vec();
        }
        let clear = matches!(&event.kind, ClaudeEventKind::SessionEnd { reason } if reason.as_deref() == Some("clear"));
        if transition || clear {
            self.freeze(deadline);
            return b"admission-ignored\n".to_vec();
        }
        if let Some(state) = event.kind.activity() {
            if !matches!(self.phase, Phase::Bound) {
                return b"admission-ignored\n".to_vec();
            }
            let Some(prompt) = event.prompt else {
                return b"admission-ignored\n".to_vec();
            };
            // Supported synchronous UserPromptSubmit hooks establish observed turn identity.
            // Opaque prompt IDs and transport revisions are not certified source ordering.
            if matches!(event.kind, ClaudeEventKind::Prompt) {
                self.prompt = Some(prompt.clone());
            } else if self.prompt.as_ref() != Some(&prompt) {
                return b"admission-ignored\n".to_vec();
            }
            let Some(lease) = &self.lease else {
                return b"admission-unavailable\n".to_vec();
            };
            let Some(binding) = lease.binding.clone() else {
                return b"admission-unavailable\n".to_vec();
            };
            self.activity_revision += 1;
            let report = ProviderReport {
                binding,
                revision: self.activity_revision,
                observation: AgentObservation::Activity(ovrcr_protocol::ActivitySample {
                    state,
                    quality: ovrcr_protocol::SampleQuality::Observed,
                    turn: Some(prompt),
                }),
            };
            if lease.publish_observation(report, deadline).is_err() {
                // Delivery uncertainty cannot leave a connected reporter claiming continuity.
                self.disconnect(deadline);
                return b"admission-unavailable\n".to_vec();
            }
            return b"admission-accepted\n".to_vec();
        }
        if !matches!(&event.kind, ClaudeEventKind::SessionStart { source } if source == "startup") {
            return b"admission-ignored\n".to_vec();
        }
        let Some(lease) = &mut self.lease else {
            return b"admission-unavailable\n".to_vec();
        };
        let response = match &self.phase {
            Phase::Waiting => {
                self.transcript_path = event.transcript_path.clone();
                let Ok(operation) = ovrcr_runtime::agent_runner::private_identifier() else {
                    self.phase = Phase::Closed;
                    return b"admission-unavailable\n".to_vec();
                };
                self.phase = Phase::Binding {
                    operation: operation.clone(),
                };
                lease.command(
                    operation,
                    AgentCommand::Bind {
                        expected_binding: None,
                        conversation: expected,
                    },
                    deadline,
                )
            }
            Phase::Binding { operation, .. } => lease.operation_status(operation.clone(), deadline),
            Phase::Bound | Phase::Closed => return b"admission-ignored\n".to_vec(),
        };
        match response {
            Ok(Response::AgentOperation(AgentOperationResult::Bound(binding))) => {
                lease.binding = Some(binding);
                self.phase = Phase::Bound;
                self.start_collector(deadline);
                b"admission-accepted\n".to_vec()
            }
            Ok(_) => {
                self.phase = Phase::Closed;
                b"admission-unavailable\n".to_vec()
            }
            Err(_) => b"admission-unavailable\n".to_vec(),
        }
    }
    fn empty_metrics() -> ovrcr_protocol::MetricsSample {
        use ovrcr_protocol::*;
        fn uncertain<T>(value: T, source: &str) -> Measurement<T> {
            Measurement {
                value,
                source: source.into(),
                source_revision: None,
                source_sequence: None,
                freshness: MeasurementFreshness::Uncertain,
            }
        }
        MetricsSample {
            model: None,
            context: uncertain(
                ContextSample {
                    used_tokens: None,
                    capacity_tokens: None,
                    quality: SampleQuality::Observed,
                },
                "claude_statusline",
            ),
            cost: uncertain(None, "claude_statusline"),
            usage: uncertain(
                UsageTotals {
                    scope: UsageScope::Conversation,
                    coverage: UsageCoverage::Partial,
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    reasoning_output_tokens: None,
                },
                "claude_root_transcript",
            ),
        }
    }
    fn disconnect(&mut self, deadline: Instant) {
        self.phase = Phase::Closed;
        if let Some(mut collector) = self.collector.take() {
            let _ = collector.cancel(deadline);
        }
        if let Some(lease) = self.lease.take() {
            let _ = lease.stream.shutdown(std::net::Shutdown::Both);
            drop(lease);
        }
    }
    fn source_health(&mut self, reason: Option<String>, deadline: Instant) {
        if !matches!(self.phase, Phase::Bound) || self.source_health == reason {
            return;
        }
        self.source_health = reason.clone();
        let Some(lease) = &mut self.lease else {
            return;
        };
        let Some(binding) = lease.binding.clone() else {
            return;
        };
        self.health_revision += 1;
        let result = ovrcr_runtime::agent_runner::private_identifier()
            .map_err(anyhow::Error::from)
            .and_then(|operation| {
                lease.command(
                    operation,
                    AgentCommand::Health(ProviderReport {
                        binding,
                        revision: self.health_revision,
                        observation: AgentObservation::Health(HealthSample {
                            state: if reason.is_some() {
                                ReporterHealth::Unavailable
                            } else {
                                ReporterHealth::Connected
                            },
                            reason,
                        }),
                    }),
                    deadline,
                )
            });
        if !matches!(
            result,
            Ok(Response::AgentOperation(
                AgentOperationResult::HealthUpdated
            ))
        ) {
            self.disconnect(deadline);
        }
    }
    fn start_collector(&mut self, deadline: Instant) {
        let Some(path) = self.transcript_path.as_ref() else {
            return;
        };
        let Some(conversation) = self.expected.clone() else {
            return;
        };
        let result = std::env::current_exe()
            .map_err(anyhow::Error::from)
            .and_then(|executable| {
                super::collector::CollectorController::spawn(
                    &executable,
                    super::collector::CollectorSource {
                        path: path.into(),
                        conversation,
                    },
                )
            });
        match result {
            Ok(collector) => self.collector = Some(collector),
            Err(_) => self.source_health(Some("collector_unavailable".into()), deadline),
        }
    }
    fn publish_metrics(&mut self, next: ovrcr_protocol::MetricsSample, deadline: Instant) {
        if !matches!(self.phase, Phase::Bound) || self.metrics.as_ref() == Some(&next) {
            return;
        }
        let Some(lease) = &self.lease else {
            return;
        };
        let Some(binding) = lease.binding.clone() else {
            return;
        };
        self.metrics_revision += 1;
        let result = lease.publish_observation(
            ProviderReport {
                binding,
                revision: self.metrics_revision,
                observation: AgentObservation::Metrics(Box::new(next.clone())),
            },
            deadline,
        );
        if result.is_ok() {
            self.metrics = Some(next);
        } else {
            self.disconnect(deadline);
        }
    }
    fn statusline(&mut self, input: &[u8], deadline: Instant) {
        if !matches!(self.phase, Phase::Bound) {
            return;
        }
        let Ok(sample) = super::claude_metrics::parse_claude_metrics(input) else {
            return;
        };
        if self.expected.as_ref() != Some(&sample.conversation) {
            return;
        }
        if let Some(cost) = &sample.cost.value {
            if self.cost_watermark.is_some_and(|old| cost.usd_ticks < old) {
                return;
            }
            self.cost_watermark = Some(cost.usd_ticks);
        }
        let mut next = self.metrics.clone().unwrap_or_else(Self::empty_metrics);
        next.model = sample.model;
        next.context = sample.context;
        next.cost = sample.cost;
        self.publish_metrics(next, deadline);
    }
    fn poll(&mut self, deadline: Instant) {
        if !matches!(self.phase, Phase::Bound) {
            return;
        }
        let Some(collector) = &mut self.collector else {
            return;
        };
        let result = collector.advance();
        match result {
            Ok(Some(snapshot)) => {
                self.collector_caught_up = snapshot.caught_up;
                let mut next = self.metrics.clone().unwrap_or_else(Self::empty_metrics);
                next.usage.value = snapshot.usage;
                self.publish_metrics(next, deadline);
                self.source_health(snapshot.diagnostic, deadline);
            }
            Ok(None) => {}
            Err(_) => {
                if let Some(mut collector) = self.collector.take() {
                    let _ = collector.cancel(deadline);
                }
                self.source_health(Some("collector_unavailable".into()), deadline);
            }
        }
    }
    fn native_completed(&mut self, deadline: Instant) {
        // Reserve 700ms for atomic finalization and original-operation receipt recovery.
        let drain_deadline = deadline
            .checked_sub(Duration::from_millis(700))
            .unwrap_or(deadline);
        self.collector_caught_up = false;
        while self.collector.is_some()
            && !self.collector_caught_up
            && Instant::now() < drain_deadline
        {
            self.poll(
                Instant::now()
                    .checked_add(Duration::from_millis(100))
                    .unwrap()
                    .min(drain_deadline),
            );
            if !self.collector_caught_up {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        if let Some(mut collector) = self.collector.take() {
            let _ = collector.cancel(drain_deadline);
        }
        let Some(mut lease) = self.lease.take() else {
            return;
        };
        if matches!(self.phase, Phase::Bound)
            && let Some(binding) = lease.binding.clone()
        {
            self.metrics_revision += 1;
            let mut final_metrics = self.metrics.clone().unwrap_or_else(Self::empty_metrics);
            final_metrics.usage.value.coverage = ovrcr_protocol::UsageCoverage::Partial;
            if let Ok(operation) = ovrcr_runtime::agent_runner::private_identifier() {
                let first_deadline = (Instant::now() + Duration::from_millis(300)).min(deadline);
                let result = lease.command(
                    operation.clone(),
                    AgentCommand::Finalize {
                        binding,
                        revision: self.metrics_revision,
                        final_metrics: Box::new(final_metrics),
                    },
                    first_deadline,
                );
                if !matches!(
                    result,
                    Ok(Response::AgentOperation(AgentOperationResult::Released))
                ) && Instant::now() < deadline
                {
                    let _ = lease.final_status(operation, deadline);
                }
            }
        } else if let Ok(operation) = ovrcr_runtime::agent_runner::private_identifier() {
            let _ = lease.command(
                operation,
                AgentCommand::Release {
                    expected_binding: lease.binding.clone(),
                },
                deadline,
            );
        }
        self.phase = Phase::Closed;
        // Ack uncertainty falls back to watched disconnection, never a new grace period.
        let _ = lease.stream.shutdown(std::net::Shutdown::Both);
        drop(lease);
    }
    fn freeze(&mut self, deadline: Instant) {
        if matches!(self.phase, Phase::Closed) {
            return;
        }
        if let Some(mut collector) = self.collector.take() {
            let _ = collector.cancel(deadline);
        }
        self.health_revision += 1;
        let pending = match &self.phase {
            Phase::Binding { operation } => Some(operation.clone()),
            _ => None,
        };
        self.phase = Phase::Closed;
        let Some(lease) = &mut self.lease else {
            return;
        };
        let closed = (|| -> anyhow::Result<()> {
            if let Some(operation) = pending {
                match lease.operation_status(operation, deadline)? {
                    Response::AgentOperation(AgentOperationResult::Bound(binding)) => {
                        lease.binding = Some(binding)
                    }
                    _ => anyhow::bail!("binding status unavailable during closure"),
                }
            }
            let Some(binding) = lease.binding.clone() else {
                return Ok(());
            };
            let operation = ovrcr_runtime::agent_runner::private_identifier()?;
            let response = lease.command(
                operation,
                AgentCommand::Health(ProviderReport {
                    binding,
                    revision: self.health_revision,
                    observation: AgentObservation::Health(HealthSample {
                        state: ReporterHealth::Unavailable,
                        reason: Some("identity_transition_unavailable".into()),
                    }),
                }),
                deadline,
            )?;
            if response != Response::AgentOperation(AgentOperationResult::HealthUpdated) {
                anyhow::bail!("unavailable health was not acknowledged");
            }
            Ok(())
        })();
        if closed.is_err()
            && let Some(lease) = self.lease.take()
        {
            // Do not let Drop's graceful release wait beyond this exhausted callback.
            // Generation-safe watch loss makes the runtime unavailable on delivery failure.
            let _ = lease.stream.shutdown(std::net::Shutdown::Both);
            drop(lease);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }
    #[test]
    fn initial_admission_argv_grammar_is_conservative_and_value_aware() {
        for arguments in [
            vec!["claude"],
            vec!["claude", "--model=sonnet", "prompt mentioning --resume"],
            vec![
                "claude",
                "--setting-sources",
                "",
                "--settings",
                "path with spaces",
                "--strict-mcp-config",
                "--agents",
                "{}",
                "--agent",
                "fixture-root",
            ],
            vec!["claude", "--", "doctor"],
            vec!["claude", "-n", "name"],
        ] {
            assert!(eligible_argv(&args(&arguments)), "{arguments:?}");
        }
        for option in [
            "--session-id=x",
            "-c",
            "--continue",
            "-r",
            "--resume=x",
            "--fork-session",
            "--from-pr",
            "--teleport",
            "--bg",
            "--background",
            "--cloud",
            "--environment=x",
            "--tmux",
            "--remote",
            "--exec",
            "-p",
            "--print",
            "--bare",
            "--safe-mode",
            "--init-only",
            "--init",
            "--maintenance",
            "--help",
            "-h",
            "--version",
            "-v",
            "--unknown-mode",
            "--worktree",
            "--add-dir",
        ] {
            assert!(!eligible_argv(&args(&["claude", option])), "{option}");
        }
        for arguments in [
            vec!["claude", "doctor"],
            vec!["claude", "agents"],
            vec!["claude", "--model"],
            vec!["claude", "--model", "--resume"],
            vec!["claude", "--strict-mcp-config=true"],
            vec!["other", "--agent", "root"],
            vec!["claude", "two", "prompts"],
        ] {
            assert!(!eligible_argv(&args(&arguments)), "{arguments:?}");
        }
    }

    #[test]
    fn initial_admission_lost_bind_reply_uses_original_operation_status() {
        use ovrcr_protocol::{
            AgentBinding, AgentProvider, AgentSecret, ClientMessage, Request, ServerMessage,
            SupervisorAuth, exchange_preamble, read_frame, write_frame,
        };
        use std::os::unix::net::{UnixListener, UnixStream};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("status.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (client, mut first) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        let auth = SupervisorAuth {
            session: ovrcr_protocol::SessionId(1),
            lease: AgentSecret([7; 32]),
        };
        let binding = AgentBinding {
            provider: AgentProvider::Claude,
            invocation: "invocation".into(),
            conversation: "expected".into(),
            generation: 1,
        };
        let returned = binding.clone();
        let expected_auth = auth.clone();
        let server = std::thread::spawn(move || {
            let message = read_frame::<ClientMessage>(&mut first).unwrap();
            let Request::Supervisor(bind) = message.request else {
                panic!("expected Bind");
            };
            assert!(matches!(
                bind.command,
                AgentCommand::Bind {
                    expected_binding: None,
                    ..
                }
            ));
            // Keep the allocation connection alive while deliberately withholding its reply.
            let (mut reattached, _) = listener.accept().unwrap();
            exchange_preamble(&mut reattached).unwrap();
            let hello = read_frame::<ClientMessage>(&mut reattached).unwrap();
            assert_eq!(hello.request, Request::SupervisorHello(expected_auth));
            write_frame(
                &mut reattached,
                &ServerMessage::Response {
                    request_id: hello.request_id,
                    response: Response::Ok,
                },
            )
            .unwrap();
            let status = read_frame::<ClientMessage>(&mut reattached).unwrap();
            let Request::AgentStatus { operation, .. } = status.request else {
                panic!("retry must request status, not bind again");
            };
            assert_eq!(operation, bind.operation);
            write_frame(
                &mut reattached,
                &ServerMessage::Response {
                    request_id: status.request_id,
                    response: Response::AgentOperation(AgentOperationResult::Bound(returned)),
                },
            )
            .unwrap();
        });
        let lease = InvocationLease {
            stream: client,
            auth,
            socket: path,
            binding: None,
            next_request: 2,
            capability: [0; 32],
        };
        let mut receiver = Receiver {
            lease: Some(lease),
            expected: Some("expected".into()),
            phase: Phase::Waiting,
            prompt: None,
            activity_revision: 0,
            metrics: None,
            metrics_revision: 0,
            health_revision: 0,
            source_health: None,
            transcript_path: None,
            collector: None,
            cost_watermark: None,
            collector_caught_up: false,
        };
        let input=br#"{"provider":"claude","origin":"claude-hook","payload":{"hook_event_name":"SessionStart","source":"startup","session_id":"expected"}}"#;
        assert_eq!(
            receiver.handle(input, Instant::now() + Duration::from_millis(30)),
            b"admission-unavailable\n"
        );
        assert!(matches!(receiver.phase, Phase::Binding { .. }));
        assert_eq!(
            receiver.handle(input, Instant::now() + Duration::from_secs(1)),
            b"admission-accepted\n"
        );
        assert_eq!(
            receiver.lease.as_ref().unwrap().binding.as_ref(),
            Some(&binding)
        );
        assert_eq!(
            receiver.handle(input, Instant::now() + Duration::from_secs(1)),
            b"admission-ignored\n"
        );
        server.join().unwrap();
    }

    #[test]
    fn initial_admission_probe_cleans_descendants_after_leader_exit() {
        use std::os::unix::fs::PermissionsExt;
        for exit in [0, 1] {
            let root = tempfile::tempdir().unwrap();
            let executable = root.path().join("probe");
            let identity = root.path().join("identity");
            std::fs::write(&executable,format!("#!/bin/sh\nsleep 60 </dev/null >/dev/null 2>&1 &\nprintf '%s %s' \"$$\" \"$!\" > '{}'\nprintf '2.1.267 (Claude Code)\\n'\nexit {exit}\n",identity.display())).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(pinned_version(executable.as_os_str()), exit == 0);
            let ids = std::fs::read_to_string(&identity).unwrap();
            let ids: Vec<libc::pid_t> =
                ids.split_whitespace().map(|s| s.parse().unwrap()).collect();
            let deadline = Instant::now() + Duration::from_millis(200);
            while unsafe { libc::kill(-ids[0], 0) } == 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            let leaked = unsafe { libc::kill(-ids[0], 0) } == 0;
            if leaked {
                assert_eq!(
                    unsafe { libc::getpgid(ids[1]) },
                    ids[0],
                    "owned fixture descendant moved"
                );
                unsafe {
                    libc::kill(-ids[0], libc::SIGKILL);
                }
            }
            assert!(
                !leaked,
                "version probe left its descendant after leader exit {exit}"
            );
        }
    }
}

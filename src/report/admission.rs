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
    let launch = if lease.is_some()
        && eligible(argv, ClaudeVersion::V2_1_268)
        && let Some(version) = pinned_version(&argv[0]).supported()
    {
        eligible_launch(argv, version).and_then(|launch| match launch {
            EligibleLaunch::Fresh => fresh_uuid()
                .ok()
                .map(|conversation| (conversation, InitialSource::Startup)),
            EligibleLaunch::Resume(conversation) => Some((conversation, InitialSource::Resume)),
        })
    } else {
        None
    };
    if let Some((expected, InitialSource::Startup)) = &launch {
        argv.splice(
            1..1,
            [OsString::from("--session-id"), OsString::from(expected)],
        );
        eprintln!("agent awaiting certified startup");
    } else if matches!(launch.as_ref(), Some((_, InitialSource::Resume))) {
        eprintln!("agent awaiting certified resume");
    } else if lease.is_some() {
        eprintln!("agent admission unavailable; running native command");
    } else {
        eprintln!("agent reporting unavailable; running native command");
    }
    let mut receiver = Receiver {
        lease,
        expected: launch
            .as_ref()
            .map(|(conversation, _)| conversation.clone()),
        initial_source: launch.map(|(_, source)| source),
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

fn eligible(argv: &[OsString], version: ClaudeVersion) -> bool {
    if unsafe { libc::isatty(0) } != 1 || unsafe { libc::isatty(1) } != 1 {
        return false;
    }
    eligible_argv(argv, version)
}
fn eligible_argv(argv: &[OsString], version: ClaudeVersion) -> bool {
    eligible_launch(argv, version).is_some()
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum EligibleLaunch {
    Fresh,
    Resume(String),
}
fn eligible_launch(argv: &[OsString], version: ClaudeVersion) -> Option<EligibleLaunch> {
    if argv.first().and_then(|arg| Path::new(arg).file_name()) != Some(OsStr::new("claude")) {
        return None;
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
    let mut resume = None;
    while index < argv.len() {
        let arg = argv[index].to_str()?;
        if arg == "--" {
            return (resume.is_none() && !prompt && argv.len() == index + 2)
                .then_some(EligibleLaunch::Fresh);
        }
        if arg.starts_with('-') {
            let (name, value) = arg
                .split_once('=')
                .map_or((arg, None), |(name, value)| (name, Some(value)));
            if name == "--resume" || (name == "-r" && version == ClaudeVersion::V2_1_268) {
                if value.is_some() || resume.is_some() {
                    return None;
                }
                index += 1;
                let value = argv.get(index).and_then(|value| value.to_str())?;
                if !canonical_uuid_v4(value) {
                    return None;
                }
                resume = Some(value.to_owned());
            } else if values.contains(&name) {
                if value.is_none() {
                    index += 1;
                    if index >= argv.len() || argv[index].as_encoded_bytes().starts_with(b"-") {
                        return None;
                    }
                }
            } else if !flags.contains(&name) || value.is_some() {
                return None;
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
                return None;
            }
            prompt = true;
        }
        index += 1;
    }
    match (resume, prompt) {
        (Some(conversation), false) => Some(EligibleLaunch::Resume(conversation)),
        (None, _) => Some(EligibleLaunch::Fresh),
        (Some(_), true) => None,
    }
}
fn canonical_uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        && bytes.iter().enumerate().all(|(index, byte)| {
            [8, 13, 18, 23].contains(&index)
                || byte.is_ascii_digit()
                || (b'a'..=b'f').contains(byte)
        })
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
}
pub const SUPPORTED_CLAUDE_VERSIONS: &[&str] = &["2.1.267", "2.1.268"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaudeVersion {
    V2_1_267,
    V2_1_268,
}

impl ClaudeVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V2_1_267 => "2.1.267",
            Self::V2_1_268 => "2.1.268",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClaudeVersionProbe {
    Supported(ClaudeVersion),
    Unsupported(String),
    Unavailable,
}

impl ClaudeVersionProbe {
    pub fn supported(&self) -> Option<ClaudeVersion> {
        match self {
            Self::Supported(version) => Some(*version),
            Self::Unsupported(_) | Self::Unavailable => None,
        }
    }

    pub fn observed(&self) -> Option<&str> {
        match self {
            Self::Supported(version) => Some(version.as_str()),
            Self::Unsupported(version) => Some(version),
            Self::Unavailable => None,
        }
    }
}

fn classify_version(bytes: &[u8]) -> ClaudeVersionProbe {
    match bytes {
        b"2.1.267 (Claude Code)\n" => ClaudeVersionProbe::Supported(ClaudeVersion::V2_1_267),
        b"2.1.268 (Claude Code)\n" => ClaudeVersionProbe::Supported(ClaudeVersion::V2_1_268),
        _ => std::str::from_utf8(bytes)
            .ok()
            .and_then(|output| output.strip_suffix(" (Claude Code)\n"))
            .filter(|version| {
                !version.is_empty()
                    && version
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || byte == b'.')
            })
            .map_or(ClaudeVersionProbe::Unavailable, |version| {
                ClaudeVersionProbe::Unsupported(version.to_owned())
            }),
    }
}

pub fn pinned_version(executable: &OsStr) -> ClaudeVersionProbe {
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
        return ClaudeVersionProbe::Unavailable;
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
                return if status.is_ok_and(|status| status.success()) {
                    classify_version(&bytes)
                } else {
                    ClaudeVersionProbe::Unavailable
                };
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
    ClaudeVersionProbe::Unavailable
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InitialSource {
    Startup,
    Resume,
}
impl InitialSource {
    fn matches(self, source: &str) -> bool {
        matches!(
            (self, source),
            (Self::Startup, "startup") | (Self::Resume, "resume")
        )
    }
}
struct Receiver {
    lease: Option<InvocationLease>,
    expected: Option<String>,
    initial_source: Option<InitialSource>,
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
        let initial_start = matches!(
            &event.kind,
            ClaudeEventKind::SessionStart { source }
                if matches!(self.phase, Phase::Waiting | Phase::Binding { .. })
                    && self.initial_source.is_some_and(|expected| expected.matches(source))
                    && event.session == expected
                    && (self.initial_source != Some(InitialSource::Resume)
                        || event.transcript_path.is_some())
        );
        let contradictory_initial_source = matches!(
            &event.kind,
            ClaudeEventKind::SessionStart { source }
                if matches!(self.phase, Phase::Waiting | Phase::Binding { .. })
                    && self.initial_source == Some(InitialSource::Resume)
                    && event.session == expected
                    && source != "resume"
        );
        let transition = matches!(&event.kind, ClaudeEventKind::SessionStart { source } if matches!(source.as_str(), "clear" | "resume" | "fork"))
            && !initial_start
            || contradictory_initial_source;
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
        if !initial_start {
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
    fn poll(&mut self, deadline: Instant) -> bool {
        if !matches!(self.phase, Phase::Bound) {
            return false;
        }
        let Some(collector) = &mut self.collector else {
            return false;
        };
        let result = collector.advance();
        match result {
            Ok(Some(snapshot)) => {
                self.collector_caught_up = snapshot.caught_up;
                let mut next = self.metrics.clone().unwrap_or_else(Self::empty_metrics);
                next.usage.value = snapshot.usage;
                self.publish_metrics(next, deadline);
                self.source_health(snapshot.diagnostic, deadline);
                true
            }
            Ok(None) => false,
            Err(_) => {
                if let Some(mut collector) = self.collector.take() {
                    let _ = collector.cancel(deadline);
                }
                self.source_health(Some("collector_unavailable".into()), deadline);
                false
            }
        }
    }
    fn native_completed(&mut self, deadline: Instant) {
        // Reserve 700ms for atomic finalization and original-operation receipt recovery.
        let drain_deadline = deadline
            .checked_sub(Duration::from_millis(700))
            .unwrap_or(deadline);
        self.collector_caught_up = false;
        // One request may already be in flight. Only the response after that one
        // proves a read was requested after native completion. This is an IPC
        // barrier, not a certified source revision or complete-accounting claim.
        let mut consumed_first_response = false;
        let mut post_exit_caught_up = false;
        while self.collector.is_some() && !post_exit_caught_up && Instant::now() < drain_deadline {
            let consumed = self.poll(
                Instant::now()
                    .checked_add(Duration::from_millis(100))
                    .unwrap()
                    .min(drain_deadline),
            );
            if consumed {
                post_exit_caught_up = consumed_first_response && self.collector_caught_up;
                consumed_first_response = true;
            }
            if !post_exit_caught_up {
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
    fn collector_process_loss_automatically_publishes_unavailable_health() {
        use ovrcr_protocol::{
            AgentBinding, AgentProvider, AgentSecret, ClientMessage, Request, ServerMessage,
            SupervisorAuth, read_frame, write_frame,
        };
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixStream;

        let root = tempfile::tempdir().unwrap();
        let helper = root.path().join("collector-helper");
        std::fs::write(&helper, "#!/bin/sh\n/bin/sleep 60\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let source = super::super::collector::CollectorSource {
            path: root.path().join("transcript"),
            conversation: "expected".into(),
        };
        std::fs::write(&source.path, "").unwrap();
        let collector =
            super::super::collector::CollectorController::spawn(&helper, source).unwrap();
        let collector_pid = collector.process_id().unwrap() as i32;
        let binding = AgentBinding {
            provider: AgentProvider::Claude,
            invocation: "invocation".into(),
            conversation: "expected".into(),
            generation: 1,
        };
        let (client, mut server_stream) = UnixStream::pair().unwrap();
        let expected_binding = binding.clone();
        let server = std::thread::spawn(move || {
            let message = read_frame::<ClientMessage>(&mut server_stream).unwrap();
            let Request::Supervisor(request) = message.request else {
                panic!("expected automatic health command");
            };
            let AgentCommand::Health(report) = request.command else {
                panic!("expected unavailable health report");
            };
            assert_eq!(report.binding, expected_binding);
            assert!(matches!(
                report.observation,
                AgentObservation::Health(HealthSample {
                    state: ReporterHealth::Unavailable,
                    reason: Some(ref reason),
                }) if reason == "collector_unavailable"
            ));
            write_frame(
                &mut server_stream,
                &ServerMessage::Response {
                    request_id: message.request_id,
                    response: Response::AgentOperation(AgentOperationResult::HealthUpdated),
                },
            )
            .unwrap();
        });
        let mut receiver = Receiver {
            lease: Some(InvocationLease {
                stream: client,
                auth: SupervisorAuth {
                    session: ovrcr_protocol::SessionId(1),
                    lease: AgentSecret([7; 32]),
                },
                socket: root.path().join("unused.sock"),
                binding: Some(binding.clone()),
                next_request: 1,
                capability: [0; 32],
            }),
            expected: Some("expected".into()),
            initial_source: Some(InitialSource::Startup),
            phase: Phase::Bound,
            prompt: None,
            activity_revision: 0,
            metrics: None,
            metrics_revision: 0,
            health_revision: 0,
            source_health: None,
            transcript_path: None,
            collector: Some(collector),
            cost_watermark: None,
            collector_caught_up: false,
        };
        assert_eq!(unsafe { libc::kill(-collector_pid, libc::SIGKILL) }, 0);
        let deadline = Instant::now() + Duration::from_secs(2);
        while receiver.source_health.is_none() && Instant::now() < deadline {
            receiver.poll(deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            receiver.source_health.as_deref(),
            Some("collector_unavailable")
        );
        assert_eq!(
            receiver.lease.as_ref().unwrap().binding.as_ref(),
            Some(&binding)
        );
        server.join().unwrap();
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
            assert!(
                eligible_argv(&args(&arguments), ClaudeVersion::V2_1_267),
                "{arguments:?}"
            );
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
            assert!(
                !eligible_argv(&args(&["claude", option]), ClaudeVersion::V2_1_267),
                "{option}"
            );
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
            assert!(
                !eligible_argv(&args(&arguments), ClaudeVersion::V2_1_267),
                "{arguments:?}"
            );
        }
    }

    #[test]
    fn initial_admission_argv_accepts_only_certified_explicit_uuid_resume() {
        let uuid = "5ebc5f9b-54b5-4928-9955-dc81c23743dd";
        let resume = args(&[
            "claude",
            "--resume",
            uuid,
            "--model",
            "sonnet",
            "--strict-mcp-config",
        ]);
        for version in [ClaudeVersion::V2_1_267, ClaudeVersion::V2_1_268] {
            assert_eq!(
                eligible_launch(&resume, version),
                Some(EligibleLaunch::Resume(uuid.into()))
            );
        }
        assert_eq!(
            eligible_launch(&args(&["claude", "-r", uuid]), ClaudeVersion::V2_1_268),
            Some(EligibleLaunch::Resume(uuid.into()))
        );
        assert!(!eligible_argv(
            &args(&["claude", "-r", uuid]),
            ClaudeVersion::V2_1_267
        ));

        for arguments in [
            vec!["claude", "--resume"],
            vec!["claude", "-r"],
            vec!["claude", "-r=5ebc5f9b-54b5-4928-9955-dc81c23743dd"],
            vec!["claude", "--resume=5ebc5f9b-54b5-4928-9955-dc81c23743dd"],
            vec!["claude", "--resume", "5EBC5F9B-54B5-4928-9955-DC81C23743DD"],
            vec![
                "claude",
                "--resume",
                "{5ebc5f9b-54b5-4928-9955-dc81c23743dd}",
            ],
            vec!["claude", "--resume", "5ebc5f9b54b549289955dc81c23743dd"],
            vec!["claude", "--resume", "00000000-0000-0000-0000-000000000000"],
            vec!["claude", "--resume", "5ebc5f9b-54b5-3928-9955-dc81c23743dd"],
            vec!["claude", "--resume", "5ebc5f9b-54b5-4928-7955-dc81c23743dd"],
            vec!["claude", "--resume", uuid, "--resume", uuid],
            vec!["claude", "--resume", uuid, "-r", uuid],
            vec!["claude", "-r", "5EBC5F9B-54B5-4928-9955-DC81C23743DD"],
            vec!["claude", "-r", uuid, "prompt"],
            vec!["claude", "--resume", uuid, "prompt"],
            vec!["claude", "--resume", uuid, "--continue"],
            vec!["claude", "--resume", uuid, "--fork-session", uuid],
            vec!["claude", "--resume", uuid, "--session-id", uuid],
            vec!["claude", "--resume", uuid, "--background"],
            vec!["claude", "--resume", uuid, "--print"],
        ] {
            assert!(
                !eligible_argv(&args(&arguments), ClaudeVersion::V2_1_268),
                "{arguments:?}"
            );
        }
    }

    #[test]
    fn initial_resume_rejects_wrong_child_missing_source_and_end_without_binding() {
        fn waiting_resume() -> Receiver {
            Receiver {
                lease: None,
                expected: Some("5ebc5f9b-54b5-4928-9955-dc81c23743dd".into()),
                initial_source: Some(InitialSource::Resume),
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
            }
        }
        let envelope = |payload| {
            serde_json::to_vec(&serde_json::json!({
                "provider":"claude",
                "origin":"claude-hook",
                "payload":payload,
            }))
            .unwrap()
        };
        let deadline = || Instant::now() + Duration::from_secs(1);
        for payload in [
            serde_json::json!({"hook_event_name":"SessionStart","session_id":"5ebc5f9b-54b5-4928-9955-dc81c23743dd","transcript_path":"/exact/root.jsonl"}),
            serde_json::json!({"hook_event_name":"SessionStart","source":"resume","session_id":"5ebc5f9b-54b5-4928-9955-dc81c23743dd","transcript_path":"/exact/root.jsonl","agent_id":"child"}),
            serde_json::json!({"hook_event_name":"SessionEnd","reason":"other","session_id":"5ebc5f9b-54b5-4928-9955-dc81c23743dd"}),
        ] {
            let mut receiver = waiting_resume();
            assert_eq!(
                receiver.handle(&envelope(payload), deadline()),
                b"admission-ignored\n"
            );
            assert!(matches!(receiver.phase, Phase::Waiting));
        }
        let mut wrong_source = waiting_resume();
        assert_eq!(
            wrong_source.handle(
                &envelope(serde_json::json!({"hook_event_name":"SessionStart","source":"startup","session_id":"5ebc5f9b-54b5-4928-9955-dc81c23743dd","transcript_path":"/exact/root.jsonl"})),
                deadline(),
            ),
            b"admission-ignored\n"
        );
        assert!(matches!(wrong_source.phase, Phase::Closed));
        assert_eq!(
            wrong_source.handle(
                &envelope(serde_json::json!({"hook_event_name":"SessionStart","source":"resume","session_id":"5ebc5f9b-54b5-4928-9955-dc81c23743dd","transcript_path":"/exact/root.jsonl"})),
                deadline(),
            ),
            b"admission-ignored\n"
        );
        assert!(matches!(wrong_source.phase, Phase::Closed));
        for payload in [
            serde_json::json!({"hook_event_name":"SessionStart","source":"resume","session_id":"wrong-conversation","transcript_path":"/exact/root.jsonl"}),
            serde_json::json!({"hook_event_name":"SessionStart","source":"resume","session_id":"5ebc5f9b-54b5-4928-9955-dc81c23743dd"}),
        ] {
            let mut receiver = waiting_resume();
            assert_eq!(
                receiver.handle(&envelope(payload), deadline()),
                b"admission-ignored\n"
            );
            assert!(matches!(receiver.phase, Phase::Closed));
        }
    }

    #[test]
    fn initial_admission_lost_bind_reply_uses_original_operation_status() {
        assert_bind_reply_status(InitialSource::Startup, true);
    }

    #[test]
    fn initial_resume_lost_bind_reply_uses_original_operation_status() {
        assert_bind_reply_status(InitialSource::Resume, true);
    }

    #[test]
    fn initial_resume_failed_bind_status_closes_without_rebinding() {
        assert_bind_reply_status(InitialSource::Resume, false);
    }

    fn assert_bind_reply_status(initial_source: InitialSource, recover: bool) {
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
                    response: if recover {
                        Response::AgentOperation(AgentOperationResult::Bound(returned))
                    } else {
                        Response::Error {
                            code: ovrcr_protocol::ErrorCode::Conflict,
                            message: "fixture rejected binding status".into(),
                        }
                    },
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
            initial_source: Some(initial_source),
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
        let transcript = root.path().join("resume.jsonl");
        std::fs::write(&transcript, "").unwrap();
        let input = serde_json::to_vec(&serde_json::json!({
            "provider":"claude",
            "origin":"claude-hook",
            "payload":{
                "hook_event_name":"SessionStart",
                "source":match initial_source {
                    InitialSource::Startup => "startup",
                    InitialSource::Resume => "resume",
                },
                "session_id":"expected",
                "transcript_path":transcript,
            }
        }))
        .unwrap();
        assert_eq!(
            receiver.handle(&input, Instant::now() + Duration::from_millis(30)),
            b"admission-unavailable\n"
        );
        assert!(matches!(receiver.phase, Phase::Binding { .. }));
        assert_eq!(
            receiver.handle(&input, Instant::now() + Duration::from_secs(1)),
            if recover {
                b"admission-accepted\n".as_slice()
            } else {
                b"admission-unavailable\n".as_slice()
            }
        );
        if recover {
            assert_eq!(
                receiver.lease.as_ref().unwrap().binding.as_ref(),
                Some(&binding)
            );
        } else {
            assert!(matches!(receiver.phase, Phase::Closed));
        }
        if recover && initial_source == InitialSource::Startup {
            assert_eq!(
                receiver.handle(&input, Instant::now() + Duration::from_secs(1)),
                b"admission-ignored\n"
            );
        }
        server.join().unwrap();
    }

    #[test]
    fn native_completion_drains_past_pending_pre_exit_eof() {
        use ovrcr_protocol::{
            AgentBinding, AgentProvider, AgentSecret, ClientMessage, Request, ServerMessage,
            SupervisorAuth, exchange_preamble, read_frame, write_frame,
        };
        use std::os::unix::{
            fs::PermissionsExt,
            net::{UnixListener, UnixStream},
        };
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("report.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (client, mut supervisor) = UnixStream::pair().unwrap();
        client.set_nonblocking(true).unwrap();
        supervisor
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let reports_done = done.clone();
        let reports = std::thread::spawn(move || {
            while !reports_done.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        exchange_preamble(&mut stream).unwrap();
                        let message = read_frame::<ClientMessage>(&mut stream).unwrap();
                        assert!(matches!(message.request, Request::AgentReport { .. }));
                        write_frame(
                            &mut stream,
                            &ServerMessage::Response {
                                request_id: message.request_id,
                                response: Response::Ok,
                            },
                        )
                        .unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        });
        let finalizer = std::thread::spawn(move || {
            let message = read_frame::<ClientMessage>(&mut supervisor).unwrap();
            let Request::Supervisor(request) = message.request else {
                panic!("expected finalize")
            };
            let AgentCommand::Finalize { final_metrics, .. } = request.command else {
                panic!("expected finalize")
            };
            write_frame(
                &mut supervisor,
                &ServerMessage::Response {
                    request_id: message.request_id,
                    response: Response::AgentOperation(AgentOperationResult::Released),
                },
            )
            .unwrap();
            done.store(true, Ordering::Release);
            final_metrics
        });
        let source = super::super::collector::CollectorSource {
            path: root.path().join("transcript"),
            conversation: "expected".into(),
        };
        let start = serde_json::to_vec(&serde_json::json!({"Start": &source})).unwrap();
        for (name, tokens) in [("a", 10), ("b", 30)] {
            let mut usage = Receiver::empty_metrics().usage.value;
            usage.input_tokens = Some(tokens);
            let snapshot = super::super::collector::CollectorSnapshot {
                usage,
                source_revision: None,
                diagnostic: None,
                caught_up: true,
                rebuilding: false,
                retained_identities: 1,
                retained_bytes: 100,
            };
            let payload = serde_json::to_vec(&snapshot).unwrap();
            let mut frame = (payload.len() as u32).to_be_bytes().to_vec();
            frame.extend(payload);
            std::fs::write(root.path().join(name), frame).unwrap();
        }
        let executable = root.path().join("helper");
        // The marker is written only after A is flushed to the real helper pipe.
        // The helper cannot send B until the controller issues its next Read.
        std::fs::write(&executable, format!(
            "#!/bin/sh\ncd '{}'\n/bin/dd bs=1 count={} 2>/dev/null >/dev/null\n/bin/cat a\n/usr/bin/touch ready\n/bin/dd bs=1 count=1 2>/dev/null >/dev/null\n/bin/cat transcript\n/bin/sleep 60\n",
            root.path().display(), start.len() + 4,
        )).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::copy(root.path().join("a"), &source.path).unwrap();
        let collector =
            super::super::collector::CollectorController::spawn(&executable, source).unwrap();
        let ready_deadline = Instant::now() + Duration::from_secs(2);
        while !root.path().join("ready").exists() && Instant::now() < ready_deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            root.path().join("ready").exists(),
            "pre-exit EOF was not queued"
        );
        let binding = AgentBinding {
            provider: AgentProvider::Claude,
            invocation: "invocation".into(),
            conversation: "expected".into(),
            generation: 1,
        };
        let mut receiver = Receiver {
            lease: Some(InvocationLease {
                stream: client,
                auth: SupervisorAuth {
                    session: ovrcr_protocol::SessionId(1),
                    lease: AgentSecret([7; 32]),
                },
                socket: path,
                binding: Some(binding),
                next_request: 2,
                capability: [0; 32],
            }),
            expected: Some("expected".into()),
            initial_source: Some(InitialSource::Startup),
            phase: Phase::Bound,
            prompt: None,
            activity_revision: 0,
            metrics: None,
            metrics_revision: 0,
            health_revision: 0,
            source_health: None,
            transcript_path: None,
            collector: Some(collector),
            cost_watermark: None,
            collector_caught_up: false,
        };
        let argv = args(&[
            "/bin/sh",
            "-c",
            &format!(
                "/bin/cp '{0}/b' '{0}/transcript'; printf 'POST_EXIT_DRAIN_NATIVE_OUTPUT\\n'; exit 17",
                root.path().display(),
            ),
        ]);
        let started = Instant::now();
        let status = ovrcr_runtime::agent_runner::run_native(&argv, move |ready, _| {
            assert!(ready);
            Some(Box::new(move |event| {
                // Keep the deliberately queued pre-exit response pending until the real
                // native-completion callback, reproducing the disputed ordering exactly.
                if let ovrcr_runtime::agent_runner::HookEvent::NativeCompleted { deadline } = event
                {
                    receiver.native_completed(deadline);
                }
                Vec::new()
            }))
        })
        .unwrap();
        assert_eq!(status.code(), Some(17));
        assert!(started.elapsed() < Duration::from_millis(2500));
        let final_metrics = finalizer.join().unwrap();
        reports.join().unwrap();
        assert_eq!(
            final_metrics.usage.value.input_tokens,
            Some(30),
            "finalization consumed only the queued pre-exit EOF"
        );
        assert_eq!(
            final_metrics.usage.value.coverage,
            ovrcr_protocol::UsageCoverage::Partial
        );
    }

    #[test]
    fn initial_admission_probe_cleans_descendants_after_leader_exit() {
        use std::os::unix::fs::PermissionsExt;
        for (version, exit, expected) in [
            ("2.1.267", 0, true),
            ("2.1.268", 0, true),
            ("2.1.266", 0, false),
            ("2.1.269", 0, false),
            ("2.1.268", 1, false),
        ] {
            let root = tempfile::tempdir().unwrap();
            let executable = root.path().join("probe");
            let identity = root.path().join("identity");
            std::fs::write(&executable,format!("#!/bin/sh\nsleep 60 </dev/null >/dev/null 2>&1 &\nprintf '%s %s' \"$$\" \"$!\" > '{}'\nprintf '{version} (Claude Code)\\n'\nexit {exit}\n",identity.display())).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let probe = pinned_version(executable.as_os_str());
            assert_eq!(probe.supported().is_some(), expected);
            assert_eq!(
                probe.observed(),
                (exit == 0).then_some(version),
                "version diagnostic"
            );
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

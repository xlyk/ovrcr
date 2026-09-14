//! Initial-only Claude admission. Provider interpretation stays outside runtime.
use super::InvocationLease;
use super::claude::{ClaudeEventKind, parse_claude_hook};
use super::reporter::{self, Reporter};
use ovrcr_protocol::AgentProvider;
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
    let reserved = lease.is_some();
    let launch = if reporter::preflight(&lease, argv, |argv| {
        eligible_argv(argv, ClaudeVersion::V2_1_268)
    })
    .is_none()
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
    } else if reserved {
        eprintln!("agent admission unavailable; running native command");
    } else {
        eprintln!("agent reporting unavailable; running native command");
    }
    Reporter::new(AgentProvider::Claude, lease, None).handler(Hooks {
        expected: launch
            .as_ref()
            .map(|(conversation, _)| conversation.clone()),
        initial_source: launch.map(|(_, source)| source),
        announced: false,
        prompt: None,
        metrics: None,
        transcript_path: None,
        collector: None,
        cost_watermark: None,
        collector_caught_up: false,
    })
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
    probe_version(executable)
        .as_deref()
        .map_or(ClaudeVersionProbe::Unavailable, classify_version)
}

/// Bounded native --version probe, shared by launch admission and diagnostics.
pub fn probe_version(executable: &OsStr) -> Option<Vec<u8>> {
    let mut command = Command::new(executable);
    command.arg("--version");
    probe_command(command)
}

fn probe_command(mut command: Command) -> Option<Vec<u8>> {
    command
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
        return None;
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
                    Some(bytes)
                } else {
                    None
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
    None
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

/// Claude's hooks and statusline. The conversation this invocation certified, the turn
/// identity its prompts establish, and the transcript reader are this adapter's; the
/// binding, the revisions and the teardown are the reporter's.
struct Hooks {
    expected: Option<String>,
    initial_source: Option<InitialSource>,
    /// Whether the certified announcement this invocation waits for has already arrived.
    announced: bool,
    prompt: Option<String>,
    metrics: Option<ovrcr_protocol::MetricsSample>,
    transcript_path: Option<String>,
    collector: Option<super::collector::CollectorController>,
    cost_watermark: Option<u64>,
    collector_caught_up: bool,
}

impl reporter::Frames for Hooks {
    /// Claude's hook helper runs inside the native command, so a frame is this receiver's
    /// by its certified conversation identity rather than by which process connected.
    fn frame(
        &mut self,
        reporter: &mut Reporter,
        input: &[u8],
        _native_root: bool,
        deadline: Instant,
    ) -> Vec<u8> {
        self.handle(reporter, input, deadline)
    }
    fn poll(&mut self, reporter: &mut Reporter, deadline: Instant) {
        self.advance(reporter, deadline);
    }
    fn finish(&mut self, reporter: &mut Reporter, deadline: Instant) {
        self.native_completed(reporter, deadline);
    }
}

impl Hooks {
    /// Bound is the state this receiver publishes from: a binding that is still live.
    fn bound(&self, reporter: &Reporter) -> bool {
        !reporter.closed() && reporter.binding().is_some()
    }
    fn handle(&mut self, reporter: &mut Reporter, input: &[u8], deadline: Instant) -> Vec<u8> {
        let Some(expected) = self.expected.clone() else {
            return reporter::UNAVAILABLE.to_vec();
        };
        let Ok(request) = serde_json::from_slice::<serde_json::Value>(input) else {
            return reporter::IGNORED.to_vec();
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
                self.statusline(reporter, raw.payload.get().as_bytes(), deadline);
            }
            return reporter::IGNORED.to_vec();
        }
        if request.get("provider").and_then(|v| v.as_str()) != Some("claude")
            || request.get("origin").and_then(|v| v.as_str()) != Some("claude-hook")
        {
            return reporter::IGNORED.to_vec();
        }
        let Some(payload) = request.get("payload") else {
            return reporter::IGNORED.to_vec();
        };
        let Ok(Some(event)) = parse_claude_hook(&serde_json::to_vec(payload).unwrap_or_default())
        else {
            return reporter::IGNORED.to_vec();
        };
        // The initial announcement is admitted only while nothing is bound yet.
        let awaiting = !reporter.closed() && reporter.binding().is_none();
        let initial_start = matches!(
            &event.kind,
            ClaudeEventKind::SessionStart { source }
                if awaiting
                    && self.initial_source.is_some_and(|expected| expected.matches(source))
                    && event.session == expected
                    && (self.initial_source != Some(InitialSource::Resume)
                        || event.transcript_path.is_some())
        );
        let contradictory_initial_source = matches!(
            &event.kind,
            ClaudeEventKind::SessionStart { source }
                if awaiting
                    && self.initial_source == Some(InitialSource::Resume)
                    && event.session == expected
                    && source != "resume"
        );
        let transition = matches!(&event.kind, ClaudeEventKind::SessionStart { source } if matches!(source.as_str(), "clear" | "resume" | "fork"))
            && !initial_start
            || contradictory_initial_source;
        if event.session != expected && !transition {
            return reporter::IGNORED.to_vec();
        }
        let clear = matches!(&event.kind, ClaudeEventKind::SessionEnd { reason } if reason.as_deref() == Some("clear"));
        if transition || clear {
            self.freeze(reporter, deadline);
            return reporter::IGNORED.to_vec();
        }
        if let Some(state) = event.kind.activity() {
            if !self.bound(reporter) {
                return reporter::IGNORED.to_vec();
            }
            let Some(prompt) = event.prompt else {
                return reporter::IGNORED.to_vec();
            };
            // Supported synchronous UserPromptSubmit hooks establish observed turn identity.
            // Opaque prompt IDs and transport revisions are not certified source ordering.
            if matches!(event.kind, ClaudeEventKind::Prompt) {
                self.prompt = Some(prompt.clone());
            } else if self.prompt.as_ref() != Some(&prompt) {
                return reporter::IGNORED.to_vec();
            }
            let published = reporter.publish(
                ovrcr_protocol::AgentObservation::Activity(ovrcr_protocol::ActivitySample {
                    state,
                    quality: ovrcr_protocol::SampleQuality::Observed,
                    turn: Some(prompt),
                }),
                deadline,
            );
            if published != reporter::ACCEPTED {
                // Delivery uncertainty cannot leave a connected reporter claiming continuity.
                self.stop_collector(deadline);
            }
            return published;
        }
        if !initial_start {
            return reporter::IGNORED.to_vec();
        }
        if !self.announced {
            self.announced = true;
            self.transcript_path = event.transcript_path.clone();
        }
        if !reporter.bind(&expected, deadline, false) {
            return reporter::UNAVAILABLE.to_vec();
        }
        self.start_collector(reporter, deadline);
        reporter::ACCEPTED.to_vec()
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
    fn stop_collector(&mut self, deadline: Instant) {
        if let Some(mut collector) = self.collector.take() {
            let _ = collector.cancel(deadline);
        }
    }
    fn source_health(&mut self, reporter: &mut Reporter, reason: Option<&str>, deadline: Instant) {
        if !self.bound(reporter) {
            return;
        }
        if !reporter.health(reason, deadline) {
            self.stop_collector(deadline);
        }
    }
    fn start_collector(&mut self, reporter: &mut Reporter, deadline: Instant) {
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
            Err(_) => self.source_health(reporter, Some("collector_unavailable"), deadline),
        }
    }
    fn publish_metrics(
        &mut self,
        reporter: &mut Reporter,
        next: ovrcr_protocol::MetricsSample,
        deadline: Instant,
    ) {
        if !self.bound(reporter) || self.metrics.as_ref() == Some(&next) {
            return;
        }
        let published = reporter.publish(
            ovrcr_protocol::AgentObservation::Metrics(Box::new(next.clone())),
            deadline,
        );
        if published == reporter::ACCEPTED {
            self.metrics = Some(next);
        } else {
            self.stop_collector(deadline);
        }
    }
    fn statusline(&mut self, reporter: &mut Reporter, input: &[u8], deadline: Instant) {
        if !self.bound(reporter) {
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
        self.publish_metrics(reporter, next, deadline);
    }
    fn advance(&mut self, reporter: &mut Reporter, deadline: Instant) -> bool {
        if !self.bound(reporter) {
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
                self.publish_metrics(reporter, next, deadline);
                self.source_health(reporter, snapshot.diagnostic.as_deref(), deadline);
                true
            }
            Ok(None) => false,
            Err(_) => {
                self.stop_collector(deadline);
                self.source_health(reporter, Some("collector_unavailable"), deadline);
                false
            }
        }
    }
    fn native_completed(&mut self, reporter: &mut Reporter, deadline: Instant) {
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
            let consumed = self.advance(
                reporter,
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
        self.stop_collector(drain_deadline);
        let mut final_metrics = self.metrics.clone().unwrap_or_else(Self::empty_metrics);
        final_metrics.usage.value.coverage = ovrcr_protocol::UsageCoverage::Partial;
        reporter.finalize(Box::new(final_metrics), deadline);
    }
    /// A conversation transition or a clear ends this invocation's reporting: the binding
    /// it certified is no longer what is on screen, and nothing replaces it here.
    fn freeze(&mut self, reporter: &mut Reporter, deadline: Instant) {
        if reporter.closed() {
            return;
        }
        self.stop_collector(deadline);
        reporter.health(Some("identity_transition_unavailable"), deadline);
        reporter.close();
    }
}

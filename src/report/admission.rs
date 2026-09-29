//! Claude admission and supported in-process conversation replacements.
//! Provider interpretation stays outside runtime.
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
    let launch = if reporter::preflight(
        reserved,
        argv,
        |argv| eligible_argv(argv, ClaudeVersion::V2_1_268),
        reporter::interactive(),
    )
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
    let recovery = launch.as_ref().and_then(|(conversation, _)| {
        let executable = if Path::new(&argv[0]).is_absolute() {
            Some(std::path::PathBuf::from(&argv[0]))
        } else {
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|directory| directory.join(&argv[0]))
                    .find(|path| path.is_file())
            })
        }?;
        Some(ovrcr_protocol::ClaudeConversation {
            conversation: conversation.clone(),
            executable,
            history: std::path::PathBuf::new(),
            config_dir: ovrcr_runtime::claude_recovery::config_dir().ok()?,
            options: ovrcr_runtime::claude_recovery::launch_options(argv).ok()?,
        })
    });
    if let Some((expected, InitialSource::Startup)) = &launch {
        argv.splice(
            1..1,
            [OsString::from("--session-id"), OsString::from(expected)],
        );
        eprintln!("agent awaiting certified startup");
    } else if matches!(launch.as_ref(), Some((_, InitialSource::Resume))) {
        eprintln!("agent awaiting certified resume");
    } else if reserved {
        eprintln!(
            "agent admission unavailable; running native command. Repair: `ovrcr agent doctor claude --json` (setup/doctor never rewrite settings or approve trust)."
        );
    } else {
        eprintln!(
            "agent reporting unavailable; running native command. Repair: `ovrcr agent doctor claude --json` (setup/doctor never rewrite settings or approve trust)."
        );
    }
    Reporter::new(AgentProvider::Claude, lease, None).handler(Hooks {
        recovery,
        expected: launch
            .as_ref()
            .map(|(conversation, _)| conversation.clone()),
        initial_source: launch.map(|(_, source)| source),
        announced: false,
        initial_accepted: false,
        pending_leave: None,
        prompt: None,
        open_requests: Vec::new(),
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
            if name == "--resume" || (name == "-r" && version != ClaudeVersion::V2_1_267) {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaudeVersion {
    V2_1_267,
    V2_1_268,
    Later([u32; 3]),
}

impl ClaudeVersion {
    pub fn as_str(self) -> String {
        match self {
            Self::V2_1_267 => "2.1.267".into(),
            Self::V2_1_268 => "2.1.268".into(),
            Self::Later([major, minor, patch]) => format!("{major}.{minor}.{patch}"),
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

    pub fn observed(&self) -> Option<String> {
        match self {
            Self::Supported(version) => Some(version.as_str()),
            Self::Unsupported(version) => Some(version.clone()),
            Self::Unavailable => None,
        }
    }
}

fn classify_version(bytes: &[u8]) -> ClaudeVersionProbe {
    let Some(version) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.strip_suffix(" (Claude Code)\n"))
    else {
        return ClaudeVersionProbe::Unavailable;
    };
    let Some(parsed) = super::versions::parse(version) else {
        return ClaudeVersionProbe::Unavailable;
    };
    if !super::versions::CLAUDE.accepts(version) {
        return ClaudeVersionProbe::Unsupported(version.into());
    }
    ClaudeVersionProbe::Supported(match parsed {
        [2, 1, 267] => ClaudeVersion::V2_1_267,
        [2, 1, 268] => ClaudeVersion::V2_1_268,
        later => ClaudeVersion::Later(later),
    })
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
pub(crate) fn fresh_uuid() -> std::io::Result<String> {
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

/// Native SessionEnd reasons that authorize a following SessionStart replacement.
/// `Clear` pairs with `SessionStart(source=clear)`. `Switch` covers observed
/// `prompt_input_exit` / `resume` ends that precede in-process resume or a
/// foreground fork (`SessionStart(source=fork)`). Background fork has no such
/// leave of the current conversation and remains unobservable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingLeave {
    Clear,
    Switch,
}

/// Claude's hooks and statusline. The conversation this invocation certified, the turn
/// identity its prompts establish, and the transcript reader are this receiver's; the
/// binding, the revisions and the teardown are the reporter's.
struct Hooks {
    recovery: Option<ovrcr_protocol::ClaudeConversation>,
    expected: Option<String>,
    initial_source: Option<InitialSource>,
    /// Whether the certified announcement this invocation waits for has already arrived.
    announced: bool,
    initial_accepted: bool,
    /// Leave of the current foreground conversation awaiting a matching SessionStart.
    pending_leave: Option<PendingLeave>,
    prompt: Option<String>,
    /// Open human-input requests for this binding, oldest first. Approvals use
    /// `approval:{prompt_id}` from a verified root `Notification(permission_prompt)`.
    open_requests: Vec<(String, ovrcr_protocol::InputKind)>,
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
        let awaiting =
            !reporter.closed() && (reporter.binding().is_none() || !self.initial_accepted);
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
        let session_end = matches!(&event.kind, ClaudeEventKind::SessionEnd { .. });
        if (transition || session_end)
            && self.initial_accepted
            && self.bound(reporter)
            && let Some(response) = self.follow_transition(reporter, &event, &expected, deadline)
        {
            return response;
        }
        let clear = matches!(&event.kind, ClaudeEventKind::SessionEnd { reason } if reason.as_deref() == Some("clear"));
        if transition || clear {
            // Unsupported / ambiguous / pre-admission transitions: retire reporting without
            // guessing a replacement identity. Claude itself keeps running.
            return if self.freeze(reporter, deadline) {
                reporter::IGNORED.to_vec()
            } else {
                reporter::UNAVAILABLE.to_vec()
            };
        }
        if session_end {
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
                // A new root turn retires any approval still open for a prior prompt_id.
                if let Some(closed) = self.close_approvals_except(reporter, None, deadline) {
                    self.stop_collector(deadline);
                    return closed;
                }
                self.prompt = Some(prompt.clone());
            } else if self.prompt.as_ref() != Some(&prompt) {
                return reporter::IGNORED.to_vec();
            }
            // Verified approval open: Notification(permission_prompt) with the current root
            // prompt_id. PermissionRequest, bare tools and child events never open a request.
            // Publish Input only — do not rewrite the underlying activity sample (Busy or
            // Ready), matching Pi/OMP so WaitingInput is effective_activity from the set.
            if matches!(event.kind, ClaudeEventKind::PermissionPrompt) {
                let opened = self.open_approval(reporter, &prompt, deadline);
                if opened != reporter::ACCEPTED && opened != reporter::IGNORED {
                    self.stop_collector(deadline);
                }
                return opened;
            }
            if let Some(closed) = self.close_approvals_except(reporter, None, deadline) {
                // Allow / deny / cancel / tool / Stop close every open approval: Claude has
                // no distinct resolve id on the notification, so the closing boundary is the
                // next attributable root activity after a genuine open.
                self.stop_collector(deadline);
                return closed;
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
        if let Some(reference) = self.recovery.as_mut()
            && let Some(history) = self.transcript_path.as_ref()
        {
            reference.history = history.into();
            if !reporter.retain_conversation(
                ovrcr_protocol::ConversationReference::Claude(reference.clone()),
                deadline,
            ) {
                return reporter::UNAVAILABLE.to_vec();
            }
        }
        self.initial_accepted = true;
        reporter::ACCEPTED.to_vec()
    }
    fn empty_metrics() -> ovrcr_protocol::MetricsSample {
        use ovrcr_protocol::*;
        fn measurement<T>(value: T, source: &str) -> Measurement<T> {
            Measurement {
                value,
                source: source.into(),
            }
        }
        MetricsSample {
            model: None,
            context: measurement(
                ContextSample {
                    used_tokens: None,
                    capacity_tokens: None,
                    quality: SampleQuality::Observed,
                },
                "claude_statusline",
            ),
            cost: measurement(None, "claude_statusline"),
            usage: measurement(
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
    /// Retire obsolete turn and request state for the conversation being left. Does not
    /// change the binding; a later supported SessionStart establishes the replacement.
    fn retire_turn_state(&mut self, reporter: &mut Reporter, deadline: Instant) -> Option<Vec<u8>> {
        self.stop_collector(deadline);
        self.prompt = None;
        self.metrics = None;
        self.cost_watermark = None;
        self.collector_caught_up = false;
        self.close_approvals_except(reporter, None, deadline)
    }

    /// Follow a native clear / resume / foreground-fork replacement when evidence names the
    /// new foreground identity. Returns `None` when the event is not an admitted form.
    fn follow_transition(
        &mut self,
        reporter: &mut Reporter,
        event: &super::claude::ClaudeEvent,
        previous: &str,
        deadline: Instant,
    ) -> Option<Vec<u8>> {
        match &event.kind {
            ClaudeEventKind::SessionEnd { reason } => {
                let leave = match reason.as_deref() {
                    Some("clear") => PendingLeave::Clear,
                    Some("prompt_input_exit") | Some("resume") => PendingLeave::Switch,
                    _ => return Some(reporter::IGNORED.to_vec()),
                };
                if event.session != previous {
                    return Some(reporter::IGNORED.to_vec());
                }
                let failed = self.retire_turn_state(reporter, deadline);
                self.pending_leave = Some(leave);
                Some(failed.unwrap_or_else(|| reporter::ACCEPTED.to_vec()))
            }
            ClaudeEventKind::SessionStart { source } => {
                let source = source.as_str();
                let admit = match source {
                    // SessionStart(source=clear) names a replacement; the preceding
                    // SessionEnd(clear) is recorded when observed but is not required to
                    // invent identity from a later callback alone.
                    "clear" if event.session != previous => true,
                    // In-process resume to another conversation, or back to this one after
                    // an observed leave. A repeated resume announcement for the current
                    // binding without a leave is not a replacement.
                    "resume" if event.session != previous || self.pending_leave.is_some() => true,
                    "resume" => return Some(reporter::IGNORED.to_vec()),
                    // Foreground /branch: SessionEnd of the current conversation then
                    // SessionStart(source=fork, new id). A bare fork SessionStart cannot
                    // be distinguished from background fork (named source gap).
                    "fork"
                        if event.session != previous
                            && self.pending_leave == Some(PendingLeave::Switch) =>
                    {
                        true
                    }
                    _ => false,
                };
                if !admit {
                    return None;
                }
                if source == "clear"
                    && self.pending_leave.is_some()
                    && self.pending_leave != Some(PendingLeave::Clear)
                {
                    return None;
                }
                Some(self.replace_foreground(
                    reporter,
                    &event.session,
                    event.transcript_path.clone(),
                    deadline,
                ))
            }
            _ => None,
        }
    }

    /// Bind the new foreground conversation under a fresh Reporting generation and start
    /// its transcript reader when the announcement names one.
    fn replace_foreground(
        &mut self,
        reporter: &mut Reporter,
        conversation: &str,
        transcript: Option<String>,
        deadline: Instant,
    ) -> Vec<u8> {
        if let Some(failed) = self.retire_turn_state(reporter, deadline) {
            return failed;
        }
        self.pending_leave = None;
        let force = self.expected.as_deref() == Some(conversation);
        self.expected = Some(conversation.to_owned());
        self.transcript_path = transcript;
        if !reporter.bind(conversation, deadline, force) {
            return reporter::UNAVAILABLE.to_vec();
        }
        self.start_collector(reporter, deadline);
        if let Some(reference) = self.recovery.as_mut() {
            reference.conversation = conversation.into();
            if let Some(history) = self.transcript_path.as_ref() {
                reference.history = history.into();
            }
            if !reference.history.as_os_str().is_empty()
                && !reporter.retain_conversation(
                    ovrcr_protocol::ConversationReference::Claude(reference.clone()),
                    deadline,
                )
            {
                return reporter::UNAVAILABLE.to_vec();
            }
        }
        reporter::ACCEPTED.to_vec()
    }

    /// An unsupported or ambiguous transition ends this invocation's reporting: the
    /// binding it certified is no longer trustworthy, and nothing replaces it here.
    fn freeze(&mut self, reporter: &mut Reporter, deadline: Instant) -> bool {
        self.pending_leave = None;
        self.stop_collector(deadline);
        let _ = self.close_approvals_except(reporter, None, deadline);
        reporter.invalidate_conversation(deadline)
    }

    fn approval_id(prompt: &str) -> String {
        format!("approval:{prompt}")
    }

    fn open_approval(
        &mut self,
        reporter: &mut Reporter,
        prompt: &str,
        deadline: Instant,
    ) -> Vec<u8> {
        let id = Self::approval_id(prompt);
        if self.open_requests.iter().any(|(open, _)| open == &id)
            || self.open_requests.len() >= ovrcr_protocol::MAX_INPUT_REQUESTS
        {
            return reporter::IGNORED.to_vec();
        }
        self.open_requests
            .push((id, ovrcr_protocol::InputKind::Approval));
        self.publish_requests(reporter, deadline)
    }

    /// Close every open approval, or all except `keep` when provided. Returns `Some` when
    /// the Input publication itself failed (caller should stop).
    fn close_approvals_except(
        &mut self,
        reporter: &mut Reporter,
        keep: Option<&str>,
        deadline: Instant,
    ) -> Option<Vec<u8>> {
        let before = self.open_requests.len();
        if let Some(keep) = keep {
            self.open_requests.retain(|(id, _)| id == keep);
        } else {
            self.open_requests.clear();
        }
        if self.open_requests.len() == before {
            return None;
        }
        let published = self.publish_requests(reporter, deadline);
        (published.as_slice() != reporter::ACCEPTED).then_some(published)
    }

    fn publish_requests(&mut self, reporter: &mut Reporter, deadline: Instant) -> Vec<u8> {
        let requests = self
            .open_requests
            .iter()
            .map(|(id, kind)| ovrcr_protocol::InputRequest {
                id: id.clone(),
                kind: *kind,
            })
            .collect();
        reporter.publish(ovrcr_protocol::AgentObservation::Input(requests), deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::reporter::Frames;
    use crate::report::reporter::scripted::{Answer, Observed, Supervisor};
    use ovrcr_protocol::ReporterHealth;

    const EXPECTED: &str = "5ebc5f9b-54b5-4928-9955-dc81c23743dd";

    fn hooks(initial_source: InitialSource) -> Hooks {
        Hooks {
            recovery: None,
            expected: Some(EXPECTED.to_owned()),
            initial_source: Some(initial_source),
            announced: false,
            initial_accepted: false,
            pending_leave: None,
            prompt: None,
            open_requests: Vec::new(),
            metrics: None,
            transcript_path: None,
            collector: None,
            cost_watermark: None,
            collector_caught_up: false,
        }
    }
    fn envelope(payload: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "provider":"claude",
            "origin":"claude-hook",
            "payload":payload,
        }))
        .unwrap()
    }
    fn start(source: &str) -> Vec<u8> {
        envelope(serde_json::json!({
            "hook_event_name":"SessionStart",
            "source":source,
            "session_id":EXPECTED,
            "transcript_path":"/exact/root.jsonl",
        }))
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }
    fn drive(hooks: &mut Hooks, reporter: &mut Reporter, input: &[u8]) -> Vec<u8> {
        hooks.frame(reporter, input, true, deadline())
    }

    #[test]
    fn initial_resume_rejects_wrong_child_missing_source_and_end_without_binding() {
        for payload in [
            serde_json::json!({"hook_event_name":"SessionStart","session_id":EXPECTED,"transcript_path":"/exact/root.jsonl"}),
            serde_json::json!({"hook_event_name":"SessionStart","source":"resume","session_id":EXPECTED,"transcript_path":"/exact/root.jsonl","agent_id":"child"}),
            serde_json::json!({"hook_event_name":"SessionEnd","reason":"other","session_id":EXPECTED}),
        ] {
            let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Claude);
            let mut receiver = hooks(InitialSource::Resume);
            assert_eq!(
                drive(&mut receiver, &mut reporter, &envelope(payload)),
                reporter::IGNORED
            );
            assert!(
                !reporter.closed() && reporter.binding().is_none(),
                "this invocation is still awaiting its certified announcement"
            );
            assert!(supervisor.observed().is_empty());
        }
        // A startup announcement contradicts a resume launch: the identity this
        // invocation certified is not what started, and nothing replaces it here.
        let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Claude);
        let mut receiver = hooks(InitialSource::Resume);
        assert_eq!(
            drive(&mut receiver, &mut reporter, &start("startup")),
            reporter::IGNORED
        );
        assert!(reporter.closed());
        assert_eq!(
            drive(&mut receiver, &mut reporter, &start("resume")),
            reporter::IGNORED,
            "the correct announcement afterwards does not reopen it"
        );
        assert!(reporter.binding().is_none());
        assert!(supervisor.observed().is_empty());

        for payload in [
            serde_json::json!({"hook_event_name":"SessionStart","source":"resume","session_id":"wrong-conversation","transcript_path":"/exact/root.jsonl"}),
            serde_json::json!({"hook_event_name":"SessionStart","source":"resume","session_id":EXPECTED}),
        ] {
            let (mut reporter, _supervisor) = Supervisor::reporter(AgentProvider::Claude);
            let mut receiver = hooks(InitialSource::Resume);
            assert_eq!(
                drive(&mut receiver, &mut reporter, &envelope(payload)),
                reporter::IGNORED
            );
            assert!(reporter.closed() && reporter.binding().is_none());
        }
    }

    #[test]
    fn the_certified_announcement_binds_once_and_a_repeat_is_ignored() {
        for (initial_source, source) in [
            (InitialSource::Startup, "startup"),
            (InitialSource::Resume, "resume"),
        ] {
            let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Claude);
            let mut receiver = hooks(initial_source);
            assert_eq!(
                drive(&mut receiver, &mut reporter, &start(source)),
                reporter::ACCEPTED
            );
            assert_eq!(
                reporter
                    .binding()
                    .map(|binding| binding.conversation.as_str()),
                Some(EXPECTED)
            );
            assert_eq!(
                receiver.transcript_path.as_deref(),
                Some("/exact/root.jsonl")
            );
            assert_eq!(
                drive(&mut receiver, &mut reporter, &start(source)),
                reporter::IGNORED,
                "a repeated announcement is not a second binding"
            );
            let binds: Vec<_> = supervisor
                .observed()
                .into_iter()
                .filter(|entry| matches!(entry, Observed::Bind { .. }))
                .collect();
            assert_eq!(
                binds,
                vec![Observed::Bind {
                    conversation: EXPECTED.to_owned(),
                    generation: 1,
                    expected: None,
                }],
                "an initial admission replaces no binding, and binds exactly once"
            );
        }
    }

    const REPLACED: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    fn bind_startup(receiver: &mut Hooks, reporter: &mut Reporter) {
        assert_eq!(
            drive(receiver, reporter, &start("startup")),
            reporter::ACCEPTED
        );
        assert_eq!(
            reporter.binding().map(|b| b.conversation.as_str()),
            Some(EXPECTED)
        );
    }

    fn session_end(reason: &str, session: &str) -> Vec<u8> {
        envelope(serde_json::json!({
            "hook_event_name":"SessionEnd",
            "reason":reason,
            "session_id":session,
        }))
    }

    fn session_start(source: &str, session: &str) -> Vec<u8> {
        envelope(serde_json::json!({
            "hook_event_name":"SessionStart",
            "source":source,
            "session_id":session,
            "transcript_path":format!("/exact/{session}.jsonl"),
        }))
    }

    fn activity(event: &str, prompt: &str, session: &str) -> Vec<u8> {
        let mut payload = serde_json::json!({
            "hook_event_name":event,
            "session_id":session,
            "prompt_id":prompt,
        });
        if event == "Notification" {
            payload["notification_type"] = "permission_prompt".into();
        }
        envelope(payload)
    }

    #[test]
    fn supported_clear_and_resume_rebind_with_fresh_generations_and_drop_stale_requests() {
        let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Claude);
        let mut receiver = hooks(InitialSource::Startup);
        bind_startup(&mut receiver, &mut reporter);

        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &activity("UserPromptSubmit", "turn-a", EXPECTED)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &activity("Notification", "turn-a", EXPECTED)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(receiver.open_requests.len(), 1);

        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &session_end("clear", EXPECTED)
            ),
            reporter::ACCEPTED
        );
        assert!(
            !reporter.closed(),
            "SessionEnd(clear) alone must not invent permanent invalidation"
        );
        assert_eq!(receiver.open_requests.len(), 0);
        assert_eq!(receiver.pending_leave, Some(PendingLeave::Clear));
        assert_eq!(
            reporter
                .binding()
                .map(|b| (b.conversation.as_str(), b.generation)),
            Some((EXPECTED, 1))
        );

        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &session_start("clear", REPLACED)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            reporter
                .binding()
                .map(|b| (b.conversation.as_str(), b.generation)),
            Some((REPLACED, 2))
        );
        assert_eq!(receiver.pending_leave, None);
        assert_eq!(receiver.expected.as_deref(), Some(REPLACED));

        // Late events from conversation A cannot affect B.
        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &activity("Stop", "turn-a", EXPECTED)
            ),
            reporter::IGNORED
        );
        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &activity("UserPromptSubmit", "turn-b", REPLACED)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &activity("Stop", "turn-b", REPLACED)
            ),
            reporter::ACCEPTED
        );

        // A -> B -> A: third generation.
        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &session_end("prompt_input_exit", REPLACED)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &session_start("resume", EXPECTED)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            reporter
                .binding()
                .map(|b| (b.conversation.as_str(), b.generation)),
            Some((EXPECTED, 3))
        );

        let binds: Vec<_> = supervisor
            .observed()
            .into_iter()
            .filter(|entry| matches!(entry, Observed::Bind { .. }))
            .collect();
        assert_eq!(binds.len(), 3, "{binds:?}");
    }

    #[test]
    fn foreground_fork_after_leave_rebinds_but_bare_fork_freezes() {
        let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Claude);
        let mut receiver = hooks(InitialSource::Startup);
        bind_startup(&mut receiver, &mut reporter);

        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &session_end("resume", EXPECTED)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(receiver.pending_leave, Some(PendingLeave::Switch));
        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &session_start("fork", REPLACED)
            ),
            reporter::ACCEPTED
        );
        assert_eq!(
            reporter
                .binding()
                .map(|b| (b.conversation.as_str(), b.generation)),
            Some((REPLACED, 2))
        );

        // Bare fork without a leave of the current conversation is the background-fork gap.
        let (mut reporter, supervisor2) = Supervisor::reporter(AgentProvider::Claude);
        let mut receiver = hooks(InitialSource::Startup);
        bind_startup(&mut receiver, &mut reporter);
        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &session_start("fork", REPLACED)
            ),
            reporter::IGNORED
        );
        assert!(reporter.closed());
        assert_eq!(
            Supervisor::health(&supervisor2.observed()),
            vec![(
                ReporterHealth::Unavailable,
                "identity_transition_unavailable".to_owned()
            )]
        );
        let _ = supervisor;
    }

    #[test]
    fn compact_and_foreign_end_do_not_invent_replacement() {
        let (mut reporter, _supervisor) = Supervisor::reporter(AgentProvider::Claude);
        let mut receiver = hooks(InitialSource::Startup);
        bind_startup(&mut receiver, &mut reporter);
        let before = reporter.binding().cloned();

        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &session_start("compact", EXPECTED)
            ),
            reporter::IGNORED
        );
        assert_eq!(reporter.binding(), before.as_ref());
        assert!(!reporter.closed());

        assert_eq!(
            drive(
                &mut receiver,
                &mut reporter,
                &session_end("other", EXPECTED)
            ),
            reporter::IGNORED
        );
        assert_eq!(reporter.binding(), before.as_ref());
        assert_eq!(receiver.pending_leave, None);
    }

    #[test]
    fn a_lost_bind_reply_is_recovered_by_the_next_announcement() {
        // The Bind and the receipt re-read that follows it are both withheld, so this
        // receiver answers unavailable and the announcement that repeats asks for the
        // original operation's receipt rather than binding a second generation.
        for (initial_source, source) in [
            (InitialSource::Startup, "startup"),
            (InitialSource::Resume, "resume"),
        ] {
            let (mut reporter, supervisor) =
                Supervisor::scripted(AgentProvider::Claude, vec![Answer::Withhold]);
            let mut receiver = hooks(initial_source);
            assert_eq!(
                receiver.frame(
                    &mut reporter,
                    &start(source),
                    true,
                    Instant::now() + Duration::from_millis(200)
                ),
                reporter::UNAVAILABLE,
                "{source}"
            );
            assert!(!reporter.closed() && reporter.binding().is_none());
            assert_eq!(
                drive(&mut receiver, &mut reporter, &start(source)),
                reporter::ACCEPTED,
                "{source}"
            );
            assert_eq!(
                reporter.binding().map(|binding| binding.generation),
                Some(1)
            );
            let observed = supervisor.observed();
            assert_eq!(
                observed
                    .iter()
                    .filter(|entry| matches!(entry, Observed::Bind { .. }))
                    .count(),
                1,
                "the retry asked for a receipt, it never bound again"
            );
            assert!(
                observed
                    .iter()
                    .any(|entry| matches!(entry, Observed::Status(_)))
            );
        }
    }

    #[test]
    fn a_resume_recovers_only_on_an_announcement_that_names_its_transcript() {
        // A resume is certified by the transcript the announcement names, and an
        // outstanding bind receipt does not relax that: an announcement without one is a
        // transition this invocation cannot vouch for, not the receipt it was waiting on.
        let (mut reporter, supervisor) =
            Supervisor::scripted(AgentProvider::Claude, vec![Answer::Withhold]);
        let mut receiver = hooks(InitialSource::Resume);
        assert_eq!(
            receiver.frame(
                &mut reporter,
                &start("resume"),
                true,
                Instant::now() + Duration::from_millis(200)
            ),
            reporter::UNAVAILABLE
        );
        let transcriptless = envelope(serde_json::json!({
            "hook_event_name":"SessionStart",
            "source":"resume",
            "session_id":EXPECTED,
        }));
        assert_eq!(
            drive(&mut receiver, &mut reporter, &transcriptless),
            reporter::IGNORED
        );
        assert!(reporter.closed());
        assert_eq!(
            drive(&mut receiver, &mut reporter, &start("resume")),
            reporter::IGNORED,
            "the announcement that would have recovered it arrives too late"
        );
        let observed = supervisor.observed();
        assert_eq!(
            observed
                .iter()
                .filter(|entry| matches!(entry, Observed::Bind { .. }))
                .count(),
            1,
            "a transition never binds"
        );
        // Ending the invocation settles the outstanding receipt first, so the health that
        // says so is published against the generation the supervisor really granted
        // rather than dropped for want of a binding.
        assert_eq!(
            reporter.binding().map(|binding| binding.generation),
            Some(1)
        );
        assert_eq!(
            Supervisor::health(&observed),
            vec![(
                ReporterHealth::Unavailable,
                "identity_transition_unavailable".to_owned()
            )]
        );
    }

    #[test]
    fn a_refused_binding_status_closes_without_rebinding() {
        let (mut reporter, supervisor) =
            Supervisor::scripted(AgentProvider::Claude, vec![Answer::Refuse]);
        let mut receiver = hooks(InitialSource::Resume);
        assert_eq!(
            drive(&mut receiver, &mut reporter, &start("resume")),
            reporter::UNAVAILABLE
        );
        assert!(reporter.closed() && reporter.binding().is_none());
        assert_eq!(
            drive(&mut receiver, &mut reporter, &start("resume")),
            reporter::IGNORED
        );
        assert_eq!(
            supervisor
                .observed()
                .iter()
                .filter(|entry| matches!(entry, Observed::Bind { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn collector_process_loss_automatically_publishes_unavailable_health() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let helper = root.path().join("collector-helper");
        std::fs::write(&helper, "#!/bin/sh\n/bin/sleep 60\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let source = super::super::collector::CollectorSource {
            path: root.path().join("transcript"),
            conversation: EXPECTED.into(),
        };
        std::fs::write(&source.path, "").unwrap();
        let collector =
            super::super::collector::CollectorController::spawn(&helper, source).unwrap();
        let collector_pid = collector.process_id().unwrap() as i32;
        let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Claude);
        let mut receiver = hooks(InitialSource::Startup);
        assert_eq!(
            drive(&mut receiver, &mut reporter, &start("startup")),
            reporter::ACCEPTED
        );
        let bound = reporter.binding().cloned().unwrap();
        receiver.collector = Some(collector);
        assert_eq!(unsafe { libc::kill(-collector_pid, libc::SIGKILL) }, 0);
        let give_up = Instant::now() + Duration::from_secs(2);
        while !reporter.paused() && Instant::now() < give_up {
            receiver.poll(&mut reporter, give_up);
            std::thread::yield_now();
        }
        assert_eq!(
            Supervisor::health(&supervisor.observed()),
            vec![(
                ReporterHealth::Unavailable,
                "collector_unavailable".to_owned()
            )]
        );
        assert_eq!(
            reporter.binding(),
            Some(&bound),
            "a lost reader does not lose the binding it was reading for"
        );
    }

    #[test]
    fn native_completion_drains_past_pending_pre_exit_eof() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let (mut reporter, supervisor) = Supervisor::reporter(AgentProvider::Claude);
        let mut receiver = hooks(InitialSource::Startup);
        assert_eq!(
            drive(&mut receiver, &mut reporter, &start("startup")),
            reporter::ACCEPTED
        );
        let source = super::super::collector::CollectorSource {
            path: root.path().join("transcript"),
            conversation: EXPECTED.into(),
        };
        let begin = serde_json::to_vec(&serde_json::json!({ "Start": &source })).unwrap();
        for (name, tokens) in [("a", 10), ("b", 30)] {
            let mut usage = Hooks::empty_metrics().usage.value;
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
            root.path().display(), begin.len() + 4,
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
        receiver.collector = Some(collector);
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
                    receiver.finish(&mut reporter, deadline);
                }
                Vec::new()
            }))
        })
        .unwrap();
        assert_eq!(status.code(), Some(17));
        assert!(started.elapsed() < Duration::from_millis(2500));
        let observed = supervisor.observed();
        let final_metrics = observed
            .iter()
            .find_map(|entry| match entry {
                Observed::Finalize(report) => match &report.observation {
                    ovrcr_protocol::AgentObservation::Metrics(metrics) => Some(metrics.clone()),
                    _ => None,
                },
                _ => None,
            })
            .expect("a bound invocation finalizes its accounting");
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
        for version in [
            ClaudeVersion::V2_1_267,
            ClaudeVersion::V2_1_268,
            ClaudeVersion::Later([2, 1, 274]),
        ] {
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
    fn blocked_version_probe_reaps_the_exact_group_within_budget() {
        let root = tempfile::tempdir().unwrap();
        let fifo = root.path().join("blocked");
        let identity = root.path().join("pid");
        let path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "printf '%s' \"$$\" > \"$1\"; IFS= read -r line < \"$2\"",
                "probe",
            ])
            .arg(&identity)
            .arg(&fifo);
        let started = Instant::now();
        assert!(probe_command(command).is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
        let pid = std::fs::read_to_string(identity)
            .unwrap()
            .parse::<libc::pid_t>()
            .unwrap();
        assert_eq!(
            unsafe { libc::kill(-pid, 0) },
            -1,
            "blocked probe group leaked"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    fn initial_admission_probe_cleans_descendants_after_leader_exit() {
        for (version, exit, expected) in [
            ("2.1.267", 0, true),
            ("2.1.268", 0, true),
            ("2.1.266", 0, false),
            ("2.1.274", 0, true),
            ("2.1.268", 1, false),
        ] {
            let root = tempfile::tempdir().unwrap();
            let identity = root.path().join("identity");
            // Exercise the same bounded probe and classification with an existing
            // interpreter. A freshly written executable can spend the whole probe
            // budget in host startup checks before reaching this lifecycle fixture.
            let mut command = Command::new("/bin/sh");
            command
                .args([
                    "-c",
                    "/bin/sleep 60 </dev/null >/dev/null 2>&1 &\nprintf '%s %s' \"$$\" \"$!\" > \"$1\"\nprintf '%s (Claude Code)\\n' \"$2\"\nexit \"$3\"\n",
                    "probe",
                ])
                .arg(&identity)
                .arg(version)
                .arg(exit.to_string());
            let probe = probe_command(command)
                .as_deref()
                .map_or(ClaudeVersionProbe::Unavailable, classify_version);
            assert_eq!(probe.supported().is_some(), expected);
            assert_eq!(
                probe.observed(),
                (exit == 0).then(|| version.to_owned()),
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

//! Server-owned account collection. Credential bytes and provider bodies stay in
//! this worker: snapshots carry windows and sanitized reasons only.
use super::{DispatchMessage, ServerState};
use ovrcr_protocol::{CLAUDE_WAITING, ErrorCode, NativeCommand, Response};
use ovrcr_protocol::{
    ProviderQuota, QuotaProvider, QuotaReport, QuotaSnapshot, QuotaSource, QuotaState, QuotaWindow,
};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, atomic::Ordering, mpsc::TrySendError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const POLL: Duration = Duration::from_secs(300);
const REQUEST: Duration = Duration::from_secs(20);
const MAX_RESPONSE: usize = 2 * 1024 * 1024;
/// Least time between accepted manual refreshes.
const COOLDOWN_MS: u64 = 30_000;
pub struct NativeQuotaUpdate {
    pub(super) provider: QuotaProvider,
    pub(super) generation: u64,
    pub(super) report: QuotaReport,
    pub(super) checked: bool,
    pub(super) reason: Option<String>,
    pub(super) next_check_unix_ms: Option<u64>,
}

/// Refresh state shared by the Server and its quota workers.
#[derive(Default)]
pub(super) struct Refresh {
    /// Unix milliseconds of the last accepted manual refresh, on [`clock_ms`].
    last_ms: Option<u64>,
    due: [bool; 2],
    /// The Claude row's state and reason while `claude auth status` rules an
    /// allowance out; `None` while signed in through claude.ai.
    pub(super) claude_auth: Option<(QuotaState, String)>,
    /// Set once `claude auth status` has answered since this process started.
    pub(super) auth_checked: bool,
    /// A manual refresh asked for a Claude probe.
    pub(super) claude_probe_due: bool,
    /// Latest Claude account read (`GET /api/oauth/usage`). Not a status-line sample.
    pub(super) claude_account: Option<ProviderQuota>,
}

fn slot(provider: QuotaProvider) -> Option<usize> {
    match provider {
        QuotaProvider::Codex => Some(0),
        QuotaProvider::Grok => Some(1),
        QuotaProvider::Claude => None,
    }
}

/// `Request::RefreshQuota`: mark the workers due now, once per cooldown.
///
/// Claude is included only while `quota.claude.probe` is on. The cooldown is
/// the same one the native providers use, read from [`clock_ms`] so a test
/// can advance it.
pub(super) fn request_refresh(state: &ServerState, provider: Option<QuotaProvider>) -> Response {
    let settings = state.quota_settings();
    let probe = settings.claude_probe;
    if provider == Some(QuotaProvider::Claude) && !probe {
        return Response::Error {
            code: ErrorCode::InvalidRequest,
            message: format!("Claude quota is not refreshable: {CLAUDE_WAITING}"),
        };
    }
    let wants_native = provider.is_none_or(|provider| provider != QuotaProvider::Claude);
    if wants_native && !settings.enabled && !(provider.is_none() && probe) {
        return Response::Error {
            code: ErrorCode::InvalidRequest,
            message: crate::settings::QUOTA_OFF.into(),
        };
    }
    let mut refresh = state.quota_refresh.lock().unwrap();
    let now = clock_ms();
    if let Some(last) = refresh.last_ms {
        let elapsed = now.saturating_sub(last);
        if elapsed < COOLDOWN_MS {
            drop(refresh);
            state.record_event(
                ovrcr_protocol::EventComponent::Quota,
                provider.map(|provider| provider.name().to_owned()),
                "cooldown refusal",
            );
            return Response::QuotaCooldown {
                remaining_ms: COOLDOWN_MS - elapsed,
            };
        }
    }
    refresh.last_ms = Some(now);
    let refresh_subject = provider.map(|provider| provider.name().to_owned());
    if probe && provider.is_none_or(|provider| provider == QuotaProvider::Claude) {
        refresh.claude_probe_due = true;
    }
    if settings.enabled {
        for (index, native) in [QuotaProvider::Codex, QuotaProvider::Grok]
            .into_iter()
            .enumerate()
        {
            if provider.is_none_or(|provider| provider == native) {
                refresh.due[index] = true;
            }
        }
    }
    drop(refresh);
    state.record_event(
        ovrcr_protocol::EventComponent::Quota,
        refresh_subject,
        "refresh",
    );
    Response::Ok
}

/// A native row before any read: Checking when enabled, otherwise Disabled
/// with the consent setting's off-state sentence.
fn consent_row(provider: QuotaProvider, enabled: bool) -> ProviderQuota {
    if enabled {
        ProviderQuota::unknown(provider, QuotaState::Checking)
    } else if provider == QuotaProvider::Grok {
        QuotaSnapshot::default().grok
    } else {
        QuotaSnapshot::default().codex
    }
}

pub(super) fn initial(enabled: bool) -> QuotaSnapshot {
    QuotaSnapshot {
        codex: consent_row(QuotaProvider::Codex, enabled),
        grok: consent_row(QuotaProvider::Grok, enabled),
        ..QuotaSnapshot::default()
    }
}

/// Follow `quota.enabled` in the Server's stored reading. Returns whether a row changed.
pub(super) fn sync_consent(state: &ServerState) -> bool {
    let enabled = state.quota_settings().enabled;
    let mut changed = false;
    let transitions = {
        let mut guard = state.quotas.lock().unwrap();
        let snapshots = &mut *guard;
        let mut transitions = Vec::new();
        for row in [&mut snapshots.codex, &mut snapshots.grok] {
            if enabled == (row.state == QuotaState::Disabled) {
                let before_state = row.state;
                let before_reason = row.reason.clone();
                *row = consent_row(row.provider, enabled);
                transitions.push((
                    row.provider,
                    before_state,
                    before_reason,
                    row.state,
                    row.reason.clone(),
                ));
                changed = true;
            }
        }
        transitions
    };
    for (provider, before_state, before_reason, after_state, after_reason) in transitions {
        note_transition(
            state,
            provider,
            before_state,
            before_reason.as_deref(),
            after_state,
            after_reason.as_deref(),
        );
    }
    changed
}

/// The `claude` the settings name (the `agents` override called "claude"),
/// else the one on PATH.
pub(super) fn claude_command(settings: &ovrcr_protocol::Settings) -> PathBuf {
    settings
        .agents
        .iter()
        .find(|agent| agent.name == "claude")
        .and_then(|agent| agent.argv.first())
        .map_or_else(|| "claude".into(), PathBuf::from)
}

/// Ask `claude auth status --json` at Server start and again whenever a
/// `quota.*` setting or the Claude executable changes, then let the
/// dispatcher recompute the Claude row.
pub(super) fn run_claude_auth(state: Arc<ServerState>) {
    let mut last = None;
    let mut pending = false;
    while !state.shutdown.load(Ordering::Acquire) && !state.stopping.load(Ordering::Acquire) {
        let settings = state.settings.lock().unwrap().report.settings.clone();
        let key = (settings.quota.clone(), claude_command(&settings));
        if last.as_ref() != Some(&key) {
            let auth = claude_auth(&state, &key.1)
                .err()
                .map(|failure| (failure.state, failure.reason));
            // A check cut short by Server stop is no answer; publish nothing.
            if state.shutdown.load(Ordering::Acquire) || state.stopping.load(Ordering::Acquire) {
                break;
            }
            let mut refresh = state.quota_refresh.lock().unwrap();
            refresh.claude_auth = auth;
            refresh.auth_checked = true;
            drop(refresh);
            last = Some(key);
            pending = true;
        }
        if pending {
            match state.dispatch.try_send(DispatchMessage::ClaudeAuth) {
                Ok(()) => pending = false,
                Err(TrySendError::Full(_)) => {}
                Err(_) => break,
            }
        }
        std::thread::park_timeout(Duration::from_millis(100));
    }
}

/// Ok while signed in through claude.ai. Only `loggedIn` and `authMethod`
/// are read; the email, org ids and every other field are dropped unread.
fn claude_auth(state: &ServerState, command: &Path) -> Result<()> {
    let workspace = tempfile::tempdir()
        .map_err(|_| Failure::unavailable("could not create a native workspace"))?;
    let program = NativeCommand {
        command: command.into(),
        home: None,
    };
    let mut rpc = Rpc::spawn(
        QuotaProvider::Claude,
        &program,
        &["auth", "status", "--json"],
        workspace.path(),
    )?;
    let deadline = Instant::now() + REQUEST;
    loop {
        if state.stopping.load(Ordering::Acquire) || state.shutdown.load(Ordering::Acquire) {
            return Err(interrupted());
        }
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        let mut chunk = [0u8; 16_384];
        match rpc.output.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) if rpc.buffer.len() + count > MAX_RESPONSE => {
                return Err(Failure::new(QuotaState::Invalid).because("reply too large"));
            }
            Ok(count) => rpc.buffer.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::park_timeout(Duration::from_millis(10));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(Failure::unavailable("native output failed")),
        }
    }
    let unreadable = || Failure::unavailable("claude auth status unreadable");
    let status = serde_json::from_slice::<Value>(&rpc.buffer).map_err(|_| unreadable())?;
    match (status["loggedIn"].as_bool(), status["authMethod"].as_str()) {
        (Some(false), _) => {
            Err(Failure::new(QuotaState::NotSignedIn).because("run claude auth login"))
        }
        (Some(true), Some("claude.ai")) => Ok(()),
        (Some(true), _) => Err(Failure::new(QuotaState::Unsupported)
            .because("API-key logins have no subscription allowance")),
        (None, _) => Err(unreadable()),
    }
}

/// Minutes before retrying after the `failures`th consecutive failure.
const LADDER: [u64; 4] = [1, 2, 5, 10];

/// A provider Retry-After wins; a deterministic failure waits the cap at once.
fn backoff(failures: u32, deterministic: bool, retry_after: Option<Duration>) -> Duration {
    retry_after.unwrap_or_else(|| {
        let step = if deterministic {
            LADDER.len() - 1
        } else {
            (failures.max(1) as usize - 1).min(LADDER.len() - 1)
        };
        Duration::from_secs(LADDER[step] * 60)
    })
}

pub(super) fn note_transition(
    state: &ServerState,
    provider: QuotaProvider,
    before_state: QuotaState,
    before_reason: Option<&str>,
    after_state: QuotaState,
    after_reason: Option<&str>,
) {
    if before_state == after_state && before_reason == after_reason {
        return;
    }
    let mut message = format!("{before_state:?} -> {after_state:?}");
    if let Some(reason) = after_reason.filter(|reason| !reason.is_empty()) {
        message.push_str(": ");
        message.push_str(reason);
    }
    state.record_event(
        ovrcr_protocol::EventComponent::Quota,
        Some(provider.name().to_owned()),
        message,
    );
}

pub(super) fn apply(state: &ServerState, update: NativeQuotaUpdate) {
    // A read that finished after `quota.enabled` went off keeps the row Disabled.
    if update.generation == 0
        || update.report.validate().is_err()
        || !state.quota_settings().enabled
    {
        return;
    }
    let mut snapshots = state.quotas.lock().unwrap();
    let target = match update.provider {
        QuotaProvider::Codex => &mut snapshots.codex,
        QuotaProvider::Grok => &mut snapshots.grok,
        QuotaProvider::Claude => return,
    };
    let before_state = target.state;
    let before_reason = target.reason.clone();
    let old_generation = match &target.source {
        Some(QuotaSource::NativeProfile { generation, .. }) => *generation,
        _ => 0,
    };
    if update.generation < old_generation {
        return;
    }
    if update.generation != old_generation {
        *target = ProviderQuota::unknown(update.provider, QuotaState::Checking);
        target.source = Some(QuotaSource::NativeProfile {
            profile: format!("{} selected native profile", update.provider.name()),
            generation: update.generation,
        });
    }
    if let Some(windows) = update.report.windows {
        if target.windows != windows || target.observed_unix_ms.is_none() {
            target.observed_unix_ms = Some(now_ms());
        }
        target.windows = windows;
    }
    target.state = update.report.state;
    target.reason = update.reason;
    target.next_check_unix_ms = update.next_check_unix_ms;
    if update.checked {
        target.checked_unix_ms = Some(now_ms());
    }
    let after_state = target.state;
    let after_reason = target.reason.clone();
    let provider = update.provider;
    drop(snapshots);
    state.publish_quotas();
    note_transition(
        state,
        provider,
        before_state,
        before_reason.as_deref(),
        after_state,
        after_reason.as_deref(),
    );
}

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Unix milliseconds. `OVRCR_QUOTA_CLOCK`, when set, is a file containing that
/// integer: a test seam for the probe cadence and the refresh cooldown, not a
/// setting. Absent or unreadable, this is the wall clock.
pub(super) fn clock_ms() -> u64 {
    let Some(path) = std::env::var_os("OVRCR_QUOTA_CLOCK") else {
        return wall_ms();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or_else(wall_ms)
}

fn now_ms() -> u64 {
    clock_ms()
}

pub(super) fn run(state: Arc<ServerState>, provider: QuotaProvider) {
    let Some(slot) = slot(provider) else {
        return;
    };
    let (identity_method, limits_method) = match provider {
        QuotaProvider::Codex => ("account/read", "account/rateLimits/read"),
        _ => ("_x.ai/auth/info", "_x.ai/billing"),
    };
    let identity_params = if provider == QuotaProvider::Codex {
        json!({"refreshToken":false})
    } else {
        json!({})
    };
    let mut generation = 1;
    let mut identity = None;
    let mut client = None;
    let mut due = Instant::now();
    // The end of a provider Retry-After, which no manual refresh skips.
    let mut hold: Option<Instant> = None;
    let mut failures = 0u32;
    let mut pending = None;
    let mut last_config = None;
    while !state.shutdown.load(Ordering::Acquire) && !state.stopping.load(Ordering::Acquire) {
        if let Some(update) = pending.take() {
            match state
                .dispatch
                .try_send(DispatchMessage::NativeQuota(update))
            {
                Ok(()) => {}
                Err(TrySendError::Full(DispatchMessage::NativeQuota(update))) => {
                    pending = Some(update)
                }
                Err(_) => break,
            }
        }
        // Every `quota.*` setting is live: any change, `enabled` included,
        // drops this cycle's client and backoff and reads at once when enabled.
        let config = state.quota_settings();
        if last_config.as_ref() != Some(&config) {
            client = None;
            identity = None;
            due = Instant::now();
            hold = None;
            failures = 0;
            last_config = Some(config.clone());
        }
        if !config.enabled {
            std::thread::park_timeout(Duration::from_millis(100));
            continue;
        }
        if !state.dashboard.is_claimed() {
            client = None;
            std::thread::park_timeout(Duration::from_millis(100));
            continue;
        }
        if std::mem::take(&mut state.quota_refresh.lock().unwrap().due[slot])
            && hold.is_none_or(|hold| Instant::now() >= hold)
        {
            due = Instant::now();
        }
        let mut checked = false;
        let result = if Instant::now() >= due {
            checked = true;
            (|| {
                let deadline = Instant::now() + REQUEST;
                if provider == QuotaProvider::Grok {
                    // x.ai/billing is method-not-found on current Grok CLIs.
                    // Read the typed credits JSON with the existing login key.
                    let (ident, windows) = read_grok_allowance(&config.grok)?;
                    if identity.as_ref() != Some(&ident) {
                        generation += 1;
                        identity = Some(ident);
                    }
                    return Ok(windows);
                }
                if client.is_none() {
                    let program = match provider {
                        QuotaProvider::Codex => &config.codex,
                        _ => &config.grok,
                    };
                    client = Some(native_client(provider, program, &state, deadline)?);
                }
                let rpc = client.as_mut().unwrap();
                let before = native_identity(
                    provider,
                    &rpc.call(identity_method, identity_params.clone(), &state, deadline)?,
                )?;
                if identity.as_ref() != Some(&before) {
                    generation += 1;
                    identity = Some(before.clone());
                    pending = Some(Box::new(NativeQuotaUpdate {
                        provider,
                        generation,
                        report: QuotaReport {
                            windows: None,
                            state: QuotaState::Checking,
                        },
                        checked: false,
                        reason: None,
                        next_check_unix_ms: None,
                    }));
                }
                let account_epoch = rpc.account_epoch;
                let limits = rpc.call(limits_method, json!({}), &state, deadline)?;
                let after = native_identity(
                    provider,
                    &rpc.call(identity_method, identity_params.clone(), &state, deadline)?,
                )?;
                if before != after || account_epoch != rpc.account_epoch {
                    generation += 1;
                    identity = Some(after);
                    return Err(changed_during_read());
                }
                normalize_native_quota(provider, &limits).map_err(Failure::new)
            })()
        } else if let Some(rpc) = client.as_mut() {
            match rpc.message(&state, Instant::now() + Duration::from_millis(100)) {
                Ok(Some(message)) if message["method"] == "account/rateLimits/updated" => {
                    // Verify context before admitting an unsolicited native update.
                    let deadline = Instant::now() + REQUEST;
                    let account_epoch = rpc.account_epoch;
                    let fresh = rpc
                        .call(
                            "account/read",
                            json!({"refreshToken":false}),
                            &state,
                            deadline,
                        )
                        .and_then(|value| codex_identity(&value));
                    match fresh {
                        Ok(fresh)
                            if identity.as_ref() == Some(&fresh)
                                && account_epoch == rpc.account_epoch =>
                        {
                            normalize_native_quota(QuotaProvider::Codex, &message["params"])
                                .map_err(Failure::new)
                        }
                        _ => {
                            generation += 1;
                            due = Instant::now();
                            Err(changed_during_read())
                        }
                    }
                }
                Ok(Some(message)) if message["method"] == "account/updated" => {
                    generation += 1;
                    identity = None;
                    due = Instant::now();
                    Err(Failure::new(QuotaState::Waiting))
                }
                Ok(_) => continue,
                Err(error) => Err(error),
            }
        } else {
            std::thread::park_timeout(Duration::from_millis(100));
            continue;
        };
        if checked {
            // One read per provider: a refresh asked for during it is answered by it.
            state.quota_refresh.lock().unwrap().due[slot] = false;
        }
        let (report, successful, reason) = match result {
            Ok(windows) => {
                failures = 0;
                if checked {
                    due = Instant::now() + POLL;
                    hold = None;
                }
                (
                    QuotaReport {
                        windows: Some(windows),
                        state: QuotaState::Current,
                    },
                    true,
                    None,
                )
            }
            Err(error) => {
                if error.state != QuotaState::Waiting {
                    failures = failures.saturating_add(1);
                    due =
                        Instant::now() + backoff(failures, error.deterministic, error.retry_after);
                    hold = error.retry_after.map(|_| due);
                    client = None;
                }
                (
                    QuotaReport {
                        windows: None,
                        state: error.state,
                    },
                    false,
                    Some(error.reason),
                )
            }
        };
        let next_check = due.saturating_duration_since(Instant::now()).as_millis() as u64;
        pending = Some(Box::new(NativeQuotaUpdate {
            provider,
            generation,
            report,
            checked: checked && successful,
            reason,
            next_check_unix_ms: Some(now_ms() + next_check),
        }));
    }
}

/// A failed read and its Quota reason: OVRCR's own words, never native bytes.
struct Failure {
    state: QuotaState,
    retry_after: Option<Duration>,
    reason: String,
    /// Retrying soon cannot help: not found, unsupported, not signed in.
    deterministic: bool,
}
impl Failure {
    fn new(state: QuotaState) -> Self {
        Self {
            state,
            retry_after: None,
            reason: state.label().into(),
            deterministic: matches!(state, QuotaState::NotSignedIn | QuotaState::Unsupported),
        }
    }
    fn because(mut self, reason: impl Into<String>) -> Self {
        self.reason = reason.into();
        self
    }
    fn unavailable(reason: impl Into<String>) -> Self {
        Self::new(QuotaState::Unavailable).because(reason)
    }
}

fn timed_out() -> Failure {
    Failure::unavailable(format!("timed out after {} s", REQUEST.as_secs()))
}

fn interrupted() -> Failure {
    Failure::unavailable("read interrupted")
}

fn changed_during_read() -> Failure {
    Failure::new(QuotaState::SourceConflict).because("account changed during read")
}
type Result<T> = std::result::Result<T, Failure>;

struct Rpc {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    buffer: Vec<u8>,
    next_id: u64,
    account_epoch: u64,
    workspace: Option<tempfile::TempDir>,
}
impl Rpc {
    fn spawn(
        provider: QuotaProvider,
        program: &NativeCommand,
        args: &[&str],
        cwd: &Path,
    ) -> Result<Self> {
        let mut command = Command::new(&program.command);
        command
            .args(args)
            .current_dir(cwd)
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for name in [
            "OVRCR_HOOK_SOCKET",
            "OVRCR_HOOK_TOKEN",
            "OVRCR_SESSION_ID",
            "OVRCR_SOCKET",
            "OVRCR_CONFIG",
        ] {
            command.env_remove(name);
        }
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("OVRCR_AGENT_") {
                command.env_remove(name);
            }
        }
        if let Some(home) = &program.home {
            command.env(
                if provider == QuotaProvider::Codex {
                    "CODEX_HOME"
                } else {
                    "GROK_HOME"
                },
                home,
            );
        }
        let name = provider.name().to_lowercase();
        let mut child = command.spawn().map_err(|error| {
            let reason = if error.kind() != std::io::ErrorKind::NotFound {
                format!("{name} could not start")
            } else if program
                .command
                .as_os_str()
                .as_encoded_bytes()
                .contains(&b'/')
            {
                format!("{name} not found at the configured path")
            } else {
                format!("{name} not found on PATH")
            };
            Failure {
                deterministic: true,
                ..Failure::unavailable(reason)
            }
        })?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let rpc = Self {
            child,
            input,
            output,
            buffer: Vec::new(),
            next_id: 1,
            account_epoch: 0,
            workspace: None,
        };
        for fd in [rpc.input.as_raw_fd(), rpc.output.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(Failure::unavailable(format!("{name} could not start")));
            }
        }
        Ok(rpc)
    }
    fn bytes(&mut self, state: &ServerState, deadline: Instant) -> Result<bool> {
        loop {
            if state.stopping.load(Ordering::Acquire)
                || state.shutdown.load(Ordering::Acquire)
                || !state.dashboard.is_claimed()
            {
                return Err(interrupted());
            }
            if Instant::now() >= deadline {
                return Ok(true);
            }
            let mut chunk = [0u8; 16_384];
            match self.output.read(&mut chunk) {
                Ok(0) => return Ok(false),
                Ok(count) => {
                    if self.buffer.len() + count > MAX_RESPONSE {
                        return Err(Failure::new(QuotaState::Invalid).because("reply too large"));
                    }
                    self.buffer.extend_from_slice(&chunk[..count]);
                    return Ok(true);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(Failure::unavailable("native output failed")),
            }
            if Instant::now() >= deadline {
                return Ok(true);
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
    fn message(&mut self, state: &ServerState, deadline: Instant) -> Result<Option<Value>> {
        loop {
            if state.stopping.load(Ordering::Acquire)
                || state.shutdown.load(Ordering::Acquire)
                || !state.dashboard.is_claimed()
            {
                return Err(interrupted());
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            if let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line = self.buffer.drain(..=end).collect::<Vec<_>>();
                if let Ok(message) = serde_json::from_slice::<Value>(&line) {
                    if message["method"] == "account/updated" {
                        self.account_epoch = self.account_epoch.saturating_add(1);
                    }
                    return Ok(Some(message));
                }
            } else if Instant::now() >= deadline {
                return Ok(None);
            } else if !self.bytes(state, deadline)? {
                return Err(Failure::unavailable("native client closed its output"));
            }
        }
    }
    fn send(&mut self, message: Value, state: &ServerState, deadline: Instant) -> Result<()> {
        let mut bytes =
            serde_json::to_vec(&message).map_err(|_| Failure::new(QuotaState::Invalid))?;
        bytes.push(b'\n');
        let mut remaining = bytes.as_slice();
        while !remaining.is_empty() {
            if state.stopping.load(Ordering::Acquire)
                || state.shutdown.load(Ordering::Acquire)
                || !state.dashboard.is_claimed()
            {
                return Err(interrupted());
            }
            if Instant::now() >= deadline {
                return Err(timed_out());
            }
            match self.input.write(remaining) {
                Ok(0) => return Err(Failure::unavailable("native pipe closed")),
                Ok(count) => remaining = &remaining[count..],
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::park_timeout(Duration::from_millis(10));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(Failure::unavailable("native pipe closed")),
            }
        }
        Ok(())
    }
    fn call(
        &mut self,
        method: &str,
        params: Value,
        state: &ServerState,
        deadline: Instant,
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(
            json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}),
            state,
            deadline,
        )?;
        while let Some(reply) = self.message(state, deadline)? {
            if reply["id"] != id {
                continue;
            }
            if let Some(error) = reply.get("error") {
                // Only the code and status numbers are read; the message never is.
                let status = error["data"]["status"].as_u64();
                let failure = match (error["code"].as_i64(), status) {
                    (Some(-32601), _) => {
                        Failure::new(QuotaState::Unsupported).because("method not supported")
                    }
                    (_, Some(401)) => Failure::new(QuotaState::NotSignedIn),
                    (_, Some(status @ 100..=599)) => Failure::unavailable(format!("HTTP {status}")),
                    _ => Failure::unavailable("request failed"),
                };
                // At least the ladder's first step, so a zero cannot spin the worker.
                let retry_after = error["data"]["retryAfterSeconds"]
                    .as_u64()
                    .map(|seconds| Duration::from_secs(seconds.clamp(60, 86_400)));
                return Err(Failure {
                    retry_after,
                    ..failure
                });
            }
            return reply
                .get("result")
                .cloned()
                .ok_or_else(|| Failure::new(QuotaState::Invalid));
        }
        Err(timed_out())
    }
}
impl Drop for Rpc {
    fn drop(&mut self) {
        // The spawn established this group; never attach to or signal native shared clients.
        let pgid = self.child.id() as libc::pid_t;
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
        std::thread::park_timeout(Duration::from_millis(100));
        // Do not reap the leader before the final group signal, avoiding PID reuse.
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Err(_) => break,
                _ => std::thread::park_timeout(Duration::from_millis(10)),
            }
        }
        eprintln!("native quota process cleanup remains unverified");
    }
}

fn native_client(
    provider: QuotaProvider,
    program: &NativeCommand,
    state: &ServerState,
    deadline: Instant,
) -> Result<Rpc> {
    let workspace = tempfile::tempdir()
        .map_err(|_| Failure::unavailable("could not create a native workspace"))?;
    let mut version = Rpc::spawn(provider, program, &["--version"], workspace.path())?;
    while version.bytes(state, deadline)? {
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
    }
    let name = provider.name().to_lowercase();
    let version = std::str::from_utf8(&version.buffer).map_err(|_| {
        Failure::new(QuotaState::Unsupported).because(format!("{name} version unreadable"))
    })?;
    let supported = if provider == QuotaProvider::Codex {
        "0.155.1"
    } else {
        "1.0.40"
    };
    if !version.split_whitespace().any(|part| part == supported) {
        return Err(Failure::new(QuotaState::Unsupported)
            .because(format!("{name} version unsupported (needs {supported})")));
    }
    let args: &[&str] = if provider == QuotaProvider::Codex {
        // Codex keeps the token. Do not read or write auth.json from here.
        &["-s", "read-only", "-a", "never", "app-server"]
    } else {
        &["--no-auto-update", "agent", "--no-leader", "stdio"]
    };
    let mut rpc = Rpc::spawn(provider, program, args, workspace.path())?;
    let init = if provider == QuotaProvider::Codex {
        json!({"clientInfo":{"name":"ovrcr-quota", "version":env!("CARGO_PKG_VERSION")}, "capabilities":{"experimentalApi":false}})
    } else {
        json!({"protocolVersion":1, "clientCapabilities":{"fs":{"readTextFile":false, "writeTextFile":false}, "terminal":false},
            "clientInfo":{"name":"ovrcr-quota", "version":env!("CARGO_PKG_VERSION")}})
    };
    rpc.call("initialize", init, state, deadline)?;
    if provider == QuotaProvider::Codex {
        rpc.send(
            json!({"jsonrpc":"2.0", "method":"initialized", "params":{}}),
            state,
            deadline,
        )?;
    }
    rpc.workspace = Some(workspace);
    Ok(rpc)
}

fn codex_identity(value: &Value) -> Result<String> {
    let account = &value["account"];
    if account.is_null() {
        return Err(Failure::new(QuotaState::NotSignedIn));
    }
    if account["type"] != "chatgpt" {
        return Err(Failure::new(QuotaState::Unsupported).because("unsupported account type"));
    }
    let email = account["email"]
        .as_str()
        .filter(|email| !email.is_empty() && email.len() <= 512)
        .ok_or_else(|| Failure::new(QuotaState::SourceConflict))?;
    // Native metadata stays private in this worker; never serialize or log it.
    Ok(format!(
        "{email}/{}",
        account["planType"].as_str().unwrap_or("unknown")
    ))
}

fn percent(value: Option<&Value>) -> Result<(Option<u16>, bool)> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok((None, false));
    };
    let used = value
        .as_f64()
        .filter(|used| used.is_finite() && *used >= 0.0)
        .ok_or_else(|| Failure::new(QuotaState::Invalid))?;
    Ok(if used > 100.0 {
        (None, true)
    } else {
        (Some((used * 100.0).floor() as u16), false)
    })
}

fn codex_windows(value: &Value) -> Result<Vec<QuotaWindow>> {
    if !value.is_object()
        || (value.get("rateLimits").is_none() && value.get("rateLimitsByLimitId").is_none())
    {
        return Err(Failure::new(QuotaState::Invalid));
    }
    let mut windows = Vec::new();
    let mut add = |bucket: &str, snapshot: &Value, general: bool| -> Result<()> {
        if snapshot.is_null() {
            return Ok(());
        }
        if !snapshot.is_object() || bucket.len() > 100 {
            return Err(Failure::new(QuotaState::Invalid));
        }
        for name in ["primary", "secondary"] {
            let window = &snapshot[name];
            if window.is_null() {
                continue;
            }
            let minutes = window
                .get("windowDurationMins")
                .filter(|v| !v.is_null())
                .map(|v| {
                    v.as_u64()
                        .filter(|v| *v > 0)
                        .ok_or_else(|| Failure::new(QuotaState::Invalid))
                })
                .transpose()?;
            let label = match minutes {
                Some(minutes) if minutes % 1440 == 0 => format!("{}d", minutes / 1440),
                Some(minutes) if minutes % 60 == 0 => format!("{}h", minutes / 60),
                Some(minutes) => format!("{minutes}m"),
                None => name.into(),
            };
            let (used_basis_points, over_limit) = percent(window.get("usedPercent"))?;
            let resets_unix_ms = window
                .get("resetsAt")
                .filter(|v| !v.is_null())
                .map(|value| {
                    value
                        .as_u64()
                        .and_then(|value| value.checked_mul(1000))
                        .ok_or_else(|| Failure::new(QuotaState::Invalid))
                })
                .transpose()?;
            windows.push(QuotaWindow {
                id: format!("{bucket}/{name}"),
                label,
                general,
                used_basis_points,
                over_limit,
                resets_unix_ms,
            });
        }
        Ok(())
    };
    let general = &value["rateLimits"];
    add(
        general["limitId"].as_str().unwrap_or("general"),
        general,
        true,
    )?;
    if let Some(buckets) = value["rateLimitsByLimitId"].as_object() {
        for (name, bucket) in buckets {
            if general["limitId"].as_str() != Some(name) {
                add(name, bucket, false)?;
            }
        }
    }
    QuotaReport {
        windows: Some(windows.clone()),
        state: QuotaState::Current,
    }
    .validate()
    .map_err(|_| Failure::new(QuotaState::Invalid))?;
    Ok(windows)
}

fn native_identity(provider: QuotaProvider, value: &Value) -> Result<String> {
    if provider == QuotaProvider::Codex {
        return codex_identity(value);
    }
    if value["teamId"].as_str().is_some_and(|id| !id.is_empty())
        || value["principalType"]
            .as_str()
            .is_some_and(|kind| !kind.eq_ignore_ascii_case("user"))
    {
        return Err(
            Failure::new(QuotaState::Unsupported).because("team or non-user context unsupported")
        );
    }
    let identity = value["principalId"]
        .as_str()
        .or_else(|| value["email"].as_str())
        .filter(|value| {
            !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| {
            Failure::new(if value["methodId"].is_null() {
                QuotaState::NotSignedIn
            } else {
                QuotaState::Unsupported
            })
        })?;
    Ok(identity.into())
}

/// Normalize a supported native reply without exporting its body or account metadata.
pub fn normalize_native_quota(
    provider: QuotaProvider,
    value: &Value,
) -> std::result::Result<Vec<QuotaWindow>, QuotaState> {
    native_windows(provider, value).map_err(|error| error.state)
}

fn native_windows(provider: QuotaProvider, value: &Value) -> Result<Vec<QuotaWindow>> {
    if provider == QuotaProvider::Codex {
        return codex_windows(value);
    }
    let config = value["config"]
        .as_object()
        .ok_or_else(|| Failure::new(QuotaState::Unsupported))?;
    let period = config
        .get("currentPeriod")
        .and_then(Value::as_object)
        .ok_or_else(|| Failure::new(QuotaState::Unsupported))?;
    let period_type = period
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Failure::new(QuotaState::Invalid))?;
    let label = match period_type {
        "USAGE_PERIOD_TYPE_WEEKLY" => "wk",
        "USAGE_PERIOD_TYPE_MONTHLY" => "mo",
        _ => return Err(Failure::new(QuotaState::Unsupported)),
    };
    let reset = period
        .get("end")
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .and_then(|value| u64::try_from(value.timestamp_millis()).ok())
        .ok_or_else(|| Failure::new(QuotaState::Invalid))?;
    if let Some(start) = period.get("start").filter(|value| !value.is_null()) {
        let start = start
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .and_then(|value| u64::try_from(value.timestamp_millis()).ok())
            .ok_or_else(|| Failure::new(QuotaState::Invalid))?;
        if start > reset || start > now_ms() {
            return Err(Failure::new(QuotaState::Invalid));
        }
    }
    let (used_basis_points, over_limit) = percent(config.get("creditUsagePercent"))?;
    let windows = vec![QuotaWindow {
        id: format!("grok/{period_type}"),
        label: label.into(),
        general: true,
        used_basis_points,
        over_limit,
        resets_unix_ms: Some(reset),
    }];
    QuotaReport {
        windows: Some(windows.clone()),
        state: QuotaState::Current,
    }
    .validate()
    .map_err(|_| Failure::new(QuotaState::Invalid))?;
    Ok(windows)
}

/// Claude account allowance from the Claude Code credential file.
/// A missing file is not an error: the status line and `claude auth status`
/// still explain a session that has not reported. This read does not refresh
/// or rewrite the file.
pub(super) fn run_claude_account(state: Arc<ServerState>) {
    let mut due = Instant::now();
    let mut failures = 0u32;
    let mut generation = 1u64;
    let mut fingerprint = None;
    let mut pending: Option<ClaudeAccountUpdate> = None;
    while !state.shutdown.load(Ordering::Acquire) && !state.stopping.load(Ordering::Acquire) {
        if let Some(update) = pending.take() {
            match state
                .dispatch
                .try_send(DispatchMessage::ClaudeAccount(Box::new(update)))
            {
                Ok(()) => {}
                Err(TrySendError::Full(DispatchMessage::ClaudeAccount(update))) => {
                    pending = Some(*update)
                }
                Err(_) => break,
            }
        }
        if !state.dashboard.is_claimed() {
            std::thread::park_timeout(Duration::from_millis(100));
            continue;
        }
        if Instant::now() < due {
            std::thread::park_timeout(Duration::from_millis(100));
            continue;
        }
        let checked = true;
        let result = read_claude_allowance();
        let (report, successful, reason) = match result {
            Ok(None) => {
                failures = 0;
                due = Instant::now() + POLL;
                fingerprint = None;
                continue;
            }
            Ok(Some((ident, windows))) => {
                failures = 0;
                due = Instant::now() + POLL;
                if fingerprint.as_ref() != Some(&ident) {
                    generation = generation.saturating_add(1);
                    fingerprint = Some(ident);
                }
                (
                    QuotaReport {
                        windows: Some(windows),
                        state: QuotaState::Current,
                    },
                    true,
                    None,
                )
            }
            Err(error) => {
                failures = failures.saturating_add(1);
                due = Instant::now() + backoff(failures, error.deterministic, error.retry_after);
                (
                    QuotaReport {
                        windows: None,
                        state: error.state,
                    },
                    false,
                    Some(error.reason),
                )
            }
        };
        let next_check = due.saturating_duration_since(Instant::now()).as_millis() as u64;
        pending = Some(ClaudeAccountUpdate {
            generation,
            report,
            checked: checked && successful,
            reason,
            next_check_unix_ms: Some(now_ms() + next_check),
        });
    }
}

pub struct ClaudeAccountUpdate {
    pub(super) generation: u64,
    pub(super) report: QuotaReport,
    pub(super) checked: bool,
    pub(super) reason: Option<String>,
    pub(super) next_check_unix_ms: Option<u64>,
}

/// Publish a Claude account read. A current status-line sample is left in
/// place: it is an in-session extra, not a replacement for this read, and not
/// required before the account row can show allowance.
pub(super) fn store_claude_account(state: &ServerState, update: ClaudeAccountUpdate) {
    if update.generation == 0 || update.report.validate().is_err() {
        return;
    }
    let mut row = ProviderQuota::unknown(QuotaProvider::Claude, update.report.state);
    row.reason = update.reason;
    row.next_check_unix_ms = update.next_check_unix_ms;
    row.source = Some(QuotaSource::NativeProfile {
        profile: "claude credentials".into(),
        generation: update.generation,
    });
    if let Some(windows) = update.report.windows {
        if !windows.is_empty() {
            row.observed_unix_ms = Some(now_ms());
        }
        row.windows = windows;
    }
    if update.checked {
        row.checked_unix_ms = Some(now_ms());
    }
    state.quota_refresh.lock().unwrap().claude_account = Some(row);
}

fn read_claude_allowance() -> Result<Option<(String, Vec<QuotaWindow>)>> {
    let Some((ident, token)) = claude_token()? else {
        return Ok(None);
    };
    let url = loopback_or(
        "OVRCR_QUOTA_CLAUDE_USAGE_URL",
        "https://api.anthropic.com/api/oauth/usage",
    );
    let body = http_get(
        &url,
        &[
            ("Authorization", &format!("Bearer {token}")),
            ("anthropic-beta", "oauth-2025-04-20"),
            ("Accept", "application/json"),
        ],
    )?;
    drop(token);
    let value: Value =
        serde_json::from_str(&body).map_err(|_| Failure::new(QuotaState::Invalid))?;
    let windows = claude_usage_windows(&value)?;
    if windows.is_empty() {
        return Err(Failure::new(QuotaState::Unsupported).because("no subscription windows"));
    }
    Ok(Some((ident, windows)))
}

fn claude_token() -> Result<Option<(String, String)>> {
    let path = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(|dir| PathBuf::from(dir).join(".credentials.json"))
        .unwrap_or_else(|| home_dir().join(".claude").join(".credentials.json"));
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Failure::unavailable("claude credentials unreadable")),
    };
    if bytes.len() > 65_536 {
        return Err(Failure::unavailable("claude credentials unreadable"));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Failure::unavailable("claude credentials unreadable"))?;
    let token = value["claudeAiOauth"]["accessToken"]
        .as_str()
        .filter(|token| !token.is_empty() && token.len() <= 8192)
        .map(str::to_owned);
    let Some(token) = token else {
        return Ok(None);
    };
    let ident = fingerprint(&token);
    Ok(Some((ident, token)))
}

fn read_grok_allowance(program: &NativeCommand) -> Result<(String, Vec<QuotaWindow>)> {
    let (ident, token) = grok_token(program)?;
    let url = loopback_or(
        "OVRCR_QUOTA_GROK_BILLING_URL",
        "https://cli-chat-proxy.grok.com/v1/billing?format=credits",
    );
    let body = http_get(
        &url,
        &[
            ("Authorization", &format!("Bearer {token}")),
            ("x-xai-token-auth", "xai-grok-cli"),
            ("Accept", "application/json"),
        ],
    )?;
    drop(token);
    let value: Value =
        serde_json::from_str(&body).map_err(|_| Failure::new(QuotaState::Invalid))?;
    Ok((ident, native_windows(QuotaProvider::Grok, &value)?))
}

fn grok_token(program: &NativeCommand) -> Result<(String, String)> {
    let path = program
        .home
        .clone()
        .or_else(|| std::env::var_os("GROK_HOME").map(PathBuf::from))
        .unwrap_or_else(|| home_dir().join(".grok"))
        .join("auth.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Failure::new(QuotaState::NotSignedIn));
        }
        Err(_) => return Err(Failure::unavailable("grok credentials unreadable")),
    };
    if bytes.len() > 65_536 {
        return Err(Failure::unavailable("grok credentials unreadable"));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Failure::unavailable("grok credentials unreadable"))?;
    let entries = value
        .as_object()
        .ok_or_else(|| Failure::unavailable("grok credentials unreadable"))?;
    let entry = entries
        .iter()
        .find(|(scope, _)| scope.starts_with("https://auth.x.ai::"))
        .or_else(|| entries.iter().find(|(scope, _)| scope.contains("sign-in")))
        .map(|(_, entry)| entry)
        .ok_or_else(|| Failure::new(QuotaState::NotSignedIn))?;
    if let Some(expires) = entry["expires_at"].as_str() {
        let expired = chrono::DateTime::parse_from_rfc3339(expires)
            .map(|stamp| stamp.timestamp_millis() <= i64::try_from(now_ms()).unwrap_or(i64::MAX))
            .unwrap_or(false);
        if expired {
            return Err(Failure::new(QuotaState::NotSignedIn).because("login expired"));
        }
    }
    let token = entry["key"]
        .as_str()
        .filter(|token| !token.is_empty() && token.len() <= 8192)
        .ok_or_else(|| Failure::new(QuotaState::NotSignedIn))?;
    Ok((fingerprint(token), token.to_owned()))
}

fn claude_usage_windows(value: &Value) -> Result<Vec<QuotaWindow>> {
    if !value.is_object() {
        return Err(Failure::new(QuotaState::Invalid));
    }
    let mut windows = Vec::new();
    for (key, label) in [("five_hour", "5h"), ("seven_day", "7d")] {
        let window = &value[key];
        if !window.is_object() {
            continue;
        }
        let (used_basis_points, over_limit) = percent(window.get("utilization"))?;
        if used_basis_points.is_none() && !over_limit {
            continue;
        }
        let resets_unix_ms = reset_ms(window.get("resets_at"))?;
        windows.push(QuotaWindow {
            id: format!("claude/{key}"),
            label: label.into(),
            general: true,
            used_basis_points,
            over_limit,
            resets_unix_ms,
        });
    }
    QuotaReport {
        windows: Some(windows.clone()),
        state: QuotaState::Current,
    }
    .validate()
    .map_err(|_| Failure::new(QuotaState::Invalid))?;
    Ok(windows)
}

fn reset_ms(value: Option<&Value>) -> Result<Option<u64>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    if let Some(text) = value.as_str() {
        let stamp = chrono::DateTime::parse_from_rfc3339(text)
            .map_err(|_| Failure::new(QuotaState::Invalid))?;
        return u64::try_from(stamp.timestamp_millis())
            .map(Some)
            .map_err(|_| Failure::new(QuotaState::Invalid));
    }
    if let Some(seconds) = value.as_u64() {
        return seconds
            .checked_mul(1000)
            .map(Some)
            .ok_or_else(|| Failure::new(QuotaState::Invalid));
    }
    Err(Failure::new(QuotaState::Invalid))
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn fingerprint(token: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in token.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Test override must be loopback HTTP. Anything else keeps the real URL, so a
/// credential cannot be sent to an arbitrary host by setting the variable.
fn loopback_or(var: &str, default: &str) -> String {
    match std::env::var(var) {
        Ok(value)
            if value.starts_with("http://127.0.0.1:") || value.starts_with("http://localhost:") =>
        {
            value
        }
        _ => default.to_owned(),
    }
}

fn http_get(url: &str, headers: &[(&str, &str)]) -> Result<String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(REQUEST)
        .redirects(0)
        .build();
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.set(name, value);
    }
    let response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            let retry = response
                .header("retry-after")
                .and_then(|value| value.parse::<u64>().ok())
                .map(|seconds| Duration::from_secs(seconds.clamp(60, 86_400)));
            let _ = std::io::copy(
                &mut response.into_reader().take(MAX_RESPONSE as u64),
                &mut std::io::sink(),
            );
            let failure = match status {
                401 | 403 => Failure::new(QuotaState::NotSignedIn),
                429 => Failure::unavailable("HTTP 429"),
                100..=599 => Failure::unavailable(format!("HTTP {status}")),
                _ => Failure::unavailable("request failed"),
            };
            return Err(Failure {
                retry_after: retry,
                ..failure
            });
        }
        Err(_) => return Err(Failure::unavailable("request failed")),
    };
    let mut body = String::new();
    response
        .into_reader()
        .take(MAX_RESPONSE as u64)
        .read_to_string(&mut body)
        .map_err(|_| Failure::unavailable("reply unreadable"))?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_state(enabled: bool) -> Arc<ServerState> {
        let state = super::super::tests::test_state(None, None);
        state.settings.lock().unwrap().report.settings.quota.enabled = enabled;
        state
    }

    #[test]
    fn backoff_climbs_one_two_five_ten_and_caps() {
        let minutes = |failures, deterministic, retry_after| {
            backoff(failures, deterministic, retry_after).as_secs() / 60
        };
        let ladder: Vec<_> = (1..=6).map(|n| minutes(n, false, None)).collect();
        assert_eq!(ladder, [1, 2, 5, 10, 10, 10]);
        assert_eq!(minutes(1, true, None), 10, "deterministic goes to the cap");
        let retry = Some(Duration::from_secs(900));
        assert_eq!(minutes(1, false, retry), 15, "Retry-After beats the ladder");
        assert_eq!(minutes(4, true, retry), 15, "Retry-After beats the cap");
    }

    #[test]
    fn native_error_reason_is_classified_without_its_body() {
        let (stream, _dashboard) = std::os::unix::net::UnixStream::pair().unwrap();
        let state = super::super::tests::test_state(
            Some(super::super::DashboardSink::new()),
            Some((Arc::new(()), stream)),
        );
        let root = tempfile::tempdir().unwrap();
        let program = NativeCommand {
            command: "/bin/cat".into(),
            home: None,
        };
        let body = "secret-body account@example.invalid";
        for (error, state_expected, reason, deterministic, retry) in [
            (
                json!({"code":-32000, "message":body, "data":{"status":503, "detail":body}}),
                QuotaState::Unavailable,
                "HTTP 503",
                false,
                None,
            ),
            (
                json!({"code":-32000, "message":body, "data":{"status":429, "retryAfterSeconds":5}}),
                QuotaState::Unavailable,
                "HTTP 429",
                false,
                Some(60),
            ),
            (
                json!({"code":-32000, "message":body, "data":{"status":401}}),
                QuotaState::NotSignedIn,
                "not signed in",
                true,
                None,
            ),
            (
                json!({"code":-32601, "message":body}),
                QuotaState::Unsupported,
                "method not supported",
                true,
                None,
            ),
            (
                json!({"code":-32000, "message":body}),
                QuotaState::Unavailable,
                "request failed",
                false,
                None,
            ),
        ] {
            let mut rpc = Rpc::spawn(QuotaProvider::Codex, &program, &[], root.path())
                .ok()
                .unwrap();
            // /bin/cat echoes the request; the queued error answers it first.
            rpc.buffer = format!("{}\n", json!({"id":1, "error":error})).into_bytes();
            let Err(failure) = rpc.call(
                "account/read",
                json!({}),
                &state,
                Instant::now() + Duration::from_secs(5),
            ) else {
                panic!("error reply accepted")
            };
            assert_eq!(failure.state, state_expected);
            assert_eq!(failure.reason, reason);
            assert_eq!(failure.deterministic, deterministic);
            assert_eq!(failure.retry_after.map(|d| d.as_secs()), retry);
            assert!(!failure.reason.contains("secret") && !failure.reason.contains('@'));
        }
        let mut rpc = Rpc::spawn(QuotaProvider::Codex, &program, &[], root.path())
            .ok()
            .unwrap();
        let Err(failure) = rpc.call("account/read", json!({}), &state, Instant::now()) else {
            panic!("expired request accepted")
        };
        assert_eq!(failure.reason, "timed out after 20 s");
        let missing = NativeCommand {
            command: "ovrcr-no-such-native-client".into(),
            home: None,
        };
        let Err(failure) = Rpc::spawn(QuotaProvider::Grok, &missing, &[], root.path()) else {
            panic!("missing executable spawned")
        };
        assert_eq!(failure.reason, "grok not found on PATH");
        assert!(failure.deterministic);
    }

    #[test]
    fn manual_refresh_has_a_server_cooldown_and_excludes_claude() {
        let off = enabled_state(false);
        assert!(matches!(
            request_refresh(&off, None),
            Response::Error { message, .. } if message == crate::settings::QUOTA_OFF
        ));
        assert!(off.quota_refresh.lock().unwrap().last_ms.is_none());
        let state = enabled_state(true);
        assert!(matches!(
            request_refresh(&state, Some(QuotaProvider::Claude)),
            Response::Error {
                code: ErrorCode::InvalidRequest,
                ..
            }
        ));
        assert_eq!(
            request_refresh(&state, Some(QuotaProvider::Grok)),
            Response::Ok
        );
        assert_eq!(state.quota_refresh.lock().unwrap().due, [false, true]);
        match request_refresh(&state, None) {
            Response::QuotaCooldown { remaining_ms } => {
                assert!((29_000..=30_000).contains(&remaining_ms), "{remaining_ms}")
            }
            other => panic!("refresh inside the cooldown accepted: {other:?}"),
        }
        assert_eq!(state.quota_refresh.lock().unwrap().due, [false, true]);
        state.quota_refresh.lock().unwrap().last_ms = Some(clock_ms().saturating_sub(COOLDOWN_MS));
        assert_eq!(request_refresh(&state, None), Response::Ok);
        assert_eq!(state.quota_refresh.lock().unwrap().due, [true, true]);
    }

    #[test]
    fn disabled_initial_snapshot_is_the_handshake_default() {
        // The hello skips an unchanged snapshot, so the default must say it all.
        assert_eq!(initial(false), QuotaSnapshot::default());
        assert_eq!(
            QuotaSnapshot::default().grok.reason.as_deref(),
            Some(crate::settings::QUOTA_OFF)
        );
        assert_ne!(initial(true), QuotaSnapshot::default());
    }

    #[test]
    fn consent_rows_follow_quota_enabled() {
        let state = enabled_state(false);
        *state.quotas.lock().unwrap() = initial(false);
        assert_eq!(
            state.quotas.lock().unwrap().codex.reason.as_deref(),
            Some(crate::settings::QUOTA_OFF)
        );
        assert!(!sync_consent(&state));
        state.settings.lock().unwrap().report.settings.quota.enabled = true;
        assert!(sync_consent(&state));
        let snapshot = state.quotas.lock().unwrap().clone();
        assert_eq!(
            snapshot.codex,
            ProviderQuota::unknown(QuotaProvider::Codex, QuotaState::Checking)
        );
        assert_eq!(snapshot.grok.state, QuotaState::Checking);
        assert_eq!(snapshot.claude.reason.as_deref(), Some(CLAUDE_WAITING));
        // A late native result after the switch goes off cannot revive the row.
        state.settings.lock().unwrap().report.settings.quota.enabled = false;
        assert!(sync_consent(&state));
        apply(
            &state,
            NativeQuotaUpdate {
                provider: QuotaProvider::Codex,
                generation: 2,
                report: QuotaReport {
                    windows: Some(Vec::new()),
                    state: QuotaState::Current,
                },
                checked: true,
                reason: None,
                next_check_unix_ms: None,
            },
        );
        assert_eq!(
            state.quotas.lock().unwrap().codex.state,
            QuotaState::Disabled
        );
    }

    #[test]
    fn expired_request_does_not_accept_a_buffered_reply() {
        let (stream, _dashboard) = std::os::unix::net::UnixStream::pair().unwrap();
        let state = super::super::tests::test_state(
            Some(super::super::DashboardSink::new()),
            Some((Arc::new(()), stream)),
        );
        let root = tempfile::tempdir().unwrap();
        let program = NativeCommand {
            command: "/bin/cat".into(),
            home: None,
        };
        let mut rpc = Rpc::spawn(QuotaProvider::Codex, &program, &[], root.path())
            .ok()
            .unwrap();
        rpc.buffer = b"{\"id\":1,\"result\":{\"late\":true}}\n".to_vec();
        assert!(
            matches!(rpc.message(&state, Instant::now()), Ok(None)),
            "a complete buffered message bypassed the expired receive deadline"
        );
        let result = rpc.call("account/read", json!({}), &state, Instant::now());
        assert!(
            matches!(
                result,
                Err(Failure {
                    state: QuotaState::Unavailable,
                    ..
                })
            ),
            "a complete buffered reply bypassed the expired request deadline"
        );
    }

    #[test]
    fn native_client_that_does_not_read_cannot_block_request_writes() {
        let (stream, _dashboard) = std::os::unix::net::UnixStream::pair().unwrap();
        let state = super::super::tests::test_state(
            Some(super::super::DashboardSink::new()),
            Some((Arc::new(()), stream)),
        );
        let root = tempfile::tempdir().unwrap();
        let program = NativeCommand {
            command: "/bin/sh".into(),
            home: None,
        };
        let mut rpc = Rpc::spawn(
            QuotaProvider::Codex,
            &program,
            &["-c", "exec sleep 30"],
            root.path(),
        )
        .ok()
        .unwrap();
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done = finished.clone();
        let pgid = rpc.child.id() as libc::pid_t;
        // Release a broken blocking implementation after a second. The Rpc still
        // owns the unreaped leader, and this thread is joined before it can drop.
        let watchdog = std::thread::spawn(move || {
            let limit = Instant::now() + Duration::from_secs(1);
            while !done.load(Ordering::Acquire) {
                if Instant::now() >= limit {
                    unsafe {
                        libc::kill(-pgid, libc::SIGKILL);
                    }
                    break;
                }
                std::thread::park_timeout(Duration::from_millis(10));
            }
        });
        let started = Instant::now();
        let result = rpc.call(
            "account/read",
            json!({"fixture": "x".repeat(131_072)}),
            &state,
            started + Duration::from_millis(100),
        );
        finished.store(true, Ordering::Release);
        watchdog.join().unwrap();
        assert!(matches!(
            result,
            Err(Failure {
                state: QuotaState::Unavailable,
                ..
            })
        ));
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "native stdin write waited for the watchdog instead of its request deadline"
        );
    }

    #[test]
    fn quota_events_record_a_transition_a_refresh_and_a_cooldown_refusal() {
        let secret = "secret-body account@example.invalid";
        let state = enabled_state(true);
        *state.quotas.lock().unwrap() = initial(true);
        apply(
            &state,
            NativeQuotaUpdate {
                provider: QuotaProvider::Codex,
                generation: 1,
                report: QuotaReport {
                    windows: None,
                    state: QuotaState::Unavailable,
                },
                checked: true,
                reason: Some("HTTP 503".into()),
                next_check_unix_ms: None,
            },
        );
        assert_eq!(
            request_refresh(&state, Some(QuotaProvider::Codex)),
            Response::Ok
        );
        assert!(matches!(
            request_refresh(&state, Some(QuotaProvider::Codex)),
            Response::QuotaCooldown { .. }
        ));
        let events = state.event_snapshot();
        let messages: Vec<_> = events.iter().map(|event| event.message.as_str()).collect();
        assert!(
            messages.contains(&"Checking -> Unavailable: HTTP 503"),
            "{messages:?}"
        );
        assert!(messages.contains(&"refresh"));
        assert!(messages.contains(&"cooldown refusal"));
        let rendered = messages.join(
            "
",
        );
        assert!(!rendered.contains(secret));
        assert!(!rendered.contains('@'));
        assert!(
            events
                .iter()
                .all(|event| event.component == ovrcr_protocol::EventComponent::Quota)
        );
    }
}

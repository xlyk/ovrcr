//! Server-owned native account collection. No credentials or native bodies cross this boundary.
use super::{DispatchMessage, ServerState};
use ovrcr_protocol::{
    ProviderQuota, QuotaProvider, QuotaReport, QuotaSource, QuotaState, QuotaWindow,
};
use serde::Deserialize;
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

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NativeCommand {
    command: PathBuf,
    home: Option<PathBuf>,
}
impl Default for NativeCommand {
    fn default() -> Self {
        Self {
            command: PathBuf::from("codex"),
            home: None,
        }
    }
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    // Native clients may refresh their own auth/logs: require explicit consent.
    enabled: bool,
    codex: NativeCommand,
    grok: NativeCommand,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            codex: NativeCommand::default(),
            grok: NativeCommand {
                command: "grok".into(),
                home: None,
            },
        }
    }
}
fn config(path: &Path) -> Config {
    #[derive(Deserialize)]
    struct Settings {
        #[serde(default)]
        quota: Config,
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str::<Settings>(&text).ok())
        .map(|settings| settings.quota)
        .unwrap_or_default()
}

pub struct NativeQuotaUpdate {
    pub(super) provider: QuotaProvider,
    pub(super) generation: u64,
    pub(super) report: QuotaReport,
    pub(super) checked: bool,
}

pub(super) fn apply(state: &ServerState, update: NativeQuotaUpdate) {
    if update.generation == 0 || update.report.validate().is_err() {
        return;
    }
    let mut snapshots = state.quotas.lock().unwrap();
    let target = match update.provider {
        QuotaProvider::Codex => &mut snapshots.codex,
        QuotaProvider::Grok => &mut snapshots.grok,
        QuotaProvider::Claude => return,
    };
    let old_generation = match &target.source {
        Some(QuotaSource::NativeProfile { generation, .. }) => *generation,
        _ => 0,
    };
    if update.generation < old_generation {
        return;
    }
    if update.generation != old_generation {
        *target = ProviderQuota::unknown(update.provider, QuotaState::Waiting);
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
    if update.checked {
        target.checked_unix_ms = Some(now_ms());
    }
    drop(snapshots);
    state.publish_quotas();
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn run(state: Arc<ServerState>, provider: QuotaProvider) {
    let config = config(&state.registry_path);
    if !config.enabled {
        return;
    }
    let program = match provider {
        QuotaProvider::Codex => &config.codex,
        QuotaProvider::Grok => &config.grok,
        _ => return,
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
    let mut failures = 0u32;
    let mut pending = None;
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
        if !state.dashboard.is_claimed() {
            client = None;
            std::thread::park_timeout(Duration::from_millis(100));
            continue;
        }
        let mut checked = false;
        let result = if Instant::now() >= due {
            checked = true;
            (|| {
                let deadline = Instant::now() + REQUEST;
                if client.is_none() {
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
                            state: QuotaState::Waiting,
                        },
                        checked: false,
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
                    return Err(Failure::new(QuotaState::SourceConflict));
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
                            Err(Failure::new(QuotaState::SourceConflict))
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
        let (report, successful) = match result {
            Ok(windows) => {
                failures = 0;
                if checked {
                    due = Instant::now() + POLL;
                }
                (
                    QuotaReport {
                        windows: Some(windows),
                        state: QuotaState::Current,
                    },
                    true,
                )
            }
            Err(error) => {
                if error.state != QuotaState::Waiting {
                    failures = failures.saturating_add(1);
                    due = Instant::now()
                        + error
                            .retry_after
                            .unwrap_or(POLL * (1u32 << failures.min(3)));
                    client = None;
                }
                (
                    QuotaReport {
                        windows: None,
                        state: error.state,
                    },
                    false,
                )
            }
        };
        pending = Some(Box::new(NativeQuotaUpdate {
            provider,
            generation,
            report,
            checked: checked && successful,
        }));
    }
}

struct Failure {
    state: QuotaState,
    retry_after: Option<Duration>,
}
impl Failure {
    fn new(state: QuotaState) -> Self {
        Self {
            state,
            retry_after: None,
        }
    }
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
        let mut child = command
            .spawn()
            .map_err(|_| Failure::new(QuotaState::Unavailable))?;
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
                return Err(Failure::new(QuotaState::Unavailable));
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
                return Err(Failure::new(QuotaState::Unavailable));
            }
            if Instant::now() >= deadline {
                return Ok(true);
            }
            let mut chunk = [0u8; 16_384];
            match self.output.read(&mut chunk) {
                Ok(0) => return Ok(false),
                Ok(count) => {
                    if self.buffer.len() + count > MAX_RESPONSE {
                        return Err(Failure::new(QuotaState::Invalid));
                    }
                    self.buffer.extend_from_slice(&chunk[..count]);
                    return Ok(true);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(Failure::new(QuotaState::Unavailable)),
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
                return Err(Failure::new(QuotaState::Unavailable));
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
                return Err(Failure::new(QuotaState::Unavailable));
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
                || Instant::now() >= deadline
            {
                return Err(Failure::new(QuotaState::Unavailable));
            }
            match self.input.write(remaining) {
                Ok(0) => return Err(Failure::new(QuotaState::Unavailable)),
                Ok(count) => remaining = &remaining[count..],
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::park_timeout(Duration::from_millis(10));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(Failure::new(QuotaState::Unavailable)),
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
                let state = match error["code"].as_i64() {
                    Some(-32601) => QuotaState::Unsupported,
                    _ if error["data"]["status"].as_u64() == Some(401) => QuotaState::NotSignedIn,
                    _ => QuotaState::Unavailable,
                };
                let retry_after = error["data"]["retryAfterSeconds"]
                    .as_u64()
                    .map(|seconds| Duration::from_secs(seconds.clamp(300, 86_400)));
                return Err(Failure { state, retry_after });
            }
            return reply
                .get("result")
                .cloned()
                .ok_or_else(|| Failure::new(QuotaState::Invalid));
        }
        Err(Failure::new(QuotaState::Unavailable))
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
    let workspace = tempfile::tempdir().map_err(|_| Failure::new(QuotaState::Unavailable))?;
    let mut version = Rpc::spawn(provider, program, &["--version"], workspace.path())?;
    while version.bytes(state, deadline)? {
        if Instant::now() >= deadline {
            return Err(Failure::new(QuotaState::Unavailable));
        }
    }
    let version =
        std::str::from_utf8(&version.buffer).map_err(|_| Failure::new(QuotaState::Unsupported))?;
    let supported = if provider == QuotaProvider::Codex {
        "0.155.1"
    } else {
        "1.0.40"
    };
    if !version.split_whitespace().any(|part| part == supported) {
        return Err(Failure::new(QuotaState::Unsupported));
    }
    let args: &[&str] = if provider == QuotaProvider::Codex {
        &["app-server"]
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
        return Err(Failure::new(QuotaState::Unsupported));
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
        return Err(Failure::new(QuotaState::Unsupported));
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

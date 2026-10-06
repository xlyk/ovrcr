//! The hidden Claude quota probe. It is a Server-owned process, never a Session.
use super::quota;
use super::{DispatchMessage, ServerState};
use ovrcr_protocol::{
    AgentProvider, PROBE_EXITED, PROBE_MISSING_USAGE, PROBE_TIMED_OUT, QuotaReport, QuotaSource,
    QuotaState, QuotaWindow,
};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::TrySendError;
use std::time::{Duration, Instant};

const PROBE_LIMIT: Duration = Duration::from_secs(60);

pub(super) struct ProbeUpdate {
    pub(super) report: QuotaReport,
    pub(super) reason: Option<String>,
    pub(super) probed_unix_ms: u64,
}

/// A live managed Claude session has reported inside the stale boundary.
pub(super) fn fresh_managed_claude(state: &ServerState, now: u64) -> bool {
    let sessions: Vec<_> = state.sessions.lock().unwrap().values().cloned().collect();
    sessions.into_iter().any(|session| {
        let summary = session.summary();
        let claude = summary.agent.as_ref().is_some_and(|agent| {
            agent.binding.provider == AgentProvider::Claude && summary.phase.is_live()
        });
        claude
            && session
                .quota_snapshot()
                .is_some_and(|quota| !quota.stale(now))
    })
}

pub(super) fn apply(state: &ServerState, update: ProbeUpdate) {
    if !state.quota_settings().claude_probe {
        return;
    }
    let now = quota::clock_ms();
    if fresh_managed_claude(state, now) {
        return;
    }
    if update.report.validate().is_err() && update.report.state == QuotaState::Current {
        return;
    }
    let fresh_session_row = {
        let claude = &state.quotas.lock().unwrap().claude;
        matches!(claude.source, Some(QuotaSource::Session { .. })) && !claude.stale(now)
    };
    if fresh_session_row {
        return;
    }
    state.quota_refresh.lock().unwrap().claude_probe = Some(super::claude_allowance::ProbeSignal {
        report: update.report,
        reason: update.reason,
        probed_unix_ms: update.probed_unix_ms,
    });
    state.refresh_claude_quota();
}

pub(super) fn run(state: Arc<ServerState>) {
    let mut was_attached = false;
    let mut was_enabled = false;
    let mut last_probe: Option<u64> = None;
    let mut pending: Option<ProbeUpdate> = None;
    let mut auth_dirty = false;
    while !state.shutdown.load(Ordering::Acquire) && !state.stopping.load(Ordering::Acquire) {
        if let Some(update) = pending.take() {
            match state.dispatch.try_send(DispatchMessage::ClaudeProbe {
                report: update.report.clone(),
                reason: update.reason.clone(),
                probed_unix_ms: update.probed_unix_ms,
            }) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => pending = Some(update),
                Err(_) => break,
            }
        }
        if auth_dirty {
            match state.dispatch.try_send(DispatchMessage::ClaudeAuth) {
                Ok(()) => auth_dirty = false,
                Err(TrySendError::Full(_)) => {}
                Err(_) => break,
            }
        }
        if pending.is_some() || auth_dirty {
            std::thread::park_timeout(Duration::from_millis(20));
            continue;
        }
        let enabled = state.quota_settings().claude_probe;
        if was_enabled && !enabled {
            auth_dirty = true;
        }
        let attached = state.dashboard.is_claimed();
        let now = quota::clock_ms();
        let signed_in = {
            let auth = state.quota_refresh.lock().unwrap();
            auth.auth_checked && auth.claude_auth.is_none()
        };
        let rising_attach = attached && !was_attached;
        let rising_enable = enabled && !was_enabled;
        was_attached = attached;
        was_enabled = enabled;
        let row_stale = state.quotas.lock().unwrap().claude.stale(now);
        let fresh = fresh_managed_claude(&state, now);
        if fresh {
            state.quota_refresh.lock().unwrap().claude_probe_due = false;
        }
        // A manual refresh asked for while detached stays due until one attaches.
        let can = attached && enabled && signed_in && !fresh;
        let manual =
            can && std::mem::take(&mut state.quota_refresh.lock().unwrap().claude_probe_due);
        let cadence = last_probe.is_some_and(|stamp| {
            now.saturating_sub(stamp) >= super::claude_allowance::PROBE_EVERY_MS
        });
        let due = manual
            || ((rising_attach || rising_enable || last_probe.is_none() || cadence) && row_stale);
        if can && due {
            let probed = quota::clock_ms();
            match run_one(&state) {
                Run::Finished(update) => {
                    last_probe = Some(probed);
                    // This probe answers a refresh that arrived while it ran.
                    state.quota_refresh.lock().unwrap().claude_probe_due = false;
                    pending = Some(update);
                }
                Run::Aborted => {}
            }
        }
        std::thread::park_timeout(Duration::from_millis(100));
    }
}

enum Run {
    Finished(ProbeUpdate),
    Aborted,
}

fn run_one(state: &ServerState) -> Run {
    let command = quota::claude_command(&state.settings.lock().unwrap().report.settings);
    let instance = state
        .socket
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let directory = instance.join("claude-quota-probe");
    let settings_path = instance.join("claude-quota-probe.json");
    let socket_path = instance.join("claude-quota-probe.sock");
    let _ = std::fs::remove_file(&socket_path);
    if let Err(reason) = prepare_directory(&directory) {
        return Run::Finished(failure(reason));
    }
    let token = match probe_token() {
        Ok(token) => token,
        Err(reason) => return Run::Finished(failure(reason)),
    };
    let listener = match std::os::unix::net::UnixListener::bind(&socket_path) {
        Ok(listener) => listener,
        Err(_) => return Run::Finished(failure("claude could not start")),
    };
    let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o700));
    let _ = listener.set_nonblocking(true);
    let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ovrcr"));
    let status_line = format!(
        "{} report claude-statusline --stdin-json",
        shell_quote(&executable.display().to_string())
    );
    let document = json!({
        "statusLine": {"type": "command", "command": status_line}
    });
    if std::fs::write(&settings_path, document.to_string()).is_err() {
        return Run::Finished(failure("claude could not start"));
    }
    let directory = directory.canonicalize().unwrap_or(directory);
    let outcome = spawn_and_wait(
        state,
        &command,
        &directory,
        &settings_path,
        &socket_path,
        &token,
        &listener,
    );
    let _ = std::fs::remove_file(&socket_path);
    outcome
}

fn prepare_directory(directory: &Path) -> Result<(), &'static str> {
    if directory.exists() {
        std::fs::remove_dir_all(directory).map_err(|_| "claude could not start")?;
    }
    std::fs::create_dir_all(directory).map_err(|_| "claude could not start")?;
    Ok(())
}

fn probe_token() -> Result<String, &'static str> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes))
        .map_err(|_| "claude could not start")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn failure(reason: &'static str) -> ProbeUpdate {
    ProbeUpdate {
        report: QuotaReport {
            windows: None,
            state: QuotaState::Unavailable,
        },
        reason: Some(reason.into()),
        probed_unix_ms: quota::clock_ms(),
    }
}

fn spawn_and_wait(
    state: &ServerState,
    command: &Path,
    directory: &Path,
    settings_path: &Path,
    socket_path: &Path,
    token: &str,
    listener: &std::os::unix::net::UnixListener,
) -> Run {
    let pty = native_pty_system();
    let pair = match pty.openpty(PtySize {
        rows: 24,
        cols: 200,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(_) => return Run::Finished(failure("claude could not start")),
    };
    let mut child_cmd = CommandBuilder::new(command);
    child_cmd.arg("--model");
    child_cmd.arg("haiku");
    child_cmd.arg("--tools");
    child_cmd.arg("");
    child_cmd.arg("--permission-mode");
    child_cmd.arg("dontAsk");
    // Empty setting sources: do not load the user's own settings files.
    child_cmd.arg("--setting-sources");
    child_cmd.arg("");
    child_cmd.arg("--settings");
    child_cmd.arg(settings_path);
    child_cmd.arg("Reply with OK.");
    child_cmd.cwd(directory);
    for name in [
        "OVRCR_HOOK_SOCKET",
        "OVRCR_HOOK_TOKEN",
        "OVRCR_SESSION_ID",
        "OVRCR_AGENT_SOCKET",
        "OVRCR_AGENT_TOKEN",
    ] {
        child_cmd.env_remove(name);
    }
    // The probe identity. The status-line callback is admitted on this socket
    // only, and it is not a Session.
    child_cmd.env("OVRCR_QUOTA_PROBE", "claude");
    child_cmd.env("OVRCR_AGENT_SOCKET", socket_path);
    child_cmd.env("OVRCR_AGENT_TOKEN", token);
    let mut child = match pair.slave.spawn_command(child_cmd) {
        Ok(child) => child,
        Err(error) => {
            let not_found = error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound);
            let reason = if !not_found {
                "claude could not start"
            } else if command.components().count() > 1 {
                "claude not found at the configured path"
            } else {
                "claude not found on PATH"
            };
            return Run::Finished(failure(reason));
        }
    };
    let reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(_) => {
            end_child(&mut child);
            return Run::Finished(failure("claude could not start"));
        }
    };
    let mut writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(_) => {
            end_child(&mut child);
            return Run::Finished(failure("claude could not start"));
        }
    };
    let (output_tx, output_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    if output_tx.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let deadline = Instant::now() + PROBE_LIMIT;
    let mut seen = String::new();
    let mut answered = false;
    let mut callback: Option<ProbeUpdate> = None;
    let mut received_callback = false;
    let mut exited = false;
    let probe_path = directory.display().to_string();
    while Instant::now() < deadline && callback.is_none() && !exited {
        if state.stopping.load(Ordering::Acquire)
            || state.shutdown.load(Ordering::Acquire)
            || !state.dashboard.is_claimed()
            || !state.quota_settings().claude_probe
        {
            end_child(&mut child);
            return Run::Aborted;
        }
        while let Ok(chunk) = output_rx.try_recv() {
            seen.push_str(&String::from_utf8_lossy(&chunk));
        }
        if !answered && trust_dialog_for(&seen, &probe_path) {
            let _ = writer.write_all(b"\x1b[B\r");
            let _ = writer.flush();
            answered = true;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if let Some(update) = read_callback(stream, token) {
                    received_callback = true;
                    // Claude's startup callback precedes its first API response.
                    // Keep the same probe alive until it supplies usage.
                    if update.report.state != QuotaState::Checking {
                        callback = Some(update);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => {}
        }
        match child.try_wait() {
            Ok(Some(_)) => exited = true,
            Ok(None) => {}
            Err(_) => exited = true,
        }
        if callback.is_none() && !exited {
            std::thread::park_timeout(Duration::from_millis(20));
        }
    }
    end_child(&mut child);
    if !state.dashboard.is_claimed() || !state.quota_settings().claude_probe {
        return Run::Aborted;
    }
    if let Some(update) = callback {
        return Run::Finished(update);
    }
    if received_callback {
        return Run::Finished(classified(QuotaState::Unavailable, PROBE_MISSING_USAGE));
    }
    if Instant::now() >= deadline {
        return Run::Finished(classified(QuotaState::Unavailable, PROBE_TIMED_OUT));
    }
    Run::Finished(classified(QuotaState::Unavailable, PROBE_EXITED))
}

fn trust_dialog_for(seen: &str, probe_path: &str) -> bool {
    seen.contains("Yes, I trust this folder") && seen.contains(probe_path)
}

fn end_child(child: &mut Box<dyn portable_pty::Child + Send + Sync>) {
    let pid = child.process_id().unwrap_or(0);
    let _ = child.kill();
    if pid > 0 {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
            libc::kill(pid as i32, libc::SIGKILL);
        }
    }
    let _ = child.wait();
}

fn read_callback(mut stream: std::os::unix::net::UnixStream, token: &str) -> Option<ProbeUpdate> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if line.len() > 80 {
            return None;
        }
        if stream.read(&mut byte).ok()? == 0 {
            return None;
        }
        if byte[0] == b'\n' {
            break;
        }
        line.push(byte[0]);
    }
    if line != token.as_bytes() {
        return None;
    }
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).ok()?;
    let length = u32::from_be_bytes(length) as usize;
    if length > 65_536 {
        return None;
    }
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body).ok()?;
    let _ = stream.write_all(b"admission-accepted\n");
    let value: Value = serde_json::from_slice(&body).ok()?;
    if value.get("origin").and_then(|v| v.as_str()) != Some("claude-statusline") {
        return None;
    }
    let payload = value.get("payload")?;
    Some(report_from_payload(payload))
}

fn report_from_payload(payload: &Value) -> ProbeUpdate {
    if payload.get("rate_limits").is_none() || payload["rate_limits"].is_null() {
        return classified(QuotaState::Checking, PROBE_MISSING_USAGE);
    }
    match parse_rate_limits(&payload["rate_limits"]) {
        Some(windows)
            if !windows
                .iter()
                .any(|window| window.used_basis_points.is_some() || window.over_limit) =>
        {
            classified(QuotaState::Checking, PROBE_MISSING_USAGE)
        }
        Some(windows) => ProbeUpdate {
            report: QuotaReport {
                windows: Some(windows),
                state: QuotaState::Current,
            },
            reason: None,
            probed_unix_ms: quota::clock_ms(),
        },
        None => classified(QuotaState::Invalid, "invalid report"),
    }
}

fn classified(state: QuotaState, reason: &str) -> ProbeUpdate {
    ProbeUpdate {
        report: QuotaReport {
            windows: None,
            state,
        },
        reason: Some(reason.into()),
        probed_unix_ms: quota::clock_ms(),
    }
}

/// `five_hour` and `seven_day` only, matching the managed status-line parser.
fn parse_rate_limits(value: &Value) -> Option<Vec<QuotaWindow>> {
    let object = value.as_object()?;
    let mut windows = Vec::new();
    for (id, label) in [("five_hour", "5h"), ("seven_day", "7d")] {
        let Some(window) = object.get(id).filter(|v| !v.is_null()) else {
            continue;
        };
        let window = window.as_object()?;
        let used = match window.get("used_percentage").filter(|v| !v.is_null()) {
            Some(value) => Some(value.as_f64()?),
            None => None,
        };
        if used.is_some_and(|value| !value.is_finite() || value < 0.0) {
            return None;
        }
        let over_limit = used.is_some_and(|value| value > 100.0);
        let resets_unix_ms = match window.get("resets_at").filter(|v| !v.is_null()) {
            Some(value) => Some(value.as_u64()?.checked_mul(1_000)?),
            None => None,
        };
        windows.push(QuotaWindow {
            id: id.into(),
            label: label.into(),
            general: true,
            used_basis_points: used
                .filter(|_| !over_limit)
                .map(|value| (value * 100.0).floor() as u16),
            over_limit,
            resets_unix_ms,
        });
    }
    Some(windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_callbacks_wait_for_usable_subscription_usage() {
        for payload in [
            json!({}),
            json!({"rate_limits": null}),
            json!({"rate_limits": {}}),
            json!({"rate_limits": {"five_hour": null, "seven_day": null}}),
            json!({"rate_limits": {"five_hour": {}, "seven_day": {"used_percentage": null}}}),
            json!({"rate_limits": {"seven_day": {"resets_at": 1800000000}}}),
            json!({"rate_limits": {"spend_limit": {"used_percentage": 10}}}),
        ] {
            let update = report_from_payload(&payload);
            assert_eq!(update.report.state, QuotaState::Checking, "{payload}");
            assert_eq!(update.report.windows, None);
            assert_eq!(update.reason.as_deref(), Some(PROBE_MISSING_USAGE));
        }
    }

    #[test]
    fn usable_subscription_usage_preserves_partial_zero_exhausted_and_over_limit() {
        for (used, expected, over_limit) in [
            (0.0, Some(0), false),
            (12.345, Some(1234), false),
            (100.0, Some(10000), false),
            (101.0, None, true),
        ] {
            let update = report_from_payload(&json!({"rate_limits": {
                "seven_day": {"used_percentage": used, "resets_at": 1800000000}
            }}));
            assert_eq!(update.report.state, QuotaState::Current);
            assert_eq!(update.reason, None);
            update.report.validate().unwrap();
            let windows = update.report.windows.unwrap();
            assert_eq!(windows.len(), 1);
            assert_eq!(windows[0].id, "seven_day");
            assert_eq!(windows[0].used_basis_points, expected);
            assert_eq!(windows[0].over_limit, over_limit);
            assert_eq!(windows[0].resets_unix_ms, Some(1800000000000));
        }
    }

    #[test]
    fn malformed_subscription_usage_is_invalid() {
        for rates in [
            json!([]),
            json!({"five_hour": 1}),
            json!({"five_hour": {"used_percentage": -1}}),
            json!({"five_hour": {"used_percentage": "12"}}),
            json!({"five_hour": {"used_percentage": 12, "resets_at": -1}}),
            json!({"five_hour": {"used_percentage": 12, "resets_at": u64::MAX}}),
            json!({"five_hour": {"used_percentage": 12}, "seven_day": false}),
        ] {
            let update = report_from_payload(&json!({"rate_limits": rates}));
            assert_eq!(update.report.state, QuotaState::Invalid);
            assert_eq!(update.report.windows, None);
            assert_eq!(update.reason.as_deref(), Some("invalid report"));
        }
    }
}

use super::{AppResult, RuntimeError};
use ovrcr::protocol::ReporterHealth;
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;

const MARKER: &str = ": ovrcr-managed-claude-v1; ";
const HOOKS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Notification",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

pub(super) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn settings(path: Option<&Path>) -> anyhow::Result<Value> {
    let Some(path) = path else {
        return Ok(json!({}));
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1_048_577)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 1_048_576, "settings file exceeds 1 MiB");
    let value: Value = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(value.is_object(), "settings must be a JSON object");
    Ok(value)
}

pub(super) fn binary() -> anyhow::Result<String> {
    std::env::current_exe()?
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("OVRCR executable path is not UTF-8"))
}

const REPORTERS: &[&str] = &[
    " report claude-statusline",
    " report claude-context",
    " report claude",
    " report codex",
];

/// Replace the `ovrcr` token in existing managed reporter commands with
/// `executable`. A command that already names that file is left byte-for-byte,
/// so a Codex trust hash does not change when `just run` only refreshes the
/// installed binary. Unrelated commands are untouched.
pub(super) fn retarget_reporter_text(text: &str, executable: &str) -> String {
    let quoted = quote(executable);
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(at) = next_reporter(&text[cursor..]) {
        let abs = cursor + at;
        match preceding_word(&text[..abs]) {
            Some((start, end, token))
                if start >= cursor
                    && Path::new(&token)
                        .file_name()
                        .is_some_and(|name| name == "ovrcr")
                    && !same_binary(&token, executable) =>
            {
                out.push_str(&text[cursor..start]);
                out.push_str(&quoted);
                cursor = end;
            }
            _ => {
                out.push_str(&text[cursor..abs + 1]);
                cursor = abs + 1;
            }
        }
    }
    out.push_str(&text[cursor..]);
    out
}

fn next_reporter(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut best = None;
    for kind in REPORTERS {
        let mut from = 0;
        while let Some(offset) = text[from..].find(kind) {
            let at = from + offset;
            let after = at + kind.len();
            let boundary = bytes
                .get(after)
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-');
            if boundary {
                best = Some(best.map_or(at, |found: usize| found.min(at)));
                break;
            }
            from = at + 1;
        }
    }
    best
}

fn preceding_word(prefix: &str) -> Option<(usize, usize, String)> {
    let bytes = prefix.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end == 0 {
        return None;
    }
    let mut start = end;
    while start > 0 && !bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }
    let (word_end, token) = read_word(prefix, start)?;
    (word_end == end).then_some((start, end, token))
}

fn read_word(text: &str, start: usize) -> Option<(usize, String)> {
    let bytes = text.as_bytes();
    let mut index = start;
    let mut token = String::new();
    while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
        match bytes[index] {
            b'\'' | b'"' => {
                let quote = bytes[index];
                let close = text[index + 1..].find(quote as char)?;
                token.push_str(&text[index + 1..index + 1 + close]);
                index += close + 2;
            }
            byte => {
                token.push(byte as char);
                index += 1;
            }
        }
    }
    Some((index, token))
}

fn same_binary(token: &str, executable: &str) -> bool {
    token == executable
        || matches!(
            (std::fs::canonicalize(token), std::fs::canonicalize(executable)),
            (Ok(left), Ok(right)) if left == right
        )
}

/// Rewrite managed reporter commands in `path` to `executable`. Returns whether
/// the file changed. Missing files are unchanged. Oversized or non-UTF-8 files
/// are left untouched and reported.
pub(super) fn retarget_file(path: &Path, executable: &str) -> anyhow::Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1_048_577)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 1_048_576, "settings file exceeds 1 MiB");
    let text = String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("settings are not UTF-8"))?;
    let next = retarget_reporter_text(&text, executable);
    if next == text {
        return Ok(false);
    }
    let mode = std::fs::metadata(path)?.permissions();
    let tmp = path.with_file_name(format!(
        ".{}.ovrcr-retarget",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("settings")
    ));
    std::fs::write(&tmp, next.as_bytes())?;
    std::fs::set_permissions(&tmp, mode)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

pub(super) fn default_hook_files() -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(entries) = std::fs::read_dir(home)
    {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name != ".claude"
                && !name.starts_with(".claude-")
                && name != ".codex"
                && name != ".grok"
            {
                continue;
            }
            for file in [
                "settings.json",
                "settings.local.json",
                "config.toml",
                "hooks.json",
            ] {
                let path = entry.path().join(file);
                if path.is_file() {
                    files.push(path);
                }
            }
        }
    }
    for (var, names) in [
        ("CLAUDE_CONFIG_DIR", &["settings.json"][..]),
        ("CODEX_HOME", &["config.toml", "hooks.json"][..]),
        ("GROK_HOME", &["config.toml", "settings.json"][..]),
    ] {
        if let Some(dir) = std::env::var_os(var) {
            for name in names {
                let path = std::path::PathBuf::from(&dir).join(name);
                if path.is_file() {
                    files.push(path);
                }
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

pub(super) fn retarget_hooks(executable: &str, files: &[std::path::PathBuf]) -> AppResult<()> {
    let mut changed = Vec::new();
    for path in files {
        match retarget_file(path, executable) {
            Ok(true) => changed.push(path.display().to_string()),
            Ok(false) => {}
            Err(error) => {
                return Err(RuntimeError::internal(anyhow::anyhow!(
                    "{}: {:#}",
                    path.display(),
                    error
                )));
            }
        }
    }
    if changed.is_empty() {
        eprintln!("retarget hooks: managed reporter commands already name {executable}");
    } else {
        for path in &changed {
            eprintln!("retarget hooks: updated {path}");
        }
    }
    Ok(())
}

fn exact(command: &str, executable: &str, report: &str) -> bool {
    [
        "ovrcr".to_string(),
        executable.to_string(),
        quote(executable),
    ]
    .iter()
    .any(|prefix| command == format!("{prefix} report {report} --stdin-json"))
}

// Recognize the exact command emitted by setup, including its single quoted
// renderer argument. Other shell syntax is deliberately left unverified.
fn supported_statusline(command: &str, executable: &str) -> bool {
    let default = format!(
        "{MARKER}{} report claude-statusline --stdin-json",
        quote(executable)
    );
    if command == default || exact(command, executable, "claude-statusline") {
        return true;
    }
    command
        .strip_prefix(&format!("{default} --render-command "))
        .and_then(|argument| argument.strip_prefix('\'')?.strip_suffix('\''))
        .is_some_and(|inner| !inner.replace("'\"'\"'", "").contains('\''))
}

pub(super) fn setup(path: Option<&Path>) -> AppResult<()> {
    let result = (|| -> anyhow::Result<Value> {
        let mut value = settings(path)?;
        let executable = binary()?;
        let command = format!("{MARKER}{} report claude --stdin-json", quote(&executable));
        let hooks = value
            .as_object_mut()
            .unwrap()
            .entry("hooks")
            .or_insert_with(|| json!({}));
        let hooks = hooks
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("hooks must be an object"))?;
        for event in HOOKS {
            let groups = hooks
                .entry(*event)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("hook event must be an array"))?;
            // Replace only exact old OVRCR reporters or entries bearing our marker.
            groups.retain_mut(|group| {
                let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                    return true;
                };
                let originally_empty = handlers.is_empty();
                handlers.retain(|handler| {
                    !handler
                        .get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|text| {
                            text.starts_with(MARKER) || exact(text, &executable, "claude")
                        })
                });
                originally_empty || !handlers.is_empty()
            });
            groups.push(json!({"hooks":[{"type":"command", "command":command, "timeout":2, "async":false}]}));
        }
        let renderer = value
            .get("statusLine")
            .and_then(|line| line.get("command"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let default = format!(
            "{MARKER}{} report claude-statusline --stdin-json",
            quote(&executable)
        );
        match renderer {
            Some(ref renderer) if renderer.starts_with(MARKER) => {}
            Some(ref renderer)
                if renderer.contains("claude-context")
                    && !exact(renderer, &executable, "claude-context") =>
            {
                eprintln!(
                    "Manual migration required: statusLine contains a wrapper around the legacy claude-context reporter and was left untouched. Edit that wrapper to remove its reporting call, then rerun setup with a renderer-only command; or replace statusLine.command with: {default}"
                );
            }
            renderer => {
                let composed = match renderer {
                    Some(renderer)
                        if !exact(&renderer, &executable, "claude-context")
                            && !exact(&renderer, &executable, "claude-statusline") =>
                    {
                        format!("{default} --render-command {}", quote(&renderer))
                    }
                    _ => default,
                };
                let status = value
                    .as_object_mut()
                    .unwrap()
                    .entry("statusLine")
                    .or_insert_with(|| json!({}));
                let status = status
                    .as_object_mut()
                    .ok_or_else(|| anyhow::anyhow!("statusLine must be an object"))?;
                status.insert("type".into(), json!("command"));
                status.insert("command".into(), json!(composed));
            }
        }
        eprintln!(
            "Review the printed JSON and merge it into the intended Claude settings file. No file was written. Use synchronous command hooks with stable Claude Code >=2.1.267; supplied settings do not prove effective enterprise/plugin configuration. Fresh launcher inside an OVRCR session: {} agent run --provider claude -- claude. Initial resume launcher throughout the range: {} agent run --provider claude -- claude --resume UUID. Claude Code 2.1.268 and later compatible patches also support the exact separate-token form claude -r UUID",
            quote(&executable),
            quote(&executable)
        );
        eprintln!(
            "Removal: remove only hook handlers whose command starts with `{MARKER}`; preserve other handlers. Restore your previous statusLine command from the supplied input (the --render-command argument), or remove the marked default statusLine. Keep all permission/trust settings unchanged. Arbitrary render commands are preserved as one quoted argument."
        );
        Ok(value)
    })();
    let value = result.map_err(RuntimeError::internal)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(RuntimeError::internal)?
    );
    Ok(())
}

/// Hook events and status line a supplied Claude settings object fails to report through.
fn configuration_issues(value: &Value, executable: &str) -> Vec<String> {
    let mut issues = Vec::new();
    for event in HOOKS {
        let mut present = false;
        if let Some(groups) = value
            .get("hooks")
            .and_then(|hooks| hooks.get(*event))
            .and_then(Value::as_array)
        {
            for group in groups {
                let all_events = group
                    .get("matcher")
                    .is_none_or(|matcher| matches!(matcher.as_str(), Some("" | "*")));
                if let Some(handlers) = group.get("hooks").and_then(Value::as_array) {
                    for handler in handlers {
                        let command = handler.get("command").and_then(Value::as_str).unwrap_or("");
                        if command.contains("report claude") {
                            if handler.get("async").and_then(Value::as_bool) == Some(true) {
                                issues.push(format!("{event}:asynchronous_reporting_unsupported"));
                            }
                            if (command
                                == format!(
                                    "{MARKER}{} report claude --stdin-json",
                                    quote(executable)
                                )
                                || exact(command, executable, "claude"))
                                && all_events
                                && handler.get("type").and_then(Value::as_str) == Some("command")
                                && handler.get("async").is_none_or(|value| value == false)
                            {
                                present = true;
                            }
                        }
                    }
                }
            }
        }
        if !present {
            issues.push(format!("{event}:synchronous_reporter_missing"));
        }
    }
    let status = value
        .get("statusLine")
        .and_then(|line| line.get("command"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if value
        .get("statusLine")
        .and_then(|line| line.get("type"))
        .and_then(Value::as_str)
        != Some("command")
        || !supported_statusline(status, executable)
    {
        issues.push("statusline_reporting_unverified".into());
    }
    issues
}

/// Dashboard boot check: Claude or Codex on PATH whose settings lack the synchronous
/// OVRCR reporters. Configuration presence is not delivery or trust; doctor keeps those
/// unverified until evidence arrives. Warnings persist in the Dashboard banner.
pub(super) fn boot_hook_warning() -> Option<String> {
    let mut warnings = Vec::new();
    if let Some(warning) = claude_boot_hook_warning() {
        warnings.push(warning);
    }
    if let Some(warning) = super::codex_setup::boot_hook_warning() {
        warnings.push(warning);
    }
    (!warnings.is_empty()).then(|| warnings.join(" · "))
}

fn claude_boot_hook_warning() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find(|dir| dir.join("claude").is_file())?;
    let settings_path = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| Some(std::path::PathBuf::from(std::env::var_os("HOME")?).join(".claude")))?
        .join("settings.json");
    let issues = match settings(Some(&settings_path)) {
        Ok(value) => configuration_issues(&value, &binary().ok()?),
        Err(_) => vec!["settings_unreadable_or_invalid".into()],
    };
    (!issues.is_empty()).then(|| {
        let shown = settings_path.display();
        format!(
            "Claude reporting hooks missing in {shown}: run `ovrcr agent setup claude --print --settings {shown}` and merge the result"
        )
    })
}

// Health reasons are arbitrary provider text on the wire. Doctor must expose
// only known diagnostic codes, never private provider strings or identities.
fn public_health_reason(reason: Option<&str>) -> Option<&str> {
    reason.filter(|reason| {
        matches!(
            *reason,
            "identity_transition_unavailable"
                | "collector_unavailable"
                | "source_unavailable"
                | "read_unavailable"
                | "source_rebuilding"
                | "usage_regression"
                | "record_limit"
                | "invalid_usage_record"
                | "unrecognized_usage_record"
                | "foreign_usage_record"
                | "conflicting_usage_record"
                | "accounting_limit"
                | "incomplete_final_accounting"
                | "supervisor_disconnected"
                | "unfinalized_release"
        )
    })
}

pub(super) fn doctor(
    path: Option<&Path>,
    session: Option<u64>,
    executable: &OsStr,
) -> AppResult<()> {
    let probe = with_version_probe(|| ovrcr::report::admission::pinned_version(executable))?;
    let supported = probe.supported();
    let mut remediation = Vec::<String>::new();
    if supported.is_none() {
        remediation.push(match probe.observed() {
            Some(version) => format!(
                "Detected Claude Code {version}; install/select stable Claude Code >=2.1.267 and rerun doctor."
            ),
            None => "The bounded executable version probe was unavailable; install/select stable Claude Code >=2.1.267 and rerun doctor.".into(),
        });
    }
    let mut issues = Vec::<String>::new();
    let configuration = match (path, settings(path)) {
        (None, _) => "unverified",
        (Some(_), Err(_)) => {
            issues.push("settings_unreadable_or_invalid".into());
            "unverified"
        }
        (Some(_), Ok(value)) => {
            let executable = binary().map_err(RuntimeError::internal)?;
            issues.extend(configuration_issues(&value, &executable));
            if issues.is_empty() {
                "supplied_file_supported"
            } else {
                "supplied_file_unsupported_or_unverified"
            }
        }
    };
    if path.is_none() || !issues.is_empty() {
        remediation.push("Run agent setup claude --print with the intended --settings file, review the composition, and verify synchronous hooks in all effective settings sources.".into());
    }
    let requested = session.or_else(|| std::env::var("OVRCR_SESSION_ID").ok()?.parse().ok());
    let mut binding = Value::Null;
    let mut health = Value::Null;
    let session_status = if let Some(id) = requested {
        match super::inspect() {
            Ok((_, sessions)) => match sessions.iter().find(|session| session.id.0 == id) {
                Some(session) => match &session.agent {
                    Some(agent) => {
                        binding = json!({"provider":agent.binding.provider, "generation":agent.binding.generation});
                        health = json!({"state":agent.health.state, "reason":public_health_reason(agent.health.reason.as_deref()), "activity_available":agent.activity.is_some(), "metrics_available":agent.metrics.is_some(),
                            "usage_available":agent.metrics.as_ref().is_some_and(|metrics| metrics.sample.usage.value.input_tokens.is_some() || metrics.sample.usage.value.output_tokens.is_some()),
                            "usage_coverage":agent.metrics.as_ref().map(|metrics| metrics.sample.usage.value.coverage),
                            "cost_available":agent.metrics.as_ref().is_some_and(|metrics| metrics.sample.cost.value.is_some())});
                        if agent.health.state == ReporterHealth::Unavailable {
                            remediation.push(if agent.health.reason.as_deref() == Some("identity_transition_unavailable") {
                                "Reporting cannot safely identify the foreground conversation after an unsupported or ambiguous transition. Claude is still usable; configuration repair cannot recover this reporting lifetime. When ready, start a fresh supervised invocation with `ovrcr agent run claude -- claude`.".into()
                            } else {
                                "Reporting is unavailable. Check the source_health reason and supervised launcher/reporting hooks; start a fresh supported invocation after correcting the reported cause.".into()
                            });
                            "reporting_unavailable"
                        } else {
                            "bound"
                        }
                    }
                    None => {
                        remediation.push("Start Claude through the Dashboard agent picker or `ovrcr agent run claude -- claude` (legacy `--provider claude` also works). Resume a known canonical UUIDv4 with `claude --resume UUID`; 2.1.268+ also accepts `claude -r UUID`. A plain `claude` launch stays untracked.".into());
                        "unbound"
                    }
                },
                None => {
                    remediation.push("The requested session is not live; select a current session ID before checking its binding.".into());
                    "session_not_found"
                }
            },
            Err(_) => {
                remediation.push("Session inspection was unavailable; check the configured OVRCR socket and server, then rerun doctor.".into());
                "inspection_unavailable"
            }
        }
    } else {
        "not_requested"
    };
    let probe_status = match &probe {
        ovrcr::report::admission::ClaudeVersionProbe::Supported(_) => "supported",
        ovrcr::report::admission::ClaudeVersionProbe::Unsupported(_) => "unsupported",
        ovrcr::report::admission::ClaudeVersionProbe::Unavailable => "unavailable",
    };
    let resume_forms: &[&str] = match supported {
        Some(ovrcr::report::admission::ClaudeVersion::V2_1_267) => &["--resume"],
        Some(_) => &["--resume", "-r"],
        None => &[],
    };
    println!("{}", serde_json::to_string_pretty(&json!({
        "provider":"claude", "executable":executable.to_string_lossy(), "compatible_versions":ovrcr::report::versions::CLAUDE.range(), "tested_versions":ovrcr::report::versions::CLAUDE.tested, "version_compatible":supported.is_some(), "version_tested":ovrcr::report::versions::CLAUDE.tested(probe.observed().as_deref()), "version":probe.observed(),
        "probe_status":probe_status,
        "configuration":{"status":configuration, "effective_configuration":"unverified", "hook_trust":"unverified", "delivery":"unverified", "issues":issues},
        "session_status":session_status, "binding":binding, "source_health":health,
        "capabilities":{"initial_invocation":{"fresh":supported.is_some(),"resume":"explicit_canonical_lowercase_uuid_v4","resume_forms":resume_forms,"continue":false,"fork":false}, "activity":"observed", "ready":"available", "completion_quality":"observed", "input_requests":"approvals_available","questions":"unavailable_no_distinct_surface", "settled_completion":"unverified", "usage":"recognized_root_transcript_records_partial", "complete_accounting":false, "context":"statusline_source_reported"},
        "remediation":remediation
    })).map_err(RuntimeError::internal)?);
    Ok(())
}

pub(super) fn with_version_probe<T>(probe: impl FnOnce() -> T) -> AppResult<T> {
    // This CLI path has not started worker threads and has no launcher's signal
    // iterator. Defer termination only while
    // the bounded probe owns its child group; restore pending signals after reap.
    let mut signals: libc::sigset_t = unsafe { std::mem::zeroed() };
    let mut previous: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut signals);
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
            libc::sigaddset(&mut signals, signal);
        }
    }
    let blocked = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &signals, &mut previous) };
    if blocked != 0 {
        return Err(RuntimeError::internal(std::io::Error::from_raw_os_error(
            blocked,
        )));
    }
    let result = probe();
    let restored =
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut()) };
    if restored != 0 {
        return Err(RuntimeError::internal(std::io::Error::from_raw_os_error(
            restored,
        )));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doctor_health_reason_exposes_known_codes_and_redacts_arbitrary_text() {
        for code in [
            "identity_transition_unavailable",
            "invalid_usage_record",
            "source_unavailable",
        ] {
            assert_eq!(public_health_reason(Some(code)), Some(code));
        }
        for reason in [
            None,
            Some("NEVER_PRIVATE_REASON"),
            Some("identity_transition_unavailable PRIVATE"),
            Some("/private/path"),
        ] {
            assert_eq!(public_health_reason(reason), None);
        }
    }

    #[test]
    fn configuration_issues_flag_missing_hooks_and_accept_setup_output() {
        let executable = "/opt/ovrcr";
        let empty = configuration_issues(&json!({}), executable);
        assert_eq!(empty.len(), HOOKS.len() + 1);
        assert!(empty.contains(&"Stop:synchronous_reporter_missing".to_owned()));
        assert!(empty.contains(&"statusline_reporting_unverified".to_owned()));
        let command = format!("{MARKER}{} report claude --stdin-json", quote(executable));
        let mut composed = json!({"statusLine":{"type":"command","command":format!("{MARKER}{} report claude-statusline --stdin-json", quote(executable))}});
        for event in HOOKS {
            composed["hooks"][*event] =
                json!([{"hooks":[{"type":"command","command":command,"timeout":2,"async":false}]}]);
        }
        assert!(configuration_issues(&composed, executable).is_empty());
        composed["hooks"]["Stop"][0]["hooks"][0]["async"] = json!(true);
        assert_eq!(
            configuration_issues(&composed, executable),
            [
                "Stop:asynchronous_reporting_unsupported",
                "Stop:synchronous_reporter_missing"
            ]
        );
    }

    #[test]
    fn retarget_replaces_only_a_different_ovrcr_reporter_token() {
        let installed = "/Users/xlyk/.local/bin/ovrcr";
        let source = "\
# keep\ncommand = \"exec '/tmp/old/ovrcr' report codex --stdin\"\nother = \"exec '/tmp/notify.sh'\"\nstatus = \": ovrcr-managed-claude-v1; '/tmp/old/ovrcr' report claude-statusline --stdin-json --render-command 'ccstatusline'\"\nsame = \"exec '/Users/xlyk/.local/bin/ovrcr' report codex --stdin\"\nplain = \"echo keep /tmp/old/ovrcr\"\n";
        let next = retarget_reporter_text(source, installed);
        assert!(next.contains("# keep"));
        assert!(next.contains("exec '/tmp/notify.sh'"));
        assert!(next.contains("echo keep /tmp/old/ovrcr"));
        assert!(next.contains(": ovrcr-managed-claude-v1; '/Users/xlyk/.local/bin/ovrcr' report claude-statusline --stdin-json --render-command 'ccstatusline'"));
        assert!(next.contains("exec '/Users/xlyk/.local/bin/ovrcr' report codex --stdin"));
        assert!(next.contains("echo keep /tmp/old/ovrcr"));
        assert!(!next.contains("exec '/tmp/old/ovrcr'"));
        assert_eq!(retarget_reporter_text(&next, installed), next);
    }
    #[test]
    fn boot_hook_warning_names_missing_claude_hooks_with_setup_command() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let claude = bin.join("claude");
        std::fs::write(&claude, []).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&claude).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&claude, permissions).unwrap();
        }
        let config = root.path().join("claude-config");
        std::fs::create_dir(&config).unwrap();
        std::fs::write(config.join("settings.json"), b"{}").unwrap();
        let home = root.path().join("home");
        std::fs::create_dir(&home).unwrap();
        // Isolate from the real Codex home so a local codex config cannot join the banner.
        let codex_home = root.path().join("codex-home-empty");
        std::fs::create_dir(&codex_home).unwrap();
        let previous_path = std::env::var_os("PATH");
        let previous_claude = std::env::var_os("CLAUDE_CONFIG_DIR");
        let previous_codex = std::env::var_os("CODEX_HOME");
        let previous_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("PATH", &bin);
            std::env::set_var("CLAUDE_CONFIG_DIR", &config);
            std::env::set_var("CODEX_HOME", &codex_home);
            std::env::set_var("HOME", &home);
        }
        let warning = boot_hook_warning();
        match previous_path {
            Some(value) => unsafe { std::env::set_var("PATH", value) },
            None => unsafe { std::env::remove_var("PATH") },
        }
        match previous_claude {
            Some(value) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", value) },
            None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
        }
        match previous_codex {
            Some(value) => unsafe { std::env::set_var("CODEX_HOME", value) },
            None => unsafe { std::env::remove_var("CODEX_HOME") },
        }
        match previous_home {
            Some(value) => unsafe { std::env::set_var("HOME", value) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        let warning = warning.expect("missing Claude hooks should warn");
        assert!(
            warning.contains("Claude reporting hooks missing"),
            "{warning}"
        );
        assert!(
            warning.contains("ovrcr agent setup claude --print --settings"),
            "{warning}"
        );
        assert!(
            warning.contains(config.join("settings.json").display().to_string().as_str()),
            "{warning}"
        );
    }
}

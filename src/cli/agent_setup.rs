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

fn quote(value: &str) -> String {
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

fn binary() -> anyhow::Result<String> {
    std::env::current_exe()?
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("OVRCR executable path is not UTF-8"))
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
            "Review the printed JSON and merge it into the intended Claude settings file. No file was written. Use synchronous command hooks with Claude Code 2.1.267; supplied settings do not prove effective enterprise/plugin configuration. Launcher inside an OVRCR session: {} agent run --provider claude -- claude",
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

pub(super) fn doctor(
    path: Option<&Path>,
    session: Option<u64>,
    executable: &OsStr,
) -> AppResult<()> {
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
    let supported = ovrcr::report::admission::pinned_version(executable);
    let restored =
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut()) };
    if restored != 0 {
        return Err(RuntimeError::internal(std::io::Error::from_raw_os_error(
            restored,
        )));
    }
    let mut remediation = Vec::<String>::new();
    if !supported {
        remediation.push("Install/select Claude Code 2.1.267 and rerun doctor; the bounded executable probe was unavailable or did not match the supported version.".into());
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
            for event in HOOKS {
                let mut present = false;
                if let Some(groups) = value
                    .get("hooks")
                    .and_then(|hooks| hooks.get(*event))
                    .and_then(Value::as_array)
                {
                    for group in groups {
                        if let Some(handlers) = group.get("hooks").and_then(Value::as_array) {
                            for handler in handlers {
                                let command =
                                    handler.get("command").and_then(Value::as_str).unwrap_or("");
                                if command.contains("report claude") {
                                    if handler.get("async").and_then(Value::as_bool) == Some(true) {
                                        issues.push(format!(
                                            "{event}:asynchronous_reporting_unsupported"
                                        ));
                                    }
                                    if (command
                                        == format!(
                                            "{MARKER}{} report claude --stdin-json",
                                            quote(&executable)
                                        )
                                        || exact(command, &executable, "claude"))
                                        && handler.get("async").and_then(Value::as_bool)
                                            != Some(true)
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
            if !status.starts_with(&format!(
                "{MARKER}{} report claude-statusline --stdin-json",
                quote(&executable)
            )) && !exact(status, &executable, "claude-statusline")
            {
                issues.push("statusline_reporting_unverified".into());
            }
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
                        health = json!({"state":agent.health.state, "activity_available":agent.activity.is_some(), "metrics_available":agent.metrics.is_some(),
                            "usage_available":agent.metrics.as_ref().is_some_and(|metrics| metrics.sample.usage.value.input_tokens.is_some() || metrics.sample.usage.value.output_tokens.is_some()),
                            "usage_coverage":agent.metrics.as_ref().map(|metrics| metrics.sample.usage.value.coverage),
                            "cost_available":agent.metrics.as_ref().is_some_and(|metrics| metrics.sample.cost.value.is_some())});
                        if agent.health.state == ReporterHealth::Unavailable {
                            remediation.push("Reporting is unavailable. Check the supervised launcher and reporting hooks; restart a fresh supported invocation after correcting configuration.".into());
                        }
                        "bound"
                    }
                    None => {
                        remediation.push("Start Claude with ovrcr agent run --provider claude -- claude inside this OVRCR session.".into());
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
    println!("{}", serde_json::to_string_pretty(&json!({
        "provider":"claude", "executable":executable.to_string_lossy(), "supported_version":"2.1.267", "version": if supported { Some("2.1.267") } else { None },
        "probe_status":if supported { "supported" } else { "unsupported_or_unavailable" },
        "configuration":{"status":configuration, "effective_configuration":"unverified", "issues":issues},
        "session_status":session_status, "binding":binding, "source_health":health,
        "capabilities":{"activity":"observed", "settled_completion":"unverified", "usage":"recognized_root_transcript_records_partial", "complete_accounting":false, "context":"statusline_source_reported"},
        "remediation":remediation
    })).map_err(RuntimeError::internal)?);
    Ok(())
}

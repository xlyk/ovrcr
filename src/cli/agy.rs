//! Antigravity CLI configuration and passive status callbacks.
use super::{AppResult, RuntimeError};
use ovrcr::protocol::{AgentProvider, ReporterHealth};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const SETTINGS_LIMIT: u64 = 1_048_576;
const HOOK_NAME: &str = "ovrcr-agent-status";
const STATUS_MARKER: &str = "OVRCR_AGENT_STATUS=agy ";

fn home() -> anyhow::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is unavailable"))?;
    anyhow::ensure!(home.is_absolute(), "HOME must be an absolute path");
    Ok(home)
}

pub(super) fn settings_path(settings: Option<&Path>) -> anyhow::Result<PathBuf> {
    match settings {
        Some(path) => Ok(path.to_owned()),
        None => Ok(home()?.join(".gemini/antigravity-cli/settings.json")),
    }
}

pub(super) fn hooks_path() -> anyhow::Result<PathBuf> {
    Ok(home()?.join(".gemini/config/hooks.json"))
}

fn read_document(path: &Path) -> anyhow::Result<Option<Value>> {
    let mut bytes = Vec::new();
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    file.take(SETTINGS_LIMIT + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= SETTINGS_LIMIT,
        "document exceeds 1 MiB"
    );
    Ok(Some(document_text(std::str::from_utf8(&bytes)?)?))
}

fn document_text(text: &str) -> anyhow::Result<Value> {
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_str(text)?;
    anyhow::ensure!(value.is_object(), "document must be a JSON object");
    Ok(value)
}

fn statusline_command(executable: &str, renderer: Option<&str>) -> String {
    let mut command = format!(
        "{STATUS_MARKER}{} report agy-statusline --stdin-json",
        super::agent_setup::quote(executable)
    );
    if let Some(renderer) = renderer {
        command.push_str(" --render-command ");
        command.push_str(&super::agent_setup::quote(renderer));
    }
    command
}

fn is_owned_statusline(command: &str) -> bool {
    command.starts_with(STATUS_MARKER)
}

/// Decode only the single-quoted shell word emitted by `agent_setup::quote`.
/// This keeps setup able to retarget its own command without interpreting a
/// user's arbitrary shell expression.
fn quoted_word(input: &str) -> Option<(String, &str)> {
    let input = input.strip_prefix('\'')?;
    let mut word = String::new();
    let mut index = 0;
    while index < input.len() {
        let rest = &input[index..];
        if rest.starts_with("'\"'\"'") {
            word.push('\'');
            index += 5;
            continue;
        }
        if rest.starts_with('\'') {
            return Some((word, &input[index + 1..]));
        }
        let character = rest.chars().next()?;
        word.push(character);
        index += character.len_utf8();
    }
    None
}

pub(super) fn owned_statusline_renderer(command: &str) -> anyhow::Result<Option<String>> {
    let command = command
        .strip_prefix(STATUS_MARKER)
        .ok_or_else(|| anyhow::anyhow!("managed statusLine marker is invalid"))?;
    let (_, suffix) = quoted_word(command)
        .ok_or_else(|| anyhow::anyhow!("managed statusLine executable is invalid"))?;
    let suffix = suffix
        .strip_prefix(" report agy-statusline --stdin-json")
        .ok_or_else(|| anyhow::anyhow!("managed statusLine command is invalid"))?;
    if suffix.is_empty() {
        return Ok(None);
    }
    let suffix = suffix
        .strip_prefix(" --render-command ")
        .ok_or_else(|| anyhow::anyhow!("managed statusLine renderer is invalid"))?;
    let (renderer, rest) = quoted_word(suffix)
        .ok_or_else(|| anyhow::anyhow!("managed statusLine renderer is invalid"))?;
    anyhow::ensure!(
        rest.is_empty(),
        "managed statusLine has extra shell arguments"
    );
    Ok(Some(renderer))
}

/// Compose one native settings document. The helper streams an existing renderer's
/// stdin and stdout unchanged; a new status line stacks below the built-in one.
pub(super) fn compose_settings(text: &str, executable: &str) -> anyhow::Result<Value> {
    compose_settings_document(document_text(text)?, executable)
}

fn compose_settings_document(mut document: Value, executable: &str) -> anyhow::Result<Value> {
    let status = document
        .as_object_mut()
        .expect("document_text returns an object")
        .entry("statusLine")
        .or_insert_with(|| json!({}));
    let status = status
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("statusLine must be an object"))?;
    anyhow::ensure!(
        status
            .get("type")
            .is_none_or(|value| value.as_str() == Some("command")),
        "statusLine type must be command"
    );
    anyhow::ensure!(
        status.get("command").is_none_or(Value::is_string),
        "statusLine command must be a string"
    );
    anyhow::ensure!(
        status.get("enabled").is_none_or(Value::is_boolean),
        "statusLine enabled must be a boolean"
    );
    anyhow::ensure!(
        status
            .get("stack_with_default")
            .is_none_or(Value::is_boolean),
        "statusLine stack_with_default must be a boolean"
    );
    anyhow::ensure!(
        status
            .get("padding")
            .is_none_or(|value| { value.as_i64().is_some() || value.as_u64().is_some() }),
        "statusLine padding must be an integer"
    );
    let existing_command = status
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let renderer = match existing_command.as_deref() {
        Some(command) if is_owned_statusline(command) => owned_statusline_renderer(command)?,
        Some(command) => Some(command.to_owned()),
        None => None,
    };
    let had_command = existing_command.is_some();
    status.insert("type".into(), json!("command"));
    status.insert(
        "command".into(),
        json!(statusline_command(executable, renderer.as_deref())),
    );
    if status.get("stack_with_default").is_none() && !had_command {
        status.insert("stack_with_default".into(), json!(true));
    }
    Ok(document)
}

fn hook_command(executable: &str) -> String {
    format!(
        "exec {} report agy --stdin",
        super::agent_setup::quote(executable)
    )
}

fn owned_hook_command(command: &str, executable: &str) -> bool {
    let Some(command) = command.strip_prefix("exec ") else {
        return false;
    };
    let Some(command) = command.strip_suffix(" report agy --stdin") else {
        return false;
    };
    let Some((token, rest)) = quoted_word(command) else {
        return false;
    };
    rest.is_empty()
        && (token == executable
            || Path::new(&token)
                .file_name()
                .is_some_and(|name| name == "ovrcr"))
}

/// Add one synchronous Stop observer under OVRCR's own hook name. Existing hook
/// names and handlers are kept in place, including other Stop handlers.
pub(super) fn compose_hooks(text: &str, executable: &str) -> anyhow::Result<Value> {
    compose_hooks_document(document_text(text)?, executable)
}

fn compose_hooks_document(mut document: Value, executable: &str) -> anyhow::Result<Value> {
    let root = document
        .as_object_mut()
        .expect("document_text returns an object");
    let hook = root.entry(HOOK_NAME).or_insert_with(|| json!({}));
    let hook = hook
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("OVRCR hook entry must be an object"))?;
    anyhow::ensure!(
        hook.get("enabled").is_none_or(Value::is_boolean),
        "OVRCR hook enabled must be a boolean"
    );
    let handlers = hook.entry("Stop").or_insert_with(|| json!([]));
    let handlers = handlers
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("OVRCR Stop hook must be an array"))?;
    anyhow::ensure!(
        handlers.iter().all(Value::is_object),
        "OVRCR Stop handlers must be objects"
    );
    let command = hook_command(executable);
    let mut found = false;
    handlers.retain_mut(|handler| {
        let owned = handler
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| owned_hook_command(command, executable));
        if !owned {
            return true;
        }
        if found {
            return false;
        }
        found = true;
        let object = handler
            .as_object_mut()
            .expect("Stop handlers were checked as objects");
        object.insert("type".into(), json!("command"));
        object.insert("command".into(), json!(command));
        object.insert("timeout".into(), json!(2));
        object.insert("async".into(), json!(false));
        true
    });
    if !found {
        handlers.push(json!({
            "type": "command",
            "command": command,
            "timeout": 2,
            "async": false
        }));
    }
    Ok(document)
}

fn setup_documents_at(
    settings_path: &Path,
    hooks_path: &Path,
    executable: &str,
) -> anyhow::Result<(PathBuf, Value, PathBuf, Value)> {
    let settings_document = read_document(settings_path)?.unwrap_or_else(|| json!({}));
    let hooks_document = read_document(hooks_path)?.unwrap_or_else(|| json!({}));
    Ok((
        settings_path.to_owned(),
        compose_settings_document(settings_document, executable)?,
        hooks_path.to_owned(),
        compose_hooks_document(hooks_document, executable)?,
    ))
}

fn setup_payload(
    settings_path: &Path,
    hooks_path: &Path,
    executable: &str,
) -> anyhow::Result<Value> {
    let (settings_path, settings_document, hooks_path, hooks_document) =
        setup_documents_at(settings_path, hooks_path, executable)?;
    Ok(json!({
        "provider": "agy",
        "writes": false,
        "settings": {"path": settings_path, "document": settings_document},
        "hooks": {"path": hooks_path, "document": hooks_document},
        "guidance": "Merge each document into its named target. --settings overrides only the status line settings path; hooks use ~/.gemini/config/hooks.json. Hook trust and delivery remain native Antigravity decisions."
    }))
}

pub(super) fn setup(settings: Option<&Path>) -> AppResult<()> {
    let result = (|| -> anyhow::Result<Value> {
        let executable = super::agent_setup::binary()?;
        let settings_path = settings_path(settings)?;
        let hooks_path = hooks_path()?;
        setup_payload(&settings_path, &hooks_path, &executable)
    })();
    let value = result.map_err(|_| {
        RuntimeError::internal(anyhow::anyhow!(
            "Cannot compose Antigravity JSON: settings or hooks unreadable, invalid, oversized, or incompatible"
        ))
    })?;
    println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(RuntimeError::internal)?
    );
    Ok(())
}

fn installed(executable: &OsStr) -> bool {
    let path = Path::new(executable);
    let executable_file = |path: &Path| {
        std::fs::metadata(path).is_ok_and(|metadata| {
            use std::os::unix::fs::PermissionsExt;
            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
        })
    };
    if path.components().count() > 1 {
        executable_file(path)
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .any(|directory| executable_file(&directory.join(path)))
    }
}

fn exact_command(command: &str, executable: &str, reporter: &str, input: &str) -> bool {
    if reporter == "agy-statusline" {
        return is_owned_statusline(command) && owned_statusline_renderer(command).is_ok();
    }
    reporter == "agy" && input == "--stdin" && owned_hook_command(command, executable)
}

fn statusline_status(
    value: &Value,
    executable: &str,
    issues: &mut Vec<String>,
    malformed: &mut bool,
) -> bool {
    let Some(status) = value.get("statusLine") else {
        issues.push("statusline_reporter_missing".into());
        return false;
    };
    if !status.is_object() {
        issues.push("statusline_invalid".into());
        *malformed = true;
        return false;
    }
    let valid = status
        .get("type")
        .is_none_or(|value| value.as_str() == Some("command"))
        && status.get("enabled").is_none_or(Value::is_boolean)
        && status
            .get("stack_with_default")
            .is_none_or(Value::is_boolean)
        && status
            .get("padding")
            .is_none_or(|value| value.as_i64().is_some() || value.as_u64().is_some())
        && status.get("command").is_none_or(Value::is_string);
    if !valid {
        issues.push("statusline_invalid".into());
        *malformed = true;
        return false;
    }
    let enabled = status
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !enabled {
        issues.push("statusline_disabled".into());
    }
    let command = status.get("command").and_then(Value::as_str).unwrap_or("");
    let present = exact_command(command, executable, "agy-statusline", "--stdin-json");
    if !present {
        issues.push("statusline_reporter_missing".into());
    }
    present && enabled
}

fn stop_hook_status(
    value: &Value,
    executable: &str,
    issues: &mut Vec<String>,
    malformed: &mut bool,
) -> bool {
    let Some(entry) = value.get(HOOK_NAME) else {
        issues.push("stop_hook_missing".into());
        return false;
    };
    if !entry.is_object() {
        issues.push("stop_hook_invalid".into());
        *malformed = true;
        return false;
    }
    if entry
        .get("enabled")
        .is_some_and(|value| !value.is_boolean())
    {
        issues.push("stop_hook_invalid".into());
        *malformed = true;
        return false;
    }
    if entry.get("Stop").is_some_and(|value| !value.is_array()) {
        issues.push("stop_hook_invalid".into());
        *malformed = true;
        return false;
    }
    let handlers = entry.get("Stop").and_then(Value::as_array);
    if handlers.is_some_and(|handlers| !handlers.iter().all(Value::is_object)) {
        issues.push("stop_hook_invalid".into());
        *malformed = true;
        return false;
    }
    let enabled = entry
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let present = handlers.is_some_and(|handlers| {
        handlers.iter().any(|handler| {
            handler.get("type").and_then(Value::as_str) == Some("command")
                && handler
                    .get("async")
                    .is_none_or(|value| value.as_bool() == Some(false))
                && handler
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| {
                        command == hook_command(executable)
                            || owned_hook_command(command, executable)
                    })
        })
    });
    if !present || !enabled {
        issues.push(if enabled {
            "stop_hook_missing".into()
        } else {
            "stop_hook_disabled".into()
        });
    }
    present && enabled
}

fn document_status(
    settings_path: &Path,
    hooks_path: &Path,
    executable: &str,
) -> (String, Vec<String>, bool) {
    let mut issues = Vec::new();
    let mut malformed = false;
    let settings = read_document(settings_path);
    let hook_doc = read_document(hooks_path);
    let statusline_present = match settings {
        Ok(Some(value)) => statusline_status(&value, executable, &mut issues, &mut malformed),
        Ok(None) => {
            issues.push("settings_missing".into());
            issues.push("statusline_reporter_missing".into());
            false
        }
        Err(_) => {
            issues.push("settings_unreadable_or_invalid".into());
            malformed = true;
            false
        }
    };
    let stop_present = match hook_doc {
        Ok(Some(value)) => stop_hook_status(&value, executable, &mut issues, &mut malformed),
        Ok(None) => {
            issues.push("hooks_missing".into());
            issues.push("stop_hook_missing".into());
            false
        }
        Err(_) => {
            issues.push("hooks_unreadable_or_invalid".into());
            malformed = true;
            false
        }
    };
    let status = if malformed {
        "unverified"
    } else if statusline_present && stop_present {
        "configured"
    } else {
        "needs_setup"
    };
    (status.into(), issues, statusline_present && stop_present)
}

fn doctor_health_reason(state: ReporterHealth, reason: Option<&str>) -> Option<&str> {
    super::agent_setup::public_health_reason(reason)
        .or_else(|| reason.map(|_| "unrecognized_reason_redacted"))
        .or_else(|| (state == ReporterHealth::Unavailable).then_some("reason_not_reported"))
}

pub(super) fn doctor(
    settings: Option<&Path>,
    session: Option<u64>,
    executable: &OsStr,
) -> AppResult<()> {
    let settings_path = settings_path(settings).map_err(RuntimeError::internal)?;
    let hooks_path = hooks_path().map_err(RuntimeError::internal)?;
    let executable_found = installed(executable);
    let executable_text = executable.to_string_lossy().into_owned();
    let reporter_executable = super::agent_setup::binary().map_err(RuntimeError::internal)?;
    let (configuration, issues, configured) =
        document_status(&settings_path, &hooks_path, &reporter_executable);
    let requested = session.or_else(|| std::env::var("OVRCR_SESSION_ID").ok()?.parse().ok());
    let mut binding = Value::Null;
    let mut health = Value::Null;
    let mut guidance = String::new();
    let session_status = match requested {
        None => "not_requested",
        Some(id) => match super::inspect() {
            Err(_) => {
                guidance.push_str("Session inspection was unavailable; check the OVRCR socket and server, then rerun doctor. ");
                "inspection_unavailable"
            }
            Ok((_, sessions)) => match sessions.iter().find(|session| session.id.0 == id) {
                None => {
                    guidance.push_str("The requested session was not found; select a current session ID before checking its binding. ");
                    "session_not_found"
                }
                Some(session) => match &session.agent {
                    None => {
                        guidance.push_str("This session has no reporting binding. Start Antigravity through the Dashboard agent picker or `ovrcr agent run agy -- agy`; a plain launch stays untracked. ");
                        "unbound"
                    }
                    Some(agent) => {
                        binding = json!({"provider": agent.binding.provider, "generation": agent.binding.generation});
                        if agent.binding.provider != AgentProvider::Agy {
                            guidance.push_str("This session is bound to another reporting provider; select an Antigravity session before diagnosing Antigravity reporting. ");
                            "provider_mismatch"
                        } else {
                            health = json!({
                                "state": agent.health.state,
                                "reason": doctor_health_reason(agent.health.state, agent.health.reason.as_deref()),
                                "activity_available": agent.activity.is_some()
                            });
                            if agent.health.state == ReporterHealth::Unavailable {
                                guidance.push_str("Reporting is unavailable for this invocation. See source_health.reason for a safe diagnostic code; verify native hook trust and delivery, then launch a fresh managed Antigravity invocation. ");
                                "reporting_unavailable"
                            } else {
                                "bound"
                            }
                        }
                    }
                },
            },
        },
    };
    if !configured {
        guidance.push_str("Run `ovrcr agent setup agy --print` and merge the emitted settings and hooks documents into their named files, or use the Dashboard Connect agent status flow. Native hook trust and delivery are not inferred from file presence. ");
    }
    guidance.push_str("Antigravity Stop callbacks are observational and return `decision: allow`; status-line rendering is passed through. Doctor does not start Antigravity or inspect credentials.");
    let report = json!({
        "provider": "agy",
        "executable": executable_text,
        "executable_status": if executable_found { "found" } else { "missing_or_not_executable" },
        "probe_status": "not_run",
        "version": null,
        "version_status": "unverified",
        "release_status": "statusline_and_stop_hook",
        "configuration": {
            "status": configuration,
            "settings_path": settings_path,
            "hooks_path": hooks_path,
            "effective_configuration": "unverified",
            "hook_trust": "unverified",
            "delivery": "unverified",
            "issues": issues
        },
        "session_status": session_status,
        "binding": binding,
        "source_health": health,
        "capabilities": {
            "managed_launch": true,
            "activity": if configured { "configured_native_delivery_unverified" } else { "needs_setup" },
            "readiness": if AgentProvider::Agy.supports_readiness() {
                if configured { "observed" } else { "needs_setup" }
            } else {
                "unavailable_pending_native_evidence"
            },
            "input_requests": "unavailable",
            "questions": "unavailable",
            "recovery": "unverified",
            "metrics": "unavailable",
            "titles": "statusline_conversation_title"
        },
        "guidance": guidance
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(RuntimeError::internal)?
    );
    Ok(())
}

/// Run Stop as a telemetry-only native observer. The CLI's non-continue decision
/// keeps its native Stop behavior; report failures are intentionally invisible.
pub(super) fn run_stop(stdin: bool) -> AppResult<()> {
    if !stdin {
        return Err(RuntimeError::new(
            ovrcr::protocol::ErrorCode::InvalidRequest,
            "--stdin is required",
        ));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    if let Ok(input) = ovrcr::report::read_hook_stdin(deadline)
        && std::env::var_os("OVRCR_AGENT_SOCKET").is_some()
    {
        let _ = ovrcr::report::send_agy_stop(&input, deadline);
    }
    let _ = std::io::stdout()
        .lock()
        .write_all(b"{\"decision\":\"allow\"}\n");
    Ok(())
}

/// Preserve Antigravity's original status line command and output while reporting
/// only the allowlisted state through the private OVRCR channel.
pub(super) fn run_statusline(stdin_json: bool, render_command: Option<String>) -> AppResult<()> {
    if !stdin_json {
        return Err(RuntimeError::new(
            ovrcr::protocol::ErrorCode::InvalidRequest,
            "--stdin-json is required",
        ));
    }
    let mut renderer = render_command
        .as_deref()
        .map(|command| {
            std::process::Command::new("/bin/sh")
                .args(["-c", command])
                .stdin(std::process::Stdio::piped())
                .spawn()
        })
        .transpose()
        .map_err(|_| {
            RuntimeError::new(
                ovrcr::protocol::ErrorCode::Internal,
                "status-line renderer could not start",
            )
        })?;
    let mut pipe = renderer.as_mut().and_then(|child| child.stdin.take());
    let mut input = std::io::stdin().lock();
    let mut retained = Vec::new();
    let mut reportable = true;
    let mut buffer = [0_u8; 8192];
    loop {
        let count = match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                reportable = false;
                break;
            }
        };
        if let Some(output) = &mut pipe
            && output.write_all(&buffer[..count]).is_err()
        {
            pipe = None;
        }
        if reportable && retained.len() + count <= ovrcr::report::HOOK_INPUT_LIMIT {
            retained.extend_from_slice(&buffer[..count]);
        } else {
            reportable = false;
            retained.clear();
        }
    }
    drop(pipe);
    if reportable && std::env::var_os("OVRCR_AGENT_SOCKET").is_some() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let _ = ovrcr::report::send_agy_statusline(&retained, deadline);
    }
    if let Some(mut renderer) = renderer {
        let status = renderer.wait().map_err(|_| {
            RuntimeError::new(
                ovrcr::protocol::ErrorCode::Internal,
                "status-line renderer wait failed",
            )
        })?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_composition_preserves_renderer_and_native_options_idempotently() {
        let original = json!({
            "notifications": true,
            "statusLine": {
                "type": "command",
                "command": "printf '%s\\n' \"custom line's output\"",
                "padding": 2,
                "enabled": true,
                "stack_with_default": false
            }
        });
        let text = serde_json::to_string(&original).unwrap();
        let composed = compose_settings(&text, "/opt/ovrcr'alias").unwrap();
        assert_eq!(composed["notifications"], true);
        assert_eq!(composed["statusLine"]["padding"], 2);
        assert_eq!(composed["statusLine"]["enabled"], true);
        assert_eq!(composed["statusLine"]["stack_with_default"], false);
        let command = composed["statusLine"]["command"].as_str().unwrap();
        assert!(command.starts_with(STATUS_MARKER));
        assert_eq!(
            owned_statusline_renderer(command).unwrap().as_deref(),
            Some("printf '%s\\n' \"custom line's output\"")
        );
        let again =
            compose_settings(&serde_json::to_string(&composed).unwrap(), "/new/ovrcr").unwrap();
        let moved = again["statusLine"]["command"].as_str().unwrap();
        assert!(moved.starts_with("OVRCR_AGENT_STATUS=agy '/new/ovrcr'"));
        assert_eq!(
            owned_statusline_renderer(moved).unwrap().as_deref(),
            Some("printf '%s\\n' \"custom line's output\"")
        );
        let repeated =
            compose_settings(&serde_json::to_string(&again).unwrap(), "/new/ovrcr").unwrap();
        assert_eq!(
            repeated, again,
            "repeating setup must not wrap the renderer again"
        );
    }

    #[test]
    fn new_settings_keep_the_builtin_renderer_and_native_status_options() {
        let composed = compose_settings("{}", "/opt/ovrcr").unwrap();
        assert_eq!(composed["statusLine"]["stack_with_default"], true);
        assert_eq!(composed["statusLine"]["type"], "command");
        assert!(is_owned_statusline(
            composed["statusLine"]["command"].as_str().unwrap()
        ));
    }

    #[test]
    fn disabled_user_statusline_stays_disabled_and_does_not_change_default_stacking() {
        let composed = compose_settings(
            r#"{"statusLine":{"command":"my-renderer","enabled":false}}"#,
            "/opt/ovrcr",
        )
        .unwrap();
        assert_eq!(composed["statusLine"]["enabled"], false);
        assert!(composed["statusLine"].get("stack_with_default").is_none());
        assert_eq!(
            owned_statusline_renderer(composed["statusLine"]["command"].as_str().unwrap())
                .unwrap()
                .as_deref(),
            Some("my-renderer")
        );
    }

    #[test]
    fn hook_composition_adds_only_our_stop_handler_and_is_idempotent() {
        let original = json!({
            "user-linter": {"PostToolUse": [{"matcher":"write_to_file","hooks":[{"type":"command","command":"lint"}]}]},
            "ovrcr-agent-status": {
                "PreInvocation": [{"type":"command","command":"keep"}],
                "Stop": [
                    {"type":"command","command":"exec '/old/ovrcr' report agy --stdin","timeout":1},
                    {"type":"command","command":"user-stop"}
                ]
            }
        });
        let composed =
            compose_hooks(&serde_json::to_string(&original).unwrap(), "/opt/ovrcr").unwrap();
        assert_eq!(composed["user-linter"], original["user-linter"]);
        assert_eq!(
            composed[HOOK_NAME]["PreInvocation"],
            original[HOOK_NAME]["PreInvocation"]
        );
        assert_eq!(composed[HOOK_NAME]["Stop"].as_array().unwrap().len(), 2);
        assert_eq!(
            composed[HOOK_NAME]["Stop"][0]["command"],
            hook_command("/opt/ovrcr")
        );
        assert_eq!(composed[HOOK_NAME]["Stop"][0]["async"], false);
        assert_eq!(composed[HOOK_NAME]["Stop"][1]["command"], "user-stop");
        let again =
            compose_hooks(&serde_json::to_string(&composed).unwrap(), "/opt/ovrcr").unwrap();
        assert_eq!(again, composed);
    }

    #[test]
    fn invalid_native_documents_are_refused_without_diagnostics_from_the_source() {
        assert!(compose_settings("{PRIVATE", "/opt/ovrcr").is_err());
        assert!(compose_hooks("[]", "/opt/ovrcr").is_err());
        assert!(compose_settings(r#"{"statusLine":{"command":false}}"#, "/opt/ovrcr").is_err());
    }

    #[test]
    fn setup_requires_both_documents_before_returning_any_printable_payload() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let hooks = root.path().join("hooks.json");
        std::fs::write(&settings, r#"{"notifications":true}"#).unwrap();
        std::fs::write(&hooks, "{PRIVATE").unwrap();
        let original_settings = std::fs::read(&settings).unwrap();
        let original_hooks = std::fs::read(&hooks).unwrap();
        assert!(setup_payload(&settings, &hooks, "/opt/ovrcr").is_err());
        assert_eq!(std::fs::read(&settings).unwrap(), original_settings);
        assert_eq!(std::fs::read(&hooks).unwrap(), original_hooks);

        std::fs::write(&hooks, "{}").unwrap();
        let payload = setup_payload(&settings, &hooks, "/opt/ovrcr").unwrap();
        assert_eq!(payload["writes"], false);
        assert_eq!(payload["settings"]["path"], settings.display().to_string());
        assert_eq!(payload["hooks"]["path"], hooks.display().to_string());
        assert!(payload["settings"]["document"]["statusLine"].is_object());
        assert!(payload["hooks"]["document"][HOOK_NAME]["Stop"].is_array());
        assert_eq!(std::fs::read(&settings).unwrap(), original_settings);
        assert_eq!(std::fs::read_to_string(&hooks).unwrap(), "{}");
    }

    #[test]
    fn doctor_distinguishes_configured_disabled_and_malformed_native_files() {
        let root = tempfile::tempdir().unwrap();
        let settings = root.path().join("settings.json");
        let hooks = root.path().join("hooks.json");
        let settings_doc = compose_settings("{}", "/opt/ovrcr").unwrap();
        let hooks_doc = compose_hooks("{}", "/opt/ovrcr").unwrap();
        std::fs::write(&settings, serde_json::to_vec(&settings_doc).unwrap()).unwrap();
        std::fs::write(&hooks, serde_json::to_vec(&hooks_doc).unwrap()).unwrap();

        let (status, issues, configured) = document_status(&settings, &hooks, "/opt/ovrcr");
        assert_eq!(status, "configured");
        assert!(issues.is_empty());
        assert!(configured);

        let mut disabled = settings_doc;
        disabled["statusLine"]["enabled"] = json!(false);
        std::fs::write(&settings, serde_json::to_vec(&disabled).unwrap()).unwrap();
        let (status, issues, configured) = document_status(&settings, &hooks, "/opt/ovrcr");
        assert_eq!(status, "needs_setup");
        assert!(issues.contains(&"statusline_disabled".into()));
        assert!(!configured);

        std::fs::write(&hooks, "{PRIVATE").unwrap();
        let (status, issues, configured) = document_status(&settings, &hooks, "/opt/ovrcr");
        assert_eq!(status, "unverified");
        assert!(issues.contains(&"hooks_unreadable_or_invalid".into()));
        assert!(!configured);
    }

    #[test]
    fn doctor_health_reason_never_exposes_unrecognized_provider_text() {
        assert_eq!(
            doctor_health_reason(ReporterHealth::Unavailable, Some("private native error")),
            Some("unrecognized_reason_redacted")
        );
        assert_eq!(
            doctor_health_reason(ReporterHealth::Unavailable, None),
            Some("reason_not_reported")
        );
        assert_eq!(doctor_health_reason(ReporterHealth::Connected, None), None);
        assert_eq!(
            doctor_health_reason(
                ReporterHealth::Unavailable,
                Some("identity_transition_unavailable")
            ),
            Some("identity_transition_unavailable")
        );
    }
}

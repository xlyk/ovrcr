//! Hermes reporting through native shell hooks, plus process supervision.
use super::{AppResult, RuntimeError};
use ovrcr::protocol::{ErrorCode, SessionKind};
use serde_json::json;
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn no_settings(settings: Option<&Path>) -> AppResult<()> {
    if settings.is_some() {
        return Err(RuntimeError::new(
            ErrorCode::InvalidRequest,
            "Hermes supervision uses no OVRCR provider settings; --settings is unsupported",
        ));
    }
    Ok(())
}

const HOOK_EVENTS: &[&str] = &[
    "on_session_start",
    "pre_api_request",
    "post_api_request",
    "post_llm_call",
    "on_session_end",
    "pre_approval_request",
    "post_approval_response",
];

fn hermes_home() -> AppResult<std::path::PathBuf> {
    let home = std::env::var_os("HERMES_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".hermes"))
        })
        .ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::InvalidRequest,
                "Hermes home is unavailable; set HOME or HERMES_HOME",
            )
        })?;
    if !home.is_absolute() {
        return Err(RuntimeError::new(
            ErrorCode::InvalidRequest,
            "Hermes home must be absolute",
        ));
    }
    Ok(home)
}

fn hook_command() -> AppResult<String> {
    let executable = std::env::current_exe().map_err(RuntimeError::internal)?;
    let executable = executable.to_str().ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::InvalidRequest,
            "ovrcr path is not UTF-8; Hermes hooks need a UTF-8 command",
        )
    })?;
    if executable.contains('\'') || executable.contains('\n') {
        return Err(RuntimeError::new(
            ErrorCode::InvalidRequest,
            "ovrcr path cannot be quoted as a Hermes hook command",
        ));
    }
    Ok(format!("'{executable}' report hermes --stdin"))
}

fn install_hooks(home: &Path, command: &str) -> AppResult<()> {
    let config = home.join("config.yaml");
    let existing = std::fs::read_to_string(&config).unwrap_or_default();
    if existing.lines().any(|line| {
        let bare = line.split('#').next().unwrap_or("").trim();
        bare == "hooks:" || bare.starts_with("hooks:")
    }) {
        return Err(RuntimeError::new(
            ErrorCode::Conflict,
            "Hermes config.yaml already has a hooks: key; review it and add the `ovrcr report hermes --stdin` command for on_session_start, pre_api_request, post_api_request, post_llm_call, on_session_end, pre_approval_request and post_approval_response",
        ));
    }
    let mut block = String::from("\n# ovrcr hermes reporting hooks\nhooks:\n");
    for event in HOOK_EVENTS {
        block.push_str(&format!("  {event}:\n    - command: \"{command}\"\n"));
    }
    std::fs::create_dir_all(home).map_err(RuntimeError::internal)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config)
        .map_err(RuntimeError::internal)?;
    use std::io::Write;
    file.write_all(block.as_bytes())
        .map_err(RuntimeError::internal)?;
    let allowlist = home.join("shell-hooks-allowlist.json");
    let mut document = std::fs::read_to_string(&allowlist)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(|value| value.is_object())
        .unwrap_or_else(|| serde_json::json!({"approvals": []}));
    let approvals = document
        .as_object_mut()
        .and_then(|object| object.get_mut("approvals"))
        .and_then(|value| value.as_array_mut());
    let Some(approvals) = approvals else {
        return Err(RuntimeError::new(
            ErrorCode::Internal,
            "Hermes hook allowlist is not an object",
        ));
    };
    let stamp = chrono_stamp();
    for event in HOOK_EVENTS {
        approvals.retain(|entry| {
            entry.get("event").and_then(|value| value.as_str()) != Some(event)
                || entry.get("command").and_then(|value| value.as_str()) != Some(command)
        });
        approvals.push(serde_json::json!({
            "event": event,
            "command": command,
            "approved_at": stamp,
        }));
    }
    std::fs::write(
        &allowlist,
        serde_json::to_vec_pretty(&document).map_err(RuntimeError::internal)?,
    )
    .map_err(RuntimeError::internal)?;
    Ok(())
}

fn chrono_stamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("{seconds}")
}

fn hooks_installed(home: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(home.join("config.yaml")) else {
        return false;
    };
    HOOK_EVENTS
        .iter()
        .all(|event| text.contains(&format!("{event}:")))
        && text.contains("report hermes --stdin")
}

pub(super) fn setup(settings: Option<&Path>) -> AppResult<()> {
    no_settings(settings)?;
    let home = hermes_home()?;
    let command = hook_command()?;
    install_hooks(&home, &command)?;
    println!(
        "Installed Hermes reporting hooks in {}. They forward session id, turn id, model, approval id and token counts to OVRCR. Prompts, transcripts and tool arguments are dropped. Ready/Unread follows post_llm_call. Approval prompts follow pre_approval_request. Token totals follow post_api_request. Cost is not in that summary, so cost stays unavailable. Titles are read from the session row in state.db. Resume is `hermes --resume <id>`. Questions that are not approval hooks stay unavailable. Re-run setup per Hermes profile by setting HERMES_HOME. See docs/hermes-harness.md.",
        home.display()
    );
    Ok(())
}

pub(super) fn doctor(
    settings: Option<&Path>,
    session: Option<u64>,
    executable: &OsStr,
) -> AppResult<()> {
    no_settings(settings)?;
    let is_executable = |path: &Path| {
        std::fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    };
    let path = Path::new(executable);
    let found = if path.components().count() > 1 {
        is_executable(path)
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .any(|directory| is_executable(&directory.join(path)))
    };
    let mut lifecycle = serde_json::Value::Null;
    let requested = session.or_else(|| std::env::var("OVRCR_SESSION_ID").ok()?.parse().ok());
    let session_status = match requested {
        None => "not_requested",
        Some(id) => match super::inspect() {
            Err(_) => "inspection_unavailable",
            Ok((_, sessions)) => match sessions.iter().find(|session| session.id.0 == id) {
                None => "session_not_found",
                Some(session)
                    if session.kind
                        == (SessionKind::Agent {
                            name: "hermes".into(),
                        }) =>
                {
                    lifecycle = json!({"process": session.phase, "activity": "unknown"});
                    "recognized_agent"
                }
                Some(_) => "different_session_kind",
            },
        },
    };
    // Do not invoke --version: Hermes' native version/startup may check updates or
    // initialize profile state. Filesystem discovery is not native acceptance.
    let home = hermes_home().ok();
    let hooks = home.as_deref().is_some_and(hooks_installed);
    let report = json!({
        "provider": "hermes",
        "executable": executable.to_string_lossy(),
        "executable_status": if found { "found" } else { "missing_or_not_executable" },
        "probe_status": "not_run", "version": null, "version_status": "unverified",
        "tested_versions": [], "source_reviewed_versions": ["0.20.5"],
        "version_gate": "native_passthrough",
        "hooks_installed": hooks,
        "capabilities": {
            "managed_launch": true, "process_lifecycle": "available",
            "reporting": if hooks { "available" } else { "needs_setup" },
            "readiness": if hooks { "available" } else { "needs_setup" },
            "approvals": if hooks { "available" } else { "needs_setup" },
            "questions": "unavailable",
            "recovery": "available",
            "metrics": if hooks { "tokens_only" } else { "needs_setup" },
            "cost": "unavailable",
            "generated_titles": "available",
        },
        "session_status": session_status, "lifecycle": lifecycle,
        "guidance": "Run `ovrcr agent setup hermes` once per Hermes profile (set HERMES_HOME). Hooks report session id, turn activity, approval ids and token counts. Cost and non-approval questions stay unavailable. Titles come from the session row. Resume is `hermes --resume <session id>`. Doctor does not inspect credentials or run Hermes. See docs/hermes-harness.md.",
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(RuntimeError::internal)?
    );
    Ok(())
}

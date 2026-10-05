//! Cursor diagnostics do not run native code or modify provider configuration.
use super::{AppResult, RuntimeError};
use ovrcr::protocol::{AgentProvider, ErrorCode, SessionKind};
use serde_json::json;
use std::{ffi::OsStr, os::unix::fs::PermissionsExt, path::Path};

fn no_settings(settings: Option<&Path>) -> AppResult<()> {
    if settings.is_some() {
        return Err(RuntimeError::new(
            ErrorCode::InvalidRequest,
            "Cursor startup reporting uses a temporary local plugin; --settings is unsupported",
        ));
    }
    Ok(())
}

pub(super) fn setup(settings: Option<&Path>) -> AppResult<()> {
    no_settings(settings)?;
    println!(
        "Cursor startup identity uses a temporary local plugin, with no persistent settings changes. Configure Cursor separately; pick cursor-agent or run `ovrcr agent run cursor-agent -- cursor-agent` inside an OVRCR terminal. Fresh interactive launches (optionally --model VALUE) inspect the installed CLI's --help for local plugin support and validate its native startup identity at runtime. Activity, Ready/Unread, Input, metrics, generated titles and recovery are unavailable. See docs/cursor-harness.md for native acceptance evidence."
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
    let requested = session.or_else(|| std::env::var("OVRCR_SESSION_ID").ok()?.parse().ok());
    let mut binding = serde_json::Value::Null;
    let session_status = match requested {
        None => "not_requested",
        Some(id) => match super::inspect() {
            Err(_) => "inspection_unavailable",
            Ok((_, sessions)) => match sessions.iter().find(|session| session.id.0 == id) {
                None => "session_not_found",
                Some(session)
                    if session.kind
                        != (SessionKind::Agent {
                            name: "cursor-agent".into(),
                        }) =>
                {
                    "different_session_kind"
                }
                Some(session) => match &session.agent {
                    Some(agent) if agent.binding.provider == AgentProvider::Cursor => {
                        binding = json!({"provider":agent.binding.provider,"generation":agent.binding.generation,"health":agent.health.state});
                        "bound"
                    }
                    Some(_) => "different_provider",
                    None => "unbound",
                },
            },
        },
    };
    println!("{}", serde_json::to_string_pretty(&json!({
        "provider":"cursor-agent", "executable":executable.to_string_lossy(),
        "executable_status":if found {"found"} else {"missing_or_not_executable"},
        "probe_status":"not_run", "admission":"runtime_capabilities",
        "capabilities":{"managed_launch":true,"process_lifecycle":"available","startup_identity":"requires_native_hook",
            "reporting":"startup_identity_only","readiness":"unavailable","approvals":"unavailable","questions":"unavailable",
            "recovery":"unavailable","metrics":"unavailable","generated_titles":"unavailable"},
        "session_status":session_status,"binding":binding,"native_acceptance":"unverified",
        "guidance":"Fresh interactive cursor-agent or explicit agent alias only; launch probes --help for local plugin support with a bounded deadline, then validates the native startup hook. No release whitelist. Resume/continue/headless/management/other options remain native with reporting unavailable. Doctor never runs the provider; see docs/cursor-harness.md."
    })).map_err(RuntimeError::internal)?);
    Ok(())
}

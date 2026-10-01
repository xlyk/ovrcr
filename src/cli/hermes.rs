//! Hermes process supervision has no verified native reporting/recovery adapter.
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

pub(super) fn setup(settings: Option<&Path>) -> AppResult<()> {
    no_settings(settings)?;
    println!(
        "Hermes process supervision needs no OVRCR settings or hooks. Configure Hermes separately, then pick hermes in the Dashboard or launch `ovrcr agent run hermes -- hermes [ARGS...]`. Choose a preconfigured Hermes profile per concurrent instance; OVRCR workspaces do not isolate Hermes profiles. Activity/reporting, Ready/Unread, Input requests, metrics, generated titles and recovery are unavailable. Native version/account acceptance is unverified; doctor does not inspect credentials or determine the active profile. See docs/hermes-harness.md."
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
    let report = json!({
        "provider": "hermes",
        "executable": executable.to_string_lossy(),
        "executable_status": if found { "found" } else { "missing_or_not_executable" },
        "probe_status": "not_run", "version": null, "version_status": "unverified",
        "tested_versions": [], "source_reviewed_versions": ["0.20.5"],
        "version_gate": "native_passthrough",
        "capabilities": {
            "managed_launch": true, "process_lifecycle": "available",
            "reporting": "unavailable", "readiness": "unavailable",
            "approvals": "unavailable", "questions": "unavailable",
            "recovery": "unavailable", "metrics": "unavailable", "generated_titles": "unavailable",
        },
        "session_status": session_status, "lifecycle": lifecycle,
        "guidance": "Install/configure Hermes separately. Choose a preconfigured Hermes profile per concurrent instance; OVRCR workspaces do not isolate Hermes profiles. Doctor does not inspect credentials or determine the active profile. OVRCR forwards native argv unchanged and observes process lifecycle only. No native version/account acceptance is claimed; see docs/hermes-harness.md.",
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(RuntimeError::internal)?
    );
    Ok(())
}

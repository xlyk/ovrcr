//! Preview-only Cursor cloud launch. Remote creation and delivery are absent here.
use super::{AppResult, RuntimeError};
use ovrcr::protocol::{AgentProvider, CursorCloudAccount, ErrorCode};
use std::{
    ffi::OsString,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn invalid(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(ErrorCode::InvalidRequest, message)
}
fn accounts(document: Option<&Path>) -> AppResult<Vec<CursorCloudAccount>> {
    let home = ovrcr::config::RegistryPath::resolve()
        .map_err(RuntimeError::internal)?
        .0;
    let report = match document {
        Some(path) => ovrcr_runtime::settings::load_document(&home, path),
        None => ovrcr_runtime::settings::load(&home),
    };
    if report.findings.iter().any(|finding| {
        finding
            .key
            .as_deref()
            .is_some_and(|key| key.starts_with("cursor_cloud_accounts"))
    }) {
        return Err(invalid(
            "invalid cursor_cloud_accounts settings; repair before launch",
        ));
    }
    Ok(report.settings.cursor_cloud_accounts)
}
pub(super) fn setup(document: Option<&Path>) -> AppResult<()> {
    let accounts = accounts(document)?;
    println!(
        "{}",
        serde_json::json!({"provider":"cursor-cloud", "platform_available":cfg!(target_os="macos"), "accounts":accounts,
        "guidance":"macOS only. Configure named Keychain references, explicit model and repository/owner policy in dashboard.toml. Choose cursor-cloud:ACCOUNT in ordinary Agent creation. Preview validates origin and published HEAD; local edits are excluded. Profiles select credentials, not attested billing identities. Creation, Send, terminal responses, Retry and active reattachment remain blocked."})
    );
    Ok(())
}
pub(super) fn doctor(document: Option<&Path>) -> AppResult<()> {
    setup(document)
}

pub(super) fn launch(argv: &[OsString]) -> AppResult<()> {
    if !cfg!(target_os = "macos") {
        return Err(invalid(
            "cursor-cloud unavailable on Linux; macOS Keychain is required",
        ));
    }
    let [account] = argv else {
        return Err(invalid(
            "cursor-cloud requires exactly one named account after --",
        ));
    };
    let account = account
        .to_str()
        .ok_or_else(|| invalid("invalid account name"))?;
    let lease = ovrcr::report::reserve_invocation_for(AgentProvider::CursorCloud)
        .map_err(RuntimeError::internal)?
        .ok_or_else(|| invalid("cursor-cloud requires positive managed admission"))?;
    let executable = std::env::current_exe().map_err(RuntimeError::internal)?;
    let native = vec![
        executable.into_os_string(),
        "agent".into(),
        "cloud-preview".into(),
        account.into(),
    ];
    let status = ovrcr::agent_runner::run_native_admitted(&native, &[], move |available, _, _| {
        anyhow::ensure!(
            available,
            "cursor-cloud invocation channel admission unavailable"
        );
        Ok(Some(ovrcr::report::cursor_cloud::receiver(lease)))
    })
    .map_err(RuntimeError::internal)?;
    use std::os::unix::process::ExitStatusExt;
    if let Some(signal) = status.signal() {
        std::process::exit(128 + signal);
    }
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

pub(super) fn admission() -> AppResult<()> {
    ovrcr::report::send_cloud_admission(Instant::now() + Duration::from_secs(1))
        .map_err(RuntimeError::internal)
}

pub(super) fn preview(account: &str) -> AppResult<()> {
    // A strict descendant proves membership in the currently supervised native tree.
    let status = Command::new(std::env::current_exe().map_err(RuntimeError::internal)?)
        .args(["agent", "cloud-admission"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(RuntimeError::internal)?;
    if !status.success() {
        return Err(invalid("cursor-cloud managed admission unavailable"));
    }
    if !cfg!(target_os = "macos") {
        return Err(invalid(
            "cursor-cloud unavailable on Linux; macOS Keychain is required",
        ));
    }
    let profile = accounts(None)?
        .into_iter()
        .find(|profile| profile.name == account)
        .ok_or_else(|| invalid("named cursor-cloud account is not configured"))?;
    let directory = std::env::current_dir().map_err(RuntimeError::internal)?;
    ovrcr_runtime::cursor_cloud::preview(&profile, &directory).map_err(|error| {
        invalid(format!(
            "Cursor cloud validation failed: {error}. No remote work was created or sent."
        ))
    })?;
    Ok(())
}

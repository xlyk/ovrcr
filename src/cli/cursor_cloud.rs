//! Managed Cursor cloud admission and terminal prompt entry.
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
    let installed_archive = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|home| home.join(".local/lib/ovrcr-cursor-sdk/1.0.40/bridge.tar.gz"));
    let inspect = |path: &Path| match ovrcr_runtime::cursor_cloud::verify_bridge_archive(path) {
        Ok(()) => serde_json::json!({"path":path, "status":"verified"}),
        Err(error) => {
            serde_json::json!({"path":path, "status":"invalid", "error":error.to_string()})
        }
    };
    let dependency = serde_json::json!({
        "sdk_version":ovrcr_runtime::cursor_cloud::SDK_VERSION,
        "bridge_version":"1.0.0", "protocol":"sdk.v1",
        "installed_archive":installed_archive.as_deref().map(inspect),
        "configured_archives":accounts.iter().map(|profile| {
            let mut report = inspect(Path::new(&profile.bridge_archive));
            report["account"] = serde_json::json!(profile.name);
            report
        }).collect::<Vec<_>>(),
        "runtime_checked":false, "release_ready":false,
        "guidance":"Use just install-release, or python3 -I scripts/install-cursor-bridge.py [OFFICIAL_ARCHIVE] from the checkout; set each profile's absolute bridge_archive to the printed path. Passive SHA256 only; bridge/protocol/capabilities are checked on managed launch. No terminal response, safe uncertain Retry or active option-preserving recovery acceptance."
    });
    println!(
        "{}",
        serde_json::json!({"provider":"cursor-cloud", "platform_available":cfg!(target_os="macos"), "accounts":accounts, "bridge_dependency":dependency,
        "guidance":"macOS only. Configure named Keychain references, explicit model and repository/owner policy in dashboard.toml. Choose cursor-cloud:ACCOUNT in ordinary Agent creation. Preview validates origin and published HEAD; local edits are excluded. Profiles select credentials, not attested billing identities. Type prepare in the validated preview to retain a local SDK handle. Send prompts on the retained conversation; /cancel persists cancellation intent before exact live CancelRun, cloud-discard separately removes saved content. Remote work may continue. Reopen reconciles exact retained runs; /retry-cancel explicitly cancels a known recovered run. Terminal responses, uncertain Retry and active reattachment remain blocked."})
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
    if !(1..=2).contains(&argv.len()) {
        return Err(invalid(
            "cursor-cloud requires a named account and optional exact retained recovery target after --",
        ));
    }
    let account = argv[0]
        .to_str()
        .ok_or_else(|| invalid("invalid account name"))?;
    let lease = ovrcr::report::reserve_invocation_for(AgentProvider::CursorCloud)
        .map_err(RuntimeError::internal)?
        .ok_or_else(|| invalid("cursor-cloud requires positive managed admission"))?;
    let executable = std::env::current_exe().map_err(RuntimeError::internal)?;
    let mut native = vec![
        executable.into_os_string(),
        "agent".into(),
        "cloud-preview".into(),
        account.into(),
    ];
    if let [_, recovery] = argv {
        native.extend(["--recovery".into(), recovery.clone()]);
    }
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

pub(super) fn creation() -> AppResult<()> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(RuntimeError::internal)?;
    if bytes.len() > 65536 {
        return Err(invalid("creation frame exceeds bound"));
    }
    ovrcr::report::send_cloud_creation(&bytes, Instant::now() + Duration::from_secs(5))
        .map_err(RuntimeError::internal)
}

pub(super) fn preview(account: &str, recovery: Option<&str>) -> AppResult<()> {
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
    let recovery = recovery
        .map(serde_json::from_str::<ovrcr::protocol::CloudRecovery>)
        .transpose()
        .map_err(RuntimeError::internal)?;
    ovrcr_runtime::cursor_cloud::preview(&profile, &directory, recovery.as_ref(), |command| {
        use std::io::Write;
        let mut child = Command::new(std::env::current_exe()?).args(["agent", "cloud-creation"])
            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
        child.stdin.take().ok_or_else(|| anyhow::anyhow!("creation admission input missing"))?.write_all(&serde_json::to_vec(command)?)?;
        anyhow::ensure!(child.wait()?.success(), "durable creation admission unavailable; original request retained, do not retry");
        Ok(())
    }).map_err(|error| {
        invalid(format!(
            "Cursor cloud validation failed: {error}. Any claimed creation or Send remains retained and may be unresolved; remote work may continue."
        ))
    })?;
    Ok(())
}

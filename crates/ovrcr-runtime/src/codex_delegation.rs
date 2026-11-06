//! Native Codex discovery and effect policy; no session or publication owner.
use anyhow::{Context, Result, bail, ensure};
use ovrcr_protocol::{DelegationChoice, DelegationHarness};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::{Component, Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

pub const PROFILE: &str = "ovrcr-research";
pub const MCP_SERVER: &str = "ovrcr_child";
pub const ARTIFACT_ENV: &str = "OVRCR_DELEGATION_ARTIFACT_ROOT";
const MAX_CATALOG: u64 = 1024 * 1024;
const MAX_DASHBOARD_MODELS: usize = 128;

pub fn executable() -> Result<PathBuf> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let path = directory.join("codex");
        if path.is_absolute()
            && std::fs::metadata(&path)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        {
            path.canonicalize().context("resolve Codex executable")?;
            // Reporter admission requires the entry basename, even for symlinked installs.
            return Ok(path);
        }
    }
    bail!("Codex executable unavailable; no substitute selected")
}

pub fn models() -> Result<Vec<DelegationChoice>> {
    // Bundled discovery never refreshes, authenticates or starts a conversation.
    let executable = executable()?;
    let private = tempfile::Builder::new().prefix("ocdx-").tempdir()?;
    let home = private.path().canonicalize()?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let help = inspect(&executable, &["--help"], &home, deadline)?;
    let help = std::str::from_utf8(&help)?;
    ensure!(
        ["--no-daemon", "--strict-config", "--profile"]
            .iter()
            .all(|flag| help.contains(flag)),
        "Codex required private policy controls unavailable"
    );
    let bytes = inspect(
        &executable,
        &["debug", "models", "--bundled"],
        &home,
        deadline,
    )?;
    parse_models(&bytes)
}

fn inspect(executable: &Path, args: &[&str], home: &Path, deadline: Instant) -> Result<Vec<u8>> {
    let mut output = tempfile::tempfile()?;
    let mut child = Command::new(executable)
        .args([
            "-c",
            "cli_auth_credentials_store=\"ephemeral\"",
            "-c",
            "analytics.enabled=false",
            "-c",
            "check_for_update_on_startup=false",
        ])
        .args(args)
        .env_clear()
        .env("HOME", home)
        .env("CODEX_HOME", home.join("codex"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("TMPDIR", home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let mut unreaped = true;
    let result = (|| -> Result<()> {
        loop {
            if let Some(status) = child.try_wait()? {
                unreaped = false;
                ensure!(status.success(), "Codex catalog/policy inspection failed");
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline && output.metadata()?.len() <= MAX_CATALOG,
                "Codex catalog/policy inspection exceeded bounds"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
    })();
    if unreaped {
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.wait();
    }
    result?;
    ensure!(
        output.metadata()?.len() <= MAX_CATALOG,
        "Codex catalog exceeds byte bound"
    );
    output.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    output.take(MAX_CATALOG + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn parse_models(bytes: &[u8]) -> Result<Vec<DelegationChoice>> {
    ensure!(
        bytes.len() as u64 <= MAX_CATALOG,
        "Codex catalog exceeds byte bound"
    );
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let models = value["models"]
        .as_array()
        .context("Codex catalog missing models")?;
    ensure!(models.len() <= 128, "Codex catalog exceeds model bound");
    let mut choices = Vec::new();
    for model in models {
        let slug = model["slug"]
            .as_str()
            .context("Codex model lacks exact slug")?;
        ensure!(
            !slug.is_empty() && slug.len() <= 256 && !slug.chars().any(char::is_control),
            "invalid Codex model slug"
        );
        if model["visibility"] != "list" {
            continue;
        }
        choices.push(DelegationChoice {
            provider: DelegationHarness::Codex,
            model_provider: "openai".into(),
            model: slug.into(),
        });
    }
    choices.sort_by(|a, b| a.model.cmp(&b.model));
    choices.dedup();
    ensure!(
        !choices.is_empty(),
        "Codex catalog has no listed supported model"
    );
    Ok(choices)
}

/// Parse the visible response from the native app-server `model/list` path.
/// `model` is Codex's model slug and the value accepted by its native `--model`
/// option; the stable preset `id` is not an invocation argument.
fn parse_dashboard_models(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    ensure!(
        bytes.len() as u64 <= MAX_CATALOG,
        "Codex dashboard catalog exceeds byte bound"
    );
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let models = value["data"]
        .as_array()
        .context("Codex model/list response missing data")?;
    ensure!(
        models.len() <= MAX_DASHBOARD_MODELS,
        "Codex dashboard catalog exceeds model bound"
    );
    ensure!(
        value
            .get("nextCursor")
            .is_none_or(serde_json::Value::is_null),
        "Codex dashboard catalog exceeds model bound"
    );

    let mut seen = std::collections::HashSet::new();
    let mut choices = Vec::new();
    for model in models {
        let hidden = model["hidden"]
            .as_bool()
            .context("Codex model/list entry lacks hidden state")?;
        if hidden {
            continue;
        }
        let id = model["id"]
            .as_str()
            .context("Codex model/list entry lacks stable id")?;
        let slug = model["model"]
            .as_str()
            .context("Codex model/list entry lacks model slug")?;
        ensure!(
            !id.is_empty()
                && id.len() <= 256
                && !id.chars().any(char::is_control)
                && !slug.is_empty()
                && slug.len() <= 256
                && !slug.chars().any(char::is_control),
            "Codex model/list entry has an invalid id or model slug"
        );
        ensure!(
            seen.insert(slug.to_owned()),
            "Codex model/list response contains a duplicate model slug"
        );
        choices.push((slug.to_owned(), slug.to_owned()));
    }
    choices.sort_by(|left, right| left.0.cmp(&right.0));
    ensure!(
        !choices.is_empty(),
        "Codex model/list has no visible selectable models"
    );
    Ok(choices)
}

fn dashboard_plugins_override_is_compatible(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() as u64 <= MAX_CATALOG,
        "Codex startup requirements exceed byte bound"
    );
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let requirements = value
        .get("configRequirements")
        .and_then(serde_json::Value::as_object)
        .context("Codex startup requirements are unavailable")?
        .get("requirements")
        .context("Codex startup requirements are unavailable")?;
    let Some(requirements) = requirements.as_object() else {
        ensure!(
            requirements.is_null(),
            "Codex startup requirements are unavailable"
        );
        return Ok(());
    };
    let Some(features) = requirements.get("featureRequirements") else {
        return Ok(());
    };
    let features = features
        .as_object()
        .context("Codex managed feature requirements are unavailable")?;
    ensure!(
        features.get("plugins").is_none_or(|value| value == false),
        "Codex managed policy requires plugins; safe model discovery is unavailable"
    );
    Ok(())
}

fn dashboard_gateway_oauth_is_ready(value: &serde_json::Value) -> Result<()> {
    let required = value
        .get("required")
        .and_then(serde_json::Value::as_bool)
        .context("Codex gateway OAuth status is unavailable")?;
    let status = value
        .get("status")
        .context("Codex gateway OAuth status is unavailable")?;
    if required {
        ensure!(
            status.as_str() == Some("succeeded"),
            "Codex gateway OAuth credentials are not ready"
        );
    } else {
        ensure!(
            status.is_null() || status.as_str() == Some("succeeded"),
            "Codex gateway OAuth credentials are not ready"
        );
    }
    Ok(())
}

fn dashboard_startup_requirements(
    executable: &Path,
    cwd: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<u8>> {
    let mut output = tempfile::tempfile()?;
    let mut command = Command::new(executable);
    command
        .args([
            "-c",
            "features.plugins=false",
            "app-server",
            "--print-startup-config",
        ])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::null())
        .process_group(0);
    remove_delegation_authority(&mut command);
    let mut child = command
        .spawn()
        .context("Codex startup requirements unavailable")?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let result = (|| -> Result<()> {
        loop {
            ensure!(
                !cancelled.load(std::sync::atomic::Ordering::Acquire),
                "Codex model discovery cancelled"
            );
            if app_server_exited(&child)? {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline && output.metadata()?.len() <= MAX_CATALOG,
                "Codex startup requirements exceeded bounds"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
    })();
    let cleanup = terminate_app_server_group(&mut child);
    match (result, cleanup) {
        (Ok(()), Ok(status)) if status.success() => {}
        (Err(error), Ok(_)) => return Err(error),
        (Ok(()), Ok(_)) => bail!("Codex startup requirements are unavailable"),
        (_, Err(error)) => return Err(error),
    }
    ensure!(
        output.metadata()?.len() <= MAX_CATALOG,
        "Codex startup requirements exceed byte bound"
    );
    output.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    output.take(MAX_CATALOG + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn remove_delegation_authority(command: &mut Command) {
    for name in [
        "OVRCR_AGENT_SOCKET",
        "OVRCR_AGENT_TOKEN",
        "OVRCR_HOOK_SOCKET",
        "OVRCR_HOOK_TOKEN",
        "OVRCR_SESSION_ID",
        "OVRCR_DELEGATION_SESSION",
        "OVRCR_DELEGATION_TOKEN",
        "OVRCR_DELEGATION_SOCKET",
        "OVRCR_DELEGATION_HOME",
        ARTIFACT_ENV,
    ] {
        command.env_remove(name);
    }
}

/// Query the native interactive catalog under the selected detected Codex
/// launcher. This deliberately does not reuse `models()`: that private,
/// bundled catalog belongs to the delegation authority and cannot represent
/// the user's effective provider or cloud policy.
pub(crate) fn dashboard_catalog(
    executable: &Path,
    native_argv: &[std::ffi::OsString],
    cwd: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<Vec<(String, String)>> {
    validate_dashboard_native_argv(executable, native_argv)?;
    ensure!(cwd.is_absolute(), "Codex workspace path is not absolute");

    let requirements = dashboard_startup_requirements(executable, cwd, cancelled)?;
    dashboard_plugins_override_is_compatible(&requirements)?;

    let mut command = Command::new(executable);
    command
        .args(["-c", "features.plugins=false", "app-server", "--stdio"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    // Keep Codex's native HOME/CODEX_HOME, provider environment, cloud policy
    // and authentication. Remove only OVRCR capabilities that could turn this
    // catalog request into a delegated session or reporter invocation.
    remove_delegation_authority(&mut command);

    let mut child = command
        .spawn()
        .context("Codex native model discovery unavailable")?;
    let result = (|| -> Result<Vec<(String, String)>> {
        let mut stdin = child
            .stdin
            .take()
            .context("Codex app-server stdin unavailable")?;
        let mut stdout = child
            .stdout
            .take()
            .context("Codex app-server stdout unavailable")?;
        let flags = unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_GETFL) };
        ensure!(
            flags >= 0
                && unsafe {
                    libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK)
                } == 0,
            "Codex app-server output pipe unavailable"
        );

        let deadline = Instant::now() + Duration::from_secs(20);
        let mut pending = Vec::new();
        let mut output_bytes = 0usize;
        write_jsonrpc_message(
            &mut stdin,
            &serde_json::json!({
                "id":1,
                "method":"initialize",
                "params":{
                    "clientInfo":{"name":"ovrcr-model-catalog","version":env!("CARGO_PKG_VERSION")},
                    "capabilities":{"explicitGatewayOauth":true}
                }
            }),
        )?;
        let _initialized = read_jsonrpc_response(
            &mut child,
            &mut stdout,
            &mut pending,
            &mut output_bytes,
            1,
            deadline,
            cancelled,
        )?;
        write_jsonrpc_message(&mut stdin, &serde_json::json!({"method":"initialized"}))?;
        write_jsonrpc_message(
            &mut stdin,
            &serde_json::json!({"id":2,"method":"account/gatewayOAuth/read"}),
        )?;
        let gateway = read_jsonrpc_response(
            &mut child,
            &mut stdout,
            &mut pending,
            &mut output_bytes,
            2,
            deadline,
            cancelled,
        )?;
        dashboard_gateway_oauth_is_ready(&gateway)?;
        write_jsonrpc_message(
            &mut stdin,
            &serde_json::json!({
                "id":3,
                "method":"model/list",
                "params":{"cursor":null,"limit":MAX_DASHBOARD_MODELS,"includeHidden":false}
            }),
        )?;
        let response = read_jsonrpc_response(
            &mut child,
            &mut stdout,
            &mut pending,
            &mut output_bytes,
            3,
            deadline,
            cancelled,
        )?;
        parse_dashboard_models(&serde_json::to_vec(&response)?)
    })();

    let cleanup = terminate_app_server_group(&mut child);
    match (result, cleanup) {
        (Ok(models), Ok(status)) if status.success() => Ok(models),
        (Err(error), Ok(_)) => Err(error),
        (Ok(_), Ok(_)) => bail!("Codex app-server exited unsuccessfully after model lookup"),
        (_, Err(cleanup_error)) => Err(cleanup_error),
    }
}

fn validate_dashboard_native_argv(
    executable: &Path,
    native_argv: &[std::ffi::OsString],
) -> Result<()> {
    let expected_executable = executable.canonicalize()?;
    let selected_executable = native_argv
        .first()
        .and_then(|arg| Path::new(arg).canonicalize().ok())
        .context("Codex native executable is unavailable")?;
    ensure!(
        selected_executable == expected_executable,
        "Codex native executable changed"
    );
    let mut expected = vec![executable.as_os_str().to_owned()];
    ovrcr_protocol::apply_default_auto_trust("codex", &mut expected);
    ensure!(
        native_argv.len() == expected.len() && native_argv[1..] == expected[1..],
        "Codex detected launch has unsupported profile, model, or config arguments"
    );
    Ok(())
}

fn write_jsonrpc_message(
    stdin: &mut std::process::ChildStdin,
    message: &serde_json::Value,
) -> Result<()> {
    serde_json::to_writer(&mut *stdin, message)?;
    stdin.write_all(b"\n")?;
    stdin.flush()?;
    Ok(())
}

fn read_jsonrpc_response(
    child: &mut Child,
    stdout: &mut ChildStdout,
    pending: &mut Vec<u8>,
    output_bytes: &mut usize,
    request_id: u64,
    deadline: Instant,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<serde_json::Value> {
    let mut buffer = [0; 8192];
    loop {
        ensure!(
            !cancelled.load(std::sync::atomic::Ordering::Acquire),
            "Codex model discovery cancelled"
        );
        ensure!(Instant::now() < deadline, "Codex model discovery deadline");
        match stdout.read(&mut buffer) {
            Ok(0) => {}
            Ok(count) => {
                *output_bytes = output_bytes.saturating_add(count);
                ensure!(
                    *output_bytes <= 512 * 1024,
                    "Codex model discovery output bound"
                );
                pending.extend_from_slice(&buffer[..count]);
                ensure!(
                    pending.len() <= 128 * 1024 || pending.contains(&b'\n'),
                    "Codex model discovery line bound"
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => anyhow::bail!("Codex model discovery output unavailable"),
        }

        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            ensure!(end <= 128 * 1024, "Codex model discovery line bound");
            let line = pending.drain(..=end).collect::<Vec<_>>();
            let line = &line[..line.len() - 1];
            if line.is_empty() {
                continue;
            }
            let message: serde_json::Value =
                serde_json::from_slice(line).context("invalid Codex app-server JSON")?;
            if message.get("id").and_then(serde_json::Value::as_u64) != Some(request_id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let code = error.get("code").and_then(serde_json::Value::as_i64);
                anyhow::bail!("Codex app-server refused model lookup (error code {code:?})");
            }
            return message
                .get("result")
                .cloned()
                .context("Codex app-server response lacks a result");
        }

        if app_server_exited(child)? {
            anyhow::bail!("Codex app-server exited before returning the model catalog");
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}

fn app_server_exited(child: &Child) -> Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let status = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    ensure!(status == 0, "Codex app-server ownership probe failed");
    let exited_pid = unsafe { info.si_pid() };
    ensure!(
        exited_pid == 0 || exited_pid == child.id() as libc::pid_t,
        "Codex app-server ownership is uncertain"
    );
    Ok(exited_pid != 0)
}

fn terminate_app_server_group(child: &mut Child) -> Result<std::process::ExitStatus> {
    let pgid = child.id() as i32;
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut leader_exited = false;
    while Instant::now() < deadline {
        if app_server_exited(child)? {
            leader_exited = true;
            break;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    if !leader_exited {
        let _term = signal_app_server_group(pgid, libc::SIGTERM);
        let deadline = Instant::now() + Duration::from_millis(150);
        while Instant::now() < deadline {
            if app_server_exited(child)? {
                leader_exited = true;
                break;
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
        if !leader_exited {
            let _kill = signal_app_server_group(pgid, libc::SIGKILL);
            let deadline = Instant::now() + Duration::from_millis(500);
            while Instant::now() < deadline {
                if app_server_exited(child)? {
                    leader_exited = true;
                    break;
                }
                std::thread::park_timeout(Duration::from_millis(10));
            }
        }
    }
    ensure!(
        leader_exited,
        "Codex app-server leader did not exit after bounded group cleanup"
    );
    // A failed group signal while the leader is an unreaped zombie is
    // inconclusive: on macOS, killpg can report EPERM for a group containing
    // only that zombie. Reap the exact owned leader, then verify group absence.
    let _group_kill = signal_app_server_group(pgid, libc::SIGKILL);
    // The leader stays unreaped until every signal is done. If it exits on EOF
    // or TERM, the final group signal is still before reap and is followed by
    // an absence probe after reap.
    let status = child.wait().context("Codex app-server child wait failed")?;
    wait_app_server_group_absent(pgid)?;
    Ok(status)
}

fn signal_app_server_group(pgid: i32, signal: i32) -> Result<()> {
    let sent = unsafe { libc::kill(-pgid, signal) };
    let errno = if sent == -1 {
        std::io::Error::last_os_error().raw_os_error()
    } else {
        None
    };
    ensure!(
        sent == 0 || (sent == -1 && errno == Some(libc::ESRCH)),
        "Codex app-server cleanup is uncertain (process group {pgid}, observed {}, signal {signal}, errno {errno:?})",
        unsafe { libc::getpgid(pgid) }
    );
    Ok(())
}

fn wait_app_server_group_absent(pgid: i32) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let present = unsafe { libc::kill(-pgid, 0) };
        let errno = if present == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        if present == -1 && errno == Some(libc::ESRCH) {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "Codex app-server process group is still present (probe errno {errno:?})"
        );
        std::thread::park_timeout(Duration::from_millis(10));
    }
}

/// Only the task artifact directory is readable; the instance and native home
/// remain denied even when a selected workspace contains either one.
pub fn profile(
    workspace: &Path,
    artifacts: &Path,
    instance: &Path,
    home: &Path,
    binary: &Path,
) -> Result<String> {
    for path in [workspace, artifacts, instance, home, binary] {
        ensure!(
            path.is_absolute()
                && path
                    .components()
                    .all(|c| !matches!(c, Component::ParentDir | Component::CurDir)),
            "Codex policy requires canonical absolute paths"
        );
    }
    ensure!(
        artifacts.parent() == Some(instance.join("workflows").as_path()),
        "foreign assignment artifact root"
    );
    ensure!(
        home.parent() == Some(instance.join("codex-children").as_path())
            && home.file_name() == artifacts.file_name(),
        "foreign private Codex home"
    );
    ensure!(
        !artifacts.starts_with(home) && !home.starts_with(artifacts),
        "native home must be outside readable artifacts"
    );
    let mut fs = toml::map::Map::new();
    for (path, access) in [
        (":root".to_owned(), "deny"),
        (":minimal".to_owned(), "read"),
        (workspace.to_string_lossy().into_owned(), "read"),
        (instance.to_string_lossy().into_owned(), "deny"),
        (artifacts.to_string_lossy().into_owned(), "read"),
        (home.to_string_lossy().into_owned(), "deny"),
    ] {
        ensure!(
            fs.insert(path, toml::Value::String(access.into()))
                .is_none(),
            "overlapping native policy roots"
        );
    }
    let value = serde_json::json!({
        "approval_policy":"never", "default_permissions":"ovrcr-research", "allow_login_shell":false,
        "check_for_update_on_startup":false, "cli_auth_credentials_store":"file", "project_root_markers":[], "web_search":"disabled",
        "analytics":{"enabled":false},
        "features":{"apps":false,"plugins":false,"browser_use":false,"computer_use":false,"in_app_browser":false,"image_generation":false,"goals":false,"memories":false,"shell_snapshot":false,"workspace_dependencies":false,"tool_suggest":false,"request_permissions_tool":false},
        "projects":{workspace.to_string_lossy().as_ref():{"trust_level":"untrusted"}},
        "shell_environment_policy":{"inherit":"none","ignore_default_excludes":false,"set":{"PATH":"/usr/bin:/bin","HOME":workspace.to_string_lossy()}},
        "permissions":{"ovrcr-research":{"filesystem":fs,"network":{"enabled":false}}},
        "mcp_servers":{MCP_SERVER:{"command":binary.to_string_lossy(),"args":["delegation-publisher"],"enabled":true,"required":true,"enabled_tools":["ovrcr_publish","ovrcr_publish_question"],"env_vars":["OVRCR_DELEGATION_SESSION","OVRCR_DELEGATION_TOKEN","OVRCR_DELEGATION_SOCKET","OVRCR_DELEGATION_HOME"],"startup_timeout_sec":10,"tool_timeout_sec":15,"default_tools_approval_mode":"approve"}}
    });
    Ok(toml::to_string_pretty(&value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_catalog_parser_uses_exact_native_model_and_rejects_bad_catalogs() {
        let choices = parse_dashboard_models(
            br#"{"data":[{"id":"gpt-5.4","model":"gpt-5.4","hidden":false,"isDefault":true},{"id":"hidden","model":"hidden-model","hidden":true,"isDefault":false}],"nextCursor":null}"#,
        )
        .unwrap();
        assert_eq!(choices, vec![("gpt-5.4".into(), "gpt-5.4".into())]);

        for bytes in [
            b"{}".as_slice(),
            br#"{"data":[{"id":"gpt-5.4","hidden":false}],"nextCursor":null}"#,
            br#"{"data":[{"id":"gpt-5.4","model":"gpt-5.4","hidden":false}],"nextCursor":"1"}"#,
            br#"{"data":[{"id":"","model":"gpt-5.4","hidden":false}],"nextCursor":null}"#,
        ] {
            assert!(parse_dashboard_models(bytes).is_err());
        }
        assert!(parse_dashboard_models(&vec![b' '; MAX_CATALOG as usize + 1]).is_err());
    }

    #[test]
    fn dashboard_startup_requirements_only_allow_a_proven_plugins_off_override() {
        for bytes in [
            br#"{"appServerVersion":"0.164.0","configRequirements":{"requirements":null}}"#.as_slice(),
            br#"{"appServerVersion":"0.164.0","configRequirements":{"requirements":{"featureRequirements":{"plugins":false}}}}"#,
            br#"{"appServerVersion":"0.164.0","configRequirements":{"requirements":{"featureRequirements":{"other":true}}}}"#,
            br#"{"appServerVersion":"0.164.0","configRequirements":{"requirements":{}}}"#,
        ] {
            assert!(dashboard_plugins_override_is_compatible(bytes).is_ok());
        }
        for bytes in [
            b"{}".as_slice(),
            br#"{"configRequirements":{"requirements":{"featureRequirements":{"plugins":true}}}}"#,
            br#"{"configRequirements":{"requirements":{"featureRequirements":true}}}"#,
            br#"{"configRequirements":{"requirements":42}}"#,
        ] {
            assert!(dashboard_plugins_override_is_compatible(bytes).is_err());
        }
    }

    #[test]
    fn dashboard_gateway_probe_requires_supported_and_ready_gateway_auth() {
        assert!(
            dashboard_gateway_oauth_is_ready(
                &serde_json::json!({"required": false, "status": null})
            )
            .is_ok()
        );
        assert!(
            dashboard_gateway_oauth_is_ready(
                &serde_json::json!({"required": true, "status": "succeeded"})
            )
            .is_ok()
        );
        for value in [
            serde_json::json!({"required": true, "status": "notReady"}),
            serde_json::json!({"required": true, "status": "failed"}),
            serde_json::json!({"status": null}),
            serde_json::json!({"required": false}),
        ] {
            assert!(dashboard_gateway_oauth_is_ready(&value).is_err());
        }
    }

    #[test]
    fn dashboard_catalog_uses_effective_app_server_without_starting_a_thread() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::AtomicBool;

        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let executable = root.path().join("codex");
        std::fs::write(
            &executable,
            "#!/bin/sh\nif [ \"$4\" = \"--print-startup-config\" ]; then\nprintf '%s\\n' \"$@\" > startup-config-args\nprintf '%s\\n' '{\"appServerVersion\":\"0.164.0\",\"configRequirements\":{\"requirements\":null}}'\nexit 0\nfi\nprintf started > regular-app-server\nprintf '%s\\n' \"$@\" > app-server-args\nprintf '%s\\n' \"$PWD\" > app-server-cwd\nwhile IFS= read -r line; do\n  printf '%s\\n' \"$line\" >> app-server-requests\n  case \"$line\" in\n    *'\"method\":\"initialize\"'*) printf '%s\\n' '{\"id\":1,\"result\":{\"serverInfo\":{\"name\":\"Codex\",\"version\":\"0.164.0\"}}}' ;;\n    *'\"method\":\"account/gatewayOAuth/read\"'*) printf '%s\\n' '{\"id\":2,\"result\":{\"required\":false,\"status\":null}}' ;;\n    *'\"method\":\"model/list\"'*) printf '%s\\n' '{\"id\":3,\"result\":{\"data\":[{\"id\":\"preset-gpt-5.4\",\"model\":\"gpt-5.4\",\"hidden\":false,\"isDefault\":true}],\"nextCursor\":null}}' ;;\n  esac\ndone\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let mut native_argv = vec![executable.as_os_str().to_owned()];
        ovrcr_protocol::apply_default_auto_trust("codex", &mut native_argv);

        let choices = dashboard_catalog(
            &executable,
            &native_argv,
            &workspace,
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(choices, vec![("gpt-5.4".into(), "gpt-5.4".into())]);
        assert_eq!(
            std::fs::read_to_string(workspace.join("app-server-args")).unwrap(),
            "-c\nfeatures.plugins=false\napp-server\n--stdio\n"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("startup-config-args")).unwrap(),
            "-c\nfeatures.plugins=false\napp-server\n--print-startup-config\n"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("app-server-cwd")).unwrap(),
            format!("{}\n", workspace.canonicalize().unwrap().display())
        );
        let requests = std::fs::read_to_string(workspace.join("app-server-requests")).unwrap();
        let methods = requests
            .lines()
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line).unwrap()["method"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            methods,
            [
                "initialize",
                "initialized",
                "account/gatewayOAuth/read",
                "model/list"
            ]
        );
        assert!(!requests.contains("thread/start"));
        assert!(!requests.contains("turn/start"));
        let initialize: serde_json::Value =
            serde_json::from_str(requests.lines().next().unwrap()).unwrap();
        assert_eq!(
            initialize["params"]["capabilities"]["explicitGatewayOauth"],
            true
        );
        let model_list: serde_json::Value =
            serde_json::from_str(requests.lines().nth(3).unwrap()).unwrap();
        assert_eq!(model_list["params"]["includeHidden"], false);
        assert_eq!(model_list["params"]["limit"], 128);
    }

    #[test]
    fn dashboard_catalog_refuses_managed_plugins_enabled_before_app_server_warmup() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::AtomicBool;

        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let executable = root.path().join("codex");
        std::fs::write(
            &executable,
            "#!/bin/sh\nif [ \"$4\" = \"--print-startup-config\" ]; then\nprintf '%s\\n' '{\"appServerVersion\":\"0.164.0\",\"configRequirements\":{\"requirements\":{\"featureRequirements\":{\"plugins\":true}}}}'\nexit 0\nfi\nprintf started > regular-app-server\nexit 0\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let mut native_argv = vec![executable.as_os_str().to_owned()];
        ovrcr_protocol::apply_default_auto_trust("codex", &mut native_argv);

        let result = dashboard_catalog(
            &executable,
            &native_argv,
            &workspace,
            &AtomicBool::new(false),
        );

        assert!(result.is_err());
        assert!(!workspace.join("regular-app-server").exists());
    }

    #[test]
    fn dashboard_catalog_cancellation_terminates_its_owned_process_group() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::{AtomicBool, Ordering};

        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let executable = root.path().join("codex");
        let pid_file = workspace.join("app-server-pid");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\nif [ \"$4\" = \"--print-startup-config\" ]; then printf '%s\\n' '{{\"appServerVersion\":\"0.164.0\",\"configRequirements\":{{\"requirements\":null}}}}'; exit 0; fi\nprintf '%s' \"$$\" > '{}'\nexec /bin/sleep 30\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let mut native_argv = vec![executable.as_os_str().to_owned()];
        ovrcr_protocol::apply_default_auto_trust("codex", &mut native_argv);
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let worker_cancelled = std::sync::Arc::clone(&cancelled);
        let worker_executable = executable.clone();
        let worker_workspace = workspace.clone();
        let worker_argv = native_argv.clone();
        let worker = std::thread::spawn(move || {
            dashboard_catalog(
                &worker_executable,
                &worker_argv,
                &worker_workspace,
                &worker_cancelled,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !pid_file.exists() {
            assert!(Instant::now() < deadline, "fake app-server did not start");
            std::thread::park_timeout(Duration::from_millis(5));
        }
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        assert_eq!(unsafe { libc::getpgid(pid) }, pid);
        cancelled.store(true, Ordering::Release);
        let error = worker.join().unwrap().unwrap_err().to_string();
        assert!(error.contains("cancelled"), "unexpected result: {error}");
        assert_eq!(unsafe { libc::kill(-pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    fn dashboard_catalog_removes_residual_same_group_child_before_reaping_leader() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::AtomicBool;

        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let executable = root.path().join("codex");
        let leader_file = workspace.join("leader-pid");
        let child_file = workspace.join("child-pid");
        std::fs::write(
            &executable,
            format!(
                "#!/bin/sh\nif [ \"$4\" = \"--print-startup-config\" ]; then printf '%s\\n' '{{\"appServerVersion\":\"0.164.0\",\"configRequirements\":{{\"requirements\":null}}}}'; exit 0; fi\nprintf '%s' \"$$\" > '{}'\n/bin/sleep 30 </dev/null >/dev/null 2>&1 &\nchild=$!\nprintf '%s' \"$child\" > '{}'\ntrap 'kill \"$child\" 2>/dev/null; wait \"$child\" 2>/dev/null; exit 0' TERM\nwhile IFS= read -r line; do\ncase \"$line\" in\n*'\"method\":\"initialize\"'*) printf '%s\\n' '{{\"id\":1,\"result\":{{\"serverInfo\":{{\"name\":\"Codex\",\"version\":\"0.164.0\"}}}}}}' ;;\n*'\"method\":\"account/gatewayOAuth/read\"'*) printf '%s\\n' '{{\"id\":2,\"result\":{{\"required\":false,\"status\":null}}}}' ;;\n*'\"method\":\"model/list\"'*) printf '%s\\n' '{{\"id\":3,\"result\":{{\"data\":[{{\"id\":\"preset-gpt-5.4\",\"model\":\"gpt-5.4\",\"hidden\":false,\"isDefault\":true}}],\"nextCursor\":null}}}}' ;;\nesac\ndone\nwait \"$child\"\nexit 0\n",
                leader_file.display(),
                child_file.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let mut native_argv = vec![executable.as_os_str().to_owned()];
        ovrcr_protocol::apply_default_auto_trust("codex", &mut native_argv);

        let models = dashboard_catalog(
            &executable,
            &native_argv,
            &workspace,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(models, vec![("gpt-5.4".into(), "gpt-5.4".into())]);
        let leader: i32 = std::fs::read_to_string(leader_file)
            .unwrap()
            .parse()
            .unwrap();
        let child: i32 = std::fs::read_to_string(child_file)
            .unwrap()
            .parse()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let group_absent = unsafe { libc::kill(-leader, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
            let child_absent = unsafe { libc::kill(child, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
            if group_absent && child_absent {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owned Codex process group remained after catalog cleanup"
            );
            std::thread::park_timeout(Duration::from_millis(5));
        }
    }

    #[test]
    fn dashboard_catalog_refuses_profile_or_model_overrides_before_app_server_start() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::AtomicBool;

        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let executable = root.path().join("codex");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf started > app-server-started\nexit 1\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();

        for args in [
            vec!["--profile", "named"],
            vec!["--model", "gpt-5.4"],
            vec!["-c", "model_provider='other'"],
        ] {
            let mut native_argv = vec![executable.as_os_str().to_owned()];
            native_argv.extend(args.iter().map(std::ffi::OsString::from));
            assert!(
                dashboard_catalog(
                    &executable,
                    &native_argv,
                    &workspace,
                    &AtomicBool::new(false),
                )
                .is_err()
            );
        }
        assert!(!workspace.join("app-server-started").exists());
    }

    #[test]
    fn catalog_keeps_exact_listed_choices_and_refuses_malformed_or_oversized_data() {
        let choices = parse_models(br#"{"models":[{"slug":"exact-model","visibility":"list"},{"slug":"hidden","visibility":"hide"}]}"#).unwrap();
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].model, "exact-model");
        assert_eq!(choices[0].model_provider, "openai");
        for bytes in [
            b"{}".as_slice(),
            br#"{"models":[{"visibility":"list"}]}"#,
            br#"{"models":[{"slug":"","visibility":"list"}]}"#,
        ] {
            assert!(parse_models(bytes).is_err());
        }
        assert!(parse_models(&vec![b' '; MAX_CATALOG as usize + 1]).is_err());
    }
    #[test]
    fn profile_is_read_only_private_and_exposes_only_scoped_publication() {
        let text = profile(
            Path::new("/work"),
            Path::new("/instance/workflows/task"),
            Path::new("/instance"),
            Path::new("/instance/codex-children/task"),
            Path::new("/bin/ovrcr"),
        )
        .unwrap();
        let v: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(v["approval_policy"].as_str(), Some("never"));
        assert!(v.get("sandbox_mode").is_none());
        let fs = v["permissions"][PROFILE]["filesystem"].as_table().unwrap();
        assert!(fs.values().all(|v| v.as_str() != Some("write")));
        assert_eq!(fs["/instance"].as_str(), Some("deny"));
        assert_eq!(fs["/instance/codex-children/task"].as_str(), Some("deny"));
        assert_eq!(fs["/instance/workflows/task"].as_str(), Some("read"));
        assert_eq!(
            v["permissions"][PROFILE]["network"]["enabled"].as_bool(),
            Some(false)
        );
        assert_eq!(
            v["mcp_servers"][MCP_SERVER]["enabled_tools"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            v["mcp_servers"][MCP_SERVER]["env_vars"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert!(!text.contains("capability"));
        assert!(
            profile(
                Path::new("/work"),
                Path::new("/instance/workflows"),
                Path::new("/instance"),
                Path::new("/instance/codex-children/task"),
                Path::new("/bin/ovrcr")
            )
            .is_err()
        );
    }
}

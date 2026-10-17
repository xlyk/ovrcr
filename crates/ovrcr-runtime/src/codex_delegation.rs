//! Native Codex discovery and effect policy; no session or publication owner.
use anyhow::{Context, Result, bail, ensure};
use ovrcr_protocol::{DelegationChoice, DelegationHarness};
use std::{
    io::{Read, Seek, SeekFrom},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const PROFILE: &str = "ovrcr-research";
pub const MCP_SERVER: &str = "ovrcr_child";
pub const ARTIFACT_ENV: &str = "OVRCR_DELEGATION_ARTIFACT_ROOT";
const MAX_CATALOG: u64 = 1024 * 1024;

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

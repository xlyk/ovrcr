//! Exact Codex recovery through the shared managed launch path. No history discovery.
use anyhow::{Context, Result, bail};
use ovrcr_protocol::CodexConversation;
use std::{
    ffi::OsString,
    io::{BufRead, BufReader, Read},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub fn config_dir() -> Result<PathBuf> {
    let path = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .context("Codex configuration directory is unavailable")?;
    if !path.is_absolute() {
        bail!("Codex configuration directory must be absolute for recovery");
    }
    Ok(path)
}

/// Keep only options supported by both fresh and exact resume launches. Prompts
/// and inline configuration never enter the durable metadata store.
pub fn launch_options(argv: &[OsString]) -> Result<Vec<String>> {
    let mut options = Vec::new();
    let mut args = argv.iter().skip(1);
    while let Some(arg) = args.next() {
        let arg = arg.to_str().context("non-UTF8 Codex option")?;
        match arg {
            "--" => break,
            "--no-alt-screen" | "--approve-for-me" | "--full-auto" => options.push(arg.into()),
            "--model" | "-m" | "--profile" | "-p" | "--sandbox" | "-s" | "--ask-for-approval"
            | "-a" | "--cd" | "-C" => {
                let value = args
                    .next()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty() && !s.starts_with('-'))
                    .context("missing Codex option value")?;
                if matches!(arg, "--cd" | "-C") && !Path::new(value).is_absolute() {
                    bail!("Codex working directory must be absolute for recovery");
                }
                options.extend([arg.into(), value.into()]);
            }
            value if !value.starts_with('-') => {} // task prompt
            _ => bail!("Codex launch configuration cannot be retained as non-secret references"),
        }
    }
    Ok(options)
}

pub fn is_exact_identity(identity: &str) -> bool {
    let id = identity.as_bytes();
    id.len() == 36
        && id.iter().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                *b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(b)
            }
        })
}

pub fn validate(reference: &CodexConversation) -> Result<()> {
    ovrcr_protocol::validate_agent_id(&reference.conversation)?;
    if !reference.executable.is_absolute()
        || !reference.config_dir.is_absolute()
        || reference.history.as_ref().is_some_and(|p| !p.is_absolute())
    {
        bail!("Codex recovery references must be absolute");
    }
    if reference.options.len() > 32
        || reference
            .options
            .iter()
            .any(|s| s.len() > 4096 || s.chars().any(char::is_control))
    {
        bail!("Codex recovery configuration exceeds limits");
    }
    let mut argv = vec![reference.executable.clone().into_os_string()];
    argv.extend(reference.options.iter().map(OsString::from));
    if launch_options(&argv)? != reference.options {
        bail!("invalid retained Codex configuration");
    }
    Ok(())
}

/// Read only the exact hook-named file's bounded metadata header. This is not a
/// transcript collector, and never searches for another conversation.
pub fn validate_history(reference: &CodexConversation) -> Result<()> {
    let history = reference
        .history
        .as_ref()
        .context("Codex conversation has no native history file")?;
    let file = std::fs::File::open(history)
        .context("Codex conversation history is unavailable; restore it before Retry")?;
    if !file.metadata()?.is_file() {
        bail!("Codex history is not a regular file");
    }
    let mut header = String::new();
    BufReader::new(file.take(1024 * 1024)).read_line(&mut header)?;
    let header: serde_json::Value =
        serde_json::from_str(&header).context("Codex history header is invalid")?;
    if header["type"] != "session_meta"
        || header["payload"]["id"] != reference.conversation
        || header["payload"]["source"] != "cli"
    {
        bail!("Codex history identity does not match the recorded conversation");
    }
    Ok(())
}

pub fn resume_argv(reference: &CodexConversation) -> Result<Vec<OsString>> {
    validate(reference)?;
    if !is_exact_identity(&reference.conversation) {
        bail!("Codex recovery requires an exact canonical UUID");
    }
    if config_dir()? != reference.config_dir || !reference.config_dir.is_dir() {
        bail!(
            "Codex configuration changed or is unavailable; restore the recorded CODEX_HOME before Retry"
        );
    }
    let executable = std::fs::metadata(&reference.executable)
        .context("Codex executable is unavailable; restore it before Retry")?;
    if !executable.is_file() || executable.permissions().mode() & 0o111 == 0 {
        bail!("Codex executable is not executable; restore it before Retry");
    }
    for pair in reference.options.windows(2) {
        if matches!(pair[0].as_str(), "--cd" | "-C") && !Path::new(&pair[1]).is_dir() {
            bail!("Codex working directory is unavailable; restore it before Retry");
        }
        if matches!(pair[0].as_str(), "--profile" | "-p")
            && !reference
                .config_dir
                .join(format!("{}.config.toml", pair[1]))
                .is_file()
        {
            bail!("Codex profile configuration is unavailable; restore it before Retry");
        }
    }
    validate_history(reference)?;
    let mut argv = vec![
        std::env::current_exe()?.into_os_string(),
        "agent".into(),
        "run".into(),
        "codex".into(),
        "--".into(),
        reference.executable.clone().into_os_string(),
        "resume".into(),
        reference.conversation.clone().into(),
    ];
    argv.extend(reference.options.iter().map(OsString::from));
    Ok(argv)
}

/// Prepare this managed Codex launch so reporters can reach the private channel.
///
/// Codex CLI 0.158 runs hooks from a detached `app-server --managed-daemon` that
/// captures environment at daemon start and can outlive a prior TUI. Stop any
/// CODEX_HOME-owned managed daemon so the new TUI starts a fresh one under this
/// process tree with the current `OVRCR_AGENT_*` channel. Does not rewrite
/// `CODEX_HOME` (recovery compares the recorded directory exactly).
pub fn prepare_managed_launch() {
    stop_stale_app_server_daemon();
}

fn stop_stale_app_server_daemon() {
    let Ok(dir) = config_dir() else {
        return;
    };
    // Resolve symlinks only for locating Codex's pid files; leave env unchanged.
    let root = std::fs::canonicalize(&dir).unwrap_or(dir);
    let daemon_dir = root.join("app-server-daemon");
    stop_codex_pid_file(&daemon_dir.join("daemon.pid"), true);
    stop_codex_pid_file(&daemon_dir.join("daemon-updater.pid"), false);
}

fn stop_codex_pid_file(path: &Path, require_managed_daemon: bool) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Some(pid) = managed_daemon_pid(&raw) else {
        return;
    };
    if require_managed_daemon && !process_looks_like_managed_daemon(pid) {
        return;
    }
    if !require_managed_daemon && !process_looks_like_codex(pid) {
        return;
    }
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while std::time::Instant::now() < deadline {
        if !process_exists(pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if process_exists(pid) {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

fn managed_daemon_pid(raw: &str) -> Option<libc::pid_t> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let pid = value.get("pid")?.as_u64()?;
    let pid = libc::pid_t::try_from(pid).ok()?;
    (pid > 1).then_some(pid)
}

fn process_exists(pid: libc::pid_t) -> bool {
    unsafe {
        libc::kill(pid, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

fn process_looks_like_managed_daemon(pid: libc::pid_t) -> bool {
    let Some(cmdline) = process_cmdline(pid) else {
        return false;
    };
    cmdline.iter().any(|arg| arg == "app-server")
        && cmdline
            .iter()
            .any(|arg| arg == "--managed-daemon" || arg.contains("managed-daemon"))
}

fn process_looks_like_codex(pid: libc::pid_t) -> bool {
    let Some(cmdline) = process_cmdline(pid) else {
        return false;
    };
    cmdline.iter().any(|arg| {
        Path::new(arg)
            .file_name()
            .is_some_and(|name| name == "codex" || name == "codex.exe")
            || arg.contains("app-server")
    })
}

#[cfg(target_os = "linux")]
fn process_cmdline(pid: libc::pid_t) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = raw
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    (!args.is_empty()).then_some(args)
}

#[cfg(target_os = "macos")]
fn process_cmdline(pid: libc::pid_t) -> Option<Vec<String>> {
    // macOS has no /proc; trust the CODEX_HOME pid file and confirm the pid is live.
    process_exists(pid).then(|| vec!["codex".into()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_preserves_controls_but_drops_prompts_and_rejects_unsafe_options() {
        let args = |args: &[&str]| args.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            launch_options(&args(&[
                "codex",
                "-m",
                "gpt-5",
                "-p",
                "work",
                "-s",
                "read-only",
                "-a",
                "on-request",
                "--no-alt-screen",
                "--approve-for-me",
                "PRIVATE_PROMPT"
            ]))
            .unwrap(),
            [
                "-m",
                "gpt-5",
                "-p",
                "work",
                "-s",
                "read-only",
                "-a",
                "on-request",
                "--no-alt-screen",
                "--approve-for-me"
            ]
        );
        // Obsolete --full-auto stays readable for sessions saved before the CLI rename.
        assert_eq!(
            launch_options(&args(&["codex", "--full-auto"])).unwrap(),
            ["--full-auto"]
        );
        assert!(
            launch_options(&args(&["codex", "--", "resume"]))
                .unwrap()
                .is_empty()
        );
        for option in [
            "--config=secret",
            "--remote",
            "--last",
            "--dangerously-bypass-hook-trust",
        ] {
            assert!(
                launch_options(&args(&["codex", option])).is_err(),
                "{option}"
            );
        }
        assert!(launch_options(&args(&["codex", "-C", "relative"])).is_err());
    }

    #[test]
    fn exact_history_header_is_required_and_missing_history_is_unavailable() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("exact.jsonl");
        let mut reference = CodexConversation {
            conversation: "01a08e5c-7480-7052-9964-9224aadebef0".into(),
            executable: "/bin/codex".into(),
            history: Some(path.clone()),
            config_dir: root.path().into(),
            options: vec![],
        };
        validate(&reference).unwrap();
        assert!(validate_history(&reference).is_err());
        std::fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"other\",\"source\":\"cli\"}}\n",
        )
        .unwrap();
        assert!(validate_history(&reference).is_err());
        std::fs::write(
            &path,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{}\",\"source\":\"cli\"}}}}\n",
                reference.conversation
            ),
        )
        .unwrap();
        validate_history(&reference).unwrap();
        reference.history = None;
        let tagged = ovrcr_protocol::ConversationReference::Codex(reference.clone());
        assert!(
            crate::recovery::unavailable("codex", Some(&tagged), false)
                .unwrap()
                .contains("history")
        );
        for identity in [
            "--last",
            "a conversation name",
            "01A08e5c-7480-7052-9964-9224aadebef0",
        ] {
            reference.conversation = identity.into();
            validate(&reference).unwrap();
            assert!(!is_exact_identity(identity));
            assert!(
                resume_argv(&reference)
                    .unwrap_err()
                    .to_string()
                    .contains("canonical UUID")
            );
        }
    }

    #[test]
    fn managed_daemon_pid_reads_codex_daemon_pid_file() {
        assert_eq!(
            managed_daemon_pid(r#"{"pid":1792397,"processStartTime":"Mon Sep 28 18:42:17 2026"}"#),
            Some(1792397)
        );
        assert_eq!(managed_daemon_pid(r#"{"pid":1}"#), None);
        assert_eq!(managed_daemon_pid("not-json"), None);
        assert_eq!(managed_daemon_pid("{}"), None);
    }
}

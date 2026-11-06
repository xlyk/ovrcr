//! Bounded Oh My Pi model listing for the current Dashboard's detected launcher.
//!
//! OMP's native `models --json` command reads the effective profile and loads
//! configured provider extensions while excluding ambient hooks. Do not add
//! `--no-extensions` or fabricate a delegated-session authority here.

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const CATALOG_LIMIT: u64 = 2 * 1024 * 1024;
const DIAGNOSTIC_LIMIT: u64 = 64 * 1024;
const CATALOG_DEADLINE: Duration = Duration::from_secs(20);
const PROCESS_GROUP_CLEANUP_GRACE: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelCatalog {
    models: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    provider: String,
    kind: String,
    id: String,
    selector: String,
}

/// Read the effective OMP chat catalog for the selected workspace.
///
/// The supplied native argv must be OVRCR's detected launcher argv. The model
/// listing is a separate, native `omp models --json` command; it inherits the
/// user's effective OMP profile and configured extensions, but not OVRCR's
/// session/delegation authority.
pub(crate) fn dashboard_catalog(
    executable: &Path,
    native_argv: &[OsString],
    cwd: &Path,
    cancelled: &AtomicBool,
) -> Result<Vec<(String, String)>> {
    let mut expected = vec![executable.as_os_str().to_owned()];
    ovrcr_protocol::apply_default_auto_trust("omp", &mut expected);
    ensure!(
        native_argv == expected,
        "OMP launch options are not the detected managed configuration"
    );
    let executable = executable
        .canonicalize()
        .context("native OMP executable unavailable")?;
    ensure!(executable.is_file(), "native OMP executable unavailable");
    let cwd = cwd.canonicalize().context("OMP workspace is unavailable")?;
    ensure!(cwd.is_dir(), "OMP workspace is unavailable");

    let mut output = tempfile::tempfile()?;
    let diagnostics = tempfile::tempfile()?;
    let mut command = Command::new(&executable);
    command
        .current_dir(cwd)
        .args(["models", "--json"])
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(diagnostics.try_clone()?)
        .process_group(0);
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
        "OVRCR_CLAUDE_ARTIFACTS",
    ] {
        command.env_remove(name);
    }

    let mut child = command
        .spawn()
        .context("native OMP model discovery unavailable")?;
    let pid = child.id() as libc::pid_t;
    let deadline = Instant::now() + CATALOG_DEADLINE;
    let probe = (|| -> Result<()> {
        loop {
            ensure!(
                !cancelled.load(Ordering::Acquire),
                "native OMP model discovery cancelled"
            );
            ensure!(
                output.metadata()?.len() <= CATALOG_LIMIT,
                "native OMP model discovery output bound"
            );
            ensure!(
                diagnostics.metadata()?.len() <= DIAGNOSTIC_LIMIT,
                "native OMP model discovery diagnostic bound"
            );
            if child_exited_unreaped(pid)? {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "native OMP model discovery deadline"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
    })();
    let status = reap_process_group(&mut child, pid, probe.is_err())?;
    probe?;
    ensure!(status.success(), "native OMP model inventory failed");
    ensure!(
        diagnostics.metadata()?.len() == 0,
        "native OMP model listing produced diagnostics"
    );
    ensure!(
        output.metadata()?.len() <= CATALOG_LIMIT,
        "native OMP model discovery output bound"
    );
    output.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    output
        .take(CATALOG_LIMIT)
        .read_to_string(&mut text)
        .context("native OMP model catalog is not UTF-8")?;
    parse_model_json(&text)
}

fn parse_model_json(output: &str) -> Result<Vec<(String, String)>> {
    ensure!(
        output.len() <= CATALOG_LIMIT as usize,
        "native OMP model discovery output bound"
    );
    let catalog: ModelCatalog = serde_json::from_str(output)
        .context("native OMP model catalog is not the supported JSON shape")?;
    ensure!(
        catalog.models.len() <= 8192,
        "native OMP model catalog entry bound"
    );
    let mut models = BTreeMap::new();
    for entry in catalog.models {
        ensure!(
            entry.kind == "chat",
            "native OMP model catalog contains a non-chat entry"
        );
        let selector = format!("{}/{}", entry.provider, entry.id);
        ensure!(
            selector == entry.selector,
            "native OMP model catalog has an inconsistent selector"
        );
        ensure!(
            ovrcr_protocol::DelegationSettings::valid_model(&selector),
            "native OMP model catalog contains an invalid provider/model selector"
        );
        ensure!(
            models.insert(selector.clone(), selector).is_none(),
            "native OMP model catalog contains a duplicate selector"
        );
    }
    Ok(models.into_iter().collect())
}

fn reap_process_group(
    child: &mut Child,
    process_group: libc::pid_t,
    terminate_if_running: bool,
) -> Result<std::process::ExitStatus> {
    let leader_exited = child_exited_unreaped(process_group)?;
    ensure!(
        leader_exited || terminate_if_running,
        "native OMP discovery exited without an ownership observation"
    );
    // Keep the unreaped leader as the process-group identity while clearing
    // owned descendants. Never signal the group after reaping that leader.
    let mut signal_error = None;
    let sent = match signal_group(process_group) {
        Ok(sent) => sent,
        Err(error) if !leader_exited => return Err(error),
        Err(error) => {
            signal_error = Some(error);
            false
        }
    };
    if !leader_exited && !sent {
        bail!("native OMP discovery cleanup group disappeared while leader remained live");
    }
    if !leader_exited {
        wait_for_child_exit_unreaped(process_group)?;
    }
    let waited = child.wait().context("native OMP discovery cleanup failed");
    let absent = process_group_absent(process_group);
    let waited = waited?;
    match absent {
        Ok(()) => {}
        Err(error) => {
            if let Some(signal_error) = signal_error {
                bail!(
                    "native OMP discovery cleanup is uncertain (signal: {signal_error:#}; absence: {error:#})"
                );
            }
            return Err(error);
        }
    }
    Ok(waited)
}

fn child_exited_unreaped(child: libc::pid_t) -> Result<bool> {
    for _ in 0..10 {
        if observed_child_exit(child)? {
            return Ok(true);
        }
        let observed_group = unsafe { libc::getpgid(child) };
        if observed_group == child {
            return Ok(false);
        }
        let group_errno = if observed_group == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        if observed_group == -1 && group_errno == Some(libc::ESRCH) {
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }
        bail!("native OMP discovery process-group ownership is uncertain");
    }
    bail!("native OMP discovery process-group ownership is uncertain")
}

fn observed_child_exit(child: libc::pid_t) -> Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let observed = unsafe {
        libc::waitid(
            libc::P_PID,
            child as u32,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    ensure!(observed == 0, "native OMP discovery ownership probe failed");
    let observed_pid = unsafe { info.si_pid() };
    ensure!(
        observed_pid == 0 || observed_pid == child,
        "native OMP discovery cleanup ownership is uncertain"
    );
    Ok(observed_pid != 0)
}

fn wait_for_child_exit_unreaped(child: libc::pid_t) -> Result<()> {
    let deadline = Instant::now() + PROCESS_GROUP_CLEANUP_GRACE;
    loop {
        if child_exited_unreaped(child)? {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "native OMP discovery cleanup termination was not observed"
        );
        std::thread::park_timeout(Duration::from_millis(10));
    }
}

fn signal_group(process_group: libc::pid_t) -> Result<bool> {
    let result = unsafe { libc::kill(-process_group, libc::SIGKILL) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        return Ok(false);
    }
    bail!(
        "native OMP discovery cleanup signal failed (errno {:?})",
        error.raw_os_error()
    )
}

fn process_group_absent(process_group: libc::pid_t) -> Result<()> {
    let deadline = Instant::now() + PROCESS_GROUP_CLEANUP_GRACE;
    loop {
        let result = unsafe { libc::kill(-process_group, 0) };
        let error = if result == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        if result == -1 && error == Some(libc::ESRCH) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "native OMP discovery cleanup is uncertain (process group {process_group} absence errno {error:?})"
            );
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    const CHAT_MODELS: &str = r#"{"models":[
        {"provider":"openai","kind":"chat","id":"gpt-5.4","selector":"openai/gpt-5.4","name":"GPT-5.4"},
        {"provider":"anthropic","kind":"chat","id":"claude-sonnet-4-5","selector":"anthropic/claude-sonnet-4-5","name":"Claude Sonnet 4.5"}
    ]}"#;

    fn executable(path: &Path, contents: &str) {
        std::fs::write(path, contents).unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    fn detected_native_argv(executable: &Path) -> Vec<OsString> {
        let mut argv = vec![executable.as_os_str().to_owned()];
        ovrcr_protocol::apply_default_auto_trust("omp", &mut argv);
        argv
    }

    #[test]
    fn parses_native_json_selectors_as_exact_provider_model_ids() {
        assert_eq!(
            parse_model_json(CHAT_MODELS).unwrap(),
            vec![
                (
                    "anthropic/claude-sonnet-4-5".into(),
                    "anthropic/claude-sonnet-4-5".into()
                ),
                ("openai/gpt-5.4".into(), "openai/gpt-5.4".into()),
            ]
        );
    }

    #[test]
    fn rejects_ambiguous_or_inconsistent_native_json() {
        assert!(parse_model_json(&format!("warning\n{CHAT_MODELS}")).is_err());
        assert!(parse_model_json(&format!("{CHAT_MODELS} trailing")).is_err());
        assert!(parse_model_json(r#"{"models":[] , "error":"partial"}"#).is_err());
        assert!(parse_model_json(
            r#"{"models":[{"provider":"openai","kind":"chat","id":"gpt-5.4","selector":"openai/gpt-5.3"}]}"#
        )
        .is_err());
    }

    #[test]
    fn rejects_duplicate_ids_and_non_chat_entries() {
        assert!(
            parse_model_json(
                r#"{"models":[
                {"provider":"openai","kind":"chat","id":"gpt-5.4","selector":"openai/gpt-5.4"},
                {"provider":"openai","kind":"chat","id":"gpt-5.4","selector":"openai/gpt-5.4"}
            ]}"#
            )
            .is_err()
        );
        assert!(parse_model_json(
            r#"{"models":[{"provider":"openai","kind":"embedding","id":"text-embedding","selector":"openai/text-embedding"}]}"#
        )
        .is_err());
    }

    #[test]
    fn listing_uses_native_json_and_keeps_the_effective_extension_configuration() {
        let root = tempfile::tempdir().unwrap();
        let executable_path = root.path().join("omp");
        let capture = root.path().join("args");
        executable(
            &executable_path,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s' '{}'\n",
                capture.display(),
                CHAT_MODELS.replace('\'', "'\\''")
            ),
        );
        let models = dashboard_catalog(
            &executable_path,
            &detected_native_argv(&executable_path),
            root.path(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            models,
            vec![
                (
                    "anthropic/claude-sonnet-4-5".into(),
                    "anthropic/claude-sonnet-4-5".into()
                ),
                ("openai/gpt-5.4".into(), "openai/gpt-5.4".into()),
            ]
        );
        let args = std::fs::read_to_string(capture).unwrap();
        assert_eq!(args.lines().collect::<Vec<_>>(), ["models", "--json"]);
        assert!(!args.contains("--no-extensions"));
    }

    #[test]
    fn listing_refuses_diagnostics_instead_of_returning_a_partial_catalog() {
        let root = tempfile::tempdir().unwrap();
        let executable_path = root.path().join("omp");
        executable(
            &executable_path,
            &format!(
                "#!/bin/sh\nprintf '%s' '{}'\nprintf 'setup or provider diagnostic\\n' >&2\n",
                CHAT_MODELS
            ),
        );
        let error = dashboard_catalog(
            &executable_path,
            &detected_native_argv(&executable_path),
            root.path(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(error.to_string().contains("produced diagnostics"));
    }

    #[test]
    fn catalog_rejects_a_changed_managed_configuration_before_spawn() {
        let root = tempfile::tempdir().unwrap();
        let executable_path = root.path().join("omp");
        let capture = root.path().join("called");
        executable(
            &executable_path,
            &format!("#!/bin/sh\nprintf called > '{}'\n", capture.display()),
        );
        let mut argv = detected_native_argv(&executable_path);
        argv.push("--profile".into());
        argv.push("other".into());

        assert!(
            dashboard_catalog(
                &executable_path,
                &argv,
                root.path(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(!capture.exists(), "unverified options must not run OMP");
    }

    #[test]
    fn successful_catalog_reaps_a_held_descendant_before_reaping_its_leader() {
        let root = tempfile::tempdir().unwrap();
        let executable_path = root.path().join("omp");
        let pid_file = root.path().join("pid");
        executable(
            &executable_path,
            &format!(
                "#!/bin/sh\nprintf '%s' '{}'\n/bin/sleep 30 &\nprintf '%s' \"$$\" > '{}'\nexit 0\n",
                CHAT_MODELS,
                pid_file.display()
            ),
        );
        let models = dashboard_catalog(
            &executable_path,
            &detected_native_argv(&executable_path),
            root.path(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(models.len(), 2);
        let pid: libc::pid_t = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        process_group_absent(pid).unwrap();
    }

    #[test]
    fn cancellation_joins_the_owned_omp_catalog_process_group() {
        let root = tempfile::tempdir().unwrap();
        let executable_path = root.path().join("omp");
        let pid_file = root.path().join("pid");
        executable(
            &executable_path,
            &format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec /bin/sleep 30\n",
                pid_file.display()
            ),
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_path = executable_path.clone();
        let worker_argv = detected_native_argv(&executable_path);
        let worker_cwd = root.path().to_path_buf();
        let worker = thread::spawn(move || {
            dashboard_catalog(&worker_path, &worker_argv, &worker_cwd, &worker_cancelled)
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while !pid_file.exists() {
            assert!(Instant::now() < deadline, "fake OMP catalog did not start");
            thread::sleep(Duration::from_millis(5));
        }
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        assert_eq!(unsafe { libc::getpgid(pid) }, pid);
        cancelled.store(true, Ordering::Release);
        let finish_deadline = Instant::now() + Duration::from_millis(500);
        while !worker.is_finished() && Instant::now() < finish_deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let cancelled_and_joined = worker.is_finished();
        if !cancelled_and_joined {
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
        let result = worker.join().unwrap();
        assert!(
            cancelled_and_joined,
            "OMP catalog worker must join promptly"
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "native OMP model discovery cancelled"
        );
        assert_eq!(unsafe { libc::kill(-pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}

//! Bounded Pi model listing for the current Dashboard's detected launcher.
//!
//! The catalog command keeps Pi's normal profile, workspace, extension, and
//! trust behavior. OVRCR adds only Pi's documented `--list-models` action and
//! strips OVRCR-only authority from the child environment.
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const CATALOG_LIMIT: u64 = 512 * 1024;
const DIAGNOSTIC_LIMIT: u64 = 64 * 1024;
const CATALOG_DEADLINE: Duration = Duration::from_secs(20);
const PROCESS_GROUP_CLEANUP_GRACE: Duration = Duration::from_secs(2);

/// Read the effective Pi model catalog for the selected workspace.
///
/// The supplied native argv must be the detected launcher's unmodified argv.
/// In particular, this must not become a route for arbitrary configured flags
/// or a delegated-session capability. Pi's catalog may initialize user or
/// project extensions, as the interactive launcher does; callers must already
/// have verified the current Dashboard, launcher provenance, and workspace.
pub(crate) fn dashboard_catalog(
    executable: &Path,
    native_argv: &[OsString],
    cwd: &Path,
    cancelled: &AtomicBool,
) -> Result<Vec<(String, String)>> {
    let mut expected = vec![executable.as_os_str().to_owned()];
    ovrcr_protocol::apply_default_auto_trust("pi", &mut expected);
    ensure!(
        native_argv == expected,
        "Pi launch options are not the detected managed configuration"
    );
    let executable = executable
        .canonicalize()
        .context("native Pi executable unavailable")?;
    ensure!(executable.is_file(), "native Pi executable unavailable");
    let cwd = cwd.canonicalize().context("Pi workspace is unavailable")?;
    ensure!(cwd.is_dir(), "Pi workspace is unavailable");

    let mut output = tempfile::tempfile()?;
    let diagnostics = tempfile::tempfile()?;
    let mut command = Command::new(&executable);
    command
        .current_dir(cwd)
        .args(native_argv.iter().skip(1))
        .arg("--list-models")
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
        .context("native Pi model discovery unavailable")?;
    let pid = child.id() as libc::pid_t;
    let deadline = Instant::now() + CATALOG_DEADLINE;
    let probe = (|| -> Result<()> {
        loop {
            ensure!(
                !cancelled.load(Ordering::Acquire),
                "native Pi model discovery cancelled"
            );
            ensure!(
                output.metadata()?.len() <= CATALOG_LIMIT,
                "native Pi model discovery output bound"
            );
            ensure!(
                diagnostics.metadata()?.len() <= DIAGNOSTIC_LIMIT,
                "native Pi model discovery diagnostic bound"
            );
            if child_exited_unreaped(pid)? {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "native Pi model discovery deadline"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
    })();
    let status = reap_process_group(&mut child, pid, probe.is_err())?;
    probe?;
    ensure!(status.success(), "native Pi model inventory failed");
    ensure!(
        diagnostics.metadata()?.len() == 0,
        "native Pi model listing produced diagnostics"
    );
    ensure!(
        output.metadata()?.len() <= CATALOG_LIMIT,
        "native Pi model discovery output bound"
    );
    output.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    output
        .take(CATALOG_LIMIT)
        .read_to_string(&mut text)
        .context("native Pi model catalog is not UTF-8")?;
    parse_model_list(&text)
}

fn reap_process_group(
    child: &mut Child,
    process_group: libc::pid_t,
    terminate_if_running: bool,
) -> Result<std::process::ExitStatus> {
    let leader_exited = child_exited_unreaped(process_group)?;
    ensure!(
        leader_exited || terminate_if_running,
        "native Pi discovery exited without an ownership observation"
    );
    // Always clear same-group descendants while the unreaped leader reserves
    // this process-group identity. On an error path, don't block on a live
    // leader if signaling or group ownership is uncertain.
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
        bail!("native Pi discovery cleanup group disappeared while leader remained live");
    }
    if !leader_exited {
        wait_for_child_exit_unreaped(process_group)?;
    }
    let waited = child.wait().context("native Pi discovery cleanup failed");
    let absent = process_group_absent(process_group);
    let waited = waited?;
    // A pre-reap signal error is resolved only by a successful post-reap
    // absence proof; a remaining or inaccessible group stays uncertain.
    match absent {
        Ok(()) => {}
        Err(error) => {
            if let Some(signal_error) = signal_error {
                bail!(
                    "native Pi discovery cleanup is uncertain (signal: {signal_error:#}; absence: {error:#})"
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
            // The leader can exit between waitid and getpgid. Recheck the
            // unreaped status briefly before treating group ownership as lost.
            std::thread::park_timeout(Duration::from_millis(1));
            continue;
        }
        bail!(
            "native Pi discovery process-group ownership is uncertain (child {child}, WNOWAIT exit not observed, getpgid {observed_group}, errno {group_errno:?})"
        );
    }
    bail!(
        "native Pi discovery process-group ownership is uncertain (child {child}, WNOWAIT exit not observed after 10 checks, getpgid -1, errno Some({}))",
        libc::ESRCH
    )
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
    if observed == -1 {
        let errno = std::io::Error::last_os_error().raw_os_error();
        bail!("native Pi discovery WNOWAIT probe failed (child {child}, errno {errno:?})");
    }
    let observed_pid = unsafe { info.si_pid() };
    ensure!(
        observed_pid == 0 || observed_pid == child,
        "native Pi discovery cleanup ownership is uncertain (child {child}, WNOWAIT reported pid {observed_pid})"
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
            "native Pi discovery cleanup termination was not observed"
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
        "native Pi discovery cleanup signal failed (errno {:?})",
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
                "native Pi discovery cleanup is uncertain (process group {process_group} absence errno {error:?})"
            );
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
}

fn parse_model_list(output: &str) -> Result<Vec<(String, String)>> {
    let mut models = BTreeMap::new();
    let mut header_seen = false;
    for line in output.lines() {
        ensure!(
            line.len() <= 128 * 1024,
            "native Pi model catalog line bound"
        );
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.is_empty() {
            continue;
        }
        if fields.as_slice()
            == [
                "provider", "model", "context", "max-out", "thinking", "images",
            ]
        {
            ensure!(
                !header_seen,
                "native Pi model catalog contains a duplicate header"
            );
            header_seen = true;
            continue;
        }
        ensure!(
            header_seen && fields.len() == 6,
            "native Pi model catalog output is ambiguous"
        );
        let [provider, model, context, max_out, thinking, images] = fields.as_slice() else {
            unreachable!("six fields were checked above")
        };
        let alias = format!("{provider}/{model}");
        if !ovrcr_protocol::DelegationSettings::valid_model(&alias) {
            bail!("native Pi model catalog contains an invalid provider/model ID");
        }
        ensure!(
            valid_token_count(context),
            "native Pi model catalog has an invalid context size"
        );
        ensure!(
            valid_token_count(max_out),
            "native Pi model catalog has an invalid output size"
        );
        ensure!(
            matches!(*thinking, "yes" | "no"),
            "native Pi model catalog has an invalid thinking flag"
        );
        ensure!(
            matches!(*images, "yes" | "no"),
            "native Pi model catalog has an invalid image flag"
        );
        ensure!(
            models.insert(alias.clone(), alias).is_none(),
            "native Pi model catalog contains a duplicate model"
        );
    }
    ensure!(header_seen, "native Pi model catalog header is missing");
    Ok(models.into_iter().collect())
}

fn valid_token_count(value: &str) -> bool {
    let number = value.strip_suffix(['K', 'M']).unwrap_or(value);
    let mut parts = number.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    !whole.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.is_none_or(|fraction| {
            !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
        && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn executable(path: &Path, contents: &str) {
        std::fs::write(path, contents).unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn parses_exact_provider_model_ids_and_rejects_duplicates() {
        assert_eq!(
            parse_model_list(
                "provider  model                     context  max-out  thinking  images\n\
                 openai    gpt-5.4                   256K     64K      yes       yes\n\
                 anthropic claude-sonnet-4-5         200K     16K      yes       yes\n"
            )
            .unwrap(),
            vec![
                (
                    "anthropic/claude-sonnet-4-5".into(),
                    "anthropic/claude-sonnet-4-5".into()
                ),
                ("openai/gpt-5.4".into(), "openai/gpt-5.4".into()),
            ]
        );
        assert!(
            parse_model_list(
                "provider  model  context  max-out  thinking  images\n\
             openai    gpt-5.4  256K  64K  yes  yes\n\
             openai    gpt-5.4  256K  64K  yes  yes\n"
            )
            .is_err()
        );
        assert!(
            parse_model_list(
                "Warning stale\nprovider  model  context  max-out  thinking  images\n\
             openai    gpt-5.4  256K  64K  yes  yes\n"
            )
            .is_err()
        );
        assert!(
            parse_model_list(
                "provider model context max-out thinking images\n\
             Warning stale\n"
            )
            .is_err()
        );
    }

    #[test]
    fn listing_keeps_managed_trust_and_extensions_and_reaps_its_process_group() {
        let root = tempfile::tempdir().unwrap();
        let executable_path = root.path().join("pi");
        let capture = root.path().join("args");
        executable(
            &executable_path,
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf 'provider  model  context  max-out  thinking  images\\nopenai    gpt-5.4  256K  64K  yes  yes\\n'\n",
                capture.display()
            ),
        );
        let mut argv = vec![executable_path.as_os_str().to_owned()];
        ovrcr_protocol::apply_default_auto_trust("pi", &mut argv);
        let models = dashboard_catalog(
            &executable_path,
            &argv,
            root.path(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            models,
            vec![("openai/gpt-5.4".into(), "openai/gpt-5.4".into())]
        );
        let args = std::fs::read_to_string(capture).unwrap();
        assert!(args.lines().any(|arg| arg == "--approve"));
        assert!(args.lines().any(|arg| arg == "--list-models"));
        assert!(!args.lines().any(|arg| arg == "--no-extensions"));
        assert!(!args.lines().any(|arg| arg == "--no-skills"));
    }

    #[test]
    fn successful_listing_kills_a_held_descendant_before_reaping_its_leader() {
        let root = tempfile::tempdir().unwrap();
        let executable_path = root.path().join("pi");
        let pid_file = root.path().join("pid");
        executable(
            &executable_path,
            &format!(
                "#!/bin/sh\nprintf 'provider  model  context  max-out  thinking  images\\nopenai    gpt-5.4  256K  64K  yes  yes\\n'\n/bin/sleep 30 &\nprintf '%s' \"$$\" > '{}'\nexit 0\n",
                pid_file.display()
            ),
        );
        let mut argv = vec![executable_path.as_os_str().to_owned()];
        ovrcr_protocol::apply_default_auto_trust("pi", &mut argv);
        let models = dashboard_catalog(
            &executable_path,
            &argv,
            root.path(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            models,
            vec![("openai/gpt-5.4".into(), "openai/gpt-5.4".into())]
        );
        let pid: libc::pid_t = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        process_group_absent(pid).unwrap();
    }

    #[test]
    fn cancellation_kills_and_joins_the_owned_pi_catalog_process_group() {
        use std::sync::Arc;

        let root = tempfile::tempdir().unwrap();
        let executable_path = root.path().join("pi");
        let pid_file = root.path().join("pid");
        executable(
            &executable_path,
            &format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec /bin/sleep 30\n",
                pid_file.display()
            ),
        );
        let mut argv = vec![executable_path.as_os_str().to_owned()];
        ovrcr_protocol::apply_default_auto_trust("pi", &mut argv);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_path = executable_path.clone();
        let worker_argv = argv.clone();
        let worker_cwd = root.path().to_path_buf();
        let worker = std::thread::spawn(move || {
            dashboard_catalog(&worker_path, &worker_argv, &worker_cwd, &worker_cancelled)
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while !pid_file.exists() {
            assert!(Instant::now() < deadline, "fake Pi catalog did not start");
            std::thread::sleep(Duration::from_millis(5));
        }
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        assert_eq!(
            unsafe { libc::getpgid(pid) },
            pid,
            "fake Pi catalog was not group leader"
        );
        cancelled.store(true, Ordering::Release);
        let finish_deadline = Instant::now() + Duration::from_millis(500);
        while !worker.is_finished() && Instant::now() < finish_deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let cancelled_and_joined = worker.is_finished();
        if !cancelled_and_joined {
            // Keep the test bounded and remove only the task-owned fake child.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
        let result = worker.join().unwrap();
        assert!(
            cancelled_and_joined,
            "shutdown must cancel and join Pi catalog work"
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "native Pi model discovery cancelled"
        );
        let absent = unsafe { libc::kill(-pid, 0) };
        let absence_errno = if absent == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        assert_eq!(
            (absent, absence_errno),
            (-1, Some(libc::ESRCH)),
            "the canceled catalog process group must be absent"
        );
    }
}

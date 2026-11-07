//! Bounded Pi model listing for the current Dashboard's detected launcher.
//!
//! The catalog command keeps Pi's normal profile, workspace, extension, and
//! trust behavior. OVRCR adds only Pi's documented `--list-models` action and
//! strips OVRCR-only authority from the child environment.
use crate::catalog_process::CatalogProcessOwner;
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const CATALOG_LIMIT: u64 = 512 * 1024;
const DIAGNOSTIC_LIMIT: u64 = 64 * 1024;
const CATALOG_DEADLINE: Duration = Duration::from_secs(20);

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
    owner: &mut CatalogProcessOwner,
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

    let mut command = Command::new(&executable);
    command
        .current_dir(cwd)
        .args(native_argv.iter().skip(1))
        .arg("--list-models");
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

    let output = owner.run_command(
        "Pi",
        &mut command,
        CATALOG_LIMIT as usize,
        DIAGNOSTIC_LIMIT as usize,
        CATALOG_DEADLINE,
        cancelled,
    )?;
    ensure!(
        output.stderr_total == 0,
        "native Pi model listing produced diagnostics"
    );
    let text = String::from_utf8(output.stdout).context("native Pi model catalog is not UTF-8")?;
    parse_model_list(&text)
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
    use std::sync::atomic::Ordering;
    use std::time::Instant;

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
        let mut owner = CatalogProcessOwner::default();
        let models = dashboard_catalog(
            &executable_path,
            &argv,
            root.path(),
            &AtomicBool::new(false),
            &mut owner,
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
        let mut owner = CatalogProcessOwner::default();
        let models = dashboard_catalog(
            &executable_path,
            &argv,
            root.path(),
            &AtomicBool::new(false),
            &mut owner,
        )
        .unwrap();
        assert_eq!(
            models,
            vec![("openai/gpt-5.4".into(), "openai/gpt-5.4".into())]
        );
        let pid: libc::pid_t = std::fs::read_to_string(pid_file).unwrap().parse().unwrap();
        let absent = unsafe { libc::kill(-pid, 0) };
        let absence_errno = if absent == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        assert_eq!((absent, absence_errno), (-1, Some(libc::ESRCH)));
    }

    #[test]
    fn cancellation_kills_and_joins_the_owned_pi_catalog_process_group() {
        use std::sync::Arc;

        let root = tempfile::tempdir().unwrap();
        let executable_path = root.path().join("pi");
        let pid_file = root.path().join("pid");
        let pid_temp_file = root.path().join("pid.tmp");
        let release_file = root.path().join("release");
        executable(
            &executable_path,
            &format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nmv '{}' '{}'\nwhile [ ! -e '{}' ]; do /bin/sleep 0.01; done\n",
                pid_temp_file.display(),
                pid_temp_file.display(),
                pid_file.display(),
                release_file.display()
            ),
        );
        let mut argv = vec![executable_path.as_os_str().to_owned()];
        ovrcr_protocol::apply_default_auto_trust("pi", &mut argv);
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_path = executable_path.clone();
        let worker_argv = argv.clone();
        let worker_cwd = root.path().to_path_buf();
        let worker_release = release_file.clone();
        let worker = std::thread::spawn(move || {
            let mut owner = CatalogProcessOwner::default();
            dashboard_catalog(
                &worker_path,
                &worker_argv,
                &worker_cwd,
                &worker_cancelled,
                &mut owner,
            )
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        let pid: libc::pid_t = loop {
            if let Some(pid) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|contents| contents.parse().ok())
            {
                break pid;
            }
            if Instant::now() >= deadline {
                cancelled.store(true, Ordering::Release);
                let _ = std::fs::write(&release_file, "release\n");
                let cleanup = worker.join();
                panic!("fake Pi catalog did not publish a parseable PID; cleanup: {cleanup:?}");
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let is_group_leader = unsafe { libc::getpgid(pid) } == pid;
        cancelled.store(true, Ordering::Release);
        let finish_deadline = Instant::now() + Duration::from_secs(3);
        while !worker.is_finished() && Instant::now() < finish_deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let cancelled_and_joined = worker.is_finished();
        if !cancelled_and_joined {
            // Let the task-owned fake child exit naturally if cancellation regresses.
            let _ = std::fs::write(worker_release, "release\n");
            let finish_deadline = Instant::now() + Duration::from_secs(3);
            while !worker.is_finished() && Instant::now() < finish_deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let result = worker.join().unwrap();
        assert!(is_group_leader, "fake Pi catalog was not group leader");
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
        let pid_result = unsafe { libc::kill(pid, 0) };
        let pid_errno = if pid_result == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        assert_eq!((pid_result, pid_errno), (-1, Some(libc::ESRCH)));
    }
}

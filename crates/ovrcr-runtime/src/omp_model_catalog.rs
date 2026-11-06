//! Bounded Oh My Pi model listing for the current Dashboard's detected launcher.
//!
//! OMP's native `models --json` command reads the effective profile and loads
//! configured provider extensions while excluding ambient hooks. Do not add
//! `--no-extensions` or fabricate a delegated-session authority here.

use crate::catalog_process::CatalogProcessOwner;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const CATALOG_LIMIT: u64 = 2 * 1024 * 1024;
const DIAGNOSTIC_LIMIT: u64 = 64 * 1024;
const CATALOG_DEADLINE: Duration = Duration::from_secs(20);

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
    owner: &mut CatalogProcessOwner,
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

    let mut command = Command::new(&executable);
    command.current_dir(cwd).args(["models", "--json"]);
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
        "OMP",
        &mut command,
        CATALOG_LIMIT as usize,
        DIAGNOSTIC_LIMIT as usize,
        CATALOG_DEADLINE,
        cancelled,
    )?;
    ensure!(
        output.stderr_total == 0,
        "native OMP model listing produced diagnostics"
    );
    let text = String::from_utf8(output.stdout).context("native OMP model catalog is not UTF-8")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::{Duration, Instant};

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

    fn catalog(
        executable: &Path,
        argv: &[OsString],
        cwd: &Path,
        cancelled: &AtomicBool,
    ) -> Result<Vec<(String, String)>> {
        let mut owner = CatalogProcessOwner::default();
        dashboard_catalog(executable, argv, cwd, cancelled, &mut owner)
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
        let models = catalog(
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
        let error = catalog(
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
            catalog(
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
        let models = catalog(
            &executable_path,
            &detected_native_argv(&executable_path),
            root.path(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(models.len(), 2);
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
    fn cancellation_joins_the_owned_omp_catalog_process_group() {
        let root = tempfile::tempdir().unwrap();
        let executable_path = root.path().join("omp");
        let pid_file = root.path().join("pid");
        let release_file = root.path().join("release");
        executable(
            &executable_path,
            &format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nwhile [ ! -e '{}' ]; do /bin/sleep 0.01; done\n",
                pid_file.display(),
                release_file.display()
            ),
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_path = executable_path.clone();
        let worker_argv = detected_native_argv(&executable_path);
        let worker_cwd = root.path().to_path_buf();
        let worker_release = release_file.clone();
        let worker = thread::spawn(move || {
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
        while !pid_file.exists() {
            assert!(Instant::now() < deadline, "fake OMP catalog did not start");
            thread::sleep(Duration::from_millis(5));
        }
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        assert_eq!(unsafe { libc::getpgid(pid) }, pid);
        cancelled.store(true, Ordering::Release);
        let finish_deadline = Instant::now() + Duration::from_secs(3);
        while !worker.is_finished() && Instant::now() < finish_deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let cancelled_and_joined = worker.is_finished();
        if !cancelled_and_joined {
            let _ = std::fs::write(worker_release, "release\n");
            let finish_deadline = Instant::now() + Duration::from_secs(3);
            while !worker.is_finished() && Instant::now() < finish_deadline {
                thread::sleep(Duration::from_millis(5));
            }
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
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}

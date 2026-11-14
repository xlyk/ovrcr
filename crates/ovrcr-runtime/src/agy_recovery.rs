//! Exact Google Antigravity conversation recovery from its local profile database.
//! The foreground UUID comes from the authenticated status line; the reported
//! transcript path is not trusted because the CLI currently reports a different root.
use anyhow::{Context, Result, bail};
use ovrcr_protocol::AgyConversation;
use std::{
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .context("Antigravity home is unavailable")?;
    if !home.is_absolute() {
        bail!("Antigravity home must be absolute for recovery");
    }
    Ok(home.join(".gemini/antigravity-cli"))
}

/// Keep only non-secret native options that are meaningful on an exact resume.
/// Prompt text, `--continue`, `--new-project`, and the conversation selector itself
/// are omitted. Remote-project selection is refused because it cannot be replayed
/// through a trusted local profile.
pub fn launch_options(argv: &[OsString]) -> Result<Vec<String>> {
    let mut options = Vec::new();
    let mut index = 1;
    let mut conversation_seen = false;
    let mut prompt_seen = false;
    while index < argv.len() {
        let arg = argv[index]
            .to_str()
            .context("non-UTF8 Antigravity option")?;
        if arg == "--" {
            if index + 1 < argv.len() {
                bail!("Antigravity prompt separator has trailing positional arguments");
            }
            break;
        }
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg, None), |(name, value)| (name, Some(value)));
        if matches!(name, "--continue" | "-c" | "--new-project") {
            if inline.is_some() {
                bail!("Antigravity option does not take a value");
            }
            index += 1;
            continue;
        }
        if matches!(name, "--conversation" | "--prompt-interactive" | "-i") {
            if name == "--conversation" {
                if conversation_seen {
                    bail!("Antigravity conversation selector is duplicated");
                }
                conversation_seen = true;
            } else {
                if prompt_seen {
                    bail!("Antigravity prompt selector is duplicated");
                }
                prompt_seen = true;
            }
            let value = if let Some(value) = inline {
                value
            } else {
                index += 1;
                argv.get(index)
                    .and_then(|value| value.to_str())
                    .context("missing Antigravity option value")?
            };
            if name == "--conversation" {
                if !is_exact_identity(value) {
                    bail!("Antigravity recovery requires an exact canonical UUID");
                }
            } else if value.is_empty() {
                bail!("Antigravity prompt selector is empty");
            }
            index += 1;
            continue;
        }
        if matches!(name, "--model" | "--effort" | "--agent" | "--add-dir") {
            let value = if let Some(value) = inline {
                value
            } else {
                index += 1;
                argv.get(index)
                    .and_then(|value| value.to_str())
                    .context("missing Antigravity option value")?
            };
            if value.is_empty()
                || value.starts_with('-')
                || value.len() > 4096
                || value.chars().any(char::is_control)
            {
                bail!("invalid Antigravity recovery option value");
            }
            if name == "--add-dir" && !Path::new(value).is_absolute() {
                bail!("Antigravity additional directories must be absolute for recovery");
            }
            options.extend([name.to_owned(), value.to_owned()]);
            index += 1;
            continue;
        }
        if name == "--mode" {
            let value = if let Some(value) = inline {
                value
            } else {
                index += 1;
                argv.get(index)
                    .and_then(|value| value.to_str())
                    .context("missing Antigravity mode")?
            };
            if !matches!(value, "accept-edits" | "auto" | "plan") {
                bail!("Antigravity mode cannot be safely resumed");
            }
            options.extend(["--mode".into(), value.into()]);
            index += 1;
            continue;
        }
        if name == "--sandbox" {
            match inline {
                Some("true") => options.push("--sandbox=true".into()),
                Some("false") => options.push("--sandbox=false".into()),
                Some(_) => bail!("Antigravity sandbox mode cannot be safely resumed"),
                None => options.push("--sandbox".into()),
            }
            index += 1;
            continue;
        }
        if name == "--dangerously-skip-permissions" {
            if inline.is_some() {
                bail!("Antigravity permission option does not take a value");
            }
            options.push(name.into());
            index += 1;
            continue;
        }
        if name == "--project" {
            bail!("Antigravity project selection may be remote and cannot be safely resumed");
        }
        if !arg.starts_with('-') {
            bail!(
                "Antigravity subcommands and positional arguments cannot be retained for recovery"
            );
        }
        bail!("Antigravity launch configuration cannot be retained as non-secret references");
    }
    Ok(options)
}

/// Antigravity 1.5.5 conversation ids are lowercase UUIDs.
pub fn is_exact_identity(identity: &str) -> bool {
    ovrcr_protocol::is_exact_agy_conversation_id(identity)
}

fn validate_title(title: Option<&str>) -> Result<()> {
    if let Some(title) = title
        && (title.trim().is_empty() || title.len() > 512 || title.chars().any(char::is_control))
    {
        bail!("Antigravity conversation title is invalid");
    }
    Ok(())
}

fn validate_shape(reference: &AgyConversation) -> Result<()> {
    if !is_exact_identity(&reference.conversation) {
        bail!("Antigravity recovery requires an exact canonical UUID");
    }
    if !reference.executable.is_absolute() || !reference.config_dir.is_absolute() {
        bail!("Antigravity recovery references must be absolute");
    }
    if reference.options.len() > 32
        || reference
            .options
            .iter()
            .any(|option| option.len() > 4096 || option.chars().any(char::is_control))
    {
        bail!("Antigravity recovery configuration exceeds limits");
    }
    validate_title(reference.title.as_deref())?;
    let mut argv = vec![reference.executable.clone().into_os_string()];
    argv.extend(reference.options.iter().map(OsString::from));
    if launch_options(&argv)? != reference.options {
        bail!("invalid retained Antigravity configuration");
    }
    Ok(())
}

pub(crate) fn database_path(config_dir: &Path, conversation: &str) -> PathBuf {
    config_dir
        .join("conversations")
        .join(format!("{conversation}.db"))
}

pub fn validate_profile(reference: &AgyConversation, active_config_dir: &Path) -> Result<()> {
    validate_shape(reference)?;
    if reference.config_dir != active_config_dir || !active_config_dir.is_dir() {
        bail!(
            "Antigravity configuration changed or is unavailable; restore the recorded profile before Retry"
        );
    }
    let history = database_path(active_config_dir, &reference.conversation);
    let metadata = std::fs::metadata(&history)
        .context("Antigravity conversation database is unavailable; restore it before Retry")?;
    if !metadata.is_file() {
        bail!("Antigravity conversation database is not a regular file");
    }
    let connection = rusqlite::Connection::open_with_flags(
        &history,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("Antigravity conversation database cannot be opened read-only")?;
    let found: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM trajectory_meta WHERE cascade_id = ?1)",
            [&reference.conversation],
            |row| row.get(0),
        )
        .context("Antigravity conversation identity lookup failed")?;
    if !found {
        bail!("Antigravity database does not contain the recorded conversation");
    }
    Ok(())
}

/// Validate against a previously selected trusted local profile. The reporter uses
/// this before binding a foreground switch; the profile path was captured once at
/// managed launch and is checked against the active profile again when resuming.
pub fn validate(reference: &AgyConversation) -> Result<()> {
    validate_profile(reference, &config_dir()?)
}

fn resume_argv_at(reference: &AgyConversation, active_config_dir: &Path) -> Result<Vec<OsString>> {
    validate_profile(reference, active_config_dir)?;
    let executable = std::fs::metadata(&reference.executable)
        .context("Antigravity executable is unavailable; restore it before Retry")?;
    if !executable.is_file() || executable.permissions().mode() & 0o111 == 0 {
        bail!("Antigravity executable is not executable; restore it before Retry");
    }
    let mut argv = vec![
        std::env::current_exe()?.into_os_string(),
        "agent".into(),
        "run".into(),
        "agy".into(),
        "--".into(),
        reference.executable.clone().into_os_string(),
        "--conversation".into(),
        reference.conversation.clone().into(),
    ];
    argv.extend(reference.options.iter().map(OsString::from));
    Ok(argv)
}

pub fn resume_argv(reference: &AgyConversation) -> Result<Vec<OsString>> {
    resume_argv_at(reference, &config_dir()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    const ID: &str = "01a08e5c-7480-7052-9964-9224aadebef0";

    fn database(config_dir: &Path, cascade_id: &str) -> PathBuf {
        let directory = config_dir.join("conversations");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{cascade_id}.db"));
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE trajectory_meta (cascade_id TEXT NOT NULL);")
            .unwrap();
        connection
            .execute(
                "INSERT INTO trajectory_meta (cascade_id) VALUES (?1)",
                [cascade_id],
            )
            .unwrap();
        drop(connection);
        path
    }

    fn reference(config_dir: &Path, executable: PathBuf) -> AgyConversation {
        AgyConversation {
            conversation: ID.into(),
            executable,
            config_dir: config_dir.to_owned(),
            options: vec![
                "--dangerously-skip-permissions".into(),
                "--model".into(),
                "gemini-2.5-pro".into(),
            ],
            title: Some("Ship the title".into()),
        }
    }

    #[test]
    fn identity_is_exact_canonical_lowercase_uuid() {
        assert!(is_exact_identity(ID));
        assert!(!is_exact_identity(&ID.to_uppercase()));
        assert!(!is_exact_identity("../../conversations/secret.db"));
        assert!(!is_exact_identity("01a08e5c7480705299649224aadebef0"));
    }

    #[test]
    fn validates_only_the_exact_database_row_and_never_writes_it() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("antigravity-cli");
        std::fs::create_dir_all(&config).unwrap();
        let executable = std::env::current_exe().unwrap();
        let history = database(&config, ID);
        let reference = reference(&config, executable);
        validate_profile(&reference, &config).unwrap();
        let before = std::fs::read(&history).unwrap();
        validate_profile(&reference, &config).unwrap();
        assert_eq!(std::fs::read(&history).unwrap(), before);

        let mut wrong = reference;
        wrong.conversation = "01a08e94-0deb-76c3-a2b5-540c4874a53f".into();
        assert!(validate_profile(&wrong, &config).is_err());
        std::fs::copy(&history, database_path(&config, &wrong.conversation)).unwrap();
        let error = validate_profile(&wrong, &config).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("does not contain the recorded conversation")
        );
    }

    #[test]
    fn recovery_retains_safe_options_and_builds_exact_conversation_resume() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("antigravity-cli");
        std::fs::create_dir_all(&config).unwrap();
        database(&config, ID);
        let executable = std::env::current_exe().unwrap();
        let reference = reference(&config, executable.clone());
        let argv = resume_argv_at(&reference, &config).unwrap();
        assert_eq!(argv[5], executable.as_os_str());
        assert_eq!(argv[6], "--conversation");
        assert_eq!(argv[7], ID);
        assert_eq!(
            &argv[8..],
            [
                "--dangerously-skip-permissions",
                "--model",
                "gemini-2.5-pro"
            ]
        );

        let options = launch_options(&[
            "agy".into(),
            "--conversation".into(),
            ID.into(),
            "--model=gemini-2.5-pro".into(),
            "--dangerously-skip-permissions".into(),
            "--mode".into(),
            "plan".into(),
            "--prompt-interactive".into(),
            "secret prompt".into(),
            "--effort".into(),
            "high".into(),
        ])
        .unwrap();
        assert_eq!(
            options,
            [
                "--model",
                "gemini-2.5-pro",
                "--dangerously-skip-permissions",
                "--mode",
                "plan",
                "--effort",
                "high"
            ]
        );
        assert!(
            !options
                .iter()
                .any(|option| option.contains("secret prompt"))
        );
        assert!(launch_options(&["agy".into(), "--settings".into(), "x".into()]).is_err());
        assert!(launch_options(&["agy".into(), "--settings".into()]).is_err());
        assert!(launch_options(&["agy".into(), "--settings=custom.json".into()]).is_err());
        assert!(launch_options(&["agy".into(), "--project".into(), "remote".into()]).is_err());
        assert!(launch_options(&["agy".into(), "models".into()]).is_err());
        assert!(launch_options(&["agy".into(), "--sandbox".into(), "false".into()]).is_err());
        assert!(launch_options(&["agy".into(), "--prompt-interactive=".into()]).is_err());
        for option in ["--model", "--effort", "--agent"] {
            assert!(
                launch_options(&["agy".into(), option.into(), "-hidden".into()]).is_err(),
                "{option} must reject a leading-hyphen value"
            );
            assert!(
                launch_options(&["agy".into(), format!("{option}=-hidden").into()]).is_err(),
                "{option} inline values must reject a leading hyphen"
            );
        }
        assert!(launch_options(&["agy".into(), "--sandbox=false".into()]).is_ok());
        assert!(launch_options(&["agy".into(), "--sandbox=true".into()]).is_ok());
        assert!(launch_options(&["agy".into(), "--".into()]).is_ok());
        assert!(launch_options(&["agy".into(), "--".into(), "secret prompt".into()]).is_err());
        assert!(
            launch_options(&["agy".into(), "--prompt-interactive".into(), "--".into()]).is_ok()
        );
        assert!(
            launch_options(&["agy".into(), "--new-project".into()])
                .unwrap()
                .is_empty()
        );
        assert!(launch_options(&["agy".into(), "--mode=accept-edits".into()]).is_ok());
        assert!(
            launch_options(&[
                "agy".into(),
                "--conversation".into(),
                ID.into(),
                "--conversation".into(),
                ID.into()
            ])
            .is_err()
        );
    }
}

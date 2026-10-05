//! Dashboard-owned offers for the installed harnesses' active native profiles.
use anyhow::Context;
use std::fs::{File, Metadata, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 1_048_576;

pub(super) fn run(confirm: &mut impl FnMut(&str) -> anyhow::Result<bool>) -> anyhow::Result<()> {
    let executable = super::agent_setup::binary()?;
    for (provider, harness, variable, directory, filename) in [
        (
            "Claude",
            "claude",
            "CLAUDE_CONFIG_DIR",
            ".claude",
            "settings.json",
        ),
        ("Codex", "codex", "CODEX_HOME", ".codex", "config.toml"),
    ] {
        if !std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|directory| {
                std::fs::metadata(directory.join(harness))
                    .is_ok_and(|file| file.is_file() && file.mode() & 0o111 != 0)
            })
        }) {
            continue;
        }
        let profile = std::env::var_os(variable)
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(directory)));
        let Some(profile) = profile else {
            super::startup::notice(
                &format!(
                    "{provider} reporting hooks unavailable: set HOME or {variable} to select the native profile"
                ),
                false,
            );
            continue;
        };
        let path = profile.join(filename);
        if let Err(error) = offer(provider, &path, &executable, confirm) {
            // Only the outer, controlled reason is shown. Native parser errors
            // and source text can contain private configuration or credentials.
            super::startup::notice(
                &format!(
                    "{provider} reporting hooks in {} were not changed: {error}; fix the native file and rerun OVRCR",
                    path.display()
                ),
                false,
            );
        }
    }
    Ok(())
}

fn offer(
    provider: &str,
    path: &Path,
    executable: &str,
    confirm: &mut impl FnMut(&str) -> anyhow::Result<bool>,
) -> anyhow::Result<()> {
    let snapshot = Snapshot::read(path)?;
    let text = if snapshot.entry.is_none() && provider == "Claude" {
        "{}"
    } else {
        &snapshot.text
    };
    let next = match provider {
        "Claude" => super::agent_setup::compose(text, executable),
        "Codex" => super::codex_setup::compose(text, executable),
        _ => unreachable!("only providers with global hooks are offered"),
    }
    .map_err(|_| {
        anyhow::anyhow!("native settings are invalid or have an incompatible hook structure")
    })?;
    if next == snapshot.text {
        return Ok(());
    }
    let effect = if provider == "Claude" {
        "Shows Claude activity and usage in OVRCR.\nKeeps your existing status-line renderer."
    } else {
        "Shows Codex activity and usage in OVRCR.\nReview and trust hooks in native Codex before tracking."
    };
    let prompt = format!(
        "{provider} reporting hooks\nInstall or repair hooks in:\n{}\n\n{effect}\nNative approvals and trust are preserved.\nReview native hook trust and delivery before relying on tracking.",
        path.display()
    );
    if !confirm(&prompt)? {
        return Ok(());
    }
    snapshot.write(&next)?;
    super::startup::notice(
        &format!(
            "{provider} reporting hooks updated. Native trust and delivery remain unverified."
        ),
        true,
    );
    Ok(())
}

#[derive(PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    length: u64,
    mode: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}

fn identity(metadata: &Metadata) -> Identity {
    Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
        length: metadata.len(),
        mode: metadata.mode(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    }
}

struct Snapshot {
    path: PathBuf,
    target: PathBuf,
    entry: Option<Identity>,
    file: Option<Identity>,
    permissions: Option<Permissions>,
    ancestor: Option<(PathBuf, Identity)>,
    text: String,
}

impl Snapshot {
    fn read(path: &Path) -> anyhow::Result<Self> {
        let entry = match std::fs::symlink_metadata(path) {
            Ok(metadata) => Some(identity(&metadata)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error).context("cannot inspect native settings path"),
        };
        let (target, ancestor) = if entry.is_some() {
            (
                std::fs::canonicalize(path)
                    .context("native settings path is a dangling link or cannot be resolved")?,
                None,
            )
        } else {
            let (target, ancestor, metadata) = resolve_missing(path)?;
            (target, Some((ancestor, identity(&metadata))))
        };
        let mut snapshot = Self {
            path: path.to_owned(),
            target,
            entry,
            file: None,
            permissions: None,
            ancestor,
            text: String::new(),
        };
        if snapshot.entry.is_some() {
            let metadata = std::fs::metadata(&snapshot.target)
                .context("cannot inspect native settings file")?;
            anyhow::ensure!(
                metadata.is_file(),
                "native settings path is not a regular file"
            );
            anyhow::ensure!(
                metadata.mode() & 0o222 != 0,
                "native settings file is read-only"
            );
            let mut file =
                File::open(&snapshot.target).context("cannot read native settings file")?;
            let original = identity(&metadata);
            anyhow::ensure!(
                identity(&file.metadata()?) == original,
                "native settings changed while reading"
            );
            let mut bytes = Vec::new();
            Read::by_ref(&mut file)
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .context("cannot read native settings file")?;
            anyhow::ensure!(
                bytes.len() as u64 <= MAX_BYTES,
                "native settings file exceeds 1 MiB"
            );
            anyhow::ensure!(
                identity(&file.metadata()?) == original
                    && identity(&std::fs::metadata(&snapshot.target)?) == original,
                "native settings changed while reading"
            );
            snapshot.text =
                String::from_utf8(bytes).context("native settings file is not UTF-8")?;
            snapshot.permissions = Some(metadata.permissions());
            snapshot.file = Some(original);
        }
        Ok(snapshot)
    }

    fn verify(&self) -> anyhow::Result<()> {
        let current =
            Self::read(&self.path).context("native settings changed during confirmation")?;
        anyhow::ensure!(
            current.target == self.target
                && current.entry == self.entry
                && current.file == self.file
                && current.text == self.text,
            "native settings changed during confirmation"
        );
        if let Some((path, before)) = &self.ancestor {
            let current = std::fs::metadata(path)
                .context("native profile directory changed during confirmation")?;
            anyhow::ensure!(
                current.dev() == before.device
                    && current.ino() == before.inode
                    && current.mode() == before.mode,
                "native profile directory changed during confirmation"
            );
        }
        Ok(())
    }

    fn write(&self, text: &str) -> anyhow::Result<()> {
        self.verify()?;
        let parent = self
            .target
            .parent()
            .ok_or_else(|| anyhow::anyhow!("native settings path has no parent"))?;
        std::fs::create_dir_all(parent).context("cannot create native profile directory")?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .context("cannot create temporary native settings file")?;
        temporary
            .write_all(text.as_bytes())
            .context("cannot write native settings file")?;
        if let Some(permissions) = &self.permissions {
            temporary
                .as_file()
                .set_permissions(permissions.clone())
                .context("cannot preserve native settings permissions")?;
        }
        temporary
            .as_file()
            .sync_all()
            .context("cannot sync native settings file")?;
        self.verify()?;
        let result = if self.entry.is_none() {
            temporary.persist_noclobber(&self.target)
        } else {
            temporary.persist(&self.target)
        };
        // Drop the owned temporary file even when PersistError is returned.
        result
            .map_err(|error| error.error)
            .context("cannot atomically replace native settings file")?;
        Ok(())
    }
}

fn resolve_missing(path: &Path) -> anyhow::Result<(PathBuf, PathBuf, Metadata)> {
    let mut ancestor = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(&ancestor) {
            Ok(_) => {
                let resolved = std::fs::canonicalize(&ancestor)
                    .context("native profile directory is a dangling link or cannot be resolved")?;
                let metadata = std::fs::metadata(&resolved)
                    .context("cannot inspect native profile directory")?;
                anyhow::ensure!(
                    metadata.is_dir(),
                    "native profile ancestor is not a directory"
                );
                let mut target = resolved.clone();
                for name in missing.into_iter().rev() {
                    target.push(name);
                }
                return Ok((target, resolved, metadata));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    ancestor
                        .file_name()
                        .ok_or_else(|| {
                            anyhow::anyhow!("native profile directory cannot be resolved")
                        })?
                        .to_owned(),
                );
                ancestor.pop();
            }
            Err(error) => return Err(error).context("cannot inspect native profile directory"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn run_detects_executables_and_only_changes_active_profiles() {
        // Give this entry-point test its own process environment; other unit
        // suites may inspect the provider profiles concurrently.
        if std::env::var_os("OVRCR_TEST_STARTUP_HOOKS_CHILD").as_deref()
            != Some(std::ffi::OsStr::new("1"))
        {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "cli::startup_hooks::tests::run_detects_executables_and_only_changes_active_profiles", "--nocapture"])
                .env("OVRCR_TEST_STARTUP_HOOKS_CHILD", "1")
                .output().unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for Environment {
            fn drop(&mut self) {
                for (key, value) in &self.0 {
                    unsafe {
                        match value {
                            Some(value) => std::env::set_var(key, value),
                            None => std::env::remove_var(key),
                        }
                    }
                }
            }
        }
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        let home = root.path().join("home");
        let claude = root.path().join("active-claude");
        let codex = root.path().join("active-codex");
        for path in [&bin, &home] {
            std::fs::create_dir(path).unwrap();
        }
        let _environment = Environment(
            ["PATH", "HOME", "CLAUDE_CONFIG_DIR", "CODEX_HOME"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        );
        unsafe {
            std::env::set_var("PATH", &bin);
            std::env::set_var("HOME", &home);
            std::env::set_var("CLAUDE_CONFIG_DIR", &claude);
            std::env::set_var("CODEX_HOME", &codex);
        }
        let historical = home.join(".claude-old/settings.json");
        std::fs::create_dir(historical.parent().unwrap()).unwrap();
        std::fs::write(&historical, "{}").unwrap();
        for name in ["pi", "omp", "grok", "claude"] {
            let path = bin.join(name);
            std::fs::write(&path, "#!/bin/sh\nexit 83\n").unwrap();
            std::fs::set_permissions(
                &path,
                std::fs::Permissions::from_mode(if name == "claude" { 0o600 } else { 0o700 }),
            )
            .unwrap();
        }
        run(&mut |_| panic!("managed-only and nonexecutable providers must not prompt")).unwrap();
        for name in ["claude", "codex"] {
            let path = bin.join(name);
            std::fs::write(&path, "#!/bin/sh\nexit 83\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut prompts = Vec::new();
        run(&mut |prompt| {
            prompts.push(prompt.to_owned());
            Ok(true)
        })
        .unwrap();
        assert_eq!(prompts.len(), 2);
        assert!(claude.join("settings.json").is_file());
        assert!(codex.join("config.toml").is_file());
        assert!(!home.join(".claude/settings.json").exists());
        assert!(!home.join(".codex/config.toml").exists());
        assert_eq!(std::fs::read_to_string(&historical).unwrap(), "{}");
        run(&mut |_| panic!("unchanged installed hooks must not prompt")).unwrap();
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
            std::env::remove_var("CODEX_HOME");
        }
        run(&mut |_| Ok(true)).unwrap();
        assert!(home.join(".claude/settings.json").is_file());
        assert!(home.join(".codex/config.toml").is_file());
    }

    #[test]
    fn offers_install_preserve_native_settings_and_stop_prompting_after_repair() {
        for (provider, filename, original) in [
            (
                "Claude",
                "settings.json",
                r#"{"permissions":{"allow":["Read"],"deny":["Bash(secret)"]},"statusLine":{"type":"command","command":"printf renderer","padding":2}}"#,
            ),
            (
                "Codex",
                "config.toml",
                "# native settings\n[projects.example]\ntrust_level = 'trusted' # trust\n[hooks.state.example]\ntrusted_hash = 'private-hash' # hash\n",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join(filename);
            std::fs::write(&path, original).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
            let mut prompts = Vec::new();
            offer(provider, &path, "/installed/ovrcr", &mut |prompt| {
                prompts.push(prompt.to_owned());
                Ok(false)
            })
            .unwrap();
            assert_eq!(prompts.len(), 1);
            assert!(prompts[0].contains(provider));
            assert!(prompts[0].contains(path.to_str().unwrap()));
            assert!(prompts[0].contains("reporting"));
            assert!(!prompts[0].contains("private-hash"));
            assert!(!prompts[0].contains("Bash(secret)"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            offer(provider, &path, "/installed/ovrcr", &mut |_| Ok(true)).unwrap();
            let installed = std::fs::read_to_string(&path).unwrap();
            assert!(installed.contains("/installed/ovrcr"));
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o640
            );
            if provider == "Claude" {
                let value: serde_json::Value = serde_json::from_str(&installed).unwrap();
                assert_eq!(value["permissions"]["deny"][0], "Bash(secret)");
                assert!(
                    value["statusLine"]["command"]
                        .as_str()
                        .unwrap()
                        .contains("--render-command 'printf renderer'")
                );
            } else {
                assert!(installed.contains("# native settings"));
                assert!(installed.contains("# trust"));
                assert!(installed.contains("# hash"));
                let value: toml::Value = toml::from_str(&installed).unwrap();
                assert_eq!(
                    value["hooks"]["state"]["example"]["trusted_hash"].as_str(),
                    Some("private-hash")
                );
            }
            offer(provider, &path, "/installed/ovrcr", &mut |_| {
                panic!("configured hooks must not prompt")
            })
            .unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), installed);
            offer(provider, &path, "/replacement/ovrcr", &mut |_| Ok(true)).unwrap();
            let repaired = std::fs::read_to_string(&path).unwrap();
            assert!(repaired.contains("/replacement/ovrcr"));
            assert!(!repaired.contains("/installed/ovrcr"));
        }
    }

    #[test]
    fn missing_settings_require_acceptance_and_valid_symlinks_keep_the_link() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("new/profile/settings.json");
        offer("Claude", &absent, "/installed/ovrcr", &mut |_| Ok(false)).unwrap();
        assert!(!root.path().join("new").exists());
        offer("Claude", &absent, "/installed/ovrcr", &mut |_| Ok(true)).unwrap();
        assert!(absent.is_file());
        assert_eq!(
            std::fs::metadata(&absent).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let target = root.path().join("actual.toml");
        let linked = root.path().join("config.toml");
        std::fs::write(&target, "# preserve\n").unwrap();
        symlink(&target, &linked).unwrap();
        offer("Codex", &linked, "/installed/ovrcr", &mut |_| Ok(true)).unwrap();
        assert_eq!(std::fs::read_link(&linked).unwrap(), target);
        assert!(
            std::fs::read_to_string(&target)
                .unwrap()
                .contains("report codex")
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
    }

    #[test]
    fn unsafe_settings_are_refused_without_prompt_or_replacement() {
        let root = tempfile::tempdir().unwrap();
        for (provider, text) in [
            ("Claude", "{ PRIVATE_SETTINGS: ["),
            ("Claude", r#"{"hooks":{"Stop":false}}"#),
            ("Claude", r#"{"statusLine":{"command":false}}"#),
            (
                "Claude",
                r#"{"hooks":{"Stop":[{"matcher":false,"hooks":[]}]}}"#,
            ),
            (
                "Claude",
                r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":false}]}]}}"#,
            ),
            (
                "Claude",
                r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo untouched","async":"false"}]}]}}"#,
            ),
            ("Codex", "private_settings = ["),
            ("Codex", "hooks.Stop = false\n"),
        ] {
            let path = root.path().join("native-config");
            std::fs::write(&path, text).unwrap();
            let error = offer(provider, &path, "/installed/ovrcr", &mut |_| {
                panic!("invalid config must not prompt")
            })
            .unwrap_err();
            assert!(!error.to_string().contains("PRIVATE_SETTINGS"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
        let linked = root.path().join("dangling.json");
        let missing = root.path().join("missing");
        symlink(&missing, &linked).unwrap();
        assert!(
            offer("Claude", &linked, "/installed/ovrcr", &mut |_| panic!(
                "dangling link must not prompt"
            ))
            .is_err()
        );
        assert_eq!(std::fs::read_link(&linked).unwrap(), missing);
        assert!(!missing.exists());
        let locked = root.path().join("locked.json");
        std::fs::write(&locked, "{}").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(
            offer("Claude", &locked, "/installed/ovrcr", &mut |_| panic!(
                "read-only file must not prompt"
            ))
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(&locked).unwrap(), "{}");
        let oversized = root.path().join("oversized");
        std::fs::write(&oversized, " ".repeat(1_048_577)).unwrap();
        assert!(
            offer("Codex", &oversized, "/installed/ovrcr", &mut |_| panic!(
                "oversized file must not prompt"
            ))
            .is_err()
        );
        assert_eq!(std::fs::metadata(&oversized).unwrap().len(), 1_048_577);
    }

    #[test]
    fn configuration_changes_during_confirmation_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("settings.json");
        std::fs::write(&path, "{}").unwrap();
        let result = offer("Claude", &path, "/installed/ovrcr", &mut |_| {
            std::fs::write(&path, r#"{"external":"edit"}"#).unwrap();
            Ok(true)
        });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"external":"edit"}"#
        );
        let absent = root.path().join("config.toml");
        let result = offer("Codex", &absent, "/installed/ovrcr", &mut |_| {
            std::fs::write(&absent, "# appeared during prompt\n").unwrap();
            Ok(true)
        });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&absent).unwrap(),
            "# appeared during prompt\n"
        );
        let identical = root.path().join("identical.json");
        std::fs::write(&identical, "{}").unwrap();
        let result = offer("Claude", &identical, "/installed/ovrcr", &mut |_| {
            let replacement = root.path().join("replacement.json");
            std::fs::write(&replacement, "{}").unwrap();
            std::fs::rename(&replacement, &identical).unwrap();
            Ok(true)
        });
        assert!(
            result.is_err(),
            "same-content replacement changes the file identity"
        );
        assert_eq!(std::fs::read_to_string(&identical).unwrap(), "{}");
        let first = root.path().join("first.json");
        let second = root.path().join("second.json");
        let link = root.path().join("linked.json");
        std::fs::write(&first, "{}").unwrap();
        std::fs::write(&second, "{}").unwrap();
        symlink(&first, &link).unwrap();
        let result = offer("Claude", &link, "/installed/ovrcr", &mut |_| {
            std::fs::remove_file(&link).unwrap();
            symlink(&second, &link).unwrap();
            Ok(true)
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read_link(&link).unwrap(), second);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "{}");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "{}");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 6);
    }
}

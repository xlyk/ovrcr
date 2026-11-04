//! Agent status: OVRCR's reporting hooks in the installed harnesses' active native
//! profiles. Startup offers them on one screen, the Dashboard says calmly when they
//! are off, and its palette's Connect agent status entry installs them later.
use anyhow::Context;
use std::fs::{File, Metadata, Permissions};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 1_048_576;

/// Provider, executable, profile variable, default profile directory, settings file.
const PROVIDERS: [(&str, &str, &str, &str, &str); 2] = [
    (
        "Claude",
        "claude",
        "CLAUDE_CONFIG_DIR",
        ".claude",
        "settings.json",
    ),
    ("Codex", "codex", "CODEX_HOME", ".codex", "config.toml"),
];

/// A settings file the screen offers to change, as read, and what would replace it.
struct Change {
    provider: &'static str,
    path: PathBuf,
    snapshot: Snapshot,
    next: String,
}

/// A provider whose status stays off, with the line that says why.
type Refusal = (&'static str, String);

/// A manual-migration note composition left for the caller to show.
type Note = Option<String>;

/// The startup screen. Declining (`n`, Enter) changes nothing.
pub(super) fn run(confirm: &mut impl FnMut(&str) -> anyhow::Result<bool>) -> anyhow::Result<()> {
    offer(
        targets(),
        &super::agent_setup::binary()?,
        confirm,
        &mut super::startup::notice,
    )
}

/// The Dashboard's one-time note while any installed provider's status is off.
pub(super) fn status_off() -> Option<String> {
    let executable = super::agent_setup::binary().ok()?;
    // Silent: the startup screen already showed any migration note.
    let (changes, refused, _) = prepare(targets(), &executable);
    let off: Vec<&str> = changes
        .iter()
        .map(|change| change.provider)
        .chain(refused.iter().map(|(provider, _)| *provider))
        .collect();
    (!off.is_empty()).then(|| format!("{} status off — :connect to set up", names(&off)))
}

/// The palette's Connect agent status entry: choosing it is the answer, so it writes
/// without asking. `Err` says what is still off and why.
pub(super) fn connect() -> Result<String, String> {
    let executable = super::agent_setup::binary()
        .map_err(|error| format!("Couldn't connect agent status: {error}"))?;
    install(targets(), &executable)
}

fn install(
    targets: Vec<(&'static str, Result<PathBuf, String>)>,
    executable: &str,
) -> Result<String, String> {
    let (changes, mut refused, notes) = prepare(targets, executable);
    let (connected, failed) = apply(changes);
    refused.extend(failed);
    let mut lines: Vec<String> = if refused.is_empty() && connected.is_empty() {
        vec!["Agent status is already connected".into()]
    } else {
        (!connected.is_empty())
            .then(|| connected_line(&connected))
            .into_iter()
            .collect()
    };
    let failed = !refused.is_empty();
    lines.extend(refused.into_iter().map(|(_, line)| line));
    // The palette's result line is where a migration note can be read.
    lines.extend(notes);
    let message = lines.join(" · ");
    if failed { Err(message) } else { Ok(message) }
}

/// Each installed harness's active settings file, or why it cannot be located.
fn targets() -> Vec<(&'static str, Result<PathBuf, String>)> {
    let installed = |harness: &str| {
        std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|directory| {
                std::fs::metadata(directory.join(harness))
                    .is_ok_and(|file| file.is_file() && file.mode() & 0o111 != 0)
            })
        })
    };
    PROVIDERS
        .into_iter()
        .filter(|(_, harness, ..)| installed(harness))
        .map(|(provider, _, variable, directory, filename)| {
            let profile = std::env::var_os(variable).map(PathBuf::from).or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(directory))
            });
            let path = profile.map(|profile| profile.join(filename)).ok_or_else(|| {
                format!(
                    "Couldn't connect {provider} status: set HOME or {variable} to choose its settings folder"
                )
            });
            (provider, path)
        })
        .collect()
}

/// Says why any target cannot be changed, then asks once for every file that needs
/// reporters and writes them only after `y`.
fn offer(
    targets: Vec<(&'static str, Result<PathBuf, String>)>,
    executable: &str,
    confirm: &mut impl FnMut(&str) -> anyhow::Result<bool>,
    notice: &mut impl FnMut(&str, bool),
) -> anyhow::Result<()> {
    let (changes, refused, notes) = prepare(targets, executable);
    for line in refused.iter().map(|(_, line)| line).chain(&notes) {
        notice(line, false);
    }
    if changes.is_empty() || !confirm(&prompt(&changes))? {
        return Ok(());
    }
    let (connected, failed) = apply(changes);
    for (_, line) in &failed {
        notice(line, false);
    }
    if !connected.is_empty() {
        notice(&connected_line(&connected), true);
    }
    Ok(())
}

/// Reads every target. Files already current drop out; a file that cannot be read or
/// composed is refused by name and reason, never by its contents. Also returns the
/// manual-migration notes composition left for the caller to show.
fn prepare(
    targets: Vec<(&'static str, Result<PathBuf, String>)>,
    executable: &str,
) -> (Vec<Change>, Vec<Refusal>, Vec<String>) {
    let mut changes = Vec::new();
    let mut refused = Vec::new();
    let mut notes = Vec::new();
    for (provider, path) in targets {
        let path = match path {
            Ok(path) => path,
            Err(line) => {
                refused.push((provider, line));
                continue;
            }
        };
        match read(provider, &path, executable) {
            Ok((change, note)) => {
                notes.extend(note);
                if let Some((snapshot, next)) = change {
                    changes.push(Change {
                        provider,
                        path,
                        snapshot,
                        next,
                    });
                }
            }
            Err(error) => refused.push((provider, refusal(provider, &path, &error))),
        }
    }
    (changes, refused, notes)
}

fn read(
    provider: &str,
    path: &Path,
    executable: &str,
) -> anyhow::Result<(Option<(Snapshot, String)>, Note)> {
    let snapshot = Snapshot::read(path)?;
    let text = if snapshot.entry.is_none() && provider == "Claude" {
        "{}"
    } else {
        &snapshot.text
    };
    let (next, note) = match provider {
        "Claude" => super::agent_setup::compose(text, executable),
        "Codex" => super::codex_setup::compose(text, executable).map(|next| (next, None)),
        _ => unreachable!("only providers with global hooks are offered"),
    }
    .map_err(|_| {
        anyhow::anyhow!("native settings are invalid or have an incompatible hook structure")
    })?;
    Ok(((next != snapshot.text).then_some((snapshot, next)), note))
}

/// Writes each change after checking its file is still as read.
fn apply(changes: Vec<Change>) -> (Vec<&'static str>, Vec<Refusal>) {
    let mut connected = Vec::new();
    let mut refused = Vec::new();
    for change in changes {
        match change.snapshot.write(&change.next) {
            Ok(()) => connected.push(change.provider),
            Err(error) => refused.push((
                change.provider,
                refusal(change.provider, &change.path, &error),
            )),
        }
    }
    (connected, refused)
}

// Only the outer, controlled reason is shown. Native parser errors and source
// text can contain private configuration or credentials.
fn refusal(provider: &str, path: &Path, error: &anyhow::Error) -> String {
    format!(
        "Couldn't connect {provider} status: {error} in {}; fix the file and try again",
        path.display()
    )
}

fn prompt(changes: &[Change]) -> String {
    let providers: Vec<&str> = changes.iter().map(|change| change.provider).collect();
    let mut text = format!(
        "Connect agent status\nLets OVRCR show when your {} sessions are working, need you or are done.\nAdds or updates OVRCR hooks in:\n",
        names(&providers)
    );
    for change in changes {
        text.push_str(&format!("  {}\n", change.path.display()));
    }
    text.push_str("Your other settings, status line and approvals stay as they are.");
    if providers.contains(&"Codex") {
        text.push_str("\nCodex asks you to trust the new hooks the next time it starts.");
    }
    text
}

fn connected_line(providers: &[&str]) -> String {
    let mut text = format!("{} status connected for new sessions", names(providers));
    if providers.contains(&"Codex") {
        text.push_str(" · Codex asks you to trust its hooks once");
    }
    text
}

fn names(providers: &[&str]) -> String {
    providers.join(" and ")
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
    use std::cell::RefCell;
    use std::os::unix::fs::{PermissionsExt, symlink};

    /// The startup screen for one file; returns the refusal lines it printed.
    fn offer_one(
        provider: &'static str,
        path: &Path,
        executable: &str,
        confirm: &mut impl FnMut(&str) -> anyhow::Result<bool>,
    ) -> Vec<String> {
        let mut refused = Vec::new();
        offer(
            vec![(provider, Ok(path.to_owned()))],
            executable,
            confirm,
            &mut |line, success| {
                if !success {
                    refused.push(line.to_owned());
                }
            },
        )
        .unwrap();
        refused
    }

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
        assert_eq!(
            status_off(),
            None,
            "no installed provider has hooks to offer"
        );
        for name in ["claude", "codex"] {
            let path = bin.join(name);
            std::fs::write(&path, "#!/bin/sh\nexit 83\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let off = Some("Claude and Codex status off — :connect to set up".to_owned());
        assert_eq!(status_off(), off);
        // Not now (Enter) writes nothing and leaves the calm note.
        let mut prompts = Vec::new();
        run(&mut |prompt| {
            prompts.push(prompt.to_owned());
            Ok(false)
        })
        .unwrap();
        assert_eq!(prompts.len(), 1, "one screen for every provider");
        assert!(!claude.exists() && !codex.exists());
        assert_eq!(status_off(), off);
        let mut prompts = Vec::new();
        run(&mut |prompt| {
            prompts.push(prompt.to_owned());
            Ok(true)
        })
        .unwrap();
        assert_eq!(prompts.len(), 1, "{prompts:?}");
        for path in [claude.join("settings.json"), codex.join("config.toml")] {
            assert!(prompts[0].contains(path.to_str().unwrap()), "{prompts:?}");
        }
        assert!(claude.join("settings.json").is_file());
        assert!(codex.join("config.toml").is_file());
        assert!(!home.join(".claude/settings.json").exists());
        assert!(!home.join(".codex/config.toml").exists());
        assert_eq!(std::fs::read_to_string(&historical).unwrap(), "{}");
        run(&mut |_| panic!("unchanged installed hooks must not prompt")).unwrap();
        assert_eq!(status_off(), None);
        assert_eq!(connect(), Ok("Agent status is already connected".into()));
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
            std::env::remove_var("CODEX_HOME");
        }
        // The palette's Connect entry installs in the default profiles without a prompt.
        assert_eq!(status_off(), off);
        let connected = connect().unwrap();
        assert!(
            connected.starts_with("Claude and Codex status connected"),
            "{connected}"
        );
        assert!(home.join(".claude/settings.json").is_file());
        assert!(home.join(".codex/config.toml").is_file());
        assert_eq!(status_off(), None);
    }

    #[test]
    fn one_screen_names_every_file_and_not_now_changes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let claude = root.path().join("settings.json");
        let codex = root.path().join("config.toml");
        std::fs::write(&claude, "{}").unwrap();
        std::fs::write(&codex, "# mine\n").unwrap();
        let targets = || vec![("Claude", Ok(claude.clone())), ("Codex", Ok(codex.clone()))];
        let mut prompts = Vec::new();
        let mut notices = Vec::new();
        offer(
            targets(),
            "/installed/ovrcr",
            &mut |prompt| {
                prompts.push(prompt.to_owned());
                Ok(false)
            },
            &mut |line, _| notices.push(line.to_owned()),
        )
        .unwrap();
        assert_eq!(prompts.len(), 1);
        let screen = &prompts[0];
        assert!(screen.starts_with("Connect agent status\n"), "{screen}");
        assert!(
            screen.contains("your Claude and Codex sessions are working, need you or are done"),
            "{screen}"
        );
        assert!(screen.contains(&format!("  {}\n", claude.display())));
        assert!(screen.contains(&format!("  {}\n", codex.display())));
        assert!(screen.contains("status line and approvals stay as they are"));
        assert!(screen.contains("Codex asks you to trust the new hooks"));
        assert!(
            !screen.contains("native hook trust and delivery"),
            "{screen}"
        );
        assert!(
            notices.is_empty(),
            "declining is not a problem: {notices:?}"
        );
        assert_eq!(std::fs::read_to_string(&claude).unwrap(), "{}");
        assert_eq!(std::fs::read_to_string(&codex).unwrap(), "# mine\n");

        offer(
            targets(),
            "/installed/ovrcr",
            &mut |_| Ok(true),
            &mut |line, success| notices.push(format!("{success} {line}")),
        )
        .unwrap();
        assert_eq!(
            notices,
            [
                "true Claude and Codex status connected for new sessions · Codex asks you to trust its hooks once"
            ]
        );
        assert!(
            std::fs::read_to_string(&claude)
                .unwrap()
                .contains("/installed/ovrcr")
        );
        assert!(
            std::fs::read_to_string(&codex)
                .unwrap()
                .contains("/installed/ovrcr")
        );

        // A Claude-only screen does not mention Codex trust.
        let only = root.path().join("only.json");
        let mut prompts = Vec::new();
        offer_one("Claude", &only, "/installed/ovrcr", &mut |prompt| {
            prompts.push(prompt.to_owned());
            Ok(false)
        });
        assert!(prompts[0].contains("your Claude sessions"), "{prompts:?}");
        assert!(!prompts[0].contains("Codex"), "{prompts:?}");
    }

    #[test]
    fn a_refused_file_is_named_before_the_screen_and_the_rest_still_connect() {
        let root = tempfile::tempdir().unwrap();
        let claude = root.path().join("settings.json");
        let codex = root.path().join("config.toml");
        std::fs::write(&claude, "{ PRIVATE_SETTINGS: [").unwrap();
        std::fs::write(&codex, "").unwrap();
        let log = RefCell::new(Vec::new());
        offer(
            vec![("Claude", Ok(claude.clone())), ("Codex", Ok(codex.clone()))],
            "/installed/ovrcr",
            &mut |prompt| {
                log.borrow_mut().push(format!("screen {prompt}"));
                Ok(true)
            },
            &mut |line, success| log.borrow_mut().push(format!("{success} {line}")),
        )
        .unwrap();
        let log = log.into_inner();
        assert_eq!(log.len(), 3, "{log:?}");
        assert!(
            log[0].starts_with("false Couldn't connect Claude status: ")
                && log[0].contains(claude.to_str().unwrap())
                && !log[0].contains("PRIVATE_SETTINGS"),
            "{log:?}"
        );
        assert!(log[1].starts_with("screen Connect agent status"), "{log:?}");
        assert!(log[1].contains("your Codex sessions") && !log[1].contains("settings.json"));
        assert!(log[2].starts_with("true Codex status connected"), "{log:?}");
        assert_eq!(
            std::fs::read_to_string(&claude).unwrap(),
            "{ PRIVATE_SETTINGS: ["
        );
    }

    #[test]
    fn install_writes_without_asking_and_says_what_stays_off() {
        let root = tempfile::tempdir().unwrap();
        let claude = root.path().join("settings.json");
        let codex = root.path().join("config.toml");
        std::fs::write(&codex, "hooks.Stop = false\n").unwrap();
        let targets = vec![
            ("Claude", Ok(claude.clone())),
            ("Codex", Ok(codex.clone())),
            (
                "Codex",
                Err("Couldn't connect Codex status: set HOME".to_owned()),
            ),
        ];
        let problem = install(targets, "/installed/ovrcr").unwrap_err();
        assert!(
            problem.starts_with(
                "Claude status connected for new sessions · Couldn't connect Codex status: "
            ),
            "{problem}"
        );
        assert!(problem.contains(codex.to_str().unwrap()), "{problem}");
        assert!(problem.ends_with(" · Couldn't connect Codex status: set HOME"));
        assert!(
            std::fs::read_to_string(&claude)
                .unwrap()
                .contains("/installed/ovrcr")
        );
        assert_eq!(
            std::fs::read_to_string(&codex).unwrap(),
            "hooks.Stop = false\n"
        );
        assert_eq!(
            install(vec![("Claude", Ok(claude.clone()))], "/installed/ovrcr"),
            Ok("Agent status is already connected".into())
        );
    }

    /// A status line wrapping the legacy reporter cannot be migrated automatically.
    /// The note reaches the user on the palette result line and, at startup, once
    /// before the screen; nothing writes it to stderr under the Dashboard.
    #[test]
    fn legacy_wrapper_note_is_returned_not_printed() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("settings.json");
        let legacy = r#"{"statusLine":{"type":"command","command":"/opt/wrap.sh claude-context"}}"#;
        std::fs::write(&path, legacy).unwrap();
        let log = RefCell::new(Vec::new());
        offer(
            vec![("Claude", Ok(path.clone()))],
            "/installed/ovrcr",
            &mut |_| {
                log.borrow_mut().push("screen".to_owned());
                Ok(false)
            },
            &mut |line, _| log.borrow_mut().push(line.to_owned()),
        )
        .unwrap();
        let log = log.into_inner();
        assert_eq!(log.len(), 2, "{log:?}");
        assert!(log[0].starts_with("Manual migration required"), "{log:?}");
        assert_eq!(log[1], "screen");

        let done = install(vec![("Claude", Ok(path.clone()))], "/installed/ovrcr").unwrap();
        assert!(
            done.starts_with(
                "Claude status connected for new sessions · Manual migration required"
            ),
            "{done}"
        );
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["statusLine"]["command"],
            "/opt/wrap.sh claude-context"
        );
        let again = install(vec![("Claude", Ok(path))], "/installed/ovrcr").unwrap();
        assert!(
            again.starts_with("Agent status is already connected · Manual migration required"),
            "{again}"
        );
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
            assert!(
                offer_one(provider, &path, "/installed/ovrcr", &mut |prompt| {
                    prompts.push(prompt.to_owned());
                    Ok(false)
                })
                .is_empty()
            );
            assert_eq!(prompts.len(), 1);
            assert!(prompts[0].contains(provider));
            assert!(prompts[0].contains(path.to_str().unwrap()));
            assert!(prompts[0].starts_with("Connect agent status\n"));
            assert!(!prompts[0].contains("private-hash"));
            assert!(!prompts[0].contains("Bash(secret)"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            assert!(offer_one(provider, &path, "/installed/ovrcr", &mut |_| Ok(true)).is_empty());
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
            assert!(
                offer_one(provider, &path, "/installed/ovrcr", &mut |_| {
                    panic!("configured hooks must not prompt")
                })
                .is_empty()
            );
            assert_eq!(std::fs::read_to_string(&path).unwrap(), installed);
            assert!(offer_one(provider, &path, "/replacement/ovrcr", &mut |_| Ok(true)).is_empty());
            let repaired = std::fs::read_to_string(&path).unwrap();
            assert!(repaired.contains("/replacement/ovrcr"));
            assert!(!repaired.contains("/installed/ovrcr"));
        }
    }

    #[test]
    fn missing_settings_require_acceptance_and_valid_symlinks_keep_the_link() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("new/profile/settings.json");
        assert!(offer_one("Claude", &absent, "/installed/ovrcr", &mut |_| Ok(false)).is_empty());
        assert!(!root.path().join("new").exists());
        assert!(offer_one("Claude", &absent, "/installed/ovrcr", &mut |_| Ok(true)).is_empty());
        assert!(absent.is_file());
        assert_eq!(
            std::fs::metadata(&absent).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let target = root.path().join("actual.toml");
        let linked = root.path().join("config.toml");
        std::fs::write(&target, "# preserve\n").unwrap();
        symlink(&target, &linked).unwrap();
        assert!(offer_one("Codex", &linked, "/installed/ovrcr", &mut |_| Ok(true)).is_empty());
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
            let refused = offer_one(provider, &path, "/installed/ovrcr", &mut |_| {
                panic!("invalid config must not prompt")
            });
            assert_eq!(refused.len(), 1, "{refused:?}");
            assert!(refused[0].starts_with(&format!("Couldn't connect {provider} status: ")));
            assert!(!refused[0].contains("PRIVATE_SETTINGS"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        }
        let linked = root.path().join("dangling.json");
        let missing = root.path().join("missing");
        symlink(&missing, &linked).unwrap();
        assert!(
            !offer_one("Claude", &linked, "/installed/ovrcr", &mut |_| panic!(
                "dangling link must not prompt"
            ))
            .is_empty()
        );
        assert_eq!(std::fs::read_link(&linked).unwrap(), missing);
        assert!(!missing.exists());
        let locked = root.path().join("locked.json");
        std::fs::write(&locked, "{}").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o444)).unwrap();
        assert!(
            !offer_one("Claude", &locked, "/installed/ovrcr", &mut |_| panic!(
                "read-only file must not prompt"
            ))
            .is_empty()
        );
        assert_eq!(std::fs::read_to_string(&locked).unwrap(), "{}");
        let oversized = root.path().join("oversized");
        std::fs::write(&oversized, " ".repeat(1_048_577)).unwrap();
        assert!(
            !offer_one("Codex", &oversized, "/installed/ovrcr", &mut |_| panic!(
                "oversized file must not prompt"
            ))
            .is_empty()
        );
        assert_eq!(std::fs::metadata(&oversized).unwrap().len(), 1_048_577);
    }

    #[test]
    fn configuration_changes_during_confirmation_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("settings.json");
        std::fs::write(&path, "{}").unwrap();
        let result = offer_one("Claude", &path, "/installed/ovrcr", &mut |_| {
            std::fs::write(&path, r#"{"external":"edit"}"#).unwrap();
            Ok(true)
        });
        assert_eq!(result.len(), 1, "{result:?}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"external":"edit"}"#
        );
        let absent = root.path().join("config.toml");
        let result = offer_one("Codex", &absent, "/installed/ovrcr", &mut |_| {
            std::fs::write(&absent, "# appeared during prompt\n").unwrap();
            Ok(true)
        });
        assert_eq!(result.len(), 1, "{result:?}");
        assert_eq!(
            std::fs::read_to_string(&absent).unwrap(),
            "# appeared during prompt\n"
        );
        let identical = root.path().join("identical.json");
        std::fs::write(&identical, "{}").unwrap();
        let result = offer_one("Claude", &identical, "/installed/ovrcr", &mut |_| {
            let replacement = root.path().join("replacement.json");
            std::fs::write(&replacement, "{}").unwrap();
            std::fs::rename(&replacement, &identical).unwrap();
            Ok(true)
        });
        assert_eq!(
            result.len(),
            1,
            "same-content replacement changes the file identity"
        );
        assert_eq!(std::fs::read_to_string(&identical).unwrap(), "{}");
        let first = root.path().join("first.json");
        let second = root.path().join("second.json");
        let link = root.path().join("linked.json");
        std::fs::write(&first, "{}").unwrap();
        std::fs::write(&second, "{}").unwrap();
        symlink(&first, &link).unwrap();
        let result = offer_one("Claude", &link, "/installed/ovrcr", &mut |_| {
            std::fs::remove_file(&link).unwrap();
            symlink(&second, &link).unwrap();
            Ok(true)
        });
        assert_eq!(result.len(), 1, "{result:?}");
        assert_eq!(std::fs::read_link(&link).unwrap(), second);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "{}");
        assert_eq!(std::fs::read_to_string(&second).unwrap(), "{}");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 6);
    }
}

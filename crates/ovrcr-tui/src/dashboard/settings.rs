use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// When OVRCR automatically creates a terminal named `local` for new workspaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutomaticLocalTerminals {
    On,
    Off,
    #[default]
    DefaultBranchOnly,
}

impl AutomaticLocalTerminals {
    pub const KEY: &'static str = "automatic_local_terminals";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::DefaultBranchOnly => "default_branch_only",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::DefaultBranchOnly => "default branch only",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "on" => Some(Self::On),
            "off" => Some(Self::Off),
            "default_branch_only" | "default branch only" => Some(Self::DefaultBranchOnly),
            _ => None,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::DefaultBranchOnly => Self::On,
            Self::On => Self::Off,
            Self::Off => Self::DefaultBranchOnly,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DashboardSettings {
    /// Desktop alerts for new background agent responses and Input requests. Off by default.
    pub desktop_notifications: bool,
    /// A sound for the same two alert kinds, independent of the desktop channel. Off by default.
    pub ready_sound: bool,
    /// Automatic `local` terminal creation policy. Default branch only when unset.
    pub automatic_local_terminals: AutomaticLocalTerminals,
    pub agents: Vec<AgentOverride>,
    pub picker_roots: Vec<PathBuf>,
    pub branch_prefix: String,
    pub launch_choices: BTreeMap<String, LaunchChoice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct AgentOverride {
    pub name: String,
    pub argv: Vec<String>,
}

impl Default for DashboardSettings {
    fn default() -> Self {
        Self {
            desktop_notifications: false,
            ready_sound: false,
            automatic_local_terminals: AutomaticLocalTerminals::default(),
            agents: Vec::new(),
            picker_roots: default_picker_roots(),
            branch_prefix: "feature/".into(),
            launch_choices: BTreeMap::new(),
        }
    }
}

#[derive(Deserialize, Default)]
struct RawSettings {
    #[serde(default)]
    desktop_notifications: bool,
    #[serde(default)]
    ready_sound: bool,
    #[serde(default)]
    automatic_local_terminals: Option<String>,
    #[serde(default)]
    agents: Vec<AgentOverride>,
    picker_roots: Option<Vec<String>>,
    branch_prefix: Option<String>,
    #[serde(default)]
    launch_choices: BTreeMap<String, LaunchChoice>,
}

pub fn load_dashboard_settings(path: &Path) -> (DashboardSettings, Option<String>) {
    match std::fs::read_to_string(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (DashboardSettings::default(), None)
        }
        Err(error) => (DashboardSettings::default(), Some(error.to_string())),
        Ok(contents) => match toml::from_str::<RawSettings>(&contents) {
            Ok(raw) => (raw.into_settings(), None),
            Err(error) => (DashboardSettings::default(), Some(error.to_string())),
        },
    }
}

/// Update only the selected preference, keeping other configuration values.
/// Publish with one rename so readers never see a partially written document.
pub(super) fn save_alert_setting(
    path: &Path,
    key: &str,
    enabled: bool,
) -> anyhow::Result<DashboardSettings> {
    save_settings(path, |document| {
        set_value(&mut document[key], enabled.into());
        Ok(())
    })
}

pub(super) fn save_automatic_local_terminals(
    path: &Path,
    policy: AutomaticLocalTerminals,
) -> anyhow::Result<DashboardSettings> {
    save_settings(path, |document| {
        set_value(
            &mut document[AutomaticLocalTerminals::KEY],
            policy.as_str().into(),
        );
        Ok(())
    })
}

fn set_value(item: &mut toml_edit::Item, mut value: toml_edit::Value) {
    if let Some(existing) = item.as_value() {
        *value.decor_mut() = existing.decor().clone();
    }
    *item = toml_edit::Item::Value(value);
}

/// Both edits read and validate the latest document before changing it.
fn save_settings(
    path: &Path,
    edit: impl FnOnce(&mut toml_edit::DocumentMut) -> anyhow::Result<()>,
) -> anyhow::Result<DashboardSettings> {
    use anyhow::{Context, bail};
    use std::io::Write;

    // A dangling link is not a missing config: never replace the link itself.
    let entry = match std::fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("inspect dashboard settings"),
    };
    // Follow an existing symlink rather than replacing the user's config link.
    let path = match std::fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && entry.is_none() => {
            path.to_path_buf()
        }
        Err(error) => return Err(error).context("locate dashboard settings"),
    };
    let permissions = match std::fs::metadata(&path) {
        Ok(metadata) => {
            let permissions = metadata.permissions();
            if permissions.readonly() {
                bail!("dashboard settings are read-only");
            }
            Some(permissions)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("inspect dashboard settings permissions"),
    };
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).context("read dashboard settings"),
    };
    // Refuse to overwrite invalid settings, including incorrectly typed values.
    toml::from_str::<RawSettings>(&contents).context("parse dashboard settings")?;
    let mut document: toml_edit::DocumentMut = contents.parse()?;
    edit(&mut document)?;
    let contents = document.to_string();
    let settings = toml::from_str::<RawSettings>(&contents)?.into_settings();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).context("create dashboard settings directory")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("create temporary dashboard settings")?;
    temporary
        .write_all(contents.as_bytes())
        .context("write dashboard settings")?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions)
            .context("preserve dashboard settings permissions")?;
    }
    temporary
        .as_file()
        .sync_all()
        .context("sync dashboard settings")?;
    temporary
        .persist(&path)
        // PersistError owns the temporary file; discard it even if the caller retains the error.
        .map_err(|error| error.error)
        .context("replace dashboard settings")?;
    Ok(settings)
}

impl RawSettings {
    fn into_settings(self) -> DashboardSettings {
        DashboardSettings {
            desktop_notifications: self.desktop_notifications,
            ready_sound: self.ready_sound,
            automatic_local_terminals: self
                .automatic_local_terminals
                .as_deref()
                .and_then(AutomaticLocalTerminals::parse)
                .unwrap_or_default(),
            agents: self.agents,
            picker_roots: self
                .picker_roots
                .map(|roots| roots.into_iter().map(|root| expand_tilde(&root)).collect())
                .unwrap_or_else(default_picker_roots),
            branch_prefix: self.branch_prefix.unwrap_or_else(|| "feature/".into()),
            launch_choices: self.launch_choices,
        }
    }
}

fn default_picker_roots() -> Vec<PathBuf> {
    ["~/Code", "~/src", "~"]
        .into_iter()
        .map(expand_tilde)
        .filter(|path| path.exists())
        .collect()
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        home_dir()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        PathBuf::from(path)
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
pub(crate) static HOME_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn desktop_notifications_default_off_and_explicit_config_opt_in() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("dashboard.toml");
        assert!(!load_dashboard_settings(&path).0.desktop_notifications);
        std::fs::write(&path, "desktop_notifications = true\n").unwrap();
        let (settings, error) = load_dashboard_settings(&path);
        assert!(settings.desktop_notifications);
        assert!(error.is_none());
        std::fs::write(&path, "desktop_notifications = false\n").unwrap();
        assert!(!load_dashboard_settings(&path).0.desktop_notifications);
        std::fs::write(&path, "desktop_notifications = \"yes\"\n").unwrap();
        let (settings, error) = load_dashboard_settings(&path);
        assert!(!settings.desktop_notifications);
        assert!(error.is_some());
    }

    #[test]
    fn ready_sound_default_off_and_independent_of_desktop_notifications() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("dashboard.toml");
        assert!(!load_dashboard_settings(&path).0.ready_sound);
        std::fs::write(&path, "ready_sound = true\n").unwrap();
        let (settings, error) = load_dashboard_settings(&path);
        assert!(settings.ready_sound);
        assert!(!settings.desktop_notifications);
        assert!(error.is_none());
        std::fs::write(&path, "desktop_notifications = true\n").unwrap();
        let settings = load_dashboard_settings(&path).0;
        assert!(settings.desktop_notifications);
        assert!(!settings.ready_sound);
        std::fs::write(&path, "ready_sound = \"yes\"\n").unwrap();
        let (settings, error) = load_dashboard_settings(&path);
        assert!(!settings.ready_sound);
        assert!(error.is_some());
    }

    #[test]
    fn automatic_local_terminals_defaults_and_parses() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("dashboard.toml");
        assert_eq!(
            load_dashboard_settings(&path).0.automatic_local_terminals,
            AutomaticLocalTerminals::DefaultBranchOnly
        );
        std::fs::write(&path, "automatic_local_terminals = \"on\"\n").unwrap();
        assert_eq!(
            load_dashboard_settings(&path).0.automatic_local_terminals,
            AutomaticLocalTerminals::On
        );
        save_automatic_local_terminals(&path, AutomaticLocalTerminals::Off).unwrap();
        let (settings, error) = load_dashboard_settings(&path);
        assert!(error.is_none());
        assert_eq!(
            settings.automatic_local_terminals,
            AutomaticLocalTerminals::Off
        );
        std::fs::write(&path, "automatic_local_terminals = \"nope\"\n").unwrap();
        let (settings, error) = load_dashboard_settings(&path);
        assert!(
            error.is_none(),
            "unknown spelling falls back without parse failure"
        );
        assert_eq!(
            settings.automatic_local_terminals,
            AutomaticLocalTerminals::DefaultBranchOnly
        );
    }

    #[test]
    fn loads_overrides_and_expands_picker_roots() {
        let _guard = HOME_ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let previous_home = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", &home) };

        let path = dir.path().join("dashboard.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            "picker_roots = [\"~/Code\"]\n[[agents]]\nname = \"claude\"\nargv = [\"claude\", \"--verbose\"]"
        )
        .unwrap();

        let (settings, error) = load_dashboard_settings(&path);
        if let Some(home) = previous_home {
            unsafe { std::env::set_var("HOME", home) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }

        assert_eq!(error, None);
        assert_eq!(
            settings.agents,
            vec![AgentOverride {
                name: "claude".into(),
                argv: vec!["claude".into(), "--verbose".into()],
            }]
        );
        assert_eq!(settings.picker_roots, vec![home.join("Code")]);
        assert_eq!(settings.branch_prefix, "feature/");
    }

    #[test]
    fn invalid_file_yields_defaults_and_error() {
        let _guard = HOME_ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(home.join("Code")).unwrap();
        let previous_home = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", &home) };

        let path = dir.path().join("dashboard.toml");
        std::fs::write(&path, "not = toml {").unwrap();
        let (settings, error) = load_dashboard_settings(&path);
        if let Some(home) = previous_home {
            unsafe { std::env::set_var("HOME", home) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }

        assert!(error.is_some());
        assert_eq!(settings.agents, Vec::<AgentOverride>::new());
        assert_eq!(settings.picker_roots, vec![home.join("Code"), home]);
        assert_eq!(settings.branch_prefix, "feature/");
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "preset")]
pub enum LaunchChoice {
    Terminal,
    Agent(String),
}

pub(super) fn save_launch_choice(
    path: &Path,
    project: &str,
    choice: &LaunchChoice,
) -> Result<(), String> {
    save_settings(path, |document| {
        use anyhow::Context;
        let choices = document
            .entry("launch_choices")
            .or_insert(toml_edit::table())
            .as_table_like_mut()
            .context("launch_choices must be a table")?;
        let entry = choices
            .entry(project)
            .or_insert(toml_edit::table())
            .as_table_like_mut()
            .context("launch choice must be a table")?;
        let kind = match choice {
            LaunchChoice::Terminal => {
                entry.remove("preset");
                "Terminal"
            }
            LaunchChoice::Agent(preset) => {
                set_value(
                    entry.entry("preset").or_insert(toml_edit::Item::None),
                    preset.clone().into(),
                );
                "Agent"
            }
        };
        set_value(
            entry.entry("kind").or_insert(toml_edit::Item::None),
            kind.into(),
        );
        Ok(())
    })
    .map(|_| ())
    .map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn failed_replacement_cleans_temporary_file_before_returning_error() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("dashboard.toml");
        // Deterministically obstruct replacement after reading, without a race or fault hook.
        let result = save_settings(&path, |document| {
            document["ready_sound"] = toml_edit::value(true);
            std::fs::create_dir(&path)?;
            Ok(())
        });
        assert!(result.is_err());
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        drop(result);
    }

    #[test]
    fn launch_edits_preserve_existing_table_styles_and_unknown_fields() {
        for original in [
            "# config\n[launch_choices.'project.with.dots'] # project\nkind = 'Agent' # kind\npreset = 'old' # preset\nextra = 42 # future\n",
            "# config\nlaunch_choices = { 'project.with.dots' = { kind = 'Agent', preset = 'old', extra = 42 } } # future\n",
            "# config\nlaunch_choices.'project.with.dots'.kind = 'Agent' # kind\nlaunch_choices.'project.with.dots'.preset = 'old' # preset\nlaunch_choices.'project.with.dots'.extra = 42 # future\n",
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("dashboard.toml");
            std::fs::write(&path, original).unwrap();
            for choice in [LaunchChoice::Agent("new".into()), LaunchChoice::Terminal] {
                save_launch_choice(&path, "project.with.dots", &choice).unwrap();
                let saved = std::fs::read_to_string(&path).unwrap();
                assert!(saved.contains("# config"));
                assert!(saved.contains("# future"));
                assert!(saved.contains("extra = 42"));
                if matches!(choice, LaunchChoice::Agent(_)) {
                    assert_eq!(
                        saved,
                        original
                            .replace("'old'", "\"new\"")
                            .replace("'Agent'", "\"Agent\"")
                    );
                } else {
                    assert!(!saved.contains("preset"));
                }
                let (loaded, error) = load_dashboard_settings(&path);
                assert_eq!(error, None);
                assert_eq!(
                    loaded.launch_choices.get("project.with.dots"),
                    Some(&choice)
                );
            }
        }
    }

    #[test]
    fn launch_save_preserves_unrelated_settings_and_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("dashboard.toml");
        std::fs::write(&path, "ready_sound = true\n[future]\nvalue = 42\n").unwrap();
        let occupied = path.with_extension("toml.tmp");
        std::fs::write(&occupied, "unrelated").unwrap();
        save_launch_choice(
            &path,
            "project.with.dots",
            &LaunchChoice::Agent("fixture".into()),
        )
        .unwrap();
        let document: toml::Table =
            toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(document["future"]["value"].as_integer(), Some(42));
        assert_eq!(std::fs::read_to_string(occupied).unwrap(), "unrelated");
        let (loaded, error) = load_dashboard_settings(&path);
        assert!(error.is_none());
        assert!(loaded.ready_sound);
        assert_eq!(
            loaded.launch_choices.get("project.with.dots"),
            Some(&LaunchChoice::Agent("fixture".into()))
        );
        assert_eq!(
            std::fs::read_dir(root.path()).unwrap().count(),
            2,
            "no disposable writer file remains"
        );
    }
}

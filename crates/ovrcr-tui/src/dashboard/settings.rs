use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DashboardSettings {
    /// Desktop alerts for new background agent responses and Input requests. Off by default.
    pub desktop_notifications: bool,
    /// A sound for the same two alert kinds, independent of the desktop channel. Off by default.
    pub ready_sound: bool,
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
    let mut value = toml_edit::Value::from(enabled);
    if let Some(existing) = document.get(key).and_then(toml_edit::Item::as_value) {
        *value.decor_mut() = existing.decor().clone();
    }
    document[key] = toml_edit::Item::Value(value);
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
        .context("replace dashboard settings")?;
    Ok(settings)
}

impl RawSettings {
    fn into_settings(self) -> DashboardSettings {
        DashboardSettings {
            desktop_notifications: self.desktop_notifications,
            ready_sound: self.ready_sound,
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
    let contents = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    let mut document: toml::Table = toml::from_str(&contents).map_err(|e| e.to_string())?;
    let choices = document
        .entry("launch_choices")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or("launch_choices must be a table")?;
    choices.insert(
        project.into(),
        toml::Value::try_from(choice).map_err(|e| e.to_string())?,
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let contents = toml::to_string_pretty(&document).map_err(|e| e.to_string())?;
    for _ in 0..100 {
        let temporary = parent.join(format!(
            ".ovrcr-launch-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        };
        let result = (|| {
            file.write_all(contents.as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        return result.map_err(|e| e.to_string());
    }
    Err("Could not allocate a settings temporary file".into())
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
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

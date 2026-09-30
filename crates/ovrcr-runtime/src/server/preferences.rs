//! Global user preferences the server reads from `dashboard.toml`.
//!
//! Automatic local-terminal creation is decided here so CLI and Dashboard
//! provisioning share one policy. Missing or invalid values use the default.

use super::*;
use std::fmt;

/// When OVRCR automatically creates a terminal named `local`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutomaticLocalTerminals {
    /// Every newly provisioned workspace gets a local terminal.
    On,
    /// Never create local terminals automatically.
    Off,
    /// Only the project's detected default-branch (root) workspace gets one.
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

    /// Cycle for the Dashboard preference control.
    pub fn next(self) -> Self {
        match self {
            Self::DefaultBranchOnly => Self::On,
            Self::On => Self::Off,
            Self::Off => Self::DefaultBranchOnly,
        }
    }

    /// Whether automatic provisioning should create a `local` terminal.
    ///
    /// `is_default_branch_workspace` is true only for the protected repository-root
    /// workspace (the detected default-branch checkout), never a feature worktree.
    pub fn should_create(self, is_default_branch_workspace: bool) -> bool {
        match self {
            Self::On => true,
            Self::Off => false,
            Self::DefaultBranchOnly => is_default_branch_workspace,
        }
    }
}

impl fmt::Display for AutomaticLocalTerminals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Resolve the preference beside the registry, matching `title_model` lookup.
pub fn load_automatic_local_terminals(registry_path: &Path) -> AutomaticLocalTerminals {
    let path = std::env::var_os("OVRCR_DASHBOARD_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            registry_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("dashboard.toml")
        });
    let Ok(text) = fs::read_to_string(path) else {
        return AutomaticLocalTerminals::default();
    };
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return AutomaticLocalTerminals::default();
    };
    value
        .get(AutomaticLocalTerminals::KEY)
        .and_then(|value| value.as_str())
        .and_then(AutomaticLocalTerminals::parse)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static DASHBOARD_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_is_default_branch_only() {
        assert_eq!(
            AutomaticLocalTerminals::default(),
            AutomaticLocalTerminals::DefaultBranchOnly
        );
        assert!(AutomaticLocalTerminals::DefaultBranchOnly.should_create(true));
        assert!(!AutomaticLocalTerminals::DefaultBranchOnly.should_create(false));
        assert!(AutomaticLocalTerminals::On.should_create(false));
        assert!(!AutomaticLocalTerminals::Off.should_create(true));
    }

    #[test]
    fn parse_accepts_documented_spellings() {
        assert_eq!(
            AutomaticLocalTerminals::parse("on"),
            Some(AutomaticLocalTerminals::On)
        );
        assert_eq!(
            AutomaticLocalTerminals::parse("off"),
            Some(AutomaticLocalTerminals::Off)
        );
        assert_eq!(
            AutomaticLocalTerminals::parse("default_branch_only"),
            Some(AutomaticLocalTerminals::DefaultBranchOnly)
        );
        assert_eq!(
            AutomaticLocalTerminals::parse("default branch only"),
            Some(AutomaticLocalTerminals::DefaultBranchOnly)
        );
        assert_eq!(AutomaticLocalTerminals::parse("always"), None);
        assert_eq!(AutomaticLocalTerminals::parse(""), None);
    }

    #[test]
    fn missing_or_invalid_dashboard_toml_uses_default() {
        let _guard = DASHBOARD_ENV_LOCK.lock().unwrap();
        // Prefer sibling dashboard.toml over a leaked env from another test.
        unsafe { std::env::remove_var("OVRCR_DASHBOARD_CONFIG") };
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        assert_eq!(
            load_automatic_local_terminals(&config),
            AutomaticLocalTerminals::DefaultBranchOnly
        );
        std::fs::write(root.path().join("dashboard.toml"), "not = toml {\n").unwrap();
        assert_eq!(
            load_automatic_local_terminals(&config),
            AutomaticLocalTerminals::DefaultBranchOnly
        );
        std::fs::write(
            root.path().join("dashboard.toml"),
            "automatic_local_terminals = 42\n",
        )
        .unwrap();
        assert_eq!(
            load_automatic_local_terminals(&config),
            AutomaticLocalTerminals::DefaultBranchOnly
        );
        std::fs::write(
            root.path().join("dashboard.toml"),
            "automatic_local_terminals = \"off\"\nready_sound = true\n",
        )
        .unwrap();
        assert_eq!(
            load_automatic_local_terminals(&config),
            AutomaticLocalTerminals::Off
        );
    }

    #[test]
    fn env_override_selects_dashboard_path() {
        let _guard = DASHBOARD_ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        let custom = root.path().join("custom-dashboard.toml");
        std::fs::write(&custom, "automatic_local_terminals = \"on\"\n").unwrap();
        let previous = std::env::var_os("OVRCR_DASHBOARD_CONFIG");
        unsafe { std::env::set_var("OVRCR_DASHBOARD_CONFIG", &custom) };
        let loaded = load_automatic_local_terminals(&config);
        match previous {
            Some(value) => unsafe { std::env::set_var("OVRCR_DASHBOARD_CONFIG", value) },
            None => unsafe { std::env::remove_var("OVRCR_DASHBOARD_CONFIG") },
        }
        assert_eq!(loaded, AutomaticLocalTerminals::On);
    }
}

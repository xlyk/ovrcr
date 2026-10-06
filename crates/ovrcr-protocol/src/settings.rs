//! Typed settings and the report the Server's settings loader produces.
//!
//! The loader lives in `ovrcr-runtime`; these types are shared so the CLI and
//! the Dashboard read the same reading. See ADR 0007.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

/// When OVRCR automatically creates a terminal named `local`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOverride {
    pub name: String,
    pub argv: Vec<String>,
}

/// A plain enum on the wire: bincode cannot decode serde-tagged enums. The
/// settings loader reads the document's `kind`/`preset` form itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaunchChoice {
    Terminal,
    Agent(String),
}

/// One native CLI the Server runs for account quota.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCommand {
    pub command: PathBuf,
    pub home: Option<PathBuf>,
}

/// Experimental personal Cursor dashboard reader; no credentials in settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorQuotaSettings {
    pub dashboard: bool,
    /// Absolute SQLite state.vscdb path, or the platform default when absent.
    pub state_db: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaSettings {
    /// Codex, Grok and opt-in Cursor account collection. On by default while a Dashboard is
    /// attached. `false` is the explicit off switch. The readers do not rewrite
    /// auth files.
    pub enabled: bool,
    /// Consent setting: a hidden Claude probe may spend allowance to read it.
    pub claude_probe: bool,
    pub codex: NativeCommand,
    pub grok: NativeCommand,
    pub cursor: CursorQuotaSettings,
}

impl Default for QuotaSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            cursor: CursorQuotaSettings::default(),
            claude_probe: false,
            codex: NativeCommand {
                command: "codex".into(),
                home: None,
            },
            grok: NativeCommand {
                command: "grok".into(),
                home: None,
            },
        }
    }
}

/// Every effective value. `picker_roots` is already `~`-expanded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub desktop_notifications: bool,
    pub ready_sound: bool,
    pub automatic_local_terminals: AutomaticLocalTerminals,
    /// `provider/model`, validated; `None` means titles are off.
    pub title_model: Option<String>,
    pub branch_prefix: String,
    pub picker_roots: Vec<PathBuf>,
    pub agents: Vec<AgentOverride>,
    pub launch_choices: BTreeMap<String, LaunchChoice>,
    pub quota: QuotaSettings,
    /// Consent setting, off until set. When on, a dirty feature worktree is
    /// pushed to `origin/wip/<branch>` when that workspace is removed and when
    /// the server shuts down, without a prompt. The checkout's own branch is
    /// not moved. The root workspace is never saved.
    #[serde(default)]
    pub save_uncommitted_work: bool,
}

/// Shown while `save_uncommitted_work` is off.
pub const SAVE_UNCOMMITTED_WORK_OFF: &str = "Uncommitted workspace work is not pushed. Set `save_uncommitted_work = true` to save it to origin/wip/<branch> when a workspace is removed or the server shuts down.";

/// Every default except `picker_roots`, which is empty here: the loader
/// fills it with whichever of `~/Code`, `~/src` and `~` exist.
impl Default for Settings {
    fn default() -> Self {
        Self {
            desktop_notifications: false,
            ready_sound: false,
            automatic_local_terminals: AutomaticLocalTerminals::default(),
            title_model: None,
            branch_prefix: "feature/".into(),
            picker_roots: Vec::new(),
            agents: Vec::new(),
            launch_choices: BTreeMap::new(),
            quota: QuotaSettings::default(),
            save_uncommitted_work: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingOwner {
    Server,
    Dashboard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingSource {
    Default,
    Document,
}

/// One setting's effective value for display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingRow {
    /// Dotted key path, for example `quota.codex.home`.
    pub key: String,
    pub owner: SettingOwner,
    /// Display text; `None` means unset.
    pub value: Option<String>,
    pub source: SettingSource,
    /// Display text of the default; `None` means unset by default.
    pub default: Option<String>,
    /// For a consent setting that is off: what to set to turn it on.
    pub off_state: Option<String>,
}

/// One load problem. `key` is `None` for a document-level finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsFinding {
    pub key: Option<String>,
    pub message: String,
    pub line: Option<u32>,
}

/// The Server's reading of the settings document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsReport {
    pub path: PathBuf,
    pub read_unix_ms: u64,
    pub settings: Settings,
    pub rows: Vec<SettingRow>,
    pub findings: Vec<SettingsFinding>,
    /// The document is not valid TOML: every setting is its default and the
    /// Server refuses edits until the file is fixed by hand.
    pub unparseable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

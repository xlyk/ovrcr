//! The settings catalog: one declaration of every setting path.
//!
//! The writer, the loader's rows, and the Dashboard editor read this module.
//! It does not hold the effective value. See ADR 0007.
use crate::{CLAUDE_PROBE_DESCRIPTION, CURSOR_QUOTA_DESCRIPTION};
use crate::{
    DESKTOP_NOTIFICATIONS_OFF, ITERM_FOCUS_OFF, LaunchChoice, QUOTA_OFF, ReadySoundChoice,
    SAVE_UNCOMMITTED_WORK_OFF, SettingOwner, SettingRow, SettingSource, Settings, TITLES_OFF,
    ThemeId,
};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

/// One step of a setting path after it has been parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingName<'a> {
    Key(&'a str),
    Index(usize),
}

/// What the writer accepts at a path. The strings are the refusal text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingWrite {
    Text(&'static str),
    Typed(&'static str),
    Entry(&'static [&'static str], &'static str),
}

/// What a write accepts and what the Dashboard editor presents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Toggle,
    Text(&'static str),
    Path(&'static str),
    Pick {
        options: &'static [&'static str],
        expected: &'static str,
    },
    /// A list of paths. The parent row is fixed; each child is a path.
    Paths,
    /// A list of `{ name, argv }` tables.
    Agents,
    /// A map of `{ kind, preset }` tables.
    Launches,
}

const AGENT_FIELDS: &[&str] = &["name", "argv"];
const LAUNCH_FIELDS: &[&str] = &["kind", "preset"];

const NOTIFY_ABOUT: &[&str] = &[
    "Turning this on may show an OS notification permission prompt.",
    #[cfg(target_os = "macos")]
    "macOS uses the installed OVRCR Bridge app. In Browse, O checks unconfirmed permission or opens System Settings when denied; choose Notifications → OVRCR.",
];

const SOUND_ABOUT: &[&str] = &[
    #[cfg(target_os = "macos")]
    "macOS plays the selected sound with new banners only when Desktop notifications and Ready sound are both on.",
];

const SOUND_CHOICE_ABOUT: &[&str] = &[
    "Used for new Ready and Input banners when both alert settings are on. Changing this does not preview a sound or replay alerts.",
    "An invalid saved choice keeps banners silent until you choose a valid sound or reset this setting.",
];

const ITERM_ABOUT: &[&str] = &[
    "A separate opt-in for selecting the existing iTerm session hosting the current Dashboard.",
    "On macOS choose Set up iTerm focus in the palette to request Automation permission explicitly. Banner clicks never request it.",
    "Unavailable or denied control keeps Dashboard navigation and parent-app activation; that fallback cannot choose an exact window.",
];

const QUOTA_ABOUT: &[&str] = &[
    "On by default while a Dashboard is attached. The readers do not rewrite auth files. Set false to turn Codex, Grok and the opt-in Cursor reader off.",
];

const TITLE_ABOUT: &[&str] = &["Setting a model makes paid calls to title sessions."];

const THEME_ABOUT: &[&str] = &[
    "80 built-in palettes across 27 families: 54 Dark and 26 Light. Catppuccin Mocha is the default (alias dark); Catppuccin Latte also accepts light.",
    "Changing theme updates the Dashboard immediately; the choice persists in dashboard.toml.",
];

const STATE_DB_ABOUT: &[&str] =
    &["Optional absolute path to Cursor's state.vscdb; otherwise the platform default."];

/// One top-level setting path, in the order the Dashboard documents.
pub struct Entry {
    pub path: &'static str,
    pub shape: Shape,
    pub owner: SettingOwner,
    pub group: &'static str,
    pub label: &'static str,
    pub off_state: Option<&'static str>,
    pub about: &'static [&'static str],
    value: fn(&Settings) -> Option<String>,
    show_off: fn(&Settings) -> bool,
}

impl Entry {
    pub fn value(&self, settings: &Settings) -> Option<String> {
        (self.value)(settings)
    }
}

/// The settings catalog.
pub fn entries() -> &'static [Entry] {
    &CATALOG
}

/// Child field names of a list or map of tables, for the loader's unknown-field check.
pub fn element_fields(path: &str) -> Option<&'static [&'static str]> {
    match entries().iter().find(|entry| entry.path == path)?.shape {
        Shape::Agents => Some(AGENT_FIELDS),
        Shape::Launches => Some(LAUNCH_FIELDS),
        _ => None,
    }
}

/// The writer's kind for a parsed path, including list indexes and map keys.
///
/// Dotted paths arrive as separate keys (`quota`, `cursor`, `state_db`).
pub fn write_of(path: &[SettingName<'_>]) -> Option<SettingWrite> {
    let mut dotted = String::new();
    for (index, step) in path.iter().enumerate() {
        let SettingName::Key(name) = step else {
            return None;
        };
        if !dotted.is_empty() {
            dotted.push('.');
        }
        dotted.push_str(name);
        let Some(shape) = entries()
            .iter()
            .find(|entry| entry.path == dotted)
            .map(|entry| entry.shape)
        else {
            continue;
        };
        let rest = &path[index + 1..];
        return Some(match (shape, rest) {
            (Shape::Toggle, []) => SettingWrite::Typed("a boolean"),
            (Shape::Text(expected) | Shape::Path(expected), []) => SettingWrite::Text(expected),
            (Shape::Pick { expected, .. }, []) => SettingWrite::Text(expected),
            (Shape::Paths, []) => SettingWrite::Typed("an array of paths"),
            (Shape::Paths, [SettingName::Index(_)]) => SettingWrite::Text("a path"),
            (Shape::Agents, []) => SettingWrite::Typed("an array of { name, argv } tables"),
            (Shape::Agents, [SettingName::Index(_)]) => {
                SettingWrite::Entry(AGENT_FIELDS, "an inline table { name, argv }")
            }
            (Shape::Agents, [SettingName::Index(_), SettingName::Key("name")]) => {
                SettingWrite::Text("a string")
            }
            (Shape::Agents, [SettingName::Index(_), SettingName::Key("argv")]) => {
                SettingWrite::Typed("an array of strings")
            }
            (Shape::Launches, []) => SettingWrite::Typed("a table of { kind, preset } tables"),
            (Shape::Launches, [SettingName::Key(_)]) => {
                SettingWrite::Entry(LAUNCH_FIELDS, "an inline table { kind, preset }")
            }
            (Shape::Launches, [SettingName::Key(_), SettingName::Key("kind")]) => {
                SettingWrite::Text("\"Terminal\" or \"Agent\"")
            }
            (Shape::Launches, [SettingName::Key(_), SettingName::Key("preset")]) => {
                SettingWrite::Text("a string")
            }
            _ => return None,
        });
    }
    None
}

/// Top-level rows. `defaults` supplies the default column; pass `settings`
/// itself when nothing in the document was accepted, so the column matches
/// the effective value.
pub fn setting_rows(
    settings: &Settings,
    defaults: &Settings,
    document_keys: &HashSet<String>,
) -> Vec<SettingRow> {
    entries()
        .iter()
        .map(|entry| SettingRow {
            key: entry.path.to_owned(),
            owner: entry.owner,
            value: entry.value(settings),
            source: if document_keys.contains(entry.path) {
                SettingSource::Document
            } else {
                SettingSource::Default
            },
            default: entry.value(defaults),
            off_state: entry
                .off_state
                .filter(|_| (entry.show_off)(settings))
                .map(str::to_owned),
        })
        .collect()
}

fn display(value: &impl Serialize) -> Option<String> {
    toml::Value::try_from(value)
        .ok()
        .map(|value| value.to_string())
}

fn display_choices(choices: &BTreeMap<String, LaunchChoice>) -> Option<String> {
    #[derive(Serialize)]
    #[serde(tag = "kind", content = "preset")]
    enum Spell<'a> {
        Terminal,
        Agent(&'a str),
    }
    let spelled: BTreeMap<&str, Spell> = choices
        .iter()
        .map(|(project, choice)| {
            let spell = match choice {
                LaunchChoice::Terminal => Spell::Terminal,
                LaunchChoice::Agent(preset) => Spell::Agent(preset),
            };
            (project.as_str(), spell)
        })
        .collect();
    display(&spelled)
}

fn value_of(settings: &Settings, path: &str) -> Option<String> {
    let quota = &settings.quota;
    let path_text = |path: &std::path::Path| Some(path.display().to_string());
    match path {
        "desktop_notifications" => display(&settings.desktop_notifications),
        "ready_sound" => display(&settings.ready_sound),
        ReadySoundChoice::KEY => settings
            .ready_sound_choice
            .map(|choice| choice.as_str().into()),
        ThemeId::KEY => Some(settings.theme.as_str().into()),
        "iterm_focus" => display(&settings.iterm_focus),
        "automatic_local_terminals" => Some(settings.automatic_local_terminals.as_str().into()),
        "title_model" => settings.title_model.clone(),
        "branch_prefix" => Some(settings.branch_prefix.clone()),
        "save_uncommitted_work" => display(&settings.save_uncommitted_work),
        "picker_roots" => display(&settings.picker_roots),
        "agents" => display(&settings.agents),
        "launch_choices" => display_choices(&settings.launch_choices),
        "quota.enabled" => display(&quota.enabled),
        "quota.claude.probe" => display(&quota.claude_probe),
        "quota.codex.command" => path_text(&quota.codex.command),
        "quota.codex.home" => quota.codex.home.as_deref().and_then(path_text),
        "quota.grok.command" => path_text(&quota.grok.command),
        "quota.grok.home" => quota.grok.home.as_deref().and_then(path_text),
        "quota.cursor.dashboard" => display(&quota.cursor.dashboard),
        "quota.cursor.state_db" => quota.cursor.state_db.as_deref().and_then(path_text),
        _ => None,
    }
}

const CATALOG: [Entry; 20] = [
    Entry {
        path: "desktop_notifications",
        shape: Shape::Toggle,
        owner: SettingOwner::Dashboard,
        group: "Alerts",
        label: "Desktop notifications",
        off_state: Some(DESKTOP_NOTIFICATIONS_OFF),
        about: NOTIFY_ABOUT,
        value: |settings| value_of(settings, "desktop_notifications"),
        show_off: |settings| !settings.desktop_notifications,
    },
    Entry {
        path: "ready_sound",
        shape: Shape::Toggle,
        owner: SettingOwner::Dashboard,
        group: "Alerts",
        label: "Ready sound",
        off_state: None,
        about: SOUND_ABOUT,
        value: |settings| value_of(settings, "ready_sound"),
        show_off: |_| false,
    },
    Entry {
        path: ReadySoundChoice::KEY,
        shape: Shape::Pick {
            options: &["default", "tap", "chime", "rise"],
            expected: "\"default\", \"tap\", \"chime\" or \"rise\"",
        },
        owner: SettingOwner::Dashboard,
        group: "Alerts",
        label: "Ready sound choice",
        off_state: None,
        about: SOUND_CHOICE_ABOUT,
        value: |settings| value_of(settings, ReadySoundChoice::KEY),
        show_off: |_| false,
    },
    Entry {
        path: "iterm_focus",
        shape: Shape::Toggle,
        owner: SettingOwner::Dashboard,
        group: "Alerts",
        label: "Exact iTerm focus",
        off_state: Some(ITERM_FOCUS_OFF),
        about: ITERM_ABOUT,
        value: |settings| value_of(settings, "iterm_focus"),
        show_off: |settings| !settings.iterm_focus,
    },
    Entry {
        path: ThemeId::KEY,
        shape: Shape::Pick {
            options: ThemeId::KEYS,
            expected: ThemeId::EXPECTED,
        },
        owner: SettingOwner::Dashboard,
        group: "Appearance",
        label: "Theme",
        off_state: None,
        about: THEME_ABOUT,
        value: |settings| value_of(settings, ThemeId::KEY),
        show_off: |_| false,
    },
    Entry {
        path: "automatic_local_terminals",
        shape: Shape::Pick {
            options: &["on", "off", "default_branch_only"],
            expected: "\"on\", \"off\" or \"default_branch_only\"",
        },
        owner: SettingOwner::Server,
        group: "Workspaces",
        label: "Automatic local terminals",
        off_state: None,
        about: &[],
        value: |settings| value_of(settings, "automatic_local_terminals"),
        show_off: |_| false,
    },
    Entry {
        path: "save_uncommitted_work",
        shape: Shape::Toggle,
        owner: SettingOwner::Server,
        group: "Workspaces",
        label: "Save uncommitted work",
        off_state: Some(SAVE_UNCOMMITTED_WORK_OFF),
        about: &[SAVE_UNCOMMITTED_WORK_OFF],
        value: |settings| value_of(settings, "save_uncommitted_work"),
        show_off: |settings| !settings.save_uncommitted_work,
    },
    Entry {
        path: "branch_prefix",
        shape: Shape::Text("a string"),
        owner: SettingOwner::Dashboard,
        group: "Workspaces",
        label: "Branch prefix",
        off_state: None,
        about: &[],
        value: |settings| value_of(settings, "branch_prefix"),
        show_off: |_| false,
    },
    Entry {
        path: "picker_roots",
        shape: Shape::Paths,
        owner: SettingOwner::Dashboard,
        group: "Workspaces",
        label: "Picker roots",
        off_state: None,
        about: &[],
        value: |settings| value_of(settings, "picker_roots"),
        show_off: |_| false,
    },
    Entry {
        path: "title_model",
        shape: Shape::Text("a \"provider/model\" string"),
        owner: SettingOwner::Server,
        group: "Titles",
        label: "Title model",
        off_state: Some(TITLES_OFF),
        about: TITLE_ABOUT,
        value: |settings| value_of(settings, "title_model"),
        show_off: |settings| settings.title_model.is_none(),
    },
    Entry {
        path: "quota.enabled",
        shape: Shape::Toggle,
        owner: SettingOwner::Server,
        group: "Usage",
        label: "Native account usage",
        off_state: Some(QUOTA_OFF),
        about: QUOTA_ABOUT,
        value: |settings| value_of(settings, "quota.enabled"),
        show_off: |settings| !settings.quota.enabled,
    },
    Entry {
        path: "quota.claude.probe",
        shape: Shape::Toggle,
        owner: SettingOwner::Server,
        group: "Usage",
        label: "Claude allowance probe",
        off_state: Some(CLAUDE_PROBE_DESCRIPTION),
        about: &[CLAUDE_PROBE_DESCRIPTION],
        value: |settings| value_of(settings, "quota.claude.probe"),
        show_off: |settings| !settings.quota.claude_probe,
    },
    Entry {
        path: "quota.codex.command",
        shape: Shape::Path("a path"),
        owner: SettingOwner::Server,
        group: "Usage",
        label: "Codex command",
        off_state: None,
        about: &[],
        value: |settings| value_of(settings, "quota.codex.command"),
        show_off: |_| false,
    },
    Entry {
        path: "quota.codex.home",
        shape: Shape::Path("a path"),
        owner: SettingOwner::Server,
        group: "Usage",
        label: "Codex home",
        off_state: None,
        about: &[],
        value: |settings| value_of(settings, "quota.codex.home"),
        show_off: |_| false,
    },
    Entry {
        path: "quota.grok.command",
        shape: Shape::Path("a path"),
        owner: SettingOwner::Server,
        group: "Usage",
        label: "Grok command",
        off_state: None,
        about: &[],
        value: |settings| value_of(settings, "quota.grok.command"),
        show_off: |_| false,
    },
    Entry {
        path: "quota.grok.home",
        shape: Shape::Path("a path"),
        owner: SettingOwner::Server,
        group: "Usage",
        label: "Grok home",
        off_state: None,
        about: &[],
        value: |settings| value_of(settings, "quota.grok.home"),
        show_off: |_| false,
    },
    Entry {
        path: "quota.cursor.dashboard",
        shape: Shape::Toggle,
        owner: SettingOwner::Server,
        group: "Usage",
        label: "Cursor dashboard usage",
        off_state: Some(CURSOR_QUOTA_DESCRIPTION),
        about: &[CURSOR_QUOTA_DESCRIPTION],
        value: |settings| value_of(settings, "quota.cursor.dashboard"),
        show_off: |settings| !settings.quota.cursor.dashboard,
    },
    Entry {
        path: "quota.cursor.state_db",
        shape: Shape::Path("an absolute path"),
        owner: SettingOwner::Server,
        group: "Usage",
        label: "Cursor state database",
        off_state: None,
        about: STATE_DB_ABOUT,
        value: |settings| value_of(settings, "quota.cursor.state_db"),
        show_off: |_| false,
    },
    Entry {
        path: "agents",
        shape: Shape::Agents,
        owner: SettingOwner::Dashboard,
        group: "Agents",
        label: "Agent overrides",
        off_state: None,
        about: &[],
        value: |settings| value_of(settings, "agents"),
        show_off: |_| false,
    },
    Entry {
        path: "launch_choices",
        shape: Shape::Launches,
        owner: SettingOwner::Dashboard,
        group: "Remembered launches",
        label: "Remembered launches",
        off_state: None,
        about: &[],
        value: |settings| value_of(settings, "launch_choices"),
        show_off: |_| false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AutomaticLocalTerminals;

    fn dotted(path: &str) -> Vec<SettingName<'_>> {
        path.split('.').map(SettingName::Key).collect()
    }

    #[test]
    fn catalog_order_owners_and_off_states() {
        let keys: Vec<_> = entries().iter().map(|entry| entry.path).collect();
        assert_eq!(
            keys,
            [
                "desktop_notifications",
                "ready_sound",
                "ready_sound_choice",
                "iterm_focus",
                "theme",
                "automatic_local_terminals",
                "save_uncommitted_work",
                "branch_prefix",
                "picker_roots",
                "title_model",
                "quota.enabled",
                "quota.claude.probe",
                "quota.codex.command",
                "quota.codex.home",
                "quota.grok.command",
                "quota.grok.home",
                "quota.cursor.dashboard",
                "quota.cursor.state_db",
                "agents",
                "launch_choices",
            ]
        );
        let groups: Vec<_> = entries().iter().map(|entry| entry.group).collect();
        let mut headers = groups.clone();
        headers.dedup();
        assert_eq!(
            headers,
            [
                "Alerts",
                "Appearance",
                "Workspaces",
                "Titles",
                "Usage",
                "Agents",
                "Remembered launches"
            ]
        );
        assert!(groups.windows(2).all(|pair| {
            let order = headers.iter().position(|group| *group == pair[0]).unwrap();
            let next = headers.iter().position(|group| *group == pair[1]).unwrap();
            order <= next
        }));
        assert_eq!(
            entries()
                .iter()
                .find(|entry| entry.path == "branch_prefix")
                .unwrap()
                .owner,
            SettingOwner::Dashboard
        );
        assert_eq!(
            entries()
                .iter()
                .find(|entry| entry.path == "title_model")
                .unwrap()
                .owner,
            SettingOwner::Server
        );
        for path in [ReadySoundChoice::KEY, "iterm_focus"] {
            assert_eq!(
                entries()
                    .iter()
                    .find(|entry| entry.path == path)
                    .unwrap()
                    .owner,
                SettingOwner::Dashboard
            );
        }
    }

    #[test]
    fn default_rows_format_values_and_off_states() {
        let settings = Settings::default();
        let rows = setting_rows(&settings, &settings, &HashSet::new());
        assert!(rows.iter().all(|row| row.source == SettingSource::Default));
        assert_eq!(
            rows.iter()
                .find(|row| row.key == "desktop_notifications")
                .unwrap()
                .value
                .as_deref(),
            Some("false")
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.key == ReadySoundChoice::KEY)
                .unwrap()
                .value
                .as_deref(),
            Some("default")
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.key == "iterm_focus")
                .unwrap()
                .value
                .as_deref(),
            Some("false")
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.key == "branch_prefix")
                .unwrap()
                .value
                .as_deref(),
            Some("feature/")
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.key == "automatic_local_terminals")
                .unwrap()
                .value
                .as_deref(),
            Some(AutomaticLocalTerminals::default().as_str())
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.key == "quota.codex.home")
                .unwrap()
                .value,
            None
        );
        let off: Vec<_> = rows
            .iter()
            .filter_map(|row| {
                row.off_state
                    .as_deref()
                    .map(|text| (row.key.as_str(), text))
            })
            .collect();
        assert_eq!(
            off,
            vec![
                ("desktop_notifications", DESKTOP_NOTIFICATIONS_OFF),
                ("iterm_focus", ITERM_FOCUS_OFF),
                ("save_uncommitted_work", SAVE_UNCOMMITTED_WORK_OFF),
                ("title_model", TITLES_OFF),
                ("quota.claude.probe", CLAUDE_PROBE_DESCRIPTION),
                ("quota.cursor.dashboard", CURSOR_QUOTA_DESCRIPTION),
            ]
        );
    }

    #[test]
    fn a_document_key_keeps_the_default_column() {
        let settings = Settings {
            branch_prefix: "kh/".into(),
            quota: crate::QuotaSettings {
                enabled: false,
                ..crate::QuotaSettings::default()
            },
            ..Settings::default()
        };
        let defaults = Settings::default();
        let mut keys = HashSet::new();
        keys.insert("branch_prefix".into());
        keys.insert("quota.enabled".into());
        let rows = setting_rows(&settings, &defaults, &keys);
        let prefix = rows.iter().find(|row| row.key == "branch_prefix").unwrap();
        assert_eq!(prefix.value.as_deref(), Some("kh/"));
        assert_eq!(prefix.default.as_deref(), Some("feature/"));
        assert_eq!(prefix.source, SettingSource::Document);
        let quota = rows.iter().find(|row| row.key == "quota.enabled").unwrap();
        assert_eq!(quota.value.as_deref(), Some("false"));
        assert_eq!(quota.off_state.as_deref(), Some(QUOTA_OFF));
        assert_eq!(quota.source, SettingSource::Document);
        assert_eq!(
            rows.iter()
                .find(|row| row.key == "ready_sound")
                .unwrap()
                .source,
            SettingSource::Default
        );
    }

    #[test]
    fn write_kinds_follow_the_collection_patterns() {
        assert_eq!(
            write_of(&dotted(ReadySoundChoice::KEY)),
            Some(SettingWrite::Text(
                "\"default\", \"tap\", \"chime\" or \"rise\""
            ))
        );
        assert_eq!(
            write_of(&dotted("iterm_focus")),
            Some(SettingWrite::Typed("a boolean"))
        );
        assert_eq!(
            write_of(&dotted("save_uncommitted_work")),
            Some(SettingWrite::Typed("a boolean"))
        );
        assert_eq!(
            write_of(&dotted("title_model")),
            Some(SettingWrite::Text("a \"provider/model\" string"))
        );
        assert_eq!(
            write_of(&dotted("quota.cursor.state_db")),
            Some(SettingWrite::Text("an absolute path"))
        );
        assert_eq!(
            write_of(&[SettingName::Key("picker_roots"), SettingName::Index(2),]),
            Some(SettingWrite::Text("a path"))
        );
        assert_eq!(
            write_of(&[
                SettingName::Key("agents"),
                SettingName::Index(0),
                SettingName::Key("argv"),
            ]),
            Some(SettingWrite::Typed("an array of strings"))
        );
        assert_eq!(element_fields("agents"), Some(&["name", "argv"][..]));
        assert_eq!(
            write_of(&[
                SettingName::Key("launch_choices"),
                SettingName::Key("myproj"),
                SettingName::Key("kind"),
            ]),
            Some(SettingWrite::Text("\"Terminal\" or \"Agent\""))
        );
        assert_eq!(
            element_fields("launch_choices"),
            Some(&["kind", "preset"][..])
        );
        assert_eq!(element_fields("picker_roots"), None);
        assert_eq!(write_of(&dotted("quota.missing")), None);
        assert_eq!(
            write_of(&[
                SettingName::Key("agents"),
                SettingName::Index(0),
                SettingName::Key("extra"),
            ]),
            None
        );
    }

    #[test]
    fn every_entry_has_a_top_level_write_and_a_reader() {
        let settings = Settings {
            title_model: Some("pi/test".into()),
            ..Settings::default()
        };
        for entry in entries() {
            assert!(write_of(&dotted(entry.path)).is_some(), "{}", entry.path);
            match entry.shape {
                Shape::Path(_)
                    if entry.path.ends_with(".home") || entry.path.ends_with("state_db") => {}
                _ => assert!(
                    entry.value(&settings).is_some(),
                    "{} produced no value",
                    entry.path
                ),
            }
        }
    }
}

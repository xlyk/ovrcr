//! The one settings loader (ADR 0007).
//!
//! Reads the settings document and returns every effective value, its source,
//! and the findings. A bad value defaults only its own setting; an unknown key
//! or spelling is a finding; only an unparseable document falls back to all
//! defaults. Nothing here fails: a settings mistake never stops the Server.

use ovrcr_protocol::{
    AgentOverride, AutomaticLocalTerminals, LaunchChoice, NativeCommand, SettingOwner, SettingRow,
    SettingSource, Settings, SettingsFinding, SettingsReport,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Environment variable that chooses the settings document path. It selects
/// a file only; there is no per-setting environment layer.
pub const DOCUMENT_ENV: &str = "OVRCR_DASHBOARD_CONFIG";

pub const QUOTA_OFF: &str = "Codex/Grok usage off: set `quota.enabled = true` in dashboard.toml";
pub const TITLES_OFF: &str =
    "Automatic titles off: set `title_model = \"provider/model\"` in dashboard.toml";
pub const DESKTOP_NOTIFICATIONS_OFF: &str = "Desktop alerts off: set `desktop_notifications = true` in dashboard.toml (needs OS notification permission)";

/// `OVRCR_DASHBOARD_CONFIG`, else `dashboard.toml` beside the instance identity.
pub fn document_path(registry_path: &Path) -> PathBuf {
    document_path_with(registry_path, std::env::var_os(DOCUMENT_ENV))
}

fn document_path_with(registry_path: &Path, env: Option<std::ffi::OsString>) -> PathBuf {
    env.map(PathBuf::from).unwrap_or_else(|| {
        registry_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("dashboard.toml")
    })
}

/// Load the settings document for the instance whose identity is `registry_path`.
pub fn load(registry_path: &Path) -> SettingsReport {
    load_document(registry_path, &document_path(registry_path))
}

/// Load `document`, and report a `[quota]` table left in the instance identity.
pub fn load_document(registry_path: &Path, document: &Path) -> SettingsReport {
    let mut report = match std::fs::read_to_string(document) {
        Ok(text) => parse(document, &text),
        Err(error) => {
            let mut report = parse(document, "");
            if error.kind() != std::io::ErrorKind::NotFound {
                report.findings.push(SettingsFinding {
                    key: None,
                    message: format!("cannot read settings document: {error}"),
                    line: None,
                });
            }
            report
        }
    };
    if let Ok(text) = std::fs::read_to_string(registry_path)
        && let Ok(identity) = toml_edit::Document::parse(text.as_str())
        && let Some((key, item)) = identity.as_table().get_key_value("quota")
    {
        report.findings.push(SettingsFinding {
            key: Some("quota".into()),
            message: format!(
                "quota settings belong in dashboard.toml; the [quota] table in {} configures nothing",
                registry_path.display()
            ),
            line: key.span().or_else(|| item.span()).map(|s| line_at(&text, s.start)),
        });
    }
    report
}

/// Parse document text. `path` is recorded in the report only.
pub fn parse(path: &Path, text: &str) -> SettingsReport {
    let mut loader = Loader {
        text,
        spans: None,
        findings: Vec::new(),
        set: HashSet::new(),
    };
    let mut settings = defaults();
    match toml::from_str::<toml::Table>(text) {
        Ok(mut table) => {
            loader.spans = toml_edit::Document::parse(text).ok();
            loader.read(&mut table, &mut settings);
        }
        Err(error) => loader.findings.push(SettingsFinding {
            key: None,
            message: format!(
                "settings document is not valid TOML, so every setting uses its default: {}",
                error.message()
            ),
            line: error.span().map(|span| line_at(text, span.start)),
        }),
    }
    let rows = rows(&settings, &loader.set);
    SettingsReport {
        path: path.to_path_buf(),
        read_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        settings,
        rows,
        findings: loader.findings,
    }
}

pub fn defaults() -> Settings {
    Settings {
        picker_roots: ["~/Code", "~/src", "~"]
            .into_iter()
            .map(expand_tilde)
            .filter(|path| path.exists())
            .collect(),
        ..Settings::default()
    }
}

struct Loader<'a> {
    text: &'a str,
    spans: Option<toml_edit::Document<&'a str>>,
    findings: Vec<SettingsFinding>,
    /// Dotted keys whose effective value came from the document.
    set: HashSet<String>,
}

impl Loader<'_> {
    fn read(&mut self, table: &mut toml::Table, settings: &mut Settings) {
        self.value(
            table,
            "desktop_notifications",
            &mut settings.desktop_notifications,
        );
        self.value(table, "ready_sound", &mut settings.ready_sound);
        if let Some(raw) = self.take::<String>(table, AutomaticLocalTerminals::KEY) {
            match AutomaticLocalTerminals::parse(&raw) {
                Some(policy) => self.accept(
                    AutomaticLocalTerminals::KEY,
                    &mut settings.automatic_local_terminals,
                    policy,
                ),
                None => self.finding(
                    AutomaticLocalTerminals::KEY,
                    format!(
                        "unknown value {raw:?}; expected \"on\", \"off\" or \"default_branch_only\""
                    ),
                ),
            }
        }
        if let Some(raw) = self.take::<String>(table, "title_model") {
            match raw.split_once('/') {
                Some((provider, model))
                    if !provider.trim().is_empty() && !model.trim().is_empty() =>
                {
                    let model = format!("{}/{}", provider.trim(), model.trim());
                    self.accept("title_model", &mut settings.title_model, Some(model));
                }
                _ => self.finding("title_model", format!("{raw:?} is not provider/model")),
            }
        }
        self.value(table, "branch_prefix", &mut settings.branch_prefix);
        if let Some(roots) = self.take::<Vec<String>>(table, "picker_roots") {
            let roots = roots.iter().map(|root| expand_tilde(root)).collect();
            self.accept("picker_roots", &mut settings.picker_roots, roots);
        }
        self.value::<Vec<AgentOverride>>(table, "agents", &mut settings.agents);
        if let Some(choices) =
            self.take::<BTreeMap<String, DocumentLaunchChoice>>(table, "launch_choices")
        {
            let choices = choices
                .into_iter()
                .map(|(project, choice)| (project, choice.into()))
                .collect();
            self.accept("launch_choices", &mut settings.launch_choices, choices);
        }
        if let Some(mut quota) = self.take::<toml::Table>(table, "quota") {
            self.value(&mut quota, "quota.enabled", &mut settings.quota.enabled);
            self.native(&mut quota, "quota.codex", &mut settings.quota.codex);
            self.native(&mut quota, "quota.grok", &mut settings.quota.grok);
            self.unknown(quota, "quota.");
        }
        self.unknown(std::mem::take(table), "");
    }

    fn native(&mut self, table: &mut toml::Table, key: &str, native: &mut NativeCommand) {
        if let Some(mut command) = self.take::<toml::Table>(table, key) {
            self.value(&mut command, &format!("{key}.command"), &mut native.command);
            if let Some(home) = self.take::<PathBuf>(&mut command, &format!("{key}.home")) {
                self.accept(&format!("{key}.home"), &mut native.home, Some(home));
            }
            self.unknown(command, &format!("{key}."));
        }
    }

    /// Read `key` into `slot`, or keep the default and record a finding.
    fn value<T: DeserializeOwned>(&mut self, table: &mut toml::Table, key: &str, slot: &mut T) {
        if let Some(value) = self.take(table, key) {
            self.accept(key, slot, value);
        }
    }

    fn accept<T>(&mut self, key: &str, slot: &mut T, value: T) {
        *slot = value;
        self.set.insert(key.to_owned());
    }

    /// Remove the last segment of dotted `key` from `table` and convert it.
    fn take<T: DeserializeOwned>(&mut self, table: &mut toml::Table, key: &str) -> Option<T> {
        let name = key.rsplit('.').next().unwrap_or(key);
        let value = table.remove(name)?;
        match value.try_into() {
            Ok(value) => Some(value),
            Err(error) => {
                let error: toml::de::Error = error;
                self.finding(key, error.message().to_owned());
                None
            }
        }
    }

    fn unknown(&mut self, table: toml::Table, prefix: &str) {
        for name in table.keys() {
            self.finding(
                &format!("{prefix}{name}"),
                "unknown setting; ignored".into(),
            );
        }
    }

    fn finding(&mut self, key: &str, message: String) {
        let line = self.line(key);
        self.findings.push(SettingsFinding {
            key: Some(key.to_owned()),
            message,
            line,
        });
    }

    fn line(&self, key: &str) -> Option<u32> {
        let mut table: &dyn toml_edit::TableLike = self.spans.as_ref()?.as_table();
        let mut segments = key.split('.').peekable();
        while let Some(segment) = segments.next() {
            let (key, item) = table.get_key_value(segment)?;
            if segments.peek().is_none() {
                let span = key.span().or_else(|| item.span())?;
                return Some(line_at(self.text, span.start));
            }
            table = item.as_table_like()?;
        }
        None
    }
}

/// The document's spelling of a launch choice: `kind = "Agent"`, `preset = "…"`.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", content = "preset", deny_unknown_fields)]
enum DocumentLaunchChoice {
    Terminal,
    Agent(String),
}

impl From<DocumentLaunchChoice> for LaunchChoice {
    fn from(choice: DocumentLaunchChoice) -> Self {
        match choice {
            DocumentLaunchChoice::Terminal => Self::Terminal,
            DocumentLaunchChoice::Agent(preset) => Self::Agent(preset),
        }
    }
}

impl From<&LaunchChoice> for DocumentLaunchChoice {
    fn from(choice: &LaunchChoice) -> Self {
        match choice {
            LaunchChoice::Terminal => Self::Terminal,
            LaunchChoice::Agent(preset) => Self::Agent(preset.clone()),
        }
    }
}

fn line_at(text: &str, offset: usize) -> u32 {
    text[..offset.min(text.len())].matches('\n').count() as u32 + 1
}

fn expand_tilde(path: &str) -> PathBuf {
    let home = || {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"))
    };
    if path == "~" {
        home()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(path)
    }
}

/// Rows in the order `docs/dashboard.md` documents the settings.
fn rows(settings: &Settings, set: &HashSet<String>) -> Vec<SettingRow> {
    use SettingOwner::{Dashboard, Server};
    let quota = &settings.quota;
    let path = |path: &Path| Some(path.display().to_string());
    let mut rows = vec![
        row(
            "desktop_notifications",
            Dashboard,
            display(&settings.desktop_notifications),
            (!settings.desktop_notifications).then_some(DESKTOP_NOTIFICATIONS_OFF),
        ),
        row(
            "ready_sound",
            Dashboard,
            display(&settings.ready_sound),
            None,
        ),
        row(
            AutomaticLocalTerminals::KEY,
            Server,
            Some(settings.automatic_local_terminals.as_str().into()),
            None,
        ),
        row(
            "title_model",
            Server,
            settings.title_model.clone(),
            settings.title_model.is_none().then_some(TITLES_OFF),
        ),
        row(
            "branch_prefix",
            Dashboard,
            Some(settings.branch_prefix.clone()),
            None,
        ),
        row(
            "picker_roots",
            Dashboard,
            display(&settings.picker_roots),
            None,
        ),
        row("agents", Dashboard, display(&settings.agents), None),
        row(
            "launch_choices",
            Dashboard,
            display(
                &settings
                    .launch_choices
                    .iter()
                    .map(|(project, choice)| (project, DocumentLaunchChoice::from(choice)))
                    .collect::<BTreeMap<_, _>>(),
            ),
            None,
        ),
        row(
            "quota.enabled",
            Server,
            display(&quota.enabled),
            (!quota.enabled).then_some(QUOTA_OFF),
        ),
        row(
            "quota.codex.command",
            Server,
            path(&quota.codex.command),
            None,
        ),
        row(
            "quota.codex.home",
            Server,
            quota.codex.home.as_deref().and_then(path),
            None,
        ),
        row(
            "quota.grok.command",
            Server,
            path(&quota.grok.command),
            None,
        ),
        row(
            "quota.grok.home",
            Server,
            quota.grok.home.as_deref().and_then(path),
            None,
        ),
    ];
    for row in &mut rows {
        if set.contains(&row.key) {
            row.source = SettingSource::Document;
        }
    }
    rows
}

fn row(
    key: &str,
    owner: SettingOwner,
    value: Option<String>,
    off_state: Option<&str>,
) -> SettingRow {
    SettingRow {
        key: key.into(),
        owner,
        value,
        source: SettingSource::Default,
        off_state: off_state.map(str::to_owned),
    }
}

/// TOML text for a non-string value.
fn display(value: &impl Serialize) -> Option<String> {
    toml::Value::try_from(value)
        .ok()
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> SettingsReport {
        parse(Path::new("dashboard.toml"), text)
    }

    fn row<'a>(report: &'a SettingsReport, key: &str) -> &'a SettingRow {
        report.rows.iter().find(|row| row.key == key).unwrap()
    }

    fn finding(key: &str, line: u32, contains: &str, report: &SettingsReport) {
        let found = &report.findings;
        assert!(
            found.iter().any(|f| f.key.as_deref() == Some(key)
                && f.line == Some(line)
                && f.message.contains(contains)),
            "no finding for {key} at line {line} containing {contains:?}: {found:?}"
        );
    }

    const FULL: &str = r#"desktop_notifications = true
ready_sound = true
automatic_local_terminals = "off"
title_model = "pi/test"
branch_prefix = "kh/"
picker_roots = ["/tmp/a", "~/b"]
agents = [{ name = "claude", argv = ["claude", "--x"] }]

[launch_choices.ovrcr]
kind = "Agent"
preset = "claude"

[launch_choices.notes]
kind = "Terminal"

[quota]
enabled = true

[quota.codex]
command = "/opt/codex"
home = "/tmp/codex-home"

[quota.grok]
command = "/opt/grok"
"#;

    #[test]
    fn full_document_sets_every_setting_with_document_source() {
        let report = read(FULL);
        assert_eq!(report.findings, vec![]);
        let settings = &report.settings;
        assert!(settings.desktop_notifications && settings.ready_sound);
        assert_eq!(
            settings.automatic_local_terminals,
            AutomaticLocalTerminals::Off
        );
        assert_eq!(settings.title_model.as_deref(), Some("pi/test"));
        assert_eq!(settings.branch_prefix, "kh/");
        assert_eq!(settings.picker_roots[0], PathBuf::from("/tmp/a"));
        assert!(!settings.picker_roots[1].starts_with("~"));
        assert_eq!(
            settings.agents,
            vec![AgentOverride {
                name: "claude".into(),
                argv: vec!["claude".into(), "--x".into()]
            }]
        );
        assert_eq!(
            settings.launch_choices["ovrcr"],
            LaunchChoice::Agent("claude".into())
        );
        assert_eq!(settings.launch_choices["notes"], LaunchChoice::Terminal);
        assert!(settings.quota.enabled);
        assert_eq!(settings.quota.codex.command, PathBuf::from("/opt/codex"));
        assert_eq!(
            settings.quota.codex.home,
            Some(PathBuf::from("/tmp/codex-home"))
        );
        assert_eq!(settings.quota.grok.command, PathBuf::from("/opt/grok"));
        assert_eq!(settings.quota.grok.home, None);
        for row in &report.rows {
            let expected = if row.key == "quota.grok.home" {
                SettingSource::Default
            } else {
                SettingSource::Document
            };
            assert_eq!(row.source, expected, "{}", row.key);
            assert_eq!(row.off_state, None, "{}", row.key);
        }
        assert_eq!(row(&report, "ready_sound").value.as_deref(), Some("true"));
        assert_eq!(
            row(&report, "title_model").value.as_deref(),
            Some("pi/test")
        );
        assert_eq!(row(&report, "quota.grok.home").value, None);
    }

    #[test]
    fn empty_document_is_all_defaults_and_consent_settings_say_how_to_turn_on() {
        let report = read("");
        assert_eq!(report.findings, vec![]);
        assert_eq!(report.settings, defaults());
        assert!(
            report
                .rows
                .iter()
                .all(|row| row.source == SettingSource::Default)
        );
        let off: Vec<_> = report
            .rows
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
                ("title_model", TITLES_OFF),
                ("quota.enabled", QUOTA_OFF),
            ]
        );
        let keys: Vec<_> = report.rows.iter().map(|row| row.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "desktop_notifications",
                "ready_sound",
                "automatic_local_terminals",
                "title_model",
                "branch_prefix",
                "picker_roots",
                "agents",
                "launch_choices",
                "quota.enabled",
                "quota.codex.command",
                "quota.codex.home",
                "quota.grok.command",
                "quota.grok.home",
            ]
        );
    }

    #[test]
    fn one_wrong_type_defaults_only_that_setting_with_one_finding() {
        let text = FULL.replace("ready_sound = true", "ready_sound = \"yes\"");
        let report = read(&text);
        let mut expected = read(FULL).settings;
        expected.ready_sound = false;
        assert_eq!(report.settings, expected);
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        finding("ready_sound", 2, "", &report);
        assert_eq!(row(&report, "ready_sound").source, SettingSource::Default);
        assert_eq!(
            row(&report, "branch_prefix").source,
            SettingSource::Document
        );
    }

    #[test]
    fn nested_wrong_type_reports_dotted_key_and_line() {
        let text = FULL.replace("command = \"/opt/codex\"", "command = 7");
        let report = read(&text);
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        finding("quota.codex.command", 20, "expected path", &report);
        assert_eq!(report.settings.quota.codex.command, PathBuf::from("codex"));
        assert_eq!(
            report.settings.quota.codex.home,
            Some(PathBuf::from("/tmp/codex-home"))
        );
        assert!(report.settings.quota.enabled);
    }

    #[test]
    fn unknown_keys_are_findings_and_ignored() {
        let report =
            read("ready_sund = true\n[quota]\nenabeld = true\n[quota.codex]\nhom = '/x'\n");
        assert_eq!(report.settings, defaults());
        assert_eq!(report.findings.len(), 3, "{:?}", report.findings);
        finding("ready_sund", 1, "unknown setting", &report);
        finding("quota.enabeld", 3, "unknown setting", &report);
        finding("quota.codex.hom", 5, "unknown setting", &report);
    }

    #[test]
    fn unknown_enum_spelling_is_a_finding_not_a_silent_default() {
        let report = read("ready_sound = true\nautomatic_local_terminals = \"always\"\n");
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        finding(AutomaticLocalTerminals::KEY, 2, "\"always\"", &report);
        assert_eq!(
            report.settings.automatic_local_terminals,
            AutomaticLocalTerminals::DefaultBranchOnly
        );
        assert!(report.settings.ready_sound);

        let report = read("[launch_choices.p]\nkind = \"Shell\"\n");
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        finding("launch_choices", 1, "Shell", &report);
    }

    #[test]
    fn title_model_must_be_provider_slash_model() {
        let report = read("title_model = \"pi\"\n");
        finding("title_model", 1, "provider/model", &report);
        assert_eq!(report.settings.title_model, None);
        assert_eq!(
            row(&report, "title_model").off_state.as_deref(),
            Some(TITLES_OFF)
        );
    }

    #[test]
    fn unparseable_document_is_all_defaults_with_one_document_finding() {
        let report = read("ready_sound = true\nnot = toml {\n");
        assert_eq!(report.settings, defaults());
        assert_eq!(report.findings.len(), 1);
        let finding = &report.findings[0];
        assert_eq!(finding.key, None);
        assert_eq!(finding.line, Some(2));
        assert!(finding.message.contains("not valid TOML"), "{finding:?}");
    }

    #[test]
    fn quota_in_instance_identity_is_a_finding_and_configures_nothing() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        std::fs::write(&config, "projects = []\n\n[quota]\nenabled = true\n").unwrap();
        let report = load_document(&config, &root.path().join("dashboard.toml"));
        assert!(!report.settings.quota.enabled);
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        finding(
            "quota",
            3,
            "quota settings belong in dashboard.toml",
            &report,
        );
    }

    #[test]
    fn unreadable_document_is_a_document_finding() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        // A directory cannot be read as a file.
        let report = load_document(&config, root.path());
        assert_eq!(report.settings, defaults());
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].key, None);
        // A missing document is the normal first run: no finding.
        let report = load_document(&config, &root.path().join("dashboard.toml"));
        assert_eq!(report.findings, vec![]);
    }

    #[test]
    fn env_chooses_the_document_path_only() {
        let config = Path::new("/inst/config.toml");
        assert_eq!(
            document_path_with(config, None),
            PathBuf::from("/inst/dashboard.toml")
        );
        assert_eq!(
            document_path_with(config, Some("/elsewhere/custom.toml".into())),
            PathBuf::from("/elsewhere/custom.toml")
        );
    }

    #[test]
    fn report_round_trips_through_json() {
        let text = FULL.replace("ready_sound = true", "ready_sound = 1\nmystery = 2");
        let report = read(&text);
        assert_eq!(report.findings.len(), 2);
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            serde_json::from_str::<SettingsReport>(&json).unwrap(),
            report
        );
    }
}

//! The one settings loader (ADR 0007).
//!
//! Reads the settings document and returns every effective value, its source,
//! and the findings. A bad value defaults only its own setting; a wrong-typed
//! element of a list or table drops only that element; an unknown key or
//! spelling is a finding; only an unparseable document falls back to all
//! defaults. An explicitly invalid sound choice instead suppresses sound.
//! Nothing here fails: a settings mistake never stops the Server.

use ovrcr_protocol::{
    AgentOverride, AutomaticLocalTerminals, LaunchChoice, NativeCommand, ReadySoundChoice,
    SettingOwner, SettingRow, SettingSource, Settings, SettingsFinding, SettingsReport,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod write;
pub use write::set;

/// Environment variable that chooses the settings document path. It selects
/// a file only; there is no per-setting environment layer.
pub const DOCUMENT_ENV: &str = "OVRCR_DASHBOARD_CONFIG";

pub use ovrcr_protocol::QUOTA_OFF;
pub const TITLES_OFF: &str =
    "Automatic titles off: set `title_model = \"provider/model\"` in dashboard.toml";
pub const ITERM_FOCUS_OFF: &str = "Exact iTerm focus off: set `iterm_focus = true`, then choose Set up iTerm focus in the Dashboard palette (separate Automation permission)";
pub const DESKTOP_NOTIFICATIONS_OFF: &str = "Desktop alerts off: set `desktop_notifications = true` in dashboard.toml (needs OS notification permission)";

/// `OVRCR_DASHBOARD_CONFIG`, else `dashboard.toml` in the instance directory.
pub fn document_path(home: &Path) -> PathBuf {
    document_path_with(home, std::env::var_os(DOCUMENT_ENV))
}

fn document_path_with(home: &Path, env: Option<std::ffi::OsString>) -> PathBuf {
    env.map(PathBuf::from)
        .unwrap_or_else(|| crate::config::settings_document_path(home))
}

/// Load the settings document for the instance whose identity is `home`.
pub fn load(home: &Path) -> SettingsReport {
    load_document(home, &document_path(home))
}

/// Load `document`, and report a `[quota]` table left in the preserved identity file.
pub fn load_document(home: &Path, document: &Path) -> SettingsReport {
    let mut report = match std::fs::read_to_string(document) {
        Ok(text) => parse(document, &text),
        Err(error) => {
            let mut report = parse(document, "");
            // A missing file is a fresh install. Reading a symlink whose
            // target does not exist is NotFound too, but that path is a
            // document the user pointed at, so it is a finding rather than silence.
            if error.kind() == std::io::ErrorKind::NotFound && is_symlink(document) {
                report.findings.push(SettingsFinding {
                    key: None,
                    message: format!(
                        "settings document {} is a dangling symlink",
                        document.display()
                    ),
                    line: None,
                });
            } else if error.kind() != std::io::ErrorKind::NotFound {
                report.findings.push(SettingsFinding {
                    key: None,
                    message: format!("cannot read settings document: {error}"),
                    line: None,
                });
            }
            report
        }
    };
    let identity_path = crate::config::legacy_identity_path(home);
    if let Ok(text) = std::fs::read_to_string(&identity_path)
        && let Ok(identity) = toml_edit::Document::parse(text.as_str())
        && let Some((key, item)) = identity.as_table().get_key_value("quota")
    {
        report.findings.push(SettingsFinding {
            key: Some("quota".into()),
            message: format!(
                "quota settings belong in dashboard.toml; the [quota] table in {} configures nothing",
                identity_path.display()
            ),
            line: key.span().or_else(|| item.span()).map(|s| line_at(&text, s.start)),
        });
    }
    if std::env::var_os(crate::config::HOME_ENV).is_none()
        && let Some(alias) = std::env::var_os(crate::config::CONFIG_ENV)
    {
        let alias = PathBuf::from(alias);
        report.findings.push(SettingsFinding {
            key: None,
            message: crate::config::config_alias_finding(&alias),
            line: None,
        });
    }
    report
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
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
    let parsed = toml::from_str::<toml::Table>(text);
    let unparseable = parsed.is_err();
    match parsed {
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
        unparseable,
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
        if table.contains_key(ReadySoundChoice::KEY) {
            self.accept(
                ReadySoundChoice::KEY,
                &mut settings.ready_sound_choice,
                None,
            );
            if let Some(raw) = self.take::<String>(table, ReadySoundChoice::KEY) {
                match ReadySoundChoice::parse(&raw) {
                    Some(choice) => self.accept(
                        ReadySoundChoice::KEY,
                        &mut settings.ready_sound_choice,
                        Some(choice),
                    ),
                    None => self.finding(
                        ReadySoundChoice::KEY,
                        format!("unknown value {raw:?}; expected \"default\", \"tap\", \"chime\" or \"rise\""),
                    ),
                }
            }
        }
        self.value(table, "iterm_focus", &mut settings.iterm_focus);
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
        self.value(
            table,
            "save_uncommitted_work",
            &mut settings.save_uncommitted_work,
        );
        if let Some(roots) = self.take_array::<String>(table, "picker_roots", "a string") {
            let roots = roots.iter().map(|root| expand_tilde(root)).collect();
            self.accept("picker_roots", &mut settings.picker_roots, roots);
        }
        self.drop_unknown_fields(table, "agents", &["name", "argv"]);
        if let Some(agents) =
            self.take_array::<AgentOverride>(table, "agents", "a table { name, argv }")
        {
            self.accept("agents", &mut settings.agents, agents);
        }
        self.drop_unknown_fields(table, "launch_choices", &["kind", "preset"]);
        if let Some(choices) = self.take_table::<DocumentLaunchChoice>(
            table,
            "launch_choices",
            "a table { kind, preset }",
        ) {
            let choices = choices
                .into_iter()
                .map(|(project, choice)| (project, choice.into()))
                .collect();
            self.accept("launch_choices", &mut settings.launch_choices, choices);
        }
        if let Some(mut quota) = self.take::<toml::Table>(table, "quota") {
            self.value(&mut quota, "quota.enabled", &mut settings.quota.enabled);
            if let Some(mut claude) = self.take::<toml::Table>(&mut quota, "quota.claude") {
                self.value(
                    &mut claude,
                    "quota.claude.probe",
                    &mut settings.quota.claude_probe,
                );
                self.unknown(claude, "quota.claude.");
            }
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

    /// Read array setting `key` element by element. A wrong-typed element is
    /// dropped with a finding naming `key[index]`; siblings stay. Any other
    /// element error still rejects the whole collection, one finding on `key`.
    fn take_array<T: DeserializeOwned>(
        &mut self,
        table: &mut toml::Table,
        key: &str,
        expected: &str,
    ) -> Option<Vec<T>> {
        let value = remove(table, key)?;
        let toml::Value::Array(items) = value.clone() else {
            self.finding(key, deserialize_message::<Vec<T>>(value));
            return None;
        };
        self.collect_array(key, value, items, expected)
    }

    /// Read table setting `key` entry by entry. A wrong-typed entry is dropped
    /// with a finding naming `key.name`; siblings stay. Any other entry error
    /// still rejects the whole table, one finding on `key`.
    fn take_table<T: DeserializeOwned>(
        &mut self,
        table: &mut toml::Table,
        key: &str,
        expected: &str,
    ) -> Option<BTreeMap<String, T>> {
        let value = remove(table, key)?;
        let toml::Value::Table(entries) = value.clone() else {
            self.finding(key, deserialize_message::<BTreeMap<String, T>>(value));
            return None;
        };
        self.collect_table(key, value, entries, expected)
    }

    fn collect_array<T: DeserializeOwned>(
        &mut self,
        key: &str,
        original: toml::Value,
        items: Vec<toml::Value>,
        expected: &str,
    ) -> Option<Vec<T>> {
        let mut kept = Vec::new();
        let mut wrong = Vec::new();
        for (index, item) in items.iter().enumerate() {
            match item.clone().try_into::<T>() {
                Ok(value) => kept.push(value),
                Err(error) => {
                    let message = de_message(error);
                    if is_wrong_type(&message) {
                        wrong.push((index, element_message(message, expected)));
                    } else {
                        self.finding(key, deserialize_message::<Vec<T>>(original));
                        return None;
                    }
                }
            }
        }
        for (index, message) in wrong {
            self.finding_at(
                format!("{key}[{index}]"),
                self.line_at_index(key, index),
                message,
            );
        }
        (!kept.is_empty() || items.is_empty()).then_some(kept)
    }

    fn collect_table<T: DeserializeOwned>(
        &mut self,
        key: &str,
        original: toml::Value,
        entries: toml::Table,
        expected: &str,
    ) -> Option<BTreeMap<String, T>> {
        let mut kept = BTreeMap::new();
        let mut wrong = Vec::new();
        for (name, item) in &entries {
            match item.clone().try_into::<T>() {
                Ok(value) => {
                    kept.insert(name.clone(), value);
                }
                Err(error) => {
                    let message = de_message(error);
                    if is_wrong_type(&message) {
                        wrong.push((name.clone(), element_message(message, expected)));
                    } else {
                        self.finding(key, deserialize_message::<BTreeMap<String, T>>(original));
                        return None;
                    }
                }
            }
        }
        for (name, message) in &wrong {
            let display = format!("{key}.{name}");
            let line = self.line(&display);
            self.finding_at(display, line, message.clone());
        }
        (!kept.is_empty() || entries.is_empty()).then_some(kept)
    }

    fn finding_at(&mut self, key: String, line: Option<u32>, message: String) {
        self.findings.push(SettingsFinding {
            key: Some(key),
            message,
            line,
        });
    }

    fn line_at_index(&self, key: &str, index: usize) -> Option<u32> {
        let mut path: Vec<_> = key
            .split('.')
            .map(|name| Segment::Key(name.into()))
            .collect();
        path.push(Segment::Index(index));
        self.line_path(&path)
    }

    /// Remove fields an element of a list or table setting `key` does not
    /// define, each as its own finding, so one unknown field never resets the
    /// whole collection.
    fn drop_unknown_fields(&mut self, table: &mut toml::Table, key: &str, known: &[&str]) {
        let mut elements: Vec<(String, Segment, &mut toml::Table)> = Vec::new();
        match table.get_mut(key) {
            Some(toml::Value::Array(rows)) => {
                for (index, row) in rows.iter_mut().enumerate() {
                    if let toml::Value::Table(fields) = row {
                        elements.push((format!("{key}[{index}]"), Segment::Index(index), fields));
                    }
                }
            }
            Some(toml::Value::Table(entries)) => {
                for (name, entry) in entries.iter_mut() {
                    if let toml::Value::Table(fields) = entry {
                        elements.push((
                            format!("{key}.{name}"),
                            Segment::Key(name.clone()),
                            fields,
                        ));
                    }
                }
            }
            _ => return,
        }
        let mut unknown = Vec::new();
        for (display, element, fields) in elements {
            fields.retain(|field, _| {
                let keep = known.contains(&field);
                if !keep {
                    unknown.push((
                        format!("{display}.{field}"),
                        vec![
                            Segment::Key(key.to_owned()),
                            element.clone(),
                            Segment::Key(field.to_owned()),
                        ],
                    ));
                }
                keep
            });
        }
        for (display, path) in unknown {
            let line = self.line_path(&path);
            self.findings.push(SettingsFinding {
                key: Some(display),
                message: "unknown setting; ignored".into(),
                line,
            });
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
        let path: Vec<_> = key
            .split('.')
            .map(|name| Segment::Key(name.into()))
            .collect();
        self.line_path(&path)
    }

    /// Line of the last key on `path`, through tables, `[[array]]` tables and
    /// inline arrays of inline tables.
    fn line_path(&self, path: &[Segment]) -> Option<u32> {
        let mut table: &dyn toml_edit::TableLike = self.spans.as_ref()?.as_table();
        let mut item: Option<&toml_edit::Item> = None;
        let mut span = None;
        for (at, segment) in path.iter().enumerate() {
            match segment {
                Segment::Key(name) => {
                    if let Some(item) = item {
                        table = item.as_table_like()?;
                    }
                    let (key, next) = table.get_key_value(name)?;
                    span = key.span().or_else(|| next.span());
                    item = Some(next);
                }
                Segment::Index(index) => {
                    let array = item.take()?;
                    if let Some(tables) = array.as_array_of_tables() {
                        let element = tables.get(*index)?;
                        span = element.span().or(span);
                        table = element as &dyn toml_edit::TableLike;
                    } else {
                        let element = array.as_array()?.get(*index)?;
                        span = element.span().or(span);
                        if let Some(inline) = element.as_inline_table() {
                            table = inline;
                        } else if at + 1 != path.len() {
                            return None;
                        }
                    }
                }
            }
        }
        span.map(|span| line_at(self.text, span.start))
    }
}

#[derive(Clone)]
enum Segment {
    Key(String),
    Index(usize),
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

fn remove(table: &mut toml::Table, key: &str) -> Option<toml::Value> {
    let name = key.rsplit('.').next().unwrap_or(key);
    table.remove(name)
}

fn de_message(error: toml::de::Error) -> String {
    error.message().to_owned()
}

fn deserialize_message<T: DeserializeOwned>(value: toml::Value) -> String {
    match value.try_into::<T>() {
        Ok(_) => "invalid value".into(),
        Err(error) => de_message(error),
    }
}

/// Serde's type-mismatch wording, as opposed to a missing field or an unknown
/// spelling, which still reject the whole collection.
fn is_wrong_type(message: &str) -> bool {
    message.starts_with("invalid type:")
}

/// Serde names the Rust type (`struct AgentOverride`, `enum DocumentLaunchChoice`).
/// The finding should name the document shape instead.
fn element_message(message: String, expected: &str) -> String {
    if (message.contains("struct ") || message.contains("enum "))
        && let Some((head, _)) = message.split_once(", expected ")
    {
        return format!("{head}, expected {expected}");
    }
    message
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
            ReadySoundChoice::KEY,
            Dashboard,
            settings
                .ready_sound_choice
                .map(|choice| choice.as_str().into()),
            None,
        ),
        row(
            "iterm_focus",
            Dashboard,
            display(&settings.iterm_focus),
            (!settings.iterm_focus).then_some(ITERM_FOCUS_OFF),
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
            "save_uncommitted_work",
            Server,
            display(&settings.save_uncommitted_work),
            (!settings.save_uncommitted_work).then_some(ovrcr_protocol::SAVE_UNCOMMITTED_WORK_OFF),
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
            "quota.claude.probe",
            Server,
            display(&quota.claude_probe),
            (!quota.claude_probe).then_some(ovrcr_protocol::CLAUDE_PROBE_DESCRIPTION),
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
    let defaults = if set.is_empty() {
        None
    } else {
        Some(self::rows(&defaults(), &HashSet::new()))
    };
    for (index, row) in rows.iter_mut().enumerate() {
        row.default = match &defaults {
            Some(defaults) => defaults[index].value.clone(),
            None => row.value.clone(),
        };
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
        default: None,
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

    #[test]
    fn ready_sound_choice_defaults_or_fails_closed_without_changing_opt_ins() {
        for notifications in [false, true] {
            for sound in [false, true] {
                for (raw, choice) in [
                    (None, Some(ReadySoundChoice::Default)),
                    (Some("\"default\""), Some(ReadySoundChoice::Default)),
                    (Some("\"tap\""), Some(ReadySoundChoice::Tap)),
                    (Some("\"chime\""), Some(ReadySoundChoice::Chime)),
                    (Some("\"rise\""), Some(ReadySoundChoice::Rise)),
                    (Some("\"Glass\""), None),
                    (Some("\" tap\""), None),
                    (Some("true"), None),
                    (Some("7"), None),
                    (Some("[\"tap\"]"), None),
                    (Some("{ tap = {} }"), None),
                ] {
                    let mut text = format!(
                        "desktop_notifications = {notifications}\nready_sound = {sound}\nbranch_prefix = 'preserved/'\n"
                    );
                    if let Some(raw) = raw {
                        text.push_str(&format!("ready_sound_choice = {raw}\n"));
                    }
                    let report = read(&text);
                    assert_eq!(report.settings.ready_sound_choice, choice, "{text}");
                    assert_eq!(report.settings.desktop_notifications, notifications);
                    assert_eq!(report.settings.ready_sound, sound);
                    assert_eq!(report.settings.branch_prefix, "preserved/");
                    let row = row(&report, ReadySoundChoice::KEY);
                    assert_eq!(row.value.as_deref(), choice.map(ReadySoundChoice::as_str));
                    assert_eq!(row.default.as_deref(), Some("default"));
                    assert_eq!(
                        row.source,
                        if raw.is_some() {
                            SettingSource::Document
                        } else {
                            SettingSource::Default
                        }
                    );
                    if choice.is_some() {
                        assert!(report.findings.is_empty(), "{:?}", report.findings);
                    } else {
                        assert!(matches!(report.findings.as_slice(), [finding]
                            if finding.key.as_deref() == Some(ReadySoundChoice::KEY)
                                && finding.line == Some(4)));
                    }
                }
            }
        }
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
iterm_focus = true
save_uncommitted_work = true
automatic_local_terminals = "off"
title_model = "pi/test"
branch_prefix = "kh/"
picker_roots = ["/tmp/a", "~/b"]
agents = [{ name = "claude", argv = ["claude", "--x"] }]
ready_sound_choice = "chime"

[launch_choices.ovrcr]
kind = "Agent"
preset = "claude"

[launch_choices.notes]
kind = "Terminal"

[quota]
enabled = true

[quota.claude]
probe = true

[quota.codex]
command = "/opt/codex"
home = "/tmp/codex-home"

[quota.grok]
command = "/opt/grok"
"#;

    #[test]
    fn save_uncommitted_work_defaults_off() {
        let report = read("");
        let row = row(&report, "save_uncommitted_work");
        assert_eq!(row.value.as_deref(), Some("false"));
        assert_eq!(row.source, SettingSource::Default);
        assert_eq!(row.owner, SettingOwner::Server);
        assert_eq!(
            row.off_state.as_deref(),
            Some(ovrcr_protocol::SAVE_UNCOMMITTED_WORK_OFF)
        );
    }

    #[test]
    fn full_document_sets_every_setting_with_document_source() {
        let report = read(FULL);
        assert_eq!(report.findings, vec![]);
        let settings = &report.settings;
        assert!(settings.desktop_notifications && settings.ready_sound);
        assert_eq!(settings.ready_sound_choice, Some(ReadySoundChoice::Chime));
        assert!(settings.iterm_focus);
        assert!(settings.save_uncommitted_work);
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
        assert!(settings.quota.claude_probe);
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
                ("iterm_focus", ITERM_FOCUS_OFF),
                ("title_model", TITLES_OFF),
                (
                    "save_uncommitted_work",
                    ovrcr_protocol::SAVE_UNCOMMITTED_WORK_OFF
                ),
                (
                    "quota.claude.probe",
                    ovrcr_protocol::CLAUDE_PROBE_DESCRIPTION
                ),
            ]
        );
        let keys: Vec<_> = report.rows.iter().map(|row| row.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "desktop_notifications",
                "ready_sound",
                "ready_sound_choice",
                "iterm_focus",
                "automatic_local_terminals",
                "title_model",
                "branch_prefix",
                "save_uncommitted_work",
                "picker_roots",
                "agents",
                "launch_choices",
                "quota.enabled",
                "quota.claude.probe",
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
        finding("quota.codex.command", 26, "expected path", &report);
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
    fn unknown_field_in_a_launch_choice_keeps_the_choice() {
        let report = read(
            "[launch_choices.p]\nkind = \"Agent\"\npreset = \"x\"\nextra = 42\n\n[launch_choices.q]\nkind = \"Terminal\"\n",
        );
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        finding(
            "launch_choices.p.extra",
            4,
            "unknown setting; ignored",
            &report,
        );
        assert_eq!(
            report.settings.launch_choices["p"],
            LaunchChoice::Agent("x".into())
        );
        assert_eq!(report.settings.launch_choices["q"], LaunchChoice::Terminal);
        assert_eq!(
            row(&report, "launch_choices").source,
            SettingSource::Document
        );
    }

    #[test]
    fn unknown_field_in_an_agents_row_keeps_the_row() {
        let report = read(
            "[[agents]]\nname = \"a\"\nargv = [\"a\"]\n\n[[agents]]\nname = \"b\"\nargv = [\"b\"]\nextra = true\n",
        );
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        finding("agents[1].extra", 8, "unknown setting; ignored", &report);
        let names: Vec<_> = report
            .settings
            .agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect();
        assert_eq!(names, ["a", "b"]);

        let report = read("agents = [{ name = \"a\", argv = [\"a\"], extra = 1 }]\n");
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        finding("agents[0].extra", 1, "unknown setting; ignored", &report);
        assert_eq!(report.settings.agents.len(), 1);
    }

    /// A wrong-typed element is not the collection and not a silent skip.
    #[test]
    fn wrong_typed_element_drops_only_that_element_with_a_finding_naming_it() {
        let report = read(
            "\
ready_sound = true
picker_roots = [\"/tmp/kept\", 7, \"/tmp/also\"]
agents = [{ name = \"kept\", argv = [\"kept\"] }, \"nope\", { name = \"also\", argv = [\"also\"] }]
launch_choices = { kept = { kind = \"Terminal\" }, bad = \"nope\" }
",
        );

        assert!(report.settings.ready_sound);
        assert_eq!(
            report.settings.picker_roots,
            vec![PathBuf::from("/tmp/kept"), PathBuf::from("/tmp/also")]
        );
        let names: Vec<_> = report
            .settings
            .agents
            .iter()
            .map(|agent| agent.name.as_str())
            .collect();
        assert_eq!(names, ["kept", "also"]);
        assert_eq!(
            report.settings.launch_choices,
            [("kept".to_owned(), LaunchChoice::Terminal)]
                .into_iter()
                .collect()
        );
        assert_eq!(row(&report, "picker_roots").source, SettingSource::Document);
        assert_eq!(row(&report, "agents").source, SettingSource::Document);
        assert_eq!(
            row(&report, "launch_choices").source,
            SettingSource::Document
        );

        assert_eq!(report.findings.len(), 3, "{:?}", report.findings);
        finding(
            "picker_roots[1]",
            2,
            "invalid type: integer `7`, expected a string",
            &report,
        );
        finding(
            "agents[1]",
            3,
            r#"invalid type: string "nope", expected a table { name, argv }"#,
            &report,
        );
        finding(
            "launch_choices.bad",
            4,
            r#"invalid type: string "nope", expected a table { kind, preset }"#,
            &report,
        );
        assert!(
            report.findings.iter().all(|found| {
                !matches!(
                    found.key.as_deref(),
                    Some("picker_roots" | "agents" | "launch_choices")
                )
            }),
            "a wrong-typed element must not wipe its collection: {:?}",
            report.findings
        );
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
        assert!(report.unparseable);
        assert!(!read("ready_sound = true\n").unparseable);
    }

    /// The editor shows the default beside an overridden value.
    #[test]
    fn every_row_carries_its_default_whatever_the_document_sets() {
        let report = read(
            "branch_prefix = \"kh/\"\nautomatic_local_terminals = \"on\"\n[quota.codex]\nhome = \"/h\"\n",
        );
        let pairs = |key| {
            let row = row(&report, key);
            (row.value.as_deref(), row.default.as_deref(), row.source)
        };
        assert_eq!(
            pairs("branch_prefix"),
            (Some("kh/"), Some("feature/"), SettingSource::Document)
        );
        assert_eq!(
            pairs("automatic_local_terminals"),
            (
                Some("on"),
                Some("default_branch_only"),
                SettingSource::Document
            )
        );
        assert_eq!(
            pairs("quota.codex.home"),
            (Some("/h"), None, SettingSource::Document)
        );
        assert_eq!(
            pairs("ready_sound"),
            (Some("false"), Some("false"), SettingSource::Default)
        );
        assert_eq!(read("").rows, {
            let mut rows = read("").rows;
            for row in &mut rows {
                row.default = row.value.clone();
            }
            rows
        });
    }

    #[test]
    fn quota_in_instance_identity_is_a_finding_and_configures_nothing() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().to_path_buf();
        std::fs::write(
            crate::config::legacy_identity_path(&config),
            "projects = []\n\n[quota]\nenabled = true\n",
        )
        .unwrap();
        let report = load_document(&config, &root.path().join("dashboard.toml"));
        // A [quota] table in the preserved identity file is ignored. Collection stays at the
        // default, which is on unless dashboard.toml sets quota.enabled = false.
        assert_eq!(report.settings.quota, defaults().quota);
        assert!(report.settings.quota.enabled);
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
        let config = root.path().join("inst");
        std::fs::create_dir(&config).unwrap();
        // A directory cannot be read as a file.
        let report = load_document(&config, root.path());
        assert_eq!(report.settings, defaults());
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].key, None);
        // A missing document is the normal first run: no finding.
        let report = load_document(&config, &config.join("dashboard.toml"));
        assert_eq!(report.findings, vec![]);
    }

    #[test]
    fn env_chooses_the_document_path_only() {
        let home = Path::new("/inst");
        assert_eq!(
            document_path_with(home, None),
            PathBuf::from("/inst/dashboard.toml")
        );
        assert_eq!(
            document_path_with(home, Some("/elsewhere/custom.toml".into())),
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

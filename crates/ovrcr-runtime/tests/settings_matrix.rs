//! The settings document matrix (item 5, Tier 1 of the usage-defaults plan).
//!
//! Generated from the loader itself: `settings::parse(path, "")` lists every
//! declared setting with its default value text, and the cases come from that
//! list, so a new setting gets its cases without new test code. Each case runs
//! twice: alone in the document, and beside every other setting at a valid
//! non-default value.
//!
//! Each test collects every failing case and reports them together, so a
//! broken loader rule shows the whole set of cases it breaks.

use ovrcr_protocol::{
    ReadySoundChoice, SettingRow, SettingSource, SettingsFinding, SettingsReport,
};
use ovrcr_runtime::settings;
use std::path::Path;

/// The TOML type of a setting, read from its default value text.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Bool,
    Str,
    Array,
    Table,
}

/// Settings whose default is unset have no value text to read a type from.
const UNSET_KINDS: &[(&str, Kind)] = &[
    ("title_model", Kind::Str),
    ("quota.codex.home", Kind::Str),
    ("quota.grok.home", Kind::Str),
    ("quota.cursor.state_db", Kind::Str),
];

/// Enum settings: how one spelling sits in the document, and the documented
/// spellings. Near misses of each spelling are the unknown-spelling cases.
const ENUMS: &[(&str, &str, &[&str])] = &[
    (
        "ready_sound_choice",
        "\"{}\"",
        &["default", "tap", "chime", "rise"],
    ),
    (
        "automatic_local_terminals",
        "\"{}\"",
        &["on", "off", "default_branch_only"],
    ),
    (
        "launch_choices",
        "{ p = { kind = \"{}\", preset = \"x\" } }",
        &["Agent"],
    ),
    ("launch_choices", "{ p = { kind = \"{}\" } }", &["Terminal"]),
];

/// A valid non-default value where the default's text cannot suggest one: an
/// empty or home-dependent collection, or a string with its own format.
const SAMPLES: &[(&str, &str)] = &[
    ("title_model", "\"pi/matrix\""),
    ("picker_roots", "[\"/tmp/ovrcr-matrix\"]"),
    ("agents", "[{ name = \"matrix\", argv = [\"matrix\"] }]"),
    ("launch_choices", "{ p = { kind = \"Terminal\" } }"),
];

/// One declared setting as the matrix sees it.
#[derive(Clone, Debug)]
struct Setting {
    key: String,
    kind: Kind,
    default: SettingRow,
}

impl Setting {
    fn all() -> Vec<Setting> {
        read("").rows.into_iter().map(Setting::new).collect()
    }

    fn new(default: SettingRow) -> Setting {
        let kind = match &default.value {
            Some(text) => match toml_value(text) {
                Some(toml::Value::Boolean(_)) => Kind::Bool,
                Some(toml::Value::Array(_)) => Kind::Array,
                Some(toml::Value::Table(_)) => Kind::Table,
                _ => Kind::Str,
            },
            None => lookup(UNSET_KINDS, &default.key)
                .unwrap_or_else(|| panic!("{}: unset default; add it to UNSET_KINDS", default.key)),
        };
        Setting {
            key: default.key.clone(),
            kind,
            default,
        }
    }

    /// The default spelled in TOML; `None` when the default is unset, which
    /// has no spelling.
    fn explicit_default(&self) -> Option<String> {
        let text = self.default.value.as_ref()?;
        Some(match self.kind {
            Kind::Str => toml::Value::String(text.clone()).to_string(),
            _ => text.clone(),
        })
    }

    fn non_default(&self) -> String {
        if let Some(sample) = lookup(SAMPLES, &self.key) {
            return sample.into();
        }
        let current = self.default.value.as_deref();
        if let Some((template, spelling)) =
            spellings(&self.key).find(|(_, spelling)| Some(*spelling) != current)
        {
            return template.replace("{}", spelling);
        }
        match self.kind {
            Kind::Bool => (current != Some("true")).to_string(),
            Kind::Str => {
                let text = match current {
                    Some(text) => format!("{text}-matrix"),
                    None => "/tmp/ovrcr-matrix".into(),
                };
                toml::Value::String(text).to_string()
            }
            Kind::Array | Kind::Table => {
                panic!("{}: add a valid non-default value to SAMPLES", self.key)
            }
        }
    }

    fn wrong_type(&self) -> &'static str {
        match self.kind {
            Kind::Bool => "\"matrix\"",
            Kind::Str | Kind::Array | Kind::Table => "7",
        }
    }

    /// The row this setting reports for a non-default value written as
    /// `text`. A consent setting explains how to turn it on only while it is
    /// off. Most of those default to off, so a non-default value drops the
    /// explanation. `quota.enabled` defaults to on, so the explanation is on
    /// the explicit false value instead.
    fn row_for(&self, text: &str) -> SettingRow {
        let written = toml_value(text).unwrap();
        let value = match (self.kind, written) {
            (Kind::Str, toml::Value::String(text)) => text,
            (_, written) => written.to_string(),
        };
        let off_state = (self.key == "quota.enabled" && value == "false")
            .then(|| ovrcr_runtime::settings::QUOTA_OFF.to_owned());
        SettingRow {
            value: Some(value),
            source: SettingSource::Document,
            off_state,
            ..self.default.clone()
        }
    }

    /// Invalid sound choices suppress sound; other invalid values keep their default.
    fn invalid_row(&self) -> SettingRow {
        if self.key == ReadySoundChoice::KEY {
            SettingRow {
                value: None,
                source: SettingSource::Document,
                ..self.default.clone()
            }
        } else {
            self.default.clone()
        }
    }
}

fn lookup<T: Copy>(table: &[(&str, T)], key: &str) -> Option<T> {
    table
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

fn spellings(key: &str) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
    ENUMS
        .iter()
        .filter(move |(name, _, _)| *name == key)
        .flat_map(|(_, template, spellings)| {
            spellings.iter().map(|spelling| (*template, *spelling))
        })
}

fn toml_value(text: &str) -> Option<toml::Value> {
    toml::from_str::<toml::Table>(&format!("v = {text}"))
        .ok()?
        .remove("v")
}

/// Near misses of `word`: its last character dropped, then each pair of
/// adjacent differing characters swapped.
fn misspellings(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut found = vec![chars[..chars.len() - 1].iter().collect::<String>()];
    for index in 0..chars.len() - 1 {
        if chars[index] != chars[index + 1] {
            let mut swapped = chars.clone();
            swapped.swap(index, index + 1);
            found.push(swapped.into_iter().collect());
        }
    }
    found.retain(|spelling| !spelling.is_empty() && spelling != word);
    found.dedup();
    found
}

/// The document generator: one `dotted.key = value` line per entry, in order.
#[derive(Clone, Default)]
struct Document {
    lines: Vec<(String, String)>,
}

impl Document {
    /// Every setting at a valid non-default value, except `skip`.
    fn every_other(settings: &[Setting], skip: &str) -> Document {
        let mut document = Document::default();
        for setting in settings.iter().filter(|setting| setting.key != skip) {
            document.set(&setting.key, &setting.non_default());
        }
        document
    }

    /// Append `key = value`; returns its 1-based line.
    fn set(&mut self, key: &str, value: &str) -> u32 {
        self.lines.push((key.into(), value.into()));
        self.lines.len() as u32
    }

    fn text(&self) -> String {
        self.lines
            .iter()
            .map(|(key, value)| format!("{key} = {value}\n"))
            .collect()
    }
}

fn read(text: &str) -> SettingsReport {
    settings::parse(Path::new("dashboard.toml"), text)
}

/// The backgrounds each case runs against.
fn backgrounds(settings: &[Setting], subject: &str) -> [(&'static str, Document); 2] {
    [
        ("alone", Document::default()),
        (
            "beside every other setting valid",
            Document::every_other(settings, subject),
        ),
    ]
}

/// Collects failing cases so one run shows every case a loader change breaks.
#[derive(Default)]
struct Failures(Vec<String>);

impl Failures {
    fn check(&mut self, case: &str, ok: bool, detail: impl FnOnce() -> String) {
        if !ok {
            self.0.push(format!("{case}: {}", detail()));
        }
    }

    /// The subject's row is `expected` and every other row matches `baseline`.
    fn rows(
        &mut self,
        case: &str,
        report: &SettingsReport,
        baseline: &SettingsReport,
        expected: &SettingRow,
    ) {
        for (row, base) in report.rows.iter().zip(&baseline.rows) {
            let want = if row.key == expected.key {
                expected
            } else {
                base
            };
            self.check(case, row == want, || {
                format!("row {}: got {row:?}, want {want:?}", row.key)
            });
        }
        self.check(case, report.rows.len() == baseline.rows.len(), || {
            "row count changed".into()
        });
    }

    /// Exactly one finding, naming `key` at `line`, its message containing `contains`.
    fn one_finding(
        &mut self,
        case: &str,
        findings: &[SettingsFinding],
        key: &str,
        line: u32,
        contains: &str,
    ) {
        let ok = matches!(findings, [finding]
            if finding.key.as_deref() == Some(key)
                && finding.line == Some(line)
                && finding.message.contains(contains));
        self.check(case, ok, || {
            format!("want one finding for {key} at line {line} containing {contains:?}; got {findings:?}")
        });
    }

    fn no_findings(&mut self, case: &str, findings: &[SettingsFinding]) {
        self.check(case, findings.is_empty(), || {
            format!("want no findings; got {findings:?}")
        });
    }

    fn assert_none(self, cases: usize) {
        assert!(cases > 0, "the matrix generated no cases");
        eprintln!("{cases} cases");
        assert!(
            self.0.is_empty(),
            "{} of {cases} cases failed:\n{}",
            self.0.len(),
            self.0.join("\n")
        );
    }
}

/// Every setting the loader declares is in the matrix with a known type.
#[test]
fn matrix_covers_every_declared_setting() {
    let settings = Setting::all();
    let keys: Vec<_> = settings
        .iter()
        .map(|setting| setting.key.as_str())
        .collect();
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
            "quota.cursor.dashboard",
            "quota.cursor.state_db",
        ]
    );
    let kinds: Vec<_> = settings.iter().map(|setting| setting.kind).collect();
    use Kind::*;
    assert_eq!(
        kinds,
        [
            Bool, Bool, Str, Bool, Str, Str, Str, Bool, Array, Array, Table, Bool, Bool, Str, Str,
            Str, Str, Bool, Str
        ]
    );
    // Every test-side table names a declared setting, so none goes stale.
    for key in UNSET_KINDS
        .iter()
        .map(|(key, _)| *key)
        .chain(SAMPLES.iter().map(|(key, _)| *key))
        .chain(ENUMS.iter().map(|(key, _, _)| *key))
    {
        assert!(keys.contains(&key), "{key} is not a declared setting");
    }
}

#[test]
fn generator_spells_near_misses() {
    assert_eq!(misspellings("home"), ["hom", "ohme", "hmoe", "hoem"]);
    // Swapping equal neighbours changes nothing and is not a case.
    assert_eq!(misspellings("all"), ["al", "lal"]);
    assert_eq!(misspellings("on"), ["o", "no"]);
}

#[test]
fn missing_setting_keeps_its_default() {
    let settings = Setting::all();
    let (mut failures, mut cases) = (Failures::default(), 0);
    for setting in &settings {
        for (background, document) in backgrounds(&settings, &setting.key) {
            cases += 1;
            let case = format!("{} missing, {background}", setting.key);
            let report = read(&document.text());
            failures.no_findings(&case, &report.findings);
            let row = report.rows.iter().find(|row| row.key == setting.key);
            failures.check(&case, row == Some(&setting.default), || {
                format!("got {row:?}, want {:?}", setting.default)
            });
        }
    }
    failures.assert_none(cases);
}

#[test]
fn explicit_default_is_the_default_from_the_document() {
    let settings = Setting::all();
    let (mut failures, mut cases) = (Failures::default(), 0);
    for setting in &settings {
        // An unset default has no TOML spelling.
        let Some(value) = setting.explicit_default() else {
            continue;
        };
        for (background, mut document) in backgrounds(&settings, &setting.key) {
            cases += 1;
            let case = format!("{} = {value} (default), {background}", setting.key);
            let baseline = read(&document.text());
            document.set(&setting.key, &value);
            let report = read(&document.text());
            failures.no_findings(&case, &report.findings);
            let expected = SettingRow {
                source: SettingSource::Document,
                ..setting.default.clone()
            };
            failures.rows(&case, &report, &baseline, &expected);
        }
    }
    failures.assert_none(cases);
}

#[test]
fn explicit_non_default_takes_effect() {
    let settings = Setting::all();
    let (mut failures, mut cases) = (Failures::default(), 0);
    for setting in &settings {
        let value = setting.non_default();
        for (background, mut document) in backgrounds(&settings, &setting.key) {
            cases += 1;
            let case = format!("{} = {value}, {background}", setting.key);
            let baseline = read(&document.text());
            document.set(&setting.key, &value);
            let report = read(&document.text());
            failures.no_findings(&case, &report.findings);
            let expected = setting.row_for(&value);
            failures.check(&case, expected.value != setting.default.value, || {
                "sample equals the default".into()
            });
            failures.rows(&case, &report, &baseline, &expected);
        }
    }
    failures.assert_none(cases);
}

#[test]
fn wrong_type_reports_one_finding_and_disables_invalid_cursor_source() {
    let settings = Setting::all();
    let (mut failures, mut cases) = (Failures::default(), 0);
    for setting in &settings {
        let value = setting.wrong_type();
        for (background, mut document) in backgrounds(&settings, &setting.key) {
            cases += 1;
            let case = format!("{} = {value} (wrong type), {background}", setting.key);
            let mut baseline = read(&document.text());
            let line = document.set(&setting.key, value);
            let report = read(&document.text());
            if setting.key == "quota.cursor.state_db" {
                // A malformed explicit path must not select the default account.
                let dashboard = baseline
                    .rows
                    .iter_mut()
                    .find(|row| row.key == "quota.cursor.dashboard")
                    .unwrap();
                dashboard.value = Some("false".into());
                dashboard.off_state = Some(ovrcr_protocol::CURSOR_QUOTA_DESCRIPTION.into());
                failures.check(&case, !report.settings.quota.cursor.dashboard, || {
                    "invalid Cursor database left its reader enabled".into()
                });
            }
            failures.one_finding(&case, &report.findings, &setting.key, line, "");
            failures.rows(&case, &report, &baseline, &setting.invalid_row());
        }
    }
    failures.assert_none(cases);
}

/// Every segment of every key, misspelled: `quota.codex.home` yields
/// `quot.codex.home`, `quota.code.home`, `quota.codex.hom` and the swaps.
/// The finding names the key up to the misspelled segment.
#[test]
fn misspelled_key_is_one_finding_and_changes_nothing_else() {
    let settings = Setting::all();
    let (mut failures, mut cases) = (Failures::default(), 0);
    for setting in &settings {
        let segments: Vec<_> = setting.key.split('.').collect();
        for (index, segment) in segments.iter().enumerate() {
            for spelling in misspellings(segment) {
                let mut path = segments.clone();
                path[index] = &spelling;
                let unknown = path[..=index].join(".");
                let misspelled = path.join(".");
                for (background, mut document) in backgrounds(&settings, &setting.key) {
                    cases += 1;
                    let case = format!("{misspelled} for {}, {background}", setting.key);
                    let baseline = read(&document.text());
                    let line = document.set(&misspelled, &setting.non_default());
                    let report = read(&document.text());
                    failures.one_finding(
                        &case,
                        &report.findings,
                        &unknown,
                        line,
                        "unknown setting",
                    );
                    failures.rows(&case, &report, &baseline, &setting.default);
                }
            }
        }
    }
    failures.assert_none(cases);
}

#[test]
fn unknown_enum_spelling_is_one_finding_not_a_silent_default() {
    let settings = Setting::all();
    let (mut failures, mut cases) = (Failures::default(), 0);
    for (key, template, spellings) in ENUMS {
        let setting = settings.iter().find(|setting| setting.key == *key).unwrap();
        for spelling in *spellings {
            // The documented spelling is accepted, so the template is right.
            let value = template.replace("{}", spelling);
            cases += 1;
            let report = read(&format!("{key} = {value}\n"));
            failures.no_findings(&format!("{key} = {value}"), &report.findings);
            for near in misspellings(spelling) {
                let value = template.replace("{}", &near);
                for (background, mut document) in backgrounds(&settings, key) {
                    cases += 1;
                    let case = format!("{key} = {value} (unknown spelling), {background}");
                    let baseline = read(&document.text());
                    let line = document.set(key, &value);
                    let report = read(&document.text());
                    failures.one_finding(&case, &report.findings, key, line, &near);
                    failures.rows(&case, &report, &baseline, &setting.invalid_row());
                }
            }
        }
    }
    failures.assert_none(cases);
}

fn default_rows() -> Vec<SettingRow> {
    read("").rows
}

fn full_document() -> String {
    Document::every_other(&Setting::all(), "").text()
}

#[test]
fn missing_and_empty_documents_are_all_defaults_without_findings() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().to_path_buf();
    let document = root.path().join("dashboard.toml");
    let report = settings::load_document(&config, &document);
    assert_eq!(report.findings, vec![]);
    assert_eq!(report.rows, default_rows());

    std::fs::write(&document, "").unwrap();
    let report = settings::load_document(&config, &document);
    assert_eq!(report.findings, vec![]);
    assert_eq!(report.rows, default_rows());
}

#[test]
fn malformed_document_is_all_defaults_with_one_document_finding() {
    let mut text = full_document();
    let line = text.lines().count() as u32 + 1;
    text.push_str("not = toml {\n");
    let report = read(&text);
    assert_eq!(report.rows, default_rows());
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert_eq!(report.findings[0].key, None);
    assert_eq!(report.findings[0].line, Some(line));
    assert!(report.findings[0].message.contains("not valid TOML"));
}

#[test]
fn quota_in_instance_identity_is_one_finding_and_configures_nothing() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().to_path_buf();
    let document = root.path().join("dashboard.toml");
    std::fs::write(
        ovrcr_runtime::config::legacy_identity_path(&config),
        "projects = []\n\n[quota]\nenabled = true\n\n[quota.codex]\ncommand = \"/opt/elsewhere\"\n",
    )
    .unwrap();

    // With no settings document: quota stays at its defaults.
    let report = settings::load_document(&config, &document);
    assert_eq!(report.rows, default_rows());
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    let finding = &report.findings[0];
    assert_eq!(finding.key.as_deref(), Some("quota"));
    assert_eq!(finding.line, Some(3));
    assert!(finding.message.contains("belong in dashboard.toml"));

    // Beside a full settings document: the document's values win untouched.
    let full = full_document();
    std::fs::write(&document, &full).unwrap();
    let report = settings::load_document(&config, &document);
    assert_eq!(report.rows, read(&full).rows);
    assert_eq!(report.findings, vec![finding.clone()]);
}

#[test]
fn symlinked_document_is_followed() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().to_path_buf();
    let target = root.path().join("dotfiles").join("ovrcr.toml");
    let link = root.path().join("dashboard.toml");
    std::fs::create_dir(target.parent().unwrap()).unwrap();
    let full = full_document();
    std::fs::write(&target, &full).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let report = settings::load_document(&config, &link);
    assert_eq!(report.findings, vec![]);
    assert_eq!(report.rows, read(&full).rows);
    assert_ne!(report.rows, default_rows());
    assert_eq!(report.path, link);
}

#[test]
fn dangling_symlink_is_a_document_finding_not_a_fresh_install() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().to_path_buf();
    let document = root.path().join("dashboard.toml");
    std::os::unix::fs::symlink("missing.toml", &document).unwrap();

    let report = settings::load_document(&config, &document);

    // A missing file is a fresh install: defaults and no finding. A link whose
    // target does not exist is not that, even though reading it is NotFound.
    assert_eq!(report.rows, default_rows());
    assert!(!report.unparseable);
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    let finding = &report.findings[0];
    assert_eq!(finding.key, None);
    assert_eq!(finding.line, None);
    assert_eq!(
        finding.message,
        format!(
            "settings document {} is a dangling symlink",
            document.display()
        )
    );
}

#[test]
fn read_only_document_is_still_read() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let config = root.path().to_path_buf();
    let document = root.path().join("dashboard.toml");
    let full = full_document();
    std::fs::write(&document, &full).unwrap();
    std::fs::set_permissions(&document, std::fs::Permissions::from_mode(0o444)).unwrap();
    let report = settings::load_document(&config, &document);
    assert_eq!(report.findings, vec![]);
    assert_eq!(report.rows, read(&full).rows);
}

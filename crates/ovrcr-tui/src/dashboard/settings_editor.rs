//! The Settings view: the Server's reading as grouped, editable rows.
//!
//! Every edit is one `SetSetting` request; nothing changes here until the
//! Server's next reading arrives, and a refusal shows on the row it came from.
//! Opened from the menu and the palette (`Details::Settings`).
use super::picker::{PathPicker, PickItem, PickList};
use super::render::{BASE, MAUVE, MUTED, PEACH, RED, SUBTEXT, SURFACE0, TEXT, YELLOW};
use super::settings::LaunchChoice;
use super::text_cursor::TextCursor;
use super::{Dashboard, DashboardAction};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
#[cfg(target_os = "macos")]
use ovrcr_protocol::ReadySoundChoice;
use ovrcr_protocol::{
    ClientMessage, Request, Response, SettingSource, SettingsReport, ThemeAppearance, ThemeId,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use std::collections::HashMap;

pub(super) const TITLE: &str =
    "Settings · Enter edit · [ up · ] down · r reset · x remove · / filter · Esc close";
const UNPARSEABLE: &str = "Editing is off until dashboard.toml is fixed by hand.";
const PICK_VISIBLE_ROWS: usize = 6;

#[derive(Default)]
pub(super) struct Editor {
    /// The selected row's id, so a new reading keeps the selection.
    selected: Option<String>,
    filter: String,
    filtering: bool,
    cursor: TextCursor,
    edit: Option<Edit>,
    /// Requests in flight, by request id, and the row each came from.
    pending: HashMap<u64, String>,
    /// The Server's refusal, by row id, until that row is edited again.
    refused: HashMap<String, String>,
    notice: Option<String>,
    /// A transient theme shown while browsing the theme picker. The Server's
    /// settings remain authoritative and are never changed by previewing.
    pub(super) preview_theme: Option<ThemeId>,
    /// A preview committed through SetSetting remains visible until the
    /// Server publishes the resulting settings report.
    pub(super) preview_request: Option<u64>,
}

struct Edit {
    row: String,
    text: String,
    picker: Option<PathPicker>,
    pick: Option<PickList>,
}

impl Editor {
    pub(super) fn cancel_draft(&mut self) {
        self.edit = None;
        self.filtering = false;
        self.preview_theme = None;
        self.preview_request = None;
    }
}

/// What Enter does on a row, and how its value is written.
#[derive(Clone, Debug, PartialEq)]
enum Kind {
    /// A boolean: Enter writes the other value at once.
    Toggle(bool),
    /// A choice; each item's value is the TOML text written at `path`.
    Pick(Vec<PickItem>),
    /// A string, written quoted.
    Text,
    /// A path, written quoted, with directory completion on Tab.
    Path,
    /// TOML value text written as typed, such as an argv array literal.
    Toml,
    /// No in-place edit; the row's children are.
    Fixed,
}

#[derive(Clone, Debug)]
struct Row {
    id: String,
    group: &'static str,
    label: String,
    value: String,
    /// Top-level settings only.
    source: Option<SettingSource>,
    default: Option<String>,
    finding: Option<String>,
    about: Vec<String>,
    child: bool,
    kind: Kind,
    /// Where an edit writes. `picker_roots` children while the list is its
    /// default rewrite the whole list instead.
    path: String,
    /// What Reset (top-level) or Remove (child) sends.
    reset: Option<(String, Option<String>)>,
    /// The whole list an edit of a default `picker_roots` child writes, with
    /// `{}` for the edited value.
    whole_list: Option<Vec<String>>,
}

fn toml_string(text: &str) -> String {
    toml_edit::Value::from(text).to_string()
}

fn toml_array<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    items.into_iter().collect::<toml_edit::Array>().to_string()
}

/// The whole `picker_roots` array with the entry at `index` swapped by `delta`.
/// `None` when that step would pass either end.
fn reordered_roots(roots: &[String], index: usize, delta: isize) -> Option<String> {
    let target = index as isize + delta;
    if target < 0 || target >= roots.len() as isize {
        return None;
    }
    let mut quoted: Vec<String> = roots.iter().map(|root| toml_string(root)).collect();
    quoted.swap(index, target as usize);
    Some(format!("[{}]", quoted.join(", ")))
}

fn count(entries: usize) -> String {
    match entries {
        1 => "1 entry".into(),
        n => format!("{n} entries"),
    }
}

fn choice_text(choice: &LaunchChoice) -> String {
    match choice {
        LaunchChoice::Terminal => "Terminal".into(),
        LaunchChoice::Agent(preset) => format!("Agent: {preset}"),
    }
}

/// The value written at `launch_choices.<project>` for `choice`.
fn choice_value(project: &str, choice: &LaunchChoice) -> String {
    match super::settings::launch_choice_request(project, choice) {
        Request::SetSetting { value, .. } => value.unwrap_or_default(),
        _ => unreachable!("launch_choice_request builds SetSetting"),
    }
}

/// Every row in group order. `presets` are the agents a launch choice may
/// name; they are only listed when an edit starts.
fn items(report: &SettingsReport, presets: &[String]) -> Vec<Row> {
    let row_of = |key: &str| report.rows.iter().find(|row| row.key == key);
    let finding = |key: &str, collection: bool| {
        report
            .findings
            .iter()
            .filter(|f| {
                f.key.as_deref().is_some_and(|k| {
                    k == key
                        || (collection
                            && (k.starts_with(&format!("{key}["))
                                || k.starts_with(&format!("{key}."))))
                })
            })
            .map(|f| match f.line {
                Some(line) => format!("line {line}: {}", f.message),
                None => f.message.clone(),
            })
            .reduce(|a, b| format!("{a}; {b}"))
    };
    let top = |group, key: &str, label: &str, kind: Kind, about: &[&str]| {
        let row = row_of(key);
        let source = row.map_or(SettingSource::Default, |row| row.source);
        let mut lines: Vec<String> = row
            .and_then(|row| row.off_state.clone())
            .into_iter()
            .collect();
        for line in about {
            let line = line.to_string();
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
        let value = row.and_then(|row| row.value.clone());
        Row {
            id: key.into(),
            group,
            label: label.into(),
            value: match (&kind, value.as_deref()) {
                (Kind::Toggle(true), _) => "on".into(),
                (Kind::Toggle(false), _) => "off".into(),
                (_, Some(value)) => value.into(),
                (_, None) => "unset".into(),
            },
            source: Some(source),
            default: row.and_then(|row| row.default.clone()).map(|default| {
                match (&kind, default.as_str()) {
                    (Kind::Toggle(_), "true") => "on".into(),
                    (Kind::Toggle(_), "false") => "off".into(),
                    _ => default,
                }
            }),
            finding: finding(key, kind == Kind::Fixed),
            about: lines,
            child: false,
            kind,
            path: key.into(),
            reset: (source == SettingSource::Document).then(|| (key.to_string(), None)),
            whole_list: None,
        }
    };
    let child = |group, id: String, label: String, value: String, kind, path| Row {
        id,
        group,
        label,
        value,
        source: None,
        default: None,
        finding: None,
        about: Vec::new(),
        child: true,
        kind,
        path,
        reset: None,
        whole_list: None,
    };
    let settings = &report.settings;
    let mut items = Vec::new();
    for entry in ovrcr_protocol::entries() {
        match entry.shape {
            ovrcr_protocol::Shape::Paths => {
                let mut parent = top(
                    entry.group,
                    entry.path,
                    entry.label,
                    Kind::Fixed,
                    entry.about,
                );
                let roots: Vec<String> = settings
                    .picker_roots
                    .iter()
                    .map(|root| root.display().to_string())
                    .collect();
                let roots_set = parent.source == Some(SettingSource::Document);
                parent.value = count(roots.len());
                items.push(parent);
                for (index, root) in roots.iter().enumerate() {
                    let mut row = child(
                        entry.group,
                        format!("picker_roots[{index}]"),
                        String::new(),
                        root.clone(),
                        Kind::Path,
                        format!("picker_roots[{index}]"),
                    );
                    if roots_set {
                        row.reset = Some((row.path.clone(), None));
                    } else {
                        // The default list is not in the document: write it whole.
                        let mut list: Vec<String> = roots.iter().map(|r| toml_string(r)).collect();
                        list[index] = "{}".into();
                        row.path = "picker_roots".into();
                        row.whole_list = Some(list.clone());
                        list.remove(index);
                        row.reset = Some((
                            "picker_roots".into(),
                            Some(format!("[{}]", list.join(", "))),
                        ));
                    }
                    items.push(row);
                }
                let mut add = child(
                    entry.group,
                    "picker_roots+".into(),
                    "+ Add root".into(),
                    String::new(),
                    Kind::Path,
                    format!("picker_roots[{}]", roots.len()),
                );
                if !roots_set {
                    let mut list: Vec<String> = roots.iter().map(|r| toml_string(r)).collect();
                    list.push("{}".into());
                    add.path = "picker_roots".into();
                    add.whole_list = Some(list);
                }
                items.push(add);
            }
            ovrcr_protocol::Shape::Agents => {
                let mut parent = top(
                    entry.group,
                    entry.path,
                    entry.label,
                    Kind::Fixed,
                    entry.about,
                );
                parent.value = count(settings.agents.len());
                items.push(parent);
                for (index, agent) in settings.agents.iter().enumerate() {
                    let mut row = child(
                        entry.group,
                        format!("agents[{index}]"),
                        agent.name.clone(),
                        toml_array(agent.argv.iter().map(String::as_str)),
                        Kind::Toml,
                        format!("agents[{index}].argv"),
                    );
                    row.reset = Some((format!("agents[{index}]"), None));
                    items.push(row);
                }
                items.push(child(
                    entry.group,
                    "agents+".into(),
                    "+ Add agent (name)".into(),
                    String::new(),
                    Kind::Text,
                    format!("agents[{}]", settings.agents.len()),
                ));
            }
            ovrcr_protocol::Shape::Launches => {
                let mut parent = top(
                    entry.group,
                    entry.path,
                    entry.label,
                    Kind::Fixed,
                    entry.about,
                );
                parent.value = count(settings.launch_choices.len());
                items.push(parent);
                for (project, choice) in &settings.launch_choices {
                    let path = format!("launch_choices.{}", toml_edit::Key::new(project.as_str()));
                    let mut choices = vec![LaunchChoice::Terminal];
                    choices.extend(presets.iter().cloned().map(LaunchChoice::Agent));
                    if !choices.contains(choice) {
                        choices.push(choice.clone());
                    }
                    let mut row = child(
                        entry.group,
                        path.clone(),
                        project.clone(),
                        choice_text(choice),
                        Kind::Pick(
                            choices
                                .iter()
                                .map(|choice| PickItem {
                                    label: choice_text(choice),
                                    value: choice_value(project, choice),
                                })
                                .collect(),
                        ),
                        path.clone(),
                    );
                    row.reset = Some((path, None));
                    items.push(row);
                }
                items.push(child(
                    entry.group,
                    "launch_choices+".into(),
                    "+ Add project".into(),
                    String::new(),
                    Kind::Text,
                    String::new(),
                ));
            }
            shape => {
                #[cfg(not(target_os = "macos"))]
                if entry.path == ovrcr_protocol::ReadySoundChoice::KEY {
                    continue;
                }
                let row = top(
                    entry.group,
                    entry.path,
                    entry.label,
                    editor_kind(shape, entry, settings),
                    entry.about,
                );
                #[cfg(target_os = "macos")]
                let row = native_sound_choice_labels(row, settings);
                let row = theme_choice_labels(row, settings);
                items.push(row);
            }
        }
    }
    items
}

/// Native labels are a presentation of the shared enum, while catalog paths
/// and picker values stay the saved setting's stable identifiers.
#[cfg(target_os = "macos")]
fn native_sound_choice_labels(mut row: Row, settings: &ovrcr_protocol::Settings) -> Row {
    if row.id == ReadySoundChoice::KEY {
        row.value = settings
            .ready_sound_choice
            .map_or("invalid", ReadySoundChoice::label)
            .into();
        row.default = row
            .default
            .as_deref()
            .and_then(ReadySoundChoice::parse)
            .map(|choice| choice.label().into());
    }
    row
}

/// Human-readable theme names in the Settings list; pick values stay kebab keys.
fn theme_choice_labels(mut row: Row, settings: &ovrcr_protocol::Settings) -> Row {
    if row.id == ThemeId::KEY {
        row.value = settings.theme.label().into();
        row.default = row
            .default
            .as_deref()
            .and_then(ThemeId::parse)
            .map(|choice| choice.label().into());
        if let Some(about) = row
            .about
            .iter_mut()
            .find(|about| about.starts_with("Changing theme updates the Dashboard"))
        {
            *about = "The highlighted choice previews across the Dashboard. Enter saves it; Esc restores the saved theme.".into();
        }
    }
    row
}

fn editor_kind(
    shape: ovrcr_protocol::Shape,
    entry: &ovrcr_protocol::Entry,
    settings: &ovrcr_protocol::Settings,
) -> Kind {
    match shape {
        ovrcr_protocol::Shape::Toggle => {
            Kind::Toggle(entry.value(settings).as_deref() == Some("true"))
        }
        ovrcr_protocol::Shape::Text(_) => Kind::Text,
        ovrcr_protocol::Shape::Path(_) => Kind::Path,
        ovrcr_protocol::Shape::Pick { options, .. } => Kind::Pick(
            options
                .iter()
                .map(|value| {
                    let mut label: &str = value;
                    if entry.path == ThemeId::KEY {
                        label = ThemeId::parse(label).map_or(label, ThemeId::label);
                    }
                    #[cfg(target_os = "macos")]
                    if entry.path == ReadySoundChoice::KEY {
                        label =
                            ReadySoundChoice::parse(label).map_or(label, ReadySoundChoice::label);
                    }
                    PickItem {
                        label: label.into(),
                        value: toml_string(value),
                    }
                })
                .collect(),
        ),
        ovrcr_protocol::Shape::Paths
        | ovrcr_protocol::Shape::Agents
        | ovrcr_protocol::Shape::Launches => Kind::Fixed,
    }
}

/// Greedy word wrap to `room` cells; a word longer than `room` stays whole.
fn wrap(text: &str, room: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthStr;

    let mut lines = vec![String::new()];
    for word in text.split(' ') {
        let line = lines.last_mut().expect("never empty");
        if !line.is_empty() && line.width() + 1 + word.width() > room {
            lines.push(word.to_string());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    lines
}

/// The palette's word filter: every word appears in the row's text.
fn matches(row: &Row, filter: &str) -> bool {
    let haystack = format!("{} {} {} {}", row.group, row.label, row.id, row.value).to_lowercase();
    filter
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

/// Findings that belong to no row: unknown keys and document-level problems.
fn loose_findings(report: &SettingsReport) -> Vec<String> {
    let keys: Vec<&str> = report.rows.iter().map(|row| row.key.as_str()).collect();
    report
        .findings
        .iter()
        .filter(|f| {
            f.key.as_deref().is_none_or(|key| {
                !keys.iter().any(|row| {
                    let collection = ovrcr_protocol::entries().iter().any(|entry| {
                        entry.path == *row
                            && matches!(
                                entry.shape,
                                ovrcr_protocol::Shape::Paths
                                    | ovrcr_protocol::Shape::Agents
                                    | ovrcr_protocol::Shape::Launches
                            )
                    });
                    key == *row
                        || (collection
                            && (key.starts_with(&format!("{row}["))
                                || key.starts_with(&format!("{row}."))))
                })
            })
        })
        .map(|f| {
            let line = f.line.map(|l| format!("line {l}, ")).unwrap_or_default();
            format!(
                "{line}{}: {}",
                f.key.as_deref().unwrap_or("document"),
                f.message
            )
        })
        .collect()
}

impl Dashboard {
    fn editor_rows(&self) -> Vec<Row> {
        let Some(report) = self.settings_report.as_deref() else {
            return Vec::new();
        };
        let filter = &self.settings_editor.filter;
        items(report, &[])
            .into_iter()
            .filter(|row| matches(row, filter))
            .collect()
    }

    fn selected_row(&self) -> Option<Row> {
        let rows = self.editor_rows();
        let editor = &self.settings_editor;
        rows.iter()
            .find(|row| Some(&row.id) == editor.selected.as_ref())
            .or(rows.first())
            .cloned()
    }

    fn launch_presets(&self) -> Vec<String> {
        self.detected_agents()
            .into_iter()
            .filter(|a| {
                a.source != super::agents::AgentSource::Shell
                    && a.source != super::agents::AgentSource::Custom
                    && a.name != "shell"
                    && a.name != "Custom"
            })
            .map(|a| a.name)
            .collect()
    }

    fn move_setting_selection(&mut self, delta: isize) {
        let rows = self.editor_rows();
        if rows.is_empty() {
            return;
        }
        let current = self
            .settings_editor
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|row| &row.id == id))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, rows.len() as isize - 1) as usize;
        self.settings_editor.selected = Some(rows[next].id.clone());
    }

    fn send_setting(&mut self, row: &str, path: String, value: Option<String>) -> DashboardAction {
        let request_id = self.next_request_id();
        let editor = &mut self.settings_editor;
        editor.refused.remove(row);
        editor.edit = None;
        if row == ThemeId::KEY && editor.preview_theme.is_some() {
            editor.preview_request = Some(request_id);
        }
        self.settings_editor
            .pending
            .insert(request_id, row.to_string());
        DashboardAction::Request(ClientMessage {
            request_id,
            request: Request::SetSetting { path, value },
        })
    }

    fn editing_disabled(&mut self) -> bool {
        let off = self
            .settings_report
            .as_ref()
            .is_none_or(|report| report.unparseable);
        if off {
            self.settings_editor.notice = Some(UNPARSEABLE.into());
        }
        off
    }

    pub(super) fn settings_editor_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
        }
        self.settings_editor.notice = None;
        if super::input::is_browse_key(key) {
            self.settings_editor.cancel_draft();
            self.details = None;
            return DashboardAction::Redraw;
        }
        if self.settings_editor.edit.is_some() {
            return self.edit_key(key);
        }
        if self.settings_editor.filtering {
            let editor = &mut self.settings_editor;
            match key.code {
                KeyCode::Esc => {
                    editor.filter.clear();
                    editor.filtering = false;
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Up => editor.filtering = false,
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    editor.cursor.insert(&mut editor.filter, &ch.to_string());
                    editor.selected = None;
                }
                code => {
                    editor.cursor.key(&mut editor.filter, code);
                    editor.selected = None;
                }
            }
            return DashboardAction::Redraw;
        }
        match key.code {
            KeyCode::Esc if !self.settings_editor.filter.is_empty() => {
                self.settings_editor.filter.clear();
            }
            KeyCode::Esc => {
                self.settings_editor.cancel_draft();
                self.details = None;
            }
            KeyCode::Char('/') => {
                let editor = &mut self.settings_editor;
                editor.filtering = true;
                editor.filter.clear();
                editor.selected = None;
                editor.cursor = TextCursor::default();
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_setting_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_setting_selection(-1),
            KeyCode::PageDown => self.move_setting_selection(10),
            KeyCode::PageUp => self.move_setting_selection(-10),
            KeyCode::Home => self.move_setting_selection(isize::MIN / 2),
            KeyCode::End => self.move_setting_selection(isize::MAX / 2),
            KeyCode::Enter => return self.start_edit(),
            KeyCode::Char('[') => return self.move_picker_root(-1),
            KeyCode::Char(']') => return self.move_picker_root(1),
            KeyCode::Char('r') => return self.reset_or_remove(false),
            KeyCode::Char('x') | KeyCode::Delete => return self.reset_or_remove(true),
            _ => {}
        }
        DashboardAction::Redraw
    }

    fn move_picker_root(&mut self, delta: isize) -> DashboardAction {
        let Some(row) = self.selected_row() else {
            return DashboardAction::Redraw;
        };
        if self.editing_disabled() {
            return DashboardAction::Redraw;
        }
        let Some(index) = row
            .id
            .strip_prefix("picker_roots[")
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|index| index.parse().ok())
        else {
            self.settings_editor.notice =
                Some("Move up and Move down apply to picker roots".into());
            return DashboardAction::Redraw;
        };
        let roots: Vec<String> = self
            .settings_report
            .as_ref()
            .expect("editing is off without a reading")
            .settings
            .picker_roots
            .iter()
            .map(|root| root.display().to_string())
            .collect();
        match reordered_roots(&roots, index, delta) {
            Some(value) => self.send_setting(&row.id, "picker_roots".into(), Some(value)),
            None => {
                self.settings_editor.notice = Some(
                    if delta < 0 {
                        "Already the first picker root"
                    } else {
                        "Already the last picker root"
                    }
                    .into(),
                );
                DashboardAction::Redraw
            }
        }
    }

    fn reset_or_remove(&mut self, remove: bool) -> DashboardAction {
        let Some(row) = self.selected_row() else {
            return DashboardAction::Redraw;
        };
        if self.editing_disabled() {
            return DashboardAction::Redraw;
        }
        match (&row.reset, row.child == remove) {
            (Some((path, value)), true) => self.send_setting(&row.id, path.clone(), value.clone()),
            _ => {
                self.settings_editor.notice = Some(match (row.child, remove) {
                    (false, false) => format!("{} is already its default", row.label),
                    (false, true) => "Remove applies to a collection's child rows; r resets".into(),
                    (true, false) => "Reset applies to a whole setting; x removes this one".into(),
                    (true, true) => "Nothing to remove here".into(),
                });
                DashboardAction::Redraw
            }
        }
    }

    fn start_edit(&mut self) -> DashboardAction {
        let Some(row) = self.selected_row() else {
            return DashboardAction::Redraw;
        };
        if self.editing_disabled() {
            return DashboardAction::Redraw;
        }
        self.settings_editor.selected = Some(row.id.clone());
        let kind = if row.id.starts_with("launch_choices.") {
            // Presets come from PATH, so only list them when asked to.
            let presets = self.launch_presets();
            let report = self.settings_report.as_deref().expect("checked above");
            items(report, &presets)
                .into_iter()
                .find(|found| found.id == row.id)
                .map_or(row.kind.clone(), |found| found.kind)
        } else {
            row.kind.clone()
        };
        let text = if row.id.ends_with('+') || row.value == "unset" {
            String::new()
        } else {
            row.value.clone()
        };
        let mut edit = Edit {
            row: row.id.clone(),
            text,
            picker: None,
            pick: None,
        };
        match kind {
            Kind::Fixed => {
                self.settings_editor.notice =
                    Some("Edit the rows below; r resets the whole list".into());
                return DashboardAction::Redraw;
            }
            Kind::Toggle(on) => {
                return self.send_setting(&row.id, row.path, Some((!on).to_string()));
            }
            Kind::Pick(options) => {
                let mut options = options;
                let mut sections = Vec::new();
                if row.id == ThemeId::KEY {
                    options.sort_by_key(|item| {
                        ThemeId::ALL
                            .iter()
                            .position(|theme| theme.label() == item.label)
                            .map(|index| {
                                let theme = ThemeId::ALL[index];
                                let appearance = match theme.appearance() {
                                    ThemeAppearance::Dark => 0,
                                    ThemeAppearance::Light => 1,
                                };
                                (appearance, index)
                            })
                            .unwrap_or((usize::MAX, usize::MAX))
                    });
                    sections = options
                        .iter()
                        .filter_map(|item| {
                            let theme = ThemeId::ALL
                                .iter()
                                .find(|theme| theme.label() == item.label)?;
                            let section = match theme.appearance() {
                                ThemeAppearance::Dark => "Dark",
                                ThemeAppearance::Light => "Light",
                            };
                            Some((item.value.clone(), section.to_string()))
                        })
                        .collect();
                }
                let mut list = PickList::new(options);
                if !sections.is_empty() {
                    list = list.with_sections(sections);
                }
                if let Some(index) = list.items.iter().position(|item| item.label == row.value) {
                    list.selected = index;
                }
                edit.pick = Some(list);
                edit.text.clear();
            }
            Kind::Path => edit.picker = Some(PathPicker::new()),
            Kind::Text | Kind::Toml => {}
        }
        self.settings_editor.cursor = TextCursor::default();
        self.settings_editor.edit = Some(edit);
        if row.id == ThemeId::KEY {
            // A new draft owns its preview; older requests still report their
            // outcome on the row, but cannot clear the new highlight.
            self.settings_editor.preview_request = None;
            self.update_theme_preview();
        }
        DashboardAction::Redraw
    }

    fn edit_key(&mut self, key: KeyEvent) -> DashboardAction {
        let roots = self.settings.picker_roots.clone();
        let editor = &mut self.settings_editor;
        let edit = editor.edit.as_mut().expect("editing");
        match (key.code, &mut edit.pick, &mut edit.picker) {
            (KeyCode::Esc, ..) => editor.edit = None,
            (KeyCode::Enter, ..) => return self.commit_edit(),
            (KeyCode::Up | KeyCode::Down, Some(list), _) => {
                list.move_selection(if key.code == KeyCode::Up { -1 } else { 1 })
            }
            (KeyCode::PageUp | KeyCode::PageDown, Some(list), _) => {
                list.move_selection(if key.code == KeyCode::PageUp {
                    -(PICK_VISIBLE_ROWS as isize)
                } else {
                    PICK_VISIBLE_ROWS as isize
                })
            }
            (KeyCode::Home | KeyCode::End, Some(list), _) => {
                list.move_selection(if key.code == KeyCode::Home {
                    isize::MIN / 2
                } else {
                    isize::MAX / 2
                })
            }
            (KeyCode::Up | KeyCode::Down, None, Some(picker)) => {
                let listing = picker.listing(&edit.text, &roots).clone();
                picker.move_selection(&listing, if key.code == KeyCode::Up { -1 } else { 1 });
            }
            (KeyCode::Tab, None, Some(picker)) => {
                if let Some(done) =
                    super::picker::complete_path(&edit.text, &roots, picker.selected)
                {
                    edit.text = done;
                    picker.selected = 0;
                    editor.cursor = TextCursor::default();
                }
            }
            (KeyCode::Char(ch), list, picker) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                match list {
                    Some(list) => {
                        editor.cursor.insert(&mut list.query, &ch.to_string());
                        list.selected = 0;
                    }
                    None => editor.cursor.insert(&mut edit.text, &ch.to_string()),
                }
                if let Some(picker) = picker {
                    picker.selected = 0;
                }
            }
            (code, Some(list), _) => {
                editor.cursor.key(&mut list.query, code);
                list.selected = 0;
            }
            (code, None, _) => editor.cursor.key(&mut edit.text, code),
        }
        if let Some(edit) = &mut self.settings_editor.edit
            && let Some(picker) = &mut edit.picker
        {
            picker.listing(&edit.text, &roots);
        }
        self.update_theme_preview();
        DashboardAction::Redraw
    }

    /// Follow the highlighted theme option without sending a settings
    /// request. No selection means the persisted theme is shown again.
    fn update_theme_preview(&mut self) {
        let candidate = self.settings_editor.edit.as_ref().and_then(|edit| {
            if edit.row != ThemeId::KEY {
                return None;
            }
            let label = &edit.pick.as_ref()?.accepted()?.label;
            ThemeId::ALL
                .iter()
                .copied()
                .find(|theme| theme.label() == label)
        });
        // Keep the saved theme explicit too: submitting it must remain tracked
        // when an earlier save publishes a different theme afterward.
        self.settings_editor.preview_theme = candidate;
    }

    pub(super) fn settings_editor_paste(&mut self, text: &str) -> DashboardAction {
        let editor = &mut self.settings_editor;
        let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
        match &mut editor.edit {
            Some(Edit {
                pick: Some(list), ..
            }) => {
                editor.cursor.insert(&mut list.query, &text);
                list.selected = 0;
            }
            Some(edit) => editor.cursor.insert(&mut edit.text, &text),
            None if editor.filtering => {
                editor.cursor.insert(&mut editor.filter, &text);
                editor.selected = None;
            }
            None => return DashboardAction::None,
        }
        self.update_theme_preview();
        DashboardAction::Redraw
    }

    fn commit_edit(&mut self) -> DashboardAction {
        let edit = self.settings_editor.edit.take().expect("editing");
        let report = self
            .settings_report
            .as_deref()
            .expect("editing needs a reading");
        let Some(row) = items(report, &[])
            .into_iter()
            .find(|row| row.id == edit.row)
        else {
            // The Server's new reading no longer has this row.
            return DashboardAction::Redraw;
        };
        let value = match (&row.kind, &edit.pick) {
            (_, Some(list)) => match list.accepted() {
                Some(item) => item.value.clone(),
                None => {
                    self.settings_editor.edit = Some(edit);
                    return DashboardAction::Redraw;
                }
            },
            (Kind::Toml, _) => edit.text.trim().to_string(),
            _ => toml_string(edit.text.trim()),
        };
        let text = edit.text.trim();
        let (path, value) = match row.id.as_str() {
            "agents+" if text.is_empty() => return DashboardAction::Redraw,
            "agents+" => (
                row.path.clone(),
                Some(format!(
                    "{{ name = {}, argv = {} }}",
                    toml_string(text),
                    toml_array([text])
                )),
            ),
            "launch_choices+" if text.is_empty() => return DashboardAction::Redraw,
            "launch_choices+" => {
                let request = super::settings::launch_choice_request(text, &LaunchChoice::Terminal);
                let Request::SetSetting { path, value } = request else {
                    unreachable!()
                };
                (path, value)
            }
            // An emptied top-level value goes back to its default.
            _ if !row.child && text.is_empty() && edit.pick.is_none() => (row.path.clone(), None),
            _ => match &row.whole_list {
                Some(list) => (
                    row.path.clone(),
                    Some(format!("[{}]", list.join(", ").replace("{}", &value))),
                ),
                None => (row.path.clone(), Some(value)),
            },
        };
        self.send_setting(&row.id, path, value)
    }

    /// Claims the answer to an editor request: a refusal shows on its row.
    pub(super) fn settings_editor_response(
        &mut self,
        request_id: u64,
        response: &Response,
    ) -> bool {
        let Some(row) = self.settings_editor.pending.remove(&request_id) else {
            return false;
        };
        if let Response::Error { message, .. } = response {
            self.settings_editor.refused.insert(row, message.clone());
            if self.settings_editor.preview_request == Some(request_id) {
                self.settings_editor.preview_request = None;
                self.settings_editor.preview_theme = None;
            }
        }
        true
    }

    pub(super) fn draw_settings_editor(&self, frame: &mut Frame<'_>, now: u64) {
        let outer = frame.area();
        let width = outer.width.saturating_sub(4).min(100);
        let height = outer.height.saturating_sub(2);
        let area = Rect::new(
            outer.x + (outer.width - width) / 2,
            outer.y + (outer.height - height) / 2,
            width,
            height,
        );
        let block = Block::bordered()
            .title(TITLE)
            .style(Style::default().bg(BASE()).fg(TEXT()));
        let inner = block.inner(area);
        let (lines, selected) = self.editor_lines(now, usize::from(inner.width));
        let room = usize::from(inner.height);
        let scroll = selected.map_or(0, |(line, below)| {
            (line + below + 1).saturating_sub(room).min(line)
        });
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(lines).scroll((scroll.min(u16::MAX as usize) as u16, 0)),
            inner,
        );
    }

    /// The view's lines, and the selected row's line with how many lines
    /// below it belong to it.
    fn editor_lines(&self, now: u64, width: usize) -> (Vec<Line<'static>>, Option<(usize, usize)>) {
        let style = |color| Style::default().fg(color);
        let Some(report) = self.settings_report.as_deref() else {
            return (
                vec![Line::raw("Waiting for the Server's settings reading.")],
                None,
            );
        };
        let editor = &self.settings_editor;
        let read = ovrcr_protocol::freshness::age_ms(report.read_unix_ms, now)
            .map(|age| format!("{}s ago", age / 1_000))
            .unwrap_or_else(|| "at an unverifiable time".into());
        let mut lines = vec![Line::styled(
            format!("Document: {} · read {read}", report.path.display()),
            style(SUBTEXT()),
        )];
        if editor.filtering || !editor.filter.is_empty() {
            let (text, _) = if editor.filtering {
                editor
                    .cursor
                    .display(&editor.filter, width.saturating_sub(8))
            } else {
                (editor.filter.clone(), 0)
            };
            lines.push(Line::styled(format!("Filter: {text}"), style(MAUVE())));
        }
        if let Some(notice) = &editor.notice {
            lines.push(Line::styled(notice.clone(), style(PEACH())));
        }
        let loose = loose_findings(report);
        if !loose.is_empty() || report.unparseable {
            lines.push(Line::styled(
                format!("Findings: {}", loose.len()),
                style(YELLOW()).add_modifier(Modifier::BOLD),
            ));
            for finding in loose {
                lines.push(Line::styled(format!("  ! {finding}"), style(YELLOW())));
            }
            if report.unparseable {
                lines.push(Line::styled(format!("  ! {UNPARSEABLE}"), style(YELLOW())));
            }
        }
        let selected_id = self.selected_row().map(|row| row.id);
        let mut selected = None;
        let mut header: Option<&str> = None;
        for row in items(report, &[]) {
            if !matches(&row, &editor.filter) {
                continue;
            }
            if header != Some(row.group) {
                header = Some(row.group);
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    row.group,
                    style(MAUVE()).add_modifier(Modifier::BOLD),
                ));
            }
            let chosen = selected_id.as_ref() == Some(&row.id);
            let start = lines.len();
            let marker = if chosen { "› " } else { "  " };
            let indent = if row.child { "    " } else { "" };
            let editing = editor.edit.as_ref().filter(|edit| edit.row == row.id);
            let mut spans = vec![Span::raw(format!("{marker}{indent}"))];
            if !row.label.is_empty() {
                spans.push(Span::styled(
                    if row.child {
                        format!("{}  ", row.label)
                    } else {
                        format!("{:<27}", row.label)
                    },
                    style(TEXT()),
                ));
            }
            match editing {
                Some(edit) if edit.pick.is_none() => {
                    let room = width
                        .saturating_sub(spans.iter().map(|span| span.width()).sum::<usize>() + 2);
                    let (text, _) = editor.cursor.display(&edit.text, room.max(4));
                    spans.push(Span::styled(format!("[{text}]"), style(MAUVE())));
                }
                _ => {
                    spans.push(Span::styled(row.value.clone(), style(TEXT())));
                    if let Some(source) = row.source {
                        let set = source == SettingSource::Document;
                        spans.push(Span::styled(
                            if set { "  set" } else { "  default" },
                            style(if set { PEACH() } else { MUTED() }),
                        ));
                        if set && row.kind != Kind::Fixed {
                            spans.push(Span::styled(
                                format!(
                                    "  (default: {})",
                                    row.default.as_deref().unwrap_or("unset")
                                ),
                                style(MUTED()),
                            ));
                        }
                    }
                }
            }
            if editor.pending.values().any(|id| id == &row.id) {
                spans.push(Span::styled("  saving…", style(MUTED())));
            }
            let mut line = Line::from(spans);
            if chosen {
                line = line.style(Style::default().bg(SURFACE0()));
            }
            lines.push(line);
            let detail =
                |text: String, color| Line::styled(format!("      {indent}{text}"), style(color));
            // Refusals, findings and descriptions wrap rather than clip.
            let room = width.saturating_sub(6 + indent.len());
            let wrapped = |text: String, color| {
                wrap(&text, room)
                    .into_iter()
                    .map(move |line| detail(line, color))
                    .collect::<Vec<_>>()
            };
            if let Some(edit) = editing {
                if let Some(list) = &edit.pick {
                    if !list.query.is_empty() {
                        lines.push(detail(format!("filter: {}", list.query), MAUVE()));
                    }
                    let (options, _) = list.lines(PICK_VISIBLE_ROWS);
                    lines.extend(options.into_iter().map(|line| {
                        let mut spans = vec![Span::raw(format!("    {indent}"))];
                        spans.extend(
                            line.spans
                                .into_iter()
                                .map(|span| Span::styled(span.content.into_owned(), span.style)),
                        );
                        Line::from(spans).style(line.style)
                    }));
                } else if let Some(listing) = edit.picker.as_ref().and_then(PathPicker::cached) {
                    let picker = edit.picker.as_ref().unwrap();
                    for (index, entry) in listing.entries.iter().enumerate().take(5) {
                        let mark = if index == picker.selected { "›" } else { " " };
                        lines.push(detail(format!("{mark} {}", entry.label), SUBTEXT()));
                    }
                }
                lines.extend(wrapped(
                    match (&row.kind, edit.pick.is_some()) {
                        (_, true) => {
                            "↑↓ choose · PgUp/PgDn page · Home/End · Enter save · Esc cancel".into()
                        }
                        (Kind::Toml, _) => {
                            "TOML array, e.g. [\"claude\", \"--verbose\"] · Enter save · Esc cancel"
                                .into()
                        }
                        (Kind::Path, _) => "Tab complete · Enter save · Esc cancel".into(),
                        _ => "Enter save · Esc cancel".into(),
                    },
                    MUTED(),
                ));
            }
            if let Some(refused) = editor.refused.get(&row.id) {
                lines.extend(wrapped(format!("refused: {refused}"), RED()));
            }
            if let Some(finding) = &row.finding {
                lines.extend(wrapped(format!("! {finding}"), YELLOW()));
            }
            for about in &row.about {
                lines.extend(wrapped(about.clone(), SUBTEXT()));
            }
            if chosen && !row.child && !row.path.is_empty() {
                lines.push(detail(format!("key: {}", row.path), MUTED()));
            }
            if chosen {
                selected = Some((start, lines.len() - start - 1));
            }
        }
        if header.is_none() {
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "No setting matches the filter.",
                style(MUTED()),
            ));
        }
        (lines, selected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{SettingOwner, SettingRow, Settings};

    fn report(
        settings: Settings,
        rows: Vec<(&str, Option<&str>, SettingSource)>,
    ) -> SettingsReport {
        SettingsReport {
            path: "/d.toml".into(),
            read_unix_ms: 0,
            settings,
            rows: rows
                .into_iter()
                .map(|(key, value, source)| SettingRow {
                    key: key.into(),
                    owner: SettingOwner::Dashboard,
                    value: value.map(Into::into),
                    source,
                    default: None,
                    off_state: None,
                })
                .collect(),
            findings: Vec::new(),
            unparseable: false,
        }
    }

    fn row(items: &[Row], id: &str) -> Row {
        items
            .iter()
            .find(|row| row.id == id)
            .cloned()
            .unwrap_or_else(|| panic!("no row {id}"))
    }

    #[test]
    fn iterm_focus_is_a_separate_default_off_setting_and_does_not_enable_alerts() {
        let settings = Settings::default();
        assert!(!settings.iterm_focus);
        let rows = items(&report(settings, Vec::new()), &[]);
        let focus = row(&rows, "iterm_focus");
        assert_eq!(focus.value, "off");
        assert_eq!(focus.kind, Kind::Toggle(false));
        assert_eq!(focus.path, "iterm_focus");
        assert!(
            focus
                .about
                .iter()
                .any(|line| line.contains("Banner clicks never request it"))
        );
        assert_eq!(
            row(&rows, "desktop_notifications").kind,
            Kind::Toggle(false)
        );
        assert_eq!(row(&rows, "ready_sound").kind, Kind::Toggle(false));
    }

    #[test]
    fn groups_come_in_the_documented_order() {
        let mut headers: Vec<_> = items(&report(Settings::default(), Vec::new()), &[])
            .into_iter()
            .map(|row| row.group)
            .collect();
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
    }

    /// A default list is not in the document, so editing one of its roots
    /// writes the whole list rather than an index the document lacks.
    #[test]
    fn default_picker_roots_are_written_whole_and_set_ones_by_index() {
        let settings = Settings {
            picker_roots: vec!["/a".into(), "/b".into()],
            ..Default::default()
        };
        let default = items(&report(settings.clone(), Vec::new()), &[]);
        let second = row(&default, "picker_roots[1]");
        assert_eq!(second.path, "picker_roots");
        assert_eq!(
            second.reset,
            Some(("picker_roots".into(), Some("[\"/a\"]".into())))
        );
        assert_eq!(
            row(&default, "picker_roots+").whole_list,
            Some(vec!["\"/a\"".into(), "\"/b\"".into(), "{}".into()])
        );
        let set = items(
            &report(
                settings,
                vec![("picker_roots", Some("[]"), SettingSource::Document)],
            ),
            &[],
        );
        let second = row(&set, "picker_roots[1]");
        assert_eq!(second.path, "picker_roots[1]");
        assert_eq!(second.reset, Some(("picker_roots[1]".into(), None)));
        assert_eq!(row(&set, "picker_roots+").path, "picker_roots[2]");
    }

    #[test]
    fn save_uncommitted_work_row_names_the_consent_sentence() {
        let row = row(
            &items(&report(Settings::default(), Vec::new()), &[]),
            "save_uncommitted_work",
        );
        assert_eq!(row.value, "off");
        assert!(
            row.about
                .iter()
                .any(|line| line == ovrcr_protocol::SAVE_UNCOMMITTED_WORK_OFF),
            "{:?}",
            row.about
        );
    }

    #[test]
    fn claude_probe_row_names_the_consent_sentence() {
        let row = row(
            &items(&report(Settings::default(), Vec::new()), &[]),
            "quota.claude.probe",
        );
        assert_eq!(row.value, "off");
        assert!(
            row.about
                .iter()
                .any(|line| line == ovrcr_protocol::CLAUDE_PROBE_DESCRIPTION),
            "{:?}",
            row.about
        );
    }

    #[test]
    fn long_details_wrap_at_word_boundaries() {
        assert_eq!(wrap("refused: a bc def", 8), ["refused:", "a bc def"]);
        assert_eq!(wrap("short", 80), ["short"]);
        assert_eq!(wrap("toolongword x", 4), ["toolongword", "x"]);
    }

    fn editor(document: bool) -> super::super::Dashboard {
        use ovrcr_protocol::{AgentOverride, ServerEvent, ServerMessage, TerminalSize};
        let settings = Settings {
            picker_roots: vec!["/a".into(), "/b".into(), "/c".into()],
            agents: vec![
                AgentOverride {
                    name: "one".into(),
                    argv: vec!["one".into()],
                },
                AgentOverride {
                    name: "two".into(),
                    argv: vec!["two".into()],
                },
            ],
            ..Settings::default()
        };
        let rows = if document {
            vec![(
                "picker_roots",
                Some("[\"/a\", \"/b\", \"/c\"]"),
                SettingSource::Document,
            )]
        } else {
            Vec::new()
        };
        let mut dashboard = super::super::Dashboard::new(TerminalSize {
            rows: 40,
            cols: 120,
        });
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
            Box::new(report(settings, rows)),
        )));
        dashboard.open_details(super::super::quota::Details::Settings);
        dashboard
    }

    fn setting_write(action: DashboardAction) -> (String, Option<String>) {
        match action {
            DashboardAction::Request(ClientMessage {
                request: Request::SetSetting { path, value },
                ..
            }) => (path, value),
            other => panic!("expected SetSetting, got {other:?}"),
        }
    }

    fn choose_next_theme(dashboard: &mut super::super::Dashboard) -> ThemeId {
        dashboard.settings_editor.selected = Some(ThemeId::KEY.into());
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Down);
        dashboard.settings_editor.preview_theme.unwrap()
    }

    #[test]
    fn theme_picker_page_and_boundary_keys_preview_without_saving() {
        let mut d = editor(false);
        d.settings_editor.selected = Some(ThemeId::KEY.into());
        d.key(KeyCode::Enter);
        let list = d
            .settings_editor
            .edit
            .as_ref()
            .unwrap()
            .pick
            .as_ref()
            .unwrap();
        let choices: Vec<_> = list
            .items
            .iter()
            .map(|item| {
                ThemeId::ALL
                    .iter()
                    .copied()
                    .find(|theme| theme.label() == item.label)
                    .unwrap()
            })
            .collect();
        assert_eq!(choices.len(), 80);
        for (key, index) in [
            (KeyCode::PageDown, 6),
            (KeyCode::PageDown, 12),
            (KeyCode::PageUp, 6),
            (KeyCode::End, 79),
            (KeyCode::PageDown, 79),
            (KeyCode::Home, 0),
            (KeyCode::PageUp, 0),
        ] {
            assert!(matches!(d.key(key), DashboardAction::Redraw));
            assert_eq!(
                d.theme_tokens(),
                crate::theme::ThemeTokens::for_id(choices[index]),
                "{key:?}"
            );
            assert_eq!(d.settings.theme, ThemeId::Dark);
            assert!(d.settings_editor.pending.is_empty());
            assert!(d.drain_outbox().is_empty());
        }
        d.key(KeyCode::Esc);
        assert_eq!(d.settings_editor.preview_theme, None);
        assert_eq!(d.theme_tokens(), crate::theme::ThemeTokens::dark());
    }

    #[test]
    fn theme_picker_pasted_filter_previews_and_no_matches_restores_saved_theme() {
        let mut d = editor(false);
        d.settings_editor.selected = Some(ThemeId::KEY.into());
        d.key(KeyCode::Enter);
        d.settings_editor_paste("Solarized Light");
        assert_eq!(
            d.settings_editor.preview_theme,
            Some(ThemeId::SolarizedLight)
        );
        assert_eq!(
            d.theme_tokens(),
            crate::theme::ThemeTokens::for_id(ThemeId::SolarizedLight)
        );
        assert_eq!(d.settings.theme, ThemeId::Dark);
        d.settings_editor_paste("no-such-theme");
        assert_eq!(d.settings_editor.preview_theme, None);
        assert_eq!(d.theme_tokens(), crate::theme::ThemeTokens::dark());
        assert!(d.settings_editor.pending.is_empty());
        assert!(d.drain_outbox().is_empty());
    }

    #[test]
    fn theme_picker_groups_dark_then_light_and_mouse_wheel_previews() {
        use crossterm::event::{MouseEvent, MouseEventKind};
        use ovrcr_protocol::ThemeAppearance;

        let mut d = editor(false);
        d.settings_editor.selected = Some(ThemeId::KEY.into());
        d.key(KeyCode::Enter);
        let list = d
            .settings_editor
            .edit
            .as_ref()
            .unwrap()
            .pick
            .as_ref()
            .unwrap();
        let labels: Vec<_> = list.items.iter().map(|item| item.label.as_str()).collect();
        let first_light = labels
            .iter()
            .position(|label| {
                ThemeId::ALL
                    .iter()
                    .find(|theme| theme.label() == *label)
                    .is_some_and(|theme| theme.appearance() == ThemeAppearance::Light)
            })
            .unwrap();
        assert_eq!(labels.len(), 80);
        assert_eq!(first_light, 54);
        assert_eq!(labels.len() - first_light, 26);
        assert!(labels[..first_light].iter().all(|label| {
            ThemeId::ALL
                .iter()
                .find(|theme| theme.label() == *label)
                .is_some_and(|theme| theme.appearance() == ThemeAppearance::Dark)
        }));
        assert!(labels[first_light..].iter().all(|label| {
            ThemeId::ALL
                .iter()
                .find(|theme| theme.label() == *label)
                .is_some_and(|theme| theme.appearance() == ThemeAppearance::Light)
        }));

        // Settings wheel input follows the same selection path as Down/Up.
        d.details_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(d.settings_editor.preview_theme, Some(ThemeId::TokyoNight));
        d.details_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(d.settings_editor.preview_theme, Some(ThemeId::Dark));
        assert_eq!(d.theme_tokens(), crate::theme::ThemeTokens::dark());
    }

    #[test]
    fn theme_picker_preview_is_transient_until_server_confirms_the_save() {
        use ovrcr_protocol::{ServerEvent, ServerMessage};

        let mut d = editor(false);
        assert_eq!(d.settings.theme, ThemeId::Dark);
        assert_eq!(d.theme_tokens(), crate::theme::ThemeTokens::dark());

        // Repeated navigation previews the current highlighted option, then
        // returns to the persisted theme when the user moves back to it.
        d.settings_editor.selected = Some(ThemeId::KEY.into());
        d.key(KeyCode::Enter);
        d.key(KeyCode::Down);
        let first = d.settings_editor.preview_theme.unwrap();
        assert_eq!(d.theme_tokens(), crate::theme::ThemeTokens::for_id(first));
        d.key(KeyCode::Down);
        let second = d.settings_editor.preview_theme.unwrap();
        assert_ne!(first, second);
        d.key(KeyCode::Up);
        assert_eq!(d.settings_editor.preview_theme, Some(first));
        d.key(KeyCode::Up);
        assert_eq!(d.settings_editor.preview_theme, Some(ThemeId::Dark));
        assert_eq!(d.theme_tokens(), crate::theme::ThemeTokens::dark());

        // Escape cancels a preview; reopening starts clean from the saved value.
        d.key(KeyCode::Down);
        d.key(KeyCode::Esc);
        assert_eq!(d.settings_editor.preview_theme, None);
        assert_eq!(d.theme_tokens(), crate::theme::ThemeTokens::dark());
        d.open_details(super::super::quota::Details::Settings);
        assert_eq!(d.settings_editor.preview_theme, None);

        // Enter sends the ordinary SetSetting request without optimistic
        // mutation; the transient preview remains until the Server reading.
        let candidate = choose_next_theme(&mut d);
        let DashboardAction::Request(ClientMessage {
            request_id,
            request: Request::SetSetting { path, value },
        }) = d.key(KeyCode::Enter)
        else {
            panic!("expected theme setting request");
        };
        assert_eq!(path, ThemeId::KEY);
        assert_eq!(value, Some(toml_string(candidate.as_str())));
        assert_eq!(d.settings.theme, ThemeId::Dark);
        assert_eq!(
            d.theme_tokens(),
            crate::theme::ThemeTokens::for_id(candidate)
        );

        // Another persisted setting update cannot overwrite an active preview.
        let mut external = d.settings.clone();
        external.theme = ThemeId::Light;
        d.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
            Box::new(report(external, Vec::new())),
        )));
        assert_eq!(d.settings.theme, ThemeId::Light);
        assert_eq!(
            d.theme_tokens(),
            crate::theme::ThemeTokens::for_id(candidate)
        );

        // A refused save drops only the preview and restores the latest
        // persisted value, including an intervening external change.
        d.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Error {
                code: ovrcr_protocol::ErrorCode::InvalidRequest,
                message: "fixture save refused".into(),
            },
        });
        assert_eq!(d.settings_editor.preview_theme, None);
        assert_eq!(d.theme_tokens(), crate::theme::ThemeTokens::light());

        // A new successful save is confirmed by the next SettingsChanged.
        d.open_details(super::super::quota::Details::Settings);
        d.settings_editor.selected = Some(ThemeId::KEY.into());
        d.key(KeyCode::Enter);
        // Select a value other than the persisted Light choice.
        let light_index = d
            .settings_editor
            .edit
            .as_ref()
            .unwrap()
            .pick
            .as_ref()
            .unwrap()
            .items
            .iter()
            .position(|item| item.label == ThemeId::Light.label())
            .unwrap();
        d.key(if light_index > 0 {
            KeyCode::Up
        } else {
            KeyCode::Down
        });
        let candidate = d.settings_editor.preview_theme.unwrap();
        let DashboardAction::Request(ClientMessage { .. }) = d.key(KeyCode::Enter) else {
            panic!("expected theme setting request");
        };
        let mut saved = d.settings.clone();
        saved.theme = candidate;
        d.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
            Box::new(report(saved, Vec::new())),
        )));
        assert_eq!(d.settings.theme, candidate);
        assert_eq!(d.settings_editor.preview_theme, None);
        assert_eq!(d.settings_editor.preview_request, None);
        assert_eq!(
            d.theme_tokens(),
            crate::theme::ThemeTokens::for_id(candidate)
        );

        // Closing Settings also removes any uncommitted preview.
        d.open_details(super::super::quota::Details::Settings);
        d.settings_editor.selected = Some(ThemeId::KEY.into());
        d.key(KeyCode::Enter);
        d.key(KeyCode::Down);
        assert!(d.settings_editor.preview_theme.is_some());
        d.key(KeyCode::Esc);
        assert!(d.details.is_some(), "first Escape cancels only the picker");
        assert_eq!(d.settings_editor.preview_theme, None);
        d.key(KeyCode::Esc);
        assert!(d.details.is_none());
        assert_eq!(d.settings_editor.preview_theme, None);
        assert_eq!(
            d.theme_tokens(),
            crate::theme::ThemeTokens::for_id(candidate)
        );
    }

    #[test]
    fn external_theme_update_during_browsing_becomes_the_cancel_restore_target() {
        use ovrcr_protocol::{ServerEvent, ServerMessage};

        let mut d = editor(false);
        let candidate = choose_next_theme(&mut d);
        assert_eq!(d.settings.theme, ThemeId::Dark);
        assert_eq!(
            d.theme_tokens(),
            crate::theme::ThemeTokens::for_id(candidate)
        );

        let mut external = d.settings.clone();
        external.theme = ThemeId::Light;
        d.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
            Box::new(report(external, Vec::new())),
        )));
        assert_eq!(d.settings.theme, ThemeId::Light);
        assert_eq!(
            d.theme_tokens(),
            crate::theme::ThemeTokens::for_id(candidate),
            "the in-progress highlight remains the preview"
        );

        d.key(KeyCode::Esc);
        assert_eq!(d.settings_editor.preview_theme, None);
        assert_eq!(d.theme_tokens(), crate::theme::ThemeTokens::light());
    }

    #[test]
    fn non_theme_setting_failure_does_not_clear_a_theme_preview_save() {
        use ovrcr_protocol::ServerMessage;

        let mut d = editor(false);
        let candidate = choose_next_theme(&mut d);
        let DashboardAction::Request(ClientMessage {
            request_id: theme_request,
            ..
        }) = d.key(KeyCode::Enter)
        else {
            panic!("expected theme setting request");
        };
        assert_eq!(d.settings_editor.preview_request, Some(theme_request));

        d.settings_editor.selected = Some("desktop_notifications".into());
        let DashboardAction::Request(ClientMessage {
            request_id: other_request,
            request: Request::SetSetting { path, .. },
        }) = d.key(KeyCode::Enter)
        else {
            panic!("expected non-theme setting request");
        };
        assert_eq!(path, "desktop_notifications");
        d.handle_server_message(ServerMessage::Response {
            request_id: other_request,
            response: Response::Error {
                code: ovrcr_protocol::ErrorCode::InvalidRequest,
                message: "unrelated setting refused".into(),
            },
        });
        assert_eq!(d.settings_editor.preview_request, Some(theme_request));
        assert_eq!(d.settings_editor.preview_theme, Some(candidate));
        assert_eq!(
            d.theme_tokens(),
            crate::theme::ThemeTokens::for_id(candidate)
        );
        assert!(!d.settings.desktop_notifications);
    }

    #[test]
    fn move_up_and_move_down_reorder_only_picker_roots() {
        for document in [true, false] {
            let mut dashboard = editor(document);
            dashboard.settings_editor.selected = Some("picker_roots[1]".into());
            let (path, value) = setting_write(dashboard.key(KeyCode::Char(']')));
            assert_eq!(path, "picker_roots", "document={document}");
            assert_eq!(
                value.as_deref(),
                Some("[\"/a\", \"/c\", \"/b\"]"),
                "move down, document={document}"
            );
            let (path, value) = setting_write(dashboard.key(KeyCode::Char('[')));
            assert_eq!(path, "picker_roots");
            assert_eq!(
                value.as_deref(),
                Some("[\"/b\", \"/a\", \"/c\"]"),
                "move up, document={document}"
            );

            dashboard.settings_editor.selected = Some("picker_roots[0]".into());
            assert!(matches!(
                dashboard.key(KeyCode::Char('[')),
                DashboardAction::Redraw
            ));
            assert_eq!(
                dashboard.settings_editor.notice.as_deref(),
                Some("Already the first picker root")
            );
            dashboard.settings_editor.selected = Some("picker_roots[2]".into());
            assert!(matches!(
                dashboard.key(KeyCode::Char(']')),
                DashboardAction::Redraw
            ));
            assert_eq!(
                dashboard.settings_editor.notice.as_deref(),
                Some("Already the last picker root")
            );

            dashboard.settings_editor.selected = Some("agents[1]".into());
            assert!(matches!(
                dashboard.key(KeyCode::Char('[')),
                DashboardAction::Redraw
            ));
            assert_eq!(
                dashboard.settings_editor.notice.as_deref(),
                Some("Move up and Move down apply to picker roots")
            );
            assert!(matches!(
                dashboard.key(KeyCode::Char(']')),
                DashboardAction::Redraw
            ));
            assert_eq!(
                dashboard.settings_editor.notice.as_deref(),
                Some("Move up and Move down apply to picker roots")
            );
        }
    }

    #[test]
    fn filter_matches_every_word_across_groups() {
        let items = items(&report(Settings::default(), Vec::new()), &[]);
        let hits: Vec<_> = items
            .iter()
            .filter(|row| matches(row, "usage HOME"))
            .map(|row| row.id.as_str())
            .collect();
        assert_eq!(hits, ["quota.codex.home", "quota.grok.home"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sound_choice_editor_shows_four_labels_and_waits_for_server_save() {
        use ovrcr_protocol::{ErrorCode, ServerEvent, ServerMessage, TerminalSize};
        for (index, choice) in [
            ReadySoundChoice::Default,
            ReadySoundChoice::Tap,
            ReadySoundChoice::Chime,
            ReadySoundChoice::Rise,
        ]
        .into_iter()
        .enumerate()
        {
            let mut d = Dashboard::new(TerminalSize {
                rows: 40,
                cols: 160,
            });
            let settings = Settings {
                ready_sound_choice: Some(ReadySoundChoice::Chime),
                ..Default::default()
            };
            let reading = report(
                settings.clone(),
                vec![(
                    ReadySoundChoice::KEY,
                    Some("chime"),
                    SettingSource::Document,
                )],
            );
            d.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
                Box::new(reading.clone()),
            )));
            for code in [KeyCode::Char(' '), KeyCode::Char('v'), KeyCode::Char(',')] {
                d.key(code);
            }
            assert!(
                matches!(d.details, Some((super::super::quota::Details::Settings, _))),
                "Settings opens through the real menu path"
            );
            d.key(KeyCode::Char('/'));
            for ch in ReadySoundChoice::KEY.chars() {
                d.key(KeyCode::Char(ch));
            }
            d.key(KeyCode::Enter);
            assert_eq!(d.selected_row().unwrap().value, "Chime");
            d.key(KeyCode::Enter);
            let list = d
                .settings_editor
                .edit
                .as_ref()
                .unwrap()
                .pick
                .as_ref()
                .unwrap();
            assert_eq!(
                list.items
                    .iter()
                    .map(|item| item.label.as_str())
                    .collect::<Vec<_>>(),
                ["System default", "Tap", "Chime", "Rise"]
            );
            assert_eq!(list.selected, 2, "current accepted value is selected");
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
            d.draw(&mut terminal).unwrap();
            let drawn: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            for label in ["System default", "Tap", "Chime", "Rise"] {
                assert!(drawn.contains(label));
            }
            let arrows = if index < 2 {
                KeyCode::Up
            } else {
                KeyCode::Down
            };
            for _ in 0..index.abs_diff(2) {
                d.key(arrows);
            }
            let DashboardAction::Request(ClientMessage {
                request_id,
                request: Request::SetSetting { path, value },
            }) = d.key(KeyCode::Enter)
            else {
                panic!("expected Server setting request");
            };
            assert_eq!(path, ReadySoundChoice::KEY);
            assert_eq!(value, Some(toml_string(choice.as_str())));
            assert_eq!(
                d.settings, settings,
                "selection never changes preferences optimistically"
            );
            d.handle_server_message(ServerMessage::Response {
                request_id,
                response: Response::Error {
                    code: ErrorCode::InvalidRequest,
                    message: "fixture save refused".into(),
                },
            });
            assert_eq!(
                d.settings, settings,
                "failed save leaves flags and choice unchanged"
            );
            assert_eq!(
                d.settings_editor
                    .refused
                    .get(ReadySoundChoice::KEY)
                    .map(String::as_str),
                Some("fixture save refused")
            );
            let mut accepted = reading;
            accepted.settings.ready_sound_choice = Some(choice);
            accepted.rows[0].value = Some(choice.as_str().into());
            d.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
                Box::new(accepted),
            )));
            assert_eq!(d.settings.ready_sound_choice, Some(choice));
            assert!(
                !d.settings.desktop_notifications && !d.settings.ready_sound,
                "choice saves never opt in"
            );
            assert!(
                !d.emit_desktop_notifications(),
                "selector with both flags off does not start alert work"
            );
        }
    }

    #[test]
    fn sound_choice_invalid_reading_keeps_findings_and_platform_scope() {
        let settings = Settings {
            ready_sound_choice: None,
            ..Default::default()
        };
        let mut reading = report(
            settings,
            vec![("ready_sound_choice", None, SettingSource::Document)],
        );
        reading.rows[0].default = Some("default".into());
        reading.findings.push(ovrcr_protocol::SettingsFinding {
            key: Some("ready_sound_choice".into()),
            message: "unrecognized saved sound choice".into(),
            line: Some(3),
        });
        let rows = items(&reading, &[]);
        #[cfg(target_os = "macos")]
        {
            let choice = row(&rows, "ready_sound_choice");
            assert_eq!(choice.value, "invalid");
            assert_eq!(choice.source, Some(SettingSource::Document));
            assert_eq!(choice.default.as_deref(), Some("System default"));
            assert_eq!(
                choice.finding.as_deref(),
                Some("line 3: unrecognized saved sound choice")
            );
            assert_eq!(choice.reset, Some(("ready_sound_choice".into(), None)));
            assert!(
                choice
                    .about
                    .iter()
                    .any(|line| line.contains("banners silent"))
            );
        }
        #[cfg(not(target_os = "macos"))]
        assert!(
            rows.iter().all(|row| row.id != "ready_sound_choice"),
            "Linux keeps the setting out of its editor"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sound_choice_invalid_finding_renders_recovery_in_browse_and_stays_editable() {
        use ovrcr_protocol::{ServerEvent, ServerMessage, TerminalSize};
        for attach in [true, false] {
            for (notifications, sound) in
                [(true, true), (true, false), (false, true), (false, false)]
            {
                let mut d = Dashboard::new(TerminalSize {
                    rows: 24,
                    cols: 120,
                });
                let settings = Settings {
                    desktop_notifications: notifications,
                    ready_sound: sound,
                    ready_sound_choice: Some(ReadySoundChoice::Tap),
                    ..Default::default()
                };
                if !attach {
                    d.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
                        Box::new(report(settings.clone(), Vec::new())),
                    )));
                }
                let mut reading = report(
                    Settings {
                        ready_sound_choice: None,
                        ..settings
                    },
                    vec![(ReadySoundChoice::KEY, None, SettingSource::Document)],
                );
                reading.rows[0].default = Some("default".into());
                reading.findings.push(ovrcr_protocol::SettingsFinding {
                    key: Some(ReadySoundChoice::KEY.into()),
                    message: "unrecognized saved sound choice".into(),
                    line: Some(3),
                });
                d.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
                    Box::new(reading.clone()),
                )));
                assert_eq!(d.mode, super::super::InputMode::Browse);
                let mut terminal =
                    ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
                d.draw(&mut terminal).unwrap();
                let footer: String = (0..120)
                    .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
                    .collect();
                let expected = if notifications && sound {
                    "Ready sound choice invalid; banners are silent. Choose a sound in Settings"
                } else {
                    "1 settings finding; see Settings"
                };
                assert!(
                    footer.contains(expected),
                    "attach={attach}, N={notifications}, S={sound}: {footer}"
                );
                assert_eq!(
                    d.settings_report.as_deref(),
                    Some(&reading),
                    "guidance preserves the full Server finding and invalid row"
                );
                assert!(
                    !d.editing_disabled(),
                    "a per-setting finding does not disable the editor"
                );
                for code in [
                    KeyCode::Char(' '),
                    KeyCode::Char('v'),
                    KeyCode::Char(','),
                    KeyCode::Char('/'),
                ] {
                    d.key(code);
                }
                for ch in ReadySoundChoice::KEY.chars() {
                    d.key(KeyCode::Char(ch));
                }
                d.key(KeyCode::Enter);
                let row = d.selected_row().unwrap();
                assert_eq!(row.value, "invalid");
                assert_eq!(
                    row.finding.as_deref(),
                    Some("line 3: unrecognized saved sound choice")
                );
                d.key(KeyCode::Enter);
                assert!(
                    d.settings_editor.edit.as_ref().unwrap().pick.is_some(),
                    "valid choices remain editable through the real menu path"
                );
                assert_eq!(
                    d.settings.ready_sound_choice, None,
                    "opening the picker does not repair or overwrite the invalid choice"
                );
                assert_eq!(
                    (d.settings.desktop_notifications, d.settings.ready_sound),
                    (notifications, sound)
                );
            }
        }
    }
}

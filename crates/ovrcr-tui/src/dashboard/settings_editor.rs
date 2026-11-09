//! The Settings view: the Server's reading as a category sidebar, one stable
//! row per setting, and a fixed details area.
//!
//! Every edit is one `SetSetting` request; nothing changes here until the
//! Server's next reading arrives, and a refusal shows on the row it came from.
//! The Server owns the document (ADR 0007). Opened from the menu and the
//! palette (`Details::Settings`).
use super::picker::{PathPicker, PickItem};
use super::render::{
    BASE, CRUST, MAUVE, MUTED, PEACH, RED, SUBTEXT, SURFACE0, SURFACE2, TEXT, YELLOW, clip_text,
};
use super::settings::LaunchChoice;
use super::text_cursor::TextCursor;
use super::{Dashboard, DashboardAction};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
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
    widgets::{Block, Clear, Paragraph, Wrap},
};
use std::cell::Cell;
use std::collections::HashMap;

const UNPARSEABLE: &str = "Editing is off until dashboard.toml is fixed by hand.";
const WAITING: &str = "Waiting for the Server's settings reading.";
const TOO_SMALL: &str = "Settings needs a larger terminal";
const RESIZE_HINT: &str = "Resize to at least 48 columns × 8 rows.";
const MIN_WIDTH: u16 = 48;
const MIN_HEIGHT: u16 = 8;
const WIDE_WIDTH: u16 = 80;
const PICK_VISIBLE_ROWS: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Focus {
    #[default]
    Categories,
    List,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Collection {
    Paths,
    Agents,
    Launches,
}

impl Collection {
    fn add_label(self) -> &'static str {
        match self {
            Self::Paths => "+ Add folder",
            Self::Agents => "+ Add agent",
            Self::Launches => "+ Add project",
        }
    }

    fn add_id(self) -> &'static str {
        match self {
            Self::Paths => "picker_roots+",
            Self::Agents => "agents+",
            Self::Launches => "launch_choices+",
        }
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
    /// A typed TOML value with its catalog description retained for editing.
    Typed(&'static str),
    /// A compact summary that opens its collection manager.
    Collection(Collection),
    /// No in-place edit.
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

impl Row {
    fn action(&self) -> &'static str {
        match &self.kind {
            Kind::Toggle(on) => {
                if *on {
                    "turn off"
                } else {
                    "turn on"
                }
            }
            Kind::Pick(_) => "choose",
            Kind::Text | Kind::Path | Kind::Toml | Kind::Typed(_) => "edit",
            Kind::Collection(_) => "manage",
            Kind::Fixed => "open",
        }
    }
}

#[derive(Default)]
struct Search {
    query: String,
    cursor: TextCursor,
    selected: usize,
    scroll: Cell<usize>,
    return_to: Snapshot,
}

#[derive(Clone, Debug, Default)]
struct Snapshot {
    category: usize,
    selected: Option<String>,
    list_scroll: usize,
    focus: Focus,
}

#[derive(Clone)]
struct Manager {
    /// The collection's summary row.
    row: Row,
    selected: Option<String>,
    scroll: Cell<usize>,
}

#[derive(Clone)]
struct ReorderPending {
    expected: Vec<String>,
    target: usize,
}

#[derive(Clone)]
struct ThemePicker {
    query: String,
    cursor: TextCursor,
    /// Index into `theme_matches(query)`.
    selected: usize,
    scroll: Cell<usize>,
}

#[derive(Clone)]
struct PickEditor {
    row: Row,
    options: Vec<PickItem>,
    query: String,
    cursor: TextCursor,
    /// Index into the filtered options.
    selected: usize,
    scroll: Cell<usize>,
}

#[derive(Clone)]
struct TextEditor {
    row: Row,
    text: String,
    cursor: TextCursor,
    path_picker: Option<PathPicker>,
}

#[derive(Clone)]
struct Document {
    scroll: Cell<usize>,
}

#[derive(Clone)]
enum Overlay {
    Manager(Manager),
    Theme(ThemePicker),
    Pick(PickEditor),
    Text(TextEditor),
    Document(Document),
}

struct Pending {
    row: String,
    report_key: String,
    value: Option<String>,
    /// The report row's value when the request was sent, so a collection
    /// edit can tell the published change apart from an unrelated report.
    before: Option<String>,
    acknowledged: bool,
    /// The published reading confirms this exact request, but completion
    /// still waits for the request's own Response so a refusal is not lost.
    published: bool,
}

#[derive(Default)]
pub(super) struct Editor {
    focus: Focus,
    /// Index into the catalog's category order.
    category: usize,
    /// The selected row's id, so a new reading keeps the selection.
    selected: Option<String>,
    search: Option<Search>,
    search_return: Option<Snapshot>,
    overlays: Vec<Overlay>,
    /// Requests in flight, by request id, and the row each came from.
    pending: HashMap<u64, Pending>,
    /// The Server's refusal, by row id, until that row is edited again.
    refused: HashMap<String, String>,
    notice: Option<String>,
    /// A transient theme shown while browsing the theme picker. The Server's
    /// settings remain authoritative and are never changed by previewing.
    pub(super) preview_theme: Option<ThemeId>,
    /// A preview committed through SetSetting remains visible until the
    /// Server publishes the resulting settings report.
    pub(super) preview_request: Option<u64>,
    /// Counts SettingsChanged reports, so a save can wait for a report that
    /// was published after its own request.
    report_seq: u64,
    /// The next finding `f` visits.
    finding_cursor: usize,
    /// A picker-root move keeps its folder selected until this exact order is
    /// published, even if the manager closes and reopens meanwhile.
    pending_reorder: Option<ReorderPending>,
    category_scroll: Cell<usize>,
    list_scroll: Cell<usize>,
    detail_scroll: Cell<usize>,
    /// Detail lines hidden below the visible details area.
    detail_more: Cell<usize>,
    /// The frame area of the last draw, so keys and mouse use the same
    /// geometry as the visible Settings view.
    last_area: Cell<Rect>,
}

impl Editor {
    pub(super) fn resize(&mut self, area: Rect) {
        self.last_area.set(area);
    }

    pub(super) fn cancel_draft(&mut self) {
        self.search = None;
        self.search_return = None;
        self.overlays.clear();
        self.pending_reorder = None;
        self.preview_theme = None;
        self.preview_request = None;
    }

    /// Select a setting by id, as the palette's setting entries do on opening.
    pub(super) fn select(&mut self, id: &str) {
        self.selected = Some(id.to_owned());
        if let Some(group) = category_of_row(id) {
            self.category = category_index(group);
        }
        self.focus = Focus::List;
        self.list_scroll.set(0);
        self.detail_scroll.set(0);
    }

    /// A SettingsChanged report settles saves whose effective value has now
    /// been published and clears a resolved unparseable notice.
    pub(super) fn report_published(&mut self, report: &SettingsReport) {
        self.report_seq = self.report_seq.saturating_add(1);
        // A published reading confirms at most one queued edit per collection
        // by its changed parent array; never mark every edit against the same
        // change. Completion still waits for each request's own Response.
        let mut confirmed: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, pending)| !pending.published && pending_exact(pending, report))
            .map(|(id, _)| *id)
            .collect();
        let mut changed_keys: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, pending)| {
                !pending.published
                    && pending.report_key != pending.row
                    && pending_child_changed(pending, report)
            })
            .map(|(_, pending)| pending.report_key.clone())
            .collect();
        changed_keys.sort();
        changed_keys.dedup();
        for key in changed_keys {
            if let Some(id) = self
                .pending
                .iter()
                .filter(|(id, pending)| {
                    pending.report_key == key
                        && pending.report_key != pending.row
                        && !pending.published
                        && !confirmed.contains(id)
                })
                .map(|(id, _)| *id)
                .min()
            {
                confirmed.push(id);
            }
        }
        for id in confirmed {
            if let Some(pending) = self.pending.get_mut(&id) {
                pending.published = true;
            }
        }
        let resolved: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, pending)| pending.acknowledged && pending.published)
            .map(|(id, _)| *id)
            .collect();
        for id in resolved {
            self.pending.remove(&id);
        }
        if !report.unparseable && self.notice.as_deref() == Some(UNPARSEABLE) {
            self.notice = None;
        }
        if let Some(pending) = self.pending_reorder.clone() {
            let published: Vec<String> = report
                .settings
                .picker_roots
                .iter()
                .map(|root| root.display().to_string())
                .collect();
            if published == pending.expected {
                for overlay in &mut self.overlays {
                    if let Overlay::Manager(manager) = overlay
                        && manager.row.id == "picker_roots"
                    {
                        manager.selected = Some(format!("picker_roots[{}]", pending.target));
                    }
                }
                self.pending_reorder = None;
            }
        }
        clamp_manager_selections(self, report);
        clamp_editor(self, report);
    }
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
        LaunchChoice::Agent {
            preset,
            model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
        } => format!("Agent: {preset} · Harness default"),
        LaunchChoice::Agent {
            preset,
            model_policy: ovrcr_protocol::AgentModelPolicy::Explicit(model),
        } => format!("Agent: {preset} · {model}"),
    }
}

/// The value written at `launch_choices.<project>` for `choice`.
fn choice_value(project: &str, choice: &LaunchChoice) -> String {
    match super::settings::launch_choice_request(project, choice) {
        Request::SetSetting { value, .. } => value.unwrap_or_default(),
        _ => unreachable!("launch_choice_request builds SetSetting"),
    }
}

/// The category names in catalog order.
fn categories() -> Vec<&'static str> {
    let mut groups: Vec<&'static str> = Vec::new();
    for entry in ovrcr_protocol::entries() {
        if !groups.contains(&entry.group) {
            groups.push(entry.group);
        }
    }
    groups
}

fn category_index(group: &str) -> usize {
    categories()
        .iter()
        .position(|name| *name == group)
        .unwrap_or(0)
}

/// The catalog category owning a row id, including collection children.
fn category_of_row(id: &str) -> Option<&'static str> {
    let path = id.trim_end_matches('+');
    ovrcr_protocol::entries()
        .iter()
        .find(|entry| {
            path == entry.path
                || path.starts_with(&format!("{}.", entry.path))
                || path.starts_with(&format!("{}[", entry.path))
        })
        .map(|entry| entry.group)
}

fn top_report_row<'a>(
    report: &'a SettingsReport,
    key: &str,
) -> Option<&'a ovrcr_protocol::SettingRow> {
    report.rows.iter().find(|row| row.key == key)
}

fn finding_for(report: &SettingsReport, key: &str) -> Option<String> {
    report
        .findings
        .iter()
        .filter(|finding| {
            finding.key.as_deref().is_some_and(|found| {
                found == key
                    || found.starts_with(&format!("{key}."))
                    || found.starts_with(&format!("{key}["))
            })
        })
        .map(|finding| match finding.line {
            Some(line) => format!("line {line}: {}", finding.message),
            None => finding.message.clone(),
        })
        .reduce(|a, b| format!("{a}; {b}"))
}

/// A display label for a pick's stored value, without changing the written id.
fn pick_label(path: &str, raw: &str) -> String {
    if path == ThemeId::KEY
        && let Some(theme) = ThemeId::parse(raw)
    {
        return theme.label().into();
    }
    #[cfg(target_os = "macos")]
    if path == ReadySoundChoice::KEY
        && let Some(choice) = ReadySoundChoice::parse(raw)
    {
        return choice.label().into();
    }
    raw.into()
}

fn display_value(key: &str, kind: &Kind, raw: Option<String>) -> String {
    match (kind, raw.as_deref()) {
        (Kind::Toggle(true), _) => "on".into(),
        (Kind::Toggle(false), _) => "off".into(),
        #[cfg(target_os = "macos")]
        (_, None) if key == ReadySoundChoice::KEY => "invalid".into(),
        (_, Some(raw)) => pick_label(key, raw),
        (_, None) => "unset".into(),
    }
}

/// A default is rendered from the Server's raw default value, never from the
/// effective toggle state, so an override's fallback stays truthful.
fn display_default(key: &str, kind: &Kind, raw: String) -> String {
    if matches!(kind, Kind::Toggle(_)) {
        return match raw.as_str() {
            "true" => "on".into(),
            "false" => "off".into(),
            _ => raw,
        };
    }
    pick_label(key, &raw)
}

fn top_row(
    entry: &ovrcr_protocol::Entry,
    report: &SettingsReport,
    kind: Kind,
    collection: Option<Collection>,
) -> Row {
    let row = top_report_row(report, entry.path);
    let source = row.map_or(SettingSource::Default, |row| row.source);
    // Guidance and consequence descriptions lead the fixed details block so
    // permission/access/cost warnings are visible before Enter acts. The
    // footer names the off action; no duplicate off instruction is needed.
    let mut lines: Vec<String> = entry.about.iter().map(|line| line.to_string()).collect();
    let raw = row.and_then(|row| row.value.clone());
    let raw_default = row.and_then(|row| row.default.clone());
    let value = display_value(entry.path, &kind, raw.clone());
    let default = raw_default.map(|default| display_default(entry.path, &kind, default));
    let finding = finding_for(report, entry.path);
    let mut value = match &collection {
        Some(Collection::Paths) => count(report.settings.picker_roots.len()),
        Some(Collection::Agents) => count(report.settings.agents.len()),
        Some(Collection::Launches) => count(report.settings.launch_choices.len()),
        None => value,
    };
    if entry.path == ThemeId::KEY {
        value = report.settings.theme.label().into();
        if let Some(about) = lines
            .iter_mut()
            .find(|line| line.starts_with("Changing theme updates the Dashboard"))
        {
            *about =
                "The highlighted choice previews across the Dashboard. Enter saves it; Esc restores the saved theme."
                    .into();
        }
    }
    Row {
        id: entry.path.into(),
        group: entry.group,
        label: entry.label.into(),
        value,
        source: Some(source),
        default,
        finding,
        about: lines,
        child: false,
        kind: collection.map_or(kind, Kind::Collection),
        path: entry.path.into(),
        reset: (source == SettingSource::Document).then(|| (entry.path.to_string(), None)),
        whole_list: None,
    }
}

/// Every top-level row in catalog order, once per setting. Collections stay a
/// single summary row; their entries live in the manager.
fn top_items(report: &SettingsReport) -> Vec<Row> {
    let mut rows = Vec::new();
    for entry in ovrcr_protocol::entries() {
        #[cfg(not(target_os = "macos"))]
        if entry.path == ovrcr_protocol::ReadySoundChoice::KEY {
            continue;
        }
        let collection = match entry.shape {
            ovrcr_protocol::Shape::Paths => Some(Collection::Paths),
            ovrcr_protocol::Shape::Agents => Some(Collection::Agents),
            ovrcr_protocol::Shape::Launches => Some(Collection::Launches),
            _ => None,
        };
        let kind = match collection {
            Some(collection) => Kind::Collection(collection),
            None => editor_kind(entry, &report.settings),
        };
        let mut row = top_row(entry, report, kind, collection);
        #[cfg(target_os = "macos")]
        if entry.path == ovrcr_protocol::ReadySoundChoice::KEY
            && let Some(choice) = report.settings.ready_sound_choice
        {
            row.value = choice.label().into();
            if !(report.settings.desktop_notifications && report.settings.ready_sound) {
                row.value.push_str(" · inactive");
                row.about.insert(
                    0,
                    if report.settings.desktop_notifications {
                        "Inactive: Ready sound is off"
                    } else {
                        "Inactive: Desktop notifications are off"
                    }
                    .into(),
                );
            }
        }
        rows.push(row);
    }
    rows
}

fn group_rows(report: &SettingsReport, group: &str) -> Vec<Row> {
    top_items(report)
        .into_iter()
        .filter(|row| row.group == group)
        .collect()
}

fn editor_kind(entry: &ovrcr_protocol::Entry, settings: &ovrcr_protocol::Settings) -> Kind {
    match entry.shape {
        ovrcr_protocol::Shape::Toggle => {
            Kind::Toggle(entry.value(settings).as_deref() == Some("true"))
        }
        ovrcr_protocol::Shape::Text(_) => Kind::Text,
        ovrcr_protocol::Shape::Typed(expected) => Kind::Typed(expected),
        ovrcr_protocol::Shape::Path(_) => Kind::Path,
        ovrcr_protocol::Shape::Pick { options, .. } => Kind::Pick(
            options
                .iter()
                .map(|value| PickItem {
                    label: pick_label(entry.path, value),
                    value: toml_string(value),
                })
                .collect(),
        ),
        ovrcr_protocol::Shape::Paths
        | ovrcr_protocol::Shape::Agents
        | ovrcr_protocol::Shape::Launches => Kind::Fixed,
    }
}

fn launch_pick_items(
    report: &SettingsReport,
    presets: &[String],
    project: &str,
    current: &LaunchChoice,
) -> Vec<PickItem> {
    let mut choices = vec![LaunchChoice::Terminal];
    choices.extend(presets.iter().cloned().map(|preset| LaunchChoice::Agent {
        preset,
        model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
    }));
    if !choices.contains(current) {
        choices.push(current.clone());
    }
    let _ = report;
    choices
        .iter()
        .map(|choice| PickItem {
            label: choice_text(choice),
            value: choice_value(project, choice),
        })
        .collect()
}

/// A collection's manager rows: its entries in order and one Add row.
fn manager_rows(report: &SettingsReport, parent: &Row, presets: &[String]) -> Vec<Row> {
    let Some(collection) = (match &parent.kind {
        Kind::Collection(collection) => Some(*collection),
        _ => None,
    }) else {
        return Vec::new();
    };
    let settings = &report.settings;
    let group = parent.group;
    let mut rows = Vec::new();
    match collection {
        Collection::Paths => {
            let roots: Vec<String> = settings
                .picker_roots
                .iter()
                .map(|root| root.display().to_string())
                .collect();
            let roots_set = parent.source == Some(SettingSource::Document);
            for (index, root) in roots.iter().enumerate() {
                let id = format!("picker_roots[{index}]");
                let mut row = Row {
                    id: id.clone(),
                    group,
                    label: root.clone(),
                    // Label is the visible folder name; value is the edit
                    // draft, so Enter opens the complete existing path.
                    value: root.clone(),
                    source: None,
                    default: None,
                    finding: finding_for(report, &id),
                    about: vec!["A folder in the picker root search order.".into()],
                    child: true,
                    kind: Kind::Path,
                    path: id.clone(),
                    reset: None,
                    whole_list: None,
                };
                if roots_set {
                    row.reset = Some((id, None));
                } else {
                    // The default list is not in the document: write it whole.
                    let mut list: Vec<String> =
                        roots.iter().map(|root| toml_string(root)).collect();
                    list[index] = "{}".into();
                    row.path = "picker_roots".into();
                    row.whole_list = Some(list.clone());
                    list.remove(index);
                    row.reset = Some((
                        "picker_roots".into(),
                        Some(format!("[{}]", list.join(", "))),
                    ));
                }
                rows.push(row);
            }
            let mut add = Row {
                id: "picker_roots+".into(),
                group,
                label: Collection::Paths.add_label().into(),
                value: String::new(),
                source: None,
                default: None,
                finding: None,
                about: vec!["Adds one folder to the end of the search order.".into()],
                child: true,
                kind: Kind::Path,
                path: format!("picker_roots[{}]", roots.len()),
                reset: None,
                whole_list: None,
            };
            if !roots_set {
                let mut list: Vec<String> = roots.iter().map(|root| toml_string(root)).collect();
                list.push("{}".into());
                add.path = "picker_roots".into();
                add.whole_list = Some(list);
            }
            rows.push(add);
        }
        Collection::Agents => {
            for (index, agent) in settings.agents.iter().enumerate() {
                let id = format!("agents[{index}]");
                rows.push(Row {
                    id: id.clone(),
                    group,
                    label: agent.name.clone(),
                    value: toml_array(agent.argv.iter().map(String::as_str)),
                    source: None,
                    default: None,
                    finding: finding_for(report, &id),
                    about: vec!["argv is edited as a TOML string array.".into()],
                    child: true,
                    kind: Kind::Toml,
                    path: format!("{id}.argv"),
                    reset: Some((id, None)),
                    whole_list: None,
                });
            }
            rows.push(Row {
                id: Collection::Agents.add_id().into(),
                group,
                label: Collection::Agents.add_label().into(),
                value: String::new(),
                source: None,
                default: None,
                finding: None,
                about: vec!["Adds an override with the typed name as its argv[0].".into()],
                child: true,
                kind: Kind::Text,
                path: format!("agents[{}]", settings.agents.len()),
                reset: None,
                whole_list: None,
            });
        }
        Collection::Launches => {
            for (project, choice) in &settings.launch_choices {
                let path = format!("launch_choices.{}", toml_edit::Key::new(project.as_str()));
                rows.push(Row {
                    id: path.clone(),
                    group,
                    label: project.clone(),
                    value: choice_text(choice),
                    source: None,
                    default: None,
                    finding: finding_for(report, &path),
                    about: vec!["Enter chooses Terminal or an Agent preset.".into()],
                    child: true,
                    kind: Kind::Pick(launch_pick_items(report, presets, project, choice)),
                    path: path.clone(),
                    reset: Some((path, None)),
                    whole_list: None,
                });
            }
            rows.push(Row {
                id: Collection::Launches.add_id().into(),
                group,
                label: Collection::Launches.add_label().into(),
                value: String::new(),
                source: None,
                default: None,
                finding: None,
                about: vec!["Adds a project that launches a Terminal by default.".into()],
                child: true,
                kind: Kind::Text,
                path: String::new(),
                reset: None,
                whole_list: None,
            });
        }
    }
    rows
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

fn matches_words(haystack: &str, filter: &str) -> bool {
    let haystack = haystack.to_lowercase();
    filter
        .to_lowercase()
        .split_whitespace()
        .all(|word| haystack.contains(word))
}

/// Global Settings search: labels, configuration keys, and the owning
/// category. Effective values and descriptions are intentionally excluded so
/// the first result is the setting the words name, not one that merely stores
/// the word. Picker filtering has its own matcher.
fn search_matches(row: &Row, filter: &str) -> bool {
    matches_words(&format!("{} {} {}", row.group, row.label, row.id), filter)
}

/// Findings that belong to no row: unknown keys and document-level problems.
fn loose_findings(report: &SettingsReport) -> Vec<String> {
    report
        .findings
        .iter()
        .filter(|finding| match finding.key.as_deref() {
            None => true,
            Some(key) => !ovrcr_protocol::entries().iter().any(|entry| {
                key == entry.path
                    || key.starts_with(&format!("{}[", entry.path))
                    || key.starts_with(&format!("{}.", entry.path))
            }),
        })
        .map(|finding| {
            let line = finding
                .line
                .map(|l| format!("line {l}: "))
                .unwrap_or_default();
            format!(
                "{line}{}: {}",
                finding.key.as_deref().unwrap_or("document"),
                finding.message
            )
        })
        .collect()
}

fn split_filtered<'a>(items: &'a [PickItem], query: &str) -> Vec<&'a PickItem> {
    items
        .iter()
        .filter(|item| matches_words(&format!("{} {}", item.label, item.value), query))
        .collect()
}

fn theme_matches(query: &str) -> Vec<ThemeId> {
    let mut themes: Vec<ThemeId> = ThemeId::ALL
        .iter()
        .copied()
        .filter(|theme| theme.appearance() == ThemeAppearance::Dark)
        .collect();
    themes.extend(
        ThemeId::ALL
            .iter()
            .copied()
            .filter(|theme| theme.appearance() == ThemeAppearance::Light),
    );
    if query.trim().is_empty() {
        return themes;
    }
    themes
        .into_iter()
        .filter(|theme| {
            let appearance = match theme.appearance() {
                ThemeAppearance::Dark => "dark",
                ThemeAppearance::Light => "light",
            };
            matches_words(
                &format!("{} {} {}", theme.label(), theme.as_str(), appearance),
                query,
            )
        })
        .collect()
}

fn theme_saved(report: Option<&SettingsReport>) -> ThemeId {
    report.map_or(ThemeId::Dark, |report| report.settings.theme)
}

fn theme_index(saved: ThemeId) -> usize {
    theme_matches("")
        .iter()
        .position(|theme| *theme == saved)
        .unwrap_or(0)
}

/// One rendered picker line; headings are not selectable.
enum PickerLine {
    Heading(&'static str),
    Theme(usize, ThemeId),
}

fn theme_lines(query: &str) -> (Vec<PickerLine>, Vec<ThemeId>) {
    let matches = theme_matches(query);
    let mut lines = Vec::new();
    let mut appearance = None;
    for (index, theme) in matches.iter().enumerate() {
        let next = theme.appearance();
        if appearance != Some(next) {
            lines.push(PickerLine::Heading(match next {
                ThemeAppearance::Dark => "Dark",
                ThemeAppearance::Light => "Light",
            }));
            appearance = Some(next);
        }
        lines.push(PickerLine::Theme(index, *theme));
    }
    (lines, matches)
}

fn point_in(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.right()
        && row >= area.y
        && row < area.bottom()
        && area.width > 0
        && area.height > 0
}

fn row_at(area: Rect, row: u16, scroll: usize, count: usize) -> Option<usize> {
    if row < area.y || row >= area.bottom() || area.height == 0 {
        return None;
    }
    let index = scroll + usize::from(row - area.y);
    (index < count).then_some(index)
}

fn category_at(area: Rect, row: u16, scroll: usize, count: usize) -> Option<usize> {
    if row <= area.y || row >= area.bottom() || area.height < 2 {
        return None;
    }
    let index = scroll + usize::from(row - area.y - 1);
    (index < count).then_some(index)
}

fn window_start(selected: usize, count: usize, height: usize, scroll: usize) -> usize {
    let height = height.max(1);
    let max_start = count.saturating_sub(height);
    let mut start = scroll.min(max_start);
    if selected < start {
        start = selected;
    } else if selected >= start + height {
        start = selected + 1 - height;
    }
    start.min(max_start)
}

fn pad_cells(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let clipped = clip_text(text, width);
    let used = clipped.width();
    format!("{clipped}{}", " ".repeat(width.saturating_sub(used)))
}

/// The shared geometry of one Settings frame for drawing and mouse input.
struct Layout {
    header: Rect,
    footer: Rect,
    content: Rect,
    sidebar: Option<Rect>,
    main: Rect,
    wide: bool,
    tiny: bool,
}

fn settings_layout(area: Rect) -> Layout {
    let tiny = area.width < MIN_WIDTH || area.height < MIN_HEIGHT;
    let header = Rect::new(area.x, area.y, area.width, 1);
    let footer = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
    let content = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(2),
    );
    if tiny {
        return Layout {
            header,
            footer,
            content,
            sidebar: None,
            main: content,
            wide: false,
            tiny,
        };
    }
    let wide = area.width >= WIDE_WIDTH;
    if !wide {
        return Layout {
            header,
            footer,
            content,
            sidebar: None,
            main: content,
            wide,
            tiny,
        };
    }
    let width = (area.width / 5).clamp(20, 24);
    let sidebar = Rect::new(content.x, content.y, width, content.height);
    let main = Rect::new(
        sidebar.right().saturating_add(1),
        content.y,
        content.width.saturating_sub(width + 1),
        content.height,
    );
    Layout {
        header,
        footer,
        content,
        sidebar: Some(sidebar),
        main,
        wide,
        tiny,
    }
}

/// Split a pane into its stable list area and fixed details area.
fn pane_split(area: Rect) -> (Rect, Rect) {
    let detail = (area.height / 3)
        .clamp(4, 14)
        .min(area.height.saturating_sub(2));
    let list_height = area.height.saturating_sub(detail + 1);
    (
        Rect::new(area.x, area.y, area.width, list_height),
        Rect::new(
            area.x,
            area.y.saturating_add(list_height + 1),
            area.width,
            detail,
        ),
    )
}

/// The theme picker's fixed areas inside the main pane.
struct ThemeAreas {
    search: Rect,
    options: Rect,
    summary: Rect,
}

fn theme_areas(area: Rect) -> ThemeAreas {
    let summary_height = 3.min(area.height.saturating_sub(3));
    let search_height = 2.min(area.height.saturating_sub(summary_height + 1));
    let options_height = area
        .height
        .saturating_sub(2 + search_height + summary_height);
    ThemeAreas {
        search: Rect::new(area.x, area.y.saturating_add(2), area.width, search_height),
        options: Rect::new(
            area.x,
            area.y.saturating_add(2 + search_height),
            area.width,
            options_height,
        ),
        summary: Rect::new(
            area.x,
            area.bottom().saturating_sub(summary_height),
            area.width,
            summary_height,
        ),
    }
}

impl Dashboard {
    fn report(&self) -> Option<&SettingsReport> {
        self.settings_report.as_deref()
    }

    /// Whether the active editor has a visible row/option to act on. The
    /// global minimum is not enough: a theme picker can have zero option rows
    /// at 60x8 while its selected palette remains actionable.
    fn settings_editor_fits(&self, layout: &Layout) -> bool {
        if layout.tiny {
            return false;
        }
        let editor = &self.settings_editor;
        match editor.overlays.last() {
            Some(Overlay::Theme(_)) => theme_areas(layout.main).options.height > 0,
            Some(Overlay::Pick(_)) => layout.main.height.saturating_sub(5) > 0,
            Some(Overlay::Text(_)) => layout.main.height > 3,
            Some(Overlay::Document(_)) => layout.main.height > 1,
            Some(Overlay::Manager(_)) => pane_split(layout.main).0.height > 0,
            None if editor.search.is_some() => pane_split(layout.main).0.height > 1,
            None if editor.focus == Focus::Categories => {
                if layout.wide {
                    pane_split(layout.main).0.height > 0
                } else {
                    layout.main.height > 1
                }
            }
            None => pane_split(layout.main).0.height > 0,
        }
    }

    fn small_settings_key(&mut self, key: KeyEvent) -> DashboardAction {
        if super::input::is_browse_key(key) {
            self.settings_editor.cancel_draft();
            self.details = None;
            return DashboardAction::Redraw;
        }
        if key.code != KeyCode::Esc {
            return DashboardAction::Redraw;
        }
        if !self.settings_editor.overlays.is_empty() {
            return self.overlay_key(key);
        }
        if self.settings_editor.search.is_some() {
            return self.search_key(key);
        }
        if self.settings_editor.focus == Focus::List {
            self.settings_editor.focus = Focus::Categories;
            self.settings_editor.detail_scroll.set(0);
            return DashboardAction::Redraw;
        }
        self.settings_editor.cancel_draft();
        self.details = None;
        DashboardAction::Redraw
    }

    fn settings_area(&self) -> Rect {
        let drawn = self.settings_editor.last_area.get();
        let outer = self.outer_area;
        if drawn.width == 0 || drawn.height == 0 {
            return outer;
        }
        Rect::new(
            outer.x,
            outer.y,
            outer.width.min(drawn.width),
            outer.height.min(drawn.height),
        )
    }

    fn categories(&self) -> Vec<&'static str> {
        categories()
    }

    fn category_name(&self) -> &'static str {
        let categories = self.categories();
        let index = self
            .settings_editor
            .category
            .min(categories.len().saturating_sub(1));
        categories.get(index).copied().unwrap_or("Alerts")
    }

    fn editor_rows(&self) -> Vec<Row> {
        self.report()
            .map_or_else(Vec::new, |report| group_rows(report, self.category_name()))
    }

    fn manager_rows_for(&self, parent: &Row) -> Vec<Row> {
        self.report().map_or_else(Vec::new, |report| {
            manager_rows(report, parent, &self.launch_presets())
        })
    }

    /// Resolve an open editor's row against the latest reading. A frozen
    /// append index would overwrite an entry another client added while the
    /// editor was open.
    fn latest_row(&self, id: &str) -> Option<Row> {
        let report = self.report()?;
        if let Some(parent_id) = collection_parent(id) {
            let parent = top_items(report)
                .into_iter()
                .find(|row| row.id == parent_id)?;
            return manager_rows(report, &parent, &self.launch_presets())
                .into_iter()
                .find(|row| row.id == id);
        }
        top_items(report).into_iter().find(|row| row.id == id)
    }

    /// The collection's current summary. The row captured when the manager
    /// opened can be stale after the Server publishes a source change, so
    /// source/reset actions always follow the latest reading.
    fn manager_parent(&self, manager: &Manager) -> Row {
        self.report()
            .and_then(|report| {
                top_items(report)
                    .into_iter()
                    .find(|row| row.id == manager.row.id)
            })
            .unwrap_or_else(|| manager.row.clone())
    }

    fn overlay_manager_rows(&self) -> Option<Vec<Row>> {
        match self.settings_editor.overlays.last() {
            Some(Overlay::Manager(manager)) => {
                let parent = self.manager_parent(manager);
                Some(self.manager_rows_for(&parent))
            }
            _ => None,
        }
    }

    fn selected_row(&self) -> Option<Row> {
        if let Some(rows) = self.overlay_manager_rows() {
            let editor = &self.settings_editor;
            let selected = match editor.overlays.last() {
                Some(Overlay::Manager(manager)) => manager.selected.as_ref(),
                _ => None,
            };
            return selected
                .and_then(|id| rows.iter().find(|row| &row.id == id).cloned())
                .or_else(|| rows.first().cloned());
        }
        let rows = self.editor_rows();
        let editor = &self.settings_editor;
        rows.iter()
            .find(|row| Some(&row.id) == editor.selected.as_ref())
            .or_else(|| rows.first())
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
        let editor = &mut self.settings_editor;
        let current = editor
            .selected
            .as_ref()
            .and_then(|id| rows.iter().position(|row| &row.id == id))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, rows.len() as isize - 1) as usize;
        editor.selected = Some(rows[next].id.clone());
        editor.list_scroll.set(0);
        editor.detail_scroll.set(0);
    }

    fn move_category(&mut self, delta: isize) {
        let categories = self.categories();
        if categories.is_empty() {
            return;
        }
        let editor = &mut self.settings_editor;
        editor.category =
            (editor.category as isize + delta).clamp(0, categories.len() as isize - 1) as usize;
        editor.detail_scroll.set(0);
    }

    fn enter_list(&mut self) {
        self.settings_editor.focus = Focus::List;
        let rows = self.editor_rows();
        let editor = &mut self.settings_editor;
        if !editor
            .selected
            .as_ref()
            .is_some_and(|id| rows.iter().any(|row| &row.id == id))
        {
            editor.selected = rows.first().map(|row| row.id.clone());
            editor.list_scroll.set(0);
        }
        editor.detail_scroll.set(0);
    }

    fn send_setting(&mut self, row: &str, path: String, value: Option<String>) -> DashboardAction {
        if self.editing_disabled() {
            return DashboardAction::Redraw;
        }
        let request_id = self.next_request_id();
        let (report_key, before) = self.report().map_or_else(
            || (row.to_string(), None),
            |report| {
                let key = report_key_for_row(report, row);
                let before = report
                    .rows
                    .iter()
                    .find(|candidate| candidate.key == key)
                    .and_then(|candidate| candidate.value.clone());
                (key, before)
            },
        );
        let editor = &mut self.settings_editor;
        editor.refused.remove(row);
        if row == "picker_roots" {
            editor.pending_reorder = None;
        }
        if row == ThemeId::KEY && editor.preview_theme.is_some() {
            editor.preview_request = Some(request_id);
        }
        editor.pending.insert(
            request_id,
            Pending {
                row: row.to_string(),
                report_key,
                value: value.clone(),
                before,
                acknowledged: false,
                published: false,
            },
        );
        self.enqueue_request(ClientMessage {
            request_id,
            request: Request::SetSetting { path, value },
        });
        DashboardAction::Redraw
    }

    fn editing_disabled(&mut self) -> bool {
        match self.report() {
            None => {
                self.settings_editor.notice = Some(WAITING.into());
                true
            }
            Some(report) if report.unparseable => {
                self.settings_editor.notice = Some(UNPARSEABLE.into());
                true
            }
            Some(_) => false,
        }
    }

    fn open_document(&mut self) {
        self.settings_editor
            .overlays
            .push(Overlay::Document(Document {
                scroll: Cell::new(0),
            }));
        self.settings_editor.detail_scroll.set(0);
    }

    fn active_row(&self) -> Option<Row> {
        self.selected_row()
    }

    fn activate_selected(&mut self) -> DashboardAction {
        if self.editing_disabled() {
            return DashboardAction::Redraw;
        }
        let Some(row) = self.active_row() else {
            return DashboardAction::Redraw;
        };
        match row.kind.clone() {
            Kind::Toggle(on) => {
                self.send_setting(&row.id, row.path.clone(), Some((!on).to_string()))
            }
            Kind::Pick(_) if row.id == ThemeId::KEY => self.open_theme(row),
            Kind::Pick(_) => self.open_pick(row),
            Kind::Text | Kind::Path | Kind::Toml | Kind::Typed(_) => self.open_text(row),
            Kind::Collection(_) => self.open_manager(row),
            Kind::Fixed => {
                self.settings_editor.notice =
                    Some("Use the rows in this collection; r resets the whole collection".into());
                DashboardAction::Redraw
            }
        }
    }

    fn open_manager(&mut self, row: Row) -> DashboardAction {
        let selected = self
            .report()
            .map(|report| {
                manager_rows(report, &row, &self.launch_presets())
                    .first()
                    .map(|row| row.id.clone())
            })
            .unwrap_or(None);
        self.settings_editor
            .overlays
            .push(Overlay::Manager(Manager {
                row,
                selected,
                scroll: Cell::new(0),
            }));
        self.settings_editor.detail_scroll.set(0);
        DashboardAction::Redraw
    }

    fn open_theme(&mut self, row: Row) -> DashboardAction {
        let saved = theme_saved(self.report());
        let picker = ThemePicker {
            query: String::new(),
            cursor: TextCursor::default(),
            selected: theme_index(saved),
            scroll: Cell::new(0),
        };
        self.settings_editor.preview_request = None;
        self.settings_editor.preview_theme = Some(saved);
        self.settings_editor.overlays.push(Overlay::Theme(picker));
        self.settings_editor.selected = Some(row.id);
        self.settings_editor.detail_scroll.set(0);
        DashboardAction::Redraw
    }

    fn open_pick(&mut self, row: Row) -> DashboardAction {
        let options = match &row.kind {
            Kind::Pick(options) => {
                if row.id.starts_with("launch_choices.") && !row.child {
                    Vec::new()
                } else {
                    options.clone()
                }
            }
            _ => Vec::new(),
        };
        let mut options = options;
        if row.id.starts_with("launch_choices.")
            && let Some(report) = self.report()
            && let Some(choice) = report.settings.launch_choices.get(&row.label)
        {
            options = launch_pick_items(report, &self.launch_presets(), &row.label, choice);
        }
        let canonical = self.report().and_then(|report| {
            report
                .rows
                .iter()
                .find(|candidate| candidate.key == row.id)
                .and_then(|candidate| candidate.value.clone())
        });
        let selected = options
            .iter()
            .position(|item| {
                item.label == row.value
                    || canonical
                        .as_deref()
                        .is_some_and(|raw| same_value(&item.value, &toml_string(raw)))
            })
            .unwrap_or(0);
        self.settings_editor
            .overlays
            .push(Overlay::Pick(PickEditor {
                row,
                options,
                query: String::new(),
                cursor: TextCursor::default(),
                selected,
                scroll: Cell::new(0),
            }));
        self.settings_editor.detail_scroll.set(0);
        DashboardAction::Redraw
    }

    fn open_text(&mut self, row: Row) -> DashboardAction {
        let text = if row.id.ends_with('+') || row.value == "unset" {
            String::new()
        } else {
            row.value.clone()
        };
        let mut path_picker = matches!(&row.kind, Kind::Path).then(PathPicker::new);
        if let Some(picker) = &mut path_picker {
            let roots = self.settings.picker_roots.clone();
            picker.listing(&text, &roots);
        }
        self.settings_editor
            .overlays
            .push(Overlay::Text(TextEditor {
                row,
                text,
                cursor: TextCursor::default(),
                path_picker,
            }));
        self.settings_editor.detail_scroll.set(0);
        DashboardAction::Redraw
    }

    fn start_edit(&mut self) -> DashboardAction {
        if self.editing_disabled() {
            return DashboardAction::Redraw;
        }
        self.activate_selected()
    }

    fn reset_or_remove(&mut self, remove: bool) -> DashboardAction {
        if self.editing_disabled() {
            return DashboardAction::Redraw;
        }
        let Some(row) = self.active_row() else {
            return DashboardAction::Redraw;
        };
        match (&row.reset, row.child && remove) {
            (Some((path, value)), true) => self.send_setting(&row.id, path.clone(), value.clone()),
            (Some((path, value)), false) if !row.child => {
                self.send_setting(&row.id, path.clone(), value.clone())
            }
            _ => {
                self.settings_editor.notice = Some(match (row.child, remove) {
                    (false, false) => format!("{} is already its default", row.label),
                    (false, true) => "Remove applies inside a collection; r resets".into(),
                    (true, false) => {
                        "Reset applies to a whole collection; x removes this entry".into()
                    }
                    (true, true) => "Nothing to remove here".into(),
                });
                DashboardAction::Redraw
            }
        }
    }

    fn reorder_root(&mut self, manager: &mut Manager, delta: isize) -> DashboardAction {
        if self.editing_disabled() {
            return DashboardAction::Redraw;
        }
        let parent = self.manager_parent(manager);
        let rows = self.manager_rows_for(&parent);
        let row = manager
            .selected
            .as_ref()
            .and_then(|id| rows.iter().find(|row| &row.id == id))
            .cloned();
        let Some(index) = row.as_ref().and_then(|row| {
            row.id
                .strip_prefix("picker_roots[")
                .and_then(|rest| rest.strip_suffix(']'))
                .and_then(|index| index.parse::<usize>().ok())
        }) else {
            self.settings_editor.notice =
                Some("Move up and Move down apply to picker roots".into());
            return DashboardAction::Redraw;
        };
        let roots: Vec<String> = self
            .settings
            .picker_roots
            .iter()
            .map(|root| root.display().to_string())
            .collect();
        match reordered_roots(&roots, index, delta) {
            Some(value) => {
                let target = (index as isize + delta) as usize;
                let mut expected = roots.clone();
                expected.swap(index, target);
                let action = self.send_setting("picker_roots", "picker_roots".into(), Some(value));
                self.settings_editor.pending_reorder = Some(ReorderPending { expected, target });
                action
            }
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

    pub(super) fn settings_editor_key(&mut self, key: KeyEvent) -> DashboardAction {
        if key.kind == KeyEventKind::Release {
            return DashboardAction::None;
        }
        let area = self.settings_area();
        let layout = settings_layout(area);
        if !self.settings_editor_fits(&layout) {
            return self.small_settings_key(key);
        }
        self.settings_editor.notice = None;
        if super::input::is_browse_key(key) {
            self.settings_editor.cancel_draft();
            self.details = None;
            return DashboardAction::Redraw;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('d' | 'u'))
            && (self.settings_editor.overlays.is_empty()
                || matches!(
                    self.settings_editor.overlays.last(),
                    Some(Overlay::Manager(_))
                )
                || self.settings_editor.search.is_some())
        {
            let editor = &mut self.settings_editor;
            let delta = if key.code == KeyCode::Char('d') {
                4
            } else {
                -4
            };
            let scroll = (editor.detail_scroll.get() as isize + delta)
                .clamp(0, editor.detail_more.get() as isize) as usize;
            editor.detail_scroll.set(scroll);
            return DashboardAction::Redraw;
        }
        if !self.settings_editor.overlays.is_empty() {
            return self.overlay_key(key);
        }
        if self.settings_editor.search.is_some() {
            return self.search_key(key);
        }
        if self.settings_editor.focus == Focus::Categories {
            return self.categories_key(key);
        }
        self.list_key(key)
    }

    pub(super) fn settings_editor_mouse(&mut self, mouse: MouseEvent) -> DashboardAction {
        let area = self.settings_area();
        let layout = settings_layout(area);
        if !self.settings_editor_fits(&layout) {
            return DashboardAction::Redraw;
        }
        let scroll = matches!(
            mouse.kind,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
        );
        let click = matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left));
        if !scroll && !click {
            return DashboardAction::None;
        }
        let direction = if matches!(
            mouse.kind,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
        ) {
            if matches!(mouse.kind, MouseEventKind::ScrollUp) {
                -1isize
            } else {
                1
            }
        } else {
            0
        };
        if scroll
            && self.settings_editor.overlays.is_empty()
            && self.settings_editor.search.is_none()
            && let Some(sidebar) = layout.sidebar
            && mouse.column >= sidebar.x
            && mouse.column < sidebar.right()
        {
            self.move_category(direction);
            self.settings_editor.focus = Focus::Categories;
            return DashboardAction::Redraw;
        }
        if let Some(overlay) = self.settings_editor.overlays.last() {
            match overlay {
                Overlay::Theme(picker) => {
                    let picker = picker.clone();
                    if click {
                        let areas = theme_areas(layout.main);
                        if point_in(areas.options, mouse.column, mouse.row) {
                            let (lines, matches) = theme_lines(&picker.query);
                            let offset =
                                usize::from(mouse.row - areas.options.y) + picker.scroll.get();
                            if let Some(PickerLine::Theme(index, _)) = lines.get(offset)
                                && let Some(theme) = matches.get(*index).copied()
                            {
                                if let Some(Overlay::Theme(picker)) =
                                    self.settings_editor.overlays.last_mut()
                                {
                                    picker.selected = *index;
                                }
                                self.settings_editor.preview_theme = Some(theme);
                            }
                        }
                    } else {
                        let mut picker = picker.clone();
                        move_theme_selection(&mut picker, direction);
                        if let Some(Overlay::Theme(current)) =
                            self.settings_editor.overlays.last_mut()
                        {
                            current.selected = picker.selected;
                        }
                        let matches = theme_matches(&picker.query);
                        self.settings_editor.preview_theme = matches.get(picker.selected).copied();
                    }
                }
                Overlay::Pick(picker) => {
                    let picker = picker.clone();
                    let filtered = split_filtered(&picker.options, &picker.query);
                    if click {
                        let options = Rect::new(
                            layout.main.x,
                            layout.main.y.saturating_add(3),
                            layout.main.width,
                            layout.main.height.saturating_sub(5),
                        );
                        if point_in(options, mouse.column, mouse.row) {
                            let index = picker.scroll.get() + usize::from(mouse.row - options.y);
                            if index < filtered.len()
                                && let Some(Overlay::Pick(current)) =
                                    self.settings_editor.overlays.last_mut()
                            {
                                current.selected = index;
                            }
                        }
                    } else if let Some(Overlay::Pick(current)) =
                        self.settings_editor.overlays.last_mut()
                    {
                        let next = (current.selected as isize + direction)
                            .clamp(0, filtered.len().saturating_sub(1) as isize)
                            as usize;
                        current.selected = next;
                    }
                }
                Overlay::Manager(manager) => {
                    let manager = manager.clone();
                    let parent = self.manager_parent(&manager);
                    let rows = self.manager_rows_for(&parent);
                    let (list, _) = pane_split(layout.main);
                    let current = manager
                        .selected
                        .as_ref()
                        .and_then(|id| rows.iter().position(|row| &row.id == id))
                        .unwrap_or(0);
                    let next = if click {
                        if point_in(list, mouse.column, mouse.row) {
                            row_at(list, mouse.row, manager.scroll.get(), rows.len())
                        } else {
                            None
                        }
                    } else {
                        Some(
                            (current as isize + direction)
                                .clamp(0, rows.len().saturating_sub(1) as isize)
                                as usize,
                        )
                    };
                    if let Some(index) = next
                        && let Some(row) = rows.get(index)
                        && let Some(Overlay::Manager(manager)) =
                            self.settings_editor.overlays.last_mut()
                    {
                        manager.selected = Some(row.id.clone());
                        self.settings_editor.detail_scroll.set(0);
                    }
                }
                Overlay::Document(document) => {
                    if scroll {
                        let next = (document.scroll.get() as isize + direction * 3).max(0) as usize;
                        if let Some(Overlay::Document(document)) =
                            self.settings_editor.overlays.last_mut()
                        {
                            document.scroll.set(next);
                        }
                    }
                }
                Overlay::Text(_) => {}
            }
            return DashboardAction::Redraw;
        }
        if self.settings_editor.search.is_none()
            && let Some(sidebar) = layout.sidebar
            && mouse.column >= sidebar.x
            && mouse.column < sidebar.right()
        {
            if click {
                let index = category_at(
                    sidebar,
                    mouse.row,
                    self.settings_editor.category_scroll.get(),
                    self.categories().len(),
                );
                if let Some(index) = index {
                    let same = index == self.settings_editor.category;
                    self.settings_editor.category = index;
                    self.settings_editor.detail_scroll.set(0);
                    if same && self.settings_editor.focus == Focus::Categories {
                        self.enter_list();
                    } else {
                        self.settings_editor.focus = Focus::Categories;
                    }
                }
            }
            return DashboardAction::Redraw;
        }
        if self.settings_editor.search.is_some() {
            let (selected, scroll) = self
                .settings_editor
                .search
                .as_ref()
                .map(|search| (search.selected, search.scroll.get()))
                .expect("checked");
            let hits = self.search_hits();
            let (list, _) = pane_split(layout.main);
            let results = Rect::new(
                list.x,
                list.y.saturating_add(1),
                list.width,
                list.height.saturating_sub(1),
            );
            if click {
                let index = if point_in(results, mouse.column, mouse.row) {
                    row_at(results, mouse.row, scroll, hits.len())
                } else {
                    None
                };
                if let Some(index) = index {
                    if let Some(search) = &mut self.settings_editor.search {
                        search.selected = index;
                    }
                    self.settings_editor.detail_scroll.set(0);
                }
            } else if let Some(search) = &mut self.settings_editor.search {
                let next = (selected as isize + direction)
                    .clamp(0, hits.len().saturating_sub(1) as isize)
                    as usize;
                search.selected = next;
                search.scroll.set(0);
                self.settings_editor.detail_scroll.set(0);
            }
            return DashboardAction::Redraw;
        }
        let (list, detail) = pane_split(layout.main);
        if scroll
            && (layout.wide || self.settings_editor.focus != Focus::Categories)
            && mouse.row >= detail.y
            && mouse.row < detail.bottom()
        {
            let editor = &mut self.settings_editor;
            let next = (editor.detail_scroll.get() as isize + direction * 3)
                .clamp(0, editor.detail_more.get() as isize) as usize;
            editor.detail_scroll.set(next);
            return DashboardAction::Redraw;
        }
        if self.settings_editor.focus == Focus::Categories && !layout.wide {
            if click {
                let index = if point_in(layout.content, mouse.column, mouse.row) {
                    category_at(
                        layout.content,
                        mouse.row,
                        self.settings_editor.category_scroll.get(),
                        self.categories().len(),
                    )
                } else {
                    None
                };
                if let Some(index) = index {
                    self.settings_editor.category = index;
                    self.enter_list();
                }
            } else {
                self.move_category(direction);
            }
            return DashboardAction::Redraw;
        }
        if click || scroll {
            let rows = self.editor_rows();
            let current = self
                .settings_editor
                .selected
                .as_ref()
                .and_then(|id| rows.iter().position(|row| &row.id == id))
                .unwrap_or(0);
            let next = if click {
                if point_in(list, mouse.column, mouse.row) {
                    row_at(
                        list,
                        mouse.row,
                        self.settings_editor.list_scroll.get(),
                        rows.len(),
                    )
                } else {
                    None
                }
            } else {
                Some(
                    (current as isize + direction).clamp(0, rows.len().saturating_sub(1) as isize)
                        as usize,
                )
            };
            if let Some(index) = next
                && let Some(row) = rows.get(index)
            {
                self.settings_editor.selected = Some(row.id.clone());
                self.settings_editor.focus = Focus::List;
                self.settings_editor.detail_scroll.set(0);
            }
        }
        DashboardAction::Redraw
    }

    fn categories_key(&mut self, key: KeyEvent) -> DashboardAction {
        match key.code {
            KeyCode::Esc => {
                self.settings_editor.cancel_draft();
                self.details = None;
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_category(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_category(-1),
            KeyCode::PageDown => self.move_category(10),
            KeyCode::PageUp => self.move_category(-10),
            KeyCode::Home => {
                self.settings_editor.category = 0;
                self.settings_editor.detail_scroll.set(0);
            }
            KeyCode::End => {
                self.settings_editor.category = self.categories().len().saturating_sub(1);
                self.settings_editor.detail_scroll.set(0);
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => {
                self.enter_list()
            }
            KeyCode::Char('/') => {
                let action = self.start_search();
                return action;
            }
            KeyCode::Char('f') => return self.next_finding(),
            KeyCode::Char('d') => {
                self.open_document();
            }
            _ => {}
        }
        DashboardAction::Redraw
    }

    fn list_key(&mut self, key: KeyEvent) -> DashboardAction {
        match key.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                if let Some(snapshot) = self.settings_editor.search_return.take() {
                    let editor = &mut self.settings_editor;
                    editor.category = snapshot.category;
                    editor.selected = snapshot.selected;
                    editor.list_scroll.set(snapshot.list_scroll);
                    editor.focus = snapshot.focus;
                    editor.detail_scroll.set(0);
                } else {
                    self.settings_editor.focus = Focus::Categories;
                    self.settings_editor.detail_scroll.set(0);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_setting_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_setting_selection(-1),
            KeyCode::PageDown => self.move_setting_selection(10),
            KeyCode::PageUp => self.move_setting_selection(-10),
            KeyCode::Home => self.move_setting_selection(isize::MIN / 2),
            KeyCode::End => self.move_setting_selection(isize::MAX / 2),
            KeyCode::Enter => return self.start_edit(),
            KeyCode::Char('r') => return self.reset_or_remove(false),
            KeyCode::Char('x') | KeyCode::Delete => return self.reset_or_remove(true),
            KeyCode::Char('/') => return self.start_search(),
            KeyCode::Char('f') => return self.next_finding(),
            KeyCode::Char('d') => {
                self.open_document();
                return DashboardAction::Redraw;
            }
            KeyCode::Char('[') | KeyCode::Char(']') => {
                self.settings_editor.notice =
                    Some("Move up and Move down apply inside the picker roots manager".into());
            }
            KeyCode::Char('a') => {
                self.settings_editor.notice =
                    Some("Add entries inside a collection manager".into());
            }
            _ => {}
        }
        DashboardAction::Redraw
    }

    fn start_search(&mut self) -> DashboardAction {
        let snapshot = Snapshot {
            category: self.settings_editor.category,
            selected: self.settings_editor.selected.clone(),
            list_scroll: self.settings_editor.list_scroll.get(),
            focus: self.settings_editor.focus,
        };
        self.settings_editor.search = Some(Search {
            query: String::new(),
            cursor: TextCursor::default(),
            selected: 0,
            scroll: Cell::new(0),
            return_to: snapshot,
        });
        self.settings_editor.detail_scroll.set(0);
        DashboardAction::Redraw
    }

    fn search_hits(&self) -> Vec<Row> {
        let Some(report) = self.report() else {
            return Vec::new();
        };
        let query = self
            .settings_editor
            .search
            .as_ref()
            .map(|search| search.query.as_str())
            .unwrap_or("");
        if query.trim().is_empty() {
            return Vec::new();
        }
        top_items(report)
            .into_iter()
            .filter(|row| search_matches(row, query))
            .collect()
    }

    fn search_key(&mut self, key: KeyEvent) -> DashboardAction {
        match key.code {
            KeyCode::Esc => {
                let Some(search) = self.settings_editor.search.take() else {
                    return DashboardAction::Redraw;
                };
                let editor = &mut self.settings_editor;
                editor.category = search.return_to.category;
                editor.selected = search.return_to.selected;
                editor.list_scroll.set(search.return_to.list_scroll);
                editor.focus = search.return_to.focus;
                editor.detail_scroll.set(0);
            }
            KeyCode::Enter => {
                let hits = self.search_hits();
                let selected = self
                    .settings_editor
                    .search
                    .as_ref()
                    .map_or(0, |search| search.selected);
                let Some(row) = hits.get(selected).cloned() else {
                    return DashboardAction::Redraw;
                };
                let Some(search) = self.settings_editor.search.take() else {
                    return DashboardAction::Redraw;
                };
                let editor = &mut self.settings_editor;
                editor.search_return = Some(search.return_to);
                editor.category = category_index(row.group);
                editor.selected = Some(row.id);
                editor.focus = Focus::List;
                editor.list_scroll.set(0);
                editor.detail_scroll.set(0);
            }
            KeyCode::Down => {
                let last = self.search_hits().len().saturating_sub(1);
                if let Some(search) = &mut self.settings_editor.search {
                    search.selected = (search.selected + 1).min(last);
                    search.scroll.set(0);
                }
                self.settings_editor.detail_scroll.set(0);
            }
            KeyCode::Up => {
                if let Some(search) = &mut self.settings_editor.search {
                    search.selected = search.selected.saturating_sub(1);
                    search.scroll.set(0);
                }
                self.settings_editor.detail_scroll.set(0);
            }
            KeyCode::PageDown => {
                let last = self.search_hits().len().saturating_sub(1);
                if let Some(search) = &mut self.settings_editor.search {
                    search.selected = (search.selected + 10).min(last);
                }
            }
            KeyCode::PageUp => {
                if let Some(search) = &mut self.settings_editor.search {
                    search.selected = search.selected.saturating_sub(10);
                }
            }
            KeyCode::Home => {
                if let Some(search) = &mut self.settings_editor.search {
                    search.selected = 0;
                }
            }
            KeyCode::End => {
                let last = self.search_hits().len().saturating_sub(1);
                if let Some(search) = &mut self.settings_editor.search {
                    search.selected = last;
                }
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                let editor = &mut self.settings_editor;
                if let Some(search) = &mut editor.search {
                    search.cursor.insert(&mut search.query, &ch.to_string());
                    search.selected = 0;
                    search.scroll.set(0);
                }
                editor.detail_scroll.set(0);
            }
            code => {
                let editor = &mut self.settings_editor;
                if let Some(search) = &mut editor.search {
                    search.cursor.key(&mut search.query, code);
                    search.selected = 0;
                    search.scroll.set(0);
                }
                editor.detail_scroll.set(0);
            }
        }
        DashboardAction::Redraw
    }

    fn next_finding(&mut self) -> DashboardAction {
        let Some(report) = self.report() else {
            self.settings_editor.notice = Some(WAITING.into());
            return DashboardAction::Redraw;
        };
        if report.findings.is_empty() {
            self.settings_editor.notice = Some("No settings findings".into());
            return DashboardAction::Redraw;
        }
        let mut targets: Vec<Option<String>> = Vec::new();
        for finding in &report.findings {
            let key = finding.key.as_deref();
            let target = key
                .and_then(|key| {
                    ovrcr_protocol::entries().iter().find(|entry| {
                        key == entry.path
                            || key.starts_with(&format!("{}[", entry.path))
                            || key.starts_with(&format!("{}.", entry.path))
                    })
                })
                .map(|entry| entry.path.to_string());
            targets.push(target);
        }
        let index = target_index(self.settings_editor.finding_cursor, targets.len());
        self.settings_editor.finding_cursor = index.saturating_add(1);
        if let Some(Some(target)) = targets.get(index)
            && let Some(group) = category_of_row(target)
        {
            self.settings_editor.category = category_index(group);
            self.settings_editor.selected = Some(target.clone());
            self.settings_editor.focus = Focus::List;
            self.settings_editor.list_scroll.set(0);
            self.settings_editor.detail_scroll.set(0);
            return DashboardAction::Redraw;
        }
        self.open_document();
        DashboardAction::Redraw
    }

    fn overlay_key(&mut self, key: KeyEvent) -> DashboardAction {
        let Some(mut overlay) = self.settings_editor.overlays.pop() else {
            return DashboardAction::Redraw;
        };
        if key.code == KeyCode::Enter
            && !matches!(&overlay, Overlay::Document(_))
            && self.editing_disabled()
        {
            self.settings_editor.overlays.push(overlay);
            return DashboardAction::Redraw;
        }
        let mut keep = true;
        let mut pending_open: Option<Row> = None;
        let mut action = DashboardAction::Redraw;
        match &mut overlay {
            Overlay::Document(document) => match key.code {
                KeyCode::Esc => keep = false,
                KeyCode::Down | KeyCode::Char('j') => {
                    document.scroll.set(document.scroll.get().saturating_add(1));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    document.scroll.set(document.scroll.get().saturating_sub(1));
                }
                KeyCode::PageDown => {
                    document.scroll.set(document.scroll.get().saturating_add(8));
                }
                KeyCode::PageUp => {
                    document.scroll.set(document.scroll.get().saturating_sub(8));
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    document.scroll.set(document.scroll.get().saturating_add(4));
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    document.scroll.set(document.scroll.get().saturating_sub(4));
                }
                KeyCode::Home => document.scroll.set(0),
                KeyCode::End => document.scroll.set(usize::MAX / 2),
                _ => {}
            },
            Overlay::Manager(manager) => {
                if key.code == KeyCode::Esc {
                    keep = false;
                    self.settings_editor.selected = Some(manager.row.id.clone());
                    self.settings_editor.focus = Focus::List;
                    self.settings_editor.detail_scroll.set(0);
                } else {
                    let parent = self.manager_parent(manager);
                    let rows = self.manager_rows_for(&parent);
                    if !rows.is_empty() {
                        let current = manager
                            .selected
                            .as_ref()
                            .and_then(|id| rows.iter().position(|row| &row.id == id))
                            .unwrap_or(0);
                        match key.code {
                            KeyCode::Down | KeyCode::Char('j') => {
                                let next = (current + 1).min(rows.len() - 1);
                                manager.selected = Some(rows[next].id.clone());
                                self.settings_editor.detail_scroll.set(0);
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                let next = current.saturating_sub(1);
                                manager.selected = Some(rows[next].id.clone());
                                self.settings_editor.detail_scroll.set(0);
                            }
                            KeyCode::PageDown => {
                                let next = (current + PICK_VISIBLE_ROWS).min(rows.len() - 1);
                                manager.selected = Some(rows[next].id.clone());
                            }
                            KeyCode::PageUp => {
                                let next = current.saturating_sub(PICK_VISIBLE_ROWS);
                                manager.selected = Some(rows[next].id.clone());
                            }
                            KeyCode::Home => manager.selected = Some(rows[0].id.clone()),
                            KeyCode::End => {
                                manager.selected = Some(rows[rows.len() - 1].id.clone());
                            }
                            KeyCode::Enter => {
                                pending_open = Some(rows[current].clone());
                            }
                            KeyCode::Char('a') => {
                                pending_open = rows.last().cloned();
                            }
                            KeyCode::Char('x') | KeyCode::Delete => {
                                let row = rows[current].clone();
                                if row.id.ends_with('+') {
                                    self.settings_editor.notice =
                                        Some("Nothing to remove here".into());
                                } else if let Some((path, value)) = row.reset.clone() {
                                    let next = rows
                                        .get(current + 1)
                                        .or_else(|| {
                                            current
                                                .checked_sub(1)
                                                .and_then(|previous| rows.get(previous))
                                        })
                                        .map(|row| row.id.clone());
                                    manager.selected = next;
                                    action = self.send_setting(&row.id, path, value);
                                } else {
                                    self.settings_editor.notice =
                                        Some("Nothing to remove here".into());
                                }
                            }
                            KeyCode::Char('r') => {
                                let parent = self.manager_parent(manager);
                                if parent.reset.is_some() {
                                    action =
                                        self.send_setting(&parent.id, parent.path.clone(), None);
                                } else {
                                    self.settings_editor.notice =
                                        Some(format!("{} is already its default", parent.label));
                                }
                            }
                            KeyCode::Char('[') => {
                                action = self.reorder_root(manager, -1);
                            }
                            KeyCode::Char(']') => {
                                action = self.reorder_root(manager, 1);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Overlay::Theme(picker) => {
                match key.code {
                    KeyCode::Esc => {
                        keep = false;
                        self.settings_editor.preview_theme = None;
                        self.settings_editor.preview_request = None;
                        self.settings_editor.selected = Some(ThemeId::KEY.into());
                        self.settings_editor.focus = Focus::List;
                        self.settings_editor.detail_scroll.set(0);
                    }
                    KeyCode::Enter => {
                        let matches = theme_matches(&picker.query);
                        if let Some(theme) = matches.get(picker.selected).copied() {
                            keep = false;
                            self.settings_editor.preview_theme = Some(theme);
                            self.settings_editor.selected = Some(ThemeId::KEY.into());
                            self.settings_editor.focus = Focus::List;
                            action = self.send_setting(
                                ThemeId::KEY,
                                ThemeId::KEY.into(),
                                Some(toml_string(theme.as_str())),
                            );
                        }
                    }
                    KeyCode::Down => move_theme_selection(picker, 1),
                    KeyCode::Up => move_theme_selection(picker, -1),
                    KeyCode::PageDown => move_theme_selection(picker, PICK_VISIBLE_ROWS as isize),
                    KeyCode::PageUp => move_theme_selection(picker, -(PICK_VISIBLE_ROWS as isize)),
                    KeyCode::Home => picker.selected = 0,
                    KeyCode::End => {
                        picker.selected = theme_matches(&picker.query).len().saturating_sub(1);
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        picker.cursor.insert(&mut picker.query, &ch.to_string());
                        picker.selected = 0;
                        picker.scroll.set(0);
                    }
                    KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right => {
                        picker.cursor.key(&mut picker.query, key.code);
                        picker.selected = 0;
                        picker.scroll.set(0);
                    }
                    _ => {}
                }
                if keep {
                    let matches = theme_matches(&picker.query);
                    self.settings_editor.preview_theme = matches.get(picker.selected).copied();
                }
            }
            Overlay::Pick(picker) => {
                let filtered = split_filtered(&picker.options, &picker.query);
                let count = filtered.len();
                match key.code {
                    KeyCode::Esc => {
                        keep = false;
                        self.settings_editor.detail_scroll.set(0);
                    }
                    KeyCode::Enter => {
                        if let Some(item) = filtered
                            .get(picker.selected.min(count.saturating_sub(1)))
                            .map(|item| (*item).clone())
                        {
                            keep = false;
                            let row = picker.row.clone();
                            action = self.send_setting(&row.id, row.path.clone(), Some(item.value));
                        }
                    }
                    KeyCode::Down => {
                        picker.selected = (picker.selected + 1).min(count.saturating_sub(1))
                    }
                    KeyCode::Up => picker.selected = picker.selected.saturating_sub(1),
                    KeyCode::PageDown => {
                        picker.selected =
                            (picker.selected + PICK_VISIBLE_ROWS).min(count.saturating_sub(1))
                    }
                    KeyCode::PageUp => {
                        picker.selected = picker.selected.saturating_sub(PICK_VISIBLE_ROWS)
                    }
                    KeyCode::Home => picker.selected = 0,
                    KeyCode::End => picker.selected = count.saturating_sub(1),
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        picker.cursor.insert(&mut picker.query, &ch.to_string());
                        picker.selected = 0;
                        picker.scroll.set(0);
                    }
                    KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right => {
                        picker.cursor.key(&mut picker.query, key.code);
                        picker.selected = 0;
                        picker.scroll.set(0);
                    }
                    _ => {}
                }
            }
            Overlay::Text(editor) => {
                let roots = self.settings.picker_roots.clone();
                match key.code {
                    KeyCode::Esc => {
                        keep = false;
                        self.settings_editor.detail_scroll.set(0);
                    }
                    KeyCode::Enter => {
                        if editor.row.id.ends_with('+') && editor.text.trim().is_empty() {
                            self.settings_editor.notice = Some(
                                match editor.row.id.as_str() {
                                    "agents+" => "Enter a name before saving",
                                    "launch_choices+" => "Enter a project before saving",
                                    _ => "Enter a value before saving",
                                }
                                .into(),
                            );
                        } else {
                            keep = false;
                            action = self.commit_text(editor.clone());
                        }
                    }
                    KeyCode::Tab if editor.path_picker.is_some() => {
                        let picker = editor.path_picker.as_ref().expect("checked");
                        if let Some(done) =
                            super::picker::complete_path(&editor.text, &roots, picker.selected)
                        {
                            editor.text = done;
                            editor.cursor = TextCursor::default();
                            if let Some(picker) = &mut editor.path_picker {
                                picker.selected = 0;
                                picker.listing(&editor.text, &roots);
                            }
                        }
                    }
                    KeyCode::Up if editor.path_picker.is_some() => {
                        let picker = editor.path_picker.as_mut().expect("checked");
                        let listing = picker.listing(&editor.text, &roots).clone();
                        picker.move_selection(&listing, -1);
                    }
                    KeyCode::Down if editor.path_picker.is_some() => {
                        let picker = editor.path_picker.as_mut().expect("checked");
                        let listing = picker.listing(&editor.text, &roots).clone();
                        picker.move_selection(&listing, 1);
                    }
                    KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        editor.cursor.insert(&mut editor.text, &ch.to_string());
                        if let Some(picker) = &mut editor.path_picker {
                            picker.selected = 0;
                            picker.listing(&editor.text, &roots);
                        }
                    }
                    code => {
                        editor.cursor.key(&mut editor.text, code);
                        if let Some(picker) = &mut editor.path_picker {
                            picker.listing(&editor.text, &roots);
                        }
                    }
                }
            }
        }
        if keep {
            self.settings_editor.overlays.push(overlay);
        }
        if let Some(row) = pending_open {
            return self.manager_activate(row);
        }
        action
    }

    fn manager_activate(&mut self, row: Row) -> DashboardAction {
        if self.editing_disabled() {
            return DashboardAction::Redraw;
        }
        if row.id.ends_with('+') {
            return self.open_text(row);
        }
        match row.kind.clone() {
            Kind::Pick(_) => self.open_pick(row),
            Kind::Text | Kind::Path | Kind::Toml | Kind::Typed(_) => self.open_text(row),
            Kind::Toggle(_) => self.activate_selected(),
            _ => DashboardAction::Redraw,
        }
    }

    fn commit_text(&mut self, editor: TextEditor) -> DashboardAction {
        let Some(row) = self.latest_row(&editor.row.id) else {
            self.settings_editor.notice = Some("The Server reading no longer has this row".into());
            return DashboardAction::Redraw;
        };
        let text = editor.text.trim().to_string();
        let value = match &row.kind {
            Kind::Toml | Kind::Typed(_) => text.clone(),
            _ => toml_string(&text),
        };
        let (path, value) = match row.id.as_str() {
            "agents+" if text.is_empty() => {
                self.settings_editor.notice = Some("Enter a name before saving".into());
                self.settings_editor.overlays.push(Overlay::Text(editor));
                return DashboardAction::Redraw;
            }
            "agents+" => (
                row.path.clone(),
                Some(format!(
                    "{{ name = {}, argv = {} }}",
                    toml_string(&text),
                    toml_array([text.as_str()])
                )),
            ),
            "launch_choices+" if text.is_empty() => {
                self.settings_editor.notice = Some("Enter a project before saving".into());
                self.settings_editor.overlays.push(Overlay::Text(editor));
                return DashboardAction::Redraw;
            }
            "launch_choices+" => {
                let request =
                    super::settings::launch_choice_request(&text, &LaunchChoice::Terminal);
                let Request::SetSetting { path, value } = request else {
                    unreachable!()
                };
                (path, value)
            }
            // An emptied top-level value goes back to its default.
            _ if !row.child && text.is_empty() => (row.path.clone(), None),
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

    pub(super) fn settings_editor_paste(&mut self, text: &str) -> DashboardAction {
        let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
        if let Some(overlay) = self.settings_editor.overlays.last_mut() {
            match overlay {
                Overlay::Theme(picker) => {
                    picker.cursor.insert(&mut picker.query, &text);
                    picker.selected = 0;
                    picker.scroll.set(0);
                    let matches = theme_matches(&picker.query);
                    self.settings_editor.preview_theme = matches.first().copied();
                    return DashboardAction::Redraw;
                }
                Overlay::Pick(picker) => {
                    picker.cursor.insert(&mut picker.query, &text);
                    picker.selected = 0;
                    picker.scroll.set(0);
                    return DashboardAction::Redraw;
                }
                Overlay::Text(editor) => {
                    editor.cursor.insert(&mut editor.text, &text);
                    if let Some(picker) = &mut editor.path_picker {
                        let roots = self.settings.picker_roots.clone();
                        picker.selected = 0;
                        picker.listing(&editor.text, &roots);
                    }
                    return DashboardAction::Redraw;
                }
                Overlay::Manager(_) | Overlay::Document(_) => return DashboardAction::None,
            }
        }
        if let Some(search) = &mut self.settings_editor.search {
            search.cursor.insert(&mut search.query, &text);
            search.selected = 0;
            search.scroll.set(0);
            self.settings_editor.detail_scroll.set(0);
            return DashboardAction::Redraw;
        }
        DashboardAction::None
    }

    /// Claims the answer to an editor request: a refusal shows on its row.
    pub(super) fn settings_editor_response(
        &mut self,
        request_id: u64,
        response: &Response,
    ) -> bool {
        let Some(pending) = self.settings_editor.pending.get(&request_id) else {
            return false;
        };
        if let Response::Error { message, .. } = response {
            let row = pending.row.clone();
            self.settings_editor.pending.remove(&request_id);
            if row == "picker_roots" {
                self.settings_editor.pending_reorder = None;
            }
            self.settings_editor.refused.insert(row, message.clone());
            if self.settings_editor.preview_request == Some(request_id) {
                self.settings_editor.preview_request = None;
                self.settings_editor.preview_theme = None;
            }
            return true;
        }
        // An acknowledgement means the Server accepted the request. If the
        // confirmed reading already satisfies it exactly (a no-op save), no
        // SettingsChanged follows, so settle here; otherwise wait for the
        // published effective value.
        if let Some(pending) = self.settings_editor.pending.get_mut(&request_id) {
            pending.acknowledged = true;
        }
        let satisfied = self
            .settings_editor
            .pending
            .get(&request_id)
            .is_some_and(|pending| {
                pending.published
                    || self
                        .settings_report
                        .as_deref()
                        .is_some_and(|report| pending_exact(pending, report))
            });
        if satisfied {
            self.settings_editor.pending.remove(&request_id);
        }
        true
    }
}

fn pending_exact(pending: &Pending, report: &SettingsReport) -> bool {
    let Some(row) = report.rows.iter().find(|row| row.key == pending.report_key) else {
        return false;
    };
    match &pending.value {
        None => row.source == SettingSource::Default,
        Some(value) => {
            row.source == SettingSource::Document
                && row
                    .value
                    .as_deref()
                    .is_some_and(|actual| same_value(actual, value))
        }
    }
}

fn pending_child_changed(pending: &Pending, report: &SettingsReport) -> bool {
    let Some(row) = report.rows.iter().find(|row| row.key == pending.report_key) else {
        return false;
    };
    pending.before.as_deref() != row.value.as_deref()
}

/// Normalize one editor-submitted TOML literal and one published display value
/// without reading or validating the settings document. The Server remains the
/// validation authority; this only recognizes that semantically equal values
/// (for example `1_0` and `10`) are the same confirmed write.
fn canonical_toml(value: &toml_edit::Value) -> String {
    use toml_edit::Value;
    match value {
        Value::String(value) => format!("s:{:?}", value.value()),
        Value::Integer(value) => format!("i:{}", value.value()),
        Value::Float(value) => format!("f:{:?}", value.value()),
        Value::Boolean(value) => format!("b:{}", value.value()),
        Value::Datetime(value) => format!("d:{}", value.value()),
        Value::Array(items) => format!(
            "a:[{}]",
            items
                .iter()
                .map(canonical_toml)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::InlineTable(table) => {
            let mut pairs: Vec<String> = table
                .iter()
                .map(|(key, value)| format!("{key}={}", canonical_toml(value)))
                .collect();
            pairs.sort();
            format!("t:{{{}}}", pairs.join(","))
        }
    }
}

fn same_value(published: &str, submitted: &str) -> bool {
    if published == submitted || published == unquote(submitted) {
        return true;
    }
    if let (Ok(published), Ok(submitted)) = (
        published.parse::<toml_edit::Value>(),
        submitted.parse::<toml_edit::Value>(),
    ) {
        return canonical_toml(&published) == canonical_toml(&submitted);
    }
    // Typed arrays and the whole picker-root list can differ only in the
    // whitespace toml_edit chooses.
    if submitted.trim_start().starts_with('[') {
        let squash = |text: &str| {
            text.chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>()
        };
        return squash(published) == squash(submitted);
    }
    false
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn report_key_for_row(report: &SettingsReport, row: &str) -> String {
    let _ = report;
    let stripped = row.trim_end_matches('+');
    ovrcr_protocol::entries()
        .iter()
        .find(|entry| {
            stripped == entry.path
                || stripped.starts_with(&format!("{}.", entry.path))
                || stripped.starts_with(&format!("{}[", entry.path))
        })
        .map_or_else(|| stripped.to_string(), |entry| entry.path.to_string())
}

fn category_finding_count(report: &SettingsReport, group: &str) -> usize {
    report
        .findings
        .iter()
        .filter(|finding| {
            finding.key.as_deref().is_some_and(|key| {
                ovrcr_protocol::entries().iter().any(|entry| {
                    entry.group == group
                        && (key == entry.path
                            || key.starts_with(&format!("{}[", entry.path))
                            || key.starts_with(&format!("{}.", entry.path)))
                })
            })
        })
        .count()
}

fn target_index(current: usize, len: usize) -> usize {
    if len == 0 { 0 } else { current % len }
}

fn move_theme_selection(picker: &mut ThemePicker, delta: isize) {
    let count = theme_matches(&picker.query).len();
    if count == 0 {
        picker.selected = 0;
        return;
    }
    let next = (picker.selected as isize + delta).clamp(0, count as isize - 1) as usize;
    picker.selected = next;
    picker.scroll.set(0);
}

/// The footer's action hints for the current focus. The first hint names what
/// Enter does; later ones fit only when the terminal has room.
fn footer_hints(editor: &Editor, report: Option<&SettingsReport>) -> Vec<(&'static str, String)> {
    match editor.overlays.last() {
        Some(Overlay::Manager(manager)) => {
            let parent = report
                .and_then(|report| {
                    top_items(report)
                        .into_iter()
                        .find(|row| row.id == manager.row.id)
                })
                .unwrap_or_else(|| manager.row.clone());
            let rows = report.map_or_else(Vec::new, |report| manager_rows(report, &parent, &[]));
            let selected = manager
                .selected
                .as_ref()
                .and_then(|id| rows.iter().find(|row| &row.id == id))
                .or_else(|| rows.first());
            let mut hints = Vec::new();
            if let Some(row) = selected {
                let add = row.id.ends_with('+');
                let enter = if add {
                    "add".to_string()
                } else {
                    row.action().to_string()
                };
                hints.push(("Enter", enter));
                hints.push(("a", "add".into()));
                if !add {
                    hints.push(("x", "remove".into()));
                }
                if !add && matches!(parent.kind, Kind::Collection(Collection::Paths)) {
                    hints.push(("[ ]", "move".into()));
                }
                if parent.reset.is_some() {
                    hints.push(("r", "reset".into()));
                }
            }
            hints.push(("Esc", "back".into()));
            hints
        }
        Some(Overlay::Theme(_)) => vec![
            ("↑↓", "preview".into()),
            ("PgUp/PgDn", "page".into()),
            ("Enter", "save".into()),
            ("Esc", "cancel".into()),
        ],
        Some(Overlay::Pick(_)) => vec![
            ("↑↓", "choose".into()),
            ("Enter", "save".into()),
            ("Esc", "cancel".into()),
        ],
        Some(Overlay::Text(text)) => {
            let mut hints = vec![("Enter", "save".into())];
            if text.path_picker.is_some() {
                hints.push(("Tab", "complete".into()));
            }
            hints.push(("Esc", "cancel".into()));
            hints
        }
        Some(Overlay::Document(_)) => {
            vec![("↑↓", "scroll".into()), ("Esc", "back".into())]
        }
        None if editor.search.is_some() => vec![
            ("Type", "search".into()),
            ("↑↓", "result".into()),
            ("Enter", "open".into()),
            ("Esc", "cancel".into()),
        ],
        None if editor.focus == Focus::Categories => {
            let mut hints = vec![
                ("↑↓", "category".into()),
                ("Enter", "open".into()),
                ("/", "search".into()),
            ];
            if report.is_some_and(|report| !report.findings.is_empty()) {
                hints.push(("f", "findings".into()));
            }
            hints.push(("Esc", "close".into()));
            hints
        }
        None => {
            let mut hints = Vec::new();
            if let Some(report) = report {
                let category = categories()
                    .get(editor.category)
                    .copied()
                    .unwrap_or("Alerts");
                if let Some(row) = group_rows(report, category)
                    .iter()
                    .find(|row| Some(&row.id) == editor.selected.as_ref())
                    .cloned()
                    .or_else(|| group_rows(report, category).first().cloned())
                {
                    hints.push(("Enter", row.action().into()));
                    if row.reset.is_some() {
                        hints.push(("r", "reset".into()));
                    }
                }
            }
            hints.push(("/", "search".into()));
            let back = if editor.search_return.is_some() {
                "back"
            } else {
                "categories"
            };
            hints.push(("←", back.into()));
            hints.push(("Esc", back.into()));
            hints
        }
    }
}

/// The collection summary row a manager child belongs to. A parent request
/// (reorder/reset) keeps its pending and refusal status visible on its rows.
fn collection_parent(id: &str) -> Option<&str> {
    if let Some(base) = id.strip_suffix('+') {
        return matches!(base, "picker_roots" | "agents" | "launch_choices").then_some(base);
    }
    if id.starts_with("launch_choices.") {
        return Some("launch_choices");
    }
    ["picker_roots", "agents"].into_iter().find(|prefix| {
        id.strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('['))
    })
}

fn row_index_suffix(id: &str, prefix: &str) -> Option<usize> {
    id.strip_prefix(prefix)?
        .strip_suffix(']')?
        .parse::<usize>()
        .ok()
}

/// Keep a manager's focus on a row that still exists after a published
/// removal, reset, or external change. A valid id is preserved; otherwise the
/// nearest former index becomes visible, falling back to the first row.
fn clamp_manager_selections(editor: &mut Editor, report: &SettingsReport) {
    let tops = top_items(report);
    for overlay in &mut editor.overlays {
        let Overlay::Manager(manager) = overlay else {
            continue;
        };
        let parent = tops
            .iter()
            .find(|row| row.id == manager.row.id)
            .cloned()
            .unwrap_or_else(|| manager.row.clone());
        let rows = manager_rows(report, &parent, &[]);
        let Some(first) = rows.first() else {
            manager.selected = None;
            continue;
        };
        if manager
            .selected
            .as_ref()
            .is_some_and(|id| rows.iter().any(|row| &row.id == id))
        {
            continue;
        }
        let former = manager.selected.as_deref().and_then(|id| {
            row_index_suffix(id, "picker_roots[").or_else(|| row_index_suffix(id, "agents["))
        });
        let nearest = former
            .map(|index| index.min(rows.len().saturating_sub(1)))
            .unwrap_or(0);
        manager.selected = Some(
            rows.get(nearest)
                .map_or_else(|| first.id.clone(), |row| row.id.clone()),
        );
    }
}

fn clamp_editor(editor: &mut Editor, report: &SettingsReport) {
    let categories = categories();
    editor.category = editor.category.min(categories.len().saturating_sub(1));
    let rows = categories
        .get(editor.category)
        .map(|group| group_rows(report, group))
        .unwrap_or_default();
    if !editor
        .selected
        .as_ref()
        .is_some_and(|id| rows.iter().any(|row| &row.id == id))
    {
        editor.selected = rows.first().map(|row| row.id.clone());
    }
}

impl Dashboard {
    pub(super) fn draw_settings_editor(&self, frame: &mut Frame<'_>, now: u64) {
        let area = frame.area();
        self.settings_editor.last_area.set(area);
        frame.render_widget(Clear, area);
        frame.render_widget(Block::default().style(Style::default().bg(BASE())), area);
        let layout = settings_layout(area);
        if !self.settings_editor_fits(&layout) {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    TOO_SMALL,
                    Style::default().fg(TEXT()).add_modifier(Modifier::BOLD),
                )),
                Rect::new(area.x, area.y, area.width, 1),
            );
            if area.height > 1 {
                frame.render_widget(
                    Paragraph::new(Line::styled(RESIZE_HINT, Style::default().fg(SUBTEXT()))),
                    Rect::new(area.x, area.y + 1, area.width, 1),
                );
            }
            return;
        }
        self.draw_settings_header(frame, layout.header);
        if let Some(sidebar) = layout.sidebar {
            // The pane shares the Settings base; only the header and footer
            // use the crust bar. This keeps the theme preview diff visible
            // across the whole view, including the sidebar.
            let categories_focused = self.settings_editor.overlays.is_empty()
                && self.settings_editor.search.is_none()
                && self.settings_editor.focus == Focus::Categories;
            self.draw_categories(
                frame,
                sidebar,
                self.settings_editor.category,
                false,
                categories_focused,
            );
            frame.render_widget(
                Paragraph::new(
                    (0..layout.content.height)
                        .map(|_| "│")
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
                .style(Style::default().fg(SURFACE2()).bg(BASE())),
                Rect::new(sidebar.right(), layout.content.y, 1, layout.content.height),
            );
        }
        self.draw_settings_body(frame, layout.main, now, layout.wide);
        self.draw_settings_footer(frame, layout.footer);
    }

    fn draw_settings_header(&self, frame: &mut Frame<'_>, area: Rect) {
        let mut spans = vec![
            Span::styled(
                "Settings",
                Style::default().fg(MAUVE()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" / ", Style::default().fg(MUTED())),
            Span::styled(self.breadcrumb(), Style::default().fg(TEXT())),
        ];
        let report = self.report();
        if let Some(report) = report {
            let mut badge = String::new();
            if report.unparseable {
                badge.push_str("! dashboard.toml cannot be parsed · editing unavailable");
            } else if !report.findings.is_empty() {
                let noun = if report.findings.len() == 1 {
                    "finding"
                } else {
                    "findings"
                };
                badge = format!("! {} {noun} · f next", report.findings.len());
            }
            if !badge.is_empty() {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    badge,
                    Style::default().fg(YELLOW()).add_modifier(Modifier::BOLD),
                ));
            }
        } else {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                "waiting for Server",
                Style::default().fg(MUTED()),
            ));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(CRUST()).fg(SUBTEXT())),
            area,
        );
    }

    fn draw_settings_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let mut hints = footer_hints(&self.settings_editor, self.report());
        let more = self.settings_editor.detail_more.get();
        let scroll = self.settings_editor.detail_scroll.get();
        if self.settings_editor.overlays.is_empty() && (more > 0 || scroll > 0) {
            hints.insert(
                1.min(hints.len()),
                (
                    if scroll > 0 { "Ctrl-u" } else { "Ctrl-d" },
                    "details".into(),
                ),
            );
        }
        let mut spans = Vec::new();
        let mut used = 0usize;
        for (index, (key, action)) in hints.into_iter().enumerate() {
            let separator = if index == 0 { "" } else { " · " };
            let lead = format!(" {separator}");
            let key_text = key.to_string();
            let action_text = format!(" {action}");
            let width = Line::raw(lead.as_str()).width()
                + Line::raw(key_text.as_str()).width()
                + Line::raw(action_text.as_str()).width();
            if used + width > usize::from(area.width) {
                break;
            }
            used += width;
            spans.push(Span::styled(lead, Style::default().fg(MUTED())));
            spans.push(Span::styled(key_text, Style::default().fg(MAUVE())));
            spans.push(Span::styled(action_text, Style::default().fg(SUBTEXT())));
        }
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(CRUST()).fg(SUBTEXT())),
            area,
        );
    }

    fn breadcrumb(&self) -> String {
        let editor = &self.settings_editor;
        match editor.overlays.last() {
            Some(Overlay::Manager(manager)) => {
                format!("{} / {}", manager.row.group, manager.row.label)
            }
            Some(Overlay::Theme(_)) => "Appearance / Theme".into(),
            Some(Overlay::Pick(picker)) => {
                format!("{} / {}", picker.row.group, picker.row.label)
            }
            Some(Overlay::Text(text)) => {
                format!("{} / {}", text.row.group, text.row.label)
            }
            Some(Overlay::Document(_)) => "Document".into(),
            None if editor.search.is_some() => "Search".into(),
            None if editor.focus == Focus::Categories => "Categories".into(),
            None => self.category_name().into(),
        }
    }

    fn draw_categories(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        selected: usize,
        expanded: bool,
        focused: bool,
    ) {
        let categories = self.categories();
        let heading = usize::from(area.height > 0);
        let room = usize::from(area.height).saturating_sub(heading);
        let start = window_start(
            selected,
            categories.len(),
            room,
            self.settings_editor.category_scroll.get(),
        );
        self.settings_editor.category_scroll.set(start);
        let mut lines = Vec::new();
        if heading > 0 {
            lines.push(Line::styled(
                "Categories",
                Style::default().fg(MUTED()).add_modifier(Modifier::BOLD),
            ));
        }
        let report = self.report();
        for (index, category) in categories.iter().enumerate().skip(start).take(room) {
            let chosen = index == selected;
            let findings = report.map_or(0, |report| category_finding_count(report, category));
            let suffix = if findings > 0 { " !" } else { "" };
            let marker = if chosen {
                if focused { "› " } else { "• " }
            } else {
                ""
            };
            // Unselected labels use the full column so the longest catalog
            // group still fits the compact 80-column sidebar.
            let label_room = usize::from(area.width)
                .saturating_sub(marker.len() + suffix.len() + if expanded { 12 } else { 0 });
            let mut text = format!("{marker}{}", pad_cells(category, label_room));
            if expanded {
                let count = report
                    .map(|report| group_rows(report, category).len())
                    .unwrap_or(0);
                let noun = if count == 1 { "setting" } else { "settings" };
                text.push_str(&format!("  {count} {noun}"));
            }
            text.push_str(suffix);
            let style = if chosen {
                Style::default()
                    .bg(SURFACE0())
                    .fg(TEXT())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(SUBTEXT())
            };
            lines.push(Line::styled(text, style));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn draw_settings_body(&self, frame: &mut Frame<'_>, area: Rect, now: u64, wide: bool) {
        let editor = &self.settings_editor;
        let Some(report) = self.report() else {
            self.settings_editor.detail_more.set(0);
            frame.render_widget(
                Paragraph::new(Line::styled(WAITING, Style::default().fg(SUBTEXT()))),
                area,
            );
            return;
        };
        match editor.overlays.last() {
            Some(Overlay::Manager(manager)) => {
                self.draw_manager(frame, area, report, manager, now);
            }
            Some(Overlay::Theme(picker)) => self.draw_theme_picker(frame, area, report, picker),
            Some(Overlay::Pick(picker)) => self.draw_pick(frame, area, picker),
            Some(Overlay::Text(text)) => self.draw_text(frame, area, text),
            Some(Overlay::Document(document)) => {
                self.draw_document(frame, area, report, document, now);
            }
            None if editor.search.is_some() => self.draw_search(frame, area, report, now),
            None if editor.focus == Focus::Categories => {
                if !wide {
                    self.draw_categories(frame, area, editor.category, true, true);
                } else {
                    let (list, detail) = pane_split(area);
                    let rows = group_rows(report, self.category_name());
                    self.draw_row_lines(
                        frame,
                        list,
                        &rows,
                        None,
                        &self.settings_editor.list_scroll,
                    );
                    self.draw_settings_details(frame, detail, report, now, None, None);
                }
            }
            None => {
                let (list, detail) = pane_split(area);
                let rows = group_rows(report, self.category_name());
                let selected = editor.selected.clone();
                self.draw_row_lines(
                    frame,
                    list,
                    &rows,
                    selected.as_deref(),
                    &self.settings_editor.list_scroll,
                );
                self.draw_settings_details(frame, detail, report, now, None, None);
            }
        }
    }

    fn draw_row_lines(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        rows: &[Row],
        selected: Option<&str>,
        scroll: &Cell<usize>,
    ) {
        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "No settings in this category.",
                    Style::default().fg(MUTED()),
                )),
                area,
            );
            return;
        }
        let selected_index = selected
            .and_then(|id| rows.iter().position(|row| row.id == id))
            .unwrap_or(0);
        let start = window_start(
            selected_index,
            rows.len(),
            usize::from(area.height),
            scroll.get(),
        );
        scroll.set(start);
        let label_width = (usize::from(area.width) * 45 / 100).clamp(10, 30);
        let mut lines: Vec<Line<'static>> = Vec::new();
        for row in rows.iter().skip(start).take(usize::from(area.height)) {
            let chosen = selected == Some(row.id.as_str());
            let marker = if chosen { "› " } else { "  " };
            let mut spans = vec![Span::styled(
                marker,
                Style::default().fg(if chosen { MAUVE() } else { MUTED() }),
            )];
            let label = if row.label.is_empty() {
                row.value.clone()
            } else {
                row.label.clone()
            };
            spans.push(Span::styled(
                pad_cells(&label, label_width),
                Style::default().fg(TEXT()),
            ));
            let mut value = row.value.clone();
            if row.label.is_empty()
                || (row.child && matches!(row.kind, Kind::Path) && row.label == row.value)
            {
                value.clear();
            }
            let status = self.row_status(row);
            let room = usize::from(area.width).saturating_sub(2 + label_width + 2 + status.len());
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                clip_text(&value, room),
                Style::default().fg(if chosen { TEXT() } else { SUBTEXT() }),
            ));
            if !status.is_empty() {
                spans.push(Span::styled(status, Style::default().fg(PEACH())));
            }
            let line = Line::from(spans);
            lines.push(if chosen {
                line.style(Style::default().bg(SURFACE0()))
            } else {
                line
            });
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn row_status(&self, row: &Row) -> String {
        let mut status = String::new();
        let parent = collection_parent(&row.id);
        if self.settings_editor.pending.values().any(|pending| {
            pending.row == row.id || parent.is_some_and(|parent| pending.row == parent)
        }) {
            status.push_str("  saving…");
        }
        if self.settings_editor.refused.contains_key(&row.id)
            || parent.is_some_and(|parent| self.settings_editor.refused.contains_key(parent))
            || row.finding.is_some()
        {
            status.push_str("  !");
        }
        status
    }

    fn draw_settings_details(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        report: &SettingsReport,
        now: u64,
        row: Option<Row>,
        manager: Option<&Manager>,
    ) {
        let width = usize::from(area.width).saturating_sub(2);
        let mut lines = if let Some(manager) = manager {
            self.manager_detail_lines(manager, report, now, width)
        } else if let Some(row) = row {
            self.row_detail_lines(&row, report, now, width)
        } else if self.settings_editor.focus == Focus::Categories
            && self.settings_editor.search.is_none()
        {
            self.category_detail_lines(report, now, width)
        } else {
            self.selected_detail_lines(report, now, width)
        };
        if let Some(notice) = &self.settings_editor.notice {
            let mut notice_lines = Vec::new();
            push_wrapped(
                &mut notice_lines,
                &format!("! {notice}"),
                Style::default().fg(PEACH()),
                width,
                "",
            );
            notice_lines.extend(lines);
            lines = notice_lines;
        }
        let total = lines.len();
        let height = usize::from(area.height);
        let max_scroll = total.saturating_sub(height);
        self.settings_editor.detail_more.set(max_scroll);
        let scroll = self.settings_editor.detail_scroll.get().min(max_scroll);
        self.settings_editor.detail_scroll.set(scroll);
        let inner = Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(1),
            area.height,
        );
        frame.render_widget(
            Paragraph::new(lines).scroll((scroll.min(u16::MAX as usize) as u16, 0)),
            inner,
        );
    }

    fn category_detail_lines(
        &self,
        report: &SettingsReport,
        now: u64,
        width: usize,
    ) -> Vec<Line<'static>> {
        let category = self.category_name();
        let rows = group_rows(report, category);
        let mut lines = vec![Line::styled(
            category.to_string(),
            Style::default().fg(TEXT()).add_modifier(Modifier::BOLD),
        )];
        let noun = if rows.len() == 1 {
            "setting"
        } else {
            "settings"
        };
        lines.push(Line::styled(
            format!("{} {noun} · Enter opens this category", rows.len()),
            Style::default().fg(SUBTEXT()),
        ));
        let findings = category_finding_count(report, category);
        if findings > 0 {
            lines.push(Line::styled(
                format!(
                    "! {findings} finding{} in this category · f next",
                    if findings == 1 { "" } else { "s" }
                ),
                Style::default().fg(YELLOW()),
            ));
        }
        let labels: Vec<String> = rows.iter().map(|row| row.label.clone()).collect();
        if !labels.is_empty() {
            for line in wrap(&labels.join(" · "), width) {
                lines.push(Line::styled(line, Style::default().fg(MUTED())));
            }
        }
        self.push_document_lines(&mut lines, report, now, width);
        lines
    }

    fn selected_detail_lines(
        &self,
        report: &SettingsReport,
        now: u64,
        width: usize,
    ) -> Vec<Line<'static>> {
        let Some(row) = self.selected_row() else {
            let mut lines = vec![Line::styled(
                "Select a setting to see its details.",
                Style::default().fg(MUTED()),
            )];
            self.push_document_lines(&mut lines, report, now, width);
            return lines;
        };
        self.row_detail_lines(&row, report, now, width)
    }

    fn row_detail_lines(
        &self,
        row: &Row,
        report: &SettingsReport,
        now: u64,
        width: usize,
    ) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        push_wrapped(
            &mut lines,
            &row.label,
            Style::default().fg(TEXT()).add_modifier(Modifier::BOLD),
            width,
            "",
        );
        push_wrapped(
            &mut lines,
            &format!("Value: {}", row.value),
            Style::default().fg(TEXT()),
            width,
            "",
        );
        if let Some(refused) = self.settings_editor.refused.get(&row.id) {
            push_wrapped(
                &mut lines,
                &format!("refused: {refused}"),
                Style::default().fg(RED()),
                width,
                "",
            );
        }
        if self
            .settings_editor
            .pending
            .values()
            .any(|pending| pending.row == row.id)
        {
            lines.push(Line::styled(
                "saving… waiting for the Server reading".to_string(),
                Style::default().fg(MUTED()),
            ));
        }
        if let Some(finding) = &row.finding {
            push_wrapped(
                &mut lines,
                &format!("! {finding}"),
                Style::default().fg(YELLOW()),
                width,
                "",
            );
        }
        // Permission/access/cost guidance precedes metadata; Source, Default
        // and Reset stay reachable through the fixed details scroll.
        for about in &row.about {
            push_wrapped(&mut lines, about, Style::default().fg(SUBTEXT()), width, "");
        }
        if let Some(source) = row.source {
            let text = match source {
                SettingSource::Document => "Source: dashboard.toml",
                SettingSource::Default => "Source: default",
            };
            lines.push(Line::styled(
                text.to_string(),
                Style::default().fg(if source == SettingSource::Document {
                    PEACH()
                } else {
                    MUTED()
                }),
            ));
            push_wrapped(
                &mut lines,
                &format!("Default: {}", row.default.as_deref().unwrap_or("unset")),
                Style::default().fg(MUTED()),
                width,
                "",
            );
            if source == SettingSource::Document {
                let reset = if row.child {
                    format!(
                        "Reset: r clears the whole {}; x removes this entry",
                        row.group
                    )
                } else {
                    format!(
                        "Reset: r removes this override and falls back to {}",
                        row.default.as_deref().unwrap_or("unset")
                    )
                };
                push_wrapped(&mut lines, &reset, Style::default().fg(PEACH()), width, "");
            }
        }
        if !row.path.is_empty() {
            push_wrapped(
                &mut lines,
                &format!("key: {}", row.path),
                Style::default().fg(MUTED()),
                width,
                "",
            );
        }
        if let Kind::Collection(collection) = &row.kind {
            let entries = collection_entries(report, *collection);
            let text = if entries.is_empty() {
                "empty".to_string()
            } else {
                entries.join(" · ")
            };
            push_wrapped(
                &mut lines,
                &format!("Entries: {text}"),
                Style::default().fg(SUBTEXT()),
                width,
                "",
            );
        }
        self.push_document_lines(&mut lines, report, now, width);
        lines
    }

    fn manager_detail_lines(
        &self,
        manager: &Manager,
        report: &SettingsReport,
        now: u64,
        width: usize,
    ) -> Vec<Line<'static>> {
        let parent = top_items(report)
            .into_iter()
            .find(|row| row.id == manager.row.id)
            .unwrap_or_else(|| manager.row.clone());
        let rows = manager_rows(report, &parent, &self.launch_presets());
        let selected = manager
            .selected
            .as_ref()
            .and_then(|id| rows.iter().find(|row| &row.id == id))
            .cloned()
            .or_else(|| rows.first().cloned());
        let mut lines = vec![Line::styled(
            format!("{} · {}", parent.group, parent.label),
            Style::default().fg(TEXT()).add_modifier(Modifier::BOLD),
        )];
        if let Some(row) = &selected {
            if row.label.is_empty() {
                push_wrapped(
                    &mut lines,
                    &format!("Folder: {}", row.value),
                    Style::default().fg(TEXT()),
                    width,
                    "",
                );
            } else {
                push_wrapped(
                    &mut lines,
                    &format!("Entry: {}", row.label),
                    Style::default().fg(TEXT()),
                    width,
                    "",
                );
            }
            for about in &row.about {
                push_wrapped(&mut lines, about, Style::default().fg(SUBTEXT()), width, "");
            }
            if let Some(source) = parent.source {
                lines.push(Line::styled(
                    match source {
                        SettingSource::Document => "Source: dashboard.toml",
                        SettingSource::Default => "Source: default",
                    }
                    .to_string(),
                    Style::default().fg(if source == SettingSource::Document {
                        PEACH()
                    } else {
                        MUTED()
                    }),
                ));
                lines.push(Line::styled(
                    format!("Default: {}", parent.default.as_deref().unwrap_or("unset")),
                    Style::default().fg(MUTED()),
                ));
                lines.push(Line::styled(
                    if source == SettingSource::Document {
                        "Reset: r clears the collection override"
                    } else {
                        "Reset: collection uses its default; r is unavailable"
                    }
                    .to_string(),
                    Style::default().fg(PEACH()),
                ));
            }
            if let Some(finding) = &row.finding {
                push_wrapped(
                    &mut lines,
                    &format!("! {finding}"),
                    Style::default().fg(YELLOW()),
                    width,
                    "",
                );
            }
            let collection_pending = self
                .settings_editor
                .pending
                .values()
                .any(|pending| pending.row == parent.id);
            if let Some(refused) = self
                .settings_editor
                .refused
                .get(&row.id)
                .or_else(|| self.settings_editor.refused.get(&parent.id))
            {
                push_wrapped(
                    &mut lines,
                    &format!("refused: {refused}"),
                    Style::default().fg(RED()),
                    width,
                    "",
                );
            }
            if collection_pending
                || self
                    .settings_editor
                    .pending
                    .values()
                    .any(|pending| pending.row == row.id)
            {
                lines.push(Line::styled(
                    "saving… waiting for the Server reading".to_string(),
                    Style::default().fg(MUTED()),
                ));
            }
            let add = row.id.ends_with('+');
            let collection = match &parent.kind {
                Kind::Collection(collection) => Some(*collection),
                _ => None,
            };
            let mut operations = match (add, collection) {
                (true, Some(Collection::Paths)) => "Enter adds a folder · a adds · Esc back",
                (true, Some(Collection::Agents)) => "Enter adds an agent · a adds · Esc back",
                (true, Some(Collection::Launches)) => "Enter adds a project · a adds · Esc back",
                (false, Some(Collection::Paths)) => {
                    "Enter edits this folder · x removes this entry · [ ] moves it · Esc back"
                }
                (false, Some(Collection::Agents)) => {
                    "Enter edits argv · x removes this entry · Esc back"
                }
                (false, Some(Collection::Launches)) => {
                    "Enter chooses Terminal or an agent · x removes this entry · Esc back"
                }
                _ => "Esc back",
            }
            .to_string();
            if parent.reset.is_some() {
                operations.push_str(" · r resets the collection");
            }
            lines.push(Line::styled(operations, Style::default().fg(MUTED())));
        }
        self.push_document_lines(&mut lines, report, now, width);
        lines
    }

    fn push_document_lines(
        &self,
        lines: &mut Vec<Line<'static>>,
        report: &SettingsReport,
        now: u64,
        width: usize,
    ) {
        let read = ovrcr_protocol::freshness::age_ms(report.read_unix_ms, now)
            .map(|age| format!("{}s ago", age / 1_000))
            .unwrap_or_else(|| "at an unverifiable time".into());
        push_wrapped(
            lines,
            &format!("Document: {} · read {read}", report.path.display()),
            Style::default().fg(MUTED()),
            width,
            "",
        );
        if report.unparseable {
            push_wrapped(
                lines,
                &format!("! {UNPARSEABLE}"),
                Style::default().fg(YELLOW()),
                width,
                "",
            );
        }
        let loose = loose_findings(report);
        if !loose.is_empty() {
            lines.push(Line::styled(
                "Findings".to_string(),
                Style::default().fg(YELLOW()).add_modifier(Modifier::BOLD),
            ));
            for finding in loose {
                push_wrapped(
                    lines,
                    &format!("! {finding}"),
                    Style::default().fg(YELLOW()),
                    width,
                    "",
                );
            }
        }
    }

    fn draw_manager(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        report: &SettingsReport,
        manager: &Manager,
        now: u64,
    ) {
        let parent = top_items(report)
            .into_iter()
            .find(|row| row.id == manager.row.id)
            .unwrap_or_else(|| manager.row.clone());
        let rows = manager_rows(report, &parent, &self.launch_presets());
        let (list, detail) = pane_split(area);
        let selected = manager.selected.clone();
        self.draw_row_lines(frame, list, &rows, selected.as_deref(), &manager.scroll);
        self.draw_settings_details(frame, detail, report, now, None, Some(manager));
    }

    fn draw_search(&self, frame: &mut Frame<'_>, area: Rect, report: &SettingsReport, now: u64) {
        let Some(search) = &self.settings_editor.search else {
            return;
        };
        let (list, detail) = pane_split(area);
        let input = Rect::new(list.x, list.y, list.width, 1);
        let (display, _) = search
            .cursor
            .display(&search.query, usize::from(input.width).saturating_sub(9));
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Search: ", Style::default().fg(MAUVE())),
                Span::styled(display, Style::default().fg(TEXT())),
            ])),
            input,
        );
        let hits = self.search_hits();
        let results = Rect::new(
            list.x,
            list.y.saturating_add(1),
            list.width,
            list.height.saturating_sub(1),
        );
        if search.query.trim().is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "Type to search settings by label or key.",
                    Style::default().fg(MUTED()),
                )),
                results,
            );
            self.settings_editor.detail_more.set(0);
            return;
        }
        if hits.is_empty() {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled(
                        format!("No settings match \"{}\".", search.query),
                        Style::default().fg(TEXT()),
                    ),
                    Line::styled(
                        "Backspace to revise · Esc to cancel",
                        Style::default().fg(MUTED()),
                    ),
                ]),
                results,
            );
            self.settings_editor.detail_more.set(0);
            return;
        }
        let selected = search.selected.min(hits.len() - 1);
        let rows: Vec<Row> = hits
            .iter()
            .map(|hit| Row {
                id: hit.id.clone(),
                group: hit.group,
                label: format!("{} · {}", hit.group, hit.label),
                value: hit.value.clone(),
                source: hit.source,
                default: hit.default.clone(),
                finding: hit.finding.clone(),
                about: hit.about.clone(),
                child: false,
                kind: hit.kind.clone(),
                path: hit.path.clone(),
                reset: hit.reset.clone(),
                whole_list: hit.whole_list.clone(),
            })
            .collect();
        self.draw_row_lines(
            frame,
            results,
            &rows,
            rows.get(selected).map(|row| row.id.as_str()),
            &search.scroll,
        );
        let selected_row = rows.get(selected).cloned();
        self.draw_settings_details(frame, detail, report, now, selected_row, None);
    }

    fn draw_theme_picker(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        report: &SettingsReport,
        picker: &ThemePicker,
    ) {
        let areas = theme_areas(area);
        frame.render_widget(
            Paragraph::new(Line::styled(
                "Choose a theme",
                Style::default().fg(TEXT()).add_modifier(Modifier::BOLD),
            )),
            Rect::new(area.x, area.y, area.width, 1),
        );
        frame.render_widget(
            Paragraph::new(Line::styled(
                "The highlighted palette previews this view; Enter saves.",
                Style::default().fg(SUBTEXT()),
            )),
            Rect::new(area.x, area.y.saturating_add(1), area.width, 1),
        );
        let (lines, matches) = theme_lines(&picker.query);
        let saved = report.settings.theme;
        let (display, _) = picker.cursor.display(
            &picker.query,
            usize::from(areas.search.width).saturating_sub(20),
        );
        let count = format!("{} of 80 themes", matches.len());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Find a theme: ", Style::default().fg(MAUVE())),
                Span::styled(display, Style::default().fg(TEXT())),
                Span::raw("  "),
                Span::styled(count, Style::default().fg(MUTED())),
            ])),
            areas.search,
        );
        let options_height = usize::from(areas.options.height);
        if matches.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "No matches. Try another theme name.",
                    Style::default().fg(SUBTEXT()),
                )),
                areas.options,
            );
        } else {
            let selected_line = lines
                .iter()
                .position(
                    |line| matches!(line, PickerLine::Theme(index, _) if *index == picker.selected),
                )
                .unwrap_or(0);
            let start = window_start(
                selected_line,
                lines.len(),
                options_height,
                picker.scroll.get(),
            );
            picker.scroll.set(start);
            let mut rendered = Vec::new();
            for line in lines.iter().skip(start).take(options_height) {
                match line {
                    PickerLine::Heading(name) => rendered.push(Line::styled(
                        format!("  {name}"),
                        Style::default().fg(SUBTEXT()).add_modifier(Modifier::BOLD),
                    )),
                    PickerLine::Theme(index, theme) => {
                        let chosen = *index == picker.selected;
                        let mut spans = vec![Span::styled(
                            if chosen { "› " } else { "  " },
                            Style::default().fg(if chosen { MAUVE() } else { MUTED() }),
                        )];
                        let tag = if *theme == saved { "  Saved" } else { "" };
                        let label_room =
                            usize::from(areas.options.width).saturating_sub(2 + tag.len());
                        spans.push(Span::styled(
                            clip_text(theme.label(), label_room),
                            Style::default().fg(TEXT()),
                        ));
                        if !tag.is_empty() {
                            spans.push(Span::styled(tag, Style::default().fg(SUBTEXT())));
                        }
                        let line = Line::from(spans);
                        rendered.push(if chosen {
                            line.style(Style::default().bg(SURFACE0()))
                        } else {
                            line
                        });
                    }
                }
            }
            frame.render_widget(Paragraph::new(rendered), areas.options);
        }
        let preview = matches.get(picker.selected).copied();
        let mut summary = Vec::new();
        if let Some(notice) = &self.settings_editor.notice {
            push_wrapped(
                &mut summary,
                &format!("! {notice}"),
                Style::default().fg(PEACH()),
                usize::from(areas.summary.width),
                "",
            );
        }
        match preview {
            Some(theme) if theme != saved => summary.push(Line::from(vec![
                Span::styled("Preview: ", Style::default().fg(MAUVE())),
                Span::styled(theme.label().to_string(), Style::default().fg(TEXT())),
                Span::styled(" · Saved: ", Style::default().fg(MUTED())),
                Span::styled(saved.label().to_string(), Style::default().fg(SUBTEXT())),
            ])),
            _ => summary.push(Line::from(vec![
                Span::styled("Saved: ", Style::default().fg(MUTED())),
                Span::styled(saved.label().to_string(), Style::default().fg(TEXT())),
            ])),
        }
        if let Some(refused) = self.settings_editor.refused.get(ThemeId::KEY) {
            push_wrapped(
                &mut summary,
                &format!("refused: {refused}"),
                Style::default().fg(RED()),
                usize::from(areas.summary.width),
                "",
            );
        } else if self
            .settings_editor
            .pending
            .values()
            .any(|pending| pending.row == ThemeId::KEY)
        {
            summary.push(Line::styled(
                "saving… waiting for the Server reading",
                Style::default().fg(MUTED()),
            ));
        }
        frame.render_widget(
            Paragraph::new(summary).wrap(Wrap { trim: false }),
            areas.summary,
        );
    }

    fn draw_pick(&self, frame: &mut Frame<'_>, area: Rect, picker: &PickEditor) {
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!("Choose {}", picker.row.label),
                Style::default().fg(TEXT()).add_modifier(Modifier::BOLD),
            )),
            Rect::new(area.x, area.y, area.width, 1),
        );
        let filtered = split_filtered(&picker.options, &picker.query);
        let (display, _) = picker
            .cursor
            .display(&picker.query, usize::from(area.width).saturating_sub(20));
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Find an option: ", Style::default().fg(MAUVE())),
                Span::styled(display, Style::default().fg(TEXT())),
                Span::raw("  "),
                Span::styled(
                    format!("{} options", filtered.len()),
                    Style::default().fg(MUTED()),
                ),
            ])),
            Rect::new(area.x, area.y.saturating_add(2), area.width, 1),
        );
        let options_area = Rect::new(
            area.x,
            area.y.saturating_add(3),
            area.width,
            area.height.saturating_sub(5),
        );
        if filtered.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "No matching options.",
                    Style::default().fg(SUBTEXT()),
                )),
                options_area,
            );
            return;
        }
        let selected = picker.selected.min(filtered.len() - 1);
        let start = window_start(
            selected,
            filtered.len(),
            usize::from(options_area.height),
            picker.scroll.get(),
        );
        picker.scroll.set(start);
        let mut lines = Vec::new();
        for (index, item) in filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(usize::from(options_area.height))
        {
            let chosen = index == selected;
            let line = Line::from(vec![
                Span::styled(
                    if chosen { "› " } else { "  " },
                    Style::default().fg(if chosen { MAUVE() } else { MUTED() }),
                ),
                Span::styled(
                    clip_text(&item.label, usize::from(area.width).saturating_sub(3)),
                    Style::default().fg(TEXT()),
                ),
            ]);
            lines.push(if chosen {
                line.style(Style::default().bg(SURFACE0()))
            } else {
                line
            });
        }
        frame.render_widget(Paragraph::new(lines), options_area);
        let summary = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
        let summary_text = self.settings_editor.notice.as_ref().map_or_else(
            || format!("Current: {}", picker.row.value),
            |notice| format!("! {notice}"),
        );
        frame.render_widget(
            Paragraph::new(Line::styled(
                summary_text,
                Style::default().fg(if self.settings_editor.notice.is_some() {
                    PEACH()
                } else {
                    SUBTEXT()
                }),
            )),
            summary,
        );
    }

    fn draw_text(&self, frame: &mut Frame<'_>, area: Rect, editor: &TextEditor) {
        let mut lines = vec![Line::styled(
            format!("Edit {}", editor.row.label),
            Style::default().fg(TEXT()).add_modifier(Modifier::BOLD),
        )];
        let (display, _) = editor
            .cursor
            .display(&editor.text, usize::from(area.width).saturating_sub(12));
        lines.push(Line::from(vec![
            Span::styled("Value: ", Style::default().fg(MAUVE())),
            Span::styled(display, Style::default().fg(TEXT())),
        ]));
        if let Some(notice) = &self.settings_editor.notice {
            push_wrapped(
                &mut lines,
                &format!("! {notice}"),
                Style::default().fg(PEACH()),
                usize::from(area.width).saturating_sub(2),
                "",
            );
        }
        match &editor.row.kind {
            Kind::Toml => lines.push(Line::styled(
                "TOML array, e.g. [\"claude\", \"--verbose\"] · Enter saves · Esc cancels"
                    .to_string(),
                Style::default().fg(MUTED()),
            )),
            Kind::Typed(expected) => lines.push(Line::styled(
                format!("Expected {expected} · Enter saves · Esc cancels"),
                Style::default().fg(MUTED()),
            )),
            Kind::Path => lines.push(Line::styled(
                "Tab completes paths · Enter saves · Esc cancels".to_string(),
                Style::default().fg(MUTED()),
            )),
            _ => lines.push(Line::styled(
                "Enter saves · Esc cancels".to_string(),
                Style::default().fg(MUTED()),
            )),
        }
        if let Some(picker) = editor.path_picker.as_ref()
            && let Some(listing) = picker.cached()
        {
            for (index, entry) in listing.entries.iter().enumerate().take(5) {
                let mark = if index == picker.selected { "›" } else { " " };
                lines.push(Line::styled(
                    format!("{mark} {}", entry.label),
                    Style::default().fg(SUBTEXT()),
                ));
            }
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    fn draw_document(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        report: &SettingsReport,
        document: &Document,
        now: u64,
    ) {
        let width = usize::from(area.width);
        let read = ovrcr_protocol::freshness::age_ms(report.read_unix_ms, now)
            .map(|age| format!("{}s ago", age / 1_000))
            .unwrap_or_else(|| "at an unverifiable time".into());
        let mut lines = vec![
            Line::styled(
                "Document",
                Style::default().fg(TEXT()).add_modifier(Modifier::BOLD),
            ),
            Line::styled(
                format!("Document: {} · read {read}", report.path.display()),
                Style::default().fg(SUBTEXT()),
            ),
        ];
        if report.unparseable {
            push_wrapped(
                &mut lines,
                &format!("! {UNPARSEABLE}"),
                Style::default().fg(YELLOW()),
                width,
                "",
            );
        }
        lines.push(Line::styled(
            format!(
                "{} settings · {} findings",
                report.rows.len(),
                report.findings.len()
            ),
            Style::default().fg(MUTED()),
        ));
        if report.findings.is_empty() {
            lines.push(Line::styled(
                "No settings findings.",
                Style::default().fg(SUBTEXT()),
            ));
        } else {
            lines.push(Line::styled(
                "Findings",
                Style::default().fg(YELLOW()).add_modifier(Modifier::BOLD),
            ));
            for finding in &report.findings {
                let key = finding.key.as_deref().unwrap_or("document");
                let line = finding
                    .line
                    .map(|line| format!("line {line}: "))
                    .unwrap_or_default();
                push_wrapped(
                    &mut lines,
                    &format!("! {key} {line}{}", finding.message),
                    Style::default().fg(YELLOW()),
                    width,
                    "",
                );
            }
        }
        let total = lines.len();
        let height = usize::from(area.height);
        let max_scroll = total.saturating_sub(height);
        self.settings_editor.detail_more.set(max_scroll);
        let scroll = document.scroll.get().min(max_scroll);
        document.scroll.set(scroll);
        frame.render_widget(
            Paragraph::new(lines).scroll((scroll.min(u16::MAX as usize) as u16, 0)),
            area,
        );
    }
}

fn push_wrapped(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    style: Style,
    width: usize,
    prefix: &str,
) {
    let room = width.saturating_sub(prefix.len()).max(1);
    for line in wrap(text, room) {
        lines.push(Line::styled(format!("{prefix}{line}"), style));
    }
}

fn collection_entries(report: &SettingsReport, collection: Collection) -> Vec<String> {
    match collection {
        Collection::Paths => report
            .settings
            .picker_roots
            .iter()
            .map(|root| root.display().to_string())
            .collect(),
        Collection::Agents => report
            .settings
            .agents
            .iter()
            .map(|agent| agent.name.clone())
            .collect(),
        Collection::Launches => report.settings.launch_choices.keys().cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{MouseButton, MouseEventKind};
    use ovrcr_protocol::{
        AgentOverride, ErrorCode, ServerEvent, ServerMessage, Settings, SettingsFinding,
        TerminalSize,
    };
    use ratatui::backend::TestBackend;
    use std::collections::{BTreeMap, HashSet};
    use std::path::PathBuf;

    fn report(settings: Settings, document: &[&str]) -> SettingsReport {
        let keys: HashSet<String> = document.iter().map(|key| key.to_string()).collect();
        SettingsReport {
            path: "/fixture/dashboard.toml".into(),
            read_unix_ms: 0,
            rows: ovrcr_protocol::setting_rows(&settings, &Settings::default(), &keys),
            settings,
            findings: Vec::new(),
            unparseable: false,
        }
    }

    fn fresh_dashboard() -> Dashboard {
        let settings = Settings {
            picker_roots: vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c"),
            ],
            agents: vec![AgentOverride {
                name: "one".into(),
                argv: vec!["one".into()],
            }],
            launch_choices: BTreeMap::from([("demo".to_string(), LaunchChoice::Terminal)]),
            ..Settings::default()
        };
        let mut dashboard = Dashboard::new(TerminalSize {
            rows: 40,
            cols: 120,
        });
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
            Box::new(report(settings, &[])),
        )));
        dashboard.open_details(super::super::quota::Details::Settings);
        dashboard
    }

    fn install(dashboard: &mut Dashboard, reading: SettingsReport) -> DashboardAction {
        dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SettingsChanged(
            Box::new(reading),
        )));
        DashboardAction::Redraw
    }

    fn choose_category(dashboard: &mut Dashboard, name: &str) {
        dashboard.settings_editor.focus = Focus::Categories;
        dashboard.settings_editor.category = category_index(name);
        dashboard.settings_editor.detail_scroll.set(0);
    }

    fn choose_row(dashboard: &mut Dashboard, id: &str) {
        dashboard.settings_editor.category =
            category_index(category_of_row(id).expect("row category"));
        dashboard.settings_editor.focus = Focus::List;
        dashboard.settings_editor.selected = Some(id.into());
        dashboard.settings_editor.list_scroll.set(0);
        dashboard.settings_editor.detail_scroll.set(0);
    }

    fn frame_text(dashboard: &mut Dashboard, cols: u16, rows: u16) -> String {
        let mut terminal = ratatui::Terminal::new(TestBackend::new(cols, rows)).unwrap();
        dashboard.draw(&mut terminal).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn frame_lines(dashboard: &mut Dashboard, cols: u16, rows: u16) -> Vec<String> {
        let mut terminal = ratatui::Terminal::new(TestBackend::new(cols, rows)).unwrap();
        dashboard.draw(&mut terminal).unwrap();
        (0..rows)
            .map(|y| {
                (0..cols)
                    .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    fn setting_request(dashboard: &mut Dashboard) -> (u64, String, Option<String>) {
        let Some(ClientMessage {
            request_id,
            request: Request::SetSetting { path, value },
        }) = dashboard.drain_one_request()
        else {
            panic!("expected SetSetting");
        };
        (request_id, path, value)
    }

    #[test]
    fn catalog_categories_keep_one_stable_row_per_setting() {
        assert_eq!(
            categories(),
            vec![
                "Alerts",
                "Appearance",
                "Workspaces",
                "Titles",
                "Usage",
                "Agents",
                "Remembered launches",
                "Operator",
                "Delegation",
                "Remote",
            ]
        );
        let reading = report(Settings::default(), &[]);
        let rows = top_items(&reading);
        let expected = if cfg!(target_os = "macos") { 33 } else { 32 };
        assert_eq!(rows.len(), expected);
        let mut ids: Vec<_> = rows.iter().map(|row| row.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), expected, "one row per setting path");
        for group in categories() {
            assert!(!group_rows(&reading, group).is_empty(), "{group}");
        }
        let roots = rows.iter().find(|row| row.id == "picker_roots").unwrap();
        assert_eq!(roots.kind, Kind::Collection(Collection::Paths));
        assert_eq!(roots.value, "0 entries");
        let agents = rows.iter().find(|row| row.id == "agents").unwrap();
        assert_eq!(agents.kind, Kind::Collection(Collection::Agents));
        assert_eq!(agents.value, "0 entries");
    }

    #[test]
    fn menu_style_navigation_enters_theme_and_unwinds_to_workspace() {
        let mut dashboard = fresh_dashboard();
        assert_eq!(dashboard.settings_editor.focus, Focus::Categories);
        assert_eq!(dashboard.category_name(), "Alerts");
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("› Alerts"), "{text}");
        assert!(!text.contains("› Appearance"), "{text}");

        dashboard.key(KeyCode::Down);
        assert_eq!(dashboard.category_name(), "Appearance");
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("› Appearance"), "{text}");
        assert!(
            !text.contains("› Theme"),
            "row marker belongs to list focus: {text}"
        );

        dashboard.key(KeyCode::Enter);
        assert_eq!(dashboard.settings_editor.focus, Focus::List);
        assert_eq!(dashboard.settings_editor.selected.as_deref(), Some("theme"));
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("• Appearance"), "{text}");
        assert!(text.contains("› Theme"), "{text}");

        dashboard.key(KeyCode::Enter);
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Theme(_))
        ));
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("Choose a theme"), "{text}");
        assert!(text.contains("Saved: Catppuccin Mocha"), "{text}");
        assert!(text.contains("› Catppuccin Mocha"), "{text}");

        dashboard.key(KeyCode::Esc);
        assert!(dashboard.settings_editor.overlays.is_empty());
        assert_eq!(dashboard.settings_editor.focus, Focus::List);
        assert_eq!(dashboard.settings_editor.preview_theme, None);
        dashboard.key(KeyCode::Esc);
        assert_eq!(dashboard.settings_editor.focus, Focus::Categories);
        dashboard.key(KeyCode::Esc);
        assert!(
            dashboard.details.is_none(),
            "third Escape returns to the workspace"
        );
    }

    #[test]
    fn every_catalog_category_is_reachable_from_the_sidebar() {
        let mut dashboard = fresh_dashboard();
        let categories = categories();
        for (index, group) in categories.iter().enumerate() {
            if index > 0 {
                dashboard.key(KeyCode::Down);
            }
            assert_eq!(dashboard.category_name(), *group);
        }
        assert_eq!(dashboard.category_name(), "Remote");
        assert!(
            ovrcr_protocol::entries()
                .iter()
                .position(|entry| entry.group == "Remote")
                .is_some()
        );
    }

    #[test]
    fn global_search_opens_context_and_restores_the_prior_place() {
        let mut dashboard = fresh_dashboard();
        dashboard.key(KeyCode::Enter);
        let prior = dashboard.settings_editor.selected.clone();
        assert_eq!(prior.as_deref(), Some("desktop_notifications"));

        dashboard.key(KeyCode::Char('/'));
        for ch in "quota.grok.home".chars() {
            dashboard.key(KeyCode::Char(ch));
        }
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("Usage · Grok home"), "{text}");
        dashboard.key(KeyCode::Enter);
        assert_eq!(dashboard.category_name(), "Usage");
        assert_eq!(
            dashboard.settings_editor.selected.as_deref(),
            Some("quota.grok.home")
        );
        assert_eq!(dashboard.settings_editor.focus, Focus::List);
        assert!(dashboard.settings_editor.search_return.is_some());

        dashboard.key(KeyCode::Esc);
        assert_eq!(dashboard.category_name(), "Alerts");
        assert_eq!(dashboard.settings_editor.selected, prior);
        assert!(dashboard.settings_editor.search_return.is_none());

        dashboard.key(KeyCode::Char('/'));
        for ch in "zzzz".chars() {
            dashboard.key(KeyCode::Char(ch));
        }
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("No settings match"), "{text}");
        dashboard.key(KeyCode::Enter);
        assert!(dashboard.drain_outbox().is_empty());
        assert!(dashboard.settings_editor.pending.is_empty());
        assert!(dashboard.settings_editor.search.is_some());
        dashboard.key(KeyCode::Esc);
        assert_eq!(dashboard.category_name(), "Alerts");
    }

    #[test]
    fn toggle_saves_immediately_and_stays_pending_until_publication() {
        let mut dashboard = fresh_dashboard();
        dashboard.key(KeyCode::Enter);
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("Enter turn on"), "{text}");
        assert!(text.contains("Turning this on may show an OS notification permission prompt"));

        dashboard.key(KeyCode::Enter);
        let (request_id, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "desktop_notifications");
        assert_eq!(value.as_deref(), Some("true"));
        assert_eq!(dashboard.settings_editor.pending.len(), 1);
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("saving…"), "{text}");

        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Ok,
        });
        assert_eq!(
            dashboard.settings_editor.pending.len(),
            1,
            "an acknowledgement is not a published value"
        );

        let mut saved = dashboard.settings.clone();
        saved.desktop_notifications = true;
        install(&mut dashboard, report(saved, &["desktop_notifications"]));
        assert!(dashboard.settings_editor.pending.is_empty());
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("Source: dashboard.toml"), "{text}");
        assert!(text.contains("Value: on"), "{text}");
    }

    #[test]
    fn refusal_stays_on_the_row_and_clears_when_the_row_is_edited_again() {
        let mut dashboard = fresh_dashboard();
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Enter);
        let (request_id, path, _) = setting_request(&mut dashboard);
        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Error {
                code: ErrorCode::InvalidRequest,
                message: "fixture save refused".into(),
            },
        });
        assert!(dashboard.settings_editor.pending.is_empty());
        assert_eq!(
            dashboard
                .settings_editor
                .refused
                .get(&path)
                .map(String::as_str),
            Some("fixture save refused")
        );
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("refused: fixture save refused"), "{text}");

        dashboard.key(KeyCode::Enter);
        assert!(setting_request(&mut dashboard).1 == "desktop_notifications");
        assert!(!dashboard.settings_editor.refused.contains_key(&path));
    }

    #[test]
    fn details_distinguish_explicit_values_and_only_document_overrides_reset() {
        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, "branch_prefix");
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("Source: default"), "{text}");
        assert!(!text.contains("Reset: r removes"), "{text}");
        dashboard.key(KeyCode::Char('r'));
        assert!(dashboard.drain_outbox().is_empty());
        assert!(
            dashboard
                .settings_editor
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("already its default"))
        );

        let settings = Settings::default();
        let reading = report(settings.clone(), &["theme", "branch_prefix"]);
        let mut dashboard = fresh_dashboard();
        dashboard.settings_editor = Default::default();
        install(&mut dashboard, reading);
        choose_row(&mut dashboard, "branch_prefix");
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("Source: dashboard.toml"), "{text}");
        assert!(text.contains("Default: feature/"), "{text}");
        assert!(text.contains("Reset: r removes"), "{text}");
        dashboard.key(KeyCode::Char('r'));
        let (_, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "branch_prefix");
        assert_eq!(value, None);
    }

    #[test]
    fn theme_picker_previews_all_palettes_and_saves_one_canonical_id() {
        let mut dashboard = fresh_dashboard();
        choose_category(&mut dashboard, "Appearance");
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Enter);
        let Some(Overlay::Theme(picker)) = dashboard.settings_editor.overlays.last() else {
            panic!("theme picker");
        };
        assert_eq!(picker.selected, 0, "opens on the Server's saved theme");
        assert_eq!(dashboard.settings_editor.preview_theme, Some(ThemeId::Dark));
        assert_eq!(theme_matches("").len(), 80);
        let (_, matches) = theme_lines("");
        assert_eq!(matches.len(), 80);
        assert_eq!(matches[54], ThemeId::Light);
        assert!(
            matches[..54]
                .iter()
                .all(|theme| theme.appearance() == ThemeAppearance::Dark)
        );
        assert!(
            matches[54..]
                .iter()
                .all(|theme| theme.appearance() == ThemeAppearance::Light)
        );

        dashboard.key(KeyCode::Down);
        assert_eq!(
            dashboard.settings_editor.preview_theme,
            Some(ThemeId::TokyoNight)
        );
        assert_eq!(
            dashboard.theme_tokens(),
            crate::theme::ThemeTokens::for_id(ThemeId::TokyoNight)
        );
        assert!(
            dashboard.drain_outbox().is_empty(),
            "highlighting never writes"
        );

        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("Preview: Tokyo Night"), "{text}");
        assert!(text.contains("Saved: Catppuccin Mocha"), "{text}");

        dashboard.settings_editor_paste("Solarized Light");
        assert_eq!(
            dashboard.settings_editor.preview_theme,
            Some(ThemeId::SolarizedLight)
        );
        dashboard.settings_editor_paste("zzz");
        assert_eq!(dashboard.settings_editor.preview_theme, None);
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("No matches"), "{text}");
        dashboard.key(KeyCode::Enter);
        assert!(dashboard.drain_outbox().is_empty());
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Theme(_))
        ));

        dashboard.key(KeyCode::Esc);
        assert_eq!(dashboard.theme_tokens(), crate::theme::ThemeTokens::dark());
        dashboard.key(KeyCode::Enter);
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Theme(_))
        ));
        dashboard.key(KeyCode::Enter);
        let (request_id, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, ThemeId::KEY);
        assert_eq!(value.as_deref(), Some("\"catppuccin-mocha\""));
        assert!(dashboard.settings_editor.preview_request.is_some());
        assert!(dashboard.settings_editor.overlays.is_empty());
        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Ok,
        });

        install(&mut dashboard, report(Settings::default(), &[ThemeId::KEY]));
        assert!(dashboard.settings_editor.pending.is_empty());
        assert_eq!(dashboard.settings_editor.preview_theme, None);
        assert_eq!(dashboard.settings_editor.preview_request, None);
    }

    #[test]
    fn older_theme_refusal_does_not_clear_a_newer_preview() {
        let mut dashboard = fresh_dashboard();
        choose_category(&mut dashboard, "Appearance");
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Enter);
        let (older_request, _, _) = {
            dashboard.key(KeyCode::Enter);
            setting_request(&mut dashboard)
        };
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Down);
        dashboard.key(KeyCode::Down);
        assert_eq!(
            dashboard.settings_editor.preview_theme,
            Some(ThemeId::Dracula)
        );
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: older_request,
            response: Response::Error {
                code: ErrorCode::InvalidRequest,
                message: "older theme refusal".into(),
            },
        });
        assert_eq!(
            dashboard.settings_editor.preview_theme,
            Some(ThemeId::Dracula)
        );
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("older theme refusal"), "{text}");
        dashboard.key(KeyCode::Esc);
        assert_eq!(dashboard.theme_tokens(), crate::theme::ThemeTokens::dark());
    }

    #[test]
    fn text_and_typed_editors_prefill_and_write_raw_values() {
        let mut dashboard = fresh_dashboard();
        install(
            &mut dashboard,
            report(
                Settings {
                    branch_prefix: "kh/".into(),
                    ..Settings::default()
                },
                &["branch_prefix"],
            ),
        );
        choose_row(&mut dashboard, "branch_prefix");
        dashboard.key(KeyCode::Enter);
        let Some(Overlay::Text(editor)) = dashboard.settings_editor.overlays.last() else {
            panic!("text editor");
        };
        assert_eq!(editor.text, "kh/", "prefilled current value");
        for _ in 0..3 {
            dashboard.key(KeyCode::Backspace);
        }
        dashboard.settings_editor_paste("main/");
        dashboard.key(KeyCode::Enter);
        let (_, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "branch_prefix");
        assert_eq!(value.as_deref(), Some("\"main/\""));
        assert!(dashboard.settings_editor.overlays.is_empty());

        choose_row(&mut dashboard, "delegation.progress_deadline_ms");
        dashboard.key(KeyCode::Enter);
        let Some(Overlay::Text(editor)) = dashboard.settings_editor.overlays.last() else {
            panic!("typed editor");
        };
        assert_eq!(editor.text, "300000");
        let length = editor.text.len();
        for _ in 0..length {
            dashboard.key(KeyCode::Backspace);
        }
        dashboard.settings_editor_paste("450000");
        dashboard.key(KeyCode::Enter);
        let (_, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "delegation.progress_deadline_ms");
        assert_eq!(value.as_deref(), Some("450000"), "typed values stay raw");
    }

    #[test]
    fn picker_cancel_and_selection_use_canonical_values() {
        #[cfg(target_os = "macos")]
        {
            let mut dashboard = fresh_dashboard();
            install(
                &mut dashboard,
                report(Settings::default(), &[ReadySoundChoice::KEY]),
            );
            choose_row(&mut dashboard, ReadySoundChoice::KEY);
            dashboard.key(KeyCode::Enter);
            let Some(Overlay::Pick(picker)) = dashboard.settings_editor.overlays.last() else {
                panic!("sound picker");
            };
            assert_eq!(
                picker
                    .options
                    .iter()
                    .map(|item| item.label.as_str())
                    .collect::<Vec<_>>(),
                ["System default", "Tap", "Chime", "Rise"]
            );
            dashboard.key(KeyCode::Esc);
            assert!(dashboard.settings_editor.overlays.is_empty());
            assert!(dashboard.drain_outbox().is_empty());

            dashboard.key(KeyCode::Enter);
            dashboard.key(KeyCode::Down);
            dashboard.key(KeyCode::Enter);
            let (_, path, value) = setting_request(&mut dashboard);
            assert_eq!(path, ReadySoundChoice::KEY);
            assert_eq!(value.as_deref(), Some("\"tap\""));
        }
    }

    #[test]
    fn collections_are_one_summary_and_managers_keep_order_and_scope() {
        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, "picker_roots");
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("3 entries"), "{text}");
        dashboard.key(KeyCode::Enter);
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Manager(_))
        ));
        let rows = dashboard.overlay_manager_rows().unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].id, "picker_roots[0]");
        assert_eq!(rows[3].id, "picker_roots+");

        dashboard.key(KeyCode::Char(']'));
        let (_, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "picker_roots");
        assert_eq!(value.as_deref(), Some("[\"/b\", \"/a\", \"/c\"]"));
        let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last() else {
            panic!("manager");
        };
        assert_eq!(
            manager.selected.as_deref(),
            Some("picker_roots[0]"),
            "the pending move keeps /a selected in the old order"
        );
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(
            text.contains("saving…"),
            "the active manager shows its pending collection edit:\n{text}"
        );

        // An unrelated report with the old order leaves the selected folder
        // alone and keeps the collection pending visible; only the published
        // reordered list moves the index.
        install(
            &mut dashboard,
            report(
                Settings {
                    picker_roots: vec![
                        PathBuf::from("/a"),
                        PathBuf::from("/b"),
                        PathBuf::from("/c"),
                    ],
                    ..Settings::default()
                },
                &[],
            ),
        );
        let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last() else {
            panic!("manager");
        };
        assert_eq!(manager.selected.as_deref(), Some("picker_roots[0]"));
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(
            text.contains("saving…"),
            "an unrelated reading must not hide the pending move:\n{text}"
        );
        install(
            &mut dashboard,
            report(
                Settings {
                    picker_roots: vec![
                        PathBuf::from("/b"),
                        PathBuf::from("/a"),
                        PathBuf::from("/c"),
                    ],
                    ..Settings::default()
                },
                &["picker_roots"],
            ),
        );
        let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last() else {
            panic!("manager");
        };
        assert_eq!(
            manager.selected.as_deref(),
            Some("picker_roots[1]"),
            "the index follows /a once its new order is published"
        );

        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, "picker_roots");
        dashboard.key(KeyCode::Enter);
        if let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last_mut() {
            manager.selected = Some("picker_roots[1]".into());
        }
        dashboard.key(KeyCode::Char('x'));
        let (_, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "picker_roots");
        assert_eq!(
            value.as_deref(),
            Some("[\"/a\", \"/c\"]"),
            "removing one default root writes the rest"
        );

        dashboard.key(KeyCode::Char('a'));
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Text(_))
        ));
        dashboard.key(KeyCode::Esc);
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Manager(_))
        ));
        dashboard.key(KeyCode::Esc);
        assert_eq!(dashboard.settings_editor.focus, Focus::List);
        assert_eq!(
            dashboard.settings_editor.selected.as_deref(),
            Some("picker_roots")
        );

        let empty = report(
            Settings {
                picker_roots: Vec::new(),
                ..Settings::default()
            },
            &[],
        );
        let mut dashboard = fresh_dashboard();
        install(&mut dashboard, empty);
        choose_row(&mut dashboard, "picker_roots");
        assert!(frame_text(&mut dashboard, 120, 40).contains("0 entries"));
        dashboard.key(KeyCode::Enter);
        assert_eq!(dashboard.overlay_manager_rows().unwrap().len(), 1);
    }

    #[test]
    fn agent_and_launch_managers_expose_their_own_operations() {
        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, "agents");
        dashboard.key(KeyCode::Enter);
        let rows = dashboard.overlay_manager_rows().unwrap();
        assert_eq!(rows[0].label, "one");
        assert_eq!(rows[1].id, "agents+");
        if let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last_mut() {
            manager.selected = Some("agents[0]".into());
        }
        dashboard.key(KeyCode::Enter);
        let Some(Overlay::Text(editor)) = dashboard.settings_editor.overlays.last() else {
            panic!("argv editor");
        };
        assert_eq!(editor.text, "[\"one\"]");
        dashboard.key(KeyCode::Esc);

        let mut dashboard = fresh_dashboard();
        let agent_launch = Settings {
            launch_choices: BTreeMap::from([(
                "demo".to_string(),
                LaunchChoice::Agent {
                    preset: "one".into(),
                    model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
                },
            )]),
            ..Settings::default()
        };
        install(&mut dashboard, report(agent_launch, &[]));
        choose_row(&mut dashboard, "launch_choices");
        dashboard.key(KeyCode::Enter);
        let rows = dashboard.overlay_manager_rows().unwrap();
        assert_eq!(rows[0].label, "demo");
        dashboard.key(KeyCode::Enter);
        let Some(Overlay::Pick(picker)) = dashboard.settings_editor.overlays.last() else {
            panic!("launch picker");
        };
        assert_eq!(picker.options[0].label, "Terminal");
        assert!(picker.options.len() >= 2);
        dashboard.key(KeyCode::Down);
        dashboard.key(KeyCode::Enter);
        let (_, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "launch_choices.demo");
        assert!(
            value
                .as_deref()
                .is_some_and(|value| value.contains("kind = \"Agent\"")),
            "{value:?}"
        );
    }

    #[test]
    fn findings_route_across_categories_and_document_details_hold_unknown_keys() {
        let mut reading = report(Settings::default(), &[]);
        reading.findings = vec![
            SettingsFinding {
                key: Some("ready_sound".into()),
                message: "expected a boolean".into(),
                line: Some(2),
            },
            SettingsFinding {
                key: Some("theme".into()),
                message: "unknown theme".into(),
                line: Some(3),
            },
            SettingsFinding {
                key: Some("colour".into()),
                message: "unknown key".into(),
                line: None,
            },
        ];
        let mut dashboard = fresh_dashboard();
        install(&mut dashboard, reading);
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("3 findings"), "{text}");
        assert!(text.contains("f next"), "{text}");

        dashboard.key(KeyCode::Char('f'));
        assert_eq!(
            dashboard.settings_editor.selected.as_deref(),
            Some("ready_sound")
        );
        dashboard.key(KeyCode::Char('f'));
        assert_eq!(dashboard.category_name(), "Appearance");
        assert_eq!(dashboard.settings_editor.selected.as_deref(), Some("theme"));
        dashboard.key(KeyCode::Char('f'));
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Document(_))
        ));
        let text = frame_text(&mut dashboard, 60, 24);
        assert!(text.contains("Document: /fixture/dashboard.toml"), "{text}");
        assert!(text.contains("colour"), "{text}");
        assert!(text.contains("unknown key"), "{text}");
        dashboard.key(KeyCode::Esc);

        let mut broken = report(Settings::default(), &[]);
        broken.unparseable = true;
        install(&mut dashboard, broken);
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("cannot be parsed"), "{text}");
        choose_row(&mut dashboard, "desktop_notifications");
        dashboard.key(KeyCode::Enter);
        assert!(dashboard.drain_outbox().is_empty(), "editing is disabled");
        assert!(
            dashboard
                .settings_editor
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("fixed by hand"))
        );

        install(&mut dashboard, report(Settings::default(), &[]));
        dashboard.settings_editor.notice = None;
        dashboard.key(KeyCode::Enter);
        assert_eq!(setting_request(&mut dashboard).1, "desktop_notifications");
    }

    #[test]
    fn responsive_frames_keep_selection_and_freeze_input_when_too_small() {
        let mut dashboard = fresh_dashboard();
        let wide = frame_text(&mut dashboard, 80, 24);
        assert!(wide.contains("Categories"), "{wide}");
        assert!(wide.contains("Desktop notifications"), "{wide}");
        assert!(wide.contains("Enter open"), "{wide}");

        dashboard.key(KeyCode::Enter);
        let list = frame_text(&mut dashboard, 80, 24);
        assert!(list.contains("Settings / Alerts"), "{list}");
        assert!(list.contains("› Desktop notifications"), "{list}");

        choose_category(&mut dashboard, "Appearance");
        let narrow = frame_text(&mut dashboard, 60, 24);
        assert!(narrow.contains("Settings / Categories"), "{narrow}");
        assert!(!narrow.contains("Desktop notifications"), "{narrow}");
        dashboard.key(KeyCode::Enter);
        let narrow = frame_text(&mut dashboard, 60, 24);
        assert!(narrow.contains("Settings / Appearance"), "{narrow}");
        assert!(narrow.contains("› Theme"), "{narrow}");

        let selected = dashboard.settings_editor.selected.clone();
        dashboard.install_area(Rect::new(0, 0, 40, 8));
        let tiny = frame_text(&mut dashboard, 40, 8);
        assert!(tiny.contains(TOO_SMALL), "{tiny}");
        dashboard.key(KeyCode::Enter);
        assert!(dashboard.drain_outbox().is_empty());
        assert_eq!(dashboard.settings_editor.selected, selected);
        dashboard.install_area(Rect::new(0, 0, 120, 40));
        let recovered = frame_text(&mut dashboard, 120, 40);
        assert!(recovered.contains("› Theme"), "{recovered}");

        choose_row(&mut dashboard, "branch_prefix");
        dashboard.key(KeyCode::Enter);
        dashboard.settings_editor_paste("XYZ");
        let _ = frame_text(&mut dashboard, 60, 24);
        dashboard.install_area(Rect::new(0, 0, 40, 8));
        let _ = frame_text(&mut dashboard, 40, 8);
        dashboard.key(KeyCode::Enter);
        assert!(dashboard.drain_outbox().is_empty());
        dashboard.install_area(Rect::new(0, 0, 120, 40));
        let recovered = frame_text(&mut dashboard, 120, 40);
        assert!(
            recovered.contains("XYZ"),
            "draft survives resize: {recovered}"
        );
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Text(_))
        ));
    }

    #[test]
    fn mouse_selects_categories_rows_and_theme_options() {
        let mut dashboard = fresh_dashboard();
        let _ = frame_text(&mut dashboard, 120, 40);
        let click = |row: u16, column: u16| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        dashboard.details_mouse(click(3, 2));
        assert_eq!(dashboard.category_name(), "Appearance");
        assert_eq!(dashboard.settings_editor.focus, Focus::Categories);
        dashboard.details_mouse(click(3, 2));
        assert_eq!(dashboard.settings_editor.focus, Focus::List);
        assert_eq!(dashboard.settings_editor.selected.as_deref(), Some("theme"));

        dashboard.details_mouse(click(1, 30));
        assert_eq!(dashboard.settings_editor.focus, Focus::List);
        dashboard.key(KeyCode::Enter);
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Theme(_))
        ));
        let _ = frame_text(&mut dashboard, 120, 40);
        let options_y = settings_layout(Rect::new(0, 0, 120, 40)).main.y + 2 + 2;
        dashboard.details_mouse(click(options_y + 1, 30));
        assert_eq!(dashboard.settings_editor.preview_theme, Some(ThemeId::Dark));
        dashboard.details_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 30,
            row: options_y + 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            dashboard.settings_editor.preview_theme,
            Some(ThemeId::TokyoNight)
        );

        dashboard.key(KeyCode::Esc);
        choose_row(&mut dashboard, "desktop_notifications");
        let _ = frame_text(&mut dashboard, 120, 40);
        dashboard.details_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 30,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            dashboard.settings_editor.selected.as_deref(),
            Some("ready_sound")
        );
    }

    #[test]
    fn published_report_waits_for_its_own_response_and_keeps_refusal() {
        let mut dashboard = fresh_dashboard();
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Enter);
        let (request_id, path, _) = setting_request(&mut dashboard);
        let mut saved = dashboard.settings.clone();
        saved.desktop_notifications = true;
        install(&mut dashboard, report(saved, &["desktop_notifications"]));
        assert_eq!(
            dashboard.settings_editor.pending.len(),
            1,
            "publication alone does not complete the request"
        );
        assert!(
            dashboard
                .settings_editor
                .pending
                .get(&request_id)
                .is_some_and(|pending| pending.published)
        );
        assert!(frame_text(&mut dashboard, 120, 40).contains("saving…"));
        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Error {
                code: ErrorCode::InvalidRequest,
                message: "late refusal".into(),
            },
        });
        assert!(dashboard.settings_editor.pending.is_empty());
        assert_eq!(
            dashboard
                .settings_editor
                .refused
                .get(&path)
                .map(String::as_str),
            Some("late refusal")
        );
    }

    #[test]
    fn acknowledged_noop_save_settles_but_default_override_waits_for_source() {
        let mut dashboard = fresh_dashboard();
        install(
            &mut dashboard,
            report(
                Settings {
                    theme: ThemeId::Nord,
                    ..Settings::default()
                },
                &[ThemeId::KEY],
            ),
        );
        choose_row(&mut dashboard, ThemeId::KEY);
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Enter);
        let (request_id, _, value) = setting_request(&mut dashboard);
        assert_eq!(value.as_deref(), Some("\"nord\""));
        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Ok,
        });
        assert!(
            dashboard.settings_editor.pending.is_empty(),
            "an already-explicit identical value needs no publication"
        );

        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, ThemeId::KEY);
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Enter);
        let (request_id, _, value) = setting_request(&mut dashboard);
        assert_eq!(value.as_deref(), Some("\"catppuccin-mocha\""));
        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Ok,
        });
        assert_eq!(
            dashboard.settings_editor.pending.len(),
            1,
            "an explicit default override waits for the document-source reading"
        );
        install(&mut dashboard, report(Settings::default(), &[ThemeId::KEY]));
        assert!(dashboard.settings_editor.pending.is_empty());
    }

    #[test]
    fn normalized_typed_literal_confirms_against_published_value() {
        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, "delegation.max_live_children");
        dashboard.key(KeyCode::Enter);
        let length = dashboard
            .settings_editor
            .overlays
            .last()
            .and_then(|overlay| match overlay {
                Overlay::Text(editor) => Some(editor.text.len()),
                _ => None,
            })
            .unwrap();
        for _ in 0..length {
            dashboard.key(KeyCode::Backspace);
        }
        dashboard.settings_editor_paste("1_0");
        dashboard.key(KeyCode::Enter);
        let (request_id, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "delegation.max_live_children");
        assert_eq!(value.as_deref(), Some("1_0"));
        dashboard.handle_server_message(ServerMessage::Response {
            request_id,
            response: Response::Ok,
        });
        assert_eq!(dashboard.settings_editor.pending.len(), 1);
        install(
            &mut dashboard,
            report(
                Settings {
                    delegation: ovrcr_protocol::DelegationSettings {
                        max_live_children: 10,
                        ..Default::default()
                    },
                    ..Settings::default()
                },
                &["delegation.max_live_children"],
            ),
        );
        assert!(
            dashboard.settings_editor.pending.is_empty(),
            "1_0 and 10 are the same published integer"
        );
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("Value: 10"), "{text}");
    }

    #[test]
    fn one_parent_change_confirms_only_one_queued_collection_edit() {
        let mut dashboard = fresh_dashboard();
        for (id, row) in [(10u64, "agents[0]"), (11u64, "agents[1]")] {
            dashboard.settings_editor.pending.insert(
                id,
                Pending {
                    row: row.into(),
                    report_key: "agents".into(),
                    value: Some(format!("[\"{row}\"]")),
                    before: Some("old".into()),
                    acknowledged: false,
                    published: false,
                },
            );
        }
        install(
            &mut dashboard,
            report(
                Settings {
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
                },
                &["agents"],
            ),
        );
        assert!(dashboard.settings_editor.pending[&10].published);
        assert!(
            !dashboard.settings_editor.pending[&11].published,
            "one changed parent array confirms one queued edit"
        );
        dashboard.handle_server_message(ServerMessage::Response {
            request_id: 11,
            response: Response::Error {
                code: ErrorCode::InvalidRequest,
                message: "second edit refused".into(),
            },
        });
        assert!(!dashboard.settings_editor.pending.contains_key(&11));
        assert_eq!(
            dashboard
                .settings_editor
                .refused
                .get("agents[1]")
                .map(String::as_str),
            Some("second edit refused")
        );
    }

    #[test]
    fn tiny_theme_picker_shows_resize_and_never_writes() {
        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, ThemeId::KEY);
        dashboard.key(KeyCode::Enter);
        dashboard.settings_editor_paste("tokyo");
        dashboard.install_area(Rect::new(0, 0, 60, 8));
        let text = frame_text(&mut dashboard, 60, 8);
        assert!(text.contains(TOO_SMALL), "{text}");
        dashboard.key(KeyCode::Enter);
        assert!(dashboard.drain_outbox().is_empty());
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Theme(_))
        ));
        dashboard.install_area(Rect::new(0, 0, 60, 24));
        let text = frame_text(&mut dashboard, 60, 24);
        assert!(text.contains("Find a theme: tokyo"), "{text}");
        assert_eq!(
            dashboard.settings_editor.preview_theme,
            Some(ThemeId::TokyoNight)
        );
    }

    #[test]
    fn empty_add_notice_is_visible_and_writes_nothing() {
        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, "agents");
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Char('a'));
        dashboard.key(KeyCode::Enter);
        assert!(dashboard.drain_outbox().is_empty());
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("! Enter a name before saving"), "{text}");
        assert!(matches!(
            dashboard.settings_editor.overlays.last(),
            Some(Overlay::Text(_))
        ));
    }

    #[test]
    fn moving_focus_does_not_move_neighboring_setting_rows() {
        let mut dashboard = fresh_dashboard();
        dashboard.key(KeyCode::Enter);
        let before = frame_lines(&mut dashboard, 120, 60);
        let y = |lines: &[String], label: &str| {
            lines
                .iter()
                .position(|line| line.contains(label))
                .unwrap_or_else(|| panic!("no {label} in {lines:#?}"))
        };
        let desktop = y(&before, "Desktop notifications");
        let ready = y(&before, "Ready sound");
        let value = y(&before, "Value:");
        dashboard.key(KeyCode::Down);
        let after = frame_lines(&mut dashboard, 120, 60);
        assert_eq!(y(&after, "Desktop notifications"), desktop);
        assert_eq!(y(&after, "Ready sound"), ready);
        assert_eq!(
            y(&after, "Value:"),
            value,
            "the fixed details area keeps its first value row"
        );
        assert!(after[ready].contains('›'), "{:?}", after[ready]);
        assert!(!after[desktop].contains('›'), "{:?}", after[desktop]);
    }

    #[test]
    fn narrow_consent_details_show_consequences_before_enter() {
        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, "save_uncommitted_work");
        let text = frame_text(&mut dashboard, 60, 24);
        assert!(text.contains("Enter turn on"), "{text}");
        assert!(text.contains("origin/wip"), "{text}");
        assert!(
            text.contains("workspace is removed") || text.contains("server shuts down"),
            "{text}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn inactive_sound_choice_names_the_missing_prerequisite() {
        let mut dashboard = fresh_dashboard();
        install(
            &mut dashboard,
            report(
                Settings {
                    ready_sound_choice: Some(ReadySoundChoice::Chime),
                    ..Settings::default()
                },
                &[ReadySoundChoice::KEY],
            ),
        );
        choose_row(&mut dashboard, ReadySoundChoice::KEY);
        let text = frame_text(&mut dashboard, 60, 24);
        assert!(text.contains("inactive"), "{text}");
        assert!(
            text.contains("Inactive: Desktop notifications are off"),
            "{text}"
        );
    }

    #[test]
    fn details_scroll_with_ctrl_keys_and_mouse_wheel() {
        let long = "x".repeat(240);
        let mut dashboard = fresh_dashboard();
        install(
            &mut dashboard,
            report(
                Settings {
                    branch_prefix: long,
                    ..Settings::default()
                },
                &["branch_prefix"],
            ),
        );
        choose_row(&mut dashboard, "branch_prefix");
        let text = frame_text(&mut dashboard, 60, 24);
        assert!(text.contains("Ctrl-d details"), "{text}");
        assert_eq!(dashboard.settings_editor.detail_scroll.get(), 0);
        dashboard.ctrl('d');
        assert!(dashboard.settings_editor.detail_scroll.get() > 0);
        let text = frame_text(&mut dashboard, 60, 24);
        assert!(text.contains("Ctrl-u details"), "{text}");
        let scrolled = dashboard.settings_editor.detail_scroll.get();
        dashboard.details_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 30,
            row: 22,
            modifiers: KeyModifiers::NONE,
        });
        assert!(dashboard.settings_editor.detail_scroll.get() < scrolled);
    }

    #[test]
    fn existing_picker_root_edits_prefill_the_complete_path() {
        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, "picker_roots");
        dashboard.key(KeyCode::Enter);
        if let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last_mut() {
            manager.selected = Some("picker_roots[1]".into());
        }
        dashboard.key(KeyCode::Enter);
        let Some(Overlay::Text(editor)) = dashboard.settings_editor.overlays.last() else {
            panic!("path editor");
        };
        assert_eq!(editor.text, "/b", "existing folder is fully prefilled");
        dashboard.key(KeyCode::Char('2'));
        dashboard.key(KeyCode::Enter);
        let (_, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "picker_roots");
        assert_eq!(value.as_deref(), Some("[\"/a\", \"/b2\", \"/c\"]"));
    }

    #[test]
    fn pending_reorder_follows_its_folder_across_manager_reopen() {
        let mut dashboard = fresh_dashboard();
        install(
            &mut dashboard,
            report(
                Settings {
                    picker_roots: vec![
                        PathBuf::from("/a"),
                        PathBuf::from("/b"),
                        PathBuf::from("/c"),
                    ],
                    ..Settings::default()
                },
                &["picker_roots"],
            ),
        );
        choose_row(&mut dashboard, "picker_roots");
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Down);
        dashboard.key(KeyCode::Char(']'));
        let (_, path, _) = setting_request(&mut dashboard);
        assert_eq!(path, "picker_roots");
        dashboard.key(KeyCode::Esc);
        assert!(dashboard.settings_editor.overlays.is_empty());

        // Reopen while the reorder is still in flight and select the moved
        // folder in the confirmed old order.
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Down);
        let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last() else {
            panic!("manager");
        };
        assert_eq!(manager.selected.as_deref(), Some("picker_roots[1]"));

        install(
            &mut dashboard,
            report(
                Settings {
                    picker_roots: vec![
                        PathBuf::from("/a"),
                        PathBuf::from("/c"),
                        PathBuf::from("/b"),
                    ],
                    ..Settings::default()
                },
                &["picker_roots"],
            ),
        );
        let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last() else {
            panic!("manager");
        };
        assert_eq!(
            manager.selected.as_deref(),
            Some("picker_roots[2]"),
            "the moved folder keeps focus after reopening"
        );
        assert!(dashboard.settings_editor.pending_reorder.is_none());
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("› /b"), "focus follows /b:\n{text}");
    }

    #[test]
    fn manager_focus_clamps_to_a_visible_row_when_the_collection_shrinks() {
        let mut dashboard = fresh_dashboard();
        install(
            &mut dashboard,
            report(
                Settings {
                    picker_roots: vec![
                        PathBuf::from("/a"),
                        PathBuf::from("/b"),
                        PathBuf::from("/c"),
                    ],
                    ..Settings::default()
                },
                &["picker_roots"],
            ),
        );
        choose_row(&mut dashboard, "picker_roots");
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Down);
        dashboard.key(KeyCode::Down);
        let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last() else {
            panic!("manager");
        };
        assert_eq!(manager.selected.as_deref(), Some("picker_roots[2]"));

        // A reset/removal/external report can leave only the Add row.
        install(
            &mut dashboard,
            report(
                Settings {
                    picker_roots: Vec::new(),
                    ..Settings::default()
                },
                &["picker_roots"],
            ),
        );
        let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last() else {
            panic!("manager");
        };
        assert_eq!(manager.selected.as_deref(), Some("picker_roots+"));
        let text = frame_text(&mut dashboard, 60, 24);
        assert!(
            text.contains("› + Add folder"),
            "keyboard focus must stay visible after shrink:\n{text}"
        );

        // A valid selection is always preserved across later readings.
        install(
            &mut dashboard,
            report(
                Settings {
                    picker_roots: vec![PathBuf::from("/a")],
                    ..Settings::default()
                },
                &["picker_roots"],
            ),
        );
        let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last() else {
            panic!("manager");
        };
        assert_eq!(manager.selected.as_deref(), Some("picker_roots+"));
        let text = frame_text(&mut dashboard, 60, 24);
        assert!(text.contains("› + Add folder"));
        assert!(text.contains("Enter add"), "{text}");
        assert!(
            !text.contains("x remove"),
            "Add has no removal action:\n{text}"
        );
        assert!(
            !text.contains("[ ] move"),
            "Add has no reorder action:\n{text}"
        );
        assert_eq!(
            text.matches("Adds one folder").count(),
            1,
            "the Add description is shown once:\n{text}"
        );
        assert!(text.contains("Enter adds a folder"), "{text}");
    }

    #[test]
    fn manager_follows_published_source_for_reset_actions() {
        let mut dashboard = fresh_dashboard();
        choose_row(&mut dashboard, "picker_roots");
        dashboard.key(KeyCode::Enter);
        dashboard.key(KeyCode::Char('a'));
        dashboard.settings_editor_paste("/d");
        dashboard.key(KeyCode::Enter);
        let (_, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "picker_roots");
        assert!(value.is_some());
        install(
            &mut dashboard,
            report(
                Settings {
                    picker_roots: vec![
                        PathBuf::from("/a"),
                        PathBuf::from("/b"),
                        PathBuf::from("/c"),
                        PathBuf::from("/d"),
                    ],
                    ..Settings::default()
                },
                &["picker_roots"],
            ),
        );
        let Some(Overlay::Manager(manager)) = dashboard.settings_editor.overlays.last() else {
            panic!("manager stays open after save");
        };
        let parent = dashboard.manager_parent(manager);
        assert_eq!(parent.source, Some(SettingSource::Document));
        assert!(parent.reset.is_some());
        let text = frame_text(&mut dashboard, 120, 40);
        assert!(text.contains("Reset: r clears"), "{text}");
        assert!(text.contains("r reset"), "{text}");
        dashboard.key(KeyCode::Char('r'));
        let (_, path, value) = setting_request(&mut dashboard);
        assert_eq!(path, "picker_roots");
        assert_eq!(value, None);
    }

    #[test]
    fn resize_event_updates_geometry_before_the_next_draw() {
        let mut dashboard = fresh_dashboard();
        let _ = frame_text(&mut dashboard, 120, 40);
        assert!(matches!(
            dashboard.event_action(crossterm::event::Event::Resize(40, 8)),
            DashboardAction::Redraw
        ));
        assert_eq!(dashboard.settings_area().width, 40);
        assert_eq!(dashboard.settings_area().height, 8);
        dashboard.key(KeyCode::Enter);
        assert!(dashboard.drain_outbox().is_empty());
        dashboard.event_action(crossterm::event::Event::Resize(120, 40));
        assert_eq!(dashboard.settings_area().width, 120);
    }
}

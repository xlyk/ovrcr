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
use ovrcr_protocol::{ClientMessage, Request, Response, SettingSource, SettingsReport};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use std::collections::HashMap;

pub(super) const TITLE: &str = "Settings · Enter edit · r reset · x remove · / filter · Esc close";
const UNPARSEABLE: &str = "Editing is off until dashboard.toml is fixed by hand.";

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
}

struct Edit {
    row: String,
    text: String,
    picker: Option<PathPicker>,
    pick: Option<PickList>,
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
    let quota = &settings.quota;
    let mut items = Vec::new();
    items.extend(vec![
        top(
            "Alerts",
            "desktop_notifications",
            "Desktop notifications",
            Kind::Toggle(settings.desktop_notifications),
            &["Turning this on may show an OS notification permission prompt."],
        ),
        top(
            "Alerts",
            "ready_sound",
            "Ready sound",
            Kind::Toggle(settings.ready_sound),
            &[],
        ),
    ]);
    let mut workspaces = vec![
        top(
            "Workspaces",
            "automatic_local_terminals",
            "Automatic local terminals",
            Kind::Pick(
                ["on", "off", "default_branch_only"]
                    .into_iter()
                    .map(|value| PickItem {
                        label: value.into(),
                        value: toml_string(value),
                    })
                    .collect(),
            ),
            &[],
        ),
        top(
            "Workspaces",
            "branch_prefix",
            "Branch prefix",
            Kind::Text,
            &[],
        ),
        top(
            "Workspaces",
            "picker_roots",
            "Picker roots",
            Kind::Fixed,
            &[],
        ),
    ];
    let roots: Vec<String> = settings
        .picker_roots
        .iter()
        .map(|root| root.display().to_string())
        .collect();
    let roots_set = workspaces[2].source == Some(SettingSource::Document);
    workspaces[2].value = count(roots.len());
    for (index, root) in roots.iter().enumerate() {
        let mut row = child(
            "Workspaces",
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
        workspaces.push(row);
    }
    let mut add = child(
        "Workspaces",
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
    workspaces.push(add);
    items.extend(workspaces);
    items.extend(vec![top(
        "Titles",
        "title_model",
        "Title model",
        Kind::Text,
        &["Setting a model makes paid calls to title sessions."],
    )]);
    items.extend(vec![
            top(
                "Usage",
                "quota.enabled",
                "Codex and Grok usage",
                Kind::Toggle(quota.enabled),
                &["On by default while a Dashboard is attached. The readers do not rewrite auth files. Set false to turn Codex and Grok collection off."],
            ),
            top(
                "Usage",
                "quota.claude.probe",
                "Claude allowance probe",
                Kind::Toggle(quota.claude_probe),
                &[ovrcr_protocol::CLAUDE_PROBE_DESCRIPTION],
            ),
            top("Usage", "quota.codex.command", "Codex command", Kind::Path, &[]),
            top("Usage", "quota.codex.home", "Codex home", Kind::Path, &[]),
            top("Usage", "quota.grok.command", "Grok command", Kind::Path, &[]),
            top("Usage", "quota.grok.home", "Grok home", Kind::Path, &[]),
        ],
    );
    let mut agents = vec![top("Agents", "agents", "Agent overrides", Kind::Fixed, &[])];
    agents[0].value = count(settings.agents.len());
    for (index, agent) in settings.agents.iter().enumerate() {
        let mut row = child(
            "Agents",
            format!("agents[{index}]"),
            agent.name.clone(),
            toml_array(agent.argv.iter().map(String::as_str)),
            Kind::Toml,
            format!("agents[{index}].argv"),
        );
        row.reset = Some((format!("agents[{index}]"), None));
        agents.push(row);
    }
    agents.push(child(
        "Agents",
        "agents+".into(),
        "+ Add agent (name)".into(),
        String::new(),
        Kind::Text,
        format!("agents[{}]", settings.agents.len()),
    ));
    items.extend(agents);
    let mut launches = vec![top(
        "Remembered launches",
        "launch_choices",
        "Remembered launches",
        Kind::Fixed,
        &[],
    )];
    launches[0].value = count(settings.launch_choices.len());
    for (project, choice) in &settings.launch_choices {
        let path = format!("launch_choices.{}", toml_edit::Key::new(project.as_str()));
        let mut choices = vec![LaunchChoice::Terminal];
        choices.extend(presets.iter().cloned().map(LaunchChoice::Agent));
        if !choices.contains(choice) {
            choices.push(choice.clone());
        }
        let mut row = child(
            "Remembered launches",
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
        launches.push(row);
    }
    launches.push(child(
        "Remembered launches",
        "launch_choices+".into(),
        "+ Add project".into(),
        String::new(),
        Kind::Text,
        String::new(),
    ));
    items.extend(launches);
    items
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
                    key == *row
                        || (["picker_roots", "agents", "launch_choices"].contains(row)
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
        let editor = &mut self.settings_editor;
        editor.refused.remove(row);
        editor.edit = None;
        let request_id = self.next_request_id();
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
        if super::input::is_browse_key(key) {
            self.details = None;
            return DashboardAction::Redraw;
        }
        match key.code {
            KeyCode::Esc if !self.settings_editor.filter.is_empty() => {
                self.settings_editor.filter.clear();
            }
            KeyCode::Esc => self.details = None,
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
            KeyCode::Char('r') => return self.reset_or_remove(false),
            KeyCode::Char('x') | KeyCode::Delete => return self.reset_or_remove(true),
            _ => {}
        }
        DashboardAction::Redraw
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
                let mut list = PickList::new(options);
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
        DashboardAction::Redraw
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
            .style(Style::default().bg(BASE).fg(TEXT));
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
            style(SUBTEXT),
        )];
        if editor.filtering || !editor.filter.is_empty() {
            let (text, _) = if editor.filtering {
                editor
                    .cursor
                    .display(&editor.filter, width.saturating_sub(8))
            } else {
                (editor.filter.clone(), 0)
            };
            lines.push(Line::styled(format!("Filter: {text}"), style(MAUVE)));
        }
        if let Some(notice) = &editor.notice {
            lines.push(Line::styled(notice.clone(), style(PEACH)));
        }
        let loose = loose_findings(report);
        if !loose.is_empty() || report.unparseable {
            lines.push(Line::styled(
                format!("Findings: {}", loose.len()),
                style(YELLOW).add_modifier(Modifier::BOLD),
            ));
            for finding in loose {
                lines.push(Line::styled(format!("  ! {finding}"), style(YELLOW)));
            }
            if report.unparseable {
                lines.push(Line::styled(format!("  ! {UNPARSEABLE}"), style(YELLOW)));
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
                    style(MAUVE).add_modifier(Modifier::BOLD),
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
                    style(TEXT),
                ));
            }
            match editing {
                Some(edit) if edit.pick.is_none() => {
                    let room = width
                        .saturating_sub(spans.iter().map(|span| span.width()).sum::<usize>() + 2);
                    let (text, _) = editor.cursor.display(&edit.text, room.max(4));
                    spans.push(Span::styled(format!("[{text}]"), style(MAUVE)));
                }
                _ => {
                    spans.push(Span::styled(row.value.clone(), style(TEXT)));
                    if let Some(source) = row.source {
                        let set = source == SettingSource::Document;
                        spans.push(Span::styled(
                            if set { "  set" } else { "  default" },
                            style(if set { PEACH } else { MUTED }),
                        ));
                        if set && row.kind != Kind::Fixed {
                            spans.push(Span::styled(
                                format!(
                                    "  (default: {})",
                                    row.default.as_deref().unwrap_or("unset")
                                ),
                                style(MUTED),
                            ));
                        }
                    }
                }
            }
            if editor.pending.values().any(|id| id == &row.id) {
                spans.push(Span::styled("  saving…", style(MUTED)));
            }
            let mut line = Line::from(spans);
            if chosen {
                line = line.style(Style::default().bg(SURFACE0));
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
                        lines.push(detail(format!("filter: {}", list.query), MAUVE));
                    }
                    let (options, _) = list.lines(6);
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
                        lines.push(detail(format!("{mark} {}", entry.label), SUBTEXT));
                    }
                }
                lines.push(detail(
                    match (&row.kind, edit.pick.is_some()) {
                        (_, true) => "↑↓ choose · Enter save · Esc cancel".into(),
                        (Kind::Toml, _) => {
                            "TOML array, e.g. [\"claude\", \"--verbose\"] · Enter save · Esc cancel"
                                .into()
                        }
                        (Kind::Path, _) => "Tab complete · Enter save · Esc cancel".into(),
                        _ => "Enter save · Esc cancel".into(),
                    },
                    MUTED,
                ));
            }
            if let Some(refused) = editor.refused.get(&row.id) {
                lines.extend(wrapped(format!("refused: {refused}"), RED));
            }
            if let Some(finding) = &row.finding {
                lines.extend(wrapped(format!("! {finding}"), YELLOW));
            }
            for about in &row.about {
                lines.extend(wrapped(about.clone(), SUBTEXT));
            }
            if chosen && !row.child && !row.path.is_empty() {
                lines.push(detail(format!("key: {}", row.path), MUTED));
            }
            if chosen {
                selected = Some((start, lines.len() - start - 1));
            }
        }
        if header.is_none() {
            lines.push(Line::raw(""));
            lines.push(Line::styled("No setting matches the filter.", style(MUTED)));
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
}

use super::agents::{AgentSource, apply_overrides, detect_agents};
use super::hints::{HintAction, KeyHint, key_hints};
use super::input::is_browse_key;
use super::picker::{PathPicker, PickItem, PickList, complete_path, expand_path};
use super::render::{CRUST, MAUVE, MUTED, PEACH, SUBTEXT, TEXT, clip_text};
use super::state::find_session;
use super::{Dashboard, DashboardAction, InputMode};
use crate::protocol::{BranchRequest, ClientMessage, CreateSessionRequest, Request, Response};
use crate::session::SessionId;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Suggestions {
    inspect: Option<u64>,
    repos: HashMap<String, PathBuf>,
    cache: HashMap<String, Result<super::git_hints::Hints, String>>,
    worker: Option<super::git_hints::Worker>,
    in_flight: Option<String>,
    project: Option<String>,
    note: Option<String>,
}

#[derive(Clone)]
enum Command {
    CreateTerminal,
    CreateWorkspace,
    RegisterProject,
    CloseTerminal(SessionId),
    RemoveWorkspace,
    RemoveProject,
    Switch(SessionId),
    Hint(HintAction),
}

struct Entry {
    label: String,
    command: Command,
}

enum FieldKind {
    Text,
    Toggle,
    Pick(PickList),
    Path(PathPicker),
}

struct Field {
    label: &'static str,
    value: String,
    required: bool,
    hidden: bool,
    edited: bool,
    kind: FieldKind,
}

enum Page {
    Search {
        query: String,
        selected: usize,
    },
    Form {
        command: Command,
        fields: Vec<Field>,
        active: usize,
        name_edited: bool,
        root_edited: bool,
    },
    Confirm {
        request: Request,
        target: String,
    },
}

pub(super) struct Palette {
    page: Page,
    pending: Option<u64>,
    error: Option<String>,
    workspace_acknowledged: bool,
    suggestions: Suggestions,
    // Some(true) retains explicit whole-form Submit; Some(false) replays Enter.
    submit_when_ready: Option<bool>,
    cursor: super::text_cursor::TextCursor,
    scroll: Option<usize>,
}

#[derive(Clone, Copy)]
enum PaletteTarget {
    Search,
    Entry(usize),
    Field(usize, bool),
    Option(usize, usize),
}

fn palette_geometry(outer: Rect) -> (Rect, Rect, Rect, Rect) {
    let width = outer.width.min(82);
    let height = outer.height.min(19);
    let area = Rect::new(
        outer.x + (outer.width - width) / 2,
        outer.y + (outer.height - height) / 2,
        width,
        height,
    );
    let inner = Block::bordered().inner(area);
    let button_height = inner.height.min(1);
    let footer = inner
        .height
        .saturating_sub(button_height)
        .min(4)
        .min(inner.height / 3);
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(footer + button_height),
    );
    let buttons = Rect::new(
        inner.x,
        inner.bottom().saturating_sub(button_height),
        inner.width,
        button_height,
    );
    (area, inner, body, buttons)
}
fn button_text(page: &Page, width: u16) -> &'static str {
    if width < 17 {
        return "[OK] [X]";
    }
    match page {
        Page::Search { .. } => "[Open] [Cancel]",
        Page::Confirm { .. } => "[Confirm] [Cancel]",
        Page::Form { .. } => "[Submit] [Cancel]",
    }
}

impl Palette {
    fn new() -> Self {
        Self {
            page: Page::Search {
                query: String::new(),
                selected: 0,
            },
            pending: None,
            error: None,
            workspace_acknowledged: false,
            suggestions: Suggestions::default(),
            submit_when_ready: None,
            cursor: Default::default(),
            scroll: None,
        }
    }

    fn insert(&mut self, text: &str) {
        if self.pending.is_some() || self.submit_when_ready.is_some() {
            return;
        }
        self.scroll = None;
        let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
        if text.is_empty() {
            return;
        }
        match &mut self.page {
            Page::Search { query, selected } => {
                *selected = 0;
                self.cursor.insert(query, &text);
            }
            Page::Form {
                fields,
                active,
                name_edited,
                root_edited,
                ..
            } => {
                mark_form_edit(fields, *active, name_edited, root_edited);
                match &mut fields[*active].kind {
                    FieldKind::Pick(list) => {
                        self.cursor.insert(&mut list.query, &text);
                        list.selected = 0;
                    }
                    FieldKind::Text => self.cursor.insert(&mut fields[*active].value, &text),
                    FieldKind::Path(picker) => {
                        picker.selected = 0;
                        self.cursor.insert(&mut fields[*active].value, &text);
                    }
                    FieldKind::Toggle => {}
                }
            }
            Page::Confirm { .. } => return,
        }
        self.error = None;
    }
}

fn visible_indices(fields: &[Field]) -> Vec<usize> {
    fields
        .iter()
        .enumerate()
        .filter(|(_, field)| !field.hidden)
        .map(|(index, _)| index)
        .collect()
}

/// `draw_palette` only holds `&self` and cannot recompute a Path field's
/// directory listing, so every key handler that edits a Path field's value
/// or moves its selection must refresh the picker's cache here.
fn refresh_path_listing(field: &mut Field, roots: &[PathBuf]) {
    let value = field.value.clone();
    if let FieldKind::Path(picker) = &mut field.kind {
        picker.listing(&value, roots);
    }
}

fn move_form_field(fields: &[Field], active: &mut usize, backwards: bool) {
    let visible = visible_indices(fields);
    let Some(position) = visible.iter().position(|index| *index == *active) else {
        return;
    };
    let next = if backwards {
        position.saturating_sub(1)
    } else {
        (position + 1).min(visible.len().saturating_sub(1))
    };
    if let Some(index) = visible.get(next) {
        *active = *index;
    }
}

/// A deferred submit replays Enter on a pick list the user never saw, so only
/// the branch they actually typed may stand; a fuzzy match must be confirmed.
fn unconfirmed_pick(page: &Page) -> Option<&'static str> {
    let Page::Form { fields, active, .. } = page else {
        return None;
    };
    let field = fields.get(*active)?;
    let FieldKind::Pick(list) = &field.kind else {
        return None;
    };
    if field.value.is_empty() {
        return None;
    }
    // Git refs are case-sensitive, so only the exact text the user typed stands.
    let exact = list
        .accepted()
        .is_some_and(|item| item.value == field.value);
    (!exact).then_some(field.label)
}

fn accept_pick(field: &mut Field) -> bool {
    let FieldKind::Pick(list) = &field.kind else {
        return true;
    };
    let Some(item) = list.accepted() else {
        return false;
    };
    field.value = item.value.clone();
    true
}

impl Dashboard {
    pub(super) fn open_palette(&mut self) -> DashboardAction {
        self.cancel_mouse_gesture();
        self.whichkey = None;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.mode = InputMode::Browse;
        if self.palette.is_some() {
            return DashboardAction::Redraw;
        }
        let request_id = self.next_request_id();
        let mut palette = Palette::new();
        palette.suggestions.inspect = Some(request_id);
        self.palette = Some(palette);
        DashboardAction::Request(ClientMessage {
            request_id,
            request: Request::Inspect,
        })
    }

    pub(super) fn open_create_terminal(&mut self) -> DashboardAction {
        self.cancel_mouse_gesture();
        self.whichkey = None;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.palette = Some(Palette {
            page: self.palette_form(Command::CreateTerminal),
            pending: None,
            error: None,
            workspace_acknowledged: false,
            suggestions: Suggestions::default(),
            submit_when_ready: None,
            cursor: Default::default(),
            scroll: None,
        });
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn open_create_workspace(&mut self) -> DashboardAction {
        let action = self.open_palette();
        let page = self.palette_form(Command::CreateWorkspace);
        self.palette.as_mut().unwrap().page = page;
        action
    }

    fn refresh_workspace_form(&self, palette: &mut Palette) {
        let hints = &mut palette.suggestions;
        if let Some((project, result)) = hints.worker.as_ref().and_then(|worker| worker.poll()) {
            hints.cache.insert(project, result);
            hints.in_flight = None;
        }
        let Page::Form {
            command: Command::CreateWorkspace,
            fields,
            ..
        } = &mut palette.page
        else {
            return;
        };
        let project = fields[0].value.clone();
        let existing = fields[2].value == "existing";
        fields[4].hidden = existing;
        fields[4].required = !existing;
        if hints.project.as_ref() != Some(&project) {
            hints.project = Some(project.clone());
            fields[3].kind = FieldKind::Text;
            if existing {
                fields[3].value.clear();
            }
        }
        if !existing && !fields[3].edited {
            fields[3].value = format!("{}{}", self.settings.branch_prefix, fields[1].value);
        }
        if hints.inspect.is_none()
            && !hints.cache.contains_key(&project)
            && hints.in_flight.is_none()
        {
            if let Some(repo) = hints.repos.get(&project) {
                if hints.worker.is_none() {
                    hints.worker = super::git_hints::Worker::start().ok();
                }
                if hints
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.request(project.clone(), repo.clone()))
                {
                    hints.in_flight = Some(project.clone());
                } else {
                    hints
                        .cache
                        .insert(project.clone(), Err("Could not start Git lookup".into()));
                }
            } else {
                hints
                    .cache
                    .insert(project.clone(), Err("Repository path unavailable".into()));
            }
        }
        hints.note = None;
        match hints.cache.get(&project) {
            Some(Ok(result)) => {
                if !fields[4].edited {
                    fields[4].value.clone_from(&result.base);
                }
                if existing && !matches!(fields[3].kind, FieldKind::Pick(_)) {
                    let mut list = PickList::new(
                        result
                            .branches
                            .iter()
                            .map(|branch| PickItem {
                                label: branch.clone(),
                                value: branch.clone(),
                            })
                            .collect(),
                    );
                    if fields[3].value.is_empty() {
                        fields[3].value = result.branches.first().cloned().unwrap_or_default();
                    }
                    if result.branches.contains(&fields[3].value) {
                        list.select_value(&fields[3].value);
                    } else {
                        // Filter to what the user typed instead of silently
                        // replacing it; an unmatched query accepts nothing.
                        list.query.clone_from(&fields[3].value);
                    }
                    if !result.branches.is_empty() {
                        fields[3].kind = FieldKind::Pick(list);
                    }
                }
            }
            Some(Err(error)) => {
                hints.note = Some(format!("{error}; enter branch/base manually"));
                if !fields[4].edited {
                    fields[4].value = "main".into();
                }
            }
            None => {
                hints.note = Some("Loading Git suggestions…".into());
                if !fields[4].edited {
                    fields[4].value = "main".into();
                }
            }
        }
    }

    // Called by the real client loop, even when there is no keyboard/server input.
    pub fn poll_palette(&mut self) -> bool {
        let Some(mut palette) = self.palette.take() else {
            return false;
        };
        let was_loading = palette.suggestions.in_flight.is_some();
        self.refresh_workspace_form(&mut palette);
        let ready = match &palette.page {
            Page::Form {
                command: Command::CreateWorkspace,
                fields,
                ..
            } => palette.suggestions.cache.contains_key(&fields[0].value),
            _ => false,
        };
        let mut submit = palette.submit_when_ready.filter(|_| ready);
        let mut refused = false;
        if submit.is_some() {
            palette.submit_when_ready = None;
            // The hints arrived after the user pressed Enter, so they never saw
            // this list. Make them confirm anything but what they typed.
            if let Some(label) = unconfirmed_pick(&palette.page) {
                palette.error = Some(format!("{label} not found in repository"));
                submit = None;
                refused = true;
            }
        }
        self.palette = Some(palette);
        if let Some(submit_all) = submit
            && let DashboardAction::Request(request) = self.palette_key_with_submit(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                submit_all,
            )
        {
            self.push_request(request);
        }
        was_loading || submit.is_some() || refused
    }

    pub(super) fn open_register_project(&mut self) -> DashboardAction {
        self.cancel_mouse_gesture();
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        let mut page = self.palette_form(Command::RegisterProject);
        if let Page::Form { fields, .. } = &mut page {
            for field in fields.iter_mut() {
                refresh_path_listing(field, &self.settings.picker_roots);
            }
        }
        self.palette = Some(Palette {
            page,
            ..Palette::new()
        });
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn open_close_terminal(&mut self) -> DashboardAction {
        let Some(id) = self
            .action_session()
            .filter(|id| find_session(self, *id).is_some())
        else {
            return DashboardAction::None;
        };
        self.cancel_mouse_gesture();
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.whichkey = None;
        self.palette = Some(Palette {
            page: self.command_page(Command::CloseTerminal(id)),
            ..Palette::new()
        });
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn open_remove_context(&mut self, workspace: bool) -> DashboardAction {
        let (project, name) = self.creation_context();
        if project.is_empty() || (workspace && name.is_empty()) {
            return DashboardAction::None;
        }
        self.cancel_mouse_gesture();
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.whichkey = None;
        self.palette = Some(Palette {
            page: self.palette_form(if workspace {
                Command::RemoveWorkspace
            } else {
                Command::RemoveProject
            }),
            ..Palette::new()
        });
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    fn command_page(&self, command: Command) -> Page {
        if let Command::CloseTerminal(id) = command {
            let session = find_session(self, id).expect("close target exists");
            Page::Confirm {
                request: Request::CloseTerminal { session: id },
                target: format!(
                    "Close {} / {} / {} (#{}). Stop its processes and remove its record.",
                    session.project, session.workspace, session.name, id.0
                ),
            }
        } else {
            self.palette_form(command)
        }
    }

    fn command_hint(&self, command: &Command) -> Option<KeyHint> {
        let action = match command {
            Command::CreateTerminal => HintAction::Key(KeyCode::Char('n')),
            Command::CreateWorkspace => HintAction::Key(KeyCode::Char('w')),
            Command::RegisterProject => HintAction::Key(KeyCode::Char('a')),
            Command::CloseTerminal(_) => HintAction::Key(KeyCode::Char('X')),
            Command::Hint(action) => *action,
            _ => return None,
        };
        key_hints(self)
            .into_iter()
            .flat_map(|g| g.hints)
            .find(|h| h.action == action)
    }

    pub(super) fn palette_paste(&mut self, text: &str) -> DashboardAction {
        let mut palette = self.palette.take().unwrap();
        palette.insert(text);
        self.refresh_workspace_form(&mut palette);
        self.palette = Some(palette);
        self.refresh_active_path_listing();
        DashboardAction::Redraw
    }

    fn palette_entries(&self, query: &str) -> Vec<Entry> {
        let mut entries = vec![
            Entry {
                label: "Create terminal (n)".into(),
                command: Command::CreateTerminal,
            },
            Entry {
                label: "Create workspace (w)".into(),
                command: Command::CreateWorkspace,
            },
            Entry {
                label: "Register project (a)".into(),
                command: Command::RegisterProject,
            },
        ];
        if let Some(id) = self
            .action_session()
            .filter(|id| find_session(self, *id).is_some())
        {
            entries.push(Entry {
                label: "Close terminal".into(),
                command: Command::CloseTerminal(id),
            });
        }
        entries.push(Entry {
            label: "Remove workspace".into(),
            command: Command::RemoveWorkspace,
        });
        entries.push(Entry {
            label: "Remove project".into(),
            command: Command::RemoveProject,
        });
        for project in &self.hierarchy.projects {
            for workspace in &project.workspaces {
                for session in &workspace.sessions {
                    entries.push(Entry {
                        label: format!(
                            "Switch terminal: {} / {} / {} (#{})",
                            project.name, workspace.name, session.name, session.id.0
                        ),
                        command: Command::Switch(session.id),
                    });
                }
            }
        }
        for hint in key_hints(self).into_iter().flat_map(|g| g.hints) {
            if matches!(hint.key, "n" | "w" | "a" | "X" | ":" | "Space") {
                continue;
            }
            entries.push(Entry {
                label: hint.name.into(),
                command: Command::Hint(hint.action),
            });
        }
        let query = query.to_lowercase();
        entries.retain(|entry| {
            query
                .split_whitespace()
                .all(|word| entry.label.to_lowercase().contains(word))
        });
        entries
    }

    /// Single authority for keeping a Path field's cached listing in sync:
    /// every `palette_key`/`palette_paste` return path funnels through here
    /// afterward, so no call site needs its own `refresh_path_listing` call.
    fn refresh_active_path_listing(&mut self) {
        let Some(palette) = &mut self.palette else {
            return;
        };
        let Page::Form { fields, active, .. } = &mut palette.page else {
            return;
        };
        refresh_path_listing(&mut fields[*active], &self.settings.picker_roots);
    }

    pub(super) fn palette_mouse(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        let (_, _, body, buttons) = palette_geometry(area);
        let point = Position::new(mouse.column, mouse.row);
        let palette = self.palette.as_ref().unwrap();
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && buttons.contains(point) {
            let label = button_text(&palette.page, buttons.width);
            let cancel = label.rfind('[').unwrap() as u16;
            let x = mouse.column - buttons.x;
            if x >= cancel && x < label.len() as u16 {
                return self.palette_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            }
            if x < cancel.saturating_sub(1)
                && palette.pending.is_none()
                && palette.submit_when_ready.is_none()
            {
                return self.palette_key_with_submit(
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                    true,
                );
            }
            return DashboardAction::Redraw;
        }
        if palette.pending.is_some() || palette.submit_when_ready.is_some() || !body.contains(point)
        {
            return DashboardAction::None;
        }
        let (lines, targets, scroll) = self.palette_body(body);
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let delta = if mouse.kind == MouseEventKind::ScrollUp {
                -1
            } else {
                1
            };
            let over_option = mouse.column >= body.x.saturating_add(4)
                && targets.iter().any(|(row, target)| {
                    *row == scroll + usize::from(mouse.row - body.y)
                        && matches!(target, PaletteTarget::Option(..))
                });
            if matches!(palette.page, Page::Search { .. }) || over_option {
                return self.palette_key(KeyEvent::new(
                    if delta < 0 {
                        KeyCode::Up
                    } else {
                        KeyCode::Down
                    },
                    KeyModifiers::NONE,
                ));
            }
            let max = lines.len().saturating_sub(body.height as usize);
            let next = scroll.saturating_add_signed(delta).min(max);
            self.palette.as_mut().unwrap().scroll = Some(next);
            return DashboardAction::Redraw;
        }
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return DashboardAction::None;
        }
        let target = targets
            .into_iter()
            .find(|(row, _)| *row == scroll + usize::from(mouse.row - body.y))
            .map(|(_, t)| t);
        let Some(target) = target else {
            return DashboardAction::None;
        };
        let mut palette = self.palette.take().unwrap();
        let mut key = None;
        match (&mut palette.page, target) {
            (Page::Search { query, .. }, PaletteTarget::Search) => palette.cursor.click(
                query,
                body.width.saturating_sub(8) as usize,
                mouse.column.saturating_sub(body.x + 8) as usize,
            ),
            (Page::Search { selected, .. }, PaletteTarget::Entry(index)) => {
                *selected = index;
                key = Some(KeyCode::Enter);
            }
            (Page::Form { fields, active, .. }, PaletteTarget::Field(index, value)) => {
                let was_active = *active == index;
                if !was_active {
                    palette.cursor = Default::default();
                }
                *active = index;
                palette.scroll = None;
                let field = &mut fields[index];
                if value
                    && let FieldKind::Pick(list) = &mut field.kind
                    && (!was_active || list.query.is_empty())
                {
                    list.query.clone_from(&field.value);
                    list.selected = 0;
                }
                match &field.kind {
                    FieldKind::Toggle => key = Some(KeyCode::Char(' ')),
                    _ if value => {
                        let text = match &field.kind {
                            FieldKind::Pick(list) if !list.query.is_empty() => &list.query,
                            _ => &field.value,
                        };
                        palette.cursor.click(
                            text,
                            body.width.saturating_sub(2) as usize,
                            mouse.column.saturating_sub(body.x + 2) as usize,
                        );
                    }
                    _ => {}
                }
            }
            (Page::Form { fields, active, .. }, PaletteTarget::Option(index, selected)) => {
                *active = index;
                palette.cursor = Default::default();
                match &mut fields[index].kind {
                    FieldKind::Pick(list) => {
                        list.selected = selected;
                        key = Some(KeyCode::Tab);
                    }
                    FieldKind::Path(picker) => {
                        picker.selected = selected;
                        key = Some(KeyCode::Tab);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        self.palette = Some(palette);
        if let Some(code) = key {
            self.palette_key(KeyEvent::new(code, KeyModifiers::NONE))
        } else {
            self.refresh_active_path_listing();
            DashboardAction::Redraw
        }
    }

    pub(super) fn palette_key(&mut self, key: KeyEvent) -> DashboardAction {
        self.palette_key_with_submit(key, false)
    }

    fn palette_key_with_submit(&mut self, key: KeyEvent, submit: bool) -> DashboardAction {
        let action = self.palette_key_inner(key, submit);
        self.refresh_active_path_listing();
        action
    }

    fn palette_key_inner(&mut self, key: KeyEvent, submit: bool) -> DashboardAction {
        let mut palette = self.palette.take().unwrap();
        if key.code == KeyCode::Esc || is_browse_key(key) {
            // Escape closes even while a request runs; its late response is
            // dropped so it cannot surface after the user has moved on.
            if !palette.workspace_acknowledged
                && let Some(request_id) = palette.pending
            {
                self.ignored_responses.insert(request_id);
            }
            if let Some(request_id) = palette.suggestions.inspect {
                self.ignored_responses.insert(request_id);
            }
            return DashboardAction::Redraw;
        }
        // Other keys wait for the running request so a submit cannot repeat.
        if palette.pending.is_some() || palette.submit_when_ready.is_some() {
            self.palette = Some(palette);
            return DashboardAction::Redraw;
        }
        self.refresh_workspace_form(&mut palette);
        palette.scroll = None;
        if matches!(
            key.code,
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter | KeyCode::Up | KeyCode::Down
        ) {
            palette.cursor = Default::default();
            palette.scroll = None;
        }
        let mut action = DashboardAction::Redraw;
        match key.code {
            KeyCode::Char('u' | 'U') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match &mut palette.page {
                    Page::Search { query, selected } => {
                        query.clear();
                        palette.cursor = Default::default();
                        *selected = 0;
                    }
                    Page::Form {
                        fields,
                        active,
                        name_edited,
                        root_edited,
                        ..
                    } => {
                        if matches!(fields[*active].kind, FieldKind::Toggle) {
                            self.palette = Some(palette);
                            return action;
                        }
                        mark_form_edit(fields, *active, name_edited, root_edited);
                        fields[*active].value.clear();
                        palette.cursor = Default::default();
                        match &mut fields[*active].kind {
                            FieldKind::Pick(list) => {
                                list.query.clear();
                                list.selected = 0;
                            }
                            FieldKind::Path(picker) => picker.selected = 0,
                            FieldKind::Text | FieldKind::Toggle => {}
                        }
                    }
                    _ => {}
                }
                palette.error = None;
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if matches!(&palette.page, Page::Form { fields, active, .. } if matches!(fields[*active].kind, FieldKind::Toggle)) => {
                if let Page::Form { fields, .. } = &mut palette.page {
                    fields[2].value = if fields[2].value == "new" {
                        "existing"
                    } else {
                        "new"
                    }
                    .into();
                    fields[3].kind = FieldKind::Text;
                    fields[3].value.clear();
                    fields[3].edited = false;
                }
            }
            KeyCode::Char(ch)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                palette.insert(&ch.to_string());
            }
            KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End => {
                let editing = matches!(key.code, KeyCode::Backspace | KeyCode::Delete);
                match &mut palette.page {
                    Page::Search { query, selected } => {
                        palette.cursor.key(query, key.code);
                        if editing {
                            *selected = 0;
                        }
                    }
                    Page::Form {
                        fields,
                        active,
                        name_edited,
                        root_edited,
                        ..
                    } => {
                        if editing {
                            mark_form_edit(fields, *active, name_edited, root_edited);
                        }
                        let field = &mut fields[*active];
                        match &mut field.kind {
                            FieldKind::Toggle => {}
                            FieldKind::Pick(list) => {
                                palette.cursor.key(&mut list.query, key.code);
                                if editing {
                                    list.selected = 0;
                                }
                            }
                            FieldKind::Text => palette.cursor.key(&mut field.value, key.code),
                            FieldKind::Path(picker) => {
                                palette.cursor.key(&mut field.value, key.code);
                                if editing {
                                    picker.selected = 0;
                                }
                            }
                        }
                    }
                    _ => {}
                }
                if editing {
                    palette.error = None;
                }
            }
            KeyCode::Up | KeyCode::Down if form_list_active(&palette.page) => {
                if let Page::Form { fields, active, .. } = &mut palette.page {
                    let delta = if matches!(key.code, KeyCode::Up) {
                        -1
                    } else {
                        1
                    };
                    let value = fields[*active].value.clone();
                    match &mut fields[*active].kind {
                        FieldKind::Pick(list) => list.move_selection(delta),
                        FieldKind::Path(picker) => {
                            let listing =
                                picker.listing(&value, &self.settings.picker_roots).clone();
                            picker.move_selection(&listing, delta);
                        }
                        FieldKind::Text | FieldKind::Toggle => {}
                    }
                }
            }
            KeyCode::Up | KeyCode::BackTab | KeyCode::Down | KeyCode::Tab => {
                let backwards = matches!(key.code, KeyCode::Up | KeyCode::BackTab);
                match &mut palette.page {
                    Page::Search { query, selected } => {
                        let count = self.palette_entries(query).len();
                        if count > 0 {
                            *selected = if backwards {
                                selected.saturating_sub(1)
                            } else {
                                (*selected + 1).min(count - 1)
                            };
                        }
                    }
                    Page::Form {
                        fields,
                        active,
                        name_edited,
                        root_edited,
                        ..
                    } => {
                        if matches!(key.code, KeyCode::Tab)
                            && matches!(fields[*active].kind, FieldKind::Path(_))
                        {
                            let selected = match &fields[*active].kind {
                                FieldKind::Path(picker) => picker.selected,
                                _ => 0,
                            };
                            // Nothing to complete (e.g. a leaf directory with
                            // no children) falls through to the shared Tab
                            // handling below instead of being a no-op.
                            if let Some(next) = complete_path(
                                &fields[*active].value,
                                &self.settings.picker_roots,
                                selected,
                            ) {
                                fields[*active].value = next;
                                if let FieldKind::Path(picker) = &mut fields[*active].kind {
                                    picker.selected = 0;
                                }
                                self.refresh_terminal_form(fields, *name_edited);
                                self.refresh_project_form(fields, *name_edited, *root_edited);
                                self.palette = Some(palette);
                                return action;
                            }
                        }
                        if matches!(key.code, KeyCode::Tab) && !accept_pick(&mut fields[*active]) {
                            self.palette = Some(palette);
                            return action;
                        }
                        if matches!(key.code, KeyCode::Tab) {
                            self.refresh_terminal_form(fields, *name_edited);
                            self.refresh_project_form(fields, *name_edited, *root_edited);
                        }
                        move_form_field(fields, active, backwards);
                    }
                    _ => {
                        self.palette = Some(palette);
                        return action;
                    }
                }
            }
            KeyCode::Enter => match &mut palette.page {
                Page::Search { query, selected } => {
                    let entries = self.palette_entries(query);
                    *selected = (*selected).min(entries.len().saturating_sub(1));
                    if let Some(entry) = entries.get(*selected) {
                        match entry.command.clone() {
                            Command::Switch(id) => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                self.select_session(id);
                                return self.request_selected();
                            }
                            Command::Hint(action) => {
                                if self.command_hint(&entry.command).is_some_and(|h| h.enabled) {
                                    if let Some(request_id) = palette.suggestions.inspect {
                                        self.ignored_responses.insert(request_id);
                                    }
                                    return self.run_hint(action);
                                }
                            }
                            command => palette.page = self.command_page(command),
                        }
                    }
                }
                Page::Form {
                    command,
                    fields,
                    active,
                    name_edited,
                    root_edited,
                } => {
                    if !accept_pick(&mut fields[*active]) {
                        let field = &fields[*active];
                        if field.required {
                            // A deferred submit must not wait on a pick that can
                            // never resolve; say which field went unmatched.
                            palette.error =
                                Some(format!("{} not found in available choices", field.label));
                        }
                        palette.submit_when_ready = None;
                        self.palette = Some(palette);
                        return action;
                    }
                    self.refresh_terminal_form(fields, *name_edited);
                    self.refresh_project_form(fields, *name_edited, *root_edited);
                    let visible = visible_indices(fields);
                    let last = submit
                        || visible.last().copied() == Some(*active)
                        || (matches!(command, Command::CreateWorkspace) && *active == 1);
                    if !last {
                        move_form_field(fields, active, false);
                    } else if let Some(index) = fields
                        .iter()
                        .position(|f| !f.hidden && f.required && f.value.trim().is_empty())
                    {
                        *active = index;
                        palette.error = Some(format!("{} is required", fields[index].label));
                    } else {
                        if matches!(command, Command::CreateWorkspace)
                            && !palette.suggestions.cache.contains_key(&fields[0].value)
                        {
                            palette.submit_when_ready = Some(submit);
                            self.palette = Some(palette);
                            return action;
                        }
                        let values: Vec<_> =
                            fields.iter().map(|f| f.value.trim().to_string()).collect();
                        let request = match command {
                            Command::CreateTerminal => self.create_terminal_request(&values),
                            Command::CreateWorkspace => Request::CreateWorkspace {
                                project: values[0].clone(),
                                name: values[1].clone(),
                                branch: if values[2] == "existing" {
                                    BranchRequest::Existing {
                                        branch: values[3].clone(),
                                    }
                                } else {
                                    BranchRequest::New {
                                        branch: values[3].clone(),
                                        base: values[4].clone(),
                                    }
                                },
                            },
                            Command::RegisterProject => Request::AddProject {
                                name: values[1].clone(),
                                repo: expand_path(&values[0]),
                                workspace_root: expand_path(&values[2]),
                            },
                            Command::RemoveWorkspace => {
                                let (project, name) = split_workspace(&values[0]);
                                Request::RemoveWorkspace { project, name }
                            }
                            Command::RemoveProject => Request::RemoveProject {
                                name: values[0].clone(),
                            },
                            _ => unreachable!(),
                        };
                        if matches!(command, Command::RemoveWorkspace | Command::RemoveProject) {
                            palette.page = removal_confirmation(request);
                        } else {
                            action = self.palette_submit(&mut palette, request);
                        }
                    }
                }
                Page::Confirm { request, .. } => {
                    let request = request.clone();
                    action = self.palette_submit(&mut palette, request);
                }
            },
            _ => {}
        }
        self.refresh_workspace_form(&mut palette);
        self.palette = Some(palette);
        action
    }

    fn detected_agents(&self) -> Vec<super::agents::AgentEntry> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let shell = std::env::var_os("SHELL");
        let launcher = std::env::current_exe().ok();
        apply_overrides(
            detect_agents(&path, shell.as_deref(), launcher.as_deref()),
            &self.settings.agents,
        )
    }

    fn refresh_terminal_form(&self, fields: &mut [Field], name_edited: bool) {
        if fields.first().map(|field| field.label) != Some("Agent") {
            return;
        }
        let custom = fields[0].value == "Custom";
        if let Some(command) = fields.iter_mut().find(|field| field.label == "Command") {
            command.hidden = !custom;
            command.required = custom;
        }
        if name_edited {
            return;
        }
        let agent = fields[0].value.clone();
        let (project, workspace) = split_workspace(&fields[1].value);
        let suggested = self.suggest_session_name(&project, &workspace, &agent);
        if let Some(name) = fields.iter_mut().find(|field| field.label == "Name") {
            name.value = suggested;
        }
    }

    fn refresh_project_form(&self, fields: &mut [Field], name_edited: bool, root_edited: bool) {
        if fields.first().map(|field| field.label) != Some("Repository") {
            return;
        }
        let repo = fields[0].value.trim_end_matches('/').to_string();
        if !name_edited {
            let name = Path::new(&repo)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(field) = fields.iter_mut().find(|field| field.label == "Name") {
                field.value = name;
            }
        }
        if root_edited {
            return;
        }
        let name = fields
            .iter()
            .find(|field| field.label == "Name")
            .map(|field| field.value.clone())
            .unwrap_or_default();
        if let Some(field) = fields
            .iter_mut()
            .find(|field| field.label == "Workspace root")
        {
            field.value = self
                .config_dir
                .join("workspaces")
                .join(name)
                .display()
                .to_string();
        }
    }

    fn suggest_session_name(&self, project: &str, workspace: &str, agent: &str) -> String {
        let sessions = self
            .hierarchy
            .projects
            .iter()
            .find(|candidate| candidate.name == project)
            .and_then(|candidate| {
                candidate
                    .workspaces
                    .iter()
                    .find(|candidate| candidate.name == workspace)
            })
            .map(|candidate| candidate.sessions.as_slice())
            .unwrap_or(&[]);
        if agent == "shell" && sessions.iter().all(|session| session.name != "local") {
            return "local".into();
        }
        for n in 1.. {
            let candidate = format!("{agent}-{n}");
            if sessions.iter().all(|session| session.name != candidate) {
                return candidate;
            }
        }
        unreachable!()
    }

    fn create_terminal_request(&self, values: &[String]) -> Request {
        let (project, workspace) = split_workspace(&values[1]);
        let agents = self.detected_agents();
        let agent = agents.iter().find(|entry| entry.name == values[0]);
        let argv = if agent.is_some_and(|entry| entry.source == AgentSource::Custom) {
            vec![
                OsString::from("/bin/sh"),
                OsString::from("-lc"),
                OsString::from(&values[3]),
            ]
        } else {
            agent.map(|entry| entry.argv.clone()).unwrap_or_else(|| {
                vec![std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into())]
            })
        };
        Request::CreateSession(CreateSessionRequest {
            project,
            workspace,
            name: values[2].clone(),
            label: Some(values[0].clone()),
            argv,
        })
    }

    fn palette_submit(&mut self, palette: &mut Palette, request: Request) -> DashboardAction {
        let request_id = self.next_request_id();
        palette.pending = Some(request_id);
        palette.error = None;
        DashboardAction::Request(ClientMessage {
            request_id,
            request,
        })
    }

    fn palette_form(&self, command: Command) -> Page {
        let (project, workspace) = self.creation_context();
        let text = |label, value: String, required| Field {
            label,
            value,
            required,
            hidden: false,
            edited: false,
            kind: FieldKind::Text,
        };
        let fields = match command {
            Command::CreateTerminal => {
                let agents = self.detected_agents();
                let agent_items = agents
                    .iter()
                    .map(|entry| PickItem {
                        label: entry.name.clone(),
                        value: entry.name.clone(),
                    })
                    .collect::<Vec<_>>();
                let agent_value = agent_items
                    .first()
                    .map(|item| item.value.clone())
                    .unwrap_or_default();
                let mut agent_list = PickList::new(agent_items);
                agent_list.select_value(&agent_value);
                let workspace_list =
                    PickList::workspaces(&self.hierarchy, &project, &workspace, true);
                let workspace_value = workspace_list
                    .accepted()
                    .map(|item| item.value.clone())
                    .unwrap_or_default();
                let (project_name, workspace_name) = split_workspace(&workspace_value);
                let name = self.suggest_session_name(&project_name, &workspace_name, &agent_value);
                vec![
                    Field {
                        label: "Agent",
                        value: agent_value,
                        required: true,
                        hidden: false,
                        edited: false,
                        kind: FieldKind::Pick(agent_list),
                    },
                    Field {
                        label: "Workspace",
                        value: workspace_value,
                        required: true,
                        hidden: false,
                        edited: false,
                        kind: FieldKind::Pick(workspace_list),
                    },
                    text("Name", name, true),
                    Field {
                        label: "Command",
                        value: String::new(),
                        required: false,
                        hidden: true,
                        edited: false,
                        kind: FieldKind::Text,
                    },
                ]
            }
            Command::CreateWorkspace => {
                let list = PickList::projects(&self.hierarchy, &project);
                let mut project = text(
                    "Project",
                    list.accepted()
                        .map(|item| item.value.clone())
                        .unwrap_or_default(),
                    true,
                );
                project.kind = FieldKind::Pick(list);
                let mut mode = text("Branch mode", "new".into(), true);
                mode.kind = FieldKind::Toggle;
                vec![
                    project,
                    text("Name", String::new(), true),
                    mode,
                    text("Branch", self.settings.branch_prefix.clone(), true),
                    text("Base", "main".into(), true),
                ]
            }
            Command::RegisterProject => vec![
                Field {
                    label: "Repository",
                    value: String::new(),
                    required: true,
                    hidden: false,
                    edited: false,
                    kind: FieldKind::Path(PathPicker::new()),
                },
                text("Name", String::new(), true),
                Field {
                    label: "Workspace root",
                    value: String::new(),
                    required: true,
                    hidden: false,
                    edited: false,
                    kind: FieldKind::Path(PathPicker::new()),
                },
            ],
            Command::RemoveWorkspace | Command::RemoveProject => {
                let (label, list) = if matches!(command, Command::RemoveWorkspace) {
                    (
                        "Workspace",
                        PickList::workspaces(&self.hierarchy, &project, &workspace, false),
                    )
                } else {
                    ("Project", PickList::projects(&self.hierarchy, &project))
                };
                let mut field = text(
                    label,
                    list.accepted()
                        .map(|item| item.value.clone())
                        .unwrap_or_default(),
                    true,
                );
                field.kind = FieldKind::Pick(list);
                vec![field]
            }
            _ => unreachable!(),
        };
        let active = if matches!(command, Command::CreateWorkspace) {
            1
        } else {
            0
        };
        Page::Form {
            command,
            fields,
            active,
            name_edited: false,
            root_edited: false,
        }
    }

    pub(super) fn palette_response(
        &mut self,
        request_id: u64,
        response: &Response,
    ) -> Option<Vec<ClientMessage>> {
        let palette = self.palette.as_mut()?;
        if palette.suggestions.inspect == Some(request_id) {
            palette.suggestions.inspect = None;
            if let Response::Inventory { registry, .. } = response {
                palette.suggestions.repos = registry
                    .projects
                    .iter()
                    .map(|project| (project.name.clone(), project.repo.clone()))
                    .collect();
            }
            let mut palette = self.palette.take().unwrap();
            self.refresh_workspace_form(&mut palette);
            self.palette = Some(palette);
            return Some(Vec::new());
        }
        if palette.pending != Some(request_id) {
            return None;
        }
        match response {
            Response::Error { code, message } => {
                palette.pending = None;
                palette.error = Some(format!("{code:?}: {message}"));
            }
            Response::Ok
                if matches!(
                    palette.page,
                    Page::Form {
                        command: Command::CreateWorkspace,
                        ..
                    }
                ) =>
            {
                palette.workspace_acknowledged = true;
                return Some(self.attach_created_workspace());
            }
            _ => {
                if let Some(request_id) = palette.suggestions.inspect {
                    self.ignored_responses.insert(request_id);
                }
                self.palette = None;
                self.error = None;
                if let Response::CreatedSession(session) = response {
                    self.select_session(session.id);
                    let request_id = self.next_request_id();
                    let outgoing = self
                        .view_request(self.outer_area, request_id)
                        .ok()
                        .flatten()
                        .into_iter()
                        .collect();
                    self.mode = InputMode::Terminal;
                    return Some(outgoing);
                }
            }
        }
        Some(Vec::new())
    }

    pub(super) fn attach_created_workspace(&mut self) -> Vec<ClientMessage> {
        let Some(palette) = &self.palette else {
            return Vec::new();
        };
        if !palette.workspace_acknowledged {
            return Vec::new();
        }
        let Page::Form {
            command: Command::CreateWorkspace,
            fields,
            ..
        } = &palette.page
        else {
            return Vec::new();
        };
        let workspace = self
            .hierarchy
            .projects
            .iter()
            .find(|project| project.name == fields[0].value.trim())
            .and_then(|project| {
                project
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.name == fields[1].value.trim())
            });
        let Some(workspace) = workspace else {
            return Vec::new();
        };
        let session = workspace
            .sessions
            .iter()
            .find(|session| {
                session.name == "local" && session.phase == crate::session::SessionPhase::Running
            })
            .map(|session| session.id);
        // The workspace is in the hierarchy, so creation finished; the palette
        // must not stay on "Working…" just because its shell already exited.
        self.palette = None;
        self.error = None;
        let Some(session) = session else {
            return Vec::new();
        };
        self.select_session(session);
        let request_id = self.next_request_id();
        let outgoing = self
            .view_request(self.outer_area, request_id)
            .ok()
            .flatten()
            .into_iter()
            .collect();
        self.mode = InputMode::Terminal;
        outgoing
    }

    fn palette_body(&self, body: Rect) -> (Vec<Line<'_>>, Vec<(usize, PaletteTarget)>, usize) {
        let palette = self.palette.as_ref().unwrap();
        let mut lines = Vec::new();
        let mut focus_line = 0;
        let mut targets = Vec::new();
        match &palette.page {
            Page::Search { query, selected } => {
                targets.push((lines.len(), PaletteTarget::Search));
                let (visible, _) = palette
                    .cursor
                    .display(query, body.width.saturating_sub(8) as usize);
                lines.push(Line::from(format!("Search: {visible}")));
                let entries = self.palette_entries(query);
                if entries.is_empty() {
                    lines.push(Line::from("No matching actions or terminals"));
                }
                let selected = (*selected).min(entries.len().saturating_sub(1));
                let count = usize::from(body.height.saturating_sub(1) / 2).max(1);
                let start = selected.saturating_sub(count - 1);
                for (index, entry) in entries.iter().enumerate().skip(start).take(count) {
                    let available = usize::from(body.width.saturating_sub(2));
                    let label = if let Command::Switch(id) = entry.command
                        && let Some(session) = find_session(self, id)
                    {
                        let suffix = format!(" (#{})", id.0);
                        if available >= suffix.len() {
                            format!(
                                "{}{}",
                                clip_text(&session.name, available - suffix.len()),
                                suffix
                            )
                        } else {
                            clip_text(&format!("#{}", id.0), available)
                        }
                    } else {
                        clip_text(&entry.label, available)
                    };
                    targets.push((lines.len(), PaletteTarget::Entry(index)));
                    lines.push(Line::styled(
                        format!("{} {}", if index == selected { "›" } else { " " }, label),
                        if index == selected {
                            Style::default().bg(MAUVE).fg(CRUST)
                        } else {
                            Style::default().fg(TEXT)
                        },
                    ));
                    if let Some(hint) = self.command_hint(&entry.command) {
                        lines.push(Line::styled(
                            format!("  {} — {}", hint.key, hint.description),
                            Style::default().fg(MUTED),
                        ));
                    } else {
                        lines.push(Line::from(""));
                    }
                }
            }
            Page::Form { fields, active, .. } => {
                for (index, field) in fields.iter().enumerate() {
                    if field.hidden {
                        continue;
                    }
                    if index == *active {
                        focus_line = lines.len() + 1;
                    }
                    targets.push((lines.len(), PaletteTarget::Field(index, false)));
                    lines.push(Line::styled(field.label, Style::default().fg(SUBTEXT)));
                    let editing = match &field.kind {
                        FieldKind::Pick(list) if index == *active && !list.query.is_empty() => {
                            &list.query
                        }
                        _ => &field.value,
                    };
                    let cursor = if index == *active {
                        palette.cursor
                    } else {
                        Default::default()
                    };
                    let (mut text, _) =
                        cursor.display(editing, body.width.saturating_sub(2) as usize);
                    if index != *active {
                        text.pop();
                    }
                    targets.push((lines.len(), PaletteTarget::Field(index, true)));
                    lines.push(Line::styled(
                        format!("{} {text}", if index == *active { "›" } else { " " }),
                        Style::default().fg(if index == *active { MAUVE } else { TEXT }),
                    ));
                    if index == *active
                        && let FieldKind::Pick(list) = &field.kind
                    {
                        let (options, selected) = list.lines(8);
                        focus_line = lines.len() + selected;
                        let start = list.selected.saturating_sub(selected);
                        for offset in 0..options.len().min(list.filtered().len()) {
                            targets.push((
                                lines.len() + offset,
                                PaletteTarget::Option(index, start + offset),
                            ));
                        }
                        lines.extend(options);
                    }
                    if index == *active
                        && let FieldKind::Path(picker) = &field.kind
                        && let Some(listing) = picker.cached()
                    {
                        let count = 8.min(listing.entries.len());
                        let start = picker.selected.saturating_sub(count.saturating_sub(1));
                        for (offset, item) in
                            listing.entries.iter().enumerate().skip(start).take(count)
                        {
                            let chosen = offset == picker.selected;
                            targets.push((lines.len(), PaletteTarget::Option(index, offset)));
                            if chosen {
                                focus_line = lines.len();
                            }
                            lines.push(Line::styled(
                                format!("  {} {}", if chosen { "›" } else { " " }, item.label),
                                if chosen {
                                    Style::default().bg(MAUVE).fg(CRUST)
                                } else {
                                    Style::default().fg(TEXT)
                                },
                            ));
                        }
                        if listing.omitted > 0 {
                            lines.push(Line::styled(
                                format!("  … {} more", listing.omitted),
                                Style::default().fg(MUTED),
                            ));
                        }
                    }
                }
            }
            Page::Confirm { target, .. } => {
                lines.push(Line::from(target.clone()));
            }
        }
        let scroll = palette
            .scroll
            .unwrap_or_else(|| {
                focus_line.saturating_sub(usize::from(body.height.saturating_sub(1)))
            })
            .min(lines.len().saturating_sub(body.height as usize));
        (lines, targets, scroll)
    }

    pub(super) fn draw_palette(&self, frame: &mut Frame<'_>) {
        let Some(palette) = &self.palette else {
            return;
        };
        let (area, inner, body, buttons) = palette_geometry(frame.area());
        frame.render_widget(Clear, area);
        let title = match &palette.page {
            Page::Search { .. } => "Command palette",
            Page::Form { command, .. } => match command {
                Command::CreateTerminal => "Create terminal",
                Command::CreateWorkspace => "Create workspace",
                Command::RegisterProject => "Register project",
                Command::RemoveWorkspace => "Remove workspace",
                _ => "Remove project",
            },
            Page::Confirm { .. } => "Confirm action",
        };
        let block = Block::bordered()
            .title(format!(" {title} "))
            .border_style(Style::default().fg(MAUVE))
            .style(Style::default().bg(CRUST).fg(TEXT));
        frame.render_widget(block, area);
        let footer_height = buttons.y.saturating_sub(body.bottom());
        let (lines, _, scroll) = self.palette_body(body);
        let paragraph = Paragraph::new(lines)
            .scroll((scroll as u16, 0))
            .style(Style::default().fg(TEXT));
        frame.render_widget(
            if matches!(palette.page, Page::Confirm { .. }) {
                paragraph.wrap(Wrap { trim: false })
            } else {
                paragraph
            },
            body,
        );
        let detail = if let Page::Search { query, selected } = &palette.page {
            let entries = self.palette_entries(query);
            let selected = (*selected).min(entries.len().saturating_sub(1));
            entries
                .get(selected)
                .map(|entry| {
                    if let Command::Switch(id) = entry.command
                        && let Some(session) = find_session(self, id)
                    {
                        format!(
                            "{}\n{}\n↑/↓ select · Enter focus · Esc close",
                            clip_text(
                                &format!("Project: {}", session.project),
                                usize::from(inner.width)
                            ),
                            clip_text(
                                &format!("Workspace: {}", session.workspace),
                                usize::from(inner.width)
                            )
                        )
                    } else if let Some(hint) = self.command_hint(&entry.command) {
                        format!(
                            "{} — {}\n↑/↓ select · Enter continue · Esc cancel",
                            hint.key, hint.description
                        )
                    } else {
                        "↑/↓ select · Enter continue · Esc cancel".into()
                    }
                })
                .or_else(|| Some("Edit search · Backspace delete · Esc close".into()))
        } else {
            None
        };
        let status = if palette.pending.is_some() {
            "Working…"
        } else if let Some(error) = &palette.error {
            error
        } else if palette.submit_when_ready.is_some() {
            "Waiting for Git suggestions before creating… · Esc cancel"
        } else if let Some(note) = &palette.suggestions.note {
            note
        } else {
            match palette.page {
                Page::Search { .. } => detail
                    .as_deref()
                    .unwrap_or("↑/↓ select · Enter continue · Esc cancel"),
                Page::Form {
                    command: Command::CreateWorkspace,
                    ..
                } => "Enter on Name submits · Tab next · ↑/↓ pick · Space/←/→ toggle · Esc cancel",
                Page::Form { .. } => {
                    "Tab accept/next · Enter next/submit · ↑/↓ pick · Ctrl-u clear · Esc cancel"
                }
                Page::Confirm { .. } => "Enter confirm · Esc cancel",
            }
        };
        let mut status_area = Rect::new(inner.x, inner.y + body.height, inner.width, footer_height);
        if palette.pending.is_none() && palette.error.is_some() && status_area.height > 1 {
            status_area.height -= 1;
            let recovery = match &palette.page {
                Page::Form { fields, active, .. }
                    if !matches!(fields[*active].kind, FieldKind::Toggle) =>
                {
                    "Esc cancel · Ctrl-u clear"
                }
                _ => "Esc cancel",
            };
            frame.render_widget(
                Paragraph::new(recovery).style(Style::default().fg(SUBTEXT)),
                Rect::new(inner.x, status_area.bottom(), inner.width, 1),
            );
        }
        frame.render_widget(
            Paragraph::new(button_text(&palette.page, buttons.width))
                .style(Style::default().fg(MAUVE)),
            buttons,
        );
        frame.render_widget(
            Paragraph::new(status)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(if palette.error.is_some() {
                    PEACH
                } else {
                    SUBTEXT
                })),
            status_area,
        );
    }
}

fn form_list_active(page: &Page) -> bool {
    matches!(
        page,
        Page::Form {
            fields,
            active,
            ..
        } if matches!(
            fields.get(*active).map(|field| &field.kind),
            Some(FieldKind::Pick(_) | FieldKind::Path(_))
        )
    )
}

fn mark_form_edit(
    fields: &mut [Field],
    active: usize,
    name_edited: &mut bool,
    root_edited: &mut bool,
) {
    fields[active].edited = true;
    match fields.get(active).map(|field| field.label) {
        Some("Name") => *name_edited = true,
        Some("Workspace root") => *root_edited = true,
        _ => {}
    }
}

fn split_workspace(value: &str) -> (String, String) {
    value
        .split_once(" / ")
        .map(|(project, workspace)| (project.to_string(), workspace.to_string()))
        .unwrap_or_else(|| (value.to_string(), String::new()))
}

fn removal_confirmation(request: Request) -> Page {
    let target = match &request {
        Request::RemoveWorkspace { project, name } => format!(
            "Remove workspace {project} / {name}. Remove its clean worktree; keep the branch."
        ),
        Request::RemoveProject { name } => {
            format!("Unregister project {name}. Keep the repository.")
        }
        _ => unreachable!("only workspace/project removal uses this confirmation"),
    };
    Page::Confirm { request, target }
}

#[cfg(test)]
mod mouse_tests {
    use super::*;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::{Terminal, backend::TestBackend};

    fn click(d: &mut Dashboard, needle: &str, dx: u16, width: u16, height: u16) -> DashboardAction {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| super::super::render::draw_dashboard_at(f, d, 0))
            .unwrap();
        let b = terminal.backend().buffer();
        let mut found = None;
        for y in 0..height {
            for x in 0..width {
                let row: String = (x..width).map(|col| b[(col, y)].symbol()).collect();
                if row.starts_with(needle) {
                    found = Some((x + dx, y));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let (column, row) = found.unwrap_or_else(|| panic!("missing rendered control: {needle}"));
        d.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, width, height),
        )
    }
    fn dashboard() -> Dashboard {
        Dashboard::new(crate::session::TerminalSize {
            rows: 30,
            cols: 100,
        })
    }
    #[test]
    fn mouse_form_focus_unicode_edit_and_cancel() {
        let mut d = dashboard();
        d.open_register_project();
        assert!(d.mouse_capture_required());
        click(&mut d, "Name", 0, 100, 30);
        d.palette_paste("a界éz");
        click(&mut d, "a界", 3, 100, 30);
        d.palette_paste("X");
        d.palette_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        d.palette_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        let Page::Form { fields, active, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(*active, 1);
        assert_eq!(fields[1].value, "a界z");
        click(&mut d, "[Cancel]", 1, 100, 30);
        assert!(d.palette.is_none());
    }
    #[test]
    fn mouse_actions_search_and_confirmation_pending_guard() {
        let mut d = dashboard();
        d.mode = InputMode::Terminal;
        click(&mut d, "[Actions]", 1, 100, 30);
        click(&mut d, "Register project", 1, 100, 30);
        assert!(matches!(
            d.palette.as_ref().unwrap().page,
            Page::Form {
                command: Command::RegisterProject,
                ..
            }
        ));
        d.palette = Some(Palette {
            page: removal_confirmation(Request::RemoveProject {
                name: "demo".into(),
            }),
            ..Palette::new()
        });
        let DashboardAction::Request(message) = click(&mut d, "[Confirm]", 1, 100, 30) else {
            panic!("confirm must submit")
        };
        assert_eq!(
            message.request,
            Request::RemoveProject {
                name: "demo".into()
            }
        );
        assert!(!matches!(
            click(&mut d, "[Confirm]", 1, 100, 30),
            DashboardAction::Request(_)
        ));
        click(&mut d, "[Cancel]", 1, 100, 30);
        assert!(d.ignored_responses.contains(&message.request_id));
    }
    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }
    #[test]
    fn mouse_menu_back_and_close_without_clickthrough() {
        let mut d = dashboard();
        click(&mut d, "[Menu]", 1, 100, 30);
        click(&mut d, "v  View", 1, 100, 30);
        assert_eq!(d.whichkey.as_ref().unwrap().group, Some('v'));
        click(&mut d, "[Back]", 1, 100, 30);
        assert_eq!(d.whichkey.as_ref().unwrap().group, None);
        d.mouse_action(
            mouse(MouseEventKind::Down(MouseButton::Left), 0, 0),
            Rect::new(0, 0, 100, 30),
        );
        assert!(d.whichkey.is_some());
        click(&mut d, "[Close]", 1, 100, 30);
        assert!(d.whichkey.is_none());
    }
    #[test]
    fn mouse_scrolled_picker_accepts_visible_option() {
        let mut d = dashboard();
        d.open_register_project();
        let p = d.palette.as_mut().unwrap();
        let Page::Form { fields, .. } = &mut p.page else {
            panic!()
        };
        let mut list = PickList::new(
            (0..20)
                .map(|i| PickItem {
                    label: format!("Choice-{i:02}"),
                    value: format!("value-{i}"),
                })
                .collect(),
        );
        list.selected = 10;
        fields[0].kind = FieldKind::Pick(list);
        // Wheel the rendered option viewport beyond its initial eight rows.
        let (_, _, body, _) = palette_geometry(Rect::new(0, 0, 100, 30));
        let (_, targets, scroll) = d.palette_body(body);
        let y = targets
            .iter()
            .find(|(_, t)| matches!(t, PaletteTarget::Option(_, 10)))
            .unwrap()
            .0;
        for _ in 0..3 {
            d.mouse_action(
                mouse(
                    MouseEventKind::ScrollDown,
                    body.x + 5,
                    body.y + (y - scroll) as u16,
                ),
                Rect::new(0, 0, 100, 30),
            );
        }
        click(&mut d, "Choice-13", 1, 100, 30);
        let Page::Form { fields, active, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(fields[0].value, "value-13");
        assert_eq!(*active, 1);
    }
    #[test]
    fn mouse_submit_from_first_field_and_outside_overlay_guard() {
        let mut d = dashboard();
        d.open_register_project();
        let Page::Form {
            fields,
            name_edited,
            root_edited,
            ..
        } = &mut d.palette.as_mut().unwrap().page
        else {
            panic!()
        };
        *name_edited = true;
        *root_edited = true;
        fields[0].value = "/tmp/repo".into();
        fields[1].value = "demo".into();
        fields[2].value = "/tmp/work".into();
        d.mouse_action(
            mouse(MouseEventKind::Down(MouseButton::Left), 0, 0),
            Rect::new(0, 0, 100, 30),
        );
        assert!(d.palette.is_some());
        let DashboardAction::Request(message) = click(&mut d, "[Submit]", 1, 100, 30) else {
            panic!("submit must validate all fields from first field")
        };
        assert!(matches!(message.request, Request::AddProject { name, .. } if name == "demo"));
    }
    #[test]
    fn mouse_toggle_and_scrolled_path_row() {
        let mut d = dashboard();
        d.open_create_workspace();
        click(&mut d, "Branch mode", 1, 100, 30);
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(fields[2].value, "existing");
        assert!(fields[4].hidden);
        d.open_register_project();
        let dir = tempfile::tempdir().unwrap();
        for i in 0..15 {
            std::fs::create_dir(dir.path().join(format!("child-{i:02}"))).unwrap();
        }
        d.palette_paste(&format!("{}/", dir.path().display()));
        for _ in 0..11 {
            d.palette_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        click(&mut d, "child-11", 1, 100, 30);
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert!(fields[0].value.ends_with("/child-11/"));
    }
    #[test]
    fn mouse_narrow_form_scroll_reaches_hidden_fields() {
        let mut d = dashboard();
        d.open_register_project();
        let area = Rect::new(0, 0, 22, 9);
        let (_, _, body, _) = palette_geometry(area);
        for _ in 0..12 {
            d.mouse_action(mouse(MouseEventKind::ScrollDown, body.x, body.y), area);
        }
        click(&mut d, "Workspace root", 1, 22, 9);
        d.palette_paste("/tmp/界/root");
        let Page::Form { fields, active, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(*active, 2);
        assert_eq!(fields[2].value, "/tmp/界/root");
        click(&mut d, "[Cancel]", 1, 22, 9);
        assert!(d.palette.is_none());
    }
    #[test]
    fn mouse_text_search_and_narrow_unicode_cursor() {
        let mut d = dashboard();
        d.open_palette();
        d.palette_paste("creat workspace");
        click(&mut d, "Search: creat", 13, 100, 30);
        d.palette_paste("e");
        let Page::Search { query, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(query, "create workspace");
        d.open_register_project();
        click(&mut d, "Name", 0, 22, 12);
        d.palette_paste("abcdefghijklmnop界Z");
        click(&mut d, "界", 0, 22, 12);
        d.palette_paste("X");
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(fields[1].value, "abcdefghijklmnopX界Z");
    }
    #[test]
    fn mouse_pick_value_edits_at_visible_cursor() {
        let mut d = dashboard();
        d.open_register_project();
        let Page::Form { fields, .. } = &mut d.palette.as_mut().unwrap().page else {
            panic!()
        };
        fields[0].value = "hello".into();
        fields[0].kind = FieldKind::Pick(PickList::new(vec![PickItem {
            label: "hello".into(),
            value: "hello".into(),
        }]));
        click(&mut d, "hello", 2, 100, 30);
        d.palette_paste("X");
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        let FieldKind::Pick(list) = &fields[0].kind else {
            panic!()
        };
        assert_eq!(list.query, "heXllo");
    }

    #[test]
    fn mouse_tiny_overlay_controls_remain_visible() {
        let mut d = dashboard();
        d.open_register_project();
        click(&mut d, "[X]", 1, 12, 8);
        assert!(d.palette.is_none());
        d.key(KeyCode::Char('?'));
        click(&mut d, "[<]", 1, 18, 12);
        assert!(d.whichkey.is_some());
        click(&mut d, "[X]", 1, 18, 12);
        assert!(d.whichkey.is_none());
    }
    #[test]
    fn mouse_cursor_uses_rendered_emoji_grapheme_width() {
        let mut d = dashboard();
        d.open_register_project();
        click(&mut d, "Name", 0, 100, 30);
        d.palette_paste("a👩‍💻Z");
        click(&mut d, "a👩‍💻", 3, 100, 30);
        d.palette_paste("X");
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(fields[1].value, "a👩‍💻XZ");
        d.palette_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        d.palette_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(fields[1].value, "aZ");
    }
    #[test]
    fn mouse_submit_from_project_survives_waiting_for_git_suggestions_once() {
        let mut d = dashboard();
        d.open_create_workspace();
        let Page::Form { fields, .. } = &mut d.palette.as_mut().unwrap().page else {
            panic!()
        };
        fields[0].value = "demo".into();
        fields[0].kind = FieldKind::Pick(PickList::new(vec![PickItem {
            label: "demo".into(),
            value: "demo".into(),
        }]));
        d.palette_paste("deferred");
        click(&mut d, "Project", 0, 100, 30);
        assert!(!matches!(
            click(&mut d, "[Submit]", 1, 100, 30),
            DashboardAction::Request(_)
        ));
        d.poll_palette();
        assert!(d.drain_outbox().is_empty(), "inspection is still pending");
        // Supply the awaited suggestion result deterministically; exercise the
        // production idle-poll replay, without a timing-dependent Git worker.
        let palette = d.palette.as_mut().unwrap();
        palette.suggestions.cache.insert(
            "demo".into(),
            Ok(super::super::git_hints::Hints {
                branches: vec!["develop".into()],
                base: "develop".into(),
            }),
        );
        palette.suggestions.inspect = None;
        d.poll_palette();
        let mut drained = d.drain_outbox();
        assert_eq!(
            drained.len(),
            1,
            "explicit Submit must survive the suggestion wait from Project"
        );
        let message = drained.remove(0);
        assert!(
            matches!(message.request, Request::CreateWorkspace { project, name, branch: BranchRequest::New { base, .. } } if project == "demo" && name == "deferred" && base == "develop")
        );
        d.poll_palette();
        assert!(
            d.drain_outbox().is_empty(),
            "later polls cannot repeat the request"
        );
        assert!(
            !matches!(
                click(&mut d, "[Submit]", 1, 100, 30),
                DashboardAction::Request(_)
            ),
            "pending request cannot repeat on click"
        );
    }
}

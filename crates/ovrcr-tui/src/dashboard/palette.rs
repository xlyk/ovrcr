use super::agents::{AgentSource, apply_overrides, detect_agents};
use super::hints::{HintAction, KeyHint, key_hints};
use super::input::is_browse_key;
use super::picker::{PickItem, PickList};
use super::render::{CRUST, MAUVE, MUTED, PEACH, TEXT};
use super::state::find_session;
use super::{Dashboard, DashboardAction, InputMode};
use crate::protocol::{BranchRequest, ClientMessage, CreateSessionRequest, Request, Response};
use crate::session::SessionId;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use std::ffi::OsString;

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
    Pick(PickList),
}

struct Field {
    label: &'static str,
    value: String,
    required: bool,
    hidden: bool,
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
        }
    }

    fn insert(&mut self, text: &str) {
        if self.pending.is_some() {
            return;
        }
        let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
        if text.is_empty() {
            return;
        }
        match &mut self.page {
            Page::Search { query, selected } => {
                *selected = 0;
                query.push_str(&text);
            }
            Page::Form {
                fields,
                active,
                name_edited,
                ..
            } => {
                if fields[*active].label == "Name" {
                    *name_edited = true;
                }
                match &mut fields[*active].kind {
                    FieldKind::Pick(list) => list.on_insert(&text),
                    FieldKind::Text => fields[*active].value.push_str(&text),
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
        self.whichkey = None;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        if self.palette.is_none() {
            self.palette = Some(Palette::new());
        }
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn open_create_terminal(&mut self) -> DashboardAction {
        self.whichkey = None;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.palette = Some(Palette {
            page: self.palette_form(Command::CreateTerminal),
            pending: None,
            error: None,
        });
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn open_hint_form(&mut self, key: char) -> DashboardAction {
        let command = match key {
            'w' => Command::CreateWorkspace,
            'a' => Command::RegisterProject,
            'X' => {
                let Some(id) = self
                    .focused_session()
                    .filter(|id| find_session(self, *id).is_some())
                else {
                    return DashboardAction::None;
                };
                Command::CloseTerminal(id)
            }
            _ => unreachable!(),
        };
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.whichkey = None;
        self.palette = Some(Palette {
            page: self.command_page(command),
            pending: None,
            error: None,
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
        self.palette.as_mut().unwrap().insert(text);
        DashboardAction::Redraw
    }

    fn palette_entries(&self, query: &str) -> Vec<Entry> {
        let mut entries = vec![
            Entry {
                label: "Create terminal (n)".into(),
                command: Command::CreateTerminal,
            },
            Entry {
                label: "Create workspace".into(),
                command: Command::CreateWorkspace,
            },
            Entry {
                label: "Register project".into(),
                command: Command::RegisterProject,
            },
        ];
        if let Some(id) = self
            .focused_session()
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

    pub(super) fn palette_key(&mut self, key: KeyEvent) -> DashboardAction {
        let mut palette = self.palette.take().unwrap();
        if key.code == KeyCode::Esc || is_browse_key(key) {
            // Escape closes even while a request runs; its late response is
            // dropped so it cannot surface after the user has moved on.
            if let Some(request_id) = palette.pending {
                self.ignored_responses.insert(request_id);
            }
            return DashboardAction::Redraw;
        }
        // Other keys wait for the running request so a submit cannot repeat.
        if palette.pending.is_some() {
            self.palette = Some(palette);
            return DashboardAction::Redraw;
        }
        let mut action = DashboardAction::Redraw;
        match key.code {
            KeyCode::Char('u' | 'U') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match &mut palette.page {
                    Page::Search { query, selected } => {
                        query.clear();
                        *selected = 0;
                    }
                    Page::Form {
                        fields,
                        active,
                        name_edited,
                        ..
                    } => {
                        if fields[*active].label == "Name" {
                            *name_edited = true;
                        }
                        fields[*active].value.clear();
                        if let FieldKind::Pick(list) = &mut fields[*active].kind {
                            list.query.clear();
                            list.selected = 0;
                        }
                    }
                    _ => {}
                }
                palette.error = None;
            }
            KeyCode::Char(ch)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                palette.insert(&ch.to_string())
            }
            KeyCode::Backspace => {
                match &mut palette.page {
                    Page::Search { query, selected } => {
                        query.pop();
                        *selected = 0;
                    }
                    Page::Form {
                        fields,
                        active,
                        name_edited,
                        ..
                    } => {
                        if fields[*active].label == "Name" {
                            *name_edited = true;
                        }
                        match &mut fields[*active].kind {
                            FieldKind::Pick(list) => list.on_backspace(),
                            FieldKind::Text => {
                                fields[*active].value.pop();
                            }
                        }
                    }
                    _ => {}
                }
                palette.error = None;
            }
            KeyCode::Up | KeyCode::Down if form_pick_active(&palette.page) => {
                if let Page::Form { fields, active, .. } = &mut palette.page
                    && let FieldKind::Pick(list) = &mut fields[*active].kind
                {
                    let delta = if matches!(key.code, KeyCode::Up) {
                        -1
                    } else {
                        1
                    };
                    list.move_selection(delta);
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
                        ..
                    } => {
                        if matches!(key.code, KeyCode::Tab) && !accept_pick(&mut fields[*active]) {
                            self.palette = Some(palette);
                            return action;
                        }
                        if matches!(key.code, KeyCode::Tab) {
                            self.refresh_terminal_form(fields, *name_edited);
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
                    if let Some(entry) = self.palette_entries(query).get(*selected) {
                        match entry.command.clone() {
                            Command::Switch(id) => {
                                self.select_session(id);
                                return self.request_selected();
                            }
                            Command::Hint(action) => {
                                if self.command_hint(&entry.command).is_some_and(|h| h.enabled) {
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
                } => {
                    if !accept_pick(&mut fields[*active]) {
                        self.palette = Some(palette);
                        return action;
                    }
                    self.refresh_terminal_form(fields, *name_edited);
                    let visible = visible_indices(fields);
                    let last = visible.last().copied() == Some(*active);
                    if !last {
                        move_form_field(fields, active, false);
                    } else if let Some(index) = fields
                        .iter()
                        .position(|f| !f.hidden && f.required && f.value.trim().is_empty())
                    {
                        *active = index;
                        palette.error = Some(format!("{} is required", fields[index].label));
                    } else {
                        let values: Vec<_> =
                            fields.iter().map(|f| f.value.trim().to_string()).collect();
                        let request = match command {
                            Command::CreateTerminal => self.create_terminal_request(&values),
                            Command::CreateWorkspace => Request::CreateWorkspace {
                                project: values[0].clone(),
                                name: values[1].clone(),
                                branch: if values[3].is_empty() {
                                    BranchRequest::Existing {
                                        branch: values[2].clone(),
                                    }
                                } else {
                                    BranchRequest::New {
                                        branch: values[2].clone(),
                                        base: values[3].clone(),
                                    }
                                },
                            },
                            Command::RegisterProject => Request::AddProject {
                                name: values[0].clone(),
                                repo: values[1].clone().into(),
                                workspace_root: values[2].clone().into(),
                            },
                            Command::RemoveWorkspace => Request::RemoveWorkspace {
                                project: values[0].clone(),
                                name: values[1].clone(),
                            },
                            Command::RemoveProject => Request::RemoveProject {
                                name: values[0].clone(),
                            },
                            _ => unreachable!(),
                        };
                        if matches!(command, Command::RemoveWorkspace | Command::RemoveProject) {
                            let target = match command {
                                Command::RemoveWorkspace => format!(
                                    "Remove workspace {} / {}. Remove its clean worktree; keep the branch.",
                                    values[0], values[1]
                                ),
                                _ => format!(
                                    "Unregister project {}. Keep the repository.",
                                    values[0]
                                ),
                            };
                            palette.page = Page::Confirm { request, target };
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
        self.palette = Some(palette);
        action
    }

    fn detected_agents(&self) -> Vec<super::agents::AgentEntry> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let shell = std::env::var_os("SHELL");
        apply_overrides(
            detect_agents(&path, shell.as_deref()),
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
                let workspace_items = self
                    .hierarchy
                    .projects
                    .iter()
                    .flat_map(|project| {
                        project.workspaces.iter().map(|workspace| {
                            let label = format!("{} / {}", project.name, workspace.name);
                            PickItem {
                                value: label.clone(),
                                label,
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                let workspace_value = (!workspace.is_empty())
                    .then(|| format!("{project} / {workspace}"))
                    .or_else(|| {
                        workspace_items
                            .iter()
                            .find(|item| item.value.starts_with(&format!("{project} / ")))
                            .map(|item| item.value.clone())
                    })
                    .or_else(|| workspace_items.first().map(|item| item.value.clone()))
                    .unwrap_or_default();
                let mut workspace_list = PickList::new(workspace_items);
                workspace_list.select_value(&workspace_value);
                let (project_name, workspace_name) = split_workspace(&workspace_value);
                let name = self.suggest_session_name(&project_name, &workspace_name, &agent_value);
                vec![
                    Field {
                        label: "Agent",
                        value: agent_value,
                        required: true,
                        hidden: false,
                        kind: FieldKind::Pick(agent_list),
                    },
                    Field {
                        label: "Workspace",
                        value: workspace_value,
                        required: true,
                        hidden: false,
                        kind: FieldKind::Pick(workspace_list),
                    },
                    text("Name", name, true),
                    Field {
                        label: "Command",
                        value: String::new(),
                        required: false,
                        hidden: true,
                        kind: FieldKind::Text,
                    },
                ]
            }
            Command::CreateWorkspace => vec![
                text("Project", project, true),
                text("Name", String::new(), true),
                text("Branch", String::new(), true),
                text("Base (blank = existing branch)", "main".into(), false),
            ],
            Command::RegisterProject => vec![
                text("Name", String::new(), true),
                text("Repository (absolute path)", String::new(), true),
                text("Workspace root (absolute path)", String::new(), true),
            ],
            Command::RemoveWorkspace => vec![
                text("Project", project, true),
                text("Workspace", workspace, true),
            ],
            Command::RemoveProject => vec![text("Project", project, true)],
            _ => unreachable!(),
        };
        Page::Form {
            command,
            fields,
            active: 0,
            name_edited: false,
        }
    }

    pub(super) fn palette_response(
        &mut self,
        request_id: u64,
        response: &Response,
    ) -> Option<Vec<ClientMessage>> {
        let palette = self.palette.as_mut()?;
        if palette.pending != Some(request_id) {
            return None;
        }
        match response {
            Response::Error { code, message } => {
                palette.pending = None;
                palette.error = Some(format!("{code:?}: {message}"));
            }
            _ => {
                self.palette = None;
                self.error = None;
                if let Response::CreatedSession(session) = response {
                    let request_id = self.next_request_id();
                    let outgoing = self
                        .select_request(session.id, request_id)
                        .into_iter()
                        .collect();
                    self.mode = InputMode::Terminal;
                    return Some(outgoing);
                }
            }
        }
        Some(Vec::new())
    }

    pub(super) fn draw_palette(&self, frame: &mut Frame<'_>) {
        let Some(palette) = &self.palette else {
            return;
        };
        let outer = frame.area();
        let width = outer.width.min(82);
        let height = outer.height.min(19);
        let area = Rect::new(
            outer.x + (outer.width - width) / 2,
            outer.y + (outer.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        let block = Block::bordered()
            .title(" Command palette ")
            .border_style(Style::default().fg(MAUVE))
            .style(Style::default().bg(CRUST).fg(TEXT));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let footer_height = inner.height.min(4);
        let body = Rect::new(inner.x, inner.y, inner.width, inner.height - footer_height);
        let mut lines = Vec::new();
        let mut focus_line = 0;
        match &palette.page {
            Page::Search { query, selected } => {
                lines.push(Line::from(format!("Search: {query}▏")));
                let entries = self.palette_entries(query);
                if entries.is_empty() {
                    lines.push(Line::from("No matching actions"));
                }
                let count = usize::from(body.height.saturating_sub(1) / 2).max(1);
                let start = selected.saturating_sub(count - 1);
                for (index, entry) in entries.iter().enumerate().skip(start).take(count) {
                    lines.push(Line::styled(
                        format!(
                            "{} {}",
                            if index == *selected { "›" } else { " " },
                            entry.label
                        ),
                        if index == *selected {
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
            Page::Form {
                command,
                fields,
                active,
                ..
            } => {
                let title = match command {
                    Command::CreateTerminal => "Create terminal",
                    Command::CreateWorkspace => "Create workspace",
                    Command::RegisterProject => "Register project",
                    Command::RemoveWorkspace => "Remove workspace",
                    _ => "Remove project",
                };
                lines.push(Line::from(title));
                for (index, field) in fields.iter().enumerate() {
                    if field.hidden {
                        continue;
                    }
                    if index == *active {
                        focus_line = lines.len() + 1;
                    }
                    lines.push(Line::styled(field.label, Style::default().fg(MUTED)));
                    let editing = match &field.kind {
                        FieldKind::Pick(list) if index == *active && !list.query.is_empty() => {
                            &list.query
                        }
                        _ => &field.value,
                    };
                    let text = format!(
                        "{} {}{}",
                        if index == *active { "›" } else { " " },
                        editing,
                        if index == *active { "▏" } else { "" }
                    );
                    // Keep the end of an edited path visible in a narrow window.
                    let tail: String = text
                        .chars()
                        .rev()
                        .take(usize::from(inner.width))
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    lines.push(Line::styled(
                        tail,
                        if index == *active {
                            Style::default().fg(MAUVE)
                        } else {
                            Style::default().fg(TEXT)
                        },
                    ));
                    if index == *active
                        && let FieldKind::Pick(list) = &field.kind
                    {
                        let filtered = list.filtered();
                        let count = 8.min(filtered.len());
                        let start = list.selected.saturating_sub(count.saturating_sub(1));
                        for (offset, item) in filtered.iter().enumerate().skip(start).take(count) {
                            let chosen = offset == list.selected;
                            lines.push(Line::styled(
                                format!("  {} {}", if chosen { "›" } else { " " }, item.label),
                                if chosen {
                                    Style::default().bg(MAUVE).fg(CRUST)
                                } else {
                                    Style::default().fg(TEXT)
                                },
                            ));
                        }
                    }
                }
            }
            Page::Confirm { target, .. } => {
                lines.push(Line::from("Confirm action"));
                lines.push(Line::from(target.clone()));
            }
        }
        let scroll = focus_line.saturating_sub(usize::from(body.height.saturating_sub(1)));
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
        let selected_hint = if let Page::Search { query, selected } = &palette.page {
            self.palette_entries(query)
                .get(*selected)
                .and_then(|entry| self.command_hint(&entry.command))
        } else {
            None
        };
        let detail = selected_hint.map(|hint| {
            format!(
                "{} — {}\n↑/↓ select · Enter continue · Esc cancel",
                hint.key, hint.description
            )
        });
        let status = if palette.pending.is_some() {
            "Working…"
        } else if let Some(error) = &palette.error {
            error
        } else {
            match palette.page {
                Page::Search { .. } => detail
                    .as_deref()
                    .unwrap_or("↑/↓ select · Enter continue · Esc cancel"),
                Page::Form { .. } => {
                    "Tab accept/next · Enter next/submit · ↑/↓ pick · Ctrl-u clear · Esc cancel"
                }
                Page::Confirm { .. } => "Enter confirm · Esc cancel",
            }
        };
        frame.render_widget(
            Paragraph::new(status)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(if palette.error.is_some() {
                    PEACH
                } else {
                    MUTED
                })),
            Rect::new(inner.x, inner.y + body.height, inner.width, footer_height),
        );
    }
}

fn form_pick_active(page: &Page) -> bool {
    matches!(
        page,
        Page::Form {
            fields,
            active,
            ..
        } if matches!(fields.get(*active).map(|field| &field.kind), Some(FieldKind::Pick(_)))
    )
}

fn split_workspace(value: &str) -> (String, String) {
    value
        .split_once(" / ")
        .map(|(project, workspace)| (project.to_string(), workspace.to_string()))
        .unwrap_or_else(|| (value.to_string(), String::new()))
}

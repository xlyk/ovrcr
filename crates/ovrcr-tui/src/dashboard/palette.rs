use super::input::is_browse_key;
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

#[derive(Clone)]
enum Command {
    CreateTerminal,
    CreateWorkspace,
    RegisterProject,
    CloseTerminal(SessionId),
    RemoveWorkspace,
    RemoveProject,
    Switch(SessionId),
}

struct Entry {
    label: String,
    command: Command,
}

struct Field {
    label: &'static str,
    value: String,
    required: bool,
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
        let value = match &mut self.page {
            Page::Search { query, selected } => {
                *selected = 0;
                query
            }
            Page::Form { fields, active, .. } => &mut fields[*active].value,
            Page::Confirm { .. } => return,
        };
        value.extend(text.chars().filter(|ch| !ch.is_control()));
        self.error = None;
    }
}

impl Dashboard {
    pub(super) fn open_palette(&mut self) -> DashboardAction {
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        if self.palette.is_none() {
            self.palette = Some(Palette::new());
        }
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn palette_paste(&mut self, text: &str) -> DashboardAction {
        self.palette.as_mut().unwrap().insert(text);
        DashboardAction::Redraw
    }

    fn palette_entries(&self, query: &str) -> Vec<Entry> {
        let mut entries = vec![
            Entry {
                label: "Create terminal".into(),
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
                    Page::Form { fields, active, .. } => fields[*active].value.clear(),
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
                    Page::Form { fields, active, .. } => {
                        fields[*active].value.pop();
                    }
                    _ => {}
                }
                palette.error = None;
            }
            KeyCode::Up | KeyCode::BackTab | KeyCode::Down | KeyCode::Tab => {
                let backwards = matches!(key.code, KeyCode::Up | KeyCode::BackTab);
                let (selected, count) = match &mut palette.page {
                    Page::Search { query, selected } => {
                        (selected, self.palette_entries(query).len())
                    }
                    Page::Form { fields, active, .. } => (active, fields.len()),
                    _ => {
                        self.palette = Some(palette);
                        return action;
                    }
                };
                if count > 0 {
                    *selected = if backwards {
                        selected.saturating_sub(1)
                    } else {
                        (*selected + 1).min(count - 1)
                    };
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
                            Command::CloseTerminal(id) => {
                                if let Some(session) = find_session(self, id) {
                                    palette.page = Page::Confirm {
                                        request: Request::CloseTerminal { session: id },
                                        target: format!(
                                            "Close {} / {} / {} (#{}). Stop its processes and remove its record.",
                                            session.project, session.workspace, session.name, id.0
                                        ),
                                    };
                                }
                            }
                            command => palette.page = self.palette_form(command),
                        }
                    }
                }
                Page::Form {
                    command,
                    fields,
                    active,
                } => {
                    if *active + 1 < fields.len() {
                        *active += 1;
                    } else if let Some(index) = fields
                        .iter()
                        .position(|f| f.required && f.value.trim().is_empty())
                    {
                        *active = index;
                        palette.error = Some(format!("{} is required", fields[index].label));
                    } else {
                        let values: Vec<_> =
                            fields.iter().map(|f| f.value.trim().to_string()).collect();
                        let request = match command {
                            Command::CreateTerminal => {
                                Request::CreateSession(CreateSessionRequest {
                                    project: values[0].clone(),
                                    workspace: values[1].clone(),
                                    name: values[2].clone(),
                                    label: None,
                                    argv: if values[3].is_empty() {
                                        vec![
                                            std::env::var_os("SHELL")
                                                .unwrap_or_else(|| "/bin/sh".into()),
                                        ]
                                    } else {
                                        vec![
                                            "/bin/sh".into(),
                                            "-lc".into(),
                                            values[3].clone().into(),
                                        ]
                                    },
                                })
                            }
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
        let selected = self.focused_session().and_then(|id| find_session(self, id));
        let project = selected.map(|s| s.project.clone()).unwrap_or_default();
        let workspace = selected.map(|s| s.workspace.clone()).unwrap_or_default();
        let field = |label, value, required| Field {
            label,
            value,
            required,
        };
        let fields = match command {
            Command::CreateTerminal => vec![
                field("Project", project, true),
                field("Workspace", workspace, true),
                field("Name", String::new(), true),
                field("Command (blank = shell)", String::new(), false),
            ],
            Command::CreateWorkspace => vec![
                field("Project", project, true),
                field("Name", String::new(), true),
                field("Branch", String::new(), true),
                field("Base (blank = existing branch)", "main".into(), false),
            ],
            Command::RegisterProject => vec![
                field("Name", String::new(), true),
                field("Repository (absolute path)", String::new(), true),
                field("Workspace root (absolute path)", String::new(), true),
            ],
            Command::RemoveWorkspace => vec![
                field("Project", project, true),
                field("Workspace", workspace, true),
            ],
            Command::RemoveProject => vec![field("Project", project, true)],
            _ => unreachable!(),
        };
        Page::Form {
            command,
            fields,
            active: 0,
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
                    return Some(
                        self.select_request(session.id, request_id)
                            .into_iter()
                            .collect(),
                    );
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
                let count = usize::from(body.height.saturating_sub(1)).max(1);
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
                }
            }
            Page::Form {
                command,
                fields,
                active,
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
                    lines.push(Line::styled(field.label, Style::default().fg(MUTED)));
                    let text = format!(
                        "{} {}{}",
                        if index == *active { "›" } else { " " },
                        field.value,
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
                }
                focus_line = 2 + *active * 2;
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
        let status = if palette.pending.is_some() {
            "Working…"
        } else if let Some(error) = &palette.error {
            error
        } else {
            match palette.page {
                Page::Search { .. } => "↑/↓ select · Enter continue · Esc cancel",
                Page::Form { .. } => {
                    "Tab/↑/↓ field · Enter next/submit · Ctrl-u clear · Esc cancel"
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

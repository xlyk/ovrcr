//! What a pane with no session shows instead of a blank screen: numbered first
//! steps before any project exists, a card for the selected project or workspace,
//! and the projects themselves while nothing is selected.
//!
//! Cards are display only. They read the hierarchy snapshot, [`SessionStatus`],
//! [`attention`] and the keymap table, send no request, and change nothing about which
//! pane holds which session or what input is allowed. Key labels come from the keymap,
//! so a card names the key that actually runs the action it suggests.
//!
//! A card fits its pane. Columns give way before the state words and elapsed time, the
//! model label first; a list that runs out of rows ends with `+N more` so the next steps
//! below it stay; anything still too wide ends in `…` rather than at the pane edge.

use super::attention::{Attention, attention, attention_line};
use super::keymap::{Action, KeyBinding, keymap};
use super::picker::display_path;
use super::render::{
    BLUE, MAUVE, MUTED, PEACH, SKY, SUBTEXT, TEXT, clip_text, clipped_session_title, label_color,
    shell_glyph, sidebar_agent_label,
};
use super::state::{find_workspace, workspace_heading, workspace_label};
use super::status::SessionStatus;
use super::{Dashboard, InputMode, TreeRow};
use ovrcr_protocol::{ProjectSummary, SessionKind, SessionSummary, WorkspaceSummary};
use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::path::Path;

/// Cells a session title keeps on a card before it is clipped.
const TITLE_WIDTH: usize = 32;
/// Cells a title or branch keeps before the columns after it give way.
const NAME_FLOOR: usize = 12;
/// Indent, status glyph and space before a session title.
const ROW_LEAD: usize = 4;

/// The pane a card fills: its size, and whether the card must name the selection itself
/// (a split header says only `no session`; the single-pane header names it).
#[derive(Clone, Copy, Debug)]
pub(super) struct Room {
    pub width: usize,
    pub height: usize,
    pub titled: bool,
}

/// A card before it is fitted: a head, a list that may be cut short, and the next steps.
struct Card {
    head: Vec<Line<'static>>,
    list: Vec<Line<'static>>,
    /// What the list holds, for its `+N more` line.
    noun: &'static str,
    steps: Vec<Line<'static>>,
}

impl Dashboard {
    pub(super) fn draw_start_screen(&self, frame: &mut Frame<'_>, now_unix_ms: u64) {
        if self.mode != InputMode::Browse {
            return;
        }
        // Every drawn pane without a session gets a card: the one a selected project or
        // workspace emptied, and in a split also a pane whose session opened elsewhere.
        let titled = self.panes.len() > 1;
        for rect in self.pane_rects(frame.area()) {
            if !self
                .panes
                .get(rect.pane_index)
                .is_some_and(|pane| pane.session.is_none())
            {
                continue;
            }
            let room = Room {
                width: usize::from(rect.terminal.width),
                height: usize::from(rect.terminal.height),
                titled,
            };
            frame.render_widget(
                Paragraph::new(self.card(now_unix_ms, room)).style(Style::default().fg(TEXT())),
                rect.terminal,
            );
        }
    }

    /// The card for the current selection, fitted to `room`, one entry per pane row.
    pub(super) fn card(&self, now_unix_ms: u64, room: Room) -> Vec<Line<'static>> {
        let keys = Keys(
            keymap(self)
                .into_iter()
                .flat_map(|group| group.keys)
                .collect(),
        );
        let card = if self.hierarchy.projects.is_empty() {
            Card {
                head: keys.welcome(),
                list: Vec::new(),
                noun: "",
                steps: Vec::new(),
            }
        } else {
            match &self.selected_container {
                Some(TreeRow::Workspace { project, id }) => {
                    let Some(workspace) = find_workspace(self, project, id) else {
                        return Vec::new();
                    };
                    self.workspace_card(workspace, &keys, now_unix_ms, room)
                }
                Some(TreeRow::Project { name }) => {
                    let Some(project) = self.hierarchy.projects.iter().find(|p| p.name == *name)
                    else {
                        return Vec::new();
                    };
                    let (head, list) = self.project_lines(project, &keys, room.width, room.titled);
                    Card {
                        head,
                        list,
                        noun: "workspaces",
                        steps: keys.project_steps(),
                    }
                }
                // A row still being created, or a failed one: the sidebar row and the
                // footer strip already say what happened to it.
                Some(_) => return Vec::new(),
                None => {
                    let mut projects: Vec<_> = self.hierarchy.projects.iter().collect();
                    projects.sort_by(|left, right| left.name.cmp(&right.name));
                    let mut list = Vec::new();
                    for project in projects {
                        let (head, rows) = self.project_lines(project, &keys, room.width, true);
                        list.extend(head);
                        list.extend(rows);
                        list.push(Line::default());
                    }
                    list.pop();
                    Card {
                        head: Vec::new(),
                        list,
                        noun: "lines",
                        steps: keys.project_steps(),
                    }
                }
            }
        };
        fit(card, room)
    }

    /// A project's name (unless the header shows it), its next step while it has no agent
    /// yet (registration alone starts at most a shell), where it lives, and then, as the
    /// list, its workspaces with their session and attention counts in sidebar order.
    fn project_lines(
        &self,
        project: &ProjectSummary,
        keys: &Keys,
        width: usize,
        named: bool,
    ) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
        let mut head = Vec::new();
        if named {
            head.push(Line::styled(
                project.name.clone(),
                Style::default().fg(BLUE()).add_modifier(Modifier::BOLD),
            ));
        }
        let has_agent = project
            .workspaces
            .iter()
            .flat_map(|workspace| &workspace.sessions)
            .any(|session| matches!(session.kind, SessionKind::Agent { .. }));
        if !has_agent && let Some(key) = keys.enabled(Action::NewAgentSession) {
            head.push(Line::from(vec![
                Span::styled(
                    "Next: ",
                    Style::default().fg(MAUVE()).add_modifier(Modifier::BOLD),
                ),
                Span::raw("press "),
                Span::styled(key, Style::default().fg(SKY())),
                Span::raw(" to start your first agent"),
            ]));
        }
        let repository = project
            .workspaces
            .iter()
            .find(|workspace| workspace.root)
            .map(|workspace| workspace.path.as_path());
        for (label, path) in [
            ("Repository", repository),
            ("Workspace root", workspace_root(project)),
        ] {
            if let Some(path) = path {
                head.push(Line::from(vec![
                    Span::styled(format!("{label:<14}  "), Style::default().fg(MUTED())),
                    Span::raw(display_path(path)),
                ]));
            }
        }
        let mut workspaces: Vec<_> = project.workspaces.iter().collect();
        workspaces.sort_by(|left, right| (!left.root, &left.name).cmp(&(!right.root, &right.name)));
        if workspaces.is_empty() {
            return (head, Vec::new());
        }
        head.push(Line::default());
        head.push(heading("Workspaces"));
        let rows: Vec<_> = workspaces
            .into_iter()
            .map(|workspace| {
                let label = workspace_label(self, workspace);
                let label = if workspace.root {
                    format!("{label} · root")
                } else {
                    label
                };
                (
                    label,
                    session_counts(&workspace.sessions, width.saturating_sub(4 + NAME_FLOOR)),
                )
            })
            .collect();
        let label_width = rows
            .iter()
            .map(|(label, _)| cells(label))
            .max()
            .unwrap_or(0);
        let counts_width = rows
            .iter()
            .map(|(_, counts)| counts.iter().map(Span::width).sum::<usize>())
            .max()
            .unwrap_or(0);
        // The counts are the news; the branch gives way down to the floor first.
        let column = width
            .saturating_sub(2 + 2 + counts_width)
            .clamp(label_width.min(NAME_FLOOR), label_width);
        let list = rows
            .into_iter()
            .map(|(label, counts)| {
                let mut spans = vec![
                    Span::raw("  "),
                    Span::raw(pad(clip_text(&label, column), column)),
                    Span::raw("  "),
                ];
                spans.extend(counts);
                Line::from(spans)
            })
            .collect();
        (head, list)
    }

    /// The selected workspace: its path (the header above names `project / branch`), any
    /// root warning, its sessions in sidebar order as the list, then what to do here.
    fn workspace_card(
        &self,
        workspace: &WorkspaceSummary,
        keys: &Keys,
        now_unix_ms: u64,
        room: Room,
    ) -> Card {
        let mut head = Vec::new();
        if room.titled {
            head.push(Line::styled(
                workspace_heading(self, workspace),
                Style::default().add_modifier(Modifier::BOLD),
            ));
        }
        let mut path = vec![Span::styled(
            display_path(&workspace.path),
            Style::default().fg(MUTED()),
        )];
        if workspace.root {
            path.push(Span::styled(" · root", Style::default().fg(MUTED())));
        }
        head.push(Line::from(path));
        if let Some(warning) = &workspace.warning {
            head.push(Line::styled(warning.clone(), Style::default().fg(PEACH())));
        }
        head.push(Line::default());
        let mut sessions: Vec<_> = workspace.sessions.iter().collect();
        sessions.sort_by_key(|session| session.name != "local");
        let list = if sessions.is_empty() {
            head.push(Line::styled(
                "No sessions yet",
                Style::default().fg(MUTED()),
            ));
            Vec::new()
        } else {
            head.push(heading("Sessions"));
            self.session_lines(&sessions, now_unix_ms, room.width)
        };
        let mut hints = Vec::new();
        // The row below an expanded workspace is its first session, so the key that
        // selects the next row opens it. A collapsed workspace has to expand first.
        let rows = self.visible_rows();
        let below = self
            .selected_container
            .as_ref()
            .and_then(|selected| rows.iter().position(|row| row == selected))
            .and_then(|index| rows.get(index + 1));
        let collapsed = self
            .collapsed_workspaces
            .contains(&(workspace.project.clone(), workspace.id.clone()));
        if let Some(TreeRow::Session { id }) = below
            && let Some(session) = sessions.iter().find(|session| session.id == *id)
            && let Some(key) = keys.enabled(Action::NextSession)
        {
            hints.push((key, format!("open {}", self.session_display_name(session))));
        } else if collapsed
            && !sessions.is_empty()
            && let Some(key) = keys.enabled(Action::Focus)
        {
            hints.push((key, "expand to reach its sessions".into()));
        }
        for (action, text) in [
            (Action::NewAgentSession, "new agent session"),
            (Action::CreateWorkspace, "create branch"),
        ] {
            if let Some(key) = keys.enabled(action) {
                hints.push((key, text.into()));
            }
        }
        if let Some(more) = keys.group_key(Action::NewAgentSession) {
            hints.push((more, "more".into()));
        }
        Card {
            head,
            list,
            noun: "sessions",
            steps: hint_lines(hints),
        }
    }

    /// One aligned row per session: status glyph, title, `agent:model`, then the headers'
    /// state words (`SessionStatus::state`) and elapsed time. The state column always gets
    /// its room; the label shortens to the agent, then goes, before the title drops below
    /// [`NAME_FLOOR`].
    fn session_lines(
        &self,
        sessions: &[&SessionSummary],
        now_unix_ms: u64,
        width: usize,
    ) -> Vec<Line<'static>> {
        let rows: Vec<_> = sessions
            .iter()
            .map(|session| {
                let status = SessionStatus::of(session, now_unix_ms);
                let state = [status.state.as_str(), status.elapsed.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                let (agent, model) = sidebar_agent_label(session).unwrap_or_default();
                (*session, status, state, agent, model)
            })
            .collect();
        let room = width.saturating_sub(ROW_LEAD);
        let state_width = rows.iter().map(|row| cells(&row.2)).max().unwrap_or(0);
        let title_width = rows
            .iter()
            .map(|row| cells(&self.session_display_name(row.0)))
            .max()
            .unwrap_or(0)
            .min(TITLE_WIDTH);
        let title_floor = title_width.min(NAME_FLOOR);
        // 0: agent:model, 1: agent, 2: nothing.
        let label = |row: &(&SessionSummary, SessionStatus, String, String, String),
                     variant: usize| match variant {
            0 => (row.3.clone(), row.4.clone()),
            1 => (row.3.clone(), String::new()),
            _ => (String::new(), String::new()),
        };
        let label_width = |variant: usize| {
            rows.iter()
                .map(|row| {
                    let (agent, model) = label(row, variant);
                    cells(&agent) + cells(&model)
                })
                .max()
                .unwrap_or(0)
        };
        let variant = (0..2)
            .find(|&variant| {
                let width = label_width(variant);
                width == 0 || title_floor + 2 + width + 2 + state_width <= room
            })
            .unwrap_or(2);
        let label_column = label_width(variant);
        let label_room = if label_column > 0 {
            label_column + 2
        } else {
            0
        };
        let title_column = room
            .saturating_sub(label_room + 2 + state_width)
            .min(title_width);
        let state_column = room.saturating_sub(title_column + 2 + label_room);
        rows.iter()
            .map(|row| {
                let (session, status, state, ..) = row;
                let quiet = session.name == "local" && session.display_name() == "local";
                let (glyph, color) = if quiet {
                    shell_glyph(status)
                } else {
                    (status.glyph, status.color)
                };
                let mut title_style = Style::default().fg(TEXT());
                if status.exited {
                    title_style = title_style.add_modifier(Modifier::DIM);
                }
                let mut spans = vec![
                    Span::raw("  "),
                    Span::styled(glyph.to_string(), Style::default().fg(color)),
                    Span::raw(" "),
                    Span::styled(
                        pad(
                            clipped_session_title(self, session, title_column),
                            title_column,
                        ),
                        title_style,
                    ),
                    Span::raw("  "),
                ];
                if label_column > 0 {
                    let (agent, model) = label(row, variant);
                    let model_width = label_column - cells(&agent);
                    spans.push(Span::styled(
                        agent.clone(),
                        Style::default().fg(label_color(&agent)),
                    ));
                    spans.push(Span::styled(
                        pad(model, model_width),
                        Style::default().fg(MUTED()),
                    ));
                    spans.push(Span::raw("  "));
                }
                spans.push(Span::styled(
                    clip_text(state, state_column),
                    Style::default().fg(SUBTEXT()),
                ));
                Line::from(spans)
            })
            .collect()
    }
}

/// Lay a card into `room`: the whole card when it fits; otherwise the head and the next
/// steps stay and the list is cut short with `+N more`, so nothing is lost without a word.
/// Every line ends in `…` instead of at the pane edge.
fn fit(card: Card, room: Room) -> Vec<Line<'static>> {
    let Card {
        head,
        mut list,
        noun,
        steps,
    } = card;
    let gap = usize::from(!list.is_empty() || !head.is_empty()) * usize::from(!steps.is_empty());
    let fixed = head.len() + gap + steps.len();
    if fixed < room.height && fixed + list.len() > room.height {
        let keep = room.height.saturating_sub(fixed + 1);
        let hidden = list.len() - keep.min(list.len());
        list.truncate(keep);
        list.push(Line::styled(
            format!("  +{hidden} more {noun}"),
            Style::default().fg(MUTED()),
        ));
    }
    let mut lines = if fixed >= room.height {
        // Not even the head and the steps fit: the steps are what the card is for.
        let mut lines = steps;
        lines.extend(head);
        lines
    } else {
        let mut lines = head;
        lines.extend(list);
        if gap > 0 {
            lines.push(Line::default());
        }
        lines.extend(steps);
        lines
    };
    lines.truncate(room.height);
    lines
        .into_iter()
        .map(|line| fit_line(line, room.width))
        .collect()
}

/// `line` within `width` cells, ending in `…` when anything had to go.
fn fit_line(line: Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line;
    }
    let mut spans = Vec::new();
    let mut used = 0;
    for span in line.spans {
        let span_width = span.width();
        if used + span_width < width {
            used += span_width;
            spans.push(span);
            continue;
        }
        // The `…` goes in this span even when its text alone would fit, since later
        // spans are dropped.
        let clipped = clip_text(&format!("{}…", span.content), width - used);
        spans.push(Span::styled(clipped, span.style));
        break;
    }
    let mut fitted = Line::from(spans);
    fitted.style = line.style;
    fitted
}

/// The current keymap, looked up by action.
struct Keys(Vec<KeyBinding>);

impl Keys {
    /// The key that runs `action` right now, written the way the footer writes it:
    /// the first spelling of a `j/Down` label.
    fn enabled(&self, action: Action) -> Option<String> {
        self.0
            .iter()
            .find(|binding| binding.action == action && binding.enabled())
            .map(|binding| first_spelling(binding.key))
    }

    /// The key for `action` even while it waits for an earlier step.
    fn any(&self, action: Action) -> Option<String> {
        self.0
            .iter()
            .find(|binding| binding.action == action)
            .map(|binding| first_spelling(binding.key))
    }

    /// `Space w`: the leader followed by the menu group that holds `action`.
    fn group_key(&self, action: Action) -> Option<String> {
        let leader = self.enabled(Action::Leader)?;
        let group = self
            .0
            .iter()
            .find(|binding| binding.action == action)?
            .group?
            .group;
        Some(format!("{leader} {group}"))
    }

    /// The welcome's numbered steps name their keys even before step 2 can run.
    fn welcome(&self) -> Vec<Line<'static>> {
        let mut lines = vec![
            Line::styled(
                "Welcome to OVRCR",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Line::default(),
        ];
        let steps = [
            (Action::RegisterProject, "Register a repository"),
            (Action::CreateWorkspace, "Create a branch"),
        ];
        let steps = steps
            .into_iter()
            .filter_map(|(action, text)| Some((self.any(action)?, text)));
        for (number, (key, text)) in steps.enumerate() {
            lines.push(Line::from(vec![
                Span::styled(format!("{}  ", number + 1), Style::default().fg(MUTED())),
                Span::styled(key, Style::default().fg(SKY())),
                Span::raw(format!("  {text}")),
            ]));
        }
        lines.push(Line::default());
        let mut more = Vec::new();
        for (action, text) in [
            (Action::Leader, "actions"),
            (Action::Palette, "commands"),
            (Action::Help, "keys"),
        ] {
            let Some(key) = self.enabled(action) else {
                continue;
            };
            if !more.is_empty() {
                more.push(Span::styled(" · ", Style::default().fg(MUTED())));
            }
            more.push(Span::styled(key, Style::default().fg(SKY())));
            more.push(Span::styled(
                format!(" {text}"),
                Style::default().fg(MUTED()),
            ));
        }
        lines.push(Line::from(more));
        lines
    }

    fn project_steps(&self) -> Vec<Line<'static>> {
        let hints = [
            (Action::NewAgentSession, "new agent session"),
            (Action::CreateWorkspace, "create branch"),
            (Action::RegisterProject, "register another project"),
        ]
        .into_iter()
        .filter_map(|(action, text)| Some((self.enabled(action)?, text.to_owned())))
        .collect();
        hint_lines(hints)
    }
}

fn first_spelling(label: &str) -> String {
    label.split('/').next().unwrap_or(label).to_owned()
}

/// The hierarchy carries no project-level workspace root, so read it off the
/// worktrees: new ones are created directly inside it. Shown only while every
/// non-root workspace agrees; a project with none yet leaves it out.
fn workspace_root(project: &ProjectSummary) -> Option<&Path> {
    let mut parents = project
        .workspaces
        .iter()
        .filter(|workspace| !workspace.root)
        .map(|workspace| workspace.path.parent());
    let first = parents.next()??;
    parents.all(|parent| parent == Some(first)).then_some(first)
}

/// `2 sessions · ? 1 needs input · ✓ 1 ready`: the count, then the attention strip's own
/// words and order from [`attention`].
fn session_counts(sessions: &[SessionSummary], width: usize) -> Vec<Span<'static>> {
    let (input, ready) =
        sessions
            .iter()
            .filter_map(attention)
            .fold((0, 0), |(input, ready), kind| match kind {
                Attention::Input => (input + 1, ready),
                Attention::Ready => (input, ready + 1),
            });
    let mut spans = vec![Span::styled(
        match sessions.len() {
            0 => "no sessions".to_owned(),
            1 => "1 session".to_owned(),
            count => format!("{count} sessions"),
        },
        Style::default().fg(MUTED()),
    )];
    if input + ready > 0 {
        spans.push(Span::styled(" ·", Style::default().fg(MUTED())));
        // The strip's line leads with its own space.
        let used = spans.iter().map(Span::width).sum::<usize>();
        spans.extend(attention_line(input, ready, width.saturating_sub(used)).spans);
    }
    spans
}

/// Key hints with the keys in one aligned column.
fn hint_lines(hints: Vec<(String, String)>) -> Vec<Line<'static>> {
    let width = hints.iter().map(|(key, _)| cells(key)).max().unwrap_or(0);
    hints
        .into_iter()
        .map(|(key, text)| {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(pad(key, width), Style::default().fg(SKY())),
                Span::raw(format!("  {text}")),
            ])
        })
        .collect()
}

fn heading(text: &'static str) -> Line<'static> {
    Line::styled(
        text,
        Style::default().fg(SUBTEXT()).add_modifier(Modifier::BOLD),
    )
}

fn cells(text: &str) -> usize {
    Line::raw(text).width()
}

/// `text` padded with spaces to `width` terminal cells.
fn pad(text: String, width: usize) -> String {
    let fill = width.saturating_sub(cells(&text));
    text + &" ".repeat(fill)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{
        AgentActivity, AgentBinding, AgentProvider, HierarchySnapshot, ReadyObservation, SessionId,
        SessionPhase, SessionRunId, SessionScope, TerminalSize,
    };
    use ratatui::layout::Rect;

    const MINUTE: u64 = 60_000;
    /// A wide single pane: the header above names the selection.
    const WIDE: Room = Room {
        width: 120,
        height: 50,
        titled: false,
    };

    fn session(id: u64, workspace: &str, name: &str, kind: SessionKind) -> SessionSummary {
        SessionSummary {
            scope: SessionScope::Workspace,
            id: SessionId(id),
            archived: false,
            cwd: "/work".into(),
            run: SessionRunId(1),
            label: match &kind {
                SessionKind::Agent { name } => format!("{name}/opus-5-5"),
                SessionKind::Terminal => "zsh".into(),
            },
            kind,
            recovery: None,
            project: "demo".into(),
            workspace: workspace.into(),
            name: name.into(),
            pid: Some(100 + id as u32),
            started_unix_ms: Some(0),
            last_activity_unix_ms: None,
            phase: SessionPhase::Running,
            activity: AgentActivity::Unknown,
            agent: None,
            agent_epoch: 0,
            unread: None,
            context_usage: None,
            title: None,
            manual_title: None,
            assignment_control: None,
        }
    }

    fn agent(id: u64, workspace: &str, name: &str, activity: AgentActivity) -> SessionSummary {
        let mut session = session(
            id,
            workspace,
            name,
            SessionKind::Agent {
                name: "claude".into(),
            },
        );
        session.activity = activity;
        session
    }

    fn unread(mut session: SessionSummary) -> SessionSummary {
        session.unread = Some(ReadyObservation {
            binding: AgentBinding {
                provider: AgentProvider::Claude,
                invocation: "i".into(),
                conversation: "c".into(),
                generation: 1,
            },
            turn: Some("t".into()),
            activity_revision: 1,
        });
        session
    }

    fn workspace(
        id: &str,
        name: &str,
        root: bool,
        sessions: Vec<SessionSummary>,
    ) -> WorkspaceSummary {
        WorkspaceSummary {
            project: "demo".into(),
            id: id.into(),
            name: name.into(),
            path: if root {
                "/srv/demo".into()
            } else {
                format!("/srv/demo-worktrees/{id}").into()
            },
            root,
            warning: None,
            sessions,
        }
    }

    fn fixture(workspaces: Vec<WorkspaceSummary>) -> Dashboard {
        let mut dashboard = Dashboard::new(TerminalSize {
            rows: 40,
            cols: 120,
        });
        dashboard.install_area(Rect::new(0, 0, 160, 50));
        dashboard.install_hierarchy(HierarchySnapshot {
            usage: ovrcr_protocol::UsageSnapshot::default(),
            projects: vec![ProjectSummary {
                name: "demo".into(),
                workspaces,
            }],
        });
        dashboard
    }

    fn select(dashboard: &mut Dashboard, row: TreeRow) {
        dashboard.select_container(row);
    }

    fn feature() -> TreeRow {
        TreeRow::Workspace {
            project: "demo".into(),
            id: "feature".into(),
        }
    }

    fn text(dashboard: &Dashboard, now: u64, room: Room) -> Vec<String> {
        dashboard
            .card(now, room)
            .iter()
            .map(|line| line.to_string().trim_end().to_owned())
            .collect()
    }

    fn the_workspace() -> Vec<WorkspaceSummary> {
        vec![
            workspace(
                "root",
                "main",
                true,
                vec![session(1, "root", "local", SessionKind::Terminal)],
            ),
            workspace(
                "feature",
                "feature/spinner",
                false,
                vec![
                    agent(2, "feature", "verify spinner", AgentActivity::Busy),
                    session(3, "feature", "local", SessionKind::Terminal),
                    unread(agent(4, "feature", "review", AgentActivity::ResponseReady)),
                    agent(5, "feature", "ask", AgentActivity::WaitingInput),
                ],
            ),
        ]
    }

    #[test]
    fn welcome_numbers_the_first_steps_with_keys_from_the_keymap() {
        let dashboard = Dashboard::new(TerminalSize {
            rows: 40,
            cols: 120,
        });
        assert_eq!(
            text(&dashboard, 0, WIDE),
            [
                "Welcome to OVRCR",
                "",
                "1  a  Register a repository",
                "2  w  Create a branch",
                "",
                "Space actions · : commands · ? keys",
            ]
        );
    }

    #[test]
    fn workspace_card_lists_sessions_in_sidebar_order_and_what_to_do_next() {
        let mut dashboard = fixture(the_workspace());
        select(&mut dashboard, feature());
        let lines = text(&dashboard, 2 * MINUTE, WIDE);
        // The pane header names `demo / feature/spinner`; the card starts at the path.
        assert_eq!(lines[..3], ["/srv/demo-worktrees/feature", "", "Sessions"]);
        // Local first, as the sidebar orders them; titles and labels in columns, then
        // the headers' own state words. A plain shell has none, only its age.
        assert_eq!(lines[3], format!("  $ local{}2m", " ".repeat(28)));
        assert_eq!(
            lines[4],
            "  ⠋ verify spinner  claude:opus-5-5  Working · 2m"
        );
        assert_eq!(
            lines[5],
            "  ✓ review          claude:opus-5-5  Ready · unread reply · 2m"
        );
        assert_eq!(
            lines[6],
            "  ? ask             claude:opus-5-5  Waiting for input · 2m"
        );
        assert_eq!(
            lines[7..],
            [
                "",
                "  j        open local",
                "  n        new agent session",
                "  w        create branch",
                "  Space w  more",
            ]
        );

        // In a split the pane header says only `no session`, so the card names it.
        let titled = text(
            &dashboard,
            2 * MINUTE,
            Room {
                titled: true,
                ..WIDE
            },
        );
        assert_eq!(
            titled[..2],
            ["demo / feature/spinner", "/srv/demo-worktrees/feature"]
        );
    }

    #[test]
    fn the_open_hint_names_what_the_next_row_key_selects() {
        let mut dashboard = fixture(the_workspace());
        select(&mut dashboard, feature());
        assert!(text(&dashboard, 0, WIDE).contains(&"  j        open local".to_owned()));
        // The hint is a promise about `j`: keep it honest through the real key.
        dashboard.key(crossterm::event::KeyCode::Char('j'));
        assert_eq!(dashboard.focused_session(), Some(SessionId(3)));

        // A collapsed workspace has no session row below it; Enter expands it.
        select(&mut dashboard, feature());
        assert!(dashboard.toggle_row_collapse(&feature()));
        let lines = text(&dashboard, 0, WIDE);
        assert!(
            lines.contains(&"  Enter    expand to reach its sessions".to_owned()),
            "{lines:#?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("open")),
            "{lines:#?}"
        );
    }

    #[test]
    fn empty_and_warned_workspaces_only_offer_keys_that_work() {
        let mut workspaces = the_workspace();
        workspaces[1].sessions.clear();
        workspaces[0].warning = Some("root is on feature/x; check out main".into());
        let mut dashboard = fixture(workspaces);
        select(&mut dashboard, feature());
        let lines = text(&dashboard, 0, WIDE);
        assert!(lines.contains(&"No sessions yet".to_owned()), "{lines:#?}");
        assert!(
            !lines.iter().any(|line| line.contains("open")),
            "{lines:#?}"
        );
        assert!(lines.contains(&"  n        new agent session".to_owned()));

        select(
            &mut dashboard,
            TreeRow::Workspace {
                project: "demo".into(),
                id: "root".into(),
            },
        );
        let lines = text(&dashboard, 0, WIDE);
        assert_eq!(lines[0], "/srv/demo · root");
        assert_eq!(lines[1], "root is on feature/x; check out main");
        // `n` still opens the chooser so another registered branch can be selected.
        assert!(
            lines.contains(&"  n        new agent session".to_owned()),
            "{lines:#?}"
        );
        assert!(
            lines.contains(&"  w        create branch".to_owned()),
            "{lines:#?}"
        );
    }

    #[test]
    fn project_card_counts_sessions_and_attention_per_workspace() {
        let mut dashboard = fixture(the_workspace());
        select(
            &mut dashboard,
            TreeRow::Project {
                name: "demo".into(),
            },
        );
        // Attention in the sidebar strip's words and order: input first.
        assert_eq!(
            text(&dashboard, 0, WIDE),
            [
                "Repository      /srv/demo",
                "Workspace root  /srv/demo-worktrees",
                "",
                "Workspaces",
                "  main · root      1 session",
                "  feature/spinner  4 sessions · ? 1 needs input · ✓ 1 ready",
                "",
                "  n  new agent session",
                "  w  create branch",
                "  a  register another project",
            ]
        );
        let titled = text(
            &dashboard,
            0,
            Room {
                titled: true,
                ..WIDE
            },
        );
        assert_eq!(titled[0], "demo");
    }

    #[test]
    fn project_card_leads_with_the_first_agent_until_one_exists() {
        // Registration leaves the root workspace with at most a local shell.
        let mut dashboard = fixture(vec![workspace(
            "root",
            "main",
            true,
            vec![session(1, "root", "local", SessionKind::Terminal)],
        )]);
        let project = TreeRow::Project {
            name: "demo".into(),
        };
        select(&mut dashboard, project.clone());
        let lines = text(&dashboard, 0, WIDE);
        assert_eq!(lines[0], "Next: press n to start your first agent");
        // No worktree yet, so there is nothing to read the workspace root from.
        assert!(
            !lines.iter().any(|line| line.starts_with("Workspace root")),
            "{lines:#?}"
        );

        let mut dashboard = fixture(the_workspace());
        select(&mut dashboard, project);
        assert!(
            !text(&dashboard, 0, WIDE)
                .iter()
                .any(|line| line.starts_with("Next:"))
        );
    }

    #[test]
    fn workspace_root_needs_every_worktree_to_agree() {
        let mut workspaces = the_workspace();
        workspaces.push(workspace("other", "other", false, Vec::new()));
        workspaces[2].path = "/elsewhere/other".into();
        let project = ProjectSummary {
            name: "demo".into(),
            workspaces,
        };
        assert_eq!(workspace_root(&project), None);
    }

    #[test]
    fn nothing_selected_shows_every_project_and_how_to_start() {
        let dashboard = fixture(vec![workspace(
            "root",
            "main",
            true,
            vec![session(1, "root", "local", SessionKind::Terminal)],
        )]);
        assert_eq!(dashboard.focused_session(), None);
        assert_eq!(dashboard.selected_container, None);
        let lines = text(&dashboard, 0, WIDE);
        // No header names a project here, so each one names itself.
        assert_eq!(lines[0], "demo");
        assert_eq!(lines[1], "Next: press n to start your first agent");
        assert_eq!(
            lines[lines.len() - 3..],
            [
                "  n  new agent session",
                "  w  create branch",
                "  a  register another project"
            ]
        );
    }

    /// An 80-column window leaves a 40-column pane.
    #[test]
    fn a_narrow_pane_keeps_state_and_elapsed_and_shortens_the_rest() {
        let narrow = Room { width: 40, ..WIDE };
        let mut dashboard = fixture(vec![
            workspace(
                "root",
                "main",
                true,
                vec![session(1, "root", "local", SessionKind::Terminal)],
            ),
            workspace(
                "feature",
                "feature/a-very-long-branch-name-for-this",
                false,
                vec![
                    session(3, "feature", "local", SessionKind::Terminal),
                    agent(
                        2,
                        "feature",
                        "implement lifecycle hooks",
                        AgentActivity::Busy,
                    ),
                ],
            ),
        ]);
        select(&mut dashboard, feature());
        let lines = text(&dashboard, 2 * MINUTE, narrow);
        // The model goes first, then the title clips with `…`; state and age stay whole.
        let row = lines
            .iter()
            .find(|line| line.contains("implement"))
            .unwrap();
        assert_eq!(row, "  ⠋ implement lif…  claude  Working · 2m");
        for line in &lines {
            assert!(cells(line) <= 40, "{line:?} in {lines:#?}");
        }

        select(
            &mut dashboard,
            TreeRow::Project {
                name: "demo".into(),
            },
        );
        let lines = text(&dashboard, 0, narrow);
        let row = lines.iter().find(|line| line.contains("feature/")).unwrap();
        assert!(row.ends_with("…  2 sessions"), "{row:?}");
        for line in &lines {
            assert!(cells(line) <= 40, "{line:?} in {lines:#?}");
        }

        // Even the welcome ends in `…` rather than at the pane edge.
        let welcome = Dashboard::new(TerminalSize { rows: 24, cols: 30 });
        let lines = text(&welcome, 0, Room { width: 30, ..WIDE });
        assert_eq!(lines[3], "2  w  Create a branch");
    }

    #[test]
    fn a_narrow_unread_row_keeps_its_whole_state_and_age() {
        let mut dashboard = fixture(vec![workspace(
            "feature",
            "feature/unread",
            false,
            vec![unread(agent(
                2,
                "feature",
                "implement lifecycle hooks",
                AgentActivity::ResponseReady,
            ))],
        )]);
        select(&mut dashboard, feature());
        let lines = text(&dashboard, 2 * MINUTE, Room { width: 40, ..WIDE });
        let row = lines.iter().find(|line| line.starts_with("  ✓ ")).unwrap();
        assert!(row.ends_with("Ready · unread reply · 2m"), "{row:?}");
        assert!(cells(row) <= 40, "{row:?}");
    }

    #[test]
    fn a_narrow_project_row_keeps_both_attention_counts() {
        let mut dashboard = fixture(the_workspace());
        select(
            &mut dashboard,
            TreeRow::Project {
                name: "demo".into(),
            },
        );
        let lines = text(&dashboard, 0, Room { width: 40, ..WIDE });
        let row = lines.iter().find(|line| line.contains("feature/")).unwrap();
        assert!(row.ends_with("4 sessions · ? 1 · ✓ 1"), "{row:?}");
        assert!(cells(row) <= 40, "{row:?}");
    }

    #[test]
    fn a_short_pane_keeps_the_next_steps_and_counts_what_it_hides() {
        let sessions = (0..50)
            .map(|index| {
                agent(
                    10 + index,
                    "feature",
                    &format!("agent {index:02}"),
                    AgentActivity::Idle,
                )
            })
            .collect();
        let mut dashboard = fixture(vec![workspace("feature", "feature/many", false, sessions)]);
        select(&mut dashboard, feature());
        let room = Room { height: 20, ..WIDE };
        let lines = text(&dashboard, 0, room);
        assert_eq!(lines.len(), 20);
        // Path, gap, heading; eleven rows; the count; gap and the four next steps.
        assert_eq!(lines[14], "  +39 more sessions");
        assert_eq!(lines[16], "  j        open agent 00");
        assert_eq!(lines[19], "  Space w  more");

        // Too short for even the head and the steps: the steps come first.
        let lines = text(&dashboard, 0, Room { height: 2, ..room });
        assert_eq!(
            lines,
            ["  j        open agent 00", "  n        new agent session"]
        );
    }

    #[test]
    fn cards_are_drawn_only_where_a_pane_has_no_session() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut dashboard = fixture(the_workspace());
        let draw = |dashboard: &Dashboard| {
            let mut terminal = Terminal::new(TestBackend::new(160, 50)).unwrap();
            terminal
                .draw(|frame| super::super::render::draw_dashboard_at(frame, dashboard, 0))
                .unwrap();
            format!("{:?}", terminal.backend().buffer())
        };
        dashboard.select_session(SessionId(2));
        assert!(!draw(&dashboard).contains("create branch"));
        select(&mut dashboard, feature());
        assert!(draw(&dashboard).contains("create branch"));
    }
}

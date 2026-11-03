//! What a pane with no session shows instead of a blank screen: numbered first
//! steps before any project exists, a card for the selected project or workspace,
//! and the projects themselves while nothing is selected.
//!
//! Cards are display only. They read the hierarchy snapshot, [`SessionStatus`] and
//! the keymap table, send no request, and change nothing about which pane holds
//! which session or what input is allowed. Key labels come from the keymap, so a
//! card names the key that actually runs the action it suggests.

use super::keymap::{Action, KeyBinding, keymap};
use super::picker::display_path;
use super::render::{
    BLUE, MAUVE, MUTED, PEACH, SKY, SUBTEXT, TEXT, clip_text, label_color, sidebar_agent_label,
};
use super::state::{find_workspace, workspace_heading, workspace_label};
use super::status::{SessionStatus, sidebar_activity_glyph};
use super::{Dashboard, InputMode, TreeRow};
use ovrcr_protocol::{
    AgentActivity, ProjectSummary, SessionKind, SessionPhase, SessionSummary, WorkspaceSummary,
};
use ratatui::Frame;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::path::Path;

/// Cells a session title keeps on a card before it is clipped.
const TITLE_WIDTH: usize = 32;

impl Dashboard {
    pub(super) fn draw_start_screen(&self, frame: &mut Frame<'_>, now_unix_ms: u64) {
        if self.mode != InputMode::Browse {
            return;
        }
        // Selecting a project or workspace empties a pane; with nothing selected,
        // only an empty focused pane has nothing better to show.
        let Some(rect) = self.pane_rects(frame.area()).into_iter().find(|rect| {
            self.panes
                .get(rect.pane_index)
                .is_some_and(|pane| pane.session.is_none())
                && (self.selected_container.is_some() || rect.pane_index == self.focused_pane)
        }) else {
            return;
        };
        frame.render_widget(
            Paragraph::new(self.card(now_unix_ms)).style(Style::default().fg(TEXT())),
            rect.terminal,
        );
    }

    /// The card for the current selection, one entry per pane row.
    pub(super) fn card(&self, now_unix_ms: u64) -> Vec<Line<'static>> {
        let keys = Keys(
            keymap(self)
                .into_iter()
                .flat_map(|group| group.keys)
                .collect(),
        );
        if self.hierarchy.projects.is_empty() {
            return keys.welcome();
        }
        match &self.selected_container {
            Some(TreeRow::Workspace { project, id }) => find_workspace(self, project, id)
                .map(|workspace| self.workspace_card(workspace, &keys, now_unix_ms))
                .unwrap_or_default(),
            Some(TreeRow::Project { name }) => self
                .hierarchy
                .projects
                .iter()
                .find(|project| project.name == *name)
                .map(|project| {
                    let mut lines = self.project_lines(project, &keys);
                    lines.push(Line::default());
                    lines.extend(keys.project_hints());
                    lines
                })
                .unwrap_or_default(),
            // A row still being created, or a failed one: the sidebar row and the
            // footer strip already say what happened to it.
            Some(_) => Vec::new(),
            None => {
                let mut projects: Vec<_> = self.hierarchy.projects.iter().collect();
                projects.sort_by(|left, right| left.name.cmp(&right.name));
                let mut lines = Vec::new();
                for project in projects {
                    lines.extend(self.project_lines(project, &keys));
                    lines.push(Line::default());
                }
                lines.extend(keys.project_hints());
                lines
            }
        }
    }

    /// A project's name, where it lives, and its workspaces with their session and
    /// attention counts, in sidebar order. Leads with the next step while the
    /// project has no agent yet: registration alone starts at most a shell.
    fn project_lines(&self, project: &ProjectSummary, keys: &Keys) -> Vec<Line<'static>> {
        let mut lines = vec![Line::styled(
            project.name.clone(),
            Style::default().fg(BLUE()).add_modifier(Modifier::BOLD),
        )];
        let has_agent = project
            .workspaces
            .iter()
            .flat_map(|workspace| &workspace.sessions)
            .any(|session| matches!(session.kind, SessionKind::Agent { .. }));
        if !has_agent && let Some(key) = keys.enabled(Action::CreateWorkspace) {
            lines.push(Line::from(vec![
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
                lines.push(Line::from(vec![
                    Span::styled(format!("{label:<14}  "), Style::default().fg(MUTED())),
                    Span::raw(display_path(path)),
                ]));
            }
        }
        let mut workspaces: Vec<_> = project.workspaces.iter().collect();
        workspaces.sort_by(|left, right| (!left.root, &left.name).cmp(&(!right.root, &right.name)));
        let labels: Vec<_> = workspaces
            .iter()
            .map(|workspace| {
                let label = workspace_label(self, workspace);
                if workspace.root {
                    format!("{label} · root")
                } else {
                    label
                }
            })
            .collect();
        let width = labels.iter().map(|label| cells(label)).max().unwrap_or(0);
        if !workspaces.is_empty() {
            lines.push(Line::default());
            lines.push(heading("Workspaces"));
        }
        for (workspace, label) in workspaces.into_iter().zip(labels) {
            let mut spans = vec![
                Span::raw("  "),
                Span::raw(pad(label, width)),
                Span::raw("  "),
            ];
            spans.extend(session_counts(&workspace.sessions));
            lines.push(Line::from(spans));
        }
        lines
    }

    /// The selected workspace: `project / branch`, its path, its sessions in sidebar
    /// order, then what to do here.
    fn workspace_card(
        &self,
        workspace: &WorkspaceSummary,
        keys: &Keys,
        now_unix_ms: u64,
    ) -> Vec<Line<'static>> {
        let mut title = vec![Span::styled(
            workspace_heading(self, workspace),
            Style::default().add_modifier(Modifier::BOLD),
        )];
        if workspace.root {
            title.push(Span::styled(" · root", Style::default().fg(MUTED())));
        }
        let mut lines = vec![
            Line::from(title),
            Line::styled(display_path(&workspace.path), Style::default().fg(MUTED())),
        ];
        if let Some(warning) = &workspace.warning {
            lines.push(Line::styled(warning.clone(), Style::default().fg(PEACH())));
        }
        lines.push(Line::default());
        let mut sessions: Vec<_> = workspace.sessions.iter().collect();
        sessions.sort_by_key(|session| session.name != "local");
        if sessions.is_empty() {
            lines.push(Line::styled(
                "No sessions yet",
                Style::default().fg(MUTED()),
            ));
        } else {
            lines.push(heading("Sessions"));
            lines.extend(self.session_lines(&sessions, now_unix_ms));
        }
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
            (Action::CreateTerminal, "new agent or terminal here"),
            (Action::CreateWorkspace, "new workspace"),
        ] {
            if let Some(key) = keys.enabled(action) {
                hints.push((key, text.into()));
            }
        }
        if let Some(more) = keys.group_key(Action::CreateTerminal) {
            hints.push((more, "more".into()));
        }
        lines.push(Line::default());
        lines.extend(hint_lines(hints));
        lines
    }

    /// One aligned row per session: status glyph, title, `agent:model`, the state
    /// words and elapsed time, all read from [`SessionStatus`] as the headers do.
    fn session_lines(&self, sessions: &[&SessionSummary], now_unix_ms: u64) -> Vec<Line<'static>> {
        let rows: Vec<_> = sessions
            .iter()
            .map(|session| {
                let title = clip_text(&self.session_display_name(session), TITLE_WIDTH);
                let (agent, model) = sidebar_agent_label(session).unwrap_or_default();
                (
                    *session,
                    SessionStatus::of(session, now_unix_ms),
                    title,
                    agent,
                    model,
                )
            })
            .collect();
        let title_width = rows.iter().map(|row| cells(&row.2)).max().unwrap_or(0);
        let label_width = rows
            .iter()
            .map(|row| cells(&row.3) + cells(&row.4))
            .max()
            .unwrap_or(0);
        rows.into_iter()
            .map(|(session, status, title, agent, model)| {
                // The sidebar's rule: a local shell shows `$` until a hook reports
                // activity in it, and then it has no state worth naming.
                let quiet = session.name == "local"
                    && session.display_name() == "local"
                    && matches!(status.glyph, '-' | ' ');
                let (glyph, color) = if quiet {
                    ('$', SUBTEXT())
                } else {
                    (status.glyph, status.color)
                };
                let mut state = Vec::new();
                if !quiet {
                    // An exited process says `closed`, as the header does, unless
                    // recovery still has something to explain.
                    state.push(if status.exited && !status.recovery_diagnostic {
                        status.pid.clone()
                    } else {
                        status
                            .unread
                            .clone()
                            .unwrap_or_else(|| status.activity.trim().to_owned())
                    });
                }
                if status.paused {
                    state.push("paused".to_owned());
                }
                if !status.elapsed.is_empty() {
                    state.push(status.elapsed.clone());
                }
                let mut title_style = Style::default().fg(TEXT());
                if status.exited {
                    title_style = title_style.add_modifier(Modifier::DIM);
                }
                let mut spans = vec![
                    Span::raw("  "),
                    Span::styled(glyph.to_string(), Style::default().fg(color)),
                    Span::raw(" "),
                    Span::styled(pad(title, title_width), title_style),
                    Span::raw("  "),
                ];
                if label_width > 0 {
                    let model_width = label_width - cells(&agent);
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
                    state.join(" · "),
                    Style::default().fg(SUBTEXT()),
                ));
                Line::from(spans)
            })
            .collect()
    }
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
            (Action::CreateWorkspace, "Start an agent in a new workspace"),
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

    fn project_hints(&self) -> Vec<Line<'static>> {
        let hints = [
            (Action::CreateWorkspace, "new workspace"),
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

/// `2 sessions · ✓ 1 ready · ? 1 needs input`, with the sidebar's attention glyphs.
fn session_counts(sessions: &[SessionSummary]) -> Vec<Span<'static>> {
    let needs_input = |session: &SessionSummary| {
        session.phase == SessionPhase::Running
            && super::ready::activity(session) == AgentActivity::WaitingInput
    };
    let input = sessions
        .iter()
        .filter(|session| needs_input(session))
        .count();
    let ready = sessions
        .iter()
        .filter(|session| !needs_input(session) && session.unread.is_some())
        .count();
    let mut spans = vec![Span::styled(
        match sessions.len() {
            0 => "no sessions".to_owned(),
            1 => "1 session".to_owned(),
            count => format!("{count} sessions"),
        },
        Style::default().fg(MUTED()),
    )];
    for (count, activity, unread, words) in [
        (ready, AgentActivity::ResponseReady, true, "ready"),
        (
            input,
            AgentActivity::WaitingInput,
            false,
            if input == 1 {
                "needs input"
            } else {
                "need input"
            },
        ),
    ] {
        if count > 0 {
            let (glyph, color) = sidebar_activity_glyph(activity, unread, 0);
            spans.push(Span::styled(" · ", Style::default().fg(MUTED())));
            spans.push(Span::styled(
                format!("{glyph} {count} {words}"),
                Style::default().fg(color),
            ));
        }
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
        AgentBinding, AgentProvider, HierarchySnapshot, ReadyObservation, SessionId, SessionRunId,
        SessionScope, TerminalSize,
    };
    use ratatui::layout::Rect;

    const MINUTE: u64 = 60_000;

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

    fn text(dashboard: &Dashboard, now: u64) -> Vec<String> {
        dashboard
            .card(now)
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
            text(&dashboard, 0),
            [
                "Welcome to OVRCR",
                "",
                "1  a  Register a repository",
                "2  w  Start an agent in a new workspace",
                "",
                "Space actions · : commands · ? keys",
            ]
        );
    }

    #[test]
    fn workspace_card_lists_sessions_in_sidebar_order_and_what_to_do_next() {
        let mut dashboard = fixture(the_workspace());
        select(
            &mut dashboard,
            TreeRow::Workspace {
                project: "demo".into(),
                id: "feature".into(),
            },
        );
        let lines = text(&dashboard, 2 * MINUTE);
        assert_eq!(lines[0], "demo / feature/spinner");
        assert_eq!(lines[1], "/srv/demo-worktrees/feature");
        assert_eq!(lines[3], "Sessions");
        // Local first, as the sidebar orders them; titles and labels in columns. A
        // quiet shell has no state worth naming, only its age.
        assert_eq!(lines[4], format!("  $ local{}2m", " ".repeat(28)));
        assert_eq!(lines[5], "  ⠋ verify spinner  claude:opus-5-5  busy · 2m");
        assert_eq!(
            lines[6],
            "  ✓ review          claude:opus-5-5  Unread Unavailable · 2m"
        );
        assert_eq!(
            lines[7],
            "  ? ask             claude:opus-5-5  waiting input · 2m"
        );
        assert_eq!(
            lines[9..],
            [
                "  j        open local",
                "  n        new agent or terminal here",
                "  w        new workspace",
                "  Space w  more",
            ]
        );
    }

    #[test]
    fn the_open_hint_names_what_the_next_row_key_selects() {
        let mut dashboard = fixture(the_workspace());
        let row = TreeRow::Workspace {
            project: "demo".into(),
            id: "feature".into(),
        };
        select(&mut dashboard, row.clone());
        assert!(text(&dashboard, 0).contains(&"  j        open local".to_owned()));
        // The hint is a promise about `j`: keep it honest through the real key.
        dashboard.key(crossterm::event::KeyCode::Char('j'));
        assert_eq!(dashboard.focused_session(), Some(SessionId(3)));

        // A collapsed workspace has no session row below it; Enter expands it.
        select(&mut dashboard, row.clone());
        assert!(dashboard.toggle_row_collapse(&row));
        let lines = text(&dashboard, 0);
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
        select(
            &mut dashboard,
            TreeRow::Workspace {
                project: "demo".into(),
                id: "feature".into(),
            },
        );
        let lines = text(&dashboard, 0);
        assert!(lines.contains(&"No sessions yet".to_owned()), "{lines:#?}");
        assert!(
            !lines.iter().any(|line| line.contains("open")),
            "{lines:#?}"
        );
        assert!(lines.contains(&"  n        new agent or terminal here".to_owned()));

        select(
            &mut dashboard,
            TreeRow::Workspace {
                project: "demo".into(),
                id: "root".into(),
            },
        );
        let lines = text(&dashboard, 0);
        assert_eq!(lines[0], "demo / main · root");
        assert_eq!(lines[2], "root is on feature/x; check out main");
        // `n` is disabled on a drifted root, so the card does not offer it.
        assert!(
            !lines.iter().any(|line| line.contains("new agent")),
            "{lines:#?}"
        );
        assert!(
            lines.contains(&"  w        new workspace".to_owned()),
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
        assert_eq!(
            text(&dashboard, 0),
            [
                "demo",
                "Repository      /srv/demo",
                "Workspace root  /srv/demo-worktrees",
                "",
                "Workspaces",
                "  main · root      1 session",
                "  feature/spinner  4 sessions · ✓ 1 ready · ? 1 needs input",
                "",
                "  w  new workspace",
                "  a  register another project",
            ]
        );
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
        select(
            &mut dashboard,
            TreeRow::Project {
                name: "demo".into(),
            },
        );
        let lines = text(&dashboard, 0);
        assert_eq!(lines[1], "Next: press w to start your first agent");
        // No worktree yet, so there is nothing to read the workspace root from.
        assert!(
            !lines.iter().any(|line| line.starts_with("Workspace root")),
            "{lines:#?}"
        );

        let mut dashboard = fixture(the_workspace());
        select(
            &mut dashboard,
            TreeRow::Project {
                name: "demo".into(),
            },
        );
        assert!(
            !text(&dashboard, 0)
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
        let lines = text(&dashboard, 0);
        assert_eq!(lines[0], "demo");
        assert_eq!(lines[1], "Next: press w to start your first agent");
        assert_eq!(
            lines[lines.len() - 2..],
            ["  w  new workspace", "  a  register another project"]
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
        assert!(!draw(&dashboard).contains("new workspace"));
        select(
            &mut dashboard,
            TreeRow::Workspace {
                project: "demo".into(),
                id: "feature".into(),
            },
        );
        assert!(draw(&dashboard).contains("new workspace"));
    }
}

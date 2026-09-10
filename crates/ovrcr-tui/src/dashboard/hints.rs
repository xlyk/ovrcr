use super::state::find_session;
use super::{Dashboard, InputMode, TreeRow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ovrcr_protocol::SessionPhase;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HintAction {
    Key(KeyCode),
    Tasks,
    Browse,
    Group(char),
    RemoveWorkspace,
    RemoveProject,
}

pub(super) struct KeyHint {
    pub key: &'static str,
    pub name: &'static str,
    pub description: String,
    pub enabled: bool,
    pub action: HintAction,
}

pub(super) struct HintGroup {
    pub title: String,
    pub hints: Vec<KeyHint>,
}

fn hint(key: &'static str, name: &'static str, description: String, code: KeyCode) -> KeyHint {
    KeyHint {
        key,
        name,
        description,
        enabled: true,
        action: HintAction::Key(code),
    }
}

impl KeyHint {
    fn unless(mut self, reason: Option<&str>) -> Self {
        if let Some(reason) = reason {
            self.enabled = false;
            self.description = format!("{}: {reason}", self.name);
        }
        self
    }

    pub(super) fn matches(&self, key: KeyEvent) -> bool {
        if key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return false;
        }
        if self.action == HintAction::Tasks {
            return key.code == KeyCode::Char('t')
                && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::CONTROL);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return self.key == "Ctrl-g" && key.code == KeyCode::Char('g');
        }
        if self.key.len() == 1 {
            return key.code == KeyCode::Char(self.key.chars().next().unwrap());
        }
        let code = match key.code {
            KeyCode::Left => KeyCode::Char('h'),
            KeyCode::Down => KeyCode::Char('j'),
            KeyCode::Up => KeyCode::Char('k'),
            KeyCode::Right => KeyCode::Char('l'),
            KeyCode::BackTab => KeyCode::Tab,
            code => code,
        };
        self.action == HintAction::Key(code)
    }
}

impl Dashboard {
    // A container selection can retain a wire-focused pane, but actions still
    // target the selected container until the user selects a session again.
    pub(super) fn creation_context(&self) -> (String, String) {
        match &self.selected_container {
            Some(TreeRow::Project { name }) => (name.clone(), String::new()),
            Some(TreeRow::Workspace { project, name }) => (project.clone(), name.clone()),
            _ => self
                .focused_session()
                .and_then(|id| find_session(self, id))
                .map(|s| (s.project.clone(), s.workspace.clone()))
                .unwrap_or_default(),
        }
    }
}

pub(super) fn key_hints(dashboard: &Dashboard) -> Vec<HintGroup> {
    use KeyCode::{Char, Enter, Esc, PageDown, PageUp, Tab};
    if dashboard.mode == InputMode::Terminal {
        return Vec::new();
    }
    let help = || {
        vec![
            hint(
                "Space",
                "Leader",
                "Show keys; the next key runs an action".into(),
                Char(' '),
            ),
            hint(
                "?",
                "Help",
                "Browse available keys; Enter or click runs an action".into(),
                Char('?'),
            ),
        ]
    };
    if matches!(dashboard.mode, InputMode::Copy | InputMode::History) {
        let history = dashboard.mode == InputMode::History;
        let capture = if history {
            "frozen history"
        } else {
            "captured screen"
        };
        let captured_session = if history {
            dashboard.history.as_ref().map(|view| view.opened.session)
        } else {
            dashboard.copy.as_ref().map(|copy| copy.session)
        };
        let target = captured_session
            .and_then(|id| find_session(dashboard, id))
            .map_or_else(
                || capture.to_owned(),
                |session| {
                    format!(
                        "{capture} of {} (#{}) in {} / {}",
                        session.name, session.id.0, session.project, session.workspace
                    )
                },
            );
        let copying = dashboard
            .history
            .as_ref()
            .is_some_and(|v| v.copy_job.is_some() || v.copy_completion.is_some());
        let reason = copying.then_some("copy in progress");
        let mut motion = vec![
            hint(
                "h/Left",
                "Move left",
                format!("Move left in the {target}"),
                Char('h'),
            )
            .unless(reason),
            hint(
                "j/Down",
                "Move down",
                format!("Move down in the {target}"),
                Char('j'),
            )
            .unless(reason),
            hint(
                "k/Up",
                "Move up",
                format!("Move up in the {target}"),
                Char('k'),
            )
            .unless(reason),
            hint(
                "l/Right",
                "Move right",
                format!("Move right in the {target}"),
                Char('l'),
            )
            .unless(reason),
        ];
        let bounds_reason = reason.or_else(|| {
            (history
                && dashboard
                    .history
                    .as_ref()
                    .is_none_or(|v| v.anchor.is_none()))
            .then_some("set an anchor with v first")
        });
        for (key, name, code) in [
            ("0", "Row start", '0'),
            ("$", "Row end", '$'),
            ("g", "First row", 'g'),
            ("G", "Last row", 'G'),
        ] {
            motion.push(
                hint(
                    key,
                    name,
                    format!("Move to {} in the {target}", name.to_lowercase()),
                    Char(code),
                )
                .unless(bounds_reason),
            );
        }
        if history {
            motion.extend([
                hint(
                    "PageUp",
                    "Previous page",
                    "Scroll one frozen history viewport up".into(),
                    PageUp,
                )
                .unless(reason),
                hint(
                    "PageDown",
                    "Next page",
                    "Scroll one frozen history viewport down".into(),
                    PageDown,
                )
                .unless(reason),
            ]);
        }
        motion.extend([
            hint(
                "Home",
                "Home",
                format!(
                    "Move to the {} start",
                    if history {
                        "history viewport or selected row"
                    } else {
                        "row"
                    }
                ),
                KeyCode::Home,
            )
            .unless(reason),
            hint(
                "End",
                "End",
                format!(
                    "Move to the {} end",
                    if history {
                        "history viewport or selected row"
                    } else {
                        "row"
                    }
                ),
                KeyCode::End,
            )
            .unless(reason),
        ]);
        let mut selection = vec![
            hint(
                "v",
                "Select",
                format!("Set the selection anchor in the {target}"),
                Char('v'),
            )
            .unless(reason),
            hint(
                "y",
                "Copy",
                format!("Send the selected {target} text to the clipboard; paste to verify"),
                Char('y'),
            )
            .unless(reason),
        ];
        if !history {
            selection.push(hint(
                "Enter",
                "Copy",
                "Send the selected captured text to the clipboard; paste to verify".into(),
                Enter,
            ));
        }
        return vec![
            HintGroup {
                title: "Move".into(),
                hints: motion,
            },
            HintGroup {
                title: "Selection".into(),
                hints: selection,
            },
            HintGroup {
                title: "View".into(),
                hints: help(),
            },
            HintGroup {
                title: "Exit".into(),
                hints: vec![
                    hint(
                        "Esc",
                        if copying { "Cancel copy" } else { "Back" },
                        if copying {
                            "Cancel the copy job; keep the history selection".into()
                        } else {
                            format!("Leave {target} and return to browse; sessions keep running")
                        },
                        Esc,
                    ),
                    hint(
                        "q",
                        "Exit",
                        format!("Leave {target} and return to browse; sessions keep running"),
                        Char('q'),
                    ),
                    KeyHint {
                        key: "Ctrl-g",
                        name: "Browse",
                        description: format!(
                            "Leave {target} and return to browse; sessions keep running"
                        ),
                        enabled: true,
                        action: HintAction::Browse,
                    },
                ],
            },
        ];
    }
    let selected = dashboard
        .action_session()
        .and_then(|id| find_session(dashboard, id));
    let (project, workspace) = dashboard.creation_context();
    let target = selected
        .map(|s| {
            format!(
                "{} (#{}) in {} / {}",
                s.name, s.id.0, s.project, s.workspace
            )
        })
        .unwrap_or_default();
    let missing = selected.is_none().then_some("no session selected");
    let no_workspace = (!dashboard
        .hierarchy
        .projects
        .iter()
        .any(|p| !p.workspaces.is_empty()))
    .then_some("no workspace available; register a project first");
    let workspace_target = if workspace.is_empty() {
        "the workspace you choose".into()
    } else {
        format!("{project} / {workspace}")
    };
    let project_target = if project.is_empty() {
        "the project you choose"
    } else {
        &project
    };
    let running = selected.is_some_and(|s| s.phase == SessionPhase::Running);
    let paused = selected.is_some_and(|s| s.phase == SessionPhase::Paused);
    let mut view = vec![
        KeyHint {
            key: "t/Ctrl-t",
            name: "Tasks",
            description: "Open scheduled tasks; sessions keep running".into(),
            enabled: true,
            action: HintAction::Tasks,
        },
        hint(
            ":",
            "Search",
            "Search dashboard actions and terminals".into(),
            Char(':'),
        ),
        hint(
            "v",
            "Split",
            format!("Open a second pane beside {target}; sessions keep running"),
            Char('v'),
        )
        .unless(
            (dashboard.panes.len() >= 2)
                .then_some("already split")
                .or(missing)
                .or_else(|| {
                    (dashboard
                        .visible_rows()
                        .iter()
                        .filter(|r| matches!(r, TreeRow::Session { .. }))
                        .count()
                        < 2)
                    .then_some("no other visible session")
                }),
        ),
        hint(
            "Tab/Shift-Tab",
            "Next pane",
            "Focus the other pane; sessions keep running".into(),
            Tab,
        )
        .unless((dashboard.panes.len() < 2).then_some("only one pane")),
        hint(
            "x",
            "Close pane",
            format!("Hide the focused pane for {target}; its session keeps running"),
            Char('x'),
        )
        .unless((dashboard.panes.len() < 2).then_some("only one pane")),
        hint(
            "j/Down",
            "Next session",
            "Select the next visible session".into(),
            Char('j'),
        ),
        hint(
            "k/Up",
            "Previous session",
            "Select the previous visible session".into(),
            Char('k'),
        ),
    ];
    view.extend(help());
    vec![
        HintGroup { title: "Create".into(), hints: vec![
            hint("n", "Create terminal", format!("Choose an agent or shell to start in {workspace_target}; opens a form"), Char('n')).unless(no_workspace),
            hint("w", "Create workspace", format!("Create a worktree and branch under {project_target} and start its local shell; opens a form"), Char('w')).unless(dashboard.hierarchy.projects.is_empty().then_some("no project registered")),
            hint("a", "Register project", "Register a repository and its root workspace; opens a form; keeps the repository".into(), Char('a')),
        ] },
        HintGroup { title: "Session".into(), hints: vec![
            hint("Enter", "Focus", format!("Send terminal input to {target}"), Enter).unless(missing.or_else(|| (!running).then_some("session is not running")).or_else(|| (!dashboard.input_is_allowed()).then_some("waiting for acknowledged screen"))),
            hint("p", "Pause", format!("Pause the processes of {target}; no confirmation"), Char('p')).unless(missing.or_else(|| (!running).then_some("not running"))),
            hint("r", "Resume", format!("Resume the processes of {target}; no confirmation"), Char('r')).unless(missing.or_else(|| (!paused).then_some("not paused"))),
            hint("X", "Close terminal", format!("Stop {target} and remove its record. Asks for confirmation."), Char('X')).unless(missing),
            hint("[", "Copy screen", format!("Freeze the current screen of {target} for copying; sessions keep running"), Char('[')).unless(missing.or_else(|| (!dashboard.focused_pane().is_some_and(|p| p.ready)).then_some("waiting for acknowledged screen"))),
            hint("PageUp", "History", format!("Read frozen output from {target}; sessions keep running"), PageUp).unless(missing.or_else(|| (!dashboard.focused_pane().is_some_and(|p| p.ready)).then_some("waiting for acknowledged screen"))),
        ] },
        HintGroup { title: "View".into(), hints: view },
        HintGroup { title: "Dashboard".into(), hints: vec![
            hint("q", "Detach", "Detach this dashboard; the server and every session keep running".into(), Char('q')),
        ] },
    ]
}

pub(super) fn footer(dashboard: &Dashboard, width: u16) -> String {
    use unicode_width::UnicodeWidthStr;
    let history = dashboard
        .history
        .as_ref()
        .filter(|_| dashboard.mode == InputMode::History);
    let mode = match dashboard.mode {
        InputMode::Browse => "BROWSE",
        InputMode::Copy => "COPY",
        InputMode::History
            if history.is_some_and(|v| v.copy_job.is_some() || v.copy_completion.is_some()) =>
        {
            "HISTORY COPY"
        }
        InputMode::History if history.is_some_and(|v| v.cursor_target.is_some()) => {
            "HISTORY Waiting"
        }
        InputMode::History if history.is_some_and(|v| v.anchor.is_some()) => "HISTORY SELECT",
        InputMode::History => "HISTORY",
        InputMode::Terminal => return "Terminal mode  Ctrl-g browse".into(),
    };
    let mut text: String = mode.chars().take(usize::from(width)).collect();
    let groups = key_hints(dashboard);
    let hints: Vec<_> = groups.iter().flat_map(|g| &g.hints).collect();
    let priority: &[&str] = if dashboard.mode == InputMode::Browse {
        &[
            "?",
            "Enter",
            "r",
            "Tab/Shift-Tab",
            "x",
            "n",
            ":",
            "v",
            "q",
            "t/Ctrl-t",
            "a",
            "w",
            "p",
            "Space",
        ]
    } else {
        &[
            "?", "Esc", "v", "y", "h/Left", "j/Down", "k/Up", "l/Right", "PageUp", "PageDown",
            "Space",
        ]
    };
    for key in priority {
        let Some(hint) = hints.iter().find(|h| h.key == *key && h.enabled) else {
            continue;
        };
        let part = format!(
            "  {} {}",
            hint.key.split('/').next().unwrap_or(hint.key),
            hint.name
        );
        if text.width() + part.width() <= usize::from(width) {
            text.push_str(&part);
        }
    }
    if let Some(view) = history {
        let detail = if view.copy_job.is_some() || view.copy_completion.is_some() {
            "Copying selection".into()
        } else if view.cursor_target.is_some() {
            "Waiting for history cell".into()
        } else if view.anchor.is_some() {
            view.cursor.map_or_else(
                || "Nothing to select on this row".into(),
                |cursor| {
                    format!(
                        "row {}:{}",
                        cursor.point.row + 1,
                        u32::from(cursor.point.col) + 1
                    )
                },
            )
        } else {
            String::new()
        };
        if !detail.is_empty() && text.width() + detail.width() + 2 <= usize::from(width) {
            text.push_str("  ");
            text.push_str(&detail);
        }
    }
    text
}

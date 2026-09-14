//! The dashboard's key bindings. One entry per key in an input mode carries the
//! label, the description, the reason the key is unavailable, and the action to
//! run. `key_action`, the footer, the key popup and the palette all read this
//! table, so a hint can never disagree with what the key does.

use super::state::find_session;
use super::{Dashboard, DashboardAction, InputMode, TreeRow};
use crate::task_tui::TasksView;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ovrcr_protocol::SessionPhase;

/// What a key does. Dispatch, the key popup and the palette run these directly;
/// none of them synthesises a key event to reach the next consumer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Palette,
    CreateTerminal,
    CreateWorkspace,
    RegisterProject,
    CloseTerminal,
    RemoveWorkspace,
    RemoveProject,
    Tasks,
    Detach,
    Focus,
    Pause,
    Resume,
    MarkReviewed,
    CopyScreen,
    History,
    Split,
    ClosePane,
    OtherPane,
    NextSession,
    PreviousSession,
    ToggleNotifications,
    ToggleSound,
    Leader,
    Help,
    /// Open a key-popup group rather than run an action.
    Group(char),
    /// Copy and History own their motion dispatcher; the table names the key it takes.
    Capture(KeyCode),
    /// Leave Copy or History for Browse.
    Browse,
}

/// Where the key popup's group shows a binding, and under which key there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GroupSlot {
    pub group: char,
    pub key: &'static str,
}

/// The one reason a key-popup group keeps a row, dimmed, instead of dropping it:
/// the pane has not acknowledged a screen yet, so the key becomes available on
/// its own. Every other reason means the action does not apply to this target.
const WAITING: &str = "waiting for acknowledged screen";

pub(super) struct KeyBinding {
    /// The label the footer and the popup print.
    pub key: &'static str,
    /// The key this binding answers to, after arrow and back-tab folding.
    pub code: KeyCode,
    pub name: &'static str,
    pub description: String,
    /// Why the binding is unavailable, or `None` while it is enabled.
    pub reason: Option<&'static str>,
    pub action: Action,
    /// Ignore repeat and release, so a toggle fires once per press.
    pub press_only: bool,
    pub group: Option<GroupSlot>,
}

pub(super) struct KeyGroup {
    pub title: String,
    pub keys: Vec<KeyBinding>,
}

pub(super) fn key_binding(
    key: &'static str,
    name: &'static str,
    description: String,
    code: KeyCode,
    action: Action,
) -> KeyBinding {
    KeyBinding {
        key,
        code,
        name,
        description,
        reason: None,
        action,
        press_only: false,
        group: None,
    }
}

/// A Copy or History key: the capture dispatcher interprets it.
fn capture(
    key: &'static str,
    name: &'static str,
    description: String,
    code: KeyCode,
) -> KeyBinding {
    key_binding(key, name, description, code, Action::Capture(code))
}

/// Browse's own modifier rule, kept from the dispatcher this table replaced:
/// Alt, Super and Shift never select a different action, so `Alt-x` closes the
/// pane that `x` closes, and Ctrl reaches Browse only as Ctrl-t. The key popup
/// is stricter and matches bindings without this fold.
fn browse_modifiers(mut key: KeyEvent) -> KeyEvent {
    key.modifiers &= KeyModifiers::CONTROL;
    key
}

/// Arrows and back-tab are spellings of the letter keys a `j/Down` style label
/// names. A single-character label answers to that character alone, so an arrow
/// never stands in for it.
fn folded(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Left => KeyCode::Char('h'),
        KeyCode::Down => KeyCode::Char('j'),
        KeyCode::Up => KeyCode::Char('k'),
        KeyCode::Right => KeyCode::Char('l'),
        KeyCode::BackTab => KeyCode::Tab,
        code => code,
    }
}

impl KeyBinding {
    fn unless(mut self, reason: Option<&'static str>) -> Self {
        if let Some(reason) = reason {
            self.reason = Some(reason);
            self.description = format!("{}: {reason}", self.name);
        }
        self
    }

    fn once(mut self) -> Self {
        self.press_only = true;
        self
    }

    /// Show in key-popup group `group` under `key`.
    fn group(mut self, group: char, key: &'static str) -> Self {
        self.group = Some(GroupSlot { group, key });
        self
    }

    pub(super) fn enabled(&self) -> bool {
        self.reason.is_none()
    }

    /// A group drops a row the current target cannot use, and dims one that is
    /// only waiting for the screen.
    pub(super) fn shown_in_group(&self) -> bool {
        self.enabled() || self.reason == Some(WAITING)
    }

    /// This binding as the popup group lists it: the group's label, and the
    /// single key that label names.
    pub(super) fn with_group_key(mut self, key: &'static str) -> Self {
        self.key = key;
        let mut chars = key.chars();
        if let (Some(ch), None) = (chars.next(), chars.next()) {
            self.code = KeyCode::Char(ch);
        }
        self
    }

    pub(super) fn matches(&self, key: KeyEvent) -> bool {
        if self.press_only && key.kind != KeyEventKind::Press {
            return false;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return false;
        }
        match self.action {
            // Tasks answers to the bare key and to Ctrl-t.
            Action::Tasks => {
                return key.code == KeyCode::Char('t')
                    && (key.modifiers.is_empty() || key.modifiers == KeyModifiers::CONTROL);
            }
            Action::Browse => {
                return key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('g');
            }
            _ => {}
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return false;
        }
        if self.key.chars().count() == 1 {
            return key.code == self.code;
        }
        folded(key.code) == self.code
    }
}

impl Dashboard {
    /// The one binding the current input mode gives `key`.
    pub(super) fn key_binding_for(&self, key: KeyEvent) -> Option<KeyBinding> {
        let key = if self.mode == InputMode::Browse {
            browse_modifiers(key)
        } else {
            key
        };
        // ponytail: rebuilds the table per key; it is already rebuilt per draw.
        keymap(self)
            .into_iter()
            .flat_map(|group| group.keys)
            .find(|binding| binding.matches(key))
    }

    /// Run a binding's action. The key popup and the palette reach the same
    /// handlers as a bare key press.
    pub(super) fn run(&mut self, action: Action) -> DashboardAction {
        // Opening a group stays inside the popup: it neither closes the popup
        // nor clears the notice, so it answers before the shared preamble.
        if let Action::Group(group) = action {
            return self.open_group(group);
        }
        self.whichkey = None;
        // Every action but these reached its handler through `key_action`, whose
        // press preamble clears the notice; a popup or palette row clicked with
        // the mouse never passes that preamble, so clear it here.
        if !matches!(action, Action::RemoveWorkspace | Action::RemoveProject) {
            self.desktop.notice = None;
        }
        match action {
            Action::Palette => self.open_palette(),
            Action::CreateTerminal => self.open_create_terminal(),
            Action::CreateWorkspace => self.open_create_workspace(),
            Action::RegisterProject => self.open_register_project(),
            Action::CloseTerminal => self.open_close_terminal(),
            Action::RemoveWorkspace => self.open_remove_context(true),
            Action::RemoveProject => self.open_remove_context(false),
            Action::Tasks => {
                if let Some(begin) = self.history_begin_request.as_mut() {
                    begin.cancelled = true;
                }
                self.cancel_mouse_gesture();
                let (project, _) = self.creation_context();
                self.tasks = Some(TasksView::with_projects(&self.hierarchy, &project));
                DashboardAction::Redraw
            }
            Action::Detach => DashboardAction::Detach,
            // `input_is_allowed` is the same predicate the Focus binding's reason
            // reports, so the refusal banner and the hint cannot disagree.
            Action::Focus => {
                if self.input_is_allowed() {
                    self.mode = InputMode::Terminal;
                    DashboardAction::Redraw
                } else {
                    self.refuse_input()
                }
            }
            Action::Pause => self.pause_request(true),
            Action::Resume => self.pause_request(false),
            Action::MarkReviewed => self.mark_reviewed_request(),
            Action::CopyScreen => self.begin_copy(),
            Action::History => self.begin_history_request(false),
            Action::Split => {
                self.split_pane();
                DashboardAction::Redraw
            }
            Action::ClosePane => {
                self.close_focused_pane();
                DashboardAction::Redraw
            }
            // Dispatch runs a disabled binding's action so it can refuse in its
            // own words, so every action holds its own bound: this one keeps the
            // pane count the "only one pane" reason reports.
            Action::OtherPane => {
                if self.panes.len() == 2 {
                    self.focus_pane((self.focused_pane + 1) % 2);
                }
                DashboardAction::Redraw
            }
            Action::NextSession => {
                self.move_selection(1);
                self.request_selected()
            }
            Action::PreviousSession => {
                self.move_selection(-1);
                self.request_selected()
            }
            Action::ToggleNotifications => self.toggle_desktop_notifications(),
            Action::ToggleSound => self.toggle_ready_sound(),
            Action::Leader => self.open_whichkey(true),
            Action::Help => self.open_whichkey(false),
            Action::Capture(code) => self.capture_key(KeyEvent::new(code, KeyModifiers::NONE)),
            Action::Browse => {
                self.capture_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL))
            }
            Action::Group(group) => self.open_group(group),
        }
    }

    /// Hand a Copy or History key to the dispatcher that owns that capture.
    fn capture_key(&mut self, key: KeyEvent) -> DashboardAction {
        match self.mode {
            InputMode::History => self.history_key_action(key),
            InputMode::Copy => self.copy_key_action(key),
            _ => DashboardAction::None,
        }
    }
}

pub(super) fn keymap(dashboard: &Dashboard) -> Vec<KeyGroup> {
    use KeyCode::{Char, Enter, Esc, PageDown, PageUp, Tab};
    if dashboard.mode == InputMode::Terminal {
        return Vec::new();
    }
    let help = || {
        vec![
            key_binding(
                "Space",
                "Leader",
                "Show keys; the next key runs an action".into(),
                Char(' '),
                Action::Leader,
            ),
            key_binding(
                "?",
                "Help",
                "Browse available keys; Enter or click runs an action".into(),
                Char('?'),
                Action::Help,
            ),
        ]
    };
    if matches!(dashboard.mode, InputMode::Copy | InputMode::History) {
        let history = dashboard.mode == InputMode::History;
        let capture_name = if history {
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
                || capture_name.to_owned(),
                |session| {
                    format!(
                        "{capture_name} of {} (#{}) in {} / {}",
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
            capture(
                "h/Left",
                "Move left",
                format!("Move left in the {target}"),
                Char('h'),
            )
            .unless(reason),
            capture(
                "j/Down",
                "Move down",
                format!("Move down in the {target}"),
                Char('j'),
            )
            .unless(reason),
            capture(
                "k/Up",
                "Move up",
                format!("Move up in the {target}"),
                Char('k'),
            )
            .unless(reason),
            capture(
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
                capture(
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
                capture(
                    "PageUp",
                    "Previous page",
                    "Scroll one frozen history viewport up".into(),
                    PageUp,
                )
                .unless(reason),
                capture(
                    "PageDown",
                    "Next page",
                    "Scroll one frozen history viewport down".into(),
                    PageDown,
                )
                .unless(reason),
            ]);
        }
        motion.extend([
            capture(
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
            capture(
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
            capture(
                "v",
                "Select",
                format!("Set the selection anchor in the {target}"),
                Char('v'),
            )
            .unless(reason),
            capture(
                "y",
                "Copy",
                format!("Send the selected {target} text to the clipboard; paste to verify"),
                Char('y'),
            )
            .unless(reason),
        ];
        if !history {
            selection.push(capture(
                "Enter",
                "Copy",
                "Send the selected captured text to the clipboard; paste to verify".into(),
                Enter,
            ));
        }
        return vec![
            KeyGroup {
                title: "Move".into(),
                keys: motion,
            },
            KeyGroup {
                title: "Selection".into(),
                keys: selection,
            },
            KeyGroup {
                title: "View".into(),
                keys: help(),
            },
            KeyGroup {
                title: "Exit".into(),
                keys: vec![
                    capture(
                        "Esc",
                        if copying { "Cancel copy" } else { "Back" },
                        if copying {
                            "Cancel the copy job; keep the history selection".into()
                        } else {
                            format!("Leave {target} and return to browse; sessions keep running")
                        },
                        Esc,
                    ),
                    capture(
                        "q",
                        "Exit",
                        format!("Leave {target} and return to browse; sessions keep running"),
                        Char('q'),
                    ),
                    key_binding(
                        "Ctrl-g",
                        "Browse",
                        format!("Leave {target} and return to browse; sessions keep running"),
                        Char('g'),
                        Action::Browse,
                    ),
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
        key_binding(
            "t/Ctrl-t",
            "Tasks",
            "Open scheduled tasks; sessions keep running".into(),
            Char('t'),
            Action::Tasks,
        )
        .group('v', "t/Ctrl-t"),
        key_binding(
            ":",
            "Search",
            "Search dashboard actions and terminals".into(),
            Char(':'),
            Action::Palette,
        )
        .group('v', ":"),
        key_binding(
            "v",
            "Split",
            format!("Open a second pane beside {target}; sessions keep running"),
            Char('v'),
            Action::Split,
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
        )
        .group('v', "v"),
        key_binding(
            "Tab/Shift-Tab",
            "Next pane",
            "Focus the other pane; sessions keep running".into(),
            Tab,
            Action::OtherPane,
        )
        .unless((dashboard.panes.len() < 2).then_some("only one pane"))
        .group('v', "Tab/Shift-Tab"),
        key_binding(
            "x",
            "Close pane",
            format!("Hide the focused pane for {target}; its session keeps running"),
            Char('x'),
            Action::ClosePane,
        )
        .unless((dashboard.panes.len() < 2).then_some("only one pane"))
        .group('v', "x"),
        key_binding(
            "j/Down",
            "Next session",
            "Select the next visible session".into(),
            Char('j'),
            Action::NextSession,
        )
        .group('v', "j/Down"),
        key_binding(
            "k/Up",
            "Previous session",
            "Select the previous visible session".into(),
            Char('k'),
            Action::PreviousSession,
        )
        .group('v', "k/Up"),
    ];
    view.extend(help());
    vec![
        KeyGroup { title: "Create".into(), keys: vec![
            key_binding("n", "Create terminal", format!("Choose an agent or shell to start in {workspace_target}; opens a form"), Char('n'), Action::CreateTerminal).unless(no_workspace).group('w', "n"),
            key_binding("w", "Create workspace", format!("Create a worktree and branch under {project_target} and start its local shell; opens a form"), Char('w'), Action::CreateWorkspace).unless(dashboard.hierarchy.projects.is_empty().then_some("no project registered")).group('p', "n"),
            key_binding("a", "Register project", "Register a repository and its root workspace; opens a form; keeps the repository".into(), Char('a'), Action::RegisterProject).group('p', "a"),
        ] },
        KeyGroup { title: "Session".into(), keys: vec![
            key_binding("Enter", "Focus", format!("Send terminal input to {target}"), Enter, Action::Focus).unless(missing.or_else(|| (!running).then_some("session is not running")).or_else(|| (!dashboard.input_is_allowed()).then_some(WAITING))).group('t', "Enter"),
            key_binding("p", "Pause", format!("Pause the processes of {target}; no confirmation"), Char('p'), Action::Pause).unless(missing.or_else(|| (!running).then_some("not running"))).group('t', "p"),
            key_binding("r", "Resume", format!("Resume the processes of {target}; no confirmation"), Char('r'), Action::Resume).unless(missing.or_else(|| (!paused).then_some("not paused"))).group('t', "r"),
            key_binding("R", "Mark reviewed", format!("Mark the displayed unread agent response from {target} reviewed; activity and reporting health stay unchanged"), Char('R'), Action::MarkReviewed).unless(missing.or_else(|| selected.and_then(|s| s.unread.as_ref()).is_none().then_some("no unread response"))).once().group('t', "R"),
            key_binding("X", "Close terminal", format!("Stop {target} and remove its record. Asks for confirmation."), Char('X'), Action::CloseTerminal).unless(missing).group('t', "x"),
            key_binding("[", "Copy screen", format!("Freeze the current screen of {target} for copying; sessions keep running"), Char('['), Action::CopyScreen).unless(missing.or_else(|| (!dashboard.focused_pane().is_some_and(|p| dashboard.pane_ready(p))).then_some(WAITING))).once().group('t', "c"),
            key_binding("PageUp", "History", format!("Read frozen output from {target}; sessions keep running"), PageUp, Action::History).unless(missing.or_else(|| (!dashboard.focused_pane().is_some_and(|p| dashboard.pane_ready(p))).then_some(WAITING))).group('t', "h"),
        ] },
        KeyGroup { title: "View".into(), keys: view },
        KeyGroup { title: "Dashboard".into(), keys: vec![
            key_binding("N", if dashboard.settings.desktop_notifications { "Disable desktop notifications" } else { "Enable desktop notifications" }, "Toggle notifications for new background agent responses or input requests in this dashboard; no replay".into(), Char('N'), Action::ToggleNotifications).once(),
            key_binding("S", if dashboard.settings.ready_sound { "Disable ready sound" } else { "Enable ready sound" }, "Toggle a sound for new background agent responses or input requests in this dashboard, independent of desktop notifications; no replay".into(), Char('S'), Action::ToggleSound).once(),
            key_binding("q", "Detach", "Detach this dashboard; the server and every session keep running".into(), Char('q'), Action::Detach),
        ] },
    ]
}

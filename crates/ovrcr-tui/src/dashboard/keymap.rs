//! The dashboard's key bindings. One entry per key in an input mode carries the
//! label, the description, the reason the key is unavailable, and the action to
//! run. `key_action`, the footer, the key popup and the palette all read this
//! table, so a hint can never disagree with what the key does.

use super::state::{find_session, workspace_heading};
use super::{Dashboard, DashboardAction, InputMode, TreeRow};
use crate::task_tui::TasksView;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ovrcr_protocol::{SessionKind, SessionPhase};

/// What a key does. Dispatch, the key popup and the palette run these directly,
/// without synthesising a key event to reach one another. `Capture` is the one
/// exception, and says why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Palette,
    Agents,
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
    /// Enter on a session that is no longer live: resume the conversation or
    /// reopen a fresh shell in the same row, through the palette's confirmation.
    Reopen,
    MarkReviewed,
    CopyScreen,
    History,
    Split,
    ClosePane,
    OtherPane,
    NextSession,
    PreviousSession,
    ToggleNotifications,
    #[cfg(target_os = "macos")]
    NotificationSettings,
    ToggleSound,
    CycleLocalTerminals,
    ToggleSidebar,
    QuotaDetails,
    /// The Settings editor. Reached from the menu and the palette
    /// only; it has no Browse key of its own.
    Settings,
    /// The Events popup. Reached from the menu and the palette only;
    /// it has no Browse key of its own.
    Events,
    Leader,
    Help,
    /// Open a key-popup group rather than run an action.
    Group(char),
    /// A Copy or History motion key. Those modes keep their own dispatcher, which
    /// reads a key event rather than an action, so this is the one action that
    /// rebuilds the event it names instead of calling a handler. Folding that
    /// dispatcher into the table is the follow-up.
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
pub(super) const LAUNCH_BLOCKED: &str = "root workspace is not on the default branch";
pub(super) const ROOT_PROTECTED: &str = "repository-root workspace cannot be removed";
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
    /// Answer only to the unmodified key. `whichkey_key` claims the popup keys
    /// before dispatch sees them and refuses any Ctrl, Alt or Super, so Browse's
    /// modifier fold must not hand them a modified key either.
    pub bare: bool,
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
        bare: false,
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
    pub(super) fn unless(mut self, reason: Option<&'static str>) -> Self {
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

    fn bare(mut self) -> Self {
        self.bare = true;
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
        self.enabled() || matches!(self.reason, Some(WAITING | LAUNCH_BLOCKED | ROOT_PROTECTED))
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
        let browse = self.mode == InputMode::Browse;
        // ponytail: rebuilds the table per key; it is already rebuilt per draw.
        keymap(self)
            .into_iter()
            .flat_map(|group| group.keys)
            .find(|binding| {
                binding.matches(if browse && !binding.bare {
                    browse_modifiers(key)
                } else {
                    key
                })
            })
    }

    /// Run a binding's action. The key popup and the palette reach the same
    /// handlers as a bare key press.
    pub(super) fn run(&mut self, action: Action) -> DashboardAction {
        self.agent_typing = None;
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
        let action = match action {
            Action::Palette => self.open_palette(),
            Action::Agents => self.open_agent_search(),
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
            Action::Focus => {
                if let Some(container) = self.selected_container.clone()
                    && self.toggle_row_collapse(&container)
                {
                    self.clamp_tree_offset(self.tree_viewport_height());
                    return DashboardAction::Redraw;
                }
                if self.input_is_allowed() {
                    self.mode = InputMode::Terminal;
                    DashboardAction::Redraw
                } else {
                    self.refuse_input()
                }
            }
            Action::Pause => self.pause_request(true),
            Action::Resume => self.pause_request(false),
            Action::Reopen => self.open_reopen_terminal(),
            Action::MarkReviewed => self.mark_reviewed_request(),
            Action::CopyScreen => self.begin_copy(),
            Action::History => self.begin_history_request(false),
            Action::Split => {
                self.split_pane();
                DashboardAction::Redraw
            }
            Action::ClosePane => {
                if self.action_session().is_some() {
                    self.close_focused_pane();
                }
                DashboardAction::Redraw
            }
            // Dispatch runs a disabled binding's action so it can refuse in its
            // own words, so every action holds its own bound: this one keeps the
            // pane count the "only one pane" reason reports.
            Action::OtherPane => {
                if self.action_session().is_some() && self.panes.len() == 2 {
                    self.focus_pane((self.focused_pane + 1) % 2);
                }
                DashboardAction::Redraw
            }
            Action::NextSession => {
                self.move_selection(1);
                if self.action_session().is_some() {
                    self.request_selected()
                } else {
                    DashboardAction::Redraw
                }
            }
            Action::PreviousSession => {
                self.move_selection(-1);
                if self.action_session().is_some() {
                    self.request_selected()
                } else {
                    DashboardAction::Redraw
                }
            }
            Action::ToggleNotifications => self.toggle_desktop_notifications(),
            #[cfg(target_os = "macos")]
            Action::NotificationSettings => self.recover_notification_permission(),
            Action::ToggleSound => self.toggle_ready_sound(),
            Action::CycleLocalTerminals => self.cycle_automatic_local_terminals(),
            Action::ToggleSidebar => self.toggle_sidebar(),
            Action::QuotaDetails => self.open_quota_details(),
            Action::Settings => self.open_details(super::quota::Details::Settings),
            Action::Events => self.open_events(),
            Action::Leader => self.open_whichkey(true),
            Action::Help => self.open_whichkey(false),
            Action::Capture(code) => self.capture_key(KeyEvent::new(code, KeyModifiers::NONE)),
            Action::Browse => {
                self.capture_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL))
            }
            Action::Group(group) => self.open_group(group),
        };
        self.reveal_queued_notice();
        action
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

pub(super) fn agents_binding() -> KeyBinding {
    key_binding(
        "s",
        "Agents",
        "Other running agents: Waiting Input first, then Unread; excludes current Agent. Candidates/order captured on open; reopen to refresh. Enter switches; typing waits for acknowledged screens. Loading input is discarded; Ctrl-g/Esc cancels."
            .into(),
        KeyCode::Char('s'),
        Action::Agents,
    )
    .group('v', "s")
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
                "Menu",
                "Show keys; the next key runs an action".into(),
                Char(' '),
                Action::Leader,
            )
            .bare(),
            key_binding(
                "?",
                "Help",
                "Browse available keys; Enter or click runs an action".into(),
                Char('?'),
                Action::Help,
            )
            .bare(),
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
                        session.display_name(),
                        session.id.0,
                        session.project,
                        session.workspace
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
    let current = dashboard.current_workspace();
    let target = selected
        .map(|s| {
            let place = current
                .map(|workspace| workspace_heading(dashboard, workspace))
                .unwrap_or_else(|| s.project.clone());
            format!("{} (#{}) in {place}", s.display_name(), s.id.0)
        })
        .unwrap_or_default();
    let missing = selected.is_none().then_some("no session selected");
    let no_workspace = (!dashboard
        .hierarchy
        .projects
        .iter()
        .any(|p| !p.workspaces.is_empty()))
    .then_some("no workspace available; register a project first");
    let launch_blocked = current
        .and_then(|workspace| workspace.warning.as_ref())
        .map(|_| LAUNCH_BLOCKED);
    let workspace_target = current
        .map(|workspace| workspace_heading(dashboard, workspace))
        .unwrap_or_else(|| {
            if workspace.is_empty() {
                "the workspace you choose".into()
            } else {
                format!("{project} / {workspace}")
            }
        });
    let project_target = if project.is_empty() {
        "the project you choose"
    } else {
        &project
    };
    let running = selected.is_some_and(|s| s.phase == SessionPhase::Running);
    let paused = selected.is_some_and(|s| s.phase == SessionPhase::Paused);
    let mut view = vec![
        agents_binding(),
        key_binding(
            "u",
            "Quota details",
            "Inspect native provider allowance, source, reset and freshness".into(),
            Char('u'),
            Action::QuotaDetails,
        )
        .bare()
        .group('v', "u"),
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
        .unless(
            (dashboard.panes.len() < 2)
                .then_some("only one pane")
                .or(missing),
        )
        .group('v', "Tab/Shift-Tab"),
        key_binding(
            "x",
            "Close pane",
            format!("Hide the focused pane for {target}; its session keeps running"),
            Char('x'),
            Action::ClosePane,
        )
        .unless(
            (dashboard.panes.len() < 2)
                .then_some("only one pane")
                .or(missing),
        )
        .group('v', "x"),
        key_binding(
            "j/Down",
            "Next row",
            "Select the next visible sidebar row".into(),
            Char('j'),
            Action::NextSession,
        )
        .group('v', "j/Down"),
        key_binding(
            "k/Up",
            "Previous row",
            "Select the previous visible sidebar row".into(),
            Char('k'),
            Action::PreviousSession,
        )
        .group('v', "k/Up"),
        key_binding(
            "b",
            if dashboard.sidebar_hidden {
                "Show sidebar"
            } else {
                "Hide sidebar"
            },
            "Hide or show the sidebar; the panes take its width; j and k still move the selection"
                .into(),
            Char('b'),
            Action::ToggleSidebar,
        )
        .once()
        .group('v', "b"),
    ];
    view.extend(help());
    let (enter_name, enter_description, enter_disabled, enter_action) = if let Some(container) =
        dashboard.selected_container.as_ref()
    {
        let collapsed = match container {
            TreeRow::Project { name } => dashboard.collapsed_projects.contains(name),
            TreeRow::Workspace { project, id } => dashboard
                .collapsed_workspaces
                .contains(&(project.clone(), id.clone())),
            TreeRow::Session { .. } => false,
        };
        if collapsed {
            (
                "Expand",
                "Expand the selected project or workspace".into(),
                None,
                Action::Focus,
            )
        } else {
            (
                "Collapse",
                "Collapse the selected project or workspace".into(),
                None,
                Action::Focus,
            )
        }
    } else if let Some(session) = selected.filter(|s| !s.phase.is_live()) {
        // The row outlived its process. Enter is the one key everyone tries, so
        // it leads straight to the recovery the palette already offers.
        let recovery = session.recovery.as_ref();
        let (name, description) = match &session.kind {
            SessionKind::Agent { .. } if recovery.is_some_and(|r| r.unavailable.is_some()) => (
                "Resume unavailable",
                format!("Explain why {target} cannot resume its conversation"),
            ),
            SessionKind::Agent { .. } if recovery.is_some_and(|r| r.failure.is_some()) => (
                "Retry resume conversation",
                format!(
                    "Resume the exact conversation of {target} in the same row, with confirmation"
                ),
            ),
            SessionKind::Agent { .. } => (
                "Resume conversation",
                format!(
                    "Resume the exact conversation of {target} in the same row, with confirmation"
                ),
            ),
            SessionKind::Terminal => (
                "Reopen shell",
                format!("Reopen {target} in a fresh shell in the same row, with confirmation"),
            ),
        };
        (name, description, None, Action::Reopen)
    } else {
        (
            "Focus",
            format!("Send terminal input to {target}"),
            missing
                .or_else(|| (!running).then_some("session is not running"))
                .or_else(|| (!dashboard.input_is_allowed()).then_some(WAITING)),
            Action::Focus,
        )
    };
    let groups = vec![
        KeyGroup { title: "Create".into(), keys: vec![
            key_binding("n", "Create terminal", format!("Choose an agent or shell to start in {workspace_target}; opens a form"), Char('n'), Action::CreateTerminal).unless(launch_blocked.or(no_workspace)).group('w', "n"),
            key_binding("w", "Create workspace", format!("Create a worktree and branch under {project_target} and choose its first Agent or Terminal; opens a form"), Char('w'), Action::CreateWorkspace).unless(dashboard.hierarchy.projects.is_empty().then_some("no project registered")).group('p', "n"),
            key_binding("a", "Register project", "Register a repository and its root workspace; opens a form; keeps the repository".into(), Char('a'), Action::RegisterProject).group('p', "a"),
        ] },
        KeyGroup { title: "Session".into(), keys: vec![
            key_binding("Enter", enter_name, enter_description, Enter, enter_action).unless(enter_disabled).group('t', "Enter"),
            key_binding("p", "Pause", format!("Pause the processes of {target}; no confirmation"), Char('p'), Action::Pause).unless(missing.or_else(|| (!running).then_some("not running"))).group('t', "p"),
            key_binding("r", "Resume", format!("Resume the processes of {target}; no confirmation"), Char('r'), Action::Resume).unless(missing.or_else(|| (!paused).then_some("not paused"))).group('t', "r"),
            key_binding("R", "Mark reviewed", format!("Mark the displayed unread agent response from {target} reviewed; activity and reporting health stay unchanged"), Char('R'), Action::MarkReviewed).unless(missing.or_else(|| selected.and_then(|s| s.unread.as_ref()).is_none().then_some("no unread response"))).once().group('t', "R"),
            key_binding("X", "Close terminal", format!("Archive {target}. Asks for confirmation before stopping live work."), Char('X'), Action::CloseTerminal).unless(missing).group('t', "x"),
            key_binding("[", "Copy screen", format!("Freeze the current screen of {target} for copying; sessions keep running"), Char('['), Action::CopyScreen).unless(missing.or_else(|| (!dashboard.focused_pane().is_some_and(|p| dashboard.pane_ready(p))).then_some(WAITING))).once().group('t', "c"),
            key_binding("PageUp", "History", format!("Read frozen output from {target}; sessions keep running"), PageUp, Action::History).unless(missing.or_else(|| (!dashboard.focused_pane().is_some_and(|p| dashboard.pane_ready(p))).then_some(WAITING))).group('t', "h"),
        ] },
        KeyGroup { title: "View".into(), keys: view },
        KeyGroup { title: "Dashboard".into(), keys: vec![
            key_binding("N", if dashboard.settings.desktop_notifications { "Disable desktop notifications" } else { "Enable desktop notifications" }, "Save notifications through the Server for new agent responses or input requests; no replay".into(), Char('N'), Action::ToggleNotifications).once(),
            key_binding("S", if dashboard.settings.ready_sound { "Disable ready sound" } else { "Enable ready sound" }, "Save ready sound through the Server for new agent responses or input requests, independent of desktop notifications; no replay".into(), Char('S'), Action::ToggleSound).once(),
            key_binding("L", "Automatic local terminals", format!("Cycle automatic local terminal creation (currently {}); applies to newly provisioned workspaces only", dashboard.settings.automatic_local_terminals.label()), Char('L'), Action::CycleLocalTerminals).once(),
            key_binding("q", "Detach", "Detach this dashboard; the server and every session keep running".into(), Char('q'), Action::Detach),
        ] },
    ];
    #[cfg(target_os = "macos")]
    let groups = {
        let mut groups = groups;
        let checking = dashboard.notification_permission_unconfirmed();
        groups.last_mut().unwrap().keys.insert(
            1,
            key_binding(
                "O",
                if checking {
                    "Check notification permission"
                } else {
                    "Open notification settings"
                },
                if checking {
                    "Check OVRCR's current macOS notification permission once"
                } else {
                    "Open System Settings, then choose Notifications → OVRCR to allow banners"
                }
                .into(),
                Char('O'),
                Action::NotificationSettings,
            )
            .once()
            .bare()
            .unless(if !dashboard.settings.desktop_notifications {
                Some("desktop notifications are disabled")
            } else {
                (!dashboard.notification_recovery_available())
                    .then_some("notification permission does not need recovery")
            }),
        );
        groups
    };
    groups
}

#[cfg(test)]
mod agent_search_tests {
    use super::*;

    #[test]
    fn agent_search_binding_is_discoverable_without_intercepting_terminal_s() {
        let mut dashboard = Dashboard::new(ovrcr_protocol::TerminalSize { rows: 24, cols: 80 });
        let binding = dashboard
            .key_binding_for(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
            .expect("Browse must expose Agents");
        assert_eq!(binding.name, "Agents");
        assert!(
            binding.group.is_some(),
            "the menu must expose the same action"
        );
        assert!(binding.enabled(), "empty search remains available");
        dashboard.mode = InputMode::Terminal;
        assert!(
            dashboard
                .key_binding_for(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE))
                .is_none()
        );
    }
}

#[cfg(test)]
mod local_terminals_binding_tests {
    use super::*;

    #[test]
    fn automatic_local_terminals_binding_is_discoverable() {
        let dashboard = Dashboard::new(ovrcr_protocol::TerminalSize { rows: 24, cols: 80 });
        let binding = dashboard
            .key_binding_for(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::NONE))
            .expect("Browse must expose Automatic local terminals");
        assert_eq!(binding.name, "Automatic local terminals");
        assert!(binding.enabled());
        assert_eq!(binding.action, Action::CycleLocalTerminals);
    }
}

use super::agents::{AgentOrigin, AgentSource, apply_overrides, detect_agents};

const HARNESS_DEFAULT_MODEL: &str = "@harness-default";
const CREATE_BRANCH_PICK_PREFIX: &str = "@create-branch:";

fn create_branch_pick_value(branch: &str) -> String {
    format!("{CREATE_BRANCH_PICK_PREFIX}{branch}")
}

fn created_branch_name(value: &str) -> Option<&str> {
    value.strip_prefix(CREATE_BRANCH_PICK_PREFIX)
}
use super::input::is_browse_key;
use super::keymap::{Action, KeyBinding, keymap};
use super::picker::{
    PathPicker, PickItem, PickList, complete_path, expand_path, split_workspace_pick,
};
use super::render::{CRUST, MAUVE, MUTED, PEACH, SUBTEXT, SURFACE0, SURFACE2, TEXT, clip_text};
use super::state::{
    find_session, find_workspace, replace_outside_paths, session_workspace_heading,
    workspace_heading, workspace_label,
};
use super::{Dashboard, DashboardAction, InputMode};
use crate::protocol::{
    BranchRequest, ClientMessage, CreateSessionRequest, Request, Response, SessionKind,
    SessionLaunch, SessionPhase, SessionRunId, SessionSummary, new_workspace_id,
};
use crate::session::SessionId;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Frame;
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthStr;

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
    WorkflowRecovery(SessionId, SessionRunId),
    TransferWorkflow(Box<crate::protocol::WorkflowTransfer>),
    NewAgentSession,
    CreateTerminal,
    // Kept for internal request-composition regression tests; public creation
    // routes use the branch-only CreateBranch form.
    #[allow(dead_code)]
    CreateWorkspace,
    CreateBranch,
    RenameTerminal(SessionId),
    ReopenTerminal(SessionId),
    StartNewConversation(SessionId),
    AcknowledgeStopped(SessionId, crate::protocol::SessionRunId),
    AgentResumeUnavailable(SessionId),
    RecoverLaunch,
    RecoverInitialPrompt,
    RegisterProject,
    CloseTerminal(SessionId),
    Archives,
    Unarchive(SessionId, crate::protocol::SessionRunId),
    DeleteArchived(SessionId, crate::protocol::SessionRunId, String),
    RemoveWorkspace,
    RemoveProject,
    Switch(SessionId),
    SwitchAgent(SessionId, SessionRunId),
    Hint(Action),
    Settings,
    /// Open Settings on this row.
    Setting(&'static ovrcr_protocol::Entry),
    Events,
    RefreshQuota,
    EnableQuota,
    ConnectStatus,
    #[cfg(target_os = "macos")]
    ITermSetup,
}

struct Entry {
    label: String,
    command: Command,
}

enum FieldKind {
    Text,
    Multiline,
    Toggle,
    Notice,
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
        request: Option<Box<Request>>,
        target: String,
    },
}

fn confirmation_request(page: &Page) -> Option<&Request> {
    match page {
        Page::Confirm { request, .. } => request.as_deref(),
        Page::Search { .. } | Page::Form { .. } => None,
    }
}

struct AgentCandidate {
    session: SessionSummary,
    search_identity: String,
    unavailable: Option<&'static str>,
}

/// Legacy Operator records remain on disk for diagnostics but are hidden from
/// every Dashboard list; their lifecycle routes are refused by the Server.
fn is_operator_scope(session: &SessionSummary) -> bool {
    matches!(
        session.scope,
        ovrcr_protocol::SessionScope::OperatorConversation { .. }
    )
}

fn running_agent(session: &SessionSummary) -> bool {
    !session.archived
        && session.phase == SessionPhase::Running
        && matches!(session.kind, SessionKind::Agent { .. })
}

/// The key action a palette entry duplicates.
fn command_action(command: &Command) -> Option<Action> {
    match command {
        Command::NewAgentSession => Some(Action::NewAgentSession),
        Command::CreateTerminal => Some(Action::CreateTerminal),
        Command::CreateWorkspace => Some(Action::CreateWorkspace),
        Command::CreateBranch => Some(Action::CreateWorkspace),
        Command::RegisterProject => Some(Action::RegisterProject),
        Command::CloseTerminal(_) => Some(Action::CloseTerminal),
        Command::Hint(action) => Some(*action),
        _ => None,
    }
}

/// Keep the entries that contain every word of `query`. Session IDs and
/// setting keys stay searchable without crowding the row.
fn retain_matching(entries: &mut Vec<Entry>, query: &str) {
    let query = query.to_lowercase();
    entries.retain(|entry| {
        let text = match &entry.command {
            Command::Switch(id) => format!("{} #{}", entry.label, id.0),
            Command::Setting(setting) => format!("{} {}", entry.label, setting.path),
            _ => entry.label.clone(),
        }
        .to_lowercase();
        query.split_whitespace().all(|word| text.contains(word))
    });
}

impl AgentCandidate {
    fn reason(&self, dashboard: &Dashboard) -> Option<&'static str> {
        if self.unavailable.is_some() {
            return self.unavailable;
        }
        match find_session(dashboard, self.session.id) {
            None => Some("Agent unavailable: session removed; reopen Agents to refresh"),
            Some(current) if current.run != self.session.run => {
                Some("Agent unavailable: process run changed; reopen Agents to refresh")
            }
            Some(current) if !running_agent(current) => {
                Some("Agent unavailable: paused or stopped; reopen Agents to refresh")
            }
            Some(current) if dashboard.focused_session() == Some(current.id) => {
                Some("Agent unavailable: already focused; reopen Agents to refresh")
            }
            _ => None,
        }
    }
}

pub(super) struct Palette {
    page: Page,
    agents: Option<Vec<AgentCandidate>>,
    pending: Option<u64>,
    archive: Option<Vec<crate::protocol::SessionSummary>>,
    workflows: Option<Vec<crate::protocol::WorkflowTransfer>>,
    error: Option<String>,
    /// A form field that would be refused, and why; drawn under that field.
    invalid: Option<(usize, String)>,
    workspace_acknowledged: bool,
    workspace_id: Option<String>,
    pub(super) launch_preference: Option<(String, super::settings::LaunchChoice)>,
    agent_catalog: Option<Vec<super::agents::AgentEntry>>,
    workspace_inventory: Option<crate::protocol::Registry>,
    launch_harness: Option<String>,
    failed_launch: Option<ovrcr_protocol::CreateAgentSessionRequest>,
    recovered_prompt_launch: Option<ovrcr_protocol::CreateAgentSessionRequest>,
    initial_prompt_summary: Option<String>,
    initial_prompt_intent: Option<ovrcr_protocol::InitialPromptIntentId>,
    model_capabilities: Option<crate::protocol::AgentModelCapabilities>,
    model_capability_identity: Option<String>,
    model_status: Option<String>,
    launch_project: Option<String>,
    suggestions: Suggestions,
    /// Creation forms keep choice lists collapsed until opened.
    new_agent_picker_open: bool,
    /// Submit was pressed while Git suggestions were still loading.
    submit_when_ready: bool,
    cursor: super::text_cursor::TextCursor,
    scroll: Option<usize>,
    /// The Search page chooses the new split pane's session.
    split: bool,
    /// The Confirm page is the Connect agent status preview: Enter writes the hooks.
    connect_status: bool,
    /// The exact request identity and lifecycle receipt being reconciled by branch creation.
    branch_attempt: Option<BranchAttempt>,
    /// Project selected in the branch creation form.
    branch_project: Option<String>,
    /// Last committed registered destination while its picker query changes.
    parent_branch_target: Option<(String, bool)>,
}

#[derive(Clone)]
struct BranchAttempt {
    token: u64,
    project: String,
    workspace_id: String,
    branch: String,
    accepted: bool,
    outcome: Option<crate::protocol::LifecycleOutcome>,
    inspect_request: Option<u64>,
    list_request: Option<u64>,
    registry: Option<crate::protocol::Registry>,
    hierarchy: Option<crate::protocol::HierarchySnapshot>,
    inspect_complete: bool,
    list_complete: bool,
    reconciliation_error: Option<String>,
    blocked: bool,
    agent_form: bool,
}

impl Palette {
    /// Whether this is the Create workspace form.
    pub(super) fn creates_workspace(&self) -> bool {
        matches!(
            self.page,
            Page::Form {
                command: Command::CreateWorkspace | Command::CreateBranch,
                ..
            }
        )
    }
}

impl Dashboard {
    pub(super) fn cancel_notification_palette(&mut self) {
        if let Some(palette) = self.palette.take() {
            if let Some(request_id) = palette.pending {
                self.navigation.detached_palette_requests.insert(request_id);
            }
            if let Some(request_id) = palette.suggestions.inspect {
                self.ignored_responses.insert(request_id);
            }
        }
    }
}

/// Agent status left off at startup ("Not now"): a calm one-time footer note, never an
/// error, and Connect agent status in the palette until connecting succeeds.
impl Dashboard {
    pub fn status_off(&mut self, note: String, connect: crate::ConnectStatusFn) {
        self.connect_status = Some((connect, note.clone()));
        // A notice already in the footer, such as settings findings, keeps its place.
        if self.desktop.notice.is_some() {
            self.notice_queue.push_back(note);
        } else {
            self.show_notice(note);
        }
    }

    /// Shows the startup screen's text on a Confirm page; nothing is written until Enter.
    /// When there is nothing to confirm (already connected, or every file refused) the
    /// result shows at once.
    fn preview_agent_status(&mut self, palette: &mut Palette) -> DashboardAction {
        let Some((connect, _)) = &self.connect_status else {
            return DashboardAction::Redraw;
        };
        match connect(false) {
            crate::ConnectStatus::Confirm(text) => {
                palette.page = Page::Confirm {
                    request: None,
                    target: text,
                };
                palette.connect_status = true;
                DashboardAction::Redraw
            }
            crate::ConnectStatus::Done(result) => self.finish_agent_status(result),
        }
    }

    fn connect_agent_status(&mut self) -> DashboardAction {
        let Some((connect, _)) = &self.connect_status else {
            return DashboardAction::Redraw;
        };
        match connect(true) {
            crate::ConnectStatus::Done(result) => self.finish_agent_status(result),
            crate::ConnectStatus::Confirm(_) => DashboardAction::Redraw,
        }
    }

    fn finish_agent_status(&mut self, result: Result<String, String>) -> DashboardAction {
        let Some((_, note)) = self.connect_status.clone() else {
            return DashboardAction::Redraw;
        };
        match result {
            Ok(done) => {
                self.connect_status = None;
                // The startup note is no longer true; drop it wherever it waits.
                self.notice_queue.retain(|queued| *queued != note);
                if self.desktop.notice.as_ref() == Some(&note) {
                    self.desktop.notice = None;
                }
                // Success replaces a failed attempt's banner; waiting notices stay queued.
                self.dismiss_error_banner();
                if let Some(waiting) = self.desktop.notice.take() {
                    self.notice_queue.push_front(waiting);
                }
                self.show_notice(done);
            }
            Err(problem) => self.set_error(problem),
        }
        DashboardAction::Redraw
    }
}

#[derive(Clone, Copy)]
enum PaletteTarget {
    Search,
    Entry(usize),
    Field(usize, bool),
    FieldPair(usize, usize, bool),
    MultilineLine(usize, usize),
    Option(usize, usize),
}

fn palette_geometry_for(outer: Rect, palette: Option<&Palette>) -> (Rect, Rect, Rect, Rect) {
    let page = palette.map(|palette| &palette.page);
    let creation_form = page.is_some_and(|page| {
        matches!(
            page,
            Page::Form {
                command: Command::NewAgentSession | Command::CreateBranch,
                ..
            }
        )
    });
    let creation_under_minimum = page.is_some_and(|page| creation_form_below_minimum(page, outer));
    let (width, height, footer_limit) = if creation_form && !creation_under_minimum {
        let height = match page.unwrap() {
            Page::Form {
                command: Command::CreateBranch,
                ..
            } => 14,
            _ => 17,
        };
        (
            outer.width.min(80).saturating_sub(8).min(72),
            outer.height.min(height),
            3,
        )
    } else {
        (outer.width.min(82), outer.height.min(19), 4)
    };
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
        .min(footer_limit)
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

struct CreationPickerPopup<'a> {
    field_index: usize,
    area: Rect,
    lines: Vec<Line<'a>>,
    option_rows: Vec<(u16, usize)>,
}

fn creation_picker_popup<'a>(
    body: Rect,
    palette: &'a Palette,
    targets: &[(usize, PaletteTarget)],
    scroll: usize,
) -> Option<CreationPickerPopup<'a>> {
    if !palette.new_agent_picker_open {
        return None;
    }
    let Page::Form {
        command,
        fields,
        active,
        ..
    } = &palette.page
    else {
        return None;
    };
    let field = fields.get(*active)?;
    if !compact_picker_field(command, field) {
        return None;
    }
    let FieldKind::Pick(list) = &field.kind else {
        return None;
    };

    let (anchor_row, anchor_x, anchor_width) =
        targets.iter().find_map(|(row, target)| match target {
            PaletteTarget::Field(index, true) if index == active => {
                Some((*row, body.x, body.width))
            }
            PaletteTarget::FieldPair(left, right, true) if left == active || right == active => {
                let (left_width, gap, right_width) = pair_columns(body);
                if active == left {
                    Some((*row, body.x, left_width))
                } else {
                    Some((
                        *row,
                        body.x.saturating_add(left_width).saturating_add(gap),
                        right_width,
                    ))
                }
            }
            _ => None,
        })?;
    let screen_row = anchor_row.checked_sub(scroll)?;
    let anchor_y = body.y.checked_add(u16::try_from(screen_row).ok()?)?;
    if anchor_y >= body.bottom() {
        return None;
    }

    let widest_option = list
        .filtered()
        .iter()
        .map(|item| UnicodeWidthStr::width(item.label.as_str()))
        .max()
        .unwrap_or(12);
    let width = anchor_width
        .max(u16::try_from(widest_option.saturating_add(6)).unwrap_or(u16::MAX))
        .min(body.width);
    if width < 4 {
        return None;
    }
    let x = anchor_x.max(body.x).min(body.right().saturating_sub(width));

    let desired_rows = list.filtered().len().clamp(1, 8);
    let desired_height = u16::try_from(desired_rows.saturating_add(2)).unwrap_or(u16::MAX);
    let below = body.bottom().saturating_sub(anchor_y.saturating_add(1));
    let above = anchor_y.saturating_sub(body.y);
    let (place_below, available) = if below >= desired_height {
        (true, below)
    } else if above >= desired_height {
        (false, above)
    } else if below >= above {
        (true, below)
    } else {
        (false, above)
    };
    let height = desired_height.min(available).min(body.height);
    if height < 3 {
        return None;
    }
    let y = if place_below {
        anchor_y.saturating_add(1)
    } else {
        anchor_y.saturating_sub(height)
    };
    let area = Rect::new(x, y, width, height);
    let row_limit = usize::from(height.saturating_sub(2));
    let (lines, option_rows) = list.lines_bounded(row_limit);
    let option_rows = option_rows
        .into_iter()
        .map(|(row, option)| (area.y.saturating_add(1).saturating_add(row as u16), option))
        .collect();
    Some(CreationPickerPopup {
        field_index: *active,
        area,
        lines,
        option_rows,
    })
}

#[cfg(test)]
fn palette_geometry_for_dashboard(dashboard: &Dashboard, outer: Rect) -> (Rect, Rect, Rect, Rect) {
    palette_geometry_for(outer, dashboard.palette.as_ref())
}

#[cfg(test)]
type CreationFormSnapshot = (
    Rect,
    Rect,
    Rect,
    Rect,
    Vec<String>,
    Vec<(usize, String)>,
    usize,
);

#[cfg(test)]
type StableFormGeometry = (Rect, Rect, Rect, Rect, Vec<(usize, String)>, usize, usize);

#[cfg(test)]
fn base_form_snapshot(dashboard: &Dashboard, width: u16, height: u16) -> CreationFormSnapshot {
    let (area, inner, body, buttons) =
        palette_geometry_for_dashboard(dashboard, Rect::new(0, 0, width, height));
    let (lines, targets, scroll) = dashboard.palette_body(body);
    let text = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();
    let targets = targets
        .iter()
        .map(|(row, target)| {
            let target = match target {
                PaletteTarget::Field(index, value) => format!("field:{index}:{value}"),
                PaletteTarget::FieldPair(left, right, value) => {
                    format!("pair:{left}:{right}:{value}")
                }
                PaletteTarget::MultilineLine(index, line) => {
                    format!("multiline:{index}:{line}")
                }
                PaletteTarget::Option(index, selected) => {
                    format!("option:{index}:{selected}")
                }
                PaletteTarget::Search => "search".into(),
                PaletteTarget::Entry(index) => format!("entry:{index}"),
            };
            (*row, target)
        })
        .collect();
    (area, inner, body, buttons, text, targets, scroll)
}

#[cfg(test)]
fn base_form_snapshot_closed_state(
    dashboard: &mut Dashboard,
    width: u16,
    height: u16,
) -> CreationFormSnapshot {
    let picker_was_open = dashboard
        .palette
        .as_ref()
        .is_some_and(|palette| palette.new_agent_picker_open);
    dashboard.palette.as_mut().unwrap().new_agent_picker_open = false;
    let snapshot = base_form_snapshot(dashboard, width, height);
    dashboard.palette.as_mut().unwrap().new_agent_picker_open = picker_was_open;
    snapshot
}

#[cfg(test)]
fn form_geometry(snapshot: &CreationFormSnapshot) -> StableFormGeometry {
    (
        snapshot.0,
        snapshot.1,
        snapshot.2,
        snapshot.3,
        snapshot.5.clone(),
        snapshot.6,
        snapshot.4.len(),
    )
}

#[cfg(test)]
fn creation_popup_snapshot(
    dashboard: &Dashboard,
    width: u16,
    height: u16,
) -> (Rect, Rect, Vec<String>, Vec<(u16, usize)>) {
    let (_, _, body, _) = palette_geometry_for_dashboard(dashboard, Rect::new(0, 0, width, height));
    let (_, targets, scroll) = dashboard.palette_body(body);
    let popup =
        creation_picker_popup(body, dashboard.palette.as_ref().unwrap(), &targets, scroll).unwrap();
    (
        body,
        popup.area,
        popup.lines.iter().map(ToString::to_string).collect(),
        popup.option_rows,
    )
}

#[cfg(test)]
fn rect_contains_rect(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.right() <= outer.right()
        && inner.bottom() <= outer.bottom()
}

#[cfg(test)]
fn render_creation_form(dashboard: &Dashboard, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| super::render::draw_dashboard_at(frame, dashboard, 0))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn button_text(page: &Page, width: u16, refresh_branch: bool) -> &'static str {
    if width < 17 {
        return "[OK] [X]";
    }
    match page {
        Page::Search { .. } => "[Open] [Cancel]",
        Page::Confirm { .. } => "[Confirm] [Cancel]",
        Page::Form {
            command: Command::NewAgentSession,
            ..
        } if refresh_branch => "[Refresh] [Cancel]",
        Page::Form {
            command: Command::NewAgentSession,
            ..
        } => "[Start agent] [Cancel]",
        Page::Form {
            command: Command::CreateBranch,
            ..
        } if refresh_branch => "[Refresh] [Cancel]",
        Page::Form {
            command: Command::CreateBranch,
            ..
        } => "[Create branch] [Cancel]",
        Page::Form { .. } => "[Submit] [Cancel]",
    }
}

fn palette_button_line(page: &Page, width: u16, refresh_branch: bool) -> Line<'static> {
    let text = button_text(page, width, refresh_branch);
    if matches!(
        page,
        Page::Form {
            command: Command::NewAgentSession | Command::CreateBranch,
            ..
        }
    ) && let Some((primary, _)) = text.split_once(" [Cancel]")
    {
        Line::from(vec![
            Span::styled(
                primary.to_owned(),
                Style::default()
                    .fg(CRUST())
                    .bg(MAUVE())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled("[Cancel]", Style::default().fg(MAUVE())),
        ])
    } else {
        Line::from(text)
    }
}

fn close_terminal_id(page: &Page) -> Option<SessionId> {
    match confirmation_request(page) {
        Some(Request::CloseTerminal { session, .. }) => Some(*session),
        _ => None,
    }
}

/// Direction D accent-rail card, terminal-close confirms only.
/// `d-confirmation-72.svg` uses a 9px cell: the peach rail is the rect at
/// x=9, width=18 (`#fab387`, two cells) and body glyphs start at x=36
/// (column 4). directions.md: the rail scales with measured card height.
const CLOSE_CONFIRM_RAIL_WIDTH: u16 = 2;
const CLOSE_CONFIRM_TEXT_COLUMN: u16 = 4;
const CLOSE_CONFIRM_ACTION_GAP: u16 = 2;
const CLOSE_CANCEL_LABEL: &str = "[Esc Cancel]";
const CLOSE_CONFIRM_LABEL: &str = "[Enter Close]";
const CLOSE_CONSEQUENCE: &str = "Stops its currently owned processes and archives its record.";
const CLOSE_TITLE: &str = "Close terminal?";

fn close_terminal_identity(name: &str, id: u64) -> String {
    let marked = format!("(#{id})");
    if name.contains(&marked) {
        name.to_string()
    } else {
        format!("{name} {marked}")
    }
}

fn close_confirm_action_line() -> String {
    let gap = " ".repeat(usize::from(CLOSE_CONFIRM_ACTION_GAP));
    format!("{CLOSE_CANCEL_LABEL}{gap}{CLOSE_CONFIRM_LABEL}")
}

struct CloseConfirmText {
    warning: Option<&'static str>,
    identity: String,
    place: String,
    project: String,
    workspace: String,
    error: Option<String>,
    pending: bool,
}

#[derive(Clone, Copy)]
enum CloseConfirmRole {
    Blank,
    Identity,
    Place,
    Meta,
    Consequence,
}

struct CloseConfirmLayout {
    dialog: Rect,
    rail: Rect,
    text: Rect,
    cancel: Rect,
    confirm: Rect,
    body_rows: u16,
}

fn close_confirm_dialog_width(screen_width: u16) -> u16 {
    let labels = CLOSE_CANCEL_LABEL.len() as u16
        + CLOSE_CONFIRM_ACTION_GAP
        + CLOSE_CONFIRM_LABEL.len() as u16;
    let min_width = CLOSE_CONFIRM_TEXT_COLUMN.saturating_mul(2) + labels;
    if screen_width <= min_width {
        screen_width
    } else {
        // Same horizontal cap as the other palette cards.
        screen_width.min(82)
    }
}

fn close_confirm_content_width(dialog_width: u16) -> u16 {
    let edge = CLOSE_CONFIRM_TEXT_COLUMN.min(dialog_width / 2);
    dialog_width.saturating_sub(edge.saturating_mul(2))
}

fn close_confirm_layout(screen: Rect, body_rows: u16) -> CloseConfirmLayout {
    let width = close_confirm_dialog_width(screen.width).min(screen.width);
    let mut height = body_rows.saturating_add(3); // action row + two border rows
    if screen.height > 0 {
        height = height.min(screen.height);
    }
    if screen.height >= 3 {
        height = height.max(3);
    }
    let dialog = Rect::new(
        screen.x + screen.width.saturating_sub(width) / 2,
        screen.y + screen.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let rail = Rect::new(
        dialog.x.saturating_add(1),
        dialog.y.saturating_add(1),
        CLOSE_CONFIRM_RAIL_WIDTH.min(dialog.width.saturating_sub(2)),
        dialog.height.saturating_sub(2),
    );
    let edge = CLOSE_CONFIRM_TEXT_COLUMN.min(dialog.width / 2);
    let text = Rect::new(
        dialog.x.saturating_add(edge),
        dialog.y.saturating_add(1),
        close_confirm_content_width(dialog.width),
        dialog.height.saturating_sub(3),
    );
    let action_y = dialog.y + dialog.height.saturating_sub(2);
    let action_h = u16::from(dialog.height >= 2);
    let confirm_w = CLOSE_CONFIRM_LABEL.len() as u16;
    let cancel_w = CLOSE_CANCEL_LABEL.len() as u16;
    let content_right = text.x.saturating_add(text.width);
    let confirm_x = content_right.saturating_sub(confirm_w);
    let cancel_x = confirm_x.saturating_sub(CLOSE_CONFIRM_ACTION_GAP + cancel_w);
    CloseConfirmLayout {
        dialog,
        rail,
        text,
        cancel: Rect::new(cancel_x, action_y, cancel_w, action_h),
        confirm: Rect::new(confirm_x, action_y, confirm_w, action_h),
        body_rows,
    }
}

fn measure_close_confirm(screen: Rect, text: &CloseConfirmText) -> CloseConfirmLayout {
    let width = close_confirm_dialog_width(screen.width).min(screen.width);
    let rows = close_confirm_line_count(text, close_confirm_content_width(width));
    close_confirm_layout(screen, rows)
}

fn close_confirm_segments(text: &CloseConfirmText) -> Vec<(String, CloseConfirmRole)> {
    let mut lines = vec![
        (String::new(), CloseConfirmRole::Blank),
        (text.identity.clone(), CloseConfirmRole::Identity),
        (text.place.clone(), CloseConfirmRole::Place),
        (format!("Project: {}", text.project), CloseConfirmRole::Meta),
        (
            format!("Workspace: {}", text.workspace),
            CloseConfirmRole::Meta,
        ),
        (String::new(), CloseConfirmRole::Blank),
        (CLOSE_CONSEQUENCE.to_string(), CloseConfirmRole::Consequence),
    ];
    if let Some(warning) = text.warning {
        lines.push((warning.to_owned(), CloseConfirmRole::Consequence));
    }
    if let Some(error) = &text.error
        && !error.is_empty()
    {
        lines.push((error.clone(), CloseConfirmRole::Consequence));
    }
    if text.pending {
        lines.push(("Working…".to_string(), CloseConfirmRole::Meta));
    }
    lines.push((String::new(), CloseConfirmRole::Blank));
    lines
}

fn close_confirm_lines(text: &CloseConfirmText, width: u16) -> Vec<Line<'static>> {
    close_confirm_segments(text)
        .into_iter()
        .flat_map(|(line, role)| {
            wrap_plain(&line, usize::from(width))
                .into_iter()
                .map(move |wrapped| Line::from(Span::styled(wrapped, close_confirm_style(role))))
        })
        .collect()
}

fn close_confirm_line_count(text: &CloseConfirmText, width: u16) -> u16 {
    u16::try_from(close_confirm_lines(text, width).len()).unwrap_or(u16::MAX)
}

fn close_confirm_style(role: CloseConfirmRole) -> Style {
    match role {
        CloseConfirmRole::Blank | CloseConfirmRole::Place => Style::default().fg(TEXT()),
        CloseConfirmRole::Identity => Style::default().fg(TEXT()).add_modifier(Modifier::BOLD),
        CloseConfirmRole::Meta => Style::default().fg(SUBTEXT()),
        CloseConfirmRole::Consequence => Style::default().fg(PEACH()),
    }
}

fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() || width == 0 {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut line = String::new();
    let mut line_width = 0usize;
    for word in text.split_whitespace() {
        let mut rest = word;
        while !rest.is_empty() {
            let rest_width = UnicodeWidthStr::width(rest);
            if line_width == 0 {
                if rest_width <= width {
                    line.push_str(rest);
                    line_width = rest_width;
                    break;
                }
                let (head, tail) = split_at_width(rest, width);
                if head.is_empty() {
                    let mut chars = rest.chars();
                    let ch = chars.next().expect("rest is not empty");
                    rows.push(ch.to_string());
                    rest = chars.as_str();
                } else {
                    rows.push(head.to_string());
                    rest = tail;
                }
                continue;
            }
            if line_width + 1 + rest_width <= width {
                line.push(' ');
                line.push_str(rest);
                line_width += 1 + rest_width;
                break;
            }
            rows.push(std::mem::take(&mut line));
            line_width = 0;
        }
    }
    if !line.is_empty() {
        rows.push(line);
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

fn split_at_width(text: &str, width: usize) -> (&str, &str) {
    let mut used = 0usize;
    for (index, ch) in text.char_indices() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if ch_width > width.saturating_sub(used) {
            if used == 0 {
                return ("", text);
            }
            return text.split_at(index);
        }
        used += ch_width;
        if used == width {
            return text.split_at(index + ch.len_utf8());
        }
    }
    (text, "")
}

fn paint_rect(frame: &mut Frame<'_>, area: Rect, style: Style) {
    let buffer = frame.buffer_mut();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol(" ");
                cell.set_style(style);
            }
        }
    }
}

fn paint_close_confirm_title(frame: &mut Frame<'_>, dialog: Rect) {
    let start = dialog.x.saturating_add(CLOSE_CONFIRM_TEXT_COLUMN);
    let end = dialog.x.saturating_add(dialog.width).saturating_sub(1);
    if start >= end || dialog.height == 0 {
        return;
    }
    let room = usize::from(end.saturating_sub(start));
    let notch = CLOSE_TITLE.len().saturating_add(2).min(room);
    let y = dialog.y;
    let buffer = frame.buffer_mut();
    for offset in 0..notch {
        if let Some(cell) = buffer.cell_mut((start + u16::try_from(offset).unwrap_or(0), y)) {
            cell.set_symbol(" ");
            cell.set_style(Style::default().bg(CRUST()).fg(MAUVE()));
        }
    }
    for (offset, ch) in CLOSE_TITLE.chars().enumerate().take(room) {
        let symbol = ch.to_string();
        if let Some(cell) = buffer.cell_mut((start + u16::try_from(offset).unwrap_or(0), y)) {
            cell.set_symbol(&symbol);
            cell.set_style(
                Style::default()
                    .bg(CRUST())
                    .fg(MAUVE())
                    .add_modifier(Modifier::BOLD),
            );
        }
    }
}

impl Palette {
    fn new() -> Self {
        Self {
            page: Page::Search {
                query: String::new(),
                selected: 0,
            },
            agents: None,
            pending: None,
            archive: None,
            workflows: None,
            error: None,
            invalid: None,
            workspace_acknowledged: false,
            workspace_id: None,
            launch_preference: None,
            agent_catalog: None,
            workspace_inventory: None,
            launch_harness: None,
            failed_launch: None,
            recovered_prompt_launch: None,
            initial_prompt_summary: None,
            initial_prompt_intent: None,
            model_capabilities: None,
            model_capability_identity: None,
            model_status: None,
            launch_project: None,
            suggestions: Suggestions::default(),
            new_agent_picker_open: false,
            submit_when_ready: false,
            cursor: Default::default(),
            scroll: None,
            split: false,
            connect_status: false,
            branch_attempt: None,
            branch_project: None,
            parent_branch_target: None,
        }
    }

    fn insert(&mut self, text: &str) {
        if self.pending.is_some() || self.submit_when_ready {
            return;
        }
        self.scroll = None;
        let multiline = matches!(
            &self.page,
            Page::Form { fields, active, .. }
                if matches!(fields[*active].kind, FieldKind::Multiline)
        );
        let mut text: String = text
            .chars()
            .filter(|ch| {
                if multiline {
                    !ch.is_control() || matches!(ch, '\n' | '\r' | '\t')
                } else {
                    !ch.is_control()
                }
            })
            .collect();
        if multiline {
            text = text.replace("\r\n", "\n").replace('\r', "\n");
        }
        if text.is_empty() {
            return;
        }
        match &mut self.page {
            Page::Search { query, selected } => {
                *selected = 0;
                self.cursor.insert(query, &text);
            }
            Page::Form {
                command,
                fields,
                active,
                name_edited,
                root_edited,
                ..
            } => {
                let filtering_compact_picker = compact_picker_field(command, &fields[*active]);
                if filtering_compact_picker {
                    self.new_agent_picker_open = true;
                } else {
                    mark_form_edit(fields, *active, name_edited, root_edited);
                }
                if self.invalid.as_ref().is_some_and(|(at, _)| at == active) {
                    self.invalid = None;
                }
                match &mut fields[*active].kind {
                    FieldKind::Pick(list) => {
                        self.cursor.insert(&mut list.query, &text);
                        list.selected = 0;
                        if !filtering_compact_picker {
                            accept_pick(&mut fields[*active]);
                        }
                    }
                    FieldKind::Text => self.cursor.insert(&mut fields[*active].value, &text),
                    FieldKind::Multiline => self.cursor.insert(&mut fields[*active].value, &text),
                    FieldKind::Path(picker) => {
                        picker.selected = 0;
                        self.cursor.insert(&mut fields[*active].value, &text);
                    }
                    FieldKind::Toggle => {}
                    FieldKind::Notice => {}
                }
            }
            Page::Confirm { .. } => return,
        }
        self.error = None;
    }
}

fn text_field(label: &'static str, value: String, required: bool) -> Field {
    Field {
        label,
        value,
        required,
        hidden: false,
        edited: false,
        kind: FieldKind::Text,
    }
}

fn notice_field(label: &'static str, value: String, hidden: bool) -> Field {
    Field {
        label,
        value,
        required: false,
        hidden,
        edited: false,
        kind: FieldKind::Notice,
    }
}

fn field_value(fields: &[Field], label: &str) -> String {
    fields
        .iter()
        .find(|field| field.label == label)
        .map(|field| field.value.clone())
        .unwrap_or_default()
}

fn agent_field_value(fields: &[Field]) -> String {
    let harness = field_value(fields, "Agent harness");
    if harness.is_empty() {
        field_value(fields, "Agent")
    } else {
        harness
    }
}

/// The visible Task, trimmed; empty when the form has none.
fn form_task(fields: &[Field]) -> String {
    fields
        .iter()
        .find(|field| field.label == "Task" && !field.hidden)
        .map(|field| field.value.trim().to_owned())
        .unwrap_or_default()
}

/// The session name a task gives: one line of at most 60 characters.
fn task_name(task: &str) -> String {
    let line = task.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(60) {
        Some((end, _)) => line[..end].trim_end().to_owned(),
        None => line,
    }
}

/// The branch name a task suggests after the prefix: lowercase ASCII letters, digits and
/// `_`, every other run of characters one `-`, at most 50 bytes. Empty when nothing is left,
/// so the bare prefix asks for a name.
fn task_slug(task: &str) -> String {
    let mut slug = String::new();
    for c in task.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_alphanumeric() || c == '_' {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.truncate(50);
    slug.trim_end_matches('-').to_owned()
}

fn branch_workspace_slug(branch: &str) -> String {
    let mut slug = String::new();
    for character in branch
        .chars()
        .map(|character| character.to_ascii_lowercase())
    {
        if character.is_ascii_alphanumeric() || character == '_' {
            slug.push(character);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.truncate(60);
    match slug.trim_end_matches('-') {
        "" => "workspace".into(),
        slug => slug.into(),
    }
}

fn preview_branch_workspace_path(
    project: &crate::protocol::ProjectRecord,
    branch: &str,
    registry: &crate::protocol::Registry,
) -> PathBuf {
    let slug = branch_workspace_slug(branch);
    for suffix in 1.. {
        let folder = if suffix == 1 {
            slug.clone()
        } else {
            format!("{slug}-{suffix}")
        };
        let candidate = project.workspace_root.join(folder);
        let candidate_text = candidate.to_string_lossy();
        let registered = registry.projects.iter().any(|project| {
            project.workspaces.iter().any(|workspace| {
                workspace
                    .path
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&candidate_text)
            })
        });
        if !candidate.exists() && !registered {
            return candidate;
        }
    }
    unreachable!("finite workspace collisions")
}

fn compact_branch_workspace_path(project: &crate::protocol::ProjectRecord, path: &Path) -> String {
    let root = project
        .workspace_root
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let folder = path.file_name().unwrap_or_default().to_string_lossy();
    format!("{root}/{folder}")
}

/// The recovery-safe launch and remembered choice: the agent or Terminal,
/// never its one-off first message.
fn remember_launch(
    palette: &mut Palette,
    launch: ovrcr_protocol::CreateAgentSessionRequest,
    task: &str,
) {
    palette.launch_preference = Some((
        launch.session.project.clone(),
        match &launch.session.label {
            Some(label) => super::settings::LaunchChoice::Agent {
                preset: label.clone(),
                model_policy: launch.model_policy.clone(),
            },
            None => super::settings::LaunchChoice::Terminal,
        },
    ));
    let mut launch = launch;
    if !task.is_empty()
        && launch
            .session
            .label
            .as_deref()
            .is_some_and(super::agents::takes_task)
        && launch.session.argv.len() >= 2
        && launch.session.argv[launch.session.argv.len() - 2] == "--"
        && launch.session.argv.last().is_some_and(|arg| arg == task)
    {
        // Keep explicit recovery retries from retaining the one-off first message.
        let args_without_task = launch.session.argv.len() - 2;
        launch.session.argv.truncate(args_without_task);
    }
    palette.failed_launch = Some(launch);
}

fn ensure_workspace_id(palette: &mut Palette) -> Result<String, String> {
    if let Some(id) = &palette.workspace_id {
        return Ok(id.clone());
    }
    match new_workspace_id() {
        Ok(id) => {
            palette.workspace_id = Some(id.clone());
            Ok(id)
        }
        Err(error) => Err(format!("Could not allocate workspace identity: {error}")),
    }
}
fn pick_field(label: &'static str, value: String, items: Vec<PickItem>) -> Field {
    let mut list = PickList::new(items);
    if !list.items.iter().any(|item| item.value == value) {
        list.query = value.clone();
    } else {
        list.select_value(&value);
    }
    let mut field = text_field(label, value, true);
    field.kind = FieldKind::Pick(list);
    field
}

fn set_model_options(
    fields: &mut [Field],
    capabilities: Option<&crate::protocol::AgentModelCapabilities>,
) {
    let Some(field) = fields.iter_mut().find(|field| field.label == "Model") else {
        return;
    };
    let selected = field.value.clone();
    let query = match &field.kind {
        FieldKind::Pick(list) => list.query.clone(),
        _ => String::new(),
    };
    let mut items = vec![PickItem {
        label: "Harness default".into(),
        value: HARNESS_DEFAULT_MODEL.into(),
    }];
    if let Some(capabilities) = capabilities
        && capabilities.binding.is_some()
        && capabilities.reason.is_none()
    {
        items.extend(capabilities.models.iter().map(|model| PickItem {
            label: if model.alias == model.resolved {
                model.alias.clone()
            } else {
                format!("{} · {}", model.alias, model.resolved)
            },
            value: model.alias.clone(),
        }));
    }
    let mut list = PickList::new(items);
    list.query = query.clone();
    if list.filtered().iter().any(|item| item.value == selected) {
        list.select_value(&selected);
    } else if query.is_empty() {
        list.query = selected.clone();
    }
    // Refreshing options must never accept a different model on the user's behalf.
    field.value = selected;
    field.kind = FieldKind::Pick(list);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModelQueryTarget {
    preset: String,
    origin: AgentOrigin,
    argv: Vec<OsString>,
    project: String,
    workspace: String,
    identity: String,
}

#[cfg(test)]
fn model_query_target(
    command: &Command,
    fields: &[Field],
    agents: &[super::agents::AgentEntry],
) -> Result<Option<ModelQueryTarget>, String> {
    model_query_target_with_branch_fallback(command, fields, agents, None)
}

fn model_query_target_with_branch_fallback(
    command: &Command,
    fields: &[Field],
    agents: &[super::agents::AgentEntry],
    branch_fallback: Option<&str>,
) -> Result<Option<ModelQueryTarget>, String> {
    if !matches!(command, Command::NewAgentSession) && field_value(fields, "Start") != "Agent" {
        return Ok(None);
    }
    let preset = agent_field_value(fields);
    if preset.is_empty() {
        return Err("Choose an agent harness to check its model support.".into());
    }
    if !matches!(command, Command::CreateTerminal | Command::NewAgentSession) {
        return Err("Explicit models require an already registered workspace.".into());
    }
    let mut target = if matches!(command, Command::NewAgentSession) {
        fields
            .iter()
            .find(|field| field.label == "Branch")
            .and_then(|field| match &field.kind {
                FieldKind::Pick(list) if !list.query.is_empty() => {
                    list.accepted().map(|item| item.value.clone())
                }
                _ => Some(field.value.clone()),
            })
            .unwrap_or_default()
    } else {
        field_value(fields, "Workspace")
    };
    if created_branch_name(&target).is_some() {
        if field_value(fields, "Model") == HARNESS_DEFAULT_MODEL {
            return Ok(None);
        }
        target = branch_fallback
            .filter(|fallback| !fallback.is_empty())
            .ok_or_else(|| {
                "Create the branch before checking explicit models for its workspace.".to_string()
            })?
            .to_owned();
    }
    let (mut project, workspace) = split_workspace_pick(&target);
    if matches!(command, Command::NewAgentSession) {
        let selected_project = field_value(fields, "Project");
        if project != selected_project {
            return Err("Select a branch from the chosen project to check explicit models.".into());
        }
        project = selected_project;
    }
    if project.is_empty() || workspace.is_empty() {
        return Err("Select a registered workspace to check explicit models.".into());
    }
    let Some(entry) = agents.iter().find(|entry| entry.name == preset) else {
        return Err(unavailable_agent(&preset));
    };
    match entry.origin {
        AgentOrigin::Configured => {
            return Err(
                "This named launcher is configured; its arguments and model policy are read-only."
                    .into(),
            );
        }
        AgentOrigin::Cloud => {
            return Err(
                "This cloud account has a pinned model and does not support overrides.".into(),
            );
        }
        AgentOrigin::Detected => {}
    }
    if !matches!(preset.as_str(), "claude" | "pi" | "codex" | "omp") {
        return Err(
            "Verified explicit model selection is not available for this harness yet.".into(),
        );
    }
    let identity = format!("{preset}|{project}|{workspace}|{entry:?}");
    Ok(Some(ModelQueryTarget {
        preset,
        origin: entry.origin,
        argv: entry.argv.clone(),
        project,
        workspace,
        identity,
    }))
}

fn resolve_agent_launcher(
    server_executable: Option<PathBuf>,
    current_executable: Option<PathBuf>,
    current_directory: Option<PathBuf>,
) -> Option<PathBuf> {
    let path = server_executable.or(current_executable)?;
    if path.is_absolute() {
        Some(path)
    } else {
        Some(current_directory?.join(path))
    }
}

fn visible_indices(fields: &[Field]) -> Vec<usize> {
    fields
        .iter()
        .enumerate()
        .filter(|(_, field)| !field.hidden && !matches!(&field.kind, FieldKind::Notice))
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

fn compact_picker_field(command: &Command, field: &Field) -> bool {
    matches!(field.kind, FieldKind::Pick(_))
        && (matches!(command, Command::NewAgentSession)
            && matches!(
                field.label,
                "Project" | "Branch" | "Agent harness" | "Model"
            )
            || matches!(command, Command::CreateBranch) && field.label == "Project")
}

fn committed_parent_branch_target(command: &Command, field: &Field) -> Option<(String, bool)> {
    (matches!(command, Command::NewAgentSession)
        && field.label == "Branch"
        && !field.value.is_empty()
        && created_branch_name(&field.value).is_none())
    .then(|| (field.value.clone(), field.edited))
}

fn creation_form_below_minimum(page: &Page, area: Rect) -> bool {
    matches!(
        page,
        Page::Form {
            command: Command::NewAgentSession | Command::CreateBranch,
            ..
        }
    ) && (area.width < 60 || area.height < 18)
}

fn reset_compact_picker(field: &mut Field) {
    let value = field.value.clone();
    if let FieldKind::Pick(list) = &mut field.kind {
        list.query.clear();
        list.select_value(&value);
    }
}

fn field_pair(fields: &[Field], width: u16) -> Option<(usize, usize)> {
    if width < 56 {
        return None;
    }
    let harness = fields
        .iter()
        .position(|field| field.label == "Agent harness")?;
    let model = fields.iter().position(|field| field.label == "Model")?;
    (!fields[harness].hidden && !fields[model].hidden).then_some((harness, model))
}

fn pair_columns(body: Rect) -> (u16, u16, u16) {
    let gap = 2.min(body.width);
    let columns = body.width.saturating_sub(gap);
    let left = columns / 2;
    let right = columns.saturating_sub(left);
    (left, gap, right)
}

fn creation_prompt_visible_rows(width: u16) -> usize {
    if width >= 56 { 3 } else { 1 }
}

fn creation_prompt_first_line(total_rows: usize, visible_rows: usize, cursor_line: usize) -> usize {
    cursor_line
        .saturating_sub(visible_rows.saturating_sub(1))
        .min(total_rows.saturating_sub(visible_rows))
}

fn creation_label_style(active: bool) -> Style {
    let mut style = Style::default().fg(if active { MAUVE() } else { SUBTEXT() });
    if active {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

fn creation_field_label(label: &str, _width: u16, active: bool) -> Line<'static> {
    if let Some(name) = label.strip_suffix(" (optional)") {
        Line::from(vec![
            Span::styled(name.to_owned(), creation_label_style(active)),
            Span::styled(" (optional)", Style::default().fg(MUTED())),
        ])
    } else {
        Line::styled(label.to_owned(), creation_label_style(active))
    }
}

fn creation_surface_spans(
    text: &str,
    width: u16,
    active: bool,
    placeholder: bool,
) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let background = if active { SURFACE2() } else { SURFACE0() };
    let style = Style::default()
        .fg(if placeholder { MUTED() } else { TEXT() })
        .bg(background);
    let marker_style = Style::default()
        .fg(if active { MAUVE() } else { SUBTEXT() })
        .bg(background);
    let marker = if active { "› " } else { "  " };
    let marker_width = UnicodeWidthStr::width(marker).min(usize::from(width));
    let content_width = usize::from(width).saturating_sub(marker_width);
    let content = clip_text(text, content_width);
    let content_width = UnicodeWidthStr::width(content.as_str()).min(content_width);
    let padding = usize::from(width).saturating_sub(marker_width + content_width);
    vec![
        Span::styled(marker.to_owned(), marker_style),
        Span::styled(content, style),
        Span::styled(" ".repeat(padding), style),
    ]
}

fn creation_surface_line(text: &str, width: u16, active: bool, placeholder: bool) -> Line<'static> {
    Line::from(creation_surface_spans(text, width, active, placeholder))
}

fn creation_inline_surface_line(
    label: &str,
    value: &str,
    width: u16,
    active: bool,
    placeholder: bool,
) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }
    let background = if active { SURFACE2() } else { SURFACE0() };
    let value_style = Style::default()
        .fg(if placeholder { MUTED() } else { TEXT() })
        .bg(background);
    let marker = if active { "› " } else { "  " };
    let marker_width = UnicodeWidthStr::width(marker).min(usize::from(width));
    let label_text = format!("{label}: ");
    let label_width = UnicodeWidthStr::width(label_text.as_str())
        .min(usize::from(width).saturating_sub(marker_width));
    let remaining = usize::from(width).saturating_sub(marker_width + label_width);
    let label_text = clip_text(&label_text, label_width);
    let value = clip_text(value, remaining);
    let used = label_width + UnicodeWidthStr::width(value.as_str()).min(remaining);
    let padding = usize::from(width).saturating_sub(marker_width + used);
    Line::from(vec![
        Span::styled(
            marker.to_owned(),
            Style::default()
                .fg(if active { MAUVE() } else { SUBTEXT() })
                .bg(background),
        ),
        Span::styled(label_text, Style::default().fg(SUBTEXT()).bg(background)),
        Span::styled(value, value_style),
        Span::styled(" ".repeat(padding), Style::default().bg(background)),
    ])
}

fn creation_pair_labels(
    left: &str,
    right: &str,
    left_width: u16,
    gap: u16,
    right_width: u16,
    left_active: bool,
    right_active: bool,
) -> Line<'static> {
    let mut spans = creation_label_spans(left, left_width, left_active);
    spans.push(Span::raw(" ".repeat(usize::from(gap))));
    spans.extend(creation_label_spans(right, right_width, right_active));
    Line::from(spans)
}

fn creation_label_spans(label: &str, width: u16, active: bool) -> Vec<Span<'static>> {
    let text = clip_text(label, usize::from(width));
    let padding = usize::from(width).saturating_sub(UnicodeWidthStr::width(text.as_str()));
    vec![
        Span::styled(text, creation_label_style(active)),
        Span::styled(" ".repeat(padding), Style::default()),
    ]
}

struct PairedFieldSurface<'a> {
    text: &'a str,
    active: bool,
    placeholder: bool,
    width: u16,
}

fn creation_pair_surfaces(
    left: PairedFieldSurface<'_>,
    gap: u16,
    right: PairedFieldSurface<'_>,
) -> Line<'static> {
    let mut spans = creation_surface_spans(left.text, left.width, left.active, left.placeholder);
    spans.push(Span::styled(
        " ".repeat(usize::from(gap)),
        Style::default().bg(CRUST()),
    ));
    spans.extend(creation_surface_spans(
        right.text,
        right.width,
        right.active,
        right.placeholder,
    ));
    Line::from(spans)
}

fn creation_field_display(
    field: &Field,
    index: usize,
    active: usize,
    palette: &Palette,
    width: u16,
) -> (String, bool) {
    if field.label == "Branch name" && field.value.is_empty() {
        return ("feature/auth-refresh".into(), true);
    }
    let editing = match &field.kind {
        FieldKind::Pick(list) if index == active && !list.query.is_empty() => list.query.as_str(),
        FieldKind::Pick(list) => list
            .items
            .iter()
            .find(|item| item.value == field.value)
            .map(|item| item.label.as_str())
            .unwrap_or(field.value.as_str()),
        _ => field.value.as_str(),
    };
    let cursor = if index == active {
        palette.cursor
    } else {
        Default::default()
    };
    let (mut display, _) = cursor.display(editing, usize::from(width.saturating_sub(2)));
    if index != active {
        display.pop();
    }
    (display, false)
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

/// A pick field's value is its highlighted option: arrows and typing change it
/// at once. False when a filter leaves nothing highlighted.
fn accept_pick(field: &mut Field) -> bool {
    let FieldKind::Pick(list) = &field.kind else {
        return true;
    };
    let Some(item) = list.accepted() else {
        return false;
    };
    field.edited |= field.value != item.value;
    field.value = item.value.clone();
    true
}

impl Dashboard {
    pub(super) fn open_agent_search(&mut self) -> DashboardAction {
        self.agent_typing = None;
        self.cancel_mouse_gesture();
        self.whichkey = None;
        self.mode = InputMode::Browse;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        let current = self.focused_session();
        let mut candidates = self
            .hierarchy_rows(true)
            .into_iter()
            .filter_map(|row| {
                let super::TreeRow::Session { id } = row else {
                    return None;
                };
                let session = find_session(self, id)?;
                (Some(id) != current && running_agent(session)).then(|| AgentCandidate {
                    session: session.clone(),
                    search_identity: format!(
                        "{} / {} (#{}) / {}",
                        session_workspace_heading(self, session),
                        session.display_name(),
                        session.id.0,
                        match &session.kind {
                            SessionKind::Agent { name } => name.as_str(),
                            SessionKind::Terminal => session.label.as_str(),
                        }
                    ),
                    unavailable: None,
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|candidate| {
            if super::ready::activity(&candidate.session)
                == crate::protocol::AgentActivity::WaitingInput
            {
                0
            } else if candidate.session.unread.is_some() {
                1
            } else {
                2
            }
        });
        let mut palette = Palette::new();
        palette.agents = Some(candidates);
        self.palette = Some(palette);
        DashboardAction::Redraw
    }

    pub(super) fn refresh_agent_search(&mut self) {
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        if let Some(candidates) = &mut palette.agents {
            for candidate in candidates {
                candidate.unavailable = candidate.reason(self);
            }
        }
        self.palette = Some(palette);
    }

    fn entry_session<'a>(
        &'a self,
        command: &Command,
        palette: &'a Palette,
    ) -> Option<&'a SessionSummary> {
        match *command {
            Command::SwitchAgent(id, run) => {
                find_session(self, id).filter(|s| s.run == run).or_else(|| {
                    palette
                        .agents
                        .as_ref()?
                        .iter()
                        .find(|c| c.session.id == id)
                        .map(|c| &c.session)
                })
            }
            _ => None,
        }
    }

    pub(super) fn open_palette(&mut self) -> DashboardAction {
        self.agent_typing = None;
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
        self.enqueue_request(ClientMessage {
            request_id,
            request: Request::Inspect,
        });
        DashboardAction::Redraw
    }

    /// Split the focused pane, then let the
    /// user choose its session. Enter places the highlighted one; Esc keeps
    /// the suggestion.
    pub(super) fn open_split_chooser(
        &mut self,
        direction: super::SplitDirection,
    ) -> DashboardAction {
        if !self.split_pane_in(direction) {
            return DashboardAction::Redraw;
        }
        let suggestion = self.focused_session();
        let selected = self
            .split_session_entries()
            .iter()
            .position(
                |entry| matches!(entry.command, Command::Switch(id) if Some(id) == suggestion),
            )
            .unwrap_or(0);
        let mut palette = Palette::new();
        palette.split = true;
        palette.page = Page::Search {
            query: String::new(),
            selected,
        };
        self.palette = Some(palette);
        DashboardAction::Redraw
    }

    fn split_session_entries(&self) -> Vec<Entry> {
        let focused = self.focused_session();
        self.session_entries(None)
            .into_iter()
            .filter(|entry| match &entry.command {
                Command::Switch(id) => {
                    Some(*id) == focused
                        || !self.panes.iter().enumerate().any(|(index, pane)| {
                            index != self.focused_pane && pane.session == Some(*id)
                        })
                }
                _ => true,
            })
            .collect()
    }

    /// One `project / branch / title` entry per session. The ID is added only
    /// when two entries would otherwise read the same.
    fn session_entries(&self, exclude: Option<SessionId>) -> Vec<Entry> {
        let mut entries: Vec<Entry> = self
            .hierarchy
            .projects
            .iter()
            .flat_map(|project| &project.workspaces)
            .flat_map(|workspace| workspace.sessions.iter().map(move |s| (workspace, s)))
            .filter(|(_, session)| Some(session.id) != exclude)
            .map(|(workspace, session)| Entry {
                label: format!(
                    "{} / {}",
                    workspace_heading(self, workspace),
                    session.display_name()
                ),
                command: Command::Switch(session.id),
            })
            .collect();
        let labels: Vec<String> = entries.iter().map(|entry| entry.label.clone()).collect();
        for entry in &mut entries {
            if let Command::Switch(id) = entry.command
                && labels.iter().filter(|label| **label == entry.label).count() > 1
            {
                entry.label.push_str(&format!(" (#{})", id.0));
            }
        }
        entries
    }

    pub(super) fn open_create_terminal(&mut self) -> DashboardAction {
        self.open_terminal_form(None)
    }

    pub(super) fn open_new_terminal(&mut self) -> DashboardAction {
        self.open_terminal_form(Some("Terminal"))
    }

    fn open_terminal_form(&mut self, start: Option<&str>) -> DashboardAction {
        if let Some(warning) = self
            .current_workspace()
            .and_then(|workspace| workspace.warning.clone())
        {
            self.set_error(warning);
            return DashboardAction::Redraw;
        }
        self.cancel_mouse_gesture();
        self.whichkey = None;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        let mut palette = Palette {
            page: self.palette_form(Command::CreateTerminal),
            ..Palette::new()
        };
        if let Some(start) = start
            && let Page::Form { fields, active, .. } = &mut palette.page
            && let Some(start_index) = fields.iter().position(|field| field.label == "Start")
        {
            fields[start_index].value = start.into();
            fields[start_index].edited = true;
            if let FieldKind::Pick(list) = &mut fields[start_index].kind {
                list.query.clear();
                list.select_value(start);
            }
            self.refresh_terminal_form(fields, false);
            *active = fields
                .iter()
                .position(|field| field.label == "Workspace" && !field.hidden)
                .or_else(|| fields.iter().position(|field| !field.hidden))
                .unwrap_or(start_index);
        }
        self.refresh_model_query(&mut palette);
        self.palette = Some(palette);
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn open_new_agent_session(&mut self) -> DashboardAction {
        self.agent_typing = None;
        self.cancel_mouse_gesture();
        self.whichkey = None;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.mode = InputMode::Browse;

        let mut palette = Palette::new();
        let request_id = self.next_request_id();
        palette.suggestions.inspect = Some(request_id);
        palette.agent_catalog = Some(self.detected_agents());
        self.begin_new_agent_session_form(&mut palette);
        self.palette = Some(palette);
        self.enqueue_request(ClientMessage {
            request_id,
            request: Request::Inspect,
        });
        let list_request_id = self.next_request_id();
        self.enqueue_request(ClientMessage {
            request_id: list_request_id,
            request: Request::List,
        });
        DashboardAction::Redraw
    }

    fn begin_new_agent_session_form(&mut self, palette: &mut Palette) {
        palette.new_agent_picker_open = false;
        if palette.agent_catalog.is_none() {
            palette.agent_catalog = Some(self.detected_agents());
        }
        let project = self.creation_context().0;
        palette.launch_project = Some(project.clone());
        palette.page = self.new_agent_session_form(
            &project,
            palette.agent_catalog.as_deref().unwrap_or_default(),
        );
        if let Page::Form { fields, .. } = &palette.page {
            palette.launch_harness = Some(agent_field_value(fields));
        }
        self.refresh_workspace_form(palette);
        self.refresh_model_query(palette);
        palette.parent_branch_target = match &palette.page {
            Page::Form {
                command, fields, ..
            } => fields
                .iter()
                .find(|field| field.label == "Branch")
                .and_then(|field| committed_parent_branch_target(command, field)),
            _ => None,
        };
    }

    pub(super) fn open_create_workspace(&mut self) -> DashboardAction {
        let action = self.open_palette();
        let project = self.creation_context().0;
        let page = self.create_branch_form(&project);
        let mut palette = self.palette.take().unwrap();
        palette.page = page;
        palette.workspace_id = None;
        palette.workspace_acknowledged = false;
        palette.branch_attempt = None;
        palette.branch_project = Some(project);
        self.refresh_workspace_form(&mut palette);
        self.palette = Some(palette);
        action
    }

    fn create_branch_form(&self, preferred_project: &str) -> Page {
        let project_list = PickList::projects(&self.hierarchy, preferred_project);
        let mut project = text_field(
            "Project",
            project_list
                .accepted()
                .map(|item| item.value.clone())
                .unwrap_or_default(),
            true,
        );
        project.kind = FieldKind::Pick(project_list);
        let branch = text_field("Branch name", String::new(), true);
        let mut options = text_field("Workspace options", "collapsed".into(), false);
        options.kind = FieldKind::Toggle;
        let mut base = text_field("Base", String::new(), true);
        base.hidden = true;
        Page::Form {
            command: Command::CreateBranch,
            fields: vec![project, branch, options, base],
            active: 1,
            name_edited: false,
            root_edited: false,
        }
    }

    fn refresh_workspace_form(&self, palette: &mut Palette) {
        if matches!(
            &palette.page,
            Page::Form {
                command: Command::CreateBranch,
                ..
            }
        ) {
            self.refresh_create_branch_form(palette);
            return;
        }
        let hints = &mut palette.suggestions;
        if let Some((project, result)) = hints.worker.as_ref().and_then(|worker| worker.poll()) {
            hints.cache.insert(project, result);
            hints.in_flight = None;
        }
        if matches!(
            &palette.page,
            Page::Form {
                command: Command::NewAgentSession,
                ..
            }
        ) {
            self.refresh_new_agent_session_form(palette);
            return;
        } else if let Page::Form {
            fields, command, ..
        } = &mut palette.page
        {
            let project = match command {
                Command::CreateTerminal => fields
                    .iter()
                    .find(|field| field.label == "Workspace")
                    .map(|field| split_workspace_pick(&field.value).0),
                Command::CreateWorkspace => fields
                    .iter()
                    .find(|field| field.label == "Project")
                    .map(|field| field.value.clone()),
                _ => None,
            };
            if let Some(project) = project {
                if palette
                    .launch_project
                    .as_ref()
                    .is_some_and(|old| old != &project)
                {
                    for replacement in
                        self.launch_fields(&project, matches!(command, Command::CreateWorkspace))
                    {
                        if let Some(field) =
                            fields.iter_mut().find(|f| f.label == replacement.label)
                        {
                            *field = replacement;
                        }
                    }
                }
                palette.launch_project = Some(project);
            }
            self.refresh_terminal_form(fields, false);
        }
        let Page::Form {
            command: Command::CreateWorkspace,
            fields,
            ..
        } = &mut palette.page
        else {
            return;
        };
        let project = fields
            .iter()
            .find(|field| field.label == "Project")
            .map(|field| field.value.clone())
            .unwrap_or_default();
        let existing = fields
            .iter()
            .find(|field| field.label == "Branch mode")
            .is_some_and(|field| field.value == "existing");
        let Some(branch_i) = fields.iter().position(|field| field.label == "Branch") else {
            return;
        };
        let Some(base_i) = fields.iter().position(|field| field.label == "Base") else {
            return;
        };
        fields[base_i].hidden = existing;
        fields[base_i].required = !existing;
        if hints.project.as_ref() != Some(&project) {
            hints.project = Some(project.clone());
            fields[branch_i].kind = FieldKind::Text;
            if existing {
                fields[branch_i].value.clear();
            }
        }
        if !existing && !fields[branch_i].edited {
            let task = form_task(fields);
            fields[branch_i].value = format!("{}{}", self.settings.branch_prefix, task_slug(&task));
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
                if !fields[base_i].edited {
                    fields[base_i].value.clone_from(&result.base);
                }
                if existing && !matches!(fields[branch_i].kind, FieldKind::Pick(_)) {
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
                    if fields[branch_i].value.is_empty() {
                        fields[branch_i].value =
                            result.branches.first().cloned().unwrap_or_default();
                    }
                    if result.branches.contains(&fields[branch_i].value) {
                        list.select_value(&fields[branch_i].value);
                    } else {
                        list.query.clone_from(&fields[branch_i].value);
                    }
                    if !result.branches.is_empty() {
                        fields[branch_i].kind = FieldKind::Pick(list);
                    }
                }
            }
            Some(Err(error)) => {
                hints.note = Some(format!("{error}; enter branch/base manually"));
                if !fields[base_i].edited {
                    fields[base_i].value = "main".into();
                }
            }
            None => {
                hints.note = Some("Loading Git suggestions…".into());
                if !fields[base_i].edited {
                    fields[base_i].value = "main".into();
                }
            }
        }
    }

    fn refresh_create_branch_form(&self, palette: &mut Palette) {
        let project_name = match &palette.page {
            Page::Form { fields, .. } => fields
                .iter()
                .find(|field| field.label == "Project")
                .map(|field| field.value.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let registered_project = palette
            .workspace_inventory
            .as_ref()
            .is_some_and(|registry| {
                registry
                    .projects
                    .iter()
                    .any(|item| item.name == project_name)
            });
        let inspected = palette.workspace_inventory.is_some();
        let hints = &mut palette.suggestions;
        if let Some((project, result)) = hints.worker.as_ref().and_then(|worker| worker.poll()) {
            hints.cache.insert(project, result);
            hints.in_flight = None;
        }
        if let Some(registry) = palette.workspace_inventory.as_ref()
            && let Page::Form { fields, .. } = &mut palette.page
            && let Some(project) = fields.iter_mut().find(|field| field.label == "Project")
        {
            let current = project.value.clone();
            let mut list = PickList::new(
                registry
                    .projects
                    .iter()
                    .map(|record| PickItem {
                        label: record.name.clone(),
                        value: record.name.clone(),
                    })
                    .collect(),
            );
            if list.items.iter().any(|item| item.value == current) {
                list.select_value(&current);
            } else if let Some(preferred) = palette
                .branch_project
                .as_deref()
                .filter(|preferred| list.items.iter().any(|item| item.value == *preferred))
            {
                list.select_value(preferred);
            }
            project.value = list
                .accepted()
                .map(|item| item.value.clone())
                .unwrap_or_default();
            project.kind = FieldKind::Pick(list);
        }
        let Page::Form { fields, .. } = &mut palette.page else {
            return;
        };
        let project = fields
            .iter()
            .find(|field| field.label == "Project")
            .map(|field| field.value.clone())
            .unwrap_or_default();
        let changed = hints.project.as_ref() != Some(&project);
        if changed {
            hints.project = Some(project.clone());
            if let Some(base) = fields.iter_mut().find(|field| field.label == "Base")
                && !base.edited
            {
                base.value.clear();
            }
        }
        palette.branch_project = (!project.is_empty()).then_some(project.clone());
        if !project.is_empty()
            && hints.inspect.is_none()
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
            } else if registered_project {
                hints
                    .cache
                    .insert(project.clone(), Err("Repository path unavailable".into()));
            } else if inspected {
                hints.cache.insert(
                    project.clone(),
                    Err("Project is no longer registered".into()),
                );
            }
        }
        hints.note = None;
        match hints.cache.get(&project) {
            Some(Ok(result)) => {
                if let Some(base) = fields.iter_mut().find(|field| field.label == "Base")
                    && !base.edited
                {
                    base.value.clone_from(&result.base);
                }
            }
            Some(Err(error)) => {
                hints.note = Some(format!(
                    "Could not detect the repository default ({error}); enter Base in Workspace options."
                ));
            }
            None if !project.is_empty() => {
                hints.note = Some("Loading the repository default branch…".into());
            }
            None => {
                hints.note = Some("Choose a project to load its default branch.".into());
            }
        }
        if let Some(options) = fields
            .iter()
            .find(|field| field.label == "Workspace options")
        {
            let expanded = options.value == "expanded";
            if let Some(base) = fields.iter_mut().find(|field| field.label == "Base") {
                base.hidden = !expanded;
            }
        }
    }

    fn refresh_new_agent_session_form(&self, palette: &mut Palette) {
        let Some(registry) = palette.workspace_inventory.as_ref() else {
            if let Page::Form { fields, .. } = &mut palette.page
                && let Some(destination) =
                    fields.iter_mut().find(|field| field.label == "Destination")
            {
                destination.value = "Loading registered workspaces…".into();
            }
            return;
        };
        let catalog = palette.agent_catalog.clone().unwrap_or_default();
        let Page::Form { fields, active, .. } = &mut palette.page else {
            return;
        };
        let Some(project_index) = fields.iter().position(|field| field.label == "Project") else {
            return;
        };
        let Some(branch_index) = fields.iter().position(|field| field.label == "Branch") else {
            return;
        };
        let current_context = self.creation_context();
        let project_query = match &fields[project_index].kind {
            FieldKind::Pick(list) => list.query.clone(),
            _ => String::new(),
        };
        let current_project = fields[project_index].value.clone();
        let selected_project = if registry
            .projects
            .iter()
            .any(|project| project.name == current_project)
        {
            current_project.clone()
        } else if registry
            .projects
            .iter()
            .any(|project| project.name == current_context.0)
        {
            current_context.0.clone()
        } else {
            registry
                .projects
                .first()
                .map(|project| project.name.clone())
                .unwrap_or_default()
        };
        let mut project_list = PickList::new(
            registry
                .projects
                .iter()
                .map(|project| PickItem {
                    label: project.name.clone(),
                    value: project.name.clone(),
                })
                .collect(),
        );
        project_list.query = project_query;
        project_list.select_value(&selected_project);
        fields[project_index].value.clone_from(&selected_project);
        fields[project_index].kind = FieldKind::Pick(project_list);

        let previous_launch_project = palette.launch_project.clone();
        let project_changed = previous_launch_project.as_deref() != Some(selected_project.as_str());
        let base_index = fields.iter().position(|field| field.label == "Base");
        let git_project_changed =
            palette.suggestions.project.as_deref() != Some(selected_project.as_str());
        if git_project_changed {
            palette.suggestions.project = Some(selected_project.clone());
            if let Some(base_index) = base_index
                && !fields[base_index].edited
            {
                fields[base_index].value.clear();
            }
        }
        if !selected_project.is_empty()
            && palette.suggestions.inspect.is_none()
            && !palette.suggestions.cache.contains_key(&selected_project)
            && palette.suggestions.in_flight.is_none()
        {
            if let Some(repo) = palette.suggestions.repos.get(&selected_project) {
                if palette.suggestions.worker.is_none() {
                    palette.suggestions.worker = super::git_hints::Worker::start().ok();
                }
                if palette
                    .suggestions
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.request(selected_project.clone(), repo.clone()))
                {
                    palette.suggestions.in_flight = Some(selected_project.clone());
                } else if palette.suggestions.worker.is_some() {
                    palette.suggestions.cache.insert(
                        selected_project.clone(),
                        Err("Could not start Git lookup".into()),
                    );
                }
            } else if palette.workspace_inventory.is_some() {
                palette.suggestions.cache.insert(
                    selected_project.clone(),
                    Err("Repository path unavailable".into()),
                );
            }
        }
        let repository_branches = palette
            .suggestions
            .cache
            .get(&selected_project)
            .and_then(|result| result.as_ref().ok())
            .map(|hints| hints.branches.clone())
            .unwrap_or_default();
        if let Some(base_index) = base_index {
            if let Some(options) = fields
                .iter()
                .find(|field| field.label == "Advanced options")
            {
                fields[base_index].hidden = options.value != "expanded";
            }
            if !fields[base_index].edited {
                fields[base_index].value = palette
                    .suggestions
                    .cache
                    .get(&selected_project)
                    .and_then(|result| result.as_ref().ok())
                    .map(|hints| hints.base.clone())
                    .unwrap_or_default();
            }
        }

        let current_query = match &fields[branch_index].kind {
            FieldKind::Pick(list) => list.query.clone(),
            _ => String::new(),
        };
        let branch_was_edited =
            (fields[branch_index].edited || !current_query.is_empty()) && !project_changed;
        let current_query = if project_changed {
            fields[branch_index].value.clear();
            fields[branch_index].edited = false;
            String::new()
        } else {
            current_query
        };
        let current_value = fields[branch_index].value.clone();
        let preferred = if current_value.is_empty()
            && !branch_was_edited
            && selected_project == current_context.0
        {
            registry
                .projects
                .iter()
                .find(|project| project.name == selected_project)
                .and_then(|project| {
                    project
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.id == current_context.1)
                })
                .map(|record| super::picker::workspace_pick_value(&current_context.0, &record.id))
                .unwrap_or_default()
        } else {
            current_value.clone()
        };
        let mut items = Vec::new();
        let mut existing_branch_names = Vec::new();
        if let Some(project) = registry
            .projects
            .iter()
            .find(|project| project.name == selected_project)
        {
            for record in &project.workspaces {
                let summary = find_workspace(self, &project.name, &record.id)
                    .filter(|summary| summary.path == record.path);
                let checkout_available =
                    summary.is_some_and(|workspace| workspace.name != "checkout unavailable");
                let branch_name = summary
                    .filter(|_| checkout_available)
                    .map(|workspace| workspace.name.as_str())
                    .unwrap_or(record.branch.as_str());
                existing_branch_names.push(branch_name.to_owned());
                let branch = summary
                    .filter(|_| checkout_available)
                    .map(|workspace| workspace.name.as_str())
                    .unwrap_or("checkout unavailable");
                let mut states = Vec::new();
                if record.setup_pending {
                    states.push("setup pending");
                }
                if !checkout_available {
                    states.push("unavailable");
                }
                if summary.is_some_and(|workspace| workspace.root && workspace.warning.is_some()) {
                    states.push("root · launch blocked");
                } else if summary.is_some_and(|workspace| workspace.root) {
                    states.push("root");
                }
                if branch.starts_with("detached @ ") {
                    states.push("detached");
                }
                let value = super::picker::workspace_pick_value(&project.name, &record.id);
                let label = if states.is_empty() {
                    branch.to_owned()
                } else {
                    format!("{branch} [{}]", states.join(" · "))
                };
                items.push(PickItem { label, value });
            }
        }
        let create_candidate = if current_query.is_empty() {
            created_branch_name(&current_value)
        } else {
            Some(current_query.as_str())
        };
        if let Some(create_candidate) = create_candidate.filter(|candidate| !candidate.is_empty())
            && !existing_branch_names
                .iter()
                .any(|branch| branch == create_candidate)
            && !repository_branches
                .iter()
                .any(|branch| branch == create_candidate)
        {
            items.push(PickItem {
                label: format!("Create branch \"{create_candidate}\""),
                value: create_branch_pick_value(create_candidate),
            });
        }
        let mut branch_list = PickList::new(items);
        if !current_query.is_empty() {
            branch_list.query = current_query.clone();
        }
        let selected_exists = branch_list.items.iter().any(|item| item.value == preferred);
        if selected_exists {
            branch_list.select_value(&preferred);
        }
        if let FieldKind::Pick(list) = &mut fields[branch_index].kind {
            *list = branch_list;
        } else {
            fields[branch_index].kind = FieldKind::Pick(branch_list);
        }
        if !preferred.is_empty() {
            fields[branch_index].value = preferred;
        } else if !branch_was_edited {
            fields[branch_index].value.clear();
        }
        let filtered_value = match &fields[branch_index].kind {
            FieldKind::Pick(list) if branch_was_edited && !list.query.is_empty() => {
                list.accepted().map(|item| item.value.clone())
            }
            _ => None,
        };
        if let Some(value) = filtered_value {
            fields[branch_index].value = value;
        } else if branch_was_edited && !current_query.is_empty() {
            fields[branch_index].value.clear();
        }

        let created_branch = created_branch_name(&fields[branch_index].value).map(str::to_owned);
        let create_new_branch = created_branch.is_some();
        let selected_workspace = if create_new_branch {
            String::new()
        } else {
            split_workspace_pick(&fields[branch_index].value).1
        };
        let selected_record = registry
            .projects
            .iter()
            .find(|project| project.name == selected_project)
            .and_then(|project| {
                project
                    .workspaces
                    .iter()
                    .find(|record| record.id == selected_workspace)
            });
        let destination_workspace = if selected_project.is_empty() || selected_workspace.is_empty()
        {
            None
        } else {
            find_workspace(self, &selected_project, &selected_workspace).filter(|workspace| {
                registry.projects.iter().any(|project| {
                    project.name == selected_project
                        && project.workspaces.iter().any(|record| {
                            record.id == selected_workspace && record.path == workspace.path
                        })
                })
            })
        };
        let destination_text = if let Some(branch) = created_branch.as_deref() {
            let base = fields
                .iter()
                .find(|field| field.label == "Base")
                .map(|field| field.value.trim())
                .unwrap_or_default();
            match (
                registry
                    .projects
                    .iter()
                    .find(|project| project.name == selected_project),
                base.is_empty(),
            ) {
                (Some(project), false) => compact_branch_workspace_path(
                    project,
                    &preview_branch_workspace_path(project, branch, registry),
                ),
                (None, false) => "Project is no longer registered.".into(),
                (_, true) => match palette.suggestions.cache.get(&selected_project) {
                    Some(Err(error)) => format!(
                        "Could not detect the default branch ({error}); expand Advanced options to set Base."
                    ),
                    _ => "Loading the repository default branch…".into(),
                },
            }
        } else {
            match (selected_record, destination_workspace) {
                (Some(record), _) if record.setup_pending => {
                    format!("{selected_project} / checkout setup pending")
                }
                (Some(_), None) => format!("{selected_project} / checkout unavailable"),
                (Some(_), Some(workspace)) if workspace.name == "checkout unavailable" => {
                    let state = if workspace.root && workspace.warning.is_some() {
                        " · root launch blocked"
                    } else if workspace.root {
                        " · root"
                    } else {
                        ""
                    };
                    format!("{} / checkout unavailable{state}", workspace.project)
                }
                (Some(_), Some(workspace)) => {
                    let state = if workspace.root && workspace.warning.is_some() {
                        " · root launch blocked"
                    } else if workspace.root {
                        " · root"
                    } else if workspace.name.starts_with("detached @ ") {
                        " · detached"
                    } else {
                        ""
                    };
                    format!("{} / {}{state}", workspace.project, workspace.name)
                }
                (None, _)
                    if fields[branch_index].value.is_empty()
                        && registry
                            .projects
                            .iter()
                            .all(|project| project.workspaces.is_empty()) =>
                {
                    "No registered workspaces. Cancel, then Register project (a).".into()
                }
                (None, _) if fields[branch_index].value.is_empty() => {
                    "Choose a registered checkout; the current target is not eligible.".into()
                }
                (None, _) => "This checkout is no longer registered; choose another.".into(),
            }
        };
        if let Some(destination) = fields.iter_mut().find(|field| field.label == "Destination") {
            destination.value = destination_text;
        }
        let branch_base_preview = if create_new_branch {
            let base = fields
                .iter()
                .find(|field| field.label == "Base")
                .map(|field| field.value.trim())
                .unwrap_or_default();
            if !base.is_empty() {
                format!("From {base}")
            } else if let Some(Err(error)) = palette.suggestions.cache.get(&selected_project) {
                format!("Default branch unavailable: {error}; set Base in Advanced options")
            } else {
                "Loading default branch…".into()
            }
        } else {
            String::new()
        };
        if let Some(preview) = fields.iter_mut().find(|field| field.label == "Branch base") {
            preview.hidden = !create_new_branch;
            preview.value = branch_base_preview;
        }
        let shared = destination_workspace
            .map(|workspace| {
                workspace
                    .sessions
                    .iter()
                    .filter(|session| running_agent(session))
                    .map(|session| session.display_name().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Some(notice) = fields
            .iter_mut()
            .find(|field| field.label == "Shared files")
        {
            notice.hidden = shared.is_empty();
            notice.value = if shared.is_empty() {
                String::new()
            } else {
                format!(
                    "{} running agent(s) share this checkout: {}",
                    shared.len(),
                    shared.join(", ")
                )
            };
        }

        let new_project = selected_project;
        let project_changed = palette.launch_project.as_deref() != Some(new_project.as_str());
        let catalog_items = catalog
            .iter()
            .filter(|agent| {
                agent.source != AgentSource::Shell
                    && agent.source != AgentSource::Custom
                    && agent.name != "shell"
                    && agent.name != "Custom"
            })
            .map(|agent| PickItem {
                label: agent.name.clone(),
                value: agent.name.clone(),
            })
            .collect::<Vec<_>>();
        if project_changed
            && let Some(harness) = fields
                .iter_mut()
                .find(|field| field.label == "Agent harness")
            && !harness.edited
        {
            let default = match self.settings.launch_choices.get(&new_project) {
                Some(super::settings::LaunchChoice::Agent { preset, .. }) => preset.clone(),
                _ => catalog_items
                    .first()
                    .map(|item| item.value.clone())
                    .unwrap_or_default(),
            };
            harness.value = default.clone();
            harness.kind = FieldKind::Pick(PickList::new(catalog_items).with_sections([]));
            if let FieldKind::Pick(list) = &mut harness.kind {
                list.select_value(&default);
            }
        }
        let current_harness = agent_field_value(fields);
        let harness_changed = palette.launch_harness.as_deref() != Some(current_harness.as_str());
        if (project_changed || harness_changed)
            && let Some(model) = fields.iter_mut().find(|field| field.label == "Model")
            && !model.edited
        {
            model.value = match self.settings.launch_choices.get(&new_project) {
                Some(super::settings::LaunchChoice::Agent {
                    preset,
                    model_policy: ovrcr_protocol::AgentModelPolicy::Explicit(model),
                }) if preset == &current_harness => model.clone(),
                _ => HARNESS_DEFAULT_MODEL.into(),
            };
            let mut list = PickList::new(vec![PickItem {
                label: "Harness default".into(),
                value: HARNESS_DEFAULT_MODEL.into(),
            }]);
            list.select_value(&model.value);
            model.kind = FieldKind::Pick(list);
        }
        palette.launch_project = Some(new_project);
        palette.launch_harness = Some(current_harness);
        if project_changed || palette.parent_branch_target.is_none() {
            palette.parent_branch_target =
                committed_parent_branch_target(&Command::NewAgentSession, &fields[branch_index]);
        }
        if *active >= fields.len() || fields[*active].hidden {
            *active = visible_indices(fields).first().copied().unwrap_or(0);
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
            }
            | Page::Form {
                command: Command::CreateBranch,
                fields,
                ..
            } => palette
                .suggestions
                .cache
                .contains_key(&field_value(fields, "Project")),
            Page::Form {
                command: Command::NewAgentSession,
                fields,
                ..
            } if created_branch_name(&field_value(fields, "Branch")).is_some() => palette
                .suggestions
                .cache
                .contains_key(&field_value(fields, "Project")),
            _ => false,
        };
        let mut submit = palette.submit_when_ready && ready;
        let mut refused = false;
        if submit {
            palette.submit_when_ready = false;
            // The hints arrived after the user pressed Enter, so they never saw
            // this list. Make them confirm anything but what they typed.
            if let Some(label) = unconfirmed_pick(&palette.page)
                && let Page::Form { active, .. } = &palette.page
            {
                palette.invalid = Some((*active, format!("{label} not found in repository")));
                submit = false;
                refused = true;
            }
        }
        self.palette = Some(palette);
        if submit {
            // Deferred submission was already queued before the writer scheduled History.
            self.palette_key_inner(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT), false);
            self.refresh_active_path_listing();
        }
        was_loading || submit || refused
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

    pub(super) fn close_confirm_session(&self) -> Option<SessionId> {
        self.palette
            .as_ref()
            .and_then(|palette| close_terminal_id(&palette.page))
    }

    pub(super) fn open_close_terminal(&mut self) -> DashboardAction {
        let Some(id) = self.action_session() else {
            return DashboardAction::None;
        };
        self.open_close_terminal_for(id)
    }

    /// Archive `id` without making it the focused pane's session.
    pub(super) fn open_close_terminal_for(&mut self, id: SessionId) -> DashboardAction {
        if find_session(self, id).is_none() {
            return DashboardAction::None;
        }
        self.cancel_mouse_gesture();
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.whichkey = None;
        let session = find_session(self, id).unwrap();
        let immediate = !session.phase.is_live() && session.close_warning().is_none();
        let run = session.run;
        let mut palette = Palette {
            page: self.command_page(Command::CloseTerminal(id)),
            ..Palette::new()
        };
        let action = if immediate {
            self.palette_submit(
                &mut palette,
                Request::CloseTerminal {
                    session: id,
                    expected_run: run,
                },
                true,
            )
        } else {
            DashboardAction::Redraw
        };
        self.palette = Some(palette);
        self.mode = InputMode::Browse;
        action
    }

    /// Enter on a session that is no longer live. Opens the same confirmation the
    /// palette's Resume / Reopen entry does, or the unavailable notice for a
    /// provider that has no native picker to fall back to.
    pub(super) fn open_reopen_terminal(&mut self) -> DashboardAction {
        let Some(session) = self
            .action_session()
            .and_then(|id| find_session(self, id))
            .filter(|session| !session.phase.is_live())
        else {
            return DashboardAction::None;
        };
        let unavailable = matches!(session.kind, SessionKind::Agent { .. })
            && !native_picker(session)
            && session
                .recovery
                .as_ref()
                .is_none_or(|recovery| recovery.unavailable.is_some());
        let command = if unavailable {
            Command::AgentResumeUnavailable(session.id)
        } else {
            Command::ReopenTerminal(session.id)
        };
        self.cancel_mouse_gesture();
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.whichkey = None;
        self.palette = Some(Palette {
            page: self.command_page(command),
            ..Palette::new()
        });
        self.mode = InputMode::Browse;
        DashboardAction::Redraw
    }

    pub(super) fn dismiss_close_confirm_for(&mut self, session: SessionId) {
        let matches_close = self
            .palette
            .as_ref()
            .and_then(|palette| confirmation_request(&palette.page))
            .is_some_and(|request| {
                matches!(request, Request::CloseTerminal { session: id, .. } if *id == session)
            });
        if matches_close {
            self.palette = None;
        }
    }

    pub(super) fn sync_palette_workspace_identity(&mut self) {
        let removal = match &self.palette {
            Some(palette) if palette.pending.is_some() => None,
            Some(Palette {
                page: Page::Confirm { request, target },
                ..
            }) => match request.as_deref() {
                Some(Request::RemoveWorkspace {
                    project,
                    name,
                    force,
                }) => Some((project.clone(), name.clone(), *force, target.clone())),
                _ => None,
            },
            _ => None,
        };
        if let Some((project, name, force, target)) = removal {
            // A new branch label or a different set of running sessions changes
            // what confirming would do, so it needs fresh consent.
            let stale = match find_workspace(self, &project, &name) {
                None => true,
                Some(workspace) => {
                    let expected = self.removal_page(Request::RemoveWorkspace {
                        project: project.clone(),
                        name: name.clone(),
                        force,
                    });
                    workspace.root
                        || !matches!(expected, Page::Confirm { target: current, .. } if current == target)
                }
            };
            if stale {
                let page = self.palette_form(Command::RemoveWorkspace);
                if let Some(palette) = self.palette.as_mut() {
                    palette.page = page;
                    palette.error = None;
                }
                return;
            }
        }
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let Page::Form {
            command, fields, ..
        } = &palette.page
        else {
            return;
        };
        if !matches!(command, Command::CreateTerminal | Command::RemoveWorkspace) {
            return;
        }
        let include_root = !matches!(command, Command::RemoveWorkspace);
        let Some(field) = fields.iter().find(|field| field.label == "Workspace") else {
            return;
        };
        let FieldKind::Pick(list) = &field.kind else {
            return;
        };
        let selected = list
            .accepted()
            .map(|item| item.value.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| field.value.clone());
        let query = list.query.clone();
        let (project, id) = split_workspace_pick(&selected);
        let mut list = PickList::workspaces(&self.hierarchy, &project, &id, include_root);
        // Keep the typed filter; never copy the selected branch label into query.
        list.query = query;
        let filtered = list.filtered();
        // An out-of-range selection cannot be accepted. A branch handoff must
        // require a new user selection, not silently choose the first match.
        list.selected = filtered
            .iter()
            .position(|item| item.value == selected)
            .unwrap_or(filtered.len());
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        let Page::Form { fields, .. } = &mut palette.page else {
            return;
        };
        let Some(field) = fields.iter_mut().find(|field| field.label == "Workspace") else {
            return;
        };
        if !selected.is_empty() {
            field.value = selected;
        }
        field.kind = FieldKind::Pick(list);
    }

    pub(super) fn open_remove_workspace(&mut self, project: String, id: String) -> DashboardAction {
        let Some(workspace) = find_workspace(self, &project, &id) else {
            return DashboardAction::None;
        };
        if workspace.root {
            self.set_error(super::keymap::ROOT_PROTECTED);
            return DashboardAction::Redraw;
        }
        let previous = self.selected_container.clone();
        self.selected_container = Some(super::TreeRow::Workspace { project, id });
        let action = self.open_remove_context(true);
        self.selected_container = previous;
        action
    }

    pub(super) fn open_remove_context(&mut self, workspace: bool) -> DashboardAction {
        let (project, name) = self.creation_context();
        if project.is_empty() || (workspace && name.is_empty()) {
            return DashboardAction::None;
        }
        if workspace
            && self
                .current_workspace()
                .is_some_and(|workspace| workspace.root)
        {
            self.set_error(super::keymap::ROOT_PROTECTED);
            return DashboardAction::Redraw;
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
        match command {
            Command::CreateWorkspace | Command::CreateBranch => {
                self.create_branch_form(&self.creation_context().0)
            }
            Command::StartNewConversation(id) => {
                let session = find_session(self, id).expect("new conversation target exists");
                if let Some(warning) = find_workspace(self, &session.project, &session.workspace)
                    .and_then(|workspace| workspace.warning.clone())
                {
                    return Page::Confirm {
                        request: None,
                        target: warning,
                    };
                }
                let mut page = self.palette_form(Command::CreateTerminal);
                if let Page::Form { fields, .. } = &mut page {
                    for field in fields.iter_mut() {
                        match field.label {
                            "Start" => field.value = "Agent".into(),
                            "Workspace" => {
                                field.value = super::picker::workspace_pick_value(
                                    &session.project,
                                    &session.workspace,
                                )
                            }
                            "Agent" => {
                                if let SessionKind::Agent { name } = &session.kind {
                                    field.value = name.clone();
                                }
                            }
                            _ => {}
                        }
                        if let FieldKind::Pick(list) = &mut field.kind {
                            list.query.clear();
                            list.select_value(&field.value);
                        }
                    }
                    self.refresh_terminal_form(fields, false);
                }
                page
            }
            Command::ReopenTerminal(id) => {
                let session = find_session(self, id).expect("reopen target exists");
                if let Some(warning) = find_workspace(self, &session.project, &session.workspace)
                    .and_then(|workspace| workspace.warning.clone())
                {
                    return Page::Confirm {
                        request: None,
                        target: warning,
                    };
                }
                let acknowledge_stopped = session
                    .recovery
                    .as_ref()
                    .is_some_and(|recovery| recovery.requires_ack);
                let target = if matches!(session.kind, SessionKind::Agent { .. }) {
                    // A certified reference is used only while it is available. Otherwise a
                    // picker-capable provider opens its own menu, so the row is never a dead end.
                    let picker = native_picker(session)
                        && (session
                            .recovery
                            .as_ref()
                            .is_none_or(|recovery| recovery.conversation.is_none())
                            || session
                                .recovery
                                .as_ref()
                                .is_some_and(|recovery| recovery.unavailable.is_some()));
                    let fallback = session
                        .recovery
                        .as_ref()
                        .is_some_and(|recovery| recovery.unavailable.is_some());
                    if picker {
                        match (fallback, acknowledge_stopped) {
                            (true, true) => "The exact recorded conversation cannot be certified. Confirm the previous agent and background processes have stopped, then open the agent's native resume picker and choose a session?",
                            (true, false) => "The exact recorded conversation cannot be certified. Open the agent's native resume picker and choose a session?",
                            (false, true) => "Confirm the previous agent and background processes have stopped, then open the agent's native resume picker?",
                            (false, false) => "Open the agent's native resume picker?",
                        }
                    } else if acknowledge_stopped {
                        "Confirm the previous agent and background processes have stopped, then resume this exact conversation without a new prompt?"
                    } else {
                        "Resume this exact conversation without a new prompt?"
                    }
                } else if acknowledge_stopped {
                    "Previous agent or background processes may still be running. Confirm they have stopped, then reopen this session in a fresh shell?"
                } else {
                    "Reopen this session in a fresh shell?"
                };
                Page::Confirm {
                    request: Some(Box::new(Request::ReopenSession {
                        session: id,
                        expected_run: session.run,
                        acknowledge_stopped,
                    })),
                    target: target.into(),
                }
            }
            // Carries the run so archived rows, absent from the hierarchy, can be acknowledged too.
            Command::AcknowledgeStopped(id, run) => Page::Confirm {
                request: Some(Box::new(Request::AcknowledgeSessionStopped {
                    session: id,
                    expected_run: run,
                })),
                target: "Acknowledge that previous agent or background processes have stopped, without launching a replacement?".into(),
            },
            Command::AgentResumeUnavailable(id) => Page::Confirm {
                request: None,
                target: find_session(self, id)
                    .and_then(|session| session.recovery.as_ref())
                    .and_then(|recovery| recovery.unavailable.clone())
                    .unwrap_or_else(|| {
                        "Agent resume is unavailable. No certified recovery reference is available."
                            .into()
                    }),
            },
            Command::CloseTerminal(id) => {
                let session = find_session(self, id).expect("close target exists");
                Page::Confirm {
                    request: Some(Box::new(Request::CloseTerminal {
                        session: id,
                        expected_run: session.run,
                    })),
                    target: format!(
                        "Close {} / {} (#{}). Stop its currently owned processes and archive its record.",
                        session_workspace_heading(self, session),
                        session.display_name(),
                        id.0
                    ),
                }
            }
            command => self.palette_form(command),
        }
    }

    /// The key binding a palette entry duplicates, so the list can print its key
    /// and its reason instead of restating them.
    fn command_hint(&self, command: &Command) -> Option<KeyBinding> {
        let action = command_action(command)?;
        keymap(self)
            .into_iter()
            .flat_map(|g| g.keys)
            .find(|binding| binding.action == action)
    }

    pub(super) fn palette_paste(&mut self, text: &str) -> DashboardAction {
        let mut palette = self.palette.take().unwrap();
        if creation_form_below_minimum(&palette.page, self.outer_area) {
            self.palette = Some(palette);
            return DashboardAction::Redraw;
        }
        palette.insert(text);
        self.refresh_workspace_form(&mut palette);
        self.refresh_model_query(&mut palette);
        self.palette = Some(palette);
        self.refresh_active_path_listing();
        DashboardAction::Redraw
    }

    fn palette_entries(
        &self,
        query: &str,
        archive: Option<&[crate::protocol::SessionSummary]>,
        agents: Option<&[AgentCandidate]>,
        workflows: Option<&[crate::protocol::WorkflowTransfer]>,
        split: bool,
    ) -> Vec<Entry> {
        if split {
            let mut entries = self.split_session_entries();
            retain_matching(&mut entries, query);
            return entries;
        }
        if let Some(transfers) = workflows {
            let query = query.to_lowercase();
            return transfers
                .iter()
                .filter_map(|transfer| {
                    let label = format!(
                        "Recover workflow {} from parent #{} epoch {}",
                        transfer.expected.workflow,
                        transfer.expected.parent.0,
                        transfer.expected.epoch
                    );
                    query
                        .split_whitespace()
                        .all(|word| label.to_lowercase().contains(word))
                        .then_some(Entry {
                            label,
                            command: Command::TransferWorkflow(Box::new(transfer.clone())),
                        })
                })
                .collect();
        }
        if let Some(candidates) = agents {
            let query = query.to_lowercase();
            return candidates
                .iter()
                .filter_map(|candidate| {
                    let label = candidate.search_identity.clone();
                    query
                        .split_whitespace()
                        .all(|word| label.to_lowercase().contains(word))
                        .then_some(Entry {
                            label,
                            command: Command::SwitchAgent(
                                candidate.session.id,
                                candidate.session.run,
                            ),
                        })
                })
                .collect();
        }
        if let Some(rows) = archive {
            let query = query.to_lowercase();
            // Legacy Operator rows stay on disk for diagnostics but never appear here.
            return rows
                .iter()
                .filter(|row| {
                    !is_operator_scope(row) && {
                        let place = session_workspace_heading(self, row);
                        let label = format!(
                            "{} {} {} {} {}",
                            row.display_name(),
                            place,
                            row.project,
                            row.workspace,
                            row.cwd.display(),
                        )
                        .to_lowercase();
                        query.split_whitespace().all(|word| label.contains(word))
                    }
                })
                .flat_map(|row| {
                    let label = format!(
                        "{} / {} (#{})",
                        session_workspace_heading(self, row),
                        row.display_name(),
                        row.id.0
                    );
                    let mut entries = vec![Entry {
                        label: format!("Unarchive: {label}"),
                        command: Command::Unarchive(row.id, row.run),
                    }];
                    entries.push(Entry {
                        label: format!("Delete record: {label}"),
                        command: Command::DeleteArchived(
                            row.id,
                            row.run,
                            row.display_name().to_owned(),
                        ),
                    });
                    // Filing a row keeps its ownership uncertainty, which blocks
                    // workspace removal; the archive must offer the acknowledgement.
                    if row
                        .recovery
                        .as_ref()
                        .is_some_and(|recovery| recovery.requires_ack)
                    {
                        entries.push(Entry {
                            label: format!("Acknowledge stopped: {label}"),
                            command: Command::AcknowledgeStopped(row.id, row.run),
                        });
                    }
                    entries
                })
                .collect();
        }
        let mut entries: Vec<Entry> = [
            ("New agent session (n)", Command::NewAgentSession),
            ("Create terminal", Command::CreateTerminal),
            ("Create branch (w)", Command::CreateBranch),
            ("Register project (a)", Command::RegisterProject),
        ]
        .into_iter()
        .map(|(label, command)| Entry {
            label: label.into(),
            command,
        })
        .collect();
        if let Some(id) = self
            .action_session()
            .filter(|id| find_session(self, *id).is_some())
        {
            if let Some(session) = find_session(self, id).filter(|session| {
                session.phase.is_live()
                    && matches!(&session.kind, SessionKind::Agent { name } if name == "pi")
            }) {
                entries.push(Entry {
                    label: "Recover workflow control in this Pi".into(),
                    command: Command::WorkflowRecovery(id, session.run),
                });
            }
            entries.push(Entry {
                label: "Rename terminal (blank = original name)".into(),
                command: Command::RenameTerminal(id),
            });
            if let Some(session) = find_session(self, id).filter(|session| !session.phase.is_live())
            {
                match &session.kind {
                    SessionKind::Agent { .. } => {
                        let available = session
                            .recovery
                            .as_ref()
                            .is_some_and(|recovery| recovery.unavailable.is_none());
                        let picker = native_picker(session);
                        entries.push(if available {
                            Entry {
                                label: if session
                                    .recovery
                                    .as_ref()
                                    .is_some_and(|recovery| recovery.failure.is_some())
                                {
                                    "Retry resume conversation"
                                } else {
                                    "Resume conversation"
                                }
                                .into(),
                                command: Command::ReopenTerminal(id),
                            }
                        } else if picker {
                            Entry {
                                label: "Open native resume picker".into(),
                                command: Command::ReopenTerminal(id),
                            }
                        } else {
                            Entry {
                                label: "Agent resume is unavailable".into(),
                                command: Command::AgentResumeUnavailable(id),
                            }
                        });
                        entries.push(Entry {
                            label: "Start new conversation in a separate session".into(),
                            command: Command::StartNewConversation(id),
                        });
                        if session
                            .recovery
                            .as_ref()
                            .is_some_and(|recovery| recovery.requires_ack)
                        {
                            entries.push(Entry {
                                label: "Acknowledge stopped processes".into(),
                                command: Command::AcknowledgeStopped(id, session.run),
                            });
                        }
                    }
                    SessionKind::Terminal => {
                        entries.push(Entry {
                            label: "Reopen in a fresh shell".into(),
                            command: Command::ReopenTerminal(id),
                        });
                        if session
                            .recovery
                            .as_ref()
                            .is_some_and(|recovery| recovery.requires_ack)
                        {
                            entries.push(Entry {
                                label: "Acknowledge stopped without reopening".into(),
                                command: Command::AcknowledgeStopped(id, session.run),
                            });
                        }
                    }
                }
            }
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
        #[cfg(target_os = "macos")]
        entries.push(Entry {
            label: "Set up iTerm focus".into(),
            command: Command::ITermSetup,
        });
        entries.push(Entry {
            label: "Settings".into(),
            command: Command::Settings,
        });
        entries.push(Entry {
            label: "Events".into(),
            command: Command::Events,
        });
        entries.push(Entry {
            label: "Refresh quota".into(),
            command: Command::RefreshQuota,
        });
        if self.quotas.as_ref().is_some_and(|quota| {
            [&quota.codex, &quota.grok]
                .iter()
                .any(|provider| provider.state == crate::protocol::QuotaState::Disabled)
        }) {
            entries.push(Entry {
                label: "Enable native account usage".into(),
                command: Command::EnableQuota,
            });
        }
        if self.connect_status.is_some() {
            entries.push(Entry {
                label: "Connect agent status".into(),
                command: Command::ConnectStatus,
            });
        }
        entries.push(Entry {
            label: "Archived sessions".into(),
            command: Command::Archives,
        });
        entries.extend(self.session_entries(None));
        let bindings: Vec<KeyBinding> = keymap(self).into_iter().flat_map(|g| g.keys).collect();
        for hint in &bindings {
            if matches!(hint.key, "n" | "w" | "a" | "X" | ":" | "Space") {
                continue;
            }
            entries.push(Entry {
                label: hint.name.into(),
                command: Command::Hint(hint.action),
            });
        }
        // The key popup's rule: drop what the current target cannot use; keep a
        // row that waits for the screen or is blocked, with its reason, so the
        // list does not shift under the cursor when it becomes available.
        entries.retain(|entry| {
            command_action(&entry.command).is_none_or(|action| {
                bindings
                    .iter()
                    .find(|binding| binding.action == action)
                    .is_none_or(KeyBinding::shown_in_group)
            })
        });
        // Settings rows are found by searching, not listed.
        if !query.trim().is_empty() {
            entries.extend(
                ovrcr_protocol::entries()
                    .iter()
                    // The editor offers the sound choice only on macOS.
                    .filter(|setting| {
                        cfg!(target_os = "macos")
                            || setting.path != ovrcr_protocol::ReadySoundChoice::KEY
                    })
                    .map(|setting| Entry {
                        label: format!("Settings: {}", setting.label),
                        command: Command::Setting(setting),
                    }),
            );
        }
        retain_matching(&mut entries, query);
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
        let previous_inputs = self.current_model_query_inputs();
        let action = self.palette_mouse_inner(mouse, area);
        self.refresh_active_path_listing();
        self.refresh_model_query_if_inputs_changed(previous_inputs);
        action
    }

    fn palette_mouse_inner(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        if self
            .palette
            .as_ref()
            .is_some_and(|palette| close_terminal_id(&palette.page).is_some())
        {
            return self.close_confirm_mouse(mouse, area);
        }
        let point = Position::new(mouse.column, mouse.row);
        let palette = self.palette.as_ref().unwrap();
        let (_, _, body, buttons) = palette_geometry_for(area, Some(palette));
        if creation_form_below_minimum(&palette.page, area) {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) && buttons.contains(point) {
                let refresh_branch = palette.pending.is_none()
                    && palette
                        .branch_attempt
                        .as_ref()
                        .is_some_and(|attempt| attempt.blocked);
                let label = button_text(&palette.page, buttons.width, refresh_branch);
                let cancel = label.rfind('[').unwrap() as u16;
                if mouse.column.saturating_sub(buttons.x) >= cancel {
                    return self
                        .palette_key_inner(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), true);
                }
            }
            return DashboardAction::None;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && buttons.contains(point) {
            let refresh_branch = palette.pending.is_none()
                && palette
                    .branch_attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.blocked);
            let label = button_text(&palette.page, buttons.width, refresh_branch);
            let cancel = label.rfind('[').unwrap() as u16;
            let x = mouse.column - buttons.x;
            if x >= cancel && x < label.len() as u16 {
                let close_picker_first = palette.new_agent_picker_open
                    && matches!(
                        &palette.page,
                        Page::Form {
                            command,
                            fields,
                            active,
                            ..
                        } if compact_picker_field(command, &fields[*active])
                    );
                let action =
                    self.palette_key_inner(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), true);
                return if close_picker_first {
                    self.palette_key_inner(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), true)
                } else {
                    action
                };
            }
            if x < cancel.saturating_sub(1)
                && palette.pending.is_none()
                && !palette.submit_when_ready
            {
                let form = matches!(&palette.page, Page::Form { .. });
                let key = if form {
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)
                } else {
                    KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
                };
                return self.palette_key_inner(key, true);
            }
            return DashboardAction::Redraw;
        }
        if palette.pending.is_some() || palette.submit_when_ready {
            return DashboardAction::None;
        }
        let (lines, targets, scroll) = self.palette_body(body);
        let popup = creation_picker_popup(body, palette, &targets, scroll);
        let popup_open = popup.is_some();
        let popup_area = popup.as_ref().map(|popup| popup.area);
        let popup_option = popup.as_ref().and_then(|popup| {
            popup
                .option_rows
                .iter()
                .find(|(row, _)| *row == mouse.row)
                .map(|(_, option)| (popup.field_index, *option))
        });
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            if popup_open {
                let over_popup = popup_area.is_some_and(|area| area.contains(point));
                drop(popup);
                drop(lines);
                if !over_popup {
                    return DashboardAction::None;
                }
                let key = if mouse.kind == MouseEventKind::ScrollUp {
                    KeyCode::Up
                } else {
                    KeyCode::Down
                };
                return self.palette_key_inner(KeyEvent::new(key, KeyModifiers::NONE), true);
            }
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
                return self.palette_key_inner(
                    KeyEvent::new(
                        if delta < 0 {
                            KeyCode::Up
                        } else {
                            KeyCode::Down
                        },
                        KeyModifiers::NONE,
                    ),
                    true,
                );
            }
            let max = lines.len().saturating_sub(body.height as usize);
            let next = scroll.saturating_add_signed(delta).min(max);
            self.palette.as_mut().unwrap().scroll = Some(next);
            return DashboardAction::Redraw;
        }
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return DashboardAction::None;
        }
        let mut paired_field_click = None;
        let clicked_popup = popup_area.is_some_and(|area| area.contains(point));
        let target = if clicked_popup {
            popup_option.map(|(index, selected)| PaletteTarget::Option(index, selected))
        } else if body.contains(point) {
            let row_target = targets
                .iter()
                .find(|(row, _)| *row == scroll + usize::from(mouse.row - body.y))
                .map(|(_, target)| *target);
            match row_target {
                Some(PaletteTarget::FieldPair(left, right, value)) => {
                    let (left_width, gap, right_width) = pair_columns(body);
                    let left_end = body.x.saturating_add(left_width);
                    let right_start = left_end.saturating_add(gap);
                    let click = if mouse.column < left_end {
                        Some((left, body.x, left_width))
                    } else if mouse.column >= right_start {
                        Some((right, right_start, right_width))
                    } else {
                        None
                    };
                    click.map(|(index, origin, width)| {
                        paired_field_click = Some((index, origin, width));
                        PaletteTarget::Field(index, value)
                    })
                }
                target => target,
            }
        } else {
            None
        };
        drop(popup);
        drop(lines);
        let Some(target) = target else {
            if clicked_popup {
                return DashboardAction::Redraw;
            }
            if popup_open {
                return self
                    .palette_key_inner(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), true);
            }
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
            (
                Page::Form {
                    command,
                    fields,
                    active,
                    ..
                },
                PaletteTarget::Field(index, value),
            ) => {
                let was_active = *active == index;
                let picker_was_open = palette.new_agent_picker_open;
                let previous_active = *active;
                if picker_was_open && !was_active {
                    reset_compact_picker(&mut fields[previous_active]);
                }
                if !was_active {
                    palette.cursor = Default::default();
                }
                *active = index;
                let field = &mut fields[index];
                let new_agent_pick = compact_picker_field(command, field);
                // A field click already identifies a visible anchor. Keep its
                // viewport while toggling or switching an anchored menu.
                palette.scroll = (new_agent_pick || picker_was_open).then_some(scroll);
                if new_agent_pick {
                    if was_active && picker_was_open {
                        reset_compact_picker(field);
                        palette.new_agent_picker_open = false;
                        palette.invalid = None;
                        palette.error = None;
                    } else {
                        if let Some(target) = committed_parent_branch_target(command, field) {
                            palette.parent_branch_target = Some(target);
                        }
                        palette.new_agent_picker_open = true;
                    }
                    if !was_active || !picker_was_open {
                        palette.cursor = Default::default();
                        reset_compact_picker(field);
                    }
                } else if picker_was_open {
                    palette.new_agent_picker_open = false;
                }
                if value
                    && let FieldKind::Pick(list) = &mut field.kind
                    && !new_agent_pick
                    && (!was_active || list.query.is_empty())
                {
                    list.query.clone_from(&field.value);
                    list.selected = 0;
                }
                match &field.kind {
                    FieldKind::Toggle => key = Some(KeyCode::Char(' ')),
                    _ if value => {
                        let text = match &field.kind {
                            FieldKind::Pick(list) if new_agent_pick || !list.query.is_empty() => {
                                &list.query
                            }
                            _ => &field.value,
                        };
                        let (origin, width) = paired_field_click
                            .filter(|(paired, _, _)| *paired == index)
                            .map(|(_, origin, width)| (origin, width))
                            .unwrap_or((body.x, body.width));
                        palette.cursor.click(
                            text,
                            width.saturating_sub(2) as usize,
                            mouse.column.saturating_sub(origin.saturating_add(2)) as usize,
                        );
                    }
                    _ => {}
                }
            }
            (Page::Form { fields, active, .. }, PaletteTarget::MultilineLine(index, line)) => {
                let picker_was_open = palette.new_agent_picker_open;
                if picker_was_open {
                    reset_compact_picker(&mut fields[*active]);
                    palette.new_agent_picker_open = false;
                }
                *active = index;
                palette.scroll = picker_was_open.then_some(scroll);
                if matches!(&fields[index].kind, FieldKind::Multiline) {
                    palette.cursor.click_line(
                        &fields[index].value,
                        line,
                        body.width.saturating_sub(2) as usize,
                        mouse.column.saturating_sub(body.x + 2) as usize,
                    );
                }
            }
            (
                Page::Form {
                    command,
                    fields,
                    active,
                    ..
                },
                PaletteTarget::Option(index, selected),
            ) => {
                *active = index;
                palette.cursor = Default::default();
                let explicit_compact_choice = compact_picker_field(command, &fields[index]);
                if explicit_compact_choice {
                    palette.new_agent_picker_open = true;
                }
                if let Some(target) = committed_parent_branch_target(command, &fields[index]) {
                    palette.parent_branch_target = Some(target);
                }
                if explicit_compact_choice {
                    fields[index].edited = true;
                }
                match &mut fields[index].kind {
                    FieldKind::Pick(list) => {
                        list.selected = selected;
                        key = Some(if explicit_compact_choice {
                            KeyCode::Enter
                        } else {
                            KeyCode::Tab
                        });
                    }
                    FieldKind::Path(picker) => {
                        picker.selected = selected;
                        key = Some(KeyCode::Tab);
                    }
                    _ => {}
                }
                if explicit_compact_choice
                    && let FieldKind::Pick(list) = &fields[index].kind
                    && let Some(value) = list.accepted().map(|item| item.value.clone())
                {
                    fields[index].value = value;
                    fields[index].edited = true;
                    if let Some(target) = committed_parent_branch_target(command, &fields[index]) {
                        palette.parent_branch_target = Some(target);
                    }
                }
            }
            _ => {}
        }
        self.palette = Some(palette);
        if let Some(code) = key {
            self.palette_key_inner(KeyEvent::new(code, KeyModifiers::NONE), true)
        } else {
            DashboardAction::Redraw
        }
    }

    pub(super) fn palette_key(&mut self, key: KeyEvent) -> DashboardAction {
        let previous_inputs = self.current_model_query_inputs();
        let action = self.palette_key_inner(key, true);
        self.refresh_active_path_listing();
        self.refresh_model_query_if_inputs_changed(previous_inputs);
        action
    }

    fn palette_key_inner(
        &mut self,
        key: KeyEvent,
        history_before_request: bool,
    ) -> DashboardAction {
        let picker_scroll = self.palette.as_ref().and_then(|palette| {
            let Page::Form {
                command,
                fields,
                active,
                ..
            } = &palette.page
            else {
                return None;
            };
            if !compact_picker_field(command, &fields[*active])
                || matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
            {
                return None;
            }
            let (_, _, body, _) = palette_geometry_for(self.outer_area, Some(palette));
            let (_, targets, scroll) = self.palette_body(body);
            targets
                .iter()
                .any(|(row, target)| {
                    *row >= scroll
                        && *row < scroll + usize::from(body.height)
                        && match target {
                            PaletteTarget::Field(index, true) => index == active,
                            PaletteTarget::FieldPair(left, right, true) => {
                                left == active || right == active
                            }
                            _ => false,
                        }
                })
                .then_some(scroll)
        });
        let mut palette = self.palette.take().unwrap();
        if key.code != KeyCode::Esc && creation_form_below_minimum(&palette.page, self.outer_area) {
            self.palette = Some(palette);
            return DashboardAction::Redraw;
        }
        if (key.code == KeyCode::Esc || is_browse_key(key))
            && matches!(
                &palette.page,
                Page::Form {
                    command: Command::CreateBranch,
                    ..
                }
            )
        {
            if palette.pending.is_some() {
                palette.error = Some(
                    "Branch creation is still being reconciled. Wait for the result before leaving."
                        .into(),
                );
                self.palette = Some(palette);
                return DashboardAction::Redraw;
            }
            if key.code == KeyCode::Esc
                && palette.new_agent_picker_open
                && let Page::Form {
                    command,
                    fields,
                    active,
                    ..
                } = &mut palette.page
                && compact_picker_field(command, &fields[*active])
            {
                reset_compact_picker(&mut fields[*active]);
                palette.new_agent_picker_open = false;
                palette.error = None;
                palette.invalid = None;
                self.palette = Some(palette);
                return DashboardAction::Redraw;
            }
            if let Some(request_id) = palette.suggestions.inspect {
                self.ignored_responses.insert(request_id);
            }
            self.present_wip_prompt();
            self.restore_deferred_initial_prompt_recovery();
            return DashboardAction::Redraw;
        }
        if (key.code == KeyCode::Esc || is_browse_key(key))
            && palette.pending.is_some()
            && palette
                .branch_attempt
                .as_ref()
                .is_some_and(|attempt| attempt.agent_form)
        {
            palette.error = Some(
                "Branch creation is still being reconciled. Wait for its result before leaving this form."
                    .into(),
            );
            self.palette = Some(palette);
            return DashboardAction::Redraw;
        }
        if key.code == KeyCode::Esc
            && let Page::Form {
                command: Command::NewAgentSession,
                fields,
                active,
                ..
            } = &mut palette.page
        {
            let active = *active;
            if palette.new_agent_picker_open
                || matches!(&fields[active].kind, FieldKind::Pick(list) if !list.query.is_empty())
            {
                let restore_parent = (fields[active].label == "Branch"
                    && created_branch_name(&fields[active].value).is_some())
                .then(|| palette.parent_branch_target.clone())
                .flatten();
                reset_compact_picker(&mut fields[active]);
                if let Some((target, edited)) = restore_parent {
                    fields[active].value = target.clone();
                    fields[active].edited = edited;
                    if let FieldKind::Pick(list) = &mut fields[active].kind {
                        list.query.clear();
                        list.select_value(&target);
                    }
                    palette.parent_branch_target = Some((target, edited));
                }
                palette.new_agent_picker_open = false;
                palette.invalid = None;
                palette.error = None;
                self.refresh_workspace_form(&mut palette);
                self.palette = Some(palette);
                return DashboardAction::Redraw;
            }
        }
        if key.code == KeyCode::Esc || is_browse_key(key) {
            if let Some(Request::AnswerWipSave {
                project, workspace, ..
            }) = confirmation_request(&palette.page)
            {
                let request = Request::AnswerWipSave {
                    project: project.clone(),
                    workspace: workspace.clone(),
                    save: false,
                };
                let action = self.palette_submit(&mut palette, request, history_before_request);
                self.palette = Some(palette);
                return action;
            }
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
            self.present_wip_prompt();
            self.restore_deferred_initial_prompt_recovery();
            return DashboardAction::Redraw;
        }
        // Other keys wait for the running request so a submit cannot repeat.
        if palette.pending.is_some() || palette.submit_when_ready {
            self.palette = Some(palette);
            return DashboardAction::Redraw;
        }
        self.refresh_workspace_form(&mut palette);
        palette.scroll = picker_scroll;
        let multiline_active = matches!(
            &palette.page,
            Page::Form { fields, active, .. }
                if matches!(fields[*active].kind, FieldKind::Multiline)
        );
        let submit_chord = matches!(&palette.page, Page::Form { .. })
            && key.code == KeyCode::Enter
            && key
                .modifiers
                .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL);
        if submit_chord
            && palette.pending.is_none()
            && let Some(token) = palette
                .branch_attempt
                .as_ref()
                .filter(|attempt| attempt.blocked)
                .map(|attempt| attempt.token)
        {
            palette.error =
                Some("Refreshing the workspace registry and hierarchy before retrying.".into());
            self.palette = Some(palette);
            self.start_branch_reconciliation(token);
            return DashboardAction::Redraw;
        }
        if matches!(
            key.code,
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter | KeyCode::Up | KeyCode::Down
        ) && !(multiline_active
            && !submit_chord
            && matches!(key.code, KeyCode::Enter | KeyCode::Up | KeyCode::Down))
        {
            palette.cursor = Default::default();
            palette.scroll = picker_scroll;
        }
        // Editing or choosing a value retracts the complaint about that field.
        if let Page::Form { fields, active, .. } = &palette.page
            && palette.invalid.as_ref().is_some_and(|(at, _)| at == active)
            && match key.code {
                KeyCode::Backspace | KeyCode::Delete => true,
                KeyCode::Char('u' | 'U') => key.modifiers.contains(KeyModifiers::CONTROL),
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => {
                    matches!(fields[*active].kind, FieldKind::Pick(_))
                }
                _ => false,
            }
        {
            palette.invalid = None;
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
                        command,
                        fields,
                        active,
                        name_edited,
                        root_edited,
                        ..
                    } => {
                        if matches!(&fields[*active].kind, FieldKind::Toggle) {
                            self.palette = Some(palette);
                            return action;
                        }
                        if !compact_picker_field(command, &fields[*active]) {
                            mark_form_edit(fields, *active, name_edited, root_edited);
                        }
                        palette.cursor = Default::default();
                        let field = &mut fields[*active];
                        match &mut field.kind {
                            // Clears the filter; the value stays chosen.
                            FieldKind::Pick(list) => {
                                list.query.clear();
                                list.select_value(&field.value);
                            }
                            FieldKind::Path(picker) => {
                                picker.selected = 0;
                                field.value.clear();
                            }
                            FieldKind::Text
                            | FieldKind::Multiline
                            | FieldKind::Toggle
                            | FieldKind::Notice => field.value.clear(),
                        }
                    }
                    _ => {}
                }
                palette.error = None;
            }
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if matches!(&palette.page, Page::Form { fields, active, .. } if matches!(&fields[*active].kind, FieldKind::Toggle)) =>
            {
                palette.invalid = None;
                if let Page::Form { fields, active, .. } = &mut palette.page {
                    if fields[*active].label == "Advanced options" {
                        let expanded = fields[*active].value != "expanded";
                        fields[*active].value =
                            if expanded { "expanded" } else { "collapsed" }.into();
                        if let Some(name) = fields.iter_mut().find(|field| field.label == "Name") {
                            name.hidden = !expanded;
                        }
                        if let Some(base) = fields.iter_mut().find(|field| field.label == "Base") {
                            base.hidden = !expanded;
                        }
                    } else if fields[*active].label == "Workspace options" {
                        let expanded = fields[*active].value != "expanded";
                        fields[*active].value =
                            if expanded { "expanded" } else { "collapsed" }.into();
                        if let Some(base) = fields.iter_mut().find(|field| field.label == "Base") {
                            base.hidden = !expanded;
                        }
                    } else {
                        // Switching branch mode replaces Branch, so a complaint no longer applies.
                        let to_existing = fields
                            .iter()
                            .find(|field| field.label == "Branch mode")
                            .is_some_and(|field| field.value == "new");
                        if let Some(mode) =
                            fields.iter_mut().find(|field| field.label == "Branch mode")
                        {
                            mode.value = if to_existing { "existing" } else { "new" }.into();
                        }
                        if let Some(branch) =
                            fields.iter_mut().find(|field| field.label == "Branch")
                        {
                            branch.kind = FieldKind::Text;
                            if to_existing {
                                branch.value.clear();
                                branch.edited = false;
                            }
                        }
                        if let Some(base) = fields.iter_mut().find(|field| field.label == "Base") {
                            base.kind = FieldKind::Text;
                            base.value.clear();
                            base.edited = false;
                        }
                    }
                }
            }
            KeyCode::Char(ch)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                if let Page::Form {
                    command,
                    fields,
                    active,
                    ..
                } = &palette.page
                    && let Some(target) = committed_parent_branch_target(command, &fields[*active])
                {
                    palette.parent_branch_target = Some(target);
                }
                palette.insert(&ch.to_string());
            }
            KeyCode::Enter if multiline_active && !submit_chord => {
                palette.insert("\n");
            }
            KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End => {
                let remembered_target = match &palette.page {
                    Page::Form {
                        command,
                        fields,
                        active,
                        ..
                    } => committed_parent_branch_target(command, &fields[*active]),
                    _ => None,
                };
                if let Some(target) = remembered_target {
                    palette.parent_branch_target = Some(target);
                }
                let editing = matches!(key.code, KeyCode::Backspace | KeyCode::Delete);
                match &mut palette.page {
                    Page::Search { query, selected } => {
                        palette.cursor.key(query, key.code);
                        if editing {
                            *selected = 0;
                        }
                    }
                    Page::Form {
                        command,
                        fields,
                        active,
                        name_edited,
                        root_edited,
                        ..
                    } => {
                        let new_agent_pick = compact_picker_field(command, &fields[*active]);
                        if editing && !new_agent_pick {
                            mark_form_edit(fields, *active, name_edited, root_edited);
                        }
                        if matches!(command, Command::NewAgentSession)
                            && matches!(fields[*active].label, "Branch" | "Agent harness" | "Model")
                            && matches!(key.code, KeyCode::Left | KeyCode::Right)
                            && matches!(fields[*active].kind, FieldKind::Pick(_))
                        {
                            // Choice cycling is a user selection even when it
                            // wraps back to the already displayed value.
                            fields[*active].edited = true;
                        }
                        let field = &mut fields[*active];
                        match &mut field.kind {
                            FieldKind::Toggle | FieldKind::Notice => {}
                            // Left/Right step through the choices, wrapping at the ends.
                            FieldKind::Pick(list)
                                if matches!(key.code, KeyCode::Left | KeyCode::Right) =>
                            {
                                list.cycle(if key.code == KeyCode::Left { -1 } else { 1 });
                                accept_pick(field);
                                if let Some(target) = committed_parent_branch_target(command, field)
                                {
                                    palette.parent_branch_target = Some(target);
                                }
                            }
                            FieldKind::Pick(list) => {
                                palette.cursor.key(&mut list.query, key.code);
                                if editing {
                                    list.selected = 0;
                                    if !new_agent_pick {
                                        accept_pick(field);
                                    }
                                }
                            }
                            FieldKind::Text => palette.cursor.key(&mut field.value, key.code),
                            FieldKind::Multiline => {
                                palette.cursor.line_key(&mut field.value, key.code)
                            }
                            FieldKind::Path(picker) => {
                                palette.cursor.key(&mut field.value, key.code);
                                if editing {
                                    picker.selected = 0;
                                }
                            }
                        }
                        if editing && new_agent_pick {
                            palette.new_agent_picker_open = true;
                        }
                    }
                    _ => {}
                }
                if editing {
                    palette.error = None;
                }
            }
            KeyCode::Up | KeyCode::Down if multiline_active => {
                if let Page::Form { fields, active, .. } = &mut palette.page {
                    let field = &mut fields[*active];
                    palette
                        .cursor
                        .vertical(&field.value, key.code == KeyCode::Down);
                }
            }
            KeyCode::Up | KeyCode::Down if form_list_active(&palette.page) => {
                let remembered_target = match &palette.page {
                    Page::Form {
                        command,
                        fields,
                        active,
                        ..
                    } => committed_parent_branch_target(command, &fields[*active]),
                    _ => None,
                };
                if let Some(target) = remembered_target {
                    palette.parent_branch_target = Some(target);
                }
                if let Page::Form {
                    command,
                    fields,
                    active,
                    ..
                } = &mut palette.page
                {
                    if compact_picker_field(command, &fields[*active]) {
                        fields[*active].edited = true;
                        palette.new_agent_picker_open = true;
                    }
                    let delta = if matches!(key.code, KeyCode::Up) {
                        -1
                    } else {
                        1
                    };
                    let value = fields[*active].value.clone();
                    match &mut fields[*active].kind {
                        FieldKind::Pick(list) => {
                            list.move_selection(delta);
                            accept_pick(&mut fields[*active]);
                            if let Some(target) =
                                committed_parent_branch_target(command, &fields[*active])
                            {
                                palette.parent_branch_target = Some(target);
                            }
                            if matches!(command, Command::CreateBranch)
                                && fields[*active].label == "Project"
                            {
                                fields[*active].edited = true;
                            }
                        }
                        FieldKind::Path(picker) => {
                            let listing =
                                picker.listing(&value, &self.settings.picker_roots).clone();
                            picker.move_selection(&listing, delta);
                        }
                        FieldKind::Text
                        | FieldKind::Multiline
                        | FieldKind::Toggle
                        | FieldKind::Notice => {}
                    }
                }
            }
            KeyCode::Up | KeyCode::BackTab | KeyCode::Down | KeyCode::Tab => {
                let backwards = matches!(key.code, KeyCode::Up | KeyCode::BackTab);
                match &mut palette.page {
                    Page::Search { query, selected } => {
                        let count = self
                            .palette_entries(
                                query,
                                palette.archive.as_deref(),
                                palette.agents.as_deref(),
                                palette.workflows.as_deref(),
                                palette.split,
                            )
                            .len();
                        if count > 0 {
                            *selected = if backwards {
                                selected.saturating_sub(1)
                            } else {
                                (*selected + 1).min(count - 1)
                            };
                        }
                    }
                    Page::Form {
                        command,
                        fields,
                        active,
                        name_edited,
                        root_edited,
                        ..
                    } => {
                        let closing_compact_picker =
                            matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
                                && compact_picker_field(command, &fields[*active]);
                        if closing_compact_picker {
                            palette.new_agent_picker_open = false;
                            reset_compact_picker(&mut fields[*active]);
                        }
                        let compact_pick = compact_picker_field(command, &fields[*active]);
                        let value = fields[*active].value.trim();
                        // A path that already names a Git checkout is complete:
                        // Tab moves on rather than descending into its folders.
                        let checkout = matches!(fields[*active].kind, FieldKind::Path(_))
                            && !value.is_empty()
                            && expand_path(value).join(".git").exists();
                        if matches!(key.code, KeyCode::Tab)
                            && matches!(fields[*active].kind, FieldKind::Path(_))
                            && !checkout
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
                        let branch_still_needs_a_choice =
                            matches!(command, Command::NewAgentSession)
                                && fields[*active].label == "Branch"
                                && match &fields[*active].kind {
                                    FieldKind::Pick(list) => !list
                                        .items
                                        .iter()
                                        .any(|item| item.value == fields[*active].value),
                                    _ => false,
                                };
                        if matches!(key.code, KeyCode::Tab)
                            && !compact_pick
                            && !branch_still_needs_a_choice
                            && !accept_pick(&mut fields[*active])
                        {
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
                    let entries = self.palette_entries(
                        query,
                        palette.archive.as_deref(),
                        palette.agents.as_deref(),
                        palette.workflows.as_deref(),
                        palette.split,
                    );
                    *selected = (*selected).min(entries.len().saturating_sub(1));
                    if let Some(entry) = entries.get(*selected) {
                        match entry.command.clone() {
                            Command::WorkflowRecovery(parent, run) => {
                                action = self.palette_submit(
                                    &mut palette,
                                    Request::DashboardWorkflow(
                                        crate::protocol::DashboardWorkflowRequest::Candidates {
                                            parent,
                                            run,
                                        },
                                    ),
                                    history_before_request,
                                );
                            }
                            Command::TransferWorkflow(transfer) => {
                                palette.page = Page::Confirm {
                                    target: format!(
                                        "Transfer workflow {} from parent #{} epoch {} to Pi #{} run {}? Old parent input is retired. Child human control and unresolved work remain.",
                                        transfer.expected.workflow,
                                        transfer.expected.parent.0,
                                        transfer.expected.epoch,
                                        transfer.parent.0,
                                        transfer.parent_run.0
                                    ),
                                    request: Some(Box::new(Request::DashboardWorkflow(
                                        crate::protocol::DashboardWorkflowRequest::Transfer(
                                            transfer,
                                        ),
                                    ))),
                                };
                            }

                            Command::SwitchAgent(id, run) => {
                                let candidate = palette.agents.as_ref().and_then(|candidates| {
                                    candidates
                                        .iter()
                                        .find(|c| c.session.id == id && c.session.run == run)
                                });
                                if let Some(reason) = candidate.and_then(|c| c.reason(self)) {
                                    palette.error = Some(reason.into());
                                } else if candidate.is_some() {
                                    return self.confirm_agent_switch(id, run);
                                }
                            }
                            Command::Switch(id) => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                self.select_session(id);
                                return self.request_selected();
                            }
                            Command::NewAgentSession => {
                                self.begin_new_agent_session_form(&mut palette);
                                let request_id = self.next_request_id();
                                self.enqueue_request(ClientMessage {
                                    request_id,
                                    request: Request::List,
                                });
                            }
                            #[cfg(target_os = "macos")]
                            Command::ITermSetup => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                return self.begin_iterm_setup();
                            }
                            Command::Settings => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                return self.open_details(super::quota::Details::Settings);
                            }
                            Command::Setting(setting) => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                let action = self.open_details(super::quota::Details::Settings);
                                self.settings_editor.select(setting.path);
                                return action;
                            }
                            Command::Events => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                return self.open_events();
                            }
                            command @ (Command::RefreshQuota | Command::EnableQuota) => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                return self.quota_request(match command {
                                    Command::RefreshQuota => {
                                        Request::RefreshQuota { provider: None }
                                    }
                                    _ => Request::SetSetting {
                                        path: "quota.enabled".into(),
                                        value: Some("true".into()),
                                    },
                                });
                            }
                            Command::ConnectStatus => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                palette.suggestions.inspect = None;
                                action = self.preview_agent_status(&mut palette);
                                if !palette.connect_status {
                                    return action;
                                }
                            }
                            Command::Hint(action) => {
                                if self
                                    .command_hint(&entry.command)
                                    .is_some_and(|h| h.enabled())
                                {
                                    if action == Action::OperatorQuick {
                                        self.palette = Some(palette);
                                        return self.run(action);
                                    }
                                    if let Some(request_id) = palette.suggestions.inspect {
                                        self.ignored_responses.insert(request_id);
                                    }
                                    return self.run(action);
                                }
                            }
                            Command::Archives => {
                                palette.archive = Some(Vec::new());
                                palette.page = Page::Search {
                                    query: String::new(),
                                    selected: 0,
                                };
                                action = self.palette_submit(
                                    &mut palette,
                                    Request::Inspect,
                                    history_before_request,
                                );
                            }
                            Command::Unarchive(session, expected_run) => {
                                action = self.palette_submit(
                                    &mut palette,
                                    Request::UnarchiveSession {
                                        session,
                                        expected_run,
                                    },
                                    history_before_request,
                                );
                            }
                            Command::DeleteArchived(session, expected_run, title) => {
                                palette.page = Page::Confirm {
                                    request: Some(Box::new(Request::DeleteArchivedSession {
                                        session,
                                        expected_run,
                                    })),
                                    target: format!(
                                        "Delete record {}? Provider history files will be kept.",
                                        title
                                    ),
                                };
                            }
                            Command::CloseTerminal(id)
                                if find_session(self, id).is_some_and(|s| {
                                    !s.phase.is_live() && s.close_warning().is_none()
                                }) =>
                            {
                                let run = find_session(self, id).unwrap().run;
                                action = self.palette_submit(
                                    &mut palette,
                                    Request::CloseTerminal {
                                        session: id,
                                        expected_run: run,
                                    },
                                    history_before_request,
                                );
                            }
                            command => {
                                palette.page = self.command_page(command.clone());
                                if matches!(command, Command::CreateWorkspace) {
                                    let _ = ensure_workspace_id(&mut palette);
                                }
                            }
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
                    if !submit_chord
                        && matches!(command, Command::CreateTerminal)
                        && fields[*active].label == "Workspace"
                        && matches!(fields[*active].kind, FieldKind::Pick(_))
                    {
                        if accept_pick(&mut fields[*active]) {
                            palette.invalid = None;
                            self.refresh_terminal_form(fields, *name_edited);
                        } else {
                            palette.invalid =
                                Some((*active, "Workspace not found in available choices".into()));
                        }
                        self.palette = Some(palette);
                        return action;
                    }
                    if !submit_chord && compact_picker_field(command, &fields[*active]) {
                        if palette.new_agent_picker_open {
                            let parent_branch = matches!(command, Command::NewAgentSession)
                                && fields[*active].label == "Branch";
                            let child_project = matches!(command, Command::CreateBranch)
                                && fields[*active].label == "Project";
                            if let Some(target) =
                                committed_parent_branch_target(command, &fields[*active])
                            {
                                palette.parent_branch_target = Some(target);
                            }
                            fields[*active].edited = true;
                            if accept_pick(&mut fields[*active]) {
                                let selected = fields[*active].value.clone();
                                if parent_branch
                                    && let Some(target) =
                                        committed_parent_branch_target(command, &fields[*active])
                                {
                                    palette.parent_branch_target = Some(target);
                                }
                                reset_compact_picker(&mut fields[*active]);
                                palette.new_agent_picker_open = false;
                                palette.invalid = None;
                                palette.error = None;
                                if child_project {
                                    palette.branch_project = Some(selected);
                                }
                                self.refresh_workspace_form(&mut palette);
                            }
                        } else {
                            if let Some(target) =
                                committed_parent_branch_target(command, &fields[*active])
                            {
                                palette.parent_branch_target = Some(target);
                            }
                            reset_compact_picker(&mut fields[*active]);
                            palette.new_agent_picker_open = true;
                        }
                        self.palette = Some(palette);
                        return action;
                    }
                    if submit_chord && matches!(command, Command::NewAgentSession) {
                        palette.new_agent_picker_open = false;
                        if compact_picker_field(command, &fields[*active]) {
                            reset_compact_picker(&mut fields[*active]);
                        }
                    }
                    // A field that Start just hid is no longer part of the form.
                    let branch_is_unselected = matches!(command, Command::NewAgentSession)
                        && fields[*active].label == "Branch"
                        && match &fields[*active].kind {
                            FieldKind::Pick(list) => !list
                                .items
                                .iter()
                                .any(|item| item.value == fields[*active].value),
                            _ => false,
                        };
                    if !fields[*active].hidden
                        && !branch_is_unselected
                        && !compact_picker_field(command, &fields[*active])
                        && !accept_pick(&mut fields[*active])
                    {
                        // A deferred submit must not wait on a pick that can
                        // never resolve; say which field went unmatched.
                        palette.invalid = Some((
                            *active,
                            format!("{} not found in available choices", fields[*active].label),
                        ));
                        palette.submit_when_ready = false;
                        self.palette = Some(palette);
                        return action;
                    }
                    if matches!(command, Command::RecoverLaunch)
                        && fields.first().is_some_and(|f| f.label == "Recovery")
                    {
                        let mut previous = palette.failed_launch.as_ref().unwrap().clone();
                        if !self
                            .hierarchy
                            .projects
                            .iter()
                            .find(|p| p.name == previous.session.project)
                            .is_some_and(|p| {
                                p.workspaces
                                    .iter()
                                    .any(|w| w.id == previous.session.workspace)
                            })
                        {
                            palette.error = Some("Workspace not yet registered; wait for the hierarchy update before retrying.".into());
                            self.palette = Some(palette);
                            return action;
                        }
                        match fields[0].value.as_str() {
                            "Choose another agent" => {
                                *fields = self.launch_fields(&previous.session.project, false);
                                fields[0] = pick_field(
                                    "Start",
                                    "Agent".into(),
                                    ["Agent", "Terminal"]
                                        .into_iter()
                                        .map(|s| PickItem {
                                            label: s.into(),
                                            value: s.into(),
                                        })
                                        .collect(),
                                );
                                self.refresh_terminal_form(fields, false);
                                *active = 1;
                                palette.error = None;
                                self.palette = Some(palette);
                                return action;
                            }
                            "Open shell" => {
                                previous.session.argv = self.shell_argv();
                                previous.session.label = None;
                                previous.session.kind = SessionKind::Terminal;
                                previous.model_policy =
                                    ovrcr_protocol::AgentModelPolicy::HarnessDefault;
                                previous.model_binding = None;
                                previous.expected_model = None;
                            }
                            _ => {}
                        }
                        let request = if matches!(&previous.session.kind, SessionKind::Agent { .. })
                        {
                            Request::CreateAgentSession(previous)
                        } else {
                            Request::CreateSession(previous.session)
                        };
                        action = self.palette_submit(&mut palette, request, history_before_request);
                        self.palette = Some(palette);
                        return action;
                    }
                    if matches!(command, Command::RecoverInitialPrompt) {
                        let Some(launch) = palette.recovered_prompt_launch.clone() else {
                            palette.error =
                                Some("This launch can no longer be recovered safely".into());
                            self.palette = Some(palette);
                            return action;
                        };
                        action = self.palette_submit(
                            &mut palette,
                            Request::CreateAgentSession(launch),
                            history_before_request,
                        );
                        self.palette = Some(palette);
                        return action;
                    }
                    self.refresh_terminal_form(fields, *name_edited);
                    self.refresh_project_form(fields, *name_edited, *root_edited);
                    // Enter submits from any field; the first field that would be
                    // refused keeps the focus and says why.
                    if let Some((index, problem)) =
                        self.form_problem(command, fields, palette.workspace_inventory.as_ref())
                    {
                        *active = index;
                        palette.invalid = Some((index, problem));
                    } else if matches!(command, Command::CreateBranch)
                        && let Some((index, problem)) =
                            self.create_branch_resolution_problem(fields, &palette.suggestions)
                    {
                        *active = index;
                        palette.invalid = Some((index, problem));
                    } else if matches!(command, Command::NewAgentSession)
                        && let Some((index, problem)) =
                            self.new_agent_branch_resolution_problem(fields, &palette.suggestions)
                    {
                        *active = index;
                        palette.invalid = Some((index, problem));
                    } else {
                        palette.invalid = None;
                        if matches!(command, Command::CreateBranch) {
                            let project = field_value(fields, "Project");
                            if !palette.suggestions.cache.contains_key(&project) {
                                palette.submit_when_ready = true;
                                self.palette = Some(palette);
                                return action;
                            }
                            let branch = field_value(fields, "Branch name");
                            let base = field_value(fields, "Base");
                            action = self.submit_create_branch(
                                &mut palette,
                                project,
                                branch,
                                base,
                                history_before_request,
                                false,
                            );
                            self.palette = Some(palette);
                            return action;
                        }
                        if matches!(command, Command::CreateWorkspace)
                            && !palette
                                .suggestions
                                .cache
                                .contains_key(&field_value(fields, "Project"))
                        {
                            palette.submit_when_ready = true;
                            self.palette = Some(palette);
                            return action;
                        }
                        let field = |label: &str| {
                            fields
                                .iter()
                                .find(|f| f.label == label)
                                .map(|f| f.value.trim().to_string())
                                .unwrap_or_default()
                        };
                        let branch_value = field("Branch");
                        if matches!(command, Command::NewAgentSession)
                            && created_branch_name(&branch_value).is_some()
                        {
                            let project = field("Project");
                            if !palette.suggestions.cache.contains_key(&project) {
                                palette.submit_when_ready = true;
                                self.palette = Some(palette);
                                return action;
                            }
                        }
                        // Recovery is a new launch, never a replay of the initial message.
                        let task = if matches!(command, Command::RecoverLaunch) {
                            String::new()
                        } else {
                            form_task(fields)
                        };
                        let mut chained: Option<ovrcr_protocol::CreateAgentSessionRequest> = None;
                        let launch_task = if matches!(command, Command::CreateWorkspace) {
                            ""
                        } else {
                            &task
                        };
                        let selected_launch = if matches!(
                            command,
                            Command::CreateTerminal
                                | Command::NewAgentSession
                                | Command::CreateWorkspace
                                | Command::RecoverLaunch
                        ) {
                            match self.selected_launch(
                                fields,
                                launch_task,
                                palette.model_capabilities.as_ref(),
                            ) {
                                Ok(launch) => launch,
                                Err(error) => {
                                    palette.error = Some(error);
                                    self.palette = Some(palette);
                                    return action;
                                }
                            }
                        } else {
                            None
                        };
                        if matches!(command, Command::NewAgentSession)
                            && let Some(branch) = created_branch_name(&branch_value)
                        {
                            let selected = selected_launch.as_ref().unwrap();
                            if !matches!(
                                &selected.model_policy,
                                ovrcr_protocol::AgentModelPolicy::HarnessDefault
                            ) {
                                let prompt = field("Initial prompt");
                                let prompt_launch = ovrcr_protocol::CreateAgentSessionRequest {
                                    session: CreateSessionRequest {
                                        project: field("Project"),
                                        workspace: String::new(),
                                        name: field("Name"),
                                        argv: selected.argv.clone(),
                                        label: selected.label.clone(),
                                        kind: selected.kind.clone(),
                                    },
                                    model_policy: selected.model_policy.clone(),
                                    model_binding: selected.model_binding.clone(),
                                    expected_model: selected.expected_model.clone(),
                                };
                                if !prompt.trim().is_empty()
                                    && !self.supports_initial_prompt_launch(&prompt_launch)
                                {
                                    palette.error = Some(
                                        "Initial prompts require a current detected Claude Code, Pi, Codex, or Oh My Pi launcher. Clear the prompt or choose one of those detected launchers.".into(),
                                    );
                                    self.palette = Some(palette);
                                    return action;
                                }
                                let project = field("Project");
                                let base = field("Base");
                                action = self.submit_create_branch(
                                    &mut palette,
                                    project,
                                    branch.to_owned(),
                                    base,
                                    history_before_request,
                                    true,
                                );
                                self.palette = Some(palette);
                                return action;
                            }
                            let workspace_id = if let Some(id) = palette.workspace_id.clone() {
                                id
                            } else {
                                match new_workspace_id() {
                                    Ok(id) => {
                                        palette.workspace_id = Some(id.clone());
                                        id
                                    }
                                    Err(error) => {
                                        palette.error = Some(format!(
                                            "Could not allocate workspace identity: {error}"
                                        ));
                                        self.palette = Some(palette);
                                        return action;
                                    }
                                }
                            };
                            let project = field("Project");
                            let launch = ovrcr_protocol::CreateAgentSessionRequest {
                                session: CreateSessionRequest {
                                    project: project.clone(),
                                    workspace: workspace_id.clone(),
                                    name: field("Name"),
                                    argv: selected.argv.clone(),
                                    label: selected.label.clone(),
                                    kind: selected.kind.clone(),
                                },
                                model_policy: selected.model_policy.clone(),
                                model_binding: selected.model_binding.clone(),
                                expected_model: selected.expected_model.clone(),
                            };
                            let request = Request::CreateWorkspaceWithLaunch {
                                project,
                                id: workspace_id,
                                launch: None,
                                branch: BranchRequest::New {
                                    branch: branch.to_owned(),
                                    base: field("Base"),
                                },
                            };
                            action =
                                self.palette_submit(&mut palette, request, history_before_request);
                            self.chain_session(&mut palette, launch);
                            self.palette = Some(palette);
                            return action;
                        }
                        if matches!(command, Command::CreateWorkspace)
                            && selected_launch.as_ref().is_some_and(|launch| {
                                matches!(
                                    &launch.model_policy,
                                    ovrcr_protocol::AgentModelPolicy::Explicit(_)
                                )
                            })
                        {
                            palette.error = Some(
                                "Explicit model selection requires an already registered workspace."
                                    .into(),
                            );
                            self.palette = Some(palette);
                            return action;
                        }
                        let request = match command {
                            Command::NewAgentSession => {
                                let (project, workspace) = split_workspace_pick(&field("Branch"));
                                let selected = selected_launch.as_ref().unwrap();
                                let session = CreateSessionRequest {
                                    project,
                                    workspace,
                                    name: field("Name"),
                                    argv: selected.argv.clone(),
                                    label: selected.label.clone(),
                                    kind: selected.kind.clone(),
                                };
                                Request::CreateAgentSession(
                                    ovrcr_protocol::CreateAgentSessionRequest {
                                        session,
                                        model_policy: selected.model_policy.clone(),
                                        model_binding: selected.model_binding.clone(),
                                        expected_model: selected.expected_model.clone(),
                                    },
                                )
                            }
                            Command::CreateTerminal => {
                                let (project, id) = split_workspace_pick(&field("Workspace"));
                                let Some(workspace) = find_workspace(self, &project, &id) else {
                                    palette.error =
                                        Some("Workspace not found in available choices".into());
                                    self.palette = Some(palette);
                                    return action;
                                };
                                if let Some(warning) = &workspace.warning {
                                    palette.error = Some(warning.clone());
                                    self.palette = Some(palette);
                                    return action;
                                }
                                let name = field("Name");
                                let selected = selected_launch.as_ref().unwrap();
                                let session = CreateSessionRequest {
                                    project: workspace.project.clone(),
                                    workspace: workspace.id.clone(),
                                    name: if name.is_empty() {
                                        task_name(&task)
                                    } else {
                                        name
                                    },
                                    argv: selected.argv.clone(),
                                    label: selected.label.clone(),
                                    kind: selected.kind.clone(),
                                };
                                if matches!(&session.kind, SessionKind::Agent { .. }) {
                                    Request::CreateAgentSession(
                                        ovrcr_protocol::CreateAgentSessionRequest {
                                            session,
                                            model_policy: selected.model_policy.clone(),
                                            model_binding: selected.model_binding.clone(),
                                            expected_model: selected.expected_model.clone(),
                                        },
                                    )
                                } else {
                                    Request::CreateSession(session)
                                }
                            }
                            Command::RenameTerminal(session) => Request::SetSessionTitle {
                                session: *session,
                                title: {
                                    let name = field("Name");
                                    (!name.is_empty()).then_some(name)
                                },
                            },
                            Command::CreateWorkspace => {
                                let id = if let Some(id) = palette.workspace_id.clone() {
                                    id
                                } else {
                                    match new_workspace_id() {
                                        Ok(id) => {
                                            palette.workspace_id = Some(id.clone());
                                            id
                                        }
                                        Err(error) => {
                                            palette.error = Some(format!(
                                                "Could not allocate workspace identity: {error}"
                                            ));
                                            self.palette = Some(palette);
                                            return action;
                                        }
                                    }
                                };
                                // Agent starts follow workspace registration as a separate
                                // lifecycle step so blank and prompted starts share one-use
                                // intent admission. The Task field names a branch; Prompt is
                                // carried separately and never becomes an argv/name/branch.
                                let agent_launch = selected_launch.as_ref().is_some_and(|launch| {
                                    matches!(&launch.kind, SessionKind::Agent { .. })
                                });
                                let launch = if agent_launch || !task.is_empty() {
                                    chained = selected_launch.clone().map(|launch| {
                                        ovrcr_protocol::CreateAgentSessionRequest {
                                            session: CreateSessionRequest {
                                                project: field("Project"),
                                                workspace: id.clone(),
                                                name: if agent_launch {
                                                    String::new()
                                                } else {
                                                    task_name(&task)
                                                },
                                                argv: launch.argv,
                                                label: launch.label,
                                                kind: launch.kind,
                                            },
                                            model_policy: launch.model_policy,
                                            model_binding: launch.model_binding,
                                            expected_model: launch.expected_model,
                                        }
                                    });
                                    None
                                } else {
                                    selected_launch.clone()
                                };
                                Request::CreateWorkspaceWithLaunch {
                                    project: field("Project"),
                                    id,
                                    launch,
                                    branch: if field("Branch mode") == "existing" {
                                        BranchRequest::Existing {
                                            branch: field("Branch"),
                                        }
                                    } else {
                                        BranchRequest::New {
                                            branch: field("Branch"),
                                            base: field("Base"),
                                        }
                                    },
                                }
                            }
                            Command::RecoverLaunch => {
                                let previous = palette.failed_launch.as_ref().unwrap();
                                let registered = find_workspace(
                                    self,
                                    &previous.session.project,
                                    &previous.session.workspace,
                                )
                                .is_some();
                                if !registered {
                                    palette.error = Some("Workspace is not yet registered in the dashboard. Wait for the hierarchy update before retrying.".into());
                                    self.palette = Some(palette);
                                    return action;
                                }
                                let selected = selected_launch.as_ref().unwrap();
                                let session = CreateSessionRequest {
                                    project: previous.session.project.clone(),
                                    workspace: previous.session.workspace.clone(),
                                    name: previous.session.name.clone(),
                                    argv: selected.argv.clone(),
                                    label: selected.label.clone(),
                                    kind: selected.kind.clone(),
                                };
                                if matches!(&session.kind, SessionKind::Agent { .. }) {
                                    Request::CreateAgentSession(
                                        ovrcr_protocol::CreateAgentSessionRequest {
                                            session,
                                            model_policy: selected.model_policy.clone(),
                                            model_binding: selected.model_binding.clone(),
                                            expected_model: selected.expected_model.clone(),
                                        },
                                    )
                                } else {
                                    Request::CreateSession(session)
                                }
                            }
                            Command::RegisterProject => Request::AddProject {
                                name: field("Name"),
                                repo: expand_path(&field("Repository")),
                                workspace_root: expand_path(&field("Workspace root")),
                            },
                            Command::RemoveWorkspace => {
                                let (project, id) = split_workspace_pick(&field("Workspace"));
                                let Some(workspace) = find_workspace(self, &project, &id) else {
                                    palette.error =
                                        Some("Workspace not found in available choices".into());
                                    self.palette = Some(palette);
                                    return action;
                                };
                                if workspace.root {
                                    palette.error = Some(super::keymap::ROOT_PROTECTED.to_string());
                                    self.palette = Some(palette);
                                    return action;
                                }
                                Request::RemoveWorkspace {
                                    project: workspace.project.clone(),
                                    name: workspace.id.clone(),
                                    force: false,
                                }
                            }
                            Command::RemoveProject => Request::RemoveProject {
                                name: field("Project"),
                            },
                            _ => unreachable!(),
                        };
                        if matches!(command, Command::RemoveWorkspace | Command::RemoveProject) {
                            palette.page = self.removal_page(request);
                        } else {
                            action =
                                self.palette_submit(&mut palette, request, history_before_request);
                            if let Some(session) = chained {
                                self.chain_session(&mut palette, session);
                            }
                        }
                    }
                }
                Page::Confirm { request, .. } => {
                    if let Some(request) = request.as_deref().cloned() {
                        // Removal first stops the running sessions it named, one
                        // Lifecycle job at a time, then removes the workspace.
                        let mut steps = self.removal_steps(&request);
                        let first = steps.remove(0);
                        action = self.palette_submit(&mut palette, first, history_before_request);
                        if let Some(row) = self
                            .lifecycle_pending
                            .as_mut()
                            .filter(|row| Some(row.token) == palette.pending)
                        {
                            row.then = steps;
                        }
                    } else if palette.connect_status {
                        self.palette = None;
                        return self.connect_agent_status();
                    } else {
                        self.palette = None;
                        return DashboardAction::Redraw;
                    }
                }
            },
            _ => {}
        }
        self.refresh_workspace_form(&mut palette);
        self.palette = Some(palette);
        action
    }

    pub(super) fn detected_agents(&self) -> Vec<super::agents::AgentEntry> {
        #[cfg(test)]
        if let Some(entries) = &self.model_agent_entries {
            return entries.clone();
        }
        let path = std::env::var_os("PATH").unwrap_or_default();
        let shell = std::env::var_os("SHELL");
        let launcher = resolve_agent_launcher(
            std::env::var_os("OVRCR_SERVER_EXECUTABLE").map(PathBuf::from),
            std::env::current_exe().ok(),
            std::env::current_dir().ok(),
        );
        let mut entries = apply_overrides(
            detect_agents(&path, shell.as_deref(), launcher.as_deref()),
            &self.settings.agents,
        );
        entries.retain(|entry| {
            entry.name != "cursor-cloud" && !entry.name.starts_with("cursor-cloud:")
        });
        if cfg!(target_os = "macos")
            && let Some(launcher) = launcher
        {
            for account in &self.settings.cursor_cloud_accounts {
                if account.validate().is_ok() {
                    entries.insert(
                        entries.len().saturating_sub(2),
                        super::agents::AgentEntry {
                            name: format!("cursor-cloud:{}", account.name),
                            argv: vec![
                                launcher.as_os_str().to_owned(),
                                "agent".into(),
                                "run".into(),
                                "cursor-cloud".into(),
                                "--".into(),
                                account.name.clone().into(),
                            ],
                            source: AgentSource::Detected,
                            origin: super::agents::AgentOrigin::Cloud,
                        },
                    );
                }
            }
        }
        entries
    }

    fn refresh_terminal_form(&self, fields: &mut [Field], _name_edited: bool) {
        let Some(start) = fields
            .iter()
            .find(|f| f.label == "Start")
            .map(|f| f.value.clone())
        else {
            return;
        };
        // Create workspace always shows Task: it names the branch too.
        let creates_workspace = fields.iter().any(|f| f.label == "Project");
        for field in fields {
            if field.label == "Task" && !creates_workspace {
                field.hidden = start != "Agent";
            }
            if field.label == "Initial prompt" {
                field.hidden = start != "Agent";
            }
            if field.label == "Agent" {
                field.hidden = start != "Agent";
                field.required = start == "Agent";
            }
            if field.label == "Model" {
                field.hidden = start != "Agent";
                field.required = start == "Agent";
            }
            if field.label == "Command" {
                field.hidden = start != "Terminal";
                field.required = false;
            }
        }
    }

    // Keep keystrokes cheap: full launcher discovery reads PATH and inspects executable files.
    fn current_model_query_inputs(
        &self,
    ) -> Option<(&'static str, String, String, Option<Vec<String>>)> {
        let Page::Form {
            command, fields, ..
        } = &self.palette.as_ref()?.page
        else {
            return None;
        };
        if !matches!(command, Command::NewAgentSession) && field_value(fields, "Start") != "Agent" {
            return None;
        }
        let command = match command {
            Command::CreateTerminal => "terminal",
            Command::CreateWorkspace => "workspace",
            Command::NewAgentSession => "new_agent_session",
            _ => return None,
        };
        let preset = agent_field_value(fields);
        let configured_argv = self
            .settings
            .agents
            .iter()
            .find(|agent| agent.name == preset)
            .map(|agent| agent.argv.clone());
        Some((
            command,
            preset,
            if command == "new_agent_session" {
                let branch = field_value(fields, "Branch");
                if created_branch_name(&branch).is_some()
                    && field_value(fields, "Model") != HARNESS_DEFAULT_MODEL
                {
                    self.palette
                        .as_ref()
                        .and_then(|palette| palette.parent_branch_target.as_ref())
                        .map(|(target, _)| target.clone())
                        .unwrap_or(branch)
                } else {
                    branch
                }
            } else {
                field_value(fields, "Workspace")
            },
            configured_argv,
        ))
    }

    fn refresh_model_query_if_inputs_changed(
        &mut self,
        previous_inputs: Option<(&'static str, String, String, Option<Vec<String>>)>,
    ) {
        if self.current_model_query_inputs() == previous_inputs {
            return;
        }
        if let Some(mut palette) = self.palette.take() {
            self.refresh_model_query(&mut palette);
            self.palette = Some(palette);
        }
    }

    fn refresh_model_query(&mut self, palette: &mut Palette) {
        let Page::Form {
            command, fields, ..
        } = &palette.page
        else {
            palette.model_capabilities = None;
            palette.model_capability_identity = None;
            palette.model_status = None;
            return;
        };
        let discovered;
        let agents = if let Some(catalog) = palette.agent_catalog.as_deref() {
            catalog
        } else {
            discovered = self.detected_agents();
            &discovered
        };
        let target = match model_query_target_with_branch_fallback(
            command,
            fields,
            agents,
            palette
                .parent_branch_target
                .as_ref()
                .map(|(target, _)| target.as_str()),
        ) {
            Ok(None) => {
                palette.model_capabilities = None;
                palette.model_capability_identity = None;
                palette.model_status = None;
                if let Page::Form { fields, .. } = &mut palette.page {
                    set_model_options(fields, None);
                }
                return;
            }
            Err(reason) => {
                palette.model_capability_identity = None;
                let preset = agent_field_value(fields);
                let capabilities = crate::protocol::AgentModelCapabilities {
                    preset,
                    binding: None,
                    models: Vec::new(),
                    reason: Some(reason.clone()),
                };
                palette.model_capabilities = Some(capabilities.clone());
                palette.model_status = Some(reason);
                if let Page::Form { fields, .. } = &mut palette.page {
                    set_model_options(fields, Some(&capabilities));
                }
                return;
            }
            Ok(Some(target)) => target,
        };
        if let Some((_, current)) = &self.model_query {
            if current == &target.identity {
                palette.model_capabilities = None;
                palette.model_capability_identity = None;
                palette.model_status =
                    Some("Waiting for the current model lookup to finish…".into());
                return;
            }
            palette.model_capabilities = None;
            palette.model_capability_identity = None;
            palette.model_status = Some("Waiting for the previous model lookup to finish…".into());
            if let Page::Form { fields, .. } = &mut palette.page {
                set_model_options(fields, None);
            }
            return;
        }
        if palette.model_capability_identity.as_deref() == Some(target.identity.as_str())
            && palette
                .model_capabilities
                .as_ref()
                .is_some_and(|capabilities| {
                    capabilities.binding.is_some() && capabilities.reason.is_none()
                })
        {
            palette.model_status = None;
            return;
        }
        let request_id = self.next_request_id();
        self.model_query = Some((request_id, target.identity));
        palette.model_capabilities = None;
        palette.model_capability_identity = None;
        palette.model_status = Some("Loading verified model choices…".into());
        if let Page::Form { fields, .. } = &mut palette.page {
            set_model_options(fields, None);
        }
        self.push_request(ClientMessage {
            request_id,
            request: Request::QueryAgentModels {
                preset: target.preset,
                origin: target.origin,
                argv: target.argv,
                project: target.project,
                workspace: target.workspace,
            },
        });
    }

    pub(super) fn agent_models_event(
        &mut self,
        request_id: u64,
        capabilities: crate::protocol::AgentModelCapabilities,
    ) {
        let Some((current_request, requested_identity)) = self.model_query.as_ref() else {
            return;
        };
        if *current_request != request_id {
            return;
        }
        let requested_identity = requested_identity.clone();
        self.model_query = None;
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let current_identity = match &palette.page {
            Page::Form {
                command, fields, ..
            } => {
                let discovered;
                let agents = if let Some(catalog) = palette.agent_catalog.as_deref() {
                    catalog
                } else {
                    discovered = self.detected_agents();
                    &discovered
                };
                model_query_target_with_branch_fallback(
                    command,
                    fields,
                    agents,
                    palette
                        .parent_branch_target
                        .as_ref()
                        .map(|(target, _)| target.as_str()),
                )
                .ok()
                .flatten()
                .map(|target| target.identity)
            }
            _ => None,
        };
        if current_identity.as_deref() == Some(requested_identity.as_str())
            && matches!(&palette.page, Page::Form { fields, .. } if agent_field_value(fields) == capabilities.preset)
        {
            if let Page::Form { fields, .. } = &mut palette.page {
                set_model_options(fields, Some(&capabilities));
            }
            palette.model_capability_identity = (capabilities.binding.is_some()
                && capabilities.reason.is_none())
            .then_some(requested_identity);
            palette.model_status = capabilities.reason.clone();
            palette.model_capabilities = Some(capabilities);
        } else {
            self.refresh_model_query(&mut palette);
        }
        self.palette = Some(palette);
    }

    fn launch_fields(&self, project: &str, empty: bool) -> Vec<Field> {
        let choice = self.settings.launch_choices.get(project);
        let start = match choice {
            Some(super::settings::LaunchChoice::Agent { .. }) => "Agent",
            _ => "Terminal",
        };
        let mut choices = vec!["Agent", "Terminal"];
        if empty {
            choices.push("Nothing yet");
        }
        let agents = self
            .detected_agents()
            .into_iter()
            .filter(|a| {
                a.source != AgentSource::Shell
                    && a.source != AgentSource::Custom
                    && a.name != "shell"
                    && a.name != "Custom"
            })
            .collect::<Vec<_>>();
        let agent = match choice {
            Some(super::settings::LaunchChoice::Agent { preset, .. }) => preset.clone(),
            _ => agents.first().map(|a| a.name.clone()).unwrap_or_default(),
        };
        let model = match choice {
            Some(super::settings::LaunchChoice::Agent {
                model_policy: ovrcr_protocol::AgentModelPolicy::Explicit(model),
                ..
            }) => model.clone(),
            _ => HARNESS_DEFAULT_MODEL.into(),
        };
        let mut fields = vec![
            pick_field(
                "Start",
                start.into(),
                choices
                    .into_iter()
                    .map(|s| PickItem {
                        label: s.into(),
                        value: s.into(),
                    })
                    .collect(),
            ),
            pick_field(
                "Agent",
                agent,
                agents
                    .into_iter()
                    .map(|a| PickItem {
                        label: a.name.clone(),
                        value: a.name,
                    })
                    .collect(),
            ),
            pick_field(
                "Model",
                model,
                vec![PickItem {
                    label: "Harness default".into(),
                    value: HARNESS_DEFAULT_MODEL.into(),
                }],
            ),
            text_field("Command", String::new(), false),
        ];
        self.refresh_terminal_form(&mut fields, false);
        fields
    }

    fn new_agent_session_form(&self, project: &str, catalog: &[super::agents::AgentEntry]) -> Page {
        let agents = catalog
            .iter()
            .filter(|agent| {
                agent.source != AgentSource::Shell
                    && agent.source != AgentSource::Custom
                    && agent.name != "shell"
                    && agent.name != "Custom"
            })
            .collect::<Vec<_>>();
        let remembered = self.settings.launch_choices.get(project);
        let remembered_agent = match remembered {
            Some(super::settings::LaunchChoice::Agent { preset, .. }) => Some(preset.as_str()),
            _ => None,
        };
        let agent = remembered_agent
            .map(str::to_owned)
            .or_else(|| agents.first().map(|agent| agent.name.clone()))
            .unwrap_or_default();
        let model = match remembered {
            Some(super::settings::LaunchChoice::Agent {
                preset,
                model_policy: ovrcr_protocol::AgentModelPolicy::Explicit(model),
            }) if preset == &agent => model.clone(),
            _ => HARNESS_DEFAULT_MODEL.into(),
        };
        let mut project = text_field("Project", project.into(), true);
        project.kind = FieldKind::Pick(PickList::new(Vec::new()));
        let branch = pick_field("Branch", String::new(), Vec::new());
        let destination = notice_field(
            "Destination",
            "Loading registered workspaces…".into(),
            false,
        );
        let branch_base = notice_field("Branch base", String::new(), true);
        let shared = notice_field("Shared files", String::new(), true);
        let harness = pick_field(
            "Agent harness",
            agent,
            agents
                .into_iter()
                .map(|agent| PickItem {
                    label: agent.name.clone(),
                    value: agent.name.clone(),
                })
                .collect(),
        );
        let model = pick_field(
            "Model",
            model,
            vec![PickItem {
                label: "Harness default".into(),
                value: HARNESS_DEFAULT_MODEL.into(),
            }],
        );
        let prompt = Field {
            label: "Initial prompt",
            value: String::new(),
            required: false,
            hidden: false,
            edited: false,
            kind: FieldKind::Multiline,
        };
        let mut advanced = text_field("Advanced options", "collapsed".into(), false);
        advanced.kind = FieldKind::Toggle;
        let mut base = text_field("Base", String::new(), true);
        base.hidden = true;
        let mut name = text_field("Name", String::new(), false);
        name.hidden = true;
        let fields = vec![
            project,
            branch,
            destination,
            branch_base,
            shared,
            harness,
            model,
            prompt,
            advanced,
            base,
            name,
        ];
        let active = 0;
        Page::Form {
            command: Command::NewAgentSession,
            fields,
            active,
            name_edited: false,
            root_edited: false,
        }
    }

    fn shell_argv(&self) -> Vec<OsString> {
        self.detected_agents()
            .into_iter()
            .find(|entry| entry.source == AgentSource::Shell)
            .expect("agent detection always includes the shell entry")
            .argv
    }

    fn selected_launch(
        &self,
        fields: &[Field],
        task: &str,
        model_capabilities: Option<&crate::protocol::AgentModelCapabilities>,
    ) -> Result<Option<SessionLaunch>, String> {
        let value = |label| {
            fields
                .iter()
                .find(|f| f.label == label)
                .map(|f| f.value.trim())
                .unwrap_or("")
        };
        match value("Start") {
            "Nothing yet" => Ok(None),
            "Agent" | "" if !agent_field_value(fields).is_empty() => {
                let agent = self
                    .detected_agents()
                    .into_iter()
                    .find(|a| {
                        a.name == agent_field_value(fields)
                            && a.source != AgentSource::Shell
                            && a.source != AgentSource::Custom
                            && a.name != "shell"
                            && a.name != "Custom"
                    })
                    .ok_or_else(|| unavailable_agent(&agent_field_value(fields)))?;
                let (model_policy, model_binding, expected_model) =
                    if value("Model") == HARNESS_DEFAULT_MODEL {
                        (ovrcr_protocol::AgentModelPolicy::HarnessDefault, None, None)
                    } else {
                        let capabilities = model_capabilities.ok_or_else(|| {
                            "Verified models are still loading for this launcher.".to_string()
                        })?;
                        if let Some(reason) = &capabilities.reason {
                            return Err(reason.clone());
                        }
                        let model = capabilities
                            .models
                            .iter()
                            .find(|model| model.alias == value("Model"))
                            .ok_or_else(|| {
                                "The selected model is no longer verified; refresh the model list."
                                    .to_string()
                            })?;
                        (
                            ovrcr_protocol::AgentModelPolicy::Explicit(model.alias.clone()),
                            capabilities.binding.clone(),
                            Some(model.resolved.clone()),
                        )
                    };
                Ok(Some(SessionLaunch {
                    argv: super::agents::with_task(&agent.name, agent.argv, task),
                    kind: SessionKind::Agent {
                        name: agent.name.clone(),
                    },
                    label: Some(agent.name),
                    model_policy,
                    model_binding,
                    expected_model,
                }))
            }
            "Terminal" => Ok(Some(SessionLaunch {
                argv: if value("Command").is_empty() {
                    self.shell_argv()
                } else {
                    vec![
                        "/bin/sh".into(),
                        "-lc".into(),
                        OsString::from(value("Command")),
                    ]
                },
                label: None,
                kind: SessionKind::Terminal,
                model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
                model_binding: None,
                expected_model: None,
            })),
            _ => Err("Choose Agent, Terminal, or Nothing yet".into()),
        }
    }

    /// The first field, top to bottom, the Server would refuse or misread,
    /// and what to tell the person beside it.
    fn form_problem(
        &self,
        command: &Command,
        fields: &[Field],
        registry: Option<&crate::protocol::Registry>,
    ) -> Option<(usize, String)> {
        let new_branch = fields
            .iter()
            .any(|field| field.label == "Branch mode" && field.value == "new");
        visible_indices(fields).into_iter().find_map(|index| {
            let field = &fields[index];
            let value = field.value.trim();
            let problem = if field.required && value.is_empty() {
                Some(format!("{} is required", field.label))
            } else if let FieldKind::Pick(list) = &field.kind
                && !list.items.iter().any(|item| item.value == field.value)
            {
                Some(match field.label {
                    "Agent" => unavailable_agent(value),
                    "Model" => "Model is not in the verified choices for this launcher".into(),
                    "Branch" => "Branch not found in repository".into(),
                    label => format!("{label} not found in available choices"),
                })
            } else if matches!(command, Command::CreateWorkspace) && field.label == "Branch" {
                let prefix = self.settings.branch_prefix.trim();
                if new_branch && !prefix.is_empty() && value == prefix {
                    Some(format!("Add a name after {prefix}"))
                } else {
                    branch_name_problem(value).map(Into::into)
                }
            } else if matches!(command, Command::CreateBranch) && field.label == "Branch name" {
                branch_name_problem(value).map(Into::into)
            } else if matches!(command, Command::NewAgentSession) && field.label == "Branch" {
                let Some(registry) = registry else {
                    return Some((index, "Registered workspaces are still loading".into()));
                };
                if let Some(branch) = created_branch_name(&field.value) {
                    if !registry
                        .projects
                        .iter()
                        .any(|project| project.name == field_value(fields, "Project"))
                    {
                        return Some((index, "Choose a registered project".into()));
                    }
                    branch_name_problem(branch).map(Into::into)
                } else {
                    let (project, workspace_id) = split_workspace_pick(&field.value);
                    if project != field_value(fields, "Project") {
                        return Some((index, "Choose a branch from the selected project".into()));
                    }
                    let Some(record) = registry
                        .projects
                        .iter()
                        .find(|record| record.name == project)
                        .and_then(|project| {
                            project
                                .workspaces
                                .iter()
                                .find(|workspace| workspace.id == workspace_id)
                        })
                    else {
                        return Some((index, "Choose a registered checkout".into()));
                    };
                    if record.setup_pending {
                        Some("Workspace setup is still pending".into())
                    } else {
                        match find_workspace(self, &project, &workspace_id) {
                            Some(workspace) if workspace.path != record.path => Some(
                                "Registered checkout identity changed; refresh and choose again"
                                    .into(),
                            ),
                            Some(workspace) if workspace.name == "checkout unavailable" => {
                                Some("Registered checkout is unavailable".into())
                            }
                            Some(workspace) if workspace.warning.is_some() => {
                                workspace.warning.clone()
                            }
                            None => Some("Registered checkout is unavailable".into()),
                            _ => None,
                        }
                    }
                }
            } else {
                None
            };
            problem.map(|problem| (index, problem))
        })
    }

    fn create_branch_resolution_problem(
        &self,
        fields: &[Field],
        suggestions: &Suggestions,
    ) -> Option<(usize, String)> {
        let field_index = |label: &str| fields.iter().position(|field| field.label == label);
        let project = field_value(fields, "Project");
        let branch = field_value(fields, "Branch name");
        let options_index = field_index("Workspace options").unwrap_or(0);
        let branch_index = field_index("Branch name").unwrap_or(0);
        match suggestions.cache.get(&project) {
            None => return None,
            Some(Err(error)) => {
                return Some((
                    options_index,
                    format!(
                        "Cannot resolve a Base branch ({error}). Choose another project or restore its repository."
                    ),
                ));
            }
            Some(Ok(hints)) if hints.branches.contains(&branch) => {
                return Some((
                    branch_index,
                    "That branch already exists in this repository".into(),
                ));
            }
            Some(Ok(_)) => {}
        }
        let hints = suggestions
            .cache
            .get(&project)
            .and_then(|result| result.as_ref().ok())?;
        let base_index = field_index("Base").unwrap_or(options_index);
        let base = field_value(fields, "Base");
        if base.is_empty() {
            return Some((base_index, "Base is required".into()));
        }
        if let Some(problem) = branch_name_problem(&base) {
            return Some((base_index, problem.into()));
        }
        if base != hints.base && !hints.branches.contains(&base) {
            return Some((
                base_index,
                "Base must resolve to a local branch or the detected repository default".into(),
            ));
        }
        None
    }

    fn new_agent_branch_resolution_problem(
        &self,
        fields: &[Field],
        suggestions: &Suggestions,
    ) -> Option<(usize, String)> {
        let branch_value = field_value(fields, "Branch");
        let branch = created_branch_name(&branch_value)?;
        let project = field_value(fields, "Project");
        let branch_index = fields.iter().position(|field| field.label == "Branch")?;
        let base_index = fields.iter().position(|field| field.label == "Base")?;
        let base = field_value(fields, "Base");
        let result = suggestions.cache.get(&project)?;
        let hints = match result {
            Ok(hints) => hints,
            Err(error) => {
                return Some((
                    base_index,
                    format!(
                        "Cannot verify the default branch ({error}). Expand Advanced options to set Base."
                    ),
                ));
            }
        };
        if hints.branches.contains(&branch.to_owned()) {
            return Some((
                branch_index,
                "That branch already exists in this repository".into(),
            ));
        }
        if let Some(problem) = branch_name_problem(branch) {
            return Some((branch_index, problem.into()));
        }
        if base.is_empty() {
            return Some((
                base_index,
                "Base is required; expand Advanced options to set it".into(),
            ));
        }
        if let Some(problem) = branch_name_problem(&base) {
            return Some((base_index, problem.into()));
        }
        if base != hints.base && !hints.branches.contains(&base) {
            return Some((
                base_index,
                "Base must resolve to a local branch or the detected repository default".into(),
            ));
        }
        None
    }

    fn submit_create_branch(
        &mut self,
        palette: &mut Palette,
        project: String,
        branch: String,
        base: String,
        history_before_request: bool,
        agent_form: bool,
    ) -> DashboardAction {
        if palette
            .branch_attempt
            .as_ref()
            .is_some_and(|attempt| attempt.blocked)
        {
            palette.error = Some(
                "The previous branch attempt is still uncertain. Reconcile it before retrying."
                    .into(),
            );
            return DashboardAction::Redraw;
        }
        let workspace_id = if let Some(id) = palette.workspace_id.clone() {
            id
        } else {
            match new_workspace_id() {
                Ok(id) => {
                    palette.workspace_id = Some(id.clone());
                    id
                }
                Err(error) => {
                    palette.error = Some(format!("Could not allocate workspace identity: {error}"));
                    return DashboardAction::Redraw;
                }
            }
        };
        let request = Request::CreateWorkspace {
            project: project.clone(),
            id: workspace_id.clone(),
            branch: BranchRequest::New {
                branch: branch.clone(),
                base,
            },
        };
        let token = self.next_request_id();
        palette.branch_attempt = Some(BranchAttempt {
            token,
            project,
            workspace_id,
            branch,
            accepted: false,
            outcome: None,
            inspect_request: None,
            list_request: None,
            registry: None,
            hierarchy: None,
            inspect_complete: false,
            list_complete: false,
            reconciliation_error: None,
            blocked: false,
            agent_form,
        });
        palette.pending = Some(token);
        palette.error = None;
        palette.invalid = None;
        self.begin_lifecycle_pending(token, &request);
        if history_before_request {
            self.queue_history_request();
        }
        self.push_request(ClientMessage {
            request_id: token,
            request,
        });
        DashboardAction::Redraw
    }

    pub(super) fn on_branch_lifecycle_completed(
        &mut self,
        token: u64,
        op: crate::protocol::LifecycleOp,
        outcome: crate::protocol::LifecycleOutcome,
        target: Option<crate::protocol::WorkspaceLifecycleIdentity>,
    ) -> Option<DashboardAction> {
        let attempt = self
            .palette
            .as_ref()?
            .branch_attempt
            .as_ref()
            .filter(|attempt| attempt.token == token)?;
        let matches_target = op == crate::protocol::LifecycleOp::CreateWorkspace
            && target.as_ref().is_some_and(|target| {
                target.project == attempt.project && target.id == attempt.workspace_id
            });
        if !matches_target {
            // The connection-local token may have been reused by a replacement Dashboard.
            return Some(DashboardAction::None);
        }
        if let Some(palette) = self.palette.as_mut()
            && let Some(attempt) = palette.branch_attempt.as_mut()
        {
            attempt.outcome = Some(outcome.clone());
        }
        if self
            .lifecycle_pending
            .as_ref()
            .is_some_and(|row| row.token == token)
        {
            self.lifecycle_pending = None;
        }
        if matches!(&outcome, crate::protocol::LifecycleOutcome::Failed { code, .. }
            if *code != crate::protocol::ErrorCode::PartialFailure)
        {
            let mut palette = self.palette.take()?;
            palette.pending = None;
            palette.branch_attempt = None;
            if let crate::protocol::LifecycleOutcome::Failed { message, .. } = &outcome {
                palette.error = Some(self.plain_failure(&palette, message));
            }
            self.palette = Some(palette);
            return Some(DashboardAction::Redraw);
        }
        if let Some(palette) = self.palette.as_mut()
            && let Some(attempt) = palette.branch_attempt.as_mut()
            && matches!(&outcome, crate::protocol::LifecycleOutcome::Failed { .. })
        {
            attempt.blocked = true;
        }
        if self
            .palette
            .as_ref()
            .and_then(|palette| palette.branch_attempt.as_ref())
            .is_some_and(|attempt| {
                attempt.accepted
                    && (attempt.inspect_request.is_none()
                        || (attempt.inspect_complete && attempt.list_complete))
            })
        {
            self.start_branch_reconciliation(token);
        }
        Some(DashboardAction::Redraw)
    }

    fn start_branch_reconciliation(&mut self, token: u64) {
        let inspect_request = self.next_request_id();
        let list_request = self.next_request_id();
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        let Some(attempt) = palette
            .branch_attempt
            .as_mut()
            .filter(|attempt| attempt.token == token)
        else {
            return;
        };
        attempt.inspect_request = Some(inspect_request);
        attempt.list_request = Some(list_request);
        attempt.registry = None;
        attempt.hierarchy = None;
        attempt.inspect_complete = false;
        attempt.list_complete = false;
        attempt.reconciliation_error = None;
        palette.pending = Some(token);
        self.push_request(ClientMessage {
            request_id: inspect_request,
            request: Request::Inspect,
        });
        self.push_request(ClientMessage {
            request_id: list_request,
            request: Request::List,
        });
    }

    fn finish_branch_reconciliation(&mut self, token: u64) -> Vec<ClientMessage> {
        let Some(mut palette) = self.palette.take() else {
            return Vec::new();
        };
        let Some(attempt) = palette
            .branch_attempt
            .as_ref()
            .filter(|attempt| attempt.token == token)
            .cloned()
        else {
            self.palette = Some(palette);
            return Vec::new();
        };
        if !attempt.inspect_complete || !attempt.list_complete {
            self.palette = Some(palette);
            return Vec::new();
        }
        if let Some(error) = &attempt.reconciliation_error {
            palette.pending = None;
            if let Some(attempt) = palette.branch_attempt.as_mut() {
                attempt.blocked = true;
            }
            palette.error = Some(format!(
                "Could not confirm whether the branch was created ({error}). Use Refresh to reconcile again, or Esc to leave this form."
            ));
            self.palette = Some(palette);
            return Vec::new();
        }

        let record = attempt.registry.as_ref().and_then(|registry| {
            registry
                .projects
                .iter()
                .find(|project| project.name == attempt.project)
                .and_then(|project| {
                    project
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.id == attempt.workspace_id)
                })
        });
        let hierarchy = attempt.hierarchy.as_ref().and_then(|snapshot| {
            snapshot
                .projects
                .iter()
                .find(|project| project.name == attempt.project)
                .and_then(|project| {
                    project
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.id == attempt.workspace_id)
                })
        });
        let exact_identity = record.zip(hierarchy).is_some_and(|(record, workspace)| {
            !record.setup_pending
                && record.branch == attempt.branch
                && record.path == workspace.path
                && workspace.project == attempt.project
                && workspace.id == attempt.workspace_id
                && workspace.name == attempt.branch
        });
        let any_identity = record.is_some() || hierarchy.is_some();
        let succeeded = matches!(
            attempt.outcome,
            Some(crate::protocol::LifecycleOutcome::Succeeded)
        );
        let partial_failure = matches!(
            attempt.outcome,
            Some(crate::protocol::LifecycleOutcome::Failed {
                code: crate::protocol::ErrorCode::PartialFailure,
                ..
            })
        );
        if exact_identity && succeeded {
            if attempt.agent_form {
                palette.pending = None;
                palette.branch_attempt = None;
                palette.error = None;
                palette.invalid = None;
                palette.workspace_id = Some(attempt.workspace_id.clone());
                let target =
                    super::picker::workspace_pick_value(&attempt.project, &attempt.workspace_id);
                if let Page::Form { fields, .. } = &mut palette.page
                    && let Some(field) = fields.iter_mut().find(|field| field.label == "Branch")
                {
                    field.value = target.clone();
                    field.edited = true;
                    if let FieldKind::Pick(list) = &mut field.kind {
                        list.query.clear();
                        list.select_value(&target);
                    }
                }
                palette.branch_project = Some(attempt.project.clone());
                palette.parent_branch_target = Some((target, true));
                self.refresh_workspace_form(&mut palette);
                self.refresh_model_query(&mut palette);
                self.palette = Some(palette);
                return Vec::new();
            }
            self.finish_created_branch(&mut palette, &attempt);
            return Vec::new();
        }

        palette.pending = None;
        if partial_failure && !any_identity {
            palette.branch_attempt = None;
            palette.error = Some(
                "The partial attempt left no matching workspace in the fresh readback. Review the branch and press Create branch to retry.".into(),
            );
        } else if attempt.outcome.is_none() && !any_identity {
            if let Some(attempt) = palette.branch_attempt.as_mut() {
                attempt.blocked = true;
            }
            palette.error = Some(
                "No matching workspace appeared, but the create has no terminal receipt. Use Refresh to reconcile again; retry stays blocked until the original job completes.".into(),
            );
        } else {
            if let Some(attempt) = palette.branch_attempt.as_mut() {
                attempt.blocked = true;
            }
            palette.error = if exact_identity {
                Some(format!(
                    "{} exists, but the lifecycle result was not successful; retry is blocked.",
                    attempt.branch
                ))
            } else {
                Some(
                    "The server result and workspace readback do not agree. Retry is blocked until this identity is reconciled.".into(),
                )
            };
        }
        self.palette = Some(palette);
        Vec::new()
    }

    fn finish_created_branch(&mut self, palette: &mut Palette, attempt: &BranchAttempt) {
        palette.pending = None;
        palette.branch_attempt = None;
        palette.error = None;
        palette.invalid = None;
        self.palette = None;
        self.dismiss_error_banner();
        self.select_container(super::TreeRow::Workspace {
            project: attempt.project.clone(),
            id: attempt.workspace_id.clone(),
        });
        self.mode = InputMode::Browse;
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

    fn palette_submit(
        &mut self,
        palette: &mut Palette,
        request: Request,
        history_before_request: bool,
    ) -> DashboardAction {
        if self.lifecycle_busy()
            && super::lifecycle_ui::Provisional::from_request(0, &request).is_some()
        {
            let message = super::lifecycle_ui::BUSY.to_string();
            self.refuse_lifecycle_job(message.clone());
            palette.error = Some(message);
            return DashboardAction::Redraw;
        }
        let initial_prompt = match &request {
            Request::CreateAgentSession(launch) => {
                let prompt = match &palette.page {
                    Page::Form { fields, .. } => fields
                        .iter()
                        .find(|field| field.label == "Initial prompt")
                        .map(|field| field.value.clone())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                Some((launch.clone(), prompt))
            }
            _ => None,
        };
        if let Some((launch, prompt)) = &initial_prompt
            && !prompt.trim().is_empty()
            && !self.supports_initial_prompt_launch(launch)
        {
            palette.error = Some(
                "Initial prompts require a current detected Claude Code, Pi, Codex, or Oh My Pi launcher. Clear the prompt or choose one of those detected launchers.".into(),
            );
            return DashboardAction::Redraw;
        }
        let launch = match &request {
            Request::CreateWorkspaceWithLaunch {
                project,
                id: name,
                launch: Some(launch),
                ..
            } => Some(ovrcr_protocol::CreateAgentSessionRequest {
                session: CreateSessionRequest {
                    project: project.clone(),
                    workspace: name.clone(),
                    name: String::new(),
                    argv: launch.argv.clone(),
                    label: launch.label.clone(),
                    kind: launch.kind.clone(),
                },
                model_policy: launch.model_policy.clone(),
                model_binding: launch.model_binding.clone(),
                expected_model: launch.expected_model.clone(),
            }),
            Request::CreateAgentSession(launch) => Some(launch.clone()),
            Request::CreateSession(session) => Some(ovrcr_protocol::CreateAgentSessionRequest {
                session: session.clone(),
                model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
                model_binding: None,
                expected_model: None,
            }),
            _ => None,
        };
        if let Some(launch) = launch {
            let task = match &palette.page {
                Page::Form {
                    command: Command::RecoverLaunch,
                    ..
                } => String::new(),
                Page::Form { fields, .. } => form_task(fields),
                _ => String::new(),
            };
            remember_launch(palette, launch, &task);
        }
        if let Some((launch, prompt)) = initial_prompt {
            let request_id = self.next_request_id();
            self.deferred_initial_prompt_recovery = None;
            palette.pending = Some(request_id);
            palette.error = None;
            self.pending_initial_prompt_allocation = Some(super::InitialPromptAllocation {
                request_id,
                launch,
                prompt,
                form: None,
                then: Vec::new(),
            });
            if history_before_request {
                self.queue_history_request();
            }
            self.push_request(ClientMessage {
                request_id,
                request: Request::AllocateInitialPromptIntent,
            });
            return DashboardAction::Redraw;
        }
        let request_id = self.next_request_id();
        palette.pending = Some(request_id);
        palette.error = None;
        self.begin_lifecycle_pending(request_id, &request);
        if history_before_request {
            self.queue_history_request();
        }
        self.push_request(ClientMessage {
            request_id,
            request,
        });
        DashboardAction::Redraw
    }

    fn supports_initial_prompt_launch(
        &self,
        launch: &ovrcr_protocol::CreateAgentSessionRequest,
    ) -> bool {
        let Some(preset) = launch
            .session
            .label
            .as_deref()
            .filter(|preset| matches!(*preset, "claude" | "pi" | "codex" | "omp"))
        else {
            return false;
        };
        if !matches!(
            &launch.session.kind,
            SessionKind::Agent { name } if name == preset
        ) {
            return false;
        }
        self.detected_agents().into_iter().any(|entry| {
            entry.name == preset
                && entry.source == AgentSource::Detected
                && entry.origin == AgentOrigin::Detected
                && entry.argv == launch.session.argv
        })
    }

    pub(super) fn queue_chained_initial_prompt_launch(
        &mut self,
        launch: ovrcr_protocol::CreateAgentSessionRequest,
        form: Option<Box<Palette>>,
        then: Vec<Request>,
    ) {
        let prompt = form
            .as_ref()
            .and_then(|form| match &form.page {
                Page::Form { fields, .. } => fields
                    .iter()
                    .find(|field| field.label == "Initial prompt")
                    .map(|field| field.value.clone()),
                _ => None,
            })
            .unwrap_or_default();
        if !prompt.trim().is_empty() && !self.supports_initial_prompt_launch(&launch) {
            if let Some(mut form) = form {
                form.pending = None;
                form.error = Some(
                    "Initial prompts require a current detected Claude Code, Pi, Codex, or Oh My Pi launcher. Clear the prompt or choose one of those detected launchers.".into(),
                );
                self.palette = Some(*form);
            } else {
                self.set_error(
                    "Initial prompts require a current detected Claude Code, Pi, Codex, or Oh My Pi launcher. Clear the prompt or choose one of those detected launchers.",
                );
            }
            return;
        }
        let request_id = self.next_request_id();
        self.deferred_initial_prompt_recovery = None;
        self.pending_initial_prompt_allocation = Some(super::InitialPromptAllocation {
            request_id,
            launch,
            prompt,
            form,
            then,
        });
        self.lifecycle_strip = Some("Starting agent…".into());
        self.push_request(ClientMessage {
            request_id,
            request: Request::AllocateInitialPromptIntent,
        });
    }

    pub(super) fn initial_prompt_allocation_response(
        &mut self,
        request_id: u64,
        response: &Response,
    ) -> bool {
        if !self
            .pending_initial_prompt_allocation
            .as_ref()
            .is_some_and(|pending| pending.request_id == request_id)
        {
            return false;
        }
        let mut allocation = self.pending_initial_prompt_allocation.take().unwrap();
        if let Response::Error { message, .. } = response {
            if let Some(mut form) = allocation.form {
                form.pending = None;
                form.error = Some(self.plain_failure(&form, message));
                self.palette = Some(*form);
                self.lifecycle_strip = None;
                return true;
            }
            return false;
        }
        let Response::InitialPromptIntent(intent) = response else {
            if let Some(mut form) = allocation.form {
                form.pending = None;
                form.error = Some("Could not prepare this agent start".into());
                self.palette = Some(*form);
                self.lifecycle_strip = None;
            } else if let Some(palette) = self.palette.as_mut()
                && palette.pending == Some(request_id)
            {
                palette.pending = None;
                palette.error = Some("Could not prepare this agent start".into());
            } else {
                self.set_error("Could not prepare this agent start");
            }
            return true;
        };
        if let Err(error) = intent.validate() {
            if let Some(mut form) = allocation.form {
                form.pending = None;
                form.error = Some(error);
                self.palette = Some(*form);
                self.lifecycle_strip = None;
            } else if let Some(palette) = self.palette.as_mut()
                && palette.pending == Some(request_id)
            {
                palette.pending = None;
                palette.error = Some(error);
            }
            return true;
        }
        let direct_palette = self
            .palette
            .as_ref()
            .is_some_and(|palette| palette.pending == Some(request_id));
        if !direct_palette && allocation.form.is_none() {
            // Escape retires the allocation response before this path. A missing form
            // here is another close path, so leave the issued intent unused.
            return true;
        }
        if let Some(form) = allocation.form.as_mut() {
            form.initial_prompt_intent = Some(intent.clone());
        }
        if direct_palette && let Some(palette) = self.palette.as_mut() {
            palette.initial_prompt_intent = Some(intent.clone());
        }
        let launch =
            Request::CreateAgentSessionWithPrompt(ovrcr_protocol::InitialPromptLaunchRequest {
                intent: intent.clone(),
                launch: allocation.launch,
                prompt: allocation.prompt,
            });
        let launch_request_id = self.next_request_id();
        if direct_palette && let Some(palette) = self.palette.as_mut() {
            palette.pending = Some(launch_request_id);
            palette.error = None;
        }
        self.initial_prompt_intent_id = Some(intent.clone());
        self.initial_prompt_focus_pending = Some(intent.clone());
        self.initial_prompt_intent = None;
        self.begin_lifecycle_pending(launch_request_id, &launch);
        if let Some(row) = self.lifecycle_pending.as_mut() {
            row.form = allocation.form;
            row.then = allocation.then;
        }
        self.lifecycle_strip = Some("Starting agent…".into());
        self.push_request(ClientMessage {
            request_id: launch_request_id,
            request: launch,
        });
        true
    }

    pub(super) fn abandon_unbound_initial_prompt_allocation(&mut self) {
        let Some(allocation) = self.pending_initial_prompt_allocation.take() else {
            return;
        };
        if let Some(mut form) = allocation.form {
            form.pending = None;
            form.error = Some(
                "Connection changed before the agent start was sent. Press Start to try again."
                    .into(),
            );
            self.palette = Some(*form);
        } else if let Some(palette) = self
            .palette
            .as_mut()
            .filter(|palette| palette.pending == Some(allocation.request_id))
        {
            palette.pending = None;
            palette.error = Some(
                "Connection changed before the agent start was sent. Press Start to try again."
                    .into(),
            );
        }
    }

    pub(super) fn initial_prompt_snapshot_updated(
        &mut self,
        snapshot: &ovrcr_protocol::InitialPromptIntentSnapshot,
    ) {
        use ovrcr_protocol::InitialPromptIntentState as State;

        match snapshot.state {
            State::Issued => {
                self.abandon_unbound_initial_prompt_allocation();
                self.lifecycle_strip = None;
            }
            State::Pending => {
                self.lifecycle_strip = Some("Starting agent…".into());
            }
            State::LaunchAccepted => {
                if self.attach_initial_prompt_session() {
                    self.lifecycle_strip = None;
                } else {
                    self.lifecycle_strip = Some("Agent started".into());
                }
            }
            State::Refused => {
                let awaiting_accept = self
                    .lifecycle_pending
                    .as_ref()
                    .is_some_and(|row| row.initial_prompt_intent.as_ref() == Some(&snapshot.id));
                if awaiting_accept {
                    self.lifecycle_strip = Some("Starting agent…".into());
                } else {
                    self.recover_initial_prompt_refusal(snapshot);
                }
            }
            State::Unknown => self.finish_unknown_initial_prompt(snapshot),
        }
    }

    fn recover_initial_prompt_refusal(
        &mut self,
        snapshot: &ovrcr_protocol::InitialPromptIntentSnapshot,
    ) {
        let matching_palette = self
            .palette
            .as_ref()
            .is_some_and(|palette| palette.initial_prompt_intent.as_ref() == Some(&snapshot.id));
        if self.palette.is_some() && !matching_palette {
            self.deferred_initial_prompt_recovery = Some(snapshot.id.clone());
            let reason = snapshot
                .message
                .as_deref()
                .unwrap_or("The agent could not be started before a process began");
            self.lifecycle_strip = Some(format!(
                "{reason}. Close this form to recover the agent start."
            ));
            return;
        }
        let mut form = self
            .provisional
            .take_if(|row| row.initial_prompt_intent.as_ref() == Some(&snapshot.id))
            .and_then(|mut row| row.form.take())
            .map(|form| *form)
            .or_else(|| matching_palette.then(|| self.palette.take()).flatten())
            .unwrap_or_else(Palette::new);
        self.deferred_initial_prompt_recovery
            .take_if(|intent| intent == &snapshot.id);
        self.lifecycle_pending
            .take_if(|row| row.initial_prompt_intent.as_ref() == Some(&snapshot.id));
        self.lifecycle_preference = None;
        self.lifecycle_strip = None;
        self.initial_prompt_focus_pending = None;

        form.pending = None;
        form.initial_prompt_intent = None;
        form.error = snapshot
            .message
            .clone()
            .or_else(|| Some("The agent could not be started before a process began".into()));
        let Some(launch) = snapshot.recoverable_launch.clone() else {
            if let Page::Form { fields, .. } = &mut form.page
                && let Some(prompt) = fields
                    .iter_mut()
                    .find(|field| field.label == "Initial prompt")
            {
                prompt.value.clear();
            }
            self.palette = Some(form);
            return;
        };
        let prompt = snapshot.recoverable_prompt.clone().unwrap_or_default();
        let model = match &launch.model_policy {
            ovrcr_protocol::AgentModelPolicy::HarnessDefault => "Harness default".to_owned(),
            ovrcr_protocol::AgentModelPolicy::Explicit(model) => model.clone(),
        };
        let name = if launch.session.name.is_empty() {
            String::new()
        } else {
            format!(" · {}", launch.session.name)
        };
        let summary = format!(
            "{} · {} / {} · {model}{name}",
            launch.session.label.as_deref().unwrap_or("Agent"),
            launch.session.project,
            launch.session.workspace
        );
        form.page = Page::Form {
            command: Command::RecoverInitialPrompt,
            fields: vec![Field {
                label: "Initial prompt",
                value: prompt,
                required: false,
                hidden: false,
                edited: false,
                kind: FieldKind::Multiline,
            }],
            active: 0,
            name_edited: false,
            root_edited: false,
        };
        form.recovered_prompt_launch = Some(launch);
        form.initial_prompt_summary = Some(summary);
        form.cursor = Default::default();
        self.palette = Some(form);
    }

    fn finish_unknown_initial_prompt(
        &mut self,
        snapshot: &ovrcr_protocol::InitialPromptIntentSnapshot,
    ) {
        let message = snapshot.message.clone().unwrap_or_else(|| {
            "Launch status is unknown. Check for the agent before trying again.".into()
        });
        let matching_palette = self
            .palette
            .as_ref()
            .is_some_and(|palette| palette.initial_prompt_intent.as_ref() == Some(&snapshot.id));
        if self.palette.is_some() && !matching_palette {
            self.initial_prompt_focus_pending = None;
            self.lifecycle_strip = Some(message);
            return;
        }
        let mut form = self
            .provisional
            .take_if(|row| row.initial_prompt_intent.as_ref() == Some(&snapshot.id))
            .and_then(|mut row| row.form.take())
            .map(|form| *form);
        if form.is_none() && matching_palette {
            form = self.palette.take();
        }
        self.lifecycle_pending
            .take_if(|row| row.initial_prompt_intent.as_ref() == Some(&snapshot.id));
        self.lifecycle_preference = None;
        self.initial_prompt_focus_pending = None;
        if let Some(mut form) = form {
            form.pending = None;
            form.initial_prompt_intent = None;
            form.error = Some(message.clone());
            if let Page::Form { fields, .. } = &mut form.page
                && let Some(prompt) = fields
                    .iter_mut()
                    .find(|field| field.label == "Initial prompt")
            {
                prompt.value.clear();
            }
            form.recovered_prompt_launch = None;
            self.palette = Some(form);
            self.lifecycle_strip = None;
        } else {
            self.lifecycle_strip = Some(message);
        }
    }

    fn restore_deferred_initial_prompt_recovery(&mut self) {
        if self.palette.is_some() {
            return;
        }
        let Some(intent) = self.deferred_initial_prompt_recovery.take() else {
            return;
        };
        let Some(snapshot) = self
            .initial_prompt_intent
            .clone()
            .filter(|snapshot| snapshot.id == intent)
        else {
            return;
        };
        if snapshot.state == ovrcr_protocol::InitialPromptIntentState::Refused {
            self.recover_initial_prompt_refusal(&snapshot);
        }
    }

    /// Starts `session` once the workspace job just submitted succeeds. Until
    /// then an empty workspace is not finished, and a failure of the session
    /// offers the same recovery as a launch the workspace request carried.
    fn chain_session(
        &mut self,
        palette: &mut Palette,
        launch: ovrcr_protocol::CreateAgentSessionRequest,
    ) {
        let Some(row) = self
            .lifecycle_pending
            .as_mut()
            .filter(|row| Some(row.token) == palette.pending)
        else {
            return;
        };
        if let super::lifecycle_ui::ProvisionalKind::CreatingWorkspace { launch, .. } =
            &mut row.kind
        {
            *launch = true;
        }
        row.then = vec![Request::CreateAgentSession(launch.clone())];
        let task = match &palette.page {
            Page::Form { fields, .. } => form_task(fields),
            _ => String::new(),
        };
        remember_launch(palette, launch, &task);
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
        if matches!(command, Command::CreateBranch) {
            return self.create_branch_form(&project);
        }
        let fields = match command {
            Command::CreateTerminal => {
                let workspace_list =
                    PickList::workspaces(&self.hierarchy, &project, &workspace, true);
                let workspace_value = workspace_list
                    .accepted()
                    .map(|item| item.value.clone())
                    .unwrap_or_default();
                let launch_project = split_workspace_pick(&workspace_value).0;
                let launch_project = if launch_project.is_empty() {
                    project.clone()
                } else {
                    launch_project
                };
                let mut launch = self.launch_fields(&launch_project, false);
                let command = launch.pop().unwrap();
                let model = launch.pop().unwrap();
                let agent = launch.pop().unwrap();
                let start = launch.pop().unwrap();
                vec![
                    Field {
                        label: "Initial prompt",
                        value: String::new(),
                        required: false,
                        hidden: false,
                        edited: false,
                        kind: FieldKind::Multiline,
                    },
                    start,
                    Field {
                        label: "Workspace",
                        value: workspace_value,
                        required: true,
                        hidden: false,
                        edited: false,
                        kind: FieldKind::Pick(workspace_list),
                    },
                    text("Name", String::new(), false),
                    command,
                    agent,
                    model,
                ]
            }
            Command::RenameTerminal(id) => vec![text(
                "Name",
                find_session(self, id)
                    .map(|s| s.display_name().to_string())
                    .unwrap_or_default(),
                false,
            )],
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
                let project_name = project.value.clone();
                let mut fields = vec![
                    text("Task", String::new(), false),
                    Field {
                        label: "Initial prompt",
                        value: String::new(),
                        required: false,
                        hidden: true,
                        edited: false,
                        kind: FieldKind::Multiline,
                    },
                    project,
                    text("Branch", self.settings.branch_prefix.clone(), true),
                    mode,
                    text("Base", "main".into(), true),
                ];
                fields.extend(self.launch_fields(&project_name, true));
                fields
            }
            Command::CreateBranch => unreachable!("handled above"),
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
        let mut fields = fields;
        self.refresh_terminal_form(&mut fields, false);
        // New agent sessions start at the optional prompt; all other forms use
        // their first visible field.
        let active = fields
            .iter()
            .position(|field| field.label == "Initial prompt" && !field.hidden)
            .or_else(|| {
                fields
                    .iter()
                    .position(|field| field.label == "Task" && !field.hidden)
            })
            .or_else(|| fields.iter().position(|field| !field.hidden))
            .unwrap_or(0);
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
        if self
            .navigation
            .detached_palette_requests
            .remove(&request_id)
        {
            if let Response::Error { message, .. } = response {
                self.set_error(message);
            }
            return Some(Vec::new());
        }
        if let Some(requests) = self.branch_attempt_response(request_id, response) {
            return Some(requests);
        }
        // Lifecycle jobs accept with Ok immediately; Provisional row takes over.
        // A removal's later steps are sent while its confirmation is out of view.
        if self
            .lifecycle_pending
            .as_ref()
            .is_some_and(|row| row.token == request_id)
        {
            let submitted = self
                .palette
                .as_ref()
                .is_some_and(|palette| palette.pending == Some(request_id));
            let busy = matches!(
                response,
                Response::Error { code, message }
                    if *code == crate::protocol::ErrorCode::Conflict
                        && message.contains("lifecycle job")
            );
            match response {
                Response::Ok => {
                    self.accept_lifecycle_job(request_id);
                    self.apply_initial_prompt_snapshot();
                    return Some(Vec::new());
                }
                // A later step's confirmation comes back with the refusal.
                Response::Error { code, message } if !submitted => {
                    let row = self.lifecycle_pending.take().unwrap();
                    let message = if busy {
                        super::lifecycle_ui::BUSY.to_string()
                    } else {
                        message.clone()
                    };
                    self.fail_lifecycle_job(row, code.clone(), message);
                    return Some(Vec::new());
                }
                Response::Error { .. } if busy => {
                    let message = super::lifecycle_ui::BUSY.to_string();
                    self.refuse_lifecycle_job(message.clone());
                    if let Some(palette) = self.palette.as_mut() {
                        palette.pending = None;
                        palette.error = Some(message);
                    }
                    return Some(Vec::new());
                }
                Response::Error { .. } => {
                    self.lifecycle_pending = None;
                }
                _ => {}
            }
        }
        if self.palette.as_ref()?.suggestions.inspect == Some(request_id) {
            let mut palette = self.palette.take().unwrap();
            palette.suggestions.inspect = None;
            if let Response::Inventory { registry, .. } = response {
                palette.suggestions.repos = registry
                    .projects
                    .iter()
                    .map(|project| (project.name.clone(), project.repo.clone()))
                    .collect();
                palette.workspace_inventory = Some(registry.clone());
            } else if let Response::Error { message, .. } = response
                && matches!(
                    &palette.page,
                    Page::Form {
                        command: Command::NewAgentSession,
                        ..
                    }
                )
            {
                palette.error = Some(self.plain_error(message));
                if let Page::Form { fields, .. } = &mut palette.page
                    && let Some(destination) =
                        fields.iter_mut().find(|field| field.label == "Destination")
                {
                    destination.value = "Could not load registered workspaces.".into();
                }
            }
            self.refresh_workspace_form(&mut palette);
            if matches!(
                &palette.page,
                Page::Form {
                    command: Command::NewAgentSession,
                    ..
                }
            ) {
                self.refresh_model_query(&mut palette);
            }
            self.palette = Some(palette);
            return Some(Vec::new());
        }
        if self.palette.as_ref()?.pending != Some(request_id) {
            return None;
        }
        let workspace_form = matches!(
            self.palette.as_ref()?.page,
            Page::Form {
                command: Command::CreateWorkspace,
                ..
            }
        );
        match response {
            Response::WorkflowCandidates(choices) => {
                let palette = self.palette.as_mut().unwrap();
                palette.pending = None;
                palette.workflows = Some(choices.candidates.clone());
                palette.page = Page::Search {
                    query: String::new(),
                    selected: 0,
                };
                palette.error = if choices.gaps.is_empty() {
                    choices.candidates.is_empty().then(|| "No workflow needs transfer to this Pi. Reconcile its existing workflows in Pi.".into())
                } else {
                    Some(format!(
                        "{} retained record(s) have missing or unverifiable recovery authority; their artifacts/receipts remain read-only and cannot be transferred. Available workflows are listed below.",
                        choices.gaps.len()
                    ))
                };
                return Some(Vec::new());
            }
            Response::Inventory { sessions, .. } if self.palette.as_ref()?.archive.is_some() => {
                let palette = self.palette.as_mut().unwrap();
                palette.pending = None;
                let mut rows: Vec<_> = sessions
                    .iter()
                    .filter(|row| row.archived && !is_operator_scope(row))
                    .cloned()
                    .collect();
                rows.sort_by_key(|row| std::cmp::Reverse(row.id.0));
                palette.archive = Some(rows);
                return Some(Vec::new());
            }
            Response::Error { code, message } => {
                let code = code.clone();
                let message = message.clone();
                let mut palette = self.palette.take().unwrap();
                palette.pending = None;
                palette.error = Some(self.plain_failure(&palette, &message));
                // A refused removal asks again with what now stands in its way;
                // only another explicit Confirm sends anything.
                if let Some(Request::RemoveWorkspace {
                    project,
                    name,
                    force,
                }) = confirmation_request(&palette.page)
                {
                    let (project, name, force) = (project.clone(), name.clone(), *force);
                    let branch = find_workspace(self, &project, &name)
                        .map(|workspace| workspace.name.clone())
                        .unwrap_or_default();
                    palette.page = if code == crate::protocol::ErrorCode::DirtyWorktree
                        && branch_name_problem(&branch).is_none()
                    {
                        Page::Confirm {
                            request: Some(Box::new(Request::SaveWorkspaceWip { project, name })),
                            target: format!("Save uncommitted work to origin/wip/{branch}?"),
                        }
                    } else {
                        // Uncommitted changes alone earn the discard question.
                        // Running sessions are named and stopped first instead:
                        // never a forced removal they would still block.
                        let running = !self.running_sessions(&project, &name).is_empty();
                        if code == crate::protocol::ErrorCode::SessionsRemain && running {
                            palette.error = None;
                        }
                        self.removal_page(Request::RemoveWorkspace {
                            project,
                            name,
                            force: force || code == crate::protocol::ErrorCode::DirtyWorktree,
                        })
                    };
                }
                let retry = match confirmation_request(&palette.page) {
                    Some(Request::ReopenSession { session, .. }) => {
                        Some((*session, Command::ReopenTerminal(*session)))
                    }
                    Some(Request::AcknowledgeSessionStopped {
                        session,
                        expected_run,
                    }) => Some((
                        *session,
                        Command::AcknowledgeStopped(*session, *expected_run),
                    )),
                    _ => None,
                };
                if let Some((id, retry)) = retry {
                    if find_session(self, id).is_some() {
                        palette.page = self.command_page(retry);
                        palette.error = Some(self.plain_failure(&palette, &message));
                    }
                } else if code == crate::protocol::ErrorCode::OwnershipUncertain {
                    palette.failed_launch = None;
                    palette.page = Page::Confirm {
                        request: None,
                        target: "Launch ownership is uncertain. No process-stop acknowledgement was sent. Close this notice, select the intended retained row, then use its Reopen or Acknowledge action after stopping old processes. Do not create another session.".into(),
                    };
                } else if code == crate::protocol::ErrorCode::PartialFailure
                    && matches!(
                        palette.page,
                        Page::Form {
                            command: Command::CreateWorkspace,
                            ..
                        }
                    )
                    && palette.failed_launch.is_some()
                {
                    let fields = vec![pick_field(
                        "Recovery",
                        "Retry".into(),
                        ["Retry", "Choose another agent", "Open shell"]
                            .into_iter()
                            .map(|s| PickItem {
                                label: s.into(),
                                value: s.into(),
                            })
                            .collect(),
                    )];
                    palette.page = Page::Form {
                        command: Command::RecoverLaunch,
                        fields,
                        active: 0,
                        name_edited: false,
                        root_edited: false,
                    };
                }
                self.palette = Some(palette);
                return Some(Vec::new());
            }
            Response::Ok if workspace_form => {
                self.palette.as_mut().unwrap().workspace_acknowledged = true;
                return Some(self.attach_created_workspace());
            }
            _ => {
                let mut palette = self.palette.take().unwrap();
                if let Response::Ok = response
                    && let Some(Request::SaveWorkspaceWip { project, name }) =
                        confirmation_request(&palette.page)
                {
                    let project = project.clone();
                    let name = name.clone();
                    let display = find_workspace(self, &project, &name)
                        .map(|workspace| workspace_heading(self, workspace))
                        .unwrap_or_default();
                    let request = Request::RemoveWorkspace {
                        project,
                        name,
                        force: true,
                    };
                    palette.page = Page::Confirm {
                        request: Some(Box::new(request.clone())),
                        target: format!("Saved to origin/wip and removing workspace {display}."),
                    };
                    // A Lifecycle job like any removal, so its failure comes back here.
                    self.palette_submit(&mut palette, request, false);
                    self.palette = Some(palette);
                    return Some(Vec::new());
                }
                let answered_wip = matches!(
                    (confirmation_request(&palette.page), response),
                    (Some(Request::AnswerWipSave { .. }), Response::Ok)
                );
                if let Some(request_id) = palette.suggestions.inspect {
                    self.ignored_responses.insert(request_id);
                }
                if let Response::CreatedSession(session) = response
                    && self.stale_created_session(session)
                {
                    return Some(Vec::new());
                }
                let preference = palette.launch_preference.take();
                self.dismiss_error_banner();
                if let Response::CreatedSession(session) = response {
                    self.select_session(session.id);
                    // The new pane's view goes first: the save waits on a
                    // synced write, and input waits on the view.
                    let request_id = self.next_request_id();
                    let mut outgoing: Vec<ClientMessage> = self
                        .view_request(self.outer_area, request_id)
                        .ok()
                        .flatten()
                        .into_iter()
                        .collect();
                    if let Some((project, choice)) = preference {
                        // The Server saves it; remembered here at once so the
                        // next launch offers it before the reading arrives.
                        let request = super::settings::launch_choice_request(&project, &choice);
                        self.settings.launch_choices.insert(project, choice);
                        outgoing.push(ClientMessage {
                            request_id: self.error_owning_request_id(),
                            request,
                        });
                    }
                    self.mode = InputMode::Terminal;
                    return Some(outgoing);
                }
                self.palette = None;
                if answered_wip {
                    self.wip_prompts.pop_front();
                }
                self.present_wip_prompt();
            }
        }
        Some(Vec::new())
    }

    fn branch_attempt_response(
        &mut self,
        request_id: u64,
        response: &Response,
    ) -> Option<Vec<ClientMessage>> {
        let attempt = self.palette.as_ref()?.branch_attempt.as_ref()?.clone();
        if request_id == attempt.token
            && self
                .palette
                .as_ref()
                .is_some_and(|palette| palette.pending == Some(request_id))
        {
            match response {
                Response::Ok => {
                    if let Some(current) = self
                        .palette
                        .as_mut()
                        .and_then(|palette| palette.branch_attempt.as_mut())
                    {
                        current.accepted = true;
                    }
                    if self
                        .palette
                        .as_ref()
                        .and_then(|palette| palette.branch_attempt.as_ref())
                        .is_some_and(|current| current.outcome.is_some())
                    {
                        if self
                            .lifecycle_pending
                            .as_ref()
                            .is_some_and(|row| row.token == request_id)
                        {
                            self.lifecycle_pending = None;
                        }
                        self.start_branch_reconciliation(request_id);
                    }
                }
                Response::Error { code, message } => {
                    if self
                        .lifecycle_pending
                        .as_ref()
                        .is_some_and(|row| row.token == request_id)
                    {
                        self.lifecycle_pending = None;
                    }
                    if *code == crate::protocol::ErrorCode::PartialFailure {
                        if let Some(current) = self
                            .palette
                            .as_mut()
                            .and_then(|palette| palette.branch_attempt.as_mut())
                        {
                            current.accepted = true;
                            current.blocked = true;
                            current.outcome = Some(crate::protocol::LifecycleOutcome::Failed {
                                code: code.clone(),
                                message: message.clone(),
                            });
                        }
                        self.start_branch_reconciliation(request_id);
                    } else if let Some(mut palette) = self.palette.take() {
                        palette.pending = None;
                        palette.branch_attempt = None;
                        palette.error = Some(self.plain_failure(&palette, message));
                        self.palette = Some(palette);
                    }
                }
                _ => {
                    if let Some(palette) = self.palette.as_mut() {
                        if let Some(current) = palette.branch_attempt.as_mut() {
                            current.accepted = true;
                            current.blocked = true;
                        }
                        palette.error = Some(
                            "The create response was unclear. Checking the workspace identity before retrying.".into(),
                        );
                    }
                    self.lifecycle_pending = None;
                    self.start_branch_reconciliation(request_id);
                }
            }
            return Some(Vec::new());
        }

        if attempt.inspect_request == Some(request_id) {
            let reconciliation_error = match response {
                Response::Error { message, .. } => Some(self.plain_error(message)),
                Response::Inventory { .. } => None,
                _ => Some("unexpected Inspect response".into()),
            };
            if let Some(palette) = self.palette.as_mut()
                && let Some(current) = palette.branch_attempt.as_mut()
            {
                current.inspect_complete = true;
                match response {
                    Response::Inventory { registry, .. } => {
                        current.registry = Some(registry.clone());
                        palette.workspace_inventory = Some(registry.clone());
                        palette.suggestions.repos = registry
                            .projects
                            .iter()
                            .map(|project| (project.name.clone(), project.repo.clone()))
                            .collect();
                    }
                    _ => current.reconciliation_error = reconciliation_error,
                }
            }
            self.finish_branch_reconciliation(attempt.token);
            return Some(Vec::new());
        }

        if attempt.list_request == Some(request_id) {
            let mut outgoing = Vec::new();
            if let Response::Hierarchy(snapshot) = response {
                outgoing.extend(self.update_hierarchy(snapshot.clone()));
            }
            let reconciliation_error = match response {
                Response::Error { message, .. } => Some(self.plain_error(message)),
                Response::Hierarchy(_) => None,
                _ => Some("unexpected List response".into()),
            };
            if let Some(palette) = self.palette.as_mut()
                && let Some(current) = palette.branch_attempt.as_mut()
            {
                current.list_complete = true;
                match response {
                    Response::Hierarchy(snapshot) => current.hierarchy = Some(snapshot.clone()),
                    _ => current.reconciliation_error = reconciliation_error,
                }
            }
            outgoing.extend(self.finish_branch_reconciliation(attempt.token));
            return Some(outgoing);
        }
        None
    }

    pub(super) fn apply_initial_prompt_snapshot(&mut self) {
        let Some(snapshot) = self.initial_prompt_intent.clone() else {
            return;
        };
        self.install_initial_prompt_snapshot(snapshot);
    }

    pub(super) fn attach_created_workspace(&mut self) -> Vec<ClientMessage> {
        let Some(palette) = &self.palette else {
            return Vec::new();
        };
        if !palette.workspace_acknowledged {
            return Vec::new();
        }
        if !matches!(
            palette.page,
            Page::Form {
                command: Command::CreateWorkspace,
                ..
            }
        ) {
            return Vec::new();
        }
        let Some(id) = palette.workspace_id.clone() else {
            return Vec::new();
        };
        let project = match &palette.page {
            Page::Form { fields, .. } => fields
                .iter()
                .find(|field| field.label == "Project")
                .map(|field| field.value.trim().to_string())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let Some(workspace) = find_workspace(self, &project, &id).cloned() else {
            return Vec::new();
        };
        if workspace.sessions.is_empty() {
            let row = super::TreeRow::Workspace {
                project: workspace.project.clone(),
                id: workspace.id.clone(),
            };
            self.palette = None;
            self.dismiss_error_banner();
            self.select_container(row);
            let request_id = self.next_request_id();
            return self
                .view_request(self.outer_area, request_id)
                .ok()
                .flatten()
                .into_iter()
                .collect();
        }
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
        self.dismiss_error_banner();
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
        if creation_form_below_minimum(&palette.page, self.outer_area) {
            return (
                vec![
                    Line::styled(
                        "Resize to at least 60 columns × 18 rows.",
                        Style::default().fg(PEACH()),
                    ),
                    Line::styled(
                        "Your draft will be kept while you resize.",
                        Style::default().fg(SUBTEXT()),
                    ),
                ],
                Vec::new(),
                0,
            );
        }
        if let Page::Form {
            command: command @ (Command::NewAgentSession | Command::CreateBranch),
            fields,
            active,
            ..
        } = &palette.page
        {
            return self.creation_form_body(body, command, fields, *active, palette);
        }
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
                let entries = self.palette_entries(
                    query,
                    palette.archive.as_deref(),
                    palette.agents.as_deref(),
                    palette.workflows.as_deref(),
                    palette.split,
                );
                if entries.is_empty() {
                    lines.push(Line::from(if palette.archive.is_some() {
                        "No matching archived sessions"
                    } else if palette.agents.is_some() {
                        if query.is_empty() {
                            "No other running agents"
                        } else {
                            "No matching agents"
                        }
                    } else if palette.split {
                        "No matching sessions"
                    } else {
                        "No matching actions or terminals"
                    }));
                }
                let selected = (*selected).min(entries.len().saturating_sub(1));
                let count = usize::from(body.height.saturating_sub(1) / 2).max(1);
                let start = selected.saturating_sub(count - 1);
                for (index, entry) in entries.iter().enumerate().skip(start).take(count) {
                    let available = usize::from(body.width.saturating_sub(2));
                    let label = if let Some(session) = self.entry_session(&entry.command, palette) {
                        let id = session.id;
                        let mut suffix = format!(" (#{})", id.0);
                        if palette.agents.is_some()
                            && available < 48
                            && let SessionKind::Agent { name } = &session.kind
                        {
                            let name_width = available
                                .saturating_sub(suffix.width() + 1)
                                .min(available / 2);
                            suffix.push_str(&format!(" {}", clip_text(name, name_width)));
                        }
                        let suffix_width = suffix.width();
                        if available >= suffix_width {
                            format!(
                                "{}{}",
                                clip_text(session.display_name(), available - suffix_width),
                                suffix
                            )
                        } else {
                            clip_text(&format!("#{}", id.0), available)
                        }
                    } else if let Command::Switch(id) = entry.command
                        && let Some(place) = entry.label.strip_suffix(&format!(" (#{})", id.0))
                    {
                        // Clip the title, not the ID that tells two equal rows apart.
                        let suffix = format!(" (#{})", id.0);
                        format!(
                            "{}{suffix}",
                            clip_text(place, available.saturating_sub(suffix.width()))
                        )
                    } else {
                        clip_text(&entry.label, available)
                    };
                    targets.push((lines.len(), PaletteTarget::Entry(index)));
                    lines.push(Line::styled(
                        format!("{} {}", if index == selected { "›" } else { " " }, label),
                        if index == selected {
                            Style::default().bg(MAUVE()).fg(CRUST())
                        } else {
                            Style::default().fg(TEXT())
                        },
                    ));
                    if let Command::SwitchAgent(id, run) = entry.command {
                        let reason = palette
                            .agents
                            .as_ref()
                            .and_then(|candidates| {
                                candidates
                                    .iter()
                                    .find(|c| c.session.id == id && c.session.run == run)
                            })
                            .and_then(|candidate| candidate.reason(self));
                        let detail = if let Some(reason) = reason {
                            reason.to_owned()
                        } else if let Some(session) = self.entry_session(&entry.command, palette) {
                            // ponytail: static picker spinner; pass draw time if animation is needed.
                            let status = super::status::SessionStatus::of(session, 0);
                            let agent = match &session.kind {
                                SessionKind::Agent { name } => name.as_str(),
                                SessionKind::Terminal => session.label.as_str(),
                            };
                            let mut activity = status
                                .unread
                                .unwrap_or_else(|| status.activity.trim().to_owned());
                            if let Some(health) = status
                                .reporting_health
                                .filter(|health| !activity.contains(health))
                            {
                                activity.push_str(&format!(" · {health}"));
                            }
                            if available < 48 {
                                status.compact
                            } else {
                                format!(
                                    "{} {} · {} · {}",
                                    status.glyph,
                                    activity,
                                    agent,
                                    session_workspace_heading(self, session)
                                )
                            }
                        } else {
                            String::new()
                        };
                        lines.push(Line::styled(
                            format!("  {detail}"),
                            Style::default().fg(SUBTEXT()),
                        ));
                    } else if let Command::Unarchive(id, _)
                    | Command::DeleteArchived(id, _, _)
                    | Command::AcknowledgeStopped(id, _) = &entry.command
                        && let Some(row) = palette
                            .archive
                            .as_ref()
                            .and_then(|rows| rows.iter().find(|row| row.id == *id))
                    {
                        lines.push(Line::styled(
                            format!("  {}", row.cwd.display()),
                            Style::default().fg(MUTED()),
                        ));
                    } else if let Some(hint) = self.command_hint(&entry.command) {
                        lines.push(Line::styled(
                            format!("  {} — {}", hint.key, hint.description),
                            Style::default().fg(MUTED()),
                        ));
                    } else if let Command::Setting(setting) = entry.command {
                        lines.push(Line::styled(
                            format!("  {} · {}", setting.group, setting.path),
                            Style::default().fg(MUTED()),
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
                if matches!(command, Command::RecoverInitialPrompt)
                    && let Some(summary) = &palette.initial_prompt_summary
                {
                    lines.push(Line::styled(
                        format!(
                            "Launch: {}",
                            clip_text(summary, body.width.saturating_sub(2) as usize)
                        ),
                        Style::default().fg(MUTED()),
                    ));
                }
                for (index, field) in fields.iter().enumerate() {
                    if field.hidden {
                        continue;
                    }
                    if matches!(&field.kind, FieldKind::Notice) {
                        let notice = match field.label {
                            "Destination" | "Shared files" => field.value.clone(),
                            label => format!("{label}: {}", field.value),
                        };
                        lines.push(Line::styled(
                            clip_text(&notice, body.width.saturating_sub(2) as usize),
                            Style::default().fg(MUTED()),
                        ));
                        continue;
                    }
                    let top = lines.len();
                    let pick_label;
                    let editing = match &field.kind {
                        FieldKind::Pick(list) if index == *active && !list.query.is_empty() => {
                            list.query.as_str()
                        }
                        FieldKind::Pick(list) => {
                            pick_label = if field.label == "Branch"
                                && let Some(branch) = created_branch_name(&field.value)
                            {
                                branch
                            } else {
                                list.items
                                    .iter()
                                    .find(|item| item.value == field.value)
                                    .map(|item| item.label.as_str())
                                    .unwrap_or(field.value.as_str())
                            };
                            pick_label
                        }
                        _ => field.value.as_str(),
                    };
                    let cursor = if index == *active {
                        palette.cursor
                    } else {
                        Default::default()
                    };
                    let compact_choice_picker =
                        palette.new_agent_picker_open && compact_picker_field(command, field);
                    let compact_choice_dialog = palette.new_agent_picker_open
                        && matches!(command, Command::NewAgentSession | Command::CreateBranch);
                    let compact_pick = compact_choice_picker;
                    let compact_toggle =
                        compact_choice_dialog && matches!(field.kind, FieldKind::Toggle);
                    if compact_pick {
                        let label = format!("{}: ", field.label);
                        let (mut text, _) = cursor.display(
                            editing,
                            body.width.saturating_sub(label.width() as u16 + 2) as usize,
                        );
                        if index != *active {
                            text.pop();
                        }
                        targets.push((lines.len(), PaletteTarget::Field(index, false)));
                        targets.push((lines.len(), PaletteTarget::Field(index, true)));
                        lines.push(Line::from(vec![
                            Span::styled(label, Style::default().fg(SUBTEXT())),
                            Span::styled(
                                format!("{} {text}", if index == *active { "›" } else { " " }),
                                Style::default().fg(if index == *active {
                                    MAUVE()
                                } else {
                                    TEXT()
                                }),
                            ),
                        ]));
                    } else if compact_toggle {
                        targets.push((lines.len(), PaletteTarget::Field(index, false)));
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{}: ", field.label),
                                Style::default().fg(SUBTEXT()),
                            ),
                            Span::styled(
                                field.value.as_str(),
                                Style::default().fg(if index == *active {
                                    MAUVE()
                                } else {
                                    TEXT()
                                }),
                            ),
                        ]));
                    } else {
                        targets.push((lines.len(), PaletteTarget::Field(index, false)));
                        lines.push(Line::styled(field.label, Style::default().fg(SUBTEXT())));
                    }
                    if matches!(&field.kind, FieldKind::Multiline) {
                        let focused_line = cursor.line(editing);
                        let width = body.width.saturating_sub(2) as usize;
                        for line in 0..=editing.bytes().filter(|byte| *byte == b'\n').count() {
                            let mut text = cursor
                                .display_line(editing, line, width)
                                .unwrap_or_default();
                            if index != *active || line != focused_line {
                                text.pop();
                            }
                            if index == *active && line == focused_line {
                                focus_line = lines.len();
                            }
                            targets.push((lines.len(), PaletteTarget::MultilineLine(index, line)));
                            lines.push(Line::styled(
                                format!(
                                    "{} {text}",
                                    if index == *active && line == focused_line {
                                        "›"
                                    } else {
                                        " "
                                    }
                                ),
                                Style::default().fg(if index == *active && line == focused_line {
                                    MAUVE()
                                } else {
                                    TEXT()
                                }),
                            ));
                        }
                        if let Some((_, problem)) =
                            palette.invalid.as_ref().filter(|(at, _)| *at == index)
                        {
                            lines.push(Line::styled(
                                format!("  {problem}"),
                                Style::default().fg(PEACH()),
                            ));
                            if index == *active {
                                focus_line = lines.len() - 1;
                            }
                        }
                        continue;
                    }
                    if !compact_pick && !compact_toggle {
                        let placeholder = field.label == "Branch name" && field.value.is_empty();
                        let (mut text, _) = if placeholder {
                            (
                                clip_text(
                                    "feature/auth-refresh",
                                    body.width.saturating_sub(2) as usize,
                                ),
                                0,
                            )
                        } else {
                            cursor.display(editing, body.width.saturating_sub(2) as usize)
                        };
                        if index != *active && !placeholder {
                            text.pop();
                        }
                        targets.push((lines.len(), PaletteTarget::Field(index, true)));
                        lines.push(Line::styled(
                            format!("{} {text}", if index == *active { "›" } else { " " }),
                            Style::default().fg(if placeholder {
                                MUTED()
                            } else if index == *active {
                                MAUVE()
                            } else {
                                TEXT()
                            }),
                        ));
                    }
                    if field.label == "Model"
                        && let Some(status) = palette
                            .model_status
                            .as_deref()
                            .or_else(|| palette.model_capabilities.as_ref()?.reason.as_deref())
                    {
                        lines.push(Line::styled(
                            format!("  {status}"),
                            Style::default().fg(MUTED()),
                        ));
                    }
                    if let Some((_, problem)) =
                        palette.invalid.as_ref().filter(|(at, _)| *at == index)
                    {
                        lines.push(Line::styled(
                            format!("  {problem}"),
                            Style::default().fg(PEACH()),
                        ));
                    }
                    if index != *active {
                        continue;
                    }
                    // The New agent session picker shares its bounded viewport
                    // with every remaining form control, model status/error,
                    // multiline prompt content, and the fixed action/footer rows.
                    let bounded_choice_picker =
                        compact_choice_picker && matches!(field.kind, FieldKind::Pick(_));
                    let room = if bounded_choice_picker {
                        let reserve_after = fields
                            .iter()
                            .enumerate()
                            .skip(index + 1)
                            .map(|(future_index, future)| {
                                field_rows_after(
                                    future,
                                    future_index,
                                    palette,
                                    command,
                                    compact_choice_dialog,
                                )
                            })
                            .sum::<usize>();
                        usize::from(body.height)
                            .saturating_sub(lines.len())
                            .saturating_sub(reserve_after)
                    } else {
                        usize::from(body.height)
                            .saturating_sub(lines.len() - top)
                            .max(1)
                    };
                    if let FieldKind::Pick(list) = &field.kind
                        && (!compact_picker_field(command, field) || palette.new_agent_picker_open)
                    {
                        if bounded_choice_picker {
                            let (options, option_rows) = list.lines_bounded(room);
                            for (offset, option) in option_rows {
                                targets.push((
                                    lines.len() + offset,
                                    PaletteTarget::Option(index, option),
                                ));
                            }
                            lines.extend(options);
                        } else {
                            let (options, _) = list.lines(room);
                            let window = list.window(room);
                            for (offset, option) in window.enumerate() {
                                targets.push((
                                    lines.len() + offset,
                                    PaletteTarget::Option(index, option),
                                ));
                            }
                            lines.extend(options);
                        }
                    }
                    if let FieldKind::Path(picker) = &field.kind
                        && let Some(listing) = picker.cached()
                    {
                        let count = room
                            .saturating_sub(usize::from(listing.omitted > 0))
                            .max(1)
                            .min(listing.entries.len());
                        let start = picker.selected.saturating_sub(count.saturating_sub(1));
                        for (offset, item) in
                            listing.entries.iter().enumerate().skip(start).take(count)
                        {
                            let chosen = offset == picker.selected;
                            targets.push((lines.len(), PaletteTarget::Option(index, offset)));
                            lines.push(Line::styled(
                                format!("  {} {}", if chosen { "›" } else { " " }, item.label),
                                if chosen {
                                    Style::default().bg(MAUVE()).fg(CRUST())
                                } else {
                                    Style::default().fg(TEXT())
                                },
                            ));
                        }
                        if listing.omitted > 0 {
                            lines.push(Line::styled(
                                format!("  … {} more", listing.omitted),
                                Style::default().fg(MUTED()),
                            ));
                        }
                    }
                    focus_line = lines.len() - 1;
                }
            }
            Page::Confirm { target, .. } => {
                lines.extend(target.lines().map(|line| Line::from(line.to_owned())));
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

    fn creation_form_body<'a>(
        &self,
        body: Rect,
        command: &Command,
        fields: &'a [Field],
        active: usize,
        palette: &Palette,
    ) -> (Vec<Line<'a>>, Vec<(usize, PaletteTarget)>, usize) {
        let compact_controls = body.width < 56;
        let pair = matches!(command, Command::NewAgentSession)
            .then(|| field_pair(fields, body.width))
            .flatten();
        let mut lines = Vec::new();
        let mut targets = Vec::new();
        let mut focus_line = 0;
        let mut index = 0;

        while index < fields.len() {
            let field = &fields[index];
            if field.hidden {
                index += 1;
                continue;
            }
            if matches!(field.kind, FieldKind::Notice) {
                let notice = match field.label {
                    "Destination" | "Shared files" => field.value.clone(),
                    label => format!("{label}: {}", field.value),
                };
                lines.push(Line::styled(
                    clip_text(&notice, usize::from(body.width)),
                    Style::default().fg(MUTED()),
                ));
                index += 1;
                continue;
            }

            if let Some((_, model)) = pair
                && index == model
            {
                index += 1;
                continue;
            }

            if let Some((harness, model)) = pair
                && index == harness
            {
                let (left_width, gap, right_width) = pair_columns(body);
                targets.push((lines.len(), PaletteTarget::FieldPair(harness, model, false)));
                lines.push(creation_pair_labels(
                    fields[harness].label,
                    fields[model].label,
                    left_width,
                    gap,
                    right_width,
                    active == harness,
                    active == model,
                ));
                targets.push((lines.len(), PaletteTarget::FieldPair(harness, model, true)));
                let left =
                    creation_field_display(&fields[harness], harness, active, palette, left_width);
                let right =
                    creation_field_display(&fields[model], model, active, palette, right_width);
                lines.push(creation_pair_surfaces(
                    PairedFieldSurface {
                        text: &left.0,
                        active: active == harness,
                        placeholder: left.1,
                        width: left_width,
                    },
                    gap,
                    PairedFieldSurface {
                        text: &right.0,
                        active: active == model,
                        placeholder: right.1,
                        width: right_width,
                    },
                ));
                if active == harness || active == model {
                    focus_line = lines.len().saturating_sub(1);
                }
                for paired in [harness, model] {
                    if let Some((_, problem)) =
                        palette.invalid.as_ref().filter(|(at, _)| *at == paired)
                    {
                        lines.push(Line::styled(
                            clip_text(problem, usize::from(body.width)),
                            Style::default().fg(PEACH()),
                        ));
                        if active == paired {
                            focus_line = lines.len().saturating_sub(1);
                        }
                    }
                }
                if let Some(status) = palette
                    .model_status
                    .as_deref()
                    .or_else(|| palette.model_capabilities.as_ref()?.reason.as_deref())
                {
                    lines.push(Line::styled(
                        clip_text(status, usize::from(body.width)),
                        Style::default().fg(MUTED()),
                    ));
                    if active == model {
                        focus_line = lines.len().saturating_sub(1);
                    }
                }
                index = model + 1;
                continue;
            }

            if matches!(field.kind, FieldKind::Toggle) {
                let expanded = field.value == "expanded";
                let line = format!("{} {}", if expanded { "▾" } else { "▸" }, field.label);
                targets.push((lines.len(), PaletteTarget::Field(index, false)));
                lines.push(Line::styled(
                    clip_text(&line, usize::from(body.width)),
                    Style::default()
                        .fg(if active == index { MAUVE() } else { SUBTEXT() })
                        .add_modifier(if active == index {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ));
                if active == index {
                    focus_line = lines.len().saturating_sub(1);
                }
                index += 1;
                continue;
            }

            let compact_picker = compact_controls && compact_picker_field(command, field);
            if matches!(field.kind, FieldKind::Multiline) {
                let display_label = if field.label == "Initial prompt" {
                    "Prompt (optional)"
                } else {
                    field.label
                };
                targets.push((lines.len(), PaletteTarget::Field(index, false)));
                lines.push(creation_field_label(
                    display_label,
                    body.width,
                    active == index,
                ));
                let text_lines = field.value.bytes().filter(|byte| *byte == b'\n').count() + 1;
                let visible_rows = creation_prompt_visible_rows(body.width);
                let cursor_line = if active == index {
                    palette.cursor.line(&field.value)
                } else {
                    0
                };
                let first_line = creation_prompt_first_line(text_lines, visible_rows, cursor_line);
                let inner_width = body.width.saturating_sub(2) as usize;
                for offset in 0..visible_rows {
                    let visible_line = first_line + offset;
                    let target_line = visible_line.min(text_lines.saturating_sub(1));
                    let placeholder =
                        field.value.is_empty() && visible_line == 0 && active != index;
                    let mut display = if placeholder {
                        "What should the agent work on?".to_owned()
                    } else {
                        palette
                            .cursor
                            .display_line(&field.value, visible_line, inner_width)
                            .unwrap_or_default()
                    };
                    if !placeholder && (active != index || cursor_line != visible_line) {
                        display.pop();
                    }
                    targets.push((
                        lines.len(),
                        PaletteTarget::MultilineLine(index, target_line),
                    ));
                    lines.push(creation_surface_line(
                        &display,
                        body.width,
                        active == index && cursor_line == visible_line,
                        placeholder,
                    ));
                    if active == index && cursor_line == visible_line {
                        focus_line = lines.len().saturating_sub(1);
                    }
                }
                if let Some((_, problem)) = palette.invalid.as_ref().filter(|(at, _)| *at == index)
                {
                    lines.push(Line::styled(
                        clip_text(problem, usize::from(body.width)),
                        Style::default().fg(PEACH()),
                    ));
                    if active == index {
                        focus_line = lines.len().saturating_sub(1);
                    }
                }
                index += 1;
                continue;
            }

            let display_label = if field.label == "Initial prompt" {
                "Prompt (optional)"
            } else {
                field.label
            };
            let display = creation_field_display(field, index, active, palette, body.width);
            let value_line = if compact_picker {
                targets.push((lines.len(), PaletteTarget::Field(index, true)));
                creation_inline_surface_line(
                    display_label,
                    &display.0,
                    body.width,
                    active == index,
                    display.1,
                )
            } else {
                targets.push((lines.len(), PaletteTarget::Field(index, false)));
                lines.push(creation_field_label(
                    display_label,
                    body.width,
                    active == index,
                ));
                targets.push((lines.len(), PaletteTarget::Field(index, true)));
                creation_surface_line(&display.0, body.width, active == index, display.1)
            };
            lines.push(value_line);
            if active == index {
                focus_line = lines.len().saturating_sub(1);
            }

            if field.label == "Model"
                && let Some(status) = palette
                    .model_status
                    .as_deref()
                    .or_else(|| palette.model_capabilities.as_ref()?.reason.as_deref())
            {
                lines.push(Line::styled(
                    clip_text(status, usize::from(body.width)),
                    Style::default().fg(MUTED()),
                ));
                if active == index {
                    focus_line = lines.len().saturating_sub(1);
                }
            }
            if let Some((_, problem)) = palette.invalid.as_ref().filter(|(at, _)| *at == index) {
                lines.push(Line::styled(
                    clip_text(problem, usize::from(body.width)),
                    Style::default().fg(PEACH()),
                ));
                if active == index {
                    focus_line = lines.len().saturating_sub(1);
                }
            }

            index += 1;
        }

        let scroll = palette
            .scroll
            .unwrap_or_else(|| {
                focus_line.saturating_sub(usize::from(body.height.saturating_sub(1)))
            })
            .min(lines.len().saturating_sub(usize::from(body.height)));
        (lines, targets, scroll)
    }

    fn close_confirm_text(&self, palette: &Palette) -> CloseConfirmText {
        let Page::Confirm { request, target } = &palette.page else {
            return CloseConfirmText {
                warning: None,
                identity: CLOSE_TITLE.to_string(),
                place: String::new(),
                project: String::new(),
                workspace: String::new(),
                error: palette.error.clone(),
                pending: palette.pending.is_some(),
            };
        };
        let Some(Request::CloseTerminal { session, .. }) = request.as_deref() else {
            return CloseConfirmText {
                warning: None,
                identity: CLOSE_TITLE.to_string(),
                place: target.clone(),
                project: String::new(),
                workspace: String::new(),
                error: palette.error.clone(),
                pending: palette.pending.is_some(),
            };
        };
        let Some(found) = find_session(self, *session) else {
            return CloseConfirmText {
                warning: None,
                identity: close_terminal_identity("Terminal", session.0),
                place: target.clone(),
                project: "unavailable".into(),
                workspace: "unavailable".into(),
                error: palette.error.clone(),
                pending: palette.pending.is_some(),
            };
        };
        let workspace = find_workspace(self, &found.project, &found.workspace)
            .map(|workspace| workspace_label(self, workspace))
            .unwrap_or_else(|| found.workspace.clone());
        let name = found.display_name();
        CloseConfirmText {
            warning: found.close_warning(),
            identity: close_terminal_identity(name, found.id.0),
            place: format!("{} / {name}", session_workspace_heading(self, found)),
            project: found.project.clone(),
            workspace,
            error: palette.error.clone(),
            pending: palette.pending.is_some(),
        }
    }

    fn close_confirm_mouse(&mut self, mouse: MouseEvent, area: Rect) -> DashboardAction {
        let text = self.close_confirm_text(self.palette.as_ref().expect("close confirm is open"));
        let layout = measure_close_confirm(area, &text);
        let point = Position::new(mouse.column, mouse.row);
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) && layout.dialog.contains(point)
        {
            let max = usize::from(layout.body_rows).saturating_sub(usize::from(layout.text.height));
            let delta = if mouse.kind == MouseEventKind::ScrollUp {
                -1
            } else {
                1
            };
            let palette = self.palette.as_mut().expect("close confirm is open");
            let current = palette.scroll.unwrap_or(0);
            palette.scroll = Some(current.saturating_add_signed(delta).min(max));
            return DashboardAction::Redraw;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && layout.cancel.contains(point) {
            return self.palette_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && layout.confirm.contains(point) {
            return self.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        }
        // Outside clicks and chrome clicks stay on the overlay. They do not
        // reach the dashboard underneath, and they do not confirm.
        DashboardAction::None
    }

    fn draw_close_confirm(&self, frame: &mut Frame<'_>, palette: &Palette) {
        let text = self.close_confirm_text(palette);
        let layout = measure_close_confirm(frame.area(), &text);
        frame.render_widget(Clear, layout.dialog);
        frame.render_widget(
            Block::bordered()
                .border_style(Style::default().fg(MAUVE()))
                .style(Style::default().bg(CRUST()).fg(TEXT())),
            layout.dialog,
        );
        paint_rect(frame, layout.rail, Style::default().bg(PEACH()).fg(PEACH()));
        paint_close_confirm_title(frame, layout.dialog);
        let lines = close_confirm_lines(&text, layout.text.width);
        let max_scroll = lines.len().saturating_sub(usize::from(layout.text.height));
        let scroll = palette.scroll.unwrap_or(0).min(max_scroll);
        if layout.text.width > 0 && layout.text.height > 0 {
            frame.render_widget(
                Paragraph::new(lines).scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0)),
                layout.text,
            );
        }
        if layout.text.width > 0 && layout.cancel.height > 0 {
            frame.render_widget(
                Paragraph::new(close_confirm_action_line())
                    .alignment(Alignment::Right)
                    .style(Style::default().fg(MAUVE()).add_modifier(Modifier::BOLD)),
                Rect::new(
                    layout.text.x,
                    layout.cancel.y,
                    layout.text.width,
                    layout.cancel.height,
                ),
            );
        }
    }

    pub(super) fn draw_palette(&self, frame: &mut Frame<'_>) {
        let Some(palette) = &self.palette else {
            return;
        };
        if close_terminal_id(&palette.page).is_some() {
            self.draw_close_confirm(frame, palette);
            return;
        }
        let (area, inner, body, buttons) = palette_geometry_for(frame.area(), Some(palette));
        let undersized_creation_form = creation_form_below_minimum(&palette.page, frame.area());
        frame.render_widget(Clear, area);
        let title = match &palette.page {
            Page::Search { .. } if palette.agents.is_some() => "Agents",
            Page::Search { .. } if palette.archive.is_some() => "Archived sessions",
            Page::Search { .. } if palette.split => "New pane · choose its session",
            Page::Search { .. } => "Command palette",
            Page::Form { command, .. } => match command {
                Command::NewAgentSession => "New agent session",
                Command::CreateTerminal => "Create terminal",
                Command::CreateWorkspace => "Create workspace",
                Command::CreateBranch => "Create branch",
                Command::RenameTerminal(_) => "Rename terminal · blank = original name",
                Command::RecoverLaunch => "Workspace retained · recover launch",
                Command::RecoverInitialPrompt => "Agent start refused · recover prompt",
                Command::RegisterProject => "Register project",
                Command::RemoveWorkspace => "Remove workspace",
                _ => "Remove project",
            },
            Page::Confirm { .. } if palette.connect_status => "Connect agent status",
            Page::Confirm { .. } => "Confirm action",
        };
        let launch_banner = if let Page::Form { fields, .. } = &palette.page {
            fields
                .iter()
                .find(|f| f.label == "Start")
                .map(|start| {
                    if start.value == "Agent" {
                        let preset = fields
                            .iter()
                            .find(|f| f.label == "Agent")
                            .map(|f| f.value.as_str())
                            .unwrap_or("");
                        format!(" · Start: Agent ({preset})")
                    } else {
                        format!(" · Start: {}", start.value)
                    }
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        let block = Block::bordered()
            .title(format!(" {title}{launch_banner} "))
            .border_style(Style::default().fg(MAUVE()))
            .style(Style::default().bg(CRUST()).fg(TEXT()));
        frame.render_widget(block, area);
        let footer_height = buttons.y.saturating_sub(body.bottom());
        let (lines, targets, scroll) = self.palette_body(body);
        let picker_popup = creation_picker_popup(body, palette, &targets, scroll);
        let paragraph = Paragraph::new(lines)
            .scroll((scroll as u16, 0))
            .style(Style::default().fg(TEXT()));
        frame.render_widget(
            if matches!(palette.page, Page::Confirm { .. }) {
                paragraph.wrap(Wrap { trim: false })
            } else {
                paragraph
            },
            body,
        );
        let detail = if let Page::Search { query, selected } = &palette.page {
            let entries = self.palette_entries(
                query,
                palette.archive.as_deref(),
                palette.agents.as_deref(),
                palette.workflows.as_deref(),
                palette.split,
            );
            let selected = (*selected).min(entries.len().saturating_sub(1));
            entries
                .get(selected)
                .map(|entry| {
                    if let Some(session) = self.entry_session(&entry.command, palette) {
                        if palette.agents.is_some() && footer_height < 3 {
                            let width = usize::from(inner.width);
                            let project_width = width.saturating_sub(1) / 2;
                            let workspace =
                                find_workspace(self, &session.project, &session.workspace)
                                    .map(|workspace| workspace_label(self, workspace))
                                    .unwrap_or_else(|| session.workspace.clone());
                            return format!(
                                "{}/{}\n↑↓ select · Enter · Esc",
                                clip_text(&session.project, project_width),
                                clip_text(&workspace, width.saturating_sub(project_width + 1))
                            );
                        }
                        format!(
                            "{}\n{}\n↑/↓ select · Enter focus · Esc close",
                            clip_text(
                                &format!("Project: {}", session.project),
                                usize::from(inner.width)
                            ),
                            clip_text(
                                &format!(
                                    "Workspace: {}",
                                    find_workspace(self, &session.project, &session.workspace)
                                        .map(|workspace| workspace_label(self, workspace))
                                        .unwrap_or_else(|| session.project.clone())
                                ),
                                usize::from(inner.width)
                            )
                        )
                    } else if let Some(hint) = self.command_hint(&entry.command) {
                        format!(
                            "{} — {}\n↑/↓ select · Enter continue · Esc cancel",
                            hint.key, hint.description
                        )
                    } else if palette.split {
                        format!(
                            "↑/↓ select · Enter show in new pane · Esc keep {}",
                            self.focused_session()
                                .and_then(|id| find_session(self, id))
                                .map_or("suggestion", |session| session.display_name())
                        )
                    } else {
                        "↑/↓ select · Enter continue · Esc cancel".into()
                    }
                })
                .or_else(|| Some("Edit search · Backspace delete · Esc close".into()))
        } else {
            None
        };
        let mut status_area = Rect::new(inner.x, inner.y + body.height, inner.width, footer_height);
        // After a refusal the way out keeps a line of its own, however narrow.
        let recovering = palette.pending.is_none()
            && (palette.error.is_some() || palette.invalid.is_some())
            && status_area.height > 1
            && picker_popup.is_none();
        let keys = if undersized_creation_form {
            "Resize to 60×18 to continue · Esc cancel".into()
        } else {
            match &palette.page {
                Page::Search { .. } => detail
                    .clone()
                    .unwrap_or_else(|| "↑/↓ select · Enter continue · Esc cancel".into()),
                Page::Form {
                    command,
                    fields,
                    active,
                    ..
                } => form_keys(
                    command,
                    fields,
                    *active,
                    recovering,
                    palette.new_agent_picker_open,
                ),
                Page::Confirm { .. } => "Enter confirm · Esc cancel".into(),
            }
        };
        let status = if palette.pending.is_some() {
            "Working…".to_owned()
        } else if undersized_creation_form {
            palette.error.as_ref().map_or_else(
                || "Resize to at least 60×18; the draft will be kept.".to_owned(),
                |error| format!("{error} · resize to at least 60×18; the draft will be kept"),
            )
        } else if picker_popup.is_some()
            && let Some(problem) = palette.error.as_deref().or_else(|| {
                palette
                    .invalid
                    .as_ref()
                    .map(|(_, problem)| problem.as_str())
            })
        {
            // The overlay may cover the inline refusal. Keep its explanation
            // and the menu's actual controls in the fixed three-row footer.
            let filter_help = match &palette.page {
                Page::Form { fields, active, .. } if matches!(&fields[*active].kind, FieldKind::Pick(list) if !list.query.is_empty()) => {
                    "↑/↓ choose · type filter · Ctrl-U clear"
                }
                _ => "↑/↓ choose · type filter",
            };
            format!(
                "{}\nEnter accept · Esc close picker · Tab fields\n{filter_help}",
                clip_text(problem, usize::from(status_area.width))
            )
        } else if let Some(error) = &palette.error {
            error.clone()
        } else if palette.submit_when_ready {
            "Waiting for Git suggestions before creating… · Esc cancel".to_owned()
        } else if matches!(&palette.page, Page::Form { fields, .. } if fields.iter().any(|f| f.label == "Start" && f.value == "Agent") && self.selected_launch(fields, "", palette.model_capabilities.as_ref()).is_err())
        {
            "Remembered/selected agent unavailable. Choose another agent or select Terminal explicitly.".to_owned()
        } else if let Some(note) = &palette.suggestions.note {
            format!("{note}\n{keys}")
        } else {
            keys
        };
        if recovering {
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
                Paragraph::new(recovery).style(Style::default().fg(SUBTEXT())),
                Rect::new(inner.x, status_area.bottom(), inner.width, 1),
            );
        }
        let refresh_branch = palette.pending.is_none()
            && palette
                .branch_attempt
                .as_ref()
                .is_some_and(|attempt| attempt.blocked);
        let button_line = if undersized_creation_form {
            Line::from("[Resize to 60×18] [Cancel]")
        } else {
            palette_button_line(&palette.page, buttons.width, refresh_branch)
        };
        frame.render_widget(
            Paragraph::new(button_line).style(Style::default().fg(MAUVE())),
            buttons,
        );
        frame.render_widget(
            Paragraph::new(status)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(if palette.error.is_some() {
                    PEACH()
                } else {
                    SUBTEXT()
                })),
            status_area,
        );
        if let Some(popup) = picker_popup {
            frame.render_widget(Clear, popup.area);
            let block = Block::bordered()
                .border_style(Style::default().fg(MAUVE()))
                .style(Style::default().bg(SURFACE0()).fg(TEXT()));
            let content = block.inner(popup.area);
            frame.render_widget(block, popup.area);
            frame.render_widget(
                Paragraph::new(popup.lines).style(Style::default().bg(SURFACE0()).fg(TEXT())),
                content,
            );
        }
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

fn field_rows_after(
    field: &Field,
    index: usize,
    palette: &Palette,
    command: &Command,
    compact_choice_dialog: bool,
) -> usize {
    if field.hidden {
        return 0;
    }
    if matches!(field.kind, FieldKind::Notice) {
        return 1;
    }
    let problem = usize::from(
        palette
            .invalid
            .as_ref()
            .is_some_and(|(invalid, _)| *invalid == index),
    );
    if matches!(field.kind, FieldKind::Multiline) {
        return 2 + field.value.bytes().filter(|byte| *byte == b'\n').count() + problem;
    }
    let compact = compact_choice_dialog
        && (compact_picker_field(command, field) || matches!(field.kind, FieldKind::Toggle));
    let model_status = usize::from(
        field.label == "Model"
            && (palette.model_status.is_some()
                || palette
                    .model_capabilities
                    .as_ref()
                    .is_some_and(|capabilities| capabilities.reason.is_some())),
    );
    (if compact { 1 } else { 2 }) + problem + model_status
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
fn unavailable_agent(name: &str) -> String {
    format!("Agent '{name}' is unavailable. Choose another agent or explicitly select Terminal.")
}

/// Whether this Agent can open its own resume menu when OVRCR cannot certify
/// the exact recorded conversation. Providers without a picker keep the
/// explicit unavailable notice instead of launching an unverifiable command.
pub(super) fn native_picker(session: &SessionSummary) -> bool {
    match &session.kind {
        SessionKind::Agent { name } => crate::protocol::AgentProvider::from_name(name)
            .is_some_and(crate::protocol::AgentProvider::has_native_resume_picker),
        SessionKind::Terminal => false,
    }
}

/// Why Git would refuse `name` as a branch, following
/// `git check-ref-format --branch`; `None` when Git accepts it.
fn branch_name_problem(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("Add a branch name");
    }
    if name.contains(' ') {
        return Some("Branch names can't contain spaces");
    }
    if name.chars().any(|ch| ch < ' ' || ch == '\u{7f}') {
        return Some("Branch names can't contain control characters");
    }
    if name.contains(['~', '^', ':', '?', '*', '[', '\\']) {
        return Some("Branch names can't contain ~ ^ : ? * [ or \\");
    }
    if name.starts_with('-') {
        return Some("Branch names can't start with -");
    }
    if name == "@" || name == "HEAD" {
        return Some("Git reserves that name");
    }
    if name.contains("..") || name.contains("@{") {
        return Some("Branch names can't contain .. or @{");
    }
    if name.starts_with('/') || name.ends_with('/') || name.contains("//") {
        return Some("Branch names can't start or end with / or contain //");
    }
    if name.ends_with('.') {
        return Some("Branch names can't end with .");
    }
    if name
        .split('/')
        .any(|part| part.starts_with('.') || part.ends_with(".lock"))
    {
        return Some("No part of a branch name can start with . or end with .lock");
    }
    None
}

/// The keys that work in this form, for its focused field. While a refusal
/// shows, its own line offers Esc and Ctrl-u, so they are left out here.
fn form_keys(
    command: &Command,
    fields: &[Field],
    active: usize,
    recovering: bool,
    new_agent_picker_open: bool,
) -> String {
    let field = &fields[active];
    if new_agent_picker_open
        && compact_picker_field(command, field)
        && let FieldKind::Pick(list) = &field.kind
    {
        let action = if matches!(command, Command::NewAgentSession) {
            "Alt-Enter start"
        } else {
            "Alt-Enter create"
        };
        let mut help = format!(
            "↑/↓ choose · type filter · Enter accept\nTab fields · {action} · Esc close picker"
        );
        if !list.query.is_empty() {
            help.push_str("\nCtrl-U clear filter");
        }
        return help;
    }
    let mut keys = Vec::new();
    match (command, field.label) {
        (Command::CreateTerminal, "Name") => keys.push("Blank = automatic name"),
        (Command::RenameTerminal(_), _) => keys.push("Blank = original name"),
        (_, "Command") => keys.push("Blank = your shell"),
        _ => {}
    }
    match &field.kind {
        FieldKind::Text if !recovering => keys.push("Ctrl-u clear"),
        FieldKind::Text => {}
        FieldKind::Multiline => keys.push("Enter newline"),
        FieldKind::Toggle => keys.push("Space/←/→ switch"),
        FieldKind::Notice => {}
        FieldKind::Pick(list) if compact_picker_field(command, field) => {
            keys.push(if new_agent_picker_open {
                "↑/↓ choose · type to filter · Enter accept"
            } else {
                "Enter open choices · ←/→ cycle"
            });
            if !list.query.is_empty() && !recovering {
                keys.push("Ctrl-u clear filter");
            }
        }
        FieldKind::Pick(list)
            if matches!(command, Command::CreateTerminal) && field.label == "Workspace" =>
        {
            keys.push("↑/↓ choose · type to filter · Enter accept");
            if !list.query.is_empty() && !recovering {
                keys.push("Ctrl-u clear filter");
            }
        }
        FieldKind::Pick(list) => {
            keys.push("↑/↓/←/→ choose · type to filter");
            if !list.query.is_empty() && !recovering {
                keys.push("Ctrl-u clear filter");
            }
        }
        FieldKind::Path(_) => {
            let value = field.value.trim();
            keys.push(
                if !value.is_empty() && expand_path(value).join(".git").exists() {
                    "↑/↓ highlight · Tab next"
                } else {
                    "↑/↓ highlight · Tab complete"
                },
            );
            if !field.value.is_empty() && !recovering {
                keys.push("Ctrl-u clear");
            }
        }
    }
    let visible = visible_indices(fields);
    if visible.len() > 1 {
        if !matches!(field.kind, FieldKind::Path(_)) {
            keys.push("Tab/Shift-Tab fields");
        } else if visible.first() != Some(&active) {
            keys.push("Shift-Tab back");
        }
    }
    keys.push(if matches!(&field.kind, FieldKind::Multiline) {
        if matches!(command, Command::RecoverInitialPrompt) {
            "Alt-Enter retry"
        } else {
            "Alt-Enter start"
        }
    } else if matches!(command, Command::NewAgentSession)
        && matches!(&field.kind, FieldKind::Pick(_))
    {
        "Alt-Enter start"
    } else if matches!(command, Command::CreateBranch) && compact_picker_field(command, field) {
        "Alt-Enter create"
    } else {
        match command {
            Command::NewAgentSession => "Enter continue · Alt-Enter start",
            Command::CreateTerminal | Command::CreateWorkspace | Command::CreateBranch => {
                "Enter create"
            }
            Command::RegisterProject => "Enter register",
            Command::RenameTerminal(_) => "Enter save",
            _ => "Enter continue",
        }
    });
    if !recovering {
        keys.push(
            if matches!(command, Command::NewAgentSession) && new_agent_picker_open {
                "Esc close picker"
            } else {
                "Esc cancel"
            },
        );
    }
    match field.label {
        "Task" => format!("{} · {}", task_hint(fields), keys.join(" · ")),
        _ => keys.join(" · "),
    }
}

/// The workspace Task chooses a branch. Startup text belongs in Initial prompt.
fn task_hint(_fields: &[Field]) -> String {
    "Names the branch; use Initial prompt for startup text".into()
}

impl Dashboard {
    pub(super) fn queue_wip_prompt(&mut self, project: String, workspace: String, branch: String) {
        self.wip_prompts.push_back((project, workspace, branch));
        self.present_wip_prompt();
    }

    fn present_wip_prompt(&mut self) {
        if self.palette.is_some() {
            return;
        }
        let Some((project, workspace, branch)) = self.wip_prompts.front().cloned() else {
            return;
        };
        self.mode = InputMode::Browse;
        self.palette = Some(Palette {
            page: Page::Confirm {
                request: Some(Box::new(Request::AnswerWipSave {
                    project,
                    workspace,
                    save: true,
                })),
                target: format!("Save uncommitted work to origin/wip/{branch}?"),
            },
            ..Palette::new()
        });
    }

    /// A workspace's live sessions: removing it stops them first.
    fn running_sessions(&self, project: &str, id: &str) -> Vec<&SessionSummary> {
        find_workspace(self, project, id)
            .map(|workspace| {
                workspace
                    .sessions
                    .iter()
                    .filter(|session| session.phase.is_live() && !session.archived)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A session's title, with its id only when another session shares it.
    pub(super) fn session_title(&self, session: &SessionSummary) -> String {
        let title = session.display_name();
        let shared = self
            .hierarchy
            .projects
            .iter()
            .flat_map(|project| &project.workspaces)
            .flat_map(|workspace| &workspace.sessions)
            .any(|other| other.id != session.id && other.display_name() == title);
        if shared {
            format!("{title} (#{})", session.id.0)
        } else {
            title.to_owned()
        }
    }

    fn session_name(&self, id: SessionId) -> String {
        find_session(self, id)
            .map(|session| self.session_title(session))
            .unwrap_or_else(|| format!("#{}", id.0))
    }

    fn workspace_name(&self, project: &str, id: &str) -> String {
        find_workspace(self, project, id)
            .map(|workspace| workspace_heading(self, workspace))
            .unwrap_or_else(|| "that workspace".into())
    }

    /// A workspace's stopped sessions whose processes may still run: removal
    /// refuses them until each is acknowledged.
    fn unacknowledged_sessions(&self, project: &str, id: &str) -> Vec<&SessionSummary> {
        find_workspace(self, project, id)
            .map(|workspace| {
                workspace
                    .sessions
                    .iter()
                    .filter(|session| {
                        !session.phase.is_live()
                            && session
                                .recovery
                                .as_ref()
                                .is_some_and(|recovery| recovery.requires_ack)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The confirmation for removing a workspace or unregistering a project.
    /// A workspace's running sessions are named, because confirming stops
    /// them first; discarding uncommitted changes is offered only on its own.
    /// A blocker the Dashboard can already see comes first, as a notice that
    /// sends nothing, so no session is stopped for a removal that would fail.
    fn removal_page(&self, request: Request) -> Page {
        if let Request::RemoveWorkspace { project, name, .. } = &request {
            let waiting = self.unacknowledged_sessions(project, name);
            if !waiting.is_empty() {
                let one = waiting.len() == 1;
                return Page::Confirm {
                    request: None,
                    target: format!(
                        "{} has {} stopped session{} that may still have processes running: {}. Once {} stopped, choose \"Acknowledge stopped\" for {} in : commands, then remove the workspace. Nothing was stopped or removed.",
                        self.workspace_name(project, name),
                        waiting.len(),
                        if one { "" } else { "s" },
                        waiting
                            .iter()
                            .map(|session| self.session_title(session))
                            .collect::<Vec<_>>()
                            .join(", "),
                        if one { "it has" } else { "they have" },
                        if one { "it" } else { "each" },
                    ),
                };
            }
        }
        let target = match &request {
            Request::RemoveWorkspace {
                project,
                name,
                force,
            } => {
                let display = self.workspace_name(project, name);
                let running = self.running_sessions(project, name);
                match (running.len(), *force) {
                    (0, false) => format!(
                        "Remove workspace {display}. Remove its clean worktree; keep the branch. Archive stopped sessions with their original paths."
                    ),
                    (0, true) => format!(
                        "{display} has uncommitted changes. Discard them and remove the workspace? The branch is kept."
                    ),
                    (count, force) => format!(
                        "{display} still has {count} running session{}: {}. Stop {}{} and remove the workspace? The branch is kept.{}",
                        if count == 1 { "" } else { "s" },
                        running
                            .iter()
                            .map(|session| self.session_title(session))
                            .collect::<Vec<_>>()
                            .join(", "),
                        if count == 1 { "it" } else { "them" },
                        if force {
                            ", discard its uncommitted changes"
                        } else {
                            ""
                        },
                        running
                            .iter()
                            .find_map(|session| session.close_warning())
                            .map(|warning| format!(" {warning}"))
                            .unwrap_or_default(),
                    ),
                }
            }
            Request::RemoveProject { name } => format!(
                "Unregister project {name}. Remove workspaces first. Keep the repository and archived session context."
            ),
            _ => unreachable!("only workspace/project removal uses this confirmation"),
        };
        Page::Confirm {
            request: Some(Box::new(request)),
            target,
        }
    }

    /// What confirming `request` sends, in order: removing a workspace first
    /// closes each running session its confirmation named.
    fn removal_steps(&self, request: &Request) -> Vec<Request> {
        let mut steps: Vec<Request> = match request {
            Request::RemoveWorkspace { project, name, .. } => self
                .running_sessions(project, name)
                .into_iter()
                .map(|session| Request::CloseTerminal {
                    session: session.id,
                    expected_run: session.run,
                })
                .collect(),
            _ => Vec::new(),
        };
        steps.push(request.clone());
        steps
    }

    /// A Server refusal in plain words: what could not be done, then the
    /// Server's reason without its error code.
    fn plain_failure(&self, palette: &Palette, message: &str) -> String {
        let lead = match &palette.page {
            Page::Form {
                command: Command::CreateBranch,
                ..
            } => "Couldn't create the branch".to_owned(),
            Page::Form {
                command: Command::CreateTerminal | Command::RecoverLaunch,
                ..
            } => "Couldn't start the session".to_owned(),
            Page::Form {
                command: Command::RegisterProject,
                ..
            } => "Couldn't register the project".to_owned(),
            Page::Form {
                command: Command::RenameTerminal(_),
                ..
            } => "Couldn't rename the session".to_owned(),
            Page::Confirm { request, .. } => match request.as_deref() {
                Some(Request::RemoveWorkspace { project, name, .. }) => {
                    format!("Couldn't remove {}", self.workspace_name(project, name))
                }
                Some(Request::RemoveProject { name }) => format!("Couldn't unregister {name}"),
                Some(Request::CloseTerminal { session, .. }) => {
                    format!("Couldn't close {}", self.session_name(*session))
                }
                _ => String::new(),
            },
            _ => String::new(),
        };
        // The workspace being created is not in the hierarchy yet.
        let created = match &palette.page {
            Page::Form {
                command: Command::CreateWorkspace,
                fields,
                ..
            } => palette.workspace_id.as_ref().map(|id| {
                let value = |label| {
                    fields
                        .iter()
                        .find(|field| field.label == label)
                        .map(|field| field.value.trim().to_owned())
                        .unwrap_or_default()
                };
                (value("Project"), id.clone(), value("Branch"))
            }),
            Page::Form {
                command: Command::CreateBranch,
                fields,
                ..
            } => palette.workspace_id.as_ref().map(|id| {
                let value = |label| {
                    fields
                        .iter()
                        .find(|field| field.label == label)
                        .map(|field| field.value.trim().to_owned())
                        .unwrap_or_default()
                };
                (value("Project"), id.clone(), value("Branch name"))
            }),
            _ => None,
        };
        let reason = self.plain_reason(message, created);
        if lead.is_empty() {
            reason
        } else {
            format!("{lead}: {reason}")
        }
    }

    /// The Server's reason for an inline form error: [`Self::plain_error`]'s ids and
    /// titles, plus the form-only cleanups below. `created` names a workspace the
    /// hierarchy does not have yet: project, id and branch.
    pub(super) fn plain_reason(
        &self,
        message: &str,
        created: Option<(String, String, String)>,
    ) -> String {
        let mut text = message.to_owned();
        let known = self
            .hierarchy
            .projects
            .iter()
            .flat_map(|project| &project.workspaces)
            .map(|workspace| (workspace.project.clone(), workspace.id.clone()));
        let created_id = created
            .as_ref()
            .map(|(project, id, _)| (project.clone(), id.clone()));
        for (project, id) in known.chain(created_id) {
            // Whoever shows this already says what was being created.
            if let Some(rest) = text.strip_prefix(&format!("create workspace {project}/{id}: ")) {
                text = rest.to_owned();
            }
        }
        if let Some((project, id, branch)) = &created {
            text = replace_outside_paths(
                &text,
                &format!("{project}/{id}"),
                &format!("{project} / {branch}"),
            );
            if id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                text = replace_outside_paths(&text, id, branch);
            }
        }
        // Git's own words are the useful part of the Git runner's report.
        if let Some(start) = text.find("git command failed (")
            && let Some(end) = text[start..].find("): ")
        {
            text.replace_range(start..start + end + 3, "");
            // Git progress can otherwise push the actual refusal out of the form.
            if let Some(fatal) = text[start..].find("\nfatal: ") {
                text.replace_range(start..start + fatal + 1, "");
            }
        }
        text = text.replace("fatal: ", "");
        // The Server advises CLI commands and --force; the Dashboard has neither.
        if let Some(start) = text.find("unacknowledged stopped: ")
            && let Some(advice) = text[start..].find("; run `ovrcr terminal acknowledge-stopped")
            && let Some(end) = text[start..].find("remove with --force")
        {
            let list = text[start + "unacknowledged stopped: ".len()..start + advice].to_owned();
            text.replace_range(
                start..start + end + "remove with --force".len(),
                &format!(
                    "stopped sessions may still have processes running: {list}; once they have stopped, choose \"Acknowledge stopped\" for each (archived ones are under Archived sessions in : commands)"
                ),
            );
        }
        self.plain_error(&text)
    }

    /// The palette waiting on Lifecycle job `token`, out of view while the job
    /// runs so that a failure can bring it back.
    pub(super) fn take_palette_for_job(&mut self, token: u64) -> Option<Box<Palette>> {
        if self.palette.as_ref()?.pending != Some(token) {
            return None;
        }
        let mut palette = self.palette.take()?;
        self.lifecycle_preference = palette.launch_preference.take();
        Some(Box::new(palette))
    }

    /// Whether a returning form would land in front of the person, rather
    /// than take keys meant for a terminal or another overlay.
    pub(super) fn can_return_to_form(&self) -> bool {
        self.palette.is_none()
            && self.tasks.is_none()
            && self.details.is_none()
            && self.mode == InputMode::Browse
            && !self.operator_quick_visible()
    }

    /// Shows the form or confirmation behind a failed Lifecycle job again and
    /// handles the failure as if the Server had refused the request outright:
    /// the values stay, the reason is in plain words, and recovery pages
    /// (another agent, save or discard changes) still follow. `stopping` is
    /// the session a workspace removal could not stop. `after_workspace`: the
    /// failed job started a task's session in the workspace this form created,
    /// so the workspace stands and the failure offers launch recovery.
    pub(super) fn return_to_form(
        &mut self,
        palette: Box<Palette>,
        token: u64,
        stopping: Option<SessionId>,
        after_workspace: bool,
        code: crate::protocol::ErrorCode,
        message: String,
    ) {
        let mut palette = *palette;
        let started = after_workspace.then(|| {
            format!(
                "Couldn't start the session: {}",
                self.plain_reason(&message, None)
            )
        });
        let code = match code {
            crate::protocol::ErrorCode::OwnershipUncertain => code,
            _ if after_workspace => crate::protocol::ErrorCode::PartialFailure,
            _ => code,
        };
        palette.pending = Some(token);
        self.cancel_mouse_gesture();
        self.whichkey = None;
        if let Some(begin) = self.history_begin_request.as_mut() {
            begin.cancelled = true;
        }
        self.palette = Some(palette);
        self.mode = InputMode::Browse;
        let stopped = stopping.map(|session| {
            format!(
                "Couldn't stop {}, so the workspace was not removed: {}",
                self.session_name(session),
                self.plain_reason(&message, None)
            )
        });
        if let Some(requests) = self.palette_response(token, &Response::Error { code, message }) {
            for request in requests {
                self.push_request(request);
            }
        }
        if let Some(palette) = self.palette.as_mut()
            && matches!(
                &palette.page,
                Page::Confirm {
                    request: Some(request),
                    ..
                } if matches!(request.as_ref(), Request::RemoveWorkspace { .. })
            )
            && stopped.is_some()
        {
            palette.error = stopped;
        }
        if let Some(palette) = self.palette.as_mut()
            && matches!(
                palette.page,
                Page::Form {
                    command: Command::RecoverLaunch,
                    ..
                }
            )
            && started.is_some()
        {
            palette.error = started;
        }
    }
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
    fn terminal_name_is_optional_and_workspace_offers_first_agent() {
        let d = dashboard();
        let Page::Form { fields, .. } = d.palette_form(Command::CreateTerminal) else {
            panic!()
        };
        let name = fields.iter().find(|f| f.label == "Name").unwrap();
        assert!(!name.required, "terminal naming must be optional");
        assert!(name.value.is_empty(), "blank selects automatic naming");
        let Page::Form { fields, .. } = d.palette_form(Command::CreateWorkspace) else {
            panic!()
        };
        assert!(
            fields.iter().any(|f| f.label == "Agent"),
            "workspace must offer first agent"
        );
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
        click(&mut d, "[Cancel]", 1, 100, 30);
        assert!(d.palette.is_none());
    }
    #[test]
    fn mouse_actions_search_and_confirmation_pending_guard() {
        let mut d = dashboard();
        d.mode = InputMode::Terminal;
        click(&mut d, "[Commands]", 1, 100, 30);
        assert_eq!(d.drain_one_request().unwrap().request, Request::Inspect);
        click(&mut d, "Register project", 1, 100, 30);
        assert!(matches!(
            d.palette.as_ref().unwrap().page,
            Page::Form {
                command: Command::RegisterProject,
                ..
            }
        ));
        d.palette = Some(Palette {
            page: d.removal_page(Request::RemoveProject {
                name: "demo".into(),
            }),
            ..Palette::new()
        });
        let Some(message) = ({
            click(&mut d, "[Confirm]", 1, 100, 30);
            d.drain_one_request()
        }) else {
            panic!("confirm must submit")
        };
        assert_eq!(
            message.request,
            Request::RemoveProject {
                name: "demo".into()
            }
        );
        click(&mut d, "[Confirm]", 1, 100, 30);
        assert!(
            d.drain_outbox().is_empty(),
            "pending confirmation cannot repeat"
        );
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
        let (_, _, body, _) = palette_geometry_for_dashboard(&d, Rect::new(0, 0, 100, 30));
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
        // The wheel moves the choice itself, as the arrows do.
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(fields[0].value, "value-13");
        // Clicking another option still visible in the scrolled list takes it.
        click(&mut d, "    Choice-11", 4, 100, 30);
        let Page::Form { fields, active, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(fields[0].value, "value-11");
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
        let Some(message) = ({
            click(&mut d, "[Submit]", 1, 100, 30);
            d.drain_one_request()
        }) else {
            panic!("submit must validate all fields from first field")
        };
        assert!(matches!(message.request, Request::AddProject { name, .. } if name == "demo"));
    }
    #[test]
    fn mouse_toggle_and_scrolled_path_row() {
        let mut d = dashboard();
        d.open_create_workspace();
        click(&mut d, "Workspace options", 1, 100, 30);
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(
            fields
                .iter()
                .find(|field| field.label == "Workspace options")
                .unwrap()
                .value,
            "expanded"
        );
        assert!(
            !fields
                .iter()
                .find(|field| field.label == "Base")
                .unwrap()
                .hidden
        );
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
        let (_, _, body, _) = palette_geometry_for_dashboard(&d, area);
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
    fn deferred_palette_poll_keeps_submission_before_due_history() {
        use crate::dashboard::state::HistoryView;
        use crate::protocol::{HistoryOpened, HistorySnapshotId, TerminalSize};

        let mut d = dashboard();
        d.open_create_workspace();
        d.drain_outbox();
        let Page::Form { fields, active, .. } = &mut d.palette.as_mut().unwrap().page else {
            panic!()
        };
        // Type the empty branch name; the request stays a single workspace job.
        let project = fields
            .iter()
            .position(|field| field.label == "Project")
            .unwrap();
        *active = fields
            .iter()
            .position(|field| field.label == "Branch name")
            .unwrap();
        fields[project].value = "demo".into();
        fields[project].kind = FieldKind::Pick(PickList::new(vec![PickItem {
            label: "demo".into(),
            value: "demo".into(),
        }]));
        d.palette_paste("deferred");
        click(&mut d, "Project", 0, 100, 30);
        click(&mut d, "[Create branch]", 1, 100, 30);
        assert!(d.drain_outbox().is_empty());
        let palette = d.palette.as_mut().unwrap();
        palette.suggestions.cache.insert(
            "demo".into(),
            Ok(super::super::git_hints::Hints {
                branches: vec!["develop".into()],
                base: "develop".into(),
            }),
        );
        palette.suggestions.inspect = None;
        let mut history = HistoryView::new(
            HistoryOpened {
                session: SessionId(1),
                snapshot: HistorySnapshotId(7),
                revision: 1,
                size: TerminalSize { rows: 20, cols: 80 },
                history_rows: 0,
                total_rows: 1,
            },
            0,
        );
        history.cursor_target = None;
        d.history = Some(history);

        assert!(d.poll_palette());
        let requests = d.drain_outbox();
        assert_eq!(requests.len(), 2, "{requests:?}");
        assert!(matches!(
            requests[0].request,
            Request::CreateWorkspace { .. }
        ));
        assert!(matches!(
            requests[1].request,
            Request::HistoryPage {
                snapshot: HistorySnapshotId(7),
                ..
            }
        ));
        d.poll_palette();
        assert!(d.drain_outbox().is_empty(), "neither request may replay");
    }

    #[test]
    fn mouse_submit_from_project_survives_waiting_for_git_suggestions_once() {
        let mut d = dashboard();
        d.open_create_workspace();
        assert!(matches!(
            d.drain_one_request().unwrap().request,
            Request::Inspect
        ));
        let Page::Form { fields, active, .. } = &mut d.palette.as_mut().unwrap().page else {
            panic!()
        };
        // Type the branch name while the project inspection is pending.
        let project = fields
            .iter()
            .position(|field| field.label == "Project")
            .unwrap();
        *active = fields
            .iter()
            .position(|field| field.label == "Branch name")
            .unwrap();
        fields[project].value = "demo".into();
        fields[project].kind = FieldKind::Pick(PickList::new(vec![PickItem {
            label: "demo".into(),
            value: "demo".into(),
        }]));
        d.palette_paste("deferred");
        click(&mut d, "Project", 0, 100, 30);
        click(&mut d, "[Create branch]", 1, 100, 30);
        assert!(
            d.drain_outbox().is_empty(),
            "submission waits for inspection"
        );
        d.poll_palette();
        assert!(d.drain_outbox().is_empty(), "inspection is still pending");
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
        assert!(matches!(
            message.request,
            Request::CreateWorkspace {
                project,
                id,
                branch: BranchRequest::New { branch, base, .. }
            } if project == "demo" && id.len() == 32 && branch == "deferred" && base == "develop"
        ));
        d.poll_palette();
        assert!(
            d.drain_outbox().is_empty(),
            "later polls cannot repeat the request"
        );
        click(&mut d, "[Create branch]", 1, 100, 30);
        assert!(
            d.drain_outbox().is_empty(),
            "pending request cannot repeat on click"
        );
    }
}

#[cfg(test)]
mod codex_adapter_tests {
    use super::*;

    fn detected_codex_entry() -> super::super::agents::AgentEntry {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "codex".into(),
            "--".into(),
            "/fixture/codex".into(),
            "--approve-for-me".into(),
        ];
        super::super::agents::AgentEntry {
            name: "codex".into(),
            argv,
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }
    }

    #[test]
    fn codex_model_and_initial_prompt_require_the_exact_detected_launcher() {
        let entry = detected_codex_entry();
        let fields = vec![
            text_field("Start", "Agent".into(), false),
            text_field("Agent", "codex".into(), false),
            text_field("Workspace", "demo/root".into(), false),
        ];
        let target = model_query_target(
            &Command::CreateTerminal,
            &fields,
            std::slice::from_ref(&entry),
        )
        .unwrap()
        .expect("detected Codex has an explicit-model catalog target");
        assert_eq!(target.preset, "codex");
        assert_eq!(target.argv, entry.argv);
        assert_eq!(
            (target.project.as_str(), target.workspace.as_str()),
            ("demo", "root")
        );

        let mut dashboard = Dashboard::new(crate::session::TerminalSize {
            rows: 30,
            cols: 100,
        });
        dashboard.model_agent_entries = Some(vec![entry.clone()]);
        let launch = ovrcr_protocol::CreateAgentSessionRequest {
            session: CreateSessionRequest {
                project: "demo".into(),
                workspace: "root".into(),
                name: "codex-prompt".into(),
                label: Some("codex".into()),
                argv: entry.argv.clone(),
                kind: SessionKind::Agent {
                    name: "codex".into(),
                },
            },
            model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
            model_binding: None,
            expected_model: None,
        };
        assert!(dashboard.supports_initial_prompt_launch(&launch));

        let mut changed = launch;
        changed.session.argv.push("--profile".into());
        assert!(!dashboard.supports_initial_prompt_launch(&changed));

        let configured = super::super::agents::AgentEntry {
            origin: AgentOrigin::Configured,
            source: AgentSource::Detected,
            ..entry
        };
        assert!(model_query_target(&Command::CreateTerminal, &fields, &[configured]).is_err());
    }
}

#[cfg(test)]
mod launch_tests {
    use super::super::settings::{AgentOverride, LaunchChoice};
    use super::*;
    use crate::protocol::{
        AgentActivity, AgentModelPolicy, ErrorCode, ProjectRecord, ProjectSummary, Registry,
        ServerEvent, ServerMessage, SessionId, SessionKind, SessionPhase, SessionRecovery,
        SessionSummary, TerminalSize, WorkspaceRecord, WorkspaceSummary,
    };

    fn agent_choice(preset: &str) -> LaunchChoice {
        LaunchChoice::Agent {
            preset: preset.into(),
            model_policy: AgentModelPolicy::HarnessDefault,
        }
    }

    fn dashboard() -> Dashboard {
        let mut d = Dashboard::new(TerminalSize {
            rows: 30,
            cols: 100,
        });
        d.hierarchy.projects.push(ProjectSummary {
            name: "demo".into(),
            workspaces: vec![WorkspaceSummary {
                project: "demo".into(),
                name: "root".into(),
                id: "root".into(),
                root: false,
                warning: None,
                path: "/tmp/unused".into(),
                sessions: vec![],
            }],
        });
        d.settings.agents.push(AgentOverride {
            name: "fixture-agent".into(),
            argv: vec!["/bin/sh".into(), "-c".into(), "exit 0".into()],
        });
        d
    }

    fn workspace_record(id: &str, path: &str, branch: &str) -> WorkspaceRecord {
        WorkspaceRecord {
            id: id.into(),
            path: path.into(),
            branch: branch.into(),
            git_identity: Some(format!("identity-{id}")),
            setup_pending: false,
        }
    }

    fn registry(workspaces: Vec<WorkspaceRecord>) -> Registry {
        Registry {
            projects: vec![ProjectRecord {
                name: "demo".into(),
                repo: "/tmp/repo".into(),
                workspace_root: "/tmp/workspaces".into(),
                workspaces,
            }],
        }
    }

    fn open_registered_agent_form(d: &mut Dashboard, inventory: Registry) {
        d.key(KeyCode::Char('n'));
        let requests = d.drain_outbox();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].request, Request::Inspect);
        assert_eq!(requests[1].request, Request::List);
        let inspect = requests[0].request_id;
        assert!(
            d.palette_response(
                inspect,
                &Response::Inventory {
                    registry: inventory,
                    sessions: Vec::new(),
                },
            )
            .is_some()
        );
    }

    fn open_branch_form(d: &mut Dashboard) {
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        d.open_create_workspace();
        let requests = d.drain_outbox();
        let [inspect] = requests.as_slice() else {
            panic!("opening the branch form should inspect the registry: {requests:?}");
        };
        assert_eq!(inspect.request, Request::Inspect);
        assert!(
            d.palette_response(
                inspect.request_id,
                &Response::Inventory {
                    registry: registry(vec![workspace_record("root", "/tmp/unused", "trunk")]),
                    sessions: Vec::new(),
                },
            )
            .is_some()
        );
        let palette = d.palette.as_mut().unwrap();
        palette.suggestions.worker = None;
        palette.suggestions.in_flight = None;
        palette.suggestions.project = Some("demo".into());
        palette.suggestions.cache.insert(
            "demo".into(),
            Ok(super::super::git_hints::Hints {
                branches: vec!["trunk".into(), "develop".into()],
                base: "develop".into(),
            }),
        );
        set(d, "Branch name", "feature/child");
    }

    fn open_explicit_new_agent_form() -> (Dashboard, Vec<OsString>, ClientMessage) {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "claude".into(),
            "--".into(),
            "/fixture/claude".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "claude".into(),
            argv: argv.clone(),
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent {
                preset: "claude".into(),
                model_policy: AgentModelPolicy::Explicit("sonnet".into()),
            },
        );
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record("root", "/tmp/unused", "trunk")]),
        );
        let requests = d.drain_outbox();
        let [query] = requests.as_slice() else {
            panic!("New agent setup should query the registered workspace model: {requests:?}");
        };
        (d, argv, query.clone())
    }

    fn branch_readback(
        id: &str,
        branch: &str,
        path: &str,
        include_target: bool,
    ) -> (Registry, crate::protocol::HierarchySnapshot) {
        let mut workspaces = vec![workspace_record("root", "/tmp/unused", "trunk")];
        let mut hierarchy_workspaces = vec![WorkspaceSummary {
            project: "demo".into(),
            name: "trunk".into(),
            id: "root".into(),
            root: false,
            warning: None,
            path: "/tmp/unused".into(),
            sessions: vec![],
        }];
        if include_target {
            workspaces.push(workspace_record(id, path, branch));
            hierarchy_workspaces.push(WorkspaceSummary {
                project: "demo".into(),
                name: branch.into(),
                id: id.into(),
                root: false,
                warning: None,
                path: path.into(),
                sessions: vec![],
            });
        }
        (
            registry(workspaces),
            crate::protocol::HierarchySnapshot {
                projects: vec![ProjectSummary {
                    name: "demo".into(),
                    workspaces: hierarchy_workspaces,
                }],
            },
        )
    }

    fn finish_branch_readback(
        d: &mut Dashboard,
        inspect: ClientMessage,
        list: ClientMessage,
        registry: Registry,
        hierarchy: crate::protocol::HierarchySnapshot,
        list_first: bool,
    ) -> Vec<ClientMessage> {
        let (inspect_response, list_response) = (
            ServerMessage::Response {
                request_id: inspect.request_id,
                response: Response::Inventory {
                    registry,
                    sessions: Vec::new(),
                },
            },
            ServerMessage::Response {
                request_id: list.request_id,
                response: Response::Hierarchy(hierarchy),
            },
        );
        let mut outgoing = Vec::new();
        if list_first {
            outgoing.extend(d.handle_server_message(list_response));
            assert!(
                d.palette
                    .as_ref()
                    .is_some_and(|palette| palette.pending.is_some())
            );
            outgoing.extend(d.handle_server_message(inspect_response));
        } else {
            outgoing.extend(d.handle_server_message(inspect_response));
            assert!(
                d.palette
                    .as_ref()
                    .is_some_and(|palette| palette.pending.is_some())
            );
            outgoing.extend(d.handle_server_message(list_response));
        }
        outgoing
    }

    fn branch_completion(
        d: &mut Dashboard,
        token: u64,
        project: &str,
        workspace_id: &str,
        outcome: crate::protocol::LifecycleOutcome,
    ) -> Vec<ClientMessage> {
        d.handle_server_message(ServerMessage::Event(ServerEvent::LifecycleCompleted {
            client_token: token,
            op: crate::protocol::LifecycleOp::CreateWorkspace,
            outcome,
            target_workspace: Some(crate::protocol::WorkspaceLifecycleIdentity {
                project: project.into(),
                id: workspace_id.into(),
            }),
        }))
    }

    fn new_agent_fields(d: &Dashboard) -> (&[Field], usize) {
        let Page::Form {
            command: Command::NewAgentSession,
            fields,
            active,
            ..
        } = &d.palette.as_ref().unwrap().page
        else {
            panic!("new agent session form")
        };
        (fields, *active)
    }

    fn visible_palette_text(d: &Dashboard, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let (_, _, body, _) = palette_geometry_for_dashboard(d, area);
        let (lines, _, scroll) = d.palette_body(body);
        lines
            .iter()
            .skip(scroll)
            .take(usize::from(body.height))
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn visible_dashboard_text(d: &Dashboard, width: u16, height: u16) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| super::super::render::draw_dashboard_at(frame, d, 0))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|row| {
                (0..width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn visible_palette_dialog_text(d: &Dashboard, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let dialog = palette_geometry_for_dashboard(d, area).0;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| super::super::render::draw_dashboard_at(frame, d, 0))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (dialog.y..dialog.bottom())
            .map(|row| {
                (dialog.x..dialog.right())
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn n_opens_agent_setup_even_when_the_current_workspace_cannot_launch() {
        let mut d = dashboard();
        d.hierarchy.projects[0].workspaces[0].warning =
            Some("repository root is not on the default branch".into());
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });

        d.key(KeyCode::Char('n'));

        let Some(Palette {
            page:
                Page::Form {
                    command: Command::NewAgentSession,
                    fields,
                    ..
                },
            ..
        }) = d.palette.as_ref()
        else {
            panic!("opening agent setup must not reuse the current target's launch guard")
        };
        for control in ["Branch", "Agent harness", "Model", "Initial prompt"] {
            assert!(
                fields.iter().any(|field| field.label == control),
                "missing {control}"
            );
        }
        assert_eq!(d.mode, InputMode::Browse);
    }

    #[test]
    fn new_agent_alt_enter_starts_a_valid_preselected_target_in_one_action() {
        let mut d = dashboard();
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record("root", "/tmp/unused", "main")]),
        );

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));

        assert!(!d.palette.as_ref().unwrap().new_agent_picker_open);
        assert_eq!(
            d.pending_initial_prompt_allocation
                .as_ref()
                .unwrap()
                .launch
                .session
                .workspace,
            "root"
        );
        assert!(
            d.drain_outbox()
                .iter()
                .any(|message| { message.request == Request::AllocateInitialPromptIntent })
        );
    }

    #[test]
    fn new_agent_primary_controls_fit_at_80x24_and_60x18_without_picker_expansion() {
        let mut d = dashboard();
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        let mut workspaces = vec![workspace_record("root", "/tmp/unused", "main")];
        workspaces.extend((0..7).map(|index| {
            workspace_record(
                &format!("extra-{index}"),
                &format!("/tmp/extra-{index}"),
                &format!("feature/extra-{index}"),
            )
        }));
        open_registered_agent_form(&mut d, registry(workspaces));

        for (width, height) in [(80, 24), (60, 18)] {
            let text = visible_palette_text(&d, width, height);
            for control in [
                "Project",
                "Branch",
                "Agent harness",
                "Model",
                "Prompt (optional)",
            ] {
                assert!(
                    text.contains(control),
                    "{width}x{height} hid {control}:\n{text}"
                );
            }
            assert!(
                new_agent_fields(&d)
                    .0
                    .iter()
                    .any(|field| field.label == "Advanced options" && !field.hidden),
                "{width}x{height} should keep Advanced options reachable"
            );
            let screen = visible_dashboard_text(&d, width, height);
            assert!(
                screen.contains("[Start agent]"),
                "{width}x{height}:\n{screen}"
            );
            assert!(screen.contains("[Cancel]"), "{width}x{height}:\n{screen}");
            assert!(
                !text.contains("feature/extra-"),
                "the unopened Branch picker must stay compact:\n{text}"
            );
        }

        d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let (fields, active) = new_agent_fields(&d);
        assert_eq!(fields[active].label, "Branch");
        assert!(
            !d.palette.as_ref().unwrap().new_agent_picker_open,
            "Tab only moves focus"
        );
        for label in ["Branch", "Agent harness", "Model"] {
            assert!(
                !fields
                    .iter()
                    .find(|field| field.label == label)
                    .unwrap()
                    .edited,
                "Tab must not make {label} an explicit choice"
            );
        }
        for (width, height) in [(80, 24), (60, 18)] {
            let text = visible_palette_text(&d, width, height);
            for control in [
                "Project",
                "Branch",
                "Agent harness",
                "Model",
                "Prompt (optional)",
            ] {
                assert!(
                    text.contains(control),
                    "{width}x{height} hid {control} after Tab:\n{text}"
                );
            }
            assert!(
                !text.contains("feature/extra-"),
                "Tab must not open the Branch picker:\n{text}"
            );
        }
    }

    #[test]
    fn new_agent_open_pickers_overlay_a_stable_form_for_branch_harness_and_model() {
        for picker_label in ["Branch", "Agent harness", "Model"] {
            let mut d = dashboard();
            d.selected_container = Some(super::super::TreeRow::Workspace {
                project: "demo".into(),
                id: "root".into(),
            });
            let mut workspaces = vec![workspace_record("root", "/tmp/unused", "root")];
            for index in 0..9 {
                let id = format!("extra-{index}");
                let path = format!("/tmp/{id}");
                let branch = format!("feature/{index}");
                d.hierarchy.projects[0].workspaces.push(WorkspaceSummary {
                    project: "demo".into(),
                    name: branch.clone(),
                    id: id.clone(),
                    root: false,
                    warning: None,
                    path: path.clone().into(),
                    sessions: vec![],
                });
                workspaces.push(workspace_record(&id, &path, &branch));
            }
            open_registered_agent_form(&mut d, registry(workspaces));

            let tabs_to_picker = match picker_label {
                "Branch" => 1,
                "Agent harness" => 2,
                "Model" => 3,
                _ => unreachable!(),
            };
            for _ in 0..tabs_to_picker {
                d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            }
            let selected_label = if picker_label == "Branch" {
                let Page::Form { fields, .. } = &mut d.palette.as_mut().unwrap().page else {
                    panic!("New agent form")
                };
                let field = fields
                    .iter_mut()
                    .find(|field| field.label == "Branch")
                    .unwrap();
                let FieldKind::Pick(list) = &mut field.kind else {
                    panic!("Branch picker")
                };
                let selected = list
                    .items
                    .iter()
                    .find(|item| item.value == "demo/extra-8")
                    .unwrap()
                    .clone();
                list.select_value(&selected.value);
                field.value.clone_from(&selected.value);
                field.edited = true;
                selected.label
            } else {
                let Page::Form { fields, .. } = &mut d.palette.as_mut().unwrap().page else {
                    panic!("New agent form")
                };
                let field = fields
                    .iter_mut()
                    .find(|field| field.label == picker_label)
                    .unwrap();
                let selected_value = format!("{picker_label}-9");
                let items = (0..10)
                    .map(|index| PickItem {
                        label: format!("{picker_label}-{index}"),
                        value: format!("{picker_label}-{index}"),
                    })
                    .collect::<Vec<_>>();
                let mut list = PickList::new(items);
                list.select_value(&selected_value);
                field.value.clone_from(&selected_value);
                field.kind = FieldKind::Pick(list);
                selected_value
            };
            if picker_label == "Agent harness" {
                d.palette.as_mut().unwrap().launch_harness = Some(selected_label.clone());
            }
            let model_index = new_agent_fields(&d)
                .0
                .iter()
                .position(|field| field.label == "Model")
                .unwrap();
            {
                let palette = d.palette.as_mut().unwrap();
                palette.model_status = Some("Loading verified model choices…".into());
                palette.invalid = Some((model_index, "Model lookup failed".into()));
                let Page::Form { fields, .. } = &mut palette.page else {
                    panic!("New agent form")
                };
                fields
                    .iter_mut()
                    .find(|field| field.label == "Initial prompt")
                    .unwrap()
                    .value = (0..10)
                    .map(|line| format!("Prompt line {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
            }

            let closed = base_form_snapshot(&d, 80, 24);
            let prompt_before = new_agent_fields(&d)
                .0
                .iter()
                .find(|field| field.label == "Initial prompt")
                .unwrap()
                .value
                .clone();
            d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert!(d.palette.as_ref().unwrap().new_agent_picker_open);
            assert_eq!(
                form_geometry(&base_form_snapshot(&d, 80, 24)),
                form_geometry(&closed)
            );
            assert_eq!(
                new_agent_fields(&d)
                    .0
                    .iter()
                    .find(|field| field.label == "Initial prompt")
                    .unwrap()
                    .value,
                prompt_before,
                "opening a menu must keep every prompt byte"
            );
            assert_eq!(
                new_agent_fields(&d).0[new_agent_fields(&d).1].label,
                picker_label
            );

            for (width, height) in [(164, 60), (80, 24), (60, 22), (60, 18)] {
                let open = base_form_snapshot(&d, width, height);
                let closed = base_form_snapshot_closed_state(&mut d, width, height);
                assert_eq!(
                    form_geometry(&open),
                    form_geometry(&closed),
                    "{picker_label} popup at {width}x{height} must preserve the base form"
                );
                let (body, popup, popup_lines, option_rows) =
                    creation_popup_snapshot(&d, width, height);
                assert!(
                    rect_contains_rect(body, popup),
                    "{picker_label} popup escaped its body at {width}x{height}: {popup:?} vs {body:?}"
                );
                assert!(
                    !option_rows.is_empty(),
                    "{picker_label} popup has no selectable rows at {width}x{height}"
                );
                assert!(
                    popup_lines
                        .iter()
                        .any(|line| line.contains("›") && line.contains(&selected_label)),
                    "{picker_label} popup at {width}x{height} hid selected option {selected_label}:\n{}",
                    popup_lines.join("\n")
                );
                let dialog = visible_palette_dialog_text(&d, width, height);
                assert!(
                    dialog.contains("[Start agent]"),
                    "{picker_label} picker at {width}x{height} hid Start:\n{dialog}"
                );
                assert!(
                    dialog.contains("[Cancel]"),
                    "{picker_label} picker at {width}x{height} hid Cancel:\n{dialog}"
                );
            }
        }
    }

    #[test]
    fn create_branch_project_picker_overlays_stable_form() {
        let mut d = dashboard();
        open_branch_form(&mut d);
        let mut palette = d.palette.take().unwrap();
        let projects = (0..10)
            .map(|index| {
                let name = format!("project-{index:02}");
                let path = format!("/tmp/{name}");
                palette
                    .suggestions
                    .repos
                    .insert(name.clone(), format!("{path}/repo").into());
                ProjectRecord {
                    name,
                    repo: format!("{path}/repo").into(),
                    workspace_root: format!("{path}/workspaces").into(),
                    workspaces: vec![workspace_record("root", &path, "main")],
                }
            })
            .collect();
        palette.workspace_inventory = Some(Registry { projects });
        palette.branch_project = Some("project-09".into());
        palette.suggestions.cache.insert(
            "project-09".into(),
            Ok(super::super::git_hints::Hints {
                branches: vec!["main".into()],
                base: "main".into(),
            }),
        );
        if let Page::Form { fields, .. } = &mut palette.page {
            fields
                .iter_mut()
                .find(|field| field.label == "Project")
                .unwrap()
                .value = "project-09".into();
            fields
                .iter_mut()
                .find(|field| field.label == "Branch name")
                .unwrap()
                .value = "feature/child".into();
            fields
                .iter_mut()
                .find(|field| field.label == "Workspace options")
                .unwrap()
                .value = "expanded".into();
            let base = fields
                .iter_mut()
                .find(|field| field.label == "Base")
                .unwrap();
            base.value = "main".into();
            base.hidden = false;
        }
        d.refresh_workspace_form(&mut palette);
        if let Page::Form { fields, active, .. } = &mut palette.page {
            *active = fields
                .iter()
                .position(|field| field.label == "Branch name")
                .unwrap();
        }
        d.palette = Some(palette);

        d.key(KeyCode::BackTab);
        let Page::Form { fields, active, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("Create branch form")
        };
        assert_eq!(fields[*active].label, "Project", "BackTab focuses Project");
        let closed = base_form_snapshot(&d, 80, 24);
        d.key(KeyCode::Enter);
        assert!(d.palette.as_ref().unwrap().new_agent_picker_open);
        assert_eq!(
            form_geometry(&base_form_snapshot(&d, 80, 24)),
            form_geometry(&closed)
        );

        for (width, height) in [(164, 60), (80, 24), (60, 22), (60, 18)] {
            let open = base_form_snapshot(&d, width, height);
            let closed = base_form_snapshot_closed_state(&mut d, width, height);
            assert_eq!(
                form_geometry(&open),
                form_geometry(&closed),
                "project popup at {width}x{height} must preserve the branch form"
            );
            let dialog = visible_palette_dialog_text(&d, width, height);
            let (body, popup, popup_lines, option_rows) =
                creation_popup_snapshot(&d, width, height);
            assert!(
                rect_contains_rect(body, popup),
                "popup outside body: {popup:?} vs {body:?}"
            );
            assert!(!option_rows.is_empty(), "project menu has no options");
            assert!(
                popup_lines
                    .iter()
                    .any(|line| line.contains("›") && line.contains("project-09")),
                "Create branch picker at {width}x{height} hid the selected project:\n{dialog}"
            );
            assert!(dialog.contains("[Create branch]"), "{dialog}");
            assert!(dialog.contains("[Cancel]"), "{dialog}");
        }

        let (body, popup, popup_lines, option_rows) = creation_popup_snapshot(&d, 80, 24);
        let project_option = match &d.palette.as_ref().unwrap().page {
            Page::Form { fields, .. } => fields
                .iter()
                .find(|field| field.label == "Project")
                .and_then(|field| match &field.kind {
                    FieldKind::Pick(list) => list
                        .filtered()
                        .iter()
                        .position(|item| item.value == "project-08"),
                    _ => None,
                })
                .unwrap(),
            _ => unreachable!(),
        };
        let option_row = option_rows
            .iter()
            .find_map(|(row, option)| (*option == project_option).then_some(*row))
            .unwrap();
        assert!(popup_lines.iter().any(|line| line.contains("project-08")));
        d.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: popup.x + 2,
                row: option_row,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 80, 24),
        );
        let Page::Form { fields, active, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("Create branch form should remain active");
        };
        let project = fields
            .iter()
            .find(|field| field.label == "Project")
            .unwrap();
        assert_eq!(fields[*active].label, "Project");
        assert_eq!(project.value, "project-08");
        assert!(project.edited);
        assert!(!d.palette.as_ref().unwrap().new_agent_picker_open);
        assert!(d.drain_outbox().iter().all(|message| !matches!(
            message.request,
            Request::CreateWorkspace { .. } | Request::CreateWorkspaceWithLaunch { .. }
        )));
        assert!(rect_contains_rect(body, popup));
    }

    #[test]
    fn undersized_creation_forms_keep_drafts_and_block_input_until_resized() {
        for branch_form in [false, true] {
            let mut d = dashboard();
            let model_argv = vec![
                "/fixture/ovrcr".into(),
                "agent".into(),
                "run".into(),
                "claude".into(),
                "--".into(),
                "/fixture/claude".into(),
            ];
            d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
                name: "claude".into(),
                argv: model_argv.clone(),
                source: AgentSource::Detected,
                origin: AgentOrigin::Detected,
            }]);
            let area = if branch_form {
                open_branch_form(&mut d);
                Rect::new(0, 0, 60, 17)
            } else {
                d.selected_container = Some(super::super::TreeRow::Workspace {
                    project: "demo".into(),
                    id: "root".into(),
                });
                open_registered_agent_form(
                    &mut d,
                    registry(vec![workspace_record("root", "/tmp/unused", "trunk")]),
                );
                let Page::Form { fields, .. } = &mut d.palette.as_mut().unwrap().page else {
                    panic!("New agent session form")
                };
                fields
                    .iter_mut()
                    .find(|field| field.label == "Initial prompt")
                    .unwrap()
                    .value = "retained prompt".into();
                Rect::new(0, 0, 59, 18)
            };
            let setup_requests = d.drain_outbox();
            if branch_form {
                assert!(setup_requests.is_empty());
            } else {
                let [setup] = setup_requests.as_slice() else {
                    panic!("opening New agent session should query model capabilities");
                };
                let Request::QueryAgentModels {
                    preset,
                    origin,
                    argv,
                    project,
                    workspace,
                } = &setup.request
                else {
                    panic!("New agent setup should only query model capabilities");
                };
                assert_eq!(preset, "claude");
                assert_eq!(origin, &AgentOrigin::Detected);
                assert_eq!(argv, &model_argv);
                assert_eq!(project, "demo");
                assert_eq!(workspace, "root");
            }

            d.install_area(area);
            let visible = visible_palette_dialog_text(&d, area.width, area.height);
            assert!(
                visible.contains("Resize to at least 60 columns × 18 rows"),
                "{visible}"
            );
            assert!(visible.contains("[Resize to 60×18]"), "{visible}");
            assert!(visible.contains("[Cancel]"), "{visible}");

            let (_, _, _, buttons) = palette_geometry_for_dashboard(&d, area);
            d.palette_mouse(
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: buttons.x + 2,
                    row: buttons.y,
                    modifiers: KeyModifiers::NONE,
                },
                area,
            );
            d.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
            d.palette_paste("must not edit the draft");
            assert!(
                d.drain_outbox().is_empty(),
                "undersized form must not submit"
            );

            let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
                panic!("undersized form should remain open")
            };
            if branch_form {
                assert_eq!(
                    fields
                        .iter()
                        .find(|field| field.label == "Branch name")
                        .unwrap()
                        .value,
                    "feature/child"
                );
            } else {
                assert_eq!(
                    fields
                        .iter()
                        .find(|field| field.label == "Initial prompt")
                        .unwrap()
                        .value,
                    "retained prompt"
                );
            }

            d.install_area(Rect::new(0, 0, 60, 18));
            let resized = visible_palette_dialog_text(&d, 60, 18);
            assert!(
                !resized.contains("Resize to at least 60 columns × 18 rows"),
                "{resized}"
            );
            assert!(
                resized.contains(if branch_form {
                    "feature/child"
                } else {
                    "retained prompt"
                }),
                "draft was not retained after resizing:\n{resized}"
            );
        }
    }

    #[test]
    fn new_agent_enter_opens_then_accepts_and_escape_closes_before_cancelling() {
        let mut d = dashboard();
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        d.hierarchy.projects[0].workspaces.push(WorkspaceSummary {
            project: "demo".into(),
            name: "feature/extra".into(),
            id: "extra".into(),
            root: false,
            warning: None,
            path: "/tmp/extra".into(),
            sessions: vec![],
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![
                workspace_record("root", "/tmp/unused", "main"),
                workspace_record("extra", "/tmp/extra", "feature/extra"),
            ]),
        );
        d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let (fields, _) = new_agent_fields(&d);
        let branch = fields.iter().find(|field| field.label == "Branch").unwrap();
        let selected = branch.value.clone();

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(d.palette.as_ref().unwrap().new_agent_picker_open);
        let (fields, _) = new_agent_fields(&d);
        let branch = fields.iter().find(|field| field.label == "Branch").unwrap();
        assert_eq!(branch.value, selected);
        assert!(
            !branch.edited,
            "opening the picker is not accepting a choice"
        );
        assert!(d.drain_outbox().iter().all(|message| !matches!(
            message.request,
            Request::CreateAgentSession(_)
                | Request::CreateAgentSessionWithPrompt(_)
                | Request::AllocateInitialPromptIntent
        )));
        assert!(
            creation_popup_snapshot(&d, 80, 24)
                .2
                .iter()
                .any(|line| line.contains("feature/extra"))
        );

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!d.palette.as_ref().unwrap().new_agent_picker_open);
        let (fields, active) = new_agent_fields(&d);
        let branch = fields.iter().find(|field| field.label == "Branch").unwrap();
        assert_eq!(branch.value, selected);
        assert!(branch.edited, "accepting the current target is explicit");
        assert_eq!(
            fields[active].label, "Branch",
            "picker acceptance keeps focus"
        );

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(d.palette.is_some(), "Escape closes the open picker first");
        assert!(!d.palette.as_ref().unwrap().new_agent_picker_open);
        assert_eq!(
            new_agent_fields(&d)
                .0
                .iter()
                .find(|field| field.label == "Branch")
                .unwrap()
                .value,
            selected
        );
        d.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(d.palette.is_none(), "the next Escape cancels the form");
    }

    #[test]
    fn picker_tab_and_backtab_close_before_advancing_fields() {
        let mut d = dashboard();
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record("root", "/tmp/unused", "main")]),
        );
        d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let branch_index = new_agent_fields(&d)
            .0
            .iter()
            .position(|field| field.label == "Branch")
            .unwrap();
        let branch_value = new_agent_fields(&d).0[branch_index].value.clone();

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(d.palette.as_ref().unwrap().new_agent_picker_open);
        d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let (fields, active) = new_agent_fields(&d);
        assert_eq!(fields[active].label, "Agent harness");
        assert!(!d.palette.as_ref().unwrap().new_agent_picker_open);
        assert_eq!(fields[branch_index].value, branch_value);
        assert!(!fields[branch_index].edited);

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(d.palette.as_ref().unwrap().new_agent_picker_open);
        d.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        let (fields, active) = new_agent_fields(&d);
        assert_eq!(fields[active].label, "Branch");
        assert!(!d.palette.as_ref().unwrap().new_agent_picker_open);
        assert_eq!(fields[branch_index].value, branch_value);
        assert!(!fields[branch_index].edited);
        assert!(d.drain_outbox().iter().all(|message| !matches!(
            message.request,
            Request::CreateAgentSession(_)
                | Request::CreateAgentSessionWithPrompt(_)
                | Request::AllocateInitialPromptIntent
        )));
    }

    #[test]
    fn clicking_a_new_agent_field_opens_its_picker_and_option_accepts_only() {
        let mut d = dashboard();
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        d.hierarchy.projects[0].workspaces.push(WorkspaceSummary {
            project: "demo".into(),
            name: "feature/clicked".into(),
            id: "clicked".into(),
            root: false,
            warning: None,
            path: "/tmp/clicked".into(),
            sessions: vec![],
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![
                workspace_record("root", "/tmp/unused", "main"),
                workspace_record("clicked", "/tmp/clicked", "feature/clicked"),
            ]),
        );
        let branch_index = new_agent_fields(&d)
            .0
            .iter()
            .position(|field| field.label == "Branch")
            .unwrap();
        let area = Rect::new(0, 0, 80, 24);
        let (_, _, body, _) = palette_geometry_for_dashboard(&d, area);
        let (_, targets, scroll) = d.palette_body(body);
        let row = targets
            .iter()
            .find_map(|(row, target)| match target {
                PaletteTarget::Field(index, false) if *index == branch_index => Some(*row),
                _ => None,
            })
            .unwrap();
        d.palette_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: body.x + 2,
                row: body.y + (row - scroll) as u16,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert!(d.palette.as_ref().unwrap().new_agent_picker_open);

        let (_, popup, _, option_rows) = creation_popup_snapshot(&d, 80, 24);
        let selected = new_agent_fields(&d)
            .0
            .iter()
            .find(|field| field.label == "Branch")
            .and_then(|field| match &field.kind {
                FieldKind::Pick(list) => list
                    .filtered()
                    .iter()
                    .position(|item| item.value == "demo/clicked"),
                _ => None,
            })
            .unwrap();
        let option_row = option_rows
            .iter()
            .find_map(|(row, option)| (*option == selected).then_some(*row))
            .unwrap();
        assert!(popup.contains(Position::new(popup.x + 2, option_row)));
        let expected_label = new_agent_fields(&d)
            .0
            .iter()
            .find(|field| field.label == "Branch")
            .and_then(|field| match &field.kind {
                FieldKind::Pick(list) => {
                    list.filtered().get(selected).map(|item| item.label.clone())
                }
                _ => None,
            })
            .unwrap();
        let buffer = render_creation_form(&d, 80, 24);
        let rendered_option_row = (0..buffer.area.width)
            .map(|column| buffer[(column, option_row)].symbol())
            .collect::<String>();
        assert!(
            rendered_option_row.contains(&expected_label),
            "the rendered popup row must match its shared mouse option target: {rendered_option_row}"
        );
        d.palette_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: popup.x + 2,
                row: option_row,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert!(!d.palette.as_ref().unwrap().new_agent_picker_open);
        let (fields, active) = new_agent_fields(&d);
        let branch = fields.iter().find(|field| field.label == "Branch").unwrap();
        assert_eq!(branch.value, "demo/clicked");
        assert!(branch.edited);
        assert_eq!(active, branch_index);
        assert!(d.drain_outbox().iter().all(|message| !matches!(
            message.request,
            Request::CreateAgentSession(_)
                | Request::CreateAgentSessionWithPrompt(_)
                | Request::AllocateInitialPromptIntent
        )));
    }

    #[test]
    fn new_agent_branch_picker_uses_registered_membership_and_live_labels() {
        let mut d = dashboard();
        d.hierarchy.projects[0].workspaces.push(WorkspaceSummary {
            project: "demo".into(),
            name: "feature/registered".into(),
            id: "registered".into(),
            root: false,
            warning: None,
            path: "/tmp/registered".into(),
            sessions: Vec::new(),
        });
        d.hierarchy.projects[0].workspaces.push(WorkspaceSummary {
            project: "demo".into(),
            name: "retained-only".into(),
            id: "synthetic".into(),
            root: false,
            warning: None,
            path: "/tmp/synthetic".into(),
            sessions: Vec::new(),
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record(
                "registered",
                "/tmp/registered",
                "old-name",
            )]),
        );

        let (fields, _) = new_agent_fields(&d);
        let branch = fields.iter().find(|field| field.label == "Branch").unwrap();
        let FieldKind::Pick(list) = &branch.kind else {
            panic!("Branch must use a registered-workspace picker")
        };
        assert_eq!(
            list.items
                .iter()
                .map(|item| item.value.as_str())
                .collect::<Vec<_>>(),
            ["demo/registered"]
        );
        assert_eq!(list.items[0].label, "feature/registered");

        let hierarchy = &mut d.hierarchy.projects[0].workspaces;
        hierarchy
            .iter_mut()
            .find(|workspace| workspace.id == "registered")
            .unwrap()
            .name = "feature/renamed".into();
        let mut palette = d.palette.take().unwrap();
        d.refresh_workspace_form(&mut palette);
        d.palette = Some(palette);
        let (fields, _) = new_agent_fields(&d);
        let branch = fields.iter().find(|field| field.label == "Branch").unwrap();
        assert_eq!(branch.value, "");
        let FieldKind::Pick(list) = &branch.kind else {
            panic!("Branch picker")
        };
        assert_eq!(list.items[0].label, "feature/renamed");
        assert!(
            fields
                .iter()
                .find(|field| field.label == "Destination")
                .unwrap()
                .value
                .contains("Choose a registered checkout")
        );
    }

    #[test]
    fn new_agent_requires_a_deliberate_branch_when_no_current_target_exists() {
        let mut d = dashboard();
        d.selected_container = None;
        d.focus_pane(0);
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record(
                "registered",
                "/tmp/registered",
                "feature/registered",
            )]),
        );

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        let requests = d.drain_outbox();
        assert!(
            requests.iter().all(|message| !matches!(
                message.request,
                Request::CreateAgentSession(_)
                    | Request::CreateAgentSessionWithPrompt(_)
                    | Request::AllocateInitialPromptIntent
            )),
            "an empty Branch must not emit a launch request: {requests:?}"
        );
        let (fields, _) = new_agent_fields(&d);
        assert!(
            fields
                .iter()
                .find(|field| field.label == "Branch")
                .unwrap()
                .value
                .is_empty()
        );
        assert_eq!(
            d.palette.as_ref().unwrap().invalid.as_ref().unwrap().1,
            "Branch is required"
        );

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(d.palette.as_ref().unwrap().new_agent_picker_open);
        assert!(
            new_agent_fields(&d)
                .0
                .iter()
                .find(|field| field.label == "Branch")
                .unwrap()
                .value
                .is_empty(),
            "opening the picker must not select its first item"
        );
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let (fields, active) = new_agent_fields(&d);
        assert_eq!(
            fields
                .iter()
                .find(|field| field.label == "Branch")
                .unwrap()
                .value,
            "demo/registered"
        );
        assert_eq!(
            active,
            fields
                .iter()
                .position(|field| field.label == "Branch")
                .unwrap(),
            "picker Enter accepts without submitting or advancing"
        );
    }

    #[test]
    fn tab_does_not_implicitly_choose_a_branch_when_no_current_target_exists() {
        let mut d = dashboard();
        d.selected_container = None;
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record(
                "registered",
                "/tmp/registered",
                "feature/registered",
            )]),
        );

        d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        let (fields, active) = new_agent_fields(&d);
        assert_eq!(fields[active].label, "Branch");
        assert!(
            fields
                .iter()
                .find(|field| field.label == "Branch")
                .unwrap()
                .value
                .is_empty()
        );
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        assert!(d.drain_outbox().is_empty());
        assert_eq!(
            d.palette.as_ref().unwrap().invalid.as_ref().unwrap().1,
            "Branch is required"
        );
    }

    #[test]
    fn new_agent_keeps_unavailable_current_identity_selected_and_blocks_start() {
        let mut d = dashboard();
        d.hierarchy.projects[0].workspaces[0].name = "checkout unavailable".into();
        d.hierarchy.projects[0].workspaces[0].root = true;
        d.hierarchy.projects[0].workspaces[0].warning =
            Some("repository root is not on the default branch".into());
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record("root", "/tmp/unused", "main")]),
        );

        let (fields, _) = new_agent_fields(&d);
        let branch = fields.iter().find(|field| field.label == "Branch").unwrap();
        assert_eq!(branch.value, "demo/root");
        let FieldKind::Pick(list) = &branch.kind else {
            panic!("Branch picker")
        };
        assert_eq!(
            list.items[0].label,
            "checkout unavailable [unavailable · root · launch blocked]"
        );
        assert!(
            fields
                .iter()
                .find(|field| field.label == "Destination")
                .unwrap()
                .value
                .contains("checkout unavailable")
        );

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        let requests = d.drain_outbox();
        assert!(
            requests.iter().all(|message| !matches!(
                message.request,
                Request::CreateAgentSession(_)
                    | Request::CreateAgentSessionWithPrompt(_)
                    | Request::AllocateInitialPromptIntent
            )),
            "an unavailable root must not emit a launch request: {requests:?}"
        );
        let (fields, active) = new_agent_fields(&d);
        assert_eq!(
            active,
            fields
                .iter()
                .position(|field| field.label == "Branch")
                .unwrap()
        );
        assert!(
            d.palette
                .as_ref()
                .unwrap()
                .invalid
                .as_ref()
                .unwrap()
                .1
                .contains("unavailable")
        );
    }

    #[test]
    fn new_agent_follows_branch_rename_by_registered_workspace_identity() {
        let mut d = dashboard();
        d.hierarchy.projects[0].workspaces[0].id = "registered".into();
        d.hierarchy.projects[0].workspaces[0].path = "/tmp/registered".into();
        d.hierarchy.projects[0].workspaces[0].name = "feature/original".into();
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "registered".into(),
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record(
                "registered",
                "/tmp/registered",
                "feature/original",
            )]),
        );

        let (fields, _) = new_agent_fields(&d);
        assert_eq!(
            fields
                .iter()
                .find(|field| field.label == "Branch")
                .unwrap()
                .value,
            "demo/registered"
        );
        d.hierarchy.projects[0].workspaces[0].name = "feature/renamed".into();
        let mut palette = d.palette.take().unwrap();
        d.refresh_workspace_form(&mut palette);
        d.palette = Some(palette);

        let (fields, _) = new_agent_fields(&d);
        let branch = fields.iter().find(|field| field.label == "Branch").unwrap();
        assert_eq!(branch.value, "demo/registered");
        let FieldKind::Pick(list) = &branch.kind else {
            panic!("Branch picker")
        };
        assert_eq!(list.items[0].label, "feature/renamed");
        assert_eq!(
            fields
                .iter()
                .find(|field| field.label == "Destination")
                .unwrap()
                .value,
            "demo / feature/renamed"
        );
    }

    #[test]
    fn new_agent_start_keeps_prompt_separate_from_name_and_argv() {
        let mut d = dashboard();
        let argv = vec!["/fixture/claude".into()];
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "claude".into(),
            argv: argv.clone(),
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.hierarchy.projects[0].workspaces[0].id = "registered".into();
        d.hierarchy.projects[0].workspaces[0].path = "/tmp/registered".into();
        d.hierarchy.projects[0].workspaces[0].name = "feature/raw-prompt".into();
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "registered".into(),
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record(
                "registered",
                "/tmp/registered",
                "feature/raw-prompt",
            )]),
        );
        let prompt = "line one\n  exact second line 🙂\n";
        let Page::Form { fields, active, .. } = &mut d.palette.as_mut().unwrap().page else {
            panic!("new agent form")
        };
        let prompt_index = fields
            .iter()
            .position(|field| field.label == "Initial prompt")
            .unwrap();
        fields[prompt_index].value = prompt.into();
        fields[prompt_index].edited = true;
        *active = prompt_index;

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));

        let requests = d.drain_outbox();
        let request = requests
            .iter()
            .find(|message| message.request == Request::AllocateInitialPromptIntent)
            .expect("intent allocation request");
        assert_eq!(request.request, Request::AllocateInitialPromptIntent);
        let pending = d.pending_initial_prompt_allocation.as_ref().unwrap();
        assert_eq!(pending.prompt, prompt);
        assert_eq!(pending.launch.session.project, "demo");
        assert_eq!(pending.launch.session.workspace, "registered");
        assert_eq!(pending.launch.session.name, "");
        assert_eq!(pending.launch.session.argv, argv);
        assert!(
            pending
                .launch
                .session
                .argv
                .iter()
                .all(|argument| argument != prompt)
        );
    }

    #[test]
    fn accepting_the_prefilled_harness_keeps_it_explicit_across_project_change() {
        let mut d = dashboard();
        d.settings.agents.push(AgentOverride {
            name: "other-agent".into(),
            argv: vec!["/bin/sh".into(), "-c".into(), "exit 0".into()],
        });
        d.settings
            .launch_choices
            .insert("demo".into(), agent_choice("fixture-agent"));
        d.settings
            .launch_choices
            .insert("other".into(), agent_choice("fixture-agent"));
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        d.hierarchy.projects.push(ProjectSummary {
            name: "other".into(),
            workspaces: vec![WorkspaceSummary {
                project: "other".into(),
                name: "feature/second".into(),
                id: "second".into(),
                root: false,
                warning: None,
                path: "/tmp/second".into(),
                sessions: vec![],
            }],
        });
        let mut inventory = registry(vec![workspace_record("root", "/tmp/unused", "main")]);
        inventory.projects.push(ProjectRecord {
            name: "other".into(),
            repo: "/tmp/second-repo".into(),
            workspace_root: "/tmp/second-workspaces".into(),
            workspaces: vec![workspace_record("second", "/tmp/second", "feature/second")],
        });
        open_registered_agent_form(&mut d, inventory);

        d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let (fields, active) = new_agent_fields(&d);
        assert_eq!(fields[active].label, "Agent harness");
        assert_eq!(fields[active].value, "fixture-agent");
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            new_agent_fields(&d)
                .0
                .iter()
                .find(|field| field.label == "Agent harness")
                .unwrap()
                .value,
            "other-agent"
        );

        d.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let (fields, _) = new_agent_fields(&d);
        let branch = fields.iter().find(|field| field.label == "Branch").unwrap();
        assert!(
            branch.value.is_empty(),
            "switching projects clears the old project's branch selection"
        );
        let FieldKind::Pick(branches) = &branch.kind else {
            panic!("Branch picker")
        };
        assert!(
            branches
                .items
                .iter()
                .all(|item| item.value.starts_with("other/"))
        );
        let harness = fields
            .iter()
            .find(|field| field.label == "Agent harness")
            .unwrap();
        assert_eq!(harness.value, "other-agent");
        assert!(
            harness.edited,
            "accepting the highlighted default is explicit"
        );
    }

    #[test]
    fn accepting_the_prefilled_model_keeps_it_explicit_across_project_change() {
        let mut d = dashboard();
        d.settings
            .launch_choices
            .insert("demo".into(), agent_choice("fixture-agent"));
        d.settings.launch_choices.insert(
            "other".into(),
            LaunchChoice::Agent {
                preset: "fixture-agent".into(),
                model_policy: AgentModelPolicy::Explicit("claude-sonnet-4".into()),
            },
        );
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        d.hierarchy.projects.push(ProjectSummary {
            name: "other".into(),
            workspaces: vec![WorkspaceSummary {
                project: "other".into(),
                name: "feature/second".into(),
                id: "second".into(),
                root: false,
                warning: None,
                path: "/tmp/second".into(),
                sessions: vec![],
            }],
        });
        let mut inventory = registry(vec![workspace_record("root", "/tmp/unused", "main")]);
        inventory.projects.push(ProjectRecord {
            name: "other".into(),
            repo: "/tmp/second-repo".into(),
            workspace_root: "/tmp/second-workspaces".into(),
            workspaces: vec![workspace_record("second", "/tmp/second", "feature/second")],
        });
        open_registered_agent_form(&mut d, inventory);

        d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let (fields, active) = new_agent_fields(&d);
        assert_eq!(fields[active].label, "Model");
        assert_eq!(fields[active].value, HARNESS_DEFAULT_MODEL);
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        d.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let (fields, _) = new_agent_fields(&d);
        let model = fields.iter().find(|field| field.label == "Model").unwrap();
        assert_eq!(model.value, HARNESS_DEFAULT_MODEL);
        assert!(model.edited, "accepting the highlighted model is explicit");
    }

    #[test]
    fn new_conversation_uses_selected_provider_despite_conflicting_launch_preference() {
        for preference in [
            None,
            Some(LaunchChoice::Terminal),
            Some(agent_choice("fixture-agent")),
        ] {
            let mut d = dashboard();
            d.settings.agents.push(AgentOverride {
                name: "claude".into(),
                argv: vec!["claude".into()],
            });
            if let Some(preference) = preference {
                d.settings.launch_choices.insert("demo".into(), preference);
            }
            let mut retained = summary(1);
            retained.kind = SessionKind::Agent {
                name: "claude".into(),
            };
            retained.phase = SessionPhase::Stopped;
            d.hierarchy.projects[0].workspaces[0]
                .sessions
                .push(retained);
            d.palette = Some(Palette {
                page: d.command_page(Command::StartNewConversation(SessionId(1))),
                ..Palette::new()
            });
            let message = request(&mut d);
            let Request::CreateAgentSession(created) = message.request else {
                panic!("new conversation must create a separate session");
            };
            assert_eq!(created.model_policy, AgentModelPolicy::HarnessDefault);
            let created = created.session;
            assert_eq!(created.project, "demo");
            assert_eq!(created.workspace, "root");
            assert_eq!(
                created.kind,
                SessionKind::Agent {
                    name: "claude".into()
                }
            );
            assert!(!created.argv.iter().any(|arg| arg == "--resume"));
        }
    }
    fn set(d: &mut Dashboard, label: &str, value: &str) {
        let Page::Form { fields, .. } = &mut d.palette.as_mut().unwrap().page else {
            panic!()
        };
        let f = fields.iter_mut().find(|f| f.label == label).unwrap();
        f.value = value.into();
        f.edited = true;
        if let FieldKind::Pick(list) = &mut f.kind {
            list.query.clear();
            list.select_value(value);
        }
    }
    fn submit(d: &mut Dashboard) -> DashboardAction {
        // The Submit control uses the explicit chord while a picker or the
        // multiline prompt has focus; plain Enter remains picker acceptance.
        let modifiers = if d.palette.as_ref().is_some_and(|palette| {
            matches!(&palette.page, Page::Form { fields, active, .. }
                    if matches!(fields[*active].kind, FieldKind::Pick(_) | FieldKind::Multiline))
        }) {
            KeyModifiers::ALT
        } else {
            KeyModifiers::NONE
        };
        d.palette_key(KeyEvent::new(KeyCode::Enter, modifiers))
    }
    fn request(d: &mut Dashboard) -> ClientMessage {
        let requests = {
            submit(d);
            d.drain_outbox()
        };
        let [request] = requests.as_slice() else {
            panic!(
                "expected exactly one request; got {requests:?}; form error: {:?}",
                d.palette
                    .as_ref()
                    .and_then(|palette| palette.error.as_ref())
            );
        };
        let mut message = request.clone();
        if message.request == Request::AllocateInitialPromptIntent {
            let sequence = d
                .initial_prompt_intent_id
                .as_ref()
                .map_or(1, |intent| intent.sequence.saturating_add(1));
            let response = d.handle_server_message(ServerMessage::Response {
                request_id: message.request_id,
                response: Response::InitialPromptIntent(crate::protocol::InitialPromptIntentId {
                    server_lifetime: "123e4567-e89b-12d3-a456-426614174000".into(),
                    sequence,
                }),
            });
            let Some(mut launch) = response.into_iter().find(|message| {
                matches!(message.request, Request::CreateAgentSessionWithPrompt(_))
            }) else {
                panic!("intent allocation must continue as one agent launch");
            };
            let Request::CreateAgentSessionWithPrompt(request) = launch.request else {
                unreachable!()
            };
            let legacy = Request::CreateAgentSession(request.launch.clone());
            d.lifecycle_pending = None;
            d.begin_lifecycle_pending(launch.request_id, &legacy);
            d.initial_prompt_intent = None;
            d.initial_prompt_intent_id = None;
            d.initial_prompt_focus_pending = None;
            if let Some(palette) = d.palette.as_mut() {
                palette.initial_prompt_intent = None;
            }
            launch.request = legacy;
            message = launch;
        }
        message
    }
    fn summary(id: u64) -> SessionSummary {
        SessionSummary {
            scope: ovrcr_protocol::SessionScope::Workspace,
            archived: false,
            cwd: "/work".into(),
            id: SessionId(id),
            run: crate::protocol::SessionRunId(1),
            kind: crate::protocol::SessionKind::Terminal,
            recovery: None,
            project: "demo".into(),
            workspace: "root".into(),
            name: format!("root-{id}"),
            title: None,
            manual_title: None,
            assignment_control: None,
            label: "shell".into(),
            pid: None,
            started_unix_ms: Some(0),
            last_activity_unix_ms: None,
            phase: SessionPhase::Running,
            activity: AgentActivity::Unknown,
            context_usage: None,
            agent: None,
            agent_epoch: 0,
            unread: None,
        }
    }
    #[test]
    fn agents_first_enter_chooses_captured_attention_priority() {
        let help = super::super::keymap::agents_binding().description;
        for term in ["Waiting Input", "Unread", "current", "captured", "reopen"] {
            assert!(
                help.contains(term),
                "shared Agents discoverability omits {term}: {help}"
            );
        }
        let mut d = dashboard();
        let mut rows = (1..=3).map(summary).collect::<Vec<_>>();
        for row in &mut rows {
            row.kind = SessionKind::Agent {
                name: "codex".into(),
            };
        }
        rows[1].activity = AgentActivity::Busy;
        rows[2].activity = AgentActivity::WaitingInput;
        d.hierarchy.projects[0].workspaces[0].sessions = rows;
        d.select_session(SessionId(1));
        d.open_agent_search();
        assert_eq!(
            d.palette
                .as_ref()
                .unwrap()
                .agents
                .as_ref()
                .unwrap()
                .iter()
                .map(|c| c.session.id)
                .collect::<Vec<_>>(),
            vec![SessionId(3), SessionId(2)]
        );
        let Some(request) = ({
            d.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            d.drain_one_request()
        }) else {
            panic!("first attention result must select");
        };
        assert!(
            matches!(request.request, Request::SetView { view } if view.focused == Some(SessionId(3)))
        );
        assert_eq!(d.mode, InputMode::Browse);
        assert!(d.agent_typing.is_some());
    }

    fn start_workspace(d: &mut Dashboard, start: &str) {
        let page = d.palette_form(Command::CreateWorkspace);
        let mut palette = Palette::new();
        palette.page = page;
        palette
            .suggestions
            .cache
            .insert("demo".into(), Err("fixture".into()));
        palette.workspace_id = Some("new-work".into());
        d.palette = Some(palette);
        set(d, "Branch", "feature/new-work");
        set(d, "Start", start);
    }
    #[test]
    fn configured_shell_reaches_terminal_workspace_and_recovery_entrypoints() {
        let argv = vec![
            OsString::from("/fixture/configured-shell"),
            OsString::from("--login"),
        ];
        for entrypoint in ["terminal", "workspace", "recovery"] {
            let mut d = dashboard();
            d.settings.agents.push(AgentOverride {
                name: "shell".into(),
                argv: argv
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect(),
            });
            if entrypoint == "terminal" {
                d.open_create_terminal();
            } else {
                start_workspace(
                    &mut d,
                    if entrypoint == "recovery" {
                        "Agent"
                    } else {
                        "Terminal"
                    },
                );
            }
            if entrypoint == "recovery" {
                set(&mut d, "Agent", "fixture-agent");
                let m = request(&mut d);
                d.handle_server_message(ServerMessage::Response {
                    request_id: m.request_id,
                    response: Response::Error {
                        code: ErrorCode::PartialFailure,
                        message: "workspace created; agent failed".into(),
                    },
                });
                d.hierarchy.projects[0].workspaces.push(WorkspaceSummary {
                    project: "demo".into(),
                    name: "new-work".into(),
                    id: "new-work".into(),
                    root: false,
                    warning: None,
                    path: "/tmp/unused".into(),
                    sessions: vec![],
                });
                set(&mut d, "Recovery", "Open shell");
            }
            let m = request(&mut d);
            match &m.request {
                Request::CreateSession(session) => {
                    assert_eq!(session.argv, argv, "{entrypoint}");
                    assert_eq!(session.label, None);
                }
                Request::CreateWorkspaceWithLaunch {
                    launch: Some(launch),
                    ..
                } => {
                    assert_eq!(launch.argv, argv);
                    assert_eq!(launch.label, None);
                }
                _ => panic!("wrong launch request: {:?}", m.request),
            }
            d.handle_server_message(ServerMessage::Response {
                request_id: m.request_id,
                response: Response::CreatedSession(Box::new(summary(1))),
            });
            assert_eq!(
                d.settings.launch_choices.get("demo"),
                Some(&LaunchChoice::Terminal),
                "{entrypoint}"
            );
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn ordinary_agent_creation_selects_named_cursor_cloud_account() {
        let mut d = dashboard();
        d.settings.cursor_cloud_accounts = vec![ovrcr_protocol::CursorCloudAccount {
            name: "work".into(),
            keychain_service: "ovrcr".into(),
            keychain_account: "work".into(),
            model: "composer-2.5".into(),
            repositories: vec!["https://github.com/acme/app".into()],
            owners: vec![],
            bridge_archive: "/tmp/bridge.tar.gz".into(),
        }];
        d.settings.agents = vec![ovrcr_protocol::AgentOverride {
            name: "cursor-cloud:work".into(),
            argv: vec!["unrelated-custom-command".into()],
        }];
        let mut fields = d.launch_fields("p", false);
        fields
            .iter_mut()
            .find(|f| f.label == "Start")
            .unwrap()
            .value = "Agent".into();
        let field = fields.iter_mut().find(|f| f.label == "Agent").unwrap();
        field.value = "cursor-cloud:work".into();
        let launch = d.selected_launch(&fields, "", None).unwrap().unwrap();
        assert_eq!(
            launch.kind,
            SessionKind::Agent {
                name: "cursor-cloud:work".into()
            }
        );
        assert_eq!(
            &launch.argv[1..],
            ["agent", "run", "cursor-cloud", "--", "work"].map(OsString::from)
        );
    }

    #[test]
    fn agent_form_offers_harness_default_before_catalog_arrives() {
        let d = dashboard();
        let fields = d.launch_fields("demo", false);
        let model = fields.iter().find(|field| field.label == "Model").unwrap();
        assert_eq!(model.value, HARNESS_DEFAULT_MODEL);
        let FieldKind::Pick(list) = &model.kind else {
            panic!("Model must be a picker");
        };
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].label, "Harness default");
    }

    #[test]
    fn detected_agent_launcher_matches_the_server_override() {
        let server = PathBuf::from("/managed/ovrcr");
        let copied_dashboard = PathBuf::from("/fixture/dashboard-client/ovrcr");
        assert_eq!(
            resolve_agent_launcher(Some(server.clone()), Some(copied_dashboard.clone()), None,),
            Some(server),
            "agent argv must identify the executable the Server verifies"
        );
        assert_eq!(
            resolve_agent_launcher(
                Some(PathBuf::from("bin/ovrcr")),
                Some(copied_dashboard),
                Some(PathBuf::from("/work")),
            ),
            Some(PathBuf::from("/work/bin/ovrcr"))
        );
    }

    #[test]
    fn mouse_selection_of_agent_starts_a_catalog_lookup() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "claude".into(),
            "--".into(),
            "/fixture/claude".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "claude".into(),
            argv,
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.open_create_terminal();
        assert!(d.drain_one_request().is_none());

        for _ in 0..8 {
            let Page::Form { fields, active, .. } = &d.palette.as_ref().unwrap().page else {
                panic!("Agent creation should remain a form");
            };
            if fields[*active].label == "Start" {
                break;
            }
            d.palette_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        let area = Rect::new(0, 0, 100, 30);
        let (_, _, body, _) = palette_geometry_for_dashboard(&d, area);
        let (_, targets, scroll) = d.palette_body(body);
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("Agent creation should remain a form");
        };
        let start = fields
            .iter()
            .position(|field| field.label == "Start")
            .unwrap();
        let FieldKind::Pick(list) = &fields[start].kind else {
            panic!("Start must be a picker");
        };
        let agent = list
            .items
            .iter()
            .position(|item| item.value == "Agent")
            .unwrap();
        let row = targets
            .iter()
            .find_map(|(row, target)| match target {
                PaletteTarget::Option(field, option) if *field == start && *option == agent => {
                    Some(*row)
                }
                _ => None,
            })
            .expect("Agent option should be visible");
        assert!(row >= scroll && row - scroll < usize::from(body.height));
        d.palette_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: body.x + 4,
                row: body.y + (row - scroll) as u16,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        let query = d
            .drain_outbox()
            .into_iter()
            .find(|message| matches!(message.request, Request::QueryAgentModels { .. }))
            .expect("mouse selection of Agent starts a catalog lookup");
        assert!(
            matches!(query.request, Request::QueryAgentModels { preset, .. } if preset == "claude")
        );
    }

    #[test]
    fn model_catalog_completion_preserves_a_typed_filter() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "claude".into(),
            "--".into(),
            "/fixture/claude".into(),
            "--dangerously-skip-permissions".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "claude".into(),
            argv,
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent {
                preset: "claude".into(),
                model_policy: AgentModelPolicy::HarnessDefault,
            },
        );
        d.open_create_terminal();
        let query = d.drain_one_request().expect("catalog lookup is pending");
        let model_index = match &mut d.palette.as_mut().unwrap().page {
            Page::Form { fields, active, .. } => {
                let index = fields
                    .iter()
                    .position(|field| field.label == "Model")
                    .unwrap();
                *active = index;
                index
            }
            _ => panic!("Agent creation should show a form"),
        };
        for character in "opu".chars() {
            d.palette_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("Agent creation should remain a form");
        };
        let FieldKind::Pick(list) = &fields[model_index].kind else {
            panic!("Model must remain a picker");
        };
        assert_eq!(list.query, "opu");

        d.agent_models_event(
            query.request_id,
            crate::protocol::AgentModelCapabilities {
                preset: "claude".into(),
                binding: Some("binding".into()),
                models: vec![crate::protocol::AgentModelOption {
                    alias: "opus".into(),
                    resolved: "claude-opus-v1".into(),
                }],
                reason: None,
            },
        );
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("Agent creation should remain a form");
        };
        let model = &fields[model_index];
        let FieldKind::Pick(list) = &model.kind else {
            panic!("Model must remain a picker");
        };
        assert_eq!(list.query, "opu");
        assert_eq!(
            model.value, HARNESS_DEFAULT_MODEL,
            "catalog completion preserves the current policy until a choice is accepted"
        );
        assert_eq!(
            list.accepted().map(|item| item.value.as_str()),
            Some("opus")
        );

        d.palette_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("Agent creation should remain a form");
        };
        assert_eq!(
            fields[model_index].value, "opus",
            "the user can explicitly accept the filtered model"
        );
    }

    #[test]
    fn catalog_refresh_does_not_replace_a_valid_model_hidden_by_the_filter() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "claude".into(),
            "--".into(),
            "/fixture/claude".into(),
            "--dangerously-skip-permissions".into(),
        ];
        let mut d = dashboard();
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "claude".into(),
            argv,
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent {
                preset: "claude".into(),
                model_policy: AgentModelPolicy::Explicit("opus".into()),
            },
        );
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record("root", "/tmp/unused", "main")]),
        );
        let query = d
            .drain_outbox()
            .into_iter()
            .find(|message| matches!(message.request, Request::QueryAgentModels { .. }))
            .expect("the New agent form requests the current model catalog");

        let model_index = new_agent_fields(&d)
            .0
            .iter()
            .position(|field| field.label == "Model")
            .unwrap();
        for _ in 0..new_agent_fields(&d).0.len() {
            if new_agent_fields(&d).1 == model_index {
                break;
            }
            d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        assert_eq!(new_agent_fields(&d).1, model_index);
        d.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        for character in "opus4".chars() {
            d.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        d.agent_models_event(
            query.request_id,
            crate::protocol::AgentModelCapabilities {
                preset: "claude".into(),
                binding: Some("current-launch-binding".into()),
                models: vec![
                    crate::protocol::AgentModelOption {
                        alias: "opus".into(),
                        resolved: "claude-opus".into(),
                    },
                    crate::protocol::AgentModelOption {
                        alias: "opus-4.2".into(),
                        resolved: "claude-opus-4.2".into(),
                    },
                ],
                reason: None,
            },
        );

        let model = &new_agent_fields(&d).0[model_index];
        let FieldKind::Pick(list) = &model.kind else {
            panic!("Model must remain a picker");
        };
        assert_eq!(model.value, "opus");
        assert_eq!(list.query, "opus4");
        assert_eq!(
            list.accepted().map(|item| item.value.as_str()),
            Some("opus-4.2"),
            "the filter can highlight another model without changing the current choice"
        );
    }

    #[test]
    fn removed_explicit_model_waits_for_a_user_choice_after_catalog_refresh() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "claude".into(),
            "--".into(),
            "/fixture/claude".into(),
            "--dangerously-skip-permissions".into(),
        ];
        let mut d = dashboard();
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "claude".into(),
            argv,
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent {
                preset: "claude".into(),
                model_policy: AgentModelPolicy::Explicit("opus".into()),
            },
        );
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record("root", "/tmp/unused", "main")]),
        );
        let query = d
            .drain_outbox()
            .into_iter()
            .find(|message| matches!(message.request, Request::QueryAgentModels { .. }))
            .expect("the New agent form requests the current model catalog");
        d.agent_models_event(
            query.request_id,
            crate::protocol::AgentModelCapabilities {
                preset: "claude".into(),
                binding: Some("current-launch-binding".into()),
                models: vec![crate::protocol::AgentModelOption {
                    alias: "opus-4.2".into(),
                    resolved: "claude-opus-4.2".into(),
                }],
                reason: None,
            },
        );

        let (fields, _) = new_agent_fields(&d);
        let model = fields.iter().find(|field| field.label == "Model").unwrap();
        assert_eq!(
            model.value, "opus",
            "a similar available model must not replace the remembered explicit choice"
        );

        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        let (fields, active) = new_agent_fields(&d);
        assert_eq!(fields[active].label, "Model");
        assert_eq!(
            d.drain_outbox(),
            Vec::<ClientMessage>::new(),
            "Start must wait for an explicit replacement choice"
        );

        d.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let (fields, active) = new_agent_fields(&d);
        assert_eq!(fields[active].label, "Model");
        assert_eq!(fields[active].value, "opus-4.2");
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        let requests = d.drain_outbox();
        let [allocation] = requests.as_slice() else {
            panic!("an accepted model should allocate one launch intent");
        };
        assert_eq!(allocation.request, Request::AllocateInitialPromptIntent);
        let launch_messages = d.handle_server_message(ServerMessage::Response {
            request_id: allocation.request_id,
            response: Response::InitialPromptIntent(crate::protocol::InitialPromptIntentId {
                server_lifetime: "123e4567-e89b-12d3-a456-426614174000".into(),
                sequence: 1,
            }),
        });
        let Some(message) = launch_messages
            .iter()
            .find(|message| matches!(message.request, Request::CreateAgentSessionWithPrompt(_)))
        else {
            panic!("accepted intent should send the selected Agent launch");
        };
        let Request::CreateAgentSessionWithPrompt(launch) = &message.request else {
            unreachable!();
        };
        assert_eq!(
            launch.launch.model_policy,
            AgentModelPolicy::Explicit("opus-4.2".into())
        );
        assert_eq!(
            launch.launch.expected_model.as_deref(),
            Some("claude-opus-4.2")
        );
        assert_eq!(
            launch.launch.model_binding.as_deref(),
            Some("current-launch-binding")
        );
    }

    #[test]
    fn detected_pi_model_query_exposes_exact_ids_and_keeps_its_binding() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "pi".into(),
            "--".into(),
            "/fixture/pi".into(),
            "--approve".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "pi".into(),
            argv: argv.clone(),
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent {
                preset: "pi".into(),
                model_policy: AgentModelPolicy::HarnessDefault,
            },
        );
        d.open_create_terminal();
        let query = d
            .drain_one_request()
            .expect("detected Pi loads its catalog");
        assert!(matches!(
            query.request,
            Request::QueryAgentModels {
                preset,
                origin: AgentOrigin::Detected,
                argv: ref selected,
                project,
                workspace,
            } if preset == "pi"
                && selected == &argv
                && project == "demo"
                && workspace == "root"
        ));

        let capabilities = crate::protocol::AgentModelCapabilities {
            preset: "pi".into(),
            binding: Some("pi-launch-binding".into()),
            models: vec![crate::protocol::AgentModelOption {
                alias: "openai/gpt-5.4".into(),
                resolved: "openai/gpt-5.4".into(),
            }],
            reason: None,
        };
        d.agent_models_event(query.request_id, capabilities.clone());
        {
            let Page::Form { fields, .. } = &mut d.palette.as_mut().unwrap().page else {
                panic!("Agent creation should remain a form");
            };
            let model = fields
                .iter_mut()
                .find(|field| field.label == "Model")
                .unwrap();
            let FieldKind::Pick(list) = &mut model.kind else {
                panic!("Model must remain a picker");
            };
            assert!(
                list.items.iter().any(|item| {
                    item.label == "openai/gpt-5.4" && item.value == "openai/gpt-5.4"
                })
            );
            model.value = "openai/gpt-5.4".into();
        }
        let mut fields = d.launch_fields("demo", false);
        fields
            .iter_mut()
            .find(|field| field.label == "Start")
            .unwrap()
            .value = "Agent".into();
        fields
            .iter_mut()
            .find(|field| field.label == "Agent")
            .unwrap()
            .value = "pi".into();
        fields
            .iter_mut()
            .find(|field| field.label == "Model")
            .unwrap()
            .value = "openai/gpt-5.4".into();
        let launch = d
            .selected_launch(&fields, "", Some(&capabilities))
            .unwrap()
            .unwrap();
        assert_eq!(
            launch.model_policy,
            AgentModelPolicy::Explicit("openai/gpt-5.4".into())
        );
        assert_eq!(launch.model_binding.as_deref(), Some("pi-launch-binding"));
        assert_eq!(launch.expected_model.as_deref(), Some("openai/gpt-5.4"));
        assert_eq!(launch.argv, argv);
        assert!(
            d.supports_initial_prompt_launch(&ovrcr_protocol::CreateAgentSessionRequest {
                session: CreateSessionRequest {
                    project: "demo".into(),
                    workspace: "root".into(),
                    name: "pi-prompt".into(),
                    label: launch.label.clone(),
                    argv: launch.argv.clone(),
                    kind: launch.kind.clone(),
                },
                model_policy: launch.model_policy.clone(),
                model_binding: launch.model_binding.clone(),
                expected_model: launch.expected_model.clone(),
            })
        );
    }

    #[test]
    fn detected_omp_model_query_exposes_exact_selectors_and_initial_prompt_support() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "omp".into(),
            "--".into(),
            "/fixture/omp".into(),
            "--auto-approve".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "omp".into(),
            argv: argv.clone(),
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent {
                preset: "omp".into(),
                model_policy: AgentModelPolicy::Explicit("openai/gpt-5.4".into()),
            },
        );
        d.open_create_terminal();
        let query = d
            .drain_one_request()
            .expect("detected OMP loads its native catalog");
        assert!(matches!(
            query.request,
            Request::QueryAgentModels {
                preset,
                origin: AgentOrigin::Detected,
                argv: ref selected,
                project,
                workspace,
            } if preset == "omp"
                && selected == &argv
                && project == "demo"
                && workspace == "root"
        ));

        let capabilities = crate::protocol::AgentModelCapabilities {
            preset: "omp".into(),
            binding: Some("omp-launch-binding".into()),
            models: vec![crate::protocol::AgentModelOption {
                alias: "openai/gpt-5.4".into(),
                resolved: "openai/gpt-5.4".into(),
            }],
            reason: None,
        };
        d.agent_models_event(query.request_id, capabilities.clone());
        let Page::Form {
            fields: current_fields,
            ..
        } = &d.palette.as_ref().unwrap().page
        else {
            panic!("Agent creation should remain a form");
        };
        assert_eq!(
            current_fields
                .iter()
                .find(|field| field.label == "Model")
                .unwrap()
                .value,
            "openai/gpt-5.4",
            "the project remembers its exact OMP model selection"
        );
        let mut fields = d.launch_fields("demo", false);
        for (label, value) in [
            ("Start", "Agent"),
            ("Agent", "omp"),
            ("Model", "openai/gpt-5.4"),
        ] {
            fields
                .iter_mut()
                .find(|field| field.label == label)
                .unwrap()
                .value = value.into();
        }
        let launch = d
            .selected_launch(&fields, "", Some(&capabilities))
            .unwrap()
            .unwrap();
        assert_eq!(
            launch.model_policy,
            AgentModelPolicy::Explicit("openai/gpt-5.4".into())
        );
        assert_eq!(launch.model_binding.as_deref(), Some("omp-launch-binding"));
        assert_eq!(launch.expected_model.as_deref(), Some("openai/gpt-5.4"));
        assert_eq!(launch.argv, argv);
        assert!(
            d.supports_initial_prompt_launch(&ovrcr_protocol::CreateAgentSessionRequest {
                session: CreateSessionRequest {
                    project: "demo".into(),
                    workspace: "root".into(),
                    name: "omp-prompt".into(),
                    label: launch.label.clone(),
                    argv: launch.argv.clone(),
                    kind: launch.kind.clone(),
                },
                model_policy: launch.model_policy.clone(),
                model_binding: launch.model_binding.clone(),
                expected_model: launch.expected_model.clone(),
            })
        );
    }

    #[test]
    fn configured_omp_keeps_model_and_initial_prompt_controls_read_only() {
        let argv = vec![
            "/custom/omp-wrapper".into(),
            "--profile".into(),
            "work".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "omp".into(),
            argv: argv.clone(),
            source: AgentSource::Detected,
            origin: AgentOrigin::Configured,
        }]);
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent {
                preset: "omp".into(),
                model_policy: AgentModelPolicy::HarnessDefault,
            },
        );
        d.open_create_terminal();
        assert!(d.drain_one_request().is_none());
        let palette = d.palette.as_ref().unwrap();
        assert!(
            palette
                .model_status
                .as_deref()
                .is_some_and(|status| { status.contains("named launcher is configured") })
        );
        let Page::Form { fields, .. } = &palette.page else {
            panic!("Agent creation should remain a form");
        };
        let model = fields.iter().find(|field| field.label == "Model").unwrap();
        let FieldKind::Pick(list) = &model.kind else {
            panic!("Model must remain a picker");
        };
        assert_eq!(list.items.len(), 1);
        assert_eq!(list.items[0].value, HARNESS_DEFAULT_MODEL);

        let launch = ovrcr_protocol::CreateAgentSessionRequest {
            session: CreateSessionRequest {
                project: "demo".into(),
                workspace: "root".into(),
                name: "configured-omp".into(),
                label: Some("omp".into()),
                argv,
                kind: SessionKind::Agent { name: "omp".into() },
            },
            model_policy: AgentModelPolicy::HarnessDefault,
            model_binding: None,
            expected_model: None,
        };
        assert!(!d.supports_initial_prompt_launch(&launch));
    }

    #[test]
    fn model_query_coalesces_target_changes_across_cancel_and_reopen() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "claude".into(),
            "--".into(),
            "/fixture/claude".into(),
            "--dangerously-skip-permissions".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "claude".into(),
            argv: argv.clone(),
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings
            .launch_choices
            .insert("demo".into(), LaunchChoice::Terminal);
        d.hierarchy.projects[0].workspaces.push(WorkspaceSummary {
            project: "demo".into(),
            name: "other".into(),
            id: "other".into(),
            root: false,
            warning: None,
            path: "/tmp/other".into(),
            sessions: vec![],
        });

        d.open_create_terminal();
        assert!(
            d.drain_one_request().is_none(),
            "Terminal setup needs no catalog"
        );
        for _ in 0..8 {
            let Page::Form { fields, active, .. } = &d.palette.as_ref().unwrap().page else {
                panic!("Agent creation should remain a form");
            };
            if fields[*active].label == "Start" {
                break;
            }
            d.palette_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        let Page::Form { fields, active, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("Agent creation should remain a form");
        };
        assert_eq!(fields[*active].label, "Start");
        d.palette_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        let first = d
            .drain_one_request()
            .expect("keyboard selection of Agent starts a catalog lookup");
        let first_id = first.request_id;
        assert!(matches!(
            first.request,
            Request::QueryAgentModels {
                preset,
                origin: AgentOrigin::Detected,
                argv: ref selected,
                project,
                workspace,
            } if preset == "claude" && selected == &argv && project == "demo" && workspace == "root"
        ));

        // Follow the same keyboard path as the visible form and leave its
        // Model field in the loading state while discovery is in flight.
        for _ in 0..4 {
            d.palette_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        let palette = d.palette.as_ref().unwrap();
        assert_eq!(
            palette.model_status.as_deref(),
            Some("Loading verified model choices…")
        );
        let Page::Form { fields, active, .. } = &palette.page else {
            panic!("Agent creation should remain a form");
        };
        assert_eq!(fields[*active].label, "Model");
        assert_eq!(fields[*active].value, HARNESS_DEFAULT_MODEL);
        let FieldKind::Pick(models) = &fields[*active].kind else {
            panic!("Model must remain a picker");
        };
        assert_eq!(models.items.len(), 1);

        // Changing the destination through keyboard dispatch keeps one
        // outstanding request and leaves the latest target in the form.
        for _ in 0..3 {
            d.palette_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
        }
        d.palette_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let Page::Form { fields, active, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("Agent creation should remain a form");
        };
        assert_eq!(fields[*active].label, "Workspace");
        assert_eq!(fields[*active].value, "demo/other");
        assert!(d.drain_one_request().is_none());
        assert_eq!(d.model_query.as_ref().unwrap().0, first_id);

        // Cancel and reopen preserve the in-flight lookup; keyboard selection
        // of its latest target still coalesces behind it.
        d.palette = None;
        d.open_create_terminal();
        d.palette_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        d.palette_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        d.palette_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert!(d.drain_one_request().is_none());

        d.agent_models_event(
            first_id,
            crate::protocol::AgentModelCapabilities {
                preset: "claude".into(),
                binding: Some("root-binding".into()),
                models: vec![crate::protocol::AgentModelOption {
                    alias: "opus".into(),
                    resolved: "claude-opus".into(),
                }],
                reason: None,
            },
        );
        let second = d
            .drain_one_request()
            .expect("latest destination is queried next");
        let second_id = second.request_id;
        assert!(matches!(
            second.request,
            Request::QueryAgentModels { project, workspace, .. }
                if project == "demo" && workspace == "other"
        ));
        assert_ne!(second_id, first_id);
        assert_eq!(d.palette.as_ref().unwrap().model_capabilities, None);

        d.agent_models_event(
            second_id,
            crate::protocol::AgentModelCapabilities {
                preset: "claude".into(),
                binding: Some("other-binding".into()),
                models: vec![crate::protocol::AgentModelOption {
                    alias: "opus".into(),
                    resolved: "claude-opus".into(),
                }],
                reason: None,
            },
        );
        assert_eq!(
            d.palette
                .as_ref()
                .unwrap()
                .model_capabilities
                .as_ref()
                .unwrap()
                .binding,
            Some("other-binding".into())
        );
    }

    #[test]
    fn refused_explicit_model_launch_does_not_remember_the_choice() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "claude".into(),
            "--".into(),
            "/fixture/claude".into(),
            "--dangerously-skip-permissions".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "claude".into(),
            argv: argv.clone(),
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent {
                preset: "claude".into(),
                model_policy: AgentModelPolicy::HarnessDefault,
            },
        );
        d.open_create_terminal();
        let query = d.drain_one_request().unwrap();
        let Request::QueryAgentModels { .. } = query.request else {
            panic!("expected the selected Claude model query");
        };
        d.agent_models_event(
            query.request_id,
            crate::protocol::AgentModelCapabilities {
                preset: "claude".into(),
                binding: Some("launch-binding".into()),
                models: vec![crate::protocol::AgentModelOption {
                    alias: "opus".into(),
                    resolved: "claude-opus-v1".into(),
                }],
                reason: None,
            },
        );
        set(&mut d, "Start", "Agent");
        set(&mut d, "Agent", "claude");
        set(&mut d, "Model", "opus");
        let launch = request(&mut d);
        let Request::CreateAgentSession(launch_request) = &launch.request else {
            panic!("explicit Agent launch must keep its model policy");
        };
        assert_eq!(
            launch_request.model_policy,
            AgentModelPolicy::Explicit("opus".into())
        );
        assert_eq!(
            launch_request.expected_model.as_deref(),
            Some("claude-opus-v1")
        );
        assert_eq!(
            launch_request.model_binding.as_deref(),
            Some("launch-binding")
        );

        d.handle_server_message(ServerMessage::Event(ServerEvent::LifecycleCompleted {
            client_token: launch.request_id,
            op: crate::protocol::LifecycleOp::CreateSession,
            outcome: crate::protocol::LifecycleOutcome::Failed {
                code: crate::protocol::ErrorCode::InvalidRequest,
                message: "Claude selected model resolution changed".into(),
            },
            target_workspace: None,
        }));
        assert!(
            d.drain_outbox()
                .iter()
                .all(|message| { !matches!(message.request, Request::SetSetting { .. }) })
        );
        assert_eq!(
            d.settings.launch_choices.get("demo"),
            Some(&LaunchChoice::Agent {
                preset: "claude".into(),
                model_policy: AgentModelPolicy::HarnessDefault,
            })
        );
    }

    #[test]
    fn workspace_submits_one_selected_launch_or_empty() {
        for start in ["Terminal", "Agent", "Nothing yet"] {
            let mut d = dashboard();
            start_workspace(&mut d, start);
            if start == "Agent" {
                set(&mut d, "Agent", "fixture-agent");
            }
            let m = request(&mut d);
            let Request::CreateWorkspaceWithLaunch {
                project,
                id: name,
                launch,
                ..
            } = m.request
            else {
                panic!()
            };
            assert_eq!((project.as_str(), name.as_str()), ("demo", "new-work"));
            if start == "Terminal" {
                let launch = launch.unwrap();
                assert!(!launch.argv.is_empty());
                assert_eq!(launch.label, None);
            } else {
                assert!(launch.is_none());
            }
            if start == "Agent" {
                d.handle_server_message(ServerMessage::Response {
                    request_id: m.request_id,
                    response: Response::Ok,
                });
                assert!(
                    d.provisional
                        .as_ref()
                        .is_some_and(|row| !row.then.is_empty())
                );
                let follow_up = d.handle_server_message(ServerMessage::Event(
                    ServerEvent::LifecycleCompleted {
                        client_token: m.request_id,
                        op: crate::protocol::LifecycleOp::CreateWorkspaceWithLaunch,
                        outcome: crate::protocol::LifecycleOutcome::Succeeded,
                        target_workspace: Some(crate::protocol::WorkspaceLifecycleIdentity {
                            project: project.clone(),
                            id: name.clone(),
                        }),
                    },
                ));
                let allocation = follow_up
                    .into_iter()
                    .find(|message| message.request == Request::AllocateInitialPromptIntent)
                    .unwrap();
                assert_eq!(allocation.request, Request::AllocateInitialPromptIntent);
                let launch = d.handle_server_message(ServerMessage::Response {
                    request_id: allocation.request_id,
                    response: Response::InitialPromptIntent(
                        crate::protocol::InitialPromptIntentId {
                            server_lifetime: "123e4567-e89b-12d3-a456-426614174000".into(),
                            sequence: 1,
                        },
                    ),
                });
                let Request::CreateAgentSessionWithPrompt(request) = &launch[0].request else {
                    panic!("workspace agent follows registration through one-use intent");
                };
                assert_eq!(request.launch.session.project, "demo");
                assert_eq!(request.launch.session.workspace, "new-work");
                assert_eq!(request.launch.session.name, "");
                assert_eq!(request.prompt, "");
            }
            assert!(
                d.drain_outbox().is_empty(),
                "creation sends no second shell request"
            );
        }
    }
    /// Launch from the palette and answer with a created session; returns
    /// what the Dashboard sends next.
    fn launch(d: &mut Dashboard, start: &str) -> Vec<ClientMessage> {
        d.open_create_terminal();
        set(d, "Start", start);
        if start == "Agent" {
            set(d, "Agent", "fixture-agent");
        }
        let m = request(d);
        let mut session = summary(1);
        session.project = d.hierarchy.projects[0].name.clone();
        d.handle_server_message(ServerMessage::Response {
            request_id: m.request_id,
            response: Response::CreatedSession(Box::new(session)),
        })
    }

    /// Split the Dashboard's requests into settings saves and the rest.
    fn saves(messages: Vec<ClientMessage>) -> (Vec<ClientMessage>, Vec<ClientMessage>) {
        messages
            .into_iter()
            .partition(|m| matches!(m.request, Request::SetSetting { .. }))
    }

    fn rendered_settings_notice(d: &Dashboard) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(240, 30)).unwrap();
        terminal
            .draw(|frame| super::super::render::draw_dashboard_at(frame, d, 0))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    fn refuse(d: &mut Dashboard, save: &ClientMessage, message: &str) -> Vec<ClientMessage> {
        d.handle_server_message(ServerMessage::Response {
            request_id: save.request_id,
            response: Response::Error {
                code: ErrorCode::InvalidRequest,
                message: message.into(),
            },
        })
    }

    /// The Dashboard writes no file: the Server merges the remembered choice
    /// into the project's own entry (see `ovrcr-runtime`'s writer tests).
    #[test]
    fn successful_launch_asks_the_server_to_remember_the_choice() {
        for start in ["Terminal", "Agent"] {
            let mut d = dashboard();
            let project = "project.with-dots_and-punctuation";
            d.hierarchy.projects[0].name = project.into();
            d.hierarchy.projects[0].workspaces[0].project = project.into();
            let other = agent_choice("other");
            d.settings
                .launch_choices
                .insert("other.project".into(), other.clone());
            let sent = launch(&mut d, start);
            assert!(
                matches!(
                    sent.first().map(|m| &m.request),
                    Some(Request::SetView { .. })
                ),
                "the new pane's view goes before the save: {sent:?}"
            );
            let (saves, _) = saves(sent);
            let expected = if start == "Agent" {
                agent_choice("fixture-agent")
            } else {
                LaunchChoice::Terminal
            };
            assert_eq!(saves.len(), 1, "{start}");
            assert_eq!(
                saves[0].request,
                super::super::settings::launch_choice_request(project, &expected)
            );
            assert_eq!(d.settings.launch_choices.get(project), Some(&expected));
            assert_eq!(d.settings.launch_choices["other.project"], other);
            assert_eq!(d.focused_session(), Some(SessionId(1)));
            assert!(d.palette.is_none());
            assert_eq!(d.mode, InputMode::Terminal);
            assert!(d.error.is_none());
        }
    }

    #[test]
    fn optional_name_custom_command_and_success_only_persistence() {
        let mut d = dashboard();
        d.open_create_terminal();
        set(&mut d, "Command", "printf secret-one-off");
        let m = request(&mut d);
        let Request::CreateSession(s) = &m.request else {
            panic!()
        };
        assert!(s.name.is_empty());
        assert_eq!(
            s.argv,
            vec![
                OsString::from("/bin/sh"),
                "-lc".into(),
                "printf secret-one-off".into()
            ]
        );
        let (saves_after_failure, _) = saves(d.handle_server_message(ServerMessage::Response {
            request_id: m.request_id,
            response: Response::Error {
                code: ErrorCode::Internal,
                message: "launch failed".into(),
            },
        }));
        assert!(d.settings.launch_choices.is_empty());
        assert!(saves_after_failure.is_empty());
        let m = request(&mut d);
        let (saves, _) = saves(d.handle_server_message(ServerMessage::Response {
            request_id: m.request_id,
            response: Response::CreatedSession(Box::new(summary(1))),
        }));
        assert_eq!(saves.len(), 1);
        assert_eq!(
            saves[0].request,
            Request::SetSetting {
                path: "launch_choices.demo".into(),
                value: Some("{ kind = \"Terminal\" }".into()),
            }
        );
    }
    #[test]
    fn successful_agent_choice_survives_restart_and_empty_workspace_leaves_it_intact() {
        let mut d = dashboard();
        let (saves, _) = saves(launch(&mut d, "Agent"));
        let saved = agent_choice("fixture-agent");
        assert_eq!(
            saves[0].request,
            super::super::settings::launch_choice_request("demo", &saved)
        );
        // A restarted Dashboard learns the choice from the Server's reading.
        let mut restarted = dashboard();
        restarted
            .settings
            .launch_choices
            .insert("demo".into(), saved);
        restarted.open_create_terminal();
        let Page::Form { fields, .. } = &restarted.palette.as_ref().unwrap().page else {
            panic!()
        };
        assert_eq!(
            fields.iter().find(|f| f.label == "Start").unwrap().value,
            "Agent"
        );
        assert_eq!(
            fields.iter().find(|f| f.label == "Agent").unwrap().value,
            "fixture-agent"
        );
        start_workspace(&mut restarted, "Nothing yet");
        let m = request(&mut restarted);
        restarted.handle_server_message(ServerMessage::Response {
            request_id: m.request_id,
            response: Response::Ok,
        });
        assert_eq!(
            restarted.settings.launch_choices.get("demo"),
            Some(&agent_choice("fixture-agent"))
        );
    }

    fn acknowledge_settings_views(d: &mut Dashboard, requests: Vec<ClientMessage>) {
        let mut requests = std::collections::VecDeque::from(requests);
        for _ in 0..8 {
            let Some(message) = requests.pop_front() else {
                return;
            };
            let Request::SetView { view } = message.request else {
                panic!("unexpected request");
            };
            for pane in view.panes {
                d.handle_server_message(ServerMessage::Response {
                    request_id: message.request_id,
                    response: Response::Screen {
                        session: pane.session,
                        run: pane.run,
                        revision: view.revision,
                        size: pane.size,
                        bytes: b"USABLE".to_vec(),
                    },
                });
            }
            requests.extend(d.handle_server_message(ServerMessage::Response {
                request_id: message.request_id,
                response: Response::Ok,
            }));
        }
        assert!(requests.is_empty(), "view acknowledgement must converge");
    }

    const REFUSED: &str = "could not save launch_choices.demo: settings document /x is read-only";

    #[test]
    fn launch_save_warning_survives_selection_ack_and_session_accepts_input() {
        for coalesced in [false, true] {
            let mut d = dashboard();
            let mut hierarchy = d.hierarchy.clone();
            hierarchy.projects[0].workspaces[0]
                .sessions
                .push(summary(10));
            let initial = d.handle_server_message(ServerMessage::Event(
                ServerEvent::HierarchyChanged(hierarchy),
            ));
            acknowledge_settings_views(&mut d, initial);
            let pending = if coalesced {
                d.handle_server_message(ServerMessage::Event(ServerEvent::ScreenDirty {
                    session: SessionId(10),
                    run: crate::protocol::SessionRunId(1),
                    revision: d.view_revision(),
                }))
            } else {
                Vec::new()
            };
            d.open_create_terminal();
            let launch = request(&mut d);
            let mut hierarchy = d.hierarchy.clone();
            hierarchy.projects[0].workspaces[0]
                .sessions
                .push(summary(1));
            let refresh = d.handle_server_message(ServerMessage::Event(
                ServerEvent::HierarchyChanged(hierarchy),
            ));
            assert!(refresh.is_empty());
            let (saves, selected) = saves(d.handle_server_message(ServerMessage::Response {
                request_id: launch.request_id,
                response: Response::CreatedSession(Box::new(summary(1))),
            }));
            assert!(refuse(&mut d, &saves[0], REFUSED).is_empty());
            assert!(rendered_settings_notice(&d).contains(REFUSED));
            acknowledge_settings_views(&mut d, pending.into_iter().chain(selected).collect());
            assert!(
                rendered_settings_notice(&d).contains(REFUSED),
                "coalesced={coalesced}"
            );
            assert_eq!(d.focused_session(), Some(SessionId(1)));
            assert!(d.palette.is_none());
            assert_eq!(
                d.key(KeyCode::Char('x')),
                DashboardAction::PtyBytes(b"x".to_vec())
            );
        }
    }

    #[test]
    fn saved_launch_choice_leaves_the_new_session_accepting_input() {
        for save_first in [false, true] {
            let mut d = dashboard();
            d.open_create_terminal();
            let launch = request(&mut d);
            let mut hierarchy = d.hierarchy.clone();
            hierarchy.projects[0].workspaces[0]
                .sessions
                .push(summary(1));
            d.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
                hierarchy,
            )));
            let (saves, views) = saves(d.handle_server_message(ServerMessage::Response {
                request_id: launch.request_id,
                response: Response::CreatedSession(Box::new(summary(1))),
            }));
            let ok = |d: &mut Dashboard| {
                d.handle_server_message(ServerMessage::Response {
                    request_id: saves[0].request_id,
                    response: Response::Ok,
                })
            };
            let mut views = views;
            if save_first {
                views.extend(ok(&mut d));
            }
            acknowledge_settings_views(&mut d, views);
            if !save_first {
                let after = ok(&mut d);
                acknowledge_settings_views(&mut d, after);
            }
            assert_eq!(d.focused_session(), Some(SessionId(1)));
            assert_eq!(
                d.key(KeyCode::Char('x')),
                DashboardAction::PtyBytes(b"x".to_vec()),
                "save_first={save_first}"
            );
        }
    }

    #[test]
    fn settings_save_failure_does_not_reopen_launch_or_lose_created_session() {
        let mut d = dashboard();
        let (saves, _) = saves(launch(&mut d, "Terminal"));
        refuse(&mut d, &saves[0], REFUSED);
        assert!(d.palette.is_none());
        assert_eq!(d.focused_session(), Some(SessionId(1)));
        assert_eq!(d.mode, InputMode::Terminal);
        assert!(
            d.error
                .as_deref()
                .is_some_and(|error| error.ends_with(REFUSED))
        );
        assert!(
            d.drain_outbox()
                .iter()
                .all(|message| !matches!(message.request, Request::CreateSession(_)))
        );
    }

    #[test]
    fn unavailable_remembered_agent_requires_explicit_replacement() {
        let mut d = dashboard();
        d.settings
            .launch_choices
            .insert("demo".into(), agent_choice("missing-fixture-agent"));
        start_workspace(&mut d, "Agent");
        assert!(matches!(submit(&mut d), DashboardAction::Redraw));
        assert!(d.drain_outbox().is_empty());
        // The refusal sits on the Agent field, which takes the focus.
        let palette = d.palette.as_ref().unwrap();
        let Page::Form { fields, active, .. } = &palette.page else {
            panic!()
        };
        assert_eq!(fields[*active].label, "Agent");
        let (at, problem) = palette.invalid.as_ref().unwrap();
        assert_eq!(*at, *active);
        assert!(problem.contains("unavailable"), "{problem}");
        set(&mut d, "Start", "Terminal");
        assert!(matches!(
            request(&mut d).request,
            Request::CreateWorkspaceWithLaunch {
                launch: Some(SessionLaunch { label: None, .. }),
                ..
            }
        ));
    }
    #[test]
    fn partial_failure_requires_registered_workspace_and_recovers_without_recreation() {
        for recovery in ["Retry", "Choose another agent", "Open shell"] {
            let mut d = dashboard();
            start_workspace(&mut d, "Agent");
            set(&mut d, "Agent", "fixture-agent");
            let m = request(&mut d);
            d.handle_server_message(ServerMessage::Response {
                request_id: m.request_id,
                response: Response::Error {
                    code: ErrorCode::PartialFailure,
                    message: "workspace created; launch failed".into(),
                },
            });
            set(&mut d, "Recovery", recovery);
            assert!(matches!(submit(&mut d), DashboardAction::Redraw));
            assert!(
                d.palette
                    .as_ref()
                    .unwrap()
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("registered")
            );
            d.hierarchy.projects[0].workspaces.push(WorkspaceSummary {
                project: "demo".into(),
                name: "new-work".into(),
                id: "new-work".into(),
                root: false,
                warning: None,
                path: "/tmp/unused".into(),
                sessions: vec![],
            });
            if recovery == "Choose another agent" {
                assert!(matches!(submit(&mut d), DashboardAction::Redraw));
                set(&mut d, "Agent", "fixture-agent");
            }
            let m = request(&mut d);
            let s = match m.request {
                Request::CreateAgentSession(launch) => launch.session,
                Request::CreateSession(session) if recovery == "Open shell" => session,
                request => panic!("recovery must never recreate the workspace: {request:?}"),
            };
            assert_eq!(s.workspace, "new-work");
            assert_eq!(
                s.label.as_deref(),
                (recovery != "Open shell").then_some("fixture-agent")
            );
        }
    }
    #[test]
    fn title_events_render_duplicates_and_rename_clear_targets_stable_id() {
        let mut d = dashboard();
        let mut one = summary(1);
        one.title = Some("Working".into());
        let mut two = summary(2);
        two.title = Some("Working".into());
        d.hierarchy.projects[0].workspaces[0].sessions = vec![one.clone(), two];
        assert_eq!(d.session_display_name(&one), "Working (#1)");
        d.palette = Some(Palette {
            page: d.palette_form(Command::RenameTerminal(one.id)),
            ..Palette::new()
        });
        set(&mut d, "Name", "Pinned");
        let m = request(&mut d);
        assert_eq!(
            m.request,
            Request::SetSessionTitle {
                session: SessionId(1),
                title: Some("Pinned".into())
            }
        );
        d.handle_server_message(ServerMessage::Response {
            request_id: m.request_id,
            response: Response::Ok,
        });
        d.palette = Some(Palette {
            page: d.palette_form(Command::RenameTerminal(one.id)),
            ..Palette::new()
        });
        d.palette_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(
            request(&mut d).request,
            Request::SetSessionTitle {
                session: SessionId(1),
                title: None
            }
        );
        one.phase = SessionPhase::Exited {
            code: Some(0),
            signal: None,
        };
        d.hierarchy.projects[0].workspaces[0].sessions[0] = one;
        d.palette = Some(Palette {
            page: d.command_page(Command::ReopenTerminal(SessionId(1))),
            ..Palette::new()
        });
        assert_eq!(
            request(&mut d).request,
            Request::ReopenSession {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(1),
                acknowledge_stopped: false,
            }
        );
        assert_eq!(d.hierarchy.projects[0].workspaces[0].sessions.len(), 2);
    }

    #[test]
    fn reopen_and_acknowledge_confirm_previous_process_termination() {
        let mut d = dashboard();
        let mut stopped = summary(1);
        stopped.phase = SessionPhase::Stopped;
        stopped.recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: None,
        });
        d.hierarchy.projects[0].workspaces[0].sessions = vec![stopped];
        d.palette = Some(Palette {
            page: d.command_page(Command::ReopenTerminal(SessionId(1))),
            ..Palette::new()
        });
        let Page::Confirm { request, target } = &d.palette.as_ref().unwrap().page else {
            panic!("reopen must confirm");
        };
        assert!(
            target.contains("Previous agent or background processes"),
            "{target}"
        );
        assert_eq!(
            request.as_deref(),
            Some(&Request::ReopenSession {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(1),
                acknowledge_stopped: true,
            })
        );
        d.palette = Some(Palette {
            page: d.command_page(Command::AcknowledgeStopped(
                SessionId(1),
                crate::protocol::SessionRunId(1),
            )),
            ..Palette::new()
        });
        let Page::Confirm { request, target } = &d.palette.as_ref().unwrap().page else {
            panic!("acknowledge must confirm");
        };
        assert!(target.contains("without launching"), "{target}");
        assert_eq!(
            request.as_deref(),
            Some(&Request::AcknowledgeSessionStopped {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(1),
            })
        );
        d.hierarchy.projects[0].workspaces[0].sessions[0].kind = SessionKind::Agent {
            name: "codex".into(),
        };
        d.palette = Some(Palette {
            page: d.command_page(Command::AgentResumeUnavailable(SessionId(1))),
            ..Palette::new()
        });
        assert!(matches!(
            d.palette.as_ref().unwrap().page,
            Page::Confirm { request: None, .. }
        ));
        assert_eq!(submit(&mut d), DashboardAction::Redraw);
    }

    #[test]
    fn unavailable_resume_shows_the_retained_reason_without_launching() {
        let mut d = dashboard();
        let mut row = summary(1);
        row.kind = SessionKind::Agent {
            name: "grok".into(),
        };
        row.phase = SessionPhase::Stopped;
        let reason = "No certified grok conversation; use managed launch and configured reporting";
        row.recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: Some(reason.into()),
            failure: None,
        });
        d.hierarchy.projects[0].workspaces[0].sessions = vec![row];
        d.select_session(SessionId(1));
        d.palette = Some(Palette {
            page: d.command_page(Command::AgentResumeUnavailable(SessionId(1))),
            ..Palette::new()
        });
        let Page::Confirm { request, target } = &d.palette.as_ref().unwrap().page else {
            panic!("expected explanation");
        };
        assert!(request.is_none());
        assert!(target.contains(reason), "{target}");
        assert_eq!(submit(&mut d), DashboardAction::Redraw);
    }

    #[test]
    fn unavailable_picker_provider_offers_the_native_menu() {
        let mut d = dashboard();
        let mut row = summary(1);
        row.kind = SessionKind::Agent { name: "omp".into() };
        row.phase = SessionPhase::Stopped;
        row.recovery = Some(SessionRecovery {
            conversation: Some("01a1b9f5-74db-7565-8aca-426afe9cf198".into()),
            attached: false,
            requires_ack: true,
            unavailable: Some(
                "provider conversation changed through an unsupported identity transition".into(),
            ),
            failure: Some(
                "provider history identity does not match the recorded conversation".into(),
            ),
        });
        d.hierarchy.projects[0].workspaces[0].sessions = vec![row];
        d.select_session(SessionId(1));
        let entries = d.palette_entries("", None, None, None, false);
        assert!(
            entries
                .iter()
                .any(|entry| entry.label == "Open native resume picker"),
            "{:?}",
            entries
                .iter()
                .map(|entry| entry.label.as_str())
                .collect::<Vec<_>>()
        );
        d.palette = None;
        let Page::Confirm { request, target } =
            d.command_page(Command::ReopenTerminal(SessionId(1)))
        else {
            panic!("expected the picker confirmation");
        };
        assert!(target.contains("cannot be certified"), "{target}");
        assert!(target.contains("native resume picker"), "{target}");
        assert_eq!(
            request.as_deref(),
            Some(&Request::ReopenSession {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(1),
                acknowledge_stopped: true,
            })
        );
    }

    #[test]
    fn missing_conversation_confirmation_offers_native_picker() {
        for requires_ack in [false, true] {
            let mut d = dashboard();
            let mut row = summary(1);
            row.kind = SessionKind::Agent {
                name: "claude".into(),
            };
            row.phase = SessionPhase::Stopped;
            row.recovery = Some(SessionRecovery {
                conversation: None,
                attached: false,
                requires_ack,
                unavailable: None,
                failure: None,
            });
            d.hierarchy.projects[0].workspaces[0].sessions = vec![row];
            let Page::Confirm { request, target } =
                d.command_page(Command::ReopenTerminal(SessionId(1)))
            else {
                panic!("expected confirmation");
            };
            assert!(target.contains("resume picker"), "{target}");
            assert_eq!(
                request.as_deref(),
                Some(&Request::ReopenSession {
                    session: SessionId(1),
                    expected_run: crate::protocol::SessionRunId(1),
                    acknowledge_stopped: requires_ack,
                })
            );
        }
    }

    #[test]
    fn enter_on_an_exited_agent_opens_the_resume_confirmation() {
        let mut d = dashboard();
        let mut row = summary(1);
        row.kind = SessionKind::Agent { name: "pi".into() };
        row.phase = SessionPhase::Exited {
            code: None,
            signal: None,
        };
        row.recovery = Some(SessionRecovery {
            conversation: Some("retained".into()),
            attached: false,
            requires_ack: false,
            unavailable: None,
            failure: Some("Agent exited; press Enter to resume the conversation".into()),
        });
        d.hierarchy.projects[0].workspaces[0].sessions = vec![row];
        d.select_session(SessionId(1));
        let enter = d
            .key_binding_for(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .expect("Enter is bound");
        assert_eq!(enter.name, "Retry resume conversation");
        assert_eq!(enter.reason, None);
        assert_eq!(d.run(Action::Reopen), DashboardAction::Redraw);
        assert_eq!(
            request(&mut d).request,
            Request::ReopenSession {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(1),
                acknowledge_stopped: false,
            }
        );

        // A picker-capable provider opens its own menu even when the exact
        // reference is unavailable, instead of leaving the row a dead end.
        d.hierarchy.projects[0].workspaces[0].sessions[0].recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: false,
            unavailable: Some("no certified conversation".into()),
            failure: None,
        });
        d.palette = None;
        let enter = d
            .key_binding_for(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .expect("Enter is bound");
        assert_eq!(enter.name, "Open native resume picker");
        assert_eq!(d.run(Action::Reopen), DashboardAction::Redraw);
        match d.palette.as_ref().map(|palette| &palette.page) {
            Some(Page::Confirm {
                request: Some(request),
                target,
            }) => {
                assert!(target.contains("native resume picker"), "{target}");
                assert_eq!(
                    request.as_ref(),
                    &Request::ReopenSession {
                        session: SessionId(1),
                        expected_run: crate::protocol::SessionRunId(1),
                        acknowledge_stopped: false,
                    }
                );
            }
            _ => panic!("expected the native resume picker confirmation"),
        }

        // A provider with no native picker keeps the explanation.
        d.hierarchy.projects[0].workspaces[0].sessions[0].kind = SessionKind::Agent {
            name: "grok".into(),
        };
        d.palette = None;
        let enter = d
            .key_binding_for(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .expect("Enter is bound");
        assert_eq!(enter.name, "Resume unavailable");
        assert_eq!(d.run(Action::Reopen), DashboardAction::Redraw);
        match d.palette.as_ref().map(|palette| &palette.page) {
            Some(Page::Confirm {
                request: None,
                target,
            }) => assert!(target.contains("no certified conversation"), "{target}"),
            _ => panic!("expected an unavailable notice"),
        }
    }

    #[test]
    fn failed_automatic_resume_exposes_explicit_retry_without_acknowledgement() {
        let mut d = dashboard();
        let mut row = summary(1);
        row.kind = SessionKind::Agent {
            name: "claude".into(),
        };
        row.phase = SessionPhase::Interrupted;
        row.recovery = Some(SessionRecovery {
            conversation: Some("retained".into()),
            attached: false,
            requires_ack: false,
            unavailable: None,
            failure: Some("live capacity exhausted".into()),
        });
        d.hierarchy.projects[0].workspaces[0].sessions = vec![row];
        d.select_session(SessionId(1));
        assert!(
            d.palette_entries("", None, None, None, false)
                .iter()
                .any(|entry| entry.label == "Retry resume conversation")
        );
        d.palette = Some(Palette {
            page: d.command_page(Command::ReopenTerminal(SessionId(1))),
            ..Palette::new()
        });
        assert_eq!(
            request(&mut d).request,
            Request::ReopenSession {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(1),
                acknowledge_stopped: false,
            }
        );
    }

    #[test]
    fn failed_reopen_retries_the_current_run() {
        let mut d = dashboard();
        let mut stopped = summary(1);
        stopped.phase = SessionPhase::Exited {
            code: Some(0),
            signal: None,
        };
        d.hierarchy.projects[0].workspaces[0].sessions = vec![stopped];
        d.palette = Some(Palette {
            page: d.command_page(Command::ReopenTerminal(SessionId(1))),
            ..Palette::new()
        });
        let first = request(&mut d);
        assert_eq!(
            first.request,
            Request::ReopenSession {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(1),
                acknowledge_stopped: false,
            }
        );
        let mut failed = summary(1);
        failed.run = crate::protocol::SessionRunId(2);
        failed.phase = SessionPhase::Stopped;
        failed.recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: false,
            unavailable: None,
            failure: Some("cwd missing".into()),
        });
        d.hierarchy.projects[0].workspaces[0].sessions[0] = failed;
        d.handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::Error {
                code: ErrorCode::Internal,
                message: "reopen failed".into(),
            },
        });
        assert_eq!(
            request(&mut d).request,
            Request::ReopenSession {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(2),
                acknowledge_stopped: false,
            }
        );
    }

    #[test]
    fn delayed_reopen_response_preserves_the_current_error() {
        let mut d = dashboard();
        let mut original = summary(1);
        original.phase = SessionPhase::Exited {
            code: Some(0),
            signal: None,
        };
        d.hierarchy.projects[0].workspaces[0].sessions = vec![original];
        d.palette = Some(Palette {
            page: d.command_page(Command::ReopenTerminal(SessionId(1))),
            ..Palette::new()
        });
        let pending = request(&mut d);
        let mut current = summary(1);
        current.run = crate::protocol::SessionRunId(3);
        d.hierarchy.projects[0].workspaces[0].sessions[0] = current;
        d.set_error("current operation failed");
        let mut delayed = summary(1);
        delayed.run = crate::protocol::SessionRunId(2);
        d.handle_server_message(ServerMessage::Response {
            request_id: pending.request_id,
            response: Response::CreatedSession(Box::new(delayed)),
        });
        assert_eq!(d.error.as_deref(), Some("current operation failed"));
    }

    #[test]
    fn ownership_uncertain_create_offers_the_existing_row_not_another_create() {
        let mut d = dashboard();
        d.open_create_terminal();
        let launched = request(&mut d);
        assert!(matches!(launched.request, Request::CreateSession(_)));
        let mut retained = summary(7);
        retained.phase = SessionPhase::Stopped;
        retained.recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: Some("spawn uncertain".into()),
        });
        d.hierarchy.projects[0].workspaces[0].sessions = vec![retained];
        d.handle_server_message(ServerMessage::Response {
            request_id: launched.request_id,
            response: Response::Error {
                code: ErrorCode::OwnershipUncertain,
                message: "process ownership is uncertain".into(),
            },
        });
        assert_eq!(d.focused_session(), None);
        submit(&mut d);
        if let Some(message) = d.drain_one_request() {
            match message.request {
                Request::CreateSession(_) | Request::AcknowledgeSessionStopped { .. } => {
                    panic!("uncertain create must not replay creation or acknowledge a guessed row")
                }
                other => panic!("unexpected request {other:?}"),
            }
        }
        d.select_session(SessionId(7));
        d.palette = Some(Palette {
            page: d.command_page(Command::AcknowledgeStopped(
                SessionId(7),
                crate::protocol::SessionRunId(1),
            )),
            ..Palette::new()
        });
        assert_eq!(
            request(&mut d).request,
            Request::AcknowledgeSessionStopped {
                session: SessionId(7),
                expected_run: crate::protocol::SessionRunId(1),
            }
        );
    }

    #[test]
    fn known_create_failure_still_retries_the_create_form() {
        let mut d = dashboard();
        d.open_create_terminal();
        let launched = request(&mut d);
        d.handle_server_message(ServerMessage::Response {
            request_id: launched.request_id,
            response: Response::Error {
                code: ErrorCode::Internal,
                message: "executable not found".into(),
            },
        });
        assert!(matches!(request(&mut d).request, Request::CreateSession(_)));
    }

    #[test]
    fn recover_launch_retry_does_not_acknowledge_stopped() {
        let mut d = dashboard();
        start_workspace(&mut d, "Agent");
        set(&mut d, "Agent", "fixture-agent");
        let launched = request(&mut d);
        d.handle_server_message(ServerMessage::Response {
            request_id: launched.request_id,
            response: Response::Error {
                code: ErrorCode::PartialFailure,
                message: "workspace created; agent failed".into(),
            },
        });
        d.hierarchy.projects[0].workspaces.push(WorkspaceSummary {
            project: "demo".into(),
            name: "new-work".into(),
            id: "new-work".into(),
            root: false,
            warning: None,
            path: "/tmp/unused".into(),
            sessions: vec![],
        });
        set(&mut d, "Recovery", "Retry");
        assert!(
            matches!(request(&mut d).request, Request::CreateAgentSession(_)),
            "Retry recovers by creating in the existing workspace, without stopped-process acknowledgement"
        );
    }

    #[test]
    fn ownership_uncertain_create_does_not_guess_among_two_rows() {
        let mut d = dashboard();
        let mut first = summary(1);
        first.phase = SessionPhase::Stopped;
        first.recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: Some("spawn uncertain".into()),
        });
        let mut second = summary(2);
        second.phase = SessionPhase::Stopped;
        second.recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: Some("spawn uncertain".into()),
        });
        d.hierarchy.projects[0].workspaces[0].sessions = vec![first, second];
        d.select_session(SessionId(1));
        d.open_create_terminal();
        let launched = request(&mut d);
        assert!(matches!(launched.request, Request::CreateSession(_)));
        d.handle_server_message(ServerMessage::Response {
            request_id: launched.request_id,
            response: Response::Error {
                code: ErrorCode::OwnershipUncertain,
                message: "process ownership is uncertain".into(),
            },
        });
        assert_eq!(d.focused_session(), Some(SessionId(1)));
        submit(&mut d);
        if let Some(message) = d.drain_one_request() {
            match message.request {
                Request::CreateSession(_) | Request::AcknowledgeSessionStopped { .. } => {
                    panic!("uncertain create must not replay creation or acknowledge a guessed row")
                }
                other => panic!("unexpected request {other:?}"),
            }
        }
    }

    #[test]
    fn ownership_uncertain_create_does_not_parse_session_id_from_error_text() {
        let mut d = dashboard();
        let mut first = summary(7);
        first.phase = SessionPhase::Stopped;
        first.recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: Some("spawn uncertain".into()),
        });
        let mut bait = summary(42);
        bait.phase = SessionPhase::Stopped;
        bait.recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: Some("spawn uncertain".into()),
        });
        d.hierarchy.projects[0].workspaces[0].sessions = vec![first, bait];
        d.open_create_terminal();
        let launched = request(&mut d);
        assert!(matches!(launched.request, Request::CreateSession(_)));
        d.handle_server_message(ServerMessage::Response {
            request_id: launched.request_id,
            response: Response::Error {
                code: ErrorCode::OwnershipUncertain,
                message: "process ownership is uncertain for session 42 run 7".into(),
            },
        });
        assert_eq!(
            d.focused_session(),
            None,
            "error text must not select the bait session, and inventory order must not guess a row"
        );
        let palette = d.palette.as_ref().expect("uncertain notice stays open");
        assert!(
            palette.failed_launch.is_none(),
            "uncertain create must not keep a recoverable launch"
        );
        assert!(
            matches!(palette.page, Page::Confirm { request: None, .. }),
            "uncertain create must keep a request-less notice"
        );
        submit(&mut d);
        if let Some(message) = d.drain_one_request() {
            match message.request {
                Request::CreateSession(_)
                | Request::AcknowledgeSessionStopped { .. }
                | Request::ReopenSession { .. } => {
                    panic!(
                        "uncertain create must not parse error text to create, ack, or reopen a row"
                    )
                }
                other => panic!("unexpected request {other:?}"),
            }
        }
    }

    #[test]
    fn reopen_confirm_keeps_captured_run_when_hierarchy_advances() {
        let mut d = dashboard();
        let mut stopped = summary(1);
        stopped.phase = SessionPhase::Exited {
            code: Some(0),
            signal: None,
        };
        d.hierarchy.projects[0].workspaces[0].sessions = vec![stopped];
        d.palette = Some(Palette {
            page: d.command_page(Command::ReopenTerminal(SessionId(1))),
            ..Palette::new()
        });
        let mut advanced = d.hierarchy.clone();
        advanced.projects[0].workspaces[0].sessions[0].run = crate::protocol::SessionRunId(4);
        d.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
            advanced,
        )));
        let first = request(&mut d);
        assert_eq!(
            first.request,
            Request::ReopenSession {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(1),
                acknowledge_stopped: false,
            }
        );
        d.handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::Error {
                code: ErrorCode::Conflict,
                message: "run moved".into(),
            },
        });
        assert!(
            d.palette.is_some(),
            "Conflict rebuilds confirmation and waits for another Enter"
        );
        assert_eq!(
            request(&mut d).request,
            Request::ReopenSession {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(4),
                acknowledge_stopped: false,
            }
        );
    }

    #[test]
    fn close_confirm_does_not_submit_after_run_replacement() {
        let mut d = dashboard();
        d.hierarchy.projects[0].workspaces[0].sessions = vec![summary(1)];
        d.select_session(SessionId(1));
        d.open_close_terminal();
        assert!(matches!(
            &d.palette.as_ref().unwrap().page,
            Page::Confirm {
                request: Some(request),
                ..
            } if matches!(request.as_ref(), Request::CloseTerminal {
                session,
                expected_run,
            } if *session == SessionId(1) && *expected_run == crate::protocol::SessionRunId(1))
        ));
        let mut advanced = d.hierarchy.clone();
        advanced.projects[0].workspaces[0].sessions[0].run = crate::protocol::SessionRunId(4);
        d.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
            advanced,
        )));
        if d.palette.is_some() {
            submit(&mut d);
        }
        assert!(
            !matches!(
                { d.drain_one_request() },
                Some(ClientMessage {
                    request: Request::CloseTerminal { .. },
                    ..
                })
            ),
            "must not send CloseTerminal for the old confirmation without a new explicit confirm"
        );
    }

    #[test]
    fn operator_archive_palette_hides_legacy_rows_entirely() {
        let d = dashboard();
        let mut row = summary(9);
        row.archived = true;
        row.scope = ovrcr_protocol::SessionScope::OperatorConversation {
            conversation: ovrcr_protocol::OperatorConversationId(3),
        };
        let mut ordinary = summary(10);
        ordinary.archived = true;
        let entries = d.palette_entries("", Some(&[row, ordinary]), None, None, false);
        assert_eq!(
            entries.len(),
            2,
            "only the ordinary archived row is offered"
        );
        assert!(
            entries
                .iter()
                .all(|entry| !entry.label.contains("Operator"))
        );
        assert!(
            entries
                .iter()
                .any(|entry| matches!(entry.command, Command::Unarchive(SessionId(10), _)))
        );
    }

    #[test]
    fn archived_row_with_uncertain_ownership_offers_acknowledge_with_its_own_run() {
        let d = dashboard();
        let mut filed = summary(9);
        filed.archived = true;
        filed.phase = SessionPhase::Stopped;
        filed.run = crate::protocol::SessionRunId(4);
        filed.recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: None,
        });
        let mut certain = summary(10);
        certain.archived = true;
        certain.phase = SessionPhase::Stopped;
        let entries = d.palette_entries("", Some(&[filed, certain]), None, None, false);
        let acknowledge: Vec<_> = entries
            .iter()
            .filter(|entry| entry.label.starts_with("Acknowledge stopped: "))
            .collect();
        assert_eq!(
            acknowledge.len(),
            1,
            "only the uncertain row offers acknowledgement"
        );
        let Command::AcknowledgeStopped(id, run) = acknowledge[0].command.clone() else {
            panic!("acknowledge entry must carry the archived row's run");
        };
        assert_eq!((id, run), (SessionId(9), crate::protocol::SessionRunId(4)));
        assert!(
            find_session(&d, SessionId(9)).is_none(),
            "archived rows are not in the hierarchy; the page must not depend on it"
        );
        let Page::Confirm { request, .. } = d.command_page(Command::AcknowledgeStopped(id, run))
        else {
            panic!("acknowledge must confirm");
        };
        assert_eq!(
            request.as_deref(),
            Some(&Request::AcknowledgeSessionStopped {
                session: SessionId(9),
                expected_run: crate::protocol::SessionRunId(4),
            })
        );
    }

    #[test]
    fn new_agent_typed_branch_creates_workspace_then_allocates_private_prompt_intent() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "codex".into(),
            "--".into(),
            "/fixture/codex".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "codex".into(),
            argv: argv.clone(),
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent {
                preset: "codex".into(),
                model_policy: AgentModelPolicy::HarnessDefault,
            },
        );
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record("root", "/tmp/unused", "trunk")]),
        );
        {
            let palette = d.palette.as_mut().unwrap();
            palette.suggestions.worker = None;
            palette.suggestions.in_flight = None;
            palette.suggestions.project = Some("demo".into());
            palette.suggestions.cache.insert(
                "demo".into(),
                Ok(super::super::git_hints::Hints {
                    branches: vec!["main".into(), "develop".into()],
                    base: "main".into(),
                }),
            );
        }
        set(&mut d, "Name", "new branch agent");
        set(&mut d, "Initial prompt", "a private one-use prompt");
        let Page::Form { fields, active, .. } = &mut d.palette.as_mut().unwrap().page else {
            panic!("New agent session form")
        };
        *active = fields
            .iter()
            .position(|field| field.label == "Branch")
            .unwrap();
        d.key(KeyCode::Enter);
        d.palette_paste("feature/from-picker");
        d.key(KeyCode::Enter);

        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("New agent session form")
        };
        let branch_value = fields
            .iter()
            .find(|field| field.label == "Branch")
            .unwrap()
            .value
            .clone();
        assert_eq!(
            created_branch_name(&branch_value),
            Some("feature/from-picker")
        );
        assert_eq!(field_value(fields, "Base"), "main");
        assert!(
            d.palette.as_ref().unwrap().model_status.is_none(),
            "Harness default does not need a registered-workspace model check"
        );

        submit(&mut d);
        let messages = d.drain_outbox();
        let request = match messages.as_slice() {
            [query, request] if matches!(query.request, Request::QueryAgentModels { .. }) => {
                assert!(matches!(
                    &query.request,
                    Request::QueryAgentModels {
                        project,
                        workspace,
                        ..
                    } if project == "demo" && workspace == "root"
                ));
                request.clone()
            }
            [request] => request.clone(),
            _ => panic!(
                "branch creation should emit its read-only lookup and one mutation: {messages:?}"
            ),
        };
        let (project, id) = match &request.request {
            Request::CreateWorkspaceWithLaunch {
                project,
                id,
                launch,
                branch: BranchRequest::New { branch, base },
            } => {
                assert_eq!(project, "demo");
                assert_eq!(branch, "feature/from-picker");
                assert_eq!(base, "main");
                assert!(
                    launch.is_none(),
                    "the prompt must not accompany workspace creation"
                );
                (project.clone(), id.clone())
            }
            other => panic!("typed branch should create its workspace first: {other:?}"),
        };
        assert_eq!(id.len(), 32);
        assert!(
            d.lifecycle_pending
                .as_ref()
                .is_some_and(|row| row.then.len() == 1)
        );

        d.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Ok,
        });
        assert!(
            d.provisional
                .as_ref()
                .is_some_and(|row| row.then.len() == 1)
        );
        let allocations =
            d.handle_server_message(ServerMessage::Event(ServerEvent::LifecycleCompleted {
                client_token: request.request_id,
                op: crate::protocol::LifecycleOp::CreateWorkspaceWithLaunch,
                outcome: crate::protocol::LifecycleOutcome::Succeeded,
                target_workspace: Some(crate::protocol::WorkspaceLifecycleIdentity {
                    project: project.clone(),
                    id: id.clone(),
                }),
            }));
        let [allocation] = allocations.as_slice() else {
            panic!("branch success should allocate one prompt intent: {allocations:?}");
        };
        assert_eq!(allocation.request, Request::AllocateInitialPromptIntent);
        let followup = d.handle_server_message(ServerMessage::Response {
            request_id: allocation.request_id,
            response: Response::InitialPromptIntent(crate::protocol::InitialPromptIntentId {
                server_lifetime: "123e4567-e89b-12d3-a456-426614174000".into(),
                sequence: 1,
            }),
        });
        let [launch] = followup.as_slice() else {
            panic!(
                "workspace success should continue through one-use prompt allocation: {followup:?}"
            );
        };
        let Request::CreateAgentSessionWithPrompt(launch) = &launch.request else {
            panic!("the privacy-safe follow-up should carry the intent: {launch:?}");
        };
        assert_eq!(launch.launch.session.project, "demo");
        assert_eq!(launch.launch.session.workspace, id);
        assert_eq!(launch.launch.session.name, "new branch agent");
        assert_eq!(launch.launch.session.argv, argv);
        assert_eq!(launch.prompt, "a private one-use prompt");
    }

    #[test]
    fn new_agent_typed_branch_uses_advanced_base_override_in_workspace_request() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "codex".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "codex".into(),
            argv,
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings
            .launch_choices
            .insert("demo".into(), agent_choice("codex"));
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record("root", "/tmp/unused", "trunk")]),
        );
        {
            let palette = d.palette.as_mut().unwrap();
            palette.suggestions.worker = None;
            palette.suggestions.in_flight = None;
            palette.suggestions.project = Some("demo".into());
            palette.suggestions.cache.insert(
                "demo".into(),
                Ok(super::super::git_hints::Hints {
                    branches: vec!["main".into(), "develop".into()],
                    base: "main".into(),
                }),
            );
        }
        set(&mut d, "Base", "develop");
        let Page::Form { fields, active, .. } = &mut d.palette.as_mut().unwrap().page else {
            panic!("New agent session form")
        };
        *active = fields
            .iter()
            .position(|field| field.label == "Branch")
            .unwrap();
        d.key(KeyCode::Enter);
        d.palette_paste("feature/other-base");
        d.key(KeyCode::Enter);
        submit(&mut d);
        let messages = d.drain_outbox();
        let request = messages
            .iter()
            .find(|message| matches!(message.request, Request::CreateWorkspaceWithLaunch { .. }))
            .expect("typed branch submits its workspace create")
            .clone();
        assert!(matches!(
            request.request,
            Request::CreateWorkspaceWithLaunch {
                branch: BranchRequest::New { base, .. },
                launch: None,
                ..
            } if base == "develop"
        ));
    }

    #[test]
    fn new_agent_typed_branch_rechecks_explicit_model_for_created_workspace_before_start() {
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "claude".into(),
            "--".into(),
            "/fixture/claude".into(),
        ];
        let mut d = dashboard();
        d.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: "claude".into(),
            argv: argv.clone(),
            source: AgentSource::Detected,
            origin: AgentOrigin::Detected,
        }]);
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent {
                preset: "claude".into(),
                model_policy: AgentModelPolicy::Explicit("sonnet".into()),
            },
        );
        d.selected_container = Some(super::super::TreeRow::Workspace {
            project: "demo".into(),
            id: "root".into(),
        });
        open_registered_agent_form(
            &mut d,
            registry(vec![workspace_record("root", "/tmp/unused", "trunk")]),
        );
        assert_eq!(
            d.palette
                .as_ref()
                .unwrap()
                .parent_branch_target
                .as_ref()
                .map(|(value, _)| value.as_str()),
            Some("demo/root"),
            "the registered destination remains the model verification target while the branch query changes"
        );
        let requests = d.drain_outbox();
        let [query] = requests.as_slice() else {
            panic!("New agent setup should query the registered workspace model: {requests:?}");
        };
        assert!(matches!(
            &query.request,
            Request::QueryAgentModels {
                preset,
                origin: AgentOrigin::Detected,
                argv: query_argv,
                project,
                workspace,
            } if preset == "claude"
                && query_argv == &argv
                && project == "demo"
                && workspace == "root"
        ));
        set(&mut d, "Name", "Parent draft title");
        set(&mut d, "Initial prompt", "Parent draft prompt");
        {
            let palette = d.palette.as_mut().unwrap();
            palette.suggestions.worker = None;
            palette.suggestions.in_flight = None;
            palette.suggestions.project = Some("demo".into());
            palette.suggestions.cache.insert(
                "demo".into(),
                Ok(super::super::git_hints::Hints {
                    branches: vec!["trunk".into(), "main".into()],
                    base: "main".into(),
                }),
            );
        }
        d.key(KeyCode::Tab);
        d.key(KeyCode::Enter);
        assert_eq!(
            d.palette
                .as_ref()
                .unwrap()
                .parent_branch_target
                .as_ref()
                .map(|(target, _)| target.as_str()),
            Some("demo/root"),
            "opening the Branch picker keeps its registered model-query target"
        );
        d.palette_paste("feature/needs-model-recheck");
        assert_eq!(
            d.palette
                .as_ref()
                .unwrap()
                .parent_branch_target
                .as_ref()
                .map(|(target, _)| target.as_str()),
            Some("demo/root"),
            "typing a branch query keeps its registered model-query target"
        );
        d.key(KeyCode::Enter);
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("typed branch should keep New agent session form open")
        };
        assert_eq!(
            created_branch_name(&field_value(fields, "Branch")),
            Some("feature/needs-model-recheck")
        );
        d.agent_models_event(
            query.request_id,
            crate::protocol::AgentModelCapabilities {
                preset: "claude".into(),
                binding: Some("registered-root-binding".into()),
                models: vec![crate::protocol::AgentModelOption {
                    alias: "sonnet".into(),
                    resolved: "claude-sonnet-4-6".into(),
                }],
                reason: None,
            },
        );
        assert!(
            d.drain_outbox().is_empty(),
            "a pending lookup for the registered fallback remains valid while the typed branch is selected"
        );
        let Page::Form { fields, .. } = &d.palette.as_ref().unwrap().page else {
            panic!("New agent session form")
        };
        assert_eq!(field_value(fields, "Model"), "sonnet");
        let model_field = fields.iter().find(|field| field.label == "Model").unwrap();
        let FieldKind::Pick(model_list) = &model_field.kind else {
            panic!("Model picker")
        };
        assert!(
            model_list.items.iter().any(|item| item.value == "sonnet"),
            "the verified parent model choice should remain available until branch readback; caps={:?}, target={:?}, identity={:?}",
            d.palette.as_ref().unwrap().model_capabilities,
            d.palette.as_ref().unwrap().parent_branch_target,
            d.palette.as_ref().unwrap().model_capability_identity
        );
        let pending_before_create = d.drain_outbox();
        assert!(
            pending_before_create.is_empty(),
            "creating a branch must wait to query its new registered workspace: {pending_before_create:?}"
        );
        submit(&mut d);
        let messages = d.drain_outbox();
        let [mutation] = messages.as_slice() else {
            panic!(
                "typed branch should create the workspace without launching: {messages:?}; error={:?}; invalid={:?}",
                d.palette
                    .as_ref()
                    .and_then(|palette| palette.error.as_ref()),
                d.palette
                    .as_ref()
                    .and_then(|palette| palette.invalid.as_ref())
            );
        };
        let (project, workspace_id, branch) = match &mutation.request {
            Request::CreateWorkspace {
                project,
                id,
                branch: BranchRequest::New { branch, base },
            } => {
                assert_eq!(base, "main");
                (project.clone(), id.clone(), branch.clone())
            }
            request => panic!("expected a workspace-only create: {request:?}"),
        };
        assert_eq!(project, "demo");
        d.key(KeyCode::Esc);
        assert!(matches!(
            d.palette.as_ref().map(|palette| &palette.page),
            Some(Page::Form {
                command: Command::NewAgentSession,
                ..
            })
        ));
        assert_eq!(
            d.palette.as_ref().unwrap().pending,
            Some(mutation.request_id),
            "a submitted branch stays visible until its correlated readback"
        );
        d.handle_server_message(ServerMessage::Response {
            request_id: mutation.request_id,
            response: Response::Ok,
        });
        let readback = branch_completion(
            &mut d,
            mutation.request_id,
            &project,
            &workspace_id,
            crate::protocol::LifecycleOutcome::Succeeded,
        );
        let inspect = readback
            .iter()
            .find(|message| message.request == Request::Inspect)
            .unwrap()
            .clone();
        let list = readback
            .iter()
            .find(|message| message.request == Request::List)
            .unwrap()
            .clone();
        let (registry, hierarchy) = branch_readback(
            &workspace_id,
            &branch,
            "/tmp/workspaces/feature-child",
            true,
        );
        let fresh_queries =
            finish_branch_readback(&mut d, inspect, list, registry, hierarchy, false);
        let [fresh_query] = fresh_queries.as_slice() else {
            panic!("verified model lookup must target the new workspace: {fresh_queries:?}");
        };
        assert!(matches!(
            &fresh_query.request,
            Request::QueryAgentModels {
                preset,
                origin: AgentOrigin::Detected,
                project: query_project,
                workspace: query_workspace,
                ..
            } if preset == "claude"
                && query_project == "demo"
                && query_workspace == &workspace_id
        ));
        let (fields, _) = new_agent_fields(&d);
        assert_eq!(
            field_value(fields, "Branch"),
            super::super::picker::workspace_pick_value(&project, &workspace_id)
        );
        assert_eq!(field_value(fields, "Model"), "sonnet");
        assert_eq!(field_value(fields, "Name"), "Parent draft title");
        assert_eq!(field_value(fields, "Initial prompt"), "Parent draft prompt");

        d.agent_models_event(
            fresh_query.request_id,
            crate::protocol::AgentModelCapabilities {
                preset: "claude".into(),
                binding: Some("created-workspace-binding".into()),
                models: vec![crate::protocol::AgentModelOption {
                    alias: "sonnet".into(),
                    resolved: "resolved-child-sonnet".into(),
                }],
                reason: None,
            },
        );
        submit(&mut d);
        let allocation_messages = d.drain_outbox();
        let [allocation] = allocation_messages.as_slice() else {
            panic!("starting requires allocating a one-use prompt intent");
        };
        assert_eq!(allocation.request, Request::AllocateInitialPromptIntent);
        let followup = d.handle_server_message(ServerMessage::Response {
            request_id: allocation.request_id,
            response: Response::InitialPromptIntent(crate::protocol::InitialPromptIntentId {
                server_lifetime: "123e4567-e89b-12d3-a456-426614174000".into(),
                sequence: 1,
            }),
        });
        let [launch_message] = followup.as_slice() else {
            panic!("verified model should continue as one prompted launch: {followup:?}");
        };
        let Request::CreateAgentSessionWithPrompt(launch) = &launch_message.request else {
            panic!("launch must carry the one-use prompt intent: {launch_message:?}");
        };
        assert_eq!(launch.launch.session.project, "demo");
        assert_eq!(launch.launch.session.workspace, workspace_id);
        assert_eq!(launch.launch.session.name, "Parent draft title");
        assert_eq!(
            launch.launch.model_policy,
            AgentModelPolicy::Explicit("sonnet".into())
        );
        assert_eq!(
            launch.launch.model_binding.as_deref(),
            Some("created-workspace-binding")
        );
        assert_eq!(
            launch.launch.expected_model.as_deref(),
            Some("resolved-child-sonnet")
        );
        assert_eq!(launch.prompt, "Parent draft prompt");
    }

    #[test]
    fn canceling_typed_branch_restores_registered_target_for_a_pending_model_response() {
        let (mut d, _argv, old_query) = open_explicit_new_agent_form();
        set(&mut d, "Name", "Parent draft title");
        set(&mut d, "Initial prompt", "Parent draft prompt");
        d.key(KeyCode::Tab);
        d.key(KeyCode::Enter);
        d.palette_paste("feature/cancelled");
        d.key(KeyCode::Enter);
        assert_eq!(
            created_branch_name(&field_value(new_agent_fields(&d).0, "Branch")),
            Some("feature/cancelled")
        );
        assert!(
            d.drain_outbox().is_empty(),
            "the original lookup is still pending"
        );

        d.key(KeyCode::Enter);
        assert!(d.palette.as_ref().unwrap().new_agent_picker_open);
        d.key(KeyCode::Esc);
        assert!(!d.palette.as_ref().unwrap().new_agent_picker_open);
        let (fields, _) = new_agent_fields(&d);
        assert_eq!(
            field_value(fields, "Branch"),
            super::super::picker::workspace_pick_value("demo", "root")
        );
        assert_eq!(field_value(fields, "Name"), "Parent draft title");
        assert_eq!(field_value(fields, "Initial prompt"), "Parent draft prompt");
        assert!(
            d.drain_outbox().is_empty(),
            "do not duplicate the pending lookup"
        );

        d.agent_models_event(
            old_query.request_id,
            crate::protocol::AgentModelCapabilities {
                preset: "claude".into(),
                binding: Some("returned-root-binding".into()),
                models: vec![crate::protocol::AgentModelOption {
                    alias: "sonnet".into(),
                    resolved: "resolved-root-sonnet".into(),
                }],
                reason: None,
            },
        );
        assert!(d.drain_outbox().is_empty());
        assert_eq!(
            d.palette
                .as_ref()
                .unwrap()
                .model_capabilities
                .as_ref()
                .unwrap()
                .binding
                .as_deref(),
            Some("returned-root-binding")
        );
        assert_eq!(field_value(new_agent_fields(&d).0, "Model"), "sonnet");
    }

    #[test]
    fn mouse_and_cycle_create_actions_cancel_back_to_registered_target() {
        for path in ["mouse", "cycle"] {
            let mut d = dashboard();
            d.selected_container = Some(super::super::TreeRow::Workspace {
                project: "demo".into(),
                id: "root".into(),
            });
            open_registered_agent_form(
                &mut d,
                registry(vec![workspace_record("root", "/tmp/unused", "trunk")]),
            );
            d.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            d.palette_paste("spike/pane-tree");

            if path == "mouse" {
                let (_, popup, lines, rows) = creation_popup_snapshot(&d, 80, 24);
                let (row, _) = *rows.first().expect("Create branch row is clickable");
                assert!(lines.iter().any(|line| line.contains("Create branch")));
                d.mouse_action(
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: popup.x + 2,
                        row,
                        modifiers: KeyModifiers::NONE,
                    },
                    Rect::new(0, 0, 80, 24),
                );
            } else {
                d.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
                d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
            assert_eq!(
                created_branch_name(&field_value(new_agent_fields(&d).0, "Branch")),
                Some("spike/pane-tree"),
                "{path} must select the typed create action"
            );

            d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            d.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            let (fields, active) = new_agent_fields(&d);
            let branch = &fields[active];
            assert_eq!(branch.label, "Branch");
            assert_eq!(branch.value, "demo/root");
            assert!(!branch.edited);
            assert!(d.drain_outbox().iter().all(|message| !matches!(
                message.request,
                Request::CreateWorkspace { .. } | Request::CreateWorkspaceWithLaunch { .. }
            )));
        }
    }

    #[test]
    fn typed_branch_with_unverified_model_does_not_create_workspace() {
        let (mut d, _argv, _old_query) = open_explicit_new_agent_form();
        set(&mut d, "Model", "unverified-model");
        d.key(KeyCode::Tab);
        d.key(KeyCode::Enter);
        d.palette_paste("feature/unverified-model");
        d.key(KeyCode::Enter);
        submit(&mut d);

        let requests = d.drain_outbox();
        assert!(
            requests.iter().all(|message| !matches!(
                message.request,
                Request::CreateWorkspace { .. }
                    | Request::CreateWorkspaceWithLaunch { .. }
                    | Request::CreateAgentSession(_)
                    | Request::CreateAgentSessionWithPrompt(_)
                    | Request::AllocateInitialPromptIntent
            )),
            "an unverified explicit model must not mutate the workspace or launch: {requests:?}"
        );
        assert!(
            d.palette
                .as_ref()
                .unwrap()
                .invalid
                .as_ref()
                .is_some_and(|(_, reason)| reason.contains("Model is not in the verified choices"))
        );
    }

    #[test]
    fn branch_form_project_choice_refreshes_detected_base_without_expanding_options() {
        let mut d = dashboard();
        open_branch_form(&mut d);
        let mut palette = d.palette.take().unwrap();
        let inventory = palette.workspace_inventory.as_mut().unwrap();
        inventory.projects.insert(
            0,
            ProjectRecord {
                name: "other".into(),
                repo: "/tmp/other-repo".into(),
                workspace_root: "/tmp/other-workspaces".into(),
                workspaces: vec![workspace_record("other-root", "/tmp/other-root", "stable")],
            },
        );
        palette
            .suggestions
            .repos
            .insert("other".into(), "/tmp/other-repo".into());
        palette.suggestions.cache.insert(
            "other".into(),
            Ok(super::super::git_hints::Hints {
                branches: vec!["stable".into()],
                base: "stable".into(),
            }),
        );
        d.refresh_workspace_form(&mut palette);
        d.palette = Some(palette);

        d.key(KeyCode::BackTab);
        d.key(KeyCode::Enter);
        d.key(KeyCode::Up);
        d.key(KeyCode::Enter);

        let palette = d.palette.as_ref().unwrap();
        let Page::Form { fields, .. } = &palette.page else {
            panic!("Create branch form")
        };
        assert_eq!(
            fields
                .iter()
                .find(|field| field.label == "Project")
                .unwrap()
                .value,
            "other"
        );
        assert_eq!(palette.branch_project.as_deref(), Some("other"));
        assert_eq!(palette.suggestions.project.as_deref(), Some("other"));
        assert_eq!(
            fields
                .iter()
                .find(|field| field.label == "Base")
                .unwrap()
                .value,
            "stable"
        );
        assert!(
            fields
                .iter()
                .find(|field| field.label == "Base")
                .unwrap()
                .hidden
        );
        assert!(!palette.new_agent_picker_open);
    }

    #[test]
    fn branch_form_submit_stays_visible_until_correlated_receipt_and_readback() {
        let mut d = dashboard();
        open_branch_form(&mut d);
        let request = request(&mut d);
        let (project, workspace_id, branch) = match &request.request {
            Request::CreateWorkspace {
                project,
                id,
                branch: BranchRequest::New { branch, base },
            } => {
                assert_eq!(base, "develop");
                (project.clone(), id.clone(), branch.clone())
            }
            other => panic!("branch form must create only a workspace: {other:?}"),
        };
        assert_eq!(
            (project.as_str(), branch.as_str()),
            ("demo", "feature/child")
        );
        assert_eq!(workspace_id.len(), 32);
        assert_eq!(
            d.palette
                .as_ref()
                .unwrap()
                .branch_attempt
                .as_ref()
                .unwrap()
                .token,
            request.request_id
        );
        d.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Ok,
        });
        assert!(
            d.palette
                .as_ref()
                .unwrap()
                .branch_attempt
                .as_ref()
                .unwrap()
                .accepted
        );
        assert!(matches!(
            d.palette.as_ref().map(|palette| &palette.page),
            Some(Page::Form {
                command: Command::CreateBranch,
                ..
            })
        ));
        assert!(d.palette.as_ref().unwrap().pending.is_some());

        d.key(KeyCode::Esc);
        d.key(KeyCode::Enter);
        assert!(matches!(
            d.palette.as_ref().map(|palette| &palette.page),
            Some(Page::Form {
                command: Command::CreateBranch,
                ..
            })
        ));
        assert!(d.drain_outbox().is_empty());

        // After admission, the matching receipt starts the fresh readback.
        let readback = branch_completion(
            &mut d,
            request.request_id,
            &project,
            &workspace_id,
            crate::protocol::LifecycleOutcome::Succeeded,
        );
        assert!(
            d.palette
                .as_ref()
                .unwrap()
                .branch_attempt
                .as_ref()
                .unwrap()
                .outcome
                .is_some()
        );
        assert_eq!(readback.len(), 2);
        let inspect = readback
            .iter()
            .find(|message| message.request == Request::Inspect)
            .unwrap()
            .clone();
        let list = readback
            .iter()
            .find(|message| message.request == Request::List)
            .unwrap()
            .clone();
        let (registry, hierarchy) = branch_readback(
            &workspace_id,
            &branch,
            "/tmp/workspaces/feature-child",
            true,
        );
        finish_branch_readback(&mut d, inspect, list, registry, hierarchy, false);
        assert!(
            d.palette.is_none(),
            "successful branch form closes after readback"
        );
        assert_eq!(
            d.selected_container,
            Some(super::super::TreeRow::Workspace {
                project,
                id: workspace_id,
            })
        );
    }

    #[test]
    fn branch_form_handles_both_receipt_and_readback_orders() {
        for event_first in [false, true] {
            for list_first in [false, true] {
                let mut d = dashboard();
                open_branch_form(&mut d);
                let request = request(&mut d);
                let Request::CreateWorkspace {
                    project,
                    id,
                    branch: BranchRequest::New { branch, .. },
                } = &request.request
                else {
                    panic!("branch request")
                };
                let readback = if event_first {
                    let early = branch_completion(
                        &mut d,
                        request.request_id,
                        project,
                        id,
                        crate::protocol::LifecycleOutcome::Succeeded,
                    );
                    assert!(early.is_empty());
                    d.handle_server_message(ServerMessage::Response {
                        request_id: request.request_id,
                        response: Response::Ok,
                    })
                } else {
                    d.handle_server_message(ServerMessage::Response {
                        request_id: request.request_id,
                        response: Response::Ok,
                    });
                    branch_completion(
                        &mut d,
                        request.request_id,
                        project,
                        id,
                        crate::protocol::LifecycleOutcome::Succeeded,
                    )
                };
                let inspect = readback
                    .iter()
                    .find(|message| message.request == Request::Inspect)
                    .unwrap()
                    .clone();
                let list = readback
                    .iter()
                    .find(|message| message.request == Request::List)
                    .unwrap()
                    .clone();
                let (registry, hierarchy) =
                    branch_readback(id, branch, "/tmp/workspaces/feature-child", true);
                finish_branch_readback(&mut d, inspect, list, registry, hierarchy, list_first);
                assert!(
                    d.palette.is_none(),
                    "successful branch form closes after readback"
                );
                assert_eq!(
                    d.selected_container,
                    Some(super::super::TreeRow::Workspace {
                        project: project.clone(),
                        id: id.clone(),
                    })
                );
            }
        }
    }

    #[test]
    fn wrong_target_with_reused_token_is_ignored_and_mismatched_path_blocks_selection() {
        let mut d = dashboard();
        open_branch_form(&mut d);
        let request = request(&mut d);
        let Request::CreateWorkspace {
            project,
            id,
            branch: BranchRequest::New { branch, .. },
        } = &request.request
        else {
            panic!("branch request")
        };
        d.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Ok,
        });
        d.handle_server_message(ServerMessage::Event(ServerEvent::LifecycleCompleted {
            client_token: request.request_id,
            op: crate::protocol::LifecycleOp::CreateWorkspace,
            outcome: crate::protocol::LifecycleOutcome::Succeeded,
            target_workspace: Some(crate::protocol::WorkspaceLifecycleIdentity {
                project: project.clone(),
                id: "stale-workspace-id".into(),
            }),
        }));
        d.handle_server_message(ServerMessage::Event(ServerEvent::LifecycleCompleted {
            client_token: request.request_id + 1,
            op: crate::protocol::LifecycleOp::CreateWorkspace,
            outcome: crate::protocol::LifecycleOutcome::Succeeded,
            target_workspace: Some(crate::protocol::WorkspaceLifecycleIdentity {
                project: project.clone(),
                id: id.clone(),
            }),
        }));
        d.handle_server_message(ServerMessage::Event(ServerEvent::LifecycleCompleted {
            client_token: request.request_id,
            op: crate::protocol::LifecycleOp::CreateWorkspaceWithLaunch,
            outcome: crate::protocol::LifecycleOutcome::Succeeded,
            target_workspace: Some(crate::protocol::WorkspaceLifecycleIdentity {
                project: project.clone(),
                id: id.clone(),
            }),
        }));
        assert!(d.drain_outbox().is_empty());
        assert!(d.palette.as_ref().unwrap().pending.is_some());

        let readback = branch_completion(
            &mut d,
            request.request_id,
            project,
            id,
            crate::protocol::LifecycleOutcome::Succeeded,
        );
        let inspect = readback
            .iter()
            .find(|message| message.request == Request::Inspect)
            .unwrap()
            .clone();
        let list = readback
            .iter()
            .find(|message| message.request == Request::List)
            .unwrap()
            .clone();
        let (registry, mut hierarchy) =
            branch_readback(id, branch, "/tmp/workspaces/feature-child", true);
        hierarchy.projects[0].workspaces[1].path = "/tmp/workspaces/wrong-path".into();
        finish_branch_readback(&mut d, inspect, list, registry, hierarchy, false);
        assert!(d.palette.as_ref().unwrap().pending.is_none());
        assert!(
            d.palette
                .as_ref()
                .unwrap()
                .branch_attempt
                .as_ref()
                .unwrap()
                .blocked
        );
        assert!(matches!(
            d.palette.as_ref().map(|palette| &palette.page),
            Some(Page::Form {
                command: Command::CreateBranch,
                ..
            })
        ));
        assert!(d.drain_outbox().is_empty());
    }

    #[test]
    fn partial_branch_failure_reconciles_before_retry_with_fresh_token_same_identity() {
        let mut d = dashboard();
        open_branch_form(&mut d);
        let first = request(&mut d);
        let Request::CreateWorkspace {
            project,
            id,
            branch: BranchRequest::New { branch, .. },
        } = &first.request
        else {
            panic!("branch request")
        };
        let early = branch_completion(
            &mut d,
            first.request_id,
            project,
            id,
            crate::protocol::LifecycleOutcome::Failed {
                code: ErrorCode::PartialFailure,
                message: "workspace setup may have written files".into(),
            },
        );
        assert!(early.is_empty());
        let readback = d.handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::Ok,
        });
        let inspect = readback
            .iter()
            .find(|message| message.request == Request::Inspect)
            .unwrap()
            .clone();
        let list = readback
            .iter()
            .find(|message| message.request == Request::List)
            .unwrap()
            .clone();
        let (registry, hierarchy) = branch_readback(id, branch, "/unused", false);
        finish_branch_readback(&mut d, inspect, list, registry, hierarchy, true);
        assert!(d.palette.as_ref().unwrap().branch_attempt.is_none());
        assert!(d.palette.as_ref().unwrap().pending.is_none());
        assert_eq!(
            d.palette.as_ref().unwrap().workspace_id.as_deref(),
            Some(id.as_str())
        );

        let retry = request(&mut d);
        assert_ne!(retry.request_id, first.request_id);
        assert!(matches!(
            retry.request,
            Request::CreateWorkspace { project: ref retry_project, id: ref retry_id, .. }
                if retry_project == project && retry_id == id
        ));
    }

    #[test]
    fn unclear_branch_response_stays_blocked_until_terminal_receipt_and_readback() {
        let (mut d, _argv, old_query) = open_explicit_new_agent_form();
        d.agent_models_event(
            old_query.request_id,
            crate::protocol::AgentModelCapabilities {
                preset: "claude".into(),
                binding: Some("registered-root-binding".into()),
                models: vec![crate::protocol::AgentModelOption {
                    alias: "sonnet".into(),
                    resolved: "claude-sonnet-4-6".into(),
                }],
                reason: None,
            },
        );
        {
            let palette = d.palette.as_mut().unwrap();
            palette.suggestions.worker = None;
            palette.suggestions.in_flight = None;
            palette.suggestions.project = Some("demo".into());
            palette.suggestions.cache.insert(
                "demo".into(),
                Ok(super::super::git_hints::Hints {
                    branches: vec!["trunk".into(), "main".into()],
                    base: "main".into(),
                }),
            );
        }
        d.key(KeyCode::Tab);
        d.key(KeyCode::Enter);
        d.palette_paste("feature/child");
        d.key(KeyCode::Enter);
        let first = request(&mut d);
        let Request::CreateWorkspace { project, id, .. } = &first.request else {
            panic!("branch request")
        };

        let readback = d.handle_server_message(ServerMessage::Response {
            request_id: first.request_id,
            response: Response::Hierarchy(crate::protocol::HierarchySnapshot {
                projects: Vec::new(),
            }),
        });
        assert_eq!(readback.len(), 2);
        assert!(
            readback
                .iter()
                .all(|message| matches!(message.request, Request::Inspect | Request::List))
        );
        assert!(
            d.palette
                .as_ref()
                .unwrap()
                .branch_attempt
                .as_ref()
                .unwrap()
                .blocked
        );
        assert!(d.palette.as_ref().unwrap().pending.is_some());

        // A second submit while the unknown request is being reconciled cannot create again.
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        assert!(d.drain_outbox().is_empty());

        let inspect = readback
            .iter()
            .find(|message| message.request == Request::Inspect)
            .unwrap()
            .clone();
        let list = readback
            .iter()
            .find(|message| message.request == Request::List)
            .unwrap()
            .clone();
        d.handle_server_message(ServerMessage::Response {
            request_id: inspect.request_id,
            response: Response::Error {
                code: ErrorCode::Conflict,
                message: "temporary Inspect failure".into(),
            },
        });
        d.handle_server_message(ServerMessage::Response {
            request_id: list.request_id,
            response: Response::Hierarchy(crate::protocol::HierarchySnapshot {
                projects: Vec::new(),
            }),
        });
        let palette = d.palette.as_ref().unwrap();
        assert!(palette.pending.is_none());
        assert!(palette.branch_attempt.as_ref().unwrap().blocked);
        assert!(
            palette
                .error
                .as_deref()
                .unwrap()
                .contains("Could not confirm whether the branch was created")
        );
        assert_eq!(
            button_text(
                &palette.page,
                100,
                palette.pending.is_none()
                    && palette
                        .branch_attempt
                        .as_ref()
                        .is_some_and(|attempt| attempt.blocked),
            ),
            "[Refresh] [Cancel]"
        );

        // The visible Refresh action repeats only Inspect + List; absence alone cannot retry.
        d.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        let refreshed = d.drain_outbox();
        assert_eq!(refreshed.len(), 2);
        assert!(
            refreshed
                .iter()
                .all(|message| matches!(message.request, Request::Inspect | Request::List))
        );
        let inspect = refreshed
            .iter()
            .find(|message| message.request == Request::Inspect)
            .unwrap()
            .clone();
        let list = refreshed
            .iter()
            .find(|message| message.request == Request::List)
            .unwrap()
            .clone();
        let (registry, hierarchy) = branch_readback(id, "feature/child", "/unused", false);
        finish_branch_readback(&mut d, inspect, list, registry, hierarchy, true);
        assert!(d.palette.as_ref().unwrap().pending.is_none());
        assert!(
            d.palette
                .as_ref()
                .unwrap()
                .branch_attempt
                .as_ref()
                .unwrap()
                .blocked
        );
        assert!(
            d.palette
                .as_ref()
                .unwrap()
                .error
                .as_deref()
                .unwrap()
                .contains("no terminal receipt")
        );
        assert_eq!(
            d.palette.as_ref().unwrap().workspace_id.as_deref(),
            Some(id.as_str())
        );
        assert!(d.drain_outbox().is_empty());

        // A late terminal receipt after an empty snapshot starts another fresh readback.
        let final_readback = branch_completion(
            &mut d,
            first.request_id,
            project,
            id,
            crate::protocol::LifecycleOutcome::Succeeded,
        );
        assert_eq!(final_readback.len(), 2);
        let inspect = final_readback
            .iter()
            .find(|message| message.request == Request::Inspect)
            .unwrap()
            .clone();
        let list = final_readback
            .iter()
            .find(|message| message.request == Request::List)
            .unwrap()
            .clone();
        let (registry, hierarchy) =
            branch_readback(id, "feature/child", "/tmp/workspaces/feature-child", true);
        let fresh_queries =
            finish_branch_readback(&mut d, inspect, list, registry, hierarchy, false);
        assert!(d.palette.as_ref().unwrap().branch_attempt.is_none());
        assert!(d.palette.as_ref().unwrap().pending.is_none());
        assert!(matches!(
            d.palette.as_ref().map(|palette| &palette.page),
            Some(Page::Form {
                command: Command::NewAgentSession,
                ..
            })
        ));
        let [fresh_query] = fresh_queries.as_slice() else {
            panic!("created workspace should be verified before launch: {fresh_queries:?}");
        };
        assert!(matches!(
            &fresh_query.request,
            Request::QueryAgentModels {
                project,
                workspace,
                ..
            } if project == "demo" && workspace == id
        ));
    }

    #[test]
    fn standalone_branch_returns_to_browse_on_workspace_even_with_local_shell() {
        let mut d = dashboard();
        d.open_create_workspace();
        let inspect = d.drain_one_request().unwrap();
        d.palette_response(
            inspect.request_id,
            &Response::Inventory {
                registry: registry(vec![workspace_record("root", "/tmp/unused", "trunk")]),
                sessions: Vec::new(),
            },
        );
        {
            let palette = d.palette.as_mut().unwrap();
            palette.suggestions.worker = None;
            palette.suggestions.in_flight = None;
            palette.suggestions.project = Some("demo".into());
            palette.suggestions.cache.insert(
                "demo".into(),
                Ok(super::super::git_hints::Hints {
                    branches: vec!["trunk".into(), "develop".into()],
                    base: "develop".into(),
                }),
            );
        }
        set(&mut d, "Branch name", "feature/child");
        let request = request(&mut d);
        let Request::CreateWorkspace {
            project,
            id,
            branch,
        } = &request.request
        else {
            panic!("standalone branch form sends workspace creation")
        };
        d.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Ok,
        });
        let readback = branch_completion(
            &mut d,
            request.request_id,
            project,
            id,
            crate::protocol::LifecycleOutcome::Succeeded,
        );
        let inspect = readback
            .iter()
            .find(|message| message.request == Request::Inspect)
            .unwrap()
            .clone();
        let list = readback
            .iter()
            .find(|message| message.request == Request::List)
            .unwrap()
            .clone();
        let (registry, mut hierarchy) =
            branch_readback(id, "feature/child", "/tmp/workspaces/feature-child", true);
        let local = summary(909);
        let local = SessionSummary {
            project: project.clone(),
            workspace: id.clone(),
            name: "local".into(),
            cwd: "/tmp/workspaces/feature-child".into(),
            ..local
        };
        hierarchy.projects[0].workspaces[1].sessions.push(local);
        finish_branch_readback(&mut d, inspect, list, registry, hierarchy, false);
        assert!(d.palette.is_none());
        assert_eq!(d.mode, InputMode::Browse);
        assert_eq!(
            d.selected_container,
            Some(super::super::TreeRow::Workspace {
                project: project.clone(),
                id: id.clone(),
            })
        );
        assert_eq!(d.focused_session(), None);
        let _ = branch;
    }
}

#[cfg(test)]
mod form_tests {
    use super::*;
    use crate::protocol::{
        AgentActivity, ProjectSummary, Response, ServerMessage, SessionPhase, WorkspaceSummary,
    };

    const ID: &str = "7833bc0a230c03372aecca12545fec60";

    fn session(id: u64, workspace: &str, name: &str) -> SessionSummary {
        SessionSummary {
            scope: ovrcr_protocol::SessionScope::Workspace,
            archived: false,
            cwd: "/work".into(),
            id: SessionId(id),
            run: crate::protocol::SessionRunId(1),
            kind: SessionKind::Terminal,
            recovery: None,
            project: "consigint".into(),
            workspace: workspace.into(),
            name: name.into(),
            title: None,
            manual_title: None,
            assignment_control: None,
            label: "zsh".into(),
            pid: Some(id as u32),
            started_unix_ms: Some(0),
            last_activity_unix_ms: None,
            phase: SessionPhase::Running,
            activity: AgentActivity::Unknown,
            context_usage: None,
            agent: None,
            agent_epoch: 0,
            unread: None,
        }
    }

    /// A workspace with an internal id, two running sessions and a second
    /// workspace whose session shares one title.
    fn dashboard() -> Dashboard {
        let mut d = Dashboard::new(crate::session::TerminalSize {
            rows: 30,
            cols: 100,
        });
        let workspace = |id: &str, name: &str, sessions| WorkspaceSummary {
            project: "consigint".into(),
            id: id.into(),
            name: name.into(),
            path: format!("/config/workspaces/consigint/{id}").into(),
            root: false,
            warning: None,
            sessions,
        };
        d.hierarchy.projects.push(ProjectSummary {
            name: "consigint".into(),
            workspaces: vec![
                workspace(
                    ID,
                    "feature/status-check",
                    vec![session(13, ID, "verify spinner"), session(14, ID, "local")],
                ),
                workspace("other", "main", vec![session(15, "other", "local")]),
            ],
        });
        d
    }

    #[test]
    fn branch_name_problems_follow_git_check_ref_format() {
        let cases: [(&str, Option<&str>); 23] = [
            ("feature/status-check", None),
            ("日本/ブランチ", None),
            ("a@b", None),
            ("feature/x.y", None),
            ("x/y.lockz", None),
            (
                "feature/",
                Some("Branch names can't start or end with / or contain //"),
            ),
            (
                "/a",
                Some("Branch names can't start or end with / or contain //"),
            ),
            (
                "a//b",
                Some("Branch names can't start or end with / or contain //"),
            ),
            ("a b", Some("Branch names can't contain spaces")),
            (
                "a\tb",
                Some("Branch names can't contain control characters"),
            ),
            (
                "a\u{7f}b",
                Some("Branch names can't contain control characters"),
            ),
            ("-x", Some("Branch names can't start with -")),
            ("HEAD", Some("Git reserves that name")),
            ("a..b", Some("Branch names can't contain .. or @{")),
            ("a@{b", Some("Branch names can't contain .. or @{")),
            ("a.", Some("Branch names can't end with .")),
            (
                "x.lock",
                Some("No part of a branch name can start with . or end with .lock"),
            ),
            (
                "a/.b",
                Some("No part of a branch name can start with . or end with .lock"),
            ),
            ("a~b", Some("Branch names can't contain ~ ^ : ? * [ or \\")),
            ("a:b", Some("Branch names can't contain ~ ^ : ? * [ or \\")),
            ("a?b", Some("Branch names can't contain ~ ^ : ? * [ or \\")),
            ("a[b", Some("Branch names can't contain ~ ^ : ? * [ or \\")),
            ("a\\b", Some("Branch names can't contain ~ ^ : ? * [ or \\")),
        ];
        let outside = tempfile::tempdir().unwrap();
        for (name, expected) in cases {
            assert_eq!(branch_name_problem(name), expected, "{name:?}");
            // Git stays the authority: the form refuses exactly what it would.
            let git = std::process::Command::new("git")
                .args(["check-ref-format", "--branch", name])
                .current_dir(outside.path())
                .env_remove("GIT_DIR")
                .output()
                .unwrap();
            assert_eq!(
                git.status.success(),
                expected.is_none(),
                "{name:?}: git disagrees"
            );
        }
        // `--branch` reads a lone @ as the current branch, never a new name.
        assert_eq!(branch_name_problem("@"), Some("Git reserves that name"));
        assert_eq!(branch_name_problem(""), Some("Add a branch name"));
    }

    #[test]
    fn plain_reasons_name_workspaces_and_sessions_and_keep_paths() {
        let d = dashboard();
        assert_eq!(
            d.plain_reason(
                &format!(
                    "sessions block removal of workspace consigint/{ID}: live: #13, #14; kill or close them"
                ),
                None
            ),
            "sessions block removal of workspace consigint / feature/status-check: live: verify spinner, local (#14); kill or close them"
        );
        let new_id = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            d.plain_reason(
                &format!(
                    "create workspace consigint/{new_id}: git command failed (exit status: 255): fatal: a branch named 'feature/new' already exists"
                ),
                Some(("consigint".into(), new_id.into(), "feature/new".into()))
            ),
            "a branch named 'feature/new' already exists"
        );
        // A directory named after the id is still the real path.
        assert_eq!(
            d.plain_reason(
                &format!(
                    "worktree for workspace {ID} remains at /config/workspaces/consigint/{ID}: boom"
                ),
                None
            ),
            format!(
                "worktree for workspace feature/status-check remains at /config/workspaces/consigint/{ID}: boom"
            )
        );
        assert_eq!(
            d.plain_reason("#99 (archived) blocks; issue #", None),
            "#99 (archived) blocks; issue #"
        );
    }

    /// The Server's advice names CLI commands and --force, which the
    /// Dashboard does not have.
    #[test]
    fn unacknowledged_refusals_point_at_acknowledge_stopped() {
        let d = dashboard();
        let reason = d.plain_reason(
            &format!(
                "sessions block removal of workspace consigint/{ID}: unacknowledged stopped: #13, #99 (archived); run `ovrcr terminal acknowledge-stopped <id>` or `ovrcr terminal remove <id>`, or remove with --force"
            ),
            None,
        );
        assert_eq!(
            reason,
            "sessions block removal of workspace consigint / feature/status-check: stopped sessions may still have processes running: verify spinner, #99 (archived); once they have stopped, choose \"Acknowledge stopped\" for each (archived ones are under Archived sessions in : commands)"
        );
        assert!(!reason.contains("--force") && !reason.contains("ovrcr terminal"));
    }

    /// A stopped session awaiting acknowledgement would refuse the removal
    /// after running sessions were already stopped; it is named first instead.
    #[test]
    fn removal_names_unacknowledged_sessions_first_and_stops_nothing() {
        let mut d = dashboard();
        let waiting = &mut d.hierarchy.projects[0].workspaces[0].sessions[1];
        waiting.phase = SessionPhase::Stopped;
        waiting.recovery = Some(crate::protocol::SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: true,
            unavailable: None,
            failure: None,
        });
        let page = d.removal_page(Request::RemoveWorkspace {
            project: "consigint".into(),
            name: ID.into(),
            force: false,
        });
        let Page::Confirm { request, target } = page else {
            panic!()
        };
        assert_eq!(request, None, "confirming sends nothing");
        assert_eq!(
            target,
            "consigint / feature/status-check has 1 stopped session that may still have processes running: local (#14). Once it has stopped, choose \"Acknowledge stopped\" for it in : commands, then remove the workspace. Nothing was stopped or removed."
        );
    }

    #[test]
    fn removal_names_running_sessions_and_stops_them_before_removing() {
        let d = dashboard();
        let remove = |force| Request::RemoveWorkspace {
            project: "consigint".into(),
            name: ID.into(),
            force,
        };
        let Page::Confirm { target, .. } = d.removal_page(remove(false)) else {
            panic!()
        };
        assert_eq!(
            target,
            "consigint / feature/status-check still has 2 running sessions: verify spinner, local (#14). Stop them and remove the workspace? The branch is kept."
        );
        let Page::Confirm { target, .. } = d.removal_page(remove(true)) else {
            panic!()
        };
        assert!(
            target.contains("Stop them, discard its uncommitted changes and remove the workspace?"),
            "{target}"
        );
        assert_eq!(
            d.removal_steps(&remove(false)),
            vec![
                Request::CloseTerminal {
                    session: SessionId(13),
                    expected_run: crate::protocol::SessionRunId(1),
                },
                Request::CloseTerminal {
                    session: SessionId(14),
                    expected_run: crate::protocol::SessionRunId(1),
                },
                remove(false),
            ]
        );
        let mut quiet = dashboard();
        for session in &mut quiet.hierarchy.projects[0].workspaces[0].sessions {
            session.phase = SessionPhase::Exited {
                code: Some(0),
                signal: None,
            };
        }
        assert_eq!(quiet.removal_steps(&remove(false)), vec![remove(false)]);
        let Page::Confirm { target, .. } = quiet.removal_page(remove(true)) else {
            panic!()
        };
        assert_eq!(
            target,
            "consigint / feature/status-check has uncommitted changes. Discard them and remove the workspace? The branch is kept."
        );
    }

    /// Create workspace focused on its Agent field, the last one, offering
    /// `count` agents with the first chosen.
    fn agent_picker(count: usize) -> Dashboard {
        let mut d = dashboard();
        let mut page = d.palette_form(Command::CreateWorkspace);
        let Page::Form { fields, active, .. } = &mut page else {
            panic!()
        };
        let items = (0..count)
            .map(|i| PickItem {
                label: format!("agent-{i:02}"),
                value: format!("agent-{i:02}"),
            })
            .collect();
        let start = fields.iter().position(|f| f.label == "Start").unwrap();
        fields[start].value = "Agent".into();
        *active = fields.iter().position(|f| f.label == "Agent").unwrap();
        fields[*active] = pick_field("Agent", "agent-00".into(), items);
        d.refresh_terminal_form(fields, false);
        d.palette = Some(Palette {
            page,
            ..Palette::new()
        });
        d
    }

    /// The options a 120×40 screen actually shows.
    fn shown_options(d: &Dashboard) -> Vec<String> {
        let (_, _, body, _) = palette_geometry_for_dashboard(d, Rect::new(0, 0, 120, 40));
        let (lines, _, scroll) = d.palette_body(body);
        lines
            .iter()
            .skip(scroll)
            .take(usize::from(body.height))
            .map(|line| line.to_string())
            .filter(|line| line.starts_with("    agent-") || line.starts_with("  › agent-"))
            .collect()
    }

    #[test]
    fn open_pick_lists_show_every_option_that_fits_and_the_chosen_one() {
        // Eight agents and the first chosen: before, only that one showed.
        let d = agent_picker(8);
        let shown = shown_options(&d);
        assert_eq!(shown.len(), 8, "{shown:?}");
        assert_eq!(shown[0], "  › agent-00");
        // More than fit: the body's twelve rows hold the label, the value and
        // ten options, and the chosen one stays among them.
        let mut d = agent_picker(20);
        assert_eq!(shown_options(&d).len(), 10);
        if let Some(Palette {
            page: Page::Form { fields, active, .. },
            ..
        }) = d.palette.as_mut()
            && let FieldKind::Pick(list) = &mut fields[*active].kind
        {
            list.selected = 15;
        }
        let shown = shown_options(&d);
        assert_eq!(shown.len(), 10);
        assert!(shown.contains(&"  › agent-15".to_owned()), "{shown:?}");
    }

    #[test]
    fn keys_hint_matches_the_form_and_its_focused_field() {
        let d = dashboard();
        let keys = |command: Command, label: &str, recovering: bool| {
            let Page::Form { fields, .. } = d.palette_form(command.clone()) else {
                panic!()
            };
            let active = fields.iter().position(|f| f.label == label).unwrap();
            form_keys(&command, &fields, active, recovering, false)
        };
        assert_eq!(
            keys(Command::RegisterProject, "Repository", false),
            "↑/↓ highlight · Tab complete · Enter register · Esc cancel"
        );
        assert_eq!(
            keys(Command::RegisterProject, "Name", false),
            "Ctrl-u clear · Tab/Shift-Tab fields · Enter register · Esc cancel"
        );
        assert_eq!(
            keys(Command::RemoveWorkspace, "Workspace", false),
            "↑/↓/←/→ choose · type to filter · Enter continue · Esc cancel"
        );
        assert_eq!(
            keys(Command::CreateBranch, "Branch name", false),
            "Ctrl-u clear · Tab/Shift-Tab fields · Enter create · Esc cancel"
        );
        assert_eq!(
            keys(Command::CreateBranch, "Workspace options", false),
            "Space/←/→ switch · Tab/Shift-Tab fields · Enter create · Esc cancel"
        );
        assert_eq!(
            keys(Command::CreateTerminal, "Name", false),
            "Blank = automatic name · Ctrl-u clear · Tab/Shift-Tab fields · Enter create · Esc cancel"
        );
        // A refusal shows Esc and Ctrl-u on a line of its own.
        assert_eq!(
            keys(Command::CreateBranch, "Branch name", true),
            "Tab/Shift-Tab fields · Enter create"
        );
    }

    #[test]
    fn task_names_are_one_bounded_line_and_slugs_are_valid_branches() {
        assert_eq!(task_name("  Fix   the\tlogin  "), "Fix the login");
        let long = "word ".repeat(30);
        let name = task_name(&long);
        assert!(
            name.chars().count() <= 60 && !name.ends_with(' '),
            "{name:?}"
        );
        assert_eq!(task_name(&"界".repeat(70)).chars().count(), 60);
        for (task, slug) in [
            ("Fix the Login bug: v2!", "fix-the-login-bug-v2"),
            ("--force push", "force-push"),
            ("snake_case stays", "snake_case-stays"),
            ("../../etc .lock", "etc-lock"),
            ("修复 the bug", "the-bug"),
            ("修复", ""),
        ] {
            assert_eq!(task_slug(task), slug, "{task}");
            if !slug.is_empty() {
                assert_eq!(branch_name_problem(&format!("feature/{slug}")), None);
            }
        }
        let slug = task_slug(&"abc ".repeat(40));
        assert!(slug.len() <= 50 && !slug.ends_with('-'), "{slug}");
    }

    #[test]
    fn a_blank_task_retry_preserves_configured_claude_arguments_after_double_dash() {
        let request = CreateSessionRequest {
            project: "consigint".into(),
            workspace: ID.into(),
            name: "session".into(),
            argv: vec![
                "custom-claude".into(),
                "--".into(),
                "configured message".into(),
            ],
            label: Some("claude".into()),
            kind: SessionKind::Agent {
                name: "claude".into(),
            },
        };
        let mut palette = Palette::new();
        let remembered = ovrcr_protocol::CreateAgentSessionRequest {
            session: request.clone(),
            model_policy: ovrcr_protocol::AgentModelPolicy::HarnessDefault,
            model_binding: None,
            expected_model: None,
        };
        remember_launch(&mut palette, remembered.clone(), "");
        assert_eq!(palette.failed_launch, Some(remembered));
        palette.page = Page::Form {
            command: Command::RecoverLaunch,
            fields: vec![pick_field(
                "Recovery",
                "Retry".into(),
                vec![PickItem {
                    label: "Retry".into(),
                    value: "Retry".into(),
                }],
            )],
            active: 0,
            name_edited: false,
            root_edited: false,
        };

        let mut d = dashboard();
        d.palette = Some(palette);
        d.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT));
        let allocation = d
            .drain_outbox()
            .into_iter()
            .find(|message| message.request == Request::AllocateInitialPromptIntent)
            .expect("Retry must allocate a fresh intent");
        let launch = d.handle_server_message(ServerMessage::Response {
            request_id: allocation.request_id,
            response: Response::InitialPromptIntent(crate::protocol::InitialPromptIntentId {
                server_lifetime: "123e4567-e89b-12d3-a456-426614174000".into(),
                sequence: 1,
            }),
        });
        let Request::CreateAgentSessionWithPrompt(retry) = &launch[0].request else {
            panic!("Retry must submit the remembered launch through the new intent");
        };
        assert_eq!(retry.launch.session.argv, request.argv);
        assert_eq!(
            retry.launch.model_policy,
            ovrcr_protocol::AgentModelPolicy::HarnessDefault
        );
    }

    #[test]
    fn workspace_task_names_only_the_branch_and_prompt_is_separate() {
        let d = dashboard();
        let Page::Form { mut fields, .. } = d.palette_form(Command::CreateWorkspace) else {
            panic!()
        };
        assert_eq!(
            task_hint(&fields),
            "Names the branch; use Initial prompt for startup text"
        );
        assert!(matches!(
            fields
                .iter()
                .find(|field| field.label == "Initial prompt")
                .unwrap()
                .kind,
            FieldKind::Multiline
        ));
        assert!(
            fields
                .iter()
                .find(|field| field.label == "Initial prompt")
                .unwrap()
                .hidden
        );

        fields
            .iter_mut()
            .find(|field| field.label == "Start")
            .unwrap()
            .value = "Agent".into();
        d.refresh_terminal_form(&mut fields, false);
        assert!(
            !fields
                .iter()
                .find(|field| field.label == "Initial prompt")
                .unwrap()
                .hidden
        );
        assert!(
            !fields
                .iter()
                .find(|field| field.label == "Task")
                .unwrap()
                .hidden
        );
    }
}

#[cfg(test)]
mod close_confirm_tests {
    use super::*;
    use crate::protocol::{
        AgentActivity, ProjectSummary, SessionId, SessionKind, SessionPhase, SessionSummary,
        WorkspaceSummary,
    };
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    fn dashboard() -> Dashboard {
        let mut d = Dashboard::new(crate::session::TerminalSize {
            rows: 30,
            cols: 100,
        });
        d.hierarchy.projects.push(ProjectSummary {
            name: "demo".into(),
            workspaces: vec![WorkspaceSummary {
                project: "demo".into(),
                name: "root".into(),
                id: "root".into(),
                root: false,
                warning: None,
                path: "/tmp/unused".into(),
                sessions: vec![SessionSummary {
                    scope: ovrcr_protocol::SessionScope::Workspace,
                    archived: false,
                    cwd: "/work".into(),
                    id: SessionId(1),
                    run: crate::protocol::SessionRunId(1),
                    kind: SessionKind::Terminal,
                    recovery: None,
                    project: "demo".into(),
                    workspace: "root".into(),
                    name: "root-1".into(),
                    title: None,
                    manual_title: None,
                    assignment_control: None,
                    label: "shell".into(),
                    pid: None,
                    started_unix_ms: Some(0),
                    last_activity_unix_ms: None,
                    phase: SessionPhase::Running,
                    activity: AgentActivity::Unknown,
                    context_usage: None,
                    agent: None,
                    agent_epoch: 0,
                    unread: None,
                }],
            }],
        });
        d
    }

    fn screen() -> Rect {
        Rect::new(0, 0, 100, 30)
    }

    fn render(d: &Dashboard) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| super::super::render::draw_dashboard_at(frame, d, 0))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row_text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    fn find_label(buffer: &Buffer, y: u16, label: &str) -> u16 {
        let chars: Vec<char> = label.chars().collect();
        (0..buffer.area.width)
            .find(|x| {
                chars.iter().enumerate().all(|(offset, ch)| {
                    let Some(col) = x.checked_add(u16::try_from(offset).unwrap_or(u16::MAX)) else {
                        return false;
                    };
                    col < buffer.area.width && buffer[(col, y)].symbol() == ch.to_string()
                })
            })
            .unwrap_or_else(|| panic!("missing {label} on row {y}"))
    }

    fn peach_rows(buffer: &Buffer) -> Vec<u16> {
        (0..buffer.area.height)
            .filter(|y| {
                (0..buffer.area.width).any(|x| buffer[(x, *y)].bg == Color::Rgb(250, 179, 135))
            })
            .collect()
    }

    fn click(d: &mut Dashboard, column: u16, row: u16) -> DashboardAction {
        d.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            screen(),
        )
    }

    #[test]
    fn rail_height_tracks_measured_dialog_height() {
        let screen = Rect::new(2, 1, 100, 40);
        let short = close_confirm_layout(screen, 8);
        let tall = close_confirm_layout(screen, 12);
        // 8 body rows + action + top and bottom border.
        assert_eq!(short.dialog.height, 11);
        assert_eq!(short.rail.height, 9);
        assert_eq!(short.rail.width, CLOSE_CONFIRM_RAIL_WIDTH);
        assert_eq!(short.rail.x, short.dialog.x + 1);
        assert_eq!(short.rail.y, short.dialog.y + 1);
        // 12 body rows + action + borders.
        assert_eq!(tall.dialog.height, 15);
        assert_eq!(tall.rail.height, 13);
        assert_eq!(
            tall.rail.height - short.rail.height,
            tall.dialog.height - short.dialog.height
        );
        let clamped = close_confirm_layout(Rect::new(0, 0, 100, 10), 30);
        assert_eq!(clamped.dialog.height, 10);
        assert_eq!(clamped.rail.height, 8);
    }

    #[test]
    fn actions_are_right_aligned_cancel_then_close() {
        let layout = close_confirm_layout(Rect::new(0, 0, 100, 30), 8);
        assert_eq!(layout.cancel.width, CLOSE_CANCEL_LABEL.len() as u16);
        assert_eq!(layout.confirm.width, CLOSE_CONFIRM_LABEL.len() as u16);
        assert_eq!(layout.cancel.y, layout.confirm.y);
        assert_eq!(
            layout.cancel.x + layout.cancel.width + CLOSE_CONFIRM_ACTION_GAP,
            layout.confirm.x
        );
        assert_eq!(layout.confirm.right(), layout.text.right());
        assert!(layout.cancel.x >= layout.text.x);
        assert_eq!(close_confirm_action_line(), "[Esc Cancel]  [Enter Close]");
    }

    #[test]
    fn draw_and_hit_test_share_measured_geometry() {
        let mut d = dashboard();
        d.open_close_terminal_for(SessionId(1));
        let text = d.close_confirm_text(d.palette.as_ref().unwrap());
        let layout = measure_close_confirm(screen(), &text);
        let buffer = render(&d);
        let painted = peach_rows(&buffer);
        // Blank, identity, place, project, workspace, blank, consequence, blank, action.
        assert_eq!(painted.len(), 9);
        assert_eq!(layout.rail.height, 9);
        assert_eq!(painted[0], layout.rail.y);
        assert_eq!(*painted.last().unwrap(), layout.rail.bottom() - 1);
        for y in painted {
            let mut run = 0u16;
            for x in layout.rail.x..layout.rail.right() {
                assert_eq!(
                    buffer[(x, y)].bg,
                    Color::Rgb(250, 179, 135),
                    "rail cell {x},{y}"
                );
                run += 1;
            }
            assert_eq!(run, 2);
            assert_ne!(buffer[(layout.dialog.x, y)].bg, Color::Rgb(250, 179, 135));
        }
        assert_ne!(
            buffer[(layout.rail.x, layout.dialog.y)].bg,
            Color::Rgb(250, 179, 135),
            "the title border is not part of the rail"
        );
        let flat: String = (0..buffer.area.height)
            .map(|y| row_text(&buffer, y))
            .collect();
        for needle in [
            "Close terminal?",
            "root-1 (#1)",
            "demo / root / root-1",
            "Project: demo",
            "Workspace: root",
            CLOSE_CONSEQUENCE,
            "[Esc Cancel]",
            "[Enter Close]",
        ] {
            assert!(flat.contains(needle), "missing {needle}");
        }
        let cancel_at = find_label(&buffer, layout.cancel.y, "[Esc Cancel]");
        let close_at = find_label(&buffer, layout.confirm.y, "[Enter Close]");
        assert!(cancel_at < close_at);
        assert_eq!(cancel_at, layout.cancel.x);
        assert_eq!(close_at, layout.confirm.x);
        let identity_y = (0..buffer.area.height)
            .find(|y| row_text(&buffer, *y).contains("root-1 (#1)"))
            .unwrap();
        assert!(
            buffer[(layout.text.x, identity_y)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        let consequence_y = (0..buffer.area.height)
            .find(|y| row_text(&buffer, *y).contains(CLOSE_CONSEQUENCE))
            .unwrap();
        assert_eq!(
            buffer[(layout.text.x, consequence_y)].fg,
            Color::Rgb(250, 179, 135)
        );

        assert!(
            matches!(click(&mut d, 0, 0), DashboardAction::None),
            "outside clicks stay shielded"
        );
        assert!(d.palette.is_some());
        let action = click(&mut d, layout.confirm.x, layout.confirm.y);
        let Some(message) = ({ d.drain_one_request() }) else {
            panic!("confirm hit target must submit, got {action:?}");
        };
        assert_eq!(
            message.request,
            Request::CloseTerminal {
                session: SessionId(1),
                expected_run: crate::protocol::SessionRunId(1),
            }
        );

        let mut d = dashboard();
        d.open_close_terminal_for(SessionId(1));
        let layout =
            measure_close_confirm(screen(), &d.close_confirm_text(d.palette.as_ref().unwrap()));
        assert!(matches!(
            click(&mut d, layout.cancel.x, layout.cancel.y),
            DashboardAction::Redraw
        ));
        assert!(d.palette.is_none(), "cancel hit target dismisses");

        let mut d = dashboard();
        d.open_close_terminal_for(SessionId(1));
        d.palette_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(d.palette.is_none());
        d.open_close_terminal_for(SessionId(1));
        d.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(
            d.drain_one_request(),
            Some(crate::protocol::ClientMessage {
                request: Request::CloseTerminal { .. },
                ..
            })
        ));
    }

    #[test]
    fn other_confirms_do_not_use_the_accent_rail() {
        let mut d = dashboard();
        let mut stopped = d.hierarchy.projects[0].workspaces[0].sessions[0].clone();
        stopped.phase = SessionPhase::Stopped;
        d.hierarchy.projects[0].workspaces[0].sessions[0] = stopped;
        d.palette = Some(Palette {
            page: d.command_page(Command::ReopenTerminal(SessionId(1))),
            ..Palette::new()
        });
        let buffer = render(&d);
        assert!(peach_rows(&buffer).is_empty());
        let flat: String = (0..buffer.area.height)
            .map(|y| row_text(&buffer, y))
            .collect();
        assert!(flat.contains("[Confirm]"));
        assert!(flat.contains("[Cancel]"));
        assert!(!flat.contains("[Enter Close]"));
        assert!(!flat.contains("Close terminal?"));
    }
}

#[cfg(test)]
mod creation_form_visual_tests {
    use super::*;
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn new_agent_dashboard() -> Dashboard {
        let mut dashboard = Dashboard::new(crate::session::TerminalSize { rows: 24, cols: 80 });
        let shared = Field {
            label: "Shared files",
            value: "6 agents share these files".into(),
            required: false,
            hidden: false,
            edited: false,
            kind: FieldKind::Notice,
        };
        let prompt = Field {
            label: "Initial prompt",
            value: String::new(),
            required: false,
            hidden: false,
            edited: false,
            kind: FieldKind::Multiline,
        };
        let mut advanced = text_field("Advanced options", "collapsed".into(), false);
        advanced.kind = FieldKind::Toggle;
        let fields = vec![
            pick_field(
                "Branch",
                "feature/auth".into(),
                vec![
                    PickItem {
                        label: "feature/auth".into(),
                        value: "feature/auth".into(),
                    },
                    PickItem {
                        label: "fix/reconnect".into(),
                        value: "fix/reconnect".into(),
                    },
                ],
            ),
            notice_field(
                "Destination",
                "consigint / feature/auth · workspace ready".into(),
                false,
            ),
            shared,
            pick_field(
                "Agent harness",
                "Claude Code".into(),
                vec![PickItem {
                    label: "Claude Code".into(),
                    value: "Claude Code".into(),
                }],
            ),
            pick_field(
                "Model",
                HARNESS_DEFAULT_MODEL.into(),
                vec![PickItem {
                    label: "Harness default".into(),
                    value: HARNESS_DEFAULT_MODEL.into(),
                }],
            ),
            prompt,
            advanced,
            {
                let mut name = text_field("Name", String::new(), false);
                name.hidden = true;
                name
            },
        ];
        dashboard.palette = Some(Palette {
            page: Page::Form {
                command: Command::NewAgentSession,
                fields,
                active: 0,
                name_edited: false,
                root_edited: false,
            },
            ..Palette::new()
        });
        dashboard
    }

    fn new_agent_fields(dashboard: &Dashboard) -> (&[Field], usize) {
        let Page::Form {
            command: Command::NewAgentSession,
            fields,
            active,
            ..
        } = &dashboard.palette.as_ref().unwrap().page
        else {
            panic!("new agent session form")
        };
        (fields, *active)
    }

    fn branch_dashboard() -> Dashboard {
        let mut dashboard = Dashboard::new(crate::session::TerminalSize { rows: 24, cols: 80 });
        let mut project = pick_field(
            "Project",
            "consigint".into(),
            vec![PickItem {
                label: "consigint".into(),
                value: "consigint".into(),
            }],
        );
        project.required = true;
        let mut options = text_field("Workspace options", "collapsed".into(), false);
        options.kind = FieldKind::Toggle;
        let mut base = text_field("Base", "main".into(), true);
        base.hidden = true;
        dashboard.palette = Some(Palette {
            page: Page::Form {
                command: Command::CreateBranch,
                fields: vec![
                    project,
                    text_field("Branch name", String::new(), true),
                    options,
                    base,
                ],
                active: 1,
                name_edited: false,
                root_edited: false,
            },
            ..Palette::new()
        });
        dashboard
    }

    fn render(dashboard: &Dashboard, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| super::super::render::draw_dashboard_at(frame, dashboard, 0))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    fn find_label(buffer: &Buffer, label: &str) -> (u16, u16) {
        let chars: Vec<char> = label.chars().collect();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if chars.iter().enumerate().all(|(offset, ch)| {
                    let col = x.saturating_add(u16::try_from(offset).unwrap());
                    col < buffer.area.width && buffer[(col, y)].symbol() == ch.to_string()
                }) {
                    return (x, y);
                }
            }
        }
        panic!("missing rendered label {label}");
    }

    fn click(dashboard: &mut Dashboard, column: u16, row: u16, width: u16, height: u16) {
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, width, height),
        );
    }

    #[test]
    fn paired_creation_surfaces_keep_a_distinct_background_gutter() {
        let line = creation_pair_surfaces(
            PairedFieldSurface {
                text: "Claude Code",
                active: true,
                placeholder: false,
                width: 20,
            },
            2,
            PairedFieldSurface {
                text: "Harness default",
                active: false,
                placeholder: false,
                width: 20,
            },
        );
        assert_eq!(line.spans[3].content, "  ");
        assert_eq!(line.spans[3].style.bg, Some(CRUST()));
        assert_ne!(line.spans[3].style.bg, line.spans[0].style.bg);
        assert_ne!(line.spans[3].style.bg, line.spans[4].style.bg);
    }

    #[test]
    fn anchored_menu_events_keep_a_manually_scrolled_visible_form() {
        let mut dashboard = new_agent_dashboard();
        let area = Rect::new(0, 0, 80, 24);
        dashboard.install_area(area);
        dashboard.palette.as_mut().unwrap().model_status =
            Some("Model choices are loading.".into());
        if let Page::Form { fields, .. } = &mut dashboard.palette.as_mut().unwrap().page {
            fields
                .iter_mut()
                .find(|field| field.label == "Initial prompt")
                .unwrap()
                .value = "first prompt line\nsecond prompt line\nthird prompt line".into();
        }
        // Enter the paired picker through the form's real focus path, then
        // create a nonzero viewport through the mouse wheel.
        dashboard.palette_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            new_agent_fields(&dashboard).0[new_agent_fields(&dashboard).1].label,
            "Agent harness"
        );
        let (_, _, body, _) = palette_geometry_for_dashboard(&dashboard, area);
        let (lines, _, _) = dashboard.palette_body(body);
        assert!(
            lines.len() > usize::from(body.height),
            "fixture must scroll"
        );
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: body.x,
                row: body.y,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        let closed = form_geometry(&base_form_snapshot(&dashboard, 80, 24));
        assert!(
            closed.5 > 0,
            "fixture must exercise a real scrolled viewport"
        );

        dashboard.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(dashboard.palette.as_ref().unwrap().new_agent_picker_open);
        assert_eq!(
            form_geometry(&base_form_snapshot(&dashboard, 80, 24)),
            closed
        );
        dashboard.palette_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(
            form_geometry(&base_form_snapshot(&dashboard, 80, 24)),
            closed
        );

        let buffer = render(&dashboard, 80, 24);
        let harness = find_label(&buffer, "Claude Code");
        click(&mut dashboard, harness.0, harness.1, 80, 24);
        assert_eq!(
            form_geometry(&base_form_snapshot(&dashboard, 80, 24)),
            closed
        );
        let buffer = render(&dashboard, 80, 24);
        let model = find_label(&buffer, "Harness default");
        click(&mut dashboard, model.0, model.1, 80, 24);
        assert!(dashboard.palette.as_ref().unwrap().new_agent_picker_open);
        assert_eq!(
            new_agent_fields(&dashboard).0[new_agent_fields(&dashboard).1].label,
            "Model"
        );
        assert_eq!(
            form_geometry(&base_form_snapshot(&dashboard, 80, 24)),
            closed
        );

        let buffer = render(&dashboard, 80, 24);
        let prompt = find_label(&buffer, "Prompt (optional)");
        click(&mut dashboard, prompt.0 + 9, prompt.1 + 2, 80, 24);
        assert!(!dashboard.palette.as_ref().unwrap().new_agent_picker_open);
        assert_eq!(
            new_agent_fields(&dashboard).0[new_agent_fields(&dashboard).1].label,
            "Initial prompt"
        );
        let prompt_value = new_agent_fields(&dashboard)
            .0
            .iter()
            .find(|field| field.label == "Initial prompt")
            .unwrap()
            .value
            .as_str();
        assert!(
            dashboard
                .palette
                .as_ref()
                .unwrap()
                .cursor
                .position(prompt_value)
                > "first prompt line\n".len(),
            "clicking the visible prompt text must place its insertion cursor"
        );
        assert_eq!(
            form_geometry(&base_form_snapshot(&dashboard, 80, 24)),
            closed
        );
    }

    #[test]
    fn anchored_menu_reveals_an_offscreen_keyboard_anchor() {
        let mut dashboard = new_agent_dashboard();
        dashboard.palette.as_mut().unwrap().model_status = Some("Loading models".into());
        dashboard.palette.as_mut().unwrap().scroll = Some(3);
        assert!(base_form_snapshot(&dashboard, 80, 24).6 > 0);
        dashboard.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let (_, popup, _, rows) = creation_popup_snapshot(&dashboard, 80, 24);
        assert!(popup.height >= 3);
        assert!(
            !rows.is_empty(),
            "keyboard opening must show selectable choices"
        );
    }

    #[test]
    fn anchored_child_menu_keeps_refusal_and_controls_visible_at_the_floor() {
        let mut dashboard = branch_dashboard();
        let area = Rect::new(0, 0, 60, 18);
        dashboard.install_area(area);
        dashboard
            .palette
            .as_mut()
            .unwrap()
            .suggestions
            .cache
            .insert(
                "consigint".into(),
                Ok(super::super::git_hints::Hints {
                    branches: vec!["main".into()],
                    base: "main".into(),
                }),
            );
        dashboard.palette_paste("bad..branch");
        dashboard.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let invalid = dashboard
            .palette
            .as_ref()
            .unwrap()
            .invalid
            .as_ref()
            .expect("real branch validation refusal")
            .1
            .clone();
        let project = find_label(&render(&dashboard, 60, 18), "consigint");
        click(&mut dashboard, project.0, project.1, 60, 18);
        assert!(dashboard.palette.as_ref().unwrap().new_agent_picker_open);
        let buffer = render(&dashboard, 60, 18);
        let visible = (0..18)
            .map(|y| row(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains(&invalid), "{visible}");
        for hint in ["Enter accept", "Esc close picker", "Tab fields"] {
            assert!(visible.contains(hint), "missing {hint}:\n{visible}");
        }

        dashboard.palette_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(dashboard.palette.is_some());
        assert!(!dashboard.palette.as_ref().unwrap().new_agent_picker_open);

        let mut refused = branch_dashboard();
        refused.install_area(area);
        refused.palette.as_mut().unwrap().suggestions.cache.insert(
            "consigint".into(),
            Ok(super::super::git_hints::Hints {
                branches: vec!["main".into()],
                base: "main".into(),
            }),
        );
        refused.palette_paste("feature/already-used");
        refused.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let request_id = refused.palette.as_ref().unwrap().pending.unwrap();
        assert!(
            refused
                .palette_response(
                    request_id,
                    &Response::Error {
                        code: crate::protocol::ErrorCode::AlreadyExists,
                        message: "branch already exists".into(),
                    },
                )
                .is_some()
        );
        let refusal = refused
            .palette
            .as_ref()
            .unwrap()
            .error
            .as_deref()
            .expect("non-partial branch refusal")
            .to_owned();
        let project = find_label(&render(&refused, 60, 18), "consigint");
        click(&mut refused, project.0, project.1, 60, 18);
        let buffer = render(&refused, 60, 18);
        let visible = (0..18)
            .map(|y| row(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains(&refusal), "{visible}");
        for hint in ["Enter accept", "Esc close picker", "Tab fields"] {
            assert!(visible.contains(hint), "missing {hint}:\n{visible}");
        }
    }

    #[test]
    fn branch_refusal_shows_git_cause_instead_of_worktree_progress() {
        let mut dashboard = branch_dashboard();
        dashboard.install_area(Rect::new(0, 0, 60, 18));
        dashboard
            .palette
            .as_mut()
            .unwrap()
            .suggestions
            .cache
            .insert(
                "consigint".into(),
                Ok(super::super::git_hints::Hints {
                    branches: vec!["main".into()],
                    base: "main".into(),
                }),
            );
        dashboard.palette_paste("feature/child-kept");
        dashboard.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let request_id = dashboard.palette.as_ref().unwrap().pending.unwrap();
        dashboard.palette_response(
            request_id,
            &Response::Error {
                code: crate::protocol::ErrorCode::Conflict,
                message: "git command failed (exit status: 255): Preparing worktree (new branch 'feature/child-kept')\nfatal: a branch named 'feature/child-kept' already exists".into(),
            },
        );
        let buffer = render(&dashboard, 60, 18);
        let visible = (0..18)
            .map(|y| row(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains("already exists"), "{visible}");
        assert!(!visible.contains("Preparing worktree"), "{visible}");
        assert!(visible.contains("Esc cancel"), "{visible}");
    }

    #[test]
    fn open_creation_picker_keeps_dialog_and_form_geometry_fixed() {
        for (width, height) in [(80, 24), (60, 18)] {
            for label in ["Branch", "Agent harness", "Model"] {
                let mut dashboard = new_agent_dashboard();
                let prompt_text = (0..10)
                    .map(|line| format!("Prompt line {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let palette = dashboard.palette.as_mut().unwrap();
                let Page::Form { fields, active, .. } = &mut palette.page else {
                    unreachable!()
                };
                fields
                    .iter_mut()
                    .find(|field| field.label == "Initial prompt")
                    .unwrap()
                    .value = prompt_text.clone();
                *active = fields
                    .iter()
                    .position(|field| field.label == label)
                    .unwrap();
                palette.scroll = None;
                let closed = base_form_snapshot(&dashboard, width, height);

                dashboard.palette.as_mut().unwrap().new_agent_picker_open = true;
                let open = base_form_snapshot(&dashboard, width, height);
                assert_eq!(
                    open, closed,
                    "opening {label} choices must only add an overlay at {width}x{height}"
                );
                assert_eq!(
                    new_agent_fields(&dashboard)
                        .0
                        .iter()
                        .find(|field| field.label == "Initial prompt")
                        .unwrap()
                        .value,
                    prompt_text,
                    "the popup must preserve the complete prompt draft"
                );
            }
        }
    }

    #[test]
    fn anchored_popup_flips_and_bounds_filtered_long_lists() {
        let mut dashboard = new_agent_dashboard();
        let active_index;
        {
            let palette = dashboard.palette.as_mut().unwrap();
            let Page::Form { fields, active, .. } = &mut palette.page else {
                unreachable!()
            };
            active_index = fields
                .iter()
                .position(|field| field.label == "Branch")
                .unwrap();
            *active = active_index;
            let branch = fields
                .iter_mut()
                .find(|field| field.label == "Branch")
                .unwrap();
            let FieldKind::Pick(list) = &mut branch.kind else {
                unreachable!()
            };
            list.items = (0..40)
                .map(|index| PickItem {
                    label: format!("workspace-{index:02}"),
                    value: format!("workspace-{index:02}"),
                })
                .collect();
            list.query = "workspace-2".into();
            list.selected = 10;
            palette.new_agent_picker_open = true;
        }
        let palette = dashboard.palette.as_ref().unwrap();
        let body = Rect::new(10, 10, 40, 10);
        let targets = vec![(8, PaletteTarget::Field(active_index, true))];
        let popup = creation_picker_popup(body, palette, &targets, 0).unwrap();
        let anchor_y = body.y + 8;
        assert_eq!(
            popup.area.bottom(),
            anchor_y,
            "a menu near the bottom should flip above its anchor"
        );
        assert!(rect_contains_rect(body, popup.area));
        assert!(popup.area.width <= body.width);
        assert!(popup.lines.len() <= usize::from(popup.area.height - 2));

        let Page::Form { fields, active, .. } = &palette.page else {
            unreachable!()
        };
        let FieldKind::Pick(list) = &fields[*active].kind else {
            unreachable!()
        };
        let filtered = list.filtered();
        assert!(
            popup
                .option_rows
                .iter()
                .any(|(_, index)| *index == list.selected),
            "the selected filtered option must remain in the bounded viewport"
        );
        for (row, index) in &popup.option_rows {
            assert!(popup.area.y < *row && *row < popup.area.bottom());
            assert!(filtered.get(*index).is_some());
        }
        assert!(filtered.len() > popup.option_rows.len());

        let mut dashboard = new_agent_dashboard();
        let model_index = new_agent_fields(&dashboard)
            .0
            .iter()
            .position(|field| field.label == "Model")
            .unwrap();
        {
            let palette = dashboard.palette.as_mut().unwrap();
            let Page::Form { fields, active, .. } = &mut palette.page else {
                unreachable!()
            };
            let model = &mut fields[model_index];
            let FieldKind::Pick(list) = &mut model.kind else {
                unreachable!()
            };
            list.items = vec![PickItem {
                label: "a model choice whose name is wider than its field surface".into(),
                value: "wide-model".into(),
            }];
            *active = model_index;
            palette.new_agent_picker_open = true;
        }
        let palette = dashboard.palette.as_ref().unwrap();
        let body = Rect::new(10, 10, 40, 10);
        let (left, right) = match &palette.page {
            Page::Form { fields, .. } => (
                fields
                    .iter()
                    .position(|field| field.label == "Agent harness")
                    .unwrap(),
                fields
                    .iter()
                    .position(|field| field.label == "Model")
                    .unwrap(),
            ),
            _ => unreachable!(),
        };
        let targets = vec![(8, PaletteTarget::FieldPair(left, right, true))];
        let popup = creation_picker_popup(body, palette, &targets, 0).unwrap();
        assert_eq!(popup.area.right(), body.right());
        assert!(rect_contains_rect(body, popup.area));
    }

    #[test]
    fn mouse_switches_visible_picker_and_outside_click_is_consumed() {
        let mut dashboard = new_agent_dashboard();
        let Page::Form { fields, active, .. } = &mut dashboard.palette.as_mut().unwrap().page
        else {
            unreachable!()
        };
        *active = fields
            .iter()
            .position(|field| field.label == "Agent harness")
            .unwrap();
        dashboard.palette.as_mut().unwrap().new_agent_picker_open = true;
        let area = Rect::new(0, 0, 80, 24);
        let (body, popup, _, _) = creation_popup_snapshot(&dashboard, 80, 24);
        let (_, targets, scroll) = dashboard.palette_body(body);
        let branch_index = new_agent_fields(&dashboard)
            .0
            .iter()
            .position(|field| field.label == "Branch")
            .unwrap();
        let branch_row = targets
            .iter()
            .find_map(|(row, target)| {
                matches!(target, PaletteTarget::Field(index, true) if *index == branch_index)
                    .then_some(*row)
            })
            .unwrap();
        let branch_y = body.y + (branch_row - scroll) as u16;
        assert!(branch_y < popup.y || branch_y >= popup.bottom());
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: body.x + 2,
                row: branch_y,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert_eq!(new_agent_fields(&dashboard).1, branch_index);
        assert!(dashboard.palette.as_ref().unwrap().new_agent_picker_open);
        assert!(
            !creation_popup_snapshot(&dashboard, 80, 24).3.is_empty(),
            "switching fields keeps the menu open"
        );

        let (body, _, _, _) = creation_popup_snapshot(&dashboard, 80, 24);
        let (_, targets, scroll) = dashboard.palette_body(body);
        let branch_row = targets
            .iter()
            .find_map(|(row, target)| {
                matches!(target, PaletteTarget::Field(index, true) if *index == branch_index)
                    .then_some(*row)
            })
            .unwrap();
        dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: body.x + 2,
                row: body.y + (branch_row - scroll) as u16,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert!(!dashboard.palette.as_ref().unwrap().new_agent_picker_open);
        dashboard.key(KeyCode::Enter);
        assert!(dashboard.palette.as_ref().unwrap().new_agent_picker_open);

        let action = dashboard.mouse_action(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
        assert_eq!(action, DashboardAction::Redraw);
        assert!(
            dashboard.palette.is_some(),
            "outside click keeps the form open"
        );
        assert!(!dashboard.palette.as_ref().unwrap().new_agent_picker_open);
        assert!(dashboard.drain_outbox().iter().all(|message| !matches!(
            message.request,
            Request::CreateAgentSession(_)
                | Request::CreateAgentSessionWithPrompt(_)
                | Request::AllocateInitialPromptIntent
        )));
    }

    #[test]
    fn invalid_open_picker_footer_names_accept_and_first_escape_truthfully() {
        let mut dashboard = new_agent_dashboard();
        let palette = dashboard.palette.as_mut().unwrap();
        palette.invalid = Some((0, "Choose a registered workspace".into()));
        palette.new_agent_picker_open = true;

        let buffer = render(&dashboard, 60, 18);
        let visible = (0..buffer.area.height)
            .map(|y| row(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains("Enter accept"), "{visible}");
        assert!(visible.contains("Esc close picker"), "{visible}");
        assert!(
            !visible.contains("Esc cancel · Ctrl-u clear"),
            "the first Escape must be documented as closing the picker:\n{visible}"
        );
    }

    #[test]
    fn supported_floor_keeps_all_new_agent_fields_and_actions_visible() {
        let mut dashboard = new_agent_dashboard();
        let palette = dashboard.palette.as_mut().unwrap();
        palette.model_status = Some("Model choices are loading.".into());
        palette.invalid = Some((4, "Model lookup failed".into()));
        let buffer = render(&dashboard, 60, 18);
        let branch = find_label(&buffer, "Branch");
        let harness = find_label(&buffer, "Agent harness");
        let model = find_label(&buffer, "Model");
        let prompt = find_label(&buffer, "Prompt (optional)");
        let advanced = find_label(&buffer, "Advanced options");
        let start = find_label(&buffer, "[Start agent]");
        let cancel = find_label(&buffer, "[Cancel]");

        assert!(branch.1 < harness.1);
        assert!(harness.1 < model.1);
        assert!(model.1 < prompt.1);
        assert!(prompt.1 < advanced.1);
        assert!(advanced.1 < start.1);
        assert_eq!(start.1, cancel.1);
        let visible = (0..buffer.area.height)
            .map(|y| row(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains("6 agents share these files"), "{visible}");
        assert!(visible.contains("Model choices are loading."), "{visible}");
        assert!(visible.contains("Model lookup failed"), "{visible}");
    }

    #[test]
    fn wide_new_agent_pairs_harness_and_model_and_mouse_targets_the_clicked_half() {
        let mut dashboard = new_agent_dashboard();
        let Page::Form { fields, .. } = &mut dashboard.palette.as_mut().unwrap().page else {
            unreachable!()
        };
        let model = fields
            .iter_mut()
            .find(|field| field.label == "Model")
            .unwrap();
        let FieldKind::Pick(list) = &mut model.kind else {
            unreachable!()
        };
        list.items = vec![
            PickItem {
                label: "Harness default".into(),
                value: HARNESS_DEFAULT_MODEL.into(),
            },
            PickItem {
                label: "Claude Sonnet 4".into(),
                value: "claude-sonnet-4".into(),
            },
        ];
        list.select_value(HARNESS_DEFAULT_MODEL);
        let buffer = render(&dashboard, 80, 24);
        let harness = find_label(&buffer, "Agent harness");
        let model = find_label(&buffer, "Model");
        assert_eq!(harness.1, model.1, "fields should share the label row");
        assert!(harness.0 < model.0, "model should occupy the right column");

        let harness_value = find_label(&buffer, "Claude Code");
        click(&mut dashboard, harness_value.0, harness_value.1, 80, 24);
        let (fields, active) = new_agent_fields(&dashboard);
        assert_eq!(fields[active].label, "Agent harness");
        assert!(dashboard.palette.as_ref().unwrap().new_agent_picker_open);
        dashboard.key(KeyCode::Esc);

        let model_value = find_label(&buffer, "Harness default");
        assert_eq!(model_value.1, model.1 + 1);
        let closed = base_form_snapshot(&dashboard, 80, 24);
        click(&mut dashboard, model_value.0, model_value.1, 80, 24);
        let Page::Form { fields, active, .. } = &dashboard.palette.as_ref().unwrap().page else {
            panic!("New agent session form should remain active");
        };
        assert_eq!(fields[*active].label, "Model");
        assert!(dashboard.palette.as_ref().unwrap().new_agent_picker_open);
        assert_eq!(
            form_geometry(&base_form_snapshot(&dashboard, 80, 24)),
            form_geometry(&closed),
            "opening the Model menu must keep the paired form in place"
        );
        let (body, popup, _, _) = creation_popup_snapshot(&dashboard, 80, 24);
        let (left_width, gap, _) = pair_columns(body);
        assert_eq!(popup.x, body.x + left_width + gap);

        let model_option = new_agent_fields(&dashboard)
            .0
            .iter()
            .find(|field| field.label == "Model")
            .and_then(|field| match &field.kind {
                FieldKind::Pick(list) => list
                    .filtered()
                    .iter()
                    .position(|item| item.value == "claude-sonnet-4"),
                _ => None,
            })
            .unwrap();
        let (_, popup, popup_lines, option_rows) = creation_popup_snapshot(&dashboard, 80, 24);
        let option_row = option_rows
            .iter()
            .find_map(|(row, option)| (*option == model_option).then_some(*row))
            .unwrap();
        assert!(
            popup_lines
                .iter()
                .any(|line| line.contains("Claude Sonnet 4"))
        );
        click(&mut dashboard, popup.x + 2, option_row, 80, 24);

        let (fields, active) = new_agent_fields(&dashboard);
        let model = fields.iter().find(|field| field.label == "Model").unwrap();
        assert_eq!(fields[active].label, "Model");
        assert_eq!(model.value, "claude-sonnet-4");
        assert!(model.edited);
        assert!(!dashboard.palette.as_ref().unwrap().new_agent_picker_open);
        assert!(dashboard.drain_outbox().iter().all(|message| !matches!(
            message.request,
            Request::CreateAgentSession(_)
                | Request::CreateAgentSessionWithPrompt(_)
                | Request::AllocateInitialPromptIntent
        )));
    }

    #[test]
    fn floor_picker_overlays_stable_form_with_shared_and_model_status() {
        let mut dashboard = new_agent_dashboard();
        let Some(palette) = dashboard.palette.as_mut() else {
            unreachable!()
        };
        palette.new_agent_picker_open = true;
        palette.model_status = Some("Model choices are loading.".into());
        palette.invalid = Some((4, "Model lookup failed".into()));
        let open = base_form_snapshot(&dashboard, 60, 18);
        let closed = base_form_snapshot_closed_state(&mut dashboard, 60, 18);
        assert_eq!(open, closed, "the 60×18 picker must preserve the form");
        let base_text = open.4.join("\n");
        for text in [
            "6 agents share these files",
            "Model choices are loading.",
            "Model lookup failed",
            "Prompt (optional)",
            "Advanced options",
        ] {
            assert!(
                base_text.contains(text),
                "base form lost {text}:\n{base_text}"
            );
        }
        let (body, popup, popup_lines, option_rows) = creation_popup_snapshot(&dashboard, 60, 18);
        assert!(rect_contains_rect(body, popup));
        assert!(!option_rows.is_empty());
        assert!(popup_lines.iter().any(|line| line.contains("feature/auth")));
        let buffer = render(&dashboard, 60, 18);
        let visible = (0..buffer.area.height)
            .map(|y| row(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains("[Start agent]"), "{visible}");
        assert!(visible.contains("[Cancel]"), "{visible}");
    }

    #[test]
    fn open_picker_bounds_a_long_prompt_without_discarding_its_draft() {
        let mut dashboard = new_agent_dashboard();
        let prompt = (1..=10)
            .map(|line| format!("draft line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let Some(palette) = dashboard.palette.as_mut() else {
            unreachable!()
        };
        let Page::Form { fields, .. } = &mut palette.page else {
            unreachable!()
        };
        fields
            .iter_mut()
            .find(|field| field.label == "Initial prompt")
            .unwrap()
            .value = prompt.clone();
        palette.new_agent_picker_open = true;

        let open = base_form_snapshot(&dashboard, 60, 18);
        let closed = base_form_snapshot_closed_state(&mut dashboard, 60, 18);
        assert_eq!(
            open, closed,
            "the overlay must not expand the prompt viewport"
        );
        let Page::Form { fields, .. } = &dashboard.palette.as_ref().unwrap().page else {
            unreachable!()
        };
        let stored_prompt = &fields
            .iter()
            .find(|field| field.label == "Initial prompt")
            .unwrap()
            .value;

        assert!(open.4.join("\n").contains("draft line 1"));
        assert!(
            !open.4.join("\n").contains("draft line 10"),
            "prompt rows should stay bounded:\n{}",
            open.4.join("\n")
        );
        assert_eq!(
            stored_prompt, &prompt,
            "rendering must preserve the full draft"
        );
    }

    #[test]
    fn long_prompt_view_tracks_its_cursor_and_mouse_clicks_keep_logical_lines() {
        let mut dashboard = new_agent_dashboard();
        let prompt = (1..=10)
            .map(|line| format!("draft line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let Some(palette) = dashboard.palette.as_mut() else {
            unreachable!()
        };
        let Page::Form { fields, active, .. } = &mut palette.page else {
            unreachable!()
        };
        let prompt_index = fields
            .iter()
            .position(|field| field.label == "Initial prompt")
            .unwrap();
        fields[prompt_index].value = prompt.clone();
        *active = prompt_index;
        palette.cursor.click_line(&prompt, 9, 46, usize::MAX);

        let buffer = render(&dashboard, 80, 24);
        let visible = (0..buffer.area.height)
            .map(|y| row(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(visible.contains("draft line 8"), "{visible}");
        assert!(visible.contains("draft line 9"), "{visible}");
        assert!(visible.contains("draft line 10"), "{visible}");
        assert!(!visible.contains("draft line 1▏"), "{visible}");

        let (x, y) = find_label(&buffer, "draft line 9");
        click(&mut dashboard, x + "draft line 9".len() as u16, y, 80, 24);
        dashboard.palette_paste("!");
        let Page::Form { fields, .. } = &dashboard.palette.as_ref().unwrap().page else {
            unreachable!()
        };
        let edited = &fields[prompt_index].value;
        assert_eq!(edited.lines().nth(8), Some("draft line 9!"));
        assert_eq!(edited.lines().count(), 10);
    }

    #[test]
    fn floor_picker_mouse_accepts_the_option_that_was_clicked() {
        let mut dashboard = new_agent_dashboard();
        dashboard.palette.as_mut().unwrap().new_agent_picker_open = true;
        let buffer = render(&dashboard, 60, 18);
        let mut option = None;
        for y in 0..buffer.area.height {
            let line = row(&buffer, y);
            if let Some(x) = line.find("fix/reconnect") {
                option = Some((u16::try_from(x).unwrap(), y));
                break;
            }
        }
        let (x, y) = option.expect("the second visible branch should have a mouse target");
        click(&mut dashboard, x, y, 60, 18);
        let Page::Form { fields, active, .. } = &dashboard.palette.as_ref().unwrap().page else {
            panic!("accepting a branch should keep the session form open");
        };
        assert_eq!(fields[*active].label, "Branch");
        assert_eq!(fields[*active].value, "fix/reconnect");
    }

    #[test]
    fn creation_forms_use_theme_surfaces_and_emphasize_the_primary_action() {
        let session = new_agent_dashboard();
        let buffer = render(&session, 80, 24);
        let prompt = find_label(&buffer, "Prompt (optional)");
        let start = find_label(&buffer, "[Start agent]");
        for row in 1..=3 {
            assert_eq!(
                buffer[(prompt.0, prompt.1 + row)].bg,
                crate::theme::SURFACE0(),
                "the wide prompt area should have three visible rows"
            );
        }
        assert_eq!(buffer[(start.0, start.1)].bg, crate::theme::MAUVE());

        let branch = branch_dashboard();
        let buffer = render(&branch, 80, 24);
        let name = find_label(&buffer, "Branch name");
        let create = find_label(&buffer, "[Create branch]");
        assert_eq!(buffer[(name.0, name.1 + 1)].bg, crate::theme::SURFACE2());
        assert_eq!(buffer[(create.0, create.1)].bg, crate::theme::MAUVE());
    }

    #[test]
    fn disclosure_and_prompt_surfaces_keep_their_mouse_actions() {
        let mut branch = branch_dashboard();
        let buffer = render(&branch, 80, 24);
        let (options_x, options_y) = find_label(&buffer, "Workspace options");
        click(&mut branch, options_x, options_y, 80, 24);
        let buffer = render(&branch, 80, 24);
        assert!(find_label(&buffer, "Base").1 > options_y);

        let mut session = new_agent_dashboard();
        let buffer = render(&session, 80, 24);
        let (prompt_x, prompt_y) = find_label(&buffer, "What should the agent work on?");
        click(&mut session, prompt_x, prompt_y, 80, 24);
        session.palette_paste("draft from mouse");
        let Page::Form { fields, .. } = &session.palette.as_ref().unwrap().page else {
            panic!("New agent session form should remain active");
        };
        assert_eq!(
            fields
                .iter()
                .find(|field| field.label == "Initial prompt")
                .unwrap()
                .value,
            "draft from mouse"
        );

        let mut padded = new_agent_dashboard();
        let buffer = render(&padded, 80, 24);
        let (prompt_x, prompt_y) = find_label(&buffer, "Prompt (optional)");
        click(&mut padded, prompt_x, prompt_y + 3, 80, 24);
        padded.palette_paste("padded row click");
        let Page::Form { fields, .. } = &padded.palette.as_ref().unwrap().page else {
            panic!("a padded prompt row should remain editable");
        };
        assert_eq!(
            fields
                .iter()
                .find(|field| field.label == "Initial prompt")
                .unwrap()
                .value,
            "padded row click"
        );
    }
}

#[cfg(test)]
mod initial_prompt_tests {
    use super::*;
    use crate::protocol::{
        AgentLaunchOrigin, AgentModelPolicy, CreateAgentSessionRequest, InitialPromptIntentId,
        InitialPromptIntentSnapshot, InitialPromptIntentState, InitialPromptLaunchRequest,
        ProjectSummary, ServerMessage, WorkspaceSummary,
    };
    use std::ffi::OsString;

    fn make_dashboard(preset: &str, origin: AgentLaunchOrigin) -> Dashboard {
        let mut dashboard = Dashboard::new(crate::session::TerminalSize {
            rows: 30,
            cols: 100,
        });
        let argv = vec![OsString::from(format!("/fixture/{preset}"))];
        dashboard.model_agent_entries = Some(vec![super::super::agents::AgentEntry {
            name: preset.into(),
            argv,
            source: AgentSource::Detected,
            origin,
        }]);
        dashboard.hierarchy.projects.push(ProjectSummary {
            name: "demo".into(),
            workspaces: vec![WorkspaceSummary {
                project: "demo".into(),
                name: "feature".into(),
                id: "feature".into(),
                root: false,
                warning: None,
                path: "/tmp/feature".into(),
                sessions: Vec::new(),
            }],
        });
        dashboard
    }

    fn start_palette(dashboard: &Dashboard, prompt: &str) -> Palette {
        let mut palette = Palette::new();
        palette.page = dashboard.palette_form(Command::CreateTerminal);
        let Page::Form { fields, active, .. } = &mut palette.page else {
            panic!("expected form")
        };
        fields
            .iter_mut()
            .find(|field| field.label == "Start")
            .unwrap()
            .value = "Agent".into();
        dashboard.refresh_terminal_form(fields, false);
        let prompt_index = fields
            .iter()
            .position(|field| field.label == "Initial prompt")
            .unwrap();
        fields[prompt_index].value = prompt.into();
        *active = prompt_index;
        palette
    }

    fn prompt_launch(preset: &str, argv: Vec<OsString>) -> CreateAgentSessionRequest {
        CreateAgentSessionRequest {
            session: CreateSessionRequest {
                project: "demo".into(),
                workspace: "feature".into(),
                name: "fixed-session-name".into(),
                label: Some(preset.into()),
                argv,
                kind: SessionKind::Agent {
                    name: preset.into(),
                },
            },
            model_policy: AgentModelPolicy::HarnessDefault,
            model_binding: None,
            expected_model: None,
        }
    }

    fn intent(sequence: u64) -> InitialPromptIntentId {
        InitialPromptIntentId {
            server_lifetime: "123e4567-e89b-12d3-a456-426614174000".into(),
            sequence,
        }
    }

    #[test]
    fn multiline_enter_and_paste_preserve_draft_then_alt_enter_allocates_once() {
        let mut dashboard = make_dashboard("claude", AgentLaunchOrigin::Detected);
        dashboard.palette = Some(start_palette(&dashboard, ""));
        dashboard.palette_key_inner(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), true);
        dashboard.palette_paste("  exact prompt\r\n界e\u{301}  ");
        let palette = dashboard.palette.as_ref().unwrap();
        let Page::Form { fields, active, .. } = &palette.page else {
            panic!()
        };
        assert_eq!(fields[*active].value, "\n  exact prompt\n界e\u{301}  ");

        dashboard.palette_key_inner(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT), true);
        let messages = dashboard.drain_outbox();
        let allocation_request = messages
            .iter()
            .find(|message| message.request == Request::AllocateInitialPromptIntent)
            .expect("submit allocates one initial prompt intent");
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.request == Request::AllocateInitialPromptIntent)
                .count(),
            1
        );
        let allocation = dashboard
            .pending_initial_prompt_allocation
            .as_ref()
            .unwrap();
        assert_eq!(allocation.prompt, "\n  exact prompt\n界e\u{301}  ");
        let launch = allocation.launch.clone();

        let outbox = dashboard.handle_server_message(ServerMessage::Response {
            request_id: allocation_request.request_id,
            response: Response::InitialPromptIntent(intent(1)),
        });
        assert_eq!(outbox.len(), 1);
        assert_eq!(
            outbox[0].request,
            Request::CreateAgentSessionWithPrompt(InitialPromptLaunchRequest {
                intent: intent(1),
                launch: launch.clone(),
                prompt: "\n  exact prompt\n界e\u{301}  ".into(),
            })
        );
        assert_eq!(launch.session.name, "");
        assert!(!launch.session.argv.iter().any(|arg| arg == "exact prompt"));
    }

    #[test]
    fn blank_configured_agent_uses_intent_but_nonblank_is_preserved_and_gated() {
        let mut dashboard = make_dashboard("claude", AgentLaunchOrigin::Configured);
        let palette = start_palette(&dashboard, "");
        dashboard.palette = Some(palette);
        dashboard.palette_key_inner(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT), true);
        let blank = dashboard.drain_outbox();
        assert_eq!(blank.len(), 1);
        assert_eq!(blank[0].request, Request::AllocateInitialPromptIntent);

        let mut dashboard = make_dashboard("claude", AgentLaunchOrigin::Configured);
        let palette = start_palette(&dashboard, "keep this draft");
        dashboard.palette = Some(palette);
        dashboard.palette_key_inner(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT), true);
        assert!(dashboard.drain_outbox().is_empty());
        let palette = dashboard.palette.as_ref().unwrap();
        assert!(
            palette
                .error
                .as_deref()
                .is_some_and(|text| !text.contains("keep this draft"))
        );
        let Page::Form { fields, .. } = &palette.page else {
            panic!()
        };
        assert_eq!(
            fields
                .iter()
                .find(|field| field.label == "Initial prompt")
                .unwrap()
                .value,
            "keep this draft"
        );
    }

    #[test]
    fn refused_recovery_keeps_frozen_model_and_prompt_until_explicit_retry() {
        let mut dashboard = make_dashboard("claude", AgentLaunchOrigin::Detected);
        let argv = dashboard.model_agent_entries.as_ref().unwrap()[0]
            .argv
            .clone();
        let mut launch = prompt_launch("claude", argv);
        launch.model_policy = AgentModelPolicy::Explicit("model-x".into());
        let snapshot = InitialPromptIntentSnapshot {
            id: intent(3),
            state: InitialPromptIntentState::Refused,
            project: "demo".into(),
            workspace: "feature".into(),
            name: launch.session.name.clone(),
            label: launch.session.label.clone(),
            session: None,
            run: None,
            recoverable_prompt: Some("exact \n prompt".into()),
            recoverable_launch: Some(launch.clone()),
            message: Some("Claude could not start".into()),
            delivery: None,
        };
        dashboard.install_initial_prompt_snapshot(snapshot);
        let palette = dashboard.palette.as_ref().unwrap();
        assert!(
            palette
                .initial_prompt_summary
                .as_deref()
                .is_some_and(|summary| summary.contains("model-x"))
        );
        let Page::Form {
            command, fields, ..
        } = &palette.page
        else {
            panic!()
        };
        assert!(matches!(command, Command::RecoverInitialPrompt));
        assert_eq!(fields[0].value, "exact \n prompt");
        assert_eq!(palette.recovered_prompt_launch.as_ref(), Some(&launch));

        dashboard.palette_key_inner(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT), true);
        let allocation = dashboard.drain_outbox();
        assert_eq!(allocation.len(), 1);
        assert_eq!(allocation[0].request, Request::AllocateInitialPromptIntent);
        let outgoing = dashboard.handle_server_message(ServerMessage::Response {
            request_id: allocation[0].request_id,
            response: Response::InitialPromptIntent(intent(4)),
        });
        let Request::CreateAgentSessionWithPrompt(request) = &outgoing[0].request else {
            panic!("expected a fresh one-use launch");
        };
        assert_eq!(request.intent, intent(4));
        assert_eq!(request.launch, launch);
        assert_eq!(request.prompt, "exact \n prompt");
    }

    #[test]
    fn refusal_waits_for_unrelated_palette_to_close_before_restoring_prompt() {
        let mut dashboard = make_dashboard("claude", AgentLaunchOrigin::Detected);
        let argv = dashboard.model_agent_entries.as_ref().unwrap()[0]
            .argv
            .clone();
        let launch = prompt_launch("claude", argv);
        let mut palette = start_palette(&dashboard, "current form draft");
        palette.pending = None;
        dashboard.palette = Some(palette);
        let snapshot = InitialPromptIntentSnapshot {
            id: intent(5),
            state: InitialPromptIntentState::Refused,
            project: "demo".into(),
            workspace: "feature".into(),
            name: launch.session.name.clone(),
            label: launch.session.label.clone(),
            session: None,
            run: None,
            recoverable_prompt: Some("previous launch draft".into()),
            recoverable_launch: Some(launch),
            message: Some("Claude could not start".into()),
            delivery: None,
        };

        dashboard.install_initial_prompt_snapshot(snapshot);
        let palette = dashboard.palette.as_ref().unwrap();
        let Page::Form {
            fields, command, ..
        } = &palette.page
        else {
            panic!()
        };
        assert!(matches!(command, Command::CreateTerminal));
        assert_eq!(
            fields
                .iter()
                .find(|field| field.label == "Initial prompt")
                .unwrap()
                .value,
            "current form draft"
        );
        assert_eq!(
            dashboard.lifecycle_strip.as_deref(),
            Some("Claude could not start. Close this form to recover the agent start.")
        );

        dashboard.palette_key_inner(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), true);
        let palette = dashboard.palette.as_ref().unwrap();
        let Page::Form {
            fields, command, ..
        } = &palette.page
        else {
            panic!()
        };
        assert!(matches!(command, Command::RecoverInitialPrompt));
        assert_eq!(fields[0].value, "previous launch draft");
    }
}

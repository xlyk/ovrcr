use super::agents::{AgentSource, apply_overrides, detect_agents};
use super::input::is_browse_key;
use super::keymap::{Action, KeyBinding, keymap};
use super::picker::{
    PathPicker, PickItem, PickList, complete_path, expand_path, split_workspace_pick,
};
use super::render::{CRUST, MAUVE, MUTED, PEACH, SUBTEXT, TEXT, clip_text};
use super::state::{
    find_session, find_workspace, session_workspace_heading, workspace_heading, workspace_label,
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
    CreateTerminal,
    CreateWorkspace,
    RenameTerminal(SessionId),
    ReopenTerminal(SessionId),
    StartNewConversation(SessionId),
    AcknowledgeStopped(SessionId, crate::protocol::SessionRunId),
    AgentResumeUnavailable(SessionId),
    RecoverLaunch,
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
    RefreshQuota,
    EnableQuota,
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
        request: Option<Request>,
        target: String,
    },
}

struct AgentCandidate {
    session: SessionSummary,
    search_identity: String,
    unavailable: Option<&'static str>,
}

fn running_agent(session: &SessionSummary) -> bool {
    !session.archived
        && session.phase == SessionPhase::Running
        && matches!(session.kind, SessionKind::Agent { .. })
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
    error: Option<String>,
    workspace_acknowledged: bool,
    workspace_id: Option<String>,
    launch_preference: Option<(String, super::settings::LaunchChoice)>,
    failed_launch: Option<CreateSessionRequest>,
    launch_project: Option<String>,
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

fn close_terminal_id(page: &Page) -> Option<SessionId> {
    match page {
        Page::Confirm {
            request: Some(Request::CloseTerminal { session, .. }),
            ..
        } => Some(*session),
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
        CloseConfirmRole::Blank | CloseConfirmRole::Place => Style::default().fg(TEXT),
        CloseConfirmRole::Identity => Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        CloseConfirmRole::Meta => Style::default().fg(SUBTEXT),
        CloseConfirmRole::Consequence => Style::default().fg(PEACH),
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
            cell.set_style(Style::default().bg(CRUST).fg(MAUVE));
        }
    }
    for (offset, ch) in CLOSE_TITLE.chars().enumerate().take(room) {
        let symbol = ch.to_string();
        if let Some(cell) = buffer.cell_mut((start + u16::try_from(offset).unwrap_or(0), y)) {
            cell.set_symbol(&symbol);
            cell.set_style(
                Style::default()
                    .bg(CRUST)
                    .fg(MAUVE)
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
            error: None,
            workspace_acknowledged: false,
            workspace_id: None,
            launch_preference: None,
            failed_launch: None,
            launch_project: None,
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
            Command::Switch(id) => find_session(self, id),
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
        DashboardAction::Request(ClientMessage {
            request_id,
            request: Request::Inspect,
        })
    }

    pub(super) fn open_create_terminal(&mut self) -> DashboardAction {
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
        self.palette = Some(Palette {
            page: self.palette_form(Command::CreateTerminal),
            agents: None,
            pending: None,
            archive: None,
            error: None,
            workspace_acknowledged: false,
            workspace_id: None,
            launch_preference: None,
            failed_launch: None,
            launch_project: None,
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
        let palette = self.palette.as_mut().unwrap();
        palette.page = page;
        let _ = ensure_workspace_id(palette);
        action
    }

    fn refresh_workspace_form(&self, palette: &mut Palette) {
        if let Page::Form {
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
            fields[branch_i].value = self.settings.branch_prefix.clone();
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
        let immediate = !session.phase.is_live();
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
            )
        } else {
            DashboardAction::Redraw
        };
        self.palette = Some(palette);
        self.mode = InputMode::Browse;
        action
    }

    /// Enter on a session that is no longer live. Opens the same confirmation the
    /// palette's Resume / Reopen entry does, or the unavailable notice.
    pub(super) fn open_reopen_terminal(&mut self) -> DashboardAction {
        let Some(session) = self
            .action_session()
            .and_then(|id| find_session(self, id))
            .filter(|session| !session.phase.is_live())
        else {
            return DashboardAction::None;
        };
        let unavailable = matches!(session.kind, SessionKind::Agent { .. })
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
        let matches_close = matches!(
            self.palette.as_ref().map(|palette| &palette.page),
            Some(Page::Confirm {
                request: Some(Request::CloseTerminal { session: id, .. }),
                ..
            }) if *id == session
        );
        if matches_close {
            self.palette = None;
        }
    }

    pub(super) fn sync_palette_workspace_identity(&mut self) {
        let removal = match &self.palette {
            Some(palette) if palette.pending.is_some() => None,
            Some(Palette {
                page:
                    Page::Confirm {
                        request:
                            Some(Request::RemoveWorkspace {
                                project,
                                name,
                                force,
                            }),
                        target,
                    },
                ..
            }) => Some((project.clone(), name.clone(), *force, target.clone())),
            _ => None,
        };
        if let Some((project, name, force, target)) = removal {
            let stale = match find_workspace(self, &project, &name) {
                None => true,
                Some(workspace) => {
                    let heading = workspace_heading(self, workspace);
                    let expected = removal_confirmation(
                        Request::RemoveWorkspace {
                            project: project.clone(),
                            name: name.clone(),
                            force,
                        },
                        &heading,
                    );
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
                    if session.recovery.as_ref().is_none_or(|recovery| recovery.conversation.is_none()) {
                        if acknowledge_stopped {
                            "Confirm the previous agent and background processes have stopped, then open the agent's native resume picker?"
                        } else {
                            "Open the agent's native resume picker?"
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
                    request: Some(Request::ReopenSession {
                        session: id,
                        expected_run: session.run,
                        acknowledge_stopped,
                    }),
                    target: target.into(),
                }
            }
            // Carries the run so archived rows, absent from the hierarchy, can be acknowledged too.
            Command::AcknowledgeStopped(id, run) => Page::Confirm {
                request: Some(Request::AcknowledgeSessionStopped {
                    session: id,
                    expected_run: run,
                }),
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
                    request: Some(Request::CloseTerminal {
                        session: id,
                        expected_run: session.run,
                    }),
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
        let action = match command {
            Command::CreateTerminal => Action::CreateTerminal,
            Command::CreateWorkspace => Action::CreateWorkspace,
            Command::RegisterProject => Action::RegisterProject,
            Command::CloseTerminal(_) => Action::CloseTerminal,
            Command::Hint(action) => *action,
            _ => return None,
        };
        keymap(self)
            .into_iter()
            .flat_map(|g| g.keys)
            .find(|binding| binding.action == action)
    }

    pub(super) fn palette_paste(&mut self, text: &str) -> DashboardAction {
        let mut palette = self.palette.take().unwrap();
        palette.insert(text);
        self.refresh_workspace_form(&mut palette);
        self.palette = Some(palette);
        self.refresh_active_path_listing();
        DashboardAction::Redraw
    }

    fn palette_entries(
        &self,
        query: &str,
        archive: Option<&[crate::protocol::SessionSummary]>,
        agents: Option<&[AgentCandidate]>,
    ) -> Vec<Entry> {
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
            return rows
                .iter()
                .filter(|row| {
                    let place = session_workspace_heading(self, row);
                    let label = format!(
                        "{} {} {} {} {}",
                        row.display_name(),
                        place,
                        row.project,
                        row.workspace,
                        row.cwd.display()
                    )
                    .to_lowercase();
                    query.split_whitespace().all(|word| label.contains(word))
                })
                .flat_map(|row| {
                    let label = format!(
                        "{} / {} (#{})",
                        session_workspace_heading(self, row),
                        row.display_name(),
                        row.id.0
                    );
                    let mut entries = vec![
                        Entry {
                            label: format!("Unarchive: {label}"),
                            command: Command::Unarchive(row.id, row.run),
                        },
                        Entry {
                            label: format!("Delete record: {label}"),
                            command: Command::DeleteArchived(
                                row.id,
                                row.run,
                                row.display_name().to_owned(),
                            ),
                        },
                    ];
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
        let mut entries = vec![
            Entry {
                label: "Archived sessions".into(),
                command: Command::Archives,
            },
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
        entries.push(Entry {
            label: "Settings".into(),
            command: Command::Settings,
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
                label: "Enable Codex and Grok usage".into(),
                command: Command::EnableQuota,
            });
        }
        for project in &self.hierarchy.projects {
            for workspace in &project.workspaces {
                for session in &workspace.sessions {
                    entries.push(Entry {
                        label: format!(
                            "Switch terminal: {} / {} / {} (#{})",
                            project.name,
                            workspace_label(self, workspace),
                            session.display_name(),
                            session.id.0
                        ),
                        command: Command::Switch(session.id),
                    });
                }
            }
        }
        for hint in keymap(self).into_iter().flat_map(|g| g.keys) {
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
        if self
            .palette
            .as_ref()
            .is_some_and(|palette| close_terminal_id(&palette.page).is_some())
        {
            return self.close_confirm_mouse(mouse, area);
        }
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
            KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right if matches!(&palette.page, Page::Form { fields, active, .. } if matches!(fields[*active].kind, FieldKind::Toggle)) =>
            {
                if let Page::Form { fields, .. } = &mut palette.page {
                    let to_existing = fields
                        .iter()
                        .find(|field| field.label == "Branch mode")
                        .is_some_and(|field| field.value == "new");
                    if let Some(mode) = fields.iter_mut().find(|field| field.label == "Branch mode")
                    {
                        mode.value = if to_existing { "existing" } else { "new" }.into();
                    }
                    if let Some(branch) = fields.iter_mut().find(|field| field.label == "Branch") {
                        branch.kind = FieldKind::Text;
                        if to_existing {
                            // A leftover new-branch name is not an existing-branch filter.
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
                        let count = self
                            .palette_entries(
                                query,
                                palette.archive.as_deref(),
                                palette.agents.as_deref(),
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
                    let entries = self.palette_entries(
                        query,
                        palette.archive.as_deref(),
                        palette.agents.as_deref(),
                    );
                    *selected = (*selected).min(entries.len().saturating_sub(1));
                    if let Some(entry) = entries.get(*selected) {
                        match entry.command.clone() {
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
                            Command::Settings => {
                                if let Some(request_id) = palette.suggestions.inspect {
                                    self.ignored_responses.insert(request_id);
                                }
                                return self.open_details(super::quota::Details::Settings);
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
                            Command::Hint(action) => {
                                if self
                                    .command_hint(&entry.command)
                                    .is_some_and(|h| h.enabled())
                                {
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
                                action = self.palette_submit(&mut palette, Request::Inspect);
                            }
                            Command::Unarchive(session, expected_run) => {
                                action = self.palette_submit(
                                    &mut palette,
                                    Request::UnarchiveSession {
                                        session,
                                        expected_run,
                                    },
                                );
                            }
                            Command::DeleteArchived(session, expected_run, title) => {
                                palette.page = Page::Confirm {
                                    request: Some(Request::DeleteArchivedSession {
                                        session,
                                        expected_run,
                                    }),
                                    target: format!(
                                        "Delete record {}? Provider history files will be kept.",
                                        title
                                    ),
                                };
                            }
                            Command::CloseTerminal(id)
                                if find_session(self, id).is_some_and(|s| !s.phase.is_live()) =>
                            {
                                let run = find_session(self, id).unwrap().run;
                                action = self.palette_submit(
                                    &mut palette,
                                    Request::CloseTerminal {
                                        session: id,
                                        expected_run: run,
                                    },
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
                    if matches!(command, Command::RecoverLaunch)
                        && fields.first().is_some_and(|f| f.label == "Recovery")
                    {
                        let mut previous = palette.failed_launch.as_ref().unwrap().clone();
                        if !self
                            .hierarchy
                            .projects
                            .iter()
                            .find(|p| p.name == previous.project)
                            .is_some_and(|p| {
                                p.workspaces.iter().any(|w| w.id == previous.workspace)
                            })
                        {
                            palette.error = Some("Workspace not yet registered; wait for the hierarchy update before retrying.".into());
                            self.palette = Some(palette);
                            return action;
                        }
                        match fields[0].value.as_str() {
                            "Choose another agent" => {
                                *fields = self.launch_fields(&previous.project, false);
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
                                previous.argv = self.shell_argv();
                                previous.label = None;
                            }
                            _ => {}
                        }
                        action =
                            self.palette_submit(&mut palette, Request::CreateSession(previous));
                        self.palette = Some(palette);
                        return action;
                    }
                    self.refresh_terminal_form(fields, *name_edited);
                    self.refresh_project_form(fields, *name_edited, *root_edited);
                    let visible = visible_indices(fields);
                    let last = submit
                        || visible.last().copied() == Some(*active)
                        || (matches!(command, Command::CreateWorkspace)
                            && fields
                                .get(*active)
                                .is_some_and(|field| field.label == "Branch"));
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
                        let field = |label: &str| {
                            fields
                                .iter()
                                .find(|f| f.label == label)
                                .map(|f| f.value.trim().to_string())
                                .unwrap_or_default()
                        };
                        let selected_launch = if matches!(
                            command,
                            Command::CreateTerminal
                                | Command::CreateWorkspace
                                | Command::RecoverLaunch
                        ) {
                            match self.selected_launch(fields) {
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
                        let request = match command {
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
                                Request::CreateSession(CreateSessionRequest {
                                    project: workspace.project.clone(),
                                    workspace: workspace.id.clone(),
                                    name: field("Name"),
                                    argv: selected_launch.as_ref().unwrap().argv.clone(),
                                    label: selected_launch.as_ref().unwrap().label.clone(),
                                    kind: selected_launch.as_ref().unwrap().kind.clone(),
                                })
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
                                Request::CreateWorkspaceWithLaunch {
                                    project: field("Project"),
                                    id,
                                    launch: selected_launch.clone(),
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
                                let registered =
                                    find_workspace(self, &previous.project, &previous.workspace)
                                        .is_some();
                                if !registered {
                                    palette.error = Some("Workspace is not yet registered in the dashboard. Wait for the hierarchy update before retrying.".into());
                                    self.palette = Some(palette);
                                    return action;
                                }
                                Request::CreateSession(CreateSessionRequest {
                                    project: previous.project.clone(),
                                    workspace: previous.workspace.clone(),
                                    name: String::new(),
                                    argv: selected_launch.as_ref().unwrap().argv.clone(),
                                    label: selected_launch.as_ref().unwrap().label.clone(),
                                    kind: selected_launch.as_ref().unwrap().kind.clone(),
                                })
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
                            let display = match &request {
                                Request::RemoveWorkspace { project, name, .. } => {
                                    find_workspace(self, project, name)
                                        .map(|workspace| workspace_heading(self, workspace))
                                        .unwrap_or_default()
                                }
                                Request::RemoveProject { name } => name.clone(),
                                _ => String::new(),
                            };
                            palette.page = removal_confirmation(request, &display);
                        } else {
                            action = self.palette_submit(&mut palette, request);
                        }
                    }
                }
                Page::Confirm { request, .. } => {
                    if let Some(request) = request.clone() {
                        action = self.palette_submit(&mut palette, request);
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

    fn detected_agents(&self) -> Vec<super::agents::AgentEntry> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let shell = std::env::var_os("SHELL");
        let launcher = std::env::current_exe().ok();
        apply_overrides(
            detect_agents(&path, shell.as_deref(), launcher.as_deref()),
            &self.settings.agents,
        )
    }

    fn refresh_terminal_form(&self, fields: &mut [Field], _name_edited: bool) {
        let Some(start) = fields
            .iter()
            .find(|f| f.label == "Start")
            .map(|f| f.value.clone())
        else {
            return;
        };
        for field in fields {
            if field.label == "Agent" {
                field.hidden = start != "Agent";
                field.required = start == "Agent";
            }
            if field.label == "Command" {
                field.hidden = start != "Terminal";
                field.required = false;
            }
        }
    }

    fn launch_fields(&self, project: &str, empty: bool) -> Vec<Field> {
        let choice = self.settings.launch_choices.get(project);
        let start = match choice {
            Some(super::settings::LaunchChoice::Agent(_)) => "Agent",
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
            Some(super::settings::LaunchChoice::Agent(name)) => name.clone(),
            _ => agents.first().map(|a| a.name.clone()).unwrap_or_default(),
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
            text_field("Command", String::new(), false),
        ];
        self.refresh_terminal_form(&mut fields, false);
        fields
    }

    fn shell_argv(&self) -> Vec<OsString> {
        self.detected_agents()
            .into_iter()
            .find(|entry| entry.source == AgentSource::Shell)
            .expect("agent detection always includes the shell entry")
            .argv
    }

    fn selected_launch(&self, fields: &[Field]) -> Result<Option<SessionLaunch>, String> {
        let value = |label| {
            fields
                .iter()
                .find(|f| f.label == label)
                .map(|f| f.value.trim())
                .unwrap_or("")
        };
        match value("Start") {
            "Nothing yet" => Ok(None),
            "Agent" => self
                .detected_agents()
                .into_iter()
                .find(|a| {
                    a.name == value("Agent")
                        && a.source != AgentSource::Shell
                        && a.source != AgentSource::Custom
                        && a.name != "shell"
                        && a.name != "Custom"
                })
                .map(|a| {
                    Some(SessionLaunch {
                        argv: a.argv,
                        kind: SessionKind::Agent {
                            name: a.name.clone(),
                        },
                        label: Some(a.name),
                    })
                })
                .ok_or_else(|| {
                    format!(
                        "Agent '{}' is unavailable. Choose another agent or explicitly select Terminal.",
                        value("Agent")
                    )
                }),
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
            })),
            _ => Err("Choose Agent, Terminal, or Nothing yet".into()),
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

    fn palette_submit(&mut self, palette: &mut Palette, request: Request) -> DashboardAction {
        let launch = match &request {
            Request::CreateWorkspaceWithLaunch {
                project,
                id: name,
                launch: Some(launch),
                ..
            } => Some(CreateSessionRequest {
                project: project.clone(),
                workspace: name.clone(),
                name: String::new(),
                argv: launch.argv.clone(),
                label: launch.label.clone(),
                kind: launch.kind.clone(),
            }),
            Request::CreateSession(session) => Some(session.clone()),
            _ => None,
        };
        if let Some(launch) = launch {
            palette.launch_preference = Some((
                launch.project.clone(),
                match &launch.label {
                    Some(label) => super::settings::LaunchChoice::Agent(label.clone()),
                    None => super::settings::LaunchChoice::Terminal,
                },
            ));
            palette.failed_launch = Some(launch);
        }
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
                let agent = launch.pop().unwrap();
                vec![
                    launch.pop().unwrap(),
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
                    project,
                    text("Branch", self.settings.branch_prefix.clone(), true),
                    mode,
                    text("Base", "main".into(), true),
                ];
                fields.extend(self.launch_fields(&project_name, true));
                fields
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
        if self.palette.as_ref()?.suggestions.inspect == Some(request_id) {
            let mut palette = self.palette.take().unwrap();
            palette.suggestions.inspect = None;
            if let Response::Inventory { registry, .. } = response {
                palette.suggestions.repos = registry
                    .projects
                    .iter()
                    .map(|project| (project.name.clone(), project.repo.clone()))
                    .collect();
            }
            self.refresh_workspace_form(&mut palette);
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
            Response::Inventory { sessions, .. } if self.palette.as_ref()?.archive.is_some() => {
                let palette = self.palette.as_mut().unwrap();
                palette.pending = None;
                let mut rows: Vec<_> = sessions
                    .iter()
                    .filter(|row| row.archived)
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
                palette.error = Some(format!("{code:?}: {message}"));
                // A refused removal reopens as a forced one; only another
                // explicit Confirm sends it.
                let forced = match &palette.page {
                    Page::Confirm {
                        request:
                            Some(Request::RemoveWorkspace {
                                project,
                                name,
                                force: false,
                            }),
                        ..
                    } if matches!(
                        code,
                        crate::protocol::ErrorCode::SessionsRemain
                            | crate::protocol::ErrorCode::DirtyWorktree
                    ) =>
                    {
                        Some(Request::RemoveWorkspace {
                            project: project.clone(),
                            name: name.clone(),
                            force: true,
                        })
                    }
                    _ => None,
                };
                if let Some(request) = forced {
                    let display = match &request {
                        Request::RemoveWorkspace { project, name, .. } => {
                            find_workspace(self, project, name)
                                .map(|workspace| workspace_heading(self, workspace))
                                .unwrap_or_default()
                        }
                        _ => String::new(),
                    };
                    palette.page = removal_confirmation(request, &display);
                }
                let retry = match &palette.page {
                    Page::Confirm {
                        request: Some(Request::ReopenSession { session, .. }),
                        ..
                    } => Some((*session, Command::ReopenTerminal(*session))),
                    Page::Confirm {
                        request:
                            Some(Request::AcknowledgeSessionStopped {
                                session,
                                expected_run,
                            }),
                        ..
                    } => Some((
                        *session,
                        Command::AcknowledgeStopped(*session, *expected_run),
                    )),
                    _ => None,
                };
                if let Some((id, retry)) = retry {
                    if find_session(self, id).is_some() {
                        palette.page = self.command_page(retry);
                        palette.error = Some(format!("{code:?}: {message}"));
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
                if let Some(request_id) = palette.suggestions.inspect {
                    self.ignored_responses.insert(request_id);
                }
                if let Response::CreatedSession(session) = response
                    && self.stale_created_session(session)
                {
                    return Some(Vec::new());
                }
                let preference = palette.launch_preference.take();
                self.error = None;
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
            self.error = None;
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
                let entries = self.palette_entries(
                    query,
                    palette.archive.as_deref(),
                    palette.agents.as_deref(),
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
                            Style::default().fg(SUBTEXT),
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
                            Style::default().fg(MUTED),
                        ));
                    } else if let Some(hint) = self.command_hint(&entry.command) {
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
                    let pick_label;
                    let editing = match &field.kind {
                        FieldKind::Pick(list) if index == *active && !list.query.is_empty() => {
                            list.query.as_str()
                        }
                        FieldKind::Pick(list) => {
                            pick_label = list
                                .items
                                .iter()
                                .find(|item| item.value == field.value)
                                .map(|item| item.label.as_str())
                                .unwrap_or(field.value.as_str());
                            pick_label
                        }
                        _ => field.value.as_str(),
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

    fn close_confirm_text(&self, palette: &Palette) -> CloseConfirmText {
        let Page::Confirm { request, target } = &palette.page else {
            return CloseConfirmText {
                identity: CLOSE_TITLE.to_string(),
                place: String::new(),
                project: String::new(),
                workspace: String::new(),
                error: palette.error.clone(),
                pending: palette.pending.is_some(),
            };
        };
        let Some(Request::CloseTerminal { session, .. }) = request else {
            return CloseConfirmText {
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
            return self
                .palette_key_with_submit(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), true);
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
                .border_style(Style::default().fg(MAUVE))
                .style(Style::default().bg(CRUST).fg(TEXT)),
            layout.dialog,
        );
        paint_rect(frame, layout.rail, Style::default().bg(PEACH).fg(PEACH));
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
                    .style(Style::default().fg(MAUVE).add_modifier(Modifier::BOLD)),
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
        let (area, inner, body, buttons) = palette_geometry(frame.area());
        frame.render_widget(Clear, area);
        let title = match &palette.page {
            Page::Search { .. } if palette.agents.is_some() => "Agents",
            Page::Search { .. } if palette.archive.is_some() => "Archived sessions",
            Page::Search { .. } => "Command palette",
            Page::Form { command, .. } => match command {
                Command::CreateTerminal => "Create terminal",
                Command::CreateWorkspace => "Create workspace",
                Command::RenameTerminal(_) => "Rename terminal · blank = original name",
                Command::RecoverLaunch => "Workspace retained · recover launch",
                Command::RegisterProject => "Register project",
                Command::RemoveWorkspace => "Remove workspace",
                _ => "Remove project",
            },
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
            let entries =
                self.palette_entries(query, palette.archive.as_deref(), palette.agents.as_deref());
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
        } else if matches!(&palette.page, Page::Form { fields, .. } if fields.iter().any(|f| f.label == "Start" && f.value == "Agent") && self.selected_launch(fields).is_err())
        {
            "Remembered/selected agent unavailable. Choose another agent or select Terminal explicitly."
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
                } => {
                    "Enter on Branch submits · Tab next · ↑/↓ pick · Space/←/→ toggle · Esc cancel"
                }
                Page::Form {
                    command: Command::RenameTerminal(_),
                    ..
                } => "Blank = original name · Enter save · Ctrl-u clear · Esc cancel",
                Page::Form { .. } => {
                    "Name blank = generated · Command blank = shell · Tab next · Enter submit · Ctrl-u clear"
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
fn removal_confirmation(request: Request, display: &str) -> Page {
    let target = match &request {
        Request::RemoveWorkspace { force: false, .. } => format!(
            "Remove workspace {display}. Remove its clean worktree; keep the branch. Archive stopped sessions with their original paths. Live or ownership-uncertain sessions block removal."
        ),
        Request::RemoveWorkspace { force: true, .. } => format!(
            "Force remove workspace {display}. Discard uncommitted changes in its worktree and acknowledge that its stopped sessions' processes are gone; keep the branch. Live sessions and active task runs still block."
        ),
        Request::RemoveProject { name } => {
            format!(
                "Unregister project {name}. Remove workspaces first. Keep the repository and archived session context."
            )
        }
        _ => unreachable!("only workspace/project removal uses this confirmation"),
    };
    Page::Confirm {
        request: Some(request),
        target,
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
            page: removal_confirmation(
                Request::RemoveProject {
                    name: "demo".into(),
                },
                "demo",
            ),
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
        assert!(
            fields
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
            matches!(message.request, Request::CreateWorkspaceWithLaunch { project, id, branch: BranchRequest::New { branch, base, .. }, .. } if project == "demo" && id.len() == 32 && branch.ends_with("deferred") && base == "develop")
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

#[cfg(test)]
mod launch_tests {
    use super::super::settings::{AgentOverride, LaunchChoice};
    use super::*;
    use crate::protocol::{
        AgentActivity, ErrorCode, ProjectSummary, ServerEvent, ServerMessage, SessionId,
        SessionKind, SessionPhase, SessionRecovery, SessionSummary, TerminalSize, WorkspaceSummary,
    };

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

    #[test]
    fn new_conversation_uses_selected_provider_despite_conflicting_launch_preference() {
        for preference in [
            None,
            Some(LaunchChoice::Terminal),
            Some(LaunchChoice::Agent("fixture-agent".into())),
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
            let Request::CreateSession(created) = message.request else {
                panic!("new conversation must create a separate session");
            };
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
        // This is the production handler used by the visible Submit control.
        d.palette_key_with_submit(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), true)
    }
    fn request(d: &mut Dashboard) -> ClientMessage {
        let DashboardAction::Request(message) = submit(d) else {
            panic!(
                "expected request: {:?}",
                d.palette.as_ref().and_then(|p| p.error.as_ref())
            );
        };
        message
    }
    fn summary(id: u64) -> SessionSummary {
        SessionSummary {
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
            label: "shell".into(),
            pid: None,
            started_unix_ms: Some(0),
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
        let DashboardAction::Request(request) =
            d.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        else {
            panic!("first attention result must select");
        };
        assert!(
            matches!(request.request, Request::SetView { view } if view.focused == Some(SessionId(3)))
        );
        assert_eq!(d.mode, InputMode::Browse);
        assert!(d.agent_typing.is_some());
    }

    fn start_workspace(d: &mut Dashboard, start: &str) {
        d.open_create_workspace();
        let p = d.palette.as_mut().unwrap();
        p.suggestions.inspect = None;
        p.suggestions
            .cache
            .insert("demo".into(), Err("fixture".into()));
        p.workspace_id = Some("new-work".into());
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
            if start == "Nothing yet" {
                assert!(launch.is_none());
            } else {
                let launch = launch.unwrap();
                assert!(!launch.argv.is_empty());
                assert_eq!(
                    launch.label.as_deref(),
                    (start == "Agent").then_some("fixture-agent")
                );
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
            let other = LaunchChoice::Agent("other".into());
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
                LaunchChoice::Agent("fixture-agent".into())
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
        let saved = LaunchChoice::Agent("fixture-agent".into());
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
            Some(&LaunchChoice::Agent("fixture-agent".into()))
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
        d.settings.launch_choices.insert(
            "demo".into(),
            LaunchChoice::Agent("missing-fixture-agent".into()),
        );
        start_workspace(&mut d, "Agent");
        assert!(matches!(submit(&mut d), DashboardAction::Redraw));
        assert!(
            d.palette
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("unavailable")
        );
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
            let Request::CreateSession(s) = m.request else {
                panic!("recovery must never recreate the workspace")
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
            request,
            &Some(Request::ReopenSession {
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
            request,
            &Some(Request::AcknowledgeSessionStopped {
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
            name: "claude".into(),
        };
        row.phase = SessionPhase::Stopped;
        let reason =
            "No certified claude conversation; use managed launch and configured reporting";
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
                request,
                Some(Request::ReopenSession {
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

        // An explicit unavailable reason still explains instead of launching.
        d.hierarchy.projects[0].workspaces[0].sessions[0].recovery = Some(SessionRecovery {
            conversation: None,
            attached: false,
            requires_ack: false,
            unavailable: Some("no certified conversation".into()),
            failure: None,
        });
        d.palette = None;
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
            d.palette_entries("", None, None)
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
        if let DashboardAction::Request(message) = submit(&mut d) {
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
            matches!(request(&mut d).request, Request::CreateSession(_)),
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
        if let DashboardAction::Request(message) = submit(&mut d) {
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
        if let DashboardAction::Request(message) = submit(&mut d) {
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
                request: Some(Request::CloseTerminal {
                    session: SessionId(1),
                    expected_run: crate::protocol::SessionRunId(1),
                }),
                ..
            }
        ));
        let mut advanced = d.hierarchy.clone();
        advanced.projects[0].workspaces[0].sessions[0].run = crate::protocol::SessionRunId(4);
        d.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
            advanced,
        )));
        let action = if d.palette.is_some() {
            submit(&mut d)
        } else {
            DashboardAction::None
        };
        assert!(
            !matches!(
                action,
                DashboardAction::Request(ClientMessage {
                    request: Request::CloseTerminal { .. },
                    ..
                })
            ),
            "must not send CloseTerminal for the old confirmation without a new explicit confirm"
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
        let entries = d.palette_entries("", Some(&[filed, certain]), None);
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
            request,
            Some(Request::AcknowledgeSessionStopped {
                session: SessionId(9),
                expected_run: crate::protocol::SessionRunId(4),
            })
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
                    label: "shell".into(),
                    pid: None,
                    started_unix_ms: Some(0),
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
        let DashboardAction::Request(message) = action else {
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
        let action = d.palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(
            action,
            DashboardAction::Request(crate::protocol::ClientMessage {
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

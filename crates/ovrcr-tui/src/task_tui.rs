//! Dedicated scheduled-task interface. Network work stays outside the input/render loop.
use crate::TaskRequestFn;
use crate::dashboard::picker::PickList;
use anyhow::{Context, Result, bail};
use crossterm::event::{Event, KeyCode, KeyModifiers};
use ovrcr_protocol::HierarchySnapshot;
use ovrcr_protocol::task::{
    Run, RunId, Schedule, Task, TaskId, TaskRequest, TaskResponse, TaskSpec, TaskTarget,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::{
    cell::{Cell, Ref, RefCell},
    collections::VecDeque,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn parse_duration(value: &str) -> Result<u64> {
    if !value.is_ascii() || value.len() < 2 {
        bail!("duration must be an integer followed by s, m, h, or d");
    }
    let (number, unit) = value.split_at(value.len() - 1);
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => bail!("duration must use s, m, h, or d"),
    };
    let seconds = number
        .parse::<u64>()
        .context("duration must be a positive integer")?
        .checked_mul(multiplier)
        .context("duration is too large")?;
    if seconds == 0 {
        bail!("duration must be positive");
    }
    Ok(seconds)
}

pub fn event_text(event: &serde_json::Value) -> String {
    let event = event.get("event").unwrap_or(event);
    match event["type"].as_str().unwrap_or("") {
        "message_update" if event["assistantMessageEvent"]["type"] == "text_delta" => {
            event["assistantMessageEvent"]["delta"]
                .as_str()
                .unwrap_or("")
                .to_owned()
        }
        "tool_execution_start" => format!(
            "\n[tool: {}]\n",
            event["toolName"].as_str().unwrap_or("unknown")
        ),
        "tool_execution_end" => {
            let mut text = String::new();
            if let Some(content) = event["result"]["content"].as_array() {
                for part in content {
                    if part["type"] == "text" {
                        text.push_str(part["text"].as_str().unwrap_or(""));
                        text.push('\n');
                    }
                }
            }
            format!(
                "\n{text}[tool {}]\n",
                if event["isError"] == true {
                    "failed"
                } else {
                    "finished"
                }
            )
        }
        "message_end" if event["message"]["stopReason"] == "error" => format!(
            "\n[error: {}]\n",
            event["message"]["errorMessage"]
                .as_str()
                .unwrap_or("provider error")
        ),
        "agent_settled" => "\n".into(),
        _ => String::new(),
    }
}

const TRANSCRIPT_LIMIT: usize = 256 * 1024;
#[derive(Default)]
pub struct Transcript {
    pub offset: u64,
    pending: Vec<u8>,
    text: String,
    pub scroll: usize,
}
impl Transcript {
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn append(&mut self, bytes: &[u8], next_offset: u64, terminal: bool) {
        self.offset = next_offset;
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let record: Vec<_> = self.pending.drain(..=end).collect();
            self.record(&record);
        }
        if terminal && bytes.is_empty() && !self.pending.is_empty() {
            let record = std::mem::take(&mut self.pending);
            self.record(&record);
        }
        if self.pending.len() > 1024 * 1024 {
            self.pending.clear();
            self.text.push_str("\n[oversized event omitted]\n");
        }
        if self.text.len() > TRANSCRIPT_LIMIT {
            let mut cut = self.text.len() - TRANSCRIPT_LIMIT;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text.drain(..cut);
        }
    }
    fn record(&mut self, bytes: &[u8]) {
        if let Ok(event) = serde_json::from_slice(bytes) {
            self.text.push_str(&crate::event_text(&event));
        } else {
            self.text.push_str(&String::from_utf8_lossy(bytes));
        }
    }
}

const LABELS: [&str; 12] = [
    "Name",
    "Target (scratch/git)",
    "Project",
    "Remote",
    "Branch",
    "Schedule (interval/cron/once)",
    "When (duration/cron/RFC3339)",
    "Timezone",
    "Model",
    "Thinking",
    "Timeout",
    "Prompt",
];
pub struct TaskEditor {
    projects: PickList,
    id: Option<TaskId>,
    submitted: Option<TaskRequest>,
    fields: Vec<String>,
    field: usize,
    cursor: usize,
}
impl TaskEditor {
    fn new(task: Option<&Task>, mut projects: PickList) -> Self {
        let mut fields = vec![
            "".into(),
            "scratch".into(),
            "".into(),
            "origin".into(),
            "main".into(),
            "interval".into(),
            "1h".into(),
            "UTC".into(),
            "".into(),
            "off".into(),
            "1h".into(),
            "".into(),
        ];
        if let Some(task) = task {
            let s = &task.spec;
            fields[0] = s.name.clone();
            fields[8] = s.model.clone();
            fields[9] = s.thinking.clone();
            fields[10] = format!("{}s", s.timeout_seconds);
            fields[11] = s.prompt.clone();
            if let TaskTarget::Git {
                project,
                remote,
                branch,
            } = &s.target
            {
                fields[1] = "git".into();
                fields[2] = project.clone();
                fields[3] = remote.clone();
                fields[4] = branch.clone();
            }
            match &s.schedule {
                Schedule::Interval { seconds } => fields[6] = format!("{seconds}s"),
                Schedule::Cron {
                    expression,
                    timezone,
                } => {
                    fields[5] = "cron".into();
                    fields[6] = expression.clone();
                    fields[7] = timezone.clone();
                }
                Schedule::Once { at } => {
                    fields[5] = "once".into();
                    fields[6] = chrono::DateTime::from_timestamp(*at, 0)
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_else(|| at.to_string());
                }
            }
        }
        if task.is_some() {
            projects.select_value(&fields[2]);
        } else {
            fields[2] = projects
                .accepted()
                .map(|item| item.value.clone())
                .unwrap_or_default();
        }
        let cursor = fields[0].len();
        Self {
            projects,
            id: task.map(|t| t.id),
            submitted: None,
            fields,
            field: 0,
            cursor,
        }
    }
    fn spec(&self) -> Result<TaskSpec> {
        let f = &self.fields;
        if f[1].trim() == "git" && !self.projects.items.iter().any(|item| item.value == f[2]) {
            bail!("Select an available project");
        }
        let target = match f[1].trim() {
            "scratch" => TaskTarget::Scratch,
            "git" => TaskTarget::Git {
                project: f[2].clone(),
                remote: f[3].clone(),
                branch: f[4].clone(),
            },
            _ => bail!("target must be scratch or git"),
        };
        let schedule = match f[5].trim() {
            "interval" => Schedule::Interval {
                seconds: crate::parse_duration(&f[6])?,
            },
            "cron" => Schedule::Cron {
                expression: f[6].clone(),
                timezone: f[7].clone(),
            },
            "once" => Schedule::Once {
                at: chrono::DateTime::parse_from_rfc3339(&f[6])?.timestamp(),
            },
            _ => bail!("schedule must be interval, cron or once"),
        };
        let spec = TaskSpec {
            name: f[0].clone(),
            prompt: f[11].clone(),
            target,
            schedule,
            model: f[8].clone(),
            thinking: f[9].clone(),
            timeout_seconds: crate::parse_duration(&f[10])?,
        };
        spec.validate()?;
        Ok(spec)
    }
    fn accept_project(&mut self) -> bool {
        let Some(item) = self.projects.accepted() else {
            return false;
        };
        self.fields[2] = item.value.clone();
        self.cursor = self.fields[2].len();
        true
    }

    fn edit(&mut self, event: Event) {
        if self.field == 2 {
            match &event {
                Event::Paste(text) => self.projects.on_insert(&text.replace(['\r', '\n'], " ")),
                Event::Key(key) => match key.code {
                    KeyCode::Up => self.projects.move_selection(-1),
                    KeyCode::Down => self.projects.move_selection(1),
                    KeyCode::Backspace => self.projects.on_backspace(),
                    KeyCode::Char('a' | 'u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.projects.query.clear();
                        self.projects.selected = 0;
                    }
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        self.projects.on_insert(&c.to_string())
                    }
                    KeyCode::Tab | KeyCode::Enter => {
                        if self.accept_project() || self.fields[1].trim() != "git" {
                            self.field = 3;
                            self.cursor = self.fields[3].len();
                        }
                    }
                    KeyCode::BackTab => {
                        self.field = 1;
                        self.cursor = self.fields[1].len();
                    }
                    _ => {}
                },
                _ => {}
            }
            return;
        }
        let text = &mut self.fields[self.field];
        match event {
            Event::Paste(value) => {
                let value = if self.field == 11 {
                    value
                } else {
                    value.replace(['\r', '\n'], " ")
                };
                text.insert_str(self.cursor, &value);
                self.cursor += value.len();
            }
            Event::Key(key) => match key.code {
                KeyCode::Tab | KeyCode::BackTab => {
                    self.field =
                        (self.field + if key.code == KeyCode::BackTab { 11 } else { 1 }) % 12;
                    self.cursor = self.fields[self.field].len();
                }
                KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    text.clear();
                    self.cursor = 0;
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    text.insert(self.cursor, c);
                    self.cursor += c.len_utf8();
                }
                KeyCode::Enter if self.field == 11 => {
                    text.insert(self.cursor, '\n');
                    self.cursor += 1;
                }
                KeyCode::Left => {
                    self.cursor = text[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i);
                }
                KeyCode::Right => {
                    self.cursor += text[self.cursor..].chars().next().map_or(0, char::len_utf8);
                }
                KeyCode::Home => {
                    self.cursor = text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
                }
                KeyCode::End => {
                    self.cursor += text[self.cursor..]
                        .find('\n')
                        .unwrap_or(text.len() - self.cursor);
                }
                KeyCode::Up | KeyCode::Down if self.field == 11 => {
                    let start = text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
                    let col = text[start..self.cursor].chars().count();
                    let destination = if key.code == KeyCode::Up {
                        start
                            .checked_sub(1)
                            .map(|end| (text[..end].rfind('\n').map_or(0, |i| i + 1), end))
                    } else {
                        text[self.cursor..].find('\n').map(|n| {
                            let start = self.cursor + n + 1;
                            (
                                start,
                                start + text[start..].find('\n').unwrap_or(text.len() - start),
                            )
                        })
                    };
                    if let Some((start, end)) = destination {
                        self.cursor = start
                            + text[start..end]
                                .char_indices()
                                .nth(col)
                                .map_or(end - start, |(i, _)| i);
                    }
                }
                KeyCode::Backspace if self.cursor > 0 => {
                    let previous = text[..self.cursor].char_indices().next_back().unwrap().0;
                    text.drain(previous..self.cursor);
                    self.cursor = previous;
                }
                KeyCode::Delete if self.cursor < text.len() => {
                    let end = self.cursor + text[self.cursor..].chars().next().unwrap().len_utf8();
                    text.drain(self.cursor..end);
                }
                _ => {}
            },
            _ => {}
        }
    }
}

pub struct TasksView {
    projects: PickList,
    pub tasks: Vec<Task>,
    pub runs: Vec<Run>,
    pub concurrency: usize,
    pub editor: Option<TaskEditor>,
    pub transcript: Transcript,
    selected: usize,
    selected_run: usize,
    history: bool,
    history_filter: Option<TaskId>,
    log_id: Option<RunId>,
    confirmation: Option<TaskRequest>,
    concurrency_input: Option<String>,
    pending: VecDeque<TaskRequest>,
    pub message: String,
    wrap_cache: RefCell<Option<WrapCache>>,
    wraps: Cell<u64>,
}
/// Transcript lines wrapped for one pane width, reused while nothing changes.
struct WrapCache {
    text: String,
    width: u16,
    lines: Vec<String>,
}
impl Default for TasksView {
    fn default() -> Self {
        Self {
            projects: PickList::new(vec![]),
            tasks: vec![],
            runs: vec![],
            concurrency: 3,
            editor: None,
            transcript: Transcript::default(),
            selected: 0,
            selected_run: 0,
            history: false,
            history_filter: None,
            log_id: None,
            confirmation: None,
            concurrency_input: None,
            pending: VecDeque::new(),
            message: String::new(),
            wrap_cache: RefCell::new(None),
            wraps: Cell::new(0),
        }
    }
}
impl TasksView {
    pub fn with_projects(hierarchy: &HierarchySnapshot, preferred: &str) -> Self {
        Self {
            projects: PickList::projects(hierarchy, preferred),
            ..Self::default()
        }
    }

    /// How many times the transcript was re-wrapped; frames with an unchanged
    /// transcript and width reuse the previous wrap.
    pub fn transcript_wraps(&self) -> u64 {
        self.wraps.get()
    }
    fn wrapped_transcript(&self, text: &str, width: u16) -> Ref<'_, [String]> {
        let stale = self
            .wrap_cache
            .borrow()
            .as_ref()
            .is_none_or(|cache| cache.width != width || cache.text != text);
        if stale {
            self.wraps.set(self.wraps.get() + 1);
            *self.wrap_cache.borrow_mut() = Some(WrapCache {
                text: text.to_owned(),
                width,
                lines: wrap_transcript(text, usize::from(width)),
            });
        }
        Ref::map(self.wrap_cache.borrow(), |cache| {
            cache
                .as_ref()
                .map_or(&[][..], |cache| cache.lines.as_slice())
        })
    }
    pub fn take_request(&mut self) -> Option<TaskRequest> {
        self.pending.pop_front()
    }
    fn queue(&mut self, request: TaskRequest) -> bool {
        if self.pending.len() < 16 {
            if !request.is_read_only() {
                self.message = "Working…".into();
            }
            self.pending.push_back(request);
            true
        } else {
            self.message = "Please wait for pending task actions".into();
            false
        }
    }
    pub fn refresh(&mut self) {
        if !self.pending.is_empty() {
            return;
        }
        self.pending.push_back(TaskRequest::ListTasks);
        self.pending.push_back(TaskRequest::Concurrency(None));
        if self.history {
            self.pending
                .push_back(TaskRequest::ListRuns(self.history_filter));
        }
        if let Some(id) = self.log_id {
            self.pending.push_back(TaskRequest::ReadLog {
                id,
                offset: self.transcript.offset,
                max_bytes: 64 * 1024,
            });
        }
    }
    /// True means return to the session dashboard.
    pub fn event(&mut self, event: Event) -> bool {
        if let Some(editor) = &mut self.editor {
            if editor.submitted.is_some() {
                if matches!(&event, Event::Key(key) if key.code == KeyCode::Esc) {
                    self.editor = None;
                }
                return false;
            }
            if let Event::Key(key) = &event {
                if key.code == KeyCode::Esc {
                    self.editor = None;
                    return false;
                }
                if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    if editor.field == 2
                        && editor.fields[1].trim() == "git"
                        && !editor.accept_project()
                    {
                        self.message = "Select an available project".into();
                        return false;
                    }
                    match editor.spec() {
                        Ok(spec) => {
                            let request = editor.id.map_or_else(
                                || TaskRequest::Create(spec.clone()),
                                |id| TaskRequest::Update {
                                    id,
                                    spec: spec.clone(),
                                },
                            );
                            if self.queue(request.clone()) {
                                self.editor.as_mut().unwrap().submitted = Some(request);
                            }
                        }
                        Err(error) => self.message = error.to_string(),
                    }
                    return false;
                }
            }
            editor.edit(event);
            return false;
        }
        let Event::Key(key) = event else {
            return false;
        };
        if self.confirmation.is_some() {
            if key.code == KeyCode::Char('y') {
                let request = self.confirmation.take().unwrap();
                self.queue(request);
            } else if matches!(key.code, KeyCode::Esc | KeyCode::Char('n')) {
                self.confirmation = None;
            }
            return false;
        }
        if let Some(value) = &mut self.concurrency_input {
            match key.code {
                KeyCode::Esc => self.concurrency_input = None,
                KeyCode::Backspace => {
                    value.pop();
                }
                KeyCode::Char(c) if c.is_ascii_digit() => value.push(c),
                KeyCode::Enter => match value.parse::<usize>() {
                    Ok(n) if n > 0 => {
                        self.concurrency_input = None;
                        self.queue(TaskRequest::Concurrency(Some(n)));
                    }
                    _ => self.message = "Concurrency must be a positive integer".into(),
                },
                _ => {}
            }
            return false;
        }
        match key.code {
            KeyCode::Esc if self.history => {
                self.history = false;
                self.log_id = None;
            }
            KeyCode::Esc | KeyCode::Char('q') => return true,
            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('n') if !self.history => {
                self.editor = Some(TaskEditor::new(None, self.projects.clone()))
            }
            KeyCode::Char('e') if !self.history => {
                if let Some(task) = self.tasks.get(self.selected) {
                    self.editor = Some(TaskEditor::new(Some(task), self.projects.clone()));
                }
            }
            KeyCode::Char('c') if !self.history => self.concurrency_input = Some(String::new()),
            KeyCode::Char('h') | KeyCode::Enter if !self.history => {
                self.history = true;
                self.history_filter = self.tasks.get(self.selected).map(|t| t.id);
                self.queue(TaskRequest::ListRuns(self.history_filter));
            }
            KeyCode::Char('H') if !self.history => {
                self.history = true;
                self.history_filter = None;
                self.queue(TaskRequest::ListRuns(None));
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageUp if self.history => {
                self.transcript.scroll = self.transcript.scroll.saturating_add(10)
            }
            KeyCode::PageDown if self.history => {
                self.transcript.scroll = self.transcript.scroll.saturating_sub(10)
            }
            KeyCode::End if self.history => self.transcript.scroll = 0,
            KeyCode::Char('x') if self.history => {
                if let Some(id) = self.log_id {
                    self.queue(TaskRequest::Cancel(id));
                }
            }
            KeyCode::Char('d') if self.history => {
                if let Some(id) = self.log_id {
                    self.confirmation = Some(TaskRequest::Clean {
                        id,
                        confirmed: true,
                    });
                }
            }
            KeyCode::Char('d') => {
                if let Some(t) = self.tasks.get(self.selected) {
                    self.confirmation = Some(TaskRequest::Delete(t.id));
                }
            }
            KeyCode::Char('p') if !self.history => {
                if let Some(t) = self.tasks.get(self.selected) {
                    self.queue(if t.enabled {
                        TaskRequest::Pause(t.id)
                    } else {
                        TaskRequest::Resume(t.id)
                    });
                }
            }
            KeyCode::Char('r') if !self.history => {
                if let Some(t) = self.tasks.get(self.selected) {
                    self.queue(TaskRequest::Enqueue(t.id));
                }
            }
            _ => {}
        }
        false
    }
    fn move_selection(&mut self, delta: isize) {
        let (selected, len) = if self.history {
            (&mut self.selected_run, self.runs.len())
        } else {
            (&mut self.selected, self.tasks.len())
        };
        *selected = selected
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
        if self.history {
            self.select_log();
        }
    }
    fn select_log(&mut self) {
        let id = self.runs.get(self.selected_run).map(|r| r.id);
        if id != self.log_id {
            self.log_id = id;
            self.transcript = Transcript::default();
            if let Some(id) = id {
                self.queue(TaskRequest::ReadLog {
                    id,
                    offset: 0,
                    max_bytes: 64 * 1024,
                });
            }
        }
    }
    pub fn receive(&mut self, request: &TaskRequest, result: Result<TaskResponse, String>) {
        let editor_response = self
            .editor
            .as_ref()
            .is_some_and(|editor| editor.submitted.as_ref() == Some(request));
        let response = match result {
            Ok(r) => {
                if editor_response {
                    self.editor = None;
                }
                r
            }
            Err(error) => {
                if editor_response {
                    self.editor.as_mut().unwrap().submitted = None;
                }
                self.message = error;
                return;
            }
        };
        if !request.is_read_only() {
            self.message = "Saved".into();
        }
        match response {
            TaskResponse::Tasks(tasks) => {
                let selected = self.tasks.get(self.selected).map(|t| t.id);
                self.tasks = tasks;
                self.selected = selected
                    .and_then(|id| self.tasks.iter().position(|t| t.id == id))
                    .unwrap_or(self.selected.min(self.tasks.len().saturating_sub(1)));
            }
            TaskResponse::Runs(runs) => {
                self.runs = runs;
                self.selected_run = self
                    .log_id
                    .and_then(|id| self.runs.iter().position(|r| r.id == id))
                    .unwrap_or(0);
                self.select_log();
            }
            TaskResponse::Concurrency(n) => self.concurrency = n,
            TaskResponse::Log {
                bytes,
                next_offset,
                terminal,
            } => {
                if let TaskRequest::ReadLog { id, offset, .. } = request
                    && Some(*id) == self.log_id
                    && *offset == self.transcript.offset
                {
                    self.transcript.append(&bytes, next_offset, terminal);
                }
            }
            _ => {}
        }
    }
}

/// One outstanding request; channels bound both requests and responses. Dropping
/// this owner closes the queue; a blocked network operation never stalls drawing.
pub struct TaskWorker {
    sender: mpsc::SyncSender<TaskRequest>,
    receiver: mpsc::Receiver<(TaskRequest, Result<TaskResponse, String>)>,
    busy: bool,
    next_refresh: Instant,
}
impl TaskWorker {
    pub fn start(send_request: TaskRequestFn) -> std::io::Result<Self> {
        let (sender, requests) = mpsc::sync_channel::<TaskRequest>(1);
        let (responses, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("ovrcr-task-control".into())
            .spawn(move || {
                while let Ok(task_request) = requests.recv() {
                    let result = send_request(task_request.clone()).map_err(|e| format!("{e:#}"));
                    if responses.send((task_request, result)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            sender,
            receiver,
            busy: false,
            next_refresh: Instant::now(),
        })
    }
    pub fn poll(&mut self, view: Option<&mut TasksView>) -> bool {
        let mut changed = false;
        let mut view = view;
        if let Ok((request, result)) = self.receiver.try_recv() {
            self.busy = false;
            if let Some(v) = view.as_deref_mut() {
                v.receive(&request, result);
                changed = true;
            }
        }
        if let Some(v) = view
            && !self.busy
        {
            if Instant::now() >= self.next_refresh {
                v.refresh();
                self.next_refresh = Instant::now() + Duration::from_millis(500);
            }
            if let Some(request) = v.take_request() {
                match self.sender.try_send(request) {
                    Ok(()) => self.busy = true,
                    Err(error) => v.message = error.to_string(),
                }
            }
        }
        changed
    }
}

fn visible_input(text: &str, cursor: usize, width: usize) -> (&str, usize) {
    let before = &text[..cursor];
    let mut column = Line::raw(before).width();
    let mut start = 0;
    for (i, ch) in before.char_indices() {
        if column < width {
            break;
        }
        column = column.saturating_sub(Line::raw(ch.to_string()).width());
        start = i + ch.len_utf8();
    }
    (&text[start..], column)
}

fn block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(Color::Rgb(108, 112, 134)))
}
fn timestamp(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| at.to_string())
}
fn schedule_text(s: &Schedule) -> String {
    match s {
        Schedule::Interval { seconds } => format!("every {seconds}s"),
        Schedule::Cron {
            expression,
            timezone,
        } => format!("{expression} {timezone}"),
        Schedule::Once { at } => timestamp(*at),
    }
}
fn wrap_transcript(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for line in text.split('\n') {
        let mut row = String::new();
        let mut used = 0;
        for ch in line.chars() {
            let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + char_width > width && !row.is_empty() {
                lines.push(std::mem::take(&mut row));
                used = 0;
            }
            row.push(ch);
            used += char_width;
        }
        lines.push(row);
    }
    lines
}

pub fn draw_tasks(frame: &mut Frame<'_>, view: &TasksView) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(
            Style::default()
                .bg(Color::Rgb(30, 30, 46))
                .fg(Color::Rgb(205, 214, 244)),
        ),
        area,
    );
    let [title, body, status, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(format!(
            " OVRCR  Tasks  │ concurrency: {}",
            view.concurrency
        ))
        .style(
            Style::default()
                .fg(Color::Rgb(30, 30, 46))
                .bg(Color::Rgb(203, 166, 247)),
        ),
        title,
    );
    let footer_text;
    if let Some(editor) = &view.editor {
        footer_text = if editor.field == 2 {
            "Esc cancel  Ctrl-s save  ↑/↓ pick  Tab/Enter accept  Shift-Tab back  Ctrl-u clear"
        } else {
            "Esc cancel  Ctrl-s save  Tab/Shift-Tab field  Ctrl-a clear  Enter newline"
        };
        let outer = block(if editor.id.is_some() {
            " Edit task "
        } else {
            " New task "
        });
        let inner = outer.inner(body);
        frame.render_widget(outer, body);
        // Scroll the field list on short terminals so every field remains reachable.
        let prompt_height = inner.height.saturating_sub(11).max(1);
        let first = if inner.height < 14 {
            editor
                .field
                .saturating_sub(inner.height.saturating_sub(3) as usize)
        } else {
            0
        };
        let mut y = inner.y;
        for (i, label) in LABELS.iter().enumerate().skip(first) {
            if y >= inner.bottom() {
                break;
            }
            let height = if i == 11 {
                prompt_height.min(inner.bottom() - y)
            } else {
                1
            };
            let rect = Rect::new(inner.x, y, inner.width, height);
            let selected = editor.field == i;
            let marker = if selected { ">" } else { " " };
            if i == 2 && selected {
                let value = if editor.projects.query.is_empty() {
                    &editor.fields[2]
                } else {
                    &editor.projects.query
                };
                frame.render_widget(Paragraph::new(format!("› Project: {value}▏")), rect);
                let count = inner.bottom().saturating_sub(y + 1).min(8) as usize;
                if count > 0 {
                    let (lines, selected) = editor.projects.lines(count);
                    let height = lines.len() as u16;
                    frame.render_widget(
                        Paragraph::new(lines),
                        Rect::new(inner.x, y + 1, inner.width, height),
                    );
                    frame.set_cursor_position((
                        inner
                            .x
                            .saturating_add(2)
                            .min(inner.right().saturating_sub(1)),
                        y + 1 + selected as u16,
                    ));
                    y += height;
                }
            } else if i == 11 {
                let prefix = format!("{marker} Prompt: ");
                let lines = editor.fields[i].split('\n').collect::<Vec<_>>();
                let cursor_line = if selected {
                    editor.fields[i][..editor.cursor]
                        .bytes()
                        .filter(|b| *b == b'\n')
                        .count()
                } else {
                    0
                };
                let start = if selected {
                    cursor_line.saturating_sub(height.saturating_sub(1) as usize)
                } else {
                    0
                };
                for (row, text) in lines.iter().skip(start).take(height as usize).enumerate() {
                    let text = if selected && start + row == cursor_line {
                        let before = editor.fields[i][..editor.cursor]
                            .rsplit('\n')
                            .next()
                            .unwrap_or("");
                        visible_input(text, before.len(), rect.width.saturating_sub(10) as usize).0
                    } else {
                        text
                    };
                    let line = format!(
                        "{}{text}",
                        if row == 0 {
                            prefix.as_str()
                        } else {
                            "          "
                        }
                    );
                    frame.render_widget(
                        Paragraph::new(line).style(if selected {
                            Style::default().fg(Color::Rgb(137, 220, 235))
                        } else {
                            Style::default()
                        }),
                        Rect::new(rect.x, rect.y + row as u16, rect.width, 1),
                    );
                }
                if selected {
                    let before = &editor.fields[i][..editor.cursor];
                    let before = before.rsplit('\n').next().unwrap_or("");
                    let column = visible_input(
                        lines[cursor_line],
                        before.len(),
                        rect.width.saturating_sub(10) as usize,
                    )
                    .1;
                    frame.set_cursor_position((
                        rect.x
                            .saturating_add(10 + column as u16)
                            .min(rect.right().saturating_sub(1)),
                        rect.y + (cursor_line - start) as u16,
                    ));
                }
            } else {
                let prefix = format!("{marker} {label}: ");
                let (value, column) = if selected {
                    visible_input(
                        &editor.fields[i],
                        editor.cursor,
                        rect.width.saturating_sub(prefix.len() as u16) as usize,
                    )
                } else {
                    (editor.fields[i].as_str(), 0)
                };
                let text = format!("{prefix}{value}");
                frame.render_widget(
                    Paragraph::new(text).style(if selected {
                        Style::default().fg(Color::Rgb(137, 220, 235))
                    } else {
                        Style::default()
                    }),
                    rect,
                );
                if selected {
                    frame.set_cursor_position((
                        rect.x
                            .saturating_add((prefix.len() + column) as u16)
                            .min(rect.right().saturating_sub(1)),
                        rect.y,
                    ));
                }
            }
            y += height;
        }
    } else if view.history {
        footer_text =
            "Esc tasks  ↑/↓ run  PgUp/PgDn transcript  End follow  x cancel run  d cleanup";
        let [history, log] = Layout::vertical([
            Constraint::Length((body.height / 3).max(4)),
            Constraint::Min(1),
        ])
        .areas(body);
        let mut rows = view
            .runs
            .iter()
            .enumerate()
            .skip(
                view.selected_run
                    .saturating_sub(history.height.saturating_sub(3) as usize),
            )
            .map(|(i, r)| {
                Line::from(Span::styled(
                    format!(
                        "{} {:>4}  {:<14}  {}  {}",
                        if i == view.selected_run { ">" } else { " " },
                        r.id.0,
                        format!("{:?}", r.status),
                        timestamp(r.created_at),
                        r.spec.name
                    ),
                    if i == view.selected_run {
                        Style::default().fg(Color::Rgb(137, 220, 235))
                    } else {
                        Style::default()
                    },
                ))
            })
            .collect::<Vec<_>>();
        if rows.is_empty() {
            rows.push(Line::raw(" No runs to display. Esc returns to tasks."));
        }
        frame.render_widget(Paragraph::new(rows).block(block(" Run history ")), history);
        let title = view
            .runs
            .get(view.selected_run)
            .map(|r| {
                format!(
                    " Run {} · {:?} · {} ",
                    r.id.0,
                    r.status,
                    r.directory
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                )
            })
            .unwrap_or_else(|| " Transcript ".into());
        let outer = block(&title);
        let inner = outer.inner(log);
        frame.render_widget(outer, log);
        let mut text = view.transcript.text().to_owned();
        if let Some(error) = view
            .runs
            .get(view.selected_run)
            .and_then(|r| r.error.as_ref())
        {
            text.push_str(&format!("\n[error: {error}]"));
        }
        // Pre-wrap so line scrolling and following work for long streamed paragraphs.
        let lines = view.wrapped_transcript(&text, inner.width);
        let start = lines
            .len()
            .saturating_sub(inner.height as usize)
            .saturating_sub(view.transcript.scroll);
        frame.render_widget(
            Paragraph::new(
                lines
                    .iter()
                    .skip(start)
                    .take(inner.height as usize)
                    .map(|line| Line::raw(line.as_str()))
                    .collect::<Vec<_>>(),
            ),
            inner,
        );
    } else {
        footer_text =
            "Esc back  n new  h/H history/all  e edit  p pause/resume  r run  d del  c limit";
        let mut rows = vec![Line::raw(
            "   ID  STATE    NAME                 SCHEDULE / NEXT (UTC)",
        )];
        rows.extend(
            view.tasks
                .iter()
                .enumerate()
                .skip(
                    view.selected
                        .saturating_sub(body.height.saturating_sub(4) as usize),
                )
                .map(|(i, t)| {
                    Line::from(Span::styled(
                        format!(
                            "{} {:>3}  {:<8} {:<20} {} / {}",
                            if i == view.selected { ">" } else { " " },
                            t.id.0,
                            if t.enabled { "enabled" } else { "paused" },
                            t.spec.name,
                            schedule_text(&t.spec.schedule),
                            t.next_due_at.map(timestamp).unwrap_or_else(|| "—".into())
                        ),
                        if i == view.selected {
                            Style::default().fg(Color::Rgb(137, 220, 235))
                        } else {
                            Style::default()
                        },
                    ))
                }),
        );
        if view.tasks.is_empty() {
            rows.push(Line::raw(
                " No tasks. Press n to create a task; h to inspect all run history.",
            ));
        }
        frame.render_widget(Paragraph::new(rows).block(block(" Scheduled tasks ")), body);
    }
    frame.render_widget(
        Paragraph::new(view.message.as_str()).style(Style::default().fg(Color::Rgb(249, 226, 175))),
        status,
    );
    let mut hints = String::new();
    for hint in footer_text.split("  ") {
        let separator = if hints.is_empty() { "" } else { "  " };
        if hints.width() + separator.len() + hint.width() <= usize::from(footer.width) {
            hints.push_str(separator);
            hints.push_str(hint);
        }
    }
    frame.render_widget(
        Paragraph::new(hints).style(Style::default().fg(Color::Rgb(166, 173, 200))),
        footer,
    );
    if let Some(request) = &view.confirmation {
        let text = match request {
            TaskRequest::Delete(id) => {
                format!("Delete task {}? Queued runs will be cancelled. [y/N]", id.0)
            }
            TaskRequest::Clean { id, .. } => format!(
                "Clean run {} working directory? This removes retained files. [y/N]",
                id.0
            ),
            _ => String::new(),
        };
        frame.render_widget(
            Paragraph::new(text).style(
                Style::default()
                    .fg(Color::Rgb(30, 30, 46))
                    .bg(Color::Rgb(249, 226, 175)),
            ),
            status,
        );
    }
    if let Some(value) = &view.concurrency_input {
        frame.render_widget(
            Paragraph::new(format!("Concurrency: {value}  Enter save · Esc cancel"))
                .style(Style::default().fg(Color::Rgb(137, 220, 235))),
            status,
        );
    }
}

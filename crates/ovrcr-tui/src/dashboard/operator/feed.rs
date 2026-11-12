//! The Operator journal drawn as an operations feed: one entry per request or
//! wake-up, its commands one per line, and the reply beneath them.
use super::render::{Anchor, TranscriptEntry, TranscriptRow};
use super::*;
use crate::dashboard::render::{BLUE, GREEN, MUTED, PEACH, RED, SUBTEXT, TEXT, clip_text};
use ratatui::style::Color;

/// Disclosure keys share one map with journal ordinals. Ordinals are SQLite
/// rowids and never reach these bits.
pub(super) const CONTEXT_KEY: u64 = 1 << 63;
pub(super) const FOLD_KEY: u64 = 1 << 62;

// Anchor parts, in the order an entry draws them. Offsets within a part only
// grow as the journal appends, so a scrolled viewport keeps its row.
const HEAD: usize = 0;
const CONTEXT: usize = 1;
const CONTEXT_DETAIL: usize = 2;
const STEP: usize = 3;
const REPLY: usize = 4;
const OUTCOME: usize = 5;
pub(super) const GAP: usize = 6;

/// A request with more commands than this folds its runs of lookups.
const FOLD_OVER: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Tier {
    Add,
    Remove,
    Change,
    Lookup,
    Other,
}

const OVRCR_NAMES: [&str; 4] = [
    "\"$OVRCR_EXECUTABLE\"",
    "\"${OVRCR_EXECUTABLE}\"",
    "${OVRCR_EXECUTABLE}",
    "$OVRCR_EXECUTABLE",
];

/// Split a shell command at `&&`, `||`, `|`, `;` and newlines outside quotes.
/// Each segment carries the separator that came before it.
fn segments(command: &str) -> Vec<(&'static str, &str)> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut before = "";
    let mut quote = None;
    let mut chars = command.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some('"') if ch == '\\' => {
                chars.next();
            }
            Some(_) => {}
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                '\\' => {
                    chars.next();
                }
                // A lone `&` is a redirect (`2>&1`) or a background job.
                '&' if chars.peek().is_none_or(|(_, next)| *next != '&') => {}
                ';' | '\n' | '|' | '&' => {
                    found.push((before, &command[start..at]));
                    start = at + ch.len_utf8();
                    let doubled =
                        matches!(ch, '|' | '&') && chars.next_if(|(_, next)| *next == ch).is_some();
                    start += usize::from(doubled);
                    before = match (ch, doubled) {
                        ('&', _) => " && ",
                        ('|', true) => " || ",
                        ('|', false) => " | ",
                        _ => "; ",
                    };
                }
                _ => {}
            },
        }
    }
    found.push((before, &command[start..]));
    found.retain(|(_, segment)| !segment.trim().is_empty());
    found
}

fn verb_tier(word: &str) -> Option<Tier> {
    Some(match word {
        "remove" | "close" | "stop" | "archive" | "delete" | "cancel" | "discard"
        | "cloud-cancel" | "cloud-discard" => Tier::Remove,
        "create" | "new" | "add" | "run" | "reopen" | "open" => Tier::Add,
        "send"
        | "keystroke"
        | "set"
        | "reset"
        | "rename"
        | "mark-reviewed"
        | "acknowledge-stopped"
        | "setup" => Tier::Change,
        "list" | "get" | "read" | "screen" | "transcript" | "context" | "usage" | "doctor"
        | "models" | "status" => Tier::Lookup,
        _ => return None,
    })
}

/// `None` is a segment that does nothing on its own, such as `cd`.
// ponytail: recognises commands by their leading words, not by parsing shell.
// An unknown command is `Other`, never a lookup, so nothing destructive is dimmed.
fn segment_tier(segment: &str) -> Option<Tier> {
    let writes = segment
        .replace("2>&1", "")
        .replace("2>/dev/null", "")
        .replace(">/dev/null", "")
        .contains('>');
    let mut words = segment
        .split_whitespace()
        .skip_while(|word| word.contains('=') && !word.starts_with('-'));
    let first = words.next()?;
    let rest: Vec<&str> = words.collect();
    let bare = || rest.iter().filter(|word| !word.starts_with('-'));
    Some(match first {
        "cd" => return None,
        "ovrcr" => {
            if rest
                .iter()
                .any(|word| matches!(*word, "--help" | "-h" | "help"))
            {
                Tier::Lookup
            } else {
                bare()
                    .take(3)
                    .find_map(|word| verb_tier(word))
                    .unwrap_or(Tier::Other)
            }
        }
        _ if writes => Tier::Other,
        "git"
            if bare().take(2).any(|word| {
                matches!(
                    *word,
                    "status" | "diff" | "log" | "show" | "rev-parse" | "ls-files" | "describe"
                )
            }) =>
        {
            Tier::Lookup
        }
        "ls" | "cat" | "grep" | "rg" | "head" | "tail" | "wc" | "which" | "echo" | "pwd" | "jq"
        | "file" | "stat" | "date" | "tree" | "printenv" => Tier::Lookup,
        _ if rest.iter().any(|word| matches!(*word, "--help" | "-h")) => Tier::Lookup,
        _ => Tier::Other,
    })
}

/// One line naming what a tool call did, and whether it changed anything.
pub(super) fn summarize(name: &str, arguments: &str) -> (String, Tier) {
    let value: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
    let field = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| value.get(key).and_then(|value| value.as_str()))
    };
    match name {
        "bash" => {
            let Some(command) = field(&["command"]) else {
                return (format!("bash {arguments}"), Tier::Other);
            };
            let command = OVRCR_NAMES
                .iter()
                .fold(command.to_owned(), |command, name| {
                    command.replace(name, "ovrcr")
                })
                .replace("ovrcr --json", "ovrcr");
            let parts = segments(&command);
            let tiers: Vec<Tier> = parts
                .iter()
                .filter_map(|(_, segment)| segment_tier(segment))
                .collect();
            let tier = [Tier::Remove, Tier::Add, Tier::Change]
                .into_iter()
                .find(|tier| tiers.contains(tier))
                .unwrap_or(
                    if !tiers.is_empty() && tiers.iter().all(|tier| *tier == Tier::Lookup) {
                        Tier::Lookup
                    } else {
                        Tier::Other
                    },
                );
            // `cd` and the `echo` banners between chained commands say nothing
            // about what ran; a command made only of them is shown whole.
            let banner = |segment: &str| {
                segment_tier(segment).is_none()
                    || (segment.split_whitespace().next() == Some("echo") && !segment.contains('>'))
            };
            let shown = if parts.iter().all(|(_, segment)| banner(segment)) {
                parts.iter().collect::<Vec<_>>()
            } else {
                parts
                    .iter()
                    .filter(|(_, segment)| !banner(segment))
                    .collect()
            };
            let text = shown
                .iter()
                .enumerate()
                .map(|(index, (before, segment))| {
                    format!("{}{}", if index == 0 { "" } else { before }, segment.trim())
                })
                .collect::<String>()
                .replace(" 2>&1", "")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            (text, tier)
        }
        "read" => (
            format!(
                "read {}",
                field(&["path", "file_path"]).unwrap_or(arguments)
            ),
            Tier::Lookup,
        ),
        "edit" | "write" => (
            format!(
                "{name} {}",
                field(&["path", "file_path"]).unwrap_or(arguments)
            ),
            Tier::Change,
        ),
        "watch_sessions" => (
            format!("watch_sessions {arguments}"),
            match field(&["action"]) {
                Some("add") => Tier::Add,
                Some("remove") => Tier::Remove,
                Some("list") => Tier::Lookup,
                _ => Tier::Other,
            },
        ),
        _ => (format!("{name} {arguments}"), Tier::Other),
    }
}

/// Word-wrap one logical line. Each row keeps its byte offset in `text`; a
/// word wider than the row breaks between graphemes.
pub(super) fn wrap_words(text: &str, width: usize) -> Vec<(String, usize)> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut start = 0;
    let mut used = 0;
    // Row bytes and cells through the last space that follows a word.
    let mut gap: Option<(usize, usize)> = None;
    for grapheme in Span::raw(text).styled_graphemes(Style::default()) {
        let cells = grapheme.symbol.width();
        if used + cells > width && !row.is_empty() {
            let (bytes, kept) = gap.take().unwrap_or((row.len(), used));
            let rest = row.split_off(bytes);
            rows.push((std::mem::replace(&mut row, rest), start));
            start += bytes;
            used -= kept;
        }
        row.push_str(grapheme.symbol);
        used += cells;
        if grapheme.symbol == " " && !row.trim().is_empty() {
            gap = Some((row.len(), used));
        }
    }
    rows.push((row, start));
    rows
}

enum Opener<'a> {
    Request {
        text: &'a str,
        context: &'a OperatorContext,
        /// The captured target differs from the previous request's.
        retargeted: bool,
    },
    Wake(&'a str),
    /// Worker events with no request before them, as in a legacy journal.
    Orphan,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Running,
    Done,
    Failed,
    Interrupted,
}

struct Step<'a> {
    ordinal: u64,
    id: &'a str,
    name: &'a str,
    arguments: &'a str,
    result: Option<(&'a OperatorToolStatus, &'a str, bool)>,
}

struct Entry<'a> {
    owner: u64,
    turn: OperatorTurnId,
    opener: Opener<'a>,
    steps: Vec<Step<'a>>,
    reply: String,
    replying: bool,
    /// The thinking that trails the journal; shown only while the turn runs.
    thought: String,
    failure: Option<&'a str>,
    state: State,
    /// The journal itself recorded how the turn ended.
    recorded: bool,
}

fn entries(chat: &Chat) -> Vec<Entry<'_>> {
    let mut entries: Vec<Entry<'_>> = Vec::new();
    let mut previous_context = None;
    for message in &chat.messages {
        let opener = match &message.content {
            // The selected model is retained metadata and compaction events are
            // transient phase state; neither is part of the feed.
            OperatorMessageContent::Worker(
                OperatorWorkerEvent::ModelSelected(_)
                | OperatorWorkerEvent::ReasoningSelected(_)
                | OperatorWorkerEvent::CompactionStarted
                | OperatorWorkerEvent::CompactionEnded,
            ) => continue,
            OperatorMessageContent::User { text, context } => {
                let retargeted = previous_context != Some(context);
                previous_context = Some(context);
                Some(Opener::Request {
                    text,
                    context,
                    retargeted,
                })
            }
            OperatorMessageContent::AsyncWakeup { text } => Some(Opener::Wake(text)),
            _ if entries
                .last()
                .is_none_or(|entry| entry.turn != message.turn) =>
            {
                Some(Opener::Orphan)
            }
            _ => None,
        };
        let opened = opener.is_some();
        if let Some(opener) = opener {
            entries.push(Entry {
                owner: message.ordinal,
                turn: message.turn,
                opener,
                steps: Vec::new(),
                reply: String::new(),
                replying: false,
                thought: String::new(),
                failure: None,
                state: State::Done,
                recorded: false,
            });
        }
        let entry = entries.last_mut().expect("an entry was opened above");
        let thinking = matches!(
            message.content,
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(_))
        );
        if !thinking && !opened {
            entry.thought.clear();
        }
        let replying = std::mem::take(&mut entry.replying);
        match &message.content {
            OperatorMessageContent::Worker(OperatorWorkerEvent::AssistantDelta(delta)) => {
                // Text on either side of a command reads as separate paragraphs.
                if !replying && !entry.reply.is_empty() {
                    let kept = entry.reply.trim_end_matches('\n').len();
                    entry.reply.truncate(kept);
                    entry.reply.push_str("\n\n");
                }
                entry.reply.push_str(delta);
                entry.replying = true;
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::ThinkingDelta(delta)) => {
                entry.thought.push_str(delta);
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolCall {
                id,
                name,
                arguments,
            }) => entry.steps.push(Step {
                ordinal: message.ordinal,
                id,
                name,
                arguments,
                result: None,
            }),
            OperatorMessageContent::Worker(OperatorWorkerEvent::ToolOutput {
                id,
                output,
                truncated,
                status,
            }) => {
                let result = Some((status, output.as_str(), *truncated));
                match entry
                    .steps
                    .iter_mut()
                    .rev()
                    .find(|step| step.id == id && step.result.is_none())
                {
                    Some(step) => step.result = result,
                    None => entry.steps.push(Step {
                        ordinal: message.ordinal,
                        id,
                        name: "tool",
                        arguments: "",
                        result,
                    }),
                }
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::Completed { .. }) => {
                (entry.state, entry.recorded) = (State::Done, true);
            }
            OperatorMessageContent::Worker(OperatorWorkerEvent::Failed(message)) => {
                (entry.state, entry.recorded) = (State::Failed, true);
                entry.failure = Some(message);
            }
            OperatorMessageContent::Interrupted => {
                (entry.state, entry.recorded) = (State::Interrupted, true);
            }
            _ => {}
        }
    }
    // A finished turn journals nothing; only a failure or an interruption is a
    // row. So a turn with neither is done unless the summary says otherwise.
    let current = chat.summary.current_turn.as_ref();
    for entry in &mut entries {
        if entry.recorded {
            continue;
        }
        entry.state = match current.filter(|turn| turn.identity.turn == entry.turn) {
            // An older turn with no failure or interruption on record finished.
            None => State::Done,
            Some(turn) => match turn.state {
                OperatorTurnState::Running => State::Running,
                OperatorTurnState::Complete => State::Done,
                OperatorTurnState::Error => State::Failed,
                OperatorTurnState::Interrupted => State::Interrupted,
            },
        };
    }
    entries
}

fn wake_title(text: &str) -> String {
    let sessions: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let (id, rest) = line.strip_prefix("- session ")?.split_once(' ')?;
            let label = rest.strip_prefix('(')?.split_once(')')?.0;
            Some(format!("#{id} {label}"))
        })
        .collect();
    if sessions.is_empty() {
        format!("Woke · {}", text.lines().next().unwrap_or_default().trim())
    } else {
        format!("Woke · {}", sessions.join(", "))
    }
}

fn bar_row(
    color: Color,
    body: Vec<Span<'static>>,
    anchor: Anchor,
    stop: Option<TranscriptEntry>,
) -> TranscriptRow {
    let mut spans = vec![Span::styled("▌ ", Style::default().fg(color))];
    spans.extend(body);
    TranscriptRow {
        line: Line::from(spans),
        anchor: Some(anchor),
        entry: stop,
    }
}

/// `left`, then `right` flush against the row's edge when both fit.
fn spread(left: Span<'static>, right: Span<'static>, room: usize) -> Vec<Span<'static>> {
    let pad = room.saturating_sub(left.width() + right.width());
    vec![left, Span::raw(" ".repeat(pad)), right]
}

impl Entry<'_> {
    fn mark(&self) -> (char, Color) {
        match (&self.opener, self.state) {
            (_, State::Running) => ('●', BLUE()),
            (_, State::Failed) => ('✕', RED()),
            (_, State::Interrupted) => ('■', PEACH()),
            (Opener::Wake(_), _) => ('◆', PEACH()),
            (_, State::Done) => ('✓', GREEN()),
        }
    }

    fn title(&self) -> String {
        match &self.opener {
            Opener::Request { text, .. } => display_text(text),
            Opener::Wake(text) => display_text(&wake_title(text)),
            Opener::Orphan => "Operator".into(),
        }
    }

    /// What a collapsed entry says about how it went.
    fn outcome(&self, tiers: &[Tier]) -> (String, Color) {
        match self.state {
            State::Failed => (
                display_text(
                    self.failure
                        .and_then(|failure| failure.lines().next())
                        .unwrap_or("failed"),
                ),
                RED(),
            ),
            State::Interrupted => ("interrupted".into(), PEACH()),
            State::Running | State::Done => {
                let count =
                    |n: usize, noun: &str| format!("{n} {noun}{}", if n == 1 { "" } else { "s" });
                let changes = tiers
                    .iter()
                    .filter(|tier| matches!(tier, Tier::Add | Tier::Remove | Tier::Change))
                    .count();
                let mut parts = Vec::new();
                if self.state == State::Running {
                    parts.push("working".to_owned());
                }
                if changes > 0 {
                    parts.push(count(changes, "change"));
                }
                if !tiers.is_empty() {
                    parts.push(count(tiers.len(), "command"));
                }
                (parts.join(" · "), SUBTEXT())
            }
        }
    }

    fn push(&self, chat: &Chat, width: usize, open: bool, rows: &mut Vec<TranscriptRow>) {
        let (glyph, color) = self.mark();
        let summaries: Vec<(String, Tier)> = self
            .steps
            .iter()
            .map(|step| summarize(step.name, step.arguments))
            .collect();
        let stop = Some(TranscriptEntry {
            ordinal: self.owner,
            expanded: open,
        });
        let lead = Span::styled(
            format!("{glyph} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        );
        let title = self.title();
        let room = width.saturating_sub(4);
        if !open {
            let tiers: Vec<Tier> = summaries.iter().map(|(_, tier)| *tier).collect();
            let (outcome, outcome_color) = self.outcome(&tiers);
            let outcome = clip_text(&outcome, room / 2);
            // The request keeps at least eight cells before an outcome joins it.
            let outcome = if outcome.width() + 10 <= room {
                outcome
            } else {
                String::new()
            };
            let first = title
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or_default()
                .trim();
            let reserved = if outcome.is_empty() {
                0
            } else {
                outcome.width() + 2
            };
            let mut body = vec![lead];
            body.extend(spread(
                Span::styled(
                    clip_text(first, room.saturating_sub(reserved)),
                    Style::default().fg(TEXT()),
                ),
                Span::styled(outcome, Style::default().fg(outcome_color)),
                room,
            ));
            rows.push(bar_row(color, body, (self.owner, HEAD, 0), stop));
            return;
        }

        let bold = Style::default().fg(TEXT()).add_modifier(Modifier::BOLD);
        let mut byte = 0;
        let mut lead = Some(lead);
        for line in title.split('\n') {
            for (text, start) in wrap_words(line, room) {
                rows.push(bar_row(
                    color,
                    vec![
                        lead.take().unwrap_or_else(|| Span::raw("  ")),
                        Span::styled(text, bold),
                    ],
                    (self.owner, HEAD, byte + start),
                    stop,
                ));
            }
            byte += line.len() + 1;
        }

        let quiet = Style::default().fg(SUBTEXT());
        let lines = |rows: &mut Vec<TranscriptRow>,
                     text: &str,
                     indent: usize,
                     style: Style,
                     part: usize,
                     base: usize,
                     stop: Option<TranscriptEntry>| {
            let mut byte = 0;
            for line in text.split('\n') {
                for (text, start) in wrap_words(line, width.saturating_sub(2 + indent)) {
                    rows.push(bar_row(
                        color,
                        vec![Span::raw(" ".repeat(indent)), Span::styled(text, style)],
                        (self.owner, part, base + byte + start),
                        stop,
                    ));
                }
                byte += line.len() + 1;
            }
        };

        match &self.opener {
            Opener::Request {
                context,
                retargeted: true,
                ..
            } => {
                let key = self.owner | CONTEXT_KEY;
                let expanded = chat.expanded.get(&key) == Some(&true);
                let stop = Some(TranscriptEntry {
                    ordinal: key,
                    expanded,
                });
                rows.push(bar_row(
                    color,
                    vec![
                        Span::raw("  "),
                        Span::styled(
                            clip_text(&format!("→ {}", context_label(context)), room),
                            quiet,
                        ),
                    ],
                    (self.owner, CONTEXT, 0),
                    stop,
                ));
                if expanded {
                    // The captured target is per request; the agent working
                    // directory is fixed for the conversation's lifetime.
                    let detail = format!(
                        "project: {}\nworkspace: {}\nworkspace id: {}\ncaptured cwd: {}\nagent workdir: {}\nsession: {} · run: {}",
                        context.project.as_deref().unwrap_or("global"),
                        context.workspace.as_deref().unwrap_or("none"),
                        context.workspace_id.as_deref().unwrap_or("none"),
                        context.cwd.as_ref().map_or_else(
                            || "none".into(),
                            |cwd| cwd.to_string_lossy().into_owned()
                        ),
                        chat.workdir.as_ref().map_or_else(
                            || "fixed operator workdir".into(),
                            |workdir| workdir.to_string_lossy().into_owned()
                        ),
                        context
                            .session
                            .map_or_else(|| "none".into(), |id| id.0.to_string()),
                        context
                            .run
                            .map_or_else(|| "none".into(), |id| id.0.to_string())
                    );
                    lines(
                        rows,
                        &display_text(&detail),
                        4,
                        quiet,
                        CONTEXT_DETAIL,
                        0,
                        stop,
                    );
                }
            }
            Opener::Wake(text) => lines(rows, &display_text(text), 2, quiet, CONTEXT, 0, None),
            _ => {}
        }

        let fold = self.steps.len() > FOLD_OVER;
        let foldable = |index: usize| {
            summaries[index].1 == Tier::Lookup
                && matches!(
                    self.steps[index].result,
                    Some((OperatorToolStatus::Succeeded, _, _))
                )
        };
        let mut index = 0;
        while index < self.steps.len() {
            let run = if fold {
                (index..self.steps.len())
                    .take_while(|index| foldable(*index))
                    .count()
            } else {
                0
            };
            let shown = if run >= 2 {
                let key = self.steps[index].ordinal | FOLD_KEY;
                let unfolded = chat.expanded.get(&key) == Some(&true);
                rows.push(bar_row(
                    color,
                    vec![Span::styled(
                        format!("    {} {run} lookups", if unfolded { "▾" } else { "▸" }),
                        quiet,
                    )],
                    (self.owner, STEP, index << 32),
                    Some(TranscriptEntry {
                        ordinal: key,
                        expanded: unfolded,
                    }),
                ));
                if unfolded { index..index + run } else { 0..0 }
            } else {
                index..index + 1
            };
            for at in shown {
                self.push_step(chat, width, at, &summaries[at], color, rows);
            }
            index += run.max(1);
        }

        let reply = display_text(self.reply.trim_end());
        if !reply.is_empty() {
            if !self.steps.is_empty() {
                rows.push(bar_row(color, Vec::new(), (self.owner, REPLY, 0), None));
            }
            lines(rows, &reply, 2, Style::default().fg(TEXT()), REPLY, 1, None);
        }
        match self.state {
            State::Failed => lines(
                rows,
                &display_text(self.failure.unwrap_or("failed")),
                2,
                Style::default().fg(RED()),
                OUTCOME,
                0,
                None,
            ),
            State::Interrupted => lines(
                rows,
                "Interrupted · operations may finish; file changes remain.",
                2,
                Style::default().fg(PEACH()),
                OUTCOME,
                0,
                None,
            ),
            State::Running => {
                if let Some(thought) = self
                    .thought
                    .lines()
                    .rev()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                {
                    rows.push(bar_row(
                        color,
                        vec![
                            Span::raw("    "),
                            Span::styled(
                                clip_text(&display_text(thought), width.saturating_sub(6)),
                                Style::default().fg(MUTED()).add_modifier(Modifier::ITALIC),
                            ),
                        ],
                        (self.owner, OUTCOME, 0),
                        None,
                    ));
                }
            }
            State::Done => {}
        }
    }

    fn push_step(
        &self,
        chat: &Chat,
        width: usize,
        index: usize,
        (text, tier): &(String, Tier),
        color: Color,
        rows: &mut Vec<TranscriptRow>,
    ) {
        let step = &self.steps[index];
        let expanded = chat.expanded.get(&step.ordinal) == Some(&true);
        let stop = Some(TranscriptEntry {
            ordinal: step.ordinal,
            expanded,
        });
        let (marker, style) = match tier {
            Tier::Add => ("+", Style::default().fg(PEACH())),
            Tier::Remove => ("−", Style::default().fg(PEACH())),
            Tier::Change => ("›", Style::default().fg(PEACH())),
            Tier::Lookup => (" ", Style::default().fg(SUBTEXT())),
            Tier::Other => (" ", Style::default().fg(TEXT())),
        };
        let (status, status_color) = match step.result {
            None if self.state == State::Running => ("running".to_owned(), BLUE()),
            None => ("no result".to_owned(), SUBTEXT()),
            Some((OperatorToolStatus::Succeeded, _, _)) => (String::new(), SUBTEXT()),
            Some((OperatorToolStatus::Failed { exit_code }, _, _)) => (
                exit_code.map_or_else(|| "failed".into(), |code| format!("exit {code}")),
                RED(),
            ),
            Some((OperatorToolStatus::TimedOut, _, _)) => ("timed out".to_owned(), RED()),
        };
        let room = width.saturating_sub(6);
        let reserved = if status.is_empty() {
            0
        } else {
            status.width() + 2
        };
        let mut body = vec![Span::raw("  "), Span::styled(format!("{marker} "), style)];
        body.extend(spread(
            Span::styled(
                clip_text(&display_text(text), room.saturating_sub(reserved)),
                style,
            ),
            Span::styled(status, Style::default().fg(status_color)),
            room,
        ));
        let at = index << 32;
        rows.push(bar_row(color, body, (self.owner, STEP, at | 1), stop));
        if !expanded {
            return;
        }
        // Expanded: the whole shell command after `$`, or another tool's
        // arguments as they were sent, then the output.
        let value: serde_json::Value = serde_json::from_str(step.arguments).unwrap_or_default();
        let mut detail = vec![(
            match value.get("command").and_then(|command| command.as_str()) {
                Some(command) => format!("$ {}", display_text(command)),
                None => display_text(step.arguments),
            },
            Style::default().fg(TEXT()),
        )];
        if let Some((_, output, truncated)) = step.result {
            detail.push((
                if output.is_empty() {
                    "(no output)".into()
                } else {
                    display_text(output.trim_end_matches('\n'))
                },
                Style::default().fg(SUBTEXT()),
            ));
            if truncated {
                detail.push(("… truncated at 64 KiB".into(), Style::default().fg(MUTED())));
            }
        }
        let mut row = 2;
        for (text, style) in detail {
            for line in text.split('\n') {
                for (text, _) in wrap_words(line, width.saturating_sub(8)) {
                    rows.push(bar_row(
                        color,
                        vec![Span::raw("      "), Span::styled(text, style)],
                        (self.owner, STEP, at | row),
                        stop,
                    ));
                    row += 1;
                }
            }
        }
    }
}

/// Every durable row of the feed. The newest entry, a running entry and an
/// unread reply are open; an explicit choice always wins.
pub(super) fn feed_rows(chat: &Chat, width: u16) -> Vec<TranscriptRow> {
    let width = usize::from(width.max(1));
    let entries = entries(chat);
    let opens: Vec<bool> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            chat.expanded.get(&entry.owner).copied().unwrap_or(
                index + 1 == entries.len()
                    || entry.state == State::Running
                    || chat.unread == Some(entry.turn),
            )
        })
        .collect();
    let mut rows = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        entry.push(chat, width, opens[index], &mut rows);
        // Collapsed neighbours sit together; an open entry gets room around it.
        if opens[index] || opens.get(index + 1).copied().unwrap_or(true) {
            rows.push(TranscriptRow {
                line: Line::default(),
                anchor: Some((entry.owner, GAP, 0)),
                entry: None,
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bash(command: &str) -> (String, Tier) {
        summarize(
            "bash",
            &serde_json::json!({"command": command, "timeout": 30}).to_string(),
        )
    }

    #[test]
    fn commands_are_named_by_what_they_run_and_whether_they_change_anything() {
        for (command, text, tier) in [
            (
                "cd /repo && \"$OVRCR_EXECUTABLE\" --json workspace list",
                "ovrcr workspace list",
                Tier::Lookup,
            ),
            (
                "$OVRCR_EXECUTABLE workspace create ovrcr issue-1",
                "ovrcr workspace create ovrcr issue-1",
                Tier::Add,
            ),
            (
                "ovrcr workspace remove issue-1 --force 2>&1",
                "ovrcr workspace remove issue-1 --force",
                Tier::Remove,
            ),
            (
                "ovrcr workspace get --branch x 2>&1; echo \"=== active; now ===\"; ovrcr terminal list",
                "ovrcr workspace get --branch x; ovrcr terminal list",
                Tier::Lookup,
            ),
            ("echo started", "echo started", Tier::Lookup),
            (
                "ovrcr list || ovrcr doctor",
                "ovrcr list || ovrcr doctor",
                Tier::Lookup,
            ),
            (
                "ovrcr terminal send 13 \"please remove; then stop && wait\"",
                "ovrcr terminal send 13 \"please remove; then stop && wait\"",
                Tier::Change,
            ),
            (
                "ovrcr agent run claude --workspace main",
                "ovrcr agent run claude --workspace main",
                Tier::Add,
            ),
            (
                "ovrcr workspace --help",
                "ovrcr workspace --help",
                Tier::Lookup,
            ),
            (
                "ovrcr list && ovrcr terminal close 446",
                "ovrcr list && ovrcr terminal close 446",
                Tier::Remove,
            ),
            (
                "git -C /repo status --short | head -5",
                "git -C /repo status --short | head -5",
                Tier::Lookup,
            ),
            (
                "claude --help 2>&1 | head",
                "claude --help | head",
                Tier::Lookup,
            ),
            // Unknown commands and writes are never dimmed as lookups.
            ("rm -rf build", "rm -rf build", Tier::Other),
            ("echo done > marker", "echo done > marker", Tier::Other),
            ("git commit -m wip", "git commit -m wip", Tier::Other),
            (
                "sqlite3 db \"SELECT a; SELECT b\"",
                "sqlite3 db \"SELECT a; SELECT b\"",
                Tier::Other,
            ),
            ("ls\ncat notes", "ls; cat notes", Tier::Lookup),
        ] {
            assert_eq!(bash(command), (text.to_owned(), tier), "{command}");
        }
        assert_eq!(
            summarize("read", r#"{"path":"/tmp/notes.md"}"#),
            ("read /tmp/notes.md".to_owned(), Tier::Lookup)
        );
        assert_eq!(
            summarize("write", r#"{"path":"/tmp/notes.md","content":"x"}"#),
            ("write /tmp/notes.md".to_owned(), Tier::Change)
        );
        assert_eq!(
            summarize("watch_sessions", r#"{"action":"add"}"#).1,
            Tier::Add
        );
        // Arguments that are not the expected JSON still produce a row.
        assert_eq!(
            summarize("bash", "not json"),
            ("bash not json".to_owned(), Tier::Other)
        );
    }

    #[test]
    fn words_wrap_at_spaces_and_long_words_break_between_graphemes() {
        let rows = wrap_words("the worktree has an uncommitted change", 16);
        assert_eq!(
            rows,
            [
                ("the worktree ".to_owned(), 0),
                ("has an ".to_owned(), 13),
                ("uncommitted ".to_owned(), 20),
                ("change".to_owned(), 32),
            ]
        );
        assert_eq!(
            wrap_words("abcdefghij", 4),
            [
                ("abcd".to_owned(), 0),
                ("efgh".to_owned(), 4),
                ("ij".to_owned(), 8)
            ]
        );
        // A wide grapheme never straddles the edge, and offsets stay in bytes.
        assert_eq!(
            wrap_words("ab 界界界", 5),
            [
                ("ab ".to_owned(), 0),
                ("界界".to_owned(), 3),
                ("界".to_owned(), 9)
            ]
        );
        // Indentation is kept and is not a break opportunity on its own.
        assert_eq!(
            wrap_words("    indented", 8),
            [("    inde".to_owned(), 0), ("nted".to_owned(), 8)]
        );
        assert_eq!(wrap_words("", 8), [(String::new(), 0)]);
    }
}

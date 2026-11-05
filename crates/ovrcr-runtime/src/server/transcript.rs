//! Read-only pages of a session's current conversation for the Dashboard's Transcript
//! view, read from the provider's own transcript file (Claude Code JSONL, Codex rollout
//! JSONL). The file is the one the session's retained conversation reference names; this
//! never searches for another. Pages are read backwards from `before` in bounded windows.

use super::title::{Candidate, first_line_matches_file};
use super::*;
use ovrcr_protocol::{
    AgentProvider, ConversationReference, SessionRunId, TranscriptEntry, TranscriptPage,
};
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;

/// Raw transcript bytes read per window. A record longer than this is skipped.
const WINDOW: u64 = 512 * 1024;
/// Stop widening a page that has found no entries after this many raw bytes.
const SCAN_LIMIT: u64 = 8 * WINDOW;
/// Longest prose kept per entry; the rest is cut with `…`.
const TEXT_LIMIT: usize = 16 * 1024;
/// Longest tool-input summary, in characters.
const INPUT_LIMIT: usize = 160;

pub(super) fn page(
    state: &ServerState,
    session: SessionId,
    run: SessionRunId,
    identity: Option<ConversationReference>,
    before: Option<u64>,
) -> Response {
    let selected = {
        let retained = state.retained.lock();
        let Some(record) = retained.get(session) else {
            return error_response(ErrorCode::NotFound, "session not found");
        };
        (record.run == run && !record.identity_invalid)
            .then(|| {
                Some((
                    Candidate::from_record(record)?,
                    record.conversation.clone()?,
                ))
            })
            .flatten()
    };
    let Some((candidate, reference)) = selected.filter(|(candidate, _)| {
        matches!(
            candidate.provider,
            AgentProvider::Claude | AgentProvider::Codex
        )
    }) else {
        return error_response(ErrorCode::NotFound, "no transcript for this session");
    };
    if before.is_some() && identity.as_ref() != Some(&reference) {
        return error_response(
            ErrorCode::Conflict,
            "The conversation changed; reopen Transcript.",
        );
    }
    let (entries, start) = match read_entries(&candidate, before) {
        Ok(page) => page,
        Err(error) => {
            return error_response(
                ErrorCode::NotFound,
                format!("transcript unavailable: {error}"),
            );
        }
    };
    // File I/O never holds retained authority; reject a changed run/reference afterwards.
    let retained = state.retained.lock();
    if !retained.get(session).is_some_and(|record| {
        record.run == run
            && !record.identity_invalid
            && record.conversation.as_ref() == Some(&reference)
    }) {
        return error_response(
            ErrorCode::Conflict,
            "The conversation changed; reopen Transcript.",
        );
    }
    Response::Transcript(TranscriptPage {
        entries,
        start,
        identity: reference,
    })
}

fn read_entries(candidate: &Candidate, before: Option<u64>) -> Result<(Vec<TranscriptEntry>, u64)> {
    // Never block the connection on a FIFO or device someone put at the path.
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&candidate.history)?;
    if !file.metadata()?.is_file() {
        bail!("not a regular file");
    }
    if candidate.provider == AgentProvider::Codex
        && !first_line_matches_file(&mut file, |value| {
            value["type"] == "session_meta"
                && value["payload"]["id"].as_str() == Some(candidate.conversation.as_str())
        })?
    {
        bail!("the file names another conversation");
    }
    let len = file.metadata()?.len();
    let mut end = before.unwrap_or(len).min(len);
    // The bytes just before `end` are the middle of one record longer than a window.
    let mut inside_record = false;
    let mut scanned = 0;
    loop {
        if end == 0 {
            return Ok((Vec::new(), 0));
        }
        if scanned >= SCAN_LIMIT {
            return Ok((Vec::new(), end));
        }
        let start = end.saturating_sub(WINDOW);
        let buffer = read_range(&mut file, start, end)?;
        scanned += end - start;
        let mut body = &buffer[..];
        if inside_record {
            body = &body[..body
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |at| at + 1)];
        }
        // Provider JSONL writes can be observed between two writes. Never show an
        // unterminated tail, even when its current bytes happen to form valid JSON.
        if body.last().is_some_and(|byte| *byte != b'\n') {
            body = &body[..body
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(0, |at| at + 1)];
        }
        let mut page_start = start;
        if start > 0 {
            // The first line began before this window, so it is incomplete here.
            let first = body.iter().position(|byte| *byte == b'\n');
            let Some(at) = first.filter(|at| at + 1 < body.len()) else {
                // No complete line: the window is inside one oversized record. Skip it.
                end = start;
                inside_record = true;
                if scanned >= SCAN_LIMIT {
                    return Ok((Vec::new(), start));
                }
                continue;
            };
            body = &body[at + 1..];
            page_start = start + at as u64 + 1;
        }
        let entries = parse(candidate, body);
        if !entries.is_empty() || page_start == 0 || scanned >= SCAN_LIMIT {
            return Ok((entries, page_start));
        }
        end = page_start;
        inside_record = false;
    }
}

fn read_range(file: &mut File, start: u64, end: u64) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start(start))?;
    let mut buffer = Vec::new();
    file.take(end - start).read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// Entries of the complete lines in `body`, oldest first. Malformed lines, lines of
/// another conversation and records the view does not show are skipped.
fn parse(candidate: &Candidate, body: &[u8]) -> Vec<TranscriptEntry> {
    let mut entries = Vec::new();
    for line in body.split(|byte| *byte == b'\n') {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        match candidate.provider {
            AgentProvider::Claude => claude(&value, &candidate.conversation, &mut entries),
            _ => codex(&value, &mut entries),
        }
    }
    entries
}

fn claude(value: &Value, conversation: &str, entries: &mut Vec<TranscriptEntry>) {
    if value["sessionId"].as_str() != Some(conversation)
        || ["isSidechain", "isMeta", "isCompactSummary"]
            .iter()
            .any(|flag| value[flag] == true)
    {
        return;
    }
    let message = &value["message"];
    let user = match value["type"].as_str() {
        Some("user") => true,
        Some("assistant") => false,
        _ => return,
    };
    let prose = |text: &str| {
        let text = clip(text.trim(), TEXT_LIMIT);
        if user {
            TranscriptEntry::User(text)
        } else {
            TranscriptEntry::Assistant(text)
        }
    };
    if let Some(text) = message["content"].as_str() {
        if !text.trim().is_empty() {
            entries.push(prose(text));
        }
        return;
    }
    for part in message["content"].as_array().into_iter().flatten() {
        match part["type"].as_str() {
            Some("text") => {
                if let Some(text) = part["text"].as_str().filter(|text| !text.trim().is_empty()) {
                    entries.push(prose(text));
                }
            }
            Some("tool_use") if !user => entries.push(TranscriptEntry::Tool {
                name: one_line(part["name"].as_str().unwrap_or("tool")),
                input: summary(&part["input"]),
            }),
            _ => {}
        }
    }
}

/// Codex keeps the prompts and replies the user saw as `event_msg` records; its
/// `response_item` messages repeat them alongside injected context, so only tool calls
/// are read from those.
fn codex(value: &Value, entries: &mut Vec<TranscriptEntry>) {
    let payload = &value["payload"];
    match (value["type"].as_str(), payload["type"].as_str()) {
        (Some("event_msg"), Some(kind @ ("user_message" | "agent_message"))) => {
            let Some(text) = payload["message"]
                .as_str()
                .filter(|text| !text.trim().is_empty())
            else {
                return;
            };
            let text = clip(text.trim(), TEXT_LIMIT);
            entries.push(if kind == "user_message" {
                TranscriptEntry::User(text)
            } else {
                TranscriptEntry::Assistant(text)
            });
        }
        (Some("response_item"), Some("function_call")) => {
            let arguments = payload["arguments"].as_str().unwrap_or_default();
            let input = serde_json::from_str::<Value>(arguments)
                .map_or_else(|_| one_line(arguments), |value| summary(&value));
            entries.push(TranscriptEntry::Tool {
                name: one_line(payload["name"].as_str().unwrap_or("tool")),
                input,
            });
        }
        (Some("response_item"), Some("custom_tool_call")) => entries.push(TranscriptEntry::Tool {
            name: one_line(payload["name"].as_str().unwrap_or("tool")),
            input: summary(&payload["input"]),
        }),
        (Some("response_item"), Some("web_search_call")) => entries.push(TranscriptEntry::Tool {
            name: "web search".into(),
            input: summary(&payload["action"]),
        }),
        (Some("response_item"), Some("tool_search_call")) => entries.push(TranscriptEntry::Tool {
            name: "tool search".into(),
            input: summary(&payload["arguments"]),
        }),
        (Some("response_item"), Some("image_generation_call")) => {
            entries.push(TranscriptEntry::Tool {
                name: "image generation".into(),
                input: summary(&payload["revised_prompt"]),
            })
        }
        (Some("response_item"), Some("local_shell_call")) => entries.push(TranscriptEntry::Tool {
            name: "shell".into(),
            input: summary(&payload["action"]),
        }),
        _ => {}
    }
}

/// The one input field that says what a call does, on one line.
fn summary(input: &Value) -> String {
    let field = [
        "command",
        "cmd",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
        "prompt",
    ]
    .iter()
    .find_map(|key| input.get(key).filter(|value| !value.is_null()));
    match field.unwrap_or(input) {
        Value::String(text) => one_line(text),
        Value::Array(parts) if parts.iter().all(Value::is_string) => one_line(
            &parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Value::Null => String::new(),
        other => one_line(&other.to_string()),
    }
}

fn one_line(text: &str) -> String {
    let first = text.trim().lines().next().unwrap_or_default();
    let more = text.trim().lines().nth(1).is_some();
    let mut line = clip(first, INPUT_LIMIT);
    if more && !line.ends_with('…') {
        line.push_str(" …");
    }
    line
}

fn clip(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const CLAUDE: &str = "5ebc5f9b-54b5-4928-9955-dc81c23743dd";
    const CODEX: &str = "01a08e5c-7480-7052-9964-9224aadebef0";

    fn candidate(provider: AgentProvider, conversation: &str, history: PathBuf) -> Candidate {
        Candidate {
            provider,
            conversation: conversation.into(),
            history,
        }
    }

    fn claude_line(kind: &str, content: Value) -> String {
        serde_json::json!({"type": kind, "sessionId": CLAUDE, "message": {"role": kind, "content": content}})
            .to_string()
    }

    fn write(lines: &[String]) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.jsonl");
        let mut file = File::create(&path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
        (directory, path)
    }

    fn all_pages(candidate: &Candidate) -> Vec<Vec<TranscriptEntry>> {
        let mut pages = Vec::new();
        let mut before = None;
        loop {
            let page = read_entries(candidate, before).unwrap();
            pages.push(page.0);
            if page.1 == 0 {
                break;
            }
            before = Some(page.1);
        }
        pages.reverse();
        pages
    }

    #[test]
    fn claude_prompts_replies_and_one_line_per_tool_call() {
        let (_directory, path) = write(&[
            claude_line("user", "fix the login redirect".into()),
            "{not json".into(),
            claude_line(
                "assistant",
                serde_json::json!([
                    {"type": "thinking", "thinking": "private"},
                    {"type": "text", "text": "Looking at the middleware."},
                    {"type": "tool_use", "name": "Bash", "input": {"command": "rg -n redirect\nsrc", "description": "find it"}},
                    {"type": "tool_use", "name": "Read\nInjected", "input": {"file_path": "/src/auth.rs"}},
                ]),
            ),
            claude_line(
                "user",
                serde_json::json!([{"type": "tool_result", "content": "TOOL OUTPUT"}]),
            ),
            serde_json::json!({"type": "user", "sessionId": "other", "message": {"role": "user", "content": "OTHER CONVERSATION"}}).to_string(),
            serde_json::json!({"type": "user", "sessionId": CLAUDE, "isMeta": true, "message": {"role": "user", "content": "META"}}).to_string(),
            serde_json::json!({"type": "assistant", "sessionId": CLAUDE, "isSidechain": true, "message": {"role": "assistant", "content": [{"type": "text", "text": "SIDECHAIN"}]}}).to_string(),
            claude_line("assistant", serde_json::json!([{"type": "text", "text": "Fixed: \u{1b}]0;title\u{7} done"}])),
        ]);
        let page = read_entries(&candidate(AgentProvider::Claude, CLAUDE, path), None).unwrap();
        assert_eq!(page.1, 0);
        assert_eq!(
            page.0,
            vec![
                TranscriptEntry::User("fix the login redirect".into()),
                TranscriptEntry::Assistant("Looking at the middleware.".into()),
                TranscriptEntry::Tool {
                    name: "Bash".into(),
                    input: "rg -n redirect …".into()
                },
                TranscriptEntry::Tool {
                    name: "Read …".into(),
                    input: "/src/auth.rs".into()
                },
                // Escaping is the reader's job: the escape sequence arrives verbatim.
                TranscriptEntry::Assistant("Fixed: \u{1b}]0;title\u{7} done".into()),
            ]
        );
    }

    #[test]
    fn codex_reads_event_messages_and_tool_calls_of_its_own_rollout() {
        let lines = [
            format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{CODEX}\",\"source\":\"cli\"}}}}"),
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>INJECTED</environment_context>"}]}}"#.into(),
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"add a test"}}"#.into(),
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"cargo test\"]}","call_id":"c1"}}"#.into(),
            r#"{"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch","input":"*** Begin Patch\n*** Update File: a.rs"}}"#.into(),
            r#"{"type":"response_item","payload":{"type":"web_search_call","action":{"type":"search","query":"fixture weather"}}}"#.into(),
            r#"{"type":"response_item","payload":{"type":"tool_search_call","execution":"server","arguments":{"paths":["crm"]}}}"#.into(),
            r#"{"type":"response_item","payload":{"type":"image_generation_call","revised_prompt":"a synthetic fixture","result":"BASE64_MUST_NOT_DISPLAY"}}"#.into(),
            r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"TOOL OUTPUT"}}"#.into(),
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done."}]}}"#.into(),
            r#"{"type":"event_msg","payload":{"type":"agent_message","message":"Done."}}"#.into(),
        ];
        let (_directory, path) = write(&lines);
        let page =
            read_entries(&candidate(AgentProvider::Codex, CODEX, path.clone()), None).unwrap();
        assert_eq!(
            page.0,
            vec![
                TranscriptEntry::User("add a test".into()),
                TranscriptEntry::Tool {
                    name: "shell".into(),
                    input: "bash -lc cargo test".into()
                },
                TranscriptEntry::Tool {
                    name: "apply_patch".into(),
                    input: "*** Begin Patch …".into()
                },
                TranscriptEntry::Tool {
                    name: "web search".into(),
                    input: "fixture weather".into()
                },
                TranscriptEntry::Tool {
                    name: "tool search".into(),
                    input: r#"{"paths":["crm"]}"#.into()
                },
                TranscriptEntry::Tool {
                    name: "image generation".into(),
                    input: "a synthetic fixture".into()
                },
                TranscriptEntry::Assistant("Done.".into()),
            ]
        );
        // A rollout of another conversation is not this session's transcript.
        let error =
            read_entries(&candidate(AgentProvider::Codex, "other", path), None).unwrap_err();
        assert!(
            error.to_string().contains("another conversation"),
            "{error}"
        );
    }

    #[test]
    fn pages_walk_back_to_the_start_and_skip_oversized_records() {
        let huge = "x".repeat(3 * WINDOW as usize);
        let mut lines = Vec::new();
        for index in 0..12_000 {
            lines.push(claude_line("user", format!("prompt {index}").into()));
            if index == 6_000 {
                lines.push(claude_line(
                    "assistant",
                    serde_json::json!([{"type": "text", "text": huge}]),
                ));
            }
        }
        let (_directory, path) = write(&lines);
        let candidate = candidate(AgentProvider::Claude, CLAUDE, path);
        let pages = all_pages(&candidate);
        assert!(pages.len() > 3, "the file spans several windows");
        let entries: Vec<_> = pages.into_iter().flatten().collect();
        let expected: Vec<_> = (0..12_000)
            .map(|index| TranscriptEntry::User(format!("prompt {index}")))
            .collect();
        assert_eq!(
            entries, expected,
            "every prompt once, in order; the huge record skipped"
        );
    }

    #[test]
    fn a_record_larger_than_the_window_at_the_end_does_not_stall_paging() {
        let huge = "y".repeat(2 * WINDOW as usize + 7);
        let (_directory, path) = write(&[
            claude_line("user", "before".into()),
            claude_line("user", huge.into()),
        ]);
        let page = read_entries(&candidate(AgentProvider::Claude, CLAUDE, path), None).unwrap();
        assert_eq!(page.0, vec![TranscriptEntry::User("before".into())]);
        assert_eq!(page.1, 0);
    }

    #[test]
    fn long_text_is_bounded_and_missing_or_irregular_files_are_errors() {
        let (directory, path) = write(&[claude_line("user", "é".repeat(TEXT_LIMIT + 10).into())]);
        let page = read_entries(&candidate(AgentProvider::Claude, CLAUDE, path), None).unwrap();
        let TranscriptEntry::User(text) = &page.0[0] else {
            panic!("{page:?}")
        };
        assert_eq!(text.chars().count(), TEXT_LIMIT + 1);
        assert!(text.ends_with('…'));
        let missing = directory.path().join("missing.jsonl");
        assert!(read_entries(&candidate(AgentProvider::Claude, CLAUDE, missing), None).is_err());
        let fifo = directory.path().join("fifo");
        let path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let error =
            read_entries(&candidate(AgentProvider::Claude, CLAUDE, fifo), None).unwrap_err();
        assert!(error.to_string().contains("regular file"), "{error}");
    }
    #[test]
    fn unterminated_json_is_not_published_and_oversized_scans_are_bounded() {
        let (directory, path) = write(&[claude_line("user", "complete".into())]);
        let pending = claude_line("assistant", "still being written".into());
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(pending.as_bytes())
            .unwrap();
        let page = read_entries(&candidate(AgentProvider::Claude, CLAUDE, path), None).unwrap();
        assert_eq!(page.0, vec![TranscriptEntry::User("complete".into())]);

        let huge = directory.path().join("huge.jsonl");
        let mut file = File::create(&huge).unwrap();
        file.write_all(&vec![b'x'; (SCAN_LIMIT + WINDOW) as usize])
            .unwrap();
        let page = read_entries(&candidate(AgentProvider::Claude, CLAUDE, huge), None).unwrap();
        assert!(page.0.is_empty());
        assert_eq!(
            page.1, WINDOW,
            "one request never scans beyond its byte budget"
        );
    }
}

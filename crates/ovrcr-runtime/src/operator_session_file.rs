//! Bounded, zero-spawn reader and cleanup helpers for Operator native Pi checkpoints.
//!
//! Designed as a dedicated module so `/session` (`#487`) and `/tree` (`#489`) share
//! one strict `O_NOFOLLOW`, regular-file, record-bounded `session.jsonl` reader without
//! exposing provider payloads, credentials, or raw stderr.

use anyhow::{Result, bail};
use ovrcr_protocol::operator::{
    MAX_OPERATOR_TREE_ENTRIES, MAX_OPERATOR_TREE_PREVIEW_BYTES, OperatorSessionTree,
    OperatorTreeEntry, OperatorTreeEntryKind,
};
use ovrcr_protocol::{
    OperatorContext, OperatorConversationId, OperatorModel, OperatorTokenStats, OperatorTurnId,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub const SESSION_CHECKPOINT_UNAVAILABLE: &str =
    "Operator session checkpoint is unavailable or invalid.";

const CAPTURED_CONTEXT_PREFIX: &str =
    "Captured OVRCR context (identities are fixed for this turn):\n";
const USER_REQUEST_MARKER: &str = "\n\nUser request:\n";

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_CHECKPOINT_RECORD: usize = 16 * 1024 * 1024;
const MAX_SESSION_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SESSION_RECORDS: usize = 100_000;
const MAX_MODELS_JSON_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromotedCheckpointSummary {
    pub pi_session_id: String,
    pub active_model: Option<OperatorModel>,
    pub user_messages: u64,
    pub assistant_messages: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
    pub total_messages: u64,
    pub epoch_total_messages: u64,
    pub tokens: OperatorTokenStats,
    pub cost_usd_nanos: u64,
    pub latest_context_tokens: Option<u64>,
    pub inline_context_window: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredTurnLineage {
    pub turn: OperatorTurnId,
    pub parent_turn: Option<OperatorTurnId>,
    pub pi_first_entry_id: Option<String>,
    pub pi_leaf_entry_id: Option<String>,
    pub user_prompt: Option<(String, OperatorContext)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTreeTarget {
    pub noop: bool,
    pub active_tip_turn: Option<OperatorTurnId>,
    pub pi_tree_target_id: Option<String>,
    pub current_leaf_id: Option<String>,
    pub editor_text: Option<String>,
    pub restored_context: Option<OperatorContext>,
}

struct RawSessionEntry {
    id: String,
    parent_id: Option<String>,
    kind: OperatorTreeEntryKind,
    raw: Value,
}

pub fn checkpoint_dir(registry_path: &Path, turn: OperatorTurnId) -> PathBuf {
    registry_path
        .join("operator-pi")
        .join("turns")
        .join(turn.0.to_string())
}

pub fn unwrap_operator_user_prompt(raw: &str) -> (String, OperatorContext) {
    if let Some(after_prefix) = raw.strip_prefix(CAPTURED_CONTEXT_PREFIX)
        && let Some((context_json, text)) = after_prefix.split_once(USER_REQUEST_MARKER)
        && let Ok(context) = serde_json::from_str::<OperatorContext>(context_json)
        && context.validate().is_ok()
    {
        return (text.to_owned(), context);
    }
    (raw.to_owned(), OperatorContext::default())
}

pub fn read_promoted_checkpoint(
    registry_path: &Path,
    conversation: OperatorConversationId,
    turn: OperatorTurnId,
) -> Result<PromotedCheckpointSummary> {
    read_promoted_checkpoint_for_branch(registry_path, conversation, turn, None)
}

pub fn read_promoted_checkpoint_for_branch(
    registry_path: &Path,
    conversation: OperatorConversationId,
    turn: OperatorTurnId,
    tree_target_id: Option<&str>,
) -> Result<PromotedCheckpointSummary> {
    let (expected_id, entries) = read_raw_session_entries(registry_path, conversation, turn)?;
    let index_by_id: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| (entry.id.as_str(), idx))
        .collect();
    let effective_leaf_id = compute_effective_leaf_id(&entries, &index_by_id, tree_target_id)?;
    let active_indices = branch_path_indices(&entries, &index_by_id, effective_leaf_id.as_deref());

    let mut epoch_summary = PromotedCheckpointSummary {
        pi_session_id: expected_id.clone(),
        ..PromotedCheckpointSummary::default()
    };
    for entry in &entries {
        accumulate_entry(&mut epoch_summary, &entry.raw);
    }

    let mut branch_summary = PromotedCheckpointSummary {
        pi_session_id: expected_id,
        ..PromotedCheckpointSummary::default()
    };
    for idx in active_indices {
        accumulate_entry(&mut branch_summary, &entries[idx].raw);
    }

    Ok(PromotedCheckpointSummary {
        pi_session_id: branch_summary.pi_session_id,
        active_model: branch_summary.active_model.or(epoch_summary.active_model),
        user_messages: branch_summary.user_messages,
        assistant_messages: branch_summary.assistant_messages,
        tool_calls: branch_summary.tool_calls,
        tool_results: branch_summary.tool_results,
        total_messages: branch_summary.total_messages,
        epoch_total_messages: epoch_summary.total_messages,
        tokens: epoch_summary.tokens,
        cost_usd_nanos: epoch_summary.cost_usd_nanos,
        latest_context_tokens: branch_summary.latest_context_tokens,
        inline_context_window: branch_summary
            .inline_context_window
            .or(epoch_summary.inline_context_window),
    })
}

pub fn read_session_tree(
    registry_path: &Path,
    conversation: OperatorConversationId,
    checkpoint_turn: OperatorTurnId,
    tree_target_id: Option<&str>,
    completed_turns: &[StoredTurnLineage],
) -> Result<OperatorSessionTree> {
    let (expected_id, entries) =
        read_raw_session_entries(registry_path, conversation, checkpoint_turn)?;
    let index_by_id: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| (entry.id.as_str(), idx))
        .collect();
    let current_leaf_id = compute_effective_leaf_id(&entries, &index_by_id, tree_target_id)?;
    let (entry_turns, user_opener_indices) =
        correlate_entry_turns(&entries, &index_by_id, completed_turns);
    let turn_by_id: HashMap<OperatorTurnId, &StoredTurnLineage> = completed_turns
        .iter()
        .map(|lineage| (lineage.turn, lineage))
        .collect();

    let kept_indices: Vec<usize> = if entries.len() <= MAX_OPERATOR_TREE_ENTRIES {
        (0..entries.len()).collect()
    } else {
        let mut must_keep: HashSet<usize> =
            branch_path_indices(&entries, &index_by_id, current_leaf_id.as_deref())
                .into_iter()
                .collect();
        let remaining = MAX_OPERATOR_TREE_ENTRIES.saturating_sub(must_keep.len());
        let mut tail = Vec::with_capacity(remaining);
        for idx in (0..entries.len()).rev() {
            if tail.len() == remaining {
                break;
            }
            if !must_keep.contains(&idx) {
                tail.push(idx);
            }
        }
        must_keep.extend(tail);
        let mut ordered: Vec<usize> = must_keep.into_iter().collect();
        ordered.sort_unstable();
        ordered
    };
    let kept_ids: HashSet<&str> = kept_indices
        .iter()
        .map(|&idx| entries[idx].id.as_str())
        .collect();

    let mut tree_entries = Vec::with_capacity(kept_indices.len());
    for idx in kept_indices {
        let entry = &entries[idx];
        let turn = entry_turns[idx];
        let stored_prompt = turn
            .filter(|t| user_opener_indices.get(t) == Some(&idx))
            .and_then(|t| turn_by_id.get(&t))
            .and_then(|lineage| lineage.user_prompt.as_ref());
        let preview = format_entry_preview(entry, stored_prompt);
        let parent_id = entry
            .parent_id
            .as_deref()
            .filter(|parent| kept_ids.contains(*parent))
            .map(str::to_owned);
        tree_entries.push(OperatorTreeEntry {
            id: entry.id.clone(),
            parent_id,
            kind: entry.kind,
            preview,
        });
    }

    let tree = OperatorSessionTree {
        pi_session_id: Some(expected_id),
        current_leaf_id,
        entries: tree_entries,
    };
    tree.validate().map_err(anyhow::Error::msg)?;
    Ok(tree)
}

pub fn resolve_tree_target(
    registry_path: &Path,
    conversation: OperatorConversationId,
    checkpoint_turn: OperatorTurnId,
    existing_tree_target_id: Option<&str>,
    completed_turns: &[StoredTurnLineage],
    target_entry_id: &str,
) -> Result<ResolvedTreeTarget> {
    let (_expected_id, entries) =
        read_raw_session_entries(registry_path, conversation, checkpoint_turn)?;
    let index_by_id: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| (entry.id.as_str(), idx))
        .collect();
    let Some(&target_idx) = index_by_id.get(target_entry_id) else {
        bail!("Selected tree entry was not found in this Operator session.");
    };
    let prior_effective_leaf_id =
        compute_effective_leaf_id(&entries, &index_by_id, existing_tree_target_id)?;
    let file_leaf_id = entries.last().map(|entry| entry.id.clone());
    let (entry_turns, user_opener_indices) =
        correlate_entry_turns(&entries, &index_by_id, completed_turns);
    let turn_by_id: HashMap<OperatorTurnId, &StoredTurnLineage> = completed_turns
        .iter()
        .map(|lineage| (lineage.turn, lineage))
        .collect();
    let leaf_turn_owners: HashMap<&str, OperatorTurnId> = completed_turns
        .iter()
        .filter_map(|lineage| {
            lineage
                .pi_leaf_entry_id
                .as_deref()
                .map(|leaf| (leaf, lineage.turn))
        })
        .collect();

    let target = &entries[target_idx];
    let is_user_rewind = matches!(
        target.kind,
        OperatorTreeEntryKind::User | OperatorTreeEntryKind::CustomMessage
    );
    if !is_user_rewind && prior_effective_leaf_id.as_deref() == Some(target_entry_id) {
        let active_tip_turn = resolve_active_tip_for_leaf(
            &entries,
            &index_by_id,
            &entry_turns,
            &leaf_turn_owners,
            prior_effective_leaf_id.as_deref(),
        );
        return Ok(ResolvedTreeTarget {
            noop: true,
            active_tip_turn,
            pi_tree_target_id: existing_tree_target_id.map(str::to_owned),
            current_leaf_id: prior_effective_leaf_id,
            editor_text: None,
            restored_context: None,
        });
    }

    let new_effective_leaf_id = if is_user_rewind {
        target.parent_id.clone()
    } else {
        Some(target.id.clone())
    };
    let active_tip_turn = resolve_active_tip_for_leaf(
        &entries,
        &index_by_id,
        &entry_turns,
        &leaf_turn_owners,
        new_effective_leaf_id.as_deref(),
    );
    let (editor_text, restored_context) = match target.kind {
        OperatorTreeEntryKind::User => {
            let turn = entry_turns[target_idx];
            if let Some((text, context)) = turn
                .filter(|t| user_opener_indices.get(t) == Some(&target_idx))
                .and_then(|t| turn_by_id.get(&t))
                .and_then(|lineage| lineage.user_prompt.clone())
            {
                (Some(text), Some(context))
            } else {
                let raw_text = extract_message_text(&target.raw["message"]);
                let (text, context) = unwrap_operator_user_prompt(&raw_text);
                (Some(text), Some(context))
            }
        }
        OperatorTreeEntryKind::CustomMessage => {
            let text = extract_custom_message_text(&target.raw);
            (Some(sanitize_editor_text(&text)), None)
        }
        _ => (None, None),
    };
    let pi_tree_target_id = if !is_user_rewind
        && new_effective_leaf_id.is_some()
        && new_effective_leaf_id == file_leaf_id
    {
        None
    } else {
        Some(target.id.clone())
    };

    Ok(ResolvedTreeTarget {
        noop: false,
        active_tip_turn,
        pi_tree_target_id,
        current_leaf_id: new_effective_leaf_id,
        editor_text,
        restored_context,
    })
}

fn read_raw_session_entries(
    registry_path: &Path,
    conversation: OperatorConversationId,
    turn: OperatorTurnId,
) -> Result<(String, Vec<RawSessionEntry>)> {
    if conversation.0 == 0 || turn.0 == 0 {
        bail!("{SESSION_CHECKPOINT_UNAVAILABLE}");
    }
    let dir = checkpoint_dir(registry_path, turn);
    let canonical = dir.join("session.jsonl");
    let source = match fs::symlink_metadata(&canonical) {
        Ok(_) => canonical,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => dir.join("checkpoint.jsonl"),
        Err(_) => bail!("{SESSION_CHECKPOINT_UNAVAILABLE}"),
    };
    let file = open_verified_private_file(&source, MAX_SESSION_FILE_BYTES)
        .ok_or_else(|| anyhow::anyhow!(SESSION_CHECKPOINT_UNAVAILABLE))?;
    let mut reader = BufReader::new(file);

    let mut header_bytes = Vec::new();
    reader
        .by_ref()
        .take((MAX_HEADER_BYTES + 1) as u64)
        .read_until(b'\n', &mut header_bytes)
        .map_err(|_| anyhow::anyhow!(SESSION_CHECKPOINT_UNAVAILABLE))?;
    if header_bytes.is_empty()
        || header_bytes.len() > MAX_HEADER_BYTES
        || !header_bytes.ends_with(b"\n")
    {
        bail!("{SESSION_CHECKPOINT_UNAVAILABLE}");
    }
    let expected_id = format!("operator-{}", conversation.0);
    let workdir = registry_path.join("operator-workdir");
    validate_header(&header_bytes, &expected_id, &workdir)?;

    let mut entries = Vec::new();
    let mut seen_ids = HashSet::new();
    let mut total_bytes = header_bytes.len() as u64;
    let mut records = 0usize;
    let mut line = Vec::new();
    let mut previous_id: Option<String> = None;
    loop {
        line.clear();
        let read = reader
            .by_ref()
            .take((MAX_CHECKPOINT_RECORD + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| anyhow::anyhow!(SESSION_CHECKPOINT_UNAVAILABLE))?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes.saturating_add(read as u64);
        records = records.saturating_add(1);
        if line.len() > MAX_CHECKPOINT_RECORD
            || total_bytes > MAX_SESSION_FILE_BYTES
            || records > MAX_SESSION_RECORDS
            || !line.ends_with(b"\n")
        {
            bail!("{SESSION_CHECKPOINT_UNAVAILABLE}");
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let raw: Value = serde_json::from_slice(&line)
            .map_err(|_| anyhow::anyhow!(SESSION_CHECKPOINT_UNAVAILABLE))?;
        if !raw.is_object() {
            bail!("{SESSION_CHECKPOINT_UNAVAILABLE}");
        }
        let has_explicit_id = raw
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty());
        let id = if let Some(explicit) = raw
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            explicit.to_owned()
        } else {
            format!("legacy-entry-{records}")
        };
        if id.len() > 256 || id.chars().any(char::is_control) || !seen_ids.insert(id.clone()) {
            bail!("{SESSION_CHECKPOINT_UNAVAILABLE}");
        }
        let parent_id = if has_explicit_id {
            match raw.get("parentId") {
                Some(Value::String(parent)) if !parent.is_empty() => {
                    if !seen_ids.contains(parent.as_str()) {
                        bail!("{SESSION_CHECKPOINT_UNAVAILABLE}");
                    }
                    Some(parent.clone())
                }
                Some(Value::Null) | None => None,
                Some(Value::String(_)) => None,
                _ => bail!("{SESSION_CHECKPOINT_UNAVAILABLE}"),
            }
        } else {
            previous_id.clone()
        };
        let kind = classify_entry_kind(&raw);
        previous_id = Some(id.clone());
        entries.push(RawSessionEntry {
            id,
            parent_id,
            kind,
            raw,
        });
    }
    Ok((expected_id, entries))
}

fn classify_entry_kind(entry: &Value) -> OperatorTreeEntryKind {
    match entry["type"].as_str() {
        Some("message") => match entry["message"]["role"].as_str() {
            Some("user") => OperatorTreeEntryKind::User,
            Some("assistant") => {
                let message = &entry["message"];
                let has_text = !extract_message_text(message).trim().is_empty();
                let is_error_or_aborted = message["stopReason"]
                    .as_str()
                    .is_some_and(|r| !r.is_empty() && r != "stop" && r != "toolUse");
                OperatorTreeEntryKind::Assistant {
                    tool_only: !has_text && !is_error_or_aborted,
                }
            }
            Some("toolResult") => OperatorTreeEntryKind::ToolResult,
            _ => OperatorTreeEntryKind::Other,
        },
        Some("compaction") => OperatorTreeEntryKind::Compaction,
        Some("branch_summary") => OperatorTreeEntryKind::BranchSummary,
        Some("custom_message") => OperatorTreeEntryKind::CustomMessage,
        Some("model_change") => OperatorTreeEntryKind::ModelChange,
        Some("thinking_level_change") => OperatorTreeEntryKind::ThinkingLevelChange,
        _ => OperatorTreeEntryKind::Other,
    }
}

fn compute_effective_leaf_id(
    entries: &[RawSessionEntry],
    index_by_id: &HashMap<&str, usize>,
    tree_target_id: Option<&str>,
) -> Result<Option<String>> {
    let Some(target_id) = tree_target_id else {
        return Ok(entries.last().map(|entry| entry.id.clone()));
    };
    let Some(&idx) = index_by_id.get(target_id) else {
        bail!("{SESSION_CHECKPOINT_UNAVAILABLE}");
    };
    let target = &entries[idx];
    if matches!(
        target.kind,
        OperatorTreeEntryKind::User | OperatorTreeEntryKind::CustomMessage
    ) {
        Ok(target.parent_id.clone())
    } else {
        Ok(Some(target.id.clone()))
    }
}

fn branch_path_indices(
    entries: &[RawSessionEntry],
    index_by_id: &HashMap<&str, usize>,
    leaf_id: Option<&str>,
) -> Vec<usize> {
    let mut path = Vec::new();
    let mut visited = HashSet::new();
    let mut cur = leaf_id;
    while let Some(id) = cur {
        let Some(&idx) = index_by_id.get(id) else {
            break;
        };
        if !visited.insert(idx) {
            break;
        }
        path.push(idx);
        cur = entries[idx].parent_id.as_deref();
    }
    path.reverse();
    path
}

fn correlate_entry_turns(
    entries: &[RawSessionEntry],
    index_by_id: &HashMap<&str, usize>,
    completed_turns: &[StoredTurnLineage],
) -> (Vec<Option<OperatorTurnId>>, HashMap<OperatorTurnId, usize>) {
    let mut entry_turns = vec![None; entries.len()];
    let mut legacy_turns = Vec::new();
    for lineage in completed_turns {
        if let Some(leaf_id) = lineage.pi_leaf_entry_id.as_deref()
            && let Some(&leaf_idx) = index_by_id.get(leaf_id)
        {
            let first_id = lineage.pi_first_entry_id.as_deref();
            let mut visited = HashSet::new();
            let mut cur = Some(leaf_idx);
            while let Some(idx) = cur {
                if !visited.insert(idx) || entry_turns[idx].is_some() {
                    break;
                }
                entry_turns[idx] = Some(lineage.turn);
                if Some(entries[idx].id.as_str()) == first_id {
                    break;
                }
                cur = entries[idx]
                    .parent_id
                    .as_deref()
                    .and_then(|parent| index_by_id.get(parent).copied());
            }
        } else {
            legacy_turns.push(lineage.turn);
        }
    }

    if !legacy_turns.is_empty() {
        let mut segments: Vec<Vec<usize>> = Vec::new();
        let mut pending_prefix: Vec<usize> = Vec::new();
        for (idx, entry) in entries.iter().enumerate() {
            if entry_turns[idx].is_some() {
                continue;
            }
            let starts_turn = matches!(
                entry.kind,
                OperatorTreeEntryKind::User | OperatorTreeEntryKind::Compaction
            );
            let is_bookkeeping = matches!(
                entry.kind,
                OperatorTreeEntryKind::ModelChange
                    | OperatorTreeEntryKind::ThinkingLevelChange
                    | OperatorTreeEntryKind::Other
            );
            if starts_turn {
                let mut seg = std::mem::take(&mut pending_prefix);
                seg.push(idx);
                segments.push(seg);
            } else if is_bookkeeping {
                pending_prefix.push(idx);
            } else if let Some(last) = segments.last_mut() {
                last.append(&mut pending_prefix);
                last.push(idx);
            } else {
                pending_prefix.push(idx);
            }
        }
        if let Some(last) = segments.last_mut() {
            last.extend(pending_prefix);
        }
        for (turn_id, segment) in legacy_turns.into_iter().zip(segments) {
            for idx in segment {
                entry_turns[idx] = Some(turn_id);
            }
        }
    }

    let mut user_opener_indices = HashMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        if entry.kind == OperatorTreeEntryKind::User
            && let Some(turn_id) = entry_turns[idx]
        {
            user_opener_indices.entry(turn_id).or_insert(idx);
        }
    }

    (entry_turns, user_opener_indices)
}

fn resolve_active_tip_for_leaf(
    entries: &[RawSessionEntry],
    index_by_id: &HashMap<&str, usize>,
    entry_turns: &[Option<OperatorTurnId>],
    leaf_turn_owners: &HashMap<&str, OperatorTurnId>,
    leaf_id: Option<&str>,
) -> Option<OperatorTurnId> {
    let mut visited = HashSet::new();
    let mut cur = leaf_id;
    while let Some(id) = cur {
        let Some(&idx) = index_by_id.get(id) else {
            break;
        };
        if !visited.insert(idx) {
            break;
        }
        if let Some(&turn_id) = leaf_turn_owners.get(id) {
            return Some(turn_id);
        }
        let is_bookkeeping = matches!(
            entries[idx].kind,
            OperatorTreeEntryKind::ModelChange
                | OperatorTreeEntryKind::ThinkingLevelChange
                | OperatorTreeEntryKind::Other
        );
        if !is_bookkeeping && let Some(turn_id) = entry_turns[idx] {
            return Some(turn_id);
        }
        cur = entries[idx].parent_id.as_deref();
    }
    None
}

fn format_entry_preview(
    entry: &RawSessionEntry,
    stored_prompt: Option<&(String, OperatorContext)>,
) -> String {
    match entry.kind {
        OperatorTreeEntryKind::User => {
            let text = if let Some((stored_text, _)) = stored_prompt {
                stored_text.clone()
            } else {
                let raw = extract_message_text(&entry.raw["message"]);
                unwrap_operator_user_prompt(&raw).0
            };
            single_line_preview(&text, "[user]")
        }
        OperatorTreeEntryKind::Assistant { .. } => {
            let message = &entry.raw["message"];
            let text = extract_message_text(message);
            if !text.trim().is_empty() {
                single_line_preview(&text, "[assistant]")
            } else {
                let tools = extract_tool_call_names(message);
                if tools.is_empty() {
                    let stop_reason = message["stopReason"].as_str().unwrap_or("");
                    if !stop_reason.is_empty() && stop_reason != "stop" {
                        single_line_preview(&format!("[assistant: {stop_reason}]"), "[assistant]")
                    } else {
                        "[assistant]".into()
                    }
                } else {
                    single_line_preview(&format!("[tool: {}]", tools.join(", ")), "[tool]")
                }
            }
        }
        OperatorTreeEntryKind::ToolResult => {
            let message = &entry.raw["message"];
            let tool_name = message["toolName"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("tool");
            let text = extract_message_text(message);
            let raw_preview = if !text.trim().is_empty() {
                format!("{tool_name}: {text}")
            } else {
                format!("{tool_name} result")
            };
            single_line_preview(&raw_preview, "[tool result]")
        }
        OperatorTreeEntryKind::Compaction => {
            let summary = entry.raw["summary"].as_str().unwrap_or("");
            let tokens_before = entry.raw["tokensBefore"].as_u64().unwrap_or(0);
            let prefix = if tokens_before > 0 {
                format!("[compaction: {tokens_before} tok]")
            } else {
                "[compaction]".into()
            };
            let combined = if summary.trim().is_empty() {
                prefix
            } else {
                format!("{prefix} {summary}")
            };
            single_line_preview(&combined, "[compaction]")
        }
        OperatorTreeEntryKind::BranchSummary => {
            let summary = entry.raw["summary"].as_str().unwrap_or("");
            let combined = if summary.trim().is_empty() {
                "[branch summary]".into()
            } else {
                format!("[branch summary] {summary}")
            };
            single_line_preview(&combined, "[branch summary]")
        }
        OperatorTreeEntryKind::CustomMessage => {
            let text = extract_custom_message_text(&entry.raw);
            single_line_preview(&text, "[custom]")
        }
        OperatorTreeEntryKind::ModelChange => {
            let provider = entry.raw["provider"].as_str().unwrap_or("?");
            let model_id = entry.raw["modelId"].as_str().unwrap_or("?");
            single_line_preview(&format!("model: {provider}/{model_id}"), "[model]")
        }
        OperatorTreeEntryKind::ThinkingLevelChange => {
            let level = entry.raw["thinkingLevel"].as_str().unwrap_or("default");
            single_line_preview(&format!("thinking: {level}"), "[thinking]")
        }
        OperatorTreeEntryKind::Other => {
            let kind = entry.raw["type"].as_str().unwrap_or("entry");
            single_line_preview(&format!("[{kind}]"), "[entry]")
        }
    }
}

fn extract_message_text(message: &Value) -> String {
    if let Some(s) = message["content"].as_str() {
        return s.to_owned();
    }
    let Some(blocks) = message["content"].as_array() else {
        return String::new();
    };
    let mut out = String::new();
    for block in blocks {
        if block["type"].as_str() == Some("text")
            && let Some(text) = block["text"].as_str()
        {
            if !out.is_empty() && !out.ends_with('\n') && !text.starts_with('\n') {
                out.push('\n');
            }
            out.push_str(text);
        }
    }
    out
}

fn extract_custom_message_text(entry: &Value) -> String {
    if let Some(s) = entry["content"].as_str() {
        return s.to_owned();
    }
    let Some(blocks) = entry["content"].as_array() else {
        return String::new();
    };
    let mut out = String::new();
    for block in blocks {
        if block["type"].as_str() == Some("text")
            && let Some(text) = block["text"].as_str()
        {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        }
    }
    out
}

fn extract_tool_call_names(message: &Value) -> Vec<&str> {
    let Some(blocks) = message["content"].as_array() else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|b| b["type"].as_str() == Some("toolCall"))
        .filter_map(|b| b["name"].as_str())
        .filter(|name| !name.is_empty())
        .collect()
}

fn single_line_preview(raw: &str, fallback: &str) -> String {
    let mut collapsed = String::new();
    let mut prev_space = true;
    for ch in raw.chars() {
        if ch == '\0' || ch.is_control() || ch.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else {
            collapsed.push(ch);
            prev_space = false;
        }
    }
    let trimmed = collapsed.trim();
    let candidate = if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    };
    truncate_utf8_bytes(candidate, MAX_OPERATOR_TREE_PREVIEW_BYTES)
}

fn sanitize_editor_text(raw: &str) -> String {
    let cleaned = raw.replace('\0', "\u{FFFD}");
    truncate_utf8_bytes(&cleaned, ovrcr_protocol::operator::MAX_OPERATOR_TEXT_BYTES)
}

fn truncate_utf8_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

pub fn lookup_model_context_window(registry_path: &Path, model: &OperatorModel) -> Option<u64> {
    let path = registry_path.join("operator-pi").join("models.json");
    let mut file = open_verified_private_file(&path, MAX_MODELS_JSON_BYTES)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_MODELS_JSON_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_MODELS_JSON_BYTES {
        return None;
    }
    let parsed: Value = serde_json::from_slice(&bytes).ok()?;
    let models = parsed["providers"][&model.provider]["models"].as_array()?;
    models
        .iter()
        .find(|entry| entry["id"].as_str() == Some(model.id.as_str()))
        .and_then(|entry| entry["contextWindow"].as_u64())
        .filter(|&window| window > 0)
}

pub fn remove_superseded_checkpoints(registry_path: &Path, turns: &[OperatorTurnId]) {
    for &turn in turns {
        if turn.0 == 0 {
            continue;
        }
        let dir = checkpoint_dir(registry_path, turn);
        for name in ["session.jsonl", "checkpoint.jsonl"] {
            let path = dir.join(name);
            if let Ok(meta) = fs::symlink_metadata(&path)
                && meta.file_type().is_file()
            {
                let _ = fs::remove_file(&path);
            }
        }
        let _ = fs::remove_dir(&dir);
    }
}

fn open_verified_private_file(path: &Path, max_bytes: u64) -> Option<File> {
    let pre = fs::symlink_metadata(path).ok()?;
    let euid = unsafe { libc::geteuid() };
    if !pre.file_type().is_file()
        || pre.mode() & 0o077 != 0
        || pre.uid() != euid
        || pre.len() > max_bytes
    {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let post = file.metadata().ok()?;
    if !post.file_type().is_file()
        || post.mode() & 0o077 != 0
        || post.uid() != euid
        || post.len() > max_bytes
        || (pre.dev(), pre.ino()) != (post.dev(), post.ino())
    {
        return None;
    }
    Some(file)
}

fn validate_header(header: &[u8], expected_session: &str, workdir: &Path) -> Result<()> {
    let value: Value = serde_json::from_slice(header)
        .map_err(|_| anyhow::anyhow!(SESSION_CHECKPOINT_UNAVAILABLE))?;
    let cwd_matches = value["cwd"].as_str().is_some_and(|cwd| {
        let path = Path::new(cwd);
        path.is_absolute()
            && match (path.canonicalize(), workdir.canonicalize()) {
                (Ok(captured), Ok(expected)) => captured == expected,
                _ => path == workdir,
            }
    });
    let valid = value["type"] == "session"
        && value["id"].as_str() == Some(expected_session)
        && value["version"]
            .as_u64()
            .is_some_and(|version| (1..=3).contains(&version))
        && cwd_matches;
    if valid {
        Ok(())
    } else {
        bail!("{SESSION_CHECKPOINT_UNAVAILABLE}");
    }
}

fn accumulate_entry(summary: &mut PromotedCheckpointSummary, entry: &Value) {
    match entry["type"].as_str() {
        Some("model_change") => {
            if let (Some(provider), Some(id)) =
                (entry["provider"].as_str(), entry["modelId"].as_str())
            {
                let candidate = OperatorModel {
                    provider: provider.to_owned(),
                    id: id.to_owned(),
                };
                if candidate.validate().is_ok() {
                    summary.active_model = Some(candidate);
                }
            }
        }
        Some("message") => {
            let message = &entry["message"];
            match message["role"].as_str() {
                Some("user") => {
                    summary.user_messages = summary.user_messages.saturating_add(1);
                    summary.total_messages = summary.total_messages.saturating_add(1);
                }
                Some("assistant") => {
                    summary.assistant_messages = summary.assistant_messages.saturating_add(1);
                    summary.total_messages = summary.total_messages.saturating_add(1);
                    if let Some(blocks) = message["content"].as_array() {
                        let calls = blocks
                            .iter()
                            .filter(|b| b["type"].as_str() == Some("toolCall"))
                            .count() as u64;
                        summary.tool_calls = summary.tool_calls.saturating_add(calls);
                    }
                    if let (Some(provider), Some(id)) =
                        (message["provider"].as_str(), message["model"].as_str())
                    {
                        let candidate = OperatorModel {
                            provider: provider.to_owned(),
                            id: id.to_owned(),
                        };
                        if candidate.validate().is_ok() {
                            summary.active_model = Some(candidate);
                        }
                    }
                    if let Some(window) = message["contextWindow"].as_u64().filter(|&w| w > 0) {
                        summary.inline_context_window = Some(window);
                    }
                    let usage = &message["usage"];
                    accumulate_usage(summary, usage);
                    let stop_reason = message["stopReason"].as_str().unwrap_or("");
                    if stop_reason != "aborted" && stop_reason != "error" {
                        let context_tokens = usage_context_tokens(usage);
                        if context_tokens > 0 {
                            summary.latest_context_tokens = Some(context_tokens);
                        }
                    }
                }
                Some("toolResult") => {
                    summary.tool_results = summary.tool_results.saturating_add(1);
                    summary.total_messages = summary.total_messages.saturating_add(1);
                    accumulate_usage(summary, &message["usage"]);
                }
                _ => {}
            }
        }
        Some("custom_message") => {
            summary.total_messages = summary.total_messages.saturating_add(1);
        }
        Some("compaction") => {
            accumulate_usage(summary, &entry["usage"]);
            summary.latest_context_tokens = None;
        }
        Some("branch_summary") | Some("usage") => {
            accumulate_usage(summary, &entry["usage"]);
        }
        _ => {}
    }
}

fn accumulate_usage(summary: &mut PromotedCheckpointSummary, usage: &Value) {
    if !usage.is_object() {
        return;
    }
    let input = usage["input"].as_u64().unwrap_or(0);
    let output = usage["output"].as_u64().unwrap_or(0);
    let cache_read = usage["cacheRead"].as_u64().unwrap_or(0);
    let cache_write = usage["cacheWrite"].as_u64().unwrap_or(0);
    summary.tokens.input = summary.tokens.input.saturating_add(input);
    summary.tokens.output = summary.tokens.output.saturating_add(output);
    summary.tokens.cache_read = summary.tokens.cache_read.saturating_add(cache_read);
    summary.tokens.cache_write = summary.tokens.cache_write.saturating_add(cache_write);
    summary.tokens.total = summary
        .tokens
        .input
        .saturating_add(summary.tokens.output)
        .saturating_add(summary.tokens.cache_read)
        .saturating_add(summary.tokens.cache_write);

    let cost_usd = usage["cost"]["total"].as_f64().unwrap_or_else(|| {
        ["input", "output", "cacheRead", "cacheWrite"]
            .into_iter()
            .filter_map(|key| usage["cost"][key].as_f64())
            .filter(|v| v.is_finite() && *v > 0.0)
            .sum()
    });
    if cost_usd.is_finite() && cost_usd > 0.0 {
        let nanos = (cost_usd * 1_000_000_000.0).round();
        let added = if nanos >= u64::MAX as f64 {
            u64::MAX
        } else {
            nanos as u64
        };
        summary.cost_usd_nanos = summary.cost_usd_nanos.saturating_add(added);
    }
}

fn usage_context_tokens(usage: &Value) -> u64 {
    if !usage.is_object() {
        return 0;
    }
    let explicit = usage["totalTokens"].as_u64().unwrap_or(0);
    if explicit > 0 {
        explicit
    } else {
        usage["input"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(usage["output"].as_u64().unwrap_or(0))
            .saturating_add(usage["cacheRead"].as_u64().unwrap_or(0))
            .saturating_add(usage["cacheWrite"].as_u64().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn write_private(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(path, contents).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn reads_counts_tokens_cost_post_compaction_context_and_model_capacity() {
        let root = tempfile::tempdir().unwrap();
        let workdir = root.path().join("operator-workdir");
        fs::create_dir_all(&workdir).unwrap();
        fs::set_permissions(&workdir, fs::Permissions::from_mode(0o700)).unwrap();

        let models = json!({
            "providers": {
                "fixture": {
                    "apiKey": "secret-api-key-never-expose",
                    "models": [
                        { "id": "operator-fixture", "contextWindow": 200_000 }
                    ]
                }
            }
        });
        write_private(
            &root.path().join("operator-pi/models.json"),
            &serde_json::to_string(&models).unwrap(),
        );

        let lines = [
            json!({
                "type": "session",
                "version": 3,
                "id": "operator-7",
                "timestamp": "2026-11-15T00:00:00Z",
                "cwd": workdir.to_str().unwrap(),
            }),
            json!({
                "type": "model_change",
                "id": "e1",
                "parentId": null,
                "provider": "fixture",
                "modelId": "operator-fixture",
            }),
            json!({
                "type": "message",
                "id": "e2",
                "parentId": "e1",
                "message": {
                    "role": "user",
                    "content": [{ "type": "text", "text": "pre-compaction question" }],
                }
            }),
            json!({
                "type": "message",
                "id": "e3",
                "parentId": "e2",
                "message": {
                    "role": "assistant",
                    "provider": "fixture",
                    "model": "operator-fixture",
                    "stopReason": "toolUse",
                    "content": [
                        { "type": "text", "text": "checking" },
                        { "type": "toolCall", "id": "c1", "name": "bash", "arguments": {} },
                        { "type": "toolCall", "id": "c2", "name": "read", "arguments": {} }
                    ],
                    "usage": {
                        "input": 50_000,
                        "output": 2_000,
                        "cacheRead": 10_000,
                        "cacheWrite": 5_000,
                        "totalTokens": 67_000,
                        "cost": { "total": 0.125 }
                    }
                }
            }),
            json!({
                "type": "message",
                "id": "e4",
                "parentId": "e3",
                "message": {
                    "role": "toolResult",
                    "toolCallId": "c1",
                    "content": [{ "type": "text", "text": "ok" }],
                }
            }),
            json!({
                "type": "compaction",
                "id": "e5",
                "parentId": "e4",
                "summary": "compacted",
                "usage": {
                    "input": 1_000,
                    "output": 200,
                    "cacheRead": 0,
                    "cacheWrite": 0,
                    "totalTokens": 1_200,
                    "cost": { "total": 0.005 }
                }
            }),
            json!({
                "type": "message",
                "id": "e6",
                "parentId": "e5",
                "message": {
                    "role": "user",
                    "content": [{ "type": "text", "text": "post-compaction question" }],
                }
            }),
            json!({
                "type": "message",
                "id": "e7",
                "parentId": "e6",
                "message": {
                    "role": "assistant",
                    "provider": "fixture",
                    "model": "operator-fixture",
                    "stopReason": "stop",
                    "content": [{ "type": "text", "text": "final answer" }],
                    "usage": {
                        "input": 3_000,
                        "output": 500,
                        "cacheRead": 400,
                        "cacheWrite": 100,
                        "totalTokens": 4_000,
                        "cost": { "total": 0.02 }
                    }
                }
            }),
            json!({
                "type": "message",
                "id": "e8",
                "parentId": "e7",
                "message": {
                    "role": "assistant",
                    "provider": "fixture",
                    "model": "operator-fixture",
                    "stopReason": "aborted",
                    "content": [],
                    "usage": {
                        "input": 99_000,
                        "output": 0,
                        "cacheRead": 0,
                        "cacheWrite": 0,
                        "totalTokens": 99_000,
                        "cost": { "total": 0.01 }
                    }
                }
            }),
        ];
        let mut body = String::new();
        for line in &lines {
            body.push_str(&serde_json::to_string(line).unwrap());
            body.push('\n');
        }
        let session_path = checkpoint_dir(root.path(), OperatorTurnId(4)).join("session.jsonl");
        write_private(&session_path, &body);

        let parsed =
            read_promoted_checkpoint(root.path(), OperatorConversationId(7), OperatorTurnId(4))
                .unwrap();
        assert_eq!(parsed.pi_session_id, "operator-7");
        assert_eq!(
            parsed.active_model,
            Some(OperatorModel {
                provider: "fixture".into(),
                id: "operator-fixture".into(),
            })
        );
        assert_eq!(parsed.user_messages, 2);
        assert_eq!(parsed.assistant_messages, 3);
        assert_eq!(parsed.tool_calls, 2);
        assert_eq!(parsed.tool_results, 1);
        assert_eq!(parsed.total_messages, 6);
        assert_eq!(
            parsed.tokens,
            OperatorTokenStats {
                input: 153_000,
                output: 2_700,
                cache_read: 10_400,
                cache_write: 5_100,
                total: 171_200,
            }
        );
        assert_eq!(parsed.cost_usd_nanos, 160_000_000);
        // Latest non-aborted assistant message AFTER compaction had 4_000 tokens (not 67_000 or aborted 99_000).
        assert_eq!(parsed.latest_context_tokens, Some(4_000));
        assert_eq!(
            lookup_model_context_window(root.path(), parsed.active_model.as_ref().unwrap()),
            Some(200_000)
        );
    }

    #[test]
    fn rejects_symlinks_loose_permissions_wrong_header_and_corrupt_lines_without_leaking_secrets() {
        let root = tempfile::tempdir().unwrap();
        let workdir = root.path().join("operator-workdir");
        fs::create_dir_all(&workdir).unwrap();
        fs::set_permissions(&workdir, fs::Permissions::from_mode(0o700)).unwrap();

        let secret = "sk-live-never-leak-this-token";
        let real_file = root.path().join("outside.jsonl");
        let valid_header = serde_json::to_string(&json!({
            "type": "session",
            "version": 3,
            "id": "operator-1",
            "cwd": workdir.to_str().unwrap(),
        }))
        .unwrap()
            + "\n";
        write_private(&real_file, &valid_header);

        // Symlink at session.jsonl must be rejected via O_NOFOLLOW / symlink_metadata.
        let turn1_dir = checkpoint_dir(root.path(), OperatorTurnId(1));
        fs::create_dir_all(&turn1_dir).unwrap();
        fs::set_permissions(&turn1_dir, fs::Permissions::from_mode(0o700)).unwrap();
        symlink(&real_file, turn1_dir.join("session.jsonl")).unwrap();
        let err =
            read_promoted_checkpoint(root.path(), OperatorConversationId(1), OperatorTurnId(1))
                .unwrap_err()
                .to_string();
        assert_eq!(err, SESSION_CHECKPOINT_UNAVAILABLE);

        // Group-readable file must be rejected.
        let turn2 = checkpoint_dir(root.path(), OperatorTurnId(2)).join("session.jsonl");
        write_private(&turn2, &valid_header);
        fs::set_permissions(&turn2, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            read_promoted_checkpoint(root.path(), OperatorConversationId(1), OperatorTurnId(2))
                .is_err()
        );

        // Wrong conversation ID or corrupt JSON containing secret must never leak secret in error.
        let turn3 = checkpoint_dir(root.path(), OperatorTurnId(3)).join("session.jsonl");
        write_private(&turn3, &format!("{valid_header}not-json-{secret}\n"));
        let err =
            read_promoted_checkpoint(root.path(), OperatorConversationId(1), OperatorTurnId(3))
                .unwrap_err()
                .to_string();
        assert!(!err.contains(secret));
        assert_eq!(err, SESSION_CHECKPOINT_UNAVAILABLE);

        // Cleanup removes regular checkpoint files and empty turn dir, but never follows symlinks.
        let turn4_file = checkpoint_dir(root.path(), OperatorTurnId(4)).join("session.jsonl");
        write_private(&turn4_file, &valid_header);
        remove_superseded_checkpoints(root.path(), &[OperatorTurnId(1), OperatorTurnId(4)]);
        assert!(!turn4_file.exists());
        assert!(!checkpoint_dir(root.path(), OperatorTurnId(4)).exists());
        assert!(
            real_file.exists(),
            "cleanup must never delete symlink target"
        );
    }

    #[test]
    fn trailing_compaction_entry_clears_stale_context_tokens_while_accumulating_usage() {
        let root = tempfile::tempdir().unwrap();
        let workdir = root.path().join("operator-workdir");
        fs::create_dir_all(&workdir).unwrap();
        fs::set_permissions(&workdir, fs::Permissions::from_mode(0o700)).unwrap();

        let lines = [
            json!({
                "type": "session",
                "version": 3,
                "id": "operator-3",
                "cwd": workdir.to_str().unwrap(),
            }),
            json!({
                "type": "message",
                "id": "m1",
                "parentId": null,
                "message": {
                    "role": "user",
                    "content": [{ "type": "text", "text": "hello" }],
                }
            }),
            json!({
                "type": "message",
                "id": "m2",
                "parentId": "m1",
                "message": {
                    "role": "assistant",
                    "provider": "fixture",
                    "model": "operator-fixture",
                    "stopReason": "stop",
                    "content": [{ "type": "text", "text": "world" }],
                    "usage": {
                        "input": 20_000,
                        "output": 1_000,
                        "cacheRead": 0,
                        "cacheWrite": 0,
                        "totalTokens": 21_000,
                        "cost": { "total": 0.05 }
                    }
                }
            }),
            json!({
                "type": "compaction",
                "id": "c1",
                "parentId": "m2",
                "firstKeptEntryId": "m1",
                "tokensBefore": 21_000,
                "summary": "Summarized turn 1.",
                "usage": {
                    "input": 1_500,
                    "output": 300,
                    "cacheRead": 0,
                    "cacheWrite": 0,
                    "totalTokens": 1_800,
                    "cost": { "total": 0.004 }
                }
            }),
        ];
        let body = lines
            .iter()
            .map(|line| format!("{}\n", serde_json::to_string(line).unwrap()))
            .collect::<String>();
        let session_path = checkpoint_dir(root.path(), OperatorTurnId(2)).join("session.jsonl");
        write_private(&session_path, &body);

        let parsed =
            read_promoted_checkpoint(root.path(), OperatorConversationId(3), OperatorTurnId(2))
                .unwrap();
        assert_eq!(parsed.user_messages, 1);
        assert_eq!(parsed.assistant_messages, 1);
        assert_eq!(parsed.total_messages, 2);
        assert_eq!(parsed.tokens.total, 22_800);
        assert_eq!(parsed.cost_usd_nanos, 54_000_000);
        assert_eq!(
            parsed.latest_context_tokens, None,
            "trailing compaction entry must clear pre-compaction context tokens until a post-compaction assistant turn runs"
        );
    }

    #[test]
    fn tree_reader_unwraps_prompts_tracks_branch_stats_and_resolves_rewinds() {
        use ovrcr_protocol::{SessionId, SessionRunId};

        let root = tempfile::tempdir().unwrap();
        let workdir = root.path().join("operator-workdir");
        fs::create_dir_all(&workdir).unwrap();
        fs::set_permissions(&workdir, fs::Permissions::from_mode(0o700)).unwrap();

        let ctx1 = OperatorContext {
            project: Some("ovrcr".into()),
            workspace_id: Some("main".into()),
            session: Some(SessionId(11)),
            run: Some(SessionRunId(2)),
            ..OperatorContext::default()
        };
        let ctx2a = OperatorContext {
            project: Some("ovrcr".into()),
            workspace_id: Some("branch-a".into()),
            session: None,
            run: None,
            ..OperatorContext::default()
        };
        let wrap = |text: &str, ctx: &OperatorContext| {
            format!(
                "Captured OVRCR context (identities are fixed for this turn):\n{}\n\nUser request:\n{text}",
                serde_json::to_string(ctx).unwrap()
            )
        };

        let lines = [
            json!({
                "type": "session",
                "version": 3,
                "id": "operator-5",
                "cwd": workdir.to_str().unwrap(),
            }),
            // Turn 1: m1 -> u1 -> a1
            json!({
                "type": "model_change",
                "id": "m1",
                "parentId": null,
                "provider": "fixture",
                "modelId": "alpha",
            }),
            json!({
                "type": "message",
                "id": "u1",
                "parentId": "m1",
                "message": {
                    "role": "user",
                    "content": [{ "type": "text", "text": wrap("first prompt\nline two", &ctx1) }],
                }
            }),
            json!({
                "type": "message",
                "id": "a1",
                "parentId": "u1",
                "message": {
                    "role": "assistant",
                    "provider": "fixture",
                    "model": "alpha",
                    "stopReason": "stop",
                    "content": [{ "type": "text", "text": "first answer" }],
                    "usage": { "input": 1000, "output": 200, "totalTokens": 1200, "cost": { "total": 0.01 } }
                }
            }),
            // Turn 2 (Branch A): m2 -> u2a -> a2a_tool -> tr2a -> a2a
            json!({
                "type": "model_change",
                "id": "m2",
                "parentId": "a1",
                "provider": "fixture",
                "modelId": "beta",
            }),
            json!({
                "type": "message",
                "id": "u2a",
                "parentId": "m2",
                "message": {
                    "role": "user",
                    "content": [{ "type": "text", "text": wrap("explore branch A", &ctx2a) }],
                }
            }),
            json!({
                "type": "message",
                "id": "a2a-tool",
                "parentId": "u2a",
                "message": {
                    "role": "assistant",
                    "provider": "fixture",
                    "model": "beta",
                    "stopReason": "toolUse",
                    "content": [{ "type": "toolCall", "id": "tc1", "name": "bash", "arguments": {} }],
                    "usage": { "input": 1500, "output": 100, "totalTokens": 1600, "cost": { "total": 0.01 } }
                }
            }),
            json!({
                "type": "message",
                "id": "tr2a",
                "parentId": "a2a-tool",
                "message": {
                    "role": "toolResult",
                    "toolName": "bash",
                    "content": [{ "type": "text", "text": "cargo test ok" }],
                }
            }),
            json!({
                "type": "message",
                "id": "a2a",
                "parentId": "tr2a",
                "message": {
                    "role": "assistant",
                    "provider": "fixture",
                    "model": "beta",
                    "stopReason": "stop",
                    "content": [{ "type": "text", "text": "branch A conclusion" }],
                    "usage": { "input": 2500, "output": 300, "totalTokens": 2800, "cost": { "total": 0.02 } }
                }
            }),
            // Turn 3 (Branch B, forked off a1): u2b -> a2b
            json!({
                "type": "message",
                "id": "u2b",
                "parentId": "a1",
                "message": {
                    "role": "user",
                    "content": [{ "type": "text", "text": wrap("explore branch B", &OperatorContext::default()) }],
                }
            }),
            json!({
                "type": "message",
                "id": "a2b",
                "parentId": "u2b",
                "message": {
                    "role": "assistant",
                    "provider": "fixture",
                    "model": "alpha",
                    "stopReason": "stop",
                    "content": [{ "type": "text", "text": "branch B conclusion" }],
                    "usage": { "input": 1400, "output": 250, "totalTokens": 1650, "cost": { "total": 0.015 } }
                }
            }),
        ];
        let body = lines
            .iter()
            .map(|line| format!("{}\n", serde_json::to_string(line).unwrap()))
            .collect::<String>();
        let session_path = checkpoint_dir(root.path(), OperatorTurnId(3)).join("session.jsonl");
        write_private(&session_path, &body);

        let completed_turns = vec![
            StoredTurnLineage {
                turn: OperatorTurnId(1),
                parent_turn: None,
                pi_first_entry_id: Some("m1".into()),
                pi_leaf_entry_id: Some("a1".into()),
                user_prompt: Some(("first prompt\nline two".into(), ctx1.clone())),
            },
            StoredTurnLineage {
                turn: OperatorTurnId(2),
                parent_turn: Some(OperatorTurnId(1)),
                pi_first_entry_id: Some("m2".into()),
                pi_leaf_entry_id: Some("a2a".into()),
                user_prompt: None, // Verify deterministic wrapper-stripping fallback too!
            },
            StoredTurnLineage {
                turn: OperatorTurnId(3),
                parent_turn: Some(OperatorTurnId(1)),
                pi_first_entry_id: Some("u2b".into()),
                pi_leaf_entry_id: Some("a2b".into()),
                user_prompt: Some(("explore branch B".into(), OperatorContext::default())),
            },
        ];

        // 1. Default Branch B tip (tree_target_id = None -> file leaf a2b).
        let branch_b = read_promoted_checkpoint_for_branch(
            root.path(),
            OperatorConversationId(5),
            OperatorTurnId(3),
            None,
        )
        .unwrap();
        assert_eq!(branch_b.user_messages, 2);
        assert_eq!(branch_b.assistant_messages, 2);
        assert_eq!(branch_b.tool_calls, 0);
        assert_eq!(branch_b.total_messages, 4);
        assert_eq!(branch_b.epoch_total_messages, 8);
        assert_eq!(branch_b.latest_context_tokens, Some(1650));

        // 2. Switched to Branch A (tree_target_id = Some("a2a")).
        let branch_a = read_promoted_checkpoint_for_branch(
            root.path(),
            OperatorConversationId(5),
            OperatorTurnId(3),
            Some("a2a"),
        )
        .unwrap();
        assert_eq!(branch_a.user_messages, 2);
        assert_eq!(branch_a.assistant_messages, 3);
        assert_eq!(branch_a.tool_calls, 1);
        assert_eq!(branch_a.tool_results, 1);
        assert_eq!(branch_a.total_messages, 6);
        assert_eq!(branch_a.epoch_total_messages, 8);
        assert_eq!(branch_a.latest_context_tokens, Some(2800));

        // 3. Rewound root user message u1 (tree_target_id = Some("u1") -> parent m1).
        let rewound_root = read_promoted_checkpoint_for_branch(
            root.path(),
            OperatorConversationId(5),
            OperatorTurnId(3),
            Some("u1"),
        )
        .unwrap();
        assert_eq!(rewound_root.total_messages, 0);
        assert_eq!(rewound_root.epoch_total_messages, 8);
        assert_eq!(rewound_root.latest_context_tokens, None);
        assert_eq!(rewound_root.tokens.total, branch_b.tokens.total);

        // 4. Tree preview unwrapping and entry kinds.
        let tree = read_session_tree(
            root.path(),
            OperatorConversationId(5),
            OperatorTurnId(3),
            None,
            &completed_turns,
        )
        .unwrap();
        assert_eq!(tree.current_leaf_id.as_deref(), Some("a2b"));
        let u1_entry = tree.entries.iter().find(|e| e.id == "u1").unwrap();
        assert_eq!(u1_entry.preview, "first prompt line two");
        let u2a_entry = tree.entries.iter().find(|e| e.id == "u2a").unwrap();
        assert_eq!(u2a_entry.preview, "explore branch A");
        let tool_entry = tree.entries.iter().find(|e| e.id == "a2a-tool").unwrap();
        assert_eq!(
            tool_entry.kind,
            OperatorTreeEntryKind::Assistant { tool_only: true }
        );

        // 5. No-op when selecting already-active leaf a2b.
        let noop = resolve_tree_target(
            root.path(),
            OperatorConversationId(5),
            OperatorTurnId(3),
            None,
            &completed_turns,
            "a2b",
        )
        .unwrap();
        assert!(noop.noop);
        assert_eq!(noop.active_tip_turn, Some(OperatorTurnId(3)));

        // 6. Switch to Branch A assistant entry a2a.
        let nav_a = resolve_tree_target(
            root.path(),
            OperatorConversationId(5),
            OperatorTurnId(3),
            None,
            &completed_turns,
            "a2a",
        )
        .unwrap();
        assert!(!nav_a.noop);
        assert_eq!(nav_a.active_tip_turn, Some(OperatorTurnId(2)));
        assert_eq!(nav_a.pi_tree_target_id.as_deref(), Some("a2a"));
        assert_eq!(nav_a.current_leaf_id.as_deref(), Some("a2a"));
        assert_eq!(nav_a.editor_text, None);
        assert_eq!(nav_a.restored_context, None);

        // 7. Rewind u2a (whose parent is m2 -> a1): active_tip_turn must be T1, NOT T2!
        let rewind_u2a = resolve_tree_target(
            root.path(),
            OperatorConversationId(5),
            OperatorTurnId(3),
            Some("a2a"),
            &completed_turns,
            "u2a",
        )
        .unwrap();
        assert!(!rewind_u2a.noop);
        assert_eq!(rewind_u2a.active_tip_turn, Some(OperatorTurnId(1)));
        assert_eq!(rewind_u2a.pi_tree_target_id.as_deref(), Some("u2a"));
        assert_eq!(rewind_u2a.current_leaf_id.as_deref(), Some("m2"));
        assert_eq!(rewind_u2a.editor_text.as_deref(), Some("explore branch A"));
        assert_eq!(rewind_u2a.restored_context, Some(ctx2a));

        // 8. Rewind root u1 (whose parent is m1 -> null): active_tip_turn must be None!
        let rewind_u1 = resolve_tree_target(
            root.path(),
            OperatorConversationId(5),
            OperatorTurnId(3),
            None,
            &completed_turns,
            "u1",
        )
        .unwrap();
        assert!(!rewind_u1.noop);
        assert_eq!(rewind_u1.active_tip_turn, None);
        assert_eq!(rewind_u1.pi_tree_target_id.as_deref(), Some("u1"));
        assert_eq!(rewind_u1.current_leaf_id.as_deref(), Some("m1"));
        assert_eq!(
            rewind_u1.editor_text.as_deref(),
            Some("first prompt\nline two")
        );
        assert_eq!(rewind_u1.restored_context, Some(ctx1));
    }
}

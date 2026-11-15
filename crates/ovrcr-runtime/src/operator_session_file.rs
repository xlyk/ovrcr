//! Bounded, zero-spawn reader and cleanup helpers for Operator native Pi checkpoints.
//!
//! Designed as a dedicated module so `/session` (`#487`) and `/tree` (`#489`) share
//! one strict `O_NOFOLLOW`, regular-file, record-bounded `session.jsonl` reader without
//! exposing provider payloads, credentials, or raw stderr.

use anyhow::{Result, bail};
use ovrcr_protocol::{OperatorConversationId, OperatorModel, OperatorTokenStats, OperatorTurnId};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub const SESSION_CHECKPOINT_UNAVAILABLE: &str =
    "Operator session checkpoint is unavailable or invalid.";

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
    pub tokens: OperatorTokenStats,
    pub cost_usd_nanos: u64,
    pub latest_context_tokens: Option<u64>,
    pub inline_context_window: Option<u64>,
}

pub fn checkpoint_dir(registry_path: &Path, turn: OperatorTurnId) -> PathBuf {
    registry_path
        .join("operator-pi")
        .join("turns")
        .join(turn.0.to_string())
}

pub fn read_promoted_checkpoint(
    registry_path: &Path,
    conversation: OperatorConversationId,
    turn: OperatorTurnId,
) -> Result<PromotedCheckpointSummary> {
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

    let mut summary = PromotedCheckpointSummary {
        pi_session_id: expected_id,
        ..PromotedCheckpointSummary::default()
    };
    let mut total_bytes = header_bytes.len() as u64;
    let mut records = 0usize;
    let mut line = Vec::new();
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
        let entry: Value = serde_json::from_slice(&line)
            .map_err(|_| anyhow::anyhow!(SESSION_CHECKPOINT_UNAVAILABLE))?;
        if !entry.is_object() {
            bail!("{SESSION_CHECKPOINT_UNAVAILABLE}");
        }
        accumulate_entry(&mut summary, &entry);
    }
    Ok(summary)
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
}

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    Generic,
    ClaudeCodeStatusline,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextUsageReport {
    pub source: ContextSource,
    pub model: Option<String>,
    pub conversation: Option<String>,
    pub used_tokens: Option<u64>,
    pub capacity_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextUsageSnapshot {
    pub report: ContextUsageReport,
    pub received_unix_ms: u64,
}

pub fn validate_context(report: &ContextUsageReport) -> Result<()> {
    validate_identifier(report.model.as_deref(), "model")?;
    validate_identifier(report.conversation.as_deref(), "conversation")?;
    if report.capacity_tokens == Some(0) {
        bail!("context capacity must be positive when known");
    }
    Ok(())
}

fn validate_identifier(value: Option<&str>, name: &str) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_empty() {
        bail!("context {name} must not be empty");
    }
    if value.len() > 256 {
        bail!("context {name} exceeds 256 UTF-8 bytes");
    }
    if value.chars().any(char::is_control) {
        bail!("context {name} contains a control character");
    }
    Ok(())
}

pub fn parse_context_json(bytes: &[u8]) -> Result<ContextUsageReport> {
    let report: ContextUsageReport = serde_json::from_slice(bytes)?;
    validate_context(&report)?;
    Ok(report)
}

#[derive(Deserialize)]
struct ClaudeContextInput {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    model: Option<ClaudeModel>,
    #[serde(default)]
    context_window: Option<ClaudeContextWindow>,
}

#[derive(Deserialize)]
struct ClaudeModel {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeContextWindow {
    #[serde(default)]
    context_window_size: Option<u64>,
    #[serde(default)]
    current_usage: Option<ClaudeCurrentUsage>,
}

#[derive(Deserialize)]
struct ClaudeCurrentUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
}

pub fn parse_claude_context(bytes: &[u8]) -> Result<ContextUsageReport> {
    let input: ClaudeContextInput =
        serde_json::from_slice(bytes).context("parse Claude context input")?;
    let used_tokens = input
        .context_window
        .as_ref()
        .and_then(|window| window.current_usage.as_ref())
        .map(|usage| -> Result<Option<u64>> {
            let Some(input_tokens) = usage.input_tokens else {
                return Ok(None);
            };
            let Some(cache_creation_input_tokens) = usage.cache_creation_input_tokens else {
                return Ok(None);
            };
            let Some(cache_read_input_tokens) = usage.cache_read_input_tokens else {
                return Ok(None);
            };
            let total = input_tokens
                .checked_add(cache_creation_input_tokens)
                .and_then(|total| total.checked_add(cache_read_input_tokens))
                .ok_or_else(|| anyhow::anyhow!("context usage exceeds u64"))?;
            Ok(Some(total))
        })
        .transpose()?
        .flatten();
    let report = ContextUsageReport {
        source: ContextSource::ClaudeCodeStatusline,
        model: input.model.and_then(|model| model.id),
        conversation: input.session_id,
        used_tokens,
        capacity_tokens: input
            .context_window
            .and_then(|window| window.context_window_size),
    };
    validate_context(&report)?;
    Ok(report)
}

pub fn format_context(sample: Option<&ContextUsageSnapshot>, now_ms: u64, exited: bool) -> String {
    let Some(sample) = sample else {
        return "—".to_owned();
    };

    let percentage = match (sample.report.used_tokens, sample.report.capacity_tokens) {
        (Some(used), Some(capacity)) if capacity > 0 => {
            if used > capacity {
                ">100%".to_owned()
            } else {
                format!("{}%", u128::from(used) * 100 / u128::from(capacity))
            }
        }
        _ => "—".to_owned(),
    };
    if crate::freshness::is_stale(sample.received_unix_ms, now_ms, exited) {
        format!("{percentage}~")
    } else {
        percentage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_semantics() {
        let mut sample = ContextUsageSnapshot {
            report: ContextUsageReport {
                source: ContextSource::Generic,
                model: None,
                conversation: None,
                used_tokens: Some(25),
                capacity_tokens: Some(100),
            },
            received_unix_ms: 1_000,
        };

        let mut cases = vec![
            ("zero", Some(0), Some(100), "0%"),
            ("unknown usage", None, Some(100), "—"),
            ("unknown capacity", Some(25), None, "—"),
            ("exact percentage", Some(25), Some(100), "25%"),
            ("over capacity", Some(101), Some(100), ">100%"),
            ("maximum usage", Some(u64::MAX), Some(u64::MAX), "100%"),
        ];
        for (name, used_tokens, capacity_tokens, expected) in cases.drain(..) {
            sample.report.used_tokens = used_tokens;
            sample.report.capacity_tokens = capacity_tokens;
            assert_eq!(
                format_context(Some(&sample), 1_000, false),
                expected,
                "{name}"
            );
        }

        sample.report.used_tokens = Some(25);
        sample.report.capacity_tokens = Some(100);
        assert_eq!(format_context(Some(&sample), 1_000, false), "25%");
        assert_eq!(format_context(Some(&sample), 300_999, false), "25%");
        assert_eq!(format_context(Some(&sample), 301_000, false), "25%~");
        assert_eq!(format_context(Some(&sample), 999, false), "25%~");
        sample.report.used_tokens = Some(u64::MAX);
        assert_eq!(format_context(Some(&sample), 1_000, false), ">100%");
        sample.report.capacity_tokens = None;
        assert_eq!(format_context(Some(&sample), 1_000, false), "—");
        assert_eq!(format_context(None, 1_000, true), "—");
    }

    #[test]
    fn context_json_validation() {
        let unknown = parse_context_json(br#"{"source":"generic"}"#).unwrap();
        assert_eq!(unknown.used_tokens, None);
        assert_eq!(unknown.capacity_tokens, None);

        let zero =
            parse_context_json(br#"{"source":"generic","used_tokens":0,"capacity_tokens":1}"#)
                .unwrap();
        assert_eq!(zero.used_tokens, Some(0));

        let invalid = [
            (
                "zero capacity",
                br#"{"source":"generic","capacity_tokens":0}"#.to_vec(),
            ),
            (
                "negative count",
                br#"{"source":"generic","used_tokens":-1,"capacity_tokens":1}"#.to_vec(),
            ),
            (
                "fractional count",
                br#"{"source":"generic","used_tokens":1.5,"capacity_tokens":1}"#.to_vec(),
            ),
            (
                "escaped control",
                br#"{"source":"generic","model":"ok\u0001"}"#.to_vec(),
            ),
            (
                "overlong identifier",
                format!(r#"{{"source":"generic","model":"{}"}}"#, "x".repeat(257)).into_bytes(),
            ),
            ("malformed JSON", br#"{"source":"generic""#.to_vec()),
            (
                "unknown key",
                br#"{"source":"generic","other":true}"#.to_vec(),
            ),
        ];
        for (name, input) in invalid {
            assert!(parse_context_json(&input).is_err(), "{name}");
        }
    }

    #[test]
    fn context_claude_projection() {
        let report = parse_claude_context(
            br#"{"session_id":"fixture-conversation","model":{"id":"fixture-model"},"context_window":{"context_window_size":200000,"current_usage":{"input_tokens":8500,"output_tokens":1200,"cache_creation_input_tokens":5000,"cache_read_input_tokens":2000}},"irrelevant":{"secret":"ignored"}}"#,
        )
        .unwrap();
        assert_eq!(report.source, ContextSource::ClaudeCodeStatusline);
        assert_eq!(report.model.as_deref(), Some("fixture-model"));
        assert_eq!(report.conversation.as_deref(), Some("fixture-conversation"));
        assert_eq!(report.used_tokens, Some(15_500));
        assert_eq!(report.capacity_tokens, Some(200_000));

        let replacement = parse_claude_context(
            br#"{"session_id":"replacement-conversation","model":{"id":"replacement-model"},"context_window":{"context_window_size":100000,"current_usage":{"input_tokens":1,"cache_creation_input_tokens":2,"cache_read_input_tokens":3}}}"#,
        )
        .unwrap();
        assert_eq!(replacement.model.as_deref(), Some("replacement-model"));
        assert_eq!(
            replacement.conversation.as_deref(),
            Some("replacement-conversation")
        );
        assert_eq!(replacement.used_tokens, Some(6));
        assert_eq!(replacement.capacity_tokens, Some(100_000));
    }

    #[test]
    fn context_claude_projection_missing_components_are_unknown() {
        for input in [
            br#"{"context_window":{"current_usage":{"cache_creation_input_tokens":1,"cache_read_input_tokens":2}}}"#.as_slice(),
            br#"{"context_window":{"current_usage":{"input_tokens":1,"cache_read_input_tokens":2}}}"#.as_slice(),
            br#"{"context_window":{"current_usage":{"input_tokens":1,"cache_creation_input_tokens":2}}}"#.as_slice(),
            br#"{"context_window":{"current_usage":{"input_tokens":null,"cache_creation_input_tokens":1,"cache_read_input_tokens":2}}}"#.as_slice(),
            br#"{"context_window":{"current_usage":{"input_tokens":1,"cache_creation_input_tokens":null,"cache_read_input_tokens":2}}}"#.as_slice(),
            br#"{"context_window":{"current_usage":{"input_tokens":1,"cache_creation_input_tokens":2,"cache_read_input_tokens":null}}}"#.as_slice(),
            br#"{"context_window":{"current_usage":null}}"#.as_slice(),
            br#"{"context_window":null}"#.as_slice(),
        ] {
            assert_eq!(parse_claude_context(input).unwrap().used_tokens, None);
        }
        assert!(parse_claude_context(
            br#"{"context_window":{"current_usage":{"input_tokens":"bad","cache_creation_input_tokens":2,"cache_read_input_tokens":3}}}"#,
        )
        .is_err());
    }

    #[test]
    fn context_claude_projection_rejects_checked_add_overflow() {
        let error = parse_claude_context(
            br#"{"context_window":{"current_usage":{"input_tokens":18446744073709551615,"cache_creation_input_tokens":1,"cache_read_input_tokens":0}}}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("context usage exceeds u64"));
    }
}

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub const CONTEXT_STALE_AFTER_MS: u64 = 300_000;

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

pub fn context_is_stale(sample: &ContextUsageSnapshot, now_ms: u64, exited: bool) -> bool {
    if exited {
        return true;
    }
    match now_ms.checked_sub(sample.received_unix_ms) {
        Some(age_ms) => age_ms >= CONTEXT_STALE_AFTER_MS,
        None => true,
    }
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
    if context_is_stale(sample, now_ms, exited) {
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
}

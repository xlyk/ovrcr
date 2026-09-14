//! Pure Claude metrics normalization. This does not certify or read transcripts.
use anyhow::{Context, Result, bail};
use ovrcr_protocol::{
    ContextSample, CostKind, Measurement, MeasurementFreshness, SampleQuality, UsageCost,
    UsageCoverage, UsageScope, UsageTotals, validate_agent_id,
};
use serde::Deserialize;
use serde_json::{Value, value::RawValue};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct ClaudeMetrics {
    pub conversation: String,
    pub model: Option<String>,
    pub context: Measurement<ContextSample>,
    pub cost: Measurement<Option<UsageCost>>,
}

#[derive(Deserialize)]
struct StatuslineInput {
    session_id: String,
    #[serde(default, deserialize_with = "present_raw")]
    agent_id: Option<Box<RawValue>>,
    cost: Option<StatuslineCost>,
}

#[derive(Deserialize)]
struct StatuslineCost {
    total_cost_usd: Option<Box<RawValue>>,
}

fn present_raw<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Box<RawValue>>, D::Error> {
    Box::<RawValue>::deserialize(d).map(Some)
}

fn uncertain<T>(value: T) -> Measurement<T> {
    Measurement {
        value,
        source: "claude_statusline".into(),
        source_revision: None,
        source_sequence: None,
        freshness: MeasurementFreshness::Uncertain,
    }
}

/// Identity is retained for the supervisor to check; parsing never authorizes binding.
pub fn parse_claude_metrics(input: &[u8]) -> Result<ClaudeMetrics> {
    if input.len() > 65_536 {
        bail!("Claude statusline exceeds input limit");
    }
    let raw: StatuslineInput = serde_json::from_slice(input).context("parse Claude statusline")?;
    validate_agent_id(&raw.session_id)?;
    if raw.agent_id.is_some() {
        bail!("child statusline cannot report root metrics");
    }
    let context = ovrcr_protocol::context::parse_claude_context(input)?;
    let cost = raw
        .cost
        .and_then(|c| c.total_cost_usd)
        .map(|value| -> Result<UsageCost> {
            Ok(UsageCost {
                usd_ticks: decimal_usd_ticks(value.get())?,
                kind: CostKind::Estimated,
                scope: UsageScope::Conversation,
            })
        })
        .transpose()?;
    Ok(ClaudeMetrics {
        conversation: raw.session_id,
        model: context.model,
        context: uncertain(ContextSample {
            used_tokens: context.used_tokens,
            capacity_tokens: context.capacity_tokens,
            quality: SampleQuality::Observed,
        }),
        cost: uncertain(cost),
    })
}

/// Input is an original, JSON-validated lexeme, never a reserialized f64.
fn decimal_usd_ticks(raw: &str) -> Result<u64> {
    if !raw.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        bail!("cost must be a nonnegative decimal number");
    }
    let (mantissa, exponent) = match raw.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (
            mantissa,
            exponent
                .parse::<i64>()
                .context("cost exponent out of range")?,
        ),
        None => (raw, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return Ok(0);
    }
    let integer_digits = whole.len() as i128 + i128::from(exponent) + 10
        - (digits.len() - significant.len()) as i128;
    if integer_digits < 0 {
        return Ok(0);
    }
    if integer_digits > 20 {
        bail!("cost exceeds USD tick range");
    }
    let count = integer_digits as usize;
    let retained = count.min(significant.len());
    let mut ticks = 0u64;
    for digit in significant
        .bytes()
        .take(retained)
        .chain(std::iter::repeat_n(b'0', count - retained))
    {
        ticks = ticks
            .checked_mul(10)
            .and_then(|n| n.checked_add(u64::from(digit - b'0')))
            .context("cost exceeds USD tick range")?;
    }
    let discarded = &significant.as_bytes()[retained..];
    if let Some(&first) = discarded.first()
        && (first > b'5'
            || (first == b'5' && (discarded[1..].iter().any(|&d| d != b'0') || ticks % 2 == 1)))
    {
        ticks = ticks
            .checked_add(1)
            .context("rounded cost exceeds USD tick range")?;
    }
    Ok(ticks)
}

type RecordKey = (Box<str>, Box<str>);
type RecordUsage = [Option<u64>; 5];

/// A pure last-record-replacement algorithm, not a certified Claude transcript reader.
/// Coverage is always partial: this index does not establish native category coverage.
pub struct ClaudeUsageAccumulator {
    records: BTreeMap<RecordKey, RecordUsage>,
    sums: [u64; 5],
    unknown: [usize; 5],
    identity_limit: usize,
    byte_limit: usize,
    retained_bytes: usize,
    diagnostic: Option<&'static str>,
}
impl Default for ClaudeUsageAccumulator {
    fn default() -> Self {
        Self {
            records: BTreeMap::new(),
            sums: [0; 5],
            unknown: [0; 5],
            identity_limit: super::reporter::MAX_IDENTITIES,
            byte_limit: super::reporter::MAX_IDENTITY_BYTES,
            retained_bytes: 0,
            diagnostic: None,
        }
    }
}
impl ClaudeUsageAccumulator {
    pub fn diagnostic(&self) -> Option<&'static str> {
        self.diagnostic
    }
    pub fn retained_identities(&self) -> usize {
        self.records.len()
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
    /// Subset mode compares the exact five normalized counted components, including
    /// unknowns and cache subsets. Excluded auxiliary metadata is not compared.
    /// A differing counted value freezes the prefix until replacement semantics
    /// are certified; no probabilistic digest or second identity index is used.
    pub fn apply_unique_record(&mut self, record: &Value) -> Result<bool> {
        self.apply(record, false)
    }
    /// The eventual reader must classify record categories and validate file identity.
    pub fn apply_record(&mut self, record: &Value) -> Result<bool> {
        self.apply(record, true)
    }
    fn apply(&mut self, record: &Value, replacement: bool) -> Result<bool> {
        if matches!(
            self.diagnostic,
            Some("accounting_limit" | "conflicting_usage_record")
        ) {
            return Ok(false);
        }
        let result = self.apply_checked(record, replacement);
        if result.is_err() {
            self.diagnostic = Some("invalid_usage_record");
        }
        result
    }

    fn apply_checked(&mut self, record: &Value, replacement: bool) -> Result<bool> {
        if record.get("type").and_then(Value::as_str) != Some("assistant") {
            return Ok(false);
        }
        let message = record.get("message").context("missing assistant message")?;
        let key = (record_id(message, "id")?, record_id(record, "requestId")?);
        let usage = message
            .get("usage")
            .filter(|u| u.is_object())
            .context("missing assistant usage")?;
        let input = token(usage, "input_tokens")?;
        let read = token(usage, "cache_read_input_tokens")?;
        let write = token(usage, "cache_creation_input_tokens")?;
        let output = token(usage, "output_tokens")?;
        let reasoning = match usage.get("output_tokens_details") {
            None | Some(Value::Null) => None,
            Some(details) if details.is_object() => token(details, "thinking_tokens")?,
            Some(_) => bail!("invalid output token details"),
        };
        if let (Some(reasoning), Some(output)) = (reasoning, output)
            && reasoning > output
        {
            bail!("reasoning subset exceeds output");
        }
        let canonical = match (input, read, write) {
            (Some(input), Some(read), Some(write)) => Some(
                input
                    .checked_add(read)
                    .and_then(|v| v.checked_add(write))
                    .context("input token overflow")?,
            ),
            _ => None,
        };
        let next = [canonical, output, read, write, reasoning];
        let old = self.records.get(&key);
        if old == Some(&next) {
            return Ok(false);
        }
        if old.is_some() && !replacement {
            self.diagnostic = Some("conflicting_usage_record");
            return Ok(false);
        }
        // Boxed strings retain exactly their lengths. Charge the fixed key/value
        // storage too; BTree node overhead is not part of the charged-byte cap.
        let added_bytes = if old.is_none() {
            std::mem::size_of::<RecordKey>()
                + std::mem::size_of::<RecordUsage>()
                + key.0.len()
                + key.1.len()
        } else {
            0
        };
        if old.is_none()
            && (self.records.len() >= self.identity_limit
                || added_bytes > self.byte_limit.saturating_sub(self.retained_bytes))
        {
            self.diagnostic = Some("accounting_limit");
            return Ok(false);
        }
        let mut sums = self.sums;
        let mut unknown = self.unknown;
        for i in 0..5 {
            if let Some(old) = old {
                if let Some(value) = old[i] {
                    sums[i] -= value;
                } else {
                    unknown[i] -= 1;
                }
            }
            if let Some(value) = next[i] {
                sums[i] = sums[i]
                    .checked_add(value)
                    .context("cumulative token overflow")?;
            } else {
                unknown[i] += 1;
            }
        }
        self.records.insert(key, next);
        self.retained_bytes += added_bytes;
        self.sums = sums;
        self.unknown = unknown;
        Ok(true)
    }
    pub fn snapshot(&self) -> UsageTotals {
        let known =
            |i: usize| (!self.records.is_empty() && self.unknown[i] == 0).then_some(self.sums[i]);
        UsageTotals {
            scope: UsageScope::Conversation,
            coverage: UsageCoverage::Partial,
            input_tokens: known(0),
            output_tokens: known(1),
            cache_read_tokens: known(2),
            cache_write_tokens: known(3),
            reasoning_output_tokens: known(4),
        }
    }
}

fn record_id(record: &Value, field: &str) -> Result<Box<str>> {
    let value = record
        .get(field)
        .and_then(Value::as_str)
        .context("missing usage identity")?;
    validate_agent_id(value)?;
    Ok(value.into())
}

fn token(record: &Value, field: &str) -> Result<Option<u64>> {
    match record.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => Ok(Some(value.as_u64().context("invalid token count")?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{CostKind, MeasurementFreshness, UsageCoverage, UsageScope};
    use serde_json::json;

    fn record(message: &str, request: &str, input: u64) -> Value {
        json!({"type":"assistant", "requestId":request, "uuid":"not-a-key",
            "message":{"id":message,"usage":{"input_tokens":input,
            "cache_read_input_tokens":4,"cache_creation_input_tokens":2,
            "output_tokens":8,"output_tokens_details":{"thinking_tokens":3}}}})
    }

    #[test]
    fn statusline_context_and_exact_cost_preserve_unknowns() {
        let sample = parse_claude_metrics(
            br#"{"session_id":"conversation","agent_type":"named-root",
            "model":{"id":"model"},"context_window":{"context_window_size":100,
            "current_usage":{"input_tokens":30,"cache_creation_input_tokens":10,
            "cache_read_input_tokens":40,"output_tokens":99}},
            "cost":{"total_cost_usd":0.23859819999999995}}"#,
        )
        .unwrap();
        assert_eq!(sample.conversation, "conversation");
        assert_eq!(sample.model.as_deref(), Some("model"));
        assert_eq!(sample.context.value.used_tokens, Some(80));
        assert_eq!(sample.context.value.capacity_tokens, Some(100));
        assert_eq!(sample.context.freshness, MeasurementFreshness::Uncertain);
        assert_eq!(sample.context.source_revision, None);
        assert_eq!(sample.context.source_sequence, None);
        let cost = sample.cost.value.unwrap();
        assert_eq!(cost.usd_ticks, 2_385_982_000);
        assert_eq!(cost.kind, CostKind::Estimated);
        assert_eq!(cost.scope, UsageScope::Conversation);
        for fragment in [
            "",
            ",\"context_window\":null,\"cost\":null",
            ",\"context_window\":{\"current_usage\":null}",
            ",\"context_window\":{\"current_usage\":{\"input_tokens\":1}}",
        ] {
            let sample =
                parse_claude_metrics(format!("{{\"session_id\":\"c\"{fragment}}}").as_bytes())
                    .unwrap();
            assert_eq!(sample.context.value.used_tokens, None);
            assert_eq!(sample.cost.value, None);
        }
    }

    #[test]
    fn cost_rounds_original_decimal_ties_even_and_checks_overflow() {
        for (lexeme, ticks) in [
            ("0", 0),
            ("0.00000000005", 0),
            ("0.00000000015", 2),
            ("0.00000000025", 2),
            ("0.00000000025000000000000001", 3),
            ("2.3859819999999995e-1", 2_385_982_000),
            ("1e-10000", 0),
            ("1844674407.3709551615", u64::MAX),
            ("1844674407.37095516154", u64::MAX),
        ] {
            let bytes =
                format!("{{\"session_id\":\"c\",\"cost\":{{\"total_cost_usd\":{lexeme}}}}}");
            assert_eq!(
                parse_claude_metrics(bytes.as_bytes())
                    .unwrap()
                    .cost
                    .value
                    .unwrap()
                    .usd_ticks,
                ticks,
                "{lexeme}"
            );
        }
        for lexeme in [
            "-1",
            "-0",
            "\"1\"",
            "true",
            "1844674407.37095516155",
            "1e10000",
            "NaN",
        ] {
            let bytes =
                format!("{{\"session_id\":\"c\",\"cost\":{{\"total_cost_usd\":{lexeme}}}}}");
            assert!(parse_claude_metrics(bytes.as_bytes()).is_err(), "{lexeme}");
        }
    }

    #[test]
    fn statusline_rejects_child_malformed_identity_and_invalid_context() {
        for input in [
            r#"{}"#,
            r#"{"session_id":""}"#,
            r#"{"session_id":"c","agent_id":null}"#,
            r#"{"session_id":"c","agent_id":"child"}"#,
            r#"{"session_id":"c","agent_id":3}"#,
            r#"{"session_id":"c","context_window":{"context_window_size":0}}"#,
            r#"{"session_id":"c","context_window":{"current_usage":{"input_tokens":-1}}}"#,
            r#"{"session_id":"c","context_window":{"current_usage":{"input_tokens":18446744073709551615,"cache_creation_input_tokens":1,"cache_read_input_tokens":0}}}"#,
        ] {
            assert!(parse_claude_metrics(input.as_bytes()).is_err(), "{input}");
        }
    }

    #[test]
    fn replacement_replay_and_distinct_requests_do_not_double_count() {
        let mut totals = ClaudeUsageAccumulator::default();
        assert!(totals.apply_record(&record("m", "r", 10)).unwrap());
        assert_eq!(totals.snapshot().input_tokens, Some(16));
        assert!(totals.apply_record(&record("m", "r", 12)).unwrap());
        assert_eq!(totals.snapshot().input_tokens, Some(18));
        assert!(!totals.apply_record(&record("m", "r", 12)).unwrap());
        assert!(totals.apply_record(&record("m", "s", 12)).unwrap());
        let snapshot = totals.snapshot();
        assert_eq!(snapshot.input_tokens, Some(36));
        assert_eq!(snapshot.output_tokens, Some(16));
        assert_eq!(snapshot.cache_read_tokens, Some(8));
        assert_eq!(snapshot.cache_write_tokens, Some(4));
        assert_eq!(snapshot.reasoning_output_tokens, Some(6));
        assert_eq!(snapshot.coverage, UsageCoverage::Partial);
        assert!(totals.apply_record(&record("m", "r", 1)).unwrap());
        assert_eq!(totals.snapshot().input_tokens, Some(25));
    }

    #[test]
    fn missing_usage_components_are_unknown_until_replaced() {
        let mut totals = ClaudeUsageAccumulator::default();
        assert_eq!(totals.snapshot().input_tokens, None);
        let mut incomplete = record("m", "r", 0);
        incomplete["message"]["usage"]
            .as_object_mut()
            .unwrap()
            .remove("input_tokens");
        incomplete["message"]["usage"]["output_tokens_details"] = Value::Null;
        assert!(totals.apply_record(&incomplete).unwrap());
        assert_eq!(totals.snapshot().input_tokens, None);
        assert_eq!(totals.snapshot().reasoning_output_tokens, None);
        assert_eq!(totals.snapshot().output_tokens, Some(8));
        assert!(totals.apply_record(&record("m", "r", 0)).unwrap());
        assert_eq!(totals.snapshot().input_tokens, Some(6));
        assert_eq!(totals.snapshot().reasoning_output_tokens, Some(3));
    }

    #[test]
    fn capacity_freezes_prefix_including_later_oldest_replacements() {
        let mut totals = ClaudeUsageAccumulator {
            identity_limit: 2,
            ..Default::default()
        };
        totals.apply_record(&record("a", "a", 10)).unwrap();
        totals.apply_record(&record("b", "b", 10)).unwrap();
        assert!(totals.apply_record(&record("a", "a", 11)).unwrap());
        assert_eq!(totals.snapshot().input_tokens, Some(33));
        assert!(!totals.apply_record(&record("c", "c", 10)).unwrap());
        assert_eq!(totals.diagnostic(), Some("accounting_limit"));
        assert!(!totals.apply_record(&record("a", "a", 99)).unwrap());
        assert_eq!(totals.snapshot().input_tokens, Some(33));
        assert_eq!(totals.records.len(), 2);
    }

    #[test]
    fn byte_cap_charges_keys_and_values_before_insertion() {
        let mut totals = ClaudeUsageAccumulator::default();
        totals.apply_record(&record("m", "r", 1)).unwrap();
        assert!(
            totals.retained_bytes
                >= 2 + std::mem::size_of::<RecordKey>() + std::mem::size_of::<RecordUsage>()
        );
        totals.byte_limit = totals.retained_bytes;
        assert!(totals.apply_record(&record("m", "r", 2)).unwrap());
        assert!(!totals.apply_record(&record("next", "r", 1)).unwrap());
        assert_eq!(totals.records.len(), 1);
        assert_eq!(totals.snapshot().input_tokens, Some(8));
        assert_eq!(totals.diagnostic(), Some("accounting_limit"));
    }

    #[test]
    fn production_identity_cap_preserves_oldest_and_freezes_at_cap_plus_one() {
        let mut totals = ClaudeUsageAccumulator::default();
        for index in 0..crate::report::reporter::MAX_IDENTITIES {
            assert!(
                totals
                    .apply_record(&record(&index.to_string(), "r", 0))
                    .unwrap()
            );
        }
        assert!(totals.apply_record(&record("0", "r", 1)).unwrap());
        assert_eq!(totals.snapshot().input_tokens, Some(393_217));
        assert!(!totals.apply_record(&record("65536", "r", 0)).unwrap());
        assert!(!totals.apply_record(&record("0", "r", 100)).unwrap());
        assert_eq!(totals.snapshot().input_tokens, Some(393_217));
        assert_eq!(
            totals.records.len(),
            crate::report::reporter::MAX_IDENTITIES
        );
        assert!(totals.retained_bytes <= crate::report::reporter::MAX_IDENTITY_BYTES);
    }

    #[test]
    fn invalid_replacement_and_total_overflow_leave_snapshot_intact() {
        let mut totals = ClaudeUsageAccumulator::default();
        totals.apply_record(&record("m", "r", 1)).unwrap();
        let before = totals.snapshot();
        for bad in [json!(-1), json!(1.5), json!("2"), json!(u64::MAX)] {
            let mut invalid = record("m", "r", 1);
            invalid["message"]["usage"]["input_tokens"] = bad;
            assert!(totals.apply_record(&invalid).is_err());
            assert_eq!(totals.snapshot(), before);
            assert_eq!(totals.diagnostic(), Some("invalid_usage_record"));
        }
        let mut overflow = record("other", "r", 0);
        overflow["message"]["usage"]["output_tokens"] = json!(u64::MAX);
        assert!(totals.apply_record(&overflow).is_err());
        assert_eq!(totals.snapshot(), before);
        let mut invalid = record("m", "r", 1);
        invalid["message"]["usage"]["output_tokens_details"]["thinking_tokens"] = json!(9);
        assert!(totals.apply_record(&invalid).is_err());
        assert_eq!(totals.snapshot(), before);
    }

    #[test]
    fn unique_subset_compares_counted_unknowns_and_ignores_excluded_metadata() {
        let mut totals = ClaudeUsageAccumulator::default();
        let original = record("m", "r", 10);
        assert!(totals.apply_unique_record(&original).unwrap());
        let mut excluded = original.clone();
        excluded["message"]["usage"]["iterations"] = json!([{"input_tokens":999}]);
        assert!(!totals.apply_unique_record(&excluded).unwrap());
        assert_eq!(totals.diagnostic(), None);
        excluded["message"]["usage"]["cache_read_input_tokens"] = Value::Null;
        assert!(!totals.apply_unique_record(&excluded).unwrap());
        assert_eq!(totals.diagnostic(), Some("conflicting_usage_record"));
        assert_eq!(totals.snapshot().input_tokens, Some(16));
        assert!(!totals.apply_record(&record("other", "s", 100)).unwrap());
        assert_eq!(totals.snapshot().input_tokens, Some(16));
    }
}

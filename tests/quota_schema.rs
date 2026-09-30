//! Public normalization and protocol contracts, supplementing the real native IPC tests.
use ovrcr::protocol::{QuotaProvider, QuotaReport, QuotaState};
use ovrcr::server::normalize_native_quota;
use serde_json::json;

#[test]
fn codex_keeps_actual_windows_and_distinguishes_unknown_exhausted_and_overlimit() {
    for (used, expected, over) in [
        (json!(null), None, false),
        (json!(0), Some(10000), false),
        (json!(67), Some(3300), false),
        (json!(100), Some(0), false),
        (json!(101), None, true),
    ] {
        let input = json!({"rateLimits":{"limitId":"general","primary":{"usedPercent":used,"windowDurationMins":120,"resetsAt":1000}},"rateLimitsByLimitId":{}});
        let windows = normalize_native_quota(QuotaProvider::Codex, &input).unwrap();
        assert_eq!(windows[0].label, "2h");
        assert_eq!(windows[0].remaining_basis_points(), expected);
        assert_eq!(windows[0].over_limit, over);
        assert_eq!(windows[0].resets_unix_ms, Some(1000000));
    }
    for used in [json!(-1), json!("67"), json!(true)] {
        let input = json!({"rateLimits":{"primary":{"usedPercent":used,"windowDurationMins":300}},"rateLimitsByLimitId":{}});
        assert_eq!(
            normalize_native_quota(QuotaProvider::Codex, &input),
            Err(QuotaState::Invalid)
        );
    }
}

#[test]
fn grok_preserves_weekly_monthly_periods_and_ignores_extra_balances() {
    for (period, label) in [
        ("USAGE_PERIOD_TYPE_WEEKLY", "wk"),
        ("USAGE_PERIOD_TYPE_MONTHLY", "mo"),
    ] {
        let input = json!({"config":{"creditUsagePercent":24,"currentPeriod":{"type":period,"start":"2026-01-01T00:00:00Z","end":"2026-02-01T00:00:00Z"},"prepaidBalance":{"val":500000},"onDemandUsed":{"val":5000}}});
        let windows = normalize_native_quota(QuotaProvider::Grok, &input).unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].label, label);
        assert_eq!(windows[0].remaining_basis_points(), Some(7600));
        assert_eq!(windows[0].resets_unix_ms, Some(1769904000000));
    }
    for input in [
        json!({"config":null}),
        json!({"config":{"creditUsagePercent":24,"currentPeriod":{"type":"UNKNOWN"}}}),
    ] {
        assert_eq!(
            normalize_native_quota(QuotaProvider::Grok, &input),
            Err(QuotaState::Unsupported)
        );
    }
    let bad = json!({"config":{"creditUsagePercent":-1,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","end":"2026-02-01T00:00:00Z"}}});
    assert_eq!(
        normalize_native_quota(QuotaProvider::Grok, &bad),
        Err(QuotaState::Invalid)
    );
}

#[test]
fn authoritative_native_observation_after_account_check_is_not_stale() {
    let mut quota =
        ovrcr::protocol::ProviderQuota::unknown(QuotaProvider::Codex, QuotaState::Current);
    quota.checked_unix_ms = Some(1);
    quota.observed_unix_ms = Some(400000);
    assert!(!quota.stale(400001));
    assert!(quota.stale(800000));
    quota.state = QuotaState::Unavailable;
    assert!(quota.stale(400001));
}

#[test]
fn quota_wire_boundary_rejects_duplicate_windows_and_invalid_percentages() {
    let mut windows = normalize_native_quota(
        QuotaProvider::Codex,
        &json!({"rateLimits":{"primary":{"usedPercent":42}}}),
    )
    .unwrap();
    windows.push(windows[0].clone());
    assert!(
        QuotaReport {
            windows: Some(windows.clone()),
            state: QuotaState::Current
        }
        .validate()
        .is_err()
    );
    windows.pop();
    windows[0].used_basis_points = Some(10001);
    assert!(
        QuotaReport {
            windows: Some(windows),
            state: QuotaState::Current
        }
        .validate()
        .is_err()
    );
}

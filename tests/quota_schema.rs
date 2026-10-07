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
        // The secondary window keeps the report usable, so a null primary stays unknown.
        let input = json!({"rateLimits":{"limitId":"general","primary":{"usedPercent":used,"windowDurationMins":120,"resetsAt":1000},
            "secondary":{"usedPercent":5,"windowDurationMins":10080}},"rateLimitsByLimitId":{}});
        let windows = normalize_native_quota(QuotaProvider::Codex, &input).unwrap();
        assert_eq!(windows[0].label, "2h");
        assert_eq!(windows[0].remaining_basis_points(), expected);
        assert_eq!(windows[0].over_limit, over);
        assert_eq!(windows[0].resets_unix_ms, Some(1000000));
    }
    // Codex 0.160.1 / 0.161.0 account/rateLimits/read, sanitized: integer percentages,
    // a single weekly primary, null secondary, extra fields OVRCR does not read.
    let current = json!({"ordinaryUsageAllowed":true,"rateLimits":{"limitId":"codex","limitName":null,"normalModelSlug":null,
        "primary":{"usedPercent":12,"windowDurationMins":10080,"resetsAt":1791700000},"secondary":null,
        "credits":{"hasCredits":false,"unlimited":false,"balance":"0"},"individualLimit":null,"spendControlReached":false,
        "planType":"fixture","rateLimitReachedType":null},
        "rateLimitsByLimitId":{"codex":{"limitId":"codex","limitName":null,"normalModelSlug":null,
        "primary":{"usedPercent":12,"windowDurationMins":10080,"resetsAt":1791700000},"secondary":null,
        "credits":{"hasCredits":false,"unlimited":false,"balance":"0"},"individualLimit":null,"spendControlReached":false,
        "planType":"fixture","rateLimitReachedType":null}},
        "rateLimitResetCredits":{"availableCount":0,"credits":[]},"accountId":"fixture","rateLimitUpsell":null});
    let windows = normalize_native_quota(QuotaProvider::Codex, &current).unwrap();
    assert_eq!(windows.len(), 1, "the general bucket is not repeated");
    assert_eq!(windows[0].id, "codex/primary");
    assert_eq!(windows[0].label, "7d");
    assert_eq!(windows[0].remaining_basis_points(), Some(8800));
    // A well-formed reply with no usable value is not Current: no row of dashes.
    for empty in [
        json!({"rateLimits":{"primary":{"usedPercent":null,"windowDurationMins":300}},"rateLimitsByLimitId":{}}),
        json!({"rateLimits":null,"rateLimitsByLimitId":{}}),
    ] {
        assert_eq!(
            normalize_native_quota(QuotaProvider::Codex, &empty),
            Err(QuotaState::Unavailable)
        );
    }
    // A reply OVRCR cannot read is how a changed protocol surfaces.
    for unrecognized in [
        json!({"limits":{"primary":{"usedPercent":12}}}),
        json!([]),
        json!({"rateLimits":"codex"}),
    ] {
        assert_eq!(
            normalize_native_quota(QuotaProvider::Codex, &unrecognized),
            Err(QuotaState::Unsupported)
        );
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
        json!({"billing":{"creditUsagePercent":24}}),
        json!({"config":{"creditUsagePercent":24}}),
        json!({"config":{"creditUsagePercent":24,"currentPeriod":{"type":"UNKNOWN"}}}),
    ] {
        assert_eq!(
            normalize_native_quota(QuotaProvider::Grok, &input),
            Err(QuotaState::Unsupported)
        );
    }
    let bad = json!({"config":{"creditUsagePercent":"24","currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY","end":"2026-02-01T00:00:00Z"}}});
    assert_eq!(
        normalize_native_quota(QuotaProvider::Grok, &bad),
        Err(QuotaState::Invalid)
    );
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

/// Sanitized from the SuperGrok Heavy credits body Grok 1.0.46 logged on mntn at
/// 2026-10-07T19:38:50Z, five hours after the weekly period reset: proto3 JSON omits
/// the zero `creditUsagePercent`. It used to publish Current with no value (dashes).
#[test]
fn grok_period_start_without_credit_usage_is_zero_used_not_unknown() {
    let reset = json!({"config":{"currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY",
        "start":"2026-10-07T14:34:38.161671+00:00","end":"2026-10-14T14:34:38.161671+00:00"},
        "onDemandCap":{"val":0},"onDemandUsed":{},"prepaidBalance":{},"isUnifiedBillingUser":true,
        "billingPeriodStart":"2026-10-01T00:00:00Z","billingPeriodEnd":"2026-11-01T00:00:00Z"},
        "subscriptionTier":"SuperGrok Heavy"});
    let windows = normalize_native_quota(QuotaProvider::Grok, &reset).unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].label, "wk");
    assert_eq!(windows[0].used_basis_points, Some(0));
    assert_eq!(windows[0].remaining_basis_points(), Some(10000));
    assert!(!windows[0].over_limit);
    assert_eq!(windows[0].resets_unix_ms, Some(1791988478161));
    // The same account once usage accrues: the field returns and is read as before.
    let mut used = reset.clone();
    used["config"]["creditUsagePercent"] = json!(1.0);
    let windows = normalize_native_quota(QuotaProvider::Grok, &used).unwrap();
    assert_eq!(windows[0].remaining_basis_points(), Some(9900));
}

//! Opt-in personal Cursor dashboard adapter. The endpoint is internal, not a
//! stable public API. Credentials and response bodies never enter the snapshot.
use super::{DispatchMessage, ServerState, quota};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ovrcr_protocol::{CursorQuotaSettings, QuotaProvider, QuotaReport, QuotaState, QuotaWindow};
use quota::{Failure, NativeQuotaUpdate};
use rusqlite::{Connection, OpenFlags, OptionalExtension, types::ValueRef};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering, mpsc::TrySendError},
    time::{Duration, Instant},
};

pub struct Update {
    pub(super) config: CursorQuotaSettings,
    pub(super) native: NativeQuotaUpdate,
}

pub(super) fn apply(state: &ServerState, update: Update) {
    // A late response for a previous database/consent setting cannot select it.
    let current = state.quota_settings();
    if current.enabled && current.cursor == update.config {
        quota::apply(state, update.native);
    }
}

pub(super) fn run(state: Arc<ServerState>) {
    let mut config = None;
    let mut identity: Option<String> = None;
    let mut generation = 1u64;
    let mut due = Instant::now();
    let mut hold = None;
    let mut failures = 0;
    let mut pending = None;
    while !state.shutdown.load(Ordering::Acquire) && !state.stopping.load(Ordering::Acquire) {
        if let Some(update) = pending.take() {
            match state
                .dispatch
                .try_send(DispatchMessage::CursorQuota(update))
            {
                Ok(()) => {}
                Err(TrySendError::Full(DispatchMessage::CursorQuota(update))) => {
                    pending = Some(update)
                }
                Err(_) => break,
            }
        }
        let settings = state.quota_settings();
        let key = (settings.enabled, settings.cursor.clone());
        if config.as_ref() != Some(&key) {
            config = Some(key.clone());
            generation += 1;
            identity = None;
            failures = 0;
            due = Instant::now();
            hold = None;
            pending = None;
            if key.0 && key.1.dashboard {
                // A new configured account starts with no previous allowance.
                pending = Some(Box::new(update(
                    &key.1,
                    generation,
                    None,
                    QuotaState::Checking,
                    false,
                    None,
                    None,
                )));
            }
        }
        // Publish a clearing generation before starting a changed account's
        // request. Queue pressure must delay the request, not keep old values.
        if pending.is_some() {
            std::thread::park_timeout(Duration::from_millis(100));
            continue;
        }
        if !key.0 || !key.1.dashboard || !state.dashboard.is_claimed() {
            std::thread::park_timeout(Duration::from_millis(100));
            continue;
        }
        if quota::take_cursor_due(&state) && hold.is_none_or(|until| Instant::now() >= until) {
            due = Instant::now();
        }
        if Instant::now() < due {
            std::thread::park_timeout(Duration::from_millis(100));
            continue;
        }
        let path = state_path(&key.1);
        let auth = path
            .and_then(|path| load_session(&path, quota::clock_ms()).map(|session| (path, session)));
        let new_identity = auth.as_ref().ok().map(|(_, session)| session.user.clone());
        if identity != new_identity {
            generation += 1;
            identity = new_identity;
            pending = Some(Box::new(update(
                &key.1,
                generation,
                None,
                QuotaState::Checking,
                false,
                None,
                None,
            )));
            continue;
        }
        let result = auth.and_then(|(path, session)| {
            let body = quota::http_get(
                &usage_url()?,
                &[
                    ("Accept", "application/json"),
                    ("Cookie", &session.cookie()),
                ],
            );
            // Reject an account change or logout while HTTP was in flight,
            // including a changed token for the same account.
            if load_session(&path, quota::clock_ms()).ok().as_ref() != Some(&session) {
                generation += 1;
                identity = None;
                return Err(Failure::new(QuotaState::SourceConflict)
                    .because("Cursor account changed during read"));
            }
            windows(&body?)
        });
        if state.shutdown.load(Ordering::Acquire) || state.stopping.load(Ordering::Acquire) {
            break;
        }
        let (windows, status, checked, reason, delay) = match result {
            Ok(windows) => {
                failures = 0;
                hold = None;
                (
                    Some(windows),
                    QuotaState::Current,
                    true,
                    None,
                    Duration::from_secs(300),
                )
            }
            Err(failure) => {
                failures += 1;
                let delay = quota::backoff(failures, failure.deterministic, failure.retry_after);
                hold = failure.retry_after.map(|delay| Instant::now() + delay);
                (None, failure.state, false, Some(failure.reason), delay)
            }
        };
        // A refresh accepted during this read is satisfied by its result.
        quota::take_cursor_due(&state);
        due = Instant::now() + delay;
        pending = Some(Box::new(update(
            &key.1,
            generation,
            windows,
            status,
            checked,
            reason,
            Some(quota::clock_ms().saturating_add(delay.as_millis() as u64)),
        )));
    }
}

fn update(
    config: &CursorQuotaSettings,
    generation: u64,
    windows: Option<Vec<QuotaWindow>>,
    state: QuotaState,
    checked: bool,
    reason: Option<String>,
    next_check_unix_ms: Option<u64>,
) -> Update {
    Update {
        config: config.clone(),
        native: NativeQuotaUpdate {
            provider: QuotaProvider::Cursor,
            generation,
            report: QuotaReport { windows, state },
            checked,
            reason,
            next_check_unix_ms,
        },
    }
}

fn state_path(config: &CursorQuotaSettings) -> Result<PathBuf, Failure> {
    if let Some(path) = &config.state_db {
        return path
            .is_absolute()
            .then(|| path.clone())
            .ok_or_else(unreadable_auth);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(unreadable_auth)?;
    #[cfg(target_os = "macos")]
    let root = home.join("Library/Application Support");
    #[cfg(not(target_os = "macos"))]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    Ok(root.join("Cursor/User/globalStorage/state.vscdb"))
}

fn unreadable_auth() -> Failure {
    Failure::unavailable("Cursor desktop credentials unreadable")
}
fn no_auth() -> Failure {
    Failure::new(QuotaState::NotSignedIn).because("sign in to Cursor desktop")
}

#[derive(PartialEq, Eq)]
struct Session {
    token: String,
    user: String,
}
impl Session {
    fn cookie(&self) -> String {
        format!("WorkosCursorSessionToken={}%3A%3A{}", self.user, self.token)
    }
}

fn session(token: String, now: u64) -> Result<Session, Failure> {
    if token.is_empty() {
        return Err(no_auth());
    }
    if token.len() > 8192 {
        return Err(unreadable_auth());
    }
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
    {
        return Err(unreadable_auth());
    }
    let json: Value = URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or_else(unreadable_auth)?;
    let user = json["sub"]
        .as_str()
        .and_then(|sub| sub.rsplit('|').next())
        .filter(|user| {
            !user.is_empty()
                && user.len() <= 256
                && user
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
        .ok_or_else(unreadable_auth)?;
    let exp = json["exp"]
        .as_u64()
        .and_then(|seconds| seconds.checked_mul(1000))
        .ok_or_else(unreadable_auth)?;
    if exp <= now.saturating_add(60_000) {
        return Err(no_auth());
    }
    Ok(Session {
        user: user.to_owned(),
        token,
    })
}

fn load_session(path: &Path, now: u64) -> Result<Session, Failure> {
    if !path.try_exists().map_err(|_| unreadable_auth())? {
        return Err(no_auth());
    }
    // Sidecars belong to the real database. An alias must not bypass its WAL
    // and return a checkpointed login for a previous account.
    let resolved = path.canonicalize().map_err(|_| unreadable_auth())?;
    let path = resolved.as_path();
    let wal = path.with_file_name(format!(
        "{}-wal",
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(unreadable_auth)?
    ));
    let shm = path.with_file_name(format!(
        "{}-shm",
        path.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(unreadable_auth)?
    ));
    let active_wal = wal.try_exists().map_err(|_| unreadable_auth())?;
    if active_wal && !shm.try_exists().map_err(|_| unreadable_auth())? {
        return Err(unreadable_auth());
    }
    // Immutable main-file reads do not create sidecars. Never ignore an active WAL.
    let db = if active_wal {
        Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
    } else {
        let path = path.to_str().ok_or_else(unreadable_auth)?;
        let encoded: String = path
            .bytes()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || b"/._-".contains(&byte) {
                    char::from(byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect();
        Connection::open_with_flags(
            format!("file:{encoded}?immutable=1"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
    }
    .map_err(|_| unreadable_auth())?;
    db.busy_timeout(Duration::from_millis(250))
        .map_err(|_| unreadable_auth())?;
    let token = db
        .query_row(
            "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken' LIMIT 1",
            [],
            |row| {
                let value = match row.get_ref(0)? {
                    ValueRef::Text(bytes) | ValueRef::Blob(bytes) if bytes.len() <= 16_384 => {
                        decode_token(bytes)
                    }
                    _ => None,
                };
                Ok(value)
            },
        )
        .optional()
        .map_err(|_| unreadable_auth())?
        .ok_or_else(no_auth)?
        .ok_or_else(unreadable_auth)?;
    if !active_wal && wal.try_exists().map_err(|_| unreadable_auth())? {
        return Err(unreadable_auth());
    }
    session(token, now)
}

fn decode_token(bytes: &[u8]) -> Option<String> {
    if bytes.len().is_multiple_of(2)
        && !bytes.is_empty()
        && bytes
            .as_chunks::<2>()
            .0
            .iter()
            .all(|b| b[0].is_ascii() && b[0] != 0 && b[1] == 0)
    {
        return String::from_utf16(
            &bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .ok();
    }
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

fn usage_url() -> Result<String, Failure> {
    match std::env::var("OVRCR_QUOTA_CURSOR_USAGE_URL") {
        Err(_) => Ok("https://cursor.com/api/usage-summary".into()),
        Ok(url) => {
            // The test seam only accepts a literal loopback IP and numeric port;
            // userinfo, remote hosts and redirect following cannot leak the cookie.
            let valid = url
                .strip_prefix("http://127.0.0.1:")
                .and_then(|rest| rest.split_once('/'))
                .is_some_and(|(port, _)| port.parse::<u16>().is_ok_and(|port| port > 0));
            if valid {
                Ok(url)
            } else {
                Err(Failure::unavailable("invalid Cursor test URL"))
            }
        }
    }
}

fn invalid() -> Failure {
    Failure::new(QuotaState::Invalid).because("Cursor usage summary invalid")
}
fn percent(value: Option<&Value>) -> Result<Option<f64>, Failure> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let number = value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or_else(invalid)?;
    Ok(Some(number))
}
fn cents(value: Option<&Value>) -> Result<Option<u64>, Failure> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    value.as_u64().map(Some).ok_or_else(invalid)
}
fn date(value: Option<&Value>) -> Result<Option<u64>, Failure> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let stamp = chrono::DateTime::parse_from_rfc3339(value.as_str().ok_or_else(invalid)?)
        .map_err(|_| invalid())?
        .timestamp_millis();
    u64::try_from(stamp).map(Some).map_err(|_| invalid())
}

fn windows(body: &str) -> Result<Vec<QuotaWindow>, Failure> {
    let value: Value = serde_json::from_str(body).map_err(|_| invalid())?;
    if !value.is_object() {
        return Err(invalid());
    }
    // Personal replies include an empty teamUsage object. Only that placeholder
    // is safe to ignore; populated or malformed team usage remains unsupported.
    if value
        .get("teamUsage")
        .is_some_and(|v| !v.is_null() && v.as_object().is_none_or(|team| !team.is_empty()))
    {
        return Err(Failure::new(QuotaState::Unsupported).because("Cursor team usage unsupported"));
    }
    let reset = date(value.get("billingCycleEnd"))?;
    let start = date(value.get("billingCycleStart"))?;
    if start.zip(reset).is_some_and(|(start, end)| start >= end) {
        return Err(invalid());
    }
    let individual = value
        .get("individualUsage")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Failure::new(QuotaState::Unsupported).because("Cursor personal usage not reported")
        })?;
    let mut windows = Vec::new();
    for (key, label, general) in [("plan", "plan", true), ("onDemand", "on-demand", false)] {
        let Some(bucket) = individual.get(key).filter(|v| !v.is_null()) else {
            continue;
        };
        let bucket = bucket.as_object().ok_or_else(invalid)?;
        if bucket
            .get("enabled")
            .is_some_and(|v| v != &Value::Bool(true) && !v.is_null())
        {
            if bucket["enabled"] == false {
                continue;
            }
            return Err(invalid());
        }
        let used = cents(bucket.get("used"))?;
        let limit = cents(bucket.get("limit"))?;
        let remaining = cents(bucket.get("remaining"))?;
        if let Some((used, limit, remaining)) =
            used.zip(limit).zip(remaining).map(|((u, l), r)| (u, l, r))
            && remaining != limit.saturating_sub(used)
        {
            return Err(
                Failure::new(QuotaState::SourceConflict).because("Cursor usage fields disagree")
            );
        }
        let total = if key == "plan" {
            percent(bucket.get("totalPercentUsed"))?
        } else {
            None
        };
        let ratio = used
            .zip(limit)
            .filter(|(_, limit)| *limit > 0)
            .map(|(used, limit)| used as f64 / limit as f64 * 100.0);
        // The explicit dashboard total is authoritative. Pool percentages may
        // use independent denominators; never average them or infer plan limits.
        let total = total.or(ratio);
        if total.is_some() || reset.is_some() {
            windows.push(window(key, label, general, total, reset));
        }
        if key == "plan" {
            for (field, label) in [("autoPercentUsed", "Auto"), ("apiPercentUsed", "API")] {
                if let Some(pct) = percent(bucket.get(field))? {
                    windows.push(window(field, label, false, Some(pct), reset));
                }
            }
        }
    }
    if windows.is_empty() {
        return Err(
            Failure::new(QuotaState::Unsupported).because("Cursor quota fields not reported")
        );
    }
    Ok(windows)
}
fn window(
    id: &str,
    label: &str,
    general: bool,
    used: Option<f64>,
    reset: Option<u64>,
) -> QuotaWindow {
    QuotaWindow {
        id: format!("cursor/{id}"),
        label: label.into(),
        general,
        used_basis_points: used
            .filter(|used| *used <= 100.0)
            .map(|used| (used * 100.0).floor() as u16),
        over_limit: used.is_some_and(|used| used > 100.0),
        resets_unix_ms: reset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn token(user: &str, expiry: u64) -> String {
        format!(
            "e30.{}.sig",
            URL_SAFE_NO_PAD
                .encode(json!({"sub": format!("auth|{user}"), "exp":expiry}).to_string())
        )
    }
    fn parse(value: Value) -> Vec<QuotaWindow> {
        windows(&value.to_string()).unwrap()
    }
    fn database(path: &Path, value: &[u8]) {
        let db = Connection::open(path).unwrap();
        db.execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)")
            .unwrap();
        db.execute(
            "INSERT INTO ItemTable VALUES ('cursorAuth/accessToken', ?1)",
            [value],
        )
        .unwrap();
    }

    #[test]
    fn reported_total_and_independent_pool_percentages_keep_percent_units() {
        let values = parse(
            json!({"billingCycleEnd":"2030-02-01T00:00:00Z", "individualUsage":{"plan":{
                "used":500,"limit":1000,"remaining":500,"totalPercentUsed":0.36,"autoPercentUsed":32,"apiPercentUsed":75
            }}}),
        );
        assert_eq!(values.len(), 3);
        assert_eq!(values[0].used_basis_points, Some(36));
        assert_eq!(values[0].remaining_basis_points(), Some(9964));
        assert_eq!(values[1].used_basis_points, Some(3200));
        assert_eq!(values[2].used_basis_points, Some(7500));
        assert!(values[0].general && !values[1].general && !values[2].general);
        assert!(
            values
                .iter()
                .all(|v| v.resets_unix_ms == Some(1_896_134_400_000))
        );
    }
    #[test]
    fn empty_team_placeholder_preserves_individual_percentage() {
        let values = parse(json!({
            "teamUsage": {},
            "billingCycleEnd": "2030-02-01T00:00:00Z",
            "individualUsage": {
                "plan": {"enabled":true,"used":2000,"limit":2000,"remaining":0,"totalPercentUsed":37.125},
                "onDemand": {"enabled":false,"used":0,"limit":null,"remaining":null}
            }
        }));
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].used_basis_points, Some(3712));
        assert_eq!(values[0].remaining_basis_points(), Some(6288));
        assert_eq!(values[0].resets_unix_ms, Some(1_896_134_400_000));
        for team in [json!({"plan":{}}), json!({"used":0}), json!([])] {
            assert_eq!(
                windows(
                    &json!({"teamUsage":team,"individualUsage":{"plan":{"totalPercentUsed":20}}})
                        .to_string()
                )
                .err()
                .unwrap()
                .state,
                QuotaState::Unsupported
            );
        }
    }
    #[test]
    fn explicit_cents_cap_is_used_without_treating_breakdown_total_as_limit() {
        let values = parse(
            json!({"individualUsage":{"plan":{"used":700,"limit":2000,"breakdown":{"total":700}},"onDemand":{"used":250,"limit":1000,"remaining":750}}}),
        );
        assert_eq!(values[0].remaining_basis_points(), Some(6500));
        assert_eq!(values[1].remaining_basis_points(), Some(7500));
        assert!(!values[1].general);
        assert!(values.iter().all(|v| v.resets_unix_ms.is_none()));
        let unknown = parse(
            json!({"billingCycleEnd":"2030-02-01T00:00:00Z","individualUsage":{"plan":{"used":700,"breakdown":{"total":700}},"onDemand":{"used":250,"limit":null}}}),
        );
        assert!(unknown.iter().all(|v| v.used_basis_points.is_none()));
    }
    #[test]
    fn missing_fields_are_unknown_and_passed_resets_never_refill_allowance() {
        let values = parse(
            json!({"billingCycleEnd":"2020-01-01T00:00:00Z","individualUsage":{"plan":{"limit":1000}}}),
        );
        assert_eq!(values[0].used_basis_points, None);
        assert_eq!(values[0].remaining_basis_points(), None);
        assert_eq!(values[0].resets_unix_ms, Some(1_577_836_800_000));
        let values = parse(json!({"individualUsage":{"plan":{"totalPercentUsed":110}}}));
        assert!(values[0].over_limit);
        assert_eq!(values[0].remaining_basis_points(), None);
        for value in [
            json!({}),
            json!({"individualUsage":{}}),
            json!({"individualUsage":{"plan":{"used":10,"limit":0}}}),
            json!({"individualUsage":{"plan":{"enabled":false}}}),
        ] {
            assert_eq!(
                windows(&value.to_string()).err().unwrap().state,
                QuotaState::Unsupported
            );
        }
    }
    #[test]
    fn malformed_usage_reset_and_conflicting_amounts_fail_closed() {
        for value in [
            json!([]),
            json!({"billingCycleEnd":"tomorrow","individualUsage":{}}),
            json!({"billingCycleStart":"2030-03-01T00:00:00Z","billingCycleEnd":"2030-02-01T00:00:00Z","individualUsage":{}}),
            json!({"individualUsage":{"plan":{"totalPercentUsed":-1}}}),
            json!({"individualUsage":{"plan":{"used":"42"}}}),
            json!({"individualUsage":{"plan":{"used":-1}}}),
            json!({"individualUsage":{"plan":[]}}),
        ] {
            assert_eq!(
                windows(&value.to_string()).err().unwrap().state,
                QuotaState::Invalid,
                "{value}"
            );
        }
        assert_eq!(
            windows("not JSON").err().unwrap().state,
            QuotaState::Invalid
        );
        assert_eq!(
            windows(
                &json!({"individualUsage":{"plan":{"used":2,"limit":10,"remaining":9}}})
                    .to_string()
            )
            .err()
            .unwrap()
            .state,
            QuotaState::SourceConflict
        );
    }
    #[test]
    fn jwt_cookie_is_bounded_and_never_refreshes_expired_or_missing_auth() {
        let valid = token("fixture-user", 400);
        let result = session(valid.clone(), 300_000).ok().unwrap();
        assert_eq!(
            result.cookie(),
            format!("WorkosCursorSessionToken=fixture-user%3A%3A{valid}")
        );
        assert_eq!(
            session(valid, 340_000).err().unwrap().state,
            QuotaState::NotSignedIn
        );
        assert_eq!(
            session("".into(), 0).err().unwrap().state,
            QuotaState::NotSignedIn
        );
        for raw in [
            "malformed".into(),
            token("evil; cookie=header", 400),
            "x".repeat(8193),
        ] {
            assert_eq!(
                session(raw, 0).err().unwrap().state,
                QuotaState::Unavailable
            );
        }
    }
    #[test]
    fn sqlite_text_and_utf16_blob_reads_leave_database_and_sidecars_unchanged() {
        let root = tempfile::tempdir().unwrap();
        for utf16 in [false, true] {
            let path = root.path().join(format!("state #{utf16}.vscdb"));
            let valid = token("fixture-user", 400);
            let bytes = if utf16 {
                valid.encode_utf16().flat_map(u16::to_le_bytes).collect()
            } else {
                valid.as_bytes().to_vec()
            };
            database(&path, &bytes);
            let before = std::fs::read(&path).unwrap();
            assert_eq!(
                load_session(&path, 300_000).ok().unwrap().user,
                "fixture-user"
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(
                std::fs::read_dir(root.path()).unwrap().count(),
                if utf16 { 2 } else { 1 }
            );
        }
        assert_eq!(
            load_session(&root.path().join("missing"), 0)
                .err()
                .unwrap()
                .state,
            QuotaState::NotSignedIn
        );
    }
    #[test]
    fn sqlite_live_wal_sees_uncheckpointed_login_and_idle_wal_creates_no_sidecars() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.vscdb");
        database(&path, token("before", 400).as_bytes());
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0")
            .unwrap();
        writer
            .execute("UPDATE ItemTable SET value=?1", [token("after", 400)])
            .unwrap();
        assert_eq!(load_session(&path, 300_000).ok().unwrap().user, "after");
        drop(writer);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        let before = std::fs::read(&path).unwrap();
        assert_eq!(load_session(&path, 300_000).ok().unwrap().user, "after");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn sqlite_symlink_uses_real_active_wal_instead_of_checkpointed_account() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state.vscdb");
        let alias = root.path().join("alias.vscdb");
        database(&path, token("checkpointed-old-user", 400).as_bytes());
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0")
            .unwrap();
        writer
            .execute(
                "UPDATE ItemTable SET value=?1",
                [token("current-user", 400)],
            )
            .unwrap();
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert_eq!(
            load_session(&alias, 300_000).ok().unwrap().user,
            "current-user"
        );
        assert!(!alias.with_extension("vscdb-wal").exists());
        assert!(!alias.with_extension("vscdb-shm").exists());
    }

    #[test]
    fn late_config_responses_and_disabled_consent_are_rejected() {
        let state = super::super::tests::test_state(None, None);
        let first = CursorQuotaSettings {
            dashboard: true,
            state_db: Some("/first".into()),
        };
        let second = CursorQuotaSettings {
            state_db: Some("/second".into()),
            ..first.clone()
        };
        state.settings.lock().unwrap().report.settings.quota.cursor = second;
        apply(
            &state,
            update(
                &first,
                2,
                Some(vec![window("plan", "plan", true, Some(20.0), None)]),
                QuotaState::Current,
                true,
                None,
                None,
            ),
        );
        assert!(state.quotas.lock().unwrap().cursor.windows.is_empty());
        state.settings.lock().unwrap().report.settings.quota.cursor = first.clone();
        apply(
            &state,
            update(
                &first,
                2,
                Some(vec![window("plan", "plan", true, Some(20.0), None)]),
                QuotaState::Current,
                true,
                None,
                None,
            ),
        );
        assert_eq!(
            state.quotas.lock().unwrap().cursor.windows[0].remaining_basis_points(),
            Some(8000)
        );
        state.settings.lock().unwrap().report.settings.quota.enabled = false;
        quota::sync_consent(&state);
        apply(
            &state,
            update(
                &first,
                3,
                Some(vec![window("plan", "plan", true, Some(40.0), None)]),
                QuotaState::Current,
                true,
                None,
                None,
            ),
        );
        assert!(state.quotas.lock().unwrap().cursor.windows.is_empty());
        assert_eq!(
            state.quotas.lock().unwrap().cursor.state,
            QuotaState::Disabled
        );
    }
}

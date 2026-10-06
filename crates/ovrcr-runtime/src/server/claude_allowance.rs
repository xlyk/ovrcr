//! Claude allowance precedence. Workers deliver signals; this module decides the row.
use ovrcr_protocol::{
    CLAUDE_WAITING, ProviderQuota, QuotaProvider, QuotaReport, QuotaSnapshot, QuotaSource,
    QuotaState,
};

/// How often a probe reading schedules the next check.
pub(super) const PROBE_EVERY_MS: u64 = 30 * 60 * 1000;

const NOT_REPORTING: &str = "managed Claude session not reporting";

/// The Claude session the gatherer chose, if any.
pub(super) enum ChosenSession {
    /// The session already has a quota snapshot.
    Reported(ProviderQuota),
    /// A Claude agent with no snapshot yet.
    Waiting {
        source: QuotaSource,
        live_connected: bool,
    },
    /// The selected session is not a Claude reporter.
    Other,
}

/// A probe result that has not yet been folded into the row.
#[derive(Clone)]
pub(super) struct ProbeSignal {
    pub(super) report: QuotaReport,
    pub(super) reason: Option<String>,
    pub(super) probed_unix_ms: u64,
}

/// The next Claude row.
///
/// `probe` is the latest stored probe signal. The caller passes `None` when a
/// fresh managed session already makes that signal inadmissible.
pub(super) fn merge(
    previous: &ProviderQuota,
    session: Option<&ChosenSession>,
    probe: Option<&ProbeSignal>,
    auth: Option<&(QuotaState, String)>,
    account: Option<&ProviderQuota>,
    probe_enabled: bool,
    now: u64,
) -> ProviderQuota {
    let mut next = match session {
        Some(ChosenSession::Reported(quota)) => quota.clone(),
        Some(ChosenSession::Waiting {
            source,
            live_connected,
        }) => {
            let (state, reason) = if *live_connected {
                (QuotaState::Checking, CLAUDE_WAITING)
            } else {
                (QuotaState::Unavailable, NOT_REPORTING)
            };
            let mut quota = if previous.source.as_ref() == Some(source) {
                previous.clone()
            } else {
                let mut quota = ProviderQuota::unknown(QuotaProvider::Claude, state);
                quota.source = Some(source.clone());
                quota
            };
            quota.state = state;
            quota.reason = Some(reason.into());
            quota
        }
        Some(ChosenSession::Other) => {
            let mut quota = previous.clone();
            quota.state = QuotaState::Unavailable;
            quota.reason = Some(NOT_REPORTING.into());
            quota
        }
        None if previous.source.is_some() => {
            let mut quota = previous.clone();
            quota.state = QuotaState::Unavailable;
            quota.reason = Some(NOT_REPORTING.into());
            quota
        }
        None => QuotaSnapshot::default().claude,
    };
    // A hidden probe is not a Session. Turning it off returns the row to
    // the auth/waiting state instead of "not reporting".
    if matches!(previous.source, Some(QuotaSource::Probe { .. })) && !probe_enabled {
        next = QuotaSnapshot::default().claude;
    }
    let mut prior = previous.clone();
    if let Some(update) = probe {
        install_probe(&mut prior, update, probe_enabled, now);
    }
    let fresh_session = matches!(next.source, Some(QuotaSource::Session { .. }))
        && next.state == QuotaState::Current
        && !next.stale(now);
    // Until a managed session reports, `claude auth status` may rule the
    // allowance out: not signed in, not a claude.ai login, or no answer.
    // A probe reading stays until a fresh managed report or that ruling.
    if matches!(prior.source, Some(QuotaSource::Probe { .. })) && probe_enabled && !fresh_session {
        if let Some((state, reason)) = auth {
            next = ProviderQuota::unknown(QuotaProvider::Claude, *state);
            next.reason = Some(reason.clone());
        } else {
            next = prior;
        }
    } else if next.state == QuotaState::Checking
        && let Some((state, reason)) = auth
    {
        next.state = *state;
        next.reason = Some(reason.clone());
    }
    // The account read is the credentials-file usage call. A status-line
    // sample with windows stays as the in-session extra when the account
    // read has not produced windows. Waiting for a session does not hide
    // an account reading, and does not invent a bar.
    if let Some(account) = account {
        let session_windows = next.state == QuotaState::Current && !next.windows.is_empty();
        let account_windows = account.state == QuotaState::Current && !account.windows.is_empty();
        if account_windows || !session_windows && next.state == QuotaState::Checking {
            next = account.clone();
        }
    }
    next
}

fn install_probe(prior: &mut ProviderQuota, update: &ProbeSignal, probe_enabled: bool, now: u64) {
    let invalid_current =
        update.report.validate().is_err() && update.report.state == QuotaState::Current;
    let fresh_session_row =
        matches!(prior.source, Some(QuotaSource::Session { .. })) && !prior.stale(now);
    if !probe_enabled || invalid_current || fresh_session_row {
        return;
    }
    prior.source = Some(QuotaSource::Probe {
        probed_unix_ms: update.probed_unix_ms,
    });
    prior.provider = QuotaProvider::Claude;
    prior.state = update.report.state;
    prior.reason = update.reason.clone();
    prior.checked_unix_ms = None;
    prior.next_check_unix_ms = Some(update.probed_unix_ms.saturating_add(PROBE_EVERY_MS));
    if let Some(windows) = &update.report.windows {
        prior.windows = windows.clone();
        prior.observed_unix_ms = Some(update.probed_unix_ms);
    } else {
        prior.windows.clear();
        prior.observed_unix_ms = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ovrcr_protocol::{AgentBinding, AgentProvider, QuotaWindow, SessionId, SessionRunId};

    fn window() -> QuotaWindow {
        QuotaWindow {
            id: "5h".into(),
            label: "5h".into(),
            general: true,
            used_basis_points: Some(2_500),
            over_limit: false,
            resets_unix_ms: None,
        }
    }

    fn current(source: QuotaSource, now: u64) -> ProviderQuota {
        ProviderQuota {
            provider: QuotaProvider::Claude,
            source: Some(source),
            windows: vec![window()],
            observed_unix_ms: Some(now),
            checked_unix_ms: None,
            state: QuotaState::Current,
            reason: None,
            next_check_unix_ms: None,
        }
    }

    fn session_source() -> QuotaSource {
        QuotaSource::Session {
            session: SessionId(7),
            run: SessionRunId(1),
            binding: AgentBinding {
                provider: AgentProvider::Claude,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
        }
    }

    fn probe_signal(now: u64) -> ProbeSignal {
        ProbeSignal {
            report: QuotaReport {
                windows: Some(vec![window()]),
                state: QuotaState::Current,
            },
            reason: None,
            probed_unix_ms: now,
        }
    }

    #[test]
    fn a_fresh_session_beats_a_probe_update() {
        let now = 1_700_000_000_000;
        let session = current(session_source(), now);
        let next = merge(
            &QuotaSnapshot::default().claude,
            Some(&ChosenSession::Reported(session.clone())),
            Some(&probe_signal(now)),
            None,
            None,
            true,
            now,
        );
        assert_eq!(next, session);
    }

    #[test]
    fn a_probe_update_fills_an_empty_row() {
        let now = 1_700_000_000_000;
        let next = merge(
            &QuotaSnapshot::default().claude,
            None,
            Some(&probe_signal(now)),
            None,
            None,
            true,
            now,
        );
        assert_eq!(
            next.source,
            Some(QuotaSource::Probe {
                probed_unix_ms: now
            })
        );
        assert_eq!(next.state, QuotaState::Current);
        assert_eq!(next.windows, vec![window()]);
        assert_eq!(
            next.next_check_unix_ms,
            Some(now.saturating_add(PROBE_EVERY_MS))
        );
    }

    #[test]
    fn an_auth_ruling_replaces_a_probe_row() {
        let now = 1_700_000_000_000;
        let previous = current(
            QuotaSource::Probe {
                probed_unix_ms: now,
            },
            now,
        );
        let next = merge(
            &previous,
            None,
            None,
            Some(&(QuotaState::NotSignedIn, "not signed in".into())),
            None,
            true,
            now,
        );
        assert_eq!(next.state, QuotaState::NotSignedIn);
        assert_eq!(next.reason.as_deref(), Some("not signed in"));
        assert!(next.windows.is_empty());
    }

    #[test]
    fn an_account_row_with_windows_replaces_a_checking_session() {
        let now = 1_700_000_000_000;
        let waiting = ChosenSession::Waiting {
            source: session_source(),
            live_connected: true,
        };
        let mut account = ProviderQuota::unknown(QuotaProvider::Claude, QuotaState::Current);
        account.windows = vec![window()];
        account.observed_unix_ms = Some(now);
        account.reason = Some("account".into());
        let next = merge(
            &QuotaSnapshot::default().claude,
            Some(&waiting),
            None,
            None,
            Some(&account),
            false,
            now,
        );
        assert_eq!(next.reason.as_deref(), Some("account"));
        assert_eq!(next.windows, vec![window()]);
    }

    #[test]
    fn session_windows_stay_when_the_account_read_has_none() {
        let now = 1_700_000_000_000;
        let session = current(session_source(), now);
        let account = ProviderQuota::unknown(QuotaProvider::Claude, QuotaState::Checking);
        let next = merge(
            &QuotaSnapshot::default().claude,
            Some(&ChosenSession::Reported(session.clone())),
            None,
            None,
            Some(&account),
            true,
            now,
        );
        assert_eq!(next, session);
    }

    #[test]
    fn turning_the_probe_off_clears_a_probe_row() {
        let now = 1_700_000_000_000;
        let previous = current(
            QuotaSource::Probe {
                probed_unix_ms: now,
            },
            now,
        );
        let next = merge(&previous, None, None, None, None, false, now);
        assert_eq!(next, QuotaSnapshot::default().claude);
    }
}

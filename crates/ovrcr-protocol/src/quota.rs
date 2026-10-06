//! Account allowance is separate from conversation tokens, cost, and context.
use crate::{AgentBinding, SessionId, SessionRunId};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuotaProvider {
    Claude,
    Codex,
    Grok,
    Cursor,
}

impl QuotaProvider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Grok => "Grok",
            Self::Cursor => "Cursor",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuotaState {
    Waiting,
    Current,
    Unavailable,
    NotSignedIn,
    Unsupported,
    Invalid,
    SourceConflict,
    /// The consent setting `quota.enabled` is off.
    Disabled,
    /// Enabled, and the first read for this source is in flight.
    Checking,
}

impl QuotaState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Waiting => "waiting for report",
            Self::Current => "not reported",
            Self::Unavailable => "unavailable",
            Self::NotSignedIn => "not signed in",
            Self::Unsupported => "unsupported auth/version",
            Self::Invalid => "invalid report",
            Self::SourceConflict => "source conflict",
            Self::Disabled => "off",
            Self::Checking => "checking",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaWindow {
    /// Actual provider window/bucket identity, not a guessed duration.
    pub id: String,
    pub label: String,
    /// Only general allowance is eligible for the compact sidebar; other buckets remain in details.
    pub general: bool,
    /// Consumed percentage rounded down to two decimal places; None is unknown.
    pub used_basis_points: Option<u16>,
    pub over_limit: bool,
    pub resets_unix_ms: Option<u64>,
}

impl QuotaWindow {
    pub fn remaining_basis_points(&self) -> Option<u16> {
        self.used_basis_points.map(|used| 10_000 - used.min(10_000))
    }

    pub fn validate(&self) -> Result<()> {
        for (text, max) in [(&self.id, 128), (&self.label, 16)] {
            if text.is_empty() || text.len() > max || text.chars().any(char::is_control) {
                bail!("invalid quota window identity");
            }
        }
        if self.used_basis_points.is_some_and(|used| used > 10_000)
            || (self.over_limit && self.used_basis_points.is_some())
        {
            bail!("invalid quota percentage");
        }
        Ok(())
    }
}

/// No native response body, account email, or credential belongs on this wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuotaSource {
    Session {
        session: SessionId,
        run: SessionRunId,
        binding: AgentBinding,
    },
    NativeProfile {
        profile: String,
        generation: u64,
    },
    /// A hidden quota probe, not a Session. `probed_unix_ms` is when it ran.
    Probe {
        probed_unix_ms: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaReport {
    /// None retains the last good windows while changing availability.
    pub windows: Option<Vec<QuotaWindow>>,
    pub state: QuotaState,
}

impl QuotaReport {
    pub fn validate(&self) -> Result<()> {
        if let Some(windows) = &self.windows {
            if windows.len() > 32 || self.state != QuotaState::Current {
                bail!("invalid quota observation");
            }
            for (i, window) in windows.iter().enumerate() {
                window.validate()?;
                if windows[..i].iter().any(|old| old.id == window.id) {
                    bail!("duplicate quota window");
                }
            }
        } else if self.state == QuotaState::Current {
            bail!("current quota requires a window observation");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderQuota {
    pub provider: QuotaProvider,
    pub source: Option<QuotaSource>,
    pub windows: Vec<QuotaWindow>,
    pub observed_unix_ms: Option<u64>,
    /// Only an authoritative native account read advances this, not callback receipt.
    pub checked_unix_ms: Option<u64>,
    pub state: QuotaState,
    /// OVRCR's own one-line classification of why the row is not current
    /// (a Quota reason); never a native body, account value or credential.
    pub reason: Option<String>,
    /// When the Server will next ask the native provider (a Next check).
    pub next_check_unix_ms: Option<u64>,
}

/// The off-state sentence of the `quota.enabled` consent setting.
pub const QUOTA_OFF: &str = "Codex/Grok usage off: set `quota.enabled = true` in dashboard.toml";

/// Cursor personal dashboard collection requires its own opt-in.
pub const CURSOR_QUOTA_OFF: &str =
    "Cursor usage off: set `quota.cursor.dashboard = true` in dashboard.toml";

/// Opt-in description for the personal dashboard adapter.
pub const CURSOR_QUOTA_DESCRIPTION: &str = "Reads the signed-in Cursor desktop account from its local SQLite state and queries Cursor’s internal dashboard usage API. Experimental personal-account support; no token refresh or credential writes. Requires quota.enabled.";

/// Claude's fallback while no native usage or managed response is available.
pub const CLAUDE_WAITING: &str = "waiting for a managed Claude session's first response";

/// Consent text for `quota.claude.probe`. The setting stays off until this is accepted.
pub const CLAUDE_PROBE_DESCRIPTION: &str = "Starts a hidden Claude Code run with a one-word prompt and reads its status line when no managed Claude session has reported recently; each probe spends a small amount of your allowance and leaves a conversation in Claude's history; OVRCR trusts its own empty probe directory for this.";

/// The probe process produced no status line before the deadline.
pub const PROBE_TIMED_OUT: &str = "probe timed out";

/// The probe process ended without a status-line callback.
pub const PROBE_EXITED: &str = "probe exited before reporting";

/// The probe ended or reached its deadline without usable subscription usage.
pub const PROBE_MISSING_USAGE: &str = "claude did not report subscription usage";

impl ProviderQuota {
    pub fn unknown(provider: QuotaProvider, state: QuotaState) -> Self {
        Self {
            provider,
            source: None,
            windows: Vec::new(),
            observed_unix_ms: None,
            checked_unix_ms: None,
            state,
            reason: None,
            next_check_unix_ms: None,
        }
    }

    pub fn stale(&self, now: u64) -> bool {
        self.state != QuotaState::Current
            || self
                .checked_unix_ms
                .max(self.observed_unix_ms)
                .is_none_or(|stamp| crate::freshness::is_stale(stamp, now, false))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaSnapshot {
    pub claude: ProviderQuota,
    pub codex: ProviderQuota,
    pub grok: ProviderQuota,
    pub cursor: ProviderQuota,
}

/// The Server's snapshot with `quota.enabled` off and nothing reported. The
/// Server sends no snapshot after hello while it still equals this.
impl Default for QuotaSnapshot {
    fn default() -> Self {
        let off = |provider| ProviderQuota {
            reason: Some(QUOTA_OFF.into()),
            ..ProviderQuota::unknown(provider, QuotaState::Disabled)
        };
        Self {
            claude: ProviderQuota {
                reason: Some(CLAUDE_WAITING.into()),
                ..ProviderQuota::unknown(QuotaProvider::Claude, QuotaState::Checking)
            },
            codex: off(QuotaProvider::Codex),
            grok: off(QuotaProvider::Grok),
            cursor: ProviderQuota {
                reason: Some(CURSOR_QUOTA_OFF.into()),
                ..ProviderQuota::unknown(QuotaProvider::Cursor, QuotaState::Disabled)
            },
        }
    }
}

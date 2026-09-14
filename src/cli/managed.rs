//! Setup and doctor for the providers whose managed launch needs no settings: Pi and
//! Oh My Pi load their owned reporting extension from the launch itself. Doctor reports
//! evidence (tested versions, what the live session has actually delivered), never an
//! allowlist.
use super::{AppResult, RuntimeError};
use ovrcr::protocol::{AgentActivity, ReporterHealth};
use serde_json::{Value, json};
use std::ffi::OsStr;

pub(super) struct Managed {
    pub name: &'static str,
    pub display: &'static str,
    /// Releases exercised by this repository's recorded evidence. Not an allowlist.
    pub tested_versions: &'static [&'static str],
    pub reporting: &'static str,
    /// Native tool-approval prompts as Input requests.
    pub approvals: &'static str,
    /// Agent questions as Input requests. `available_tool_lifetime` means the request
    /// spans the asking tool's execution, not the dialog's visibility: it opens a moment
    /// before the dialog and covers a question queued behind another one.
    pub questions: &'static str,
    pub recovery: &'static str,
}

pub(super) const PI: Managed = Managed {
    name: "pi",
    display: "Pi",
    tested_versions: &["0.85.1"],
    reporting: "available",
    approvals: "available",
    questions: "unavailable_no_provider_surface",
    recovery: "available",
};

pub(super) const OMP: Managed = Managed {
    name: "omp",
    display: "Oh My Pi",
    tested_versions: &["18.1.19"],
    reporting: "available",
    approvals: "available",
    questions: "available_tool_lifetime",
    recovery: "pending #96",
};

/// The reasons a receiver pauses *and this provider can come back from*: live reporting is
/// uncertain, but the lease, the binding and the native session are all intact, so a source
/// boundary or the extension's own reattach command recovers it. An overflow is the
/// exception — it disables the producer inside the extension, so no boundary can arrive on
/// its own and only a provider with a reattach control recovers. Anything else Unavailable
/// is a reporter that is gone, and only a fresh managed launch brings it back.
fn paused(provider: &Managed, agent: &ovrcr::protocol::AgentSnapshot) -> bool {
    agent.health.state == ReporterHealth::Unavailable
        && agent
            .health
            .reason
            .as_deref()
            .is_some_and(|reason| match reason {
                "source_gap" | "producer_replaced" | "transition_mismatch" => true,
                "source_overflow" => provider.recovery == "available",
                _ => false,
            })
}

pub(super) fn managed(name: &str) -> Option<&'static Managed> {
    match name {
        "pi" => Some(&PI),
        "omp" => Some(&OMP),
        _ => None,
    }
}

pub(super) fn setup(provider: &Managed) -> AppResult<()> {
    println!(
        "{} reporting needs no settings changes. Launch through `ovrcr agent run {name} -- {name} [ARGS...]` inside an OVRCR terminal, or pick {name} in the Dashboard agent picker. A plain `{name}` launch stays untracked.",
        provider.display,
        name = provider.name
    );
    Ok(())
}

/// The version as the harness prints it: Pi prints `0.85.1`, Oh My Pi prints `omp/18.1.19`.
/// Anything that is not a short version token is treated as no version at all.
fn version_of(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let first = text.lines().next()?.trim();
    let version = first.rsplit('/').next().unwrap_or(first).trim();
    let plausible = !version.is_empty()
        && version.len() <= 64
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        && version.chars().next().is_some_and(|c| c.is_ascii_digit());
    plausible.then(|| version.to_owned())
}

pub(super) fn doctor(
    provider: &Managed,
    session: Option<u64>,
    executable: &OsStr,
) -> AppResult<()> {
    let raw = ovrcr::report::admission::probe_version(executable);
    let version = raw.as_deref().and_then(version_of);
    let probe_status = if raw.is_some() {
        "probed"
    } else {
        "unavailable"
    };
    let version_status = match &version {
        None => "unknown",
        Some(found) if provider.tested_versions.contains(&found.as_str()) => "tested",
        Some(_) => "unverified_compatible_until_proven_otherwise",
    };
    let mut remediation = Vec::<String>::new();
    if raw.is_some() && version.is_none() {
        remediation.push(format!(
            "{} printed no recognizable version for --version; reporting stays enabled, but the release cannot be recorded as evidence.",
            executable.to_string_lossy()
        ));
    }
    if raw.is_none() {
        remediation.push(format!(
            "The bounded --version probe of {} failed; check the executable path, then rerun doctor.",
            executable.to_string_lossy()
        ));
    }
    let requested = session.or_else(|| std::env::var("OVRCR_SESSION_ID").ok()?.parse().ok());
    let mut binding = Value::Null;
    let mut lifecycle = Value::Null;
    let session_status = match requested {
        None => "not_requested",
        Some(id) => match super::inspect() {
            Err(_) => {
                remediation.push("Session inspection was unavailable; check the configured OVRCR socket and server, then rerun doctor.".into());
                "inspection_unavailable"
            }
            Ok((_, sessions)) => match sessions.iter().find(|session| session.id.0 == id) {
                None => {
                    remediation.push("The requested session is not live; select a current session ID before checking its binding.".into());
                    "session_not_found"
                }
                Some(session) => match &session.agent {
                    None => {
                        remediation.push(format!(
                            "Start {} through `ovrcr agent run {name} -- {name} [ARGS...]` or the Dashboard picker; a plain launch stays untracked.",
                            provider.display,
                            name = provider.name
                        ));
                        "unbound"
                    }
                    Some(agent) => {
                        binding = json!({"provider": agent.binding.provider, "generation": agent.binding.generation});
                        let activity = agent.activity.as_ref().map(|sample| sample.state);
                        lifecycle = json!({
                            // A binding exists only after the extension delivered session_start.
                            "extension": "loaded_and_bound",
                            "delivery": match (paused(provider, agent), agent.health.state, activity.is_some()) {
                                (true, _, _) => "paused_recoverable",
                                (false, ReporterHealth::Unavailable, _) => "lost",
                                (false, _, true) => "observed",
                                (false, _, false) => "bound_without_activity",
                            },
                            // The same rule `terminal list` reports: a wait covers the sample.
                            "activity": agent.effective_activity(),
                            "input_requests": agent.input_requests.iter().map(|request| request.kind).collect::<Vec<_>>(),
                            "work_seen": matches!(activity, Some(AgentActivity::Busy | AgentActivity::ResponseReady | AgentActivity::Error | AgentActivity::WaitingInput)),
                            "health": agent.health.state,
                            "unread": session.unread.is_some(),
                        });
                        if paused(provider, agent) {
                            // Only name the reattach command where the provider has one.
                            let control = if provider.recovery == "available" {
                                format!(
                                    "Run /ovrcr-reattach in {}, or send the next prompt",
                                    provider.display
                                )
                            } else {
                                "Send the next prompt".to_owned()
                            };
                            remediation.push(format!(
                                "Reporting is paused for this invocation ({}): the source data this reporter delivered is no longer certain, and nothing is being applied. The native session is untouched. {control} — the next session boundary recovers reporting as a fresh generation.",
                                agent.health.reason.as_deref().unwrap_or("unknown"),
                            ));
                        } else if agent.health.state == ReporterHealth::Unavailable {
                            remediation.push("Reporting is unavailable for this invocation: the transport was lost or the producer ended with its session. Start a fresh managed launch to restore reporting.".into());
                        }
                        "bound"
                    }
                },
            },
        },
    };
    let report = json!({
        "provider": provider.name,
        "executable": executable.to_string_lossy(),
        "probe_status": probe_status,
        "version": version,
        "version_status": version_status,
        "tested_versions": provider.tested_versions,
        "supported_versions": "any compatible release; tested versions are evidence, not an allowlist",
        "known_incompatibilities": [],
        "capabilities": {
            "managed_launch": true,
            "reporting": provider.reporting,
            "approvals": provider.approvals,
            "questions": provider.questions,
            "recovery": provider.recovery,
            "metrics": "absent",
        },
        "session_status": session_status,
        "binding": binding,
        "lifecycle": lifecycle,
        "remediation": remediation,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(RuntimeError::internal)?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{OMP, PI, paused, version_of};

    #[test]
    fn a_pause_is_recoverable_only_where_the_provider_has_a_way_out_of_it() {
        use ovrcr::protocol::{
            AgentBinding, AgentProvider, AgentSnapshot, HealthSample, ReporterHealth,
        };
        let snapshot = |reason: Option<&str>| AgentSnapshot {
            binding: AgentBinding {
                provider: AgentProvider::Pi,
                invocation: "inv".into(),
                conversation: "conv".into(),
                generation: 1,
            },
            activity: None,
            metrics: None,
            health: HealthSample {
                state: ReporterHealth::Unavailable,
                reason: reason.map(str::to_owned),
            },
            activity_revision: 0,
            metrics_revision: 0,
            health_revision: 0,
            input_requests: Vec::new(),
            input_revision: 0,
        };
        for reason in ["source_gap", "producer_replaced", "transition_mismatch"] {
            // A boundary the harness reaches on its own ends these, for either provider.
            assert!(paused(&PI, &snapshot(Some(reason))), "{reason}");
            assert!(paused(&OMP, &snapshot(Some(reason))), "{reason}");
        }
        // An overflow disables the producer inside the extension, so nothing the harness
        // does can end it: only a provider with a reattach control can recover from one.
        assert!(paused(&PI, &snapshot(Some("source_overflow"))));
        assert!(
            !paused(&OMP, &snapshot(Some("source_overflow"))),
            "Oh My Pi has no reattach command: an overflow is a lost reporter, not a pause"
        );
        assert!(!paused(&PI, &snapshot(Some("collector_unavailable"))));
        assert!(!paused(&PI, &snapshot(None)));
        let mut connected = snapshot(None);
        connected.health.state = ReporterHealth::Connected;
        connected.health.reason = Some("source_gap".into());
        assert!(!paused(&PI, &connected), "health decides first");
    }

    #[test]
    fn version_of_accepts_harness_formats_and_rejects_everything_else() {
        assert_eq!(version_of(b"0.85.1\n").as_deref(), Some("0.85.1"));
        assert_eq!(version_of(b"omp/18.1.19\n").as_deref(), Some("18.1.19"));
        assert_eq!(
            version_of(b"1.2.3-beta+build\nsecond line\n").as_deref(),
            Some("1.2.3-beta+build")
        );
        for garbage in [
            &b""[..],
            b"\n",
            b"/opt/homebrew/bin/pi\n",
            b"pi version unknown\n",
            b"v0.85.1\n",
            b"0.85.1; rm -rf /\n",
        ] {
            assert_eq!(version_of(garbage), None, "{garbage:?}");
        }
        let long = format!("{}\n", "9".repeat(65));
        assert_eq!(version_of(long.as_bytes()), None);
    }
}

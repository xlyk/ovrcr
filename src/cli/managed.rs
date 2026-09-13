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
    pub input_requests: &'static str,
    pub recovery: &'static str,
}

pub(super) const PI: Managed = Managed {
    name: "pi",
    display: "Pi",
    tested_versions: &["0.85.1"],
    reporting: "available",
    input_requests: "pending #90",
    recovery: "pending #91",
};

pub(super) const OMP: Managed = Managed {
    name: "omp",
    display: "Oh My Pi",
    tested_versions: &["18.1.19"],
    reporting: "available",
    input_requests: "pending #95",
    recovery: "pending #96",
};

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
    let version =
        ovrcr::report::admission::probe_version(executable).and_then(|raw| version_of(&raw));
    let (probe_status, version_status) = match &version {
        None => ("unavailable", "unknown"),
        Some(found) if provider.tested_versions.contains(&found.as_str()) => ("probed", "tested"),
        Some(_) => ("probed", "unverified_compatible_until_proven_otherwise"),
    };
    let mut remediation = Vec::<String>::new();
    if version.is_none() {
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
                            "delivery": if activity.is_some() { "observed" } else { "bound_without_activity" },
                            "activity": activity,
                            "work_seen": matches!(activity, Some(AgentActivity::Busy | AgentActivity::ResponseReady | AgentActivity::Error | AgentActivity::WaitingInput)),
                            "health": agent.health.state,
                            "unread": session.unread.is_some(),
                        });
                        if agent.health.state == ReporterHealth::Unavailable {
                            remediation.push("Reporting is unavailable for this invocation: the transport was lost or the producer retired. Reporter recovery arrives with a later ticket; start a fresh managed launch to restore reporting.".into());
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
            "input_requests": provider.input_requests,
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

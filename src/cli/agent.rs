use super::{AppResult, RuntimeError, args::AgentCommand};
use ovrcr::protocol::AgentProvider;
use std::os::unix::process::ExitStatusExt;

fn provider(name: &str) -> AgentProvider {
    match name {
        "codex" => AgentProvider::Codex,
        "pi" => AgentProvider::Pi,
        "omp" => AgentProvider::Omp,
        _ => AgentProvider::Claude,
    }
}

fn display_name(name: &str) -> &'static str {
    match name {
        "codex" => "Codex",
        "pi" => "Pi",
        "omp" => "Oh My Pi",
        _ => "Claude",
    }
}

/// Pi and Oh My Pi need no settings: the managed launch loads the owned extension itself.
fn managed_setup(name: &str) -> AppResult<()> {
    println!(
        "{} reporting needs no settings changes. Launch through `ovrcr agent run {name} -- {name} [ARGS...]` inside an OVRCR terminal, or pick {name} in the Dashboard agent picker. A plain `{name}` launch stays untracked.",
        display_name(name)
    );
    Ok(())
}

fn managed_doctor(name: &str, executable: &std::ffi::OsStr) -> AppResult<()> {
    let version = ovrcr::report::admission::probe_version(executable)
        .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned());
    let report = serde_json::json!({
        "provider": name,
        "probe_status": if version.is_some() { "probed" } else { "unavailable" },
        "version": version,
        "supported_versions": "any compatible release; tested versions are evidence, not an allowlist",
        "capabilities": { "managed_launch": true, "reporting": if name == "pi" { "pending #89" } else { "pending #94" } },
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("a JSON value serializes")
    );
    Ok(())
}

pub(super) fn run(command: AgentCommand) -> AppResult<()> {
    let (name, argv) = match command {
        AgentCommand::Setup {
            provider,
            print: _,
            settings,
        } => {
            return match provider.as_str() {
                "codex" => super::codex_setup::setup(settings.as_deref()),
                "pi" | "omp" => managed_setup(&provider),
                _ => super::agent_setup::setup(settings.as_deref()),
            };
        }
        AgentCommand::Doctor {
            provider,
            settings,
            session,
            executable,
        } => {
            let executable = executable.unwrap_or_else(|| provider.clone().into());
            return match provider.as_str() {
                "codex" => super::codex_setup::doctor(settings.as_deref(), session, &executable),
                "pi" | "omp" => managed_doctor(&provider, &executable),
                _ => super::agent_setup::doctor(settings.as_deref(), session, &executable),
            };
        }
        AgentCommand::Run {
            provider,
            legacy_provider,
            argv,
        } => (
            provider.or(legacy_provider).expect("required provider"),
            argv,
        ),
    };
    let swallow_conflict = name != "claude";
    let mut lease = ovrcr::report::reserve_invocation_for(provider(&name))
        .or_else(|error| {
            if swallow_conflict {
                Ok(None)
            } else {
                Err(error)
            }
        })
        .map_err(|error| {
            if let Some(report) = error.downcast_ref::<ovrcr::report::ReportError>() {
                RuntimeError::new(report.code(), report.to_string())
            } else {
                RuntimeError::internal(error)
            }
        })?;
    let status = ovrcr::agent_runner::run_native(&argv, move |available, native_argv| {
        if !available {
            drop(lease.take());
            eprintln!("agent reporting unavailable; running native command");
            return None;
        }
        Some(match name.as_str() {
            "codex" => ovrcr::report::codex::receiver(lease, native_argv),
            "pi" => ovrcr::report::pi::receiver(lease, native_argv),
            "omp" => ovrcr::report::unavailable_receiver(
                lease,
                "Oh My Pi",
                "provider reporting not implemented yet",
            ),
            _ => ovrcr::report::admission::receiver(lease, native_argv),
        })
    })
    .map_err(RuntimeError::internal)?;
    if let Some(signal) = status.signal() {
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
        std::process::exit(128 + signal);
    }
    std::process::exit(status.code().unwrap_or(1));
}

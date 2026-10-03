use super::{AppResult, RuntimeError, args::AgentCommand};
use ovrcr::protocol::AgentProvider;
use std::os::unix::process::ExitStatusExt;

/// A managed Grok launch needs no hooks or configuration: nothing is reported and the
/// retained history is a title source, so there is nothing to set up or diagnose.
fn grok_needs_no_setup() -> RuntimeError {
    RuntimeError::new(
        ovrcr::protocol::ErrorCode::InvalidRequest,
        "grok has no setup or doctor: `ovrcr agent run grok -- grok` retains the session history without hooks; resume is unavailable",
    )
}

fn provider(name: &str) -> AgentProvider {
    AgentProvider::from_name(name).expect("clap value_parser accepts known provider names")
}

pub(super) fn run(command: AgentCommand) -> AppResult<()> {
    let (name, argv) = match command {
        AgentCommand::Setup {
            provider,
            print: _,
            settings,
        } => {
            return match provider.as_str() {
                "cursor-agent" => super::cursor::setup(settings.as_deref()),
                "hermes" => super::hermes::setup(settings.as_deref()),
                "grok" => Err(grok_needs_no_setup()),
                "codex" => super::codex_setup::setup(settings.as_deref()),
                "pi" | "omp" => super::managed::setup(
                    super::managed::managed(&provider).expect("managed provider"),
                ),
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
                "cursor-agent" => super::cursor::doctor(settings.as_deref(), session, &executable),
                "hermes" => super::hermes::doctor(settings.as_deref(), session, &executable),
                "grok" => Err(grok_needs_no_setup()),
                "codex" => super::codex_setup::doctor(settings.as_deref(), session, &executable),
                "pi" | "omp" => super::managed::doctor(
                    super::managed::managed(&provider).expect("managed provider"),
                    session,
                    &executable,
                ),
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
    // Hermes has no verified reporting adapter or conversation identity. Process
    // supervision needs no reporting reservation and must not displace another reporter.
    let mut lease = if name == "hermes" {
        None
    } else {
        ovrcr::report::reserve_invocation_for(provider(&name))
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
            })?
    };
    let mut argv = argv;
    // Managed Claude, Codex, Pi, and Oh My Pi auto-trust unless the caller already
    // passed a permission, approve, full-auto, or dangerously-* flag.
    ovrcr::protocol::apply_default_auto_trust(&name, &mut argv);
    let status = ovrcr::agent_runner::run_native(&argv, move |available, native_argv| {
        if name == "hermes" {
            eprintln!("Hermes activity/reporting and recovery are unavailable; supervising native process only (see `ovrcr agent doctor hermes --json`).");
            return None;
        }
        if !available {
            drop(lease.take());
            eprintln!(
                "agent reporting unavailable; running native command. Repair: `ovrcr agent doctor {name} --json` (setup/doctor never rewrite settings or approve trust)."
            );
            return None;
        }
        Some(match name.as_str() {
            "cursor-agent" => ovrcr::report::cursor::receiver(lease, native_argv),
            "codex" => ovrcr::report::codex::receiver(lease, native_argv),
            "pi" => ovrcr::report::pi::receiver(lease, native_argv),
            "omp" => ovrcr::report::omp::receiver(lease, native_argv),
            "grok" => ovrcr::report::grok::receiver(lease, native_argv),
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

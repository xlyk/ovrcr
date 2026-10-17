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
    let mut argv = argv;
    let assigned_claude = name == "claude" && super::claude_delegation::prepare(&argv)?;
    ovrcr::protocol::apply_default_auto_trust(&name, &mut argv);
    let pi_resume = if name == "pi" && ovrcr::report::pi::eligible_argv(&argv) {
        ovrcr_runtime::extension_recovery::explicit_pi_resume(&argv)
            .ok()
            .flatten()
    } else {
        None
    };
    let swallow_conflict = name != "claude";
    let mut lease = {
        ovrcr::report::reserve_invocation_with_resume(provider(&name), pi_resume)
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
    let mut delegation_environment = Vec::new();
    if (name == "pi" || assigned_claude)
        && let (Ok(session), Ok(token), Ok(socket)) = (
            std::env::var("OVRCR_SESSION_ID"),
            std::env::var("OVRCR_HOOK_TOKEN"),
            std::env::var("OVRCR_HOOK_SOCKET"),
        )
    {
        delegation_environment = vec![
            ("OVRCR_DELEGATION_SESSION".into(), session.into()),
            (
                "OVRCR_DELEGATION_TOKEN".into(),
                ovrcr::agent_runner::delegation_capability(&token).into(),
            ),
            ("OVRCR_DELEGATION_SOCKET".into(), socket.into()),
            (
                "OVRCR_DELEGATION_HOME".into(),
                ovrcr::config::RegistryPath::resolve()
                    .map_err(RuntimeError::internal)?
                    .0
                    .into_os_string(),
            ),
        ];
    }
    let status = ovrcr::agent_runner::run_native_with_env(&argv, &delegation_environment, move |available, native_argv| {
        if name == "hermes" {
            if !available {
                drop(lease.take());
                eprintln!(
                    "Hermes reporting unavailable; supervising native process only. Repair: `ovrcr agent doctor hermes --json`."
                );
                return None;
            }
            return Some(ovrcr::report::hermes::receiver(lease, native_argv));
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

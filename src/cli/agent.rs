use super::{AppResult, RuntimeError, args::AgentCommand};
use std::os::unix::process::ExitStatusExt;

pub(super) fn run(command: AgentCommand) -> AppResult<()> {
    let argv = match command {
        AgentCommand::Setup {
            provider: _,
            print: _,
            settings,
        } => return super::agent_setup::setup(settings.as_deref()),
        AgentCommand::Doctor {
            provider: _,
            settings,
            session,
            executable,
        } => return super::agent_setup::doctor(settings.as_deref(), session, &executable),
        AgentCommand::Run { provider: _, argv } => argv,
    };
    let mut lease = ovrcr::report::reserve_invocation().map_err(|error| {
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
        Some(ovrcr::report::admission::receiver(lease, native_argv))
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

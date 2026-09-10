use super::{AppResult, RuntimeError, args::AgentCommand};
use std::os::unix::process::ExitStatusExt;

pub(super) fn run(command: AgentCommand) -> AppResult<()> {
    let AgentCommand::Run { provider: _, argv } = command;
    let lease = ovrcr::report::reserve_invocation().map_err(|error| {
        if let Some(report) = error.downcast_ref::<ovrcr::report::ReportError>() {
            RuntimeError::new(report.code(), report.to_string())
        } else {
            RuntimeError::internal(error)
        }
    })?;
    if lease.is_some() {
        eprintln!("agent admission unavailable; running native command");
    } else {
        eprintln!("agent reporting unavailable; running native command");
    }
    let status = ovrcr::agent_runner::run_native(&argv).map_err(RuntimeError::internal)?;
    drop(lease);
    if let Some(signal) = status.signal() {
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
        std::process::exit(128 + signal);
    }
    std::process::exit(status.code().unwrap_or(1));
}

use ovrcr::context::{
    ContextUsageSnapshot, format_context, parse_claude_context, parse_context_json,
};
use ovrcr::protocol::{AgentUpdate, ErrorCode};
use ovrcr::report as app_report;
use std::time::{Duration, Instant};

use super::args::ReportCommand;
use super::{AppResult, RuntimeError};

pub(super) fn run_report(command: ReportCommand) -> AppResult<()> {
    match command {
        ReportCommand::Activity { state, sequence } => {
            let deadline = Instant::now() + Duration::from_secs(1);
            app_report::send_report(AgentUpdate::Activity(state), sequence, deadline)
                .map_err(report_runtime_error)
        }
        ReportCommand::Context {
            stdin_json,
            sequence,
        } => run_report_context(stdin_json, sequence),
        ReportCommand::Claude {
            stdin_json,
            verbose,
        } => run_report_claude(stdin_json, verbose),
        ReportCommand::ClaudeContext { stdin_json } => run_report_claude_context(stdin_json),
    }
}

fn run_report_context(stdin_json: bool, sequence: Option<u64>) -> AppResult<()> {
    if !stdin_json {
        return Err(RuntimeError::new(
            ErrorCode::InvalidRequest,
            "--stdin-json is required",
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    let input = app_report::read_hook_stdin(deadline).map_err(report_runtime_error)?;
    let context = parse_context_json(&input).map_err(|_| invalid_context_input())?;
    app_report::send_report(AgentUpdate::Context(context), sequence, deadline)
        .map_err(report_runtime_error)
}

fn run_report_claude_context(stdin_json: bool) -> AppResult<()> {
    if !stdin_json {
        return Err(RuntimeError::new(
            ErrorCode::InvalidRequest,
            "--stdin-json is required",
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    let input = app_report::read_hook_stdin(deadline).map_err(report_runtime_error)?;
    let report = parse_claude_context(&input).map_err(|_| invalid_context_input())?;
    app_report::send_report(AgentUpdate::Context(report.clone()), None, deadline)
        .map_err(report_runtime_error)?;
    let sample = ContextUsageSnapshot {
        report,
        received_unix_ms: 0,
    };
    println!("ctx {}", format_context(Some(&sample), 0, false));
    Ok(())
}

fn run_report_claude(stdin_json: bool, verbose: bool) -> AppResult<()> {
    if !stdin_json {
        return Err(RuntimeError::new(
            ErrorCode::InvalidRequest,
            "--stdin-json is required",
        ));
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    let input = match app_report::read_hook_stdin(deadline) {
        Ok(input) => input,
        Err(_) => {
            if verbose {
                eprintln!("hook adapter: stdin unavailable or timed out");
            }
            return Ok(());
        }
    };
    if std::env::var_os("OVRCR_AGENT_SOCKET").is_some() {
        if app_report::send_claude_hook(&input, deadline).is_err() && verbose {
            eprintln!("hook adapter: admission unavailable");
        }
        return Ok(());
    }
    let activity = match app_report::claude_activity(&input) {
        Ok(activity) => activity,
        Err(_) => {
            if verbose {
                eprintln!("hook adapter: provider input invalid");
            }
            return Ok(());
        }
    };
    let Some(activity) = activity else {
        return Ok(());
    };
    if app_report::send_report(AgentUpdate::Activity(activity), None, deadline).is_err() && verbose
    {
        eprintln!("hook adapter: report transport failed");
    }
    Ok(())
}

fn report_runtime_error(error: anyhow::Error) -> RuntimeError {
    if let Some(report_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<app_report::ReportError>())
    {
        RuntimeError::new(report_error.code(), report_error.to_string())
    } else {
        RuntimeError::new(ErrorCode::Internal, "hook report failed")
    }
}

fn invalid_context_input() -> RuntimeError {
    RuntimeError::new(ErrorCode::InvalidRequest, "hook input invalid")
}

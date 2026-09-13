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
        ReportCommand::Codex { stdin: _ } => {
            if std::env::var_os("OVRCR_AGENT_SOCKET").is_none() {
                return Ok(());
            }
            let deadline = Instant::now() + Duration::from_secs(1);
            if let Ok(input) = app_report::read_hook_stdin(deadline) {
                let _ = app_report::send_codex_hook(&input, deadline);
            }
            Ok(())
        }
        // Both extension harnesses speak the same envelope; only the provider differs.
        ReportCommand::Pi { .. } | ReportCommand::Omp { .. } => {
            let send: fn(&[u8], Instant) -> anyhow::Result<()> =
                if matches!(command, ReportCommand::Pi { .. }) {
                    app_report::send_pi_event
                } else {
                    app_report::send_omp_event
                };
            if std::env::var_os("OVRCR_AGENT_SOCKET").is_none() {
                return Ok(());
            }
            let deadline = Instant::now() + Duration::from_secs(1);
            if let Ok(input) = app_report::read_hook_stdin(deadline) {
                let _ = send(&input, deadline);
            }
            Ok(())
        }
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
        ReportCommand::ClaudeStatusline {
            stdin_json,
            render_command,
        } => run_claude_statusline(stdin_json, render_command),
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

fn run_claude_statusline(stdin_json: bool, render_command: Option<String>) -> AppResult<()> {
    use std::io::{Read, Write};
    if !stdin_json {
        return Err(invalid_context_input());
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    if let Some(command) = render_command {
        let mut renderer = std::process::Command::new("/bin/sh")
            .args(["-c", &command])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|_| RuntimeError::new(ErrorCode::Internal, "renderer could not start"))?;
        let mut pipe = renderer.stdin.take();
        let mut retained = Vec::new();
        let mut oversized = false;
        let mut buffer = [0u8; 8192];
        let mut stdin = std::io::stdin().lock();
        loop {
            let count = match stdin.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    oversized = true;
                    break;
                }
            };
            if let Some(output) = &mut pipe
                && output.write_all(&buffer[..count]).is_err()
            {
                pipe = None;
            }
            if !oversized && retained.len() + count <= 65_536 {
                retained.extend_from_slice(&buffer[..count]);
            } else {
                oversized = true;
                retained.clear();
            }
        }
        drop(pipe);
        if !oversized && Instant::now() < deadline {
            let _ = app_report::send_claude_statusline(&retained, deadline);
        }
        let status = renderer
            .wait()
            .map_err(|_| RuntimeError::new(ErrorCode::Internal, "renderer wait failed"))?;
        if !status.success() {
            std::process::exit(status.code().unwrap_or(1));
        }
        return Ok(());
    }
    let Ok(input) = app_report::read_hook_stdin(deadline) else {
        return Ok(());
    };
    let rendered = parse_claude_context(&input).ok().map(|report| {
        let sample = ContextUsageSnapshot {
            report,
            received_unix_ms: 0,
        };
        format!("ctx {}", format_context(Some(&sample), 0, false))
    });
    let _ = app_report::send_claude_statusline(&input, deadline);
    if let Some(rendered) = rendered {
        println!("{rendered}");
    }
    Ok(())
}

mod agent;
mod agent_setup;
mod args;
mod codex_setup;
mod cursor;
mod hermes;
mod managed;
mod output;
mod report;
mod resources;

use anyhow::{Context, Result};
use clap::Parser;
use ovrcr::client::{connect_if_running, connect_or_start};
use ovrcr::config::{Registry, RegistryPath, load_registry};
use ovrcr::protocol::client;
use ovrcr::protocol::{ErrorCode, Request, Response};
use ovrcr::server::{ServerPaths, run_server};
use ovrcr::session::{SessionId, SessionSummary};
use ovrcr::tui::run_dashboard;
use serde_json::json;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use args::*;
use output::*;
use report::*;
use resources::*;

#[derive(Debug)]
struct RuntimeError {
    code: ErrorCode,
    message: String,
}

type AppResult<T> = std::result::Result<T, RuntimeError>;

impl RuntimeError {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn internal(error: impl Into<anyhow::Error>) -> Self {
        // `{:#}` keeps the whole cause chain; `to_string` shows only the
        // outermost context and hid the OS error behind "connect server".
        Self::new(ErrorCode::Internal, format!("{:#}", error.into()))
    }
}

pub(crate) fn main() {
    let cli = Cli::parse();
    let json = cli.json;
    if let Err(error) = run(cli) {
        print_runtime_error(&error, json);
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> AppResult<()> {
    let json_output = cli.json;
    let Some(command) = cli.command else {
        let paths = ServerPaths::resolve().map_err(RuntimeError::internal)?;
        return run_dashboard(
            connect_or_start(&paths).map_err(RuntimeError::internal)?,
            dashboard_settings_path().map_err(RuntimeError::internal)?,
            agent_setup::boot_hook_warning(),
        )
        .map_err(RuntimeError::internal);
    };
    match command {
        Command::AgentCollector => {
            ovrcr::report::collector::run_helper().map_err(RuntimeError::internal)
        }
        Command::RetargetHooks { settings } => {
            let executable = agent_setup::binary().map_err(RuntimeError::internal)?;
            let files = if settings.is_empty() {
                agent_setup::default_hook_files()
            } else {
                settings
            };
            agent_setup::retarget_hooks(&executable, &files)
        }
        Command::Agent { command } => agent::run(command),
        Command::Task(args) => {
            ovrcr::task_cli::run_task(args.command, json_output).map_err(RuntimeError::internal)
        }
        Command::Run(args) => {
            ovrcr::task_cli::run_run(args.command, json_output).map_err(RuntimeError::internal)
        }
        Command::Service(args) => {
            ovrcr::service::run(args.command, json_output).map_err(RuntimeError::internal)
        }
        Command::TaskRunner {
            run_dir,
            pi_executable,
        } => {
            ovrcr::task_runner::supervise(&run_dir, &pi_executable).map_err(RuntimeError::internal)
        }
        Command::Server => {
            ovrcr::service::load_environment_file().map_err(RuntimeError::internal)?;
            run_server(
                ServerPaths::resolve().map_err(RuntimeError::internal)?,
                RegistryPath::resolve().map_err(RuntimeError::internal)?.0,
            )
            .map_err(RuntimeError::internal)
        }
        Command::List => {
            let paths = ServerPaths::resolve().map_err(RuntimeError::internal)?;
            let Some(mut stream) = connect_if_running(&paths).map_err(RuntimeError::internal)?
            else {
                return if json_output {
                    print_json(&json!([]))
                } else {
                    Ok(())
                };
            };
            print_legacy_response(
                send_request(&mut stream, Request::List).map_err(RuntimeError::internal)?,
                json_output,
            )
        }
        Command::Shutdown { kill } => {
            let paths = ServerPaths::resolve().map_err(RuntimeError::internal)?;
            let Some(mut stream) = connect_if_running(&paths).map_err(RuntimeError::internal)?
            else {
                return if json_output {
                    print_json(&json!({ "ok": true }))
                } else {
                    Ok(())
                };
            };
            print_mutation(
                send_request(&mut stream, Request::Shutdown { kill })
                    .map_err(RuntimeError::internal)?,
                json_output,
            )
        }
        Command::Project { command } => run_project(command, json_output),
        Command::Workspace { command } => run_workspace(command, json_output),
        Command::Terminal { command } => run_terminal(command, json_output),
        Command::New(args) => create_terminal(args, json_output),
        Command::Kill { id } => mutate_without_start(
            Request::KillSession {
                session: SessionId(id),
            },
            json_output,
        ),
        Command::Pause { id } => mutate_without_start(
            Request::PauseSession {
                session: SessionId(id),
            },
            json_output,
        ),
        Command::Resume { id } => mutate_without_start(
            Request::ResumeSession {
                session: SessionId(id),
            },
            json_output,
        ),
        Command::Session {
            command: SessionCommand::Context { id },
        } => inspect_session_context(id),
        Command::Session {
            command: SessionCommand::Usage { id },
        } => inspect_session_usage(id),
        Command::Session {
            command: SessionCommand::Remove { id },
        } => resources::remove_terminal(id, json_output),
        Command::Report { command } => run_report(command),
        Command::Settings => {
            let RegistryPath(config) = RegistryPath::resolve().map_err(RuntimeError::internal)?;
            let report = ovrcr::settings::load(&config);
            if json_output {
                print_json(&serde_json::to_value(&report).map_err(RuntimeError::internal)?)
            } else {
                print!("{}", settings_text(&report));
                Ok(())
            }
        }
    }
}

fn settings_text(report: &ovrcr::protocol::SettingsReport) -> String {
    use std::fmt::Write;
    let mut text = format!("Settings document: {}\n", report.path.display());
    for row in &report.rows {
        let source = match row.source {
            ovrcr::protocol::SettingSource::Default => "default",
            ovrcr::protocol::SettingSource::Document => "document",
        };
        let _ = writeln!(
            text,
            "  {:<26} {:<9} {:<8} {}",
            row.key,
            format!("{:?}", row.owner),
            source,
            row.value.as_deref().unwrap_or("unset")
        );
        if let Some(off) = &row.off_state {
            let _ = writeln!(text, "  {:<26} {off}", "");
        }
    }
    if report.findings.is_empty() {
        text.push_str("No findings.\n");
    } else {
        let _ = writeln!(text, "Findings ({}):", report.findings.len());
        for finding in &report.findings {
            let line = finding
                .line
                .map(|line| format!(" (line {line})"))
                .unwrap_or_default();
            let key = finding.key.as_deref().unwrap_or("document");
            let _ = writeln!(text, "  {key}{line}: {}", finding.message);
        }
    }
    text
}

fn inspect() -> AppResult<(Registry, Vec<SessionSummary>)> {
    let paths = ServerPaths::resolve().map_err(RuntimeError::internal)?;
    if let Some(mut stream) = connect_if_running(&paths).map_err(RuntimeError::internal)? {
        return match send_request(&mut stream, Request::Inspect).map_err(RuntimeError::internal)? {
            Response::Inventory { registry, sessions } => Ok((registry, sessions)),
            Response::Error { code, message } => Err(RuntimeError::new(code, message)),
            response => Err(unexpected_response(response)),
        };
    }
    let path = RegistryPath::resolve().map_err(RuntimeError::internal)?.0;
    let mut registry = load_registry(&path).map_err(RuntimeError::internal)?;
    ovrcr::git::observe_registry(&mut registry);
    Ok((
        registry,
        ovrcr::retained::load_session_summaries(&path).map_err(RuntimeError::internal)?,
    ))
}

fn request_started(request: Request) -> AppResult<Response> {
    let paths = ServerPaths::resolve().map_err(RuntimeError::internal)?;
    let mut stream = connect_or_start(&paths).map_err(RuntimeError::internal)?;
    response_or_error(send_request(&mut stream, request).map_err(RuntimeError::internal)?)
}

fn request_without_start(request: Request) -> AppResult<Response> {
    let paths = ServerPaths::resolve().map_err(RuntimeError::internal)?;
    let Some(mut stream) = connect_if_running(&paths).map_err(RuntimeError::internal)? else {
        return Err(RuntimeError::new(
            ErrorCode::NotFound,
            "OVRCR server is not running",
        ));
    };
    response_or_error(send_request(&mut stream, request).map_err(RuntimeError::internal)?)
}

fn response_or_error(response: Response) -> AppResult<Response> {
    match response {
        Response::Error { code, message } => Err(RuntimeError::new(code, message)),
        response => Ok(response),
    }
}

fn unexpected_response(response: Response) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::Internal,
        format!("unexpected server response: {response:?}"),
    )
}

/// Default bound on one request round trip. Kill, close, and shutdown may
/// legitimately wait out a termination grace period, so they get longer.
fn request_timeout(request: &Request) -> Duration {
    let default = match request {
        Request::KillSession { .. } | Request::CloseTerminal { .. } | Request::Shutdown { .. } => {
            Duration::from_secs(60)
        }
        _ => Duration::from_secs(30),
    };
    // Test seam so a wedged-server test does not wait half a minute.
    std::env::var("OVRCR_REQUEST_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(default)
}

fn send_request(stream: &mut UnixStream, request: Request) -> Result<Response> {
    let timeout = request_timeout(&request);
    stream
        .set_read_timeout(Some(timeout))
        .context("bound server request")?;
    stream
        .set_write_timeout(Some(timeout))
        .context("bound server request")?;
    client::request(stream, 1, request).map_err(|error| {
        let timed_out = error.chain().any(|cause| {
            cause.downcast_ref::<std::io::Error>().is_some_and(|io| {
                matches!(
                    io.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                )
            })
        });
        if timed_out {
            anyhow::anyhow!(
                "timed out after {timeout:?} waiting for the server's response; the server may be wedged, see its log"
            )
        } else {
            error
        }
    })
}

fn dashboard_settings_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("OVRCR_DASHBOARD_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    let RegistryPath(config) = RegistryPath::resolve()?;
    Ok(config
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("dashboard.toml"))
}

mod args;
mod output;
mod report;
mod resources;

use anyhow::{Result, bail};
use clap::Parser;
use ovrcr::client::{connect_if_running, connect_or_start};
use ovrcr::config::{Registry, RegistryPath, load_registry};
use ovrcr::protocol::{
    ClientMessage, ErrorCode, Request, Response, ServerMessage, read_frame, write_frame,
};
use ovrcr::server::{ServerPaths, run_server};
use ovrcr::session::{SessionId, SessionSummary};
use ovrcr::tui::run_dashboard;
use serde_json::json;
use std::os::unix::net::UnixStream;

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
        return run_dashboard(connect_or_start(&paths).map_err(RuntimeError::internal)?)
            .map_err(RuntimeError::internal);
    };
    match command {
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
        Command::Kill { id } => mutate_started(
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
            command: SessionCommand::Remove { id },
        } => mutate_started(
            Request::RemoveSession {
                session: SessionId(id),
            },
            json_output,
        ),
        Command::Report { command } => run_report(command),
    }
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
    Ok((
        load_registry(&path).map_err(RuntimeError::internal)?,
        Vec::new(),
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

fn send_request(stream: &mut UnixStream, request: Request) -> Result<Response> {
    write_frame(
        stream,
        &ClientMessage {
            request_id: 1,
            request,
        },
    )?;
    match read_frame::<ServerMessage>(stream)? {
        ServerMessage::Response { response, .. } => Ok(response),
        ServerMessage::Event(_) => bail!("server sent an event before the response"),
    }
}

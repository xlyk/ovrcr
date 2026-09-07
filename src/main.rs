use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Args, Parser, Subcommand};
use ovrcr::client::{connect_if_running, connect_or_start};
use ovrcr::config::{ProjectRecord, Registry, RegistryPath, WorkspaceRecord, load_registry};
use ovrcr::context::{
    ContextUsageSnapshot, context_is_stale, format_context, parse_claude_context,
    parse_context_json,
};
use ovrcr::protocol::{
    AgentUpdate, BranchRequest, ClientMessage, CreateSessionRequest, ErrorCode, Request, Response,
    ServerMessage, read_frame, write_frame,
};
use ovrcr::report;
use ovrcr::server::{ServerPaths, run_server};
use ovrcr::session::{AgentActivity, SessionId, SessionPhase, SessionSummary};
use ovrcr::tui::run_dashboard;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "ovrcr")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Server,
    #[command(visible_alias = "projects")]
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    #[command(visible_alias = "workspaces")]
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    #[command(visible_alias = "terminals")]
    Terminal {
        #[command(subcommand)]
        command: TerminalCommand,
    },
    New(NewArgs),
    List,
    Kill {
        id: u64,
    },
    Pause {
        id: u64,
    },
    Resume {
        id: u64,
    },
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Shutdown {
        #[arg(long)]
        kill: bool,
    },
    Report {
        #[command(subcommand)]
        command: ReportCommand,
    },
}

#[derive(Subcommand)]
enum ReportCommand {
    Activity {
        #[arg(long, value_parser = parse_activity_state)]
        state: AgentActivity,
        #[arg(long)]
        sequence: Option<u64>,
    },
    Context {
        #[arg(long)]
        stdin_json: bool,
        #[arg(long)]
        sequence: Option<u64>,
    },
    Claude {
        #[arg(long)]
        stdin_json: bool,
        #[arg(long)]
        verbose: bool,
    },
    ClaudeContext {
        #[arg(long)]
        stdin_json: bool,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    #[command(visible_alias = "create")]
    Add {
        name: String,
        repo: PathBuf,
        #[arg(long)]
        workspace_root: PathBuf,
    },
    List,
    Get {
        name: String,
    },
    #[command(visible_alias = "delete")]
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
enum WorkspaceCommand {
    Create(WorkspaceCreateArgs),
    List {
        #[arg(long)]
        project: Option<String>,
    },
    Get {
        #[arg(long)]
        project: String,
        #[arg(long)]
        name: String,
    },
    #[command(visible_alias = "delete")]
    Remove {
        #[arg(long)]
        project: String,
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
enum TerminalCommand {
    Create(NewArgs),
    List {
        #[arg(long)]
        project: Option<String>,
        #[arg(long, requires = "project")]
        workspace: Option<String>,
    },
    Read {
        id: u64,
        #[arg(long, value_parser = parse_positive_usize)]
        max_lines: Option<usize>,
    },
    Send {
        id: u64,
        #[arg(long)]
        text: String,
        #[arg(long)]
        no_submit: bool,
    },
    Close {
        id: u64,
    },
    Kill {
        id: u64,
    },
    Remove {
        id: u64,
    },
}

#[derive(Args)]
#[command(group(ArgGroup::new("branch_source").required(true).args(["new_branch", "branch"])))]
struct WorkspaceCreateArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    name: String,
    #[arg(long, conflicts_with = "branch", requires = "base")]
    new_branch: Option<String>,
    #[arg(long, requires = "new_branch")]
    base: Option<String>,
    #[arg(long, conflicts_with_all = ["new_branch", "base"])]
    branch: Option<String>,
}

#[derive(Args)]
struct NewArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    workspace: String,
    #[arg(long)]
    name: String,
    #[arg(long)]
    label: Option<String>,
    #[arg(last = true)]
    argv: Vec<OsString>,
}

#[derive(Subcommand)]
enum SessionCommand {
    Context { id: u64 },
    Remove { id: u64 },
}

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

    fn internal(error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::Internal, error.to_string())
    }
}

fn main() {
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
        Command::Server => run_server(
            ServerPaths::resolve().map_err(RuntimeError::internal)?,
            RegistryPath::resolve().map_err(RuntimeError::internal)?.0,
        )
        .map_err(RuntimeError::internal),
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

fn run_report(command: ReportCommand) -> AppResult<()> {
    match command {
        ReportCommand::Activity { state, sequence } => {
            let deadline = Instant::now() + Duration::from_secs(1);
            report::send_report(AgentUpdate::Activity(state), sequence, deadline)
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
    let input = report::read_hook_stdin(deadline).map_err(report_runtime_error)?;
    let context = parse_context_json(&input).map_err(|_| invalid_context_input())?;
    report::send_report(AgentUpdate::Context(context), sequence, deadline)
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
    let input = report::read_hook_stdin(deadline).map_err(report_runtime_error)?;
    let report = parse_claude_context(&input).map_err(|_| invalid_context_input())?;
    report::send_report(AgentUpdate::Context(report.clone()), None, deadline)
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
    let input = match report::read_hook_stdin(deadline) {
        Ok(input) => input,
        Err(_) => {
            if verbose {
                eprintln!("hook adapter: stdin unavailable or timed out");
            }
            return Ok(());
        }
    };
    let activity = match report::claude_activity(&input) {
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
    if report::send_report(AgentUpdate::Activity(activity), None, deadline).is_err() && verbose {
        eprintln!("hook adapter: report transport failed");
    }
    Ok(())
}

fn report_runtime_error(error: anyhow::Error) -> RuntimeError {
    if let Some(report_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<report::ReportError>())
    {
        RuntimeError::new(report_error.code(), report_error.to_string())
    } else {
        RuntimeError::new(ErrorCode::Internal, "hook report failed")
    }
}

fn invalid_context_input() -> RuntimeError {
    RuntimeError::new(ErrorCode::InvalidRequest, "hook input invalid")
}

fn run_project(command: ProjectCommand, json_output: bool) -> AppResult<()> {
    match command {
        ProjectCommand::Add {
            name,
            repo,
            workspace_root,
        } => mutate_started(
            Request::AddProject {
                name,
                repo: resolve_cli_path(repo).map_err(RuntimeError::internal)?,
                workspace_root: resolve_cli_path(workspace_root).map_err(RuntimeError::internal)?,
            },
            json_output,
        ),
        ProjectCommand::List => {
            let (registry, _) = inspect()?;
            let mut projects = registry.projects.iter().collect::<Vec<_>>();
            projects.sort_by(|left, right| left.name.cmp(&right.name));
            let values = projects.into_iter().map(project_value).collect::<Vec<_>>();
            print_values(values, json_output, print_project_row)
        }
        ProjectCommand::Get { name } => {
            let (registry, _) = inspect()?;
            let project = find_project(&registry, &name)?;
            print_value(project_value(project), json_output, print_project)
        }
        ProjectCommand::Remove { name } => {
            mutate_started(Request::RemoveProject { name }, json_output)
        }
    }
}

fn run_workspace(command: WorkspaceCommand, json_output: bool) -> AppResult<()> {
    match command {
        WorkspaceCommand::Create(args) => {
            let branch = match (args.new_branch, args.branch, args.base) {
                (Some(branch), None, Some(base)) => BranchRequest::New { branch, base },
                (None, Some(branch), None) => BranchRequest::Existing { branch },
                _ => unreachable!("clap validates branch arguments"),
            };
            mutate_started(
                Request::CreateWorkspace {
                    project: args.project,
                    name: args.name,
                    branch,
                },
                json_output,
            )
        }
        WorkspaceCommand::List { project } => {
            let (registry, sessions) = inspect()?;
            if let Some(name) = project.as_deref() {
                find_project(&registry, name)?;
            }
            let mut workspaces = registry
                .projects
                .iter()
                .filter(|record| project.as_deref().is_none_or(|name| record.name == name))
                .flat_map(|record| {
                    record
                        .workspaces
                        .iter()
                        .map(move |workspace| (&record.name, workspace))
                })
                .collect::<Vec<_>>();
            workspaces.sort_by(|(left_project, left), (right_project, right)| {
                (left.name.as_str(), left_project.as_str())
                    .cmp(&(right.name.as_str(), right_project.as_str()))
            });
            let values = workspaces
                .into_iter()
                .map(|(project, workspace)| workspace_value(project, workspace, &sessions))
                .collect::<Vec<_>>();
            print_values(values, json_output, print_workspace_row)
        }
        WorkspaceCommand::Get { project, name } => {
            let (registry, sessions) = inspect()?;
            let workspace = find_workspace(&registry, &project, &name)?;
            print_value(
                workspace_value(&project, workspace, &sessions),
                json_output,
                print_workspace,
            )
        }
        WorkspaceCommand::Remove { project, name } => {
            mutate_started(Request::RemoveWorkspace { project, name }, json_output)
        }
    }
}

fn run_terminal(command: TerminalCommand, json_output: bool) -> AppResult<()> {
    match command {
        TerminalCommand::Create(args) => create_terminal(args, json_output),
        TerminalCommand::List { project, workspace } => {
            let (registry, mut sessions) = inspect()?;
            if let Some(project) = project.as_deref() {
                find_project(&registry, project)?;
                if let Some(workspace) = workspace.as_deref() {
                    find_workspace(&registry, project, workspace)?;
                }
            }
            sessions.retain(|session| {
                project
                    .as_deref()
                    .is_none_or(|name| session.project == name)
                    && workspace
                        .as_deref()
                        .is_none_or(|name| session.workspace == name)
            });
            sessions.sort_by_key(|session| session.id.0);
            let now_unix_ms = now_unix_ms();
            let values = sessions
                .iter()
                .map(|session| terminal_value(session, now_unix_ms))
                .collect::<Vec<_>>();
            print_values(values, json_output, print_terminal_row)
        }
        TerminalCommand::Read { id, max_lines } => {
            match request_without_start(Request::ReadTerminal {
                session: SessionId(id),
                max_lines,
            })? {
                Response::TerminalText {
                    session,
                    size,
                    text,
                } => {
                    if json_output {
                        print_json(&json!({
                            "id": session.0,
                            "rows": size.rows,
                            "cols": size.cols,
                            "text": text,
                        }))
                    } else {
                        print!("{text}");
                        std::io::stdout().flush().map_err(RuntimeError::internal)
                    }
                }
                response => Err(unexpected_response(response)),
            }
        }
        TerminalCommand::Send {
            id,
            text,
            no_submit,
        } => mutate_without_start(
            Request::SendTerminal {
                session: SessionId(id),
                text,
                submit: !no_submit,
            },
            json_output,
        ),
        TerminalCommand::Close { id } => mutate_without_start(
            Request::CloseTerminal {
                session: SessionId(id),
            },
            json_output,
        ),
        TerminalCommand::Kill { id } => mutate_started(
            Request::KillSession {
                session: SessionId(id),
            },
            json_output,
        ),
        TerminalCommand::Remove { id } => mutate_started(
            Request::RemoveSession {
                session: SessionId(id),
            },
            json_output,
        ),
    }
}

fn create_terminal(args: NewArgs, json_output: bool) -> AppResult<()> {
    let argv = if args.argv.is_empty() {
        vec![
            std::env::var_os("SHELL")
                .context("SHELL is unset and no command was provided")
                .map_err(RuntimeError::internal)?,
        ]
    } else {
        args.argv
    };
    let response = request_started(Request::CreateSession(CreateSessionRequest {
        project: args.project,
        workspace: args.workspace,
        name: args.name,
        label: args.label,
        argv,
    }))?;
    match response {
        Response::CreatedSession(summary) => {
            if json_output {
                print_json(&terminal_value(&summary, now_unix_ms()))
            } else {
                println!("{}", summary.id.0);
                Ok(())
            }
        }
        response => Err(unexpected_response(response)),
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

fn inspect_session_context(id: u64) -> AppResult<()> {
    let response = request_without_start(Request::List)?;
    let Response::Hierarchy(snapshot) = response else {
        return Err(unexpected_response(response));
    };
    let session = snapshot
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .find(|session| session.id == SessionId(id))
        .ok_or_else(|| {
            RuntimeError::new(ErrorCode::NotFound, format!("session not found: {id}"))
        })?;
    let now_unix_ms = now_unix_ms();
    let exited = matches!(session.phase, SessionPhase::Exited { .. });
    print_json(&json!({
        "session": id,
        "context_usage": session.context_usage,
        "stale": session
            .context_usage
            .as_ref()
            .map(|sample| context_is_stale(sample, now_unix_ms, exited)),
    }))
}

fn response_or_error(response: Response) -> AppResult<Response> {
    match response {
        Response::Error { code, message } => Err(RuntimeError::new(code, message)),
        response => Ok(response),
    }
}

fn mutate_started(request: Request, json_output: bool) -> AppResult<()> {
    print_mutation(request_started(request)?, json_output)
}

fn mutate_without_start(request: Request, json_output: bool) -> AppResult<()> {
    print_mutation(request_without_start(request)?, json_output)
}

fn print_mutation(response: Response, json_output: bool) -> AppResult<()> {
    match response {
        Response::Ok => {
            if json_output {
                print_json(&json!({ "ok": true }))
            } else {
                Ok(())
            }
        }
        Response::Error { code, message } => Err(RuntimeError::new(code, message)),
        response => Err(unexpected_response(response)),
    }
}

fn project_value(project: &ProjectRecord) -> Value {
    let workspace_count = project.workspaces.len();
    json!({
        "name": project.name,
        "repo": path_text(&project.repo),
        "workspace_root": path_text(&project.workspace_root),
        "workspace_count": workspace_count,
    })
}

fn workspace_value(
    project: &str,
    workspace: &WorkspaceRecord,
    sessions: &[SessionSummary],
) -> Value {
    let terminal_count = sessions
        .iter()
        .filter(|session| session.project == project && session.workspace == workspace.name)
        .count();
    json!({
        "project": project,
        "name": workspace.name,
        "path": path_text(&workspace.path),
        "branch": workspace.branch,
        "terminal_count": terminal_count,
    })
}

fn terminal_value(session: &SessionSummary, now_unix_ms: u64) -> Value {
    let (phase, exit_code, exit_signal) = match &session.phase {
        SessionPhase::Running => ("running", Value::Null, Value::Null),
        SessionPhase::Paused => ("paused", Value::Null, Value::Null),
        SessionPhase::Exited { code, signal } => (
            "exited",
            code.map_or(Value::Null, |code| json!(code)),
            signal.as_ref().map_or(Value::Null, |signal| json!(signal)),
        ),
    };
    json!({
        "id": session.id.0,
        "project": session.project,
        "workspace": session.workspace,
        "name": session.name,
        "label": session.label,
        "pid": session.pid,
        "started_unix_ms": session.started_unix_ms,
        "phase": phase,
        "activity": match session.activity {
            AgentActivity::Unknown => "unknown",
            AgentActivity::Idle => "idle",
            AgentActivity::Busy => "busy",
            AgentActivity::WaitingInput => "waiting_input",
            AgentActivity::Error => "error",
        },
        "exit_code": exit_code,
        "exit_signal": exit_signal,
        "context_usage": session.context_usage,
        "context_stale": session.context_usage.as_ref().map(|sample| {
            context_is_stale(
                sample,
                now_unix_ms,
                matches!(session.phase, SessionPhase::Exited { .. }),
            )
        }),
    })
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn find_project<'a>(registry: &'a Registry, name: &str) -> AppResult<&'a ProjectRecord> {
    registry
        .projects
        .iter()
        .find(|project| project.name == name)
        .ok_or_else(|| RuntimeError::new(ErrorCode::NotFound, format!("project not found: {name}")))
}

fn find_workspace<'a>(
    registry: &'a Registry,
    project: &str,
    name: &str,
) -> AppResult<&'a WorkspaceRecord> {
    find_project(registry, project)?
        .workspaces
        .iter()
        .find(|workspace| workspace.name == name)
        .ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::NotFound,
                format!("workspace not found: {project}/{name}"),
            )
        })
}

fn print_values(values: Vec<Value>, json_output: bool, print_text: fn(&Value)) -> AppResult<()> {
    if json_output {
        print_json(&Value::Array(values))
    } else {
        for value in &values {
            print_text(value);
        }
        Ok(())
    }
}

fn print_value(value: Value, json_output: bool, print_text: fn(&Value)) -> AppResult<()> {
    if json_output {
        print_json(&value)
    } else {
        print_text(&value);
        Ok(())
    }
}

fn print_project_row(value: &Value) {
    println!(
        "{}\t{}\t{}\t{}",
        value["name"].as_str().unwrap_or_default(),
        value["repo"].as_str().unwrap_or_default(),
        value["workspace_root"].as_str().unwrap_or_default(),
        value["workspace_count"]
    );
}

fn print_project(value: &Value) {
    println!(
        "name\t{}\nrepo\t{}\nworkspace_root\t{}\nworkspace_count\t{}",
        value["name"].as_str().unwrap_or_default(),
        value["repo"].as_str().unwrap_or_default(),
        value["workspace_root"].as_str().unwrap_or_default(),
        value["workspace_count"]
    );
}

fn print_workspace_row(value: &Value) {
    println!(
        "{}\t{}\t{}\t{}\t{}",
        value["project"].as_str().unwrap_or_default(),
        value["name"].as_str().unwrap_or_default(),
        value["path"].as_str().unwrap_or_default(),
        value["branch"].as_str().unwrap_or_default(),
        value["terminal_count"]
    );
}

fn print_workspace(value: &Value) {
    println!(
        "project\t{}\nname\t{}\npath\t{}\nbranch\t{}\nterminal_count\t{}",
        value["project"].as_str().unwrap_or_default(),
        value["name"].as_str().unwrap_or_default(),
        value["path"].as_str().unwrap_or_default(),
        value["branch"].as_str().unwrap_or_default(),
        value["terminal_count"]
    );
}

fn print_terminal_row(value: &Value) {
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        value["id"],
        value["project"].as_str().unwrap_or_default(),
        value["workspace"].as_str().unwrap_or_default(),
        value["name"].as_str().unwrap_or_default(),
        value["label"].as_str().unwrap_or_default(),
        json_scalar(&value["pid"]),
        value["started_unix_ms"],
        value["phase"].as_str().unwrap_or_default(),
        value["activity"].as_str().unwrap_or_default(),
        json_scalar(&value["exit_code"]),
        json_scalar(&value["exit_signal"]),
    );
}

fn json_scalar(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        value => value.to_string(),
    }
}

fn print_json(value: &Value) -> AppResult<()> {
    println!(
        "{}",
        serde_json::to_string(value).map_err(RuntimeError::internal)?
    );
    Ok(())
}

fn print_runtime_error(error: &RuntimeError, json_output: bool) {
    if json_output {
        let value = json!({
            "error": {
                "code": format!("{:?}", error.code),
                "message": error.message,
            }
        });
        eprintln!(
            "{}",
            serde_json::to_string(&value).unwrap_or_else(|_| {
                r#"{"error":{"code":"Internal","message":"serialize runtime error"}}"#.into()
            })
        );
    } else {
        eprintln!("{:?}: {}", error.code, error.message);
    }
}

fn unexpected_response(response: Response) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::Internal,
        format!("unexpected server response: {response:?}"),
    )
}

fn print_legacy_response(response: Response, json_output: bool) -> AppResult<()> {
    match response {
        Response::Error { code, message } => Err(RuntimeError::new(code, message)),
        Response::CreatedSession(summary) => {
            println!("{}", summary.id.0);
            Ok(())
        }
        Response::Hierarchy(snapshot) if json_output => {
            let now_unix_ms = now_unix_ms();
            let projects = snapshot
                .projects
                .into_iter()
                .map(|project| {
                    let workspaces = project
                        .workspaces
                        .into_iter()
                        .map(|workspace| {
                            let terminals = workspace
                                .sessions
                                .iter()
                                .map(|session| legacy_terminal_value(session, now_unix_ms))
                                .collect::<Vec<_>>();
                            json!({
                                "project": workspace.project,
                                "name": workspace.name,
                                "path": path_text(&workspace.path),
                                "terminals": terminals,
                            })
                        })
                        .collect::<Vec<_>>();
                    json!({
                        "name": project.name,
                        "workspaces": workspaces,
                    })
                })
                .collect::<Vec<_>>();
            print_json(&Value::Array(projects))
        }
        Response::Hierarchy(snapshot) => {
            for project in snapshot.projects {
                println!("project {}", project.name);
                for workspace in project.workspaces {
                    println!("  workspace {}", workspace.name);
                    for session in workspace.sessions {
                        let phase = match session.phase {
                            SessionPhase::Running => "running",
                            SessionPhase::Paused => "paused",
                            SessionPhase::Exited { .. } => "exited",
                        };
                        println!("    session {} {} {phase}", session.id.0, session.name);
                    }
                }
            }
            Ok(())
        }
        Response::Screen { .. } | Response::Ok => Ok(()),
        response => Err(unexpected_response(response)),
    }
}

fn legacy_terminal_value(session: &SessionSummary, now_unix_ms: u64) -> Value {
    let mut value = terminal_value(session, now_unix_ms);
    if let Value::Object(fields) = &mut value {
        fields.remove("context_usage");
        fields.remove("context_stale");
    }
    value
}

fn send_request(stream: &mut std::os::unix::net::UnixStream, request: Request) -> Result<Response> {
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

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn resolve_cli_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn parse_positive_usize(value: &str) -> std::result::Result<usize, String> {
    let value = value
        .parse::<usize>()
        .map_err(|_| "must be a positive integer".to_owned())?;
    if value == 0 {
        Err("must be greater than zero".to_owned())
    } else {
        Ok(value)
    }
}

fn parse_activity_state(value: &str) -> std::result::Result<AgentActivity, String> {
    match value {
        "unknown" => Ok(AgentActivity::Unknown),
        "idle" => Ok(AgentActivity::Idle),
        "busy" => Ok(AgentActivity::Busy),
        "waiting-input" => Ok(AgentActivity::WaitingInput),
        "error" => Ok(AgentActivity::Error),
        _ => Err("must be one of: unknown, idle, busy, waiting-input, error".into()),
    }
}

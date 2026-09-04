use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Args, Parser, Subcommand};
use ovrcr::config::RegistryPath;
use ovrcr::protocol::{
    BranchRequest, ClientMessage, CreateSessionRequest, Request, Response, ServerMessage,
    read_frame, write_frame,
};
use ovrcr::server::{ServerPaths, connect_if_running, connect_or_start, run_server};
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ovrcr")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Server,
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    New(NewArgs),
    List,
    Kill {
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
}

#[derive(Subcommand)]
enum ProjectCommand {
    Add {
        name: String,
        repo: PathBuf,
        #[arg(long)]
        workspace_root: PathBuf,
    },
    List,
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
enum WorkspaceCommand {
    Create(WorkspaceCreateArgs),
    Remove {
        #[arg(long)]
        project: String,
        #[arg(long)]
        name: String,
    },
}

#[derive(Args)]
#[command(group(ArgGroup::new("branch_source").required(true).args(["new_branch", "branch"])))]
struct WorkspaceCreateArgs {
    #[arg(long)]
    project: String,
    #[arg(long)]
    name: String,
    #[arg(long)]
    new_branch: Option<String>,
    #[arg(long)]
    base: Option<String>,
    #[arg(long)]
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
    Remove { id: u64 },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or(Command::List);
    match command {
        Command::Server => run_server(ServerPaths::resolve()?, RegistryPath::resolve()?.0),
        Command::List => {
            let paths = ServerPaths::resolve()?;
            let Some(mut stream) = connect_if_running(&paths)? else {
                return Ok(());
            };
            print_response(send_request(&mut stream, Request::List)?)
        }
        Command::Shutdown { kill } => {
            let paths = ServerPaths::resolve()?;
            let Some(mut stream) = connect_if_running(&paths)? else {
                return Ok(());
            };
            print_response(send_request(&mut stream, Request::Shutdown { kill })?)
        }
        Command::Project { command } => {
            let request = match command {
                ProjectCommand::Add {
                    name,
                    repo,
                    workspace_root,
                } => Request::AddProject {
                    name,
                    repo,
                    workspace_root,
                },
                ProjectCommand::List => {
                    let paths = ServerPaths::resolve()?;
                    let Some(mut stream) = connect_if_running(&paths)? else {
                        return Ok(());
                    };
                    return print_response(send_request(&mut stream, Request::List)?);
                }
                ProjectCommand::Remove { name } => Request::RemoveProject { name },
            };
            let paths = ServerPaths::resolve()?;
            let mut stream = connect_or_start(&paths)?;
            print_response(send_request(&mut stream, request)?)
        }
        Command::Workspace { command } => {
            let request = match command {
                WorkspaceCommand::Create(args) => {
                    let branch = match (args.new_branch, args.branch, args.base) {
                        (Some(branch), None, Some(base)) => BranchRequest::New { branch, base },
                        (Some(_), None, None) => bail!("--base is required with --new-branch"),
                        (Some(_), Some(_), _) => unreachable!("clap branch group rejects this"),
                        (None, Some(branch), None) => BranchRequest::Existing { branch },
                        (None, Some(_), Some(_)) => bail!("--base cannot be used with --branch"),
                        (None, None, _) => unreachable!("clap requires a branch source"),
                    };
                    Request::CreateWorkspace {
                        project: args.project,
                        name: args.name,
                        branch,
                    }
                }
                WorkspaceCommand::Remove { project, name } => {
                    Request::RemoveWorkspace { project, name }
                }
            };
            let paths = ServerPaths::resolve()?;
            let mut stream = connect_or_start(&paths)?;
            print_response(send_request(&mut stream, request)?)
        }
        Command::New(args) => {
            let argv = if args.argv.is_empty() {
                vec![
                    std::env::var_os("SHELL")
                        .context("SHELL is unset and no command was provided")?,
                ]
            } else {
                args.argv
            };
            let paths = ServerPaths::resolve()?;
            let mut stream = connect_or_start(&paths)?;
            print_response(send_request(
                &mut stream,
                Request::CreateSession(CreateSessionRequest {
                    project: args.project,
                    workspace: args.workspace,
                    name: args.name,
                    label: args.label,
                    argv,
                }),
            )?)
        }
        Command::Kill { id } => {
            let paths = ServerPaths::resolve()?;
            let mut stream = connect_or_start(&paths)?;
            print_response(send_request(
                &mut stream,
                Request::KillSession {
                    session: ovrcr::session::SessionId(id),
                },
            )?)
        }
        Command::Session {
            command: SessionCommand::Remove { id },
        } => {
            let paths = ServerPaths::resolve()?;
            let mut stream = connect_or_start(&paths)?;
            print_response(send_request(
                &mut stream,
                Request::RemoveSession {
                    session: ovrcr::session::SessionId(id),
                },
            )?)
        }
    }
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

fn print_response(response: Response) -> Result<()> {
    match response {
        Response::Error { code, message } => bail!("{code:?}: {message}"),
        Response::CreatedSession(summary) => {
            println!("{}", summary.id.0);
            Ok(())
        }
        Response::Hierarchy(snapshot) => {
            for project in snapshot.projects {
                println!("project {}", project.name);
                for workspace in project.workspaces {
                    println!("  workspace {}", workspace.name);
                    for session in workspace.sessions {
                        println!("    session {} {}", session.id.0, session.name);
                    }
                }
            }
            Ok(())
        }
        Response::Screen { .. } | Response::Ok => Ok(()),
    }
}

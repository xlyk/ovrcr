use anyhow::Context;
use clap::Parser;
use ovrcr::config::{ProjectRecord, Registry, WorkspaceRecord};
use ovrcr::freshness;
use ovrcr::protocol::{
    AgentProvider, BranchRequest, CreateSessionRequest, ErrorCode, Request, Response, SessionKind,
};
use ovrcr::session::{SessionId, SessionSummary};
use serde_json::json;
use std::io::Write;

use super::args::*;
use super::output::*;
use super::{
    AppResult, RuntimeError, inspect, request_started, request_without_start, unexpected_response,
};

pub(super) fn run_project(command: ProjectCommand, json_output: bool) -> AppResult<()> {
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
            mutate_without_start(Request::RemoveProject { name }, json_output)
        }
    }
}

pub(super) fn run_workspace(command: WorkspaceCommand, json_output: bool) -> AppResult<()> {
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
            mutate_without_start(Request::RemoveWorkspace { project, name }, json_output)
        }
    }
}

pub(super) fn run_terminal(command: TerminalCommand, json_output: bool) -> AppResult<()> {
    match command {
        TerminalCommand::Create(args) => create_terminal(args, json_output),
        TerminalCommand::Rename {
            id,
            title,
            automatic: _,
        } => mutate_without_start(
            Request::SetSessionTitle {
                session: SessionId(id),
                title,
            },
            json_output,
        ),
        TerminalCommand::Reopen { id, ack_stopped } => {
            reopen_terminal(id, ack_stopped, json_output)
        }
        TerminalCommand::AcknowledgeStopped { id } => acknowledge_stopped(id, json_output),
        TerminalCommand::MarkReviewed { id, expected } => mutate_without_start(
            Request::MarkReviewed {
                session: SessionId(id),
                expected,
            },
            json_output,
        ),
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
        TerminalCommand::Close { id } => {
            let session = inventory_session(id)?;
            mutate_without_start(
                Request::CloseTerminal {
                    session: SessionId(id),
                    expected_run: session.run,
                },
                json_output,
            )
        }
        TerminalCommand::Kill { id } => mutate_without_start(
            Request::KillSession {
                session: SessionId(id),
            },
            json_output,
        ),
        TerminalCommand::Remove { id } => mutate_without_start(
            Request::RemoveSession {
                session: SessionId(id),
            },
            json_output,
        ),
    }
}

pub(super) fn create_terminal(args: NewArgs, json_output: bool) -> AppResult<()> {
    let argv = if args.argv.is_empty() {
        vec![
            std::env::var_os("SHELL")
                .context("SHELL is unset and no command was provided")
                .map_err(RuntimeError::internal)?,
        ]
    } else {
        args.argv
    };
    let kind = session_kind_from_argv(&argv);
    let response = request_started(Request::CreateSession(CreateSessionRequest {
        project: args.project,
        workspace: args.workspace,
        name: args.name,
        label: args.label,
        argv,
        kind,
    }))?;
    print_created_terminal(response, json_output)
}

fn session_kind_from_argv(argv: &[std::ffi::OsString]) -> SessionKind {
    if let Some(kind) = managed_wrapper_kind(argv) {
        return kind;
    }
    let Some(name) = argv
        .first()
        .and_then(|exe| std::path::Path::new(exe).file_name())
        .and_then(|name| name.to_str())
    else {
        return SessionKind::Terminal;
    };
    AgentProvider::from_name(name)
        .map(|provider| SessionKind::Agent {
            name: provider.name().to_string(),
        })
        .unwrap_or(SessionKind::Terminal)
}

fn managed_wrapper_kind(argv: &[std::ffi::OsString]) -> Option<SessionKind> {
    let cli = Cli::try_parse_from(argv).ok()?;
    match cli.command {
        Some(Command::Agent {
            command:
                AgentCommand::Run {
                    provider,
                    legacy_provider,
                    ..
                },
        }) => {
            let name = provider.or(legacy_provider)?;
            AgentProvider::from_name(&name).map(|provider| SessionKind::Agent {
                name: provider.name().to_string(),
            })
        }
        _ => None,
    }
}

fn reopen_terminal(id: u64, acknowledge_stopped: bool, json_output: bool) -> AppResult<()> {
    let session = inventory_session(id)?;
    print_created_terminal(
        request_started(Request::ReopenSession {
            session: SessionId(id),
            expected_run: session.run,
            acknowledge_stopped,
        })?,
        json_output,
    )
}

fn acknowledge_stopped(id: u64, json_output: bool) -> AppResult<()> {
    let session = inventory_session(id)?;
    mutate_started(
        Request::AcknowledgeSessionStopped {
            session: SessionId(id),
            expected_run: session.run,
        },
        json_output,
    )
}

fn inventory_session(id: u64) -> AppResult<SessionSummary> {
    let (_, sessions) = inspect()?;
    sessions
        .into_iter()
        .find(|session| session.id == SessionId(id))
        .ok_or_else(|| RuntimeError::new(ErrorCode::NotFound, format!("session not found: {id}")))
}

fn print_created_terminal(response: Response, json_output: bool) -> AppResult<()> {
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

pub(super) fn inspect_session_context(id: u64) -> AppResult<()> {
    let session = listed_session(id)?;
    let now_unix_ms = now_unix_ms();
    let exited = !session.phase.is_live();
    print_json(&json!({
        "session": id,
        "context_usage": session.context_usage,
        "stale": session
            .context_usage
            .as_ref()
            .map(|sample| freshness::is_stale(sample.received_unix_ms, now_unix_ms, exited)),
    }))
}

pub(super) fn inspect_session_usage(id: u64) -> AppResult<()> {
    let session = listed_session(id)?;
    print_json(&json!({
        "session": id,
        "agent": session.agent,
        "agent_epoch": session.agent_epoch,
        "reporting_unavailable": reporting_unavailable(&session),
        "measurement_age_ms": measurement_age_ms(&session, now_unix_ms()),
    }))
}

fn listed_session(id: u64) -> AppResult<SessionSummary> {
    let response = request_without_start(Request::List)?;
    let Response::Hierarchy(snapshot) = response else {
        return Err(unexpected_response(response));
    };
    snapshot
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .find(|session| session.id == SessionId(id))
        .ok_or_else(|| RuntimeError::new(ErrorCode::NotFound, format!("session not found: {id}")))
}

#[cfg(test)]
mod session_kind_tests {
    use super::*;
    use std::ffi::OsString;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn managed_wrapper_provider_flag_is_agent() {
        assert_eq!(
            session_kind_from_argv(&argv(&[
                "ovrcr",
                "agent",
                "run",
                "--provider",
                "claude",
                "--",
                "claude"
            ])),
            SessionKind::Agent {
                name: "claude".into()
            }
        );
    }

    #[test]
    fn managed_wrapper_positional_provider_is_agent() {
        assert_eq!(
            session_kind_from_argv(&argv(&[
                "/usr/local/bin/ovrcr",
                "agent",
                "run",
                "pi",
                "--",
                "/usr/bin/pi"
            ])),
            SessionKind::Agent { name: "pi".into() }
        );
    }

    #[test]
    fn managed_wrapper_global_flag_before_agent_stays_agent() {
        assert_eq!(
            session_kind_from_argv(&argv(&[
                "ovrcr",
                "--json",
                "agent",
                "run",
                "--provider",
                "codex",
                "--",
                "codex"
            ])),
            SessionKind::Agent {
                name: "codex".into()
            }
        );
    }

    #[test]
    fn direct_known_executable_is_agent() {
        assert_eq!(
            session_kind_from_argv(&argv(&["claude"])),
            SessionKind::Agent {
                name: "claude".into()
            }
        );
    }

    #[test]
    fn shell_wrapping_agent_run_stays_terminal() {
        assert_eq!(
            session_kind_from_argv(&argv(&[
                "/bin/sh",
                "-c",
                "ovrcr agent run --provider claude -- claude"
            ])),
            SessionKind::Terminal
        );
    }
}

use ovrcr::config::{ProjectRecord, WorkspaceRecord};
use ovrcr::context::context_is_stale;
use ovrcr::protocol::Response;
use ovrcr::session::{AgentActivity, SessionPhase, SessionSummary};
use serde_json::{Value, json};
use std::path::Path;

use super::{AppResult, RuntimeError, request_started, request_without_start, unexpected_response};

pub(super) fn mutate_started(
    request: ovrcr::protocol::Request,
    json_output: bool,
) -> AppResult<()> {
    print_mutation(request_started(request)?, json_output)
}

pub(super) fn mutate_without_start(
    request: ovrcr::protocol::Request,
    json_output: bool,
) -> AppResult<()> {
    print_mutation(request_without_start(request)?, json_output)
}

pub(super) fn print_mutation(response: Response, json_output: bool) -> AppResult<()> {
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

pub(super) fn project_value(project: &ProjectRecord) -> Value {
    let workspace_count = project.workspaces.len();
    json!({
        "name": project.name,
        "repo": path_text(&project.repo),
        "workspace_root": path_text(&project.workspace_root),
        "workspace_count": workspace_count,
    })
}

pub(super) fn workspace_value(
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

pub(super) fn terminal_value(session: &SessionSummary, now_unix_ms: u64) -> Value {
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

pub(super) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn print_values(
    values: Vec<Value>,
    json_output: bool,
    print_text: fn(&Value),
) -> AppResult<()> {
    if json_output {
        print_json(&Value::Array(values))
    } else {
        for value in &values {
            print_text(value);
        }
        Ok(())
    }
}

pub(super) fn print_value(
    value: Value,
    json_output: bool,
    print_text: fn(&Value),
) -> AppResult<()> {
    if json_output {
        print_json(&value)
    } else {
        print_text(&value);
        Ok(())
    }
}

pub(super) fn print_project_row(value: &Value) {
    println!(
        "{}\t{}\t{}\t{}",
        value["name"].as_str().unwrap_or_default(),
        value["repo"].as_str().unwrap_or_default(),
        value["workspace_root"].as_str().unwrap_or_default(),
        value["workspace_count"]
    );
}

pub(super) fn print_project(value: &Value) {
    println!(
        "name\t{}\nrepo\t{}\nworkspace_root\t{}\nworkspace_count\t{}",
        value["name"].as_str().unwrap_or_default(),
        value["repo"].as_str().unwrap_or_default(),
        value["workspace_root"].as_str().unwrap_or_default(),
        value["workspace_count"]
    );
}

pub(super) fn print_workspace_row(value: &Value) {
    println!(
        "{}\t{}\t{}\t{}\t{}",
        value["project"].as_str().unwrap_or_default(),
        value["name"].as_str().unwrap_or_default(),
        value["path"].as_str().unwrap_or_default(),
        value["branch"].as_str().unwrap_or_default(),
        value["terminal_count"]
    );
}

pub(super) fn print_workspace(value: &Value) {
    println!(
        "project\t{}\nname\t{}\npath\t{}\nbranch\t{}\nterminal_count\t{}",
        value["project"].as_str().unwrap_or_default(),
        value["name"].as_str().unwrap_or_default(),
        value["path"].as_str().unwrap_or_default(),
        value["branch"].as_str().unwrap_or_default(),
        value["terminal_count"]
    );
}

pub(super) fn print_terminal_row(value: &Value) {
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

pub(super) fn print_json(value: &Value) -> AppResult<()> {
    println!(
        "{}",
        serde_json::to_string(value).map_err(RuntimeError::internal)?
    );
    Ok(())
}

pub(super) fn print_runtime_error(error: &RuntimeError, json_output: bool) {
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

pub(super) fn print_legacy_response(response: Response, json_output: bool) -> AppResult<()> {
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

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

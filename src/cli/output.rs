use ovrcr::config::{ProjectRecord, WorkspaceRecord};
use ovrcr::freshness;
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
        .filter(|session| {
            !session.archived && session.project == project && session.workspace == workspace.name
        })
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
        SessionPhase::Stopped => ("stopped", Value::Null, Value::Null),
        SessionPhase::Interrupted => ("interrupted", Value::Null, Value::Null),
    };
    let kind = match &session.kind {
        ovrcr::protocol::SessionKind::Terminal => json!("terminal"),
        ovrcr::protocol::SessionKind::Agent { name } => json!({"agent": name}),
    };
    json!({
        "id": session.id.0,
        "archived": session.archived,
        "run": session.run.0,
        "kind": kind,
        "project": session.project,
        "workspace": session.workspace,
        "name": session.name,
        "title": session.title,
        "display_name": session.display_name(),
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
            AgentActivity::ResponseReady => "response_ready",
        },
        "exit_code": exit_code,
        "exit_signal": exit_signal,
        "agent": session.agent,
        "agent_epoch": session.agent_epoch,
        "unread": session.unread,
        "recovery": session.recovery,
        "reporting_unavailable": reporting_unavailable(session),
        "measurement_age_ms": measurement_age_ms(session, now_unix_ms),
        "context_usage": session.context_usage,
        "context_stale": session.context_usage.as_ref().map(|sample| {
            freshness::is_stale(
                sample.received_unix_ms,
                now_unix_ms,
                !session.phase.is_live(),
            )
        }),
    })
}

pub(super) fn reporting_unavailable(session: &SessionSummary) -> Option<bool> {
    if session.resume_reporting_limitation().is_some() {
        return Some(true);
    }
    session.agent.as_ref().map(|agent| {
        agent.health.state == ovrcr::protocol::ReporterHealth::Unavailable
            || !session.phase.is_live()
    })
}

pub(super) fn measurement_age_ms(session: &SessionSummary, now: u64) -> Value {
    session
        .agent
        .as_ref()
        .and_then(|agent| agent.metrics.as_ref())
        .map_or(Value::Null, |metrics| {
            json!({
                "context": freshness::age_ms(metrics.context_received_unix_ms, now),
                "usage": freshness::age_ms(metrics.usage_received_unix_ms, now),
                "cost": freshness::age_ms(metrics.cost_received_unix_ms, now),
            })
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
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        value["id"],
        value["run"],
        json_scalar(&value["kind"]),
        value["project"].as_str().unwrap_or_default(),
        value["workspace"].as_str().unwrap_or_default(),
        value["display_name"].as_str().unwrap_or_default(),
        value["label"].as_str().unwrap_or_default(),
        json_scalar(&value["pid"]),
        json_scalar(&value["started_unix_ms"]),
        value["phase"].as_str().unwrap_or_default(),
        value["activity"].as_str().unwrap_or_default(),
        json_scalar(&value["exit_code"]),
        json_scalar(&value["exit_signal"]),
        if value["unread"].is_object() {
            "unread"
        } else {
            ""
        },
        json_scalar(&value["recovery"]),
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
                            SessionPhase::Stopped => "stopped",
                            SessionPhase::Interrupted => "interrupted",
                        };
                        println!(
                            "    session {} {} {phase}",
                            session.id.0,
                            session.display_name()
                        );
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

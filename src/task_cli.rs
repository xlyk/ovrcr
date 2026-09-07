use crate::config::RegistryPath;
use crate::protocol::{ClientMessage, Request, Response, ServerMessage, read_frame, write_frame};
use crate::server::{ServerPaths, connect_if_running, connect_or_start};
use crate::task_manager::{TaskManager, TaskRequest, TaskResponse};
use crate::tasks::*;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

pub use ovrcr_tui::{event_text, parse_duration};

#[derive(Args)]
pub struct TaskArgs {
    #[command(subcommand)]
    pub command: TaskCommand,
}
#[derive(Subcommand)]
pub enum TaskCommand {
    Create {
        name: String,
        #[command(flatten)]
        fields: TaskFields,
    },
    Update {
        id: u64,
        #[arg(long)]
        name: Option<String>,
        #[command(flatten)]
        fields: TaskFields,
    },
    List,
    Get {
        id: u64,
    },
    Pause {
        id: u64,
    },
    Resume {
        id: u64,
    },
    Delete {
        id: u64,
    },
    Run {
        id: u64,
    },
    Concurrency {
        limit: Option<usize>,
    },
}
#[derive(Args, Default)]
pub struct TaskFields {
    #[arg(long, conflicts_with = "prompt_file")]
    pub prompt: Option<String>,
    #[arg(long)]
    pub prompt_file: Option<PathBuf>,
    #[arg(long,conflicts_with_all=["project","remote","branch"])]
    pub scratch: bool,
    #[arg(long,requires_all=["remote","branch"])]
    pub project: Option<String>,
    #[arg(long,requires_all=["project","branch"])]
    pub remote: Option<String>,
    #[arg(long,requires_all=["project","remote"])]
    pub branch: Option<String>,
    #[arg(long,conflicts_with_all=["every","at"],requires="timezone")]
    pub cron: Option<String>,
    #[arg(long, requires = "cron")]
    pub timezone: Option<String>,
    #[arg(long, conflicts_with = "at")]
    pub every: Option<String>,
    #[arg(long)]
    pub at: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub thinking: Option<String>,
    #[arg(long)]
    pub timeout: Option<String>,
}
#[derive(Args)]
pub struct RunArgs {
    #[command(subcommand)]
    pub command: RunCommand,
}
#[derive(Subcommand)]
pub enum RunCommand {
    List {
        #[arg(long)]
        task: Option<u64>,
    },
    Get {
        id: u64,
    },
    Logs {
        id: u64,
        #[arg(long)]
        follow: bool,
    },
    Cancel {
        id: u64,
    },
    Clean {
        id: u64,
        #[arg(long)]
        yes: bool,
    },
}
impl TaskFields {
    pub fn apply(self, name: String, previous: Option<TaskSpec>) -> Result<TaskSpec> {
        let prompt = match (self.prompt, self.prompt_file) {
            (Some(text), None) => Some(text),
            (None, Some(path)) => Some(std::fs::read_to_string(path).context("read prompt file")?),
            (None, None) => None,
            _ => bail!("choose --prompt or --prompt-file"),
        };
        let target = if self.scratch {
            Some(TaskTarget::Scratch)
        } else if let Some(project) = self.project {
            Some(TaskTarget::Git {
                project,
                remote: self.remote.context("--remote is required")?,
                branch: self.branch.context("--branch is required")?,
            })
        } else {
            None
        };
        let schedule = if let Some(expression) = self.cron {
            Some(Schedule::Cron {
                expression,
                timezone: self.timezone.context("--timezone is required")?,
            })
        } else if let Some(duration) = self.every {
            Some(Schedule::Interval {
                seconds: parse_duration(&duration)?,
            })
        } else if let Some(at) = self.at {
            Some(Schedule::Once {
                at: chrono::DateTime::parse_from_rfc3339(&at)
                    .context("--at requires RFC3339 timestamp with offset")?
                    .timestamp(),
            })
        } else {
            None
        };
        let spec = TaskSpec {
            name,
            prompt: prompt
                .or_else(|| previous.as_ref().map(|p| p.prompt.clone()))
                .context("--prompt or --prompt-file is required")?,
            target: target
                .or_else(|| previous.as_ref().map(|p| p.target.clone()))
                .context("--scratch or --project/--remote/--branch is required")?,
            schedule: schedule
                .or_else(|| previous.as_ref().map(|p| p.schedule.clone()))
                .context("--cron, --every, or --at is required")?,
            model: self
                .model
                .or_else(|| previous.as_ref().map(|p| p.model.clone()))
                .context("--model PROVIDER/MODEL is required")?,
            thinking: self
                .thinking
                .or_else(|| previous.as_ref().map(|p| p.thinking.clone()))
                .unwrap_or_else(|| "off".into()),
            timeout_seconds: match self.timeout {
                Some(value) => parse_duration(&value)?,
                None => previous.as_ref().map_or(3600, |p| p.timeout_seconds),
            },
        };
        spec.validate()?;
        Ok(spec)
    }
}
pub fn run_task(command: TaskCommand, json_output: bool) -> Result<()> {
    let request = match command {
        TaskCommand::Create { name, fields } => TaskRequest::Create(fields.apply(name, None)?),
        TaskCommand::Update { id, name, fields } => {
            let TaskResponse::Task(task) = request(TaskRequest::GetTask(TaskId(id)))? else {
                bail!("unexpected task response");
            };
            TaskRequest::Update {
                id: TaskId(id),
                spec: fields.apply(
                    name.unwrap_or_else(|| task.spec.name.clone()),
                    Some(task.spec),
                )?,
            }
        }
        TaskCommand::List => TaskRequest::ListTasks,
        TaskCommand::Get { id } => TaskRequest::GetTask(TaskId(id)),
        TaskCommand::Pause { id } => TaskRequest::Pause(TaskId(id)),
        TaskCommand::Resume { id } => TaskRequest::Resume(TaskId(id)),
        TaskCommand::Delete { id } => TaskRequest::Delete(TaskId(id)),
        TaskCommand::Run { id } => TaskRequest::Enqueue(TaskId(id)),
        TaskCommand::Concurrency { limit } => TaskRequest::Concurrency(limit),
    };
    print_response(request_response(request)?, json_output)
}
pub fn run_run(command: RunCommand, json_output: bool) -> Result<()> {
    match command {
        RunCommand::List { task } => print_response(
            request(TaskRequest::ListRuns(task.map(TaskId)))?,
            json_output,
        ),
        RunCommand::Get { id } => {
            print_response(request(TaskRequest::GetRun(RunId(id)))?, json_output)
        }
        RunCommand::Cancel { id } => {
            print_response(request(TaskRequest::Cancel(RunId(id)))?, json_output)
        }
        RunCommand::Clean { id, yes } => print_response(
            request(TaskRequest::Clean {
                id: RunId(id),
                confirmed: yes,
            })?,
            json_output,
        ),
        RunCommand::Logs { id, follow } => follow_logs(RunId(id), follow, json_output),
    }
}
fn request_response(req: TaskRequest) -> Result<TaskResponse> {
    request(req)
}
pub fn request(req: TaskRequest) -> Result<TaskResponse> {
    match request_raw(req.clone())? {
        TaskResponse::TasksPage {
            mut items,
            mut next_offset,
        } => {
            let mut previous = match req {
                TaskRequest::PageTasks { offset } => offset,
                _ => 0,
            };
            while let Some(offset) = next_offset {
                if offset <= previous {
                    bail!("task list page cursor did not advance");
                }
                let TaskResponse::TasksPage {
                    items: page,
                    next_offset: next,
                } = request_raw(TaskRequest::PageTasks { offset })?
                else {
                    bail!("unexpected task list page response");
                };
                items.extend(page);
                next_offset = next;
                previous = offset;
            }
            Ok(TaskResponse::Tasks(items))
        }
        TaskResponse::RunsPage {
            mut items,
            mut next_offset,
        } => {
            let (task, mut previous) = match req {
                TaskRequest::ListRuns(task) => (task, 0),
                TaskRequest::PageRuns { task, offset } => (task, offset),
                _ => bail!("unexpected run list page response"),
            };
            while let Some(offset) = next_offset {
                if offset == 0 || (previous != 0 && offset >= previous) {
                    bail!("run list page cursor did not advance");
                }
                let TaskResponse::RunsPage {
                    items: page,
                    next_offset: next,
                } = request_raw(TaskRequest::PageRuns { task, offset })?
                else {
                    bail!("unexpected run list page response");
                };
                items.extend(page);
                next_offset = next;
                previous = offset;
            }
            Ok(TaskResponse::Runs(items))
        }
        response => Ok(response),
    }
}
fn request_raw(req: TaskRequest) -> Result<TaskResponse> {
    let paths = ServerPaths::resolve()?;
    let mut connection = if req.is_read_only() {
        match connect_if_running(&paths)? {
            Some(stream) => stream,
            None => return TaskManager::inspect_offline(&RegistryPath::resolve()?.0, req),
        }
    } else {
        connect_or_start(&paths)?
    };
    connection.set_read_timeout(Some(Duration::from_secs(30)))?;
    connection.set_write_timeout(Some(Duration::from_secs(30)))?;
    write_frame(
        &mut connection,
        &ClientMessage {
            request_id: 1,
            request: Request::Task(Box::new(req)),
        },
    )?;
    match read_frame::<ServerMessage>(&mut connection)? {
        ServerMessage::Response {
            response: Response::Task(value),
            ..
        } => Ok(*value),
        ServerMessage::Response {
            response: Response::Error { code, message },
            ..
        } => bail!("{code:?}: {message}"),
        _ => bail!("unexpected task response; update and restart the OVRCR server"),
    }
}
fn print_response(response: TaskResponse, json_output: bool) -> Result<()> {
    let value = match response {
        TaskResponse::Tasks(tasks) => serde_json::to_value(tasks)?,
        TaskResponse::Task(task) => {
            let mut value = serde_json::to_value(&task)?;
            let mut at = task.next_due_at;
            let mut upcoming = Vec::new();
            for _ in 0..5 {
                let Some(time) = at else { break };
                upcoming.push(time);
                at = match &task.spec.schedule {
                    Schedule::Once { .. } => None,
                    s => s.next_after(time)?,
                };
            }
            value["upcoming"] = json!(upcoming);
            value
        }
        TaskResponse::Runs(runs) => serde_json::to_value(runs)?,
        TaskResponse::Run(run) => serde_json::to_value(run)?,
        TaskResponse::Concurrency(n) => json!({"max_concurrent":n}),
        TaskResponse::Ok => json!({"ok":true}),
        TaskResponse::Log { .. } => bail!("unexpected log response"),
        TaskResponse::TasksPage { .. } | TaskResponse::RunsPage { .. } => {
            bail!("unexpected uncollected list page")
        }
    };
    if json_output {
        println!("{}", serde_json::to_string(&value)?);
    } else if let Value::Array(rows) = value {
        for row in rows {
            print_row(&row);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}
fn print_row(value: &Value) {
    let id = value["id"].as_u64().unwrap_or(0);
    let name = value["spec"]["name"].as_str().unwrap_or("");
    let status = value
        .get("status")
        .map(|v| v.as_str().unwrap_or("").to_owned())
        .unwrap_or_else(|| {
            if value["enabled"] == false {
                "paused".into()
            } else {
                "enabled".into()
            }
        });
    println!("{id}\t{name}\t{status}");
}
fn follow_logs(id: RunId, follow: bool, json_output: bool) -> Result<()> {
    let mut offset = 0;
    let mut pending = Vec::new();
    loop {
        let TaskResponse::Log {
            bytes,
            next_offset,
            terminal,
        } = request(TaskRequest::ReadLog {
            id,
            offset,
            max_bytes: 64 * 1024,
        })?
        else {
            bail!("unexpected logs response");
        };
        let empty = bytes.is_empty();
        offset = next_offset;
        if json_output {
            io::stdout().write_all(&bytes)?;
        } else {
            pending.extend_from_slice(&bytes);
            while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                let line = pending.drain(..=end).collect::<Vec<_>>();
                if let Ok(event) = serde_json::from_slice::<Value>(&line) {
                    print!("{}", event_text(&event));
                }
            }
        }
        io::stdout().flush()?;
        if empty && (!follow || terminal) {
            break;
        }
        if empty {
            if connect_if_running(&ServerPaths::resolve()?)?.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    Ok(())
}

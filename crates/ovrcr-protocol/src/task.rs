use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::Cron;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TaskId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RunId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Schedule {
    Cron {
        expression: String,
        timezone: String,
    },
    Interval {
        seconds: u64,
    },
    Once {
        at: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskTarget {
    Git {
        project: String,
        remote: String,
        branch: String,
    },
    Scratch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub name: String,
    pub prompt: String,
    pub target: TaskTarget,
    pub schedule: Schedule,
    pub model: String,
    pub thinking: String,
    pub timeout_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub spec: TaskSpec,
    pub enabled: bool,
    pub next_due_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunTrigger {
    Scheduled,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Queued,
    Preparing,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
    CleanupFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub task_id: TaskId,
    pub spec: TaskSpec,
    pub trigger: RunTrigger,
    pub scheduled_at: i64,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub status: RunStatus,
    pub directory: Option<PathBuf>,
    pub workspace: Option<String>,
    pub base_commit: Option<String>,
    pub pi_version: Option<String>,
    pub session_file: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskRequest {
    ListTasks,
    PageTasks {
        offset: usize,
    },
    PageRuns {
        task: Option<TaskId>,
        offset: usize,
    },
    GetTask(TaskId),
    Create(TaskSpec),
    Update {
        id: TaskId,
        spec: TaskSpec,
    },
    Pause(TaskId),
    Resume(TaskId),
    Delete(TaskId),
    Enqueue(TaskId),
    Concurrency(Option<usize>),
    ListRuns(Option<TaskId>),
    GetRun(RunId),
    ReadLog {
        id: RunId,
        offset: u64,
        max_bytes: usize,
    },
    Cancel(RunId),
    Clean {
        id: RunId,
        confirmed: bool,
    },
}

impl TaskRequest {
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            Self::ListTasks
                | Self::PageTasks { .. }
                | Self::PageRuns { .. }
                | Self::GetTask(_)
                | Self::ListRuns(_)
                | Self::GetRun(_)
                | Self::ReadLog { .. }
                | Self::Concurrency(None)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskResponse {
    Tasks(Vec<Task>),
    TasksPage {
        items: Vec<Task>,
        next_offset: Option<usize>,
    },
    RunsPage {
        items: Vec<Run>,
        next_offset: Option<usize>,
    },
    Task(Task),
    Runs(Vec<Run>),
    Run(Run),
    Concurrency(usize),
    Log {
        bytes: Vec<u8>,
        next_offset: u64,
        terminal: bool,
    },
    Ok,
}

impl Schedule {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Cron {
                expression,
                timezone,
            } => {
                if expression.split_whitespace().count() != 5 {
                    bail!("cron expression must contain exactly five fields");
                }
                Cron::from_str(expression).context("parse cron expression")?;
                timezone.parse::<Tz>().context("parse cron timezone")?;
            }
            Self::Interval { seconds: 0 } => bail!("interval seconds must be greater than zero"),
            Self::Interval { seconds } => {
                i64::try_from(*seconds).context("interval seconds exceed timestamp range")?;
            }
            Self::Once { .. } => {}
        }
        Ok(())
    }

    pub fn next_after(&self, timestamp: i64) -> Result<Option<i64>> {
        self.validate()?;
        match self {
            Self::Cron {
                expression,
                timezone,
            } => {
                let cron = Cron::from_str(expression).context("parse cron expression")?;
                let timezone = timezone.parse::<Tz>().context("parse cron timezone")?;
                let start = DateTime::<Utc>::from_timestamp(timestamp, 0)
                    .context("timestamp is outside the supported range")?
                    .with_timezone(&timezone);
                Ok(Some(
                    cron.find_next_occurrence(&start, false)
                        .context("find next cron occurrence")?
                        .timestamp(),
                ))
            }
            Self::Interval { seconds } => {
                let seconds =
                    i64::try_from(*seconds).context("interval seconds exceed timestamp range")?;
                Ok(Some(
                    timestamp
                        .checked_add(seconds)
                        .context("next interval occurrence overflows timestamp")?,
                ))
            }
            Self::Once { at } => Ok((*at > timestamp).then_some(*at)),
        }
    }
}

impl TaskSpec {
    pub fn validate(&self) -> Result<()> {
        if bincode::serde::encode_to_vec(self, bincode::config::standard())?.len() > 256 * 1024 {
            bail!("task definition exceeds 256 KiB");
        }
        require_text(&self.name, "task name")?;
        require_text(&self.prompt, "task prompt")?;
        require_text(&self.model, "task model")?;
        let (provider, model) = self
            .model
            .split_once('/')
            .context("task model must be PROVIDER/MODEL")?;
        require_text(provider, "model provider")?;
        require_text(model, "model name")?;
        if !matches!(
            self.thinking.as_str(),
            "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
        ) {
            bail!("task thinking level must be off, minimal, low, medium, high, xhigh, or max");
        }
        if self.timeout_seconds == 0 {
            bail!("task timeout must be greater than zero");
        }
        if let TaskTarget::Git {
            project,
            remote,
            branch,
        } = &self.target
        {
            require_text(project, "Git project")?;
            require_text(remote, "Git remote")?;
            require_text(branch, "Git branch")?;
            if remote.starts_with('-') {
                bail!("Git remote must not start with '-'");
            }
            if branch.starts_with('-') {
                bail!("Git branch must not start with '-'");
            }
        }
        self.schedule.validate()
    }
}

fn require_text(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

impl RunStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::Failed
                | Self::Cancelled
                | Self::TimedOut
                | Self::Interrupted
                | Self::CleanupFailed
        )
    }

    pub fn occupies_slot(&self) -> bool {
        matches!(self, Self::Preparing | Self::Running | Self::Cancelling)
    }
}

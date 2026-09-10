use clap::{ArgGroup, Args, Parser, Subcommand};
use ovrcr::session::AgentActivity;
use std::ffi::OsString;
use std::path::PathBuf;

/// `ovrcr --version` output: the package version plus the wire protocol
/// version, so a client and a long-running server can be compared.
fn version_string() -> &'static str {
    Box::leak(
        format!(
            "{} (protocol {})",
            env!("CARGO_PKG_VERSION"),
            ovrcr::protocol::PROTOCOL_VERSION
        )
        .into_boxed_str(),
    )
}

#[derive(Parser)]
#[command(name = "ovrcr", version = version_string())]
pub(super) struct Cli {
    #[arg(long, global = true)]
    pub(super) json: bool,
    #[command(subcommand)]
    pub(super) command: Option<Command>,
}

#[derive(Subcommand)]
pub(super) enum Command {
    /// Run a native agent with invocation supervision.
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
    /// Run the server in the foreground (normally started on demand).
    Server,
    /// Manage scheduled Pi tasks.
    #[command(visible_alias = "tasks")]
    Task(Box<ovrcr::task_cli::TaskArgs>),
    /// Inspect and control task runs.
    #[command(visible_alias = "runs")]
    Run(ovrcr::task_cli::RunArgs),
    /// Install, start, stop, or remove the background service.
    Service(ovrcr::service::ServiceArgs),
    #[command(name = "__task-runner", hide = true)]
    TaskRunner {
        run_dir: PathBuf,
        pi_executable: PathBuf,
    },
    /// Register and inspect Git projects.
    #[command(visible_alias = "projects")]
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Create and remove worktree workspaces.
    #[command(visible_alias = "workspaces")]
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
    /// Create, read, send to, and close terminals.
    #[command(visible_alias = "terminals")]
    Terminal {
        #[command(subcommand)]
        command: TerminalCommand,
    },
    /// Start a session in a workspace (starts the server if needed).
    New(NewArgs),
    /// List projects, workspaces, and sessions.
    List,
    /// Stop a session's process group and keep its final screen.
    Kill { id: u64 },
    /// Stop a running session with SIGSTOP.
    Pause { id: u64 },
    /// Resume a paused session with SIGCONT.
    Resume { id: u64 },
    /// Inspect or remove a session record.
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Stop the server; refuses while sessions remain unless --kill is given.
    Shutdown {
        #[arg(long)]
        kill: bool,
    },
    /// Report agent activity from a provider hook (needs the hook environment).
    Report {
        #[command(subcommand)]
        command: ReportCommand,
    },
}

#[derive(Subcommand)]
pub(super) enum ReportCommand {
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
pub(super) enum ProjectCommand {
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
pub(super) enum WorkspaceCommand {
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
pub(super) enum TerminalCommand {
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
pub(super) struct WorkspaceCreateArgs {
    #[arg(long)]
    pub(super) project: String,
    #[arg(long)]
    pub(super) name: String,
    #[arg(long, conflicts_with = "branch", requires = "base")]
    pub(super) new_branch: Option<String>,
    #[arg(long, requires = "new_branch")]
    pub(super) base: Option<String>,
    #[arg(long, conflicts_with_all = ["new_branch", "base"])]
    pub(super) branch: Option<String>,
}

#[derive(Args)]
pub(super) struct NewArgs {
    #[arg(long)]
    pub(super) project: String,
    #[arg(long)]
    pub(super) workspace: String,
    #[arg(long)]
    pub(super) name: String,
    #[arg(long)]
    pub(super) label: Option<String>,
    #[arg(last = true)]
    pub(super) argv: Vec<OsString>,
}

#[derive(Subcommand)]
pub(super) enum SessionCommand {
    Context { id: u64 },
    Remove { id: u64 },
}

pub(super) fn resolve_cli_path(path: PathBuf) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

pub(super) fn parse_positive_usize(value: &str) -> std::result::Result<usize, String> {
    let value = value
        .parse::<usize>()
        .map_err(|_| "must be a positive integer".to_owned())?;
    if value == 0 {
        Err("must be greater than zero".to_owned())
    } else {
        Ok(value)
    }
}

pub(super) fn parse_activity_state(value: &str) -> std::result::Result<AgentActivity, String> {
    match value {
        "unknown" => Ok(AgentActivity::Unknown),
        "idle" => Ok(AgentActivity::Idle),
        "busy" => Ok(AgentActivity::Busy),
        "waiting-input" => Ok(AgentActivity::WaitingInput),
        "error" => Ok(AgentActivity::Error),
        _ => Err("must be one of: unknown, idle, busy, waiting-input, error".into()),
    }
}

#[derive(Subcommand)]
pub(super) enum AgentCommand {
    #[command(
        long_about = "Run a native command with invocation supervision. Initial conversation admission supports only interactive Claude Code 2.1.267 with a fresh startup. Eligible options: --model, --permission-mode, --agent, --agents, --settings, --setting-sources, --system-prompt, --append-system-prompt, --name/-n, --strict-mcp-config, --verbose, and permission bypass flags. One prompt is supported; use an explicit -- before a prompt matching a native subcommand. Unknown or ambiguous options, help/version, history selection, and other execution modes run with original argv and admission unavailable. Only an eligible invocation with successful reporting setup receives a supervisor-selected --session-id."
    )]
    Run {
        #[arg(long, value_parser = ["claude"])]
        provider: String,
        #[arg(last = true, required = true)]
        argv: Vec<OsString>,
    },
}

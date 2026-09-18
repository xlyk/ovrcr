use clap::{ArgGroup, Args, Parser, Subcommand};
use ovrcr::session::AgentActivity;
use std::ffi::OsString;
use std::path::PathBuf;

/// Providers accepted by `agent run|setup|doctor`, shared so help text and
/// value parsing cannot drift between the three subcommands.
pub(super) const PROVIDERS: [&str; 4] = ["claude", "codex", "pi", "omp"];

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
    #[command(name = "__agent-collector", hide = true)]
    AgentCollector,
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
        /// unknown, idle, busy, waiting-input, response-ready, or error.
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
    Codex {
        #[arg(long, required = true)]
        stdin: bool,
    },
    Pi {
        #[arg(long, required = true)]
        stdin: bool,
    },
    Omp {
        #[arg(long, required = true)]
        stdin: bool,
    },
    ClaudeContext {
        #[arg(long)]
        stdin_json: bool,
    },
    ClaudeStatusline {
        #[arg(long)]
        stdin_json: bool,
        #[arg(long)]
        render_command: Option<String>,
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
    /// Unregister an empty project; keep its repository and retained session context.
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
        #[command(flatten)]
        target: WorkspaceSelect,
    },
    /// Remove a worktree and archive stopped sessions. --force discards changes and acknowledges uncertain stopped rows; live sessions still block.
    #[command(visible_alias = "delete")]
    Remove {
        #[command(flatten)]
        target: WorkspaceSelect,
        /// Acknowledge uncertain stopped sessions and discard uncommitted changes. Live sessions still block.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum TerminalCommand {
    /// Mark one observed agent response reviewed. Changes unread only.
    MarkReviewed {
        id: u64,
        /// Exact non-null unread object from `terminal list --json`.
        #[arg(long, value_parser = parse_ready_observation)]
        expected: ovrcr::protocol::ReadyObservation,
    },
    Create(NewArgs),
    /// Pin a display title or restore application-controlled titles.
    #[command(group(ArgGroup::new("title_mode").required(true).args(["title", "automatic"])))]
    Rename {
        id: u64,
        #[arg(conflicts_with = "automatic")]
        title: Option<String>,
        #[arg(long)]
        automatic: bool,
    },
    /// Reopen a fresh shell or resume a certified Claude conversation without a new prompt. Confirm uncertain previous processes stopped with --ack-stopped.
    Reopen {
        id: u64,
        #[arg(long)]
        ack_stopped: bool,
    },
    /// Confirm old processes stopped without launching a session. Required after unverified or natural exits.
    AcknowledgeStopped {
        id: u64,
    },
    List {
        /// List archived records instead of active sessions.
        #[arg(long)]
        archived: bool,
        #[arg(long)]
        project: Option<String>,
        /// Current Git branch of the workspace. Full names such as feature/auth are valid.
        #[arg(long, requires = "project", conflicts_with = "path")]
        workspace: Option<String>,
        /// Worktree path when the branch is ambiguous or detached.
        #[arg(long, requires = "project", conflicts_with = "workspace")]
        path: Option<PathBuf>,
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
    /// Stop currently owned work and archive the session record.
    Close {
        id: u64,
    },
    /// Return an archived record to the active list without launching.
    Unarchive {
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
    /// Create a new local branch in a worktree. Full Git names such as feature/auth are valid.
    #[arg(long, conflicts_with = "branch", requires = "base")]
    pub(super) new_branch: Option<String>,
    #[arg(long, requires = "new_branch")]
    pub(super) base: Option<String>,
    /// Use an existing local branch. Full Git names such as feature/auth are valid.
    #[arg(long, conflicts_with_all = ["new_branch", "base"])]
    pub(super) branch: Option<String>,
}

#[derive(Args)]
#[command(group(ArgGroup::new("workspace_target").required(true).args(["branch", "path"])))]
pub(super) struct WorkspaceSelect {
    #[arg(long)]
    pub(super) project: String,
    /// Current checkout branch (full Git name; slashes are valid).
    #[arg(long)]
    pub(super) branch: Option<String>,
    /// Worktree path when the branch is ambiguous or detached.
    #[arg(long)]
    pub(super) path: Option<PathBuf>,
}

#[derive(Args)]
#[command(group(ArgGroup::new("workspace_target").required(true).args(["workspace", "path"])))]
pub(super) struct NewArgs {
    #[arg(long)]
    pub(super) project: String,
    /// Current Git branch of the workspace. Full names such as feature/auth are valid.
    #[arg(long)]
    pub(super) workspace: Option<String>,
    /// Worktree path when the branch is ambiguous or detached.
    #[arg(long)]
    pub(super) path: Option<PathBuf>,
    /// Pin a name; omit to follow application titles automatically.
    #[arg(long, default_value = "")]
    pub(super) name: String,
    #[arg(long)]
    pub(super) label: Option<String>,
    #[arg(last = true)]
    pub(super) argv: Vec<OsString>,
}

#[derive(Subcommand)]
pub(super) enum SessionCommand {
    Context {
        id: u64,
    },
    /// Inspect managed activity, usage, cost, and source ages as JSON.
    Usage {
        id: u64,
    },
    Remove {
        id: u64,
    },
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
        "response-ready" => Ok(AgentActivity::ResponseReady),
        _ => {
            Err("must be one of: unknown, idle, busy, waiting-input, response-ready, error".into())
        }
    }
}

#[derive(Subcommand)]
pub(super) enum AgentCommand {
    /// Print a reviewed settings composition; never modify provider settings.
    Setup {
        #[arg(value_parser = PROVIDERS)]
        provider: String,
        #[arg(long, required = true)]
        print: bool,
        #[arg(long)]
        settings: Option<PathBuf>,
    },
    /// Inspect supported reporting prerequisites without starting a server.
    Doctor {
        #[arg(value_parser = PROVIDERS)]
        provider: String,
        #[arg(long)]
        settings: Option<PathBuf>,
        #[arg(long)]
        session: Option<u64>,
        #[arg(long)]
        executable: Option<OsString>,
    },
    #[command(
        long_about = "Run a native command with invocation supervision. Codex reporting supports stable interactive Codex CLI >=0.153.0 fresh launches with synchronous direct-exec hooks configured through report codex --stdin. Native argv is unchanged. Managed fresh root hooks retain exact conversation identity for terminal reopen; recovery executes codex resume UUID with no prompt and preserves the reference across restarts. Initial resume reporting stays unavailable and attachment is not confirmed; no old Ready or Unread is restored. Resume, fork, exec, remote, unknown versions/options and failed probes run with reporting unavailable. A root prompt establishes Busy; its Stop yields Ready once and Interrupt closes it without Ready. Missing ending hooks can leave Busy; no transcript or timeout implies completion. Initial conversation admission supports stable interactive Claude Code >=2.1.267 with a fresh startup or separate-token --resume UUID. 2.1.268 and later compatible patches also accept -r UUID. Resume values must be canonical lowercase UUIDv4. Eligible options: --model, --permission-mode, --agent, --agents, --settings, --setting-sources, --system-prompt, --append-system-prompt, --name/-n, --strict-mcp-config, --verbose, and permission bypass flags. One prompt is supported only for fresh launches; use an explicit -- before a prompt matching a native subcommand. Unknown or ambiguous options, help/version, history selection, and other execution modes run with original argv and admission unavailable. Eligible fresh invocations receive a supervisor-selected --session-id; resume argv remains unchanged. Pi and Oh My Pi: interactive terminal launches are supervised and load the owned OVRCR reporting extension beside the user's own extensions; help, version, print, RPC, JSON, ACP, export and package commands run native with reporting unavailable."
    )]
    Run {
        #[arg(value_parser = PROVIDERS, required_unless_present = "legacy_provider")]
        provider: Option<String>,
        #[arg(long = "provider", value_parser = PROVIDERS, conflicts_with = "provider")]
        legacy_provider: Option<String>,
        #[arg(last = true, required = true)]
        argv: Vec<OsString>,
    },
}

fn parse_ready_observation(value: &str) -> Result<ovrcr::protocol::ReadyObservation, String> {
    let expected: ovrcr::protocol::ReadyObservation =
        serde_json::from_str(value).map_err(|error| {
            format!("expected the unread JSON object from terminal list --json: {error}")
        })?;
    expected.validate().map_err(|error| error.to_string())?;
    Ok(expected)
}

#[cfg(test)]
mod launch_cli_tests {
    use super::*;

    #[test]
    fn terminal_creation_can_request_automatic_name() {
        assert!(
            Cli::try_parse_from([
                "ovrcr",
                "terminal",
                "create",
                "--project",
                "p",
                "--workspace",
                "w",
                "--",
                "/bin/sh",
            ])
            .is_ok()
        );
    }

    #[test]
    fn workspace_create_rejects_name_and_accepts_slash_branches() {
        assert!(
            Cli::try_parse_from([
                "ovrcr",
                "workspace",
                "create",
                "--project",
                "p",
                "--name",
                "work",
                "--branch",
                "feature/auth",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "ovrcr",
                "workspace",
                "create",
                "--project",
                "p",
                "--new-branch",
                "feature/auth",
                "--base",
                "main",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "ovrcr",
                "new",
                "--project",
                "p",
                "--workspace",
                "feature/auth",
                "--",
                "/bin/sh",
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "ovrcr",
                "workspace",
                "get",
                "--project",
                "p",
                "--branch",
                "feature/auth",
                "--path",
                "/tmp/work",
            ])
            .is_err()
        );
    }

    #[test]
    fn title_and_reopen_commands_parse_without_ambiguous_reset() {
        for args in [
            vec!["ovrcr", "terminal", "rename", "7", "Review login"],
            vec!["ovrcr", "terminal", "rename", "7", "--automatic"],
            vec!["ovrcr", "terminal", "reopen", "7"],
            vec!["ovrcr", "terminal", "reopen", "7", "--ack-stopped"],
            vec!["ovrcr", "terminal", "acknowledge-stopped", "7"],
        ] {
            assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
        }
        assert!(Cli::try_parse_from(["ovrcr", "terminal", "rename", "7"]).is_err());
        assert!(
            Cli::try_parse_from(["ovrcr", "terminal", "rename", "7", "Pinned", "--automatic",])
                .is_err()
        );
        assert!(Cli::try_parse_from(["ovrcr", "terminal", "relaunch", "7"]).is_err());
    }
}

use clap::{ArgGroup, Args, Parser, Subcommand};
use ovrcr::session::AgentActivity;
use std::ffi::OsString;
use std::path::PathBuf;

/// Providers accepted by `agent run|setup|doctor`, shared so help text and
/// value parsing cannot drift between the three subcommands.
pub(super) const PROVIDERS: [&str; 7] = [
    "claude",
    "codex",
    "pi",
    "omp",
    "grok",
    "hermes",
    "cursor-agent",
];

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

/// Edits go through the running Server, which validates, saves and
/// republishes; with no Server running the document is edited directly.
#[derive(Subcommand)]
pub(super) enum SettingsCommand {
    /// Set one setting. PATH is a setting path such as `quota.enabled`,
    /// `picker_roots[0]` (index len appends), `agents[1].argv` or
    /// `launch_choices.myproj.kind`. VALUE is TOML value text (`true`, `"kh/"`,
    /// `["claude", "--verbose"]`); a bare word is taken as is for a string or
    /// path setting.
    Set { path: String, value: String },
    /// Remove one setting, list element or launch choice from the document,
    /// so the setting returns to its default.
    Reset { path: String },
}

#[derive(Subcommand)]
pub(super) enum Command {
    /// Connect-only Bridge navigation and current-owner validation. Never starts a Server.
    Bridge {
        #[command(subcommand)]
        command: BridgeCommand,
    },
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
    /// Point existing managed reporter commands at this binary. `just run` calls this
    /// after installing ~/.local/bin/ovrcr. It does not add hooks or change trust.
    #[command(name = "__retarget-hooks", hide = true)]
    RetargetHooks {
        #[arg(long = "settings")]
        settings: Vec<PathBuf>,
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
    /// Create, read, send text or keys to, and close terminals.
    #[command(visible_alias = "terminals")]
    Terminal {
        #[command(subcommand)]
        command: TerminalCommand,
    },
    /// Start a session in a workspace (starts the server if needed).
    New(NewArgs),
    /// List projects, workspaces, and sessions.
    List,
    /// Show the settings document this instance reads: each setting's owner,
    /// effective value and source, then any findings. Exits 0 with findings.
    Settings {
        #[command(subcommand)]
        command: Option<SettingsCommand>,
    },
    /// Print the running Server's event ring, oldest first. Does not start a
    /// Server and does not read `events.jsonl`.
    Events {
        /// Keep the connection open and print events recorded after the snapshot.
        #[arg(long)]
        follow: bool,
    },
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
pub(super) enum BridgeCommand {
    /// Validate only the existing Server/current Dashboard owner; no setup or launch.
    Owner {
        #[arg(long, required = true)]
        stdin: bool,
    },
    Navigate {
        #[arg(long, required = true)]
        stdin: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum ReportCommand {
    /// Passive native Cursor startup identity hook.
    CursorAgent {
        #[arg(long, required = true)]
        stdin: bool,
    },
    /// Hermes shell hook. Reads the native payload and forwards identity,
    /// activity, approval and token fields only.
    Hermes {
        #[arg(long, required = true)]
        stdin: bool,
    },
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
    /// Set a display title or restore the original session name.
    #[command(group(ArgGroup::new("title_mode").required(true).args(["title", "reset"])))]
    Rename {
        id: u64,
        #[arg(conflicts_with = "reset")]
        title: Option<String>,
        /// Restore the original session name.
        #[arg(long, alias = "automatic")]
        reset: bool,
    },
    /// Reopen a terminal and execute the agent resume command, or open its native resume picker when no conversation is saved. Confirm uncertain previous processes stopped with --ack-stopped.
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
    /// Deliver one named key to the program, as a keyboard would. Refused while the Dashboard is focused on the session.
    #[command(
        after_help = "KEY is one character or a lowercase key name between colons, after optional ctrl-, alt-, shift- prefixes in that order: :j: :enter: :escape: :tab: :backspace: :space: :colon: :up: :down: :left: :right: :home: :end: :pageup: :pagedown: :insert: :delete: :f1:-:f12: :ctrl-c: :alt-down: :ctrl-shift-enter:. Shift applies only to named keys. Use `terminal send` to paste text."
    )]
    Keystroke {
        id: u64,
        #[arg(value_parser = parse_keystroke, allow_hyphen_values = true)]
        key: String,
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
    /// Set a stable name; omit to generate one from the workspace.
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

fn parse_keystroke(value: &str) -> std::result::Result<String, String> {
    ovrcr_terminal::key::parse_keystroke(value).map(|_| value.to_owned())
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
        long_about = "Run a native command with invocation supervision. Codex reporting supports interactive Codex CLI fresh launches with synchronous direct-exec hooks configured through report codex --stdin. Claude, Codex, Pi, and Oh My Pi append one default auto-trust flag when argv has no permission-mode, approve, auto-approve, approve-for-me, full-auto, or dangerously-* flag: --dangerously-skip-permissions, --approve-for-me, --approve, or --auto-approve. An existing flag in that family is left untouched. Grok, Hermes, and Cursor keep native argv unchanged. Managed fresh root hooks retain exact conversation identity for terminal reopen; recovery executes codex resume UUID with no prompt and preserves the reference across restarts. Exact `codex resume UUID` admits reporting after matching SessionStart(source=resume) Root identity; process creation alone does not confirm attachment and historical responses are not replayed as Ready. Fork, picker-without-UUID, exec, remote and unknown options run with reporting unavailable. A root prompt establishes Busy; its Stop yields Ready once and Interrupt closes it without Ready. Missing ending hooks can leave Busy; no transcript or timeout implies completion. Initial conversation admission supports interactive Claude Code with a fresh startup or a separate-token --resume UUID or -r UUID. No harness version is required or refused: admission is decided by argv, hooks and conversation identity. Resume values must be canonical lowercase UUIDv4. Eligible options: --model, --permission-mode, --agent, --agents, --settings, --setting-sources, --system-prompt, --append-system-prompt, --name/-n, --strict-mcp-config, --verbose, and permission bypass flags. One prompt is supported only for fresh launches; use an explicit -- before a prompt matching a native subcommand. Unknown or ambiguous options, help/version, history selection, and other execution modes run with admission unavailable. Eligible fresh invocations receive a supervisor-selected --session-id; resume argv remains unchanged. Pi and Oh My Pi: interactive terminal launches are supervised and load the owned OVRCR reporting extension beside the user's own extensions; help, version, print, RPC, JSON, ACP, export and package commands run native with reporting unavailable. Grok: a fresh interactive launch in the current directory receives a supervisor-selected --session-id and retains the updates.jsonl that Grok's session store keeps for it, as a title source only; nothing is reported and resume stays unavailable. Resume, continue, fork, explicit session IDs, headless output, another working directory or worktree, and subcommands run with original argv. Hermes: process supervision only, with native argv unchanged and no OVRCR hooks/settings. Activity, readiness, Input requests, metrics, generated titles and recovery are unavailable; the installed native command runs without a version gate. Use agent doctor hermes for filesystem discovery and capability guidance without executing Hermes or validating its version, authentication or model; see docs/hermes-harness.md for recorded acceptance and its limits. Cursor CLI: fresh interactive cursor-agent or explicit agent alias, with no args or --model VALUE, checks the installed CLI for local plugin support and loads a temporary passive plugin for validated startup identity only. Activity, Ready/Unread, Input, metrics, titles and recovery are unavailable. Missing local plugin support, unsupported options and resume/continue remain native unchanged with reporting unavailable."
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
            vec!["ovrcr", "terminal", "rename", "7", "--reset"],
            vec!["ovrcr", "terminal", "rename", "7", "--automatic"],
            vec!["ovrcr", "terminal", "reopen", "7"],
            vec!["ovrcr", "terminal", "reopen", "7", "--ack-stopped"],
            vec!["ovrcr", "terminal", "acknowledge-stopped", "7"],
        ] {
            assert!(Cli::try_parse_from(&args).is_ok(), "{args:?}");
        }
        assert!(Cli::try_parse_from(["ovrcr", "terminal", "rename", "7"]).is_err());
        assert!(
            Cli::try_parse_from(["ovrcr", "terminal", "rename", "7", "Pinned", "--reset"]).is_err()
        );
        assert!(
            Cli::try_parse_from(["ovrcr", "terminal", "rename", "7", "Pinned", "--automatic",])
                .is_err()
        );
        assert!(Cli::try_parse_from(["ovrcr", "terminal", "relaunch", "7"]).is_err());
    }
}

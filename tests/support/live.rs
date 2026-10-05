//! One live OVRCR server per fixture.
//!
//! Every `Live` owns a private temporary root and, inside it, the Git
//! repository, workspace root, instance directory and Unix socket the server under
//! test is given. Nothing is shared with the developer's own server, with the
//! other suites, or with a sibling fixture in the same binary.
//!
//! Two adapters sit at one seam — how the server is hosted:
//!
//! * [`Live::thread`] runs `run_server` on a thread of the test process, so
//!   a test can reach into server state and a panic carries the server down
//!   with it.
//! * [`Live::binary`] spawns the compiled `ovrcr` binary, so a test can prove
//!   things about a real process: signals, exit status, a CLI that talks to a
//!   server it did not start.
//!
//! Everything above the seam is shared: repository and registry construction,
//! the wait for the socket, control requests through
//! [`ovrcr::protocol::client`], "project and workspace ready", the process
//! groups the fixture owns, and the cleanup that runs from [`Drop`].

// Eight test binaries compile this module and each drives a different part
// of it, so what one suite never calls is not dead.
#![allow(dead_code)]

#[path = "claude_auth.rs"]
pub mod claude_auth;
#[path = "deadline.rs"]
mod deadline;

pub use deadline::wait_deadline;

use ovrcr::protocol::{BranchRequest, Request, Response, client, connect_server};
use ovrcr::server::ServerPaths;
#[cfg(feature = "acceptance-diagnostics")]
use ovrcr::server::ServerQueueDiagnostics;
#[cfg(not(feature = "acceptance-diagnostics"))]
use ovrcr::server::run_server;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Where the default instance directory sits under HOME (`directories::ProjectDirs`).
#[cfg(target_os = "macos")]
pub const DEFAULT_CONFIG_DIR: &str = "Library/Application Support/ovrcr";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_CONFIG_DIR: &str = ".config/ovrcr";

/// The project [`Live::ready`] registers.
pub const PROJECT: &str = "fixture";
/// The workspace [`Live::ready`] creates in [`PROJECT`].
pub const WORKSPACE: &str = "work";

/// The seam: who hosts the server this fixture drives.
enum Host {
    Idle,
    Thread(JoinHandle<()>),
    Binary(Child),
}

/// How a hosted server stopped. A test that waited for a clean stop needs to
/// tell "it is still running" from "it stopped, badly".
#[derive(Debug)]
pub enum Stop {
    /// The thread returned, or the child exited successfully.
    Finished,
    /// The child exited, unsuccessfully, with this status.
    Exited(ExitStatus),
    /// The server thread panicked. Its message, so the panic is not swallowed.
    Panicked(String),
    /// Still running when the budget ran out.
    TimedOut,
}

impl Stop {
    pub fn is_finished(&self) -> bool {
        matches!(self, Stop::Finished)
    }
}

pub struct Live {
    pub root: tempfile::TempDir,
    pub repo: PathBuf,
    pub workspace_root: PathBuf,
    /// Instance directory (`OVRCR_HOME`).
    pub config: PathBuf,
    pub socket: PathBuf,
    pub executable: PathBuf,
    /// The private HOME of an [`isolated_home`](Self::isolated_home) fixture.
    pub home: Option<PathBuf>,
    /// The `claude` this instance's Server asks for `claude auth status`,
    /// never the developer's: first on the binary adapter's PATH, and named by
    /// the thread adapter's settings document.
    pub claude: claude_auth::ClaudeAuth,
    host: Mutex<Host>,
    pgids: Mutex<Vec<libc::pid_t>>,
    ready: Mutex<Option<String>>,
    timeout: Option<Duration>,
    #[cfg(feature = "acceptance-diagnostics")]
    diagnostics: ServerQueueDiagnostics,
}

impl Live {
    /// A private root, repository, registry and socket path — no server yet.
    ///
    /// Tests that drive server start-up itself take this and start the server
    /// the way they mean to test it.
    pub fn idle() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        // Not `workspaces`: tests build their own paths under the root and
        // must be able to assert that theirs does not exist yet.
        let workspace_root = root.path().join("fixture-workspaces");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&workspace_root).unwrap();
        init_repo(&repo);
        // Instance directory (`OVRCR_HOME`). Empty until the server creates
        // `registry.sqlite3` and friends; no legacy `config.toml` sibling.
        let config = root.path().join("instance");
        std::fs::create_dir(&config).unwrap();
        let bin = root.path().join("claude-auth-fixture");
        std::fs::create_dir(&bin).unwrap();
        let claude = claude_auth::ClaudeAuth::install(&bin);
        Self {
            repo: repo.canonicalize().unwrap(),
            workspace_root: workspace_root.canonicalize().unwrap(),
            // The server makes this directory private; keeping the socket out
            // of the instance leaves the instance's own mode the test's business.
            socket: root.path().join("private").join("server.sock"),
            config,
            root,
            executable: PathBuf::from(env!("CARGO_BIN_EXE_ovrcr")),
            home: None,
            claude,
            host: Mutex::new(Host::Idle),
            pgids: Mutex::new(Vec::new()),
            ready: Mutex::new(None),
            timeout: None,
            #[cfg(feature = "acceptance-diagnostics")]
            diagnostics: ServerQueueDiagnostics::default(),
        }
    }

    /// A fresh install: like [`idle`](Self::idle), but every process of this
    /// instance runs with an empty private HOME and no `OVRCR_HOME`, so the
    /// default instance directory is the one under test. Nothing is written
    /// there; `config` is that directory (`OVRCR_HOME`).
    pub fn isolated_home() -> Self {
        let mut live = Self::idle();
        std::fs::remove_dir(&live.config).unwrap();
        let home = live.root.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let home = home.canonicalize().unwrap();
        live.config = home.join(DEFAULT_CONFIG_DIR);
        live.home = Some(home);
        live
    }

    /// An `ovrcr` command with this instance's environment.
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command
            // Account readers must never inherit the developer's profiles.
            // Explicit per-test child overrides are applied after this builder.
            .env("HOME", self.home.as_deref().unwrap_or(self.root.path()))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("XDG_STATE_HOME")
            .env_remove("XDG_CACHE_HOME")
            .env_remove("CODEX_HOME")
            .env_remove("GROK_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env("OVRCR_SOCKET", &self.socket)
            // The settings document is the one beside `config`, never the developer's.
            .env_remove("OVRCR_DASHBOARD_CONFIG")
            // The sessions a workspace opens run this shell. The developer's
            // own login shell and its rc files are not the test's subject.
            .env("SHELL", "/bin/sh")
            .env("PATH", self.path());
        match &self.home {
            None => command
                .env("OVRCR_HOME", &self.config)
                .env_remove("OVRCR_CONFIG"),
            Some(_) => command
                .env_remove("OVRCR_HOME")
                .env_remove("OVRCR_CONFIG"),
        };
        command
    }

    /// PATH with the fixture's `claude` first.
    pub fn path(&self) -> std::ffi::OsString {
        let mut paths = vec![self.claude.command.parent().unwrap().to_path_buf()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::join_paths(paths).unwrap()
    }

    /// Adapter: the server runs on a thread of this test process.
    pub fn thread() -> Self {
        let live = Self::idle();
        live.start();
        live
    }

    /// Adapter: the server runs as a spawned `ovrcr` child process.
    pub fn binary() -> Self {
        let live = Self::idle();
        live.start_binary();
        live.bounded_by(wait_deadline())
    }

    /// Start or restart the compiled server against this fixture's retained storage.
    pub fn start_binary(&self) {
        self.start_binary_env(&[]);
    }

    /// Explicit child-only environment for provider configuration restart cases.
    pub fn start_binary_env(&self, environment: &[(&str, &std::ffi::OsStr)]) {
        self.spawn_server(environment, Stdio::null());
    }

    /// Same as [`start_binary_env`](Self::start_binary_env), but keep the server's stderr.
    ///
    /// The default fixture discards it. A test that must read a log the server
    /// actually writes passes a file here.
    pub fn start_binary_logged(&self, environment: &[(&str, &std::ffi::OsStr)], log: &Path) {
        let file = std::fs::File::create(log).expect("create server log");
        self.spawn_server(environment, Stdio::from(file));
    }

    fn spawn_server(&self, environment: &[(&str, &std::ffi::OsStr)], stderr: Stdio) {
        assert!(!self.hosted(), "fixture already owns a running server");
        let child = self
            .command()
            .arg("server")
            .envs(environment.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr)
            .spawn()
            .expect("spawn isolated OVRCR server");
        *self.host.lock().unwrap() = Host::Binary(child);
        self.wait_socket();
    }

    /// Start the thread adapter under an [`idle`](Self::idle) fixture.
    pub fn start(&self) {
        assert!(
            self.home.is_none(),
            "a private HOME needs a real process: use start_binary"
        );
        // The thread shares the test's PATH, so name the fixture `claude`
        // unless the test wrote its own settings.
        let document = self.config.join("dashboard.toml");
        if !document.exists() {
            std::fs::write(
                &document,
                format!(
                    "agents = [{{ name = \"claude\", argv = [{}] }}]\n[quota]\nenabled = false\n",
                    toml::Value::String(self.claude.command.display().to_string())
                ),
            )
            .unwrap();
        }
        let paths = self.paths();
        let config = self.config.clone();
        #[cfg(feature = "acceptance-diagnostics")]
        let diagnostics = self.diagnostics.clone();
        #[cfg(feature = "acceptance-diagnostics")]
        let handle = thread::spawn(move || {
            ovrcr::server::run_server_with_diagnostics(paths, config, diagnostics).unwrap()
        });
        #[cfg(not(feature = "acceptance-diagnostics"))]
        let handle = thread::spawn(move || run_server(paths, config).unwrap());
        *self.host.lock().unwrap() = Host::Thread(handle);
        self.wait_socket();
    }

    /// Give every control request a deadline, for fixtures whose tests must
    /// fail rather than hang when the server stops answering.
    pub fn bounded(self) -> Self {
        self.bounded_by(Duration::from_secs(5))
    }

    fn bounded_by(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// The spawned server's process id, for fixtures that record what they own.
    pub fn server_pid(&self) -> Option<u32> {
        match &*self.host.lock().unwrap() {
            Host::Binary(child) => Some(child.id()),
            _ => None,
        }
    }

    /// The per-request deadline, if this fixture was made [`bounded`](Self::bounded).
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    pub fn paths(&self) -> ServerPaths {
        ServerPaths {
            socket: self.socket.clone(),
        }
    }

    #[cfg(feature = "acceptance-diagnostics")]
    pub fn diagnostics(&self) -> &ServerQueueDiagnostics {
        &self.diagnostics
    }

    /// One control request over a fresh connection.
    ///
    /// It blocks until the server answers unless this fixture was made
    /// [`bounded`](Self::bounded), which [`binary`](Self::binary) fixtures are.
    pub fn request(&self, request: Request) -> Response {
        let mut stream = connect_with(&self.socket, self.timeout).unwrap();
        client::request(&mut stream, 1, request).unwrap()
    }

    /// Register [`PROJECT`] and create [`WORKSPACE`] on a new `branch`.
    ///
    /// Idempotent, so a fixture can make itself ready lazily; the workspace
    /// exists on one branch, and asking for a second is a test's mistake
    /// rather than a silent no-op.
    pub fn ready(&self, branch: &str) {
        let mut ready = self.ready.lock().unwrap();
        if let Some(existing) = ready.as_deref() {
            assert_eq!(
                existing, branch,
                "the fixture workspace already exists on another branch"
            );
            return;
        }
        assert_eq!(
            self.request(Request::AddProject {
                name: PROJECT.into(),
                repo: self.repo.clone(),
                workspace_root: self.workspace_root.clone(),
            }),
            Response::Ok
        );
        self.clear_root_shell();
        assert_eq!(
            self.request(Request::CreateWorkspace {
                project: PROJECT.into(),
                id: WORKSPACE.into(),
                branch: BranchRequest::New {
                    branch: branch.into(),
                    base: "main".into(),
                },
            }),
            Response::Ok
        );
        // Default policy is default_branch_only, so feature workspaces no longer
        // receive an automatic local shell. Fixtures that need one create it
        // explicitly so capacity and CLI suites keep a live session to drive.
        let shell = std::env::var_os("SHELL")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        let Response::CreatedSession(_) = self.request(Request::CreateSession(
            ovrcr::protocol::CreateSessionRequest {
                project: PROJECT.into(),
                workspace: WORKSPACE.into(),
                name: "local".into(),
                label: None,
                argv: vec![shell],
                kind: ovrcr::protocol::SessionKind::Terminal,
            },
        )) else {
            panic!("feature workspace local shell");
        };
        *ready = Some(branch.to_owned());
    }

    /// Feature-workspace fixtures do not need the separately tested initial root shell.
    pub fn clear_root_shell(&self) {
        let Response::Hierarchy(hierarchy) = self.request(Request::List) else {
            panic!("expected hierarchy");
        };
        for session in hierarchy
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .filter(|workspace| workspace.path == self.repo)
            .flat_map(|workspace| workspace.sessions)
        {
            assert_eq!(
                self.request(Request::CloseTerminal {
                    session: session.id,
                    expected_run: session.run,
                }),
                Response::Ok
            );
            let Response::Inventory { sessions, .. } = self.request(Request::Inspect) else {
                panic!("expected inventory");
            };
            let archived = sessions.iter().find(|row| row.id == session.id).unwrap();
            assert_eq!(
                self.request(Request::DeleteArchivedSession {
                    session: archived.id,
                    expected_run: archived.run,
                }),
                Response::Ok
            );
        }
    }

    /// Record a process group this fixture must reap.
    ///
    /// A session leader owns its own group, so a group id the caller could not
    /// resolve means the fixture has lost track of a child it is responsible
    /// for: fail here rather than quietly own nothing.
    pub fn own_group(&self, pgid: libc::pid_t) {
        assert!(
            pgid > 1,
            "fixture child has no process group to own: {pgid}"
        );
        let mut pgids = self.pgids.lock().unwrap();
        if !pgids.contains(&pgid) {
            pgids.push(pgid);
        }
    }

    /// Forget a test-owned group only after its disappearance has been observed.
    pub fn forget_group(&self, pgid: libc::pid_t) {
        assert!(
            pgid > 1 && !group_exists(pgid),
            "owned process group still exists"
        );
        self.pgids.lock().unwrap().retain(|owned| *owned != pgid);
    }

    /// The process groups this fixture is responsible for reaping.
    pub fn owned_groups(&self) -> Vec<libc::pid_t> {
        self.pgids.lock().unwrap().clone()
    }

    /// The process groups of the sessions the server lists right now.
    ///
    /// A read, not a claim: the caller decides which of these it owns. A
    /// session that exits between the list and the lookup has no group left to
    /// reap, so it drops out here.
    pub fn session_groups(&self) -> Vec<libc::pid_t> {
        let Response::Hierarchy(snapshot) = self.request(Request::List) else {
            panic!("server list did not return a hierarchy");
        };
        snapshot
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .filter_map(|session| session.pid)
            .map(|pid| unsafe { libc::getpgid(pid as libc::pid_t) })
            .filter(|pgid| *pgid > 1)
            .collect()
    }

    /// Wait for the hosted server to finish, however it is hosted.
    pub fn join(&self) {
        let stop = self.join_within(Duration::from_secs(30));
        assert!(
            stop.is_finished(),
            "fixture server did not finish: {stop:?}"
        );
    }

    /// The same wait, bounded, reporting how the server stopped.
    pub fn join_within(&self, timeout: Duration) -> Stop {
        let deadline = Instant::now() + timeout;
        loop {
            // One lock per turn: the guard must be gone before the next wait.
            let mut host = self.host.lock().unwrap();
            let finished = match &mut *host {
                Host::Idle => true,
                Host::Thread(handle) => handle.is_finished(),
                Host::Binary(child) => matches!(child.try_wait(), Ok(Some(_))),
            };
            if finished {
                return match std::mem::replace(&mut *host, Host::Idle) {
                    Host::Idle => Stop::Finished,
                    Host::Thread(handle) => match handle.join() {
                        Ok(()) => Stop::Finished,
                        Err(panic) => Stop::Panicked(panic_message(panic)),
                    },
                    Host::Binary(mut child) => {
                        match child.wait().expect("wait for fixture server child") {
                            status if status.success() => Stop::Finished,
                            status => Stop::Exited(status),
                        }
                    }
                };
            }
            drop(host);
            if Instant::now() >= deadline {
                return Stop::TimedOut;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
    }

    fn wait_socket(&self) {
        assert!(
            wait_for_socket(&self.socket, wait_deadline()),
            "fixture server socket did not appear: {}",
            self.socket.display()
        );
    }

    /// Whether a server is still hosted here — no shutdown has completed yet.
    pub fn hosted(&self) -> bool {
        !matches!(&*self.host.lock().unwrap(), Host::Idle)
    }

    fn kill_owned_groups(&self) -> bool {
        let groups = self.owned_groups();
        let mut cleaned = true;
        for pgid in groups {
            if !group_exists(pgid) {
                continue;
            }
            if unsafe { libc::kill(-pgid, libc::SIGKILL) } != 0 && group_exists(pgid) {
                cleaned = false;
                continue;
            }
            if !wait_group_absent(pgid, Duration::from_secs(2)) {
                cleaned = false;
            }
        }
        cleaned
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let mut cleanup_failed = false;
        if self.hosted() {
            let shutdown = request_with_timeout(
                &self.socket,
                999,
                Request::Shutdown { kill: true },
                self.timeout.unwrap_or(Duration::from_secs(2)),
            );
            if shutdown != Some(Response::Ok) {
                eprintln!("live fixture shutdown response: {shutdown:?}");
                cleanup_failed = true;
            }
        }
        if !self.kill_owned_groups() {
            cleanup_failed = true;
        }
        if self.hosted() {
            let stop = self.join_within(Duration::from_secs(5));
            if !stop.is_finished() {
                eprintln!("live fixture server stop: {stop:?}");
                cleanup_failed = true;
            }
        }
        if let Host::Binary(child) = &mut *self.host.get_mut().unwrap() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        if !self
            .pgids
            .get_mut()
            .unwrap()
            .iter()
            .copied()
            .all(|pgid| !group_exists(pgid))
        {
            cleanup_failed = true;
        }
        if cleanup_failed {
            let kept = std::mem::replace(&mut self.root, tempfile::tempdir().unwrap()).keep();
            eprintln!(
                "live fixture cleanup incomplete; preserved {}",
                kept.display()
            );
        }
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panicked without a message".to_owned()
    }
}

fn connect_with(socket: &Path, timeout: Option<Duration>) -> anyhow::Result<UnixStream> {
    let stream = connect_server(socket)?;
    if let Some(timeout) = timeout {
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
    }
    Ok(stream)
}

/// A Git repository with one commit on `main`, committed by a fixture identity
/// so the developer's own `user.name` cannot change what the test sees.
pub fn init_repo(repo: &Path) {
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "OVRCR Tests"],
        vec!["config", "user.email", "tests@example.invalid"],
    ] {
        git(repo, &args);
    }
    std::fs::write(repo.join("README"), "fixture\n").unwrap();
    git(repo, &["add", "README"]);
    git(repo, &["commit", "-m", "initial"]);
}

/// Run `git` in `repo`, with the ambient repository environment removed so a
/// fixture repository cannot be confused with the checkout under test.
pub fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// One control request to `socket`, with every failure — refused connection,
/// timeout, closed socket — reported as `None` instead of a panic.
pub fn request_with_timeout(
    socket: &Path,
    request_id: u64,
    request: Request,
    timeout: Duration,
) -> Option<Response> {
    let mut stream = connect_with(socket, Some(timeout)).ok()?;
    client::request(&mut stream, request_id, request).ok()
}

/// Poll `ready` until it holds, or `timeout` runs out.
fn poll_until(timeout: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if ready() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
}

pub fn wait_for_socket(socket: &Path, timeout: Duration) -> bool {
    poll_until(timeout, || UnixStream::connect(socket).is_ok())
}

pub fn wait_for_absent(path: &Path, timeout: Duration) -> bool {
    poll_until(timeout, || !path.exists())
}

/// Whether any process still belongs to this group. `EPERM` counts as present:
/// a group the test may not signal is a group that has not been reaped.
pub fn group_exists(pgid: libc::pid_t) -> bool {
    let result = unsafe { libc::kill(-pgid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn wait_group_absent(pgid: libc::pid_t, timeout: Duration) -> bool {
    poll_until(timeout, || !group_exists(pgid))
}

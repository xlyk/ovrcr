//! One live OVRCR server per fixture.
//!
//! Every `Live` owns a private temporary root and, inside it, the Git
//! repository, workspace root, registry file and Unix socket the server under
//! test is given. Nothing is shared with the developer's own server, with the
//! other suites, or with a sibling fixture in the same binary.
//!
//! Two adapters sit at one seam — how the server is hosted:
//!
//! * [`Live::thread`] runs [`run_server`] on a thread of the test process, so
//!   a test can reach into server state and a panic carries the server down
//!   with it.
//! * [`Live::binary`] spawns the compiled `ovrcr` binary, so a test can prove
//!   things about a real process: signals, exit status, a CLI that talks to a
//!   server it did not start.
//!
//! Everything above the seam is shared: repository and registry construction,
//! the wait for the socket, timed control requests through
//! [`ovrcr::protocol::client`], "project and workspace ready", the process
//! groups the fixture owns, and the cleanup that runs from [`Drop`].

// Eight test binaries include this module; each uses its own subset.
#![allow(dead_code, unused_imports)]

#[path = "deadline.rs"]
mod deadline;

pub use deadline::wait_deadline;

use ovrcr::config::{Registry, save_registry_atomic};
use ovrcr::protocol::{BranchRequest, Request, Response, client, connect_server};
#[cfg(feature = "acceptance-diagnostics")]
use ovrcr::server::ServerQueueDiagnostics;
use ovrcr::server::{ServerPaths, run_server};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The project every `ready` fixture registers.
pub const PROJECT: &str = "fixture";
/// The workspace every `ready` fixture creates in [`PROJECT`].
pub const WORKSPACE: &str = "work";

/// The seam: who hosts the server this fixture drives.
enum Host {
    Idle,
    Thread(JoinHandle<()>),
    Binary(Child),
}

pub struct Live {
    pub root: tempfile::TempDir,
    pub repo: PathBuf,
    pub workspace_root: PathBuf,
    pub config: PathBuf,
    pub socket: PathBuf,
    pub executable: PathBuf,
    host: Mutex<Host>,
    pgids: Mutex<Vec<libc::pid_t>>,
    ready: Mutex<bool>,
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
        let config = root.path().join("config.toml");
        save_registry_atomic(&Registry::default(), &config).unwrap();
        Self {
            repo: repo.canonicalize().unwrap(),
            workspace_root: workspace_root.canonicalize().unwrap(),
            // The server makes this directory private; keeping the socket out
            // of the root leaves the root's own mode the test's business.
            socket: root.path().join("private").join("server.sock"),
            config,
            root,
            executable: PathBuf::from(env!("CARGO_BIN_EXE_ovrcr")),
            host: Mutex::new(Host::Idle),
            pgids: Mutex::new(Vec::new()),
            ready: Mutex::new(false),
            timeout: None,
            #[cfg(feature = "acceptance-diagnostics")]
            diagnostics: ServerQueueDiagnostics::default(),
        }
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
        let child = Command::new(&live.executable)
            .arg("server")
            .env("OVRCR_SOCKET", &live.socket)
            .env("OVRCR_CONFIG", &live.config)
            // The sessions a workspace opens run this shell. The developer's
            // own login shell and its rc files are not the test's subject.
            .env("SHELL", "/bin/sh")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn isolated OVRCR server");
        *live.host.lock().unwrap() = Host::Binary(child);
        live.wait_socket();
        // A spawned server can die; a control request must fail rather than
        // block this suite's own process forever.
        live.bounded_by(wait_deadline())
    }

    /// Start the thread adapter under an [`idle`](Self::idle) fixture.
    pub fn start(&self) {
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
    pub fn request(&self, request: Request) -> Response {
        let mut stream = connect_server(&self.socket).unwrap();
        if let Some(timeout) = self.timeout {
            stream.set_read_timeout(Some(timeout)).unwrap();
            stream.set_write_timeout(Some(timeout)).unwrap();
        }
        client::request(&mut stream, 1, request).unwrap()
    }

    /// Register [`PROJECT`] and create [`WORKSPACE`] on a new `branch`.
    ///
    /// Idempotent: the first caller pays for it and later callers see the same
    /// workspace, so a fixture can make itself ready lazily.
    pub fn ready(&self, branch: &str) {
        let mut ready = self.ready.lock().unwrap();
        if *ready {
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
        assert_eq!(
            self.request(Request::CreateWorkspace {
                project: PROJECT.into(),
                name: WORKSPACE.into(),
                branch: BranchRequest::New {
                    branch: branch.into(),
                    base: "main".into(),
                },
            }),
            Response::Ok
        );
        *ready = true;
    }

    /// Record the process group of `pid` as one this fixture must reap.
    pub fn own(&self, pid: u32) {
        self.own_group(unsafe { libc::getpgid(pid as libc::pid_t) });
    }

    /// Record a process group directly, for a group the test learned about
    /// from the child rather than from the server.
    pub fn own_group(&self, pgid: libc::pid_t) {
        if pgid > 1 {
            let mut pgids = self.pgids.lock().unwrap();
            if !pgids.contains(&pgid) {
                pgids.push(pgid);
            }
        }
    }

    /// The process groups this fixture is responsible for reaping.
    pub fn owned_groups(&self) -> Vec<libc::pid_t> {
        self.pgids.lock().unwrap().clone()
    }

    /// Record every live session's process group, and return them.
    pub fn own_sessions(&self) -> Vec<libc::pid_t> {
        let Response::Hierarchy(snapshot) = self.request(Request::List) else {
            panic!("server list did not return a hierarchy");
        };
        let pids: Vec<u32> = snapshot
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .filter_map(|session| session.pid)
            .collect();
        for pid in pids {
            self.own(pid);
        }
        self.owned_groups()
    }

    /// Wait for the hosted server to finish, however it is hosted.
    pub fn join(&self) {
        assert!(
            self.join_within(Duration::from_secs(30)),
            "fixture server did not finish"
        );
    }

    /// The same wait, bounded; `false` if the server outlived the budget.
    pub fn join_within(&self, timeout: Duration) -> bool {
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
                    Host::Idle => true,
                    Host::Thread(handle) => handle.join().is_ok(),
                    Host::Binary(mut child) => {
                        child.wait().map(|status| status.success()).unwrap_or(false)
                    }
                };
            }
            drop(host);
            if Instant::now() >= deadline {
                return false;
            }
            thread::park_timeout(Duration::from_millis(5));
        }
    }

    fn wait_socket(&self) {
        assert!(
            wait_for_socket(&self.socket, Duration::from_secs(5)),
            "fixture server socket did not appear: {}",
            self.socket.display()
        );
    }

    /// Whether a server is still hosted here — no shutdown has completed yet.
    pub fn hosted(&self) -> bool {
        !matches!(&*self.host.lock().unwrap(), Host::Idle)
    }

    fn kill_owned_groups(&self) -> bool {
        let groups = self.pgids.lock().unwrap().clone();
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
        if self.hosted() && !self.join_within(Duration::from_secs(5)) {
            cleanup_failed = true;
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
    let mut stream = connect_server(socket).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    client::request(&mut stream, request_id, request).ok()
}

pub fn wait_for_socket(socket: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
}

pub fn wait_for_absent(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !path.exists() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
}

/// Whether any process still belongs to this group. `EPERM` counts as present:
/// a group the test may not signal is a group that has not been reaped.
pub fn group_exists(pgid: libc::pid_t) -> bool {
    let result = unsafe { libc::kill(-pgid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn wait_group_absent(pgid: libc::pid_t, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !group_exists(pgid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::park_timeout(Duration::from_millis(5));
    }
}

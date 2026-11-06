//! Development-only outer terminal and disposable OVRCR instance.
pub mod input;
use anyhow::{Context, Result, bail};
use eframe::egui;
use ovrcr_terminal::vt100;
use portable_pty::{Child as PtyChild, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Settings the GUI demo's `dashboard.toml` may set differently from
/// production defaults: automatic local terminals on, and the quota fragment
/// from `OVRCR_GUI_QUOTA_CONFIG` when that variable is set.
pub const DEMO_SETTINGS_ALLOWLIST: &[&str] = &["automatic_local_terminals", "quota"];

const KITTY_KEYBOARD_PUSH: &[u8] = b"\x1b[>11u";
const KITTY_KEYBOARD_POP: &[u8] = b"\x1b[<1u";
const FOCUS_REPORTING_ENABLE: &[u8] = b"\x1b[?1004h";
const FOCUS_REPORTING_DISABLE: &[u8] = b"\x1b[?1004l";

#[derive(Default)]
struct KeyboardEnhancementObserver {
    enabled: bool,
    tail: Vec<u8>,
}

impl KeyboardEnhancementObserver {
    fn process(&mut self, bytes: &[u8]) -> bool {
        let mut input = std::mem::take(&mut self.tail);
        input.extend_from_slice(bytes);
        for start in 0..input.len() {
            let remaining = &input[start..];
            if remaining.starts_with(KITTY_KEYBOARD_PUSH) {
                self.enabled = true;
            } else if remaining.starts_with(KITTY_KEYBOARD_POP) {
                self.enabled = false;
            }
        }
        let keep = input.len().saturating_sub(KITTY_KEYBOARD_PUSH.len() - 1);
        self.tail.extend_from_slice(&input[keep..]);
        self.enabled
    }
}

#[derive(Default)]
struct FocusReportingObserver {
    enabled: bool,
    tail: Vec<u8>,
}

impl FocusReportingObserver {
    fn process(&mut self, bytes: &[u8]) -> bool {
        let mut input = std::mem::take(&mut self.tail);
        input.extend_from_slice(bytes);
        for start in 0..input.len() {
            let remaining = &input[start..];
            if remaining.starts_with(FOCUS_REPORTING_ENABLE) {
                self.enabled = true;
            } else if remaining.starts_with(FOCUS_REPORTING_DISABLE) {
                self.enabled = false;
            }
        }
        let keep = input.len().saturating_sub(FOCUS_REPORTING_ENABLE.len() - 1);
        self.tail.extend_from_slice(&input[keep..]);
        self.enabled
    }
}

/// The demo settings document: the allowlisted local-terminals line, then
/// `quota_fragment` when the caller has one.
pub fn demo_dashboard_document(quota_fragment: Option<&str>) -> String {
    let mut document = String::from("automatic_local_terminals = \"on\"\n");
    if let Some(fragment) = quota_fragment {
        document.push('\n');
        document.push_str(fragment);
        document.push('\n');
    }
    document
}

pub struct Demo {
    root: PathBuf,
    executable: PathBuf,
    server: Option<Child>,
    cleaned: bool,
}

impl Demo {
    pub fn start(executable: &Path) -> Result<Self> {
        let executable = executable
            .canonicalize()
            .context("locate ovrcr executable")?;
        // Keep the directory until shutdown succeeds, including on partial setup failure.
        let root = tempfile::Builder::new()
            .prefix("ovrcr-gui-")
            .tempdir()?
            .keep();
        let mut demo = Self {
            root,
            executable,
            server: None,
            cleaned: false,
        };
        if let Err(error) = demo.setup() {
            let path = demo.root.clone();
            if let Err(cleanup) = demo.shutdown() {
                bail!(
                    "{error:#}; cleanup failed: {cleanup:#}; demo retained at {}",
                    path.display()
                );
            }
            return Err(error);
        }
        Ok(demo)
    }

    fn setup(&mut self) -> Result<()> {
        let repositories = self.root.join("repositories");
        let workspaces = self.root.join("workspaces");
        fs::create_dir(&repositories)?;
        fs::create_dir(&workspaces)?;
        // The disposable provider-name symlinks resolve to /bin/sh; putting only
        // this demo's bin first lets local shells create Agent fixtures safely.
        fs::create_dir(self.root.join("agent-bin"))?;
        crate::config::save_registry_atomic(&crate::config::Registry::default(), &self.root)?;
        // Demo workspaces intentionally keep automatic local shells on every
        // worktree so the GUI helper exercises a full sidebar of terminals.
        let fragment = match std::env::var_os("OVRCR_GUI_QUOTA_CONFIG") {
            Some(path) => Some(fs::read_to_string(path).context("read GUI quota config fragment")?),
            None => None,
        };
        fs::write(
            self.root.join("dashboard.toml"),
            demo_dashboard_document(fragment.as_deref()),
        )?;
        self.create_project_fixture(&repositories, &workspaces, "consigint")?;
        self.create_project_fixture(&repositories, &workspaces, "spacelift-agent")?;
        let lifecycle = self.create_workspace_fixture("consigint", "worktree-lifecycle")?;
        let auth = self.create_workspace_fixture("consigint", "auth-handoff")?;
        let pipeline = self.create_workspace_fixture("spacelift-agent", "pipeline-progress-v2")?;
        let quality = self.create_workspace_fixture("spacelift-agent", "scope-quality")?;

        self.create_agent_fixture(
            "consigint",
            &lifecycle,
            "implement lifecycle cleanup",
            "claude / sonnet-4",
            "lifecycle cleanup",
        )?;
        self.create_agent_fixture(
            "consigint",
            &lifecycle,
            "review websocket shutdown",
            "codex / gpt-5.4",
            "websocket shutdown review",
        )?;
        self.create_agent_fixture(
            "consigint",
            &lifecycle,
            "plan snapshot restore",
            "pi / grok-4.6",
            "snapshot restore plan",
        )?;
        self.create_agent_fixture(
            "consigint",
            &auth,
            "review auth handoff",
            "grok / grok-4.6",
            "authentication handoff review",
        )?;
        self.create_agent_fixture(
            "spacelift-agent",
            &pipeline,
            "build pipeline progress",
            "claude / opus-4",
            "pipeline progress fixture",
        )?;
        self.create_agent_fixture(
            "spacelift-agent",
            &quality,
            "review scope quality",
            "codex / gpt-5.4",
            "scope quality review",
        )?;
        Ok(())
    }

    fn create_project_fixture(
        &mut self,
        repositories: &Path,
        workspaces: &Path,
        name: &str,
    ) -> Result<()> {
        let repo = repositories.join(name);
        let workspace_root = workspaces.join(name);
        fs::create_dir(&repo)?;
        fs::create_dir(&workspace_root)?;
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "OVRCR GUI"],
            vec!["config", "user.email", "gui@example.invalid"],
        ] {
            checked(
                Command::new("git").arg("-C").arg(&repo).args(args),
                Duration::from_secs(5),
            )?;
        }
        fs::write(
            repo.join("README"),
            format!("Disposable OVRCR GUI fixture for {name}\n"),
        )?;
        checked(
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["add", "README"]),
            Duration::from_secs(5),
        )?;
        checked(
            Command::new("git").arg("-C").arg(&repo).args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-m",
                "fixture",
            ]),
            Duration::from_secs(5),
        )?;
        if self.server.is_none() {
            let log = File::create(self.root.join("server.log"))?;
            self.server = Some(
                self.command()
                    .arg("server")
                    .stdin(Stdio::null())
                    .stdout(log.try_clone()?)
                    .stderr(log)
                    .spawn()?,
            );
            let deadline = Instant::now() + Duration::from_secs(5);
            while !self.root.join("server.sock").exists() {
                if let Some(status) = self.server.as_mut().unwrap().try_wait()? {
                    bail!("demo server exited: {status}");
                }
                if Instant::now() >= deadline {
                    bail!("demo server startup timed out");
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        self.cli(&[
            "project",
            "add",
            name,
            repo.to_str().context("demo path is not UTF-8")?,
            "--workspace-root",
            workspace_root.to_str().context("demo path is not UTF-8")?,
        ])?;
        Ok(())
    }

    fn create_workspace_fixture(&self, project: &str, name: &str) -> Result<String> {
        let branch = format!("gui-{project}-{name}");
        self.cli(&[
            "workspace",
            "create",
            "--project",
            project,
            "--new-branch",
            &branch,
            "--base",
            "main",
        ])?;
        Ok(branch)
    }

    fn create_agent_fixture(
        &self,
        project: &str,
        workspace: &str,
        name: &str,
        label: &str,
        task: &str,
    ) -> Result<()> {
        let script = fixture_shell(task, label);
        let provider = label
            .split_once(" / ")
            .map_or(label, |(provider, _)| provider);
        let agent_bin = self.root.join("agent-bin");
        fs::create_dir_all(&agent_bin)?;
        let executable = agent_bin.join(provider);
        if !executable.exists() {
            std::os::unix::fs::symlink("/bin/sh", &executable)?;
        }
        let executable = executable
            .to_str()
            .context("demo agent path is not UTF-8")?;
        self.cli(&[
            "new",
            "--project",
            project,
            "--workspace",
            workspace,
            "--name",
            name,
            "--label",
            label,
            "--",
            executable,
            "-c",
            &script,
        ])?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .env("OVRCR_HOME", &self.root)
            .env("OVRCR_SOCKET", self.root.join("server.sock"))
            // The demo's settings document is the one in its instance directory.
            .env_remove("OVRCR_DASHBOARD_CONFIG")
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("PATH", self.path_with_agent_fixtures())
            .env_remove("NO_COLOR");
        if std::env::var_os("SHELL").is_none() {
            command.env("SHELL", "/bin/sh");
        }
        command
    }

    fn cli(&self, args: &[&str]) -> Result<Output> {
        checked(self.command().args(args), Duration::from_secs(15))
    }

    fn path_with_agent_fixtures(&self) -> std::ffi::OsString {
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::join_paths(
            std::iter::once(self.root.join("agent-bin")).chain(std::env::split_paths(&path)),
        )
        .expect("the task-owned fixture path must be joinable")
    }

    pub fn dashboard(&self, rows: u16, cols: u16, context: egui::Context) -> Result<Terminal> {
        // A disposable demo must not discover installation assets from the
        // caller's build directory. Keep its original Server image explicit.
        let directory = self.root.join("dashboard-client");
        fs::create_dir_all(&directory)?;
        let executable = directory.join("ovrcr");
        if !executable.exists() {
            fs::copy(&self.executable, &executable)?;
        }
        let mut command = CommandBuilder::new(&executable);
        command.env("OVRCR_SERVER_EXECUTABLE", &self.executable);
        command.env("OVRCR_HOME", &self.root);
        command.env("OVRCR_SOCKET", self.root.join("server.sock"));
        command.env_remove("OVRCR_DASHBOARD_CONFIG");
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("PATH", self.path_with_agent_fixtures());
        command.env_remove("NO_COLOR");
        Terminal::start(command, rows, cols, context)
    }

    pub fn shutdown(&mut self) -> Result<()> {
        if self.cleaned {
            return Ok(());
        }
        if let Some(server) = self.server.as_mut() {
            if server.try_wait()?.is_none() {
                checked(
                    self.command().args(["shutdown", "--kill"]),
                    Duration::from_secs(60),
                )?;
                let server = self.server.as_mut().unwrap();
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    if let Some(status) = server.try_wait()? {
                        if !status.success() {
                            bail!("demo server exited: {status}");
                        }
                        break;
                    }
                    if Instant::now() >= deadline {
                        bail!("demo server shutdown timed out");
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            } else {
                bail!("demo server exited unexpectedly; preserving fixture for recovery");
            }
        }
        if self.root.join("server.sock").exists() {
            bail!("demo socket remains after shutdown");
        }
        self.server = None;
        fs::remove_dir_all(&self.root).context("remove disposable demo")?;
        self.cleaned = true;
        Ok(())
    }
}

fn fixture_shell(task: &str, label: &str) -> String {
    format!(
        "printf '\\033[38;5;141m╭─ {label} ────────────────────────────────────────╮\\033[0m\\n'; \\
         printf '│ fixture: {task}\\n'; \\
         printf '\\033[38;5;141m╰──────────────────────────────────────────────────╯\\033[0m\\n\\n'; \\
         printf '\\033[38;5;114m✓ deterministic local transcript ready\\033[0m\\n'; \\
         exec /bin/sh -i"
    )
}

impl Drop for Demo {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!(
                "OVRCR GUI cleanup failed: {error:#}; demo retained at {}",
                self.root.display()
            );
        }
    }
}

fn checked(command: &mut Command, timeout: Duration) -> Result<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("command timed out: {command:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "command failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

pub struct Terminal {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn PtyChild + Send + Sync>,
    parser: Arc<Mutex<vt100::Parser>>,
    reader: Option<JoinHandle<()>>,
    stopping: Arc<AtomicBool>,
    read_error: Arc<Mutex<Option<String>>>,
    keyboard_enhancement_enabled: Arc<AtomicBool>,
    focus_reporting_enabled: Arc<AtomicBool>,
    context: egui::Context,
}

impl Terminal {
    pub fn start(
        command: CommandBuilder,
        rows: u16,
        cols: u16,
        context: egui::Context,
    ) -> Result<Self> {
        let pair = native_pty_system().openpty(size(rows, cols))?;
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let fd = pair
            .master
            .as_raw_fd()
            .context("outer PTY has no file descriptor")?;
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);
        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows.max(1), cols.max(1), 0)));
        let stopping = Arc::new(AtomicBool::new(false));
        let read_error = Arc::new(Mutex::new(None));
        let keyboard_enhancement_enabled = Arc::new(AtomicBool::new(false));
        let focus_reporting_enabled = Arc::new(AtomicBool::new(false));
        let mut terminal = Self {
            master: pair.master,
            writer,
            child,
            parser,
            reader: None,
            stopping,
            read_error,
            keyboard_enhancement_enabled,
            focus_reporting_enabled,
            context,
        };
        let parser = Arc::clone(&terminal.parser);
        let stopping = Arc::clone(&terminal.stopping);
        let read_error = Arc::clone(&terminal.read_error);
        let keyboard_enhancement_enabled = Arc::clone(&terminal.keyboard_enhancement_enabled);
        let focus_reporting_enabled = Arc::clone(&terminal.focus_reporting_enabled);
        let context = terminal.context.clone();
        terminal.reader = Some(
            thread::Builder::new()
                .name("ovrcr-gui-output".into())
                .spawn(move || {
                    let mut reader = reader;
                    let mut buffer = [0; 8192];
                    let mut keyboard_mode = KeyboardEnhancementObserver::default();
                    let mut focus_reporting = FocusReportingObserver::default();
                    while !stopping.load(Ordering::Relaxed) {
                        let mut poll = libc::pollfd {
                            fd,
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        let ready = unsafe { libc::poll(&mut poll, 1, 100) };
                        if ready == 0 {
                            continue;
                        }
                        if ready < 0 {
                            let error = std::io::Error::last_os_error();
                            if error.kind() == std::io::ErrorKind::Interrupted {
                                continue;
                            }
                            *read_error.lock().unwrap() = Some(error.to_string());
                            break;
                        }
                        match reader.read(&mut buffer) {
                            Ok(0) => break,
                            Ok(count) => {
                                let enabled = keyboard_mode.process(&buffer[..count]);
                                keyboard_enhancement_enabled.store(enabled, Ordering::Release);
                                let enabled = focus_reporting.process(&buffer[..count]);
                                focus_reporting_enabled.store(enabled, Ordering::Release);
                                parser.lock().unwrap().process(&buffer[..count]);
                                if !stopping.load(Ordering::Relaxed) {
                                    context.request_repaint();
                                }
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                                continue;
                            }
                            Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                            Err(error) => {
                                *read_error.lock().unwrap() = Some(error.to_string());
                                break;
                            }
                        }
                    }
                    if !stopping.load(Ordering::Relaxed) {
                        context.request_repaint();
                    }
                })?,
        );
        Ok(terminal)
    }

    pub fn screen(&self) -> vt100::Screen {
        self.parser.lock().unwrap().screen().clone()
    }

    /// Whether this Dashboard has actually enabled enhanced keyboard reporting
    /// on the outer PTY. The GUI never infers modifier support from platform alone.
    pub fn keyboard_enhancement_enabled(&self) -> bool {
        self.keyboard_enhancement_enabled.load(Ordering::Acquire)
    }

    /// Whether this Dashboard has requested terminal focus-change reports.
    /// `vt100` does not expose DEC mode 1004, so observe the exact mode sequence
    /// emitted by the Dashboard guard instead.
    pub fn focus_reporting_enabled(&self) -> bool {
        self.focus_reporting_enabled.load(Ordering::Acquire)
    }

    pub fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer
            .write_all(bytes)
            .context("write dashboard input")?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        let mut parser = self.parser.lock().unwrap();
        if parser.screen().size() != (rows.max(1), cols.max(1)) {
            self.master.resize(size(rows, cols))?;
            parser.screen_mut().set_size(rows.max(1), cols.max(1));
        }
        Ok(())
    }

    pub fn status(&mut self) -> Result<Option<String>> {
        if let Some(error) = self.read_error.lock().unwrap().as_ref() {
            bail!("PTY read: {error}");
        }
        Ok(self.child.try_wait()?.map(|status| status.to_string()))
    }

    pub fn stop(&mut self) -> Result<()> {
        if self.reader.is_none() {
            return Ok(());
        }
        if self.child.try_wait()?.is_none() {
            // Ctrl-g leaves terminal mode; q asks the real dashboard to detach.
            let _ = self.send(b"\x07q");
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.child.try_wait()?.is_none() {
                if Instant::now() >= deadline {
                    self.child.kill()?;
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        self.child.wait()?;
        self.stopping.store(true, Ordering::Relaxed);
        if let Some(reader) = self.reader.take() {
            reader
                .join()
                .map_err(|_| anyhow::anyhow!("PTY reader panicked"))?;
        }
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("dashboard cleanup: {error:#}");
        }
    }
}

fn size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_mode_observer_tracks_split_push_and_pop_sequences() {
        let mut observer = KeyboardEnhancementObserver::default();
        assert!(!observer.process(b"dashboard output\x1b[>11"));
        assert!(observer.process(b"uordinary output\x1b[<"));
        assert!(!observer.process(b"1u"));
    }

    #[test]
    fn keyboard_mode_observer_ignores_different_flags() {
        let mut observer = KeyboardEnhancementObserver::default();
        assert!(!observer.process(b"\x1b[>1u"));
        assert!(!observer.process(b"\x1b[<1u"));
    }

    #[test]
    fn focus_reporting_observer_tracks_split_enable_and_disable_sequences() {
        let mut observer = FocusReportingObserver::default();
        assert!(!observer.process(b"dashboard output\x1b[?100"));
        assert!(observer.process(b"4hordinary output\x1b[?1004"));
        assert!(!observer.process(b"l"));
    }

    #[test]
    fn focus_reporting_observer_ignores_other_modes() {
        let mut observer = FocusReportingObserver::default();
        assert!(!observer.process(b"\x1b[?1000h"));
        assert!(!observer.process(b"\x1b[?1000l"));
    }
}

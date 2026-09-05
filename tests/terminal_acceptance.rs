use anyhow::{Context, Result, bail};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

struct AcceptanceFixture {
    _root: tempfile::TempDir,
    repo: PathBuf,
    workspace_root: PathBuf,
    socket: PathBuf,
    config: PathBuf,
    executable: PathBuf,
}

impl AcceptanceFixture {
    fn new() -> Result<Self> {
        let root = tempfile::tempdir()?;
        let repo = root.path().join("repo");
        let workspace_root = root.path().join("workspaces");
        std::fs::create_dir(&repo)?;
        std::fs::create_dir(&workspace_root)?;
        git(&repo, &["init", "-b", "main"])?;
        git(&repo, &["config", "user.name", "OVRCR Acceptance"])?;
        git(
            &repo,
            &["config", "user.email", "acceptance@example.invalid"],
        )?;
        std::fs::write(repo.join("README"), "acceptance\n")?;
        git(&repo, &["add", "README"])?;
        git(&repo, &["commit", "-m", "initial"])?;
        Ok(Self {
            config: root.path().join("config.toml"),
            socket: root.path().join("server.sock"),
            executable: PathBuf::from(env!("CARGO_BIN_EXE_ovrcr")),
            _root: root,
            repo,
            workspace_root,
        })
    }

    fn cli(&self, args: &[&str]) -> Result<std::process::Output> {
        Command::new(&self.executable)
            .args(args)
            .env("OVRCR_SOCKET", &self.socket)
            .env("OVRCR_CONFIG", &self.config)
            .output()
            .context("run OVRCR CLI")
    }

    fn setup(&self) -> Result<()> {
        let repo = self.repo.to_string_lossy();
        let workspace_root = self.workspace_root.to_string_lossy();
        let output = self.cli(&[
            "project",
            "add",
            "fixture",
            &repo,
            "--workspace-root",
            &workspace_root,
        ])?;
        require_success(output, "project add")?;
        let output = self.cli(&[
            "workspace",
            "create",
            "--project",
            "fixture",
            "--name",
            "work",
            "--new-branch",
            "feature/acceptance",
            "--base",
            "main",
        ])?;
        require_success(output, "workspace create")?;
        let output = self.cli(&[
            "new",
            "--project",
            "fixture",
            "--workspace",
            "work",
            "--name",
            "waiting",
            "--",
            "sh",
            "-c",
            "printf WAITING_READY; while :; do sleep 1; done",
        ])?;
        require_success(output, "waiting session")?;
        Ok(())
    }
}

impl Drop for AcceptanceFixture {
    fn drop(&mut self) {
        let _ = self.cli(&["shutdown", "--kill"]);
    }
}

fn git(cwd: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git").args(args).current_dir(cwd).output()?;
    if !output.status.success() {
        bail!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn require_success(output: std::process::Output, operation: &str) -> Result<String> {
    if !output.status.success() {
        bail!(
            "{operation} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

struct OuterDashboard {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    received: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
}

impl OuterDashboard {
    fn start(fixture: &AcceptanceFixture, size: PtySize) -> Result<Self> {
        let pty = native_pty_system();
        let pair = pty.openpty(size)?;
        let mut command = CommandBuilder::new(&fixture.executable);
        command.env("OVRCR_SOCKET", &fixture.socket);
        command.env("OVRCR_CONFIG", &fixture.config);
        let child = pair.slave.spawn_command(command)?;
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (sender, received) = mpsc::channel();
        let reader_handle = thread::spawn(move || read_outer(reader, sender));
        let mut dashboard = Self {
            master: pair.master,
            writer,
            child,
            received,
            reader: Some(reader_handle),
        };
        dashboard.wait_for(b"OVRCR", Duration::from_secs(3))?;
        Ok(dashboard)
    }

    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    fn wait_for(&mut self, needle: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        let deadline = Instant::now() + timeout;
        let mut output = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self
                .received
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(bytes) => {
                    output.extend_from_slice(&bytes);
                    if output.windows(needle.len()).any(|window| window == needle) {
                        return Ok(output);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        bail!(
            "outer terminal did not render {:?}: {}",
            String::from_utf8_lossy(needle),
            String::from_utf8_lossy(&output)
        )
    }

    fn detach(mut self) -> Result<()> {
        self.send(b"\x07")?;
        self.send(b"q")?;
        let status = self.child.wait()?;
        if !status.success() {
            bail!("dashboard exited unsuccessfully: {status:?}");
        }
        if let Some(reader) = self.reader.take() {
            reader
                .join()
                .map_err(|_| anyhow::anyhow!("outer reader panicked"))?;
        }
        Ok(())
    }
}

impl Drop for OuterDashboard {
    fn drop(&mut self) {
        let _ = self.writer.write_all(b"\x07q");
        let _ = self.writer.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = self.master.as_ref();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_outer(mut reader: Box<dyn Read + Send>, sender: Sender<Vec<u8>>) {
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                if sender.send(buffer[..read].to_vec()).is_err() {
                    break;
                }
            }
        }
    }
}

#[test]
fn default_dashboard_acceptance_wrapper_exercises_pty_controls() -> Result<()> {
    let fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.wait_for(b"BROWSE", Duration::from_secs(3))?;
    dashboard.send(b"j")?;
    dashboard.wait_for(b"WAITING_READY", Duration::from_secs(3))?;
    dashboard.send(b"\r")?;
    dashboard.wait_for(b"TERMINAL", Duration::from_secs(3))?;
    dashboard.send(b"\x1b[200~printf PASTE_MARKER\x1b[201~\r")?;
    dashboard.send(b"\x07")?;
    dashboard.send(b"j")?;
    dashboard.send(b"\x1b[<0;5;5M")?;
    dashboard.send(b"\r")?;
    dashboard.send(b"printf WAITING_INPUT\r")?;
    dashboard.wait_for(b"WAITING_INPUT", Duration::from_secs(3))?;
    dashboard.resize(40, 120)?;
    dashboard.detach()?;

    let mut reattached = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    reattached.wait_for(b"BROWSE", Duration::from_secs(3))?;
    reattached.send(b"j")?;
    reattached.wait_for(b"WAITING_READY", Duration::from_secs(3))?;
    reattached.detach()?;
    let _ = fixture.cli(&["shutdown", "--kill"])?;
    Ok(())
}

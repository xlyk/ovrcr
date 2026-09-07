use anyhow::{Context, Result, bail};
use ovrcr::config::{Registry, save_registry_atomic};
use ovrcr::protocol::{
    ClientMessage, HierarchySnapshot, Request, Response, ServerMessage, read_frame, write_frame,
};
use portable_pty::{Child as PtyChild, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
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
    server: Option<Child>,
    managed_pgids: Vec<libc::pid_t>,
}

impl AcceptanceFixture {
    fn new() -> Result<Self> {
        let root = tempfile::tempdir()?;
        let repo = root.path().join("repo");
        let workspace_root = root.path().join("workspaces");
        std::fs::create_dir(&repo)?;
        std::fs::create_dir(&workspace_root)?;
        let config = root.path().join("config.toml");
        save_registry_atomic(&Registry::default(), &config)?;
        let socket = root.path().join("server.sock");
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_ovrcr"));
        let server = Command::new(&executable)
            .arg("server")
            .env("OVRCR_SOCKET", &socket)
            .env("OVRCR_CONFIG", &config)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("start isolated OVRCR server")?;
        let fixture = Self {
            config,
            socket,
            executable,
            _root: root,
            repo,
            workspace_root,
            server: Some(server),
            managed_pgids: Vec::new(),
        };
        wait_for_socket(&fixture.socket, Duration::from_secs(3))?;
        git(&fixture.repo, &["init", "-b", "main"])?;
        git(&fixture.repo, &["config", "user.name", "OVRCR Acceptance"])?;
        git(
            &fixture.repo,
            &["config", "user.email", "acceptance@example.invalid"],
        )?;
        std::fs::write(fixture.repo.join("README"), "acceptance\n")?;
        git(&fixture.repo, &["add", "README"])?;
        git(&fixture.repo, &["commit", "-m", "initial"])?;
        Ok(fixture)
    }

    fn cli(&self, args: &[&str]) -> Result<std::process::Output> {
        self.cli_timeout(args, Duration::from_secs(5))
    }

    fn cli_timeout(&self, args: &[&str], timeout: Duration) -> Result<std::process::Output> {
        let child = Command::new(&self.executable)
            .args(args)
            .env("OVRCR_SOCKET", &self.socket)
            .env("OVRCR_CONFIG", &self.config)
            .spawn()
            .context("run OVRCR CLI")?;
        bounded_output(child, timeout)
            .with_context(|| format!("CLI command timed out or failed: {args:?}"))
    }

    fn setup(&mut self) -> Result<()> {
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
            "printf WAITING_READY; while IFS= read -r line; do case \"$line\" in *PASTE_TOKEN*) printf PASTE_ACK;; *INPUT_TOKEN*) printf INPUT_ACK;; *LATENCY_TOKEN_*) printf 'ECHO_%s' \"$line\";; *RAW_MODE*) stty -icanon -echo min 1 time 0; printf RAW_READY; while :; do byte=$(dd bs=1 count=1 2>/dev/null); [ -n \"$byte\" ] && printf '\\rRAW_ACK_%s' \"$byte\"; done;; *WAITING_TOKEN*) printf WAITING_ACK;; *PAUSE_RESUME_TOKEN*) printf PAUSE_RESUME_ACK;; *SIZE_TOKEN*) printf 'SIZE_ACK_%s' \"$(stty size)\";; esac; done",
        ])?;
        require_success(output, "waiting session")?;
        self.managed_pgids = self.session_pgids()?;
        let output = self.cli(&[
            "new",
            "--project",
            "fixture",
            "--workspace",
            "work",
            "--name",
            "mouse",
            "--",
            "sh",
            "-c",
            "printf MOUSE_READY; while IFS= read -r line; do case \"$line\" in *PASTE_TOKEN*) printf PASTE_ACK;; *INPUT_TOKEN*) printf INPUT_ACK;; *MOUSE_TOKEN*) printf MOUSE_ACK;; *SIZE_TOKEN*) printf 'SIZE_ACK_%s' \"$(stty size)\";; esac; done",
        ])?;
        require_success(output, "mouse session")?;
        self.managed_pgids = self.session_pgids()?;
        Ok(())
    }

    fn session_pgids(&self) -> Result<Vec<libc::pid_t>> {
        let mut stream = std::os::unix::net::UnixStream::connect(&self.socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(3)))?;
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id: 1,
                request: Request::List,
            },
        )?;
        let message = read_frame::<ServerMessage>(&mut stream)?;
        let ServerMessage::Response {
            response: Response::Hierarchy(hierarchy),
            ..
        } = message
        else {
            bail!("server list did not return a hierarchy")
        };
        Ok(hierarchy
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .filter_map(|session| {
                session
                    .pid
                    .map(|pid| unsafe { libc::getpgid(pid as libc::pid_t) })
            })
            .filter(|pgid| *pgid > 1)
            .collect())
    }

    fn list(&self) -> Result<HierarchySnapshot> {
        let mut stream = std::os::unix::net::UnixStream::connect(&self.socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(3)))?;
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id: 1,
                request: Request::List,
            },
        )?;
        let ServerMessage::Response {
            response: Response::Hierarchy(hierarchy),
            ..
        } = read_frame::<ServerMessage>(&mut stream)?
        else {
            bail!("server list did not return a hierarchy")
        };
        Ok(hierarchy)
    }

    fn read_terminal(&self, session: ovrcr::session::SessionId) -> Result<String> {
        let mut stream = std::os::unix::net::UnixStream::connect(&self.socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        write_frame(
            &mut stream,
            &ClientMessage {
                request_id: 2,
                request: Request::ReadTerminal {
                    session,
                    max_lines: None,
                },
            },
        )?;
        let ServerMessage::Response { response, .. } = read_frame(&mut stream)? else {
            bail!("terminal read returned an event")
        };
        let Response::TerminalText { text, .. } = response else {
            bail!("terminal read returned an unexpected response")
        };
        Ok(text)
    }

    fn shutdown(&mut self) -> Result<()> {
        let pgids = self.session_pgids()?;
        self.managed_pgids = pgids.clone();
        let output = self.cli_timeout(&["shutdown", "--kill"], Duration::from_secs(12))?;
        require_success(output, "shutdown --kill")?;
        let server = self
            .server
            .take()
            .context("isolated server already stopped")?;
        let status = wait_std_child(server, Duration::from_secs(3))?;
        if !status.success() {
            bail!("isolated server exited unsuccessfully: {status:?}");
        }
        wait_for_absent(&self.socket, Duration::from_secs(3))?;
        for pgid in pgids {
            wait_for_group_absent(pgid, Duration::from_secs(3))?;
        }
        Ok(())
    }

    fn terminate_managed_groups(&self) {
        for &pgid in &self.managed_pgids {
            if group_exists(pgid) {
                unsafe {
                    libc::kill(-pgid, libc::SIGTERM);
                }
            }
        }
        let grace_deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < grace_deadline
            && self.managed_pgids.iter().any(|&pgid| group_exists(pgid))
        {
            thread::park_timeout(Duration::from_millis(5));
        }
        for &pgid in &self.managed_pgids {
            if group_exists(pgid) {
                unsafe {
                    libc::kill(-pgid, libc::SIGKILL);
                }
            }
        }
        let kill_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < kill_deadline
            && self.managed_pgids.iter().any(|&pgid| group_exists(pgid))
        {
            thread::park_timeout(Duration::from_millis(5));
        }
    }
}

impl Drop for AcceptanceFixture {
    fn drop(&mut self) {
        if self.server.is_some() && self.shutdown().is_err() {
            self.terminate_managed_groups();
        }
        if let Some(mut server) = self.server.take() {
            if server.try_wait().ok().flatten().is_none() {
                let _ = server.kill();
            }
            let _ = wait_std_child(server, Duration::from_secs(2));
        }
    }
}

fn wait_for_socket(socket: &Path, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    bail!(
        "isolated server socket did not appear: {}",
        socket.display()
    )
}

fn wait_for_absent(path: &Path, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !path.exists() {
            return Ok(());
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    bail!("path did not disappear: {}", path.display())
}

fn group_exists(pgid: libc::pid_t) -> bool {
    let result = unsafe { libc::kill(-pgid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn wait_for_group_absent(pgid: libc::pid_t, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !group_exists(pgid) {
            return Ok(());
        }
        thread::park_timeout(Duration::from_millis(5));
    }
    bail!("PTY process group {pgid} did not disappear")
}

fn wait_std_child(mut child: Child, timeout: Duration) -> Result<std::process::ExitStatus> {
    let pid = child.id() as libc::pid_t;
    let (sender, receiver) = mpsc::sync_channel(1);
    let waiter = thread::spawn(move || {
        let _ = sender.send(child.wait());
    });
    let result = match receiver.recv_timeout(timeout) {
        Ok(result) => result?,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            receiver
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| anyhow::anyhow!("server waiter did not finish after SIGKILL"))??
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            bail!("server waiter disconnected")
        }
    };
    let _ = waiter.join();
    Ok(result)
}

fn bounded_output(child: Child, timeout: Duration) -> Result<std::process::Output> {
    let pid = child.id() as libc::pid_t;
    let (sender, receiver) = mpsc::sync_channel(1);
    let waiter = thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    match receiver.recv_timeout(timeout) {
        Ok(result) => {
            let _ = waiter.join();
            result.context("wait for OVRCR CLI")
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            unsafe { libc::kill(pid, libc::SIGKILL) };
            let result = receiver
                .recv_timeout(Duration::from_secs(2))
                .map_err(|_| anyhow::anyhow!("OVRCR CLI waiter did not finish after SIGKILL"))?;
            let _ = waiter.join();
            let _ = result;
            bail!("OVRCR CLI timed out after {timeout:?}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let _ = waiter.join();
            bail!("OVRCR CLI waiter disconnected")
        }
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
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Box<dyn Write + Send>,
    child: Option<Box<dyn PtyChild + Send + Sync>>,
    received: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<()>>,
    parser: vt100::Parser,
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
            master: Some(pair.master),
            writer,
            child: Some(child),
            received,
            reader: Some(reader_handle),
            parser: vt100::Parser::new(size.rows, size.cols, 0),
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
        self.master
            .as_ref()
            .context("outer PTY already closed")?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
        self.parser = vt100::Parser::new(rows, cols, 0);
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
                    self.parser.process(&bytes);
                    output.extend_from_slice(&bytes);
                    let rendered = self.parser.screen().contents();
                    if rendered.contains(String::from_utf8_lossy(needle).as_ref()) {
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

    fn wait_for_output(&mut self, needle: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        let deadline = Instant::now() + timeout;
        let mut output = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self
                .received
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(bytes) => {
                    self.parser.process(&bytes);
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
            "outer terminal did not emit {:?}: {}",
            String::from_utf8_lossy(needle),
            String::from_utf8_lossy(&output)
        )
    }

    fn rendered(&self) -> String {
        self.parser.screen().contents()
    }

    fn click_visible_text(&mut self, needle: &str) -> Result<()> {
        let row = self
            .rendered()
            .lines()
            .position(|line| line.contains(needle))
            .context("visible hierarchy row")?;
        let sequence = format!("\x1b[<0;5;{}M", row + 1);
        self.send(sequence.as_bytes())
    }

    fn wait_for_screen<F>(&mut self, predicate: F, timeout: Duration) -> Result<()>
    where
        F: Fn(&str) -> bool,
    {
        let deadline = Instant::now() + timeout;
        let mut output = Vec::new();
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self
                .received
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(bytes) => {
                    self.parser.process(&bytes);
                    output.extend_from_slice(&bytes);
                    let rendered = self.parser.screen().contents();
                    if predicate(&rendered) {
                        return Ok(());
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        bail!(
            "outer terminal did not reach expected screen state: {}",
            String::from_utf8_lossy(&output)
        )
    }

    fn detach(mut self) -> Result<()> {
        self.send(b"\x07")?;
        self.send(b"q")?;
        let child = self.child.take().context("outer child already detached")?;
        let pid = child.process_id().context("outer child has no PID")? as libc::pid_t;
        let (sender, receiver) = mpsc::sync_channel(1);
        let waiter = thread::spawn(move || {
            let mut child = child;
            let _ = sender.send(child.wait());
        });
        let status = match receiver.recv_timeout(Duration::from_secs(3)) {
            Ok(status) => status?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                unsafe { libc::kill(pid, libc::SIGKILL) };
                receiver
                    .recv_timeout(Duration::from_secs(2))
                    .map_err(|_| anyhow::anyhow!("outer dashboard did not exit after SIGKILL"))??
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("outer dashboard waiter disconnected")
            }
        };
        let _ = waiter.join();
        if !status.success() {
            bail!("dashboard exited unsuccessfully: {status:?}");
        }
        drop(std::mem::replace(
            &mut self.writer,
            Box::new(std::io::sink()),
        ));
        self.master.take();
        if let Some(reader) = self.reader.take() {
            join_reader(reader, Duration::from_secs(2))?;
        }
        Ok(())
    }
}

impl Drop for OuterDashboard {
    fn drop(&mut self) {
        let _ = self.writer.write_all(b"\x07q");
        let _ = self.writer.flush();
        drop(std::mem::replace(
            &mut self.writer,
            Box::new(std::io::sink()),
        ));
        self.master.take();
        if let Some(child) = self.child.take() {
            if let Ok(pid) = child.process_id().ok_or(()) {
                unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            }
            let _ = wait_pty_child(child, Duration::from_secs(2));
        }
        if let Some(reader) = self.reader.take() {
            let _ = join_reader(reader, Duration::from_secs(2));
        }
    }
}

fn wait_pty_child(
    child: Box<dyn PtyChild + Send + Sync>,
    timeout: Duration,
) -> Result<portable_pty::ExitStatus> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let waiter = thread::spawn(move || {
        let mut child = child;
        let _ = sender.send(child.wait());
    });
    let result = receiver
        .recv_timeout(timeout)
        .map_err(|_| anyhow::anyhow!("PTY child waiter timed out after {timeout:?}"))??;
    let _ = waiter.join();
    Ok(result)
}

fn join_reader(reader: JoinHandle<()>, timeout: Duration) -> Result<()> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let waiter = thread::spawn(move || {
        let _ = sender.send(reader.join());
    });
    let result = receiver
        .recv_timeout(timeout)
        .map_err(|_| anyhow::anyhow!("outer reader did not stop after {timeout:?}"))?;
    let _ = waiter.join();
    result.map_err(|_| anyhow::anyhow!("outer reader panicked"))
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

fn write_fifo_bounded(path: &Path, bytes: &[u8], timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let mut writer = loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
        {
            Ok(writer) => break writer,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::NotFound
                ) || error.raw_os_error() == Some(libc::ENXIO) =>
            {
                if Instant::now() >= deadline {
                    bail!("timed out opening FIFO writer {}: {error}", path.display());
                }
                thread::yield_now();
            }
            Err(error) => return Err(error.into()),
        }
    };
    let mut written = 0;
    while written < bytes.len() {
        match writer.write(&bytes[written..]) {
            Ok(0) => bail!("FIFO writer closed before token was written"),
            Ok(count) => written += count,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                if Instant::now() >= deadline {
                    bail!("timed out writing FIFO token to {}", path.display());
                }
                thread::yield_now();
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[test]
fn history_keyboard_reads_old_output_during_live_session() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let fifo = fixture._root.path().join("history-trigger.fifo");
    let fifo_arg = fifo.to_string_lossy().into_owned();
    let output = fixture.cli(&[
        "new",
        "--project",
        "fixture",
        "--workspace",
        "work",
        "--name",
        "history",
        "--",
        "sh",
        "-c",
        "fifo=\"$1\"; rm -f \"$fifo\"; mkfifo \"$fifo\"; i=0; while [ \"$i\" -lt 100 ]; do printf 'HIST_OLD_%03d\\n' \"$i\"; i=$((i+1)); done; printf '\\033[2J\\033[H'; printf 'HISTORY_READY\\n'; while IFS= read -r token; do printf 'HIST_NEW_%s\\n' \"$token\"; done < \"$fifo\"",
        "history-child",
        &fifo_arg,
    ])?;
    require_success(output, "history session")?;
    let history_id = fixture
        .list()?
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .find(|session| session.name == "history")
        .context("history session id")?
        .id
        .0;
    fixture.managed_pgids = fixture.session_pgids()?;
    let fifo_deadline = Instant::now() + Duration::from_secs(3);
    while !fifo.exists() && Instant::now() < fifo_deadline {
        thread::yield_now();
    }
    if !fifo.exists() {
        bail!("history child did not create its FIFO");
    }
    let ready_deadline = Instant::now() + Duration::from_secs(3);
    let mut ready_seen = false;
    while Instant::now() < ready_deadline {
        if fixture
            .read_terminal(ovrcr::session::SessionId(history_id))?
            .contains("HISTORY_READY")
        {
            ready_seen = true;
            break;
        }
        thread::yield_now();
    }
    assert!(
        ready_seen,
        "history child did not render its initial marker"
    );

    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.wait_for(b"agent runtime", Duration::from_secs(3))?;
    dashboard.wait_for_screen(|screen| screen.contains("history"), Duration::from_secs(3))?;
    dashboard.click_visible_text("history")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("HISTORY_READY"),
        Duration::from_secs(3),
    )?;
    dashboard.send(b"\r")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("Terminal mode"),
        Duration::from_secs(3),
    )?;
    dashboard.send(b"\x07")?;
    dashboard.wait_for_screen(
        |screen| !screen.contains("Terminal mode"),
        Duration::from_secs(3),
    )?;
    dashboard.wait_for_screen(
        |screen| screen.contains("j/k/↑/↓") && screen.contains("Ctrl-g browse"),
        Duration::from_secs(3),
    )?;
    let page_up = b"\x1b[5~";
    dashboard.send(page_up)?;
    dashboard.wait_for_screen(
        |screen| screen.contains("HISTORY · frozen"),
        Duration::from_secs(3),
    )?;
    let mut old_marker_visible = false;
    for _ in 0..10 {
        dashboard.send(page_up)?;
        if dashboard
            .wait_for_screen(
                |screen| screen.contains("HIST_OLD_000"),
                Duration::from_millis(500),
            )
            .is_ok()
        {
            old_marker_visible = true;
            break;
        }
    }
    assert!(
        old_marker_visible,
        "PageUp did not reveal old history output"
    );
    let frozen = dashboard.rendered();
    assert!(frozen.contains("HIST_OLD_000"));
    assert!(frozen.contains("HISTORY · frozen"));

    write_fifo_bounded(&fifo, b"LIVE_TOKEN\n", Duration::from_secs(3))?;
    let live_deadline = Instant::now() + Duration::from_secs(3);
    let mut live_seen = false;
    while Instant::now() < live_deadline {
        if fixture
            .read_terminal(ovrcr::session::SessionId(history_id))?
            .contains("HIST_NEW_LIVE_TOKEN")
        {
            live_seen = true;
            break;
        }
        thread::yield_now();
    }
    assert!(
        live_seen,
        "fixture terminal did not observe live output while history was frozen"
    );
    dashboard.wait_for_screen(
        |screen| {
            screen.contains("HIST_OLD_000") && screen.contains("HISTORY · frozen · new output")
        },
        Duration::from_secs(3),
    )?;
    let frozen_with_new_output = dashboard.rendered();
    assert!(frozen_with_new_output.contains("HIST_OLD_000"));
    assert!(frozen_with_new_output.contains("HISTORY · frozen · new output"));
    let frozen_status = frozen_with_new_output
        .lines()
        .find(|line| line.contains("HISTORY · frozen · new output"))
        .context("frozen history status line")?
        .to_owned();
    dashboard.resize(40, 120)?;
    dashboard.wait_for_screen(
        |screen| {
            screen.contains("HIST_OLD_000") && screen.contains("HISTORY · frozen · new output")
        },
        Duration::from_secs(3),
    )?;
    let resized = dashboard.rendered();
    assert!(resized.contains("HIST_OLD_000"));
    assert!(resized.contains("HISTORY · frozen · new output"));
    let resized_status = resized
        .lines()
        .find(|line| line.contains("HISTORY · frozen · new output"))
        .context("resized history status line")?;
    assert_ne!(resized_status, frozen_status);
    dashboard.send(b"\x07")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("HIST_NEW_LIVE_TOKEN"),
        Duration::from_secs(3),
    )?;
    dashboard.detach()?;
    fixture.shutdown()?;
    Ok(())
}

#[test]
fn default_dashboard_acceptance_wrapper_exercises_pty_controls() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
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
    dashboard.wait_for(b"mouse", Duration::from_secs(3))?;
    dashboard.wait_for(b"agent runtime", Duration::from_secs(3))?;
    dashboard.send(b"\x1b[<0;5;10M")?;
    dashboard.wait_for(b"MOUSE_READY", Duration::from_secs(3))?;
    dashboard.send(b"k")?;
    dashboard.wait_for(b"WAITING_READY", Duration::from_secs(3))?;
    dashboard.send(b"\r")?;
    dashboard.send(b"\x1b[200~PASTE_TOKEN\x1b[201~")?;
    dashboard.send(b"\r")?;
    dashboard.wait_for(b"PASTE_ACK", Duration::from_secs(3))?;
    dashboard.send(b"INPUT_TOKEN\r")?;
    dashboard.wait_for(b"INPUT_ACK", Duration::from_secs(3))?;
    let mut latencies = Vec::new();
    for index in 0..20 {
        let token = format!("LATENCY_TOKEN_{index}");
        let needle = format!("ECHO_{token}");
        let started = Instant::now();
        dashboard.send(format!("{token}\r").as_bytes())?;
        dashboard.wait_for(needle.as_bytes(), Duration::from_secs(3))?;
        latencies.push(started.elapsed());
    }
    let mut sorted = latencies.clone();
    sorted.sort_unstable();
    eprintln!(
        "command-to-visible-echo: p50={:?} p95={:?} max={:?}",
        sorted[sorted.len() / 2],
        sorted[sorted.len() * 19 / 20],
        sorted[sorted.len() - 1],
    );
    assert!(
        sorted[sorted.len() * 19 / 20] < Duration::from_millis(100),
        "command-to-visible echo exceeded 100ms: {sorted:?}"
    );
    dashboard.send(b"RAW_MODE\r")?;
    dashboard.wait_for(b"RAW_READY", Duration::from_secs(3))?;
    let mut single_key_latencies = Vec::new();
    for byte in b'a'..=b't' {
        let started = Instant::now();
        dashboard.send(&[byte])?;
        let needle = format!("RAW_ACK_{}", byte as char);
        dashboard.wait_for(needle.as_bytes(), Duration::from_secs(3))?;
        single_key_latencies.push(started.elapsed());
    }
    let mut sorted_single_key = single_key_latencies.clone();
    sorted_single_key.sort_unstable();
    eprintln!(
        "single-key-to-visible-echo: p50={:?} p95={:?} max={:?}",
        sorted_single_key[sorted_single_key.len() / 2],
        sorted_single_key[sorted_single_key.len() * 19 / 20],
        sorted_single_key[sorted_single_key.len() - 1],
    );
    assert!(
        sorted_single_key[sorted_single_key.len() * 19 / 20] < Duration::from_millis(100),
        "single-key visible echo exceeded 100ms: {sorted_single_key:?}"
    );
    dashboard.send(b"\x07")?;
    dashboard.wait_for_output(b"\x1b[?1000h", Duration::from_secs(3))?;
    dashboard.send(b"\x1b[<0;5;10M")?;
    dashboard.wait_for(b"MOUSE_READY", Duration::from_secs(3))?;
    dashboard.send(b"\r")?;
    dashboard.send(b"MOUSE_TOKEN\r")?;
    dashboard.wait_for(b"MOUSE_ACK", Duration::from_secs(3))?;
    dashboard.resize(40, 120)?;
    dashboard.send(b"SIZE_TOKEN\r")?;
    dashboard.wait_for(b"SIZE_ACK_36 80", Duration::from_secs(3))?;
    let rendered = dashboard.rendered();
    let resize_ack = rendered
        .split("SIZE_ACK_36 ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|cols| cols.parse::<u16>().ok())
        .context("rendered resize acknowledgement missing columns")?;
    assert_eq!(
        resize_ack, 80,
        "rendered resized pane geometry changed unexpectedly"
    );
    dashboard.send(b"\x07")?;
    dashboard.wait_for(b"agent runtime", Duration::from_secs(3))?;
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
    reattached.wait_for(b"mouse", Duration::from_secs(3))?;
    reattached.wait_for(b"agent runtime", Duration::from_secs(3))?;
    reattached.send(b"j")?;
    reattached.wait_for(b"WAITING_READY", Duration::from_secs(3))?;
    reattached.detach()?;
    fixture.shutdown()?;
    Ok(())
}

#[test]
fn pause_resume_dashboard_round_trip() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
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
    dashboard.wait_for(b"mouse", Duration::from_secs(3))?;
    dashboard.wait_for(b"agent runtime", Duration::from_secs(3))?;
    dashboard.send(b"\x1b[<0;5;10M")?;
    dashboard.wait_for(b"MOUSE_READY", Duration::from_secs(3))?;
    dashboard.send(b"k")?;
    dashboard.wait_for(b"WAITING_READY", Duration::from_secs(3))?;
    dashboard.send(b"\r\x07")?;
    dashboard.wait_for(b"p pause", Duration::from_secs(3))?;

    dashboard.send(b"p")?;
    dashboard.wait_for(b"paused", Duration::from_secs(3))?;
    let listed = fixture.list()?;
    let waiting = listed
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .flat_map(|workspace| workspace.sessions.iter())
        .find(|session| session.name == "waiting")
        .context("List did not return waiting session")?;
    assert_eq!(waiting.phase, ovrcr::session::SessionPhase::Paused);

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
    reattached.wait_for(b"agent runtime", Duration::from_secs(3))?;
    reattached.send(b"j")?;
    reattached.wait_for(b"paused", Duration::from_secs(3))?;
    reattached.send(b"r")?;
    reattached.wait_for_screen(|screen| !screen.contains("paused"), Duration::from_secs(3))?;
    reattached.send(b"\r")?;
    reattached.wait_for(b"Terminal mode", Duration::from_secs(3))?;
    reattached.send(b"PAUSE_RESUME_TOKEN\r")?;
    reattached.wait_for(b"PAUSE_RESUME_ACK", Duration::from_secs(3))?;
    reattached.detach()?;
    fixture.shutdown()?;
    Ok(())
}

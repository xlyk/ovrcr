#[path = "support/live.rs"]
mod live;
#[path = "support/mouse_app.rs"]
mod mouse_app;

use anyhow::{Context, Result, bail};
use live::{Live, Stop, group_exists, wait_deadline};
use ovrcr::protocol::{HierarchySnapshot, Request, Response};
use portable_pty::{Child as PtyChild, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// A live server hosted by the compiled `ovrcr` binary, plus the CLI and
/// terminal helpers this suite drives it with.
struct AcceptanceFixture {
    live: Live,
    /// The session groups seen at the last look. Replaced, not accumulated: a
    /// group whose session is long gone must not decide whether this fixture
    /// cleaned up after itself.
    managed_pgids: Vec<libc::pid_t>,
}

impl std::ops::Deref for AcceptanceFixture {
    type Target = Live;

    fn deref(&self) -> &Live {
        &self.live
    }
}

impl AcceptanceFixture {
    fn new() -> Result<Self> {
        Ok(Self {
            live: Live::binary(),
            managed_pgids: Vec::new(),
        })
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
            "--new-branch",
            "feature/acceptance",
            "--base",
            "main",
        ])?;
        require_success(output, "workspace create")?;
        // Default policy no longer auto-creates feature-workspace locals.
        let output = self.cli(&[
            "new",
            "--project",
            "fixture",
            "--workspace",
            "feature/acceptance",
            "--name",
            "local",
            "--",
            "sh",
            "-c",
            "while :; do sleep 3600; done",
        ])?;
        require_success(output, "feature local shell")?;
        let output = self.cli(&[
            "new",
            "--project",
            "fixture",
            "--workspace",
            "feature/acceptance",
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
            "feature/acceptance",
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

    fn start_mouse_fixture(&mut self) -> Result<()> {
        let exe = std::env::current_exe()?;
        let output = self.cli(&[
            "new",
            "--project",
            "fixture",
            "--workspace",
            "feature/acceptance",
            "--name",
            "mouse-protocol",
            "--",
            exe.to_str().context("test executable path")?,
            "--ignored",
            "--exact",
            "mouse_fixture_child",
            "--nocapture",
        ])?;
        require_success(output, "mouse-protocol session")?;
        self.managed_pgids = self.session_pgids()?;
        Ok(())
    }

    fn session_pgids(&self) -> Result<Vec<libc::pid_t>> {
        Ok(self.session_groups())
    }

    fn list(&self) -> Result<HierarchySnapshot> {
        match self.request(Request::List) {
            Response::Hierarchy(hierarchy) => Ok(hierarchy),
            response => bail!("server list did not return a hierarchy: {response:?}"),
        }
    }

    fn read_terminal(&self, session: ovrcr::session::SessionId) -> Result<String> {
        match self.request(Request::ReadTerminal {
            session,
            max_lines: None,
        }) {
            Response::TerminalText { text, .. } => Ok(text),
            response => bail!("terminal read returned an unexpected response: {response:?}"),
        }
    }

    fn shutdown(&mut self) -> Result<()> {
        let pgids = self.session_pgids()?;
        self.managed_pgids = pgids.clone();
        let output = self.cli_timeout(&["shutdown", "--kill"], Duration::from_secs(12))?;
        require_success(output, "shutdown --kill")?;
        match self.join_within(wait_deadline()) {
            Stop::Finished => {}
            Stop::Exited(status) => bail!("isolated server exited unsuccessfully: {status:?}"),
            Stop::Panicked(message) => bail!("isolated server panicked: {message}"),
            Stop::TimedOut => bail!("isolated server did not stop within {:?}", wait_deadline()),
        }
        if !live::wait_for_absent(&self.socket, wait_deadline()) {
            bail!("server socket did not disappear: {}", self.socket.display());
        }
        for pgid in pgids {
            if !live::wait_group_absent(pgid, wait_deadline()) {
                bail!("PTY process group {pgid} did not disappear");
            }
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
        if self.hosted() && self.shutdown().is_err() {
            self.terminate_managed_groups();
        }
    }
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
        command.env(
            "OVRCR_DASHBOARD_CONFIG",
            fixture.config.with_file_name("dashboard.toml"),
        );
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
        dashboard.wait_for(b"OVRCR", wait_deadline())?;
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

    fn wait_until<F>(&mut self, predicate: F, timeout: Duration) -> Result<()>
    where
        F: Fn(&str) -> bool,
    {
        let deadline = Instant::now() + timeout;
        loop {
            if predicate(&self.parser.screen().contents()) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "outer terminal did not reach expected screen state: {}",
                    self.rendered()
                );
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if let Ok(bytes) = self
                .received
                .recv_timeout(remaining.min(Duration::from_millis(50)))
            {
                self.parser.process(&bytes);
            }
        }
    }

    fn wait_for_mouse_capture(&mut self, enabled: bool, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let mode = self.parser.screen().mouse_protocol_mode();
            let active = mode != vt100::MouseProtocolMode::None;
            if active == enabled {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("mouse capture enabled={enabled} timed out, mode={mode:?}");
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if let Ok(bytes) = self
                .received
                .recv_timeout(remaining.min(Duration::from_millis(50)))
            {
                self.parser.process(&bytes);
            }
        }
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

    fn find_text_in_rect(&self, area: ratatui::layout::Rect, needle: &str) -> Result<(u16, u16)> {
        let screen = self.parser.screen();
        for row in area.y..area.y.saturating_add(area.height) {
            let mut line = String::new();
            let mut cell_starts = Vec::new();
            for col in area.x..area.x.saturating_add(area.width) {
                cell_starts.push((line.len(), col));
                let contents = screen.cell(row, col).map_or(" ", |cell| {
                    if cell.contents().is_empty() {
                        " "
                    } else {
                        cell.contents()
                    }
                });
                line.push_str(contents);
            }
            if let Some(offset) = line.find(needle) {
                let col = cell_starts
                    .iter()
                    .find_map(|(start, col)| (*start == offset).then_some(*col))
                    .context("visible text did not start on a terminal cell")?;
                return Ok((row.saturating_sub(area.y), col.saturating_sub(area.x)));
            }
        }
        bail!(
            "outer terminal did not render {needle:?} in pane {area:?}: {}",
            self.rendered()
        )
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
        let status = match receiver.recv_timeout(wait_deadline()) {
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

fn create_search_agent(
    fixture: &mut AcceptanceFixture,
    name: &str,
) -> Result<ovrcr::protocol::SessionSummary> {
    let workspace = fixture
        .list()?
        .projects
        .into_iter()
        .flat_map(|p| p.workspaces)
        .find(|w| w.name == "feature/acceptance")
        .context("fixture workspace")?;
    let response = fixture.request(Request::CreateSession(ovrcr::protocol::CreateSessionRequest {
        project: "fixture".into(), workspace: workspace.id, name: name.into(), label: None,
        kind: ovrcr::protocol::SessionKind::Agent { name: "fixture-agent".into() },
        argv: vec!["/bin/sh".into(), "-c".into(), format!("stty -echo; umask 077; printf '%s' \"$OVRCR_HOOK_TOKEN\" > '{}'; printf 'READY_{name}\\n'; while IFS= read -r line; do printf 'RECEIVED_{name}_%s\\n' \"$line\"; done", fixture.live.root.path().join(format!("{name}.capability")).display()).into()],
    }));
    fixture.managed_pgids = fixture.session_pgids()?;
    match response {
        Response::CreatedSession(session) => Ok(*session),
        other => bail!("agent launch failed: {other:?}"),
    }
}

#[test]
fn agent_search_selects_and_types_through_real_dashboard() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let first = create_search_agent(&mut fixture, "agent-a")?;
    let second = create_search_agent(&mut fixture, "agent-b")?;
    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.send(b"sagent-a\r")?;
    dashboard.wait_for_screen(
        |screen| {
            screen.contains("READY_agent-a")
                && screen.contains("Terminal mode")
                && !screen.contains("\u{250c} Agents")
        },
        wait_deadline(),
    )?;
    dashboard.send(b"s\r")?;
    dashboard.wait_for(b"RECEIVED_agent-a_s", wait_deadline())?;
    dashboard.send(b"\x07s")?;
    dashboard.wait_for_screen(|screen| screen.contains("\u{250c} Agents"), wait_deadline())?;
    dashboard.send(b"agent-b\r")?;
    dashboard.wait_for_screen(
        |screen| {
            screen.contains("READY_agent-b")
                && screen.contains("Terminal mode")
                && !screen.contains("\u{250c} Agents")
        },
        wait_deadline(),
    )?;
    assert!(
        !fixture.read_terminal(first.id)?.contains("agent-b"),
        "filter must not leak into the previous child"
    );
    dashboard.send(b"SEARCH_INPUT_OK\r")?;
    dashboard.wait_for(b"RECEIVED_agent-b_SEARCH_INPUT_OK", wait_deadline())?;
    assert!(
        !fixture.read_terminal(first.id)?.contains("SEARCH_INPUT_OK"),
        "payload must reach only the selected agent"
    );
    assert!(
        fixture
            .read_terminal(second.id)?
            .contains("RECEIVED_agent-b_SEARCH_INPUT_OK")
    );
    eprintln!(
        "agent search fixture: root={} socket={} groups={:?}",
        fixture.root.path().display(),
        fixture.socket.display(),
        fixture.managed_pgids
    );
    dashboard.detach()?;
    fixture.shutdown()?;
    Ok(())
}

type AgentViewGate = (Receiver<()>, Sender<()>, JoinHandle<Result<()>>);

/// Hold one real SetView's final Ok, without a production testing endpoint.
fn gate_agent_view(
    socket: &Path,
    upstream: &Path,
    target: ovrcr::protocol::SessionId,
) -> Result<AgentViewGate> {
    use std::os::unix::net::UnixListener;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    let listener = UnixListener::bind(socket)?;
    let upstream = upstream.to_owned();
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let thread = thread::spawn(move || -> Result<()> {
        let (mut front, _) = listener.accept()?;
        ovrcr::protocol::exchange_preamble(&mut front)?;
        let mut back = ovrcr::protocol::connect_server(upstream)?;
        let mut front_read = front.try_clone()?;
        let mut back_write = back.try_clone()?;
        let selected = Arc::new(AtomicU64::new(0));
        let request_id = selected.clone();
        let forward = thread::spawn(move || -> Result<()> {
            while let Ok(message) =
                ovrcr::protocol::read_frame::<ovrcr::protocol::ClientMessage>(&mut front_read)
            {
                if matches!(&message.request, Request::SetView { view } if view.focused == Some(target))
                {
                    let _ = selected.compare_exchange(
                        0,
                        message.request_id,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    );
                }
                if let Err(error) = ovrcr::protocol::write_frame(&mut back_write, &message) {
                    if error.downcast_ref::<std::io::Error>().is_some_and(|e| {
                        matches!(
                            e.kind(),
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                        )
                    }) {
                        break;
                    }
                    return Err(error);
                }
            }
            Ok(())
        });
        let result = (|| -> Result<()> {
            while let Ok(message) =
                ovrcr::protocol::read_frame::<ovrcr::protocol::ServerMessage>(&mut back)
            {
                if matches!(&message, ovrcr::protocol::ServerMessage::Response { request_id: id, response: Response::Ok } if *id != 0 && *id == request_id.load(Ordering::SeqCst))
                {
                    held_tx.send(())?;
                    release_rx.recv_timeout(wait_deadline())?;
                    request_id.store(u64::MAX, Ordering::SeqCst);
                }
                if let Err(error) = ovrcr::protocol::write_frame(&mut front, &message) {
                    // Dashboard detach exits without waiting for its final response.
                    if error.downcast_ref::<std::io::Error>().is_some_and(|e| {
                        matches!(
                            e.kind(),
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                        )
                    }) {
                        break;
                    }
                    return Err(error);
                }
            }
            Ok(())
        })();
        let _ = front.shutdown(std::net::Shutdown::Both);
        let _ = back.shutdown(std::net::Shutdown::Both);
        forward
            .join()
            .map_err(|_| anyhow::anyhow!("view proxy forwarder panicked"))??;
        result
    });
    Ok((held_rx, release_tx, thread))
}

#[test]
fn agent_search_real_loading_input_is_discarded_and_never_replayed() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let first = create_search_agent(&mut fixture, "agent-a")?;
    let second = create_search_agent(&mut fixture, "agent-b")?;
    let actual_socket = fixture.live.socket.clone();
    let proxy_socket = actual_socket.with_file_name("view-gate.sock");
    let (held, release, proxy) = gate_agent_view(&proxy_socket, &actual_socket, second.id)?;
    fixture.live.socket = proxy_socket;
    let outer = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
    );
    fixture.live.socket = actual_socket;
    let mut dashboard = outer?;
    dashboard.send(b"sagent-a\r")?;
    dashboard.wait_for_screen(
        |s| s.contains("READY_agent-a") && s.contains("Terminal mode"),
        wait_deadline(),
    )?;
    dashboard.send(b"\x07sagent-b\r")?;
    held.recv_timeout(wait_deadline())?;
    dashboard.send(b"LOADING_KBD_REJECT\r\x1b[200~LOADING_PASTE_REJECT\x1b[201~")?;
    dashboard.wait_for_screen(|s| s.contains("paste was not sent"), wait_deadline())?;
    // Leave a multi-batch burst pending at acknowledgement cutover; unlike the
    // preceding paste, do not wait for a rejection frame before releasing Ok.
    dashboard.send(
        b"CUTOVER_QUEUE_REJECT\r\x1b[200~CUTOVER_PASTE_REJECT\r\x1b[201~"
            .repeat(64)
            .as_slice(),
    )?;
    release.send(())?;
    dashboard.wait_for_screen(|s| s.contains("Terminal mode"), wait_deadline())?;
    dashboard.send(b"AFTER_GATE_OK\r")?;
    dashboard.wait_for(b"RECEIVED_agent-b_AFTER_GATE_OK", wait_deadline())?;
    for session in [first.id, second.id] {
        let text = fixture.read_terminal(session)?;
        assert!(
            !text.contains("LOADING_KBD_REJECT")
                && !text.contains("LOADING_PASTE_REJECT")
                && !text.contains("CUTOVER_QUEUE_REJECT")
                && !text.contains("CUTOVER_PASTE_REJECT"),
            "loading input must not appear later in either child: {text}"
        );
        assert!(
            !text
                .lines()
                .any(|line| line == "RECEIVED_agent-a_" || line == "RECEIVED_agent-b_"),
            "confirmation Enter must be consumed: {text}"
        );
    }
    assert!(!fixture.read_terminal(first.id)?.contains("AFTER_GATE_OK"));
    dashboard.detach()?;
    proxy
        .join()
        .map_err(|_| anyhow::anyhow!("view proxy panicked"))??;
    fixture.shutdown()?;
    Ok(())
}

#[test]
fn agent_attention_ranks_accepted_reports_and_selects_the_real_destination() -> Result<()> {
    use ovrcr::protocol as p;
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let unread = create_search_agent(&mut fixture, "agent-a-unread")?;
    let waiting = create_search_agent(&mut fixture, "agent-z-input")?;
    let mut leases = Vec::new();
    for session in [&unread, &waiting] {
        let deadline = Instant::now() + wait_deadline();
        while !fixture
            .read_terminal(session.id)?
            .contains(&format!("READY_{}", session.name))
        {
            if Instant::now() >= deadline {
                bail!("fixture reporter child did not become ready");
            }
            thread::sleep(Duration::from_millis(10));
        }
        let token = std::fs::read_to_string(
            fixture
                .live
                .root
                .path()
                .join(format!("{}.capability", session.name)),
        )?;
        assert_eq!(
            token.len(),
            64,
            "fixture capability must have expected shape"
        );
        let bytes = (0..32)
            .map(|i| u8::from_str_radix(&token[i * 2..i * 2 + 2], 16))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let capability: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("fixture capability shape"))?;
        let mut admission = p::connect_server(&fixture.socket)?;
        admission.set_read_timeout(Some(wait_deadline()))?;
        let Response::AgentOperation(p::AgentOperationResult::Reserved(reserved)) =
            p::client::request(
                &mut admission,
                1,
                Request::ReserveAgent(p::ReserveAgent {
                    session: session.id,
                    capability: p::AgentSecret(capability),
                    operation: "fixture-reserve".into(),
                    expected_epoch: 0,
                    invocation: format!("fixture-{}", session.id.0),
                    provider: p::AgentProvider::Codex,
                }),
            )?
        else {
            bail!("reserve fixture reporter");
        };
        let auth = p::SupervisorAuth {
            session: session.id,
            lease: reserved.lease,
        };
        let mut lease = p::connect_server(&fixture.socket)?;
        lease.set_read_timeout(Some(wait_deadline()))?;
        assert_eq!(
            p::client::request(&mut lease, 1, Request::SupervisorHello(auth.clone()))?,
            Response::Ok
        );
        drop(admission);
        let Response::AgentOperation(p::AgentOperationResult::Bound(binding)) =
            fixture.request(Request::Supervisor(p::SupervisorRequest {
                auth,
                operation: "fixture-bind".into(),
                command: p::AgentCommand::Bind {
                    expected_binding: None,
                    conversation: format!("conversation-{}", session.id.0),
                },
            }))
        else {
            bail!("bind fixture reporter");
        };
        let activity = |state, revision| {
            Request::AgentReport(p::AgentReport {
                session: session.id,
                capability,
                sequence: None,
                update: p::AgentUpdate::Provider(p::ProviderReport {
                    binding: binding.clone(),
                    revision,
                    observation: p::AgentObservation::Activity(p::ActivitySample {
                        state,
                        quality: p::SampleQuality::Observed,
                        turn: Some("fixture-turn".into()),
                    }),
                }),
            })
        };
        if session.id == unread.id {
            assert_eq!(
                fixture.request(activity(p::AgentActivity::ResponseReady, 1)),
                Response::Ok
            );
        }
        assert_eq!(
            fixture.request(activity(p::AgentActivity::Busy, 2)),
            Response::Ok
        );
        if session.id == waiting.id {
            assert_eq!(
                fixture.request(Request::AgentReport(p::AgentReport {
                    session: session.id,
                    capability,
                    sequence: None,
                    update: p::AgentUpdate::Provider(p::ProviderReport {
                        binding,
                        revision: 1,
                        observation: p::AgentObservation::Input(vec![p::InputRequest {
                            id: "question:fixture".into(),
                            kind: p::InputKind::Confirm
                        }])
                    })
                })),
                Response::Ok
            );
        }
        leases.push(lease);
    }
    let rows = fixture.list()?;
    let rows = rows
        .projects
        .iter()
        .flat_map(|p| &p.workspaces)
        .flat_map(|w| &w.sessions)
        .collect::<Vec<_>>();
    assert!(
        rows.iter()
            .find(|s| s.id == unread.id)
            .unwrap()
            .unread
            .is_some()
    );
    assert_eq!(
        rows.iter()
            .find(|s| s.id == waiting.id)
            .unwrap()
            .agent
            .as_ref()
            .unwrap()
            .effective_activity(),
        p::AgentActivity::WaitingInput
    );
    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.send(b"s")?;
    dashboard.wait_for_screen(
        |s| s.contains("Agents") && s.contains("input needed") && s.contains("Unread"),
        wait_deadline(),
    )?;
    let screen = dashboard.parser.screen().contents();
    assert!(
        screen.find("› agent-z-input").unwrap() < screen.find("  agent-a-unread").unwrap(),
        "accepted input must outrank earlier alphabetical Unread result: {screen}"
    );
    dashboard.send(b"\r")?;
    dashboard.wait_for_screen(
        |s| s.contains("READY_agent-z-input") && s.contains("Terminal mode"),
        wait_deadline(),
    )?;
    dashboard.send(b"ATTENTION_DESTINATION_OK\r")?;
    dashboard.wait_for(
        b"RECEIVED_agent-z-input_ATTENTION_DESTINATION_OK",
        wait_deadline(),
    )?;
    assert!(
        !fixture
            .read_terminal(unread.id)?
            .contains("ATTENTION_DESTINATION_OK")
    );
    let live = fixture.list()?;
    let rows = live
        .projects
        .iter()
        .flat_map(|p| &p.workspaces)
        .flat_map(|w| &w.sessions)
        .collect::<Vec<_>>();
    assert!(
        rows.iter()
            .find(|s| s.id == unread.id)
            .unwrap()
            .unread
            .is_some()
    );
    assert_eq!(
        rows.iter()
            .find(|s| s.id == waiting.id)
            .unwrap()
            .agent
            .as_ref()
            .unwrap()
            .input_requests
            .len(),
        1,
        "navigation cannot resolve accepted input requests"
    );
    dashboard.detach()?;
    drop(leases);
    fixture.shutdown()?;
    Ok(())
}

#[test]
fn agent_search_navigates_fifty_live_sessions_through_real_dashboard() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let mut last = None;
    // The fixture already owns four live shells; add agents up to the real limit.
    for index in 0..46 {
        last = Some(create_search_agent(
            &mut fixture,
            &format!("navigation-{index:02}"),
        )?);
    }
    assert_eq!(ovrcr::protocol::client::session_count(&fixture.list()?), 50);
    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.send(b"s")?;
    dashboard.wait_for_screen(|screen| screen.contains("\u{250c} Agents"), wait_deadline())?;
    dashboard.send(&b"\x1b[B".repeat(45))?;
    dashboard.wait_for_screen(|screen| screen.contains("› navigation-45"), wait_deadline())?;
    dashboard.send(b"\r")?;
    dashboard.wait_for(b"READY_navigation-45", wait_deadline())?;
    dashboard.send(b"FIFTY_NAV_OK\r")?;
    dashboard.wait_for(b"RECEIVED_navigation-45_FIFTY_NAV_OK", wait_deadline())?;
    assert!(
        fixture
            .read_terminal(last.unwrap().id)?
            .contains("RECEIVED_navigation-45_FIFTY_NAV_OK")
    );
    dashboard.detach()?;
    fixture.shutdown()?;
    Ok(())
}

#[test]
fn workspace_shortcut_creates_and_attaches_through_real_dashboard() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    eprintln!(
        "workspace fixture: config={} socket={} server_pid={:?}",
        fixture.config.display(),
        fixture.socket.display(),
        fixture.server_pid()
    );
    git(&fixture.repo, &["switch", "-c", "trunk"])?;
    git(
        &fixture.repo,
        &["commit", "--allow-empty", "-m", "remote default tip"],
    )?;
    git(&fixture.repo, &["switch", "main"])?;
    git(
        &fixture.repo,
        &["update-ref", "refs/remotes/origin/trunk", "trunk"],
    )?;
    git(
        &fixture.repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ],
    )?;
    git(&fixture.repo, &["branch", "-D", "trunk"])?;
    std::fs::write(
        fixture.config.with_file_name("dashboard.toml"),
        "branch_prefix = \"task/\"\n",
    )?;
    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    // One input burst also exercises submission while Inspect/Git are pending.
    dashboard.send(b"\x07wwizard\r")?;
    dashboard.wait_until(
        |screen| {
            screen.contains("wizard")
                && screen.contains("Terminal mode")
                && !screen.contains("┌ Create workspace")
        },
        Duration::from_secs(5),
    )?;
    fixture.managed_pgids = fixture.session_pgids()?;
    eprintln!("workspace fixture owned PGIDs: {:?}", fixture.managed_pgids);
    let hierarchy = fixture.list()?;
    let workspace = hierarchy
        .projects
        .iter()
        .find(|project| project.name == "fixture")
        .and_then(|project| {
            project
                .workspaces
                .iter()
                .find(|workspace| workspace.name == "task/wizard")
        })
        .context("new workspace missing")?;
    assert_eq!(
        workspace.sessions.len(),
        1,
        "workspace launch must create only its selected first terminal"
    );
    let terminal = workspace
        .sessions
        .first()
        .context("first terminal missing")?;
    assert_eq!(
        terminal.name, workspace.name,
        "an automatic first terminal uses the workspace's stable fallback name"
    );
    let branch = Command::new("git")
        .arg("-C")
        .arg(&workspace.path)
        .args(["branch", "--show-current"])
        .output()?;
    assert!(branch.status.success());
    assert_eq!(String::from_utf8(branch.stdout)?.trim(), "task/wizard");
    let tip = Command::new("git")
        .arg("-C")
        .arg(&workspace.path)
        .args(["rev-parse", "HEAD", "refs/remotes/origin/trunk"])
        .output()?;
    assert!(tip.status.success());
    let tips = String::from_utf8(tip.stdout)?;
    let tips: Vec<_> = tips.lines().collect();
    assert_eq!(tips.len(), 2);
    assert_eq!(
        tips[0], tips[1],
        "workspace must start from the remote default, not main"
    );
    // The output marker is not present literally in the command echo.
    dashboard.send(b"printf 'WORKSPACE_%s\\n' SHELL_OK\r")?;
    dashboard.wait_until(
        |screen| screen.contains("WORKSPACE_SHELL_OK"),
        wait_deadline(),
    )?;
    assert!(
        fixture
            .read_terminal(terminal.id)?
            .contains("WORKSPACE_SHELL_OK")
    );
    dashboard.detach()?;
    fixture.shutdown()?;
    eprintln!("workspace fixture cleanup: owned process groups and socket absent");
    Ok(())
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
fn split_terminal_acceptance_preserves_input_and_geometry() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let session_pid = |fixture: &AcceptanceFixture, name: &str| -> Result<u32> {
        fixture
            .list()?
            .projects
            .into_iter()
            .flat_map(|project| project.workspaces)
            .flat_map(|workspace| workspace.sessions)
            .find(|session| session.name == name)
            .and_then(|session| session.pid)
            .with_context(|| format!("{name} session PID"))
    };
    let waiting_pid = session_pid(&fixture, "waiting")?;
    let mouse_pid = session_pid(&fixture, "mouse")?;
    eprintln!("split outer session PIDs: waiting={waiting_pid} mouse={mouse_pid}");

    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.wait_until(|screen| screen.contains("waiting"), wait_deadline())?;
    dashboard.click_visible_text("waiting")?;
    dashboard.wait_for(b"WAITING_READY", wait_deadline())?;
    dashboard.send(b"v")?;
    dashboard.wait_for(b"MOUSE_READY", wait_deadline())?;
    dashboard.send(b"\rMOUSE_TOKEN\r")?;
    dashboard.wait_for(b"MOUSE_ACK", wait_deadline())?;
    dashboard.send(b"\x07\t")?;
    dashboard.wait_for(b"> waiting", wait_deadline())?;
    dashboard.send(b"\rWAITING_TOKEN\r")?;
    dashboard.wait_for(b"WAITING_ACK", wait_deadline())?;

    let assert_split_cells =
        |dashboard: &OuterDashboard, area: ratatui::layout::Rect| -> Result<()> {
            let rects = ovrcr::tui::pane_rects(area, 2, 0);
            anyhow::ensure!(rects.len() == 2, "expected two visible pane rectangles");
            dashboard.find_text_in_rect(rects[0].terminal, "WAITING_ACK")?;
            anyhow::ensure!(
                dashboard
                    .find_text_in_rect(rects[1].terminal, "WAITING_ACK")
                    .is_err(),
                "WAITING_ACK rendered in the mouse pane"
            );
            dashboard.find_text_in_rect(rects[1].terminal, "MOUSE_ACK")?;
            anyhow::ensure!(
                dashboard
                    .find_text_in_rect(rects[0].terminal, "MOUSE_ACK")
                    .is_err(),
                "MOUSE_ACK rendered in the waiting pane"
            );
            Ok(())
        };
    assert_split_cells(&dashboard, ratatui::layout::Rect::new(0, 0, 120, 40))?;

    dashboard.resize(30, 100)?;
    dashboard.wait_for_screen(|screen| screen.contains("> waiting 29x26"), wait_deadline())?;
    dashboard.send(b"SIZE_TOKEN\r")?;
    dashboard.wait_for(b"SIZE_ACK_26 29", wait_deadline())?;
    dashboard.send(b"\x07\t")?;
    dashboard.wait_for_screen(|screen| screen.contains("> mouse 30x26"), wait_deadline())?;
    dashboard.send(b"\rSIZE_TOKEN\r")?;
    dashboard.wait_for(b"SIZE_ACK_26 30", wait_deadline())?;
    dashboard.send(b"\x07")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("> mouse 30x26") && screen.contains("BROWSE"),
        wait_deadline(),
    )?;
    assert_split_cells(&dashboard, ratatui::layout::Rect::new(0, 0, 100, 30))?;

    dashboard.resize(30, 80)?;
    dashboard.wait_for_screen(
        |screen| screen.contains("split hidden") && screen.contains("> mouse 40x26"),
        wait_deadline(),
    )?;
    dashboard.resize(40, 120)?;
    dashboard.wait_for_screen(
        |screen| screen.contains("waiting 39x36") && screen.contains("> mouse 40x36"),
        wait_deadline(),
    )?;
    // The metadata rows are the dashboard's own parser state. Ask each pane's
    // shell what the kernel handed its PTY instead of trusting those labels.
    dashboard.send(b"\rSIZE_TOKEN\r")?;
    dashboard.wait_for(b"SIZE_ACK_36 40", wait_deadline())?;
    dashboard.send(b"\x07\t")?;
    dashboard.wait_for_screen(|screen| screen.contains("> waiting 39x36"), wait_deadline())?;
    dashboard.send(b"\rSIZE_TOKEN\r")?;
    dashboard.wait_for(b"SIZE_ACK_36 39", wait_deadline())?;
    dashboard.send(b"\x07\t")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("> mouse 40x36") && screen.contains("BROWSE"),
        wait_deadline(),
    )?;
    dashboard.send(b"x")?;
    dashboard.wait_for_screen(
        |screen| screen.lines().all(|line| line.chars().nth(79) != Some('│')),
        wait_deadline(),
    )?;
    for line in dashboard.rendered().lines() {
        anyhow::ensure!(
            line.chars().nth(79) != Some('│'),
            "split separator remained after closing the mouse pane"
        );
    }
    assert_eq!(session_pid(&fixture, "waiting")?, waiting_pid);
    assert_eq!(session_pid(&fixture, "mouse")?, mouse_pid);
    dashboard.detach()?;

    let mut reattached = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    reattached.wait_for_screen(
        |screen| {
            screen.contains("pid:") && screen.lines().all(|line| line.chars().nth(79) != Some('│'))
        },
        wait_deadline(),
    )?;
    reattached.click_visible_text("waiting")?;
    reattached.wait_for(b"WAITING_READY", wait_deadline())?;
    assert_eq!(session_pid(&fixture, "waiting")?, waiting_pid);
    assert_eq!(session_pid(&fixture, "mouse")?, mouse_pid);

    reattached.send(b"v")?;
    reattached.wait_for(b"MOUSE_READY", wait_deadline())?;
    reattached.send(b"\rMOUSE_TOKEN\r")?;
    reattached.wait_for(b"MOUSE_ACK", wait_deadline())?;
    assert_split_cells(&reattached, ratatui::layout::Rect::new(0, 0, 120, 40))?;
    assert_eq!(session_pid(&fixture, "waiting")?, waiting_pid);
    assert_eq!(session_pid(&fixture, "mouse")?, mouse_pid);
    reattached.detach()?;
    fixture.shutdown()?;
    Ok(())
}

#[test]
fn history_keyboard_reads_old_output_during_live_session() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let fifo = fixture.root.path().join("history-trigger.fifo");
    let fifo_arg = fifo.to_string_lossy().into_owned();
    let output = fixture.cli(&[
        "new",
        "--project",
        "fixture",
        "--workspace",
        "feature/acceptance",
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
    let fifo_deadline = Instant::now() + wait_deadline();
    while !fifo.exists() && Instant::now() < fifo_deadline {
        thread::yield_now();
    }
    if !fifo.exists() {
        bail!("history child did not create its FIFO");
    }
    let ready_deadline = Instant::now() + wait_deadline();
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
    dashboard
        .wait_for(b"agent runtime", wait_deadline())
        .context("history dashboard initial runtime header")?;
    dashboard
        .wait_for_screen(|screen| screen.contains("history"), wait_deadline())
        .context("history dashboard hierarchy")?;
    dashboard.click_visible_text("history")?;
    dashboard
        .wait_for_screen(|screen| screen.contains("HISTORY_READY"), wait_deadline())
        .context("history dashboard selected session snapshot")?;
    dashboard.send(b"\r")?;
    dashboard
        .wait_for_screen(|screen| screen.contains("Terminal mode"), wait_deadline())
        .context("history dashboard terminal mode")?;
    dashboard.send(b"\x07")?;
    dashboard
        .wait_for_screen(
            |screen| {
                !screen.contains("Terminal mode")
                    && screen.contains("BROWSE")
                    && screen.contains("? Help")
            },
            wait_deadline(),
        )
        .context("history dashboard browse mode")?;
    let page_up = b"\x1b[5~";
    dashboard.send(page_up)?;
    dashboard
        .wait_for_screen(
            |screen| screen.contains("HISTORY · frozen"),
            wait_deadline(),
        )
        .context("history dashboard frozen history")?;
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

    write_fifo_bounded(&fifo, b"LIVE_TOKEN\n", wait_deadline())?;
    let live_deadline = Instant::now() + wait_deadline();
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
    dashboard
        .wait_for_screen(
            |screen| {
                screen.contains("HIST_OLD_000") && screen.contains("HISTORY · frozen · new output")
            },
            wait_deadline(),
        )
        .context("history dashboard frozen history after live output")?;
    let frozen_with_new_output = dashboard.rendered();
    assert!(frozen_with_new_output.contains("HIST_OLD_000"));
    assert!(frozen_with_new_output.contains("HISTORY · frozen · new output"));
    let frozen_status = frozen_with_new_output
        .lines()
        .find(|line| line.contains("HISTORY · frozen · new output"))
        .context("frozen history status line")?
        .to_owned();
    dashboard.resize(40, 120)?;
    dashboard
        .wait_for_screen(
            |screen| {
                screen.contains("HIST_OLD_000") && screen.contains("HISTORY · frozen · new output")
            },
            wait_deadline(),
        )
        .context("history dashboard resized frozen history")?;
    let resized = dashboard.rendered();
    assert!(resized.contains("HIST_OLD_000"));
    assert!(resized.contains("HISTORY · frozen · new output"));
    let resized_status = resized
        .lines()
        .find(|line| line.contains("HISTORY · frozen · new output"))
        .context("resized history status line")?;
    assert_ne!(resized_status, frozen_status);
    dashboard.send(b"\x07")?;
    dashboard
        .wait_for_screen(
            |screen| screen.contains("HIST_NEW_LIVE_TOKEN"),
            wait_deadline(),
        )
        .context("history dashboard live output after history exit")?;
    dashboard.detach()?;
    fixture.shutdown()?;
    Ok(())
}

#[test]
fn copy_history_acceptance_emits_across_page_and_tile_boundaries() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    let fifo = fixture.root.path().join("historical-copy-trigger.fifo");
    let fifo_arg = fifo.to_string_lossy().into_owned();
    let output = fixture.cli(&[
        "new",
        "--project",
        "fixture",
        "--workspace",
        "feature/acceptance",
        "--name",
        "history-copy",
        "--",
        "sh",
        "-c",
        r##"fifo="$1"; rm -f "$fifo"; mkfifo "$fifo"; printf 'INITIAL_READY\n'; IFS= read -r token < "$fifo"; printf '\033[2J\033[H'; xs=$(printf '%*s' 125 '' | tr ' ' x); i=0; while [ "$i" -lt 40 ]; do printf 'ROW_%03d%s\r\n' "$i" "$xs"; i=$((i+1)); done; printf 'HISTORY_DONE\n'; while IFS= read -r token; do printf 'LIVE_%s\n' "$token"; done < "$fifo""##,
        "history-copy-child",
        &fifo_arg,
    ])?;
    require_success(output, "historical copy session")?;
    let history_id = fixture
        .list()?
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .find(|session| session.name == "history-copy")
        .context("historical copy session id")?
        .id;
    fixture.managed_pgids = fixture.session_pgids()?;
    let server_pid = fixture.server_pid();
    let server_pgid = server_pid.map(|pid| unsafe { libc::getpgid(pid as libc::pid_t) });
    eprintln!(
        "historical copy fixture root: {} server pid/pgid: {server_pid:?}/{server_pgid:?} session id: {:?} groups: {:?}",
        fixture.root.path().display(),
        history_id,
        fixture.managed_pgids,
    );
    let fifo_deadline = Instant::now() + wait_deadline();
    while !fifo.exists() && Instant::now() < fifo_deadline {
        thread::yield_now();
    }
    if !fifo.exists() {
        bail!("historical copy child did not create its FIFO");
    }
    let ready_deadline = Instant::now() + wait_deadline();
    let mut ready_seen = false;
    while Instant::now() < ready_deadline {
        if fixture.read_terminal(history_id)?.contains("INITIAL_READY") {
            ready_seen = true;
            break;
        }
        thread::yield_now();
    }
    assert!(
        ready_seen,
        "historical copy child did not render its ready marker"
    );

    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 180,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    let outer_pid = dashboard
        .child
        .as_ref()
        .and_then(|child| child.process_id());
    let outer_pgid = outer_pid.map(|pid| unsafe { libc::getpgid(pid as libc::pid_t) });
    eprintln!("historical copy outer dashboard pid/pgid while alive: {outer_pid:?}/{outer_pgid:?}");
    dashboard.wait_for(b"mouse", wait_deadline())?;
    dashboard.wait_for(b"agent runtime", wait_deadline())?;
    dashboard.click_visible_text("history-copy")?;
    dashboard.wait_for_screen(|screen| screen.contains("INITIAL_READY"), wait_deadline())?;
    dashboard.send(b"\r")?;
    dashboard.wait_for_screen(|screen| screen.contains("Terminal mode"), wait_deadline())?;
    dashboard.send(b"\x07")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("BROWSE  Space Menu") && !screen.contains("Terminal mode"),
        wait_deadline(),
    )?;

    write_fifo_bounded(&fifo, b"GO\n", wait_deadline())?;
    let history_done_deadline = Instant::now() + wait_deadline();
    let mut history_done = false;
    while Instant::now() < history_done_deadline {
        if fixture.read_terminal(history_id)?.contains("HISTORY_DONE") {
            history_done = true;
            break;
        }
        thread::yield_now();
    }
    assert!(
        history_done,
        "historical copy child did not render all rows"
    );

    dashboard.send(b"\x1b[5~")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("HISTORY · frozen"),
        wait_deadline(),
    )?;
    dashboard.wait_for_screen(
        |screen| screen.contains("HISTORY · frozen · loaded"),
        wait_deadline(),
    )?;
    dashboard.send(b"\x1b[H")?;
    dashboard.wait_for_screen(
        |screen| {
            screen.contains("ROW_000")
                && screen.contains("row 1-")
                && screen.contains("HISTORY  ? Help")
                && !screen.contains("Waiting for history cell")
        },
        wait_deadline(),
    )?;
    for row in 1..=15 {
        dashboard.send(b"j")?;
        let expected = format!("row {}-", row + 1);
        dashboard.wait_for_screen(
            |screen| {
                screen.contains(&expected)
                    && screen.contains("HISTORY  ? Help")
                    && !screen.contains("Waiting for history cell")
            },
            wait_deadline(),
        )?;
    }
    for col in 1..=126 {
        dashboard.send(b"l")?;
        let expected = format!("col {}-", col + 1);
        dashboard.wait_for_screen(
            |screen| {
                screen.contains(&expected)
                    && screen.contains("HISTORY  ? Help")
                    && !screen.contains("Waiting for history cell")
            },
            wait_deadline(),
        )?;
    }
    dashboard.send(b"v")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("HISTORY SELECT") && screen.contains("row 16:127"),
        wait_deadline(),
    )?;
    dashboard.send(b"j")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("HISTORY SELECT") && screen.contains("row 17:127"),
        wait_deadline(),
    )?;
    for col in 128..=130 {
        dashboard.send(b"l")?;
        let expected = format!("row 17:{col}");
        dashboard.wait_for_screen(
            |screen| screen.contains("HISTORY SELECT") && screen.contains(&expected),
            wait_deadline(),
        )?;
    }

    let expected = format!("{}\nROW_016{}", "x".repeat(6), "x".repeat(123));
    let mut expected_osc = Vec::new();
    ovrcr::tui::write_clipboard(&mut expected_osc, &expected)?;
    dashboard.send(b"y")?;
    dashboard.wait_for_output(&expected_osc, wait_deadline())?;
    dashboard.wait_for_screen(
        |screen| screen.contains("Clipboard request sent; paste to verify"),
        wait_deadline(),
    )?;
    dashboard.send(b"j")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("HISTORY SELECT") && screen.contains("row 18:130"),
        wait_deadline(),
    )?;
    dashboard.send(b"\x1b")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("BROWSE  Space Menu") && !screen.contains("HISTORY SELECT"),
        wait_deadline(),
    )?;
    let detach_result = dashboard.detach();
    eprintln!("historical copy outer dashboard detach: {detach_result:?}");
    detach_result?;
    let shutdown_result = fixture.shutdown();
    eprintln!("historical copy fixture shutdown: {shutdown_result:?}");
    shutdown_result?;
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
    dashboard.wait_for(b"mouse", wait_deadline())?;
    dashboard.wait_for(b"agent runtime", wait_deadline())?;
    dashboard.click_visible_text("     - mouse")?;
    dashboard.wait_for(b"MOUSE_READY", wait_deadline())?;
    dashboard.send(b"k")?;
    dashboard.wait_for(b"WAITING_READY", wait_deadline())?;
    dashboard.send(b"\r")?;
    dashboard.send(b"\x1b[200~PASTE_TOKEN\x1b[201~")?;
    dashboard.send(b"\r")?;
    dashboard.wait_for(b"PASTE_ACK", wait_deadline())?;
    dashboard.send(b"INPUT_TOKEN\r")?;
    dashboard.wait_for(b"INPUT_ACK", wait_deadline())?;
    let mut latencies = Vec::new();
    for index in 0..20 {
        let token = format!("LATENCY_TOKEN_{index}");
        let needle = format!("ECHO_{token}");
        let started = Instant::now();
        dashboard.send(format!("{token}\r").as_bytes())?;
        dashboard.wait_for(needle.as_bytes(), wait_deadline())?;
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
    dashboard.wait_for(b"RAW_READY", wait_deadline())?;
    let mut single_key_latencies = Vec::new();
    for byte in b'a'..=b't' {
        let started = Instant::now();
        dashboard.send(&[byte])?;
        let needle = format!("RAW_ACK_{}", byte as char);
        dashboard.wait_for(needle.as_bytes(), wait_deadline())?;
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
    dashboard.wait_for(b"BROWSE", wait_deadline())?;
    dashboard.click_visible_text("     - mouse")?;
    dashboard.wait_for(b"MOUSE_READY", wait_deadline())?;
    dashboard.send(b"\r")?;
    dashboard.send(b"MOUSE_TOKEN\r")?;
    dashboard.wait_for(b"MOUSE_ACK", wait_deadline())?;
    dashboard.resize(40, 120)?;
    let readiness_deadline = Instant::now() + wait_deadline();
    let remaining = |deadline: Instant| deadline.saturating_duration_since(Instant::now());
    dashboard.wait_for_screen(
        |screen| screen.contains("Terminal mode  Ctrl-g browse"),
        remaining(readiness_deadline),
    )?;
    dashboard.send(b"\x07")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("BROWSE  Space Menu"),
        remaining(readiness_deadline),
    )?;
    loop {
        let timeout = remaining(readiness_deadline);
        if timeout.is_zero() {
            bail!("resized dashboard did not become input-ready before deadline");
        }
        dashboard.send(b"\r")?;
        dashboard.wait_for_screen(
            |screen| {
                screen.contains("Terminal mode  Ctrl-g browse")
                    || screen.contains("Pane is loading; retry input")
            },
            timeout,
        )?;
        if dashboard
            .rendered()
            .contains("Terminal mode  Ctrl-g browse")
        {
            break;
        }
    }
    dashboard.send(b"SIZE_TOKEN\r")?;
    dashboard.wait_for(b"SIZE_ACK_36 80", wait_deadline())?;
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
    dashboard.wait_for(b"agent runtime", wait_deadline())?;
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
    reattached.wait_for(b"mouse", wait_deadline())?;
    reattached.wait_for(b"agent runtime", wait_deadline())?;
    reattached.click_visible_text("waiting")?;
    reattached.wait_for(b"WAITING_READY", wait_deadline())?;
    reattached.detach()?;
    fixture.shutdown()?;
    Ok(())
}

#[test]
fn empty_dashboard_start_screen_reports_isolated_paths() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 180,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    eprintln!(
        "start screen fixture root: {} outer pid: {:?}",
        fixture.root.path().display(),
        dashboard
            .child
            .as_ref()
            .and_then(|child| child.process_id())
    );
    dashboard.wait_for_screen(
        |screen| {
            screen.contains("Welcome to OVRCR")
                && screen.contains(fixture.config.to_str().unwrap())
                && screen.contains(fixture.socket.to_str().unwrap())
        },
        wait_deadline(),
    )?;
    dashboard.send(b"a")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("Repository") && screen.contains("Workspace root"),
        wait_deadline(),
    )?;
    dashboard.send(b"\x1b")?;
    dashboard.wait_for_screen(|screen| !screen.contains("Repository"), wait_deadline())?;
    dashboard.detach()?;
    fixture.shutdown()?;
    Ok(())
}

#[test]
fn copy_mode_acceptance_emits_selected_text_and_reattaches() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    eprintln!(
        "copy acceptance fixture root: {}",
        fixture.root.path().display()
    );
    eprintln!(
        "copy acceptance fixture session process groups while alive: {:?}",
        fixture.managed_pgids
    );
    let waiting_id = fixture
        .list()?
        .projects
        .into_iter()
        .flat_map(|project| project.workspaces)
        .flat_map(|workspace| workspace.sessions)
        .find(|session| session.name == "waiting")
        .context("waiting session id")?
        .id;

    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    let outer_pid = dashboard
        .child
        .as_ref()
        .and_then(|child| child.process_id());
    let outer_pgid = outer_pid.map(|pid| unsafe { libc::getpgid(pid as libc::pid_t) });
    eprintln!("copy acceptance outer dashboard pid/pgid while alive: {outer_pid:?}/{outer_pgid:?}");
    dashboard.wait_for(b"mouse", wait_deadline())?;
    dashboard.wait_for(b"agent runtime", wait_deadline())?;
    dashboard.click_visible_text("     - mouse")?;
    dashboard.wait_for(b"MOUSE_READY", wait_deadline())?;
    dashboard.send(b"k")?;
    dashboard.wait_for(b"WAITING_READY", wait_deadline())?;
    let waiting_screen = fixture.read_terminal(waiting_id)?;
    assert!(
        waiting_screen.starts_with("WAITING_READY"),
        "waiting snapshot did not start at (0,0): {waiting_screen:?}"
    );
    assert!(
        dashboard
            .rendered()
            .lines()
            .any(|line| line.contains("WAITING_READY")),
        "waiting session snapshot did not render its marker"
    );

    dashboard.send(b"?")?;
    dashboard.wait_for_screen(|screen| screen.contains("Which key"), wait_deadline())?;
    dashboard.send(b"\x1b")?;
    dashboard.wait_for_screen(|screen| !screen.contains("Which key"), wait_deadline())?;
    dashboard.send(b" wn")?;
    dashboard.wait_for_screen(
        |screen| {
            screen.contains("Create terminal")
                && screen.contains("Agent")
                && !screen.contains("Which key")
        },
        wait_deadline(),
    )?;
    dashboard.send(b"\x1b")?;
    dashboard.wait_for_screen(
        |screen| !screen.contains("┌ Create terminal"),
        wait_deadline(),
    )?;
    dashboard.send(b" tx")?;
    dashboard.wait_for_screen(
        |screen| {
            screen.contains("Close terminal?")
                && screen.contains(&format!("waiting (#{})", waiting_id.0))
        },
        wait_deadline(),
    )?;
    dashboard.send(b"\x1b")?;
    dashboard.wait_for_screen(
        |screen| !screen.contains("Close terminal?"),
        wait_deadline(),
    )?;
    assert_eq!(
        fixture.read_terminal(waiting_id)?,
        waiting_screen,
        "opening help and cancelling forms must preserve the live session"
    );

    dashboard.send(b"[gv")?;
    dashboard.send(b"lllllly")?;
    dashboard.wait_for_output(b"\x1b]52;c;V0FJVElORw==\x1b\\", wait_deadline())?;
    dashboard.send(b"\x1b")?;
    dashboard.wait_for_screen(
        |screen| screen.contains("BROWSE  Space Menu") && !screen.contains("COPY  "),
        wait_deadline(),
    )?;
    dashboard.send(b"\rINPUT_TOKEN\r")?;
    dashboard.wait_for(b"INPUT_ACK", wait_deadline())?;
    assert!(
        dashboard.rendered().contains("INPUT_ACK"),
        "input acknowledgement did not reach the outer terminal"
    );
    let detach_result = dashboard.detach();
    eprintln!("copy acceptance first dashboard detach: {detach_result:?}");
    detach_result?;
    eprintln!(
        "copy acceptance fixture process groups after first detach: {:?}",
        fixture.managed_pgids
    );

    let mut reattached = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    let reattached_pid = reattached
        .child
        .as_ref()
        .and_then(|child| child.process_id());
    let reattached_pgid = reattached_pid.map(|pid| unsafe { libc::getpgid(pid as libc::pid_t) });
    eprintln!(
        "copy acceptance reattached dashboard pid/pgid while alive: {reattached_pid:?}/{reattached_pgid:?}"
    );
    reattached.wait_for(b"mouse", wait_deadline())?;
    reattached.wait_for(b"agent runtime", wait_deadline())?;
    reattached.wait_for_screen(
        |screen| screen.contains("BROWSE  Space Menu") && !screen.contains("Terminal mode"),
        wait_deadline(),
    )?;
    reattached.click_visible_text("waiting")?;
    reattached.wait_for(b"INPUT_ACK", wait_deadline())?;
    let reattached_screen = reattached.rendered();
    assert!(reattached_screen.contains("BROWSE  Space Menu"));
    assert!(!reattached_screen.contains("Terminal mode"));
    assert!(!reattached_screen.contains("COPY  "));
    let pane = ovrcr::tui::actual_drawn_inner_rect(ratatui::layout::Rect::new(0, 0, 100, 30));
    let (ack_row, ack_col) = reattached.find_text_in_rect(pane, "INPUT_ACK")?;
    eprintln!("copy acceptance retained INPUT_ACK pane position: row={ack_row} col={ack_col}");
    reattached.send(b"[")?;
    reattached.wait_for_screen(|screen| screen.contains("COPY  "), wait_deadline())?;
    reattached.send(b"y")?;
    reattached.wait_for_screen(
        |screen| screen.contains("Set an anchor with v"),
        wait_deadline(),
    )?;
    let mut select_ack = vec![b'g'];
    select_ack.extend(vec![b'j'; usize::from(ack_row)]);
    select_ack.extend(vec![b'l'; usize::from(ack_col)]);
    select_ack.push(b'v');
    select_ack.extend(vec![b'l'; b"INPUT_ACK".len() - 1]);
    select_ack.push(b'y');
    reattached.send(&select_ack)?;
    reattached.wait_for_output(b"\x1b]52;c;SU5QVVRfQUNL\x1b\\", wait_deadline())?;
    let detach_result = reattached.detach();
    eprintln!("copy acceptance reattached dashboard detach: {detach_result:?}");
    detach_result?;
    let shutdown_result = fixture.shutdown();
    eprintln!("copy acceptance fixture shutdown: {shutdown_result:?}");
    shutdown_result?;
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
    dashboard.wait_for(b"mouse", wait_deadline())?;
    dashboard.wait_for(b"agent runtime", wait_deadline())?;
    dashboard.click_visible_text("     - mouse")?;
    dashboard.wait_for(b"MOUSE_READY", wait_deadline())?;
    dashboard.send(b"k")?;
    dashboard.wait_for(b"WAITING_READY", wait_deadline())?;
    dashboard.send(b"\r\x07")?;
    dashboard.wait_for(b"BROWSE  Space Menu", wait_deadline())?;

    dashboard.send(b"p")?;
    dashboard.wait_for(b"paused", wait_deadline())?;
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
    reattached.wait_for(b"agent runtime", wait_deadline())?;
    reattached.click_visible_text("waiting")?;
    reattached.wait_for(b"paused", wait_deadline())?;
    reattached.send(b"r")?;
    reattached.wait_for_screen(|screen| !screen.contains("paused"), wait_deadline())?;
    reattached.send(b"\r")?;
    reattached.wait_for(b"Terminal mode", wait_deadline())?;
    reattached.send(b"PAUSE_RESUME_TOKEN\r")?;
    reattached.wait_for(b"PAUSE_RESUME_ACK", wait_deadline())?;
    reattached.detach()?;
    fixture.shutdown()?;
    Ok(())
}

#[test]
#[ignore]
fn mouse_fixture_child() {
    mouse_app::run().expect("mouse fixture child");
}

#[test]
fn mouse_forwarding_outer_pty_round_trip() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    fixture.setup()?;
    fixture.start_mouse_fixture()?;
    let mut dashboard = OuterDashboard::start(
        &fixture,
        PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    dashboard.wait_for(b"mouse-protocol", wait_deadline())?;
    dashboard.click_visible_text("     - mouse-protocol")?;
    dashboard.wait_until(
        |screen| screen.contains("MOUSE_FIXTURE_READY"),
        Duration::from_secs(5),
    )?;
    dashboard.wait_until(|screen| !screen.contains("loading"), wait_deadline())?;
    dashboard.send(b"\r")?;
    dashboard.wait_until(|screen| screen.contains("Terminal mode"), wait_deadline())?;
    dashboard.send(b"E")?;
    dashboard.wait_for(b"MOUSE_ENABLED", wait_deadline())?;
    dashboard.wait_for_mouse_capture(true, wait_deadline())?;
    let inner = ovrcr::tui::pane_rects(ratatui::layout::Rect::new(0, 0, 100, 30), 1, 0)
        .into_iter()
        .next()
        .context("focused pane rect")?
        .terminal;
    dashboard.send(format!("\x1b[<0;{};{}M", inner.x + 3, inner.y + 4).as_bytes())?;
    dashboard.send(format!("\x1b[<0;{};{}mQ", inner.x + 3, inner.y + 4).as_bytes())?;
    dashboard.wait_for(
        b"MOUSE_CHECK_1:1b5b3c303b333b344d1b5b3c303b333b346d:END",
        wait_deadline(),
    )?;
    dashboard.send(b"D")?;
    dashboard.wait_for(b"MOUSE_DISABLED", wait_deadline())?;
    dashboard.send(format!("\x1b[<0;{};{}MQ", inner.x + 3, inner.y + 4).as_bytes())?;
    dashboard.wait_for(b"MOUSE_CHECK_2::END", wait_deadline())?;

    // A plain shell never asks for mouse reports, so the same wheel tick over
    // its pane has to reach the dashboard's own history instead of the child.
    dashboard.send(b"\x07")?;
    dashboard.wait_for_screen(|screen| screen.contains("BROWSE"), wait_deadline())?;
    dashboard.click_visible_text("     - waiting")?;
    dashboard.wait_until(|screen| screen.contains("WAITING_READY"), wait_deadline())?;
    dashboard.send(b"\r")?;
    dashboard.wait_until(|screen| screen.contains("Terminal mode"), wait_deadline())?;
    dashboard.send(format!("\x1b[<64;{};{}M", inner.x + 3, inner.y + 4).as_bytes())?;
    dashboard.wait_until(
        |screen| screen.contains("HISTORY · frozen"),
        wait_deadline(),
    )?;
    dashboard.send(b"\x07")?;
    dashboard.wait_for_screen(|screen| screen.contains("BROWSE"), wait_deadline())?;
    dashboard.detach()?;
    fixture.shutdown()?;
    Ok(())
}

use super::render::{draw_dashboard, pane_size};
use super::terminal_guard::TerminalGuard;
use super::{
    DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, DashboardAction, InputMode,
    PANIC_TERMINAL_RESTORED, TreeRow, write_clipboard,
};
use crate::protocol::{ClientMessage, Request, Response, ServerMessage};
use crate::session::TerminalSize;
use anyhow::{Context, Result, bail};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::{execute, terminal as crossterm_terminal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::panic;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const DASHBOARD_INPUT_BATCH_LIMIT: usize = 32;
const DASHBOARD_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const DASHBOARD_EVENT_PROBE: Duration = Duration::from_micros(100);
pub(super) const DASHBOARD_IDLE_REDRAW_INTERVAL: Duration = Duration::from_secs(1);

pub fn dashboard_message_channel() -> (
    mpsc::SyncSender<ServerMessage>,
    mpsc::Receiver<ServerMessage>,
) {
    mpsc::sync_channel(DASHBOARD_READER_QUEUE_CAPACITY)
}

pub fn run_dashboard(mut stream: UnixStream) -> Result<()> {
    let size = terminal_size()?;
    let pane_size = pane_size(size);
    write_client(&mut stream, 1, Request::DashboardHello)?;
    let initial = read_server(&mut stream)?;
    dashboard_hello_result(&initial)?;
    let mut dashboard = Dashboard::new(pane_size);
    dashboard.handle_server_message(initial);
    write_client(
        &mut stream,
        2,
        Request::DashboardGeometry { size: pane_size },
    )?;
    let geometry_ack = read_server(&mut stream)?;
    dashboard.handle_server_message(geometry_ack);
    let first_session = dashboard
        .visible_rows()
        .into_iter()
        .find_map(|row| match row {
            TreeRow::Session { id } => Some(id),
            TreeRow::Project { .. } | TreeRow::Workspace { .. } => None,
        });
    if let Some(id) = first_session {
        write_client(&mut stream, 3, dashboard.select_request(id, 3).request)?;
        if let Ok(message) = read_server(&mut stream) {
            dashboard.handle_server_message(message);
        }
    }

    let mut guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(guard.writer_mut());
    let mut terminal = Terminal::new(backend).context("create dashboard terminal")?;
    // Initialize crossterm's singleton event source before adding the
    // dashboard's SIGWINCH self-pipe handler.
    let _ = event::poll(DASHBOARD_EVENT_PROBE)?;
    let reader_stream = stream.try_clone().context("clone dashboard socket")?;
    let input = dashboard_input()?.context("dashboard input terminal is unavailable")?;
    let (wake, reader_wake) = DashboardWake::new()?;
    let (received, messages) = dashboard_message_channel();
    thread::Builder::new()
        .name("ovrcr-dashboard-reader".into())
        .spawn(move || read_messages(reader_stream, received, reader_wake))?;
    let prior_hook = Arc::new(Mutex::new(Some(panic::take_hook())));
    let hook_prior = Arc::clone(&prior_hook);
    panic::set_hook(Box::new(move |panic_info| {
        let mut cleanup = TerminalGuard::with_writer(io::stdout());
        cleanup.raw = true;
        cleanup.alternate = true;
        cleanup.mouse = true;
        cleanup.cursor_hidden = true;
        cleanup.bracketed_paste = true;
        cleanup.restore_before(|| {
            PANIC_TERMINAL_RESTORED.with(|restored| restored.set(true));
            if let Some(prior) = hook_prior.lock().ok().and_then(|mut hooks| hooks.take()) {
                prior(panic_info);
            }
        });
    }));
    let result = dashboard_loop(
        &mut terminal,
        &mut stream,
        &mut dashboard,
        &messages,
        input,
        wake,
    );
    drop(terminal);
    guard.mark_mouse(dashboard.mode != InputMode::Terminal);
    let _ = panic::take_hook();
    if let Some(prior) = prior_hook.lock().ok().and_then(|mut hooks| hooks.take()) {
        panic::set_hook(prior);
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
    result
}

fn dashboard_loop<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    messages: &mpsc::Receiver<ServerMessage>,
    input: File,
    mut wake: DashboardWake,
) -> Result<()> {
    let mut mouse_enabled = dashboard.mode == InputMode::Browse;
    let input_fd = input.as_raw_fd();
    let mut next_idle_redraw = Instant::now() + dashboard.redraw_interval();
    let mut next_frame_redraw = Instant::now();
    let mut pending_redraw = false;
    let mut first_frame = true;
    loop {
        let outer = terminal.size()?;
        let next_size = pane_size(TerminalSize {
            rows: outer.height,
            cols: outer.width,
        });
        let request_id = dashboard.next_request_id();
        if let Some(request) = dashboard.resize_request(next_size, request_id) {
            write_frame(stream, &request)?;
        }
        if let Some(request) = dashboard.history_request_if_needed() {
            write_frame(stream, &request)?;
        }
        wake.clear()?;
        pending_redraw |= next_dashboard_messages(messages, dashboard, stream)?;
        pending_redraw |= emit_pending_history_copy_if_idle(terminal, stream, dashboard)?;
        pending_redraw |= first_frame;
        first_frame = false;

        // Crossterm may already have parsed an event while reading an escape
        // sequence. Check its buffered event queue before polling the raw fd.
        if event::poll(DASHBOARD_EVENT_PROBE)? {
            if process_dashboard_input(terminal, stream, dashboard, &mut mouse_enabled)? {
                break;
            }
            pending_redraw = true;
            next_dashboard_messages(messages, dashboard, stream)?;
            emit_pending_history_copy_if_idle(terminal, stream, dashboard)?;
        }
        if pending_redraw && Instant::now() >= next_frame_redraw {
            update_mouse_capture(terminal, dashboard, &mut mouse_enabled)?;
            terminal.draw(|frame| draw_dashboard(frame, dashboard))?;
            pending_redraw = false;
            next_frame_redraw = Instant::now() + DASHBOARD_FRAME_INTERVAL;
            next_idle_redraw = Instant::now() + dashboard.redraw_interval();
            continue;
        }

        {
            let now = Instant::now();
            let idle_wait = next_idle_redraw.saturating_duration_since(now);
            let frame_wait = next_frame_redraw.saturating_duration_since(now);
            let wait = if pending_redraw && frame_wait < idle_wait {
                wait_for_dashboard_activity(input_fd, None, frame_wait)?
            } else {
                wait_for_dashboard_activity(input_fd, Some(wake.receiver()), idle_wait)?
            };
            // A terminal resize and keyboard input can become ready together.
            // Send the new PTY geometry before forwarding that input.
            let outer = terminal.size()?;
            let next_size = pane_size(TerminalSize {
                rows: outer.height,
                cols: outer.width,
            });
            let request_id = dashboard.next_request_id();
            if let Some(request) = dashboard.resize_request(next_size, request_id) {
                write_frame(stream, &request)?;
            }
            if let Some(request) = dashboard.history_request_if_needed() {
                write_frame(stream, &request)?;
            }
            // If both are ready, process keyboard input first so output floods
            // cannot delay a user's command.
            // Raw descriptor readiness can represent an incomplete escape
            // sequence. Let crossterm confirm a complete event before read().
            let input_ready = if wait.input_ready {
                // The raw descriptor can wake for the first byte of an
                // escape sequence. Give crossterm a short bounded window to
                // finish parsing it before deciding whether read() is safe.
                event::poll(DASHBOARD_EVENT_PROBE)?
            } else if wait.server_ready || wait.timed_out {
                event::poll(DASHBOARD_EVENT_PROBE)?
            } else {
                false
            };
            if input_ready
                && process_dashboard_input(terminal, stream, dashboard, &mut mouse_enabled)?
            {
                break;
            }
            if input_ready {
                let outer = terminal.size()?;
                let next_size = pane_size(TerminalSize {
                    rows: outer.height,
                    cols: outer.width,
                });
                let request_id = dashboard.next_request_id();
                if let Some(request) = dashboard.resize_request(next_size, request_id) {
                    write_frame(stream, &request)?;
                }
                if let Some(request) = dashboard.history_request_if_needed() {
                    write_frame(stream, &request)?;
                }
            }
            if input_ready {
                pending_redraw = true;
                for _ in 1..DASHBOARD_INPUT_BATCH_LIMIT {
                    if !event::poll(DASHBOARD_EVENT_PROBE)? {
                        break;
                    }
                    if process_dashboard_input(terminal, stream, dashboard, &mut mouse_enabled)? {
                        return Ok(());
                    }
                }
            }
            if wait.server_ready {
                wake.clear()?;
                pending_redraw |= next_dashboard_messages(messages, dashboard, stream)?;
                pending_redraw |= emit_pending_history_copy_if_idle(terminal, stream, dashboard)?;
            }
            if wait.timed_out {
                pending_redraw = true;
            }
        }
        update_mouse_capture(terminal, dashboard, &mut mouse_enabled)?;
        if pending_redraw && Instant::now() >= next_frame_redraw {
            // Output may have arrived while the frame wait ignored the server
            // wake. Clear before draining so a producer racing this drain
            // leaves a wake for the next iteration.
            wake.clear()?;
            let _ = next_dashboard_messages(messages, dashboard, stream)?;
            let _ = emit_pending_history_copy_if_idle(terminal, stream, dashboard)?;
            terminal.draw(|frame| draw_dashboard(frame, dashboard))?;
            pending_redraw = false;
            next_frame_redraw = Instant::now() + DASHBOARD_FRAME_INTERVAL;
            next_idle_redraw = Instant::now() + dashboard.redraw_interval();
        }
    }
    Ok(())
}

fn update_mouse_capture<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    dashboard: &Dashboard,
    mouse_enabled: &mut bool,
) -> Result<()> {
    if dashboard.mode == InputMode::Terminal {
        if *mouse_enabled {
            execute!(terminal.backend_mut(), DisableMouseCapture)
                .context("disable dashboard mouse")?;
            *mouse_enabled = false;
        }
    } else if !*mouse_enabled {
        execute!(terminal.backend_mut(), EnableMouseCapture).context("enable dashboard mouse")?;
        *mouse_enabled = true;
    }
    Ok(())
}

pub(super) fn dashboard_hello_result(message: &ServerMessage) -> Result<()> {
    if let ServerMessage::Response {
        response: Response::Error { code, message },
        ..
    } = message
    {
        bail!("dashboard hello failed ({code:?}): {message}");
    }
    Ok(())
}

pub(super) fn next_dashboard_message(
    messages: &mpsc::Receiver<ServerMessage>,
) -> Result<Option<ServerMessage>> {
    match messages.try_recv() {
        Ok(message) => Ok(Some(message)),
        Err(mpsc::TryRecvError::Empty) => Ok(None),
        Err(mpsc::TryRecvError::Disconnected) => Err(anyhow::anyhow!("dashboard connection lost")),
    }
}

fn next_dashboard_messages(
    messages: &mpsc::Receiver<ServerMessage>,
    dashboard: &mut Dashboard,
    stream: &mut UnixStream,
) -> Result<bool> {
    let mut redraw = false;
    for _ in 0..DASHBOARD_READER_QUEUE_CAPACITY {
        let Some(message) = next_dashboard_message(messages)? else {
            break;
        };
        redraw = true;
        for request in dashboard.handle_server_message(message) {
            write_frame(stream, &request)?;
        }
    }
    Ok(redraw)
}

pub(super) fn emit_pending_history_copy<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    dashboard: &mut Dashboard,
) -> bool {
    let Some(text) = dashboard.take_pending_history_copy() else {
        return false;
    };
    let result = write_clipboard(terminal.backend_mut(), &text);
    dashboard.finish_copy(result);
    true
}

fn emit_pending_history_copy_if_idle<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
) -> Result<bool> {
    if event::poll(DASHBOARD_EVENT_PROBE)? {
        return Ok(false);
    }
    let emitted = emit_pending_history_copy(terminal, dashboard);
    if emitted {
        if let Some(request) = dashboard.history_request_if_needed() {
            write_frame(stream, &request)?;
        }
    }
    Ok(emitted)
}

fn process_dashboard_input<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    mouse_enabled: &mut bool,
) -> Result<bool> {
    let event = event::read()?;
    let action = match event {
        Event::Mouse(mouse) => {
            let area = terminal.size()?.into();
            dashboard.mouse_action(mouse, area)
        }
        event @ (Event::Key(_) | Event::Paste(_)) => dashboard.event_action(event),
        Event::Resize(_, _) => DashboardAction::Redraw,
        Event::FocusGained | Event::FocusLost => DashboardAction::None,
    };
    match action {
        DashboardAction::None | DashboardAction::Redraw => Ok(false),
        DashboardAction::Detach => Ok(true),
        DashboardAction::EnterBrowse => {
            if !*mouse_enabled {
                execute!(terminal.backend_mut(), EnableMouseCapture)
                    .context("enable dashboard mouse")?;
                *mouse_enabled = true;
            }
            if let Some(request) = dashboard.take_pending_history_end() {
                write_frame(stream, &request)?;
            }
            Ok(false)
        }
        DashboardAction::PtyBytes(bytes) => {
            let request_id = dashboard.next_request_id();
            if let Some(request) = dashboard.input_request(bytes, request_id) {
                write_frame(stream, &request)?;
            }
            Ok(false)
        }
        DashboardAction::CopyText(text) => {
            let result = write_clipboard(terminal.backend_mut(), &text);
            dashboard.finish_copy(result);
            Ok(false)
        }
        DashboardAction::Request(request) => {
            write_frame(stream, &request)?;
            Ok(false)
        }
        DashboardAction::RequestBatch(requests) => {
            for request in requests {
                write_frame(stream, &request)?;
            }
            Ok(false)
        }
    }
}

pub(super) struct DashboardActivity {
    pub(super) input_ready: bool,
    pub(super) server_ready: bool,
    pub(super) timed_out: bool,
}

struct DashboardWake {
    receiver: UnixStream,
    sigwinch: signal_hook::SigId,
}

impl DashboardWake {
    fn new() -> Result<(Self, UnixStream)> {
        let (receiver, sender) = UnixStream::pair().context("create dashboard wake pair")?;
        receiver
            .set_nonblocking(true)
            .context("set dashboard wake receiver nonblocking")?;
        sender
            .set_nonblocking(true)
            .context("set dashboard wake sender nonblocking")?;
        let sigwinch = signal_hook::low_level::pipe::register(libc::SIGWINCH, sender.try_clone()?)
            .context("register dashboard resize wake")?;
        Ok((Self { receiver, sigwinch }, sender))
    }

    fn receiver(&self) -> &UnixStream {
        &self.receiver
    }

    fn clear(&mut self) -> Result<()> {
        let mut bytes = [0_u8; 128];
        loop {
            match (&self.receiver).read(&mut bytes) {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error).context("drain dashboard wake")?,
            }
        }
    }
}

impl Drop for DashboardWake {
    fn drop(&mut self) {
        signal_hook::low_level::unregister(self.sigwinch);
    }
}

fn notify_dashboard_wake(wake: &mut UnixStream) {
    match wake.write(&[1]) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        Err(error) if error.kind() == io::ErrorKind::Interrupted => notify_dashboard_wake(wake),
        Err(_) => {}
    }
}

pub(super) fn wait_for_dashboard_activity(
    input_fd: RawFd,
    wake: Option<&UnixStream>,
    timeout: Duration,
) -> Result<DashboardActivity> {
    let timeout_ms = timeout
        .as_millis()
        .saturating_add(u128::from(!timeout.is_zero()))
        .min(i32::MAX as u128) as libc::c_int;
    let wake_fd = wake.map_or(-1, UnixStream::as_raw_fd);
    let mut fds = [
        libc::pollfd {
            fd: input_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let result = {
        // SAFETY: `fds` points to two valid pollfd values for the duration of
        // the call. A negative input fd is intentionally ignored by poll.
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            return Ok(DashboardActivity {
                input_ready: false,
                server_ready: false,
                timed_out: true,
            });
        }
        result
    };
    if result < 0 {
        return Err(io::Error::last_os_error()).context("poll dashboard input and output");
    }
    if fds.iter().any(|fd| fd.revents & libc::POLLNVAL != 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "dashboard poll descriptor is invalid",
        ))
        .context("poll dashboard input and output");
    }
    Ok(DashboardActivity {
        input_ready: fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0,
        server_ready: fds[1].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0,
        timed_out: result == 0,
    })
}

fn dashboard_input() -> Result<Option<File>> {
    let input = if unsafe { libc::isatty(libc::STDIN_FILENO) == 1 } {
        File::open("/dev/stdin").context("open dashboard stdin")?
    } else {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("open dashboard controlling terminal")?
    };
    Ok(Some(input))
}

fn read_messages(
    mut stream: UnixStream,
    sender: mpsc::SyncSender<ServerMessage>,
    mut wake: UnixStream,
) {
    while let Ok(message) = read_server(&mut stream) {
        if sender.send(message).is_err() {
            break;
        }
        notify_dashboard_wake(&mut wake);
    }
    drop(sender);
    notify_dashboard_wake(&mut wake);
}

fn read_server(stream: &mut UnixStream) -> Result<ServerMessage> {
    crate::protocol::read_frame(stream)
}

fn write_client(stream: &mut UnixStream, request_id: u64, request: Request) -> Result<()> {
    crate::protocol::write_frame(
        stream,
        &ClientMessage {
            request_id,
            request,
        },
    )
}

fn write_frame(stream: &mut UnixStream, message: &ClientMessage) -> Result<()> {
    crate::protocol::write_frame(stream, message)
}

fn terminal_size() -> Result<TerminalSize> {
    let size = crossterm_terminal::size().context("read terminal size")?;
    Ok(TerminalSize {
        rows: size.1.max(1),
        cols: size.0.max(1),
    })
}

use super::render::pane_size;
use super::settings::load_dashboard_settings;
use super::terminal_guard::TerminalGuard;
use super::{
    DASHBOARD_READER_QUEUE_CAPACITY, Dashboard, DashboardAction, PANIC_TERMINAL_RESTORED, TreeRow,
    write_clipboard,
};
use crate::TaskRequestFn;
use crate::protocol::{ClientMessage, Request, Response, ServerMessage};
use crate::session::TerminalSize;
use anyhow::{Context, Result, bail};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::{execute, terminal as crossterm_terminal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::panic;
use std::path::PathBuf;
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

pub fn run_dashboard(
    mut stream: UnixStream,
    task_request: TaskRequestFn,
    settings_path: PathBuf,
    configuration_paths: (PathBuf, PathBuf),
) -> Result<()> {
    let size = terminal_size()?;
    let pane_size = pane_size(size);
    write_client(&mut stream, 1, Request::DashboardHello)?;
    let initial = read_server(&mut stream)?;
    dashboard_hello_result(&initial)?;
    let mut dashboard = Dashboard::new(pane_size);
    dashboard.configuration_paths = Some(configuration_paths);
    let (settings, settings_error) = load_dashboard_settings(&settings_path);
    dashboard.settings = settings;
    if let Some(parent) = settings_path.parent() {
        dashboard.config_dir = parent.to_path_buf();
    }
    dashboard.settings_path = Some(settings_path);
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
        // The initial hello and geometry requests reserve IDs 1 and 2, so the selection is 3.
        dashboard.next_request_id = 3;
        dashboard.select_session(id);
        let view = dashboard
            .request_view_at(Rect::new(0, 0, size.cols, size.rows))
            .context("create initial dashboard view")?;
        let expected_screens = dashboard.pending_targets();
        // The view above is in the drain; writing it here as well would send it twice.
        flush(&mut stream, &mut dashboard)?;
        read_initial_selection(
            &mut stream,
            &mut dashboard,
            view.request_id,
            expected_screens,
        )?;
    }
    dashboard.next_request_id = 4;
    // Applied last: the handshake acknowledgements above clear the banner they do not own.
    if let Some(error) = settings_error {
        dashboard.set_error(error);
    }

    let mut guard = TerminalGuard::enter()?;
    let mut mouse_enabled = guard.mouse;
    let backend = CrosstermBackend::new(guard.writer_mut());
    let mut terminal = Terminal::new(backend).context("create dashboard terminal")?;
    // Initialize crossterm's singleton event source before adding the
    // dashboard's SIGWINCH self-pipe handler.
    let _ = event::poll(DASHBOARD_EVENT_PROBE)?;
    let reader_stream = stream.try_clone().context("clone dashboard socket")?;
    let input = dashboard_input()?.context("dashboard input terminal is unavailable")?;
    let (wake, reader_wake) = DashboardWake::new()?;
    dashboard.desktop.wake = reader_wake.try_clone().ok();
    let (received, messages) = dashboard_message_channel();
    thread::Builder::new()
        .name("ovrcr-dashboard-reader".into())
        .spawn(move || read_messages(reader_stream, received, reader_wake))?;
    let prior_hook = install_panic_terminal_restore_hook(io::stdout);
    let result = dashboard_loop(
        &mut terminal,
        &mut stream,
        &mut dashboard,
        &messages,
        input,
        wake,
        task_request,
        &mut mouse_enabled,
    );
    drop(terminal);
    guard.mark_mouse(mouse_enabled);
    let _ = panic::take_hook();
    if let Some(prior) = prior_hook.lock().ok().and_then(|mut hooks| hooks.take()) {
        panic::set_hook(prior);
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
    result
}

pub(super) type PriorPanicHook =
    Arc<Mutex<Option<Box<dyn Fn(&panic::PanicHookInfo<'_>) + Sync + Send>>>>;

/// Installs a panic hook that restores the terminal, then returns a handle
/// the caller uses to reinstall whatever hook was active before this call.
///
/// The hook is process-global, but only the thread that installs it (the
/// dashboard's main loop) owns the terminal. A panic on any other thread —
/// the socket reader or a task worker — delegates straight to the hook that
/// was active before installation and never touches the terminal, since the
/// main loop is still drawing to it.
pub(super) fn install_panic_terminal_restore_hook<W, F>(make_writer: F) -> PriorPanicHook
where
    W: Write + 'static,
    F: Fn() -> W + Sync + Send + 'static,
{
    let installing_thread = thread::current().id();
    let prior_hook: PriorPanicHook = Arc::new(Mutex::new(Some(panic::take_hook())));
    let hook_prior = Arc::clone(&prior_hook);
    panic::set_hook(Box::new(move |panic_info| {
        if thread::current().id() != installing_thread {
            if let Some(prior) = hook_prior.lock().ok().and_then(|mut hooks| hooks.take()) {
                prior(panic_info);
            }
            return;
        }
        let mut cleanup = TerminalGuard::with_writer(make_writer());
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
    prior_hook
}

// The caller owns the terminal guard and needs the final mouse-capture state.
#[allow(clippy::too_many_arguments)]
fn dashboard_loop<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    messages: &mpsc::Receiver<ServerMessage>,
    input: File,
    mut wake: DashboardWake,
    task_request: TaskRequestFn,
    mouse_enabled: &mut bool,
) -> Result<()> {
    let input_fd = input.as_raw_fd();
    let mut next_idle_redraw = Instant::now() + dashboard.redraw_interval();
    let mut next_frame_redraw = Instant::now();
    let mut pending_redraw = false;
    let mut first_frame = true;
    let mut task_worker =
        crate::task_tui::TaskWorker::start(task_request).context("start task control worker")?;
    loop {
        // The palette's request rides the drain of the `emit_view_request` below.
        pending_redraw |= dashboard.poll_palette();
        pending_redraw |= task_worker.poll(dashboard.tasks.as_mut());
        let outer = terminal.size()?;
        emit_view_request(
            stream,
            dashboard,
            Rect::new(0, 0, outer.width, outer.height),
        )?;
        wake.clear()?;
        pending_redraw |= next_dashboard_messages(messages, dashboard, stream)?;
        match drain_dashboard_input_then_emit(terminal, stream, dashboard, mouse_enabled)? {
            DashboardBoundary::Detached => break,
            DashboardBoundary::InputPending => {
                pending_redraw = true;
                continue;
            }
            DashboardBoundary::Work => pending_redraw = true,
            DashboardBoundary::Idle => {}
        }
        pending_redraw |= first_frame;
        first_frame = false;
        if pending_redraw && Instant::now() >= next_frame_redraw {
            update_mouse_capture(terminal, dashboard, mouse_enabled)?;
            dashboard.draw(terminal)?;
            pending_redraw = false;
            next_frame_redraw = Instant::now() + DASHBOARD_FRAME_INTERVAL;
            next_idle_redraw = Instant::now() + dashboard.redraw_interval();
            continue;
        }

        {
            let now = Instant::now();
            // A refused view is re-sent by the `emit_view_request` below, so the wait cannot
            // outlast its backoff deadline.
            let idle_wait = dashboard
                .view_retry_deadline()
                .map_or(next_idle_redraw, |retry| next_idle_redraw.min(retry))
                .saturating_duration_since(now);
            let frame_wait = next_frame_redraw.saturating_duration_since(now);
            let wait = if pending_redraw && frame_wait < idle_wait {
                wait_for_dashboard_activity(input_fd, None, frame_wait)?
            } else {
                wait_for_dashboard_activity(input_fd, Some(wake.receiver()), idle_wait)?
            };
            // A terminal resize and keyboard input can become ready together.
            // Send the new PTY geometry before forwarding that input.
            let outer = terminal.size()?;
            emit_view_request(
                stream,
                dashboard,
                Rect::new(0, 0, outer.width, outer.height),
            )?;
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
            if input_ready && process_dashboard_input(terminal, stream, dashboard, mouse_enabled)? {
                break;
            }
            if input_ready {
                let outer = terminal.size()?;
                emit_view_request(
                    stream,
                    dashboard,
                    Rect::new(0, 0, outer.width, outer.height),
                )?;
            }
            if input_ready {
                pending_redraw = true;
                for _ in 1..DASHBOARD_INPUT_BATCH_LIMIT {
                    if !event::poll(DASHBOARD_EVENT_PROBE)? {
                        break;
                    }
                    if process_dashboard_input(terminal, stream, dashboard, mouse_enabled)? {
                        return Ok(());
                    }
                }
            }
            match drain_dashboard_input_then_emit(terminal, stream, dashboard, mouse_enabled)? {
                DashboardBoundary::Detached => return Ok(()),
                DashboardBoundary::InputPending => {
                    pending_redraw = true;
                    continue;
                }
                DashboardBoundary::Work => pending_redraw = true,
                DashboardBoundary::Idle => {}
            }
            if wait.server_ready {
                wake.clear()?;
                pending_redraw |= next_dashboard_messages(messages, dashboard, stream)?;
                match drain_dashboard_input_then_emit(terminal, stream, dashboard, mouse_enabled)? {
                    DashboardBoundary::Detached => return Ok(()),
                    DashboardBoundary::InputPending => {
                        pending_redraw = true;
                        continue;
                    }
                    DashboardBoundary::Work => pending_redraw = true,
                    DashboardBoundary::Idle => {}
                }
            }
            if wait.timed_out {
                pending_redraw = true;
            }
        }
        update_mouse_capture(terminal, dashboard, mouse_enabled)?;
        if pending_redraw && Instant::now() >= next_frame_redraw {
            // Output may have arrived while the frame wait ignored the server
            // wake. Clear before draining so a producer racing this drain
            // leaves a wake for the next iteration.
            wake.clear()?;
            next_dashboard_messages(messages, dashboard, stream)?;
            match drain_dashboard_input_then_emit(terminal, stream, dashboard, mouse_enabled)? {
                DashboardBoundary::Detached => break,
                DashboardBoundary::InputPending => {
                    pending_redraw = true;
                    continue;
                }
                DashboardBoundary::Work | DashboardBoundary::Idle => {}
            }
            dashboard.draw(terminal)?;
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
    let required = dashboard.mouse_capture_required();
    if required == *mouse_enabled {
        return Ok(());
    }
    if required {
        execute!(terminal.backend_mut(), EnableMouseCapture).context("enable dashboard mouse")?;
    } else {
        execute!(terminal.backend_mut(), DisableMouseCapture).context("disable dashboard mouse")?;
    }
    *mouse_enabled = required;
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

pub(super) fn next_dashboard_messages(
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
        // A synthetic release targets the session that is focused now; the server rejects it
        // once the replacement SetView has moved focus, and the outbox keeps it leading the batch.
        for request in dashboard.handle_server_message(message) {
            write_frame(stream, &request)?;
        }
    }
    Ok(redraw)
}

/// Sends everything the dashboard has queued, in the order it queued it.
fn flush(stream: &mut UnixStream, dashboard: &mut Dashboard) -> Result<()> {
    for message in dashboard.drain_outbox() {
        write_frame(stream, &message)?;
    }
    Ok(())
}

fn emit_view_request(stream: &mut UnixStream, dashboard: &mut Dashboard, area: Rect) -> Result<()> {
    let _ = dashboard.request_view_at(area);
    flush(stream, dashboard)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DashboardBoundary {
    Detached,
    InputPending,
    Work,
    Idle,
}

fn drain_dashboard_input_then_emit<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    mouse_enabled: &mut bool,
) -> Result<DashboardBoundary> {
    drain_dashboard_input_then_emit_with(
        terminal,
        stream,
        dashboard,
        mouse_enabled,
        || event::poll(DASHBOARD_EVENT_PROBE).map_err(Into::into),
        |terminal, stream, dashboard, mouse_enabled| {
            process_dashboard_input(terminal, stream, dashboard, mouse_enabled)
        },
    )
}

pub(super) fn drain_dashboard_input_then_emit_with<
    W: Write,
    Poll: FnMut() -> Result<bool>,
    Process: FnMut(
        &mut Terminal<CrosstermBackend<&mut W>>,
        &mut UnixStream,
        &mut Dashboard,
        &mut bool,
    ) -> Result<bool>,
>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    mouse_enabled: &mut bool,
    poll_ready: Poll,
    mut process: Process,
) -> Result<DashboardBoundary> {
    let input_boundary = drain_ready_dashboard_input(poll_ready, || {
        process(terminal, stream, dashboard, mouse_enabled)
    })?;
    if matches!(
        input_boundary,
        DashboardBoundary::Detached | DashboardBoundary::InputPending
    ) {
        return Ok(input_boundary);
    }

    let desktop_changed = dashboard.emit_desktop_notifications();
    if emit_pending_history_copy(terminal, dashboard) {
        flush(stream, dashboard)?;
        return Ok(DashboardBoundary::Work);
    }
    Ok(if desktop_changed {
        DashboardBoundary::Work
    } else {
        input_boundary
    })
}

pub(super) fn drain_ready_dashboard_input<Poll, Process>(
    mut poll_ready: Poll,
    mut process: Process,
) -> Result<DashboardBoundary>
where
    Poll: FnMut() -> Result<bool>,
    Process: FnMut() -> Result<bool>,
{
    let mut did_work = false;
    for _ in 0..DASHBOARD_INPUT_BATCH_LIMIT {
        if !poll_ready()? {
            break;
        }
        did_work = true;
        if process()? {
            return Ok(DashboardBoundary::Detached);
        }
    }
    if poll_ready()? {
        return Ok(DashboardBoundary::InputPending);
    }
    Ok(if did_work {
        DashboardBoundary::Work
    } else {
        DashboardBoundary::Idle
    })
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
        event @ (Event::Key(_) | Event::Paste(_) | Event::FocusGained | Event::FocusLost) => {
            dashboard.event_action(event)
        }
        Event::Resize(_, _) => DashboardAction::Redraw,
    };
    send_dashboard_action(terminal, stream, dashboard, mouse_enabled, action)
}

fn send_dashboard_action<W: Write>(
    terminal: &mut Terminal<CrosstermBackend<&mut W>>,
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    mouse_enabled: &mut bool,
    action: DashboardAction,
) -> Result<bool> {
    // Whatever producing the action queued — a synthetic release, the history end a browse
    // return owes — leads the request the action itself carries, as it did when each had its
    // own slot written here.
    flush(stream, dashboard)?;
    let result = match action {
        DashboardAction::None | DashboardAction::Redraw => Ok(false),
        DashboardAction::Detach => Ok(true),
        DashboardAction::EnterBrowse => Ok(false),
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
    };
    update_mouse_capture(terminal, dashboard, mouse_enabled)?;
    result
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

pub(super) fn notify_dashboard_wake(wake: &mut UnixStream) {
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

pub(super) fn read_initial_selection(
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    request_id: u64,
    expected_screens: usize,
) -> Result<()> {
    let mut screens_seen = HashSet::new();
    loop {
        let message = read_server(stream)?;
        let done = match &message {
            ServerMessage::Response {
                request_id: response_id,
                response:
                    Response::Screen {
                        session, revision, ..
                    },
            } if *response_id == request_id => {
                if dashboard
                    .handshake
                    .snapshot_matches(request_id, *revision, *session)
                {
                    screens_seen.insert(*session);
                }
                false
            }
            ServerMessage::Response {
                request_id: response_id,
                response: Response::Ok,
            } if *response_id == request_id => screens_seen.len() >= expected_screens,
            ServerMessage::Response {
                request_id: response_id,
                response: Response::Error { .. },
            } if *response_id == request_id => true,
            _ => false,
        };
        for request in dashboard.handle_server_message(message) {
            write_frame(stream, &request)?;
        }
        if done {
            return Ok(());
        }
    }
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

#[cfg(test)]
mod unread_review_tests {
    use super::*;
    use crate::protocol::{
        AgentActivity, AgentBinding, AgentProvider, HierarchySnapshot, ProjectSummary,
        ReadyObservation, ServerEvent, SessionId, SessionPhase, SessionSummary, WorkspaceSummary,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{TerminalOptions, Viewport};

    fn fixture() -> (Dashboard, SessionSummary) {
        let summary = SessionSummary {
            id: SessionId(1),
            project: "project".into(),
            workspace: "work".into(),
            name: "codex".into(),
            label: "codex".into(),
            pid: Some(1),
            started_unix_ms: 0,
            phase: SessionPhase::Running,
            activity: AgentActivity::ResponseReady,
            agent: None,
            agent_epoch: 1,
            context_usage: None,
            unread: Some(ReadyObservation {
                binding: AgentBinding {
                    provider: AgentProvider::Codex,
                    invocation: "inv".into(),
                    conversation: "conv".into(),
                    generation: 1,
                },
                turn: Some("displayed-A".into()),
                activity_revision: 2,
            }),
        };
        let mut dashboard = Dashboard::new(TerminalSize {
            rows: 20,
            cols: 120,
        });
        dashboard.hierarchy = HierarchySnapshot {
            projects: vec![ProjectSummary {
                name: "project".into(),
                workspaces: vec![WorkspaceSummary {
                    project: "project".into(),
                    name: "work".into(),
                    path: "/fixture".into(),
                    sessions: vec![summary.clone()],
                }],
            }],
        };
        dashboard.select_session(summary.id);
        (dashboard, summary)
    }

    #[test]
    fn unread_input_boundary_reviews_presented_ready_before_next_draw() {
        let (mut dashboard, first) = fixture();
        let mut bytes = Vec::new();
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(&mut bytes),
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 120, 20)),
            },
        )
        .unwrap();
        dashboard.draw(&mut terminal).unwrap();
        let (mut stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        let (sender, messages) = dashboard_message_channel();
        let mut second = first.clone();
        second.unread.as_mut().unwrap().turn = Some("unseen-B".into());
        second.unread.as_mut().unwrap().activity_revision = 4;
        sender
            .send(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
                second.clone(),
            ))))
            .unwrap();
        // This is the real loop's message-drain-before-input order. No intervening draw.
        assert!(next_dashboard_messages(&messages, &mut dashboard, &mut stream).unwrap());
        assert_eq!(
            dashboard.hierarchy.projects[0].workspaces[0].sessions[0],
            second
        );
        let mut queued = true;
        let mut mouse_enabled = false;
        let boundary = drain_dashboard_input_then_emit_with(
            &mut terminal,
            &mut stream,
            &mut dashboard,
            &mut mouse_enabled,
            || {
                let ready = queued;
                queued = false;
                Ok(ready)
            },
            |terminal, stream, dashboard, mouse_enabled| {
                let action = dashboard.event_action(Event::Key(KeyEvent::new(
                    KeyCode::Char('R'),
                    KeyModifiers::NONE,
                )));
                send_dashboard_action(terminal, stream, dashboard, mouse_enabled, action)
            },
        )
        .unwrap();
        assert_eq!(boundary, DashboardBoundary::Work);
        let sent: ClientMessage = crate::protocol::read_frame(&mut peer).unwrap();
        assert_eq!(
            sent.request,
            Request::MarkReviewed {
                session: first.id,
                expected: first.unread.unwrap(),
            },
            "queued R must name A, the last drawn observation, after B updates the hierarchy"
        );
        // Once the new frame succeeds, the same explicit action may name B.
        dashboard.draw(&mut terminal).unwrap();
        let action = dashboard.event_action(Event::Key(KeyEvent::new(
            KeyCode::Char('R'),
            KeyModifiers::NONE,
        )));
        send_dashboard_action(
            &mut terminal,
            &mut stream,
            &mut dashboard,
            &mut mouse_enabled,
            action,
        )
        .unwrap();
        let sent: ClientMessage = crate::protocol::read_frame(&mut peer).unwrap();
        assert_eq!(
            sent.request,
            Request::MarkReviewed {
                session: second.id,
                expected: second.unread.unwrap(),
            }
        );
    }

    #[test]
    fn unread_failed_draw_and_overlay_do_not_advance_presented_observation() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct Writer(Arc<AtomicBool>);
        impl Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                if self.0.load(Ordering::SeqCst) {
                    Err(io::Error::other("fixture draw failed"))
                } else {
                    Ok(())
                }
            }
        }
        let (mut dashboard, first) = fixture();
        assert_eq!(
            dashboard.key(KeyCode::Char('R')),
            DashboardAction::None,
            "no review before the first presentation"
        );
        let fail = Arc::new(AtomicBool::new(false));
        let mut writer = Writer(fail.clone());
        let mut terminal = Terminal::with_options(
            CrosstermBackend::new(&mut writer),
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 120, 20)),
            },
        )
        .unwrap();
        dashboard.draw(&mut terminal).unwrap();
        dashboard.hierarchy.projects[0].workspaces[0].sessions[0]
            .unread
            .as_mut()
            .unwrap()
            .turn = Some("unseen-B".into());
        fail.store(true, Ordering::SeqCst);
        assert!(dashboard.draw(&mut terminal).is_err());
        fail.store(false, Ordering::SeqCst);
        let DashboardAction::Request(sent) = dashboard.key(KeyCode::Char('R')) else {
            panic!("expected review request");
        };
        assert_eq!(
            sent.request,
            Request::MarkReviewed {
                session: first.id,
                expected: first.unread.clone().unwrap()
            }
        );
        dashboard.key(KeyCode::Char(':'));
        dashboard.draw(&mut terminal).unwrap();
        dashboard.key(KeyCode::Esc);
        let DashboardAction::Request(sent) = dashboard.key(KeyCode::Char('R')) else {
            panic!("expected review request");
        };
        assert_eq!(
            sent.request,
            Request::MarkReviewed {
                session: first.id,
                expected: first.unread.unwrap()
            },
            "overlay cannot commit obscured Ready state"
        );
        let mut other = dashboard.hierarchy.projects[0].workspaces[0].sessions[0].clone();
        other.id = SessionId(2);
        dashboard.hierarchy.projects[0].workspaces[0]
            .sessions
            .push(other);
        dashboard.select_session(SessionId(2));
        assert_eq!(
            dashboard.key(KeyCode::Char('R')),
            DashboardAction::None,
            "session selection needs a successful presentation before review"
        );
    }
}

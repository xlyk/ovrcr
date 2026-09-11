use super::render::{draw_dashboard, pane_size};
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
        dashboard.select_session(id);
        let view = dashboard
            .view_request(Rect::new(0, 0, size.cols, size.rows), 3)?
            .context("create initial dashboard view")?;
        let expected_screens = dashboard
            .pending_view
            .as_ref()
            .map_or(0, |pending| pending.view.targets.len());
        write_frame(&mut stream, &view)?;
        read_initial_selection(&mut stream, &mut dashboard, 3, expected_screens)?;
    }
    // The initial hello, geometry, and selection requests reserve IDs 1 through 3.
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
        let (palette_redraw, palette_request) = dashboard.poll_palette();
        pending_redraw |= palette_redraw;
        if let Some(request) = palette_request {
            write_frame(stream, &request)?;
        }
        pending_redraw |= task_worker.poll(dashboard.tasks.as_mut());
        let outer = terminal.size()?;
        emit_view_request(
            stream,
            dashboard,
            Rect::new(0, 0, outer.width, outer.height),
        )?;
        if let Some(request) = dashboard.history_request_if_needed() {
            write_frame(stream, &request)?;
        }
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
            terminal.draw(|frame| draw_dashboard(frame, dashboard))?;
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
        let requests = dashboard.handle_server_message(message);
        // A synthetic release targets the session that is focused now; the server rejects it
        // once the replacement SetView has moved focus, so it must lead the batch.
        if let Some(cleanup) = dashboard.take_mouse_cleanup() {
            write_frame(stream, &cleanup)?;
        }
        for request in requests {
            write_frame(stream, &request)?;
        }
    }
    Ok(redraw)
}

fn emit_view_request(stream: &mut UnixStream, dashboard: &mut Dashboard, area: Rect) -> Result<()> {
    let request_id = dashboard.next_request_id();
    let request = dashboard.view_request(area, request_id)?;
    if let Some(cleanup) = dashboard.take_mouse_cleanup() {
        write_frame(stream, &cleanup)?;
    }
    if let Some(request) = request {
        write_frame(stream, &request)?;
    }
    Ok(())
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
        if let Some(request) = dashboard.history_request_if_needed() {
            write_frame(stream, &request)?;
        }
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
    if let Some(cleanup) = dashboard.take_mouse_cleanup() {
        write_frame(stream, &cleanup)?;
    }
    let result = match action {
        DashboardAction::None | DashboardAction::Redraw => Ok(false),
        DashboardAction::Detach => Ok(true),
        DashboardAction::EnterBrowse => {
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

pub(super) fn read_initial_selection(
    stream: &mut UnixStream,
    dashboard: &mut Dashboard,
    request_id: u64,
    expected_screens: usize,
) -> Result<()> {
    let expected_targets = dashboard
        .pending_view
        .as_ref()
        .filter(|pending| pending.request_id == request_id)
        .map(|pending| {
            (
                pending.view.revision,
                pending
                    .view
                    .targets
                    .iter()
                    .map(|(session, _)| *session)
                    .collect::<HashSet<_>>(),
            )
        })
        .unwrap_or_default();
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
                if *revision == expected_targets.0 && expected_targets.1.contains(session) {
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

//! Dashboard-local opt-in delivery. Two lanes, one queue: a Ready keyed by response
//! identity, and an Input request keyed by request identity. Both reuse the same
//! preferences, host, visibility suppression and reconnect baseline.
use super::ready::{delivery_live, input_live};
use super::{Dashboard, DashboardAction};
#[cfg(test)]
use ovrcr_protocol::{AgentActivity, AgentProvider, SessionPhase};
use ovrcr_protocol::{
    AgentBinding, HierarchySnapshot, InputRequest, ReadyObservation, SessionId, SessionSummary,
};
use std::collections::VecDeque;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const QUEUE_CAPACITY: usize = 50;
const HOST_TIMEOUT: Duration = Duration::from_secs(2);
// The system player exits after playback; measured afplay wall time for the
// fixed sound is about 2.5 seconds on macOS.
const SOUND_TIMEOUT: Duration = Duration::from_secs(5);
const CHANNEL_DESKTOP: u8 = 1;
const CHANNEL_SOUND: u8 = 2;
const HOST_POLL: Duration = Duration::from_millis(10);

#[derive(Clone)]
enum Alert {
    Ready(ReadyObservation),
    Input {
        binding: AgentBinding,
        request: InputRequest,
    },
}

#[derive(Clone)]
struct Notification {
    session: SessionId,
    alert: Alert,
    title: &'static str,
    body: String,
}

const DELIVERY_ACTIVE: u8 = 0;
const DELIVERY_CANCELLED: u8 = 1;
const DELIVERY_FINISHED: u8 = 2;

#[derive(Clone)]
struct Delivery {
    notification: Notification,
    state: Arc<AtomicU8>,
}

impl Delivery {
    fn cancel(&self) {
        let _ = self.state.compare_exchange(
            DELIVERY_ACTIVE,
            DELIVERY_CANCELLED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

#[derive(Default)]
pub(super) struct DesktopNotifications {
    initialized: bool,
    pending: VecDeque<Notification>,
    in_flight: Option<Delivery>,
    pub(super) wake: Option<std::os::unix::net::UnixStream>,
    pub(super) notice: Option<String>,
    host: Option<DesktopHost>,
}

impl Dashboard {
    fn alert_channels(&self) -> u8 {
        let mut channels = 0;
        if self.settings.desktop_notifications {
            channels |= CHANNEL_DESKTOP;
        }
        if self.settings.ready_sound {
            channels |= CHANNEL_SOUND;
        }
        channels
    }

    pub(super) fn toggle_desktop_notifications(&mut self) -> DashboardAction {
        self.toggle_alert_setting(
            "desktop_notifications",
            "Desktop notifications",
            !self.settings.desktop_notifications,
        )
    }

    pub(super) fn toggle_ready_sound(&mut self) -> DashboardAction {
        self.toggle_alert_setting("ready_sound", "Ready sound", !self.settings.ready_sound)
    }

    fn toggle_alert_setting(&mut self, key: &str, name: &str, enabled: bool) -> DashboardAction {
        if let Some(path) = &self.settings_path {
            match super::settings::save_alert_setting(path, key, enabled) {
                Ok(settings) => self.settings = settings,
                Err(error) => {
                    self.desktop.notice = Some(format!("Could not save {name}: {error:#}"));
                    return DashboardAction::Redraw;
                }
            }
        } else {
            // Embedded dashboards constructed without a file remain in-memory.
            match key {
                "desktop_notifications" => self.settings.desktop_notifications = enabled,
                "ready_sound" => self.settings.ready_sound = enabled,
                _ => unreachable!(),
            }
        }
        self.alerts_changed(name, enabled)
    }

    fn alerts_changed(&mut self, name: &str, enabled: bool) -> DashboardAction {
        let channels = self.alert_channels();
        if channels == 0 {
            self.desktop.pending.clear();
        }
        if let Some(host) = &mut self.desktop.host {
            host.set_channels(channels);
        }
        self.desktop.notice = Some(format!("{name}: {}", if enabled { "on" } else { "off" }));
        DashboardAction::Redraw
    }

    fn desktop_session_visible(&self, id: SessionId) -> bool {
        if self.tasks.is_some() {
            return false;
        }
        self.pane_rects(self.outer_area).iter().any(|rect| {
            self.panes
                .get(rect.pane_index)
                .is_some_and(|pane| pane.session == Some(id))
        })
    }

    pub(super) fn observe_desktop_responses(&mut self, hierarchy: &HierarchySnapshot) {
        let initial = !self.desktop.initialized;
        self.desktop.initialized = true;
        let sessions = hierarchy
            .projects
            .iter()
            .flat_map(|p| &p.workspaces)
            .flat_map(|w| &w.sessions);
        let mut existing = std::collections::HashSet::new();
        for session in sessions {
            existing.insert(session.id);
            self.observe_desktop_session(session, initial);
        }
        self.unread.retain(&existing);
    }

    pub(super) fn observe_desktop_update(&mut self, session: &SessionSummary) {
        self.observe_desktop_session(session, !self.desktop.initialized);
    }

    fn observe_desktop_session(&mut self, session: &SessionSummary, initial: bool) {
        self.desktop.pending.retain(|notification| {
            notification.session != session.id
                || notification_matches_session(notification, session)
        });
        // Health and lifecycle changes do not necessarily advance activity.
        if let Some(delivery) = &self.desktop.in_flight
            && delivery.notification.session == session.id
            && !notification_matches_session(&delivery.notification, session)
        {
            delivery.cancel();
        }
        let new_unread = self.unread.observe(session);
        // Consume unread even when disabled or visible so later enable/hide cannot replay.
        if !initial
            && self.alert_channels() != 0
            && new_unread
            && delivery_live(session)
            && !self.desktop_session_visible(session.id)
            && self.desktop.pending.len() < QUEUE_CAPACITY
        {
            self.desktop.pending.push_back(Notification {
                session: session.id,
                alert: Alert::Ready(session.unread.clone().unwrap()),
                title: "OVRCR · response ready",
                body: identity_body(session),
            });
        }
        // The second lane, consumed on the same terms: answering is not a review, so this
        // never reads or writes Unread. Several requests can be open at once, and each
        // newly seen one is its own alert.
        let new_requests = self.unread.observe_request(session);
        if !initial && self.alert_channels() != 0 && !self.desktop_session_visible(session.id) {
            let binding = session.agent.as_ref().map(|agent| agent.binding.clone());
            for request in new_requests {
                if self.desktop.pending.len() >= QUEUE_CAPACITY {
                    break;
                }
                let Some(binding) = binding.clone() else {
                    break;
                };
                self.desktop.pending.push_back(Notification {
                    session: session.id,
                    alert: Alert::Input { binding, request },
                    title: "OVRCR · input needed",
                    body: identity_body(session),
                });
            }
        }
    }

    fn desktop_notification_valid(&self, notification: &Notification) -> bool {
        self.alert_channels() != 0
            && !self.desktop_session_visible(notification.session)
            && super::state::find_session(self, notification.session)
                .is_some_and(|session| notification_matches_session(notification, session))
    }

    pub(super) fn emit_desktop_notifications(&mut self) -> bool {
        // Cancellation is permanent even when the host is busy: a response seen
        // in a pane must not reappear as a notification after that pane is hidden.
        let mut pending = std::mem::take(&mut self.desktop.pending);
        pending.retain(|notification| self.desktop_notification_valid(notification));
        self.desktop.pending = pending;
        let mut changed = false;
        if let Some(delivery) = &self.desktop.in_flight {
            if !self.desktop_notification_valid(&delivery.notification) {
                delivery.cancel();
            }
            if delivery.state.load(Ordering::Acquire) == DELIVERY_FINISHED {
                self.desktop.in_flight = None;
            }
        }
        // Keep queued responses in the dashboard until the single host operation finishes.
        // They are validated against current identity and geometry at actual dispatch time.
        while self.desktop.in_flight.is_none() {
            let Some(notification) = self.desktop.pending.pop_front() else {
                break;
            };
            if !self.desktop_notification_valid(&notification) {
                continue;
            }
            if self.desktop.host.is_none() {
                let host = self
                    .desktop
                    .wake
                    .as_ref()
                    .map(std::os::unix::net::UnixStream::try_clone)
                    .transpose()
                    .and_then(|wake| DesktopHost::start(wake, self.alert_channels()));
                match host {
                    Ok(host) => self.desktop.host = Some(host),
                    Err(_) => {
                        self.desktop.notice =
                            Some(unavailable_notice(self.alert_channels()).into());
                        changed = true;
                        continue;
                    }
                }
            }
            let delivery = Delivery {
                notification,
                state: Arc::new(AtomicU8::new(DELIVERY_ACTIVE)),
            };
            if let Some(host) = &self.desktop.host
                && host.send(delivery.clone())
            {
                self.desktop.in_flight = Some(delivery);
            }
        }
        let channels = self.alert_channels();
        if let Some(host) = &self.desktop.host {
            let mut failed = 0;
            while let Ok((ticket, channel)) = host.failures.try_recv() {
                if ticket == host.epoch.load(Ordering::Acquire) {
                    // A channel disabled after its command started reports nothing.
                    failed |= channel & channels;
                }
            }
            if failed != 0 {
                self.desktop.notice = Some(unavailable_notice(failed).into());
                changed = true;
            }
        }
        changed
    }
}

fn notification_matches_session(notification: &Notification, session: &SessionSummary) -> bool {
    match &notification.alert {
        Alert::Ready(unread) => {
            delivery_live(session)
                && session.agent.as_ref().is_some_and(|agent| {
                    agent.binding == unread.binding
                        && agent
                            .activity
                            .as_ref()
                            .is_some_and(|activity| activity.turn == unread.turn)
                })
        }
        // Membership, not identity: the alert stays valid while its request is one of the
        // session's open ones, whatever else opened or closed alongside it.
        Alert::Input { binding, request } => {
            input_live(session).iter().any(|live| live.id == request.id)
                && session
                    .agent
                    .as_ref()
                    .is_some_and(|agent| agent.binding == *binding)
        }
    }
}

/// Session identity only: never a prompt title, a turn, a path or a label.
fn identity_body(session: &SessionSummary) -> String {
    format!(
        "{} / {} / {} (#{})",
        safe_identity(&session.project),
        safe_identity(&session.workspace),
        safe_identity(&session.name),
        session.id.0
    )
}

fn unavailable_notice(channels: u8) -> &'static str {
    match channels {
        CHANNEL_SOUND => "Ready sound unavailable",
        CHANNEL_DESKTOP => "Desktop notifications unavailable",
        _ => "Desktop notifications and ready sound unavailable",
    }
}

fn safe_identity(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(80).collect()
}

struct DesktopHost {
    sender: mpsc::SyncSender<(u64, Delivery)>,
    failures: mpsc::Receiver<(u64, u8)>,
    epoch: Arc<AtomicU64>,
    channels: Arc<AtomicU8>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl DesktopHost {
    fn start(
        mut wake: Option<std::os::unix::net::UnixStream>,
        channels: u8,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<(u64, Delivery)>(1);
        let (failure, failures) = mpsc::sync_channel(2);
        let epoch = Arc::new(AtomicU64::new(0));
        let channels = Arc::new(AtomicU8::new(channels));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_epoch = Arc::clone(&epoch);
        let worker_channels = Arc::clone(&channels);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("ovrcr-desktop-notifications".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    let Ok((ticket, delivery)) = receiver.recv_timeout(HOST_POLL) else {
                        continue;
                    };
                    let cancelled = || {
                        worker_epoch.load(Ordering::Acquire) != ticket
                            || worker_stop.load(Ordering::Acquire)
                            || delivery.state.load(Ordering::Acquire) != DELIVERY_ACTIVE
                    };
                    // ponytail: channels run one after another; run them concurrently
                    // if a sound-length backlog per response ever matters.
                    for (channel, command, timeout) in [
                        (
                            CHANNEL_DESKTOP,
                            host_command(&delivery.notification),
                            HOST_TIMEOUT,
                        ),
                        (CHANNEL_SOUND, sound_command(), SOUND_TIMEOUT),
                    ] {
                        // A toggle applies to every host command not yet started.
                        if cancelled() || worker_channels.load(Ordering::Acquire) & channel == 0 {
                            continue;
                        }
                        if !run_host(command, timeout, cancelled) && !cancelled() {
                            let _ = failure.try_send((ticket, channel));
                        }
                    }
                    delivery.state.store(DELIVERY_FINISHED, Ordering::Release);
                    if let Some(wake) = &mut wake {
                        super::event_loop::notify_dashboard_wake(wake);
                    }
                }
            })?;
        Ok(Self {
            sender,
            failures,
            epoch,
            channels,
            stop,
            worker: Some(worker),
        })
    }
    fn enabled(&self) -> bool {
        self.channels.load(Ordering::Acquire) != 0
    }
    fn set_channels(&mut self, channels: u8) {
        let previous = self.channels.swap(channels, Ordering::AcqRel);
        if (previous != 0) != (channels != 0) {
            self.epoch.fetch_add(1, Ordering::AcqRel);
        }
    }
    fn send(&self, delivery: Delivery) -> bool {
        self.enabled()
            && self
                .sender
                .try_send((self.epoch.load(Ordering::Acquire), delivery))
                .is_ok()
    }
}

impl Drop for DesktopHost {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_host(mut command: Command, timeout: Duration, cancelled: impl Fn() -> bool) -> bool {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if !cancelled() && Instant::now() < deadline => {
                thread::park_timeout(HOST_POLL)
            }
            _ => {
                // This command owns its process group, including a host tool's descendants.
                unsafe {
                    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                }
                let _ = child.wait();
                return false;
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn host_command(notification: &Notification) -> Command {
    let mut command = Command::new("osascript");
    command.args([
        "-e",
        "on run argv\ndisplay notification (item 1 of argv) with title (item 2 of argv)\nend run",
        "--",
        &notification.body,
        notification.title,
    ]);
    command
}

#[cfg(not(target_os = "macos"))]
fn host_command(notification: &Notification) -> Command {
    let mut command = Command::new("notify-send");
    // The notification specification interprets body markup; identity is always plain text.
    let body = notification
        .body
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    command.args([
        "--app-name=OVRCR",
        "--hint=boolean:suppress-sound:true",
        "--",
        notification.title,
        &body,
    ]);
    command
}

#[cfg(target_os = "macos")]
fn sound_command() -> Command {
    let mut command = Command::new("afplay");
    command.arg("/System/Library/Sounds/Glass.aiff");
    command
}

#[cfg(not(target_os = "macos"))]
fn sound_command() -> Command {
    let mut command = Command::new("paplay");
    command.arg("/usr/share/sounds/freedesktop/stereo/complete.oga");
    command
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::{DashboardAction, InputMode};
    use crate::protocol::{
        ActivitySample, AgentBinding, AgentSnapshot, ClientMessage, HealthSample, InputKind,
        InputRequest, ProjectSummary, ReadyObservation, ReporterHealth, Response, SampleQuality,
        ServerEvent, ServerMessage, WorkspaceSummary,
    };
    use crossterm::event::KeyCode;
    use ovrcr_protocol::TerminalSize;
    use ratatui::layout::Rect;

    fn snapshot(revision: u64, turn: &str, state: AgentActivity) -> HierarchySnapshot {
        let binding = AgentBinding {
            provider: AgentProvider::Codex,
            invocation: "PRIVATE_INVOCATION".into(),
            conversation: "PRIVATE_CONVERSATION".into(),
            generation: 1,
        };
        let unread = (state == AgentActivity::ResponseReady).then(|| ReadyObservation {
            binding: binding.clone(),
            turn: Some(turn.into()),
            activity_revision: revision,
        });
        HierarchySnapshot {
            projects: vec![ProjectSummary {
                name: "project".into(),
                workspaces: vec![WorkspaceSummary {
                    project: "project".into(),
                    name: "workspace".into(),
                    id: "workspace".into(),
                    root: false,
                    warning: None,
                    path: "/private/path-must-not-leak".into(),
                    sessions: vec![SessionSummary {
                        archived: false,
                        cwd: "/work".into(),
                        id: SessionId(1),
                        run: crate::protocol::SessionRunId(1),
                        kind: crate::protocol::SessionKind::Terminal,
                        recovery: None,
                        project: "project".into(),
                        workspace: "workspace".into(),
                        name: "session".into(),
                        title: None,
                        label: "PRIVATE_LABEL".into(),
                        pid: Some(1),
                        started_unix_ms: Some(0),
                        phase: SessionPhase::Running,
                        activity: state,
                        context_usage: None,
                        agent_epoch: 1,
                        unread,
                        agent: Some(AgentSnapshot {
                            binding,
                            activity: Some(ActivitySample {
                                state,
                                quality: SampleQuality::Observed,
                                turn: Some(turn.into()),
                            }),
                            metrics: None,
                            health: HealthSample {
                                state: ReporterHealth::Connected,
                                reason: None,
                            },
                            activity_revision: revision,
                            metrics_revision: 0,
                            health_revision: 0,
                            input_requests: Vec::new(),
                            input_revision: 0,
                        }),
                    }],
                }],
            }],
        }
    }
    fn request(id: &str, kind: InputKind) -> InputRequest {
        InputRequest {
            id: id.into(),
            kind,
        }
    }
    /// `snapshot()` with a set of Input requests open on the same agent; the underlying
    /// activity sample is untouched, exactly as the runtime publishes it.
    fn snapshot_with_requests(
        revision: u64,
        turn: &str,
        state: AgentActivity,
        open: &[InputRequest],
    ) -> HierarchySnapshot {
        let mut hierarchy = snapshot(revision, turn, state);
        let agent = session(&mut hierarchy).agent.as_mut().unwrap();
        agent.input_requests = open.to_vec();
        agent.input_revision = revision;
        hierarchy
    }
    fn request_ids(d: &Dashboard) -> Vec<String> {
        d.desktop
            .pending
            .iter()
            .map(|n| match &n.alert {
                Alert::Input { request, .. } => request.id.clone(),
                Alert::Ready(_) => "ready".into(),
            })
            .collect()
    }
    fn titles(d: &Dashboard) -> Vec<&'static str> {
        d.desktop.pending.iter().map(|n| n.title).collect()
    }
    #[test]
    fn desktop_input_request_alerts_once_per_request_identity() {
        let mut d = dashboard();
        deliver(
            &mut d,
            snapshot_with_requests(
                2,
                "a",
                AgentActivity::Busy,
                &[request("p1", InputKind::Select)],
            ),
        );
        assert_eq!(d.desktop.pending.len(), 1);
        assert_eq!(d.desktop.pending[0].title, "OVRCR · input needed");
        assert_eq!(
            d.desktop.pending[0].body,
            "project / workspace / session (#1)"
        );
        deliver(
            &mut d,
            snapshot_with_requests(
                3,
                "a",
                AgentActivity::Busy,
                &[request("p1", InputKind::Select)],
            ),
        );
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "a republication of the same request never alerts twice"
        );
        deliver(
            &mut d,
            snapshot_with_requests(
                4,
                "a",
                AgentActivity::Busy,
                &[request("p2", InputKind::Confirm)],
            ),
        );
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "a replacement drops the superseded alert and queues its own"
        );
        assert_eq!(titles(&d), ["OVRCR · input needed"]);
        d.desktop.pending.clear();
        deliver(
            &mut d,
            snapshot_with_requests(
                5,
                "a",
                AgentActivity::Busy,
                &[request("p2", InputKind::Confirm)],
            ),
        );
        assert!(
            d.desktop.pending.is_empty(),
            "a consumed request does not replay"
        );
    }
    #[test]
    fn desktop_input_request_close_removes_pending_and_cancels_in_flight() {
        let mut d = dashboard();
        let (received, _failed) = stub_host(&mut d);
        deliver(
            &mut d,
            snapshot_with_requests(
                2,
                "a",
                AgentActivity::Busy,
                &[request("p1", InputKind::Select)],
            ),
        );
        assert_eq!(d.desktop.pending.len(), 1);
        deliver(
            &mut d,
            snapshot_with_requests(3, "a", AgentActivity::Busy, &[]),
        );
        assert!(
            d.desktop.pending.is_empty(),
            "an answered request cancels its queued alert"
        );
        d.emit_desktop_notifications();
        assert!(received.try_recv().is_err());
        deliver(
            &mut d,
            snapshot_with_requests(
                4,
                "a",
                AgentActivity::Busy,
                &[request("p2", InputKind::Editor)],
            ),
        );
        d.emit_desktop_notifications();
        let (_, delivery) = received.try_recv().unwrap();
        assert_eq!(delivery.state.load(Ordering::Acquire), DELIVERY_ACTIVE);
        deliver(
            &mut d,
            snapshot_with_requests(5, "a", AgentActivity::Busy, &[]),
        );
        assert_eq!(
            delivery.state.load(Ordering::Acquire),
            DELIVERY_CANCELLED,
            "closing cancels the delivery already with the host"
        );
    }
    #[test]
    fn desktop_pending_input_alert_dies_with_its_binding_or_its_reporter() {
        fn input_generation(notification: &Notification) -> u64 {
            match &notification.alert {
                Alert::Input { binding, .. } => binding.generation,
                Alert::Ready(_) => panic!("expected an input alert"),
            }
        }
        // A rebinding invalidates the queued alert: the same request id under a new
        // generation is a different request, so the superseded alert is dropped and only
        // the new binding's alert survives to reach the host.
        let mut d = dashboard();
        let (received, _failed) = stub_host(&mut d);
        deliver(
            &mut d,
            snapshot_with_requests(
                2,
                "a",
                AgentActivity::Busy,
                &[request("p1", InputKind::Select)],
            ),
        );
        assert_eq!(d.desktop.pending.len(), 1);
        assert_eq!(input_generation(&d.desktop.pending[0]), 1);
        let mut rebound = snapshot_with_requests(
            3,
            "a",
            AgentActivity::Busy,
            &[request("p1", InputKind::Select)],
        );
        session(&mut rebound)
            .agent
            .as_mut()
            .unwrap()
            .binding
            .generation = 2;
        d.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
            session(&mut rebound).clone(),
        ))));
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "the superseded alert is dropped"
        );
        assert_eq!(
            input_generation(&d.desktop.pending[0]),
            2,
            "only the live binding's request stays deliverable"
        );
        d.emit_desktop_notifications();
        let (_, delivery) = received.try_recv().unwrap();
        assert_eq!(input_generation(&delivery.notification), 2);
        assert!(
            received.try_recv().is_err(),
            "the alert for the retired binding never reached the host"
        );

        // A lost reporter cannot vouch for the dialog: the queued alert dies with it and
        // nothing is delivered, exactly as a Ready alert dies on health loss.
        let mut d = dashboard();
        let (received, _failed) = stub_host(&mut d);
        deliver(
            &mut d,
            snapshot_with_requests(
                2,
                "a",
                AgentActivity::Busy,
                &[request("p1", InputKind::Select)],
            ),
        );
        assert_eq!(d.desktop.pending.len(), 1);
        let mut lost = snapshot_with_requests(
            3,
            "a",
            AgentActivity::Busy,
            &[request("p1", InputKind::Select)],
        );
        session(&mut lost).agent.as_mut().unwrap().health.state = ReporterHealth::Unavailable;
        d.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
            session(&mut lost).clone(),
        ))));
        assert!(
            d.desktop.pending.is_empty(),
            "an unavailable reporter invalidates the queued input alert"
        );
        d.emit_desktop_notifications();
        assert!(received.try_recv().is_err());
    }
    #[test]
    fn desktop_input_request_on_the_attach_baseline_never_replays() {
        let mut d = Dashboard::new(TerminalSize {
            rows: 24,
            cols: 120,
        });
        d.key(KeyCode::Char('N'));
        d.handle_server_message(ServerMessage::Response {
            request_id: 1,
            response: Response::Hierarchy(snapshot_with_requests(
                2,
                "a",
                AgentActivity::Busy,
                &[request("p1", InputKind::Select)],
            )),
        });
        assert!(
            d.desktop.pending.is_empty(),
            "a request open at attach is a baseline"
        );
        deliver(
            &mut d,
            snapshot_with_requests(
                3,
                "a",
                AgentActivity::Busy,
                &[request("p1", InputKind::Select)],
            ),
        );
        assert!(d.desktop.pending.is_empty(), "reconnect does not replay");
        deliver(
            &mut d,
            snapshot_with_requests(
                4,
                "a",
                AgentActivity::Busy,
                &[request("p2", InputKind::Input)],
            ),
        );
        assert_eq!(d.desktop.pending.len(), 1, "a genuinely new request alerts");
    }
    #[test]
    fn desktop_input_request_in_a_visible_pane_never_alerts() {
        let mut d = dashboard();
        d.select_session(SessionId(1));
        deliver(
            &mut d,
            snapshot_with_requests(
                2,
                "a",
                AgentActivity::Busy,
                &[request("p1", InputKind::Select)],
            ),
        );
        assert!(d.desktop.pending.is_empty(), "visible focused pane");
    }
    #[test]
    fn desktop_ready_and_input_request_are_two_lanes_that_do_not_suppress_each_other() {
        let mut d = dashboard();
        let mut hierarchy = snapshot(2, "a", AgentActivity::ResponseReady);
        let agent = session(&mut hierarchy).agent.as_mut().unwrap();
        agent.input_requests = vec![request("p1", InputKind::Select)];
        agent.input_revision = 2;
        deliver(&mut d, hierarchy.clone());
        assert_eq!(
            titles(&d),
            ["OVRCR · response ready", "OVRCR · input needed"],
            "one cycle can be both ready and waiting"
        );
        deliver(&mut d, hierarchy);
        assert_eq!(
            titles(&d),
            ["OVRCR · response ready", "OVRCR · input needed"],
            "neither lane cancels the other on republication"
        );
        assert_eq!(
            d.desktop.pending[0].session, d.desktop.pending[1].session,
            "both alerts identify the same session"
        );
    }
    #[test]
    fn desktop_several_open_requests_alert_once_each_and_close_independently() {
        let approval = request("approval:c1", InputKind::Approval);
        let question = request("question:q1", InputKind::Select);
        let mut d = dashboard();
        deliver(
            &mut d,
            snapshot_with_requests(2, "a", AgentActivity::Busy, std::slice::from_ref(&approval)),
        );
        assert_eq!(request_ids(&d), ["approval:c1"]);
        deliver(
            &mut d,
            snapshot_with_requests(
                3,
                "a",
                AgentActivity::Busy,
                &[approval.clone(), question.clone()],
            ),
        );
        assert_eq!(
            request_ids(&d),
            ["approval:c1", "question:q1"],
            "each newly opened request is its own alert"
        );
        // Closing one drops only its own pending alert: membership, not identity.
        deliver(
            &mut d,
            snapshot_with_requests(4, "a", AgentActivity::Busy, std::slice::from_ref(&question)),
        );
        assert_eq!(request_ids(&d), ["question:q1"]);
        deliver(
            &mut d,
            snapshot_with_requests(5, "a", AgentActivity::Busy, &[question]),
        );
        assert_eq!(
            request_ids(&d),
            ["question:q1"],
            "a republication of the remaining set alerts nothing new"
        );
    }
    #[test]
    fn desktop_a_reconnect_baseline_with_several_open_requests_never_replays() {
        let mut d = Dashboard::new(TerminalSize {
            rows: 24,
            cols: 120,
        });
        d.key(KeyCode::Char('N'));
        let open = [
            request("approval:c1", InputKind::Approval),
            request("question:q1", InputKind::Select),
        ];
        d.handle_server_message(ServerMessage::Response {
            request_id: 1,
            response: Response::Hierarchy(snapshot_with_requests(
                2,
                "a",
                AgentActivity::Busy,
                &open,
            )),
        });
        assert!(
            d.desktop.pending.is_empty(),
            "a whole set open at attach is a baseline"
        );
        deliver(
            &mut d,
            snapshot_with_requests(3, "a", AgentActivity::Busy, &open),
        );
        assert!(d.desktop.pending.is_empty(), "reconnect does not replay");
        deliver(
            &mut d,
            snapshot_with_requests(
                4,
                "a",
                AgentActivity::Busy,
                &[
                    open[0].clone(),
                    open[1].clone(),
                    request("approval:c2", InputKind::Approval),
                ],
            ),
        );
        assert_eq!(
            request_ids(&d),
            ["approval:c2"],
            "only the request that genuinely joined the set alerts"
        );
    }
    fn session(snapshot: &mut HierarchySnapshot) -> &mut SessionSummary {
        &mut snapshot.projects[0].workspaces[0].sessions[0]
    }
    fn deliver(d: &mut Dashboard, snapshot: HierarchySnapshot) -> Vec<ClientMessage> {
        d.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(
            snapshot,
        )))
    }
    fn dashboard() -> Dashboard {
        let mut d = Dashboard::new(TerminalSize {
            rows: 24,
            cols: 120,
        });
        d.key(KeyCode::Char('N'));
        deliver(&mut d, snapshot(1, "a", AgentActivity::Busy));
        d
    }
    fn queued_behind_active() -> (Dashboard, mpsc::Receiver<(u64, Delivery)>) {
        let mut d = dashboard();
        let mut hierarchy = snapshot(2, "a", AgentActivity::ResponseReady);
        let mut other = session(&mut hierarchy).clone();
        other.id = SessionId(2);
        other.name = "queued".into();
        other.agent.as_mut().unwrap().binding.invocation = "second-invocation".into();
        other.unread.as_mut().unwrap().binding.invocation = "second-invocation".into();
        hierarchy.projects[0].workspaces[0].sessions.push(other);
        deliver(&mut d, hierarchy);
        d.desktop.in_flight = Some(Delivery {
            notification: d.desktop.pending.pop_front().unwrap(),
            state: Arc::new(AtomicU8::new(DELIVERY_ACTIVE)),
        });
        let (receiver, _failed) = stub_host(&mut d);
        assert_eq!(d.desktop.pending.len(), 1);
        (d, receiver)
    }
    #[test]
    fn desktop_queued_visible_then_hidden_candidate_stays_cancelled() {
        let (mut d, receiver) = queued_behind_active();
        d.select_session(SessionId(2));
        d.emit_desktop_notifications();
        assert!(
            d.desktop.pending.is_empty(),
            "visible queued response must be consumed while host remains busy"
        );
        d.select_session(SessionId(1));
        d.desktop
            .in_flight
            .as_ref()
            .unwrap()
            .state
            .store(DELIVERY_FINISHED, Ordering::Release);
        d.emit_desktop_notifications();
        assert!(
            receiver.try_recv().is_err(),
            "hiding again must not restore cancelled response"
        );
    }
    #[test]
    fn desktop_queued_health_loss_then_recovery_stays_cancelled() {
        let (mut d, receiver) = queued_behind_active();
        let mut queued = d.hierarchy.projects[0].workspaces[0].sessions[1].clone();
        queued.agent.as_mut().unwrap().metrics_revision = 5;
        d.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
            queued.clone(),
        ))));
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "unrelated metrics never revoke activity"
        );
        queued.agent.as_mut().unwrap().health.state = ReporterHealth::Unavailable;
        d.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
            queued.clone(),
        ))));
        queued.agent.as_mut().unwrap().health.state = ReporterHealth::Connected;
        d.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
            queued,
        ))));
        assert!(
            d.desktop.pending.is_empty(),
            "same-revision health recovery must not revive cancelled Ready"
        );
        d.desktop
            .in_flight
            .as_ref()
            .unwrap()
            .state
            .store(DELIVERY_FINISHED, Ordering::Release);
        d.emit_desktop_notifications();
        assert!(receiver.try_recv().is_err());
    }
    #[test]
    fn desktop_unavailable_ready_is_consumed_without_replaying_on_health_recovery() {
        let mut d = dashboard();
        let mut unavailable = snapshot(2, "a", AgentActivity::ResponseReady);
        session(&mut unavailable)
            .agent
            .as_mut()
            .unwrap()
            .health
            .state = ReporterHealth::Unavailable;
        deliver(&mut d, unavailable);
        assert!(d.desktop.pending.is_empty());
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert!(
            d.desktop.pending.is_empty(),
            "health recovery cannot replay Ready"
        );
        deliver(&mut d, snapshot(4, "b", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
    }
    #[test]
    fn desktop_health_loss_before_dispatch_cancels_pending_ready() {
        let mut d = dashboard();
        let (received, _failed) = stub_host(&mut d);
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        let mut unavailable = snapshot(2, "a", AgentActivity::ResponseReady);
        session(&mut unavailable)
            .agent
            .as_mut()
            .unwrap()
            .health
            .state = ReporterHealth::Unavailable;
        d.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
            session(&mut unavailable).clone(),
        ))));
        d.emit_desktop_notifications();
        assert!(
            received.try_recv().is_err(),
            "unavailable historical Ready reached host queue"
        );
    }
    #[test]
    fn desktop_session_event_before_hello_is_part_of_attach_baseline() {
        let mut d = Dashboard::new(TerminalSize {
            rows: 24,
            cols: 120,
        });
        d.key(KeyCode::Char('N'));
        let mut ready = snapshot(2, "old", AgentActivity::ResponseReady);
        d.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
            session(&mut ready).clone(),
        ))));
        d.handle_server_message(ServerMessage::Response {
            request_id: 1,
            response: Response::Hierarchy(ready),
        });
        assert!(
            d.desktop.pending.is_empty(),
            "pre-Hello event must not replay historical Ready"
        );
        deliver(&mut d, snapshot(4, "new", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
    }
    #[test]
    fn desktop_host_completion_wakes_dashboard_for_remaining_candidates() {
        let mut d = dashboard();
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        let (mut receiver, sender) = std::os::unix::net::UnixStream::pair().unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        sender.set_nonblocking(true).unwrap();
        let host = DesktopHost::start(Some(sender), CHANNEL_DESKTOP).unwrap();
        assert!(host.send(Delivery {
            notification: d.desktop.pending.pop_front().unwrap(),
            state: Arc::new(AtomicU8::new(DELIVERY_CANCELLED))
        }));
        let wake = super::super::event_loop::wait_for_dashboard_activity(
            -1,
            Some(&receiver),
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(
            wake.server_ready,
            "completed host operation did not wake dashboard"
        );
        assert!(!wake.timed_out);
        let mut byte = [0];
        std::io::Read::read_exact(&mut receiver, &mut byte).unwrap();
        assert_eq!(
            byte,
            [1],
            "descriptor closure alone is not a completion wake"
        );
    }
    #[test]
    fn desktop_provider_session_event_reaches_notification_observer() {
        let mut d = dashboard();
        let mut ready = snapshot(2, "a", AgentActivity::ResponseReady);
        d.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
            session(&mut ready).clone(),
        ))));
        assert_eq!(d.desktop.pending.len(), 1);
    }
    #[test]
    fn desktop_does_not_queue_when_unread_is_absent() {
        let mut d = dashboard();
        let mut ready = snapshot(2, "a", AgentActivity::ResponseReady);
        session(&mut ready).unread = None;
        deliver(&mut d, ready);
        assert!(
            d.desktop.pending.is_empty(),
            "activity Ready without unread must not queue"
        );
    }
    #[test]
    fn desktop_confirmed_without_unread_does_not_queue() {
        let mut d = dashboard();
        let mut ready = snapshot(2, "a", AgentActivity::ResponseReady);
        session(&mut ready)
            .agent
            .as_mut()
            .unwrap()
            .activity
            .as_mut()
            .unwrap()
            .quality = SampleQuality::Confirmed;
        session(&mut ready).unread = None;
        deliver(&mut d, ready);
        assert!(d.desktop.pending.is_empty(), "Confirmed is not Ready");
    }
    #[test]
    fn desktop_tasks_view_has_no_visible_terminal_panes() {
        let mut d = dashboard();
        d.select_session(SessionId(1));
        d.ctrl('t');
        assert!(d.tasks.is_some());
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
    }
    #[test]
    fn desktop_ready_requires_new_accepted_root_identity_and_only_exports_identity() {
        let mut d = dashboard();
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
        assert_eq!(
            d.desktop.pending[0].body,
            "project / workspace / session (#1)"
        );
        assert_eq!(d.desktop.pending[0].title, "OVRCR · response ready");
        d.desktop.pending.clear();
        let mut duplicate = snapshot(2, "a", AgentActivity::ResponseReady);
        session(&mut duplicate)
            .agent
            .as_mut()
            .unwrap()
            .health_revision = 99;
        deliver(&mut d, duplicate);
        deliver(&mut d, snapshot(3, "a", AgentActivity::ResponseReady));
        assert!(
            d.desktop.pending.is_empty(),
            "same unread turn does not requeue"
        );
        deliver(&mut d, snapshot(4, "b", AgentActivity::Busy));
        deliver(&mut d, snapshot(5, "b", AgentActivity::Idle));
        assert!(d.desktop.pending.is_empty(), "interrupt is not Ready");
        let mut generic = snapshot(6, "c", AgentActivity::ResponseReady);
        session(&mut generic).agent = None;
        deliver(&mut d, generic);
        assert!(
            d.desktop.pending.is_empty(),
            "generic Ready lacks root identity"
        );
        for invalid in ["provider", "missing-turn", "estimated", "exited"] {
            let mut d = dashboard();
            let mut event = snapshot(6, "c", AgentActivity::ResponseReady);
            let s = session(&mut event);
            match invalid {
                "provider" => s.agent.as_mut().unwrap().binding.provider = AgentProvider::Claude,
                "missing-turn" => s.agent.as_mut().unwrap().activity.as_mut().unwrap().turn = None,
                "estimated" => {
                    s.agent.as_mut().unwrap().activity.as_mut().unwrap().quality =
                        SampleQuality::Estimated;
                    s.unread = None;
                }
                _ => {
                    s.phase = SessionPhase::Exited {
                        code: Some(0),
                        signal: None,
                    }
                }
            }
            deliver(&mut d, event);
            assert!(d.desktop.pending.is_empty(), "{invalid}");
        }
    }
    #[test]
    fn desktop_attach_disabled_and_reenable_baselines_never_replay() {
        let mut d = Dashboard::new(TerminalSize {
            rows: 24,
            cols: 120,
        });
        d.key(KeyCode::Char('N'));
        d.handle_server_message(ServerMessage::Response {
            request_id: 1,
            response: Response::Hierarchy(snapshot(2, "a", AgentActivity::ResponseReady)),
        });
        assert!(d.desktop.pending.is_empty(), "attach is a baseline");
        d.key(KeyCode::Char('N'));
        deliver(&mut d, snapshot(4, "b", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty(), "disabled event is consumed");
        d.key(KeyCode::Char('N'));
        deliver(&mut d, snapshot(4, "b", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty(), "enable does not replay");
        deliver(&mut d, snapshot(6, "c", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
        d.key(KeyCode::Char('N'));
        assert!(
            d.desktop.pending.is_empty(),
            "disable cancels queued candidates"
        );
    }
    #[test]
    fn desktop_visibility_uses_drawn_geometry_including_hidden_split_and_empty_view() {
        fn snapshot(revision: u64, turn: &str, state: AgentActivity) -> HierarchySnapshot {
            let mut hierarchy = super::tests::snapshot(revision, turn, state);
            let mut other = session(&mut hierarchy).clone();
            other.id = SessionId(2);
            other.name = "visible-other".into();
            other.agent = None;
            hierarchy.projects[0].workspaces[0].sessions.push(other);
            hierarchy
        }
        let mut d = dashboard();
        d.select_session(SessionId(1));
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty(), "visible focused pane");
        d.panes.push(crate::dashboard::PaneState::new(TerminalSize {
            rows: 24,
            cols: 80,
        }));
        d.panes[1].session = Some(SessionId(2));
        d.focused_pane = 1;
        deliver(&mut d, snapshot(4, "b", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty(), "visible unfocused pane");
        d.outer_area = Rect::new(0, 0, 60, 24);
        deliver(&mut d, snapshot(6, "c", AgentActivity::ResponseReady));
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "assigned pane hidden by narrow geometry"
        );
        d.desktop.pending.clear();
        d.outer_area = Rect::new(0, 0, 120, 24);
        deliver(&mut d, snapshot(8, "d", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty());
        d.outer_area = Rect::new(0, 0, 120, 2);
        deliver(&mut d, snapshot(8, "d", AgentActivity::ResponseReady));
        assert!(
            d.desktop.pending.is_empty(),
            "becoming hidden never replays"
        );
        deliver(&mut d, snapshot(10, "e", AgentActivity::ResponseReady));
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "zero visible panes means background"
        );
    }
    #[test]
    fn desktop_new_unread_queues_and_epoch_bump_does_not_renotify() {
        let mut d = dashboard();
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
        d.desktop.pending.clear();
        let mut restarted = snapshot(2, "a", AgentActivity::ResponseReady);
        session(&mut restarted).agent_epoch = 2;
        deliver(&mut d, restarted);
        assert!(
            d.desktop.pending.is_empty(),
            "epoch bump does not make the same unread new"
        );
        let mut rebound = snapshot(3, "a", AgentActivity::ResponseReady);
        let session = session(&mut rebound);
        session.agent.as_mut().unwrap().binding.generation = 2;
        session.unread.as_mut().unwrap().binding.generation = 2;
        deliver(&mut d, rebound);
        assert_eq!(d.desktop.pending.len(), 1, "new unread binding queues");
        d.desktop.pending.clear();
        deliver(&mut d, snapshot(4, "b", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1, "new unread turn queues");
    }
    #[test]
    fn desktop_busy_cancels_pending_while_unread_remains() {
        let mut d = dashboard();
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
        let unread = d.hierarchy.projects[0].workspaces[0].sessions[0]
            .unread
            .clone();
        let mut busy = snapshot(3, "a", AgentActivity::Busy);
        session(&mut busy).unread = unread;
        deliver(&mut d, busy);
        assert!(
            d.desktop.pending.is_empty(),
            "Busy cancels delivery while unread remains"
        );
    }
    #[test]
    fn desktop_mark_reviewed_does_not_cancel_pending() {
        let mut d = dashboard();
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
        let mut reviewed = snapshot(2, "a", AgentActivity::ResponseReady);
        session(&mut reviewed).unread = None;
        deliver(&mut d, reviewed);
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "mark-reviewed must not cancel a queued alert"
        );
    }
    #[test]
    fn desktop_pending_candidates_are_bounded_and_identity_text_is_not_code() {
        let mut d = dashboard();
        let mut hierarchy = snapshot(2, "turn-2", AgentActivity::ResponseReady);
        let template = session(&mut hierarchy).clone();
        hierarchy.projects[0].workspaces[0].sessions = (1..=50)
            .map(|id| {
                let mut session = template.clone();
                session.id = SessionId(id);
                session.name = "name\nwith\u{1b}controls <b> & \"$(secret)\"".into();
                if id > 1 {
                    let invocation = format!("invocation-{id}");
                    session.agent.as_mut().unwrap().binding.invocation = invocation.clone();
                    session.unread.as_mut().unwrap().binding.invocation = invocation;
                }
                session
            })
            .collect();
        deliver(&mut d, hierarchy);
        assert_eq!(d.desktop.pending.len(), QUEUE_CAPACITY);
        // Newer activity replaces only its session's obsolete candidate; all
        // fifty live sessions remain representable in the bounded backlog.
        for revision in 3..102 {
            let mut session = d.hierarchy.projects[0].workspaces[0].sessions[0].clone();
            let agent = session.agent.as_mut().unwrap();
            agent.activity_revision = revision;
            let turn = format!("turn-{revision}");
            agent.activity.as_mut().unwrap().turn = Some(turn.clone());
            session.unread.as_mut().unwrap().turn = Some(turn);
            session.unread.as_mut().unwrap().activity_revision = revision;
            d.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
                session,
            ))));
            assert_eq!(d.desktop.pending.len(), QUEUE_CAPACITY);
        }
        let notice = d
            .desktop
            .pending
            .iter()
            .find(|notice| notice.session == SessionId(1))
            .unwrap();
        assert_eq!(
            notice.body,
            "project / workspace / namewithcontrols <b> & \"$(secret)\" (#1)"
        );
        let command = host_command(notice);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        #[cfg(target_os = "macos")]
        {
            assert_eq!(
                args[1],
                "on run argv\ndisplay notification (item 1 of argv) with title (item 2 of argv)\nend run"
            );
            assert!(args.iter().any(|arg| arg == &notice.body));
            assert!(!args[1].contains("secret"));
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(args.iter().any(|arg| arg.contains("&lt;b&gt; &amp;")));
            assert!(
                args.iter()
                    .any(|arg| arg == "--hint=boolean:suppress-sound:true")
            );
        }
        d.desktop.pending.clear();
        let repeated = d.hierarchy.clone();
        deliver(&mut d, repeated);
        assert!(
            d.desktop.pending.is_empty(),
            "consumed bounded backlog does not replay"
        );
    }
    #[test]
    fn desktop_cancelled_host_failure_cannot_replace_new_toggle_notice() {
        let mut d = dashboard();
        let (_received, failed) = stub_host(&mut d);
        d.key(KeyCode::Char('N'));
        d.key(KeyCode::Char('N'));
        failed.send((0, CHANNEL_DESKTOP)).unwrap();
        assert!(!d.emit_desktop_notifications());
        assert_eq!(
            d.desktop.notice.as_deref(),
            Some("Desktop notifications: on")
        );
    }
    #[test]
    fn desktop_notice_yields_to_the_next_keyboard_action() {
        let mut d = dashboard();
        assert_eq!(
            d.desktop.notice.as_deref(),
            Some("Desktop notifications: on")
        );
        d.key(KeyCode::Char('?'));
        assert!(d.desktop.notice.is_none());
    }
    #[test]
    fn desktop_notification_toggle_does_not_intercept_native_input() {
        let mut d = dashboard();
        d.select_session(SessionId(1));
        let request = d.view_request(d.outer_area, 77).unwrap().unwrap();
        let ovrcr_protocol::Request::SetView { view } = request.request else {
            panic!("expected view");
        };
        d.handle_server_message(ServerMessage::Response {
            request_id: 77,
            response: Response::Screen {
                session: SessionId(1),
                run: ovrcr_protocol::SessionRunId(1),
                revision: view.revision,
                size: view.panes[0].size,
                bytes: Vec::new(),
            },
        });
        d.handle_server_message(ServerMessage::Response {
            request_id: 77,
            response: Response::Ok,
        });
        assert_eq!(d.key(KeyCode::Enter), DashboardAction::Redraw);
        assert_eq!(d.mode, InputMode::Terminal);
        assert_eq!(
            d.key(KeyCode::Char('N')),
            DashboardAction::PtyBytes(b"N".to_vec())
        );
        assert!(d.settings.desktop_notifications);
    }
    type StubHost = (mpsc::Receiver<(u64, Delivery)>, mpsc::SyncSender<(u64, u8)>);
    fn stub_host(d: &mut Dashboard) -> StubHost {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (failed, failures) = mpsc::sync_channel(2);
        d.desktop.host = Some(DesktopHost {
            sender,
            failures,
            epoch: Arc::new(AtomicU64::new(0)),
            channels: Arc::new(AtomicU8::new(d.alert_channels())),
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
        });
        (receiver, failed)
    }
    #[test]
    fn sound_only_desktop_only_both_and_neither_share_one_accepted_candidate() {
        for (desktop, sound) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut d = Dashboard::new(TerminalSize {
                rows: 24,
                cols: 120,
            });
            d.settings.desktop_notifications = desktop;
            d.settings.ready_sound = sound;
            deliver(&mut d, snapshot(1, "a", AgentActivity::Busy));
            let (receiver, _failed) = stub_host(&mut d);
            deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
            if !desktop && !sound {
                assert!(d.desktop.pending.is_empty(), "neither channel enqueues");
                continue;
            }
            assert_eq!(
                d.desktop.pending.len(),
                1,
                "desktop={desktop} sound={sound}"
            );
            assert!(d.emit_desktop_notifications() || d.desktop.in_flight.is_some());
            let (_, delivery) = receiver.try_recv().unwrap();
            assert_eq!(delivery.notification.session, SessionId(1));
            let mask = d
                .desktop
                .host
                .as_ref()
                .unwrap()
                .channels
                .load(Ordering::Acquire);
            assert_eq!(mask & CHANNEL_DESKTOP != 0, desktop);
            assert_eq!(mask & CHANNEL_SOUND != 0, sound);
            delivery.state.store(DELIVERY_FINISHED, Ordering::Release);
            deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
            deliver(&mut d, snapshot(3, "a", AgentActivity::ResponseReady));
            d.emit_desktop_notifications();
            assert!(
                receiver.try_recv().is_err(),
                "one delivery per accepted response for desktop={desktop} sound={sound}"
            );
        }
    }
    #[test]
    fn ready_sound_toggle_is_independent_and_only_disabling_both_cancels_pending() {
        let mut d = dashboard();
        assert_eq!(d.key(KeyCode::Char('S')), DashboardAction::Redraw);
        assert!(d.settings.ready_sound && d.settings.desktop_notifications);
        assert_eq!(d.desktop.notice.as_deref(), Some("Ready sound: on"));
        let (_receiver, _failed) = stub_host(&mut d);
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
        d.key(KeyCode::Char('N'));
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "sound alone keeps the candidate"
        );
        let host = d.desktop.host.as_ref().unwrap();
        assert_eq!(host.channels.load(Ordering::Acquire), CHANNEL_SOUND);
        assert_eq!(
            host.epoch.load(Ordering::Acquire),
            0,
            "no cancellation while enabled"
        );
        d.key(KeyCode::Char('S'));
        assert_eq!(d.desktop.notice.as_deref(), Some("Ready sound: off"));
        assert!(
            d.desktop.pending.is_empty(),
            "disabling the last channel cancels"
        );
        let host = d.desktop.host.as_ref().unwrap();
        assert_eq!(host.channels.load(Ordering::Acquire), 0);
        assert_eq!(host.epoch.load(Ordering::Acquire), 1);
        d.key(KeyCode::Char('S'));
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty(), "enabling sound never replays");
        deliver(&mut d, snapshot(4, "b", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
    }
    #[test]
    fn sound_host_command_is_a_fixed_system_player_without_identity() {
        let command = sound_command();
        let program = command.get_program().to_string_lossy().into_owned();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        #[cfg(target_os = "macos")]
        {
            assert_eq!(program, "afplay");
            assert_eq!(args, ["/System/Library/Sounds/Glass.aiff"]);
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(program, "paplay");
            assert_eq!(args, ["/usr/share/sounds/freedesktop/stereo/complete.oga"]);
        }
        assert!(SOUND_TIMEOUT >= HOST_TIMEOUT);
    }
    #[test]
    fn sound_host_failure_reports_its_own_notice() {
        let mut d = dashboard();
        d.key(KeyCode::Char('S'));
        let (_receiver, failed) = stub_host(&mut d);
        d.desktop.notice = None;
        failed.send((0, CHANNEL_SOUND)).unwrap();
        assert!(d.emit_desktop_notifications());
        assert_eq!(d.desktop.notice.as_deref(), Some("Ready sound unavailable"));
        failed.send((0, CHANNEL_DESKTOP)).unwrap();
        assert!(d.emit_desktop_notifications());
        assert_eq!(
            d.desktop.notice.as_deref(),
            Some("Desktop notifications unavailable")
        );
        failed.send((0, CHANNEL_DESKTOP)).unwrap();
        failed.send((0, CHANNEL_SOUND)).unwrap();
        assert!(d.emit_desktop_notifications());
        assert_eq!(
            d.desktop.notice.as_deref(),
            Some("Desktop notifications and ready sound unavailable")
        );
        d.key(KeyCode::Char('S'));
        d.desktop.notice = None;
        failed.send((0, CHANNEL_SOUND)).unwrap();
        assert!(
            !d.emit_desktop_notifications(),
            "a channel disabled after its command started reports nothing"
        );
        assert!(d.desktop.notice.is_none());
        assert_eq!(unavailable_notice(CHANNEL_SOUND), "Ready sound unavailable");
    }
    #[test]
    fn ready_sound_toggle_does_not_intercept_native_input() {
        let mut d = dashboard();
        d.key(KeyCode::Char('S'));
        d.select_session(SessionId(1));
        let request = d.view_request(d.outer_area, 78).unwrap().unwrap();
        let ovrcr_protocol::Request::SetView { view } = request.request else {
            panic!("expected view");
        };
        d.handle_server_message(ServerMessage::Response {
            request_id: 78,
            response: Response::Screen {
                session: SessionId(1),
                run: ovrcr_protocol::SessionRunId(1),
                revision: view.revision,
                size: view.panes[0].size,
                bytes: Vec::new(),
            },
        });
        d.handle_server_message(ServerMessage::Response {
            request_id: 78,
            response: Response::Ok,
        });
        assert_eq!(d.key(KeyCode::Enter), DashboardAction::Redraw);
        assert_eq!(d.mode, InputMode::Terminal);
        assert_eq!(
            d.key(KeyCode::Char('S')),
            DashboardAction::PtyBytes(b"S".to_vec())
        );
        assert!(d.settings.ready_sound);
    }
}

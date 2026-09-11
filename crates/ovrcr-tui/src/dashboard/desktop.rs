//! Dashboard-local opt-in delivery. Only accepted root activity identities enter this module.
use super::{Dashboard, DashboardAction};
use ovrcr_protocol::{
    AgentActivity, AgentBinding, AgentProvider, HierarchySnapshot, ReporterHealth, SampleQuality,
    SessionId, SessionPhase, SessionSummary,
};
use std::collections::{HashMap, VecDeque};
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
const HOST_POLL: Duration = Duration::from_millis(10);

#[derive(Clone)]
struct Observed {
    epoch: u64,
    binding: AgentBinding,
    revision: u64,
    ready_turn: Option<String>,
}

#[derive(Clone)]
struct Notification {
    session: SessionId,
    observed: Observed,
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
    observed: HashMap<SessionId, Observed>,
    pending: VecDeque<Notification>,
    in_flight: Option<Delivery>,
    pub(super) wake: Option<std::os::unix::net::UnixStream>,
    pub(super) notice: Option<String>,
    host: Option<DesktopHost>,
}

impl Dashboard {
    pub(super) fn toggle_desktop_notifications(&mut self) -> DashboardAction {
        self.settings.desktop_notifications = !self.settings.desktop_notifications;
        if !self.settings.desktop_notifications {
            self.desktop.pending.clear();
        }
        if let Some(host) = &mut self.desktop.host {
            host.set_enabled(self.settings.desktop_notifications);
        }
        self.desktop.notice = Some(format!(
            "Desktop notifications: {}",
            if self.settings.desktop_notifications {
                "on"
            } else {
                "off"
            }
        ));
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
        self.desktop.observed.retain(|id, _| existing.contains(id));
    }

    pub(super) fn observe_desktop_update(&mut self, session: &SessionSummary) {
        self.observe_desktop_session(session, !self.desktop.initialized);
    }

    fn observe_desktop_session(&mut self, session: &SessionSummary, initial: bool) {
        // Health and lifecycle changes do not necessarily advance the activity revision.
        if let Some(delivery) = &self.desktop.in_flight
            && delivery.notification.session == session.id
            && !notification_matches_session(&delivery.notification, session)
        {
            delivery.cancel();
        }
        let Some(agent) = &session.agent else {
            return;
        };
        if agent.binding.provider != AgentProvider::Codex {
            return;
        }
        let previous = self.desktop.observed.get(&session.id);
        let same_binding = previous.is_some_and(|previous| {
            previous.epoch == session.agent_epoch && previous.binding == agent.binding
        });
        if let Some(previous) = previous
            && (session.agent_epoch < previous.epoch
                || (session.agent_epoch == previous.epoch
                    && (!same_binding
                        && (previous.binding.invocation != agent.binding.invocation
                            || agent.binding.generation <= previous.binding.generation)))
                || (same_binding && agent.activity_revision <= previous.revision))
        {
            return;
        }
        let prior_turn = previous
            .filter(|_| same_binding)
            .and_then(|previous| previous.ready_turn.clone());
        let turn = agent
            .activity
            .as_ref()
            .filter(|activity| activity.state == AgentActivity::ResponseReady)
            .and_then(|activity| activity.turn.clone());
        let new_ready = turn.is_some() && turn != prior_turn;
        let observed = Observed {
            epoch: session.agent_epoch,
            binding: agent.binding.clone(),
            revision: agent.activity_revision,
            ready_turn: turn.clone().or(prior_turn),
        };
        self.desktop.observed.insert(session.id, observed.clone());
        // Consume all accepted revisions, including disabled and visible responses. Visibility
        // and settings changes never rescan old Ready state for later delivery.
        if !initial
            && self.settings.desktop_notifications
            && new_ready
            && ready_session(session)
            && !self.desktop_session_visible(session.id)
            && self.desktop.pending.len() < QUEUE_CAPACITY
        {
            self.desktop.pending.push_back(Notification {
                session: session.id,
                observed,
                title: "OVRCR · response ready",
                body: format!(
                    "{} / {} / {} (#{})",
                    safe_identity(&session.project),
                    safe_identity(&session.workspace),
                    safe_identity(&session.name),
                    session.id.0
                ),
            });
        }
    }

    fn desktop_notification_valid(&self, notification: &Notification) -> bool {
        self.settings.desktop_notifications
            && !self.desktop_session_visible(notification.session)
            && super::state::find_session(self, notification.session)
                .is_some_and(|session| notification_matches_session(notification, session))
    }

    pub(super) fn emit_desktop_notifications(&mut self) -> bool {
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
                    .and_then(DesktopHost::start);
                match host {
                    Ok(host) => self.desktop.host = Some(host),
                    Err(_) => {
                        self.desktop.notice = Some("Desktop notifications unavailable".into());
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
        if let Some(host) = &self.desktop.host
            && host
                .failures
                .try_recv()
                .is_ok_and(|ticket| host.enabled && ticket == host.epoch.load(Ordering::Acquire))
        {
            self.desktop.notice = Some("Desktop notifications unavailable".into());
            changed = true;
        }
        changed
    }
}

fn notification_matches_session(notification: &Notification, session: &SessionSummary) -> bool {
    ready_session(session)
        && session.agent_epoch == notification.observed.epoch
        && session.agent.as_ref().is_some_and(|agent| {
            agent.binding == notification.observed.binding
                && agent.activity_revision == notification.observed.revision
        })
}

fn ready_session(session: &SessionSummary) -> bool {
    session.phase == SessionPhase::Running
        && session.agent.as_ref().is_some_and(|agent| {
            agent.binding.provider == AgentProvider::Codex
                && agent.health.state == ReporterHealth::Connected
                && agent.activity_revision > 0
                && agent.activity.as_ref().is_some_and(|activity| {
                    activity.state == AgentActivity::ResponseReady
                        && matches!(
                            activity.quality,
                            SampleQuality::Observed | SampleQuality::Confirmed
                        )
                        && activity.turn.as_ref().is_some_and(|turn| !turn.is_empty())
                })
        })
}

fn safe_identity(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(80).collect()
}

struct DesktopHost {
    sender: mpsc::SyncSender<(u64, Delivery)>,
    failures: mpsc::Receiver<u64>,
    enabled: bool,
    epoch: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl DesktopHost {
    fn start(mut wake: Option<std::os::unix::net::UnixStream>) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<(u64, Delivery)>(1);
        let (failure, failures) = mpsc::sync_channel(1);
        let epoch = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_epoch = Arc::clone(&epoch);
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
                    let success = cancelled() || run_host(&delivery.notification, cancelled);
                    let report_failure = !success && !cancelled();
                    delivery.state.store(DELIVERY_FINISHED, Ordering::Release);
                    if report_failure {
                        let _ = failure.try_send(ticket);
                    }
                    if let Some(wake) = &mut wake {
                        super::event_loop::notify_dashboard_wake(wake);
                    }
                }
            })?;
        Ok(Self {
            sender,
            failures,
            enabled: true,
            epoch,
            stop,
            worker: Some(worker),
        })
    }
    fn set_enabled(&mut self, enabled: bool) {
        if enabled != self.enabled {
            self.enabled = enabled;
            self.epoch.fetch_add(1, Ordering::AcqRel);
        }
    }
    fn send(&self, delivery: Delivery) -> bool {
        self.enabled
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

fn run_host(notification: &Notification, cancelled: impl Fn() -> bool) -> bool {
    let mut command = host_command(notification);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = Instant::now() + HOST_TIMEOUT;
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::{DashboardAction, InputMode};
    use crate::protocol::{
        ActivitySample, AgentSnapshot, ClientMessage, HealthSample, ProjectSummary, ReporterHealth,
        Response, ServerEvent, ServerMessage, WorkspaceSummary,
    };
    use crossterm::event::KeyCode;
    use ovrcr_protocol::TerminalSize;
    use ratatui::layout::Rect;

    fn snapshot(revision: u64, turn: &str, state: AgentActivity) -> HierarchySnapshot {
        HierarchySnapshot {
            projects: vec![ProjectSummary {
                name: "project".into(),
                workspaces: vec![WorkspaceSummary {
                    project: "project".into(),
                    name: "workspace".into(),
                    path: "/private/path-must-not-leak".into(),
                    sessions: vec![SessionSummary {
                        id: SessionId(1),
                        project: "project".into(),
                        workspace: "workspace".into(),
                        name: "session".into(),
                        label: "PRIVATE_LABEL".into(),
                        pid: Some(1),
                        started_unix_ms: 0,
                        phase: SessionPhase::Running,
                        activity: state,
                        context_usage: None,
                        agent_epoch: 1,
                        agent: Some(AgentSnapshot {
                            binding: AgentBinding {
                                provider: AgentProvider::Codex,
                                invocation: "PRIVATE_INVOCATION".into(),
                                conversation: "PRIVATE_CONVERSATION".into(),
                                generation: 1,
                            },
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
                        }),
                    }],
                }],
            }],
        }
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
        let (sender, received) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (_failed, failures) = mpsc::sync_channel(1);
        d.desktop.host = Some(DesktopHost {
            sender,
            failures,
            enabled: true,
            epoch: Arc::new(AtomicU64::new(0)),
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
        });
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
        let host = DesktopHost::start(Some(sender)).unwrap();
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
        deliver(&mut d, snapshot(1, "old", AgentActivity::ResponseReady));
        deliver(&mut d, snapshot(3, "a", AgentActivity::ResponseReady));
        assert!(
            d.desktop.pending.is_empty(),
            "duplicate turn and stale revision"
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
                        SampleQuality::Estimated
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
    fn desktop_rebind_and_supervisor_restart_reject_prior_identity() {
        let mut d = dashboard();
        let mut current = snapshot(2, "a", AgentActivity::ResponseReady);
        session(&mut current)
            .agent
            .as_mut()
            .unwrap()
            .binding
            .generation = 2;
        session(&mut current)
            .agent
            .as_mut()
            .unwrap()
            .binding
            .conversation = "B".into();
        deliver(&mut d, current.clone());
        assert_eq!(d.desktop.pending.len(), 1);
        d.desktop.pending.clear();
        deliver(&mut d, snapshot(100, "stale", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty());
        session(&mut current)
            .agent
            .as_mut()
            .unwrap()
            .binding
            .generation = 3;
        session(&mut current)
            .agent
            .as_mut()
            .unwrap()
            .binding
            .conversation = "PRIVATE_CONVERSATION".into();
        session(&mut current)
            .agent
            .as_mut()
            .unwrap()
            .activity_revision = 3;
        deliver(&mut d, current.clone());
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "A to B to A is a new generation"
        );
        d.desktop.pending.clear();
        session(&mut current).agent_epoch = 2;
        session(&mut current)
            .agent
            .as_mut()
            .unwrap()
            .binding
            .invocation = "new-invocation".into();
        session(&mut current)
            .agent
            .as_mut()
            .unwrap()
            .binding
            .generation = 1;
        session(&mut current)
            .agent
            .as_mut()
            .unwrap()
            .activity_revision = 1;
        deliver(&mut d, current);
        assert_eq!(d.desktop.pending.len(), 1);
        d.desktop.pending.clear();
        deliver(&mut d, snapshot(500, "late", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty());
    }
    #[test]
    fn desktop_pending_candidates_are_bounded_and_identity_text_is_not_code() {
        let mut d = dashboard();
        for revision in 2..102 {
            let mut event = snapshot(
                revision,
                &format!("turn-{revision}"),
                AgentActivity::ResponseReady,
            );
            session(&mut event).name = "name\nwith\u{1b}controls <b> & \"$(secret)\"".into();
            deliver(&mut d, event);
        }
        assert_eq!(d.desktop.pending.len(), QUEUE_CAPACITY);
        let notice = &d.desktop.pending[0];
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
        deliver(
            &mut d,
            snapshot(101, "turn-101", AgentActivity::ResponseReady),
        );
        assert!(
            d.desktop.pending.is_empty(),
            "queue overflow does not replay"
        );
    }
    #[test]
    fn desktop_cancelled_host_failure_cannot_replace_new_toggle_notice() {
        let mut d = dashboard();
        let (sender, _received) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (failed, failures) = mpsc::sync_channel(1);
        d.desktop.host = Some(DesktopHost {
            sender,
            failures,
            enabled: true,
            epoch: Arc::new(AtomicU64::new(0)),
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
        });
        d.key(KeyCode::Char('N'));
        d.key(KeyCode::Char('N'));
        failed.send(0).unwrap();
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
}

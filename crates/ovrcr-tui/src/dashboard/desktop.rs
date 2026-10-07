//! Dashboard-local opt-in delivery. Two lanes, one queue: a Ready keyed by response
//! identity, and an Input request keyed by request identity. Both reuse the same
//! preferences, host and reconnect baseline. Selection and pane visibility
//! do not suppress or cancel otherwise valid alerts.
use super::ready::{delivery_live, input_live};
use super::settings::{AutomaticLocalTerminals, Settings};
#[cfg(target_os = "macos")]
use super::settings::{INVALID_SOUND_CHOICE_NOTICE, invalid_sound_choice_notice};
use super::{Dashboard, DashboardAction};
#[cfg(target_os = "macos")]
use ovrcr_protocol::bridge_installation::BridgeProfile;
#[cfg(test)]
use ovrcr_protocol::{AgentActivity, AgentProvider, SessionPhase};
use ovrcr_protocol::{
    AgentBinding, HierarchySnapshot, InputRequest, ReadyObservation, SessionId, SessionSummary,
};
#[cfg(target_os = "macos")]
use ovrcr_protocol::{
    BRIDGE_SCHEMA_VERSION, BridgeOperation, BridgeReply, BridgeRequest, BridgeSoundFailure,
    BridgeStatus, MAX_FRAME_BYTES, PROTOCOL_VERSION, ReadySoundChoice,
};
use ovrcr_protocol::{ClientMessage, Request};
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
#[cfg(target_os = "macos")]
use std::{
    io::{Read, Write},
    os::fd::AsRawFd,
    path::PathBuf,
    sync::Mutex,
};

const QUEUE_CAPACITY: usize = 50;
const HOST_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(not(target_os = "macos"))]
const SOUND_TIMEOUT: Duration = Duration::from_secs(5);
const CHANNEL_DESKTOP: u8 = 1;
const CHANNEL_SOUND: u8 = 2;
const HOST_POLL: Duration = Duration::from_millis(10);
#[cfg(target_os = "macos")]
const PERMISSION_POLL_BUDGET: Duration = Duration::from_secs(30);

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
    #[cfg(target_os = "macos")]
    subtitle: String,
    body: String,
    #[cfg(target_os = "macos")]
    navigation: Option<ovrcr_protocol::BridgeNavigationTicket>,
}

const DELIVERY_ACTIVE: u8 = 0;
const DELIVERY_CANCELLED: u8 = 1;
const DELIVERY_FINISHED: u8 = 2;
const DELIVERY_CANCELLED_FINISHED: u8 = DELIVERY_CANCELLED | DELIVERY_FINISHED;

#[derive(Clone)]
struct Delivery {
    notification: Notification,
    state: Arc<AtomicU8>,
    #[cfg(target_os = "macos")]
    started: Arc<AtomicBool>,
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
    #[cfg(target_os = "macos")]
    bridge: BridgePermission,
    #[cfg(target_os = "macos")]
    sound_unavailable: Option<(ReadySoundChoice, BridgeSoundFailure)>,
    #[cfg(target_os = "macos")]
    pub(super) iterm: super::iterm::Setup,
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct BridgePermission {
    enabled: bool,
    status: Option<BridgeStatus>,
    pending: Option<BridgeOperation>,
    in_flight: Option<BridgeControl>,
    authorization_requested: bool,
    poll_until: Option<Instant>,
    next_poll: Option<Instant>,
    check_after_settings: bool,
    fault: Option<BridgeFault>,
}

/// Why the Bridge answered `incompatible` or `failed`, so the banner can
/// name the fix. Startup owns every repair; the Dashboard only reports.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BridgeFault {
    /// No app at the selected installation path.
    NotInstalled,
    /// The installed app's contract differs from this Dashboard's.
    AppOutdated,
    /// The installed app matches, so the running Bridge process is older.
    ProcessOutdated,
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(super) struct BridgeControl {
    operation: BridgeOperation,
    state: Arc<AtomicU8>,
}

#[cfg(target_os = "macos")]
impl BridgeControl {
    pub(super) fn cancel(&self) {
        self.state.store(DELIVERY_CANCELLED, Ordering::Release);
    }
    pub(super) fn finished(&self) -> bool {
        self.state.load(Ordering::Acquire) == DELIVERY_FINISHED
    }
}

#[cfg(target_os = "macos")]
struct BridgeUpdate {
    operation: BridgeOperation,
    reply: BridgeReply,
    fault: Option<BridgeFault>,
    state: Arc<AtomicU8>,
    ticket: Option<u64>,
}

impl Dashboard {
    pub(super) fn alert_channels(&self) -> u8 {
        let mut channels = 0;
        if self.settings.desktop_notifications {
            channels |= CHANNEL_DESKTOP;
        }
        if self.settings.ready_sound
            && (!cfg!(target_os = "macos") || self.settings.desktop_notifications)
        {
            channels |= CHANNEL_SOUND;
        }
        channels
    }

    #[cfg(target_os = "macos")]
    fn notification_sound(&self) -> Option<ReadySoundChoice> {
        (self.settings.desktop_notifications && self.settings.ready_sound)
            .then_some(self.settings.ready_sound_choice)
            .flatten()
    }

    pub(super) fn toggle_desktop_notifications(&mut self) -> DashboardAction {
        self.toggle_alert_setting(
            "desktop_notifications",
            !self.settings.desktop_notifications,
        )
    }

    pub(super) fn toggle_ready_sound(&mut self) -> DashboardAction {
        self.toggle_alert_setting("ready_sound", !self.settings.ready_sound)
    }

    pub(super) fn cycle_automatic_local_terminals(&mut self) -> DashboardAction {
        let next = self.settings.automatic_local_terminals.next();
        self.set_setting(
            AutomaticLocalTerminals::KEY,
            format!("\"{}\"", next.as_str()),
        )
    }

    fn toggle_alert_setting(&mut self, key: &str, enabled: bool) -> DashboardAction {
        self.set_setting(key, enabled.to_string())
    }

    /// Ask the Server to save one setting. Nothing changes here until the
    /// Server's next reading arrives; a refusal shows as the footer error.
    fn set_setting(&mut self, path: &str, value: String) -> DashboardAction {
        DashboardAction::Request(ClientMessage {
            request_id: self.error_owning_request_id(),
            request: Request::SetSetting {
                path: path.into(),
                value: Some(value),
            },
        })
    }

    /// Footer notice for an alert or automatic-terminal setting a new Server
    /// reading changed, whether by a toggle here or an edit elsewhere.
    pub(super) fn settings_changed_notice(&mut self, before: &Settings) {
        let on_off = |enabled: bool| if enabled { "on" } else { "off" };
        #[cfg(target_os = "macos")]
        if invalid_sound_choice_notice(&self.settings).is_none() {
            if self.desktop.notice.as_deref() == Some(INVALID_SOUND_CHOICE_NOTICE) {
                self.desktop.notice = None;
            }
            self.notice_queue
                .retain(|notice| notice != INVALID_SOUND_CHOICE_NOTICE);
        }
        #[cfg(target_os = "macos")]
        if before.ready_sound_choice != self.settings.ready_sound_choice
            || before.ready_sound != self.settings.ready_sound
            || before.desktop_notifications != self.settings.desktop_notifications
        {
            self.desktop.sound_unavailable = None;
        }
        let automatic = (before.automatic_local_terminals
            != self.settings.automatic_local_terminals)
            .then(|| {
                format!(
                    "Automatic local terminals: {}",
                    self.settings.automatic_local_terminals.label()
                )
            });
        let sound = (before.ready_sound != self.settings.ready_sound)
            .then(|| format!("Ready sound: {}", on_off(self.settings.ready_sound)));
        #[cfg(target_os = "macos")]
        let choice = if before.ready_sound_choice != self.settings.ready_sound_choice {
            match self.settings.ready_sound_choice {
                Some(choice) => Some(format!("Ready sound choice: {}", choice.label())),
                None => invalid_sound_choice_notice(&self.settings).map(str::to_owned),
            }
        } else {
            None
        };
        let desktop =
            (before.desktop_notifications != self.settings.desktop_notifications).then(|| {
                format!(
                    "Desktop notifications: {}",
                    on_off(self.settings.desktop_notifications)
                )
            });
        let iterm = (before.iterm_focus != self.settings.iterm_focus)
            .then(|| format!("Exact iTerm focus: {}", on_off(self.settings.iterm_focus)));
        let theme = (before.theme != self.settings.theme)
            .then(|| format!("Theme: {}", self.settings.theme.label()));
        for notice in [
            automatic,
            sound,
            #[cfg(target_os = "macos")]
            choice,
            desktop,
            iterm,
            theme,
        ]
        .into_iter()
        .flatten()
        {
            self.show_notice(notice);
        }
    }

    /// Hand the current alert preferences to a running host.
    pub(super) fn apply_alert_channels(&mut self, #[cfg(target_os = "macos")] before: &Settings) {
        #[cfg(target_os = "macos")]
        self.cancel_iterm_setup_if_disabled();
        let channels = self.alert_channels();
        if channels == 0 {
            self.desktop.pending.clear();
        }
        if let Some(host) = &mut self.desktop.host {
            host.set_channels(
                channels,
                #[cfg(target_os = "macos")]
                (self.settings.desktop_notifications && self.settings.ready_sound)
                    .then_some(self.settings.ready_sound_choice)
                    .flatten(),
                #[cfg(target_os = "macos")]
                (before.ready_sound != self.settings.ready_sound
                    || before.ready_sound_choice != self.settings.ready_sound_choice)
                    .then_some(self.desktop.in_flight.as_ref())
                    .flatten(),
            );
        }
        #[cfg(target_os = "macos")]
        {
            let bridge = &mut self.desktop.bridge;
            let enabled = self.settings.desktop_notifications;
            if enabled != bridge.enabled {
                bridge.enabled = enabled;
                bridge.pending = enabled.then_some(BridgeOperation::Status);
                bridge.authorization_requested = false;
                bridge.poll_until = None;
                bridge.next_poll = None;
                bridge.check_after_settings = false;
                if let Some(control) = bridge.in_flight.take() {
                    control.state.store(DELIVERY_CANCELLED, Ordering::Release);
                }
            }
            // The complete Server reading has posted its notices before this
            // preference application. Reveal any unrelated notice still waiting.
            self.reveal_queued_notice();
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn recover_notification_permission(&mut self) -> DashboardAction {
        if !self.notification_recovery_available() {
            return DashboardAction::None;
        }
        if self.notification_permission_unconfirmed() {
            self.desktop.bridge.pending = Some(BridgeOperation::Status);
            // An explicit check never opens a new authorization attempt.
            self.desktop.bridge.authorization_requested = true;
            // Keep this one user check from starting another automatic poll budget.
            self.desktop
                .bridge
                .poll_until
                .get_or_insert_with(Instant::now);
            self.desktop.bridge.next_poll = None;
            self.show_notice("Checking OVRCR notification permission");
        } else {
            self.desktop.bridge.pending = Some(BridgeOperation::Settings);
            self.desktop.bridge.poll_until = None;
            self.show_notice("Open System Settings, then Notifications → OVRCR");
        }
        DashboardAction::Redraw
    }

    #[cfg(target_os = "macos")]
    pub(super) fn notification_permission_unconfirmed(&self) -> bool {
        let bridge = &self.desktop.bridge;
        if bridge.check_after_settings {
            return true;
        }
        match bridge.status {
            Some(BridgeStatus::PermissionPending) => bridge
                .poll_until
                .is_some_and(|until| Instant::now() >= until),
            Some(BridgeStatus::NotDetermined) => {
                bridge.authorization_requested
                    && bridge.pending.is_none()
                    && bridge.in_flight.is_none()
            }
            _ => false,
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn notification_recovery_available(&self) -> bool {
        self.settings.desktop_notifications
            && (self.desktop.bridge.status == Some(BridgeStatus::Denied)
                || self.notification_permission_unconfirmed())
    }

    pub(super) fn desktop_status_notice(&self) -> Option<&'static str> {
        #[cfg(target_os = "macos")]
        if self.settings.desktop_notifications {
            if self.notification_permission_unconfirmed() {
                return Some("OVRCR notification permission unconfirmed; O: check permission");
            }
            let permission = match self.desktop.bridge.status {
                Some(BridgeStatus::Denied) => {
                    Some("OVRCR notifications denied; O: System Settings → Notifications → OVRCR")
                }
                Some(BridgeStatus::PermissionPending) => {
                    Some("OVRCR notification permission pending; respond to the macOS prompt")
                }
                Some(BridgeStatus::NotDetermined) => {
                    Some("OVRCR notification permission needed; checking authorization")
                }
                Some(BridgeStatus::Incompatible) => Some(match self.desktop.bridge.fault {
                    Some(BridgeFault::ProcessOutdated) => {
                        "Running OVRCR Bridge is an older build; restart OVRCR to review the Bridge restart offer"
                    }
                    _ => {
                        "OVRCR Bridge needs updating; restart OVRCR to review the Bridge repair offer"
                    }
                }),
                Some(BridgeStatus::Failed) => Some(match self.desktop.bridge.fault {
                    Some(BridgeFault::NotInstalled) => {
                        "OVRCR Bridge not installed; restart OVRCR to review the Bridge install offer"
                    }
                    _ => {
                        "OVRCR Bridge unavailable; restart OVRCR to review the Bridge install or repair offer"
                    }
                }),
                _ => None,
            };
            if permission.is_some() {
                return permission;
            }
            if self.settings.ready_sound {
                if let Some(notice) = invalid_sound_choice_notice(&self.settings) {
                    return Some(notice);
                }
                if let Some((choice, failure)) = self.desktop.sound_unavailable
                    && Some(choice) == self.notification_sound()
                {
                    return Some(match failure {
                        BridgeSoundFailure::LookupConflict => {
                            "Ready sound unavailable; banner submitted silently. Sound name conflict; choose System default in Settings"
                        }
                        _ => {
                            "Ready sound unavailable; banner submitted silently. Reinstall OVRCR Bridge or choose System default in Settings"
                        }
                    });
                }
            }
        }
        #[cfg(target_os = "macos")]
        if self.settings.ready_sound && !self.settings.desktop_notifications {
            return Some("Ready sound saved; enable desktop notifications to hear it");
        }
        None
    }

    #[cfg(target_os = "macos")]
    fn emit_bridge_control(&mut self) -> bool {
        let updates: Vec<_> = self
            .desktop
            .host
            .as_ref()
            .map_or_else(Vec::new, |host| host.updates.try_iter().collect());
        let mut changed = false;
        for update in updates {
            if matches!(
                update.state.load(Ordering::Acquire),
                DELIVERY_CANCELLED | DELIVERY_CANCELLED_FINISHED
            ) || update.ticket.is_some_and(|ticket| {
                self.desktop
                    .host
                    .as_ref()
                    .is_none_or(|host| ticket != host.epoch.load(Ordering::Acquire))
            }) || matches!(update.operation, BridgeOperation::Deliver { .. })
                && !self.settings.desktop_notifications
            {
                continue;
            }
            changed = true;
            let status = update.reply.status;
            if matches!(
                update.operation,
                BridgeOperation::ITermSetup { .. } | BridgeOperation::ITermStatus { .. }
            ) {
                self.handle_iterm_bridge_update(&update.operation, status);
                continue;
            }
            if let BridgeOperation::Deliver { sound, .. } = &update.operation
                && status == BridgeStatus::Submitted
                && *sound == self.notification_sound()
            {
                self.desktop.sound_unavailable = sound.and_then(|choice| {
                    update
                        .reply
                        .sound_unavailable
                        .map(|failure| (choice, failure))
                });
                if update.reply.sound_unavailable.is_some() {
                    self.desktop.notice = None;
                }
            }
            let bridge = &mut self.desktop.bridge;
            if update.operation == BridgeOperation::Status {
                // A fresh check retires the completed recovery action, including
                // guidance that waited behind an error banner. Keep other notices.
                let recovery_notice = |notice: &str| {
                    matches!(
                        notice,
                        "Checking OVRCR notification permission"
                            | "Open System Settings, then Notifications → OVRCR"
                            | "System Settings opened; choose Notifications → OVRCR, then Browse O checks permission"
                            | "Open System Settings manually: Notifications → OVRCR; then Browse O checks permission"
                    )
                };
                if self.desktop.notice.as_deref().is_some_and(recovery_notice) {
                    self.desktop.notice = None;
                }
                self.notice_queue.retain(|notice| !recovery_notice(notice));
            }
            if update.operation == BridgeOperation::Settings {
                bridge.check_after_settings = true;
                self.show_notice(
                    if status == BridgeStatus::SettingsOpened {
                        "System Settings opened; choose Notifications → OVRCR, then Browse O checks permission"
                    } else {
                        "Open System Settings manually: Notifications → OVRCR; then Browse O checks permission"
                    }
                );
                continue;
            }
            bridge.check_after_settings = false;
            bridge.fault = update.fault;
            bridge.status = Some(if status == BridgeStatus::Submitted {
                BridgeStatus::Available
            } else {
                status
            });
            if matches!(
                status,
                BridgeStatus::Denied
                    | BridgeStatus::PermissionPending
                    | BridgeStatus::Incompatible
                    | BridgeStatus::Failed
            ) {
                self.desktop.notice = None;
            }
            if update.operation == BridgeOperation::Status
                && status == BridgeStatus::NotDetermined
                && bridge.enabled
                && !bridge.authorization_requested
            {
                bridge.pending = Some(BridgeOperation::Authorize);
                bridge.authorization_requested = true;
            }
            if status == BridgeStatus::PermissionPending {
                bridge
                    .poll_until
                    .get_or_insert_with(|| Instant::now() + PERMISSION_POLL_BUDGET);
                bridge.next_poll = Some(Instant::now() + Duration::from_secs(1));
            } else {
                bridge.poll_until = None;
                bridge.next_poll = None;
            }
        }
        let bridge = &mut self.desktop.bridge;
        if bridge
            .in_flight
            .as_ref()
            .is_some_and(|control| control.state.load(Ordering::Acquire) == DELIVERY_FINISHED)
        {
            bridge.in_flight = None;
        }
        let now = Instant::now();
        if bridge.enabled
            && bridge.pending.is_none()
            && bridge.poll_until.is_some_and(|until| now < until)
            && bridge.next_poll.is_some_and(|poll| now >= poll)
        {
            bridge.pending = Some(BridgeOperation::Status);
            bridge.next_poll = None;
        }
        if bridge.in_flight.is_none()
            && let Some(operation) = bridge.pending.clone()
            && let Some(host) = &self.desktop.host
        {
            let control = BridgeControl {
                operation,
                state: Arc::new(AtomicU8::new(DELIVERY_ACTIVE)),
            };
            if host.controls.try_send(control.clone()).is_ok() {
                bridge.pending = None;
                bridge.in_flight = Some(control);
            }
        }
        changed |= self.emit_iterm_control();
        changed
    }

    #[cfg(target_os = "macos")]
    pub(super) fn submit_iterm_control(&self, operation: BridgeOperation) -> Option<BridgeControl> {
        let control = BridgeControl {
            operation,
            state: Arc::new(AtomicU8::new(DELIVERY_ACTIVE)),
        };
        self.desktop
            .host
            .as_ref()?
            .controls
            .try_send(control.clone())
            .ok()?;
        Some(control)
    }

    #[cfg(test)]
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
        // Consume unread even when disabled so later enable cannot replay.
        if !initial
            && self.alert_channels() != 0
            && new_unread
            && delivery_live(session)
            && self.desktop.pending.len() < QUEUE_CAPACITY
        {
            self.desktop.pending.push_back(Notification {
                session: session.id,
                alert: Alert::Ready(session.unread.clone().unwrap()),
                title: "OVRCR · response ready",
                #[cfg(target_os = "macos")]
                subtitle: identity_subtitle(session),
                body: identity_body(session),
                #[cfg(target_os = "macos")]
                navigation: self.navigation_ticket(session.id, session.run),
            });
        }
        // The second lane, consumed on the same terms: answering is not a review, so this
        // never reads or writes Unread. Several requests can be open at once, and each
        // newly seen one is its own alert.
        let new_requests = self.unread.observe_request(session);
        if !initial && self.alert_channels() != 0 {
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
                    #[cfg(target_os = "macos")]
                    subtitle: identity_subtitle(session),
                    body: identity_body(session),
                    #[cfg(target_os = "macos")]
                    navigation: self.navigation_ticket(session.id, session.run),
                });
            }
        }
    }

    fn desktop_notification_valid(&self, notification: &Notification) -> bool {
        self.alert_channels() != 0
            && super::state::find_session(self, notification.session)
                .is_some_and(|session| notification_matches_session(notification, session))
    }

    pub(super) fn emit_desktop_notifications(&mut self) -> bool {
        // Drop candidates that are no longer channel- or identity-valid, even while
        // the host is busy; selection and pane visibility never revoke them.
        let mut pending = std::mem::take(&mut self.desktop.pending);
        pending.retain(|notification| self.desktop_notification_valid(notification));
        self.desktop.pending = pending;
        let mut changed = false;
        #[cfg(target_os = "macos")]
        if (self.desktop.bridge.pending.is_some() || self.iterm_control_pending())
            && self.desktop.host.is_none()
        {
            changed |= self.start_desktop_host();
        }
        #[cfg(target_os = "macos")]
        {
            changed |= self.emit_bridge_control();
        }
        if let Some(delivery) = &self.desktop.in_flight {
            if !self.desktop_notification_valid(&delivery.notification) {
                delivery.cancel();
            }
            if matches!(
                delivery.state.load(Ordering::Acquire),
                DELIVERY_FINISHED | DELIVERY_CANCELLED_FINISHED
            ) {
                self.desktop.in_flight = None;
            }
        }
        // Keep queued responses in the dashboard until the single host operation finishes.
        // They are validated against current identity and channel prefs at dispatch time.
        while self.desktop.in_flight.is_none() {
            let Some(notification) = self.desktop.pending.pop_front() else {
                break;
            };
            if !self.desktop_notification_valid(&notification) {
                continue;
            }
            if self.desktop.host.is_none() {
                changed |= self.start_desktop_host();
                if self.desktop.host.is_none() {
                    continue;
                }
            }
            let delivery = Delivery {
                notification,
                state: Arc::new(AtomicU8::new(DELIVERY_ACTIVE)),
                #[cfg(target_os = "macos")]
                started: Arc::new(AtomicBool::new(false)),
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
                self.show_notice(unavailable_notice(failed));
                changed = true;
            }
        }
        changed
    }

    fn start_desktop_host(&mut self) -> bool {
        let host = self
            .desktop
            .wake
            .as_ref()
            .map(std::os::unix::net::UnixStream::try_clone)
            .transpose()
            .and_then(|wake| {
                DesktopHost::start(
                    wake,
                    self.alert_channels(),
                    #[cfg(target_os = "macos")]
                    self.notification_sound(),
                )
            });
        match host {
            Ok(host) => {
                self.desktop.host = Some(host);
                false
            }
            Err(_) => {
                self.show_notice(unavailable_notice(self.alert_channels()));
                #[cfg(target_os = "macos")]
                {
                    self.desktop.bridge.pending = None;
                    self.desktop.bridge.status = Some(BridgeStatus::Failed);
                    if self.iterm_control_pending() {
                        self.fail_iterm_host();
                    }
                }
                true
            }
        }
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

#[cfg(target_os = "macos")]
fn identity_subtitle(session: &SessionSummary) -> String {
    safe_identity(session.manual_title.as_deref().unwrap_or(&session.name))
}

fn unavailable_notice(channels: u8) -> &'static str {
    match channels {
        CHANNEL_SOUND => "Ready sound unavailable",
        CHANNEL_DESKTOP => "Desktop notifications unavailable",
        _ => "Desktop notifications and ready sound unavailable",
    }
}

fn safe_identity(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c,
                    '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}'
                    | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
                )
        })
        .take(80)
        .collect()
}

struct DesktopHost {
    sender: mpsc::SyncSender<(u64, Delivery)>,
    failures: mpsc::Receiver<(u64, u8)>,
    epoch: Arc<AtomicU64>,
    channels: Arc<AtomicU8>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    #[cfg(target_os = "macos")]
    controls: mpsc::SyncSender<BridgeControl>,
    #[cfg(target_os = "macos")]
    updates: mpsc::Receiver<BridgeUpdate>,
    #[cfg(target_os = "macos")]
    sound: Arc<Mutex<Option<ReadySoundChoice>>>,
}

impl DesktopHost {
    fn start(
        wake: Option<std::os::unix::net::UnixStream>,
        channels: u8,
        #[cfg(target_os = "macos")] sound: Option<ReadySoundChoice>,
    ) -> std::io::Result<Self> {
        #[cfg(target_os = "macos")]
        {
            use ovrcr_protocol::bridge_installation::LOCAL_DEVELOPMENT_ENV;
            let home = std::env::var_os("HOME");
            let opt_in = std::env::var_os(LOCAL_DEVELOPMENT_ENV);
            Self::start_with_environment(wake, channels, sound, home.as_deref(), opt_in.as_deref())
        }
        #[cfg(not(target_os = "macos"))]
        Self::start_at(wake, channels)
    }

    #[cfg(target_os = "macos")]
    fn start_with_environment(
        wake: Option<std::os::unix::net::UnixStream>,
        channels: u8,
        sound: Option<ReadySoundChoice>,
        home: Option<&std::ffi::OsStr>,
        opt_in: Option<&std::ffi::OsStr>,
    ) -> std::io::Result<Self> {
        let profile = BridgeProfile::from_opt_in(opt_in).map_err(std::io::Error::other)?;
        let client = bridge_client_path(home, profile);
        if profile == BridgeProfile::Production {
            Self::start_at(wake, channels, sound, client)
        } else {
            Self::start_with_profile(wake, channels, sound, client, profile)
        }
    }

    fn start_at(
        wake: Option<std::os::unix::net::UnixStream>,
        channels: u8,
        #[cfg(target_os = "macos")] sound: Option<ReadySoundChoice>,
        #[cfg(target_os = "macos")] bridge_client: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        Self::start_with_profile(
            wake,
            channels,
            #[cfg(target_os = "macos")]
            sound,
            #[cfg(target_os = "macos")]
            bridge_client,
            #[cfg(target_os = "macos")]
            BridgeProfile::Production,
        )
    }

    fn start_with_profile(
        mut wake: Option<std::os::unix::net::UnixStream>,
        channels: u8,
        #[cfg(target_os = "macos")] sound: Option<ReadySoundChoice>,
        #[cfg(target_os = "macos")] bridge_client: Option<PathBuf>,
        #[cfg(target_os = "macos")] profile: BridgeProfile,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<(u64, Delivery)>(1);
        let (failure, failures) = mpsc::sync_channel(2);
        #[cfg(target_os = "macos")]
        drop(failure);
        #[cfg(target_os = "macos")]
        let (controls, control_requests) = mpsc::sync_channel::<BridgeControl>(1);
        #[cfg(target_os = "macos")]
        let (updated, updates) = mpsc::sync_channel(4);
        let epoch = Arc::new(AtomicU64::new(0));
        let channels = Arc::new(AtomicU8::new(channels));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_epoch = Arc::clone(&epoch);
        let worker_channels = Arc::clone(&channels);
        let worker_stop = Arc::clone(&stop);
        #[cfg(target_os = "macos")]
        let sound = Arc::new(Mutex::new(sound));
        #[cfg(target_os = "macos")]
        let worker_sound = Arc::clone(&sound);
        let worker = thread::Builder::new()
            .name("ovrcr-desktop-notifications".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    let work = receiver.recv_timeout(HOST_POLL);
                    #[cfg(target_os = "macos")]
                    if let Ok(control) = control_requests.try_recv() {
                        let cancelled = || {
                            worker_stop.load(Ordering::Acquire)
                                || control.state.load(Ordering::Acquire) != DELIVERY_ACTIVE
                        };
                        if !cancelled() {
                            let reply = run_bridge_with_profile(
                                bridge_client.as_deref(),
                                profile,
                                &control.operation,
                                cancelled,
                            );
                            let fault = bridge_fault(bridge_client.as_deref(), reply.status);
                            let _ = updated.try_send(BridgeUpdate {
                                operation: control.operation.clone(),
                                reply,
                                fault,
                                state: Arc::clone(&control.state),
                                ticket: None,
                            });
                        }
                        let _ = control.state.compare_exchange(
                            DELIVERY_ACTIVE,
                            DELIVERY_FINISHED,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                        if let Some(wake) = &mut wake {
                            super::event_loop::notify_dashboard_wake(wake);
                        }
                    }
                    let Ok((ticket, delivery)) = work else {
                        continue;
                    };
                    let cancelled = || {
                        worker_epoch.load(Ordering::Acquire) != ticket
                            || worker_stop.load(Ordering::Acquire)
                            || delivery.state.load(Ordering::Acquire) != DELIVERY_ACTIVE
                    };
                    #[cfg(target_os = "macos")]
                    if !cancelled()
                        && worker_channels.load(Ordering::Acquire) & CHANNEL_DESKTOP != 0
                        && let Some(navigation) = delivery.notification.navigation.clone()
                    {
                        let sound = worker_sound.lock().ok().and_then(|choice| {
                            delivery.started.store(true, Ordering::Release);
                            *choice
                        });
                        let operation = BridgeOperation::Deliver {
                            title: delivery.notification.title.into(),
                            subtitle: delivery.notification.subtitle.clone(),
                            body: delivery.notification.body.clone(),
                            sound,
                            navigation,
                        };
                        let reply = run_bridge_with_profile(
                            bridge_client.as_deref(),
                            profile,
                            &operation,
                            || {
                                cancelled()
                                    || worker_channels.load(Ordering::Acquire) & CHANNEL_DESKTOP
                                        == 0
                            },
                        );
                        if !cancelled()
                            && worker_channels.load(Ordering::Acquire) & CHANNEL_DESKTOP != 0
                        {
                            let fault = bridge_fault(bridge_client.as_deref(), reply.status);
                            let _ = updated.try_send(BridgeUpdate {
                                operation,
                                reply,
                                fault,
                                state: Arc::clone(&delivery.state),
                                ticket: Some(ticket),
                            });
                        }
                    }
                    // ponytail: Linux channels run one after another; run them
                    // concurrently if a sound-length backlog ever matters.
                    #[cfg(not(target_os = "macos"))]
                    for channel in [CHANNEL_DESKTOP, CHANNEL_SOUND] {
                        // A toggle applies to every host command not yet started.
                        if cancelled() || worker_channels.load(Ordering::Acquire) & channel == 0 {
                            continue;
                        }
                        let (command, timeout) = if channel == CHANNEL_SOUND {
                            (sound_command(), SOUND_TIMEOUT)
                        } else {
                            (host_command(&delivery.notification), HOST_TIMEOUT)
                        };
                        if !run_host(command, timeout, cancelled) && !cancelled() {
                            let _ = failure.try_send((ticket, channel));
                        }
                    }
                    // Set the finished bit without clearing cancellation.
                    let _ = delivery.state.fetch_or(DELIVERY_FINISHED, Ordering::AcqRel);
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
            #[cfg(target_os = "macos")]
            controls,
            #[cfg(target_os = "macos")]
            updates,
            #[cfg(target_os = "macos")]
            sound,
        })
    }
    fn enabled(&self) -> bool {
        self.channels.load(Ordering::Acquire) != 0
    }
    fn set_channels(
        &mut self,
        channels: u8,
        #[cfg(target_os = "macos")] sound: Option<ReadySoundChoice>,
        #[cfg(target_os = "macos")] cancel_sound_delivery: Option<&Delivery>,
    ) {
        #[cfg(target_os = "macos")]
        if let Ok(mut choice) = self.sound.lock() {
            *choice = sound;
            if let Some(delivery) = cancel_sound_delivery
                && delivery.started.load(Ordering::Acquire)
            {
                // Only this client is cancelled, best effort. Native/OS
                // admission may already have happened; no retraction or replay.
                delivery.cancel();
            }
        }
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

/// `--check-contract` runs before any app, notification or IPC lifecycle, so
/// it tells a stale installed app from a stale running process.
#[cfg(target_os = "macos")]
fn bridge_fault(client: Option<&std::path::Path>, status: BridgeStatus) -> Option<BridgeFault> {
    let client = client?;
    match status {
        BridgeStatus::Failed if std::fs::symlink_metadata(client).is_err() => {
            Some(BridgeFault::NotInstalled)
        }
        BridgeStatus::Incompatible => {
            let mut command = Command::new(client);
            command
                .arg("--check-contract")
                .arg(BRIDGE_SCHEMA_VERSION.to_string())
                .arg(PROTOCOL_VERSION.to_string());
            Some(if run_host(command, Duration::from_secs(1), || false) {
                BridgeFault::ProcessOutdated
            } else {
                BridgeFault::AppOutdated
            })
        }
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn bridge_client_path(home: Option<&std::ffi::OsStr>, profile: BridgeProfile) -> Option<PathBuf> {
    home.filter(|home| !home.is_empty())
        .map(|home| profile.client_path(std::path::Path::new(home)))
}

#[cfg(all(test, target_os = "macos"))]
fn run_bridge(
    client: Option<&std::path::Path>,
    operation: &BridgeOperation,
    cancelled: impl Fn() -> bool,
) -> BridgeReply {
    run_bridge_with_profile(client, BridgeProfile::Production, operation, cancelled)
}

/// Only the short-lived client is a child. Its LaunchServices app lives outside
/// this owned process group and survives cancellation or Dashboard detach.
#[cfg(target_os = "macos")]
fn run_bridge_with_profile(
    client: Option<&std::path::Path>,
    profile: BridgeProfile,
    operation: &BridgeOperation,
    cancelled: impl Fn() -> bool,
) -> BridgeReply {
    let unavailable = |status| BridgeReply {
        schema: BRIDGE_SCHEMA_VERSION,
        server_wire: PROTOCOL_VERSION,
        status,
        sound_unavailable: None,
    };
    let request = BridgeRequest {
        schema: BRIDGE_SCHEMA_VERSION,
        server_wire: PROTOCOL_VERSION,
        op: operation.clone(),
    };
    let Ok(input) = serde_json::to_vec(&request) else {
        return unavailable(BridgeStatus::Failed);
    };
    let Some(client) = client else {
        return unavailable(BridgeStatus::Failed);
    };
    if cancelled() || input.len() > MAX_FRAME_BYTES {
        return unavailable(BridgeStatus::Failed);
    }
    let deadline = Instant::now() + HOST_TIMEOUT;
    let mut command = Command::new(client);
    command.arg("--client");
    if profile == BridgeProfile::LocalDevelopment {
        // The native entry guard rejects a wrong-mode bundle before input or IPC.
        command.arg("--local-development");
    }
    let Ok(mut child) = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    else {
        return unavailable(BridgeStatus::Failed);
    };
    let mut reaped = false;
    let result = (|| -> std::io::Result<BridgeReply> {
        let mut stdin = child.stdin.take();
        let mut stdout = child.stdout.take().unwrap();
        for fd in [stdin.as_ref().unwrap().as_raw_fd(), stdout.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags == -1
                || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        let mut written = 0;
        let mut output = Vec::new();
        let mut eof = false;
        loop {
            if cancelled() || Instant::now() >= deadline {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            if let Some(writer) = &mut stdin {
                match writer.write(&input[written..]) {
                    Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
                    Ok(bytes) => written += bytes,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
                if written == input.len() {
                    stdin = None;
                }
            }
            let mut buffer = [0; 8192];
            loop {
                match stdout.read(&mut buffer) {
                    Ok(0) => {
                        eof = true;
                        break;
                    }
                    Ok(bytes) => {
                        if output.len() + bytes > MAX_FRAME_BYTES {
                            return Err(std::io::ErrorKind::InvalidData.into());
                        }
                        output.extend_from_slice(&buffer[..bytes]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error),
                }
            }
            if let Some(status) = child.try_wait()? {
                reaped = true;
                if !status.success() {
                    return Err(std::io::ErrorKind::Other.into());
                }
                if eof {
                    let reply: BridgeReply = serde_json::from_slice(&output)
                        .map_err(|_| std::io::ErrorKind::InvalidData)?;
                    if reply.sound_unavailable.is_some() && reply.status != BridgeStatus::Submitted
                    {
                        return Err(std::io::ErrorKind::InvalidData.into());
                    }
                    return Ok(
                        if reply.schema == BRIDGE_SCHEMA_VERSION
                            && reply.server_wire == PROTOCOL_VERSION
                        {
                            reply
                        } else {
                            unavailable(BridgeStatus::Incompatible)
                        },
                    );
                }
            }
            thread::park_timeout(HOST_POLL);
        }
    })();
    match result {
        Ok(status) => status,
        Err(_) => {
            // Reaping ends ownership of the client's PID and process group.
            if !reaped {
                unsafe {
                    libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
                }
                let _ = child.wait();
            }
            unavailable(BridgeStatus::Failed)
        }
    }
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
                        manual_title: None,
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
        super::super::tests::press_setting(&mut d, 'N');
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
    fn desktop_input_request_in_a_visible_pane_alerts() {
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
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "visible focused pane still alerts when channels are on"
        );
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
        super::super::tests::press_setting(&mut d, 'N');
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
        super::super::tests::press_setting(&mut d, 'N');
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
            #[cfg(target_os = "macos")]
            started: Arc::new(AtomicBool::new(false)),
        });
        let (receiver, _failed) = stub_host(&mut d);
        assert_eq!(d.desktop.pending.len(), 1);
        (d, receiver)
    }
    #[test]
    fn desktop_queued_visible_then_hidden_candidate_still_delivers() {
        let (mut d, receiver) = queued_behind_active();
        d.select_session(SessionId(2));
        d.emit_desktop_notifications();
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "becoming visible must not cancel a queued response"
        );
        d.select_session(SessionId(1));
        d.desktop
            .in_flight
            .as_ref()
            .unwrap()
            .state
            .store(DELIVERY_FINISHED, Ordering::Release);
        d.emit_desktop_notifications();
        let (_, delivery) = receiver
            .try_recv()
            .expect("must still deliver after visibility churn");
        assert_eq!(delivery.notification.session, SessionId(2));
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
        super::super::tests::press_setting(&mut d, 'N');
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
        let host = DesktopHost::start(
            Some(sender),
            CHANNEL_DESKTOP,
            #[cfg(target_os = "macos")]
            None,
        )
        .unwrap();
        assert!(host.send(Delivery {
            notification: d.desktop.pending.pop_front().unwrap(),
            state: Arc::new(AtomicU8::new(DELIVERY_CANCELLED)),
            #[cfg(target_os = "macos")]
            started: Arc::new(AtomicBool::new(false)),
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
                "provider" => s.agent.as_mut().unwrap().binding.provider = AgentProvider::Grok,
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
        super::super::tests::press_setting(&mut d, 'N');
        d.handle_server_message(ServerMessage::Response {
            request_id: 1,
            response: Response::Hierarchy(snapshot(2, "a", AgentActivity::ResponseReady)),
        });
        assert!(d.desktop.pending.is_empty(), "attach is a baseline");
        super::super::tests::press_setting(&mut d, 'N');
        deliver(&mut d, snapshot(4, "b", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty(), "disabled event is consumed");
        super::super::tests::press_setting(&mut d, 'N');
        deliver(&mut d, snapshot(4, "b", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty(), "enable does not replay");
        deliver(&mut d, snapshot(6, "c", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
        super::super::tests::press_setting(&mut d, 'N');
        assert!(
            d.desktop.pending.is_empty(),
            "disable cancels queued candidates"
        );
    }
    #[test]
    fn desktop_visibility_does_not_suppress_alerts_including_split_and_empty_view() {
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
        assert!(
            d.desktop_session_visible(SessionId(1)),
            "focused pane is visible"
        );
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "visible focused pane still alerts"
        );
        d.desktop.pending.clear();
        d.panes.push(crate::dashboard::PaneState::new(TerminalSize {
            rows: 24,
            cols: 80,
        }));
        d.panes[1].session = Some(SessionId(2));
        d.focused_pane = 1;
        assert!(
            d.desktop_session_visible(SessionId(1)),
            "unfocused assigned pane stays visible"
        );
        deliver(&mut d, snapshot(4, "b", AgentActivity::ResponseReady));
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "visible unfocused pane still alerts"
        );
        d.desktop.pending.clear();
        d.outer_area = Rect::new(0, 0, 60, 24);
        assert!(
            !d.desktop_session_visible(SessionId(1)),
            "narrow geometry hides the assigned split"
        );
        deliver(&mut d, snapshot(6, "c", AgentActivity::ResponseReady));
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "hidden-by-geometry session still alerts"
        );
        d.desktop.pending.clear();
        d.outer_area = Rect::new(0, 0, 120, 24);
        deliver(&mut d, snapshot(8, "d", AgentActivity::ResponseReady));
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "becoming visible again still alerts a new unread"
        );
        d.desktop.pending.clear();
        d.outer_area = Rect::new(0, 0, 120, 2);
        deliver(&mut d, snapshot(8, "d", AgentActivity::ResponseReady));
        assert!(
            d.desktop.pending.is_empty(),
            "same unread never replays after geometry shrink"
        );
        deliver(&mut d, snapshot(10, "e", AgentActivity::ResponseReady));
        assert_eq!(
            d.desktop.pending.len(),
            1,
            "empty / background view still alerts"
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
        #[cfg(target_os = "macos")]
        {
            assert_eq!(notice.subtitle, "namewithcontrols <b> & \"$(secret)\"");
        }
        #[cfg(not(target_os = "macos"))]
        {
            let command = host_command(notice);
            let args: Vec<_> = command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
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
        super::super::tests::press_setting(&mut d, 'N');
        super::super::tests::press_setting(&mut d, 'N');
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
        #[cfg(target_os = "macos")]
        {
            d.desktop.bridge.status = Some(BridgeStatus::Denied);
            assert_eq!(
                d.key(KeyCode::Char('O')),
                DashboardAction::PtyBytes(b"O".to_vec())
            );
            assert!(d.desktop.bridge.pending.as_ref() != Some(&BridgeOperation::Settings));
        }
        assert!(d.settings.desktop_notifications);
    }
    type StubHost = (mpsc::Receiver<(u64, Delivery)>, mpsc::SyncSender<(u64, u8)>);
    fn stub_host(d: &mut Dashboard) -> StubHost {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (failed, failures) = mpsc::sync_channel(2);
        #[cfg(target_os = "macos")]
        let (controls, _control_requests) = mpsc::sync_channel(1);
        #[cfg(target_os = "macos")]
        let (_updated, updates) = mpsc::sync_channel(4);
        d.desktop.host = Some(DesktopHost {
            sender,
            failures,
            epoch: Arc::new(AtomicU64::new(0)),
            channels: Arc::new(AtomicU8::new(d.alert_channels())),
            stop: Arc::new(AtomicBool::new(false)),
            worker: None,
            #[cfg(target_os = "macos")]
            controls,
            #[cfg(target_os = "macos")]
            updates,
            #[cfg(target_os = "macos")]
            sound: Arc::new(Mutex::new(d.notification_sound())),
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
            if !desktop && (!sound || cfg!(target_os = "macos")) {
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
            assert_eq!(
                mask & CHANNEL_SOUND != 0,
                sound && (desktop || !cfg!(target_os = "macos"))
            );
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
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn ready_sound_toggle_is_independent_and_only_disabling_both_cancels_pending() {
        let mut d = dashboard();
        assert!(matches!(
            super::super::tests::press_setting(&mut d, 'S'),
            DashboardAction::Request(_)
        ));
        assert!(d.settings.ready_sound && d.settings.desktop_notifications);
        assert_eq!(d.desktop.notice.as_deref(), Some("Ready sound: on"));
        let (_receiver, _failed) = stub_host(&mut d);
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
        super::super::tests::press_setting(&mut d, 'N');
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
        super::super::tests::press_setting(&mut d, 'S');
        assert_eq!(d.desktop.notice.as_deref(), Some("Ready sound: off"));
        assert!(
            d.desktop.pending.is_empty(),
            "disabling the last channel cancels"
        );
        let host = d.desktop.host.as_ref().unwrap();
        assert_eq!(host.channels.load(Ordering::Acquire), 0);
        assert_eq!(host.epoch.load(Ordering::Acquire), 1);
        super::super::tests::press_setting(&mut d, 'S');
        deliver(&mut d, snapshot(2, "a", AgentActivity::ResponseReady));
        assert!(d.desktop.pending.is_empty(), "enabling sound never replays");
        deliver(&mut d, snapshot(4, "b", AgentActivity::ResponseReady));
        assert_eq!(d.desktop.pending.len(), 1);
    }
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn sound_host_command_is_a_fixed_system_player_without_identity() {
        let command = sound_command();
        let program = command.get_program().to_string_lossy().into_owned();
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(program, "paplay");
        assert_eq!(args, ["/usr/share/sounds/freedesktop/stereo/complete.oga"]);
        assert!(SOUND_TIMEOUT >= HOST_TIMEOUT);
    }
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn sound_host_failure_reports_its_own_notice() {
        let mut d = dashboard();
        super::super::tests::press_setting(&mut d, 'S');
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
        super::super::tests::press_setting(&mut d, 'S');
        d.desktop.notice = None;
        failed.send((0, CHANNEL_SOUND)).unwrap();
        assert!(
            !d.emit_desktop_notifications(),
            "a channel disabled after its command started reports nothing"
        );
        assert!(d.desktop.notice.is_none());
        assert_eq!(unavailable_notice(CHANNEL_SOUND), "Ready sound unavailable");
    }
    #[cfg(target_os = "macos")]
    fn fake_bridge(status: &str) -> (tempfile::TempDir, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let client = root.path().join("OVRCRBridge");
        std::fs::write(
            &client,
            format!(
                r#"#!/bin/sh
[ "$1" = '--client' ] && [ "$#" = 1 ] || exit 99
input=$(cat)
printf '%s\n' "$input" >> "$0.requests"
while [ -e "$0.pause" ]; do sleep 0.01; done
status=$(cat "$0.status")
case "$input" in
  *'"type":"authorize"'*) status=$(cat "$0.authorize"); printf '%s' "$status" > "$0.status" ;;
  *'"type":"settings"'*) status=$(cat "$0.settings") ;;
  *'"type":"deliver"'*) [ "$status" != available ] || status=submitted ;;
esac
extra=''
if [ "$status" = submitted ] && [ -s "$0.sound_failure" ]; then
  extra=$(cat "$0.sound_failure")
fi
printf '{{"schema":{},"server_wire":{},"status":"%s"%s}}\n' "$status" "$extra"
"#,
                BRIDGE_SCHEMA_VERSION, PROTOCOL_VERSION
            ),
        )
        .unwrap();
        std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(client.with_extension("status"), status).unwrap();
        std::fs::write(client.with_extension("settings"), "settings_opened").unwrap();
        std::fs::write(client.with_extension("authorize"), "available").unwrap();
        (root, client)
    }

    #[cfg(target_os = "macos")]
    fn bridge_requests(client: &std::path::Path) -> Vec<BridgeRequest> {
        std::fs::read_to_string(client.with_extension("requests"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[cfg(target_os = "macos")]
    fn profile_fake_bridge(
        home: &std::path::Path,
        profile: BridgeProfile,
        status: &str,
    ) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let (_root, source) = fake_bridge(status);
        let client = profile.client_path(home);
        std::fs::create_dir_all(client.parent().unwrap()).unwrap();
        let guard = match profile {
            BridgeProfile::Production => "[ \"$1\" = '--client' ] && [ \"$#\" = 1 ] || exit 99",
            BridgeProfile::LocalDevelopment => {
                "[ \"$1\" = '--client' ] && [ \"$2\" = '--local-development' ] && [ \"$#\" = 2 ] || exit 99"
            }
        };
        let script = std::fs::read_to_string(&source).unwrap().replace(
            "[ \"$1\" = '--client' ] && [ \"$#\" = 1 ] || exit 99",
            &format!("{guard}\nprintf '%s\\n' \"$@\" > \"$0.arguments\""),
        );
        std::fs::write(&client, script).unwrap();
        std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).unwrap();
        for extension in ["status", "settings", "authorize"] {
            std::fs::copy(
                source.with_extension(extension),
                client.with_extension(extension),
            )
            .unwrap();
        }
        client
    }

    #[cfg(target_os = "macos")]
    fn host_status(host: &DesktopHost) -> BridgeReply {
        host.controls
            .send(BridgeControl {
                operation: BridgeOperation::Status,
                state: Arc::new(AtomicU8::new(DELIVERY_ACTIVE)),
            })
            .unwrap();
        let update = host.updates.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(update.operation, BridgeOperation::Status);
        update.reply
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_production_constructor_selects_one_fixed_profile_and_guarded_client() {
        use std::ffi::OsStr;
        for (opt_in, profile, expected_status, expected_args) in [
            (
                None,
                BridgeProfile::Production,
                BridgeStatus::Denied,
                "--client\n",
            ),
            (
                Some(OsStr::new("")),
                BridgeProfile::Production,
                BridgeStatus::Denied,
                "--client\n",
            ),
            (
                Some(OsStr::new("0")),
                BridgeProfile::Production,
                BridgeStatus::Denied,
                "--client\n",
            ),
            (
                Some(OsStr::new("1")),
                BridgeProfile::LocalDevelopment,
                BridgeStatus::Available,
                "--client\n--local-development\n",
            ),
        ] {
            let home = tempfile::tempdir().unwrap();
            let production = profile_fake_bridge(home.path(), BridgeProfile::Production, "denied");
            let local =
                profile_fake_bridge(home.path(), BridgeProfile::LocalDevelopment, "available");
            let host = DesktopHost::start_with_environment(
                None,
                CHANNEL_DESKTOP,
                None,
                Some(home.path().as_os_str()),
                opt_in,
            )
            .unwrap();
            assert_eq!(host_status(&host).status, expected_status);
            drop(host);
            let (selected, other) = if profile == BridgeProfile::Production {
                (&production, &local)
            } else {
                (&local, &production)
            };
            assert_eq!(bridge_requests(selected).len(), 1);
            assert!(bridge_requests(other).is_empty());
            assert_eq!(
                std::fs::read_to_string(selected.with_extension("arguments")).unwrap(),
                expected_args
            );
            assert!(!other.with_extension("arguments").exists());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_production_constructor_refuses_invalid_opt_in_before_any_client() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let home = tempfile::tempdir().unwrap();
        let production = profile_fake_bridge(home.path(), BridgeProfile::Production, "available");
        for opt_in in [
            OsStr::new("true"),
            OsStr::new("01"),
            OsStr::new(" 1"),
            OsStr::from_bytes(b"\xff"),
        ] {
            let result = DesktopHost::start_with_environment(
                None,
                CHANNEL_DESKTOP,
                None,
                Some(home.path().as_os_str()),
                Some(opt_in),
            );
            assert!(result.is_err());
        }
        assert!(bridge_requests(&production).is_empty());
        assert!(!production.with_extension("arguments").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_local_constructor_missing_client_never_uses_production() {
        let home = tempfile::tempdir().unwrap();
        let production = profile_fake_bridge(home.path(), BridgeProfile::Production, "available");
        let host = DesktopHost::start_with_environment(
            None,
            CHANNEL_DESKTOP,
            None,
            Some(home.path().as_os_str()),
            Some(std::ffi::OsStr::new("1")),
        )
        .unwrap();
        assert_eq!(host_status(&host).status, BridgeStatus::Failed);
        drop(host);
        assert!(bridge_requests(&production).is_empty());
        assert!(!production.with_extension("arguments").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_local_constructor_flags_wrong_mode_client_before_input() {
        let home = tempfile::tempdir().unwrap();
        let production = profile_fake_bridge(home.path(), BridgeProfile::Production, "available");
        let local = BridgeProfile::LocalDevelopment.client_path(home.path());
        std::fs::create_dir_all(local.parent().unwrap()).unwrap();
        std::fs::copy(&production, &local).unwrap();
        let host = DesktopHost::start_with_environment(
            None,
            CHANNEL_DESKTOP,
            None,
            Some(home.path().as_os_str()),
            Some(std::ffi::OsStr::new("1")),
        )
        .unwrap();
        assert_eq!(host_status(&host).status, BridgeStatus::Failed);
        drop(host);
        assert!(bridge_requests(&production).is_empty());
        assert!(
            bridge_requests(&local).is_empty(),
            "wrong mode must refuse before reading the request"
        );
        assert!(!local.with_extension("arguments").exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_local_constructor_keeps_profile_for_notification_delivery() {
        let home = tempfile::tempdir().unwrap();
        let local = profile_fake_bridge(home.path(), BridgeProfile::LocalDevelopment, "available");
        let production = profile_fake_bridge(home.path(), BridgeProfile::Production, "available");
        let mut d = dashboard();
        install_fake_bridge(&mut d, local.clone());
        deliver(
            &mut d,
            snapshot(2, "local-turn", AgentActivity::ResponseReady),
        );
        let notification = d.desktop.pending.pop_front().unwrap();
        assert!(notification.navigation.is_some());
        let host = DesktopHost::start_with_environment(
            None,
            CHANNEL_DESKTOP,
            None,
            Some(home.path().as_os_str()),
            Some(std::ffi::OsStr::new("1")),
        )
        .unwrap();
        assert!(host.send(Delivery {
            notification,
            state: Arc::new(AtomicU8::new(DELIVERY_ACTIVE)),
            started: Arc::new(AtomicBool::new(false)),
        }));
        let update = host.updates.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(update.reply.status, BridgeStatus::Submitted);
        assert!(matches!(
            update.operation,
            BridgeOperation::Deliver { sound: None, .. }
        ));
        drop(host);
        assert!(matches!(
            bridge_requests(&local)[0].op,
            BridgeOperation::Deliver { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(local.with_extension("arguments")).unwrap(),
            "--client\n--local-development\n"
        );
        assert!(bridge_requests(&production).is_empty());
    }

    #[cfg(target_os = "macos")]
    fn install_fake_bridge(d: &mut Dashboard, client: PathBuf) {
        d.navigation.context = Some(ovrcr_protocol::BridgeContext {
            server_socket: "/tmp/owning-fake-server.sock".into(),
            callback_executable: "/tmp/ovrcr".into(),
            callback_executable_sha256: "0".repeat(64),
            server_lifetime: "12345678-1234-4234-8234-123456789abc".into(),
        });
        d.desktop.host = Some(
            DesktopHost::start_at(
                None,
                d.alert_channels(),
                d.notification_sound(),
                Some(client),
            )
            .unwrap(),
        );
    }

    #[cfg(target_os = "macos")]
    fn wait_bridge(d: &mut Dashboard, ready: impl Fn(&Dashboard) -> bool) {
        let until = Instant::now() + Duration::from_secs(3);
        loop {
            d.emit_desktop_notifications();
            if ready(d) {
                return;
            }
            assert!(Instant::now() < until, "Bridge worker did not complete");
            thread::park_timeout(HOST_POLL);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_startup_and_accepted_settings_request_permission_without_an_alert() {
        let (_root, client) = fake_bridge("not_determined");
        let mut d = Dashboard::new(TerminalSize {
            rows: 24,
            cols: 120,
        });
        install_fake_bridge(&mut d, client.clone());
        let mut settings = d.settings.clone();
        settings.desktop_notifications = true;
        d.handle_server_message(super::super::tests::settings_reading(settings.clone()));
        assert!(d.desktop.pending.is_empty());
        wait_bridge(&mut d, |d| {
            d.desktop.bridge.status == Some(BridgeStatus::Available)
        });
        assert_eq!(
            bridge_requests(&client)
                .iter()
                .map(|r| &r.op)
                .collect::<Vec<_>>(),
            [&BridgeOperation::Status, &BridgeOperation::Authorize]
        );
        for _ in 0..5 {
            d.handle_server_message(super::super::tests::settings_reading(settings.clone()));
            d.emit_desktop_notifications();
        }
        assert_eq!(
            bridge_requests(&client).len(),
            2,
            "unchanged accepted settings never reauthorize"
        );
        let DashboardAction::Request(request) = d.key(KeyCode::Char('N')) else {
            panic!("expected Server save");
        };
        assert!(
            d.settings.desktop_notifications,
            "a request does not change the preference"
        );
        d.handle_server_message(ServerMessage::Response {
            request_id: request.request_id,
            response: Response::Error {
                code: ovrcr_protocol::ErrorCode::Conflict,
                message: "fixture save failed".into(),
            },
        });
        d.emit_desktop_notifications();
        assert!(d.settings.desktop_notifications);
        assert_eq!(
            bridge_requests(&client).len(),
            2,
            "failed save never changes permission lifecycle"
        );
        settings.desktop_notifications = false;
        d.handle_server_message(super::super::tests::settings_reading(settings.clone()));
        std::fs::write(client.with_extension("status"), "not_determined").unwrap();
        settings.desktop_notifications = true;
        d.handle_server_message(super::super::tests::settings_reading(settings));
        wait_bridge(&mut d, |_| bridge_requests(&client).len() == 4);
        assert_eq!(
            bridge_requests(&client)
                .iter()
                .map(|r| &r.op)
                .collect::<Vec<_>>(),
            [
                &BridgeOperation::Status,
                &BridgeOperation::Authorize,
                &BridgeOperation::Status,
                &BridgeOperation::Authorize
            ]
        );
        assert!(d.desktop.pending.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_off_and_sound_only_settings_do_not_start_the_native_host() {
        let mut d = Dashboard::new(TerminalSize {
            rows: 24,
            cols: 120,
        });
        for sound in [false, true] {
            let mut settings = d.settings.clone();
            settings.ready_sound = sound;
            d.handle_server_message(super::super::tests::settings_reading(settings));
            d.emit_desktop_notifications();
            assert!(d.desktop.host.is_none());
            assert!(d.desktop.bridge.pending.is_none());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_sound_choices_flags_and_both_lanes_use_server_settings() {
        for choice in [
            Some(ReadySoundChoice::Default),
            Some(ReadySoundChoice::Tap),
            Some(ReadySoundChoice::Chime),
            Some(ReadySoundChoice::Rise),
            None,
        ] {
            for (desktop, sound) in [(false, false), (true, false), (false, true), (true, true)] {
                let (_root, client) = fake_bridge("available");
                let mut d = Dashboard::new(TerminalSize {
                    rows: 24,
                    cols: 120,
                });
                let settings = Settings {
                    desktop_notifications: desktop,
                    ready_sound: sound,
                    ready_sound_choice: choice,
                    ..Default::default()
                };
                d.handle_server_message(super::super::tests::settings_reading(settings.clone()));
                deliver(&mut d, snapshot(1, "baseline", AgentActivity::Busy));
                install_fake_bridge(&mut d, client.clone());
                if desktop {
                    wait_bridge(&mut d, |d| {
                        d.desktop.bridge.status == Some(BridgeStatus::Available)
                    });
                }
                for hierarchy in [
                    snapshot(2, "ready", AgentActivity::ResponseReady),
                    snapshot_with_requests(
                        3,
                        "input",
                        AgentActivity::Busy,
                        &[request("question", InputKind::Input)],
                    ),
                ] {
                    deliver(&mut d, hierarchy.clone());
                    wait_bridge(&mut d, |d| {
                        d.desktop.pending.is_empty() && d.desktop.in_flight.is_none()
                    });
                    deliver(&mut d, hierarchy);
                    d.emit_desktop_notifications();
                }
                let requests = bridge_requests(&client);
                let banners: Vec<_> = requests
                    .iter()
                    .filter_map(|request| match &request.op {
                        BridgeOperation::Deliver { title, sound, .. } => {
                            Some((title.as_str(), *sound))
                        }
                        _ => None,
                    })
                    .collect();
                let selected = (desktop && sound).then_some(choice).flatten();
                if desktop {
                    assert_eq!(
                        banners,
                        [
                            ("OVRCR · response ready", selected),
                            ("OVRCR · input needed", selected)
                        ],
                        "choice={choice:?}, N={desktop}, S={sound}"
                    );
                    assert_eq!(
                        requests.len(),
                        3,
                        "one status and two banners, no sound retry or permission request"
                    );
                    assert_eq!(requests[0].op, BridgeOperation::Status);
                } else {
                    assert!(
                        requests.is_empty(),
                        "N off sends neither native sound nor banner nor permission controls"
                    );
                }
                assert_eq!(
                    d.settings, settings,
                    "delivery must not change saved flags or choice"
                );
                if desktop && sound && choice.is_none() {
                    assert_eq!(
                        d.desktop_status_notice(),
                        Some(
                            "Ready sound choice invalid; banners are silent. Choose a sound in Settings"
                        )
                    );
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_sound_changes_before_dispatch_apply_without_replay_or_permission() {
        let (_root, client) = fake_bridge("available");
        std::fs::write(client.with_extension("pause"), "").unwrap();
        let mut d = dashboard();
        super::super::tests::press_setting(&mut d, 'S');
        install_fake_bridge(&mut d, client.clone());
        wait_bridge(&mut d, |_| !bridge_requests(&client).is_empty());
        assert_eq!(bridge_requests(&client)[0].op, BridgeOperation::Status);
        deliver(&mut d, snapshot(2, "ready", AgentActivity::ResponseReady));
        d.emit_desktop_notifications();
        let mut settings = d.settings.clone();
        settings.ready_sound_choice = Some(ReadySoundChoice::Rise);
        d.handle_server_message(super::super::tests::settings_reading(settings.clone()));
        assert_eq!(
            bridge_requests(&client).len(),
            1,
            "held native control keeps banner unsubmitted"
        );
        std::fs::remove_file(client.with_extension("pause")).unwrap();
        wait_bridge(&mut d, |d| {
            d.desktop.pending.is_empty() && d.desktop.in_flight.is_none()
        });
        let requests = bridge_requests(&client);
        assert_eq!(requests.len(), 2);
        assert!(matches!(
            requests[1].op,
            BridgeOperation::Deliver {
                sound: Some(ReadySoundChoice::Rise),
                ..
            }
        ));
        settings.ready_sound_choice = Some(ReadySoundChoice::Tap);
        d.handle_server_message(super::super::tests::settings_reading(settings.clone()));
        deliver(&mut d, snapshot(2, "ready", AgentActivity::ResponseReady));
        d.emit_desktop_notifications();
        assert_eq!(
            bridge_requests(&client).len(),
            2,
            "choice change never replays the response"
        );
        deliver(
            &mut d,
            snapshot_with_requests(
                3,
                "input",
                AgentActivity::Busy,
                &[request("question", InputKind::Input)],
            ),
        );
        wait_bridge(&mut d, |d| {
            d.desktop.pending.is_empty() && d.desktop.in_flight.is_none()
        });
        let requests = bridge_requests(&client);
        assert_eq!(requests.len(), 3);
        assert!(matches!(
            requests[2].op,
            BridgeOperation::Deliver {
                sound: Some(ReadySoundChoice::Tap),
                ..
            }
        ));
        assert!(
            requests
                .iter()
                .all(|r| !matches!(r.op, BridgeOperation::Authorize | BridgeOperation::Settings))
        );
        assert_eq!(d.settings, settings);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_sound_changes_cancel_only_started_delivery_clients_without_replay() {
        for change in ["choice", "sound", "notifications"] {
            for input in [false, true] {
                let (_root, client) = fake_bridge("available");
                let mut d = dashboard();
                super::super::tests::press_setting(&mut d, 'S');
                let mut settings = d.settings.clone();
                settings.ready_sound_choice = Some(ReadySoundChoice::Rise);
                d.handle_server_message(super::super::tests::settings_reading(settings));
                install_fake_bridge(&mut d, client.clone());
                wait_bridge(&mut d, |d| {
                    d.desktop.bridge.status == Some(BridgeStatus::Available)
                });
                std::fs::write(client.with_extension("pause"), "").unwrap();
                let hierarchy = if input {
                    snapshot_with_requests(
                        2,
                        "input",
                        AgentActivity::Busy,
                        &[request("question", InputKind::Input)],
                    )
                } else {
                    snapshot(2, "ready", AgentActivity::ResponseReady)
                };
                deliver(&mut d, hierarchy.clone());
                wait_bridge(&mut d, |d| {
                    bridge_requests(&client).len() == 2
                        && d.desktop
                            .in_flight
                            .as_ref()
                            .is_some_and(|delivery| delivery.started.load(Ordering::Acquire))
                });
                let delivery = d.desktop.in_flight.clone().unwrap();
                let mut settings = d.settings.clone();
                match change {
                    "choice" => settings.ready_sound_choice = Some(ReadySoundChoice::Tap),
                    "sound" => settings.ready_sound = false,
                    "notifications" => settings.desktop_notifications = false,
                    _ => unreachable!(),
                }
                let started = Instant::now();
                d.handle_server_message(super::super::tests::settings_reading(settings.clone()));
                assert!(
                    started.elapsed() < Duration::from_millis(100),
                    "accepted preference update blocks on neither host nor permission"
                );
                wait_bridge(&mut d, |d| d.desktop.in_flight.is_none());
                assert_eq!(
                    delivery.state.load(Ordering::Acquire),
                    DELIVERY_CANCELLED_FINISHED,
                    "{change}, input={input}: owned client was cancelled"
                );
                assert_eq!(bridge_requests(&client).len(), 2, "no cancellation retry");
                assert_eq!(d.settings, settings);
                assert!(d.desktop.sound_unavailable.is_none());
                std::fs::remove_file(client.with_extension("pause")).unwrap();
                if change == "notifications" {
                    assert!(d.settings.ready_sound, "N off retains S");
                    assert_eq!(d.settings.ready_sound_choice, Some(ReadySoundChoice::Rise));
                    super::super::tests::press_setting(&mut d, 'N');
                    wait_bridge(&mut d, |d| {
                        d.desktop.bridge.status == Some(BridgeStatus::Available)
                            && d.desktop.bridge.in_flight.is_none()
                            && d.desktop.bridge.pending.is_none()
                    });
                }
                deliver(&mut d, hierarchy);
                d.emit_desktop_notifications();
                let controls = if change == "notifications" { 2 } else { 1 };
                assert_eq!(
                    bridge_requests(&client).len(),
                    controls + 1,
                    "accepted preference change does not replay cancelled work"
                );
                let next = if input {
                    snapshot_with_requests(
                        3,
                        "next-input",
                        AgentActivity::Busy,
                        &[request("next-question", InputKind::Input)],
                    )
                } else {
                    snapshot(3, "next-ready", AgentActivity::ResponseReady)
                };
                deliver(&mut d, next);
                wait_bridge(&mut d, |d| {
                    d.desktop.pending.is_empty() && d.desktop.in_flight.is_none()
                });
                let requests = bridge_requests(&client);
                assert_eq!(requests.len(), controls + 2);
                assert!(
                    matches!(requests.last().unwrap().op, BridgeOperation::Deliver { sound, .. } if sound == d.notification_sound())
                );
                assert!(requests.iter().all(|r| !matches!(
                    r.op,
                    BridgeOperation::Authorize | BridgeOperation::Settings
                )));
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_sound_resource_failure_is_a_silent_submitted_banner_without_retry() {
        for failure in [
            BridgeSoundFailure::MissingResource,
            BridgeSoundFailure::UnreadableResource,
            BridgeSoundFailure::InvalidResource,
            BridgeSoundFailure::LookupConflict,
        ] {
            let (_root, client) = fake_bridge("available");
            std::fs::write(
                client.with_extension("sound_failure"),
                format!(
                    ",\"sound_unavailable\":{}",
                    serde_json::to_string(&failure).unwrap()
                ),
            )
            .unwrap();
            let mut d = dashboard();
            super::super::tests::press_setting(&mut d, 'S');
            let mut settings = d.settings.clone();
            settings.ready_sound_choice = Some(ReadySoundChoice::Chime);
            d.handle_server_message(super::super::tests::settings_reading(settings.clone()));
            install_fake_bridge(&mut d, client.clone());
            deliver(&mut d, snapshot(2, "ready", AgentActivity::ResponseReady));
            wait_bridge(&mut d, |d| {
                d.desktop.pending.is_empty()
                    && d.desktop.in_flight.is_none()
                    && d.desktop.sound_unavailable == Some((ReadySoundChoice::Chime, failure))
            });
            assert_eq!(d.desktop.bridge.status, Some(BridgeStatus::Available));
            assert_eq!(
                d.desktop.sound_unavailable,
                Some((ReadySoundChoice::Chime, failure))
            );
            assert_eq!(d.settings, settings);
            let notice = d.desktop_status_notice().unwrap();
            assert!(
                notice.contains("banner submitted silently")
                    && notice.contains("choose System default in Settings")
            );
            assert_eq!(
                notice.contains("Sound name conflict"),
                failure == BridgeSoundFailure::LookupConflict
            );
            assert_eq!(
                notice.contains("Reinstall OVRCR Bridge"),
                failure != BridgeSoundFailure::LookupConflict
            );
            d.key(KeyCode::Char('?'));
            assert_eq!(
                d.desktop_status_notice(),
                Some(notice),
                "warning persists after a key clears transient notices"
            );
            d.key(KeyCode::Esc);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
            d.draw(&mut terminal).unwrap();
            let footer: String = (0..120)
                .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
                .collect();
            assert!(
                footer.contains(notice),
                "actionable sound warning fits the rendered footer: {footer}"
            );
            deliver(&mut d, snapshot(3, "ready", AgentActivity::ResponseReady));
            d.emit_desktop_notifications();
            assert_eq!(
                bridge_requests(&client).len(),
                2,
                "no delivery retry or fallback"
            );
            std::fs::remove_file(client.with_extension("sound_failure")).unwrap();
            deliver(
                &mut d,
                snapshot_with_requests(
                    4,
                    "input",
                    AgentActivity::Busy,
                    &[request("question", InputKind::Input)],
                ),
            );
            wait_bridge(&mut d, |d| {
                d.desktop.pending.is_empty()
                    && d.desktop.in_flight.is_none()
                    && d.desktop.sound_unavailable.is_none()
            });
            assert_eq!(bridge_requests(&client).len(), 3);
            assert!(
                d.desktop.sound_unavailable.is_none(),
                "a later successful sound request clears the warning without replay"
            );
            assert!(d.desktop_status_notice().is_none());
            assert_eq!(d.settings, settings);
        }
    }

    #[cfg(target_os = "macos")]
    fn sound_settings_event(settings: Settings) -> ServerMessage {
        let invalid = settings.ready_sound_choice.is_none();
        let mut event = super::super::tests::settings_reading(settings);
        if invalid && let ServerMessage::Event(ServerEvent::SettingsChanged(report)) = &mut event {
            report.findings.push(ovrcr_protocol::SettingsFinding {
                key: Some("ready_sound_choice".into()),
                message: "unrecognized saved sound choice".into(),
                line: Some(3),
            });
        }
        event
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_invalid_sound_guidance_is_retired_on_valid_settings_behind_an_error() {
        let guidance = "Ready sound choice invalid; banners are silent. Choose a sound in Settings";
        let mut d = Dashboard::new(TerminalSize {
            rows: 24,
            cols: 120,
        });
        let settings = Settings {
            desktop_notifications: true,
            ready_sound: true,
            ready_sound_choice: Some(ReadySoundChoice::Tap),
            ..Default::default()
        };
        d.handle_server_message(sound_settings_event(settings.clone()));
        d.set_error("Unrelated fixture error");
        d.show_notice("Unrelated fixture notice A");
        d.handle_server_message(sound_settings_event(Settings {
            ready_sound_choice: None,
            ..settings.clone()
        }));
        d.show_notice("Unrelated fixture notice B");
        assert_eq!(
            d.notice_queue
                .iter()
                .filter(|notice| notice.as_str() == guidance)
                .count(),
            2,
            "both changed-choice and finding producers queue guidance behind the error"
        );
        d.handle_server_message(sound_settings_event(Settings {
            ready_sound_choice: Some(ReadySoundChoice::Chime),
            ..settings
        }));
        assert_eq!(d.error.as_deref(), Some("Unrelated fixture error"));
        assert!(d.desktop.notice.is_none());
        assert!(
            d.notice_queue.iter().all(|notice| notice != guidance),
            "fresh valid choice retires every queued copy of obsolete guidance"
        );
        assert_eq!(
            d.notice_queue
                .iter()
                .take(2)
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["Unrelated fixture notice A", "Unrelated fixture notice B"]
        );
        assert_eq!(d.settings.ready_sound_choice, Some(ReadySoundChoice::Chime));
        assert!(d.settings.desktop_notifications && d.settings.ready_sound);
        assert!(d.settings_report.as_ref().unwrap().findings.is_empty());
        d.dismiss_error_banner();
        assert_eq!(
            d.desktop.notice.as_deref(),
            Some("Unrelated fixture notice A")
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
        d.draw(&mut terminal).unwrap();
        let footer: String = (0..120)
            .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
            .collect();
        assert!(footer.contains("Unrelated fixture notice A") && !footer.contains(guidance));
        d.key(KeyCode::Null);
        assert_eq!(
            d.desktop.notice.as_deref(),
            Some("Unrelated fixture notice B")
        );
        while d.desktop.notice.is_some() {
            assert_ne!(d.desktop.notice.as_deref(), Some(guidance));
            d.key(KeyCode::Null);
        }
        assert!(d.notice_queue.is_empty());
        assert!(d.desktop_status_notice().is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_invalid_sound_guidance_is_retired_on_opt_out_without_losing_findings() {
        let guidance = "Ready sound choice invalid; banners are silent. Choose a sound in Settings";
        for desktop_off in [true, false] {
            for was_invalid in [true, false] {
                let mut d = Dashboard::new(TerminalSize {
                    rows: 24,
                    cols: 120,
                });
                let settings = Settings {
                    desktop_notifications: true,
                    ready_sound: true,
                    ready_sound_choice: Some(ReadySoundChoice::Tap),
                    ..Default::default()
                };
                d.handle_server_message(sound_settings_event(settings.clone()));
                d.set_error("Unrelated opt-out error");
                d.show_notice("Unrelated fixture notice A");
                if was_invalid {
                    d.handle_server_message(sound_settings_event(Settings {
                        ready_sound_choice: None,
                        ..settings.clone()
                    }));
                    assert!(d.notice_queue.iter().any(|notice| notice == guidance));
                }
                d.show_notice("Unrelated fixture notice B");
                let fresh = Settings {
                    desktop_notifications: !desktop_off,
                    ready_sound: desktop_off,
                    ready_sound_choice: None,
                    ..settings
                };
                d.handle_server_message(sound_settings_event(fresh.clone()));
                assert_eq!(d.error.as_deref(), Some("Unrelated opt-out error"));
                assert!(
                    d.notice_queue.iter().all(|notice| notice != guidance),
                    "N-off={desktop_off}, already invalid={was_invalid}: opt-out cannot retain or create obsolete sound guidance"
                );
                assert_eq!(
                    d.notice_queue
                        .iter()
                        .take(2)
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    ["Unrelated fixture notice A", "Unrelated fixture notice B"]
                );
                assert_eq!(
                    d.settings, fresh,
                    "retiring guidance does not repair invalid settings or change consent"
                );
                assert_eq!(d.settings_report.as_ref().unwrap().findings.len(), 1);
                d.dismiss_error_banner();
                assert_eq!(
                    d.desktop.notice.as_deref(),
                    Some("Unrelated fixture notice A")
                );
                d.key(KeyCode::Null);
                assert_eq!(
                    d.desktop.notice.as_deref(),
                    Some("Unrelated fixture notice B")
                );
                while d.desktop.notice.is_some() {
                    assert_ne!(d.desktop.notice.as_deref(), Some(guidance));
                    d.key(KeyCode::Null);
                }
                assert_ne!(d.desktop_status_notice(), Some(guidance));
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_invalid_sound_guidance_retires_current_and_queued_copies_only() {
        let guidance = "Ready sound choice invalid; banners are silent. Choose a sound in Settings";
        for current_is_guidance in [true, false] {
            let mut d = Dashboard::new(TerminalSize {
                rows: 24,
                cols: 120,
            });
            let settings = Settings {
                desktop_notifications: true,
                ready_sound: true,
                ready_sound_choice: Some(ReadySoundChoice::Rise),
                ..Default::default()
            };
            d.handle_server_message(sound_settings_event(settings.clone()));
            d.desktop.notice = Some(
                if current_is_guidance {
                    guidance
                } else {
                    "Unrelated fixture notice A"
                }
                .into(),
            );
            if current_is_guidance {
                d.notice_queue
                    .push_back("Unrelated fixture notice A".into());
            }
            d.notice_queue.push_back(guidance.into());
            d.notice_queue
                .push_back("Unrelated fixture notice B".into());
            d.notice_queue.push_back(guidance.into());
            d.handle_server_message(sound_settings_event(settings.clone()));
            assert_eq!(
                d.desktop.notice.as_deref(),
                Some("Unrelated fixture notice A"),
                "current exact guidance retires, current unrelated notice survives"
            );
            assert_eq!(
                d.notice_queue
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                ["Unrelated fixture notice B"],
                "only exact obsolete guidance is removed from the queue"
            );
            assert_eq!(d.settings, settings);
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
            d.draw(&mut terminal).unwrap();
            let footer: String = (0..120)
                .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
                .collect();
            assert!(footer.contains("Unrelated fixture notice A") && !footer.contains(guidance));
            d.key(KeyCode::Null);
            assert_eq!(
                d.desktop.notice.as_deref(),
                Some("Unrelated fixture notice B")
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_outbound_banners_use_only_manual_or_original_identity() {
        let (_root, client) = fake_bridge("available");
        let mut d = dashboard();
        install_fake_bridge(&mut d, client.clone());
        for (revision, manual, subtitle) in [
            (2, None, "session"),
            (4, Some("manual\n\u{202e}界\u{2028}title"), "manual界title"),
            (6, None, "session"),
        ] {
            let mut hierarchy = snapshot(
                revision,
                &format!("turn-{revision}"),
                AgentActivity::ResponseReady,
            );
            let summary = session(&mut hierarchy);
            summary.title = Some("PRIVATE_GENERATED_CONVERSATION_SUBJECT".into());
            summary.manual_title = manual.map(str::to_string);
            summary.project = "project\u{2066}\u{2069}".into();
            summary.workspace = "workspace\u{2029}\u{061c}".into();
            deliver(&mut d, hierarchy);
            wait_bridge(&mut d, |d| {
                d.desktop.pending.is_empty() && d.desktop.in_flight.is_none()
            });
            let requests = bridge_requests(&client);
            let BridgeOperation::Deliver {
                title,
                subtitle: actual,
                body,
                navigation,
                ..
            } = &requests.last().unwrap().op
            else {
                panic!("expected banner");
            };
            assert_eq!(title, "OVRCR · response ready");
            assert!(navigation.validate());
            assert_eq!(navigation.session, SessionId(1));
            assert_eq!(actual, subtitle);
            assert_eq!(body, "project / workspace / session (#1)");
            let outbound = std::fs::read_to_string(client.with_extension("requests")).unwrap();
            for private in [
                "PRIVATE_GENERATED",
                "PRIVATE_LABEL",
                "PRIVATE_CONVERSATION",
                "PRIVATE_INVOCATION",
                "/private/path",
                "turn-",
            ] {
                assert!(!outbound.contains(private), "exported {private}");
            }
            assert!(
                requests
                    .iter()
                    .all(|request| request.schema == BRIDGE_SCHEMA_VERSION
                        && request.server_wire == PROTOCOL_VERSION)
            );
        }
        let mut hierarchy = snapshot_with_requests(
            8,
            "private-turn",
            AgentActivity::Busy,
            &[request("PRIVATE_REQUEST", InputKind::Select)],
        );
        let summary = session(&mut hierarchy);
        summary.manual_title = Some("🙂".repeat(100));
        summary.name = "界".repeat(100);
        summary.project = "🙂".repeat(100);
        summary.workspace = "🙂".repeat(100);
        deliver(&mut d, hierarchy);
        wait_bridge(&mut d, |d| {
            d.desktop.pending.is_empty() && d.desktop.in_flight.is_none()
        });
        let requests = bridge_requests(&client);
        let BridgeOperation::Deliver {
            title,
            subtitle,
            body,
            navigation,
            ..
        } = &requests.last().unwrap().op
        else {
            panic!("expected Input banner");
        };
        assert_eq!(title, "OVRCR · input needed");
        assert!(navigation.validate());
        assert_eq!(navigation.session, SessionId(1));
        assert_eq!(subtitle.chars().count(), 80);
        assert!(subtitle.len() <= 320 && body.len() <= 1024 && title.len() <= 96);
        assert!(body.ends_with(" (#1)"));
        assert!(
            !std::fs::read_to_string(client.with_extension("requests"))
                .unwrap()
                .contains("PRIVATE_REQUEST")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_recovery_notices_wait_for_errors_and_completed_checks_do_not_linger() {
        let (_root, client) = fake_bridge("denied");
        let mut d = dashboard();
        install_fake_bridge(&mut d, client.clone());
        wait_bridge(&mut d, |d| {
            d.desktop.bridge.status == Some(BridgeStatus::Denied)
        });
        assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
        d.set_error("Fixture startup error");
        d.show_notice("Unrelated fixture notice");
        wait_bridge(&mut d, |d| {
            d.desktop.bridge.check_after_settings && d.desktop.bridge.in_flight.is_none()
        });
        let guidance =
            "System Settings opened; choose Notifications → OVRCR, then Browse O checks permission";
        assert_eq!(d.error.as_deref(), Some("Fixture startup error"));
        assert!(d.desktop.notice.is_none());
        assert!(d.notice_queue.iter().any(|notice| notice == guidance));
        d.dismiss_error_banner();
        assert_eq!(
            d.desktop.notice.as_deref(),
            Some("Open System Settings, then Notifications → OVRCR")
        );

        std::fs::write(client.with_extension("status"), "available").unwrap();
        assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
        d.set_error("Fixture check error");
        wait_bridge(&mut d, |d| {
            d.desktop.bridge.status == Some(BridgeStatus::Available)
                && d.desktop.bridge.in_flight.is_none()
        });
        assert_eq!(d.error.as_deref(), Some("Fixture check error"));
        assert!(
            d.notice_queue
                .iter()
                .all(|notice| notice != "Checking OVRCR notification permission")
        );
        d.dismiss_error_banner();
        assert_eq!(
            d.desktop.notice.as_deref(),
            Some("Unrelated fixture notice")
        );
        d.key(KeyCode::Char('?'));
        assert!(d.desktop.notice.is_none());
        assert!(d.desktop_status_notice().is_none());
        assert!(!d.notification_recovery_available());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_denial_has_persistent_recovery_and_settings_only_runs_explicitly() {
        let (_root, client) = fake_bridge("denied");
        let mut d = dashboard();
        install_fake_bridge(&mut d, client.clone());
        wait_bridge(&mut d, |d| {
            d.desktop.bridge.status == Some(BridgeStatus::Denied)
        });
        assert_eq!(
            bridge_requests(&client)
                .iter()
                .map(|r| &r.op)
                .collect::<Vec<_>>(),
            [&BridgeOperation::Status]
        );
        assert!(
            d.desktop_status_notice()
                .unwrap()
                .contains("O: System Settings → Notifications → OVRCR")
        );
        d.key(KeyCode::Char('?'));
        assert!(d.desktop.notice.is_none());
        assert!(
            d.desktop_status_notice().is_some(),
            "typing cannot hide denial"
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 24)).unwrap();
        terminal
            .draw(|frame| super::super::render::draw_dashboard_at(frame, &d, 0))
            .unwrap();
        let footer: String = (0..120)
            .map(|column| terminal.backend().buffer()[(column, 23)].symbol())
            .collect();
        assert!(footer.contains("notifications denied") && footer.contains("O: System Settings"));
        d.key(KeyCode::Esc);
        assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
        assert!(d.settings.desktop_notifications);
        wait_bridge(&mut d, |d| {
            d.desktop.notice.as_deref()
                == Some(
                    "System Settings opened; choose Notifications → OVRCR, then Browse O checks permission",
                )
        });
        assert_eq!(
            bridge_requests(&client).last().unwrap().op,
            BridgeOperation::Settings
        );
        assert!(d.notification_recovery_available());
        assert!(d.notification_permission_unconfirmed());
        assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
        wait_bridge(&mut d, |d| {
            !d.desktop.bridge.check_after_settings && d.desktop.bridge.in_flight.is_none()
        });
        assert_eq!(
            bridge_requests(&client)
                .iter()
                .map(|r| &r.op)
                .collect::<Vec<_>>(),
            [
                &BridgeOperation::Status,
                &BridgeOperation::Settings,
                &BridgeOperation::Status
            ]
        );
        std::fs::write(client.with_extension("settings"), "failed").unwrap();
        assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
        wait_bridge(&mut d, |d| {
            d.desktop.notice.as_deref()
                == Some(
                    "Open System Settings manually: Notifications → OVRCR; then Browse O checks permission",
                )
        });
        assert_eq!(bridge_requests(&client).len(), 4);
        assert!(d.notification_permission_unconfirmed());
        std::fs::write(client.with_extension("status"), "available").unwrap();
        assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
        wait_bridge(&mut d, |d| {
            d.desktop.bridge.status == Some(BridgeStatus::Available)
                && d.desktop.bridge.in_flight.is_none()
        });
        assert!(d.desktop_status_notice().is_none());
        assert!(d.desktop.notice.is_none());
        assert_eq!(
            bridge_requests(&client)
                .iter()
                .map(|r| &r.op)
                .collect::<Vec<_>>(),
            [
                &BridgeOperation::Status,
                &BridgeOperation::Settings,
                &BridgeOperation::Status,
                &BridgeOperation::Settings,
                &BridgeOperation::Status
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_pending_polls_are_bounded_and_never_repeat_authorization() {
        let (_root, client) = fake_bridge("permission_pending");
        let mut d = dashboard();
        install_fake_bridge(&mut d, client.clone());
        wait_bridge(&mut d, |d| {
            d.desktop.bridge.status == Some(BridgeStatus::PermissionPending)
        });
        assert!(d.desktop_status_notice().unwrap().contains("pending"));
        d.desktop.bridge.next_poll = Some(Instant::now());
        wait_bridge(&mut d, |_| bridge_requests(&client).len() == 2);
        wait_bridge(&mut d, |d| d.desktop.bridge.in_flight.is_none());
        d.desktop.bridge.poll_until = Some(Instant::now() - Duration::from_secs(1));
        d.desktop.bridge.next_poll = Some(Instant::now() - Duration::from_secs(1));
        for _ in 0..5 {
            d.emit_desktop_notifications();
        }
        assert_eq!(
            bridge_requests(&client)
                .iter()
                .map(|r| &r.op)
                .collect::<Vec<_>>(),
            [&BridgeOperation::Status, &BridgeOperation::Status]
        );
        std::fs::write(client.with_extension("status"), "not_determined").unwrap();
        assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
        wait_bridge(&mut d, |d| {
            d.desktop.bridge.status == Some(BridgeStatus::NotDetermined)
                && d.desktop.bridge.in_flight.is_none()
        });
        assert_eq!(
            bridge_requests(&client)
                .iter()
                .map(|r| &r.op)
                .collect::<Vec<_>>(),
            [
                &BridgeOperation::Status,
                &BridgeOperation::Status,
                &BridgeOperation::Status
            ]
        );
        assert!(d.notification_permission_unconfirmed());
        assert_eq!(d.key(KeyCode::Char('q')), DashboardAction::Detach);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_expired_permission_recovers_only_through_an_explicit_status_check() {
        use crossterm::event::{KeyEvent, KeyModifiers};
        for (reply, status) in [
            ("available", BridgeStatus::Available),
            ("denied", BridgeStatus::Denied),
            ("not_determined", BridgeStatus::NotDetermined),
            ("permission_pending", BridgeStatus::PermissionPending),
        ] {
            let (_root, client) = fake_bridge("not_determined");
            std::fs::write(client.with_extension("authorize"), "permission_pending").unwrap();
            let mut d = dashboard();
            install_fake_bridge(&mut d, client.clone());
            wait_bridge(&mut d, |d| {
                d.desktop.bridge.status == Some(BridgeStatus::PermissionPending)
                    && d.desktop.bridge.in_flight.is_none()
            });
            assert_eq!(
                bridge_requests(&client)
                    .iter()
                    .map(|r| &r.op)
                    .collect::<Vec<_>>(),
                [&BridgeOperation::Status, &BridgeOperation::Authorize]
            );
            d.desktop.bridge.poll_until = Some(Instant::now() - Duration::from_secs(1));
            d.desktop.bridge.next_poll = Some(Instant::now() - Duration::from_secs(1));
            for _ in 0..5 {
                d.emit_desktop_notifications();
            }
            assert_eq!(
                bridge_requests(&client).len(),
                2,
                "expiration cannot schedule another operation"
            );
            assert_eq!(
                d.desktop_status_notice(),
                Some("OVRCR notification permission unconfirmed; O: check permission")
            );
            let binding = d
                .key_binding_for(KeyEvent::new(KeyCode::Char('O'), KeyModifiers::NONE))
                .unwrap();
            assert!(binding.enabled());
            assert_eq!(binding.name, "Check notification permission");
            d.key(KeyCode::Char(':'));
            d.palette_paste("Check notification permission");
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
            terminal
                .draw(|frame| super::super::render::draw_dashboard_at(frame, &d, 0))
                .unwrap();
            let text = (0..40)
                .map(|row| {
                    (0..120)
                        .map(|column| terminal.backend().buffer()[(column, row)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                text.contains("Check notification permission"),
                "the palette must name the actual O action"
            );
            d.key(KeyCode::Esc);
            std::fs::write(client.with_extension("status"), reply).unwrap();
            assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
            assert_eq!(
                d.desktop.notice.as_deref(),
                Some("Checking OVRCR notification permission")
            );
            wait_bridge(&mut d, |d| {
                d.desktop.bridge.status == Some(status) && d.desktop.bridge.in_flight.is_none()
            });
            assert_eq!(
                bridge_requests(&client)
                    .iter()
                    .map(|r| &r.op)
                    .collect::<Vec<_>>(),
                [
                    &BridgeOperation::Status,
                    &BridgeOperation::Authorize,
                    &BridgeOperation::Status
                ]
            );
            assert!(
                d.desktop.notice.is_none(),
                "a completed check cannot leave its checking notice"
            );
            let binding = d
                .key_binding_for(KeyEvent::new(KeyCode::Char('O'), KeyModifiers::NONE))
                .unwrap();
            match status {
                BridgeStatus::Available => {
                    assert!(d.desktop_status_notice().is_none());
                    assert!(!binding.enabled());
                }
                BridgeStatus::Denied => {
                    assert_eq!(binding.name, "Open notification settings");
                    assert!(binding.enabled());
                    d.key(KeyCode::Char(':'));
                    d.palette_paste("Open notification settings");
                    terminal
                        .draw(|frame| super::super::render::draw_dashboard_at(frame, &d, 0))
                        .unwrap();
                    let text = (0..40)
                        .map(|row| {
                            (0..120)
                                .map(|column| terminal.backend().buffer()[(column, row)].symbol())
                                .collect::<String>()
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    assert!(text.contains("Open notification settings"));
                    d.key(KeyCode::Esc);
                    assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
                    wait_bridge(&mut d, |d| {
                        d.desktop.bridge.in_flight.is_none() && d.desktop.bridge.pending.is_none()
                    });
                    assert_eq!(
                        bridge_requests(&client).last().unwrap().op,
                        BridgeOperation::Settings
                    );
                    assert_eq!(bridge_requests(&client).len(), 4);
                    assert!(d.notification_permission_unconfirmed());
                    std::fs::write(client.with_extension("status"), "permission_pending").unwrap();
                    assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
                    wait_bridge(&mut d, |d| {
                        d.desktop.bridge.status == Some(BridgeStatus::PermissionPending)
                            && d.desktop.bridge.in_flight.is_none()
                    });
                    assert!(d.notification_permission_unconfirmed());
                    for _ in 0..5 {
                        d.emit_desktop_notifications();
                    }
                    assert_eq!(
                        bridge_requests(&client).len(),
                        5,
                        "checking after Settings must not begin automatic polling"
                    );
                    std::fs::write(client.with_extension("status"), "available").unwrap();
                    assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::Redraw);
                    wait_bridge(&mut d, |d| {
                        d.desktop.bridge.status == Some(BridgeStatus::Available)
                            && d.desktop.bridge.in_flight.is_none()
                    });
                    assert!(d.desktop_status_notice().is_none());
                    assert!(d.desktop.notice.is_none());
                    assert_eq!(
                        bridge_requests(&client)
                            .iter()
                            .map(|r| &r.op)
                            .collect::<Vec<_>>(),
                        [
                            &BridgeOperation::Status,
                            &BridgeOperation::Authorize,
                            &BridgeOperation::Status,
                            &BridgeOperation::Settings,
                            &BridgeOperation::Status,
                            &BridgeOperation::Status
                        ]
                    );
                }
                _ => {
                    assert_eq!(binding.name, "Check notification permission");
                    assert!(binding.enabled());
                    assert_eq!(
                        d.desktop_status_notice(),
                        Some("OVRCR notification permission unconfirmed; O: check permission")
                    );
                    for _ in 0..5 {
                        d.emit_desktop_notifications();
                    }
                    assert_eq!(
                        bridge_requests(&client).len(),
                        3,
                        "a check never restarts polling or authorization"
                    );
                }
            }
            let before = bridge_requests(&client).len();
            let mut settings = d.settings.clone();
            settings.desktop_notifications = false;
            d.handle_server_message(super::super::tests::settings_reading(settings));
            assert_eq!(d.key(KeyCode::Char('O')), DashboardAction::None);
            for _ in 0..5 {
                d.emit_desktop_notifications();
            }
            assert_eq!(
                bridge_requests(&client).len(),
                before,
                "N off permits no recovery controls"
            );
            assert_eq!(
                bridge_requests(&client)
                    .iter()
                    .filter(|r| r.op == BridgeOperation::Authorize)
                    .count(),
                1
            );
            assert!(d.desktop.pending.is_empty());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_invalid_missing_and_incompatible_clients_have_fixed_guidance() {
        use std::os::unix::fs::PermissionsExt;
        for (reply, expected) in [
            ("PRIVATE_HOST_ERROR".to_string(), BridgeStatus::Failed),
            (
                format!(
                    r#"{{"schema":{},"server_wire":{},"status":"available"}}"#,
                    BRIDGE_SCHEMA_VERSION + 1,
                    PROTOCOL_VERSION
                ),
                BridgeStatus::Incompatible,
            ),
            (
                format!(
                    r#"{{"schema":{},"server_wire":{},"status":"available"}}"#,
                    BRIDGE_SCHEMA_VERSION,
                    PROTOCOL_VERSION - 1
                ),
                BridgeStatus::Incompatible,
            ),
            (
                format!(
                    r#"{{"schema":{},"server_wire":{},"status":"untyped PRIVATE_HOST_ERROR"}}"#,
                    BRIDGE_SCHEMA_VERSION, PROTOCOL_VERSION
                ),
                BridgeStatus::Failed,
            ),
            (
                format!(
                    r#"{{"schema":{},"server_wire":{},"status":"available","sound_unavailable":"missing_resource"}}"#,
                    BRIDGE_SCHEMA_VERSION, PROTOCOL_VERSION
                ),
                BridgeStatus::Failed,
            ),
            (
                format!(
                    r#"{{"schema":{},"server_wire":{},"status":"submitted","sound_unavailable":"PRIVATE_HOST_ERROR"}}"#,
                    BRIDGE_SCHEMA_VERSION, PROTOCOL_VERSION
                ),
                BridgeStatus::Failed,
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let client = root.path().join("client");
            std::fs::write(
                &client,
                format!("#!/bin/sh\ncat >/dev/null\nprintf '%s' '{}'\n", reply),
            )
            .unwrap();
            std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(
                run_bridge(Some(&client), &BridgeOperation::Status, || false).status,
                expected
            );
            let mut d = dashboard();
            install_fake_bridge(&mut d, client);
            wait_bridge(&mut d, |d| d.desktop.bridge.status == Some(expected));
            let notice = d.desktop_status_notice().unwrap();
            assert!(notice.contains("restart OVRCR to review the Bridge"));
            assert!(!notice.contains("PRIVATE_HOST_ERROR"));
            assert!(!d.notification_recovery_available());
            assert_eq!(d.recover_notification_permission(), DashboardAction::None);
        }
        let root = tempfile::tempdir().unwrap();
        let mut d = dashboard();
        install_fake_bridge(&mut d, root.path().join("missing"));
        wait_bridge(&mut d, |d| {
            d.desktop.bridge.status == Some(BridgeStatus::Failed)
        });
        assert_eq!(
            d.desktop_status_notice(),
            Some("OVRCR Bridge not installed; restart OVRCR to review the Bridge install offer")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn incompatible_bridge_banner_names_the_stale_app_or_the_stale_process() {
        use std::os::unix::fs::PermissionsExt;
        // The client answers an older wire. `--check-contract` reports whether
        // the installed app itself matches (exit 0) or not (exit 78).
        for (contract_exit, expected) in [
            (
                0,
                "Running OVRCR Bridge is an older build; restart OVRCR to review the Bridge restart offer",
            ),
            (
                78,
                "OVRCR Bridge needs updating; restart OVRCR to review the Bridge repair offer",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let client = root.path().join("client");
            let checks = root.path().join("checks");
            std::fs::write(
                &client,
                format!(
                    "#!/bin/sh\nif [ \"$1\" = --check-contract ]; then printf '%s %s\\n' \"$2\" \"$3\" >> '{}'; exit {contract_exit}; fi\ncat >/dev/null\nprintf '{{\"schema\":{},\"server_wire\":{},\"status\":\"available\"}}'\n",
                    checks.display(),
                    BRIDGE_SCHEMA_VERSION,
                    PROTOCOL_VERSION - 1
                ),
            )
            .unwrap();
            std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).unwrap();
            let mut d = dashboard();
            install_fake_bridge(&mut d, client);
            wait_bridge(&mut d, |d| {
                d.desktop.bridge.status == Some(BridgeStatus::Incompatible)
            });
            assert_eq!(d.desktop_status_notice(), Some(expected));
            assert_eq!(
                std::fs::read_to_string(&checks).unwrap(),
                format!("{BRIDGE_SCHEMA_VERSION} {PROTOCOL_VERSION}\n")
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_reaped_client_failures_do_not_signal_its_former_process_group() {
        use std::os::unix::{
            ffi::OsStrExt,
            fs::{OpenOptionsExt, PermissionsExt},
            process::ExitStatusExt,
        };
        let mut outcomes = Vec::new();
        for (case, reply, exit) in [
            (
                "invalid sound status",
                format!(
                    r#"{{"schema":{},"server_wire":{},"status":"available","sound_unavailable":"missing_resource"}}"#,
                    BRIDGE_SCHEMA_VERSION, PROTOCOL_VERSION
                ),
                "0",
            ),
            ("invalid JSON", "not JSON".into(), "0"),
            (
                "nonzero exit",
                format!(
                    r#"{{"schema":{},"server_wire":{},"status":"available"}}"#,
                    BRIDGE_SCHEMA_VERSION, PROTOCOL_VERSION
                ),
                "17",
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            let client = root.path().join("client");
            let witness_script = root.path().join("witness");
            let ready_path = client.with_extension("ready");
            let c_ready = std::ffi::CString::new(ready_path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(c_ready.as_ptr(), 0o600) }, 0);
            // Open both ends before the worker starts. The parent never waits
            // for a FIFO reader, including when the client times out.
            let mut ready = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&ready_path)
                .unwrap();
            std::fs::write(
                &client,
                r#"#!/bin/sh
set -eu
request=
IFS= read -r request || [ -n "$request" ]
printf '%s' "$request" > "$0.request"
printf '%s' "$$" > "$0.pid.tmp"
/bin/mv "$0.pid.tmp" "$0.pid"
IFS= read -r ready < "$0.ready"
[ "$ready" = ready ] || exit 99
IFS= read -r reply < "$0.reply"
IFS= read -r exit_code < "$0.exit"
printf '%s' "$reply"
printf '%s' "$exit_code" > "$0.exiting"
exit "$exit_code"
"#,
            )
            .unwrap();
            std::fs::write(
                &witness_script,
                r#"#!/bin/sh
set -eu
while IFS= read -r line; do
  :
done
printf 'survived' > "$1.survived"
"#,
            )
            .unwrap();
            for script in [&client, &witness_script] {
                std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            std::fs::write(client.with_extension("reply"), format!("{reply}\n")).unwrap();
            std::fs::write(client.with_extension("exit"), format!("{exit}\n")).unwrap();
            let (reply, status, survived) = thread::scope(|scope| {
                let started = Instant::now();
                let call =
                    scope.spawn(|| run_bridge(Some(&client), &BridgeOperation::Status, || false));
                let until = Instant::now() + HOST_TIMEOUT;
                while !client.with_extension("pid").exists() {
                    assert!(
                        Instant::now() < until,
                        "{case}: client never reached its gate"
                    );
                    thread::park_timeout(HOST_POLL);
                }
                let group: i32 = std::fs::read_to_string(client.with_extension("pid"))
                    .unwrap()
                    .parse()
                    .unwrap();
                // This child is directly owned and reaped by the test. The gate
                // keeps the client alive until its group has the ready witness.
                let mut witness = Command::new(&witness_script)
                    .arg(&client)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .process_group(group)
                    .spawn()
                    .unwrap();
                let witness_group = unsafe { libc::getpgid(witness.id() as libc::pid_t) };
                let gate_release = ready.write_all(b"ready\n");
                let reply = call.join();
                let elapsed = started.elapsed();
                let exiting = std::fs::read_to_string(client.with_extension("exiting")).ok();
                // EOF releases the owned witness without a blocking write or
                // another process. Its stdin is held until the real call ends.
                drop(witness.stdin.take());
                let until = Instant::now() + HOST_TIMEOUT;
                let status = loop {
                    match witness.try_wait() {
                        Ok(Some(status)) => break status,
                        Ok(None) if Instant::now() < until => thread::park_timeout(HOST_POLL),
                        _ => {
                            // Only the still-owned witness needs cleanup on an
                            // unexpected fixture failure; never signal a reaped PID.
                            let _ = witness.kill();
                            break witness.wait().unwrap();
                        }
                    }
                };
                let survived = client.with_extension("survived").exists();
                eprintln!(
                    "{case}: client PG {group}, witness PID {}, witness PG {witness_group}, client call {elapsed:?}, exit marker {exiting:?}, gate released={}, witness status {status}, survived={survived}",
                    witness.id(),
                    gate_release.is_ok()
                );
                // Check admission after cleanup so a failed gate cannot leave
                // a directly owned witness running during assertion unwind.
                assert_eq!(
                    witness_group, group,
                    "{case}: witness did not join client group"
                );
                gate_release.unwrap();
                assert_eq!(
                    exiting.as_deref(),
                    Some(exit),
                    "{case}: client did not reach its expected exit before the real call ended"
                );
                (reply.unwrap(), status, survived)
            });
            outcomes.push((
                case,
                reply.status,
                reply.sound_unavailable,
                status.code(),
                status.signal(),
                survived,
            ));
        }
        assert_eq!(
            outcomes,
            ["invalid sound status", "invalid JSON", "nonzero exit"].map(|case| (
                case,
                BridgeStatus::Failed,
                None,
                Some(0),
                None,
                true
            )),
            "a rejected, already-reaped client must not signal the owned witness in its former group"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn bridge_client_output_and_drop_are_bounded_without_stopping_persistent_app() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let client = root.path().join("client");
        std::fs::write(&client, "#!/bin/sh\ncat > \"$0.request\"\n/usr/bin/yes x\n").unwrap();
        std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        assert_eq!(
            run_bridge(Some(&client), &BridgeOperation::Status, || false).status,
            BridgeStatus::Failed
        );
        assert!(started.elapsed() < HOST_TIMEOUT + Duration::from_secs(1));
        std::fs::write(&client, "#!/bin/sh\ncat > \"$0.request\"\nsleep 30\n").unwrap();
        std::fs::remove_file(client.with_extension("request")).unwrap();
        let mut persistent = Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .unwrap();
        let mut d = dashboard();
        install_fake_bridge(&mut d, client.clone());
        let started = Instant::now();
        d.emit_desktop_notifications();
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "Dashboard blocked on the host"
        );
        let until = Instant::now() + Duration::from_secs(1);
        while !client.with_extension("request").exists() {
            assert!(Instant::now() < until);
            thread::park_timeout(HOST_POLL);
        }
        assert_eq!(d.key(KeyCode::Char('q')), DashboardAction::Detach);
        let started = Instant::now();
        drop(d);
        let detach_elapsed = started.elapsed();
        let alive = persistent.try_wait().unwrap().is_none();
        unsafe {
            libc::kill(-(persistent.id() as libc::pid_t), libc::SIGKILL);
        }
        persistent.wait().unwrap();
        assert!(
            detach_elapsed < Duration::from_secs(1),
            "detach waited for client timeout"
        );
        assert!(
            alive,
            "Dashboard teardown stopped the separately owned persistent host"
        );
    }

    #[test]
    fn ready_sound_toggle_does_not_intercept_native_input() {
        let mut d = dashboard();
        super::super::tests::press_setting(&mut d, 'S');
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

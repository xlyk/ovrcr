//! Notification clicks use the existing active owner and outbound writer.
//! There is one bounded pending click per owner; replacing the owner drops it.
use super::*;
use ovrcr_protocol::{
    BRIDGE_SCHEMA_VERSION, BridgeActivationTarget, BridgeContext, BridgeNavigationOffer,
    BridgeNavigationResult, BridgeNavigationTicket,
};

const NAVIGATION_BUDGET: Duration = Duration::from_millis(1200);

/// Byte compatibility for a reviewed CLI. Hashing admits regular same-user
/// executable files only and rejects modifications observed during the read.
pub fn bridge_executable_sha256(path: &Path, deadline: Instant) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let before = file.metadata()?;
    if !before.is_file()
        || before.uid() != unsafe { libc::geteuid() }
        || before.mode() & 0o111 == 0
        || before.len() == 0
        || before.len() > 512 * 1024 * 1024
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut digest = Sha256::new();
    let mut chunk = [0; 64 * 1024];
    let mut count = 0_u64;
    loop {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        count += read as u64;
        if count > before.len() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        digest.update(&chunk[..read]);
    }
    let after = file.metadata()?;
    let named = fs::symlink_metadata(path)?;
    if count != before.len()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
        || named.dev() != before.dev()
        || named.ino() != before.ino()
        || named.file_type().is_symlink()
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(super) fn new_identity() -> Result<String> {
    use std::fmt::Write;
    let mut bytes = generate_hook_capability()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut value = String::with_capacity(36);
    for (index, byte) in bytes[..16].iter().enumerate() {
        if [4, 6, 8, 10].contains(&index) {
            value.push('-');
        }
        write!(&mut value, "{byte:02x}").expect("write UUID string");
    }
    Ok(value)
}

pub(super) struct PendingNavigation {
    offer: BridgeNavigationOffer,
    deadline: Instant,
    confirmed: bool,
    result: SyncSender<BridgeNavigationResult>,
    callback: Option<UnixStream>,
}

impl ServerState {
    pub(super) fn bridge_context(&self) -> Option<BridgeContext> {
        Some(BridgeContext {
            server_socket: self.socket.to_str()?.to_owned(),
            callback_executable: std::env::current_exe().ok()?.to_str()?.to_owned(),
            callback_executable_sha256: self.callback_executable_sha256.clone()?,
            server_lifetime: self.server_lifetime.clone(),
        })
    }

    fn navigation_target_valid(&self, ticket: &BridgeNavigationTicket) -> bool {
        ticket.validate()
            && ticket.server_lifetime == self.server_lifetime
            && self.bridge_context().is_some_and(|context| {
                ticket.server_socket == context.server_socket
                    && ticket.callback_executable == context.callback_executable
                    && ticket.callback_executable_sha256 == context.callback_executable_sha256
            })
            && !self.shutdown.load(Ordering::Acquire)
            && !self.stopping.load(Ordering::Acquire)
            && self.retained.try_lock().is_some_and(|retained| {
                retained.get(ticket.session).is_some_and(|record| {
                    record.run == ticket.run
                        && record.disposition != crate::retained::Disposition::Archived
                })
            })
    }

    pub(super) fn navigate_notification(&self, ticket: BridgeNavigationTicket) -> Response {
        self.navigate_notification_from(ticket, None)
    }

    pub(super) fn navigate_notification_from(
        &self,
        ticket: BridgeNavigationTicket,
        callback: Option<UnixStream>,
    ) -> Response {
        let (result, received) = mpsc::sync_channel(1);
        let deadline = Instant::now() + NAVIGATION_BUDGET;
        {
            let Ok(_mutation) = self.mutation_lock.try_lock() else {
                return Response::NotificationNavigation(BridgeNavigationResult::ignored());
            };
            if !self.navigation_target_valid(&ticket) {
                return Response::NotificationNavigation(BridgeNavigationResult::ignored());
            }
            let Some(owner) = self.dashboard.snapshot().filter(|s| !s.sink.is_closing()) else {
                return Response::NotificationNavigation(BridgeNavigationResult::ignored());
            };
            let Ok(navigation) = new_identity() else {
                return Response::NotificationNavigation(BridgeNavigationResult::ignored());
            };
            let offer = BridgeNavigationOffer { navigation, ticket };
            let accepted = self
                .dashboard
                .with_owned(&owner.identity, |slot| {
                    if slot.sink.is_closing()
                        || slot
                            .navigation
                            .as_ref()
                            .is_some_and(|pending| pending.deadline > Instant::now())
                    {
                        return false;
                    }
                    slot.navigation = Some(PendingNavigation {
                        offer: offer.clone(),
                        deadline,
                        confirmed: false,
                        result,
                        callback,
                    });
                    true
                })
                .unwrap_or(false);
            if !accepted
                || !self.dashboard.send_owner(
                    &owner.identity,
                    ServerMessage::Event(ServerEvent::NotificationNavigation(offer)),
                )
            {
                return Response::NotificationNavigation(BridgeNavigationResult::ignored());
            }
        }
        Response::NotificationNavigation(
            received
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|_| BridgeNavigationResult::ignored()),
        )
    }

    /// Queue the confirmation while lifecycle mutations remain excluded. The
    /// sole owner writer orders it with later hierarchy/run changes. Application
    /// also compares its current hierarchy before changing any UI state.
    pub(super) fn confirm_notification_navigation(
        &self,
        owner: &Arc<()>,
        navigation: &str,
        request_id: u64,
    ) -> Response {
        let Ok(_mutation) = self.mutation_lock.try_lock() else {
            self.dashboard.send_owner(
                owner,
                response_message(request_id, Response::NotificationNavigationConfirmed(None)),
            );
            return Response::Ok;
        };
        if !self.dashboard.owns(owner) {
            return Response::Ok;
        }
        let offer = self
            .dashboard
            .with_owned(owner, |slot| {
                slot.navigation
                    .as_ref()
                    .filter(|pending| {
                        pending.offer.navigation == navigation
                            && !pending.confirmed
                            && pending.deadline > Instant::now()
                            && pending
                                .callback
                                .as_ref()
                                .is_none_or(|stream| !super::dashboard::peer_has_closed(stream))
                    })
                    .map(|pending| pending.offer.clone())
            })
            .flatten();
        let offer = offer.filter(|offer| self.navigation_target_valid(&offer.ticket));
        if offer.is_some() {
            self.dashboard.with_owned(owner, |slot| {
                if let Some(pending) = slot.navigation.as_mut() {
                    pending.confirmed = true;
                }
            });
        } else {
            self.dashboard.with_owned(owner, |slot| {
                if slot
                    .navigation
                    .as_ref()
                    .is_some_and(|p| p.offer.navigation == navigation)
                {
                    slot.navigation.take();
                }
            });
        }
        self.dashboard.send_owner(
            owner,
            response_message(
                request_id,
                Response::NotificationNavigationConfirmed(offer.map(Box::new)),
            ),
        );
        Response::Ok
    }

    pub(super) fn notification_navigation_applied(
        &self,
        owner: &Arc<()>,
        navigation: &str,
    ) -> Response {
        let Ok(_mutation) = self.mutation_lock.try_lock() else {
            return Response::Ok;
        };
        if !self.dashboard.owns(owner) {
            return Response::Ok;
        }
        let pending = self
            .dashboard
            .with_owned(owner, |slot| {
                if slot
                    .navigation
                    .as_ref()
                    .is_some_and(|pending| pending.offer.navigation == navigation)
                {
                    slot.navigation
                        .take()
                        .map(|pending| (pending, slot.bridge_identity.clone()))
                } else {
                    None
                }
            })
            .flatten();
        if let Some((pending, activation)) = pending {
            let valid = pending.confirmed
                && pending.deadline > Instant::now()
                && pending
                    .callback
                    .as_ref()
                    .is_none_or(|stream| !super::dashboard::peer_has_closed(stream))
                && self.navigation_target_valid(&pending.offer.ticket);
            let activation = activation.and_then(|_| self.bridge_activation_owned(owner));
            let result = if valid {
                BridgeNavigationResult {
                    schema: BRIDGE_SCHEMA_VERSION,
                    server_wire: PROTOCOL_VERSION,
                    applied: true,
                    activation,
                }
            } else {
                BridgeNavigationResult::ignored()
            };
            let _ = pending.result.try_send(result);
        }
        Response::Ok
    }

    fn bridge_activation_owned(&self, owner: &Arc<()>) -> Option<BridgeActivationTarget> {
        if !self.dashboard.owns(owner)
            || self.shutdown.load(Ordering::Acquire)
            || self.stopping.load(Ordering::Acquire)
        {
            return None;
        }
        let context = self.bridge_context()?;
        let enabled = self.settings.lock().unwrap().report.settings.iterm_focus;
        self.dashboard
            .with_owned(owner, |slot| {
                if slot.sink.is_closing() || super::dashboard::peer_has_closed(&slot.stream) {
                    return None;
                }
                let mut target = slot.bridge_identity.clone()?;
                target.iterm_focus = enabled;
                target.owner = Some(Box::new(ovrcr_protocol::BridgeOwnerTicket {
                    schema: BRIDGE_SCHEMA_VERSION,
                    server_wire: PROTOCOL_VERSION,
                    context,
                    dashboard_owner: slot.bridge_owner.clone()?,
                }));
                Some(target)
            })
            .flatten()
    }

    pub(super) fn prepare_iterm_focus(&self, owner: &Arc<()>) -> Response {
        let Ok(_mutation) = self.mutation_lock.try_lock() else {
            return Response::ITermFocusPrepared(None);
        };
        let target = self
            .bridge_activation_owned(owner)
            .filter(|target| target.iterm_focus && target.iterm_session_id.is_some());
        Response::ITermFocusPrepared(target)
    }

    pub(super) fn bridge_owner(&self, call: ovrcr_protocol::BridgeOwnerCall) -> Response {
        let unavailable =
            || Response::BridgeOwner(ovrcr_protocol::BridgeOwnerResult::unavailable());
        let Ok(_mutation) = self.mutation_lock.try_lock() else {
            return unavailable();
        };
        if !call.owner.validate() || Some(&call.owner.context) != self.bridge_context().as_ref() {
            return unavailable();
        }
        let Some(snapshot) = self.dashboard.snapshot().filter(|s| !s.sink.is_closing()) else {
            return unavailable();
        };
        let Some(target) = self
            .bridge_activation_owned(&snapshot.identity)
            .filter(|target| target.owner.as_deref() == Some(&call.owner))
        else {
            return unavailable();
        };
        if target.iterm_focus
            && let Some(status) = call.outcome
        {
            self.dashboard.send_owner(
                &snapshot.identity,
                ServerMessage::Event(ServerEvent::ITermFocus(status)),
            );
        }
        Response::BridgeOwner(ovrcr_protocol::BridgeOwnerResult {
            schema: BRIDGE_SCHEMA_VERSION,
            server_wire: PROTOCOL_VERSION,
            target: Some(target),
        })
    }

    pub(super) fn dashboard_bridge_identity(
        &self,
        owner: &Arc<()>,
        iterm_session_id: Option<String>,
    ) -> Response {
        if !self.dashboard.owns(owner) {
            return Response::Ok;
        }
        let value = iterm_session_id.filter(|value| {
            value.len() <= 256
                && !value.chars().any(char::is_control)
                && value.rsplit_once(':').is_some_and(|(_, guid)| {
                    ovrcr_protocol::bridge::canonical_uuid(&guid.to_ascii_lowercase())
                })
        });
        self.dashboard.with_owned(owner, |slot| {
            if let Some(identity) = slot.bridge_identity.as_mut() {
                identity.iterm_session_id = value;
            }
        });
        Response::Ok
    }
}

#[cfg(target_os = "macos")]
pub(super) fn peer_identity(stream: &UnixStream) -> Option<BridgeActivationTarget> {
    let mut uid = 0;
    let mut gid = 0;
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of_val(&pid) as libc::socklen_t;
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as libc::c_int;
    unsafe {
        if libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) != 0
            || uid != libc::geteuid()
            || libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_LOCAL,
                libc::LOCAL_PEERPID,
                (&mut pid as *mut libc::pid_t).cast(),
                &mut len,
            ) != 0
            || len as usize != std::mem::size_of_val(&pid)
            || pid <= 0
            || libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            ) != size
            || info.pbi_uid != uid
            || info.pbi_pid != pid as u32
        {
            return None;
        }
    }
    Some(BridgeActivationTarget {
        dashboard_pid: pid as u32,
        dashboard_start_seconds: info.pbi_start_tvsec,
        dashboard_start_microseconds: info.pbi_start_tvusec,
        iterm_session_id: None,
        iterm_focus: false,
        owner: None,
    })
}

#[cfg(not(target_os = "macos"))]
pub(super) fn peer_identity(_: &UnixStream) -> Option<BridgeActivationTarget> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_executable_hash_rejects_symlinks_nonexecutables_expiry_and_changed_bytes() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("ovrcr");
        std::fs::write(&executable, b"reviewed executable bytes").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let original =
            bridge_executable_sha256(&executable, Instant::now() + Duration::from_secs(1)).unwrap();
        assert_eq!(original.len(), 64);
        assert_eq!(
            original, "7e4f90f95b08f94a48d3575718a085ce4fd7b5dd1fc12862b43bbe2b5a082111",
            "SHA-256 byte compatibility"
        );
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&executable, &alias).unwrap();
        assert!(bridge_executable_sha256(&alias, Instant::now() + Duration::from_secs(1)).is_err());
        assert!(bridge_executable_sha256(&executable, Instant::now()).is_err());
        std::fs::write(&executable, b"unreviewed replacement bytes").unwrap();
        assert_ne!(
            bridge_executable_sha256(&executable, Instant::now() + Duration::from_secs(1)).unwrap(),
            original
        );
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            bridge_executable_sha256(&executable, Instant::now() + Duration::from_secs(1)).is_err()
        );
    }

    fn fixture() -> (Arc<ServerState>, BridgeNavigationTicket) {
        let state = super::super::tests::test_state(None, None);
        let record = state
            .retained
            .lock()
            .create(crate::retained::SessionMetadata {
                project: "p".into(),
                workspace: "w".into(),
                name: "n".into(),
                label: "sh".into(),
                cwd: "/tmp".into(),
                kind: SessionKind::Terminal,
                pinned_title: None,
                application_title: None,
            })
            .unwrap();
        let record = state
            .retained
            .lock()
            .begin_run(record.id, record.run)
            .unwrap();
        let context = state.bridge_context().unwrap();
        let ticket = BridgeNavigationTicket {
            schema: BRIDGE_SCHEMA_VERSION,
            server_wire: PROTOCOL_VERSION,
            server_socket: context.server_socket,
            callback_executable: context.callback_executable,
            callback_executable_sha256: context.callback_executable_sha256,
            server_lifetime: context.server_lifetime,
            session: record.id,
            run: record.run,
        };
        (state, ticket)
    }

    #[test]
    fn iterm_setup_and_owner_checks_use_current_opt_in_and_replace_connection_nonce() {
        let (state, _) = fixture();
        let (peer, stream) = UnixStream::pair().unwrap();
        let sink = DashboardSink::new();
        let owner = state.dashboard.claim(sink, stream).unwrap();
        state.dashboard.with_owned(&owner, |slot| {
            slot.bridge_identity = Some(BridgeActivationTarget {
                dashboard_pid: 7,
                dashboard_start_seconds: 8,
                dashboard_start_microseconds: 9,
                iterm_session_id: Some("w0t0p0:12345678-1234-4234-8234-123456789abc".into()),
                iterm_focus: false,
                owner: None,
            })
        });
        assert_eq!(
            state.prepare_iterm_focus(&owner),
            Response::ITermFocusPrepared(None)
        );
        state.settings.lock().unwrap().report.settings.iterm_focus = true;
        let Response::ITermFocusPrepared(Some(target)) = state.prepare_iterm_focus(&owner) else {
            panic!("explicit setup target missing");
        };
        let call = ovrcr_protocol::BridgeOwnerCall {
            owner: *target.owner.clone().unwrap(),
            outcome: None,
        };
        let Response::BridgeOwner(result) = state.bridge_owner(call.clone()) else {
            panic!("owner response missing");
        };
        assert_eq!(result.target, Some(target.clone()));
        state.settings.lock().unwrap().report.settings.iterm_focus = false;
        let Response::BridgeOwner(result) = state.bridge_owner(call.clone()) else {
            panic!("owner response missing");
        };
        assert!(
            !result.target.unwrap().iterm_focus,
            "old applied target cannot retain old opt-in"
        );
        assert!(
            !state
                .settings
                .lock()
                .unwrap()
                .report
                .settings
                .desktop_notifications
        );
        assert!(!state.settings.lock().unwrap().report.settings.ready_sound);
        state.dashboard.release(&owner);
        let (_replacement_peer, replacement) = UnixStream::pair().unwrap();
        let replacement_owner = state
            .dashboard
            .claim(DashboardSink::new(), replacement)
            .unwrap();
        state.dashboard.with_owned(&replacement_owner, |slot| {
            slot.bridge_identity = Some(target)
        });
        assert_eq!(
            state.bridge_owner(call),
            Response::BridgeOwner(ovrcr_protocol::BridgeOwnerResult::unavailable()),
            "same process context must not defeat connection replacement"
        );
        assert_eq!(
            state.prepare_iterm_focus(&owner),
            Response::ITermFocusPrepared(None)
        );
        drop(peer);
    }

    #[test]
    fn applied_click_uses_current_accepted_opt_in_instead_of_pre_offer_preference() {
        for (before, accepted) in [(false, true), (true, false)] {
            let (state, ticket) = fixture();
            state.settings.lock().unwrap().report.settings.iterm_focus = before;
            let (owner, sink, _peer, received, navigation) = pending(&state, ticket.clone());
            state.dashboard.with_owned(&owner, |slot| {
                slot.bridge_identity = Some(BridgeActivationTarget {
                    dashboard_pid: 7,
                    dashboard_start_seconds: 8,
                    dashboard_start_microseconds: 9,
                    iterm_session_id: Some("w0t0p0:12345678-1234-4234-8234-123456789abc".into()),
                    iterm_focus: before,
                    owner: None,
                });
            });
            assert_eq!(
                state.confirm_notification_navigation(&owner, &navigation, 19),
                Response::Ok
            );
            assert_eq!(confirmation(&sink).unwrap().ticket, ticket);
            state.settings.lock().unwrap().report.settings.iterm_focus = accepted;
            assert_eq!(
                state.notification_navigation_applied(&owner, &navigation),
                Response::Ok
            );
            let result = received.try_recv().unwrap();
            assert!(result.applied);
            let target = result.activation.unwrap();
            assert_eq!(
                target.iterm_focus, accepted,
                "applied result reads the current accepted preference"
            );
            assert_eq!(target.dashboard_pid, 7);
            assert_eq!(
                target.owner.unwrap().context,
                state.bridge_context().unwrap()
            );
            assert_eq!(
                state.retained.lock().get(ticket.session).unwrap().run,
                ticket.run
            );
        }
    }

    #[test]
    fn control_role_cannot_initiate_iterm_setup_and_bad_lifetime_cannot_report_to_dashboard() {
        let (state, _) = fixture();
        let mut role = ClientRole::Control;
        assert_eq!(
            super::super::connections::handle_request_with_id(
                &state,
                &mut role,
                Request::PrepareITermFocus,
                1,
                None
            ),
            Response::ITermFocusPrepared(None)
        );
        let context = state.bridge_context().unwrap();
        let call = ovrcr_protocol::BridgeOwnerCall {
            owner: ovrcr_protocol::BridgeOwnerTicket {
                schema: BRIDGE_SCHEMA_VERSION,
                server_wire: PROTOCOL_VERSION,
                context,
                dashboard_owner: new_identity().unwrap(),
            },
            outcome: Some(ovrcr_protocol::ITermFocusStatus::Authorized),
        };
        assert_eq!(
            state.bridge_owner(call),
            Response::BridgeOwner(ovrcr_protocol::BridgeOwnerResult::unavailable())
        );
    }

    #[test]
    fn notification_navigation_rejects_new_lifetime_run_missing_and_stopping_but_accepts_exit() {
        let (state, ticket) = fixture();
        assert!(state.navigation_target_valid(&ticket));
        state
            .retained
            .lock()
            .mark_stopped(ticket.session, ticket.run)
            .unwrap();
        assert!(
            state.navigation_target_valid(&ticket),
            "exit does not redirect the original run"
        );
        let mut other = ticket.clone();
        other.server_lifetime = new_identity().unwrap();
        assert!(!state.navigation_target_valid(&other));
        let mut other = ticket.clone();
        other.server_socket.push_str(".other");
        assert!(!state.navigation_target_valid(&other));
        let mut other = ticket.clone();
        other.callback_executable_sha256 = "1".repeat(64);
        assert!(
            !state.navigation_target_valid(&other),
            "new on-disk CLI bytes cannot replace cached Server identity"
        );
        state.stopping.store(true, Ordering::Release);
        assert!(!state.navigation_target_valid(&ticket));
        state.stopping.store(false, Ordering::Release);
        state
            .retained
            .lock()
            .begin_run(ticket.session, ticket.run)
            .unwrap();
        assert!(
            !state.navigation_target_valid(&ticket),
            "reopen fences even the same retained row"
        );
        assert_eq!(
            state.navigate_notification(ticket),
            Response::NotificationNavigation(BridgeNavigationResult::ignored())
        );
    }

    #[test]
    fn notification_navigation_archive_remove_absent_dashboard_and_busy_lifecycle_are_noops() {
        for change in ["archive", "remove", "dashboard", "busy"] {
            let (state, ticket) = fixture();
            match change {
                "archive" => state
                    .retained
                    .lock()
                    .set_archived(ticket.session, ticket.run, true)
                    .unwrap(),
                "remove" => {
                    state
                        .retained
                        .lock()
                        .remove(ticket.session, ticket.run)
                        .unwrap();
                }
                _ => {}
            }
            let mutation = (change == "busy").then(|| state.mutation_lock.lock().unwrap());
            assert_eq!(
                state.navigate_notification(ticket),
                Response::NotificationNavigation(BridgeNavigationResult::ignored()),
                "{change}"
            );
            drop(mutation);
        }
    }

    fn pending(
        state: &ServerState,
        ticket: BridgeNavigationTicket,
    ) -> (
        Arc<()>,
        Arc<DashboardSink>,
        UnixStream,
        Receiver<BridgeNavigationResult>,
        String,
    ) {
        let (peer, stream) = UnixStream::pair().unwrap();
        let sink = DashboardSink::new();
        let owner = state.dashboard.claim(Arc::clone(&sink), stream).unwrap();
        let navigation = new_identity().unwrap();
        let (result, received) = mpsc::sync_channel(1);
        state.dashboard.with_owned(&owner, |slot| {
            slot.navigation = Some(PendingNavigation {
                offer: BridgeNavigationOffer {
                    navigation: navigation.clone(),
                    ticket,
                },
                deadline: Instant::now() + NAVIGATION_BUDGET,
                confirmed: false,
                result,
                callback: None,
            })
        });
        (owner, sink, peer, received, navigation)
    }

    fn confirmation(sink: &DashboardSink) -> Option<BridgeNavigationOffer> {
        let Some(DashboardDelivery::Message(message)) = sink.next() else {
            panic!("owner confirmation absent")
        };
        let ServerMessage::Response {
            response: Response::NotificationNavigationConfirmed(offer),
            ..
        } = message.message
        else {
            panic!("wrong confirmation")
        };
        offer.map(|offer| *offer)
    }

    #[test]
    fn notification_navigation_confirmation_and_applied_receipt_revalidate_original_run() {
        for stage in ["confirm", "apply", "current"] {
            let (state, ticket) = fixture();
            let (owner, sink, _peer, received, navigation) = pending(&state, ticket.clone());
            if stage == "confirm" {
                state
                    .retained
                    .lock()
                    .begin_run(ticket.session, ticket.run)
                    .unwrap();
            }
            assert_eq!(
                state.confirm_notification_navigation(&owner, &navigation, 19),
                Response::Ok
            );
            assert_eq!(confirmation(&sink).is_some(), stage != "confirm");
            if stage == "apply" {
                state
                    .retained
                    .lock()
                    .begin_run(ticket.session, ticket.run)
                    .unwrap();
            }
            state.notification_navigation_applied(&owner, &navigation);
            if stage == "confirm" {
                assert!(received.try_recv().is_err());
            } else {
                assert_eq!(received.try_recv().unwrap().applied, stage == "current");
            }
            assert!(
                state
                    .dashboard
                    .with_owned(&owner, |slot| slot.navigation.is_none())
                    .unwrap()
            );
        }
    }

    #[test]
    fn notification_navigation_stale_owner_and_expired_or_unconfirmed_receipts_never_activate() {
        for stage in ["owner", "expired", "unconfirmed"] {
            let (state, ticket) = fixture();
            let (owner, _sink, _peer, received, navigation) = pending(&state, ticket);
            if stage == "owner" {
                state.dashboard.release(&owner);
                let (_replacement_peer, replacement) = UnixStream::pair().unwrap();
                let replacement_owner = state
                    .dashboard
                    .claim(DashboardSink::new(), replacement)
                    .unwrap();
                state.notification_navigation_applied(&owner, &navigation);
                assert!(received.try_recv().is_err());
                assert!(
                    state
                        .dashboard
                        .with_owned(&replacement_owner, |slot| slot.navigation.is_none())
                        .unwrap()
                );
            } else {
                state.dashboard.with_owned(&owner, |slot| {
                    let pending = slot.navigation.as_mut().unwrap();
                    pending.confirmed = stage != "unconfirmed";
                    if stage == "expired" {
                        pending.deadline = Instant::now();
                    }
                });
                state.notification_navigation_applied(&owner, &navigation);
                assert!(!received.try_recv().unwrap().applied, "{stage}");
            }
        }
    }

    #[test]
    fn notification_navigation_closed_callback_or_draining_owner_cannot_confirm_or_activate() {
        for stage in ["before_confirm", "before_apply", "draining"] {
            let (state, ticket) = fixture();
            let (owner, sink, _peer, received, navigation) = pending(&state, ticket);
            let (callback_peer, callback) = UnixStream::pair().unwrap();
            state.dashboard.with_owned(&owner, |slot| {
                slot.navigation.as_mut().unwrap().callback = Some(callback)
            });
            let mut callback_peer = Some(callback_peer);
            if stage == "before_confirm" {
                callback_peer.take();
            }
            state.confirm_notification_navigation(&owner, &navigation, 20);
            assert_eq!(confirmation(&sink).is_some(), stage != "before_confirm");
            if stage == "before_apply" {
                callback_peer.take();
            }
            if stage == "draining" {
                assert!(state.dashboard.send_owner_terminal(
                    &owner,
                    ServerMessage::Response {
                        request_id: 21,
                        response: Response::Ok
                    },
                    None
                ));
            }
            state.notification_navigation_applied(&owner, &navigation);
            if stage == "before_apply" {
                assert!(!received.try_recv().unwrap().applied);
            } else {
                assert!(received.try_recv().is_err(), "{stage}");
            }
            state.dashboard.release(&owner);
            drop(callback_peer);
        }
    }
}

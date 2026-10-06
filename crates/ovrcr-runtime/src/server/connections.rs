use super::*;

// Arm on the handler thread so unrelated parallel tests cannot steal the panic.
#[cfg(test)]
thread_local! {
    pub(super) static PANIC_AFTER_DASHBOARD_REGISTRATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Ownership of the server's single dashboard slot for one connection.
///
/// Registration and cleanup used to sit at opposite ends of
/// `handle_connection` with fallible and panicking calls in between, so an
/// early exit leaked the slot and every later dashboard was refused. Dropping
/// this guard releases the slot whether the handler returns, breaks, or
/// unwinds.
pub(super) struct DashboardOwnership {
    pub(super) state: Arc<ServerState>,
    pub(super) identity: Arc<()>,
}

impl Drop for DashboardOwnership {
    fn drop(&mut self) {
        self.state.dashboard.release(&self.identity);
    }
}

const TERMINAL_FAILURE_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

fn await_view_completion(receiver: Receiver<DispatchCompletion>) -> Response {
    match receiver.recv() {
        Ok(DispatchCompletion::Complete) => Response::Ok,
        Ok(DispatchCompletion::Terminal(delivery)) => {
            let _ = delivery.recv_timeout(TERMINAL_FAILURE_CLOSE_TIMEOUT);
            Response::Ok
        }
        Err(_) => error_response(ErrorCode::Internal, "dispatcher is unavailable"),
    }
}

struct SupervisorOwnership {
    state: Arc<ServerState>,
    session: SessionId,
    identity: Arc<()>,
}
impl Drop for SupervisorOwnership {
    fn drop(&mut self) {
        // This is outside the dispatcher; preserve lifecycle delivery when its queue is full.
        let _ = self
            .state
            .dispatch
            .send(DispatchMessage::AgentDisconnected {
                session: self.session,
                owner: self.identity.clone(),
            });
    }
}

pub(super) fn handle_connection(state: Arc<ServerState>, mut stream: UnixStream) {
    // Complete the version handshake before any frame. Probes that connect
    // and drop, and clients built from other sources, are simply closed.
    if write_preamble(&mut stream).is_err() {
        return;
    }
    match read_preamble(&mut stream) {
        Ok(version) if version == PROTOCOL_VERSION => {}
        _ => return,
    }
    let mut role = ClientRole::Control;
    let mut ownership: Option<DashboardOwnership> = None;
    let mut supervisor: Option<SupervisorOwnership> = None;
    let dashboard_sink = DashboardSink::new();
    let mut writer: Option<JoinHandle<()>> = None;
    while !state.shutdown.load(Ordering::Acquire) {
        let message = match read_frame::<ClientMessage>(&mut stream) {
            Ok(message) => message,
            Err(_) => break,
        };
        if let Some(session) = match &message.request {
            Request::SupervisorHello(auth) => Some(auth.session),
            Request::ReserveAgent(reserve) => Some(reserve.session),
            _ => None,
        } {
            if ownership.is_some()
                || supervisor
                    .as_ref()
                    .is_some_and(|owned| owned.session != session)
            {
                let _ = send_direct(
                    &mut stream,
                    message.request_id,
                    error_response(ErrorCode::Conflict, "connection already has an owner"),
                );
                break;
            }
            let identity = supervisor
                .as_ref()
                .map_or_else(|| Arc::new(()), |owned| owned.identity.clone());
            let response = handle_request_with_id(
                &state,
                &mut role,
                message.request.clone(),
                message.request_id,
                Some(&identity),
            );
            let accepted = matches!(
                response,
                Response::Ok
                    | Response::AgentOperation(ovrcr_protocol::AgentOperationResult::Reserved(_))
            );
            if accepted && supervisor.is_none() {
                supervisor = Some(SupervisorOwnership {
                    state: state.clone(),
                    session,
                    identity,
                });
            }
            if send_direct(&mut stream, message.request_id, response).is_err() || !accepted {
                break;
            }
            continue;
        }
        if supervisor.is_some()
            && !matches!(
                message.request,
                Request::Supervisor(_) | Request::AgentStatus { .. }
            )
        {
            let _ = send_direct(
                &mut stream,
                message.request_id,
                error_response(
                    ErrorCode::InvalidRequest,
                    "dedicated supervisor connection accepts only supervisor requests",
                ),
            );
            break;
        }
        if matches!(message.request, Request::DashboardHello) {
            if writer.is_some() {
                let (completion, result) = mpsc::sync_channel(1);
                let queued = ownership.as_ref().is_some_and(|owned| {
                    state.dashboard.send_owner_with_completion(
                        &owned.identity,
                        response_message(
                            message.request_id,
                            error_response(
                                ErrorCode::Conflict,
                                "dashboard is already registered on this connection",
                            ),
                        ),
                        Some(completion),
                    )
                });
                if queued {
                    let _ = result.recv();
                }
                break;
            }
            let Ok(close_stream) = stream.try_clone() else {
                break;
            };
            let Ok(writer_stream) = stream.try_clone() else {
                break;
            };
            // Built before the slot is claimed. Collection starts at claim and
            // can publish during a slow hierarchy walk; the greeting is queued
            // with this snapshot immediately afterwards.
            let hierarchy = state.hierarchy();
            let Some(identity) = state
                .dashboard
                .claim(Arc::clone(&dashboard_sink), close_stream)
            else {
                let _ = send_direct(
                    &mut stream,
                    message.request_id,
                    error_response(
                        ErrorCode::Conflict,
                        "another dashboard is already connected",
                    ),
                );
                break;
            };
            #[cfg(feature = "acceptance-diagnostics")]
            if let Some(monitor) = &state.dashboard_monitor {
                monitor.register(&dashboard_sink);
            }
            ownership = Some(DashboardOwnership {
                state: Arc::clone(&state),
                identity: Arc::clone(&identity),
            });
            #[cfg(test)]
            if PANIC_AFTER_DASHBOARD_REGISTRATION.with(|armed| armed.replace(false)) {
                panic!("injected panic after dashboard registration");
            }
            role = ClientRole::Dashboard;
            // Queue the greeting before the writer thread exists. Quota
            // collection starts as soon as the slot is claimed, and a native
            // read can finish during thread spawn; the greeting still has to
            // be the first frame on this connection.
            state.dashboard.send_owner(
                &identity,
                response_message(message.request_id, Response::Hierarchy(hierarchy)),
            );
            let spawned = spawn_writer(
                Arc::clone(&state),
                Arc::clone(&dashboard_sink),
                Arc::clone(&identity),
                writer_stream,
            );
            match spawned {
                Ok(handle) => writer = Some(handle),
                Err(_) => break,
            }
            state.refresh_settings();
            state.publish_settings();
            state.publish_quotas();
            continue;
        }
        if matches!(message.request, Request::Events { follow: true })
            && matches!(role, ClientRole::Control)
        {
            let (events, follower) = state.subscribe_events();
            if send_direct(&mut stream, message.request_id, Response::Events(events)).is_ok() {
                follow_events(&state, &mut stream, follower);
            }
            break;
        }
        let shutdown = matches!(message.request, Request::Shutdown { .. });
        let deferred_view = matches!(
            message.request,
            Request::Select { .. } | Request::Resize { .. } | Request::SetView { .. }
        );
        let history = matches!(
            message.request,
            Request::HistoryBegin { .. } | Request::HistoryPage { .. } | Request::HistoryEnd { .. }
        );
        let response = handle_request_with_id(
            &state,
            &mut role,
            message.request.clone(),
            message.request_id,
            ownership.as_ref().map(|owned| &owned.identity),
        );
        let successful_shutdown = shutdown && state.stopping.load(Ordering::Acquire);
        let (delivered, dashboard_shutdown_attempt) = match role {
            ClientRole::Control => (
                send_direct(&mut stream, message.request_id, response).is_ok(),
                false,
            ),
            ClientRole::Dashboard => {
                if history {
                    if matches!(response, Response::Ok) {
                        (true, false)
                    } else {
                        let delivered = ownership.as_ref().is_some_and(|owned| {
                            state.dashboard.send_owner(
                                &owned.identity,
                                response_message(message.request_id, response),
                            )
                        });
                        (delivered, false)
                    }
                } else if !deferred_view {
                    if successful_shutdown {
                        let (completion, result) = mpsc::sync_channel(1);
                        let queued = ownership.as_ref().is_some_and(|owned| {
                            state.dashboard.send_owner_with_completion(
                                &owned.identity,
                                response_message(message.request_id, response),
                                Some(completion),
                            )
                        });
                        let delivered =
                            queued && result.recv().is_ok_and(|write_result| write_result.is_ok());
                        (delivered, true)
                    } else {
                        // A response belongs to the dashboard that asked. A
                        // replacement owner must never receive an evicted
                        // connection's late answer under its own request id.
                        let delivered = ownership.as_ref().is_some_and(|owned| {
                            state.dashboard.send_owner(
                                &owned.identity,
                                response_message(message.request_id, response),
                            )
                        });
                        (delivered, false)
                    }
                } else if matches!(response, Response::Ok) {
                    (true, false)
                } else {
                    let delivered = ownership.as_ref().is_some_and(|owned| {
                        state.dashboard.send_owner(
                            &owned.identity,
                            response_message(message.request_id, response),
                        )
                    });
                    (delivered, false)
                }
            }
        };
        // Accepted shutdown must complete even if its response could not be delivered.
        if successful_shutdown {
            debug_assert!(dashboard_shutdown_attempt || matches!(role, ClientRole::Control));
            state.shutdown.store(true, Ordering::Release);
            wake_accept(&state);
        }
        if dashboard_sink.is_closing() {
            if let Some(owned) = ownership.as_ref()
                && let Some(snapshot) = state.dashboard.snapshot_owned(&owned.identity)
            {
                state.dashboard.disconnect(snapshot);
            }
            break;
        }
        if !delivered {
            break;
        }
        if matches!(role, ClientRole::Control) && supervisor.is_none() {
            break;
        }
    }
    drop(supervisor);
    drop(ownership);
    dashboard_sink.close();
    if let Some(writer) = writer {
        let _ = writer.join();
    }
}

pub(super) fn handle_request_with_id(
    state: &ServerState,
    role: &mut ClientRole,
    request: Request,
    request_id: u64,
    owner: Option<&Arc<()>>,
) -> Response {
    let dashboard = matches!(role, ClientRole::Dashboard);
    match request {
        Request::Task(request) => match &state.tasks {
            Some(tasks) => tasks.handle(state, *request).map_or_else(
                |error| error_response(ErrorCode::InvalidRequest, format!("{error:#}")),
                |value| Response::Task(Box::new(value)),
            ),
            None => error_response(ErrorCode::Internal, "task manager unavailable"),
        },
        Request::List => Response::Hierarchy(state.hierarchy()),
        Request::Inspect => {
            let (registry, sessions) = state.inventory();
            Response::Inventory { registry, sessions }
        }
        Request::ReadTerminal { session, max_lines } => state
            .read_terminal(session, max_lines)
            .map_or_else(error_for_lifecycle, |(size, text)| Response::TerminalText {
                session,
                size,
                text,
            }),
        Request::SendTerminal {
            session,
            text,
            submit,
        } => state
            .send_terminal(session, &text, submit)
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::RefreshQuota { provider } => super::quota::request_refresh(state, provider),
        Request::Keystroke {
            session,
            expected_run,
            key,
        } => state
            .keystroke(session, expected_run, &key)
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::DeleteArchivedSession {
            session,
            expected_run,
        } => state
            .delete_archived_session(session, expected_run)
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::UnarchiveSession {
            session,
            expected_run,
        } => {
            state
                .unarchive_session(session, expected_run)
                .map_or_else(error_for_lifecycle, |_| {
                    state
                        .dashboard
                        .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                            state.hierarchy(),
                        )));
                    Response::Ok
                })
        }
        Request::CloseTerminal {
            session,
            expected_run,
        } if dashboard && state.lifecycle.async_ready() => lifecycle::try_accept(
            state,
            request_id,
            lifecycle::JobKind::CloseTerminal {
                session,
                expected_run,
            },
        ),
        Request::CloseTerminal {
            session,
            expected_run,
        } => state
            .close_terminal(session, expected_run, requested_kill_grace())
            .map_or_else(error_for_lifecycle, |_| {
                state
                    .dashboard
                    .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                        state.hierarchy(),
                    )));
                Response::Ok
            }),
        request @ (Request::ReserveAgent(_)
        | Request::Supervisor(_)
        | Request::AgentStatus { .. }
        | Request::SupervisorHello(_)
        | Request::MarkReviewed { .. }) => {
            let (completion, result) = mpsc::sync_channel(1);
            match state.dispatch.try_send(DispatchMessage::AgentCommand {
                request,
                owner: owner.cloned(),
                completion,
            }) {
                Ok(()) => result.recv().unwrap_or_else(|_| {
                    error_response(ErrorCode::Internal, "dispatcher is unavailable")
                }),
                Err(mpsc::TrySendError::Full(_)) => {
                    error_response(ErrorCode::Conflict, "dispatcher queue is full")
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    error_response(ErrorCode::Internal, "dispatcher is unavailable")
                }
            }
        }
        Request::AgentReport(report) => {
            let (completion, result) = mpsc::sync_channel(1);
            match state
                .dispatch
                .try_send(DispatchMessage::AgentReport { report, completion })
            {
                Ok(()) => result.recv().unwrap_or_else(|_| {
                    error_response(ErrorCode::Internal, "dispatcher is unavailable")
                }),
                Err(mpsc::TrySendError::Full(_)) => {
                    error_response(ErrorCode::Conflict, "dispatcher queue is full")
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    error_response(ErrorCode::Internal, "dispatcher is unavailable")
                }
            }
        }
        Request::PauseSession { session } => state
            .set_session_paused(session, true)
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::ResumeSession { session } => state
            .set_session_paused(session, false)
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::Select { session, size } => {
            if !dashboard {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "Select requires a dashboard connection",
                );
            }
            let Some(owner) = owner else {
                return error_response(ErrorCode::Conflict, "dashboard is disconnected");
            };
            let run = {
                let live = state
                    .sessions
                    .lock()
                    .unwrap()
                    .get(&session)
                    .map(|session| session.run());
                match live {
                    Some(run) => run,
                    None => match state.session_summary(session) {
                        Some(summary) => summary.run,
                        None => {
                            return error_response(
                                ErrorCode::NotFound,
                                format!("session {} not found", session.0),
                            );
                        }
                    },
                }
            };
            let revision = state.dashboard.next_revision();
            let (sender, receiver) = mpsc::sync_channel(1);
            if state
                .dispatch
                .send(DispatchMessage::SetView {
                    owner: Arc::clone(owner),
                    request_id,
                    view: ovrcr_protocol::DashboardView {
                        revision,
                        panes: vec![ovrcr_protocol::PaneTarget { session, run, size }],
                        focused: Some(session),
                    },
                    completion: sender,
                })
                .is_err()
            {
                return error_response(ErrorCode::Internal, "dispatcher is unavailable");
            }
            await_view_completion(receiver)
        }
        Request::HistoryBegin { session } => dispatch_history_request(
            state,
            dashboard,
            owner,
            request_id,
            HistoryRequest::Begin { session },
        ),
        Request::HistoryPage {
            session,
            snapshot,
            start_row,
            rows,
            start_col,
            cols,
        } => dispatch_history_request(
            state,
            dashboard,
            owner,
            request_id,
            HistoryRequest::Page {
                session,
                snapshot,
                start_row,
                rows,
                start_col,
                cols,
            },
        ),
        Request::HistoryEnd { session, snapshot } => dispatch_history_request(
            state,
            dashboard,
            owner,
            request_id,
            HistoryRequest::End { session, snapshot },
        ),
        Request::Input {
            session,
            run,
            bytes,
        } => {
            if !dashboard
                || owner.is_none_or(|owner| !state.dashboard.owns(owner))
                || state.dashboard.view().and_then(|view| view.focused) != Some(session)
            {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "input is only accepted for the focused dashboard session",
                );
            }
            let pane_run = state.dashboard.view().and_then(|view| {
                view.panes
                    .iter()
                    .find(|pane| pane.session == session)
                    .map(|pane| pane.run)
            });
            if pane_run != Some(run) {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "input is only accepted for the focused dashboard session",
                );
            }
            let selected_session = state.sessions.lock().unwrap().get(&session).cloned();
            match selected_session {
                Some(session) if session.run() == run => session.write(&bytes).map_or_else(
                    |error| error_response(input_error_code(&error), error_chain_string(&error)),
                    |_| Response::Ok,
                ),
                Some(_) => error_response(
                    ErrorCode::InvalidRequest,
                    "input is only accepted for the focused dashboard session",
                ),
                None => error_response(
                    ErrorCode::NotFound,
                    format!("session {} not found", session.0),
                ),
            }
        }
        Request::Resize { session, size } => {
            if !dashboard {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "Resize requires a dashboard connection",
                );
            }
            let Some(owner) = owner else {
                return error_response(ErrorCode::Conflict, "dashboard is disconnected");
            };
            let Some(current) = state.dashboard.view() else {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "resize is only accepted for a singleton dashboard view",
                );
            };
            if current.panes.len() != 1 || current.focused != Some(session) {
                return error_response(ErrorCode::InvalidRequest, "use SetView for split geometry");
            }
            let run = current.panes[0].run;
            let revision = current
                .revision
                .checked_add(1)
                .ok_or_else(|| error_response(ErrorCode::InvalidRequest, "view revision overflow"));
            let revision = match revision {
                Ok(revision) => revision,
                Err(response) => return response,
            };
            let (sender, receiver) = mpsc::sync_channel(1);
            if state
                .dispatch
                .send(DispatchMessage::SetView {
                    owner: Arc::clone(owner),
                    request_id,
                    view: ovrcr_protocol::DashboardView {
                        revision,
                        panes: vec![ovrcr_protocol::PaneTarget { session, run, size }],
                        focused: Some(session),
                    },
                    completion: sender,
                })
                .is_err()
            {
                return error_response(ErrorCode::Internal, "dispatcher is unavailable");
            }
            await_view_completion(receiver)
        }
        Request::Shutdown { kill } => state.request_shutdown(kill),
        Request::AddProject {
            name,
            repo,
            workspace_root,
        } => state.add_project(name, repo, workspace_root).map_or_else(
            |error| lifecycle_response_with_partial_hierarchy(state, error),
            |_| {
                state
                    .dashboard
                    .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                        state.hierarchy(),
                    )));
                Response::Ok
            },
        ),
        Request::RemoveProject { name } => {
            state
                .remove_project(&name)
                .map_or_else(error_for_lifecycle, |_| {
                    state
                        .dashboard
                        .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                            state.hierarchy(),
                        )));
                    Response::Ok
                })
        }
        Request::CreateWorkspace {
            project,
            id: name,
            branch,
        } if dashboard && state.lifecycle.async_ready() => lifecycle::try_accept(
            state,
            request_id,
            lifecycle::JobKind::CreateWorkspace {
                project,
                id: name,
                branch,
            },
        ),
        Request::CreateWorkspace {
            project,
            id: name,
            branch,
        } => state.create_workspace(project, name, branch).map_or_else(
            |error| lifecycle_response_with_partial_hierarchy(state, error),
            |_| {
                state
                    .dashboard
                    .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                        state.hierarchy(),
                    )));
                Response::Ok
            },
        ),
        Request::CreateWorkspaceWithLaunch {
            project,
            id: name,
            branch,
            launch,
        } if dashboard && state.lifecycle.async_ready() => lifecycle::try_accept(
            state,
            request_id,
            lifecycle::JobKind::CreateWorkspaceWithLaunch {
                project,
                id: name,
                branch,
                launch,
            },
        ),
        Request::CreateWorkspaceWithLaunch {
            project,
            id: name,
            branch,
            launch,
        } => {
            state
                .create_workspace_with_launch(project, name, branch, launch)
                .map_or_else(
                    |error| lifecycle_response_with_partial_hierarchy(state, error),
                    |summary| {
                        state.dashboard.try_send(ServerMessage::Event(
                            ServerEvent::HierarchyChanged(state.hierarchy()),
                        ));
                        summary.map_or(Response::Ok, |summary| {
                            Response::CreatedSession(Box::new(summary))
                        })
                    },
                )
        }
        Request::SetSetting { path, value } => {
            state.set_setting(&path, value.as_deref()).map_or_else(
                |error| error_response(ErrorCode::InvalidRequest, format!("{error:#}")),
                |()| Response::Ok,
            )
        }
        Request::SetSessionTitle { session, title } => {
            state.set_session_title(session, title).map_or_else(
                |error| lifecycle_response_with_partial_hierarchy(state, error),
                |_| Response::Ok,
            )
        }
        Request::ReopenSession {
            session,
            expected_run,
            acknowledge_stopped,
        } => {
            let result = state.reopen_session(session, expected_run, acknowledge_stopped);
            state
                .dashboard
                .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                    state.hierarchy(),
                )));
            result.map_or_else(error_for_lifecycle, |summary| {
                Response::CreatedSession(Box::new(summary))
            })
        }
        Request::RecoverSession {
            session,
            expected_run,
        } => {
            let result = state.recover_session(session, expected_run);
            state
                .dashboard
                .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                    state.hierarchy(),
                )));
            result.map_or_else(error_for_lifecycle, |summary| {
                Response::CreatedSession(Box::new(summary))
            })
        }
        Request::AcknowledgeSessionStopped {
            session,
            expected_run,
        } => {
            let result = state.acknowledge_session_stopped(session, expected_run);
            state
                .dashboard
                .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                    state.hierarchy(),
                )));
            if let Some(summary) = state.session_summary(session) {
                state
                    .dashboard
                    .try_send(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
                        summary,
                    ))));
            }
            result.map_or_else(error_for_lifecycle, |_| Response::Ok)
        }
        Request::RemoveWorkspace {
            project,
            name,
            force,
        } if dashboard && state.lifecycle.async_ready() => lifecycle::try_accept(
            state,
            request_id,
            lifecycle::JobKind::RemoveWorkspace {
                project,
                name,
                force,
            },
        ),
        Request::RemoveWorkspace {
            project,
            name,
            force,
        } => state.remove_workspace(&project, &name, force).map_or_else(
            |error| lifecycle_response_with_partial_hierarchy(state, error),
            |_| {
                state
                    .dashboard
                    .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                        state.hierarchy(),
                    )));
                Response::Ok
            },
        ),
        Request::CreateSession(request) if dashboard && state.lifecycle.async_ready() => {
            lifecycle::try_accept(
                state,
                request_id,
                lifecycle::JobKind::CreateSession(request),
            )
        }
        Request::CreateSession(request) => state.create_session(request).map_or_else(
            |error| lifecycle_response_with_partial_hierarchy(state, error),
            |summary| {
                state
                    .dashboard
                    .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                        state.hierarchy(),
                    )));
                Response::CreatedSession(Box::new(summary))
            },
        ),
        Request::KillSession { session } => state
            .kill_session(session, requested_kill_grace())
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::RemoveSession { session } => {
            state
                .remove_session(session)
                .map_or_else(error_for_lifecycle, |_| {
                    state
                        .dashboard
                        .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                            state.hierarchy(),
                        )));
                    Response::Ok
                })
        }
        Request::DashboardHello => {
            *role = ClientRole::Dashboard;
            Response::Ok
        }
        Request::DashboardGeometry { size } => {
            if !dashboard {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "DashboardGeometry requires a dashboard connection",
                );
            }
            let Some(owner) = owner else {
                return error_response(ErrorCode::Conflict, "dashboard is disconnected");
            };
            let _mutation = state.mutation_lock.lock().unwrap();
            // Geometry belongs to the dashboard that reported it. A request
            // that waited out a mutation must not resize a replacement owner's
            // panes, or the next session spawns with the wrong size.
            if state.dashboard.set_geometry(owner, size) {
                Response::Ok
            } else {
                error_response(ErrorCode::Conflict, "dashboard is disconnected")
            }
        }
        Request::SetView { view } => {
            if !dashboard {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "SetView requires a dashboard connection",
                );
            }
            let Some(owner) = owner else {
                return error_response(ErrorCode::Conflict, "dashboard is disconnected");
            };
            let (sender, receiver) = mpsc::sync_channel(1);
            if state
                .dispatch
                .send(DispatchMessage::SetView {
                    owner: Arc::clone(owner),
                    request_id,
                    view,
                    completion: sender,
                })
                .is_err()
            {
                return error_response(ErrorCode::Internal, "dispatcher is unavailable");
            }
            await_view_completion(receiver)
        }
        Request::Events { .. } => Response::Events(state.event_snapshot()),
        Request::SaveWorkspaceWip { project, name } => state
            .save_workspace_wip(&project, &name)
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::AnswerWipSave {
            project,
            workspace,
            save,
        } => {
            if state.answer_wip_save(&project, &workspace, save) {
                Response::Ok
            } else {
                error_response(
                    ErrorCode::InvalidRequest,
                    "no uncommitted-work question is waiting for that workspace",
                )
            }
        }
    }
}

fn dispatch_history_request(
    state: &ServerState,
    dashboard: bool,
    owner: Option<&Arc<()>>,
    request_id: u64,
    request: HistoryRequest,
) -> Response {
    if !dashboard {
        return error_response(
            ErrorCode::InvalidRequest,
            "history requires a dashboard connection",
        );
    }
    let Some(owner) = owner else {
        return error_response(ErrorCode::Conflict, "dashboard is disconnected");
    };
    let (completion, result) = mpsc::sync_channel(1);
    if state
        .dispatch
        .send(DispatchMessage::History {
            owner: Arc::clone(owner),
            request_id,
            request,
            completion,
        })
        .is_err()
    {
        return error_response(ErrorCode::Internal, "dispatcher is unavailable");
    }
    match result.recv() {
        Ok(()) => Response::Ok,
        Err(_) => error_response(ErrorCode::Internal, "dispatcher is unavailable"),
    }
}

pub(super) fn error_for_lifecycle(error: anyhow::Error) -> Response {
    let message = error_chain_string(&error);
    error_response(lifecycle_code(&error), message)
}

pub(super) fn input_error_code(error: &anyhow::Error) -> ErrorCode {
    if error
        .chain()
        .any(|cause| cause.downcast_ref::<InputAdmissionError>().is_some())
    {
        ErrorCode::Conflict
    } else {
        ErrorCode::Internal
    }
}

pub(super) fn lifecycle_code(error: &anyhow::Error) -> ErrorCode {
    if let Some(failure) = error.downcast_ref::<LifecycleFailure>() {
        return failure.code.clone();
    }
    if error
        .downcast_ref::<crate::session::DiscoveryFailed>()
        .is_some()
    {
        return ErrorCode::OwnershipUncertain;
    }
    ErrorCode::Conflict
}

pub(super) fn combine_control_and_refresh(
    id: SessionId,
    control: Result<()>,
    refresh: Result<()>,
) -> Result<()> {
    match (control, refresh) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(lifecycle_error(
            ErrorCode::PartialFailure,
            format!(
                "session {} refresh failed: {}",
                id.0,
                error_chain_string(&error)
            ),
        )),
        (Err(control), Err(refresh)) => Err(lifecycle_error(
            lifecycle_code(&control),
            format!(
                "{}; session {} refresh failed: {}",
                error_chain_string(&control),
                id.0,
                error_chain_string(&refresh)
            ),
        )),
    }
}

pub(super) fn error_chain_string(error: &anyhow::Error) -> String {
    let mut chain = error.chain().map(ToString::to_string);
    chain.next().map_or_else(String::new, |first| {
        chain.fold(first, |mut message, cause| {
            if !message.contains(&cause) {
                message.push_str(": ");
                message.push_str(&cause);
            }
            message
        })
    })
}

pub(super) fn lifecycle_response_with_partial_hierarchy(
    state: &ServerState,
    error: anyhow::Error,
) -> Response {
    let publish_hierarchy = error
        .downcast_ref::<LifecycleFailure>()
        .is_some_and(|failure| failure.publish_hierarchy);
    let response = error_for_lifecycle(error);
    if publish_hierarchy {
        state
            .dashboard
            .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                state.hierarchy(),
            )));
    }
    response
}

#[cfg(test)]
pub(super) fn handle_shutdown<F>(state: &Arc<ServerState>, kill: bool, mut terminate: F) -> Response
where
    F: FnMut(&Arc<Session>) -> Result<()>,
{
    let _mutation = state.mutation_lock.lock().unwrap();
    let sessions = state
        .sessions
        .lock()
        .unwrap()
        .values()
        .cloned()
        .collect::<Vec<_>>();
    // Retained or exited rows do not keep a server alive.
    if !kill && sessions.iter().any(|session| session.is_live()) {
        return error_response(ErrorCode::SessionsRemain, "live sessions remain");
    }
    if kill {
        let mut failures = Vec::new();
        for session in sessions {
            session.revoke_hook_capability();
            if let Err(error) = terminate(&session) {
                failures.push(format!(
                    "session {}: {}",
                    session.summary().id.0,
                    error_chain_string(&error)
                ));
            }
        }
        if !failures.is_empty() {
            return error_response(ErrorCode::PartialFailure, failures.join("; "));
        }
    }
    Response::Ok
}

pub(super) fn error_response(code: ErrorCode, message: impl std::fmt::Display) -> Response {
    Response::Error {
        code,
        message: message.to_string(),
    }
}

pub(super) fn requested_kill_grace() -> Duration {
    // Integration tests use this per-server-process seam to exercise the short
    // escalation path without changing the normal five-second CLI behavior.
    std::env::var("OVRCR_KILL_GRACE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(5))
}
pub(super) fn response_message(request_id: u64, response: Response) -> ServerMessage {
    ServerMessage::Response {
        request_id,
        response,
    }
}
fn send_direct(stream: &mut UnixStream, request_id: u64, response: Response) -> Result<()> {
    write_frame(stream, &response_message(request_id, response))
}

fn follow_events(
    state: &ServerState,
    stream: &mut UnixStream,
    follower: mpsc::Receiver<ovrcr_protocol::Event>,
) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    loop {
        if state.shutdown.load(Ordering::Acquire) || peer_closed(stream) {
            break;
        }
        match follower.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => {
                if write_frame(stream, &ServerMessage::Event(ServerEvent::Recorded(event))).is_err()
                {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn peer_closed(stream: &UnixStream) -> bool {
    let fd = stream.as_raw_fd();
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN | libc::POLLHUP,
        revents: 0,
    };
    if unsafe { libc::poll(&mut pollfd, 1, 0) } <= 0 {
        return false;
    }
    if pollfd.revents & libc::POLLIN != 0 {
        let mut byte = [0u8; 1];
        let read = unsafe {
            libc::recv(
                fd,
                byte.as_mut_ptr().cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        return read == 0;
    }
    pollfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
}

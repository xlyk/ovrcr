use super::*;

pub(super) fn handle_connection(state: Arc<ServerState>, mut stream: UnixStream) {
    let mut role = ClientRole::Control;
    let mut dashboard_identity = None;
    let dashboard_sink = DashboardSink::new();
    let mut writer: Option<JoinHandle<()>> = None;
    while !state.shutdown.load(Ordering::Acquire) {
        let message = match read_frame::<ClientMessage>(&mut stream) {
            Ok(message) => message,
            Err(_) => break,
        };
        if matches!(message.request, Request::DashboardHello) {
            if writer.is_some() {
                let (completion, result) = mpsc::sync_channel(1);
                if dashboard_send(
                    &state,
                    response_message(
                        message.request_id,
                        error_response(
                            ErrorCode::Conflict,
                            "dashboard is already registered on this connection",
                        ),
                    ),
                    Some(completion),
                ) {
                    let _ = result.recv();
                }
                break;
            }
            let identity = Arc::new(());
            let Ok(close_stream) = stream.try_clone() else {
                break;
            };
            let mut slot = state.dashboard_slot.lock().unwrap();
            if slot.is_some() {
                drop(slot);
                let _ = send_direct(
                    &mut stream,
                    message.request_id,
                    error_response(
                        ErrorCode::Conflict,
                        "another dashboard is already connected",
                    ),
                );
                break;
            }
            *slot = Some(DashboardSlot {
                sink: Arc::clone(&dashboard_sink),
                identity: identity.clone(),
                stream: close_stream,
                history: None,
                next_history_id: 1,
            });
            drop(slot);
            *state.dashboard.lock().unwrap() = Some(Arc::clone(&dashboard_sink));
            dashboard_identity = Some(Arc::clone(&identity));
            role = ClientRole::Dashboard;
            let mut output = stream.try_clone().ok();
            let writer_sink = Arc::clone(&dashboard_sink);
            let writer_state = Arc::clone(&state);
            let writer_identity = Arc::clone(&identity);
            let writer_close_stream = stream.try_clone().expect("clone dashboard close stream");
            writer = Some(
                thread::Builder::new()
                    .name("ovrcr-dashboard-writer".into())
                    .spawn(move || {
                        while let Some(delivery) = writer_sink.next() {
                            let (message, completion, dirty) = match delivery {
                                DashboardDelivery::Message(outbound) => {
                                    (outbound.message, outbound.completion, None)
                                }
                                DashboardDelivery::Dirty(session) => (
                                    ServerMessage::Event(ServerEvent::ScreenDirty { session }),
                                    None,
                                    Some(session),
                                ),
                            };
                            let result = match output.as_mut() {
                                Some(stream) => {
                                    write_frame(stream, &message).map_err(|error| error.to_string())
                                }
                                None => Err("dashboard writer stream unavailable".into()),
                            };
                            if result.is_ok()
                                && let Some(session) = dirty
                            {
                                writer_sink.dirty_sent(session);
                            }
                            if let Some(completion) = completion {
                                let _ = completion.send(result.clone());
                            }
                            if result.is_err() {
                                writer_sink.close();
                                let _ = writer_close_stream.shutdown(std::net::Shutdown::Both);
                                disconnect_dashboard(
                                    &writer_state,
                                    DashboardSnapshot {
                                        sink: writer_sink,
                                        identity: writer_identity,
                                        stream: writer_close_stream,
                                    },
                                );
                                break;
                            }
                        }
                    })
                    .expect("dashboard writer thread"),
            );
            dashboard_try_send(
                &state,
                response_message(message.request_id, Response::Hierarchy(snapshot(&state))),
            );
            continue;
        }
        let shutdown = matches!(message.request, Request::Shutdown { .. });
        let select = matches!(message.request, Request::Select { .. });
        let history = matches!(
            message.request,
            Request::HistoryBegin { .. } | Request::HistoryPage { .. } | Request::HistoryEnd { .. }
        );
        let response = handle_request_with_id(
            &state,
            &mut role,
            message.request.clone(),
            message.request_id,
            dashboard_identity.as_ref(),
        );
        let successful_shutdown = shutdown && matches!(&response, Response::Ok);
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
                        (
                            dashboard_try_send(
                                &state,
                                response_message(message.request_id, response),
                            ),
                            false,
                        )
                    }
                } else if !select {
                    if successful_shutdown {
                        let (completion, result) = mpsc::sync_channel(1);
                        let queued = dashboard_send(
                            &state,
                            response_message(message.request_id, response),
                            Some(completion),
                        );
                        let delivered =
                            queued && result.recv().is_ok_and(|write_result| write_result.is_ok());
                        (delivered, true)
                    } else {
                        (
                            dashboard_try_send(
                                &state,
                                response_message(message.request_id, response),
                            ),
                            false,
                        )
                    }
                } else {
                    (true, false)
                }
            }
        };
        if successful_shutdown {
            debug_assert!(dashboard_shutdown_attempt || matches!(role, ClientRole::Control));
            state.shutdown.store(true, Ordering::Release);
            wake_accept(&state);
        }
        if !delivered {
            break;
        }
        if matches!(role, ClientRole::Control) {
            break;
        }
    }
    {
        let mut slot = state.dashboard_slot.lock().unwrap();
        let owns_dashboard = dashboard_identity.as_ref().is_some_and(|identity| {
            slot.as_ref()
                .is_some_and(|current| Arc::ptr_eq(&current.identity, identity))
        });
        if owns_dashboard {
            if let Some(current) = slot.take() {
                current.sink.close();
            }
            *state.dashboard.lock().unwrap() = None;
            *state.selected.lock().unwrap() = None;
            clear_dashboard_geometry(&state, dashboard_identity.as_ref().unwrap());
        }
    }
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
        Request::CloseTerminal { session } => state
            .close_terminal(session, requested_kill_grace())
            .map_or_else(error_for_lifecycle, |_| {
                dashboard_try_send_arc(
                    state,
                    ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                );
                Response::Ok
            }),
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
            let (sender, receiver) = mpsc::sync_channel(1);
            if state
                .dispatch
                .send(DispatchMessage::Select {
                    request_id,
                    session,
                    size,
                    completion: sender,
                })
                .is_err()
            {
                return error_response(ErrorCode::Internal, "dispatcher is unavailable");
            }
            let _ = receiver.recv();
            Response::Ok
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
        Request::Input { session, bytes } => {
            if !dashboard || state.selected.lock().unwrap().as_ref() != Some(&session) {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "input is only accepted for the selected dashboard session",
                );
            }
            let selected_session = state.sessions.lock().unwrap().get(&session).cloned();
            match selected_session {
                Some(session) => session.write(&bytes).map_or_else(
                    |error| error_response(input_error_code(&error), error_chain_string(&error)),
                    |_| Response::Ok,
                ),
                None => error_response(
                    ErrorCode::NotFound,
                    format!("session {} not found", session.0),
                ),
            }
        }
        Request::Resize { session, size } => {
            if !dashboard || state.selected.lock().unwrap().as_ref() != Some(&session) {
                return error_response(
                    ErrorCode::InvalidRequest,
                    "resize is only accepted for the selected dashboard session",
                );
            }
            let geometry_owner = dashboard_snapshot(state).map(|snapshot| snapshot.identity);
            let selected_session = state.sessions.lock().unwrap().get(&session).cloned();
            match selected_session {
                Some(session) => session.resize(size).map_or_else(
                    |error| error_response(ErrorCode::Internal, error.to_string()),
                    |_| {
                        if let Some(owner) = geometry_owner.as_ref() {
                            set_dashboard_geometry(state, owner, size);
                        }
                        Response::Ok
                    },
                ),
                None => error_response(
                    ErrorCode::NotFound,
                    format!("session {} not found", session.0),
                ),
            }
        }
        Request::Shutdown { kill } => state.request_shutdown(kill),
        Request::AddProject {
            name,
            repo,
            workspace_root,
        } => state
            .add_project(name, repo, workspace_root)
            .map_or_else(error_for_lifecycle, |_| {
                dashboard_try_send_arc(
                    state,
                    ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                );
                Response::Ok
            }),
        Request::RemoveProject { name } => {
            state
                .remove_project(&name)
                .map_or_else(error_for_lifecycle, |_| {
                    dashboard_try_send_arc(
                        state,
                        ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                    );
                    Response::Ok
                })
        }
        Request::CreateWorkspace {
            project,
            name,
            branch,
        } => state.create_workspace(project, name, branch).map_or_else(
            |error| lifecycle_response_with_partial_hierarchy(state, error),
            |_| {
                dashboard_try_send_arc(
                    state,
                    ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                );
                Response::Ok
            },
        ),
        Request::RemoveWorkspace { project, name } => {
            state.remove_workspace(&project, &name).map_or_else(
                |error| lifecycle_response_with_partial_hierarchy(state, error),
                |_| {
                    dashboard_try_send_arc(
                        state,
                        ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                    );
                    Response::Ok
                },
            )
        }
        Request::CreateSession(request) => {
            state
                .create_session(request)
                .map_or_else(error_for_lifecycle, |summary| {
                    dashboard_try_send_arc(
                        state,
                        ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                    );
                    Response::CreatedSession(summary)
                })
        }
        Request::KillSession { session } => state
            .kill_session(session, requested_kill_grace())
            .map_or_else(error_for_lifecycle, |_| Response::Ok),
        Request::RemoveSession { session } => {
            state
                .remove_session(session)
                .map_or_else(error_for_lifecycle, |_| {
                    dashboard_try_send_arc(
                        state,
                        ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                    );
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
            let _mutation = state.mutation_lock.lock().unwrap();
            if let Some(snapshot) = dashboard_snapshot(state) {
                set_dashboard_geometry(state, &snapshot.identity, size);
                Response::Ok
            } else {
                error_response(ErrorCode::Conflict, "dashboard is disconnected")
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
    let _ = result.recv();
    Response::Ok
}

pub(super) fn error_for_lifecycle(error: anyhow::Error) -> Response {
    let message = error_chain_string(&error);
    let code = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<LifecycleFailure>())
        .map(|failure| failure.code.clone())
        .unwrap_or(ErrorCode::Conflict);
    error_response(code, message)
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

fn lifecycle_code(error: &anyhow::Error) -> ErrorCode {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<LifecycleFailure>())
        .map_or(ErrorCode::Conflict, |failure| failure.code.clone())
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
        dashboard_try_send_arc(
            state,
            ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
        );
    }
    response
}

fn dashboard_try_send_arc(state: &ServerState, message: ServerMessage) -> bool {
    dashboard_send(state, message, None)
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
    if !kill
        && sessions.iter().any(|session| {
            matches!(
                session.summary().phase,
                SessionPhase::Running | SessionPhase::Paused
            )
        })
    {
        return error_response(ErrorCode::SessionsRemain, "sessions remain");
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

fn requested_kill_grace() -> Duration {
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

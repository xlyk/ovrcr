use super::*;
use crate::session::{SessionEvent, SessionId, TerminalSize};
use ovrcr_protocol::{
    AgentReport, DashboardView, ErrorCode, HistorySnapshotId, PAGE_COLS, PAGE_ROWS, Response,
};

pub enum DispatchMessage {
    Session(SessionEvent),
    AgentReport {
        report: AgentReport,
        completion: std::sync::mpsc::SyncSender<Response>,
    },
    RefreshSession {
        session: SessionId,
    },
    RefreshHierarchy,
    NativeQuota(Box<super::quota::NativeQuotaUpdate>),
    SetView {
        owner: Arc<()>,
        request_id: u64,
        view: DashboardView,
        completion: std::sync::mpsc::SyncSender<DispatchCompletion>,
    },
    History {
        owner: Arc<()>,
        request_id: u64,
        request: HistoryRequest,
        completion: std::sync::mpsc::SyncSender<()>,
    },
    AgentCommand {
        request: Request,
        owner: Option<Arc<()>>,
        completion: SyncSender<Response>,
    },
    AgentDisconnected {
        session: SessionId,
        owner: Arc<()>,
    },
    Stop,
}

pub enum DispatchCompletion {
    Complete,
    Terminal(Receiver<Result<(), String>>),
}

pub enum HistoryRequest {
    Begin {
        session: SessionId,
    },
    Page {
        session: SessionId,
        snapshot: HistorySnapshotId,
        start_row: u32,
        rows: u16,
        start_col: u16,
        cols: u16,
    },
    End {
        session: SessionId,
        snapshot: HistorySnapshotId,
    },
}
pub(super) fn bridge_events(
    events: ReportingReceiver<SessionEvent>,
    dispatch: ReportingSender<DispatchMessage>,
) {
    while let Ok(event) = events.recv() {
        if dispatch.send(DispatchMessage::Session(event)).is_err() {
            break;
        }
    }
}

pub fn run_dispatcher(state: Arc<ServerState>, commands: ReportingReceiver<DispatchMessage>) {
    while let Ok(command) = commands.recv() {
        match command {
            DispatchMessage::Session(event) => dispatch_session_event(&state, event),
            DispatchMessage::AgentReport { report, completion } => {
                dispatch_agent_report(&state, report, completion)
            }
            DispatchMessage::RefreshSession { session } => {
                dispatch_refresh_session(&state, session)
            }
            DispatchMessage::RefreshHierarchy => dispatch_refresh_hierarchy(&state),
            DispatchMessage::NativeQuota(update) => super::quota::apply(&state, *update),
            DispatchMessage::SetView {
                owner,
                request_id,
                view,
                completion,
            } => dispatch_set_view(&state, &owner, request_id, view, completion),
            DispatchMessage::History {
                owner,
                request_id,
                request,
                completion,
            } => {
                dispatch_history(&state, &owner, request_id, request);
                let _ = completion.send(());
            }
            DispatchMessage::AgentCommand {
                request,
                owner,
                completion,
            } => {
                let id = match &request {
                    Request::ReserveAgent(r) => r.session,
                    Request::MarkReviewed { session, .. } => *session,
                    Request::Supervisor(r) => r.auth.session,
                    Request::AgentStatus { auth, .. } | Request::SupervisorHello(auth) => {
                        auth.session
                    }
                    _ => unreachable!(),
                };
                let session = state.sessions.lock().unwrap().get(&id).cloned();
                let response = match session {
                    Some(session) => {
                        let before = state.session_summary(id);
                        let result = {
                            // Keep the established retained -> Session lock order. The
                            // authenticated operation persists before publishing its receipt.
                            let mut retained = state.retained.lock();
                            session.agent_command(&request, owner.as_ref(), |command| match command
                            {
                                ovrcr_protocol::AgentCommand::RetainConversation {
                                    reference,
                                    ..
                                } => {
                                    retained.retain_conversation(id, session.run(), Some(reference))
                                }
                                ovrcr_protocol::AgentCommand::InvalidateConversation => {
                                    retained.retain_conversation(id, session.run(), None)
                                }
                                _ => Ok(()),
                            })
                        };
                        if state.session_summary(id) != before {
                            publish_session_changed(&state, id);
                        }
                        match result {
                            Ok(response) => response,
                            Err(error) => error_for_lifecycle(error),
                        }
                    }
                    None => error_response(ErrorCode::NotFound, "session not found"),
                };
                let _ = completion.send(response);
            }
            DispatchMessage::AgentDisconnected { session, owner } => {
                let session = state.sessions.lock().unwrap().get(&session).cloned();
                if let Some(session) = session
                    && session.agent_supervisor_disconnected(&owner)
                {
                    publish_session_changed(&state, session.id());
                }
            }
            DispatchMessage::Stop => break,
        }
    }
}

fn dispatch_history(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    request: HistoryRequest,
) {
    match request {
        HistoryRequest::Begin { session } => {
            dispatch_history_begin(state, owner, request_id, session)
        }
        HistoryRequest::Page {
            session,
            snapshot,
            start_row,
            rows,
            start_col,
            cols,
        } => dispatch_history_page(
            state, owner, request_id, session, snapshot, start_row, rows, start_col, cols,
        ),
        HistoryRequest::End { session, snapshot } => {
            dispatch_history_end(state, owner, request_id, session, snapshot)
        }
    }
}

fn dispatch_history_begin(state: &ServerState, owner: &Arc<()>, request_id: u64, id: SessionId) {
    if !state.dashboard.owns(owner) {
        return;
    }
    let Some(snapshot) = state.dashboard.with_owned(owner, |current| {
        current.next_history_id.checked_add(1).map(|next| {
            let snapshot = HistorySnapshotId(current.next_history_id);
            current.next_history_id = next;
            current.history.take();
            snapshot
        })
    }) else {
        return;
    };
    let Some(snapshot) = snapshot else {
        send_history_response(
            state,
            owner,
            request_id,
            error_response(ErrorCode::Internal, "history snapshot id overflow"),
        );
        return;
    };

    let session = state.sessions.lock().unwrap().get(&id).cloned();
    let Some(session) = session else {
        send_history_response(
            state,
            owner,
            request_id,
            error_response(ErrorCode::NotFound, format!("session {} not found", id.0)),
        );
        return;
    };
    let history = match session.capture_history(snapshot) {
        Ok(history) => history,
        Err(error) => {
            let code = if error.to_string().contains("alternate screen") {
                ErrorCode::Conflict
            } else {
                ErrorCode::Internal
            };
            send_history_response(
                state,
                owner,
                request_id,
                error_response(code, error.to_string()),
            );
            return;
        }
    };
    let opened = history.opened().clone();
    let install = {
        let sessions = state.sessions.lock().unwrap();
        if !sessions
            .get(&id)
            .is_some_and(|registered| Arc::ptr_eq(registered, &session))
        {
            Err(())
        } else {
            state
                .dashboard
                .with_owned(owner, |current| current.history = Some(history))
                .ok_or(())
        }
    };
    match install {
        Ok(()) => send_history_response(state, owner, request_id, Response::HistoryOpened(opened)),
        Err(()) if state.dashboard.owns(owner) => send_history_response(
            state,
            owner,
            request_id,
            error_response(ErrorCode::NotFound, format!("session {} not found", id.0)),
        ),
        Err(()) => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_history_page(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    session: SessionId,
    snapshot: HistorySnapshotId,
    start_row: u32,
    rows: u16,
    start_col: u16,
    cols: u16,
) {
    if rows == 0 || cols == 0 || rows > PAGE_ROWS || cols > PAGE_COLS {
        send_history_response(
            state,
            owner,
            request_id,
            error_response(
                ErrorCode::InvalidRequest,
                "history page dimensions are invalid",
            ),
        );
        return;
    }
    if start_row.checked_add(u32::from(rows)).is_none() || start_col.checked_add(cols).is_none() {
        send_history_response(
            state,
            owner,
            request_id,
            error_response(ErrorCode::InvalidRequest, "history page range overflows"),
        );
        return;
    }
    let response = state
        .dashboard
        .with_owned(owner, |current| match current.history.as_mut() {
            None => error_response(ErrorCode::NotFound, "history snapshot not found"),
            Some(history)
                if history.opened().session != session || history.opened().snapshot != snapshot =>
            {
                error_response(ErrorCode::NotFound, "history snapshot not found")
            }
            Some(history) if start_row > history.opened().total_rows => error_response(
                ErrorCode::InvalidRequest,
                "history row start is out of bounds",
            ),
            Some(history) => match history.page(start_row, rows, start_col, cols) {
                Ok(page) => Response::HistoryRows(page),
                Err(error) => error_response(ErrorCode::InvalidRequest, error.to_string()),
            },
        });
    let Some(response) = response else { return };
    send_history_response(state, owner, request_id, response);
}

fn dispatch_history_end(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    session: SessionId,
    snapshot: HistorySnapshotId,
) {
    if state
        .dashboard
        .with_owned(owner, |current| {
            if current.history.as_ref().is_some_and(|history| {
                history.opened().session == session && history.opened().snapshot == snapshot
            }) {
                current.history.take();
            }
        })
        .is_none()
    {
        return;
    }
    send_history_response(state, owner, request_id, Response::Ok);
}

fn send_history_response(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    response: Response,
) {
    let _ = state
        .dashboard
        .send_owner(owner, response_message(request_id, response));
}

fn dispatch_agent_report(
    state: &Arc<ServerState>,
    report: AgentReport,
    completion: SyncSender<Response>,
) {
    let session = state.sessions.lock().unwrap().get(&report.session).cloned();
    let response = match session {
        Some(session) => match session.apply_agent_report(&report) {
            Ok(changed) => {
                if changed {
                    publish_session_changed(state, report.session);
                }
                Response::Ok
            }
            Err(error) => error_for_lifecycle(error),
        },
        None => error_response(ErrorCode::NotFound, "session not found"),
    };
    let _ = completion.send(response);
}

fn persist_session_exit(state: &ServerState, session: &Session) {
    if let Err(error) = state.persist_session_exit(session) {
        eprintln!("persist session {} exit: {error:#}", session.id().0);
    }
}

pub(super) fn publish_session_changed(state: &ServerState, id: SessionId) {
    state.refresh_claude_quota();
    if let Some(summary) = state.session_summary(id) {
        state
            .dashboard
            .try_send(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
                summary,
            ))));
    }
}

fn dispatch_session_event(state: &Arc<ServerState>, event: SessionEvent) {
    let (id, run) = match &event {
        SessionEvent::Output { id, run, .. }
        | SessionEvent::Exited { id, run, .. }
        | SessionEvent::RestoreInputFailed { id, run, .. } => (*id, *run),
    };
    let session = state.sessions.lock().unwrap().get(&id).cloned();
    let Some(session) = session else { return };
    if session.run() != run {
        return;
    }
    if let SessionEvent::RestoreInputFailed { message, .. } = &event {
        if let Err(error) = state
            .retained
            .lock()
            .record_failure(id, run, message.clone())
        {
            eprintln!("retain restore input failure: {error:#}");
        }
        publish_session_changed(state, id);
        return;
    }
    let output = match &event {
        SessionEvent::Output { bytes, .. } => Some(bytes.clone()),
        SessionEvent::Exited { .. } | SessionEvent::RestoreInputFailed { .. } => None,
    };
    session.apply_event(event);
    if let Some(bytes) = output {
        let revision = state.dashboard.view().and_then(|view| {
            view.panes
                .iter()
                .any(|pane| pane.session == id && pane.run == run)
                .then_some(view.revision)
        });
        if let Some(revision) = revision {
            state
                .dashboard
                .try_send(ServerMessage::Event(ServerEvent::Output {
                    session: id,
                    run,
                    revision,
                    bytes,
                }));
        }
    } else {
        persist_session_exit(state, &session);
        publish_session_changed(state, id);
        state
            .dashboard
            .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
                snapshot(state),
            )));
    }
}

fn dispatch_refresh_session(state: &Arc<ServerState>, id: SessionId) {
    publish_session_changed(state, id);
}

fn dispatch_refresh_hierarchy(state: &ServerState) {
    if !state.dashboard.is_claimed() || state.stopping.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    state
        .dashboard
        .try_send(ServerMessage::Event(ServerEvent::HierarchyChanged(
            snapshot_from_state(state),
        )));
}

fn view_error(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    code: ErrorCode,
    message: impl Into<String>,
) {
    let _ = state.dashboard.send_owner(
        owner,
        response_message(
            request_id,
            Response::Error {
                code,
                message: message.into(),
            },
        ),
    );
}

fn dispatch_set_view(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    view: DashboardView,
    completion: SyncSender<DispatchCompletion>,
) {
    #[cfg(test)]
    if let Some(resize) = state.resize_hook.lock().unwrap().clone() {
        dispatch_set_view_with_resize(
            state,
            owner,
            request_id,
            view,
            completion,
            move |session, size| resize(session, size),
        );
        return;
    }
    dispatch_set_view_with_resize(
        state,
        owner,
        request_id,
        view,
        completion,
        |session, size| session.resize(size),
    );
}

enum ViewPane {
    Live(Arc<Session>),
    Retained,
}

fn resolve_view_pane(
    state: &ServerState,
    pane: &ovrcr_protocol::PaneTarget,
) -> Result<ViewPane, String> {
    let live = state.sessions.lock().unwrap().get(&pane.session).cloned();
    if let Some(session) = live {
        if session.run() != pane.run {
            return Err(format!("session {} not found", pane.session.0));
        }
        return Ok(ViewPane::Live(session));
    }
    match state.session_summary(pane.session) {
        Some(summary) if summary.run == pane.run => Ok(ViewPane::Retained),
        _ => Err(format!("session {} not found", pane.session.0)),
    }
}

fn dispatch_set_view_with_resize(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    view: DashboardView,
    completion: SyncSender<DispatchCompletion>,
    mut resize: impl FnMut(&Session, TerminalSize) -> anyhow::Result<()>,
) {
    if !state.dashboard.owns(owner) {
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    }
    if let Err(error) = view.validate() {
        view_error(state, owner, request_id, ErrorCode::InvalidRequest, error);
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    }
    let previous = state.dashboard.view();
    if previous
        .as_ref()
        .is_some_and(|current| view.revision <= current.revision)
    {
        view_error(
            state,
            owner,
            request_id,
            ErrorCode::InvalidRequest,
            "view revision must increase",
        );
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    }
    let mut resolved = Vec::with_capacity(view.panes.len());
    for pane in &view.panes {
        match resolve_view_pane(state, pane) {
            Ok(target) => resolved.push(target),
            Err(message) => {
                view_error(state, owner, request_id, ErrorCode::NotFound, message);
                let _ = completion.send(DispatchCompletion::Complete);
                return;
            }
        }
    }
    let targets = view
        .panes
        .iter()
        .zip(resolved.iter())
        .filter_map(|(pane, target)| {
            let ViewPane::Live(session) = target else {
                return None;
            };
            let previous_pane = previous.as_ref().and_then(|current| {
                current
                    .panes
                    .iter()
                    .find(|current| current.session == pane.session)
            });
            if previous_pane
                .is_some_and(|current| current.run == pane.run && current.size == pane.size)
            {
                None
            } else {
                Some((Arc::clone(session), pane.size))
            }
        })
        .collect::<Vec<_>>();
    let mut resized = 0;
    let resize_result = resize_view_targets(&targets, |session, size| {
        let result = resize(session, size);
        if result.is_ok() {
            resized += 1;
        }
        result
    });
    if let Err(error) = resize_result {
        if resized > 0 {
            state
                .dashboard
                .clear_view_for(owner, previous.as_ref().map(|view| view.revision));
            let (terminal_sender, terminal_receiver) = mpsc::sync_channel(1);
            let queued = state.dashboard.send_owner_terminal(
                owner,
                response_message(
                    request_id,
                    Response::Error {
                        code: ErrorCode::PartialFailure,
                        message: error.to_string(),
                    },
                ),
                Some(terminal_sender),
            );
            if queued {
                let _ = completion.send(DispatchCompletion::Terminal(terminal_receiver));
            } else {
                let _ = completion.send(DispatchCompletion::Complete);
            }
        } else {
            view_error(
                state,
                owner,
                request_id,
                ErrorCode::Internal,
                error.to_string(),
            );
            let _ = completion.send(DispatchCompletion::Complete);
        }
        return;
    }
    let Some(snapshot) = state.dashboard.snapshot_owned(owner) else {
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    };
    let mut screens = resolved
        .iter()
        .map(|target| match target {
            ViewPane::Live(session) => session.current_screen(),
            ViewPane::Retained => Vec::new(),
        })
        .collect::<Vec<_>>();
    // Panes were resolved before the resizes, which can block for as long as a
    // PTY takes, and `remove_session` runs on connection threads. Hold retained
    // then the session registry from this membership re-check through
    // publication so a row removed in between cannot be published as a focused
    // pane at a valid revision and then admit input that only fails at the PTY.
    let retained = state.retained.lock();
    let registered = state.sessions.lock().unwrap();
    let mut published = view.clone();
    published.panes.retain(|pane| {
        let Some(index) = view
            .panes
            .iter()
            .position(|requested| requested.session == pane.session)
        else {
            return false;
        };
        match &resolved[index] {
            ViewPane::Live(captured) => registered
                .get(&pane.session)
                .is_some_and(|session| Arc::ptr_eq(session, captured) && session.run() == pane.run),
            ViewPane::Retained => match registered.get(&pane.session) {
                Some(session) if session.run() == pane.run => {
                    screens[index] = session.current_screen();
                    true
                }
                Some(_) => false,
                None => retained
                    .get(pane.session)
                    .is_some_and(|record| record.run == pane.run),
            },
        }
    });
    if let Some(focused) = view
        .focused
        .filter(|focused| !published.panes.iter().any(|pane| pane.session == *focused))
    {
        drop(registered);
        drop(retained);
        view_error(
            state,
            owner,
            request_id,
            ErrorCode::NotFound,
            format!("session {} not found", focused.0),
        );
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    }
    // The client still gets one snapshot per pane it asked for; only the
    // published view drops the panes whose sessions are gone, so neither output
    // nor input is admitted for them.
    if !snapshot.sink.replace_view(&view, request_id, screens) {
        state.dashboard.disconnect(snapshot);
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    }
    let focus_changed = previous.as_ref().and_then(|old| old.focused) != view.focused;
    if !state
        .dashboard
        .publish_view(owner, published, focus_changed, || {
            #[cfg(test)]
            if let Some(hook) = state.before_view_publish_hook.lock().unwrap().clone() {
                hook();
            }
        })
    {
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    }
    drop(registered);
    drop(retained);
    state.refresh_claude_quota();
    state.publish_quotas();
    let _ = completion.send(DispatchCompletion::Complete);
}

pub(super) fn resize_view_targets(
    targets: &[(Arc<Session>, TerminalSize)],
    mut resize: impl FnMut(&Session, TerminalSize) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    for (session, size) in targets {
        resize(session, *size)?;
    }
    Ok(())
}

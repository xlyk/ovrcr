use super::*;
use crate::session::{SessionEvent, SessionId, TerminalSize};
use ovrcr_protocol::{
    AgentReport, DashboardView, ErrorCode, HistorySnapshotId, PAGE_COLS, PAGE_ROWS, PaneTarget,
    Response,
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
    Select {
        request_id: u64,
        session: SessionId,
        size: TerminalSize,
        completion: std::sync::mpsc::SyncSender<DispatchCompletion>,
    },
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
pub(super) fn bridge_events(events: Receiver<SessionEvent>, dispatch: SyncSender<DispatchMessage>) {
    while let Ok(event) = events.recv() {
        if dispatch.send(DispatchMessage::Session(event)).is_err() {
            break;
        }
    }
}

pub fn run_dispatcher(state: Arc<ServerState>, commands: Receiver<DispatchMessage>) {
    while let Ok(command) = commands.recv() {
        match command {
            DispatchMessage::Session(event) => dispatch_session_event(&state, event),
            DispatchMessage::AgentReport { report, completion } => {
                dispatch_agent_report(&state, report, completion)
            }
            DispatchMessage::RefreshSession { session } => {
                dispatch_refresh_session(&state, session)
            }
            DispatchMessage::Select {
                request_id,
                session,
                size,
                completion,
            } => dispatch_select(&state, request_id, session, size, completion),
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
    if !dashboard_owner_matches(state, owner) {
        return;
    }
    let snapshot = {
        let mut slot = state.dashboard_slot.lock().unwrap();
        let Some(current) = slot.as_ref() else { return };
        if !Arc::ptr_eq(&current.identity, owner) {
            return;
        }
        match current.next_history_id.checked_add(1) {
            Some(next) => {
                let current = slot.as_mut().unwrap();
                let snapshot = HistorySnapshotId(current.next_history_id);
                current.next_history_id = next;
                current.history.take();
                Some(snapshot)
            }
            None => None,
        }
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
            let mut slot = state.dashboard_slot.lock().unwrap();
            if slot
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(&current.identity, owner))
            {
                slot.as_mut().unwrap().history = Some(history);
                Ok(())
            } else {
                Err(())
            }
        }
    };
    match install {
        Ok(()) => send_history_response(state, owner, request_id, Response::HistoryOpened(opened)),
        Err(()) if dashboard_owner_matches(state, owner) => send_history_response(
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
    let response = {
        let mut slot = state.dashboard_slot.lock().unwrap();
        let Some(current) = slot.as_mut() else { return };
        if !Arc::ptr_eq(&current.identity, owner) {
            return;
        }
        match current.history.as_mut() {
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
        }
    };
    send_history_response(state, owner, request_id, response);
}

fn dispatch_history_end(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    session: SessionId,
    snapshot: HistorySnapshotId,
) {
    let mut slot = state.dashboard_slot.lock().unwrap();
    let Some(current) = slot.as_mut() else { return };
    if !Arc::ptr_eq(&current.identity, owner) {
        return;
    }
    if current.history.as_ref().is_some_and(|history| {
        history.opened().session == session && history.opened().snapshot == snapshot
    }) {
        current.history.take();
    }
    drop(slot);
    send_history_response(state, owner, request_id, Response::Ok);
}

fn send_history_response(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    response: Response,
) {
    let _ = dashboard_send_owner(state, owner, response_message(request_id, response));
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
                    dashboard_try_send(
                        state,
                        ServerMessage::Event(ServerEvent::SessionChanged(Box::new(
                            session.summary(),
                        ))),
                    );
                }
                Response::Ok
            }
            Err(error) => error_for_lifecycle(error),
        },
        None => error_response(ErrorCode::NotFound, "session not found"),
    };
    let _ = completion.send(response);
}

fn dispatch_session_event(state: &Arc<ServerState>, event: SessionEvent) {
    let id = match &event {
        SessionEvent::Output { id, .. } | SessionEvent::Exited { id, .. } => *id,
    };
    let session = state.sessions.lock().unwrap().get(&id).cloned();
    let Some(session) = session else { return };
    let output = match &event {
        SessionEvent::Output { bytes, .. } => Some(bytes.clone()),
        SessionEvent::Exited { .. } => None,
    };
    session.apply_event(event);
    if let Some(bytes) = output {
        let revision = state
            .view
            .lock()
            .unwrap()
            .as_ref()
            .filter(|view| view.panes.iter().any(|pane| pane.session == id))
            .map(|view| view.revision);
        if let Some(revision) = revision {
            dashboard_try_send(
                state,
                ServerMessage::Event(ServerEvent::Output {
                    session: id,
                    revision,
                    bytes,
                }),
            );
        }
    } else {
        dashboard_try_send(
            state,
            ServerMessage::Event(ServerEvent::SessionChanged(Box::new(session.summary()))),
        );
        dashboard_try_send(
            state,
            ServerMessage::Event(ServerEvent::HierarchyChanged(snapshot(state))),
        );
    }
}

fn dispatch_refresh_session(state: &Arc<ServerState>, id: SessionId) {
    let session = state.sessions.lock().unwrap().get(&id).cloned();
    let Some(session) = session else { return };
    dashboard_try_send(
        state,
        ServerMessage::Event(ServerEvent::SessionChanged(Box::new(session.summary()))),
    );
}

fn dispatch_select(
    state: &ServerState,
    request_id: u64,
    id: SessionId,
    size: TerminalSize,
    completion: SyncSender<DispatchCompletion>,
) {
    let Some(snapshot) = dashboard_snapshot(state) else {
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    };
    let revision = state
        .view
        .lock()
        .unwrap()
        .as_ref()
        .map_or(1, |view| view.revision.saturating_add(1));
    dispatch_set_view(
        state,
        &snapshot.identity,
        request_id,
        DashboardView {
            revision,
            panes: vec![PaneTarget { session: id, size }],
            focused: Some(id),
        },
        completion,
    );
}

fn view_error(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    code: ErrorCode,
    message: impl Into<String>,
) {
    let _ = dashboard_send_owner(
        state,
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

fn dispatch_set_view_with_resize(
    state: &ServerState,
    owner: &Arc<()>,
    request_id: u64,
    view: DashboardView,
    completion: SyncSender<DispatchCompletion>,
    mut resize: impl FnMut(&Session, TerminalSize) -> anyhow::Result<()>,
) {
    if !dashboard_owner_matches(state, owner) {
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    }
    if let Err(error) = view.validate() {
        view_error(state, owner, request_id, ErrorCode::InvalidRequest, error);
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    }
    let previous = state.view.lock().unwrap().clone();
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
    let sessions = {
        let sessions = state.sessions.lock().unwrap();
        let mut resolved = Vec::with_capacity(view.panes.len());
        for pane in &view.panes {
            let Some(session) = sessions.get(&pane.session).cloned() else {
                view_error(
                    state,
                    owner,
                    request_id,
                    ErrorCode::NotFound,
                    format!("session {} not found", pane.session.0),
                );
                let _ = completion.send(DispatchCompletion::Complete);
                return;
            };
            resolved.push((session, pane.size));
        }
        resolved
    };
    let targets = sessions
        .iter()
        .filter(|(session, size)| {
            previous
                .as_ref()
                .and_then(|current| {
                    current
                        .panes
                        .iter()
                        .find(|pane| pane.session == session.summary().id)
                })
                .is_none_or(|pane| pane.size != *size)
        })
        .cloned()
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
            clear_view_subscription_for_owner(
                state,
                owner,
                previous.as_ref().map(|view| view.revision),
            );
            let (terminal_sender, terminal_receiver) = mpsc::sync_channel(1);
            let queued = dashboard_send_owner_terminal(
                state,
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
    let Some(snapshot) =
        dashboard_snapshot(state).filter(|snapshot| Arc::ptr_eq(&snapshot.identity, owner))
    else {
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    };
    let screens = sessions
        .iter()
        .map(|(session, _)| session.current_screen())
        .collect::<Vec<_>>();
    // Panes were resolved before the resizes, which can block for as long as a
    // PTY takes, and `remove_session` runs on connection threads. Hold the
    // session registry from this membership re-check through publication so a
    // session removed in between cannot be published as a focused pane at a
    // valid revision and then admit input that only fails at the PTY.
    let registered = state.sessions.lock().unwrap();
    if let Some(focused) = view
        .focused
        .filter(|focused| !registered.contains_key(focused))
    {
        drop(registered);
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
    let mut published = view.clone();
    published
        .panes
        .retain(|pane| registered.contains_key(&pane.session));
    if !snapshot.sink.replace_view(&view, request_id, screens) {
        disconnect_dashboard(state, snapshot);
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    }
    let focus_changed = previous.as_ref().and_then(|old| old.focused) != view.focused;
    let mut slot = state.dashboard_slot.lock().unwrap();
    let Some(current) = slot
        .as_mut()
        .filter(|current| Arc::ptr_eq(&current.identity, owner) && !current.sink.is_closing())
    else {
        let _ = completion.send(DispatchCompletion::Complete);
        return;
    };
    #[cfg(test)]
    if let Some(hook) = state.before_view_publish_hook.lock().unwrap().clone() {
        hook();
    }
    if focus_changed {
        current.history.take();
    }
    *state.view.lock().unwrap() = Some(published.clone());
    if let Some(focused) = published.focused
        && let Some(pane) = published.panes.iter().find(|pane| pane.session == focused)
    {
        set_dashboard_geometry(state, owner, pane.size);
    }
    drop(registered);
    let _ = completion.send(DispatchCompletion::Complete);
}

pub(super) fn clear_view_subscription(state: &ServerState, fallback_revision: Option<u64>) {
    let mut view = state.view.lock().unwrap();
    match view.as_mut() {
        Some(current) => {
            current.panes.clear();
            current.focused = None;
        }
        None => {
            if let Some(revision) = fallback_revision {
                *view = Some(DashboardView {
                    revision,
                    panes: Vec::new(),
                    focused: None,
                });
            }
        }
    }
}

fn clear_view_subscription_for_owner(
    state: &ServerState,
    owner: &Arc<()>,
    fallback_revision: Option<u64>,
) {
    let slot = state.dashboard_slot.lock().unwrap();
    if !slot
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(&current.identity, owner))
    {
        return;
    }
    let mut view = state.view.lock().unwrap();
    match view.as_mut() {
        Some(current) => {
            current.panes.clear();
            current.focused = None;
        }
        None => {
            if let Some(revision) = fallback_revision {
                *view = Some(DashboardView {
                    revision,
                    panes: Vec::new(),
                    focused: None,
                });
            }
        }
    }
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

pub(super) fn set_dashboard_geometry(state: &ServerState, owner: &Arc<()>, size: TerminalSize) {
    *state.dashboard_size.lock().unwrap() = Some(DashboardGeometry {
        owner: Arc::clone(owner),
        size,
    });
}

pub(super) fn clear_dashboard_geometry(state: &ServerState, owner: &Arc<()>) {
    let mut geometry = state.dashboard_size.lock().unwrap();
    if geometry
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(&current.owner, owner))
    {
        *geometry = None;
    }
}

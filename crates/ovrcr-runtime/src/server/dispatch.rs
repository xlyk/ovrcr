use super::*;
use crate::session::{SessionEvent, SessionId, TerminalSize};
use ovrcr_protocol::{AgentReport, ErrorCode, HistorySnapshotId, PAGE_COLS, PAGE_ROWS, Response};

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
        completion: std::sync::mpsc::SyncSender<()>,
    },
    History {
        owner: Arc<()>,
        request_id: u64,
        request: HistoryRequest,
        completion: std::sync::mpsc::SyncSender<()>,
    },
    Stop,
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
        if state.selected.lock().unwrap().as_ref() == Some(&id) {
            dashboard_try_send(
                state,
                ServerMessage::Event(ServerEvent::Output { session: id, bytes }),
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
    completion: SyncSender<()>,
) {
    let session = state.sessions.lock().unwrap().get(&id).cloned();
    let Some(session) = session else {
        dashboard_try_send(
            state,
            response_message(
                request_id,
                Response::Error {
                    code: ErrorCode::NotFound,
                    message: format!("session {} not found", id.0),
                },
            ),
        );
        let _ = completion.send(());
        return;
    };
    if let Err(error) = session.resize(size) {
        dashboard_try_send(
            state,
            response_message(
                request_id,
                Response::Error {
                    code: ErrorCode::Internal,
                    message: error.to_string(),
                },
            ),
        );
        let _ = completion.send(());
        return;
    }
    let previous = *state.selected.lock().unwrap();
    let Some(snapshot) = dashboard_snapshot(state) else {
        let _ = completion.send(());
        return;
    };
    if previous != Some(id)
        && let Some(current) = state.dashboard_slot.lock().unwrap().as_mut()
        && Arc::ptr_eq(&current.identity, &snapshot.identity)
    {
        current.history.take();
    }
    set_dashboard_geometry(state, &snapshot.identity, size);
    if !snapshot
        .sink
        .replace_selection(previous, id, request_id, size, session.current_screen())
    {
        disconnect_dashboard(state, snapshot);
        let _ = completion.send(());
        return;
    }
    *state.selected.lock().unwrap() = Some(id);
    let _ = completion.send(());
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

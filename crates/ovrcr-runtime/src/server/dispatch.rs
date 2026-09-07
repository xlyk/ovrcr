use super::*;
use crate::session::{SessionEvent, SessionId, TerminalSize};
use ovrcr_protocol::{AgentReport, Response};

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
    Stop,
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
            DispatchMessage::Stop => break,
        }
    }
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
                        ServerMessage::Event(ServerEvent::SessionChanged(session.summary())),
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
            ServerMessage::Event(ServerEvent::SessionChanged(session.summary())),
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
        ServerMessage::Event(ServerEvent::SessionChanged(session.summary())),
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

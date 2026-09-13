//! One request, one matched response. The only place a caller learns request
//! ids, event interleaving, and which `Response` variant answers a `Request`.
use crate::{
    ClientMessage, ErrorCode, HierarchySnapshot, Registry, Request, Response, ServerMessage,
    SessionSummary, read_frame, write_frame,
};
use anyhow::{Result, bail};
use std::io::{Read, Write};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerError {
    pub code: ErrorCode,
    pub message: String,
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ServerError {}

/// Send `request` under `request_id` and return the first response frame that
/// carries that id. Events and responses to other ids are skipped.
pub fn request<S: Read + Write>(io: &mut S, request_id: u64, request: Request) -> Result<Response> {
    write_frame(
        io,
        &ClientMessage {
            request_id,
            request,
        },
    )?;
    loop {
        match read_frame::<ServerMessage>(io)? {
            ServerMessage::Response {
                request_id: id,
                response,
            } if id == request_id => {
                return Ok(response);
            }
            ServerMessage::Response { .. } | ServerMessage::Event(_) => {}
        }
    }
}

pub fn expect_ok(response: Response) -> Result<()> {
    match response {
        Response::Ok => Ok(()),
        Response::Error { code, message } => Err(ServerError { code, message }.into()),
        other => bail!("unexpected server response: {other:?}"),
    }
}

pub fn list<S: Read + Write>(io: &mut S, request_id: u64) -> Result<HierarchySnapshot> {
    match request(io, request_id, Request::List)? {
        Response::Hierarchy(snapshot) => Ok(snapshot),
        Response::Error { code, message } => Err(ServerError { code, message }.into()),
        other => bail!("unexpected server response: {other:?}"),
    }
}

pub fn inspect<S: Read + Write>(
    io: &mut S,
    request_id: u64,
) -> Result<(Registry, Vec<SessionSummary>)> {
    match request(io, request_id, Request::Inspect)? {
        Response::Inventory { registry, sessions } => Ok((registry, sessions)),
        Response::Error { code, message } => Err(ServerError { code, message }.into()),
        other => bail!("unexpected server response: {other:?}"),
    }
}

pub fn shutdown<S: Read + Write>(io: &mut S, request_id: u64, kill: bool) -> Result<()> {
    expect_ok(request(io, request_id, Request::Shutdown { kill })?)
}

pub fn session_count(snapshot: &HierarchySnapshot) -> usize {
    snapshot
        .projects
        .iter()
        .flat_map(|project| project.workspaces.iter())
        .map(|workspace| workspace.sessions.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientMessage, ServerEvent, ServerMessage, SessionId};
    use std::os::unix::net::UnixStream;
    use std::thread;

    /// Answer one request on `server` with the frames in `reply`, in order.
    fn responder(
        mut server: UnixStream,
        reply: Vec<ServerMessage>,
    ) -> thread::JoinHandle<ClientMessage> {
        thread::spawn(move || {
            let received = crate::read_frame::<ClientMessage>(&mut server).unwrap();
            for frame in reply {
                crate::write_frame(&mut server, &frame).unwrap();
            }
            received
        })
    }

    #[test]
    fn request_skips_events_and_foreign_ids() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let handle = responder(
            server,
            vec![
                ServerMessage::Event(ServerEvent::ScreenDirty {
                    session: SessionId(1),
                    revision: 1,
                }),
                ServerMessage::Response {
                    request_id: 99,
                    response: Response::Ok,
                },
                ServerMessage::Response {
                    request_id: 7,
                    response: Response::Hierarchy(HierarchySnapshot {
                        projects: Vec::new(),
                    }),
                },
            ],
        );
        let response = request(&mut client, 7, Request::List).unwrap();
        assert_eq!(
            response,
            Response::Hierarchy(HierarchySnapshot {
                projects: Vec::new()
            })
        );
        assert_eq!(
            handle.join().unwrap(),
            ClientMessage {
                request_id: 7,
                request: Request::List
            }
        );
    }

    #[test]
    fn list_returns_hierarchy_and_rejects_inventory() {
        let (mut client, server) = UnixStream::pair().unwrap();
        responder(
            server,
            vec![ServerMessage::Response {
                request_id: 1,
                response: Response::Inventory {
                    registry: Registry::default(),
                    sessions: Vec::new(),
                },
            }],
        );
        let error = list(&mut client, 1).unwrap_err().to_string();
        assert!(error.contains("unexpected server response"), "{error}");
    }

    #[test]
    fn expect_ok_surfaces_server_error_with_code() {
        let error = expect_ok(Response::Error {
            code: ErrorCode::SessionsRemain,
            message: "sessions remain".into(),
        })
        .unwrap_err();
        let server = error.downcast_ref::<ServerError>().expect("ServerError");
        assert_eq!(server.code, ErrorCode::SessionsRemain);
        assert_eq!(server.message, "sessions remain");
        assert!(expect_ok(Response::Ok).is_ok());
    }

    #[test]
    fn peer_close_before_response_is_an_error() {
        let (mut client, server) = UnixStream::pair().unwrap();
        drop(server);
        assert!(request(&mut client, 1, Request::List).is_err());
    }
}

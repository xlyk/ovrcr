//! Loopback remote gateway. Speaks the OVRCR protocol directly (no CLI child),
//! serves the PWA and its assets, and keeps Tailscale as an optional outer layer.

pub mod http;

use crate::client::connect_if_running;
use crate::server::ServerPaths;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use ovrcr_protocol::client as protocol_client;
use ovrcr_protocol::{
    AgentActivity, ConversationReference, ErrorCode, ReadyObservation, Request, Response,
    SessionId, SessionPhase, SessionRunId, SessionSummary,
};
use serde_json::{Value, json};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 8787;
const CACHE_TTL: Duration = Duration::from_secs(5);
const MAX_TEXT: usize = 32_000;
const MAX_SCREEN_ROWS: u16 = 128;
const MAX_SCREEN_COLS: u16 = 256;

// The prototype's PWA is the source of truth while the port settles. These are
// embedded so the installed binary never depends on the checkout.
const INDEX: &[u8] = include_bytes!("../../experiments/remote-web/public/index.html");
const APP_JS: &[u8] = include_bytes!("../../experiments/remote-web/public/app.js");
const STYLES: &[u8] = include_bytes!("../../experiments/remote-web/public/styles.css");
const FONT_REGULAR: &[u8] =
    include_bytes!("../../experiments/remote-web/public/fonts/jetbrains-mono-nerd-regular.woff2");
const FONT_BOLD: &[u8] =
    include_bytes!("../../experiments/remote-web/public/fonts/jetbrains-mono-nerd-bold.woff2");
const FONT_ITALIC: &[u8] =
    include_bytes!("../../experiments/remote-web/public/fonts/jetbrains-mono-nerd-italic.woff2");
const FONT_BOLD_ITALIC: &[u8] = include_bytes!(
    "../../experiments/remote-web/public/fonts/jetbrains-mono-nerd-bold-italic.woff2"
);
const FONT_LICENSE: &[u8] = include_bytes!("../../experiments/remote-web/public/fonts/OFL.txt");

#[derive(Args)]
pub struct RemoteArgs {
    #[command(subcommand)]
    pub command: RemoteCommand,
}

#[derive(Subcommand)]
pub enum RemoteCommand {
    /// Serve the remote web UI on loopback.
    Serve {
        #[arg(long)]
        port: Option<u16>,
        /// Tailscale login allowed through Serve; defaults to OVRCR_REMOTE_LOGIN.
        #[arg(long)]
        login: Option<String>,
    },
    /// Report whether a gateway is listening on the port.
    Status {
        #[arg(long)]
        port: Option<u16>,
    },
}

pub fn run(args: RemoteArgs, _json: bool) -> Result<()> {
    match args.command {
        RemoteCommand::Serve { port, login } => serve(port, login),
        RemoteCommand::Status { port } => status(port),
    }
}

pub fn serve(port: Option<u16>, login: Option<String>) -> Result<()> {
    let port = port.unwrap_or(DEFAULT_PORT);
    let login = login
        .or_else(|| std::env::var("OVRCR_REMOTE_LOGIN").ok())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let listener =
        TcpListener::bind(("127.0.0.1", port)).with_context(|| format!("bind 127.0.0.1:{port}"))?;
    let address = listener.local_addr().context("read listener address")?;
    println!("ovrcr remote web listening on http://{address}");
    println!(
        "  tailnet login: {}",
        login.as_deref().unwrap_or("unset (loopback only)")
    );
    let gateway = Arc::new(Gateway::new(login));
    http::serve(listener, move |request| gateway.handle(request))
}

pub fn status(port: Option<u16>) -> Result<()> {
    let port = port.unwrap_or(DEFAULT_PORT);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let running = TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_ok();
    println!("{}", if running { "running" } else { "not running" });
    Ok(())
}

struct Gateway {
    login: Option<String>,
    next_id: AtomicU64,
    cache: Mutex<Option<(Instant, Vec<SessionSummary>)>>,
}

impl Gateway {
    fn new(login: Option<String>) -> Self {
        Self {
            login,
            next_id: AtomicU64::new(1),
            cache: Mutex::new(None),
        }
    }

    fn handle(&self, request: &http::Request) -> http::Response {
        if !self.authorized(request) {
            return http::Response::error(
                403,
                if self.login.is_some() {
                    "this tailnet request is not from the configured OVRCR login"
                } else {
                    "set OVRCR_REMOTE_LOGIN or start with --login to accept tailnet requests"
                },
            );
        }
        match request.method.as_str() {
            "GET" | "HEAD" => self.get(request),
            "POST" => self.post(request),
            _ => http::Response::error(405, "method not allowed"),
        }
    }

    fn authorized(&self, request: &http::Request) -> bool {
        let host = request.header("host").unwrap_or("");
        if !host_is_loopback(host) {
            let Some(login) = &self.login else {
                return false;
            };
            let Some(caller) = request.header("tailscale-user-login") else {
                return false;
            };
            if !caller.eq_ignore_ascii_case(login) {
                return false;
            }
        }
        if request.method != "GET" && request.method != "HEAD" {
            if let Some(origin) = request.header("origin") {
                if origin != format!("https://{host}") && origin != format!("http://{host}") {
                    return false;
                }
            }
        }
        true
    }

    fn get(&self, request: &http::Request) -> http::Response {
        match request.path() {
            "/" | "/index.html" => bytes(INDEX, "text/html; charset=utf-8", false),
            "/app.js" => bytes(APP_JS, "text/javascript; charset=utf-8", false),
            "/styles.css" => bytes(STYLES, "text/css; charset=utf-8", false),
            "/fonts/jetbrains-mono-nerd-regular.woff2" => bytes(FONT_REGULAR, "font/woff2", true),
            "/fonts/jetbrains-mono-nerd-bold.woff2" => bytes(FONT_BOLD, "font/woff2", true),
            "/fonts/jetbrains-mono-nerd-italic.woff2" => bytes(FONT_ITALIC, "font/woff2", true),
            "/fonts/jetbrains-mono-nerd-bold-italic.woff2" => {
                bytes(FONT_BOLD_ITALIC, "font/woff2", true)
            }
            "/fonts/OFL.txt" => bytes(FONT_LICENSE, "text/plain; charset=utf-8", false),
            "/api/state" => self.api_state(),
            _ => self.get_session_route(request),
        }
    }

    fn get_session_route(&self, request: &http::Request) -> http::Response {
        let parts = segments(request.path());
        if parts.len() != 4 || parts[0] != "api" || parts[1] != "session" || parts[3] != "screen" {
            return http::Response::error(404, "not found");
        }
        let Ok(id) = parse_id(parts[2]) else {
            return http::Response::error(400, "session id must be a positive integer");
        };
        let start_row = match query_number(request, "start_row", 0, 1_000_000) {
            Ok(value) => value as u32,
            Err(response) => return response,
        };
        let rows = match query_number(request, "rows", 40, u32::from(MAX_SCREEN_ROWS)) {
            Ok(value) if value >= 1 => value as u16,
            Ok(_) => return http::Response::error(400, "rows must be positive"),
            Err(response) => return response,
        };
        let cols = match query_number(request, "cols", 200, u32::from(MAX_SCREEN_COLS)) {
            Ok(value) if value >= 1 => value as u16,
            Ok(_) => return http::Response::error(400, "cols must be positive"),
            Err(response) => return response,
        };
        match self.call(Request::ReadScreen {
            session: SessionId(id),
            start_row,
            rows,
            cols,
        }) {
            Ok(Response::ScreenWindow(window)) => json_response(&window),
            Ok(Response::Error { code, message }) => error_response(code, message),
            Ok(_) => http::Response::error(502, "unexpected screen response"),
            Err(response) => response,
        }
    }

    fn post(&self, request: &http::Request) -> http::Response {
        let parts = segments(request.path());
        if parts.len() != 4 || parts[0] != "api" || parts[1] != "session" {
            return http::Response::error(404, "not found");
        }
        let Ok(id) = parse_id(parts[2]) else {
            return http::Response::error(400, "session id must be a positive integer");
        };
        let session = SessionId(id);
        let body = match serde_json::from_slice::<Value>(&request.body) {
            Ok(Value::Object(map)) => Value::Object(map),
            Ok(_) => return http::Response::error(400, "request body must be a JSON object"),
            Err(_) => return http::Response::error(400, "request body is not valid JSON"),
        };

        match parts[3] {
            "send" => {
                let Some(text) = body.get("text").and_then(Value::as_str) else {
                    return http::Response::error(400, "text must be a string");
                };
                if text.len() > MAX_TEXT {
                    return http::Response::error(413, "text is too long");
                }
                let submit = body.get("submit").and_then(Value::as_bool).unwrap_or(true);
                self.mutate(Request::SendTerminal {
                    session,
                    text: text.to_owned(),
                    submit,
                })
            }
            "key" => {
                let Some(key) = body.get("key").and_then(Value::as_str) else {
                    return http::Response::error(400, "key must be a string");
                };
                if !valid_key(key) {
                    return http::Response::error(400, "key must look like :escape: or :ctrl-c:");
                }
                let run = match self.session_run(session) {
                    Ok(run) => run,
                    Err(response) => return response,
                };
                self.mutate(Request::Keystroke {
                    session,
                    expected_run: run,
                    key: key.to_owned(),
                })
            }
            "review" => {
                let Some(expected) = body.get("expected") else {
                    return http::Response::error(400, "expected must be the unread object");
                };
                let Ok(expected) = serde_json::from_value::<ReadyObservation>(expected.clone())
                else {
                    return http::Response::error(400, "expected is not a Ready observation");
                };
                self.mutate(Request::MarkReviewed { session, expected })
            }
            "transcript" => {
                let before = match body.get("before") {
                    None | Some(Value::Null) => None,
                    Some(value) => match value.as_u64() {
                        Some(value) => Some(value),
                        None => {
                            return http::Response::error(
                                400,
                                "before must be a non-negative integer",
                            );
                        }
                    },
                };
                let identity = match body.get("identity") {
                    None | Some(Value::Null) => None,
                    Some(value) => {
                        match serde_json::from_value::<ConversationReference>(value.clone()) {
                            Ok(identity) => Some(identity),
                            Err(_) => {
                                return http::Response::error(
                                    400,
                                    "identity is not a conversation reference",
                                );
                            }
                        }
                    }
                };
                let run = match self.session_run(session) {
                    Ok(run) => run,
                    Err(response) => return response,
                };
                match self.call(Request::Transcript {
                    session,
                    run,
                    identity,
                    before,
                }) {
                    Ok(Response::Transcript(page)) => json_response(&page),
                    Ok(Response::Error { code, message }) => error_response(code, message),
                    Ok(_) => http::Response::error(502, "unexpected transcript response"),
                    Err(response) => response,
                }
            }
            "pause" => self.mutate(Request::PauseSession { session }),
            "resume" => self.mutate(Request::ResumeSession { session }),
            "kill" => self.mutate(Request::KillSession { session }),
            _ => http::Response::error(404, "not found"),
        }
    }

    fn mutate(&self, request: Request) -> http::Response {
        match self.call(request) {
            Ok(Response::Ok) => http::Response::json(200, &json!({ "ok": true })),
            Ok(Response::Error { code, message }) => error_response(code, message),
            Ok(_) => http::Response::error(502, "unexpected server response"),
            Err(response) => response,
        }
    }

    fn api_state(&self) -> http::Response {
        match self.sessions() {
            Ok(sessions) => {
                let values: Vec<Value> = sessions.iter().map(session_value).collect();
                http::Response::json(200, &json!({ "sessions": values }))
            }
            Err(response) => response,
        }
    }

    fn sessions(&self) -> Result<Vec<SessionSummary>, http::Response> {
        if let Some((at, sessions)) = &*self.cache.lock().unwrap() {
            if at.elapsed() < CACHE_TTL {
                return Ok(sessions.clone());
            }
        }
        match self.call(Request::Inspect)? {
            Response::Inventory { sessions, .. } => {
                let mut sessions = sessions;
                // `terminal list` hides archived rows by default; match it.
                sessions.retain(|session| !session.archived);
                *self.cache.lock().unwrap() = Some((Instant::now(), sessions.clone()));
                Ok(sessions)
            }
            Response::Error { code, message } => Err(error_response(code, message)),
            _ => Err(http::Response::error(502, "unexpected inventory response")),
        }
    }

    fn session_run(&self, id: SessionId) -> Result<SessionRunId, http::Response> {
        self.sessions()?
            .into_iter()
            .find(|session| session.id == id)
            .map(|session| session.run)
            .ok_or_else(|| http::Response::error(404, "session not found"))
    }

    fn call(&self, request: Request) -> Result<Response, http::Response> {
        let paths = ServerPaths::resolve()
            .map_err(|error| http::Response::error(500, &format!("{error:#}")))?;
        let Some(mut stream) = connect_if_running(&paths)
            .map_err(|error| http::Response::error(502, &format!("{error:#}")))?
        else {
            return Err(http::Response::error(503, "OVRCR server is not running"));
        };
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        protocol_client::request(&mut stream, id, request)
            .map_err(|error| http::Response::error(502, &format!("{error:#}")))
    }
}

fn bytes(body: &'static [u8], content_type: &'static str, immutable: bool) -> http::Response {
    let response = if immutable {
        http::Response::immutable(200, content_type, body.to_vec())
    } else {
        http::Response::new(200, content_type, body.to_vec())
    };
    response
}

fn json_response<T: serde::Serialize>(value: &T) -> http::Response {
    match serde_json::to_value(value) {
        Ok(value) => http::Response::json(200, &value),
        Err(_) => http::Response::error(500, "serialize response"),
    }
}

fn error_response(code: ErrorCode, message: String) -> http::Response {
    let status = match code {
        ErrorCode::InvalidRequest => 400,
        ErrorCode::NotFound => 404,
        ErrorCode::Conflict => 409,
        ErrorCode::Internal => 502,
        _ => 502,
    };
    http::Response::error(status, &message)
}

fn segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect()
}

fn parse_id(raw: &str) -> Result<u64, ()> {
    if raw.is_empty() || raw.len() > 20 || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(());
    }
    raw.parse::<u64>()
        .map_err(|_| ())
        .and_then(|id| if id == 0 { Err(()) } else { Ok(id) })
}

fn query_number(
    request: &http::Request,
    name: &str,
    default: u32,
    max: u32,
) -> Result<u32, http::Response> {
    match request.query(name) {
        None => Ok(default),
        Some(raw) => raw
            .parse::<u32>()
            .ok()
            .filter(|value| *value <= max)
            .ok_or_else(|| http::Response::error(400, &format!("{name} is out of range"))),
    }
}

fn valid_key(key: &str) -> bool {
    let Some(inner) = key
        .strip_prefix(':')
        .and_then(|value| value.strip_suffix(':'))
    else {
        return false;
    };
    !inner.is_empty()
        && inner.len() <= 46
        && inner
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn host_is_loopback(host: &str) -> bool {
    if host.starts_with("[::1]") {
        return true;
    }
    let name = host.split(':').next().unwrap_or(host);
    matches!(name, "127.0.0.1" | "localhost")
}

fn phase_name(phase: &SessionPhase) -> &'static str {
    match phase {
        SessionPhase::Running => "running",
        SessionPhase::Paused => "paused",
        SessionPhase::Exited { .. } => "exited",
        SessionPhase::Stopped => "stopped",
        SessionPhase::Interrupted => "interrupted",
    }
}

fn activity_name(activity: AgentActivity) -> &'static str {
    match activity {
        AgentActivity::Unknown => "unknown",
        AgentActivity::Idle => "idle",
        AgentActivity::Busy => "busy",
        AgentActivity::WaitingInput => "waiting_input",
        AgentActivity::Error => "error",
        AgentActivity::ResponseReady => "response_ready",
    }
}

fn session_value(session: &SessionSummary) -> Value {
    json!({
        "scope": session.scope,
        "id": session.id.0,
        "archived": session.archived,
        "run": session.run.0,
        "kind": session.kind,
        "project": session.project,
        "workspace": session.workspace,
        "cwd": session.cwd,
        "name": session.name,
        "title": session.title,
        "display_name": session.display_name(),
        "label": session.label,
        "pid": session.pid,
        "started_unix_ms": session.started_unix_ms,
        "phase": phase_name(&session.phase),
        "activity": activity_name(session.activity),
        "agent": session.agent,
        "agent_epoch": session.agent_epoch,
        "unread": session.unread,
        "context_usage": session.context_usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, target: &str, host: &str, login: Option<&str>) -> http::Request {
        let mut headers = vec![("host".into(), host.into())];
        if let Some(login) = login {
            headers.push(("tailscale-user-login".into(), login.into()));
        }
        http::Request {
            method: method.into(),
            target: target.into(),
            headers,
            body: Vec::new(),
        }
    }

    #[test]
    fn loopback_requests_bypass_the_login_gate() {
        let gateway = Gateway::new(Some("owner@example.com".into()));
        assert!(gateway.authorized(&request("GET", "/", "127.0.0.1:8787", None)));
        assert!(gateway.authorized(&request("GET", "/", "localhost:8787", None)));
    }

    #[test]
    fn tailnet_requests_need_the_configured_login() {
        let gateway = Gateway::new(Some("owner@example.com".into()));
        assert!(!gateway.authorized(&request("GET", "/", "node.example.ts.net", None)));
        assert!(!gateway.authorized(&request(
            "GET",
            "/",
            "node.example.ts.net",
            Some("other@example.com")
        )));
        assert!(gateway.authorized(&request(
            "GET",
            "/",
            "node.example.ts.net",
            Some("owner@example.com")
        )));
    }

    #[test]
    fn an_unconfigured_gateway_refuses_tailnet_requests() {
        let gateway = Gateway::new(None);
        assert!(gateway.authorized(&request("GET", "/", "127.0.0.1:8787", None)));
        assert!(!gateway.authorized(&request(
            "GET",
            "/",
            "node.example.ts.net",
            Some("owner@example.com")
        )));
    }

    #[test]
    fn keys_and_ids_are_validated() {
        assert!(valid_key(":escape:"));
        assert!(valid_key(":ctrl-shift-enter:"));
        assert!(!valid_key("escape"));
        assert!(!valid_key(":Escape:"));
        assert!(!valid_key(":hello world:"));
        assert_eq!(parse_id("12"), Ok(12));
        assert!(parse_id("0").is_err());
        assert!(parse_id("-1").is_err());
        assert!(parse_id("abc").is_err());
    }
}

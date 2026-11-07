//! Minimal synchronous HTTP/1.1 server for the loopback remote gateway.
//!
//! One thread per connection, one request per connection, `Connection: close`.
//! The gateway is loopback-only and typically sits behind `tailscale serve`,
//! which is happy to open a fresh connection per request. Keeping the parser
//! deliberately small avoids a new dependency and an async runtime.

use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

pub const MAX_BODY: usize = 64 * 1024;
const MAX_HEAD: usize = 16 * 1024;
const MAX_CONNECTIONS: usize = 24;
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'; \
script-src 'self'; connect-src 'self'; font-src 'self'; base-uri 'none'; \
form-action 'none'; frame-ancestors 'none'";

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("/")
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn query(&self, name: &str) -> Option<&str> {
        let query = self.target.split_once('?')?.1;
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then_some(value)
        })
    }
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    pub cache: &'static str,
    pub extra: Vec<(&'static str, String)>,
}

impl Response {
    pub fn new(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body,
            cache: "no-store",
            extra: Vec::new(),
        }
    }

    pub fn immutable(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body,
            cache: "public, max-age=86400, immutable",
            extra: Vec::new(),
        }
    }

    pub fn json(status: u16, value: &serde_json::Value) -> Self {
        let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
        Self::new(status, "application/json; charset=utf-8", body)
    }

    pub fn error(status: u16, message: &str) -> Self {
        Self::json(
            status,
            &serde_json::json!({ "error": { "message": message } }),
        )
    }

    pub fn empty(status: u16) -> Self {
        Self::new(status, "text/plain; charset=utf-8", Vec::new())
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

/// Read one request from `io`. Sends `100 Continue` before a nonempty body when
/// the client asked for it.
pub fn read_request<R: Read + Write>(io: &mut R) -> Result<Request> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 4096];
    let head_end = loop {
        if buffer.len() > MAX_HEAD {
            bail!("request headers too large");
        }
        let read = io.read(&mut chunk).context("read request")?;
        if read == 0 {
            bail!("connection closed before request headers");
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
    };

    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().context("missing request line")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().context("missing method")?.to_owned();
    let target = parts.next().context("missing target")?.to_owned();
    let version = parts.next().context("missing version")?;
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        bail!("unsupported HTTP version");
    }
    if method.len() > 16
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
    {
        bail!("invalid method");
    }
    if target.len() > 4096 || !target.starts_with('/') {
        bail!("invalid target");
    }

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            bail!("folded headers are not supported");
        }
        let (name, value) = line.split_once(':').context("malformed header")?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_graphic()) {
            bail!("invalid header name");
        }
        headers.push((name, value.trim().to_owned()));
    }

    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map(|(_, value)| value.parse::<usize>().context("invalid content-length"))
        .transpose()?
        .unwrap_or(0);
    if content_length > MAX_BODY {
        bail!("request body too large");
    }
    if headers.iter().any(|(name, _)| name == "transfer-encoding") {
        bail!("chunked request bodies are not supported");
    }
    let expects_continue = headers
        .iter()
        .any(|(name, value)| name == "expect" && value.eq_ignore_ascii_case("100-continue"));
    if expects_continue && content_length > 0 {
        io.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
            .context("write 100-continue")?;
        io.flush().context("flush 100-continue")?;
    }

    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < content_length {
        let read = io.read(&mut chunk).context("read request body")?;
        if read == 0 {
            bail!("connection closed before request body");
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    Ok(Request {
        method,
        target,
        headers,
        body,
    })
}

pub fn write_response<W: Write>(io: &mut W, response: &Response, head_only: bool) -> Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason(response.status)
    );
    head.push_str("Connection: close\r\n");
    head.push_str(&format!("Content-Type: {}\r\n", response.content_type));
    head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    head.push_str(&format!("Cache-Control: {}\r\n", response.cache));
    head.push_str(&format!("Content-Security-Policy: {CSP}\r\n"));
    head.push_str("X-Content-Type-Options: nosniff\r\n");
    head.push_str("Referrer-Policy: no-referrer\r\n");
    for (name, value) in &response.extra {
        if !value.contains('\r') && !value.contains('\n') {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    head.push_str("\r\n");
    io.write_all(head.as_bytes())
        .context("write response head")?;
    if !head_only {
        io.write_all(&response.body)
            .context("write response body")?;
    }
    io.flush().context("flush response")
}

/// Accept connections forever, running `handler` on a thread per connection.
pub fn serve<F>(listener: TcpListener, handler: F) -> Result<()>
where
    F: Fn(&Request) -> Response + Send + Sync + 'static,
{
    let handler = Arc::new(handler);
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("remote: accept failed: {error}");
                continue;
            }
        };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            let _ = stream.write_all(
                b"HTTP/1.1 503 Service Unavailable\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            );
            continue;
        }
        let handler = Arc::clone(&handler);
        let active = Arc::clone(&active);
        std::thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
            let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
            let _ = handle_connection(&mut stream, &*handler);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

fn handle_connection<F>(stream: &mut TcpStream, handler: &F) -> Result<()>
where
    F: Fn(&Request) -> Response,
{
    let request = match read_request(stream) {
        Ok(request) => request,
        Err(error) => {
            let response = Response::error(400, &format!("{error:#}"));
            let _ = write_response(stream, &response, false);
            return Ok(());
        }
    };
    let response = handler(&request);
    write_response(stream, &response, request.method == "HEAD")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(raw: &[u8]) -> Result<Request> {
        let mut cursor = Cursor::new(raw.to_vec());
        read_request(&mut cursor)
    }

    #[test]
    fn parses_get_and_headers_case_insensitively() {
        let request =
            parse(b"GET /api/state?x=1 HTTP/1.1\r\nHost: 127.0.0.1:8787\r\nX-Test: yes\r\n\r\n")
                .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path(), "/api/state");
        assert_eq!(request.query("x"), Some("1"));
        assert_eq!(request.header("x-test"), Some("yes"));
        assert_eq!(request.header("HOST"), Some("127.0.0.1:8787"));
    }

    #[test]
    fn parses_a_body_and_reports_a_missing_content_length_as_zero() {
        let raw = b"POST /api/session/3/send HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let request = parse(raw).unwrap();
        assert_eq!(request.body, b"hello");
        let raw = b"POST /x HTTP/1.1\r\n\r\n";
        assert!(parse(raw).unwrap().body.is_empty());
    }

    #[test]
    fn refuses_oversized_and_chunked_bodies_and_folded_headers() {
        let too_large = format!(
            "POST /x HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert!(parse(too_large.as_bytes()).is_err());
        assert!(parse(b"POST /x HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n").is_err());
        assert!(parse(b"GET / HTTP/1.1\r\nHost: x\r\n more\r\n\r\n").is_err());
    }

    #[test]
    fn writes_a_response_with_security_headers_and_honours_head() {
        let mut output = Vec::new();
        let response = Response::json(200, &serde_json::json!({"ok": true}));
        write_response(&mut output, &response, false).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Security-Policy:"));
        assert!(text.contains("Connection: close"));
        assert!(text.ends_with("{\"ok\":true}"));

        let mut head_only = Vec::new();
        write_response(&mut head_only, &response, true).unwrap();
        let text = String::from_utf8(head_only).unwrap();
        assert!(text.ends_with("\r\n\r\n"));
    }
}

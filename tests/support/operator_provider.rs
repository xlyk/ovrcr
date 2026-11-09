//! A local OpenAI-compatible streaming provider for the installed-pi and GUI
//! Operator acceptance paths.
//!
//! It binds loopback only, speaks `chat/completions` SSE (the shape in
//! `scripts/validate-pi-rpc.py`), records every request body and authorization
//! header, and answers from a test-supplied synchronous plan. A plan may hold
//! the stream after its text/thinking/tool chunks and emit further chunks only
//! after release, so a test or GUI can observe partial output before the answer.
//! Thinking chunks use `delta.reasoning_content`, which pi's
//! `openai-completions` provider turns into thinking deltas.
//!
//! The provider also seeds only the fixture instance's private `operator-pi`
//! profile with a synthetic provider/model/auth and default model. It never
//! reads or writes a real Pi profile or credential.

#![allow(dead_code)]

use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const PROVIDER: &str = "fixture";
pub const MODEL: &str = "operator-fixture";
pub const MODEL_ID: &str = "fixture/operator-fixture";
pub const API_KEY: &str = "fixture-synthetic-key";
const CONTEXT_WINDOW: u64 = 200_000;

/// One streamed piece the fixture "model" returns.
#[derive(Clone, Debug)]
pub enum Chunk {
    Text(String),
    /// `delta.reasoning_content`, which pi's openai-completions provider turns
    /// into a thinking block. It is never answer text.
    Thinking(String),
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
}

/// One scripted assistant response.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub chunks: Vec<Chunk>,
    /// Emit `chunks`, then wait for `Provider::release` before finishing.
    pub hold: bool,
    /// Emitted only after a hold is released, such as an answer that follows
    /// streamed thinking.
    pub after_release: Vec<Chunk>,
}

impl Plan {
    pub fn new(chunks: Vec<Chunk>) -> Self {
        Self {
            chunks,
            hold: false,
            after_release: Vec::new(),
        }
    }

    pub fn text(text: &str) -> Self {
        Self::new(vec![Chunk::Text(text.into())])
    }

    pub fn tool(id: &str, name: &str, arguments: Value) -> Self {
        Self::new(vec![Chunk::ToolCall {
            id: id.into(),
            name: name.into(),
            arguments,
        }])
    }

    /// Text followed by a tool call in the same assistant message.
    pub fn text_then_tool(text: &str, id: &str, name: &str, arguments: Value) -> Self {
        Self::new(vec![
            Chunk::Text(text.into()),
            Chunk::ToolCall {
                id: id.into(),
                name: name.into(),
                arguments,
            },
        ])
    }

    pub fn holding(mut self) -> Self {
        self.hold = true;
        self
    }

    /// Chunks sent only after `Provider::release` finishes the hold.
    pub fn then_after_release(mut self, mut chunks: Vec<Chunk>) -> Self {
        self.after_release.append(&mut chunks);
        self
    }

    pub fn then_text_after_release(self, text: &str) -> Self {
        self.then_after_release(vec![Chunk::Text(text.into())])
    }
}

pub type Handler = Box<dyn Fn(&Value, usize) -> Plan + Send + Sync>;

#[derive(Clone, Debug)]
pub struct Recorded {
    pub body: Value,
    pub authorization: Option<String>,
    pub path: String,
}

#[derive(Default)]
struct State {
    requests: Vec<Recorded>,
    connections: usize,
    errors: Vec<String>,
    released: bool,
    stopped: bool,
}

pub struct Provider {
    pub base: String,
    port: u16,
    shared: Arc<(Mutex<State>, Condvar)>,
    stop: Arc<AtomicBool>,
    listener: Option<JoinHandle<()>>,
}

impl Provider {
    pub fn start(handler: Handler) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(handler);
        let state = Arc::clone(&shared);
        let stopping = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        assert!(
                            peer.ip().is_loopback(),
                            "provider accepted a non-loopback peer"
                        );
                        // macOS may let an accepted socket inherit the listener's
                        // nonblocking flag; each exchange is served to completion.
                        stream.set_nonblocking(false).unwrap();
                        state.0.lock().unwrap().connections += 1;
                        let state = Arc::clone(&state);
                        let handler = Arc::clone(&handler);
                        thread::spawn(move || {
                            if let Err(error) = serve(stream, &state, handler) {
                                state.0.lock().unwrap().errors.push(error.to_string());
                            }
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("provider accept failed: {error}"),
                }
            }
        });
        Self {
            base,
            port,
            shared,
            stop,
            listener: Some(thread),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Seed the instance's private profile: a synthetic provider/model/auth and
    /// the profile's default model. Returns the profile directory.
    pub fn seed_profile(&self, instance: &Path) -> PathBuf {
        self.seed_profile_with(instance, CONTEXT_WINDOW, None)
    }

    /// Same, with an explicit context window and optional compaction settings.
    pub fn seed_profile_with(
        &self,
        instance: &Path,
        context_window: u64,
        compaction: Option<Value>,
    ) -> PathBuf {
        let profile = instance.join("operator-pi");
        fs::create_dir_all(&profile).unwrap();
        fs::set_permissions(&profile, fs::Permissions::from_mode(0o700)).unwrap();
        let models = json!({
            "providers": {
                PROVIDER: {
                    "baseUrl": format!("{}/v1", self.base),
                    "api": "openai-completions",
                    "apiKey": API_KEY,
                    "models": [{
                        "id": MODEL,
                        "name": "Fixture Operator",
                        // Keep delta.reasoning_content visible through pi's
                        // openai-completions thinking deltas.
                        "reasoning": true,
                        "input": ["text"],
                        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
                        "contextWindow": context_window,
                        "maxTokens": 1024
                    }]
                }
            }
        });
        let mut settings = json!({ "defaultModel": MODEL_ID });
        if let Some(compaction) = compaction {
            settings["compaction"] = compaction;
        }
        write_private(&profile.join("models.json"), &models);
        write_private(&profile.join("settings.json"), &settings);
        profile
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.shared.0.lock().unwrap().requests.clone()
    }

    pub fn request_bodies(&self) -> Vec<Value> {
        self.requests()
            .into_iter()
            .map(|request| request.body)
            .collect()
    }

    pub fn authorizations(&self) -> Vec<Option<String>> {
        self.requests()
            .into_iter()
            .map(|request| request.authorization)
            .collect()
    }

    pub fn wait_requests(&self, at_least: usize, budget: Duration) -> Vec<Recorded> {
        let deadline = Instant::now() + budget;
        loop {
            self.assert_healthy();
            let requests = self.requests();
            if requests.len() >= at_least {
                return requests;
            }
            assert!(
                Instant::now() < deadline,
                "provider request deadline: saw {} of {at_least}",
                requests.len()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Let every held stream finish.
    pub fn release(&self) {
        let (lock, condvar) = &*self.shared;
        let mut state = lock.lock().unwrap();
        state.released = true;
        condvar.notify_all();
    }

    pub fn diagnostics(&self) -> (usize, Vec<String>) {
        let state = self.shared.0.lock().unwrap();
        (state.connections, state.errors.clone())
    }

    pub fn assert_healthy(&self) {
        assert!(
            !self.stop.load(Ordering::Acquire) && self.listener.is_some(),
            "provider is stopped"
        );
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.release();
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}

fn write_private(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn serve(
    mut stream: TcpStream,
    shared: &Arc<(Mutex<State>, Condvar)>,
    handler: Arc<Handler>,
) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/v1/chat/completions")
        .to_owned();
    let mut authorization = None;
    let mut content_length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let trimmed = header.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.trim().to_owned());
            } else if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    let decoded: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let index = {
        let (lock, _) = &**shared;
        let mut state = lock.lock().unwrap();
        state.requests.push(Recorded {
            body: decoded.clone(),
            authorization,
            path: path.clone(),
        });
        state.requests.len() - 1
    };
    let plan = handler(&decoded, index);
    let model = decoded["model"].as_str().unwrap_or(MODEL).to_owned();

    let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n";
    stream.write_all(headers.as_bytes())?;
    stream.flush()?;

    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let has_tools = plan
        .chunks
        .iter()
        .chain(&plan.after_release)
        .any(|chunk| matches!(chunk, Chunk::ToolCall { .. }));
    let mut first = true;
    // Pi identifies streamed tool calls by `index`; two calls in one assistant
    // message need distinct indexes or the second is merged into the first.
    let mut tool_index = 0u64;
    for chunk in &plan.chunks {
        write_chunk(
            &mut stream,
            chunk,
            &model,
            created,
            &mut first,
            &mut tool_index,
        )?;
    }
    stream.flush()?;

    if plan.hold {
        let (lock, condvar) = &**shared;
        let mut state = lock.lock().unwrap();
        while !state.released {
            state = condvar.wait(state).unwrap();
        }
        if state.stopped {
            return Ok(());
        }
    }

    for chunk in &plan.after_release {
        write_chunk(
            &mut stream,
            chunk,
            &model,
            created,
            &mut first,
            &mut tool_index,
        )?;
    }
    stream.flush()?;

    let finish = if has_tools { "tool_calls" } else { "stop" };
    let done = sse(&model, created, json!({}), json!(finish));
    stream.write_all(done.as_bytes())?;
    stream.write_all(b"data: [DONE]\n\n")?;
    stream.flush()?;
    Ok(())
}

fn write_chunk(
    stream: &mut TcpStream,
    chunk: &Chunk,
    model: &str,
    created: u64,
    first: &mut bool,
    tool_index: &mut u64,
) -> std::io::Result<()> {
    let mut delta = match chunk {
        Chunk::Text(text) => json!({ "content": text }),
        Chunk::Thinking(text) => json!({ "reasoning_content": text }),
        Chunk::ToolCall {
            id,
            name,
            arguments,
        } => {
            let call = json!({
                "index": *tool_index,
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": serde_json::to_string(arguments).unwrap()
                }
            });
            *tool_index += 1;
            json!({ "tool_calls": [call] })
        }
    };
    if *first {
        delta["role"] = json!("assistant");
        *first = false;
    }
    stream.write_all(sse(model, created, delta, Value::Null).as_bytes())
}

fn sse(model: &str, created: u64, delta: Value, finish_reason: Value) -> String {
    let chunk = json!({
        "id": "fixture",
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}]
    });
    format!("data: {chunk}\n\n")
}

/// The last user message text in an OpenAI-completions request.
pub fn last_user_text(body: &Value) -> String {
    let mut texts = body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["role"] == "user")
        .map(|message| content_text(&message["content"]));
    texts.next_back().unwrap_or_default()
}

/// OpenAI content may be a plain string or text parts.
pub fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Whether the request is Pi's compaction summarization call.
pub fn is_compaction_request(body: &Value) -> bool {
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    messages.iter().any(|message| {
        message["role"].as_str().is_some_and(is_instruction_role)
            && message["content"]
                .as_str()
                .is_some_and(|text| text.to_lowercase().contains("summar"))
    }) || messages.iter().any(|message| {
        message["content"].as_str().is_some_and(|text| {
            text.contains("Summarize the conversation") || text.contains("compaction summary")
        })
    })
}

/// All text content in a recorded request, for context assertions.
pub fn request_text(body: &Value) -> String {
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    messages
        .iter()
        .map(|message| content_text(&message["content"]))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pi's OpenAI-completions adapter sends the native system channel as
/// `developer` for reasoning models and `system` otherwise. Both are instruction
/// text, never conversation payload.
pub fn is_instruction_role(role: &str) -> bool {
    matches!(role, "system" | "developer")
}

/// Every instruction-channel message's content in a recorded request.
pub fn system_text(body: &Value) -> String {
    body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["role"].as_str().is_some_and(is_instruction_role))
        .map(|message| content_text(&message["content"]))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Non-instruction message content, tool-call names and arguments. This is the
/// conversation payload a provider receives, with the system/developer prompt
/// excluded.
pub fn non_system_text(body: &Value) -> String {
    let mut parts = Vec::new();
    for message in body["messages"].as_array().into_iter().flatten() {
        if message["role"].as_str().is_some_and(is_instruction_role) {
            continue;
        }
        parts.push(content_text(&message["content"]));
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            if let Some(name) = call["function"]["name"].as_str() {
                parts.push(name.to_owned());
            }
            if let Some(arguments) = call["function"]["arguments"].as_str() {
                parts.push(arguments.to_owned());
            }
        }
    }
    parts.join("\n")
}

pub fn tool_result_texts(body: &Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["role"] == "tool")
        .map(|message| content_text(&message["content"]))
        .collect()
}

pub fn read_marker(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

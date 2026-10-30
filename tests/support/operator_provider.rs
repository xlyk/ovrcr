//! Only the operator test's OAuth and Responses HTTP fixture, layered on `Live`.
use ovrcr::operator::auth::{AuthCancel, AuthEndpoints, AuthManager};
use ovrcr_protocol::OperatorProvider;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const BUDGET: Duration = Duration::from_secs(20);
#[derive(Default)]
struct State {
    stopped: bool,
    inference_attempts: usize,
    released: HashSet<String>,
    requests: HashMap<String, Vec<Value>>,
    errors: Vec<String>,
}
type Shared = Arc<(Mutex<State>, Condvar)>;

pub struct Provider {
    pub base: String,
    shared: Shared,
    listener: Option<JoinHandle<()>>,
}
impl Provider {
    pub fn new(workdir: &Path, gate_budget: Duration) -> Self {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        socket.set_nonblocking(true).unwrap();
        let base = format!("http://{}", socket.local_addr().unwrap());
        let shared = Shared::default();
        let state = shared.clone();
        let workdir = workdir.to_owned();
        let listener = thread::spawn(move || {
            let mut connections = Vec::new();
            while !state.0.lock().unwrap().stopped {
                match socket.accept() {
                    Ok((stream, peer)) => {
                        stream.set_nonblocking(false).unwrap();
                        assert!(peer.ip().is_loopback());
                        let state = state.clone();
                        let workdir = workdir.clone();
                        connections.push(thread::spawn(move || {
                            if let Err(error) = serve(stream, &state, &workdir, gate_budget) {
                                state.0.lock().unwrap().errors.push(error.to_string());
                            }
                        }));
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("loopback provider accept: {error}"),
                }
            }
            for connection in connections {
                connection.join().unwrap();
            }
        });
        Self {
            base,
            shared,
            listener: Some(listener),
        }
    }

    /// Resolve OAuth in a child with no inherited proxy/profile environment.
    pub fn sign_in(&self, instance: &Path) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "compiled_worker::seed_grok_auth",
                "--ignored",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", instance.parent().unwrap())
            .env("NO_PROXY", "*")
            .env("no_proxy", "*")
            .env("OVRCR_OPERATOR_E2E_AUTH_BASE", &self.base)
            .env("OVRCR_OPERATOR_E2E_AUTH_INSTANCE", instance)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + BUDGET;
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("fixture OAuth child exceeded its deadline");
            }
            thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        let errors = self.shared.0.lock().unwrap().errors.clone();
        assert!(
            output.status.success(),
            "fixture OAuth child status {}: {} {}; provider errors: {:?}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            errors
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
            "OAuth fixture filter did not execute exactly one test: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub fn release(&self, model: &str) {
        self.shared
            .0
            .lock()
            .unwrap()
            .released
            .insert(model.to_owned());
        self.shared.1.notify_all();
    }
    pub fn inference_attempts(&self) -> usize {
        self.shared.0.lock().unwrap().inference_attempts
    }
    pub fn requests(&self, model: &str) -> Vec<Value> {
        self.shared
            .0
            .lock()
            .unwrap()
            .requests
            .get(model)
            .cloned()
            .unwrap_or_default()
    }
    pub fn assert_healthy(&self) {
        let errors = self.shared.0.lock().unwrap().errors.clone();
        assert!(errors.is_empty(), "loopback provider errors: {errors:?}");
    }
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.shared
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .stopped = true;
        self.shared.1.notify_all();
        if let Some(listener) = self.listener.take() {
            let result = listener.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn read_request(stream: &mut TcpStream) -> io::Result<(String, String, String)> {
    stream.set_read_timeout(Some(BUDGET))?;
    stream.set_write_timeout(Some(BUDGET))?;
    let mut bytes = Vec::new();
    loop {
        if bytes.len() > 1024 * 1024 {
            return Err(io::Error::other("fixture request exceeds 1 MiB"));
        }
        let mut block = [0; 4096];
        let count = stream.read(&mut block)?;
        if count == 0 {
            return Err(io::Error::other("request closed before body"));
        }
        bytes.extend_from_slice(&block[..count]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
            let length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                let path = headers
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or_default()
                    .to_owned();
                let body = String::from_utf8(bytes[end + 4..end + 4 + length].to_vec())
                    .map_err(io::Error::other)?;
                return Ok((path, headers, body));
            }
        }
    }
}
fn json_response(stream: &mut TcpStream, body: Value) -> io::Result<()> {
    let body = body.to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
fn event(stream: &mut TcpStream, body: Value) -> io::Result<()> {
    write!(
        stream,
        "event: {}\ndata: {body}\n\n",
        body["type"].as_str().unwrap()
    )?;
    stream.flush()
}
fn complete(stream: &mut TcpStream, model: &str, output: Vec<Value>) -> io::Result<()> {
    event(
        stream,
        json!({"type":"response.completed","response":{"id":format!("resp-{model}"),"status":"completed","output":output,"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}),
    )
}
fn message(model: &str, text: &str) -> Value {
    json!({"type":"message","id":format!("msg-{model}"),"role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]})
}
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}
fn serve(
    mut stream: TcpStream,
    shared: &Shared,
    workdir: &Path,
    gate_budget: Duration,
) -> io::Result<()> {
    let (path, headers, body) = read_request(&mut stream)?;
    if path == "/device" {
        if !body.contains("referrer=ovrcr") {
            return Err(io::Error::other("missing real device request"));
        }
        return json_response(
            &mut stream,
            json!({"device_code":"fixture-device","user_code":"E2E-LOCAL","verification_uri":"https://127.0.0.1/activate","expires_in":60,"interval":1}),
        );
    }
    if path == "/token" {
        if !body.contains("device_code=fixture-device") {
            return Err(io::Error::other("unexpected token request or refresh"));
        }
        return json_response(
            &mut stream,
            json!({"access_token":"operator-e2e-oauth-only","refresh_token":"operator-e2e-refresh-only","token_type":"Bearer","expires_in":86400}),
        );
    }
    shared.0.lock().unwrap().inference_attempts += 1;
    if path != "/v1/responses" || !headers.contains("authorization: bearer operator-e2e-oauth-only")
    {
        return Err(io::Error::other("unexpected inference path or bearer"));
    }
    let request: Value = serde_json::from_str(&body).map_err(io::Error::other)?;
    let model = request["model"]
        .as_str()
        .ok_or_else(|| io::Error::other("missing model"))?
        .to_owned();
    if !matches!(
        model.as_str(),
        "fixture-a" | "fixture-b" | "fixture-timeout" | "fixture-incomplete" | "fixture-lifecycle"
    ) || request["stream"] != true
    {
        return Err(io::Error::other("unexpected model or nonstreaming request"));
    }
    let ordinal = {
        let mut state = shared.0.lock().unwrap();
        let requests = state.requests.entry(model.clone()).or_default();
        requests.push(request.clone());
        requests.len()
    };
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n"
    )?;
    let label = model.strip_prefix("fixture-").unwrap().to_ascii_uppercase();
    if model == "fixture-lifecycle" {
        let text = message(&model, "LIFECYCLE waiting");
        event(
            &mut stream,
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg-lifecycle","role":"assistant","content":[]}}),
        )?;
        event(
            &mut stream,
            json!({"type":"response.output_text.delta","output_index":0,"item_id":"msg-lifecycle","content_index":0,"delta":"LIFECYCLE waiting"}),
        )?;
        if !wait_release(shared, &format!("fixture-lifecycle-{ordinal}"), gate_budget)? {
            return Ok(());
        }
        event(
            &mut stream,
            json!({"type":"response.output_item.done","output_index":0,"item":text}),
        )?;
        complete(&mut stream, &model, vec![text])
    } else if model == "fixture-incomplete" {
        if ordinal != 1 {
            return Err(io::Error::other("incomplete response retried"));
        }
        let text = message(&model, "INCOMPLETE partial visible");
        event(
            &mut stream,
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":format!("msg-{model}"),"role":"assistant","content":[]}}),
        )?;
        event(
            &mut stream,
            json!({"type":"response.output_text.delta","output_index":0,"item_id":format!("msg-{model}"),"content_index":0,"delta":"INCOMPLETE partial visible"}),
        )?;
        event(
            &mut stream,
            json!({"type":"response.output_item.done","output_index":0,"item":text}),
        )?;
        if !wait_release(shared, &model, gate_budget)? {
            return Ok(());
        }
        event(
            &mut stream,
            json!({"type":"response.incomplete","response":{"id":"resp-incomplete","status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[text],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}),
        )
    } else if ordinal == 1 {
        let timeout = model == "fixture-timeout";
        let command = if timeout {
            format!(
                "printf 'started\n' >> {}; printf '%s\n' \"$$\" > {}; exec /bin/sleep 30",
                shell_quote(&workdir.join("timeout.executions")),
                shell_quote(&workdir.join("timeout.pid"))
            )
        } else {
            format!(
                "printf 'E2E_{label}_TOOL\nCLI_BEGIN\n'; \"$OVRCR_EXECUTABLE\" --json list; cli_status=$?; printf '\nCLI_END\n'; printf 'ran\n' >> {}; exit \"$cli_status\"",
                shell_quote(&workdir.join(format!("{model}.executions")))
            )
        };
        let arguments =
            json!({"command":command,"cwd":workdir,"timeout_seconds":if timeout {2} else {10}})
                .to_string();
        let call = json!({"type":"function_call","id":format!("fc-{model}"),"call_id":format!("call-{model}"),"name":"bash","arguments":arguments,"status":"completed"});
        event(
            &mut stream,
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":format!("msg-{model}"),"role":"assistant","content":[]}}),
        )?;
        event(
            &mut stream,
            json!({"type":"response.output_text.delta","output_index":0,"item_id":format!("msg-{model}"),"content_index":0,"delta":format!("{label} before tool")}),
        )?;
        let text = message(&model, &format!("{label} before tool"));
        event(
            &mut stream,
            json!({"type":"response.output_item.done","output_index":0,"item":text}),
        )?;
        event(
            &mut stream,
            json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":format!("fc-{model}"),"call_id":format!("call-{model}"),"name":"bash","arguments":""}}),
        )?;
        event(
            &mut stream,
            json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":arguments}),
        )?;
        event(
            &mut stream,
            json!({"type":"response.output_item.done","output_index":1,"item":call}),
        )?;
        complete(&mut stream, &model, vec![text, call])
    } else if ordinal == 2 || (ordinal == 3 && model == "fixture-b") {
        // Validate the actual serialized Bash result, including CLI exit and JSON bytes.
        let result = request["input"]
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item["type"] == "function_call_output")
            })
            .and_then(|item| item["output"].as_str())
            .ok_or_else(|| io::Error::other("missing Bash result"))?;
        let result: Value = serde_json::from_str(result).map_err(io::Error::other)?;
        let output = result["output"]
            .as_str()
            .ok_or_else(|| io::Error::other("missing Bash output"))?;
        if !output.contains(&format!("E2E_{label}_TOOL"))
            || result["exit_code"] != 0
            || result["truncated"] != false
            || result["timed_out"] != false
        {
            return Err(io::Error::other("real Bash/CLI command did not succeed"));
        }
        cli_array(output)?;
        let partial = if ordinal == 3 {
            "B follow-up waiting".to_owned()
        } else {
            format!("{label} partial waiting")
        };
        let tail = if ordinal == 3 {
            " B follow-up final".to_owned()
        } else {
            format!(" {label} final")
        };
        let gate = if ordinal == 3 {
            "fixture-b-followup"
        } else {
            &model
        };
        let message_id = format!("msg-{model}-final-{ordinal}");
        event(
            &mut stream,
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":message_id,"role":"assistant","content":[]}}),
        )?;
        event(
            &mut stream,
            json!({"type":"response.output_text.delta","output_index":0,"item_id":message_id,"content_index":0,"delta":partial}),
        )?;
        // A bounded barrier proves partial text reached the journal before final completion.
        if !wait_release(shared, gate, gate_budget)? {
            return Ok(());
        }
        // A stopped worker closes its socket; that is expected for the independently cancelled arm.
        let result = (|| {
            event(
                &mut stream,
                json!({"type":"response.output_text.delta","output_index":0,"item_id":message_id,"content_index":0,"delta":tail}),
            )?;
            let mut text = message(
                &format!("{model}-final-{ordinal}"),
                &format!("{partial}{tail}"),
            );
            text["id"] = json!(message_id);
            event(
                &mut stream,
                json!({"type":"response.output_item.done","output_index":0,"item":text}),
            )?;
            complete(&mut stream, &model, vec![text])
        })();
        match result {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
                ) =>
            {
                Ok(())
            }
            other => other,
        }
    } else {
        Err(io::Error::other(
            "unexpected replay or extra inference request",
        ))
    }
}

fn wait_release(shared: &Shared, gate: &str, budget: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + budget;
    let mut state = shared.0.lock().unwrap();
    while !state.stopped && !state.released.contains(gate) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::other("model gate was not released"));
        }
        state = shared.1.wait_timeout(state, remaining).unwrap().0;
    }
    Ok(!state.stopped)
}

/// The public CLI list is an array; delimiters separate it from the Bash evidence marker.
pub fn cli_array(output: &str) -> io::Result<Value> {
    let (_, json) = output
        .split_once("CLI_BEGIN\n")
        .ok_or_else(|| io::Error::other("missing CLI_BEGIN"))?;
    let (json, _) = json
        .split_once("\nCLI_END")
        .ok_or_else(|| io::Error::other("missing CLI_END"))?;
    let value: Value = serde_json::from_str(json.trim()).map_err(io::Error::other)?;
    if !value.is_array() {
        return Err(io::Error::other("CLI list must be an array"));
    }
    Ok(value)
}

/// Child-only real OAuth seeding; all six configurable endpoints point at one loopback listener.
pub fn sign_in_local(base: &str, instance: &Path) {
    let url = reqwest::Url::parse(base).unwrap();
    assert_eq!(url.scheme(), "http");
    assert!(
        url.host_str()
            .unwrap()
            .parse::<std::net::IpAddr>()
            .unwrap()
            .is_loopback()
    );
    assert_eq!(std::env::var("NO_PROXY").unwrap(), "*");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let auth = AuthManager::new(instance)
            .unwrap()
            .with_endpoints(AuthEndpoints {
                grok_device: format!("{base}/device"),
                grok_token: format!("{base}/token"),
                chat_token: format!("{base}/token"),
                chat_jwks: format!("{base}/jwks"),
                chat_models: format!("{base}/models"),
                chat_revoke: format!("{base}/revoke"),
                ..Default::default()
            });
        let cancel = AuthCancel::default();
        let login = auth.begin_grok(&cancel).await.unwrap();
        auth.finish_grok(login, &cancel).await.unwrap();
        assert!(auth.status(OperatorProvider::Grok).await.unwrap().signed_in);
    });
}

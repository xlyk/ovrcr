//! Same-executable, supervised Rig worker. The Server alone certifies termination.
use super::{
    auth::{AuthCancel, AuthManager},
    bash::{Bash, BashArgs, BashOutput},
};
use crate::report::{reporter::Reporter, reserve_invocation_for};
use anyhow::{Result, anyhow, bail};
use futures::StreamExt;
use ovrcr_protocol::{
    AgentProvider, OperatorEventBatch, OperatorModelHistory, OperatorProvider, OperatorToolStatus,
    OperatorTurnIdentity, OperatorWorkerEvent, OperatorWorkerStart,
};
use rig::{
    Agent, AgentBuilder,
    agent::{
        AgentHook, DispatchAction, DispatchEvent, HookContext, ModelTurnAction, ModelTurnFinished,
        MultiTurnStreamItem,
    },
    catalog::ModelSpec,
    client::signed_in::{CredentialSource, SignedInModel},
    effect::{EffectKind, HandlerKey},
    handlers::Handlers,
    http_client::DynHttpClient,
    message::Message,
    providers::{
        chatgpt::auth::{AuthContext, AuthError as RigAuthError},
        registry::ProviderId,
    },
    streaming::{Item, StreamEvent},
    tool::{Tool, ToolContext},
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

const BATCH_BYTES: usize = 64 * 1024;
type SharedReporter = Arc<Mutex<Reporter>>;

pub fn run(identity: OperatorTurnIdentity) -> Result<()> {
    identity.validate().map_err(anyhow::Error::msg)?;
    let lease = reserve_invocation_for(AgentProvider::Operator)?
        .ok_or_else(|| anyhow!("managed operator reservation required"))?;
    let reporter = Arc::new(Mutex::new(Reporter::new(
        AgentProvider::Operator,
        Some(lease),
        None,
    )));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let start = reporting(&reporter, move |reporter| {
            reporter.operator_start(identity, deadline())
        })
        .await;
        match start {
            Ok(start) => {
                let mut sequence = 1;
                let outcome = run_turn(&reporter, &mut sequence, start).await;
                let event = match outcome {
                    Ok(history) => OperatorWorkerEvent::Completed(history),
                    Err(message) => OperatorWorkerEvent::Failed(message.to_string()),
                };
                if commit(&reporter, identity, &mut sequence, event)
                    .await
                    .is_ok()
                {
                    // Self EOF is expected here, and is never a controlled-stop receipt.
                    let _ = retire(&reporter, identity, false).await;
                }
            }
            Err(_) => eprintln!("Operator bootstrap unavailable; stop this turn before retrying."),
        }
        // Keep the lease, process and child ownership alive even when control was refused.
        // Only the Server's scoped termination (or explicit user stop) may finish this worker.
        std::future::pending::<()>().await;
    });
    Ok(())
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
async fn reporting<T: Send + 'static>(
    reporter: &SharedReporter,
    action: impl FnOnce(&mut Reporter) -> Result<T> + Send + 'static,
) -> Result<T> {
    let reporter = reporter.clone();
    tokio::task::spawn_blocking(move || {
        let mut reporter = reporter
            .lock()
            .map_err(|_| anyhow!("operator reporting unavailable"))?;
        action(&mut reporter)
    })
    .await
    .map_err(|_| anyhow!("operator reporting unavailable"))?
}
async fn commit(
    reporter: &SharedReporter,
    identity: OperatorTurnIdentity,
    sequence: &mut u64,
    event: OperatorWorkerEvent,
) -> Result<()> {
    let batch = OperatorEventBatch {
        identity,
        sequence: *sequence,
        events: vec![event],
    };
    reporting(reporter, move |reporter| {
        reporter.operator_batch(batch, deadline())
    })
    .await?;
    *sequence += 1;
    Ok(())
}
async fn retire(
    reporter: &SharedReporter,
    identity: OperatorTurnIdentity,
    stop: bool,
) -> Result<()> {
    reporting(reporter, move |reporter| {
        reporter.operator_retire(identity, stop, Instant::now() + Duration::from_secs(15))
    })
    .await
}

#[derive(Clone)]
struct Credentials {
    auth: AuthManager,
    provider: OperatorProvider,
    base: String,
}
impl CredentialSource for Credentials {
    async fn auth_context(
        &self,
        _: &DynHttpClient,
    ) -> std::result::Result<AuthContext, RigAuthError> {
        let token = self
            .auth
            .resolve(self.provider, &AuthCancel::default())
            .await
            .map_err(|_| {
                RigAuthError::Message(
                    "operator subscription credential unavailable; sign in again".into(),
                )
            })?;
        Ok(AuthContext {
            api_key: token.expose().to_owned().into(),
            account_id: None,
            api_base: Some(self.base.clone()),
        })
    }
}
fn production_base(provider: OperatorProvider) -> &'static str {
    match provider {
        OperatorProvider::ChatGpt => "https://api.openai.com/v1",
        OperatorProvider::Grok => "https://api.x.ai",
    }
}
fn diagnostic_base(value: Option<std::ffi::OsString>, enabled: bool) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !enabled {
        bail!("operator test endpoint requires acceptance-diagnostics");
    }
    let value = value
        .into_string()
        .map_err(|_| anyhow!("invalid operator test endpoint"))?;
    let url = reqwest::Url::parse(&value).map_err(|_| anyhow!("invalid operator test endpoint"))?;
    let loopback = url
        .host_str()
        .and_then(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .is_some_and(|ip| ip.is_loopback());
    if url.scheme() != "http"
        || !loopback
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("invalid operator test endpoint");
    }
    Ok(Some(url.as_str().trim_end_matches('/').to_owned()))
}
fn make_agent(
    auth: AuthManager,
    provider: OperatorProvider,
    model: &str,
    base: String,
    bash: Bash,
    host: Host,
) -> Result<Agent> {
    let provider_id = ProviderId::catalog(match provider {
        OperatorProvider::ChatGpt => "openai",
        OperatorProvider::Grok => "xai",
    })
    .ok_or_else(|| anyhow!("operator provider unavailable"))?;
    let spec = ModelSpec::new(provider_id, model).with_tools(true);
    let model = SignedInModel::new(
        spec,
        Credentials {
            auth,
            provider,
            base,
        },
        rig::rig_reqwest::shared(),
    )
    .map_err(|_| anyhow!("operator model unavailable"))?;
    let handlers = Handlers::default();
    handlers
        .register("operator-model", model)
        .map_err(|_| anyhow!("operator model unavailable"))?;
    Ok(
        AgentBuilder::over_handlers(handlers, "operator", HandlerKey::from("operator-model"))
            .name("OVRCR Operator")
            .preamble(PREAMBLE)
            .default_max_turns(20)
            .record_content_telemetry(false)
            .additional_params(serde_json::json!({"store": false}))
            .add_hook(CompletedTurnGuard)
            .add_hook(ToolBoundary {
                host: host.clone(),
                namespace: provider == OperatorProvider::ChatGpt,
            })
            .tool(BashTool { bash, host })
            .build(),
    )
}
const PREAMBLE: &str = "You are OVRCR Operator. Act only on the user's requested task. Use bash to operate the matching OVRCR CLI through \"$OVRCR_EXECUTABLE\" --json with the inherited instance configuration and socket. Honor refusals and exact session/run identities; never guess control authority. The captured context is fixed for this message; later UI navigation does not retarget this work. Do not read or upload terminal content unless requested. Do not inspect credentials or personal agent profiles. Do not start watchers, schedulers or workflow-controller authority. Stop cancels this turn only: previously accepted OVRCR operations may finish, so never claim rollback or replay an interrupted command. If context is exhausted, ask the user to start New Conversation.";

#[derive(Clone)]
struct Host {
    sender: mpsc::Sender<HostEvent>,
    call: Arc<Mutex<Option<String>>>,
}
enum HostEvent {
    Text(String),
    Boundary(OperatorWorkerEvent, oneshot::Sender<bool>),
    Flush(oneshot::Sender<bool>),
    Stop,
}
impl Host {
    async fn boundary(&self, event: OperatorWorkerEvent) -> Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(HostEvent::Boundary(event, sender))
            .await
            .map_err(|_| anyhow!("operator reporting unavailable"))?;
        if receiver.await.unwrap_or(false) {
            Ok(())
        } else {
            bail!("operator reporting unavailable")
        }
    }
    async fn flush(&self) -> Result<()> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(HostEvent::Flush(sender))
            .await
            .map_err(|_| anyhow!("operator reporting unavailable"))?;
        if receiver.await.unwrap_or(false) {
            Ok(())
        } else {
            bail!("operator reporting unavailable")
        }
    }
}
struct CompletedTurnGuard;
impl AgentHook for CompletedTurnGuard {
    async fn on_model_turn_finished(
        &self,
        _: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> ModelTurnAction {
        // Rig can accept nonempty truncated output, including calls. Reject before effects/history.
        match event.finish_reason {
            Some(
                rig::completion::FinishReason::Stop | rig::completion::FinishReason::ToolCalls,
            ) => ModelTurnAction::Continue,
            _ => ModelTurnAction::stop("operator provider did not complete this model turn"),
        }
    }
}

struct ToolBoundary {
    host: Host,
    namespace: bool,
}
impl AgentHook for ToolBoundary {
    async fn on_dispatch(&self, _: &HookContext, event: DispatchEvent<'_>) -> DispatchAction {
        if self.namespace && event.completion_request().is_some() {
            let mut patched = event.kind.clone();
            if let EffectKind::Completion { request, .. } = &mut patched {
                let request = Arc::make_mut(request);
                // Rig0.45 exposes bare function tools; SIWC requires namespaced functions.
                // Patch only the outbound request: Rig retains its original tool dispatch allowlist.
                let tools: Vec<_> = std::mem::take(&mut request.tools)
                    .into_iter()
                    .map(|tool| {
                        serde_json::json!({
                            "type":"function", "name":tool.name, "description":tool.description,
                            "parameters":tool.parameters, "strict":false,
                        })
                    })
                    .collect();
                request.additional_params.as_mut().and_then(serde_json::Value::as_object_mut)
                    .expect("operator request has parameters").insert("tools".into(), serde_json::json!([{
                        "type":"namespace", "name":"ovrcr", "description":"Operate the user's OVRCR instance", "tools":tools,
                    }]));
            }
            return DispatchAction::Patch(patched);
        }
        let Some(name) = event.tool_name() else {
            return DispatchAction::Proceed;
        };
        let Some(id) = event.call_id else {
            return DispatchAction::stop("tool identity unavailable");
        };
        let id = id.to_string();
        if let Ok(mut call) = self.host.call.lock() {
            *call = Some(id.clone());
        }
        let result = self
            .host
            .boundary(OperatorWorkerEvent::ToolCall {
                id,
                name: name.to_owned(),
                arguments: event.tool_args().unwrap_or("{}").to_owned(),
            })
            .await;
        if result.is_ok() {
            DispatchAction::Proceed
        } else {
            DispatchAction::stop("operator reporting unavailable")
        }
    }
}
struct BashTool {
    bash: Bash,
    host: Host,
}
#[derive(Debug)]
struct BashError;
impl std::fmt::Display for BashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bash could not run")
    }
}
impl std::error::Error for BashError {}
impl Tool for BashTool {
    const NAME: &'static str = "bash";
    type Args = BashArgs;
    type Output = BashOutput;
    type Error = BashError;
    fn description(&self) -> String {
        "Run bash --noprofile --norc -c. Use the matching $OVRCR_EXECUTABLE --json. Combined output is capped at 64 KiB. Default timeout is 120 seconds, maximum 600. A timeout stops this entire turn; accepted external operations may still finish.".into()
    }
    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type":"object", "properties": {
        "command":{"type":"string"}, "cwd":{"type":"string"}, "timeout_seconds":{"type":"integer", "minimum":1, "maximum":600}
    }, "required":["command"], "additionalProperties":false})
    }
    async fn call(
        &self,
        _: &mut ToolContext,
        args: BashArgs,
    ) -> std::result::Result<BashOutput, BashError> {
        let id = self
            .host
            .call
            .lock()
            .ok()
            .and_then(|call| call.clone())
            .ok_or(BashError)?;
        let mut execution = match self.bash.execute(args).await {
            Ok(execution) => execution,
            Err(error) => {
                let result = BashOutput {
                    output: error.to_string(),
                    exit_code: None,
                    truncated: false,
                    timed_out: false,
                };
                self.host
                    .boundary(OperatorWorkerEvent::ToolOutput {
                        id,
                        output: result.output.clone(),
                        truncated: false,
                        status: OperatorToolStatus::Failed { exit_code: None },
                    })
                    .await
                    .map_err(|_| BashError)?;
                return Ok(result);
            }
        };
        let status = if execution.result.timed_out {
            OperatorToolStatus::TimedOut
        } else if execution.result.exit_code == Some(0) {
            OperatorToolStatus::Succeeded
        } else {
            OperatorToolStatus::Failed {
                exit_code: execution.result.exit_code,
            }
        };
        self.host
            .boundary(OperatorWorkerEvent::ToolOutput {
                id,
                output: execution.result.output.clone(),
                truncated: execution.result.truncated,
                status,
            })
            .await
            .map_err(|_| BashError)?;
        if execution.child.is_some() {
            self.host
                .sender
                .send(HostEvent::Stop)
                .await
                .map_err(|_| BashError)?;
            // Never return an uncertain timed-out command to the Rig loop for retry.
            let _child = execution.child.take();
            std::future::pending::<()>().await;
        }
        Ok(execution.result)
    }
}
async fn flush_text(
    reporter: &SharedReporter,
    identity: OperatorTurnIdentity,
    sequence: &mut u64,
    text: &mut String,
) -> Result<()> {
    if !text.is_empty() {
        commit(
            reporter,
            identity,
            sequence,
            OperatorWorkerEvent::AssistantDelta(std::mem::take(text)),
        )
        .await?;
    }
    Ok(())
}
async fn run_turn(
    reporter: &SharedReporter,
    sequence: &mut u64,
    start: OperatorWorkerStart,
) -> Result<OperatorModelHistory> {
    let instance = ovrcr_runtime::config::RegistryPath::resolve()
        .map_err(|_| anyhow!("operator instance unavailable"))?
        .0;
    let base = diagnostic_base(
        std::env::var_os("OVRCR_OPERATOR_TEST_API_BASE"),
        cfg!(feature = "acceptance-diagnostics"),
    )?
    .unwrap_or_else(|| production_base(start.provider).into());
    let auth = AuthManager::new(&instance)?;
    // Establish setup errors before building a stream; the SDK resolves again for each actual request.
    drop(auth.resolve(start.provider, &AuthCancel::default()).await?);
    let executable =
        std::env::current_exe().map_err(|_| anyhow!("operator executable unavailable"))?;
    let cwd = start
        .context
        .cwd
        .clone()
        .unwrap_or_else(|| instance.join("operator-workdir"));
    if start.context.cwd.is_none() {
        std::fs::create_dir_all(&cwd).map_err(|_| anyhow!("operator workdir unavailable"))?;
    }
    let history: Vec<Message> = match start.history {
        Some(history) => {
            history.validate().map_err(anyhow::Error::msg)?;
            serde_json::from_str(&history.json).map_err(|_| {
                anyhow!("saved model context is unsupported; start New Conversation")
            })?
        }
        None => Vec::new(),
    };
    let (sender, mut receiver) = mpsc::channel(32);
    let host = Host {
        sender,
        call: Arc::new(Mutex::new(None)),
    };
    let agent = make_agent(
        auth,
        start.provider,
        &start.model,
        base,
        Bash::new(cwd, executable),
        host.clone(),
    )?;
    let prompt = format!(
        "Captured OVRCR context (identities are fixed for this turn):\n{}\n\nUser request:\n{}",
        serde_json::to_string(&start.context)?,
        start.text
    );
    let drive = async {
        let mut stream = agent
            .prompt(prompt)
            .history(history.clone())
            .tool_concurrency(1)
            .max_invalid_tool_call_retries(0)
            .stream();
        let mut final_history = None;
        let result = loop {
            match stream.next().await {
                Some(Ok(MultiTurnStreamItem::StreamAssistantItem(Item::Event(
                    StreamEvent::Text { text, .. },
                )))) => {
                    host.sender
                        .send(HostEvent::Text(text))
                        .await
                        .map_err(|_| anyhow!("operator reporting unavailable"))?;
                }
                Some(Ok(MultiTurnStreamItem::FinalResponse(response))) => {
                    let mut messages = history.clone();
                    messages.extend(response.messages);
                    let saved = OperatorModelHistory {
                        version: 1,
                        json: serde_json::to_string(&messages)?,
                    };
                    saved
                        .validate()
                        .map_err(|_| anyhow!("model context is full; start New Conversation"))?;
                    final_history = Some(saved);
                    break Ok(());
                }
                Some(Ok(_)) => {}
                Some(Err(_)) => {
                    break Err(anyhow!(
                        "operator provider request failed; check sign-in and model, then retry or start New Conversation"
                    ));
                }
                None => {
                    break Err(anyhow!(
                        "operator provider stream ended without a final response"
                    ));
                }
            }
        };
        host.flush().await?;
        result?;
        final_history.ok_or_else(|| anyhow!("operator final model context unavailable"))
    };
    tokio::pin!(drive);
    let mut text = String::new();
    let mut cadence = tokio::time::interval(Duration::from_millis(50));
    cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            event = receiver.recv() => match event {
                Some(HostEvent::Text(fragment)) => {
                    let mut remaining = fragment.as_str();
                    while !remaining.is_empty() {
                        let mut count = remaining.len().min(BATCH_BYTES - text.len());
                        while !remaining.is_char_boundary(count) { count -= 1; }
                        if count == 0 { flush_text(reporter, start.identity, sequence, &mut text).await?; continue; }
                        text.push_str(&remaining[..count]); remaining = &remaining[count..];
                        if text.len() == BATCH_BYTES { flush_text(reporter, start.identity, sequence, &mut text).await?; }
                    }
                }
                Some(HostEvent::Boundary(event, ack)) => {
                    let flushed = flush_text(reporter, start.identity, sequence, &mut text).await;
                    let result = if flushed.is_ok() { commit(reporter, start.identity, sequence, event).await } else { flushed };
                    let _ = ack.send(result.is_ok()); result?;
                }
                Some(HostEvent::Flush(ack)) => { let result = flush_text(reporter, start.identity, sequence, &mut text).await; let _ = ack.send(result.is_ok()); result?; }
                Some(HostEvent::Stop) => {
                    // Stop may kill us before its response. Refusal/EOF never resumes the model.
                    if retire(reporter, start.identity, true).await.is_err() {
                        eprintln!("Operator timeout stop unconfirmed; use Stop before retrying.");
                    }
                    std::future::pending::<()>().await;
                }
                None => bail!("operator event channel closed"),
            },
            result = &mut drive => { flush_text(reporter, start.identity, sequence, &mut text).await?; return result; }
            _ = cadence.tick(), if !text.is_empty() => { flush_text(reporter, start.identity, sequence, &mut text).await?; }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_endpoint_is_explicit_loopback_only_and_feature_gated() {
        for value in [
            "https://api.openai.com/v1",
            "http://localhost:80",
            "http://192.0.2.1",
            "http://127.0.0.1/?token=x",
            "http://u:p@127.0.0.1",
        ] {
            assert!(diagnostic_base(Some(value.into()), true).is_err());
        }
        assert!(diagnostic_base(Some("http://127.0.0.1:1234".into()), false).is_err());
        assert_eq!(
            diagnostic_base(Some("http://127.0.0.1:1234/".into()), true)
                .unwrap()
                .unwrap(),
            "http://127.0.0.1:1234"
        );
        assert!(diagnostic_base(None, false).unwrap().is_none());
    }
    fn seed(auth_root: &std::path::Path, provider: OperatorProvider, token: &str) {
        use sha2::{Digest, Sha256};
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let name = match provider {
            OperatorProvider::ChatGpt => "chatgpt",
            OperatorProvider::Grok => "grok",
        };
        let path = auth_root
            .join("operator-auth")
            .join(format!("{name}-{:x}.json", Sha256::digest(b"default")));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        write!(file, "{}", serde_json::json!({"client_id":"fixture", "subject":"fixture", "issuer":"https://auth.openai.com", "access_token":token,
            "refresh_token":"synthetic-refresh", "expires_at":4102444800u64, "scopes":["chatgpt.tokens.use.direct"]})).unwrap();
    }
    async fn request(socket: &mut tokio::net::TcpStream) -> (String, serde_json::Value) {
        use tokio::io::AsyncReadExt;
        let mut bytes = Vec::new();
        loop {
            let mut block = [0; 4096];
            let n = socket.read(&mut block).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&block[..n]);
            if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                let len = headers
                    .lines()
                    .find_map(|l| {
                        l.strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap();
                if bytes.len() >= end + 4 + len {
                    return (
                        headers,
                        serde_json::from_slice(&bytes[end + 4..end + 4 + len]).unwrap(),
                    );
                }
            }
        }
    }
    async fn event(socket: &mut tokio::net::TcpStream, value: serde_json::Value) {
        use tokio::io::AsyncWriteExt;
        socket
            .write_all(
                format!(
                    "event: {}\ndata: {value}\n\n",
                    value["type"].as_str().unwrap()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn released_rig_both_provider_wires_stream_stateless_full_history_and_fresh_credentials()
    {
        use serde_json::json;
        use tokio::io::AsyncWriteExt;
        for provider in [OperatorProvider::ChatGpt, OperatorProvider::Grok] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let (release, mut gate) = mpsc::channel(2);
            let server = tokio::spawn(async move {
                let mut captured = Vec::new();
                for index in 0..2 {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    captured.push(request(&mut socket).await);
                    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
                    let reasoning = json!({"type":"reasoning","id":"rs-fixture","summary":[],"encrypted_content":"opaque-reasoning-fixture"});
                    event(&mut socket,json!({"type":"response.output_item.added","output_index":0,"item":reasoning})).await;
                    event(&mut socket,json!({"type":"response.output_item.done","output_index":0,"item":reasoning})).await;
                    event(&mut socket,json!({"type":"response.output_item.added","output_index":1,"item":{"type":"message","id":"msg-fixture","role":"assistant","content":[]}})).await;
                    event(&mut socket,json!({"type":"response.output_text.delta","output_index":1,"content_index":0,"item_id":"msg-fixture","delta":"early"})).await;
                    // The consumer must observe a text fragment before this final event is released.
                    assert_eq!(gate.recv().await, Some(index));
                    let output = json!({"type":"message","id":"msg-fixture","role":"assistant","status":"completed","content":[{"type":"output_text","text":"early","annotations":[]}]});
                    event(
                        &mut socket,
                        json!({"type":"response.output_item.done","output_index":1,"item":output}),
                    )
                    .await;
                    event(&mut socket,json!({"type":"response.completed","response":{"id":"resp-fixture","status":"completed","output":[reasoning,output],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}})).await;
                }
                captured
            });
            let root = tempfile::tempdir().unwrap();
            let auth = AuthManager::new(root.path()).unwrap();
            seed(root.path(), provider, "synthetic-first");
            let (sender, _receiver) = mpsc::channel(32);
            let host = Host {
                sender,
                call: Arc::new(Mutex::new(None)),
            };
            let agent = make_agent(
                auth,
                provider,
                "fixture-model",
                base,
                Bash::new(root.path().into(), std::env::current_exe().unwrap()),
                host,
            )
            .unwrap();
            let mut history = Vec::<Message>::new();
            for index in 0..2 {
                if index == 1 {
                    seed(root.path(), provider, "synthetic-second");
                }
                let mut stream = agent
                    .prompt(if index == 0 {
                        "first request"
                    } else {
                        "second request"
                    })
                    .history(history.clone())
                    .tool_concurrency(1)
                    .stream();
                let mut partial = false;
                let mut completed = false;
                while let Some(item) = tokio::time::timeout(Duration::from_secs(5), stream.next())
                    .await
                    .unwrap()
                {
                    match item.unwrap() {
                        MultiTurnStreamItem::StreamAssistantItem(Item::Event(
                            StreamEvent::Text { text, .. },
                        )) => {
                            assert_eq!(text, "early");
                            partial = true;
                            release.send(index).await.unwrap();
                        }
                        MultiTurnStreamItem::FinalResponse(response) => {
                            assert!(partial);
                            history.extend(response.messages);
                            completed = true;
                        }
                        _ => {}
                    }
                }
                assert!(completed);
            }
            let captured = server.await.unwrap();
            assert_eq!(captured.len(), 2);
            for (index, (headers, body)) in captured.iter().enumerate() {
                let path = if provider == OperatorProvider::ChatGpt {
                    "/responses"
                } else {
                    "/v1/responses"
                };
                assert!(headers.starts_with(&format!("post {path} ")));
                assert!(headers.contains(if index == 0 {
                    "bearer synthetic-first"
                } else {
                    "bearer synthetic-second"
                }));
                assert_eq!(body["stream"], true);
                assert_eq!(body["store"], false);
                for key in [
                    "temperature",
                    "max_output_tokens",
                    "max_tool_calls",
                    "previous_response_id",
                    "conversation",
                    "background",
                    "top_p",
                    "metadata",
                    "truncation",
                ] {
                    assert!(body.get(key).is_none(), "forbidden {key}: {body}");
                }
                assert!(body["input"].is_array());
                if index == 1 {
                    let input = body["input"].to_string();
                    assert!(input.contains("first request"));
                    assert!(input.contains("second request"));
                    assert!(input.contains("early"));
                    assert!(
                        input.contains("opaque-reasoning-fixture"),
                        "stateless replay must retain encrypted reasoning: {input}"
                    );
                }
                if provider == OperatorProvider::ChatGpt {
                    assert_eq!(body["tools"].as_array().unwrap().len(), 1);
                    assert_eq!(
                        body["tools"][0]["type"], "namespace",
                        "SIWC tools must be grouped: {body}"
                    );
                    assert_eq!(body["tools"][0]["tools"][0]["name"], "bash");
                } else {
                    assert_eq!(body["tools"][0]["name"], "bash");
                }
            }
        }
    }

    #[tokio::test]
    async fn timed_out_bash_publishes_output_requests_stop_and_never_returns_to_rig() {
        let root = tempfile::tempdir().unwrap();
        let (sender, mut receiver) = mpsc::channel(32);
        let host = Host {
            sender,
            call: Arc::new(Mutex::new(Some("timeout-call".into()))),
        };
        let tool = BashTool {
            bash: Bash::new(root.path().into(), std::env::current_exe().unwrap()),
            host,
        };
        let mut task = tokio::spawn(async move {
            tool.call(
                &mut ToolContext::default(),
                BashArgs {
                    command: "exec sleep 1.05".into(),
                    cwd: None,
                    timeout_seconds: Some(1),
                },
            )
            .await
        });
        let event = tokio::time::timeout(Duration::from_secs(3), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        match event {
            HostEvent::Boundary(OperatorWorkerEvent::ToolOutput { id, status, .. }, ack) => {
                assert_eq!(id, "timeout-call");
                assert_eq!(status, OperatorToolStatus::TimedOut);
                ack.send(true).unwrap();
            }
            _ => panic!("timeout must publish its actual partial result first"),
        }
        assert!(matches!(receiver.recv().await, Some(HostEvent::Stop)));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut task)
                .await
                .is_err(),
            "an uncertain tool cannot return to the model loop"
        );
        // This short fixture child naturally finishes; only the test task is aborted.
        tokio::time::sleep(Duration::from_millis(70)).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::task::yield_now().await;
    }

    #[tokio::test]
    async fn chatgpt_namespaced_call_executes_real_bash_and_preserves_provider_context() {
        use serde_json::json;
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let first = request(&mut socket).await.1;
            assert_eq!(first["tools"][0]["type"], "namespace");
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
            let args = json!({"command":"printf namespaced-bash"}).to_string();
            let call = json!({"type":"function_call","id":"fc-namespace","call_id":"namespace-call","namespace":"ovrcr","name":"bash","arguments":args,"status":"completed"});
            event(&mut socket,json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc-namespace","call_id":"namespace-call","namespace":"ovrcr","name":"bash","arguments":""}})).await;
            event(&mut socket,json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":args})).await;
            event(
                &mut socket,
                json!({"type":"response.output_item.done","output_index":0,"item":call}),
            )
            .await;
            event(&mut socket,json!({"type":"response.completed","response":{"id":"resp-call","status":"completed","output":[call]}})).await;
            drop(socket);
            let (mut socket, _) = listener.accept().await.unwrap();
            let second = request(&mut socket).await.1;
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
            let text = json!({"type":"message","id":"msg-done","role":"assistant","status":"completed","content":[{"type":"output_text","text":"done","annotations":[]}]});
            event(&mut socket,json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg-done","role":"assistant","content":[]}})).await;
            event(&mut socket,json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"done"})).await;
            event(
                &mut socket,
                json!({"type":"response.output_item.done","output_index":0,"item":text}),
            )
            .await;
            event(&mut socket,json!({"type":"response.completed","response":{"id":"resp-done","status":"completed","output":[text]}})).await;
            second
        });
        let root = tempfile::tempdir().unwrap();
        let auth = AuthManager::new(root.path()).unwrap();
        seed(root.path(), OperatorProvider::ChatGpt, "synthetic-chat");
        let (sender, mut receiver) = mpsc::channel(32);
        let host = Host {
            sender,
            call: Arc::new(Mutex::new(None)),
        };
        let pump = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(event) = receiver.recv().await {
                match event {
                    HostEvent::Boundary(event, ack) => {
                        events.push(event);
                        ack.send(true).unwrap();
                    }
                    _ => panic!("unexpected host event"),
                }
            }
            events
        });
        let agent = make_agent(
            auth,
            OperatorProvider::ChatGpt,
            "fixture-chat",
            base,
            Bash::new(root.path().into(), std::env::current_exe().unwrap()),
            host,
        )
        .unwrap();
        let mut stream = agent.prompt("use Bash once").tool_concurrency(1).stream();
        let mut completed = false;
        while let Some(item) = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
        {
            if let MultiTurnStreamItem::FinalResponse(response) = item.unwrap() {
                assert_eq!(response.output(), "done");
                completed = true;
            }
        }
        assert!(completed);
        drop(stream);
        drop(agent);
        let second = server.await.unwrap();
        let call = second["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "function_call")
            .unwrap();
        assert_eq!(
            call["namespace"], "ovrcr",
            "SDK replay must retain the provider namespace"
        );
        assert!(second["input"].to_string().contains("namespaced-bash"));
        let events = pump.await.unwrap();
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0],OperatorWorkerEvent::ToolCall {id,name,..} if id=="namespace-call" && name=="bash")
        );
        assert!(
            matches!(&events[1],OperatorWorkerEvent::ToolOutput {output,status:OperatorToolStatus::Succeeded,..} if output=="namespaced-bash")
        );
    }
    async fn assert_incomplete_rejected(provider: OperatorProvider, with_tool: bool, reason: &str) {
        use serde_json::json;
        use tokio::io::AsyncWriteExt;
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("bash-ran");
        let command = format!("printf performed > '{}'", marker.display());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let reason = reason.to_owned();
        let (finished, mut stop) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut requests = 0;
            loop {
                let accepted = tokio::select! {
                    _ = &mut stop => break,
                    accepted = listener.accept() => accepted.unwrap(),
                };
                let (mut socket, _) = accepted;
                request(&mut socket).await;
                requests += 1;
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").await.unwrap();
                let text = json!({"type":"message","id":"msg-incomplete","role":"assistant","status":"completed","content":[{"type":"output_text","text":"visible partial output","annotations":[]}]});
                event(&mut socket,json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg-incomplete","role":"assistant","content":[]}})).await;
                event(&mut socket,json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"item_id":"msg-incomplete","delta":"visible partial output"})).await;
                event(
                    &mut socket,
                    json!({"type":"response.output_item.done","output_index":0,"item":text}),
                )
                .await;
                let mut output = vec![text];
                if with_tool && requests == 1 {
                    let args = json!({"command":command}).to_string();
                    let call = json!({"type":"function_call","id":"fc-incomplete","call_id":"incomplete-call","namespace":"ovrcr","name":"bash","arguments":args,"status":"completed"});
                    event(&mut socket,json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"fc-incomplete","call_id":"incomplete-call","namespace":"ovrcr","name":"bash","arguments":""}})).await;
                    event(&mut socket,json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":args})).await;
                    event(
                        &mut socket,
                        json!({"type":"response.output_item.done","output_index":1,"item":call}),
                    )
                    .await;
                    output.push(call);
                }
                if requests == 1 {
                    event(&mut socket,json!({"type":"response.incomplete","response":{"id":"resp-incomplete","status":"incomplete","incomplete_details":{"reason":reason},"output":output}})).await;
                } else {
                    // A buggy implementation may execute the incomplete call and ask again.
                    event(&mut socket,json!({"type":"response.completed","response":{"id":"resp-unexpected-retry","status":"completed","output":output}})).await;
                }
            }
            requests
        });
        let auth = AuthManager::new(root.path()).unwrap();
        seed(root.path(), provider, "synthetic-incomplete");
        let (sender, mut receiver) = mpsc::channel(32);
        let host = Host {
            sender,
            call: Arc::new(Mutex::new(None)),
        };
        let pump = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(event) = receiver.recv().await {
                if let HostEvent::Boundary(event, ack) = event {
                    events.push(event);
                    ack.send(true).unwrap();
                }
            }
            events
        });
        let agent = make_agent(
            auth,
            provider,
            "fixture-incomplete",
            base,
            Bash::new(root.path().into(), std::env::current_exe().unwrap()),
            host,
        )
        .unwrap();
        let mut stream = agent
            .prompt("do not accept incomplete output")
            .tool_concurrency(1)
            .max_invalid_tool_call_retries(0)
            .stream();
        let mut partial = String::new();
        let mut completed = false;
        let mut failed = false;
        while let Some(item) = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
        {
            match item {
                Ok(MultiTurnStreamItem::StreamAssistantItem(Item::Event(StreamEvent::Text {
                    text,
                    ..
                }))) => partial.push_str(&text),
                Ok(MultiTurnStreamItem::FinalResponse(_)) => completed = true,
                Err(_) => failed = true,
                _ => {}
            }
        }
        finished.send(()).unwrap();
        let requests = server.await.unwrap();
        drop(stream);
        drop(agent);
        let events = pump.await.unwrap();
        assert!(
            partial.contains("visible partial output"),
            "partial text remains observable"
        );
        assert!(
            !marker.exists(),
            "an incomplete model turn must never execute Bash"
        );
        assert_eq!(
            requests, 1,
            "an incomplete model turn must never be retried"
        );
        assert!(
            events.is_empty(),
            "no tool-dispatch boundaries are accepted for an incomplete model turn"
        );
        assert!(
            !completed,
            "nonempty incomplete output is not completed model context"
        );
        assert!(
            failed,
            "an incomplete model turn must fail the worker stream"
        );
    }
    #[tokio::test]
    async fn chatgpt_incomplete_text_is_not_completed_history() {
        assert_incomplete_rejected(OperatorProvider::ChatGpt, false, "max_output_tokens").await;
    }
    #[tokio::test]
    async fn grok_incomplete_text_is_not_completed_history() {
        assert_incomplete_rejected(OperatorProvider::Grok, false, "max_output_tokens").await;
    }
    #[tokio::test]
    async fn chatgpt_incomplete_tool_call_never_executes_bash_or_retries() {
        assert_incomplete_rejected(OperatorProvider::ChatGpt, true, "max_output_tokens").await;
    }
    #[tokio::test]
    async fn grok_incomplete_tool_call_never_executes_bash_or_retries() {
        assert_incomplete_rejected(OperatorProvider::Grok, true, "max_output_tokens").await;
    }
    #[tokio::test]
    async fn both_providers_reject_filtered_and_unknown_terminal_outcomes() {
        for provider in [OperatorProvider::ChatGpt, OperatorProvider::Grok] {
            for reason in ["content_filter", "unknown_fixture_reason"] {
                assert_incomplete_rejected(provider, false, reason).await;
            }
        }
    }
}

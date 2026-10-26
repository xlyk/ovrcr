//! Exact-run SDK 1.0.40 observation; no inference, replay or lifecycle owner.
use super::*;
use ovrcr_protocol::{
    CloudCreationCommand as C, CloudCreationIntent, CloudRunStatus, CloudSendIntent,
};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Control {
    pub target: std::sync::Mutex<Option<(String, String)>>,
    attempted: std::sync::atomic::AtomicBool,
    serviced: std::sync::atomic::AtomicBool,
}
const SOURCE_PROBE: &str = "git rev-parse HEAD";
const TYPES: &[&str] = &[
    "system",
    "assistant",
    "user",
    "tool_call",
    "status",
    "thinking",
];
fn identity(value: &Value, agent: &str, run: &str) -> Result<()> {
    ensure!(
        value["agentId"] == agent && value["runId"] == run,
        "exact run identity mismatch"
    );
    Ok(())
}
fn terminal(value: &Value) -> Result<CloudRunStatus> {
    Ok(match value.as_str() {
        Some("RUN_LIFECYCLE_STATUS_FINISHED") => CloudRunStatus::Finished,
        Some("RUN_LIFECYCLE_STATUS_ERROR") => CloudRunStatus::Error,
        Some("RUN_LIFECYCLE_STATUS_CANCELLED") => CloudRunStatus::Cancelled,
        Some("RUN_LIFECYCLE_STATUS_EXPIRED") => CloudRunStatus::Expired,
        _ => bail!("authoritative terminal status unavailable"),
    })
}
fn source_tool(message: &Value, call: &mut Option<(String, String)>, sha: &str) -> Result<bool> {
    let name = message["name"]
        .as_str()
        .context("source tool name missing")?;
    let id = message["call_id"]
        .as_str()
        .context("source call identity missing")?;
    ensure!(
        !id.is_empty()
            && id.len() <= 256
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            && matches!(name, "shell" | "run_terminal_cmd")
            && message["truncated"] != true,
        "source tool unrecognized"
    );
    let status = message["status"]
        .as_str()
        .context("source tool status missing")?;
    if let Some((old_id, old_name)) = call.as_ref() {
        ensure!(
            old_id == id && old_name == name,
            "source tool correlation mismatch"
        );
    }
    let command = message["args"]["command"].as_str();
    ensure!(
        command == Some(SOURCE_PROBE)
            || command == Some("cd /workspace && git rev-parse HEAD")
            || (name == "run_terminal_cmd" && status == "running" && message.get("args").is_none()),
        "source command unverified"
    );
    if status == "running" {
        *call = Some((id.into(), name.into()));
        return Ok(false);
    }
    ensure!(
        status == "completed" && call.is_some(),
        "source tool completion without correlated start"
    );
    let result = &message["result"];
    let success = if name == "run_terminal_cmd" {
        ensure!(
            result["isBackground"] == false && result.as_object().is_some_and(|v| v.len() == 2),
            "background/unknown source result"
        );
        let value = &result["success"];
        ensure!(
            value["command"].as_str() == command && value["interleavedOutput"] == value["stdout"],
            "source result command/output mismatch"
        );
        value
    } else if result.get("status").is_some() {
        ensure!(
            result["status"] == "success"
                && result.as_object().is_some_and(|v| v.len() == 2)
                && result["value"].is_object()
                && result["value"]["signal"] == "",
            "shell result discriminator is not a recognized success"
        );
        &result["value"]
    } else {
        ensure!(
            result.as_object().is_some_and(|v| v.len() == 3
                && ["stdout", "stderr", "exitCode"]
                    .iter()
                    .all(|k| v.contains_key(*k))),
            "unrecognized flat shell success shape"
        );
        result
    };
    if name == "shell" {
        ensure!(
            success["exitCode"].as_i64() == Some(0) && success["stderr"] == "",
            "shell success receipt incomplete"
        );
    }
    ensure!(
        success["stdout"] == format!("{sha}\n")
            && success.get("stderr").is_none_or(|v| v == "")
            && success.get("signal").is_none_or(|v| v == "")
            && success
                .get("exitCode")
                .is_none_or(|v| v.as_i64() == Some(0)),
        "source HEAD not independently confirmed"
    );
    Ok(true)
}

impl Catalog {
    fn send(&self, request: &Value, mut receive: impl FnMut(Value) -> Result<()>) -> Result<()> {
        let bytes = serde_json::to_vec(request)?;
        let mut payload = vec![0];
        payload.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        payload.extend_from_slice(&bytes);
        let response = self
            .agent
            .post(&format!("{}/sdk.v1.SdkAgentService/Send", self.endpoint))
            .timeout(self.deadline.saturating_duration_since(Instant::now()))
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/connect+json")
            .set("Connect-Protocol-Version", "1")
            .send_bytes(&payload)
            .map_err(|_| {
                anyhow::anyhow!(
                    "Send observation disconnected; remote work may continue; do not retry"
                )
            })?;
        ensure!(
            response.status() == 200
                && response
                    .header("Content-Type")
                    .is_some_and(|v| v.split(';').next() == Some("application/connect+json")),
            "unrecognized Connect stream"
        );
        let mut reader = response.into_reader();
        let mut total = 0u64;
        for _ in 0..4096 {
            ensure!(
                Instant::now() < self.deadline,
                "Send observation deadline; remote state unknown"
            );
            let mut header = [0u8; 5];
            reader
                .read_exact(&mut header)
                .context("Send stream ended without complete receipt; remote state unknown")?;
            let len = u32::from_be_bytes(header[1..].try_into()?) as u64;
            total += len + 5;
            ensure!(
                matches!(header[0], 0 | 2) && len <= LIMIT && total <= 16 * LIMIT,
                "Connect stream bounds/flags invalid"
            );
            let mut data = vec![0; len as usize];
            reader.read_exact(&mut data)?;
            let value: Value = serde_json::from_slice(&data)?;
            ensure!(value.is_object(), "Connect object required");
            if header[0] == 2 {
                ensure!(
                    value.get("error").is_none(),
                    "Connect terminal error; outcome unverified"
                );
                let mut extra = [0];
                ensure!(reader.read(&mut extra)? == 0, "bytes after Connect end");
                return Ok(());
            }
            receive(value)?;
        }
        bail!("Connect frame bound reached; remote state unknown")
    }
}

pub(super) fn send(
    catalog: &mut Catalog,
    creation: &CloudCreationIntent,
    binding: (&str, &str, &Control),
    prompt: String,
    source_required: bool,
    fixture: Option<&Value>,
    admit: &mut impl FnMut(&C) -> Result<()>,
) -> Result<bool> {
    let (agent, key, control) = binding;
    let intent = CloudSendIntent {
        key: crate::agent_runner::private_identifier()?,
        creation_key: creation.key.clone(),
        agent_id: agent.into(),
        credential_generation: format!("{:x}", Sha256::digest(key.as_bytes())),
        model: creation.account.model.clone(),
        prompt: if source_required {
            format!(
                "Before any other tool, run exactly `git rev-parse HEAD` and wait for its result to verify the selected source. Then follow this request:\n{prompt}"
            )
        } else {
            prompt
        },
    };
    intent.validate()?;
    admit(&C::PrepareSend(intent.clone()))?;
    send_boundary(fixture, "before_dispatch")?;
    admit(&C::ClaimSend {
        key: intent.key.clone(),
        credential_generation: intent.credential_generation.clone(),
    })?;
    send_boundary(fixture, "after_claim")?;
    catalog.deadline = Instant::now() + Duration::from_secs(900);
    println!("Busy: Send claimed; remote work may continue after disconnect.");
    let mut run: Option<String> = None;
    let mut source = !source_required;
    let mut probe = None;
    let mut result: Option<Value> = None;
    let mut done = false;
    let mut seen = HashSet::new();
    let observed = catalog.send(&intent.request(), |envelope| {
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&envelope)?));
        if !seen.insert(digest) {
            return Ok(());
        }
        ensure!(!done, "frame after done; outcome unverified");
        ensure!(
            ["sdkMessage", "result", "done", "interactionUpdate", "step"]
                .iter()
                .filter(|n| envelope.get(**n).is_some())
                .count()
                == 1,
            "ambiguous/unknown SDK envelope"
        );
        if let Some(sdk) = envelope.get("sdkMessage") {
            let kind = sdk["type"].as_str().context("SDK type missing")?;
            ensure!(
                TYPES.contains(&kind),
                "SDK shape unrecognized; outcome unverified"
            );
            let message = &sdk["message"];
            let id = message["run_id"]
                .as_str()
                .context("original stream run identity missing")?;
            ensure!(
                message["agent_id"] == agent
                    && message
                        .get("model")
                        .is_none_or(|v| *v == json!({"id":creation.account.model})),
                "SDK conversation/model mismatch"
            );
            if run.is_none() {
                send_boundary(fixture, "after_reply")?;
                admit(&C::AcknowledgeSend {
                    key: intent.key.clone(),
                    agent_id: agent.into(),
                    run_id: id.into(),
                })?;
                run = Some(id.into());
                *control.target.lock().unwrap() = Some((intent.key.clone(), id.into()));
                cancel(catalog, agent, &intent.key, id, admit, control);
                send_boundary(fixture, "after_ack")?;
                let info = catalog.rpc(
                    "SdkAgentService",
                    "GetAgent",
                    &json!({"agentId":agent,"options":{"apiKey":key}}),
                )?;
                ensure!(
                    info["agent"]["agentId"] == agent
                        && info["agent"].get("local").is_none()
                        && info["agent"]["cloud"]["repos"] == json!([creation.repository]),
                    "GetAgent cloud/repository identity mismatch"
                );
                admit(&C::ObserveSend {
                    key: intent.key.clone(),
                    agent_id: agent.into(),
                    run_id: id.into(),
                })?;
                println!("Observing exact run {id}");
            }
            ensure!(run.as_deref() == Some(id), "cross-run SDK message rejected");
            if kind == "tool_call" && !source {
                source = source_tool(message, &mut probe, &creation.source_sha)?;
            }
            if kind == "assistant" {
                // Actual SDK assistant content is a typed text block list; never infer controls.
                if let Some(content) = message["message"]["content"].as_array() {
                    for block in content {
                        if block["type"] == "text"
                            && let Some(text) = block["text"].as_str()
                        {
                            print!("{text}");
                        }
                    }
                }
            }
        } else if let Some(terminal) = envelope.get("result") {
            let id = run
                .as_deref()
                .context("terminal result before exact run receipt")?;
            identity(terminal, agent, id)?;
            identity(&terminal["result"], agent, id)?;
            ensure!(
                result.is_none()
                    && terminal["status"] == terminal["result"]["status"]
                    && (terminal["status"] != "RUN_LIFECYCLE_STATUS_FINISHED"
                        || terminal.get("errorCode").is_none_or(|v| v == "")),
                "terminal result shape/order mismatch"
            );
            terminal_status_model(&terminal["result"], &creation.account.model)?;
            result = Some(terminal["result"].clone());
        } else if let Some(end) = envelope.get("done") {
            identity(
                end,
                agent,
                run.as_deref().context("done without exact run")?,
            )?;
            ensure!(result.is_some(), "done before result");
            done = true;
        } else {
            bail!("unrecognized step/delta; result remains unverified");
        }
        Ok(())
    });
    let qualify = || -> Result<(String, CloudRunStatus, Value)> {
        observed?;
        let id = run
            .clone()
            .context("Send unresolved; original reply lost; do not retry")?;
        let result = result.context("terminal result missing")?;
        ensure!(done, "done receipt missing");
        terminal(&result["status"])?;
        let snapshot = catalog.rpc(
            "SdkAgentService",
            "GetRun",
            &json!({"runId":id,"options":{"runtime":"RUNTIME_CLOUD","agentId":agent,"apiKey":key}}),
        )?;
        identity(&snapshot["run"], agent, &id)?;
        terminal_status_model(&snapshot["run"], &creation.account.model)?;
        ensure!(
            control.attempted.load(std::sync::atomic::Ordering::Acquire)
                || snapshot["run"]["status"] == result["status"],
            "independent exact-run status mismatch"
        );
        let status = terminal(&snapshot["run"]["status"])?;
        if status == CloudRunStatus::Finished {
            ensure!(source, "source tool proof missing; result unverified");
        }
        Ok((id, status, snapshot["run"].clone()))
    }();
    match qualify {
        Ok((id, status, snapshot)) => {
            if let Err(error) = admit(&C::CompleteSend {
                key: intent.key.clone(),
                agent_id: agent.into(),
                run_id: id.clone(),
                status,
            }) {
                let _ = admit(&C::ObservationLost {
                    key: intent.key,
                    agent_id: agent.into(),
                    run_id: id,
                });
                return Err(error);
            }
            *control.target.lock().unwrap() = None;
            if status == CloudRunStatus::Finished {
                println!("\nReady: exact run {id}. Source and terminal status verified.");
            } else {
                println!("\nError: exact run {id} ended {status:?}.");
            }
            if let Some(text) = snapshot["result"].as_str() {
                println!("{text}");
            }
            show_links(catalog, creation, &snapshot)?;
            Ok(source)
        }
        Err(error) => {
            if let Some(id) = run {
                let _ = admit(&C::ObservationLost {
                    key: intent.key,
                    agent_id: agent.into(),
                    run_id: id,
                });
            }
            Err(error)
        }
    }
}
fn terminal_status_model(value: &Value, model: &str) -> Result<()> {
    let status = terminal(&value["status"])?;
    if status == CloudRunStatus::Finished {
        ensure!(
            value.get("errorCode").is_none_or(|v| v == ""),
            "finished run carries an error"
        );
    }
    ensure!(
        value.get("model").is_none_or(|v| *v == json!({"id":model})),
        "terminal model mismatch"
    );
    Ok(())
}
fn send_boundary(fixture: Option<&Value>, boundary: &str) -> Result<()> {
    let Some(fixture) = fixture else {
        return Ok(());
    };
    let mut controls = fixture.clone();
    controls["creation_crash_at"] = fixture["send_crash_at"].clone();
    controls["creation_pause_at"] = fixture["send_pause_at"].clone();
    controls["creation_checkpoint"] = fixture["send_checkpoint"].clone();
    creation_boundary(Some(&controls), boundary)
}
fn show_links(catalog: &Catalog, creation: &CloudCreationIntent, snapshot: &Value) -> Result<()> {
    let Some(branches) = snapshot["git"]["branches"].as_array() else {
        return Ok(());
    };
    for branch in branches {
        if branch["repoUrl"] != creation.repository {
            println!("Result link unverified: repository mismatch");
            continue;
        }
        // No missing-branch fallback: a branch/PR is displayed only after independent GitHub evidence.
        let verified = (|| -> Result<String> {
            let name = branch["branch"].as_str().context("result branch missing")?;
            ensure!(
                !name.is_empty() && !name.starts_with('-') && name.len() <= 256,
                "invalid branch"
            );
            let repo = creation
                .repository
                .strip_prefix("https://github.com/")
                .context("repository identity")?;
            let encoded: String = name
                .bytes()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || b"-_.".contains(&c) {
                        (c as char).to_string()
                    } else {
                        format!("%{c:02X}")
                    }
                })
                .collect();
            let data = output(
                Command::new("gh").args(["api", &format!("repos/{repo}/commits/{encoded}")]),
                catalog.deadline,
                LIMIT,
            )?;
            let commit: Value = serde_json::from_slice(&data)?;
            let sha = commit["sha"].as_str().context("published commit missing")?;
            ensure!(
                sha.len() == 40 && sha.bytes().all(|c| c.is_ascii_hexdigit()),
                "invalid commit"
            );
            let comparison: Value = serde_json::from_slice(&output(
                Command::new("gh").args([
                    "api",
                    &format!("repos/{repo}/compare/{}...{sha}", creation.source_sha),
                ]),
                catalog.deadline,
                LIMIT,
            )?)?;
            ensure!(
                comparison["merge_base_commit"]["sha"] == creation.source_sha
                    && ["ahead", "identical"]
                        .iter()
                        .any(|v| comparison["status"] == *v),
                "published result ancestry unverified"
            );
            if let Some(pr) = branch["prUrl"].as_str().filter(|s| !s.is_empty()) {
                let number = pr
                    .strip_prefix(&format!("{}/pull/", creation.repository))
                    .filter(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
                    .context("PR repository identity mismatch")?;
                let info: Value = serde_json::from_slice(&output(
                    Command::new("gh").args(["api", &format!("repos/{repo}/pulls/{number}")]),
                    catalog.deadline,
                    LIMIT,
                )?)?;
                ensure!(
                    info["html_url"] == pr
                        && info["head"]["sha"] == sha
                        && info["head"]["ref"] == name
                        && info["head"]["repo"]["html_url"] == creation.repository,
                    "PR head identity mismatch"
                );
                return Ok(format!("Verified existing PR: {pr} (commit {sha})"));
            }
            Ok(format!(
                "Verified published branch: {}/tree/{encoded} (commit {sha}; no PR)",
                creation.repository
            ))
        })();
        match verified {
            Ok(link) => println!("{link}"),
            Err(_) => println!(
                "Result link unverified: independent repository/head/ancestry evidence unavailable"
            ),
        }
    }
    Ok(())
}

/// A live original attachment may finish an explicit intent; failed claims permit no RPC.
pub(super) fn cancel(
    catalog: &Catalog,
    agent: &str,
    key: &str,
    run: &str,
    admit: &mut impl FnMut(&C) -> Result<()>,
    control: &Control,
) {
    if control.serviced.load(std::sync::atomic::Ordering::Acquire)
        || admit(&C::CheckCancel {
            key: key.into(),
            agent_id: agent.into(),
            run_id: run.into(),
        })
        .is_err()
        || control
            .serviced
            .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        return;
    }
    if admit(&C::ClaimCancel {
        key: key.into(),
        agent_id: agent.into(),
        run_id: run.into(),
    })
    .is_err()
    {
        println!(
            "Cancellation admission not durably acknowledged; remote cancellation not attempted; no automatic retry."
        );
        return;
    }
    control
        .attempted
        .store(true, std::sync::atomic::Ordering::Release);
    let mut catalog = catalog.clone();
    catalog.deadline = Instant::now() + Duration::from_secs(30);
    match catalog.rpc(
        "SdkAgentService",
        "CancelRun",
        &json!({"agentId":agent,"runId":run}),
    ) {
        Ok(value) if value == json!({}) => println!(
            "Cancellation requested for exact run {run}; authoritative terminal outcome still required."
        ),
        _ => println!("Remote cancellation unconfirmed for exact run {run}; no automatic retry."),
    }
}

/// Fresh exact lookup registers a credential-bound handle; no active stream is restored.
pub(super) fn recover(
    catalog: &Catalog,
    key: &str,
    target: &ovrcr_protocol::CloudRecovery,
    admit: &mut impl FnMut(&C) -> Result<()>,
) -> Result<()> {
    let agent = &target.receipt.agent_id;
    if target.cancel_requested {
        println!(
            "Saved cancellation intent retained; remote cancellation unconfirmed. Explicit /retry-cancel required."
        );
    }
    let info = catalog.rpc(
        "SdkAgentService",
        "GetAgent",
        &json!({"agentId":agent,"options":{"apiKey":key}}),
    )?;
    ensure!(
        info["agent"]["agentId"] == *agent
            && info["agent"].get("local").is_none()
            && info["agent"]["cloud"]["repos"] == json!([target.intent.repository]),
        "retained conversation/source binding mismatch"
    );
    let mut catalog = catalog.clone();
    let lookup = |catalog: &Catalog| -> Result<Option<CloudRunStatus>> {
        let snapshot = catalog.rpc("SdkAgentService", "GetRun", &json!({"runId":target.run_id,"options":{"runtime":"RUNTIME_CLOUD","agentId":agent,"apiKey":key}}))?;
        identity(&snapshot["run"], agent, &target.run_id)?;
        if let Ok(status) = terminal(&snapshot["run"]["status"]) {
            return Ok(Some(status));
        }
        ensure!(
            matches!(
                snapshot["run"]["status"].as_str(),
                Some("RUN_LIFECYCLE_STATUS_CREATING" | "RUN_LIFECYCLE_STATUS_RUNNING")
            ),
            "unknown exact-run lifecycle; observation remains uncertain"
        );
        Ok(None)
    };
    if let Some(status) = lookup(&catalog)? {
        admit(&C::Reconcile {
            target: target.clone(),
            status,
        })?;
        println!(
            "Reconciled exact run {}: {status:?}. Historical outcome; no new Ready or Unread.",
            target.run_id
        );
        return Ok(());
    }
    println!(
        "Active reattachment unavailable: option-preserving fresh observation is unproved. Remote work/charges may continue; no mutation issued.\nSaved cancellation intent, if present, is not dispatched automatically. Type /retry-cancel for this exact known run, or /detach. Uncertain CreateAgent/Send Retry unavailable; unresolved immutable requests stay retained."
    );
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    loop {
        let mut line = String::new();
        if input.by_ref().take(65).read_line(&mut line)? == 0 {
            return Ok(());
        }
        ensure!(
            line.len() <= 64,
            "recovery control exceeds 64 bytes; no dispatch"
        );
        match line.trim() {
            "" | "/detach" => return Ok(()),
            "/retry-cancel" => {
                catalog.deadline = Instant::now() + Duration::from_secs(30);
                // A new credentialed GET registers the exact handle before CancelRun.
                if let Some(status) = lookup(&catalog)? {
                    admit(&C::Reconcile {
                        target: target.clone(),
                        status,
                    })?;
                    println!(
                        "Reconciled exact run {}: {status:?}. Historical outcome; no new Ready or Unread.",
                        target.run_id
                    );
                    return Ok(());
                }
                admit(&C::RetryCancel(target.clone()))?;
                let response = catalog.rpc(
                    "SdkAgentService",
                    "CancelRun",
                    &json!({"agentId":agent,"runId":target.run_id}),
                );
                if matches!(response, Ok(ref value) if *value == json!({})) {
                    println!(
                        "Cancellation requested; acknowledgement does not prove remote termination."
                    );
                } else {
                    println!(
                        "Cancellation RPC failed or refused; remote cancellation unconfirmed."
                    );
                }
                // Never trust the registered handle's locally cached cancelled status.
                match lookup(&catalog) {
                    Ok(Some(status)) => {
                        admit(&C::Reconcile {
                            target: target.clone(),
                            status,
                        })?;
                        println!(
                            "Reconciled exact run {}: {status:?}. Historical outcome; no new Ready or Unread.",
                            target.run_id
                        );
                    }
                    _ => println!(
                        "Remote cancellation unconfirmed; saved intent retained. Detach and reopen for another explicit Retry Cancel."
                    ),
                }
                return Ok(());
            }
            _ => println!(
                "Recovery is read-only. Uncertain Retry unavailable; retained operation identity unchanged. Use /retry-cancel or /detach."
            ),
        }
    }
}

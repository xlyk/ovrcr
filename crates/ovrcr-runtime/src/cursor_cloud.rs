//! Cursor SDK 1.0.40 validation, durable Send admission and exact-run observation.
use anyhow::{Context, Result, bail, ensure};
use ovrcr_protocol::{CursorCloudAccount, canonical_cloud_repository};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const LIMIT: u64 = 1024 * 1024;
const READY_PREFIX: &str = "cursor-sdk-bridge ready ";
mod run;
pub const SDK_VERSION: &str = "1.0.40";

#[derive(Debug, PartialEq, Eq)]
pub struct CloudLaunchPreview {
    pub account: String,
    pub repository: String,
    pub source_sha: String,
    pub model: String,
}
impl std::fmt::Display for CloudLaunchPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Cursor cloud validated preview\nAccount: {} (selected profile; billing identity not attested)\nRepository: {}\nPublished commit: {}\nModel: {}\nLocal edits are excluded; nothing is uploaded, committed or pushed.\nCreateAgent: explicit prepare required. Send: available after prepare. Terminal responses, Retry and active reattachment remain blocked.\nType prepare then Enter to retain a cloud handle (no prompt or Send).\nPress Enter to close preview.",
            self.account, self.repository, self.source_sha, self.model
        )
    }
}

// Every command runs outside Server input/lifecycle locks, with a shared deadline.
fn output(command: &mut Command, deadline: Instant, max: u64) -> Result<Vec<u8>> {
    let mut capture = tempfile::tempfile()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(capture.try_clone()?)
        .stderr(Stdio::null())
        .spawn()?;
    let result = (|| -> Result<()> {
        loop {
            let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id(),
                    &mut status,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            ensure!(rc == 0, "preflight child status unavailable");
            if unsafe { status.si_pid() } != 0 {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline && capture.metadata()?.len() <= max,
                "preflight command exceeded bounds"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
    })();
    // Descendants inherit the adapter group; its supervisor also cleans them after owner death.
    let _ = child.kill();
    let status = child.wait()?;
    result?;
    ensure!(status.success(), "preflight command failed");
    capture.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    capture.take(max + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= max,
        "preflight output exceeded bounds"
    );
    Ok(bytes)
}
fn git(workspace: &Path, args: &[&str], deadline: Instant) -> Result<String> {
    let bytes = output(
        Command::new("/usr/bin/git")
            .current_dir(workspace)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .args(args),
        deadline,
        LIMIT,
    )?;
    Ok(String::from_utf8(bytes)?.trim().to_owned())
}
fn source(
    profile: &CursorCloudAccount,
    workspace: &Path,
    deadline: Instant,
    fixture: Option<&Value>,
) -> Result<CloudLaunchPreview> {
    profile.validate()?;
    let origin = git(workspace, &["remote", "get-url", "origin"], deadline)?;
    let repository = canonical_cloud_repository(&origin).context("unsupported workspace origin")?;
    ensure!(
        profile.permits(&repository),
        "repository policy refused workspace origin"
    );
    let sha = git(
        workspace,
        &["rev-parse", "--verify", "HEAD^{commit}"],
        deadline,
    )?;
    ensure!(
        sha.len() == 40
            && sha
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "invalid HEAD"
    );
    let published = match fixture {
        Some(fixture) => fixture["published_sha"]
            .as_str()
            .context("fixture published SHA required")?
            .to_owned(),
        None => git(workspace, &["ls-remote", "--exit-code", "origin"], deadline)?,
    };
    ensure!(
        published
            .lines()
            .any(|line| line.split_whitespace().next() == Some(&sha)),
        "HEAD is not published at workspace origin"
    );
    Ok(CloudLaunchPreview {
        account: profile.name.clone(),
        repository,
        source_sha: sha,
        model: profile.model.clone(),
    })
}

fn private_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o777 == 0o600
            && metadata.nlink() == 1
            && metadata.len() <= limit,
        "private file identity invalid"
    );
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "private file exceeded bound");
    Ok(bytes)
}

/// Diagnostic fixtures are absent from normal builds and never accept a real key.
fn fixture() -> Result<Option<Value>> {
    #[cfg(feature = "acceptance-diagnostics")]
    if let Some(path) = std::env::var_os("OVRCR_CURSOR_CLOUD_FIXTURE") {
        let path = PathBuf::from(path);
        ensure!(path.is_absolute(), "fixture path must be absolute");
        let parent = fs::symlink_metadata(path.parent().context("fixture parent")?)?;
        ensure!(
            parent.is_dir()
                && parent.uid() == unsafe { libc::geteuid() }
                && parent.mode() & 0o777 == 0o700,
            "fixture directory must be private"
        );
        let value: Value = serde_json::from_slice(&private_file(&path, 65536)?)?;
        ensure!(
            value["schema"] == "ovrcr-cursor-cloud-fixture-v1",
            "invalid fixture schema"
        );
        return Ok(Some(value));
    }
    Ok(None)
}

fn key(profile: &CursorCloudAccount, deadline: Instant, fixture: Option<&Value>) -> Result<String> {
    if let Some(fixture) = fixture {
        ensure!(
            fixture["credential_missing"] != true,
            "selected Keychain credential missing"
        );
        return Ok("ovrcr-fixture-not-a-credential".into());
    }
    ensure!(
        cfg!(target_os = "macos"),
        "cursor-cloud unavailable on Linux"
    );
    let bytes = output(
        Command::new("/usr/bin/security").args([
            "find-generic-password",
            "-s",
            &profile.keychain_service,
            "-a",
            &profile.keychain_account,
            "-w",
        ]),
        deadline,
        4096,
    )?;
    let value = String::from_utf8(bytes)?.trim().to_owned();
    ensure!(
        !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control),
        "selected Keychain credential missing or malformed"
    );
    Ok(value)
}

fn binary(
    profile: &CursorCloudAccount,
    root: &Path,
    deadline: Instant,
    fixture: Option<&Value>,
) -> Result<PathBuf> {
    if let Some(fixture) = fixture {
        return Ok(PathBuf::from(
            fixture["bridge"]
                .as_str()
                .context("fixture bridge required")?,
        ));
    }
    let expected = if cfg!(target_arch = "aarch64") {
        "56f7f68a43ce73daa8d93c60b35b5301feb6a351e1dabfefe9873d80d56931cb"
    } else if cfg!(target_arch = "x86_64") {
        "cc1e1779e9768fc0db09a93d98dce51498e507ee26a24ae04c0dd21b21700ea6"
    } else {
        bail!("unsupported bridge architecture")
    };
    let mut archive = File::open(&profile.bridge_archive)?;
    ensure!(
        archive.metadata()?.is_file() && archive.metadata()?.len() <= 256 * LIMIT,
        "archive exceeded bounds"
    );
    let mut pinned = tempfile::NamedTempFile::new_in(root)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = archive.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        ensure!(Instant::now() < deadline, "archive deadline");
        digest.update(&buffer[..n]);
        std::io::Write::write_all(&mut pinned, &buffer[..n])?;
    }
    ensure!(
        format!("{:x}", digest.finalize()) == expected,
        "pinned SDK archive checksum mismatch"
    );
    // Extract only the reviewed regular entrypoint, never archive paths or links.
    let bytes = output(
        Command::new("/usr/bin/tar")
            .args(["-xOf"])
            .arg(pinned.path())
            .arg("bin/cursor-sdk-bridge"),
        deadline,
        256 * LIMIT,
    )?;
    let path = root.join("bridge");
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&path)?;
    use std::io::Write;
    target.write_all(&bytes)?;
    target.sync_all()?;
    Ok(path)
}

struct Bridge {
    child: Child,
}
impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Bridge {
    fn start(binary: &Path, root: &Path, key: &str, deadline: Instant) -> Result<(Self, Value)> {
        for name in ["home", "tmp", "workspace", "state"] {
            fs::create_dir(root.join(name))?;
        }
        let mut stderr = tempfile::tempfile()?;
        let child = Command::new(binary)
            .args(["--host", "127.0.0.1", "--port", "0", "--workspace"])
            .arg(root.join("workspace"))
            .arg("--state-root")
            .arg(root.join("state"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", root.join("home"))
            .env("TMPDIR", root.join("tmp"))
            .env("TMP", root.join("tmp"))
            .env("TEMP", root.join("tmp"))
            .env("CURSOR_API_KEY", key)
            .env("CURSOR_SDK_CLIENT_LANGUAGE", "rust")
            .current_dir(root.join("workspace"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr.try_clone()?)
            .spawn()?;
        let bridge = Self { child };
        loop {
            ensure!(
                Instant::now() < deadline && stderr.metadata()?.len() <= 65536,
                "bridge discovery exceeded bounds"
            );
            stderr.seek(SeekFrom::Start(0))?;
            let mut data = String::new();
            stderr.by_ref().take(65537).read_to_string(&mut data)?;
            if let Some(line) = data
                .lines()
                .find_map(|line| line.strip_prefix(READY_PREFIX))
            {
                let ready: Value = serde_json::from_str(line)?;
                ensure!(
                    ready["schemaVersion"] == 1
                        && ready["serverVersion"] == "1.0.0"
                        && ready["pid"] == bridge.child.id()
                        && ready["transport"] == "tcp"
                        && ready["protocol"] == "connect"
                        && ready["host"] == "127.0.0.1"
                        && ready["workspaceRef"]
                            == root.join("workspace").to_string_lossy().as_ref()
                        && ready["stateRoot"] == root.join("state").to_string_lossy().as_ref(),
                    "bridge discovery identity mismatch"
                );
                return Ok((bridge, ready));
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
}
#[derive(Clone)]
struct Catalog {
    agent: ureq::Agent,
    endpoint: String,
    token: String,
    deadline: Instant,
}
impl Catalog {
    fn rpc(&self, service: &str, method: &str, body: &Value) -> Result<Value> {
        // There is intentionally no generic public remote-operation interface.
        ensure!(
            matches!(
                (service, method),
                ("SdkBridgeControlService", "GetVersion" | "Ping")
                    | ("SdkCursorService", "Me" | "ListModels" | "ListRepositories")
                    | (
                        "SdkAgentService",
                        "CreateAgent" | "GetAgent" | "GetRun" | "ListArtifacts" | "CancelRun"
                    )
            ),
            "preview RPC unavailable"
        );
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        ensure!(!remaining.is_zero(), "catalog deadline");
        let response = self
            .agent
            .post(&format!("{}/sdk.v1.{service}/{method}", self.endpoint))
            .timeout(remaining)
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json")
            .set("Connect-Protocol-Version", "1")
            .send_string(&body.to_string())
            .map_err(|_| anyhow::anyhow!("selected-key catalog request failed"))?;
        ensure!(response.status() == 200, "catalog request refused");
        let mut data = Vec::new();
        response
            .into_reader()
            .take(LIMIT + 1)
            .read_to_end(&mut data)?;
        ensure!(data.len() as u64 <= LIMIT, "catalog exceeded bounds");
        Ok(serde_json::from_slice(&data)?)
    }
}

fn creation_boundary(fixture: Option<&Value>, boundary: &str) -> Result<()> {
    if fixture.is_some_and(|value| value["creation_crash_at"] == boundary) {
        std::process::exit(86);
    }
    if let Some(value) = fixture.filter(|value| value["creation_pause_at"] == boundary) {
        let marker = PathBuf::from(
            value["creation_checkpoint"]
                .as_str()
                .context("fixture checkpoint required")?,
        );
        ensure!(marker.is_absolute(), "fixture checkpoint must be absolute");
        fs::write(&marker, boundary)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while !marker.with_extension("release").exists() {
            ensure!(
                Instant::now() < deadline,
                "fixture creation checkpoint deadline"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }
    Ok(())
}

pub fn preview(
    profile: &CursorCloudAccount,
    workspace: &Path,
    admit: impl Fn(&ovrcr_protocol::CloudCreationCommand) -> Result<()> + Send + Sync + 'static,
) -> Result<()> {
    let admit = std::sync::Arc::new(admit);
    let mut submit = |command: &ovrcr_protocol::CloudCreationCommand| admit(command);
    let fixture = fixture()?;
    let budget = fixture
        .as_ref()
        .and_then(|fixture| fixture["preflight_timeout_ms"].as_u64())
        .unwrap_or(30_000);
    ensure!(
        (1..=30_000).contains(&budget),
        "invalid fixture preflight budget"
    );
    let deadline = Instant::now() + Duration::from_millis(budget);
    if let Some(helper) = fixture
        .as_ref()
        .and_then(|fixture| fixture["preflight_helper"].as_str())
    {
        output(&mut Command::new(helper), deadline, 4096)?;
    }
    let validated = source(profile, workspace, deadline, fixture.as_ref())?;
    let key = key(profile, deadline, fixture.as_ref())?;
    let root = PathBuf::from(
        std::env::var_os("OVRCR_CURSOR_CLOUD_ROOT").context("Server-owned scratch is required")?,
    );
    let metadata = fs::symlink_metadata(&root)?;
    ensure!(
        root.is_absolute()
            && metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o777 == 0o700
            && fs::read_dir(&root)?.next().is_none(),
        "invalid Server-owned scratch"
    );
    let binary = binary(profile, &root, deadline, fixture.as_ref())?;
    let (bridge, ready) = Bridge::start(&binary, &root, &key, deadline)?;
    let port = ready["port"]
        .as_u64()
        .filter(|port| *port > 0 && *port <= 65535)
        .context("invalid loopback port")?;
    let endpoint = format!("http://127.0.0.1:{port}");
    ensure!(ready["url"] == endpoint, "bridge endpoint mismatch");
    let token_path = PathBuf::from(
        ready["authTokenFile"]
            .as_str()
            .context("bridge token identity missing")?,
    );
    let directory = token_path.parent().context("bridge token parent")?;
    ensure!(
        token_path
            .file_name()
            .is_some_and(|name| name == "auth-token")
            && directory.parent() == Some(root.join("tmp").as_path())
            && directory
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("cursor-sdk-bridge-")),
        "unowned bridge token"
    );
    let metadata = fs::symlink_metadata(directory)?;
    ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o777 == 0o700,
        "invalid token directory"
    );
    let token = String::from_utf8(private_file(&token_path, 4096)?)?
        .trim()
        .to_owned();
    ensure!(
        token.len() >= 16
            && token
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
        "invalid bridge token"
    );
    let mut catalog = Catalog {
        agent: ureq::AgentBuilder::new()
            .try_proxy_from_env(false)
            .redirects(0)
            .build(),
        endpoint,
        token,
        deadline,
    };
    let version = catalog.rpc("SdkBridgeControlService", "GetVersion", &json!({}))?;
    ensure!(
        version["bridgeVersion"] == "1.0.0" && version["protocolVersion"] == "sdk.v1",
        "bridge runtime version mismatch"
    );
    let options = json!({"options":{"apiKey":key}});
    let user = catalog.rpc("SdkCursorService", "Me", &options)?;
    ensure!(
        user["user"]["userId"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit())),
        "selected credential validation failed"
    );
    let models = catalog.rpc("SdkCursorService", "ListModels", &options)?;
    ensure!(
        models["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == validated.model)),
        "explicit model unavailable"
    );
    let repositories = catalog.rpc("SdkCursorService", "ListRepositories", &options)?;
    ensure!(
        repositories["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["url"] == validated.repository)),
        "selected key cannot access repository"
    );
    println!("{validated}");
    let mut input = String::new();
    use std::io::BufRead;
    std::io::stdin().lock().take(64).read_line(&mut input)?;
    if input.trim() == "prepare" {
        use ovrcr_protocol::{CloudCreationCommand, CloudCreationIntent, CloudCreationReceipt};
        let intent = CloudCreationIntent {
            key: crate::agent_runner::private_identifier()?,
            account: profile.clone(),
            credential_generation: format!("{:x}", Sha256::digest(key.as_bytes())),
            repository: validated.repository,
            source_sha: validated.source_sha,
        };
        intent.validate()?;
        admit(&CloudCreationCommand::Prepare(intent.clone()))?;
        creation_boundary(fixture.as_ref(), "before_dispatch")?;
        admit(&CloudCreationCommand::Claim {
            key: intent.key.clone(),
        })?;
        creation_boundary(fixture.as_ref(), "after_claim")?;
        // The durable claim is the serialized dispatch boundary; closure after it is uncertain.
        let mut request = intent.request();
        request["options"]["apiKey"] = json!(key);
        catalog.deadline = Instant::now() + Duration::from_secs(30);
        let response = catalog.rpc("SdkAgentService", "CreateAgent", &request)?;
        let receipt = CloudCreationReceipt {
            agent_id: response["agentId"]
                .as_str()
                .context("creation agent identity missing")?
                .into(),
            model: response["model"]["id"]
                .as_str()
                .context("creation model receipt missing")?
                .into(),
        };
        receipt.validate()?;
        ensure!(
            receipt.model == profile.model
                && response == json!({"agentId":receipt.agent_id,"model":{"id":receipt.model}}),
            "creation receipt differs from the exact supported model/identity shape"
        );
        creation_boundary(fixture.as_ref(), "after_reply")?;
        admit(&CloudCreationCommand::Acknowledge {
            key: intent.key.clone(),
            receipt: receipt.clone(),
        })?;
        creation_boundary(fixture.as_ref(), "after_ack")?;
        println!(
            "Cloud handle durably retained: {}\nThis SDK acknowledgement allocates a local handle; backend existence is unverified until Send.\nAfter bridge exit, handle restoration unavailable. No automatic creation or Retry.\nEnter a prompt to Send; empty line detaches. Busy input is retained for explicit /resubmit.",
            receipt.agent_id
        );
        // Only the UI reader runs concurrently; durable operation and Reporter ownership stay unchanged.
        struct InputState {
            busy: bool,
            epoch: u64,
            rejected: Option<String>,
        }
        let input_state = std::sync::Arc::new(std::sync::Mutex::new(InputState {
            busy: false,
            epoch: 0,
            rejected: None,
        }));
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let control = std::sync::Arc::new(run::Control::default());
        let reader_control = control.clone();
        let control_admit = admit.clone();
        let control_catalog = catalog.clone();
        let control_agent = receipt.agent_id.clone();
        let reader_state = input_state.clone();
        std::thread::spawn(move || {
            use std::os::fd::FromRawFd;
            let fd = unsafe { libc::dup(libc::STDIN_FILENO) };
            if fd < 0 {
                return;
            }
            let mut stdin = unsafe { File::from_raw_fd(fd) };
            let mut pending = Vec::new();
            let mut lines = std::collections::VecDeque::new();
            let mut check_at = Instant::now();
            let mut eof = false;
            'input: loop {
                if Instant::now() >= check_at {
                    let target = reader_control.target.lock().unwrap().clone();
                    if let Some((send, run)) = target {
                        run::cancel(
                            &control_catalog,
                            &control_agent,
                            &send,
                            &run,
                            &mut |c| control_admit(c),
                            &reader_control,
                        );
                    }
                    check_at = Instant::now() + Duration::from_secs(1);
                }
                let Some(line) = lines.pop_front() else {
                    if eof {
                        break;
                    }
                    let mut poll = libc::pollfd {
                        fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let ready = unsafe { libc::poll(&mut poll, 1, 100) };
                    if ready == 0 {
                        continue;
                    }
                    if ready < 0 {
                        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                        {
                            continue;
                        }
                        break;
                    }
                    let mut bytes = [0; 4096];
                    let count = match stdin.read(&mut bytes) {
                        Ok(0) => {
                            if let Ok(line) = String::from_utf8(std::mem::take(&mut pending))
                                && !line.is_empty()
                            {
                                lines.push_back(line);
                            }
                            eof = true;
                            continue;
                        }
                        Err(_) => break,
                        Ok(count) => count,
                    };
                    for byte in &bytes[..count] {
                        pending.push(*byte);
                        if pending.len() > 32768 {
                            println!("Prompt exceeds 32768 bytes; not submitted. Input closed.");
                            break 'input;
                        }
                        if *byte == b'\n' {
                            let Ok(line) = String::from_utf8(std::mem::take(&mut pending)) else {
                                break 'input;
                            };
                            lines.push_back(line);
                        }
                    }
                    continue;
                };
                if line.trim() == "/cancel" {
                    match control_admit(&CloudCreationCommand::Cancel) {
                        Ok(()) => {
                            println!(
                                "Cancellation intent durably retained; delivery closed. Remote cancellation unconfirmed; unresolved content retained until Discard."
                            );
                            let target = reader_control.target.lock().unwrap().clone();
                            if let Some((send, run)) = target {
                                run::cancel(
                                    &control_catalog,
                                    &control_agent,
                                    &send,
                                    &run,
                                    &mut |c| control_admit(c),
                                    &reader_control,
                                );
                            } else {
                                println!(
                                    "Local delivery stopped before dispatch if no attempt was claimed; any original in-flight attempt remains uncertain."
                                );
                            }
                        }
                        Err(error) => println!(
                            "Cancellation not durably acknowledged: {error}; remote cancellation not attempted."
                        ),
                    }
                    continue;
                }
                let mut state = reader_state.lock().unwrap();
                if line.len() > 32768 {
                    println!("Prompt exceeds 32768 bytes; not submitted. Input closed.");
                    break;
                }
                if state.busy && line.trim() != "/detach" {
                    state.rejected = Some(line);
                    println!(
                        "Busy: prompt rejected and retained locally. When idle type /resubmit to submit it manually."
                    );
                } else if let Err(std::sync::mpsc::TrySendError::Full((_, line))) =
                    sender.try_send((state.epoch, line))
                {
                    state.rejected = Some(line);
                    println!("Prompt rejected and retained locally; explicit /resubmit required.");
                }
            }
        });
        let mut source_verified = false;
        while let Ok((epoch, line)) = receiver.recv() {
            if line.trim() == "/detach" {
                break;
            }
            let prompt = {
                let mut state = input_state.lock().unwrap();
                if epoch != state.epoch {
                    state.rejected = Some(line);
                    println!("Prompt rejected and retained locally; explicit /resubmit required.");
                    continue;
                }
                let prompt = if line.trim() == "/resubmit" {
                    state.rejected.take().unwrap_or_default()
                } else {
                    line
                };
                if prompt.trim().is_empty() {
                    break;
                }
                state.busy = true;
                state.epoch += 1;
                prompt
            };
            match run::send(
                &mut catalog,
                &intent,
                (&receipt.agent_id, &key),
                prompt,
                !source_verified,
                fixture.as_ref(),
                &mut submit,
                &control,
            ) {
                Ok(verified) => source_verified |= verified,
                Err(error) => {
                    println!(
                        "Observation unknown/unverified: {error}. Remote work may continue. No automatic Retry or further Send; /detach closes local observation. Discard: ovrcr terminal cloud-discard <id>."
                    );
                    while let Ok((_, line)) = receiver.recv() {
                        if line.trim() == "/detach" {
                            break;
                        }
                    }
                    break;
                }
            }
            let mut state = input_state.lock().unwrap();
            state.busy = false;
            println!(
                "Idle on retained conversation {}. Enter follow-up, /resubmit, or empty line to detach.",
                receipt.agent_id
            );
        }
    }
    drop(bridge);
    Ok(())
}

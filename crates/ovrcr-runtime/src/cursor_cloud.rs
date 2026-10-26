//! Cursor SDK 1.0.40 preview. This module has no CreateAgent or Send operation.
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
            "Cursor cloud validated preview\nAccount: {} (selected profile; billing identity not attested)\nRepository: {}\nPublished commit: {}\nModel: {}\nLocal edits are excluded; nothing is uploaded, committed or pushed.\nCreateAgent/Send: unavailable in this preview. Terminal responses, Retry and active reattachment remain blocked.\nPress Enter to close preview.",
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

pub fn preview(profile: &CursorCloudAccount, workspace: &Path) -> Result<()> {
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
    let catalog = Catalog {
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
    let mut byte = [0];
    while std::io::stdin().read(&mut byte)? != 0 && byte[0] != b'\n' {}
    drop(bridge);
    Ok(())
}

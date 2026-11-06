//! Native Claude policy identities and bounded pre-input model discovery.
//! No SDK, Session, Reporter lease or assignment publisher is created by discovery.
use crate::catalog_process::{CatalogCapture, CatalogProcessOwner, owner_error};
use anyhow::{Context, Result, bail, ensure};
use ovrcr_protocol::ResearchContract;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::{fs::MetadataExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub const ARTIFACT_ENV: &str = "OVRCR_CLAUDE_ARTIFACTS";
pub const POLICY: &str = "ovrcr-claude-research-v1";

#[cfg(test)]
std::thread_local! {
    static FAIL_NEXT_CATALOG_CLEANUP_WAITID: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn fail_next_catalog_cleanup_waitid() {
    FAIL_NEXT_CATALOG_CLEANUP_WAITID.with(|failure| failure.set(true));
}

fn catalog_cleanup_waitid(pid: libc::pid_t, info: &mut libc::siginfo_t) -> std::io::Result<()> {
    #[cfg(test)]
    if FAIL_NEXT_CATALOG_CLEANUP_WAITID.with(|failure| failure.replace(false)) {
        return Err(std::io::Error::from_raw_os_error(libc::EIO));
    }
    let status = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileIdentity {
    device: u64,
    inode: u64,
    size: u64,
    modified: i64,
    modified_ns: i64,
}
fn file_identity(path: &Path) -> Result<FileIdentity> {
    let meta = fs::metadata(path)?;
    ensure!(
        meta.is_file() && meta.mode() & 0o111 != 0,
        "native Claude executable unavailable"
    );
    Ok(FileIdentity {
        device: meta.dev(),
        inode: meta.ino(),
        size: meta.len(),
        modified: meta.mtime(),
        modified_ns: meta.mtime_nsec(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelProof {
    pub requested: String,
    pub resolved: String,
    pub executable: PathBuf,
    target: PathBuf,
    binary_digest: String,
    file: FileIdentity,
    pub profile: PathBuf,
    profile_entry: PathBuf,
    profile_device: u64,
    profile_inode: u64,
    pub native_version: String,
    policy: String,
    // This identifies only model/backend-affecting policy, not complete launch
    // topology. Discovery excludes the assignment publisher and Reporter transport.
    policy_digest: String,
}

pub fn executable() -> Result<PathBuf> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").context("PATH unavailable")?) {
        let candidate = directory.join("claude");
        if candidate.is_file() {
            let entry = candidate
                .parent()
                .context("native executable has no directory")?
                .canonicalize()?
                .join("claude");
            file_identity(&entry)?;
            return Ok(entry);
        }
    }
    bail!("native Claude executable unavailable")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PolicyManifest {
    recipe: String,
    helper: PathBuf,
    settings: String,
    mcp: String,
}
pub struct PolicyFiles {
    pub settings: Vec<u8>,
    pub mcp: Vec<u8>,
    pub manifest: Vec<u8>,
}
pub fn policy_files(helper: &Path, handle: &str) -> Result<PolicyFiles> {
    let helper = helper.canonicalize()?;
    let settings = format!(
        "{}\n",
        serde_json::to_string_pretty(&crate::claude_recovery::reporter_settings(
            helper.to_str().context("non-UTF8 Reporter executable")?
        ))?
    )
    .into_bytes();
    let mcp = serde_json::to_vec(&serde_json::json!({"mcpServers":{"ovrcr_child":{
        "type":"stdio","command":helper,"args":["delegation-publisher","--read-handle",handle]
    }}}))?;
    let manifest = PolicyManifest {
        recipe: POLICY.into(),
        helper,
        settings: format!("{:x}", Sha256::digest(&settings)),
        mcp: format!("{:x}", Sha256::digest(&mcp)),
    };
    Ok(PolicyFiles {
        settings,
        mcp,
        manifest: serde_json::to_vec(&manifest)?,
    })
}

/// Only model/backend-affecting flags are shared with discovery; transport
/// differences are explicit and never claim identical complete MCP topology.
pub fn launch_argv(executable: &Path, model: &str, artifacts: &Path) -> Vec<std::ffi::OsString> {
    vec![
        executable.as_os_str().into(),
        "--model".into(),
        model.into(),
        "--permission-mode".into(),
        "default".into(),
        "--tools".into(),
        "".into(),
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        artifacts.join("claude-mcp.json").into_os_string(),
        "--settings".into(),
        artifacts.join("claude-settings.json").into_os_string(),
        "--setting-sources".into(),
        "".into(),
    ]
}

fn native_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:/[]-".contains(&byte))
}
fn version(value: &str) -> bool {
    let values: Option<Vec<u64>> = value.split('.').map(|part| part.parse().ok()).collect();
    values.is_some_and(|parts| parts.len() == 3 && parts.as_slice() >= [2, 1, 300].as_slice())
}
fn policy_digest(model: &str, profile: &Path) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&serde_json::json!({
            "policy":POLICY,"model":model,"model_provider":"native","profile":profile,
            "permission_mode":"default","tools":"","strict_mcp_config":true,
            "setting_sources":"","model_settings":{}
        }))?)
    ))
}

impl ModelProof {
    pub(crate) fn validate(&self, contract: &ResearchContract) -> Result<()> {
        ensure!(
            contract.provider == "claude"
                && contract.model_provider == "native"
                && self.requested == contract.model
                && native_id(&self.resolved)
                && self.policy == POLICY
                && version(&self.native_version)
                && self.binary_digest.len() == 64
                && self
                    .binary_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "native model proof does not match the immutable contract"
        );
        ensure!(
            executable()? == self.executable
                && self.executable.canonicalize()? == self.target
                && file_identity(&self.executable)? == self.file,
            "native executable changed since model discovery"
        );
        let profile_entry = crate::claude_recovery::config_dir()?;
        let profile = profile_entry.canonicalize()?;
        let meta = fs::metadata(&profile)?;
        ensure!(
            profile_entry == self.profile_entry
                && meta.is_dir()
                && profile == self.profile
                && meta.dev() == self.profile_device
                && meta.ino() == self.profile_inode
                && policy_digest(&self.requested, &profile)? == self.policy_digest,
            "native profile or model policy changed since discovery"
        );
        Ok(())
    }
}

fn models(value: &serde_json::Value) -> Result<Vec<(String, String)>> {
    let list = value
        .as_array()
        .filter(|list| list.len() <= 512)
        .context("native model catalog unavailable")?;
    let mut found = std::collections::BTreeMap::new();
    for model in list {
        let requested = model["value"]
            .as_str()
            .filter(|value| native_id(value))
            .context("invalid native model alias")?;
        let Some(resolved) = model
            .get("resolvedModel")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        ensure!(native_id(resolved), "invalid native resolved model");
        ensure!(
            found
                .insert(requested.to_owned(), resolved.to_owned())
                .is_none(),
            "duplicate native model alias"
        );
    }
    Ok(found.into_iter().collect())
}

/// Start the native initialize handshake used by bounded catalog discovery.
/// The caller supplies the native launch policy; this helper adds only the
/// stream transport and enforces bounded output, time and process cleanup.
fn initialized_catalog(command: Command) -> Result<(Vec<(String, String)>, String)> {
    initialized_catalog_inner(
        command,
        &std::sync::atomic::AtomicBool::new(false),
        true,
        |_| {},
    )
}

pub(crate) fn initialized_catalog_cancellable_owned(
    command: Command,
    cancelled: &std::sync::atomic::AtomicBool,
    owner: &mut CatalogProcessOwner,
) -> Result<(Vec<(String, String)>, String)> {
    initialized_catalog_inner_with_owner(command, cancelled, false, |_| {}, Some(owner))
}

#[cfg(test)]
pub(crate) fn initialized_catalog_cancellable(
    command: Command,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(Vec<(String, String)>, String)> {
    initialized_catalog_inner(command, cancelled, false, |_| {})
}

fn initialized_catalog_inner<F>(
    command: Command,
    cancelled: &std::sync::atomic::AtomicBool,
    inject_postwait_fault: bool,
    spawned: F,
) -> Result<(Vec<(String, String)>, String)>
where
    F: FnOnce(u32),
{
    initialized_catalog_inner_with_owner(command, cancelled, inject_postwait_fault, spawned, None)
}

fn initialized_catalog_inner_with_owner<F>(
    mut command: Command,
    cancelled: &std::sync::atomic::AtomicBool,
    inject_postwait_fault: bool,
    spawned: F,
    mut owner: Option<&mut CatalogProcessOwner>,
) -> Result<(Vec<(String, String)>, String)>
where
    F: FnOnce(u32),
{
    let request_id = crate::agent_runner::private_identifier()?;
    let frame = format!(
        "{}\n",
        serde_json::json!({"type":"control_request","request_id":request_id,
        "request":{"subtype":"initialize","hooks":null}})
    );
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // The native account/profile route remains opaque. OVRCR authority and
    // delegation capabilities never enter a catalog process.
    for name in [
        "OVRCR_AGENT_SOCKET",
        "OVRCR_AGENT_TOKEN",
        "OVRCR_HOOK_SOCKET",
        "OVRCR_HOOK_TOKEN",
        "OVRCR_SESSION_ID",
        "OVRCR_DELEGATION_SESSION",
        "OVRCR_DELEGATION_TOKEN",
        "OVRCR_DELEGATION_SOCKET",
        "OVRCR_DELEGATION_HOME",
        ARTIFACT_ENV,
    ] {
        command.env_remove(name);
    }
    let mut child = command
        .spawn()
        .context("native model discovery unavailable")?;
    spawned(child.id());
    let mut capture = CatalogCapture::take_from_child(&mut child, 512 * 1024, 64 * 1024);
    let mut stdout_bytes = 0usize;
    let mut error_bytes = 0usize;
    let deadline = Instant::now() + Duration::from_secs(20);
    let result = (|| -> Result<(Vec<(String, String)>, String)> {
        let stdout_fd = capture
            .stdout_mut()
            .context("native discovery stdout unavailable")?;
        let stdout_fd = stdout_fd.as_raw_fd();
        let stderr_fd = capture
            .stderr_mut()
            .context("native discovery stderr unavailable")?;
        let stderr_fd = stderr_fd.as_raw_fd();
        for fd in [stdout_fd, stderr_fd] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            ensure!(
                flags >= 0
                    && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
                "native discovery pipe unavailable"
            );
        }
        let mut bytes = Vec::new();
        child
            .stdin
            .as_mut()
            .context("native discovery stdin unavailable")?
            .write_all(frame.as_bytes())?;
        loop {
            ensure!(
                !cancelled.load(std::sync::atomic::Ordering::Acquire),
                "native model discovery cancelled"
            );
            ensure!(Instant::now() < deadline, "native model discovery deadline");
            let mut buffer = [0; 16384];
            loop {
                ensure!(
                    !cancelled.load(std::sync::atomic::Ordering::Acquire),
                    "native model discovery cancelled"
                );
                ensure!(Instant::now() < deadline, "native model discovery deadline");
                let read = capture
                    .stdout_mut()
                    .context("native discovery stdout unavailable")?
                    .read(&mut buffer);
                match read {
                    Ok(0) => break,
                    Ok(count) => {
                        bytes.extend_from_slice(&buffer[..count]);
                        stdout_bytes = stdout_bytes.saturating_add(count);
                        capture.record_totals(stdout_bytes, error_bytes);
                        ensure!(
                            bytes.len() <= 512 * 1024,
                            "native model discovery output bound"
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => bail!("native discovery stdout unavailable"),
                }
            }
            loop {
                ensure!(Instant::now() < deadline, "native model discovery deadline");
                let read = capture
                    .stderr_mut()
                    .context("native discovery stderr unavailable")?
                    .read(&mut buffer);
                match read {
                    Ok(0) => break,
                    Ok(count) => {
                        error_bytes += count;
                        capture.record_totals(stdout_bytes, error_bytes);
                        ensure!(
                            error_bytes <= 64 * 1024,
                            "native model discovery error bound"
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => bail!("native discovery stderr unavailable"),
                }
            }
            for line in bytes.split_inclusive(|byte| *byte == b'\n') {
                ensure!(
                    line.len() <= 128 * 1024,
                    "native model discovery line bound"
                );
                if line.last() != Some(&b'\n') {
                    continue;
                }
                let value: serde_json::Value =
                    serde_json::from_slice(line).context("invalid native discovery JSON")?;
                if value["type"] == "control_request" {
                    bail!("native discovery requires unresolved action");
                }
                if value["type"] != "control_response"
                    || value["response"]["request_id"] != request_id
                {
                    continue;
                }
                ensure!(
                    value["response"]["subtype"] == "success",
                    "native initialize refused"
                );
                let native = &value["response"]["response"];
                ensure!(
                    native["pid"].as_u64() == Some(child.id() as u64),
                    "native initialize identity mismatch"
                );
                let native_version = native["claude_code_version"]
                    .as_str()
                    .filter(|value| version(value))
                    .context("unsupported native Claude version")?;
                for key in ["pending_permission_requests", "pending_control_requests"] {
                    if let Some(pending) = native.get(key) {
                        ensure!(
                            pending.as_array().is_some_and(Vec::is_empty),
                            "native discovery has pending actions"
                        );
                    }
                }
                return Ok((models(&native["models"])?, native_version.to_owned()));
            }
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let status = unsafe {
                libc::waitid(
                    libc::P_PID,
                    child.id(),
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            ensure!(status == 0, "native discovery ownership probe failed");
            ensure!(
                unsafe { info.si_pid() } == 0,
                "native discovery exited without correlated model evidence"
            );
            std::thread::park_timeout(Duration::from_millis(10));
        }
    })();
    drop(child.stdin.take());
    let using_owner = owner.is_some();
    let cleanup = if let Some(owner) = owner.as_deref_mut() {
        cleanup_catalog_child_owned(
            owner,
            "Claude",
            child,
            capture,
            cancelled,
            deadline,
            stdout_bytes,
            error_bytes,
        )
    } else {
        drop(capture);
        cleanup_catalog_child_legacy(child, cancelled, deadline, inject_postwait_fault)
    };
    match cleanup {
        Ok(()) => result,
        Err(cleanup_error) if using_owner => match result {
            Err(primary_error) => Err(owner_error(format!(
                "native discovery failed ({primary_error:#}); cleanup remains uncertain ({cleanup_error:#})"
            ))),
            Ok(_) => Err(cleanup_error),
        },
        Err(cleanup_error) => Err(cleanup_error),
    }
}

fn cleanup_catalog_child_owned(
    owner: &mut CatalogProcessOwner,
    label: &'static str,
    child: Child,
    capture: CatalogCapture,
    cancelled: &std::sync::atomic::AtomicBool,
    catalog_deadline: Instant,
    mut stdout_bytes: usize,
    mut error_bytes: usize,
) -> Result<()> {
    let pid = child.id() as libc::pid_t;
    let grace = if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        Instant::now()
    } else {
        catalog_deadline.min(Instant::now() + Duration::from_secs(2))
    };
    while Instant::now() < grace {
        if let Err(error) =
            drain_claude_catalog_capture(&mut capture, &mut stdout_bytes, &mut error_bytes)
        {
            owner.retain_unresolved_child(label, child, capture)?;
            return Err(owner_error(format!(
                "native {label} cleanup capture is uncertain: {error:#}"
            )));
        }
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        if let Err(error) = catalog_cleanup_waitid(pid, &mut info) {
            owner.retain_unresolved_child(label, child, capture)?;
            return Err(owner_error(format!(
                "native {label} cleanup ownership is uncertain (waitid errno {:?})",
                error.raw_os_error()
            )));
        }
        let observed_pid = unsafe { info.si_pid() };
        if observed_pid != 0 && observed_pid != pid {
            owner.retain_unresolved_child(label, child, capture)?;
            return Err(owner_error(format!(
                "native {label} cleanup ownership is uncertain (observed pid {observed_pid})"
            )));
        }
        if observed_pid != 0 {
            owner.retain_unresolved_child(label, child, capture)?;
            return owner.resolve_existing();
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }

    if let Err(error) =
        drain_claude_catalog_capture(&mut capture, &mut stdout_bytes, &mut error_bytes)
    {
        owner.retain_unresolved_child(label, child, capture)?;
        return Err(owner_error(format!(
            "native {label} cleanup capture is uncertain: {error:#}"
        )));
    }
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    if let Err(error) = catalog_cleanup_waitid(pid, &mut info) {
        owner.retain_unresolved_child(label, child, capture)?;
        return Err(owner_error(format!(
            "native {label} cleanup ownership is uncertain (waitid errno {:?})",
            error.raw_os_error()
        )));
    }
    let observed_pid = unsafe { info.si_pid() };
    if observed_pid != 0 && observed_pid != pid {
        owner.retain_unresolved_child(label, child, capture)?;
        return Err(owner_error(format!(
            "native {label} cleanup ownership is uncertain (observed pid {observed_pid})"
        )));
    }
    owner.retain_unresolved_child(label, child, capture)?;
    owner.resolve_existing()
}

fn drain_claude_catalog_capture(
    capture: &mut CatalogCapture,
    stdout_bytes: &mut usize,
    error_bytes: &mut usize,
) -> Result<()> {
    let mut buffer = [0; 8192];
    loop {
        let read = capture
            .stdout_mut()
            .context("native discovery stdout unavailable")?
            .read(&mut buffer);
        match read {
            Ok(0) => break,
            Ok(count) => {
                *stdout_bytes = stdout_bytes.saturating_add(count);
                capture.record_totals(*stdout_bytes, *error_bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    loop {
        let read = capture
            .stderr_mut()
            .context("native discovery stderr unavailable")?
            .read(&mut buffer);
        match read {
            Ok(0) => break,
            Ok(count) => {
                *error_bytes = error_bytes.saturating_add(count);
                capture.record_totals(*stdout_bytes, *error_bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn cleanup_catalog_child_legacy(
    mut child: Child,
    cancelled: &std::sync::atomic::AtomicBool,
    catalog_deadline: Instant,
    inject_postwait_fault: bool,
) -> Result<()> {
    let grace = if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        Instant::now()
    } else {
        catalog_deadline.min(Instant::now() + Duration::from_secs(2))
    };
    while Instant::now() < grace {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        ensure!(
            catalog_cleanup_waitid(child.id() as libc::pid_t, &mut info).is_ok(),
            "native discovery cleanup ownership is uncertain"
        );
        let observed_pid = unsafe { info.si_pid() };
        ensure!(
            observed_pid == 0 || observed_pid == child.id() as libc::pid_t,
            "native discovery cleanup ownership is uncertain"
        );
        if observed_pid != 0 {
            break;
        }
        std::thread::park_timeout(Duration::from_millis(10));
    }
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    ensure!(
        catalog_cleanup_waitid(child.id() as libc::pid_t, &mut info).is_ok(),
        "native discovery cleanup ownership is uncertain"
    );
    let observed_pid = unsafe { info.si_pid() };
    ensure!(
        observed_pid == 0 || observed_pid == child.id() as libc::pid_t,
        "native discovery cleanup ownership is uncertain"
    );
    if observed_pid == 0 {
        let cleanup = unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
        let cleanup_errno = if cleanup == -1 {
            std::io::Error::last_os_error().raw_os_error()
        } else {
            None
        };
        ensure!(
            cleanup == 0 || (cleanup == -1 && cleanup_errno == Some(libc::ESRCH)),
            "native discovery cleanup is uncertain (process-group signal errno {cleanup_errno:?})"
        );
    }
    let waited = child.wait();
    let absent = unsafe { libc::kill(-(child.id() as i32), 0) };
    let absence_errno = if absent == -1 {
        std::io::Error::last_os_error().raw_os_error()
    } else {
        None
    };
    let known_absent = waited.is_ok() && absent == -1 && absence_errno == Some(libc::ESRCH);
    let injected_unknown = known_absent
        && inject_postwait_fault
        && crate::server::delegation::take_fault(
            "claude-discovery-postwait-unknown",
            "preallocation",
        )
        .is_some();
    ensure!(
        known_absent && !injected_unknown,
        "native discovery cleanup is uncertain"
    );
    Ok(())
}

/// Query the catalog under a verified detected Claude launcher. Native trust
/// flags are deliberately omitted; transport-only flags disable prompt input,
/// tools, MCP and user/project/local hooks while preserving model settings.
pub(crate) fn dashboard_catalog(
    executable: &Path,
    native_argv: &[std::ffi::OsString],
    cwd: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
    owner: &mut CatalogProcessOwner,
) -> Result<Vec<(String, String)>> {
    let query_args = dashboard_catalog_args(native_argv)?;
    ensure!(
        native_argv
            .first()
            .is_some_and(|arg| arg == executable.as_os_str()),
        "Claude executable does not match its resolved launch argv"
    );
    let profile_entry = crate::claude_recovery::config_dir()?;
    let profile = profile_entry.canonicalize()?;
    ensure!(
        fs::metadata(&profile)?.is_dir(),
        "native profile unavailable"
    );
    let working_directory = cwd.canonicalize()?;
    ensure!(
        fs::metadata(&working_directory)?.is_dir(),
        "Claude workspace is unavailable"
    );
    let mut command = Command::new(executable);
    command
        .current_dir(working_directory)
        .args(query_args.iter().skip(1))
        .env("CLAUDE_CONFIG_DIR", &profile_entry);
    initialized_catalog_cancellable_owned(command, cancelled, owner).map(|(catalog, _)| catalog)
}

fn dashboard_catalog_args(native_argv: &[std::ffi::OsString]) -> Result<Vec<std::ffi::OsString>> {
    ensure!(
        native_argv.len() == 2 && native_argv[1] == "--dangerously-skip-permissions",
        "Claude launch options are not a supported detected configuration"
    );
    Ok([
        native_argv[0].clone(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--tools".into(),
        "".into(),
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        r#"{"mcpServers":{}}"#.into(),
        "--settings".into(),
        r#"{"disableAllHooks":true}"#.into(),
    ]
    .into_iter()
    .collect())
}

/// This remains preflight evidence, never a proof of managed-TUI tool inventory,
/// native authentication, helper access or actual model inference.
pub(crate) fn discover(requested: &str) -> Result<(Vec<(String, String)>, ModelProof)> {
    ensure!(native_id(requested), "invalid native model request");
    let executable = executable()?;
    let file = file_identity(&executable)?;
    let target = executable.canonicalize()?;
    let mut binary = Vec::new();
    File::open(&target)?
        .take(512 * 1024 * 1024 + 1)
        .read_to_end(&mut binary)?;
    ensure!(
        binary.len() <= 512 * 1024 * 1024 && file_identity(&executable)? == file,
        "native executable exceeds bounds or changed"
    );
    let binary_digest = format!("{:x}", Sha256::digest(&binary));
    drop(binary);
    let profile_entry = crate::claude_recovery::config_dir()?;
    let profile = profile_entry.canonicalize()?;
    let meta = fs::metadata(&profile)?;
    ensure!(meta.is_dir(), "native profile unavailable");
    let mut command = Command::new(&executable);
    command
        .args([
            "--output-format",
            "stream-json",
            "--verbose",
            "--input-format",
            "stream-json",
            "--model",
            requested,
            "--permission-mode",
            "default",
            "--tools",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            r#"{"mcpServers":{}}"#,
            "--setting-sources",
            "",
            "--settings",
            "{}",
        ])
        .env("CLAUDE_CONFIG_DIR", &profile_entry);
    let (catalog, native_version) = initialized_catalog(command)?;
    let resolved = catalog
        .iter()
        .find(|(alias, _)| alias == requested)
        .map(|(_, resolved)| resolved.clone())
        .context("requested native model alias is unavailable")?;
    let proof = ModelProof {
        requested: requested.into(),
        resolved,
        executable,
        target,
        binary_digest,
        file,
        profile: profile.clone(),
        profile_entry,
        profile_device: meta.dev(),
        profile_inode: meta.ino(),
        native_version,
        policy: POLICY.into(),
        policy_digest: policy_digest(requested, &profile)?,
    };
    Ok((catalog, proof))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Instant;

    #[test]
    fn dashboard_catalog_preserves_default_model_policy_and_disables_user_effects() {
        let native = [
            std::ffi::OsString::from("/bin/claude"),
            std::ffi::OsString::from("--dangerously-skip-permissions"),
        ];
        let args = dashboard_catalog_args(&native).unwrap();
        assert_eq!(args[0], "/bin/claude");
        assert!(
            !args
                .iter()
                .any(|arg| arg == "--model" || arg == "--setting-sources")
        );
        assert!(
            !args
                .iter()
                .any(|arg| arg == "--dangerously-skip-permissions")
        );
        assert!(
            args.windows(2).any(|pair| {
                pair[0] == "--settings" && pair[1] == r#"{"disableAllHooks":true}"#
            })
        );
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "--tools" && pair[1].is_empty())
        );
        assert!(
            args.windows(2)
                .any(|pair| { pair[0] == "--mcp-config" && pair[1] == r#"{"mcpServers":{}}"# })
        );
    }

    #[test]
    fn dashboard_catalog_rejects_ambiguous_native_model_flags() {
        let args = [
            std::ffi::OsString::from("/bin/claude"),
            std::ffi::OsString::from("--model"),
            std::ffi::OsString::from("opus"),
        ];
        assert!(dashboard_catalog_args(&args).is_err());
    }

    #[test]
    fn server_shutdown_cancels_a_held_native_catalog_process_group() {
        let root = tempfile::tempdir().unwrap();
        let pid_file = root.path().join("pid");
        let descendant_file = root.path().join("descendant.pid");
        let script = root.path().join("held-catalog");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s' \"$$\" > pid\nIFS= read -r line\n/bin/sleep 30 </dev/null >/dev/null 2>&1 &\nchild=$!\nprintf '%s' \"$child\" > descendant.pid\nwhile IFS= read -r line; do :; done\nwait \"$child\"\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let shutdown = Arc::new(AtomicBool::new(false));
        let child_shutdown = Arc::clone(&shutdown);
        let child_script = script.clone();
        let child_cwd = root.path().to_path_buf();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut command = Command::new(child_script);
            command.current_dir(child_cwd);
            let mut owner = CatalogProcessOwner::default();
            let result = initialized_catalog_inner_with_owner(
                command,
                &child_shutdown,
                false,
                move |pid| {
                    let _ = started_tx.send(pid);
                },
                Some(&mut owner),
            );
            (result, owner)
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while !pid_file.exists() {
            assert!(Instant::now() < deadline, "fake catalog did not start");
            std::thread::sleep(Duration::from_millis(5));
        }
        let pid: i32 = started_rx.recv_timeout(Duration::from_secs(1)).unwrap() as i32;
        let script_pid: i32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        assert_eq!(script_pid, pid, "fake CLI pid differs from owned child");
        while !descendant_file.exists() {
            assert!(Instant::now() < deadline, "fake descendant did not start");
            std::thread::sleep(Duration::from_millis(5));
        }
        let descendant: i32 = std::fs::read_to_string(&descendant_file)
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            unsafe { libc::getpgid(pid) },
            pid,
            "fake CLI was not group leader"
        );
        assert_eq!(unsafe { libc::getpgid(descendant) }, pid);
        shutdown.store(true, Ordering::Release);
        let finished = Instant::now() + Duration::from_millis(250);
        while !worker.is_finished() && Instant::now() < finished {
            std::thread::sleep(Duration::from_millis(5));
        }
        let cancelled = worker.is_finished();
        if !cancelled {
            // Keep the red test bounded and remove the task-owned fake child.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
        }
        let (result, mut owner) = worker.join().unwrap();
        owner.resolve_existing().unwrap();
        assert!(
            cancelled,
            "server shutdown must cancel and join catalog work"
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "native model discovery cancelled"
        );
        assert_eq!(unsafe { libc::kill(-pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(unsafe { libc::kill(descendant, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    fn dashboard_catalog_waits_for_native_child_after_successful_eof() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("fake-claude");
        std::fs::write(
            &script,
            r##"#!/bin/sh
printf '%s' "$$" > native.pid
IFS= read -r line
request_id=$(printf '%s\n' "$line" | sed -E 's/.*"request_id":"([^"]+)".*/\1/')
printf '%s\n' '{"type":"control_response","response":{"request_id":"'"$request_id"'","subtype":"success","response":{"pid":'"$$"',"claude_code_version":"2.1.318","models":[{"value":"sonnet","resolvedModel":"claude-sonnet-4-6"}]}}}'
"##,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let mut command = Command::new(&script);
        command.current_dir(root.path()).env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let mut owner = CatalogProcessOwner::default();
        let result = initialized_catalog_inner_with_owner(
            command,
            &AtomicBool::new(false),
            false,
            move |pid| {
                let _ = started_tx.send(pid);
            },
            Some(&mut owner),
        );
        let pid = started_rx.recv().unwrap() as libc::pid_t;
        assert_eq!(
            std::fs::read_to_string(root.path().join("native.pid"))
                .unwrap()
                .parse::<libc::pid_t>()
                .unwrap(),
            pid
        );

        let deadline = Instant::now() + Duration::from_secs(2);
        let was_waited = loop {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let observed = unsafe {
                libc::waitid(
                    libc::P_PID,
                    pid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if observed == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD)
            {
                break true;
            }
            if observed == 0 && unsafe { info.si_pid() } == pid {
                break false;
            }
            assert!(Instant::now() < deadline, "fake CLI did not exit");
            std::thread::park_timeout(Duration::from_millis(5));
        };
        if !was_waited {
            // Clean up this exact test-owned zombie after recording the failed
            // ownership assertion, so a red run cannot leave it behind.
            let mut status = 0;
            assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        }
        assert!(
            was_waited,
            "successful catalog EOF returned before waiting for its direct child"
        );
        assert_eq!(
            unsafe { libc::kill(-pid, 0) },
            -1,
            "successful catalog cleanup left its process group present"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert_eq!(
            result.unwrap().0,
            [("sonnet".into(), "claude-sonnet-4-6".into())]
        );
    }

    #[test]
    fn dashboard_catalog_does_not_drop_child_when_cleanup_probe_fails() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("fake-claude");
        std::fs::write(
            &script,
            r##"#!/bin/sh
printf '%s' "$$" > native.pid
IFS= read -r line
request_id=$(printf '%s\n' "$line" | sed -E 's/.*"request_id":"([^"]+)".*/\1/')
printf '%s\n' '{"type":"control_response","response":{"request_id":"'"$request_id"'","subtype":"success","response":{"pid":'"$$"',"claude_code_version":"2.1.318","models":[{"value":"sonnet","resolvedModel":"claude-sonnet-4-6"}]}}}'
"##,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let mut command = Command::new(&script);
        command.current_dir(root.path()).env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let mut owner = CatalogProcessOwner::default();
        fail_next_catalog_cleanup_waitid();
        let result = initialized_catalog_inner_with_owner(
            command,
            &AtomicBool::new(false),
            false,
            move |pid| {
                let _ = started_tx.send(pid);
            },
            Some(&mut owner),
        );
        let pid = started_rx.recv().unwrap() as libc::pid_t;
        assert_eq!(
            std::fs::read_to_string(root.path().join("native.pid"))
                .unwrap()
                .parse::<libc::pid_t>()
                .unwrap(),
            pid
        );
        assert!(result.is_err(), "injected cleanup uncertainty was ignored");
        owner.resolve_existing().unwrap();

        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let observed = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        let was_waited =
            observed == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ECHILD);
        if !was_waited {
            // Clean up this exact test-owned child after recording a failed
            // owner-resolution assertion, so a red run cannot leak it.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            let mut status = 0;
            unsafe { libc::waitpid(pid, &mut status, 0) };
        }
        assert!(
            was_waited,
            "cleanup uncertainty returned after dropping an un-waited child"
        );
    }

    #[test]
    fn native_catalog_preserves_alias_and_refuses_missing_or_conflicting_resolution() {
        assert_eq!(
            models(&serde_json::json!([{"value":"alias","resolvedModel":"native-resolved"}]))
                .unwrap(),
            [("alias".into(), "native-resolved".into())]
        );
        assert!(
            models(&serde_json::json!([{"value":"alias"}]))
                .unwrap()
                .is_empty()
        );
        assert!(models(&serde_json::json!([{"value":"alias","resolvedModel":"one"},{"value":"alias","resolvedModel":"two"}])).is_err());
        assert!(
            models(&serde_json::json!([{"value":"alias","resolvedModel":"bad model"}])).is_err()
        );
        assert!(!version("2.1.299"));
        assert!(version("2.1.300"));
    }
}

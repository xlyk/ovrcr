//! Native Claude policy identities and bounded pre-input model discovery.
//! No SDK, Session, Reporter lease or assignment publisher is created by discovery.
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
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const ARTIFACT_ENV: &str = "OVRCR_CLAUDE_ARTIFACTS";
pub const POLICY: &str = "ovrcr-claude-research-v3";
/// Invocation-only permissions for the existing authenticated assignment server.
pub const SCOPED_MCP_APPROVALS: &str = "mcp__ovrcr_child__ovrcr_read,mcp__ovrcr_child__ovrcr_publish,mcp__ovrcr_child__ovrcr_publish_question";
const HELPER_POLICY: &str = "ovrcr-claude-research-helpers-v1";
const COORDINATOR: &str = "ovrcr-assigned-coordinator";
const HELPER: &str = "ovrcr-assigned-research";

/// Fixed nonsecret recipe. Native helpers receive no publisher or nesting tool.
pub fn helper_agents(model: &str) -> String {
    serde_json::json!({
        COORDINATOR: {"description":"Managed assigned research coordinator", "prompt":"",
            "tools":[format!("Agent({HELPER})"), "mcp__ovrcr_child__ovrcr_read", "mcp__ovrcr_child__ovrcr_publish", "mcp__ovrcr_child__ovrcr_publish_question"]},
        HELPER: {"description":"Read evidence within the current assigned research contract", "model":model,
            "prompt":"Use only the assigned read tool within the current contract. Return evidence to the coordinator; do not publish, ask questions, or delegate.",
            "tools":["mcp__ovrcr_child__ovrcr_read"]}
    }).to_string()
}

/// Recovery admits this generated recipe only, never arbitrary inline policy.
pub fn helper_model(agents: &str) -> Result<String> {
    ensure!(agents.len() <= 4096, "native helper recipe exceeds bounds");
    let value: serde_json::Value = serde_json::from_str(agents)?;
    let model = value[HELPER]["model"]
        .as_str()
        .context("native helper model missing")?;
    ensure!(
        native_id(model) && helper_agents(model) == agents,
        "foreign native helper recipe"
    );
    Ok(model.to_owned())
}

pub fn helper_settings(model: Option<&str>) -> serde_json::Value {
    model.map_or_else(|| serde_json::json!({}), |model| serde_json::json!({
        "env":{"CLAUDE_CODE_SUBAGENT_MODEL":model,"CLAUDE_CODE_SUBAGENT_MODEL_FORCE":"1","CLAUDE_CODE_FORK_SUBAGENT":"0"},
        "permissions":{"deny":["Agent(fork)"]}
    }))
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
    // Seals model/backend policy and intended assigned permissions, not complete
    // discovery topology: discovery omits the publisher and Reporter transport.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agents: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    approved_tools: Option<String>,
}
pub struct PolicyFiles {
    pub settings: Vec<u8>,
    pub mcp: Vec<u8>,
    pub manifest: Vec<u8>,
}
pub fn policy_files(
    helper: &Path,
    handle: &str,
    helper_model: Option<&str>,
) -> Result<PolicyFiles> {
    let helper = helper.canonicalize()?;
    let mut value = crate::claude_recovery::reporter_settings(
        helper.to_str().context("non-UTF8 Reporter executable")?,
    );
    value
        .as_object_mut()
        .unwrap()
        .extend(helper_settings(helper_model).as_object().unwrap().clone());
    let settings = format!("{}\n", serde_json::to_string_pretty(&value)?).into_bytes();
    let mut mcp = serde_json::json!({"mcpServers":{"ovrcr_child":{
        "type":"stdio","command":helper,"args":["delegation-publisher","--read-handle",handle]
    }}});
    if helper_model.is_none() {
        mcp["mcpServers"]["ovrcr_child"]["alwaysLoad"] = true.into();
    }
    let mcp = serde_json::to_vec(&mcp)?;
    let manifest = PolicyManifest {
        recipe: if helper_model.is_some() {
            HELPER_POLICY
        } else {
            POLICY
        }
        .into(),
        helper,
        settings: format!("{:x}", Sha256::digest(&settings)),
        mcp: format!("{:x}", Sha256::digest(&mcp)),
        agents: helper_model
            .map(|model| format!("{:x}", Sha256::digest(helper_agents(model).as_bytes()))),
        approved_tools: helper_model.is_none().then(|| SCOPED_MCP_APPROVALS.into()),
    };
    Ok(PolicyFiles {
        settings,
        mcp,
        manifest: serde_json::to_vec(&manifest)?,
    })
}

/// Only model/backend-affecting flags are shared with discovery; transport
/// differences are explicit and never claim identical complete MCP topology.
pub fn launch_argv(
    executable: &Path,
    model: &str,
    artifacts: &Path,
    helper_model: Option<&str>,
) -> Vec<std::ffi::OsString> {
    let mut argv = vec![
        executable.as_os_str().into(),
        "--model".into(),
        model.into(),
        "--permission-mode".into(),
        "default".into(),
        "--tools".into(),
        if helper_model.is_some() { "Agent" } else { "" }.into(),
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        artifacts.join("claude-mcp.json").into_os_string(),
        "--settings".into(),
        artifacts.join("claude-settings.json").into_os_string(),
        "--setting-sources".into(),
        "".into(),
    ];
    if let Some(model) = helper_model {
        argv.extend([
            "--agent".into(),
            COORDINATOR.into(),
            "--agents".into(),
            helper_agents(model).into(),
        ]);
    } else {
        argv.extend(["--allowedTools".into(), SCOPED_MCP_APPROVALS.into()]);
    }
    argv
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
fn policy_digest(model: &str, profile: &Path, helpers: bool) -> Result<String> {
    let mut policy = serde_json::json!({
        "policy":POLICY,"model":model,"model_provider":"native","profile":profile,
        "permission_mode":"default","tools":"","strict_mcp_config":true,
        "setting_sources":"","model_settings":{}
    });
    if helpers {
        policy["policy"] = HELPER_POLICY.into();
        policy["tools"] = "Agent".into();
        policy["native_helpers"] = serde_json::json!({"agents":helper_agents(model),"settings":helper_settings(Some(model))});
    } else {
        policy["approved_tools"] = SCOPED_MCP_APPROVALS.into();
        policy["mcp_always_load"] = true.into();
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&policy)?)
    ))
}

impl ModelProof {
    pub(crate) fn validate(&self, contract: &ResearchContract) -> Result<()> {
        ensure!(
            contract.provider == "claude"
                && contract.model_provider == "native"
                && self.requested == contract.model
                && native_id(&self.resolved)
                && self.policy
                    == if contract.native_helpers() {
                        HELPER_POLICY
                    } else {
                        POLICY
                    }
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
                && policy_digest(&self.requested, &profile, contract.native_helpers())?
                    == self.policy_digest,
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

/// This remains preflight evidence, never a proof of managed-TUI tool inventory,
/// native authentication, helper access or actual model inference.
pub(crate) fn discover(
    requested: &str,
    helpers: bool,
) -> Result<(Vec<(String, String)>, ModelProof)> {
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
    let request_id = crate::agent_runner::private_identifier()?;
    let frame = format!(
        "{}\n",
        serde_json::json!({"type":"control_request","request_id":request_id,
        "request":{"subtype":"initialize","hooks":null}})
    );
    let settings = helper_settings(helpers.then_some(requested)).to_string();
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
            if helpers { "Agent" } else { "" },
            "--strict-mcp-config",
            "--mcp-config",
            r#"{"mcpServers":{}}"#,
            "--setting-sources",
            "",
            "--settings",
            &settings,
        ])
        .env("CLAUDE_CONFIG_DIR", &profile_entry)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if helpers {
        command.args([
            "--agent",
            COORDINATOR,
            "--agents",
            &helper_agents(requested),
        ]);
    }
    // The native account/profile routing stays opaque. Parent and assignment
    // capabilities never enter a preallocation discovery process.
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
    let deadline = Instant::now() + Duration::from_secs(20);
    let result = (|| -> Result<(Vec<(String, String)>, String)> {
        let mut stdout = child
            .stdout
            .take()
            .context("native discovery stdout unavailable")?;
        let mut stderr = child
            .stderr
            .take()
            .context("native discovery stderr unavailable")?;
        for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            ensure!(
                flags >= 0
                    && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
                "native discovery pipe unavailable"
            );
        }
        // Native account/error bodies remain in bounded memory only. No raw
        // account, transcript or stderr is written to a temporary file or log.
        let mut bytes = Vec::new();
        let mut error_bytes = 0usize;
        child
            .stdin
            .as_mut()
            .context("native discovery stdin unavailable")?
            .write_all(frame.as_bytes())?;
        loop {
            ensure!(Instant::now() < deadline, "native model discovery deadline");
            let mut buffer = [0; 16384];
            loop {
                ensure!(Instant::now() < deadline, "native model discovery deadline");
                match stdout.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        bytes.extend_from_slice(&buffer[..count]);
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
                match stderr.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        error_bytes += count;
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
    // Let ordinary native shutdown release its own profile/refresh locks first.
    // The leader stays unreaped, anchoring the known task-owned group throughout.
    let grace = deadline.min(Instant::now() + Duration::from_secs(2));
    while Instant::now() < grace {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let observed = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        ensure!(
            observed == 0,
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
    // Keep the leader unreaped while deciding whether a signal is necessary.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let observed = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id(),
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    ensure!(
        observed == 0,
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
            "native discovery cleanup is uncertain"
        );
    }
    let waited = child.wait();
    // Never send a nonzero signal after reap. Surviving/unknown groups refuse.
    let absent = unsafe { libc::kill(-(child.id() as i32), 0) };
    let absence_errno = if absent == -1 {
        std::io::Error::last_os_error().raw_os_error()
    } else {
        None
    };
    let known_absent = waited.is_ok() && absent == -1 && absence_errno == Some(libc::ESRCH);
    // Acceptance-only synthetic uncertainty never skips the actual wait/probe.
    let injected_unknown = known_absent
        && crate::server::delegation::take_fault(
            "claude-discovery-postwait-unknown",
            "preallocation",
        )
        .is_some();
    ensure!(
        known_absent && !injected_unknown,
        "native discovery cleanup is uncertain"
    );
    let (catalog, native_version) = result?;
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
        policy: if helpers { HELPER_POLICY } else { POLICY }.into(),
        policy_digest: policy_digest(requested, &profile, helpers)?,
    };
    Ok((catalog, proof))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_recipe_confines_tools_model_and_recovery_preserving_helper_v1() {
        let model = "claude-exact-1";
        let agents = helper_agents(model);
        assert!(agents.len() <= 4096);
        assert_eq!(helper_model(&agents).unwrap(), model);
        let mut value: serde_json::Value = serde_json::from_str(&agents).unwrap();
        assert_eq!(
            value[HELPER]["tools"],
            serde_json::json!(["mcp__ovrcr_child__ovrcr_read"])
        );
        assert_eq!(value[COORDINATOR]["tools"][0], format!("Agent({HELPER})"));
        value[HELPER]["tools"] = serde_json::json!(["mcp__ovrcr_child__ovrcr_publish"]);
        assert!(helper_model(&value.to_string()).is_err());
        assert!(helper_model(&helper_agents("foreign model")).is_err());
        assert!(helper_model(&"x".repeat(4097)).is_err());
        let settings = helper_settings(Some(model));
        assert_eq!(settings["env"]["CLAUDE_CODE_SUBAGENT_MODEL"], model);
        assert_eq!(settings["env"]["CLAUDE_CODE_SUBAGENT_MODEL_FORCE"], "1");
        assert_eq!(settings["env"]["CLAUDE_CODE_FORK_SUBAGENT"], "0");
        assert_eq!(
            settings["permissions"]["deny"],
            serde_json::json!(["Agent(fork)"])
        );
        assert_eq!(helper_settings(None), serde_json::json!({}));
        let path = Path::new("/opaque/native/profile");
        let old_helper = serde_json::json!({"policy":"ovrcr-claude-research-helpers-v1","model":model,"model_provider":"native","profile":path,
            "permission_mode":"default","tools":"Agent","strict_mcp_config":true,"setting_sources":"","model_settings":{},
            "native_helpers":{"agents":helper_agents(model),"settings":helper_settings(Some(model))}});
        assert_eq!(
            policy_digest(model, path, true).unwrap(),
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&old_helper).unwrap())
            )
        );
        assert_ne!(
            policy_digest(model, path, false).unwrap(),
            policy_digest(model, path, true).unwrap()
        );
    }

    #[test]
    fn default_research_seals_exact_invocation_permissions_without_changing_helpers() {
        let argv = launch_argv(
            Path::new("/bin/claude"),
            "sonnet",
            Path::new("/private/assignment"),
            None,
        );
        assert_eq!(
            argv[argv.len() - 2..],
            ["--allowedTools", SCOPED_MCP_APPROVALS].map(std::ffi::OsString::from)
        );
        assert_eq!(
            SCOPED_MCP_APPROVALS,
            "mcp__ovrcr_child__ovrcr_read,mcp__ovrcr_child__ovrcr_publish,mcp__ovrcr_child__ovrcr_publish_question"
        );
        let helper = launch_argv(
            Path::new("/bin/claude"),
            "sonnet",
            Path::new("/private/assignment"),
            Some("claude-exact-1"),
        );
        assert!(!helper.iter().any(|value| value == "--allowedTools"));
        let files = policy_files(Path::new("/bin/sh"), "request", None).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&files.manifest).unwrap();
        assert_eq!(manifest["recipe"], "ovrcr-claude-research-v3");
        let mcp: serde_json::Value = serde_json::from_slice(&files.mcp).unwrap();
        assert_eq!(mcp["mcpServers"]["ovrcr_child"]["alwaysLoad"], true);
        assert_eq!(manifest["approved_tools"], SCOPED_MCP_APPROVALS);
        let helper_files =
            policy_files(Path::new("/bin/sh"), "request", Some("claude-exact-1")).unwrap();
        let old_helper_mcp = serde_json::json!({"mcpServers":{"ovrcr_child":{
            "type":"stdio", "command":Path::new("/bin/sh").canonicalize().unwrap(),
            "args":["delegation-publisher", "--read-handle", "request"]
        }}});
        assert_eq!(
            helper_files.mcp,
            serde_json::to_vec(&old_helper_mcp).unwrap()
        );
        let old_helper_manifest = serde_json::json!({"recipe":"ovrcr-claude-research-helpers-v1", "helper": Path::new("/bin/sh").canonicalize().unwrap(),
            "settings":format!("{:x}",Sha256::digest(&helper_files.settings)),"mcp":format!("{:x}",Sha256::digest(&helper_files.mcp)),
            "agents":format!("{:x}",Sha256::digest(helper_agents("claude-exact-1").as_bytes()))});
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&helper_files.manifest).unwrap(),
            old_helper_manifest
        );
        #[derive(Serialize)]
        struct LegacyManifest {
            recipe: String,
            helper: PathBuf,
            settings: String,
            mcp: String,
            agents: String,
        }
        let old_bytes = serde_json::to_vec(&LegacyManifest {
            recipe: "ovrcr-claude-research-helpers-v1".into(),
            helper: Path::new("/bin/sh").canonicalize().unwrap(),
            settings: format!("{:x}", Sha256::digest(&helper_files.settings)),
            mcp: format!("{:x}", Sha256::digest(&helper_files.mcp)),
            agents: format!(
                "{:x}",
                Sha256::digest(helper_agents("claude-exact-1").as_bytes())
            ),
        })
        .unwrap();
        assert_eq!(helper_files.manifest, old_bytes);
        let path = Path::new("/opaque/profile");
        let sealed = serde_json::json!({"policy":"ovrcr-claude-research-v3","model":"sonnet","model_provider":"native","profile":path,
            "permission_mode":"default","tools":"","strict_mcp_config":true,"setting_sources":"","model_settings":{},"approved_tools":SCOPED_MCP_APPROVALS,"mcp_always_load":true});
        assert_eq!(
            policy_digest("sonnet", path, false).unwrap(),
            format!("{:x}", Sha256::digest(serde_json::to_vec(&sealed).unwrap()))
        );
    }

    #[test]
    fn old_research_proof_is_not_upgraded_to_native_approval_authority() {
        let contract: ResearchContract = serde_json::from_value(serde_json::json!({
            "schema":1,"workflow":"workflow","controller":"controller","task":"task","request":"request","project":"project","workspace":"workspace",
            "scope":"read","permitted_actions":["read"],"provider":"claude","model_provider":"native","model":"sonnet","acceptance_evidence":[],"mandatory_capabilities":["read","result_file"]
        })).unwrap();
        let proof: ModelProof = serde_json::from_value(serde_json::json!({
            "requested":"sonnet","resolved":"claude-exact-1","executable":"/bin/claude","target":"/bin/claude","binary_digest":"a".repeat(64),
            "file":{"device":0,"inode":0,"size":0,"modified":0,"modified_ns":0},"profile":"/opaque/profile","profile_entry":"/opaque/profile",
            "profile_device":0,"profile_inode":0,"native_version":"2.1.309","policy":"ovrcr-claude-research-v1","policy_digest":"old"
        })).unwrap();
        for policy in ["ovrcr-claude-research-v1", "ovrcr-claude-research-v2"] {
            let mut old = proof.clone();
            old.policy = policy.into();
            assert_eq!(
                old.validate(&contract).unwrap_err().to_string(),
                "native model proof does not match the immutable contract"
            );
            assert_eq!(old.policy, policy);
        }
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

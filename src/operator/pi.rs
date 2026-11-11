//! Dedicated Pi profile, launcher command construction, and bounded model
//! listing for the Operator.
//!
//! The profile at `<instance>/operator-pi` is the only Pi configuration the
//! Operator uses; the personal `~/.pi/agent` profile and OVRCR credentials are
//! never copied. The instance-wide agent working directory is
//! `<instance>/operator-workdir`. Every worker run gets a private Pi session
//! directory, and every turn gets a separate checkpoint attempt directory.
//! Children use umask 077 and an environment stripped of private
//! OVRCR authority and inherited provider credentials.
use crate::operator::rpc::{RpcChild, RpcMessage};
use crate::protocol::{OperatorConversationId, OperatorTurnId, SessionRunId};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

const PROFILE_DIR: &str = "operator-pi";
const WORKDIR_DIR: &str = "operator-workdir";
const TURNS_DIR: &str = "turns";
const EXTENSION_FILE: &str = "pi-operator-extension.mjs";
const TOOLS: &str = "read,bash,edit,write";
const MODEL_DEADLINE: Duration = Duration::from_secs(20);
const MODEL_LIMIT: usize = 4096;
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// The repository CLI skill is compiled into the binary, so the Operator has
/// its OVRCR operating guide before the first model request without reading a
/// checkout or discovering any skill at runtime. `--no-skills` stays in force.
const BUNDLED_SKILL: &str = include_str!("../../.agents/skills/ovrcr/SKILL.md");
const SYSTEM_PROMPT_GAP: &str = "\n\n";

/// Fixed, non-native user-facing failures. Native stderr and provider bodies
/// never reach a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PiErrorKind {
    Unavailable,
    NoModel,
    ModelUnavailable,
    Context,
    Integration,
    Request,
    Timeout,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PiError {
    kind: PiErrorKind,
    detail: String,
}

impl PiError {
    pub fn new(kind: PiErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub fn unavailable(detail: impl Into<String>) -> Self {
        Self::new(PiErrorKind::Unavailable, detail)
    }

    pub fn no_model() -> Self {
        Self::new(PiErrorKind::NoModel, "no model selected")
    }

    pub fn model_unavailable(detail: impl Into<String>) -> Self {
        Self::new(PiErrorKind::ModelUnavailable, detail)
    }

    pub fn context(detail: impl Into<String>) -> Self {
        Self::new(PiErrorKind::Context, detail)
    }

    pub fn integration(detail: impl Into<String>) -> Self {
        Self::new(PiErrorKind::Integration, detail)
    }

    pub fn request(detail: impl Into<String>) -> Self {
        Self::new(PiErrorKind::Request, detail)
    }

    pub fn timeout(detail: impl Into<String>) -> Self {
        Self::new(PiErrorKind::Timeout, detail)
    }

    pub fn unsupported(detail: impl Into<String>) -> Self {
        Self::new(PiErrorKind::Unsupported, detail)
    }

    pub fn kind(&self) -> PiErrorKind {
        self.kind
    }

    /// Redacted diagnostic detail for logs. Never persist it with native bodies.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn timed_out(&self) -> bool {
        matches!(self.kind, PiErrorKind::Timeout)
    }

    /// Alias used by the child driver's poll loop.
    pub fn timed_out_error(&self) -> bool {
        self.timed_out()
    }

    pub fn user_message(&self) -> &'static str {
        match self.kind {
            PiErrorKind::Unavailable => {
                "Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE."
            }
            PiErrorKind::NoModel => "No Pi model selected; run ovrcr operator setup.",
            PiErrorKind::ModelUnavailable => {
                "Configured Operator model is unavailable in the dedicated Pi profile."
            }
            PiErrorKind::Context => "Operator Pi context is unavailable; saved data was preserved.",
            PiErrorKind::Integration => {
                "Pi Operator integration is unavailable; no prompt was sent."
            }
            PiErrorKind::Request | PiErrorKind::Timeout => {
                "Pi request failed; inspect the partial journal before retrying."
            }
            PiErrorKind::Unsupported => "Unsupported Pi interaction; the turn was stopped.",
        }
    }
}

impl std::fmt::Display for PiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.user_message())
    }
}

impl std::error::Error for PiError {}

/// Resolve the Pi executable exactly like the existing title/task integrations.
pub fn executable() -> Result<PathBuf, PiError> {
    match std::env::var_os("OVRCR_PI_EXECUTABLE") {
        Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
        Some(_) => Err(PiError::unavailable("OVRCR_PI_EXECUTABLE is empty")),
        None => Ok(PathBuf::from("pi")),
    }
}

/// One validation of a model returned by the dedicated profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperatorModel {
    pub provider: String,
    pub id: String,
    pub name: String,
}

/// Paths this profile owns inside one OVRCR instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PiPaths {
    pub root: PathBuf,
    pub profile: PathBuf,
    pub workdir: PathBuf,
    pub turns: PathBuf,
    pub runs: PathBuf,
}

impl PiPaths {
    /// Per-attempt session directory. Callers create it exclusively.
    pub fn attempt_dir(&self, turn: OperatorTurnId) -> PathBuf {
        self.turns.join(turn.0.to_string())
    }

    /// One native Pi session directory owned by a worker run.
    pub fn run_dir(&self, run: SessionRunId) -> PathBuf {
        self.runs.join(run.0.to_string())
    }
}

/// Per-attempt session directory as a free helper.
pub fn attempt_dir(paths: &PiPaths, turn: OperatorTurnId) -> PathBuf {
    paths.attempt_dir(turn)
}

/// Deterministic native session identity for the singleton conversation.
pub fn conversation_session_id(conversation: OperatorConversationId) -> String {
    format!("operator-{}", conversation.0)
}

/// Resolve and validate the instance profile. Creates `operator-pi`,
/// `operator-pi/turns`, `operator-pi/runs`, and `operator-workdir` with mode
/// 0700. Existing paths
/// that are symlinks, non-directories, foreign-owned, or group/world-writable
/// are refused rather than replaced.
pub fn paths(instance: &Path) -> Result<PiPaths, PiError> {
    let instance = resolve_instance(instance)?;
    let profile = instance.join(PROFILE_DIR);
    let workdir = instance.join(WORKDIR_DIR);
    let turns = profile.join(TURNS_DIR);
    let runs = profile.join("runs");
    prepare_private_dir(&profile)?;
    prepare_private_dir(&turns)?;
    prepare_private_dir(&runs)?;
    prepare_private_dir(&workdir)?;
    Ok(PiPaths {
        root: instance,
        profile,
        workdir,
        turns,
        runs,
    })
}

fn resolve_instance(instance: &Path) -> Result<PathBuf, PiError> {
    if instance.as_os_str().is_empty() {
        return crate::config::RegistryPath::resolve()
            .map(|path| path.0)
            .map_err(|_| PiError::context("OVRCR instance directory is unavailable"));
    }
    if instance.is_absolute() {
        Ok(instance.to_path_buf())
    } else {
        Ok(crate::config::RegistryPath::resolve()
            .map_err(|_| PiError::context("OVRCR instance directory is unavailable"))?
            .0
            .join(instance))
    }
}

fn prepare_private_dir(path: &Path) -> Result<(), PiError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => check_private_dir(path, &metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(path)
                .map_err(|error| PiError::context(format!("create {}: {error}", path.display())))?;
            // umask can only reduce the mode; normalise anyway.
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|error| PiError::context(format!("secure {}: {error}", path.display())))?;
            let metadata = fs::symlink_metadata(path).map_err(|error| {
                PiError::context(format!("inspect {}: {error}", path.display()))
            })?;
            check_private_dir(path, &metadata)
        }
        Err(error) => Err(PiError::context(format!(
            "inspect {}: {error}",
            path.display()
        ))),
    }
}

fn check_private_dir(path: &Path, metadata: &fs::Metadata) -> Result<(), PiError> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PiError::context(format!(
            "{} is not a private directory",
            path.display()
        )));
    }
    let current = unsafe { libc::geteuid() } as u32;
    if owner_rejected(metadata.uid(), current) || mode_rejected(metadata.mode()) {
        return Err(PiError::context(format!(
            "{} is not owned privately",
            path.display()
        )));
    }
    Ok(())
}

fn owner_rejected(owner: u32, current: u32) -> bool {
    owner != current
}

fn mode_rejected(mode: u32) -> bool {
    mode & 0o022 != 0
}

/// Materialize the bundled lifecycle extension into the private profile with
/// atomic replacement and mode 0600.
pub fn materialize_extension(paths: &PiPaths) -> Result<PathBuf, PiError> {
    let target = paths.profile.join(EXTENSION_FILE);
    let write = (|| -> io::Result<()> {
        // NamedTempFile uses exclusive creation with a random suffix, so a
        // planted path can neither redirect this write nor be truncated.
        let mut file = tempfile::Builder::new()
            .prefix(&format!(".{EXTENSION_FILE}."))
            .suffix(".tmp")
            .tempfile_in(&paths.profile)?;
        file.write_all(include_str!("../pi-operator-extension.mjs").as_bytes())?;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        file.as_file().sync_all()?;
        file.persist(&target).map_err(|error| error.error)?;
        Ok(())
    })();
    if let Err(error) = write {
        return Err(PiError::integration(format!(
            "materialize Operator extension: {error}"
        )));
    }
    Ok(target)
}

/// Managed-run launcher. `session_dir` is private and owned by one worker run.
/// The caller materializes the extension first. Pi stays in the caller's
/// process group so the lifetime Session owns it and its tools.
pub fn rpc_command(
    paths: &PiPaths,
    session_dir: &Path,
    conversation: OperatorConversationId,
    policy: &str,
) -> Result<Command, PiError> {
    if policy.is_empty() || policy.contains('\0') {
        return Err(PiError::integration("Operator policy is invalid"));
    }
    let system_prompt = format!("{policy}{SYSTEM_PROMPT_GAP}{BUNDLED_SKILL}");
    let extension = paths.profile.join(EXTENSION_FILE);
    let mut command = Command::new(executable()?);
    command.args([
        "--mode",
        "rpc",
        "--no-extensions",
        "--no-skills",
        "--no-prompt-templates",
        "--no-themes",
        "--no-context-files",
        "--no-approve",
        "--offline",
        "--tools",
        TOOLS,
        "--session-dir",
    ]);
    command.arg(session_dir);
    command
        .arg("--session-id")
        .arg(conversation_session_id(conversation))
        .arg("--append-system-prompt")
        .arg(&system_prompt)
        .arg("--extension")
        .arg(&extension)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    configure(&mut command, paths)?;
    Ok(command)
}

/// Interactive native setup. No Server involvement and no `--offline`, because
/// native authentication and catalog work must be able to use the network.
pub fn setup_command(paths: &PiPaths) -> Result<Command, PiError> {
    let mut command = Command::new(executable()?);
    command.args([
        "--no-session",
        "--no-tools",
        "--no-extensions",
        "--no-skills",
        "--no-prompt-templates",
        "--no-themes",
        "--no-context-files",
        "--no-approve",
    ]);
    configure(&mut command, paths)?;
    Ok(command)
}

/// Replace this process with interactive native Pi setup.
pub fn run_setup() -> Result<(), PiError> {
    let instance = crate::config::RegistryPath::resolve()
        .map_err(|_| PiError::context("OVRCR instance directory is unavailable"))?
        .0;
    let paths = paths(&instance)?;
    let mut command = setup_command(&paths)?;
    // `exec` returns only on failure.
    let error = command.exec();
    Err(PiError::integration(format!(
        "could not start native Pi setup: {error}"
    )))
}

/// One-shot model catalog launcher. Uses an in-memory session and no tools.
pub fn models_command(paths: &PiPaths) -> Result<Command, PiError> {
    let mut command = Command::new(executable()?);
    command.args([
        "--mode",
        "rpc",
        "--no-session",
        "--no-tools",
        "--no-extensions",
        "--no-skills",
        "--no-prompt-templates",
        "--no-themes",
        "--no-context-files",
        "--no-approve",
        "--offline",
    ]);
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    configure(&mut command, paths)?;
    Ok(command)
}

/// Validated, deduplicated models discoverable in the dedicated profile.
/// Launches in its own task-owned process group and always terminates/reaps it.
pub fn list_models() -> Result<Vec<OperatorModel>, PiError> {
    let instance = crate::config::RegistryPath::resolve()
        .map_err(|_| PiError::context("OVRCR instance directory is unavailable"))?
        .0;
    let paths = paths(&instance)?;
    let mut command = models_command(&paths)?;
    list_models_with_command(&mut command)
}

fn list_models_with_command(command: &mut Command) -> Result<Vec<OperatorModel>, PiError> {
    let mut child = RpcChild::spawn(command, true)?;
    list_models_from_child(&mut child)
}

fn list_models_from_child(child: &mut RpcChild) -> Result<Vec<OperatorModel>, PiError> {
    let expires = Instant::now() + MODEL_DEADLINE;
    let result = (|| -> Result<Vec<OperatorModel>, PiError> {
        child.send(&json!({"id": "models", "type": "get_available_models"}))?;
        loop {
            let remaining = expires.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(PiError::timeout("Pi model catalog deadline"));
            }
            match child.next_message(Some(remaining))? {
                RpcMessage::Event(_) => continue,
                RpcMessage::Response(response) if response.id == "models" => {
                    if !response.success {
                        return Err(PiError::request("Pi refused the model catalog request"));
                    }
                    return parse_models(&response.data);
                }
                RpcMessage::Response(_) => {
                    return Err(PiError::request(
                        "Pi model catalog response id is unexpected",
                    ));
                }
            }
        }
    })();
    // Bounded group termination and reaping on every path, including errors.
    if child.terminate(SHUTDOWN_GRACE).is_none() {
        return Err(PiError::request("Pi model catalog cleanup is unproven"));
    }
    result
}

fn parse_models(data: &Value) -> Result<Vec<OperatorModel>, PiError> {
    let models = data
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| PiError::request("Pi model catalog is malformed"))?;
    if models.len() > MODEL_LIMIT {
        return Err(PiError::request("Pi model catalog is too large"));
    }
    let mut seen = BTreeSet::new();
    let mut parsed = Vec::with_capacity(models.len());
    for model in models {
        let provider = model.get("provider").and_then(Value::as_str);
        let id = model.get("id").and_then(Value::as_str);
        let (Some(provider), Some(id)) = (provider, id) else {
            return Err(PiError::request("Pi model catalog entry is malformed"));
        };
        if !valid_field(provider, 128) || !valid_field(id, 256) {
            return Err(PiError::request("Pi model catalog entry is malformed"));
        }
        let name = model
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| valid_field(name, 256))
            .unwrap_or(id);
        if !seen.insert((provider.to_owned(), id.to_owned())) {
            continue;
        }
        parsed.push(OperatorModel {
            provider: provider.to_owned(),
            id: id.to_owned(),
            name: name.to_owned(),
        });
    }
    Ok(parsed)
}

fn valid_field(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

fn configure(command: &mut Command, paths: &PiPaths) -> Result<(), PiError> {
    command.current_dir(&paths.workdir);
    for key in FORBIDDEN_ENV {
        command.env_remove(key);
    }
    for (key, _) in std::env::vars_os() {
        if let Some(key) = key.to_str().filter(|key| forbidden_env(key)) {
            command.env_remove(key);
        }
    }
    command
        .env("PI_CODING_AGENT_DIR", &paths.profile)
        .env("PI_TELEMETRY", "0")
        .env("PI_SKIP_VERSION_CHECK", "1")
        .env("OVRCR_EXECUTABLE", executable_self()?);
    private_umask(command);
    Ok(())
}

fn executable_self() -> Result<PathBuf, PiError> {
    std::env::current_exe()
        .map_err(|_| PiError::integration("OVRCR executable path is unavailable"))
}

fn private_umask(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            libc::umask(0o077);
            Ok(())
        });
    }
}

fn forbidden_env(key: &str) -> bool {
    FORBIDDEN_ENV.contains(&key)
        || FORBIDDEN_ENV_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

// Pi configuration can redirect code/configuration loading; AWS configuration
// includes ambient credential sources and service-specific endpoint overrides.
// Neither namespace is needed for ordinary tool paths or OVRCR instance routing.
const FORBIDDEN_ENV_PREFIXES: &[&str] = &["PI_", "AWS_"];

/// Private OVRCR authority stripped from `src/operator/bash.rs` and
/// `agent_runner.rs`, plus Operator-only and Pi session/provider variables.
const FORBIDDEN_ENV: &[&str] = &[
    // Private OVRCR reporting/delegation authority.
    "OVRCR_AGENT_SOCKET",
    "OVRCR_AGENT_TOKEN",
    "OVRCR_HOOK_SOCKET",
    "OVRCR_HOOK_TOKEN",
    "OVRCR_SESSION_ID",
    "OVRCR_DELEGATION_SESSION",
    "OVRCR_DELEGATION_TOKEN",
    "OVRCR_DELEGATION_SOCKET",
    "OVRCR_DELEGATION_HOME",
    "OVRCR_DELEGATION_INSTANCE",
    "OVRCR_DELEGATION_EPOCH",
    "OVRCR_PI_ENTRY",
    "OVRCR_CLAUDE_ARTIFACTS",
    // Shell startup hooks.
    "BASH_ENV",
    "ENV",
    // Provider API keys from the installed providers.md table.
    "ANTHROPIC_API_KEY",
    "ANT_LING_API_KEY",
    "OPENAI_API_KEY",
    "DEEPSEEK_API_KEY",
    "NVIDIA_API_KEY",
    "GEMINI_API_KEY",
    "COPILOT_GITHUB_TOKEN",
    "MISTRAL_API_KEY",
    "GROQ_API_KEY",
    "CEREBRAS_API_KEY",
    "XAI_API_KEY",
    "OPENROUTER_API_KEY",
    "AI_GATEWAY_API_KEY",
    "ZAI_API_KEY",
    "ZAI_CODING_CN_API_KEY",
    "OPENCODE_API_KEY",
    "RADIUS_API_KEY",
    "HF_TOKEN",
    "FIREWORKS_API_KEY",
    "TOGETHER_API_KEY",
    "BASETEN_API_KEY",
    "KIMI_API_KEY",
    "META_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_CN_API_KEY",
    "MOONSHOT_API_KEY",
    "QWEN_TOKEN_PLAN_API_KEY",
    "QWEN_TOKEN_PLAN_CN_API_KEY",
    "XIAOMI_API_KEY",
    "XIAOMI_TOKEN_PLAN_CN_API_KEY",
    "XIAOMI_TOKEN_PLAN_AMS_API_KEY",
    "XIAOMI_TOKEN_PLAN_SGP_API_KEY",
    // Anthropic aliases and legacy OVRCR credential names.
    "ANTHROPIC_OAUTH_TOKEN",
    "ANTHROPIC_AUTH_TOKEN",
    "OPENAI_ACCESS_TOKEN",
    "OPENAI_REFRESH_TOKEN",
    "OPENAI_AUTH_TOKEN",
    "CHATGPT_ACCESS_TOKEN",
    "CHATGPT_REFRESH_TOKEN",
    "CHATGPT_TOKEN",
    "XAI_ACCESS_TOKEN",
    "XAI_REFRESH_TOKEN",
    "GROK_API_KEY",
    "GROK_ACCESS_TOKEN",
    "GROK_REFRESH_TOKEN",
    "GROK_TOKEN",
    // Cloud provider credentials and configuration.
    "AZURE_OPENAI_API_KEY",
    "AZURE_OPENAI_BASE_URL",
    "AZURE_OPENAI_RESOURCE_NAME",
    "CLOUDFLARE_API_KEY",
    "CLOUDFLARE_ACCOUNT_ID",
    "CLOUDFLARE_GATEWAY_ID",
    "GOOGLE_CLOUD_API_KEY",
    "GOOGLE_CLOUD_PROJECT",
    "GCLOUD_PROJECT",
    "GOOGLE_CLOUD_LOCATION",
    "GOOGLE_APPLICATION_CREDENTIALS",
];

#[cfg(test)]
mod tests {
    use super::*;

    const FORBIDDEN_PROBE: &[&str] = &[
        "OVRCR_AGENT_TOKEN",
        "OVRCR_HOOK_TOKEN",
        "OVRCR_SESSION_ID",
        "OVRCR_DELEGATION_TOKEN",
        "OVRCR_PI_ENTRY",
        "OVRCR_CLAUDE_ARTIFACTS",
        "BASH_ENV",
        "ENV",
        "PI_OFFLINE",
        "PI_CODING_AGENT_SESSION_DIR",
        "PI_SESSION_ID",
        "PI_SESSION_FILE",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "OPENAI_API_KEY",
        "XAI_API_KEY",
        "GEMINI_API_KEY",
        "HF_TOKEN",
        "AWS_ACCESS_KEY_ID",
        "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
        "CLOUDFLARE_API_KEY",
        "GOOGLE_APPLICATION_CREDENTIALS",
    ];

    const PATTERN_ENV_PROBES: &[&str] = &[
        "AWS_CONTAINER_AUTHORIZATION_TOKEN",
        "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE",
        "AWS_CONTAINER_CREDENTIALS_FULL_URI",
        "AWS_SHARED_CREDENTIALS_FILE",
        "AWS_CONFIG_FILE",
        "AWS_ROLE_ARN",
        "AWS_ROLE_SESSION_NAME",
        "AWS_SECURITY_TOKEN",
        "AWS_ENDPOINT_URL",
        "AWS_ENDPOINT_URL_BEDROCK",
        "AWS_ENDPOINT_URL_BEDROCK_RUNTIME",
        "AWS_FUTURE_CREDENTIAL_SOURCE",
        "AWS_PROFILE",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AWS_BEARER_TOKEN_BEDROCK",
        "AWS_REGION",
        "AWS_DEFAULT_REGION",
        "AWS_WEB_IDENTITY_TOKEN_FILE",
        "PI_OFFLINE",
        "PI_CODING_AGENT_SESSION_DIR",
        "PI_CODING_AGENT_SESSION_FILE",
        "PI_PACKAGE_DIR",
        "PI_CACHE_RETENTION",
        "PI_SHARE_VIEWER_URL",
        "PI_RADIUS_GATEWAY",
        "PI_CODING_AGENT",
        "PI_PROVIDER",
        "PI_MODEL",
        "PI_REASONING_LEVEL",
        "PI_HARDWARE_CURSOR",
        "PI_HYPERLINKS",
        "PI_IMAGE_PROTOCOL",
        "PI_TRUE_COLOR",
        "PI_TUI_ESC_TIMEOUT",
        "PI_SESSION_FUTURE",
        "PI_CODING_AGENT_SESSION_FUTURE",
    ];

    const ORDINARY_ENV_PROBES: &[&str] = &[
        "GITHUB_TOKEN",
        "NPM_TOKEN",
        "CARGO_REGISTRY_TOKEN",
        "GITLAB_TOKEN",
        "MY_SERVICE_API_KEY",
        "MY_SERVICE_SECRET",
    ];

    const REQUIRED: &[&str] = &[
        "PI_CODING_AGENT_DIR",
        "PI_TELEMETRY",
        "PI_SKIP_VERSION_CHECK",
        "OVRCR_EXECUTABLE",
    ];

    fn temp_instance() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn env_value<'a>(command: &'a Command, key: &str) -> Option<&'a std::ffi::OsStr> {
        command
            .get_envs()
            .find(|(name, _)| *name == key)
            .and_then(|(_, value)| value)
    }

    fn assert_env_removed(command: &Command, key: &str) {
        assert_eq!(
            command.get_envs().find(|(name, _)| *name == key),
            Some((std::ffi::OsStr::new(key), None)),
            "{key} must be explicitly removed"
        );
    }

    fn assert_args(command: &Command, expected: &[&str]) {
        assert_eq!(
            args(command),
            expected
                .iter()
                .map(|arg| (*arg).to_owned())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn parent_environment_credentials_are_removed_but_tool_routing_survives() {
        const MARKER: &str = "OVRCR_PI_ENV_TEST";
        const TEST: &str = "operator::pi::tests::parent_environment_credentials_are_removed_but_tool_routing_survives";
        let instance = temp_instance();
        if std::env::var_os(MARKER).is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env_clear()
                .env(MARKER, "1")
                .env("HOME", instance.path())
                .env("PATH", "/usr/bin:/bin")
                .env("OVRCR_HOME", instance.path())
                .env("OVRCR_INSTANCE", "operator-env-fixture")
                .env("OVRCR_SOCKET", instance.path().join("server.sock"))
                .env("PI_CODING_AGENT_DIR", "inherited-profile-sentinel")
                .env("PI_TELEMETRY", "inherited-telemetry-sentinel")
                .env("PI_SKIP_VERSION_CHECK", "inherited-version-sentinel")
                .env("OVRCR_EXECUTABLE", "inherited-executable-sentinel")
                .envs(
                    FORBIDDEN_ENV
                        .iter()
                        .chain(FORBIDDEN_PROBE)
                        .chain(PATTERN_ENV_PROBES)
                        .map(|key| (*key, "parent-credential-sentinel")),
                )
                .envs(
                    ORDINARY_ENV_PROBES
                        .iter()
                        .map(|key| (*key, "ordinary-tool-credential-sentinel")),
                )
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let paths = paths(instance.path()).unwrap();
        let commands = [
            rpc_command(
                &paths,
                &paths.attempt_dir(OperatorTurnId(7)),
                OperatorConversationId(3),
                "policy",
            )
            .unwrap(),
            setup_command(&paths).unwrap(),
            models_command(&paths).unwrap(),
        ];
        for key in FORBIDDEN_ENV
            .iter()
            .chain(FORBIDDEN_PROBE)
            .chain(PATTERN_ENV_PROBES)
        {
            assert_eq!(
                std::env::var_os(key).as_deref(),
                Some(std::ffi::OsStr::new("parent-credential-sentinel"))
            );
            for command in &commands {
                assert_env_removed(command, key);
            }
        }
        for key in ORDINARY_ENV_PROBES {
            assert_eq!(
                std::env::var_os(key).as_deref(),
                Some(std::ffi::OsStr::new("ordinary-tool-credential-sentinel"))
            );
            for command in &commands {
                assert!(
                    !command.get_envs().any(|(name, _)| name == *key),
                    "{key} must remain inherited for ordinary tools"
                );
            }
        }

        let mut command = Command::new("/usr/bin/env");
        configure(&mut command, &paths).unwrap();
        let output = command.output().unwrap();
        assert!(output.status.success());
        let environment = String::from_utf8(output.stdout).unwrap();
        let values: std::collections::BTreeMap<_, _> = environment
            .lines()
            .map(|line| line.split_once('=').unwrap())
            .collect();
        for key in FORBIDDEN_ENV
            .iter()
            .chain(FORBIDDEN_PROBE)
            .chain(PATTERN_ENV_PROBES)
        {
            assert!(!values.contains_key(key), "{key} reached the child");
        }
        for key in [
            "HOME",
            "PATH",
            "OVRCR_HOME",
            "OVRCR_INSTANCE",
            "OVRCR_SOCKET",
        ]
        .into_iter()
        .chain(ORDINARY_ENV_PROBES.iter().copied())
        {
            assert_eq!(values[key], std::env::var(key).unwrap());
        }
        assert_eq!(
            values["PI_CODING_AGENT_DIR"],
            paths.profile.to_str().unwrap()
        );
        assert_eq!(values["PI_TELEMETRY"], "0");
        assert_eq!(values["PI_SKIP_VERSION_CHECK"], "1");
        assert!(Path::new(values["OVRCR_EXECUTABLE"]).is_absolute());
    }

    #[test]
    fn forbidden_environment_filter_preserves_ordinary_tool_variables() {
        for key in FORBIDDEN_ENV
            .iter()
            .chain(FORBIDDEN_PROBE)
            .chain(PATTERN_ENV_PROBES)
        {
            assert!(forbidden_env(key), "{key} must be removed");
        }
        for key in [
            "PATH",
            "HOME",
            "OVRCR_HOME",
            "OVRCR_INSTANCE",
            "OVRCR_SOCKET",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "VISUAL",
            "EDITOR",
        ]
        .into_iter()
        .chain(ORDINARY_ENV_PROBES.iter().copied())
        {
            assert!(!forbidden_env(key), "{key} must remain available");
        }
    }

    #[test]
    fn creates_private_profile_directories() {
        let instance = temp_instance();
        let paths = paths(instance.path()).unwrap();
        assert_eq!(paths.root, instance.path());
        assert_eq!(paths.profile, instance.path().join("operator-pi"));
        assert_eq!(paths.workdir, instance.path().join("operator-workdir"));
        assert_eq!(paths.turns, instance.path().join("operator-pi/turns"));
        assert_eq!(paths.runs, instance.path().join("operator-pi/runs"));
        for directory in [&paths.profile, &paths.turns, &paths.runs, &paths.workdir] {
            let metadata = fs::symlink_metadata(directory).unwrap();
            assert!(metadata.is_dir());
            assert_eq!(metadata.mode() & 0o777, 0o700);
            assert_eq!(metadata.uid(), unsafe { libc::geteuid() } as u32);
        }
    }

    #[test]
    fn rejects_symlinked_profile() {
        let instance = temp_instance();
        let target = instance.path().join("elsewhere");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, instance.path().join("operator-pi")).unwrap();
        let error = paths(instance.path()).unwrap_err();
        assert_eq!(error.kind(), PiErrorKind::Context);
    }

    #[test]
    fn rejects_non_directory_profile() {
        let instance = temp_instance();
        fs::write(instance.path().join("operator-workdir"), b"not a dir").unwrap();
        let error = paths(instance.path()).unwrap_err();
        assert_eq!(error.kind(), PiErrorKind::Context);
    }

    #[test]
    fn rejects_group_and_world_writable_profile() {
        let instance = temp_instance();
        let profile = instance.path().join("operator-pi");
        fs::create_dir(&profile).unwrap();
        fs::set_permissions(&profile, fs::Permissions::from_mode(0o777)).unwrap();
        let error = paths(instance.path()).unwrap_err();
        assert_eq!(error.kind(), PiErrorKind::Context);
        assert!(mode_rejected(0o777));
        assert!(mode_rejected(0o775));
        assert!(mode_rejected(0o757));
        assert!(!mode_rejected(0o755));
        assert!(!mode_rejected(0o700));
        assert!(owner_rejected(1000, 0));
        assert!(!owner_rejected(0, 0));
    }

    #[test]
    fn rpc_argv_is_exact_and_sanitized() {
        let instance = temp_instance();
        let paths = paths(instance.path()).unwrap();
        let attempt = paths.attempt_dir(OperatorTurnId(7));
        let command = rpc_command(&paths, &attempt, OperatorConversationId(3), "policy").unwrap();
        let argv = args(&command);
        let prompt = argv
            .iter()
            .position(|arg| arg == "--append-system-prompt")
            .expect("managed turns append a system prompt");
        assert_eq!(
            argv.iter()
                .filter(|arg| *arg == "--append-system-prompt")
                .count(),
            1,
            "the policy and bundled skill share one argument"
        );
        assert_eq!(
            argv[..prompt]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![
                "--mode",
                "rpc",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-context-files",
                "--no-approve",
                "--offline",
                "--tools",
                "read,bash,edit,write",
                "--session-dir",
                attempt.to_str().unwrap(),
                "--session-id",
                "operator-3",
            ]
        );
        let system_prompt = &argv[prompt + 1];
        assert!(system_prompt.starts_with(&format!("policy{SYSTEM_PROMPT_GAP}")));
        assert_eq!(
            system_prompt.matches(BUNDLED_SKILL).count(),
            1,
            "the bundled CLI skill is appended exactly once"
        );
        assert_eq!(argv[prompt + 2].as_str(), "--extension");
        assert_eq!(
            argv[prompt + 3].as_str(),
            paths
                .profile
                .join("pi-operator-extension.mjs")
                .to_str()
                .unwrap()
        );
        assert_eq!(command.get_current_dir(), Some(paths.workdir.as_path()));
        assert_eq!(
            env_value(&command, "PI_CODING_AGENT_DIR").unwrap(),
            paths.profile.as_os_str()
        );
        assert_eq!(env_value(&command, "PI_TELEMETRY").unwrap(), "0");
        assert_eq!(env_value(&command, "PI_SKIP_VERSION_CHECK").unwrap(), "1");
        assert!(
            env_value(&command, "OVRCR_EXECUTABLE")
                .is_some_and(|value| Path::new(value).is_absolute())
        );
        for key in FORBIDDEN_ENV {
            assert_env_removed(&command, key);
        }
        assert_eq!(command.get_program(), executable().unwrap().as_os_str());
        assert_eq!(
            conversation_session_id(OperatorConversationId(3)),
            "operator-3"
        );
        assert_eq!(paths.attempt_dir(OperatorTurnId(9)), paths.turns.join("9"));
    }

    #[test]
    fn setup_argv_is_exact_and_sanitized() {
        let instance = temp_instance();
        let paths = paths(instance.path()).unwrap();
        let command = setup_command(&paths).unwrap();
        assert_args(
            &command,
            &[
                "--no-session",
                "--no-tools",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-context-files",
                "--no-approve",
            ],
        );
        assert!(!args(&command).contains(&"--offline".to_owned()));
        assert_eq!(command.get_current_dir(), Some(paths.workdir.as_path()));
        for key in FORBIDDEN_ENV {
            assert_env_removed(&command, key);
        }
        for key in REQUIRED {
            assert!(env_value(&command, key).is_some(), "{key} must reach Pi");
        }
    }

    #[test]
    fn models_argv_is_exact_and_sanitized() {
        let instance = temp_instance();
        let paths = paths(instance.path()).unwrap();
        let command = models_command(&paths).unwrap();
        assert_args(
            &command,
            &[
                "--mode",
                "rpc",
                "--no-session",
                "--no-tools",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--no-context-files",
                "--no-approve",
                "--offline",
            ],
        );
        assert_eq!(command.get_current_dir(), Some(paths.workdir.as_path()));
        for key in FORBIDDEN_ENV {
            assert_env_removed(&command, key);
        }
        for key in REQUIRED {
            assert!(env_value(&command, key).is_some(), "{key} must reach Pi");
        }
    }

    #[test]
    fn extension_materialization_is_private_and_replaces_stale_copy() {
        let instance = temp_instance();
        let paths = paths(instance.path()).unwrap();
        let stale = paths.profile.join("pi-operator-extension.mjs");
        fs::write(&stale, b"stale").unwrap();
        let target = materialize_extension(&paths).unwrap();
        assert_eq!(target, stale);
        let metadata = fs::symlink_metadata(&target).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.mode() & 0o777, 0o600);
        let bytes = fs::read(&target).unwrap();
        assert_eq!(
            bytes,
            include_str!("../pi-operator-extension.mjs").as_bytes()
        );
        let leftovers = fs::read_dir(&paths.profile)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn extension_materialization_never_follows_a_planted_temp_symlink() {
        use std::io::Read as _;

        let instance = temp_instance();
        let paths = paths(instance.path()).unwrap();
        let victim = instance.path().join("victim");
        fs::write(&victim, b"victim bytes must survive").unwrap();
        let planted = paths
            .profile
            .join(format!(".{EXTENSION_FILE}.{}.tmp", std::process::id()));
        std::os::unix::fs::symlink(&victim, &planted).unwrap();
        let target = paths.profile.join(EXTENSION_FILE);
        fs::write(&target, b"old extension").unwrap();
        let mut old_file = fs::File::open(&target).unwrap();

        assert_eq!(materialize_extension(&paths).unwrap(), target);

        assert_eq!(fs::read(&victim).unwrap(), b"victim bytes must survive");
        assert!(fs::symlink_metadata(&planted).unwrap().is_symlink());
        assert_eq!(fs::read_link(&planted).unwrap(), victim);
        assert_eq!(fs::symlink_metadata(&target).unwrap().mode() & 0o777, 0o600);
        assert_eq!(
            fs::read(&target).unwrap(),
            include_str!("../pi-operator-extension.mjs").as_bytes()
        );
        let mut previous = Vec::new();
        old_file.read_to_end(&mut previous).unwrap();
        assert_eq!(previous, b"old extension", "replacement must be atomic");
    }

    #[test]
    fn fixed_user_messages_map_exactly() {
        let cases = [
            (
                PiError::unavailable("x"),
                "Pi is unavailable; install pi 0.87.1 or set OVRCR_PI_EXECUTABLE.",
            ),
            (
                PiError::no_model(),
                "No Pi model selected; run ovrcr operator setup.",
            ),
            (
                PiError::model_unavailable("x"),
                "Configured Operator model is unavailable in the dedicated Pi profile.",
            ),
            (
                PiError::context("x"),
                "Operator Pi context is unavailable; saved data was preserved.",
            ),
            (
                PiError::integration("x"),
                "Pi Operator integration is unavailable; no prompt was sent.",
            ),
            (
                PiError::request("x"),
                "Pi request failed; inspect the partial journal before retrying.",
            ),
            (
                PiError::timeout("x"),
                "Pi request failed; inspect the partial journal before retrying.",
            ),
            (
                PiError::unsupported("x"),
                "Unsupported Pi interaction; the turn was stopped.",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.user_message(), message);
            assert_eq!(error.to_string(), message);
        }
        assert!(PiError::timeout("x").timed_out());
        assert!(!PiError::request("x").timed_out());
    }

    #[test]
    fn model_catalog_rejects_a_wrong_id_before_the_real_response() {
        for (id, command_name) in [
            ("decoy", "get_available_models"),
            ("decoy", "get_state"),
            ("", "get_available_models"),
        ] {
            let decoy = json!({
                "type": "response",
                "id": id,
                "command": command_name,
                "success": true,
                "data": {"models": [{"provider": "fake", "id": "decoy"}]}
            });
            let real = json!({
                "type": "response",
                "id": "models",
                "command": "get_available_models",
                "success": true,
                "data": {"models": [{"provider": "fake", "id": "real"}]}
            });
            let mut command = Command::new("/bin/sh");
            command.args([
                "-c",
                &format!(
                    "IFS= read -r request; printf '%s\\n' '{{\"type\":\"notification\"}}' '{decoy}' '{real}'"
                ),
            ]);
            let error = list_models_with_command(&mut command).unwrap_err();
            assert_eq!(error.kind(), PiErrorKind::Request);
            assert_eq!(
                error.user_message(),
                "Pi request failed; inspect the partial journal before retrying."
            );
        }
    }

    #[test]
    fn model_catalog_fails_when_reaped_leader_leaves_cleanup_unproven() {
        let instance = temp_instance();
        let ready = instance.path().join("member-ready");
        let release = instance.path().join("member-release");
        let response = json!({
            "type": "response",
            "id": "models",
            "command": "get_available_models",
            "success": true,
            "data": {"models": [{"provider": "fake", "id": "valid"}]}
        });
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "/bin/sh -c 'trap \"\" TERM; printf ready > \"$READY\"; \
                    IFS= read -r request; printf \"%s\\n\" \"$RESPONSE\"; \
                    while [ ! -e \"$RELEASE\" ]; do /bin/sleep 0.01; done' & \
                 while [ ! -e \"$READY\" ]; do /bin/sleep 0.01; done; exit 0",
            ])
            .env("READY", &ready)
            .env("RELEASE", &release)
            .env("RESPONSE", response.to_string());
        let mut child = RpcChild::spawn(&mut command, true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut leader_exited = false;
        while Instant::now() < deadline {
            if child.try_exit().is_some() {
                leader_exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let result = list_models_from_child(&mut child);
        // Once the leader is reaped, cleanup must never signal this remaining
        // member. Release it explicitly, then prove cleanup with read-only polls.
        fs::write(&release, b"release").unwrap();
        let cleanup = child.terminate(Duration::ZERO);
        assert!(leader_exited && ready.exists());
        assert!(cleanup.is_some(), "fixture group must be gone");
        let error = result.unwrap_err();
        assert_eq!(error.kind(), PiErrorKind::Request);
        assert_eq!(error.detail(), "Pi model catalog cleanup is unproven");
        assert_eq!(
            error.user_message(),
            "Pi request failed; inspect the partial journal before retrying."
        );
    }

    #[test]
    fn model_catalog_validation_deduplicates_and_rejects_oversize() {
        let data = json!({
            "models": [
                {"provider": "anthropic", "id": "a", "name": "A", "baseUrl": "https://x"},
                {"provider": "anthropic", "id": "a", "name": "A"},
                {"provider": "openai", "id": "b"},
            ]
        });
        let models = parse_models(&data).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "A");
        assert_eq!(models[1].name, "b");

        let too_many = json!({"models": vec![json!({"provider":"p","id":"i"}); MODEL_LIMIT + 1]});
        assert!(parse_models(&too_many).is_err());
        assert!(parse_models(&json!({"models": [{"provider": "p"}]})).is_err());
        assert!(parse_models(&json!({})).is_err());
        // Cross-module contract: RPC framing must admit a full 16 MiB record.
        let frame_limit = crate::operator::rpc::FRAME_LIMIT;
        assert!(frame_limit >= 16 * 1024 * 1024, "frame limit {frame_limit}");
    }
}
